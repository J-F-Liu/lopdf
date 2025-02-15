use crate::rc4::Rc4;
use crate::{Document, Object, ObjectId};
use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use md5::{Digest as _, Md5};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DecryptionError {
    #[error("the /Encrypt dictionary is missing")]
    MissingEncryptDictionary,
    #[error("missing encryption revision")]
    MissingRevision,
    #[error("missing the owner password (/O)")]
    MissingOwnerPassword,
    #[error("missing the permissions field (/P)")]
    MissingPermissions,
    #[error("missing the file /ID elements")]
    MissingFileID,

    #[error("invalid key length")]
    InvalidKeyLength,
    #[error("invalid ciphertext length")]
    InvalidCipherTextLength,
    #[error("invalid revision")]
    InvalidRevision,
    // Used generically when the object type violates the spec
    #[error("unexpected type; document does not comply with the spec")]
    InvalidType,

    #[error("the object is not capable of being decrypted")]
    NotDecryptable,
    #[error("the supplied password is incorrect")]
    IncorrectPassword,

    #[error("the document uses an encryption scheme that is not implemented in lopdf")]
    UnsupportedEncryption,
}

const PAD_BYTES: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08, 0x2E, 0x2E, 0x00,
    0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

const DEFAULT_KEY_LEN: Object = Object::Integer(40);
const DEFAULT_ALGORITHM: Object = Object::Integer(0);

pub trait CryptFilter: std::fmt::Debug {
    fn compute_key(&self, key: &[u8], obj_id: ObjectId) -> Result<Vec<u8>, DecryptionError>;
    fn decrypt(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, DecryptionError>;
}

#[derive(Clone, Copy, Debug)]
pub struct IdentityCryptFilter;

impl CryptFilter for IdentityCryptFilter {
    fn compute_key(&self, key: &[u8], _obj_id: ObjectId) -> Result<Vec<u8>, DecryptionError> {
        Ok(key.to_vec())
    }

