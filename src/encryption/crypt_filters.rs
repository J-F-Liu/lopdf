use super::DecryptionError;
use super::pkcs5::Pkcs5;
use super::rc4::Rc4;
use crate::ObjectId;
use aes::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use md5::{Digest as _, Md5};
use rand::RngExt as _;

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

pub trait CryptFilter: std::fmt::Debug + Send + Sync {
    fn method(&self) -> &[u8];
    fn compute_key(&self, key: &[u8], obj_id: ObjectId) -> Result<Vec<u8>, DecryptionError>;
    fn encrypt(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, DecryptionError>;
    fn decrypt(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, DecryptionError>;
}

#[derive(Clone, Copy, Debug)]
pub struct IdentityCryptFilter;

impl CryptFilter for IdentityCryptFilter {
    fn method(&self) -> &[u8] {
        b"Identity"
    }

    fn compute_key(&self, key: &[u8], _obj_id: ObjectId) -> Result<Vec<u8>, DecryptionError> {
        Ok(key.to_vec())
    }

    fn encrypt(&self, _key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
        Ok(plaintext.to_vec())
    }

    fn decrypt(&self, _key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
        Ok(ciphertext.to_vec())
    }
}

/// Extend the file key with the low-order 3 bytes of the object number and the
/// low-order 2 bytes of the generation number, which is how every per-object
/// key is seeded before hashing (Algorithms 1 and 2.B).
fn key_with_object_number(key: &[u8], obj_id: ObjectId) -> Vec<u8> {
    let mut builder = Vec::with_capacity(key.len() + 5);
    builder.extend_from_slice(key);
    builder.extend_from_slice(&obj_id.0.to_le_bytes()[..3]);
    builder.extend_from_slice(&obj_id.1.to_le_bytes()[..2]);
    builder
}

/// The per-object key is the first min(n + 5, 16) bytes of the MD5 of the
/// extended file key, where n is the file key's length.
fn md5_object_key(extended_key: &[u8], file_key_len: usize) -> Vec<u8> {
    let key_len = (file_key_len + 5).min(16);
    Md5::digest(extended_key)[..key_len].to_vec()
}

#[derive(Clone, Copy, Debug)]
pub struct Rc4CryptFilter;

impl CryptFilter for Rc4CryptFilter {
    fn method(&self) -> &[u8] {
        b"V2"
    }

    fn compute_key(&self, key: &[u8], obj_id: ObjectId) -> Result<Vec<u8>, DecryptionError> {
        Ok(md5_object_key(&key_with_object_number(key, obj_id), key.len()))
    }

    fn encrypt(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
        Ok(Rc4::new(key).encrypt(plaintext))
    }

    fn decrypt(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
        Ok(Rc4::new(key).decrypt(ciphertext))
    }
}

/// AES-CBC with PKCS#5 padding (RFC 2898) under a key of the width the named
/// cipher takes. `Aes128CryptFilter` and `Aes256CryptFilter` agree on the
/// whole CBC envelope and differ only in key size and cipher, so it is
/// generated; `compute_key` stays per-filter because the two derive the key
/// differently (Algorithms 1 and 2.B respectively).
macro_rules! aes_cbc_crypt_filter {
    ($name:ident, $method:literal, $key_len:literal, $enc:ty, $dec:ty, $compute_key:expr) => {
        #[derive(Clone, Copy, Debug)]
        pub struct $name;

        impl CryptFilter for $name {
            fn method(&self) -> &[u8] {
                $method
            }

            fn compute_key(&self, key: &[u8], obj_id: ObjectId) -> Result<Vec<u8>, DecryptionError> {
                $compute_key(key, obj_id)
            }

            fn encrypt(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
                let key: &[u8; $key_len] = key.try_into().map_err(|_| DecryptionError::InvalidKeyLength)?;

                // The ciphertext needs to be a multiple of 16 bytes to include the padding.
                let ciphertext_len = (plaintext.len() + 16) / 16 * 16;

                // Allocate sufficient bytes for the initialization vector, the ciphertext and the padding
                // combined.
                let mut ciphertext = Vec::with_capacity(16 + ciphertext_len);

                // Generate random numbers to populate the initialization vector.
                let mut rng = rand::rng();
                let mut iv = [0u8; 16];
                rng.fill(&mut iv);

                // Combine the IV and the plaintext.
                ciphertext.extend_from_slice(&iv);
                ciphertext.extend_from_slice(plaintext);
                ciphertext.resize(16 + ciphertext_len, 0);

                <$enc>::new(key.into(), &iv.into())
                    .encrypt_padded::<Pkcs5>(&mut ciphertext[16..], plaintext.len())
                    // Padding errors should not occur when encrypting, but avoid causing a panic.
                    .map_err(|_| DecryptionError::Padding)?;

                Ok(ciphertext)
            }

            fn decrypt(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
                let key: &[u8; $key_len] = key.try_into().map_err(|_| DecryptionError::InvalidKeyLength)?;

                // Ensure that the ciphertext length is a multiple of 16 bytes.
                if !ciphertext.len().is_multiple_of(16) {
                    return Err(DecryptionError::InvalidCipherTextLength);
                }

                // There is nothing to decrypt if the ciphertext is empty or only contains the IV.
                if ciphertext.is_empty() || ciphertext.len() == 16 {
                    return Ok(vec![]);
                }

                let mut iv = [0x00u8; 16];
                iv.copy_from_slice(&ciphertext[..16]);

                let data = &mut ciphertext[16..].to_vec();

                Ok(<$dec>::new(key.into(), &iv.into())
                    .decrypt_padded::<Pkcs5>(data)
                    .map_err(|_| DecryptionError::Padding)?
                    .to_vec())
            }
        }
    };
}

/// Algorithm 1: extend the file key with the object number and generation,
/// then, for AES, with the "sAlT" marker, and take the first min(n + 5, 16)
/// bytes of the MD5 as the key.
fn aes128_compute_key(key: &[u8], obj_id: ObjectId) -> Result<Vec<u8>, DecryptionError> {
    let mut builder = key_with_object_number(key, obj_id);

    // If using the AES algorithm, extend the file encryption key an additional 4 bytes by
    // adding the value "sAlT".
    builder.extend_from_slice(b"sAlT");

    Ok(md5_object_key(&builder, key.len()))
}

/// Algorithm 2.B: the 32-byte file encryption key is already the AES-256 key.
fn aes256_compute_key(key: &[u8], _obj_id: ObjectId) -> Result<Vec<u8>, DecryptionError> {
    Ok(key.to_vec())
}

aes_cbc_crypt_filter!(
    Aes128CryptFilter,
    b"AESV2",
    16,
    Aes128CbcEnc,
    Aes128CbcDec,
    aes128_compute_key
);
aes_cbc_crypt_filter!(
    Aes256CryptFilter,
    b"AESV3",
    32,
    Aes256CbcEnc,
    Aes256CbcDec,
    aes256_compute_key
);