    fn decrypt(&self, _key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
        Ok(ciphertext.to_vec())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Rc4CryptFilter;

impl CryptFilter for Rc4CryptFilter {
    fn compute_key(&self, key: &[u8], obj_id: ObjectId) -> Result<Vec<u8>, DecryptionError> {
        let mut builder = Vec::with_capacity(key.len() + 5);

        builder.extend_from_slice(key);

        // For all strings and streams without crypt filter specifier; treating the object number
        // and generation number as binary integers, extend the original n-byte file encryption key
        // to n + 5 bytes by appending the low-order 3 bytes of the object number and the low-order
        // 2 bytes of the generation number in that order, low-order byte first.
        builder.extend_from_slice(&obj_id.0.to_le_bytes()[..3]);
        builder.extend_from_slice(&obj_id.1.to_le_bytes()[..2]);

        // Initialise the MD5 hash function and pass the result of the previous step as an input to
        // this function.
        //
        // Use the first (n + 5) bytes, up to a maximum of 16, of the output from the MD5 hash as
        // the key for the AES symmetric key algorithm.
        let key_len = std::cmp::min(key.len() + 5, 16);
        let key = Md5::digest(builder)[..key_len].to_vec();

        Ok(key)
    }

    fn decrypt(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
        Ok(Rc4::new(key).decrypt(ciphertext))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Aes128CryptFilter;

impl CryptFilter for Aes128CryptFilter {
    fn compute_key(&self, key: &[u8], obj_id: ObjectId) -> Result<Vec<u8>, DecryptionError> {
        let mut builder = Vec::with_capacity(key.len() + 9);

        builder.extend_from_slice(key);

        // For all strings and streams without crypt filter specifier; treating the object number
        // and generation number as binary integers, extend the original n-byte file encryption key
        // to n + 5 bytes by appending the low-order 3 bytes of the object number and the low-order
        // 2 bytes of the generation number in that order, low-order byte first.
        builder.extend_from_slice(&obj_id.0.to_le_bytes()[..3]);
        builder.extend_from_slice(&obj_id.1.to_le_bytes()[..2]);

        // If using the AES algorithm, extend the file encryption key an additional 4 bytes by
        // adding the value "sAlT".
        builder.extend_from_slice(b"sAlT");

        // Initialise the MD5 hash function and pass the result of the previous step as an input to
        // this function.
        //
        // Use the first (n + 5) bytes, up to a maximum of 16, of the output from the MD5 hash as
        // the key for the AES symmetric key algorithm.
        let key_len = std::cmp::min(key.len() + 5, 16);
        let key = Md5::digest(builder)[..key_len].to_vec();

        Ok(key)
    }

    fn decrypt(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
        // Ensure that the ciphertext length is a multiple of 16 bytes.
        if ciphertext.len() % 16 != 0 {
            return Err(DecryptionError::InvalidCipherTextLength);
        }

        // There is nothing to decrypt if the ciphertext is empty or only contains the IV.
        if ciphertext.is_empty() || ciphertext.len() == 16 {
            return Ok(vec![]);
        }

        let mut iv = [0x00u8; 16];
        iv.copy_from_slice(&ciphertext[..16]);

        // Use the 128-bit AES-CBC algorithm with PKCS#7 padding to decrypt the ciphertext.
        let data = &mut ciphertext[16..].to_vec();

        Ok(Aes128CbcDec::new(key.into(), &iv.into())
            .decrypt_padded_mut::<Pkcs7>(data)
            .unwrap()
            .to_vec())
    }
}

/// Generates the encryption key for the document and, if `check_password`
/// is true, verifies that the key is correct.
pub fn get_encryption_key<P>(doc: &Document, password: P, check_password: bool) -> Result<Vec<u8>, DecryptionError>
where
    P: AsRef<[u8]>,
{
    let password = password.as_ref();

    let encryption_dict = doc
        .get_encrypted()
        .map_err(|_| DecryptionError::MissingEncryptDictionary)?;

    // Very early versions of PDF assume a key length of 40 bits
    let key_len = encryption_dict
        .get(b"Length")
        .unwrap_or(&DEFAULT_KEY_LEN)
        .as_i64()
        .map_err(|_| DecryptionError::InvalidType)? as usize
        / 8; // Length is in bits, convert to bytes

    // MD5 produces 128bit digests, so key_len must not be greater
    if key_len > 128 / 8 {
        return Err(DecryptionError::InvalidKeyLength);
    }

    // Make sure we support the encryption algorithm
    let algorithm = encryption_dict
        .get(b"V")
        .unwrap_or(&DEFAULT_ALGORITHM)
        .as_i64()
        .map_err(|_| DecryptionError::InvalidType)?;
    // Currently only support V = 1, 2 or 4
    match algorithm {
        1..=2 => {}
        4 => {}
        _ => return Err(DecryptionError::UnsupportedEncryption),
    }

    // Revision number dictates hashing strategy
    let revision = encryption_dict
        .get(b"R")
        .map_err(|_| DecryptionError::MissingRevision)?
        .as_i64()
        .map_err(|_| DecryptionError::InvalidType)?;
    if !(2..=4).contains(&revision) {
        return Err(DecryptionError::UnsupportedEncryption);
    }

    // Algorithm 3.2

    // 3.2.1 Start building up the key, starting with the user password plaintext,
    //  padding as needed to 32 bytes
    let mut key = Vec::with_capacity(128);
    let password_len = std::cmp::min(password.len(), 32);
    key.extend_from_slice(&password[0..password_len]);
    key.extend_from_slice(&PAD_BYTES[0..32 - password_len]);

    // 3.2.3 Append hashed owner password
    let hashed_owner_password = encryption_dict
        .get(b"O")
        .map_err(|_| DecryptionError::MissingOwnerPassword)?
        .as_str()
        .map_err(|_| DecryptionError::InvalidType)?;
    key.extend_from_slice(hashed_owner_password);

    // 3.2.4 Append the permissions (4 bytes)
    let permissions = encryption_dict
        .get(b"P")
        // we don't actually care about the permissions but we need the correct
        //  value to get the correct key
        .map_err(|_| DecryptionError::MissingPermissions)?
        .as_i64()
        .map_err(|_| DecryptionError::InvalidType)? as u32;
    key.extend_from_slice(&permissions.to_le_bytes());

    // 3.2.5 Append the first element of the file identifier
    let file_id_0 = doc
        .trailer
        .get(b"ID")
        .map_err(|_| DecryptionError::MissingFileID)?
        .as_array()
        .map_err(|_| DecryptionError::InvalidType)?
        .first()
        .ok_or(DecryptionError::InvalidType)?
        .as_str()
        .map_err(|_| DecryptionError::InvalidType)?;
    key.extend_from_slice(file_id_0);

    let encrypt_metadata = encryption_dict
        .get(b"EncryptMetadata")
        .unwrap_or(&Object::Boolean(true))
        .as_bool()
        .map_err(|_| DecryptionError::InvalidType)?;

    // 3.2.6 Revision >=4
    if revision >= 4 && !encrypt_metadata {
        key.extend_from_slice(&[0xFF_u8, 0xFF, 0xFF, 0xFF]);
    }

    // 3.2.7+8
    // Hash the contents of key and take the first key_len bytes
    let n_hashes = if revision < 3 { 1 } else { 51 };
    for _ in 0..n_hashes {
        let digest = Md5::digest(&key);
        key.truncate(key_len); // only keep the first key_len bytes
        key.copy_from_slice(&digest[..key_len]);
    }

    // Check that the password is correct
    if check_password {
        let check = compute_user_password(&key, revision, file_id_0);
        if let Ok(Object::String(expected, _)) = encryption_dict.get(b"U") {
            // Only first 16 bytes are significant, the rest are arbitrary padding
            if expected[..16] != check[..16] {
                return Err(DecryptionError::IncorrectPassword);
            }
        }
    }

    Ok(key)
}

fn compute_user_password<K, ID>(key: K, revision: i64, file_id_0: ID) -> Vec<u8>
where
    K: AsRef<[u8]>,
    ID: AsRef<[u8]>,
{
    let key = key.as_ref();
    let encryptor = Rc4::new(key);

    if revision == 2 {
        // Algorithm 3.4
        encryptor.decrypt(PAD_BYTES)
    } else {
        // Algorithm 3.5
        // 3.5.2
        let mut ctx = Md5::new();
        ctx.update(PAD_BYTES);

        // 3.5.3
        ctx.update(file_id_0);
        let hash = ctx.finalize();

        // 3.5.4
        let mut encrypted_hash = encryptor.encrypt(&hash[..]);

        // 3.5.5
        let mut temp_key = vec![0; key.len()];
        for i in 1..=19 {
            for (in_byte, out_byte) in key.iter().zip(temp_key.iter_mut()) {
                *out_byte = in_byte ^ (i as u8);
            }
            encrypted_hash = Rc4::new(&temp_key).encrypt(encrypted_hash);
        }

        // 3.5.6
        encrypted_hash.extend_from_slice(&PAD_BYTES[0..16]);

        encrypted_hash
    }
}

/// Decrypts `obj` and returns the content of the string or stream.
/// If obj is not an decryptable type, returns the NotDecryptable error.
pub fn decrypt_object<Key>(key: Key, obj_id: ObjectId, obj: &mut Object, aes: bool) -> Result<(), DecryptionError>
where
    Key: AsRef<[u8]>,
{
    // The cross-reference stream shall not be encrypted and strings appearing in the
    // cross-reference stream dictionary shall not be encrypted.
    let is_xref_stream = obj.as_stream()
        .map(|stream| stream.dict.has_type(b"XRef"))
        .unwrap_or(false);

    if is_xref_stream {
        return Ok(());
    }

    let key = key.as_ref();
    let len = if aes { key.len() + 9 } else { key.len() + 5 };
    let mut builder = Vec::<u8>::with_capacity(len);
    builder.extend_from_slice(key.as_ref());

    // Extend the key with the lower 3 bytes of the object number
    builder.extend_from_slice(&obj_id.0.to_le_bytes()[..3]);
    // and the lower 2 bytes of the generation number
    builder.extend_from_slice(&obj_id.1.to_le_bytes()[..2]);

    if aes {
        builder.append(&mut vec![0x73, 0x41, 0x6C, 0x54]);
    }

    // Now construct the rc4 key
    let key_len = std::cmp::min(key.len() + 5, 16);
    let rc4_key = &Md5::digest(builder)[..key_len];

    let encrypted = match obj {
        Object::Array(objects) => {
            for obj in objects {
                decrypt_object(key, obj_id, obj, aes)?;
            }

            return Ok(());
        }
        Object::Dictionary(objects) => {
            for (_, obj) in objects.iter_mut() {
                decrypt_object(key, obj_id, obj, aes)?;
            }

            return Ok(());
        }
        Object::String(content, _) => content,
        Object::Stream(stream) => &stream.content,
        _ => {
            return Ok(());
        }
    };

    let decrypted = if aes {
        // Ensure that the ciphertext length is a multiple of 16 bytes.
        if encrypted.len() % 16 != 0 {
            return Err(DecryptionError::InvalidCipherTextLength);
        }

        // There is nothing to decrypt if the ciphertext is empty or only contains the IV.
        if encrypted.is_empty() || encrypted.len() == 16 {
            vec![]
        } else {
            let mut iv = [0x00u8; 16];
            iv.copy_from_slice(&encrypted[..16]);

            // Use the 128-bit AES-CBC algorithm with PKCS#7 padding to decrypt the ciphertext.
            let data = &mut encrypted[16..].to_vec();

            Aes128CbcDec::new(rc4_key.into(), &iv.into())
                .decrypt_padded_mut::<Pkcs7>(data)
                .unwrap()
                .to_vec()
        }
    } else {
        // Use the RC4 algorithm to decrypt the ciphertext.
        Rc4::new(rc4_key).decrypt(encrypted)
    };

    match obj {
        Object::Stream(stream) => stream.set_content(decrypted.clone()),
        Object::String(content, _) => *content = decrypted.clone(),
        _ => (),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rc4_works() {
        let cases = [
            // Key, Plain, Cipher
            (
                String::from("Key"),
                String::from("Plaintext"),
                String::from("BBF316E8D940AF0AD3"),
            ),
            (String::from("Wiki"), String::from("pedia"), String::from("1021BF0420")),
        ];

        for (key, plain, cipher) in cases {
            // Reencode cipher from a hex string to a Vec<u8>
            let cipher = cipher.as_bytes();
            let mut cipher_bytes = Vec::with_capacity(cipher.len() / 2);
            for hex_pair in cipher.chunks_exact(2) {
                cipher_bytes.push(u8::from_str_radix(std::str::from_utf8(hex_pair).unwrap(), 16).unwrap());
            }

            let decryptor = Rc4::new(key);
            let decrypted = decryptor.decrypt(&cipher_bytes);
            assert_eq!(plain.as_bytes(), &decrypted[..]);
        }
    }
}
