use super::DecryptionError;
use super::rc4::Rc4;
use crate::encodings;
use crate::encryption::Permissions;
use crate::{Document, Error, Object};
use aes::cipher::{BlockModeDecrypt as _, BlockModeEncrypt as _, KeyInit as _, KeyIvInit as _};
use md5::{Digest as _, Md5};
use rand::RngExt as _;
use sha2::{Sha256, Sha384, Sha512};

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256EbcEnc = ecb::Encryptor<aes::Aes256>;

type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
type Aes256EbcDec = ecb::Decryptor<aes::Aes256>;
type AesBlock = aes::cipher::Block<aes::Aes128>;

fn aes_block_mut(block: &mut [u8]) -> &mut AesBlock {
    block.try_into().expect("AES block must be 16 bytes")
}

/// The 19-round RC4 loop of Algorithms 3, 5 and 7 (revisions 3 and above):
/// each round keys RC4 with the file key XORed by the round counter. The
/// rounds run 1..=19 when building `/O` and `/U`, and 19..=1 when recovering a
/// user password from `/O`; RC4 is symmetric, so the only difference is which
/// way the counter runs.
fn rc4_rounds(mut data: Vec<u8>, file_key: &[u8], rounds: impl IntoIterator<Item = u8>) -> Vec<u8> {
    let mut key = vec![0u8; file_key.len()];
    for i in rounds {
        for (in_byte, out_byte) in file_key.iter().zip(key.iter_mut()) {
            *out_byte = in_byte ^ i;
        }
        data = Rc4::new(&key).encrypt(&data);
    }
    data
}

/// Apply a 16-byte-block cipher to every block of `data` in place, which is how
/// the spec's "no padding, whole blocks" AES uses in Algorithms 2.B, 8, 9, 10
/// and 13 all behave. The key is always 32 bytes: either the file encryption key
/// or an Algorithm 2.B hash.
fn aes256_cbc(data: &mut [u8], key: [u8; 32], encrypt: bool) {
    let iv = [0u8; 16];
    if encrypt {
        let mut cipher = Aes256CbcEnc::new(&key.into(), &iv.into());
        for block in data.as_chunks_mut::<16>().0 {
            cipher.encrypt_block(aes_block_mut(block));
        }
    } else {
        let mut cipher = Aes256CbcDec::new(&key.into(), &iv.into());
        for block in data.as_chunks_mut::<16>().0 {
            cipher.decrypt_block(aes_block_mut(block));
        }
    }
}

/// The AES-256-ECB counterpart of [`aes256_cbc`], used for the 16-byte
/// `/Perms` block of Algorithms 10 and 13.
fn aes256_ecb(data: &mut [u8; 16], key: [u8; 32], encrypt: bool) {
    if encrypt {
        let mut cipher = Aes256EbcEnc::new(&key.into());
        for block in data.as_chunks_mut::<16>().0 {
            cipher.encrypt_block(aes_block_mut(block));
        }
    } else {
        let mut cipher = Aes256EbcDec::new(&key.into());
        for block in data.as_chunks_mut::<16>().0 {
            cipher.decrypt_block(aes_block_mut(block));
        }
    }
}

/// Reinterpret a 32-byte Algorithm 2.B hash as the AES-256 key it is.
fn aes256_key(hash: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    key.copy_from_slice(hash);
    key
}

/// Truncate a sanitized password to the 127 bytes Algorithms 2.A, 2.B, 11 and 12
/// operate on (ISO 32000-2 step (a)).
fn truncate_password(password: &[u8]) -> &[u8] {
    &password[..password.len().min(127)]
}

/// The first element of the trailer's `/ID` array, which Algorithms 2 and 5
/// feed into the MD5 that derives the file encryption key.
fn first_file_id(doc: &Document) -> Result<&[u8], DecryptionError> {
    doc.trailer
        .get(b"ID")
        .map_err(|_| DecryptionError::MissingFileID)?
        .as_array()
        .map_err(|_| DecryptionError::InvalidType)?
        .first()
        .ok_or(DecryptionError::InvalidType)?
        .as_str()
        .map_err(|_| DecryptionError::InvalidType)
}

// If the password string is less than 32 bytes long, pad it by appending the required number of
// additional bytes from the beginning of the following padding string.
const PAD_BYTES: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08, 0x2E, 0x2E, 0x00,
    0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// Truncate or pad `password` to exactly 32 bytes, filling from the start of
/// `PAD_BYTES` (ISO 32000-1, 7.6.3.3 step 2).
fn padded_password(password: &[u8]) -> [u8; 32] {
    let len = password.len().min(32);
    let mut bytes = [0u8; 32];

    bytes[..len].copy_from_slice(&password[..len]);
    bytes[len..].copy_from_slice(&PAD_BYTES[..32 - len]);

    bytes
}

/// The number of MD5 digest bytes an MD5-based revision keeps as its key: 5 for
/// R2, else `/Length / 8`. The maximum is 16 bytes (128 bits) because of MD5.
fn md5_key_length(algorithm: &PasswordAlgorithm) -> Result<usize, DecryptionError> {
    let n = if algorithm.revision >= 3 {
        algorithm.length.unwrap_or(40) / 8
    } else {
        5
    };

    if n > 16 {
        return Err(DecryptionError::InvalidKeyLength);
    }

    Ok(n)
}

/// The first `n` bytes of the MD5 digest of the padded `password`, rehashed 50
/// times for revision 3 and above, where `n` is [`md5_key_length`]. This is the
/// step shared by Algorithms 2, 3 and 7, which differ only in what they feed
/// the resulting key to.
fn hash_padded_password(algorithm: &PasswordAlgorithm, password: &[u8]) -> Result<Vec<u8>, DecryptionError> {
    let mut hasher = Md5::new();
    hasher.update(padded_password(password));

    let mut hash = hasher.finalize();

    // (Security handlers of revision 3 or greater) Do the following 50 times: take the output from
    // the previous MD5 hash and pass it as input into a new MD5 hash.
    if algorithm.revision >= 3 {
        for _ in 0..50 {
            hash = Md5::digest(hash);
        }
    }

    let n = md5_key_length(algorithm)?;

    Ok(hash[..n].to_vec())
}

/// Normalize an `/O` or `/U` value in place against the document's revision: 32
/// bytes up to revision 4, 48 bytes from revision 5 on, where writers' trailing
/// zero padding is truncated away.
fn normalize_r5_value(value: &mut Vec<u8>, revision: i64) -> Result<(), DecryptionError> {
    if revision <= 4 {
        if value.len() != 32 {
            Err(DecryptionError::InvalidHashLength)?;
        }
    } else if value.len() < 48 {
        Err(DecryptionError::InvalidHashLength)?;
    } else {
        value.truncate(48);
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct PasswordAlgorithm {
    pub(crate) encrypt_metadata: bool,
    pub(crate) length: Option<usize>,
    pub(crate) version: i64,
    pub(crate) revision: i64,
    pub(crate) owner_value: Vec<u8>,
    pub(crate) owner_encrypted: Vec<u8>,
    pub(crate) user_value: Vec<u8>,
    pub(crate) user_encrypted: Vec<u8>,
    pub(crate) permissions: Permissions,
    pub(crate) permission_encrypted: Vec<u8>,
}

impl TryFrom<&Document> for PasswordAlgorithm {
    type Error = Error;

    fn try_from(value: &Document) -> Result<Self, Self::Error> {
        // Get the encrypted dictionary.
        let encrypted = value
            .get_encrypted()
            .map_err(|_| DecryptionError::MissingEncryptDictionary)?;

        // Get the EncryptMetadata field.
        let encrypt_metadata = encrypted
            .get(b"EncryptMetadata")
            .unwrap_or(&Object::Boolean(true))
            .as_bool()
            .map_err(|_| DecryptionError::InvalidType)?;

        // Get the Length field if any. Make sure that if it is present that it is a 64-bit integer and
        // that it can be converted to an unsigned size.
        let length: Option<usize> = if encrypted.get(b"Length").is_ok() {
            Some(encrypted.get(b"Length")?.as_i64()?.try_into()?)
        } else {
            None
        };

        // Get the V field.
        let version = encrypted
            .get(b"V")
            .map_err(|_| DecryptionError::MissingVersion)?
            .as_i64()
            .map_err(|_| DecryptionError::InvalidType)?;

        // A code specifying the algorithm to be used in encrypting and decrypting the document.
        match version {
            // (Deprecated in PDF 2.0) An algorithm that is undocumented. This value shall not be
            // used.
            0 => return Err(DecryptionError::InvalidVersion)?,
            // (PDF 1.4; deprecated in PDF 2.0) Indicates the use of encryption of data using the
            // RC4 or AES algorithms with a file encryption key length of 40 bits.
            1 => (),
            // (PDF 1.4; deprecated in PDF 2.0) Indicates the use of encryption of data using the
            // RC4 or AES algorithms but permitting file encryption key lengths greater or 40 bits.
            2 => (),
            // Unpublished, 40-128 bit keys; not allowed in a conforming file.
            3 => return Err(DecryptionError::InvalidVersion)?,
            // RC4 or AES with a 128-bit key, per CF/StmF/StrF.
            4 => (),
            // AES with a 256-bit key, per CF/StmF/StrF/EFF.
            5 => (),
            // Unknown codes.
            _ => Err(DecryptionError::UnsupportedVersion)?,
        }

        // V4/V5 fix the key length (128/256 bits) and producers often omit /Length; without a
        // default the derivation below would fall back to 40 bits and fail the password check.
        let length = length.or(match version {
            4 => Some(128),
            5 => Some(256),
            _ => None,
        });

        // The length of the file encryption key shall only be present if V is 2 or 3 (but
        // documents with higher values for V seem to have this field).
        if let Some(length) = length {
            match version {
                // Although "Optional" and/or not required for V1 it appears in some documents
                // with a default value of 40.
                1 => {
                    if length != 40 {
                        Err(DecryptionError::InvalidKeyLength)?;
                    }
                }
                // The length of the file encryption key shall be a multiple of 8 in the range 40
                // to and including 128.
                2..=3 => {
                    if length % 8 != 0 || !(40..=128).contains(&length) {
                        Err(DecryptionError::InvalidKeyLength)?;
                    }
                }
                // The Length field should not be present if V is 4. However, if it is present it
                // must be 128.
                4 => {
                    if length != 128 {
                        Err(DecryptionError::InvalidKeyLength)?;
                    }
                }
                // The Length field should not be present if V is 5. However, if it is present it
                // must be 256.
                5 => {
                    if length != 256 {
                        Err(DecryptionError::InvalidKeyLength)?;
                    }
                }
                // The Length field may not be present otherwise.
                _ => Err(DecryptionError::InvalidKeyLength)?,
            }
        }

        // Get the R field.
        let revision = encrypted
            .get(b"R")
            .map_err(|_| DecryptionError::MissingRevision)?
            .as_i64()
            .map_err(|_| DecryptionError::InvalidType)?;

        // Get the owner value and owner encrypted blobs.
        let mut owner_value = encrypted
            .get(b"O")
            .map_err(|_| DecryptionError::MissingOwnerPassword)?
            .as_str()
            .map_err(|_| DecryptionError::InvalidType)?
            .to_vec();

        normalize_r5_value(&mut owner_value, revision)?;

        let owner_encrypted = encrypted
            .get(b"OE")
            .and_then(Object::as_str)
            .map(|s| s.to_vec())
            .ok()
            .unwrap_or_default();

        // The owner encrypted blob is required if R is 5 or greater and the blob shall be 32 bytes
        // long.
        if revision >= 5 && owner_encrypted.len() != 32 {
            Err(DecryptionError::InvalidCipherTextLength)?;
        }

        // Get the user value and user encrypted blobs.
        let mut user_value = encrypted
            .get(b"U")
            .map_err(|_| DecryptionError::MissingUserPassword)?
            .as_str()
            .map_err(|_| DecryptionError::InvalidType)?
            .to_vec();

        normalize_r5_value(&mut user_value, revision)?;

        let user_encrypted = encrypted
            .get(b"UE")
            .and_then(Object::as_str)
            .map(|s| s.to_vec())
            .ok()
            .unwrap_or_default();

        // The user encrypted blob is required if R is 5 or greater and the blob shall be 32 bytes
        // long.
        if revision >= 5 && user_encrypted.len() != 32 {
            Err(DecryptionError::InvalidCipherTextLength)?;
        }

        // Get the permission value and permission encrypted blobs.
        let permission_value = encrypted
            .get(b"P")
            .map_err(|_| DecryptionError::MissingPermissions)?
            .as_i64()
            .map_err(|_| DecryptionError::InvalidType)? as u64;

        let permissions = Permissions::from_bits_retain(permission_value);

        let permission_encrypted = encrypted
            .get(b"Perms")
            .and_then(Object::as_str)
            .map(|s| s.to_vec())
            .ok()
            .unwrap_or_default();

        // The permission encrypted blob is required if R is 65 or greater and the blob shall be
        // 16 bytes long.
        if revision >= 5 && permission_encrypted.len() != 16 {
            Err(DecryptionError::InvalidCipherTextLength)?;
        }

        Ok(Self {
            encrypt_metadata,
            length,
            version,
            revision,
            owner_value,
            owner_encrypted,
            user_value,
            user_encrypted,
            permissions,
            permission_encrypted,
        })
    }
}

impl PasswordAlgorithm {
    /// Sanitize the password (revision 4 and earlier).
    ///
    /// This implements the first step of Algorithm 2 as described in ISO 32000-2:2020 (PDF 2.0).
    ///
    /// This algorithm is deprecated in PDF 2.0.
    pub(crate) fn sanitize_password_r4(&self, password: &str) -> Result<Vec<u8>, DecryptionError> {
        // Convert the password to PDFDocEncoding.
        let password = encodings::string_to_bytes(&encodings::PDF_DOC_ENCODING, password);

        Ok(password)
    }

    /// Compute a file encryption key in order to encrypt/decrypt a document (revision 4 and
    /// earlier).
    ///
    /// This implements Algorithm 2 as described in ISO 32000-2:2020 (PDF 2.0).
    ///
    /// This algorithm is deprecated in PDF 2.0.
    pub(crate) fn compute_file_encryption_key_r4<P>(
        &self, doc: &Document, password: P,
    ) -> Result<Vec<u8>, DecryptionError>
    where
        P: AsRef<[u8]>,
    {
        let password = password.as_ref();

        // Initialize the MD5 hash function and pass the result as input to this function.
        let mut hasher = Md5::new();

        hasher.update(padded_password(password));

        // Pass the value of the encryption dictionary's O entry (owner password hash) to the MD5 hash
        // function.
        hasher.update(&self.owner_value);

        // Hash the P entry, low-order byte first: the value matters only for key derivation.
        hasher.update((self.permissions.bits() as u32).to_le_bytes());

        // Pass the first element of the file's file identifier array (the value of the ID entry in the
        // document's trailer dictionary to the MD5 hash function.
        hasher.update(first_file_id(doc)?);

        // (Security handlers of revision 4 or greater) If document metadata is not being encrypted,
        // pass 4 bytes with the value 0xFFFFFFFF to the MD5 hash function.
        if self.revision >= 4 && !self.encrypt_metadata {
            hasher.update(b"\xff\xff\xff\xff");
        }

        // Finish the hash.
        let mut hash = hasher.finalize();

        // Revision 3+: re-hash 50 times, each time feeding in the first n bytes of the previous digest.
        let n = md5_key_length(self)?;

        if self.revision >= 3 {
            for _ in 0..50 {
                hash = Md5::digest(&hash[..n]);
            }
        }

        // The key is the first n bytes of the final digest (n = 5 for R2, else /Length / 8).
        Ok(hash[..n].to_vec())
    }

    /// Sanitize the password (revision 6 and later).
    ///
    /// This implements the first step of Algorithm 2.A as described in ISO 32000-2:2020 (PDF 2.0).
    pub(crate) fn sanitize_password_r6(&self, password: &str) -> Result<Vec<u8>, DecryptionError> {
        // Normalize the password with SASLprep (RFC 4013) and return its UTF-8 bytes.
        Ok(stringprep::saslprep(password)?.as_bytes().to_vec())
    }

    /// Compute a file encryption key in order to encrypt/decrypt a document (revision 6 and
    /// later).
    ///
    /// This implements Algorithm 2.A as described in ISO 32000-2:2020 (PDF 2.0).
    fn compute_file_encryption_key_r6<P>(&self, password: P) -> Result<Vec<u8>, DecryptionError>
    where
        P: AsRef<[u8]>,
    {
        let password = truncate_password(password.as_ref());

        let hashed_owner_password = &self.owner_value[0..][..32];
        let owner_validation_salt = &self.owner_value[32..][..8];
        let owner_key_salt = &self.owner_value[40..][..8];

        let hashed_user_password = &self.user_value[0..][..32];
        let user_validation_salt = &self.user_value[32..][..8];
        let user_key_salt = &self.user_value[40..][..8];

        // Algorithm 2.B over password + owner validation salt + U; a match with O means the
        // owner password.
        if self.compute_hash(password, owner_validation_salt, Some(&self.user_value))? == hashed_owner_password {
            // Algorithm 2.B over password + owner key salt + U gives the key for /OE.
            let key = aes256_key(&self.compute_hash(password, owner_key_salt, Some(&self.user_value))?);

            // Decrypt /OE with AES-256 CBC, zero IV, no padding: the result is the file key.
            let mut owner_encrypted = self.owner_encrypted.clone();
            aes256_cbc(&mut owner_encrypted, key, false);

            return Ok(owner_encrypted);
        }

        // Not in the spec, but a precaution: 2.B over password + user validation salt;
        // a match with U means the user password.
        if self.compute_hash(password, user_validation_salt, None)? == hashed_user_password {
            // Algorithm 2.B over password + user key salt gives the key for /UE.
            let key = aes256_key(&self.compute_hash(password, user_key_salt, None)?);

            // Decrypt /UE with AES-256 CBC, zero IV, no padding: the result is the file key.
            let mut user_encrypted = self.user_encrypted.clone();
            aes256_cbc(&mut user_encrypted, key, false);

            // Algorithm 13: /Perms must decrypt to "adb" at bytes 9-11 with permissions equal to P.
            self.validate_permissions(&user_encrypted)?;

            return Ok(user_encrypted);
        }

        Err(DecryptionError::IncorrectPassword)
    }

    /// Compute a hash (revision 6 and later).
    ///
    /// This implements Algorithm 2.B as described in ISO 32000-2:2020 (PDF 2.0).
    fn compute_hash<P, S>(&self, password: P, salt: S, user_key: Option<&[u8]>) -> Result<Vec<u8>, DecryptionError>
    where
        P: AsRef<[u8]>,
        S: AsRef<[u8]>,
    {
        let password = password.as_ref();
        let salt = salt.as_ref();

        // Take the SHA-256 hash of the original input to the algorithm and name the resulting 32
        // bytes, K.
        let mut hasher = Sha256::new();

        hasher.update(password);
        hasher.update(salt);

        if let Some(user_key) = user_key {
            hasher.update(user_key);
        }

        let mut k = hasher.finalize().to_vec();

        // Revision 5 uses a simplified hash algorithm that simply calculates the SHA-256 hash of
        // the original input to the algorithm.
        if self.revision == 5 {
            return Ok(k);
        }

        let mut k1 =
            Vec::with_capacity(64 * (password.len() + 64 + user_key.map(|user_key| user_key.len()).unwrap_or(0)));

        // Perform the following steps at least 64 times, until the value of the last byte in K is
        // less than or equal to (round number) - 32.
        for round in 1.. {
            // K0 is password + K + U when checking/creating the owner key, else password + K;
            // K1 is 64 repetitions of K0.
            k1.clear();

            for _ in 0..64 {
                k1.extend_from_slice(password);
                k1.extend_from_slice(&k);

                if let Some(user_key) = user_key {
                    k1.extend_from_slice(user_key);
                }
            }

            // AES-128 CBC, no padding, key = K[0..16], IV = K[16..32]. 64 repetitions make K1 a
            // multiple of 16 bytes, so no padding is needed.
            let key: &[u8; 16] = k[..16].try_into().expect("hash key must be 16 bytes");
            let iv: &[u8; 16] = k[16..32].try_into().expect("hash IV must be 16 bytes");

            let mut encryptor = Aes128CbcEnc::new(key.into(), iv.into());

            for block in k1.as_chunks_mut::<16>().0 {
                encryptor.encrypt_block(aes_block_mut(block));
            }

            let e = k1;

            // Pick the next hash (SHA-256/384/256) by E[0..16] mod 3; it becomes the new K.
            k = match e[..16].iter().map(|v| *v as u32).sum::<u32>() % 3 {
                0 => Sha256::digest(&e).to_vec(),
                1 => Sha384::digest(&e).to_vec(),
                2 => Sha512::digest(&e).to_vec(),
                _ => unreachable!(),
            };

            // Repeat the round while E's last byte is greater than round - 32.
            if round >= 64 && e.last().copied().unwrap_or(0) as u32 <= round - 32 {
                break;
            }

            // Move e into k1 for the next round (to reuse k1).
            k1 = e;
        }

        // The first 32 bytes of the final K are the output of the algorithm.
        k.truncate(32);

        Ok(k)
    }

    /// Compute the encryption dictionary's O-entry value (revision 4 and earlier).
    ///
    /// This implements Algorithm 3 as described in ISO 32000-2:2020 (PDF 2.0).
    ///
    /// This algorithm is deprecated in PDF 2.0.
    pub(crate) fn compute_hashed_owner_password_r4<O, U>(
        &self, owner_password: Option<O>, user_password: U,
    ) -> Result<Vec<u8>, DecryptionError>
    where
        O: AsRef<[u8]>,
        U: AsRef<[u8]>,
    {
        let user_password = user_password.as_ref();

        // Pad or truncate the owner string. If there is no owner password, use the user password
        // instead.
        let password = owner_password
            .as_ref()
            .map(|password| password.as_ref())
            .unwrap_or(user_password);

        let hash = hash_padded_password(self, password)?;

        // Encrypt the result of the previous step using an RC4 encryption function with the RC4 file
        // encryption key obtained in the step before the previous step.
        let mut result = Rc4::new(&hash).encrypt(padded_password(user_password));

        // Revision 3+: 19 RC4 rounds, each key being the file key XORed with the counter byte
        // (1 to 19).
        if self.revision >= 3 {
            result = rc4_rounds(result, &hash, 1..=19);
        }

        // Store the output from the final invocation of the RC4 function as the value of the O entry
        // in the encryption dictionary.
        Ok(result)
    }

    /// Compute the encryption dictionary's U-entry value (revision 2).
    ///
    /// This implements Algorithm 4 as described in ISO 32000-2:2020 (PDF 2.0).
    ///
    /// This algorithm is deprecated in PDF 2.0.
    pub(crate) fn compute_hashed_user_password_r2<U>(
        &self, doc: &Document, user_password: U,
    ) -> Result<Vec<u8>, DecryptionError>
    where
        U: AsRef<[u8]>,
    {
        // Create a file encryption key based on the user password string.
        let file_encryption_key = self.compute_file_encryption_key_r4(doc, user_password)?;

        // Encrypt the 32-byte padding string using an RC4 encryption function with the file encryption
        // key from the preceding step.
        let result = Rc4::new(&file_encryption_key).encrypt(PAD_BYTES);

        // Store the result of the previous step as the value of the U entry in the encryption dictionary.
        Ok(result)
    }

    /// Compute the encryption dictionary's U-entry value (revision 3 or 4).
    ///
    /// This implements Algorithm 5 as described in ISO 32000-2:2020 (PDF 2.0).
    ///
    /// This algorithm is deprecated in PDF 2.0.
    pub(crate) fn compute_hashed_user_password_r3_r4<U>(
        &self, doc: &Document, user_password: U,
    ) -> Result<Vec<u8>, DecryptionError>
    where
        U: AsRef<[u8]>,
    {
        // Create a file encryption key based on the user password string.
        let file_encryption_key = self.compute_file_encryption_key_r4(doc, user_password)?;

        // Initialize the MD5 hash function and pass the 32-byte padding string.
        let mut hasher = Md5::new();

        hasher.update(PAD_BYTES);

        // Pass the first element of the file's file identifier array (the value of the ID entry in the
        // document's trailer dictionary) to the hash function and finish the hash.
        hasher.update(first_file_id(doc)?);

        let hash = hasher.finalize();

        // Encrypt the 16-byte result of the hash, using an RC4 encryption function with the file
        // encryption key.
        let result = Rc4::new(&file_encryption_key).encrypt(hash);

        // 19 RC4 rounds, each key being the file key XORed with the counter byte (1 to 19).
        let mut result = rc4_rounds(result, &file_encryption_key, 1..=19);

        // Pad the final RC4 output to 32 bytes; that is /U.
        result.resize(32, 0);

        let mut rng = rand::rng();
        rng.fill(&mut result[16..]);

        Ok(result)
    }

    /// Authenticate the user password (revision 4 and earlier).
    ///
    /// This implements Algorithm 6 as described in ISO 32000-2:2020 (PDF 2.0).
    ///
    /// This algorithm is deprecated in PDF 2.0.
    fn authenticate_user_password_r4<U>(&self, doc: &Document, user_password: U) -> Result<(), DecryptionError>
    where
        U: AsRef<[u8]>,
    {
        // All but the last step of Algorithm 4 (R2) or 5 (R3/R4): compute the /U value.
        let hashed_user_password = match self.revision {
            2 => self.compute_hashed_user_password_r2(doc, &user_password)?,
            3 | 4 => self.compute_hashed_user_password_r3_r4(doc, &user_password)?,
            _ => return Err(DecryptionError::InvalidRevision),
        };

        // A match with /U (first 16 bytes for R3+) means the user password is correct.
        let len = match self.revision {
            3 | 4 => 16,
            _ => hashed_user_password.len(),
        };

        if self.user_value.len() < len {
            return Err(DecryptionError::InvalidHashLength);
        }

        if hashed_user_password[..len] != self.user_value[..len] {
            return Err(DecryptionError::IncorrectPassword);
        }

        Ok(())
    }

    /// Authenticate the owner password (revision 4 and earlier).
    ///
    /// This implements Algorithm 7 as described in ISO 32000-2:2020 (PDF 2.0).
    ///
    /// This algorithm is deprecated in PDF 2.0.
    fn authenticate_owner_password_r4<O>(&self, doc: &Document, owner_password: O) -> Result<(), DecryptionError>
    where
        O: AsRef<[u8]>,
    {
        self.recover_user_password_r4(doc, owner_password).map(|_| ())
    }

    /// Recover the padded (32-byte) user password from the encryption
    /// dictionary's `/O` entry, given a candidate owner password — the
    /// bulk of Algorithm 7 (revision 4 and earlier) — authenticating the
    /// recovered value against `/U` before returning it.
    ///
    /// The owner and user passwords are independent credentials; either is
    /// sufficient to open the document, but they need not be equal, and
    /// Algorithm 2 (the file encryption key) is always derived from the
    /// *user* password. A caller that only checks "does this password
    /// authenticate" (e.g. [`Self::authenticate_owner_password_r4`]) and
    /// then reuses the literal input for Algorithm 2 derives the wrong file
    /// key whenever the caller supplied the owner password and it differs
    /// from the user password: decryption reports success (dictionaries,
    /// names, integers and references are never encrypted, so structure
    /// and page count still resolve correctly), but every decrypted string
    /// and stream comes out as garbage. This method exists so callers can
    /// recover and use the correct value instead — see
    /// [`crate::encryption::PasswordAlgorithm::resolve_password_for_key_derivation`].
    pub(crate) fn recover_user_password_r4<O>(
        &self, doc: &Document, owner_password: O,
    ) -> Result<Vec<u8>, DecryptionError>
    where
        O: AsRef<[u8]>,
    {
        let hash = hash_padded_password(self, owner_password.as_ref())?;

        // Decrypt the value of the encryption dictionary's O entry, using an RC4 encryption function
        // with the file encryption key to retrieve the user password.
        let mut result = self.owner_value.to_vec();

        // Revision 3+: 19 RC4 rounds, each key being the file key XORed with the counter byte
        // (19 down to 1). RC4 is symmetric, so decrypting is the same round
        // function run in the opposite order.
        if self.revision >= 3 {
            result = rc4_rounds(result, &hash, (1..=19).rev());
        }

        // Decrypt /O with RC4 using the file encryption key.
        result = Rc4::new(&hash).decrypt(&result);

        // The result purports to be the user password: authenticate it with Algorithm 5, then feed
        // it to Algorithm 2 to derive the file key.
        self.authenticate_user_password_r4(doc, &result)?;
        Ok(result)
    }

    /// Compute the encryption dictionary's U-entry value (revision 6).
    ///
    /// This implements Algorithm 8 as described in ISO 32000-2:2020 (PDF 2.0).
    pub(crate) fn compute_hashed_user_password_r6<K, U>(
        &self, file_encryption_key: K, user_password: U,
    ) -> Result<(Vec<u8>, Vec<u8>), DecryptionError>
    where
        K: AsRef<[u8]>,
        U: AsRef<[u8]>,
    {
        let file_encryption_key = file_encryption_key.as_ref();
        let user_password = user_password.as_ref();

        // /U = 2.B(password + user validation salt) + validation salt + user key salt, where the
        // two 8-byte salts are fresh random bytes.
        self.compute_salted_value_r6(file_encryption_key, user_password, None)
    }

    /// Compute the encryption dictionary's O-entry value (revision 6).
    ///
    /// This implements Algorithm 9 as described in ISO 32000-2:2020 (PDF 2.0).
    pub(crate) fn compute_hashed_owner_password_r6<K, O>(
        &self, file_encryption_key: K, owner_password: O,
    ) -> Result<(Vec<u8>, Vec<u8>), DecryptionError>
    where
        K: AsRef<[u8]>,
        O: AsRef<[u8]>,
    {
        let file_encryption_key = file_encryption_key.as_ref();
        let owner_password = owner_password.as_ref();

        // /O = 2.B(password + owner validation salt + U) + validation salt + owner key salt, where
        // the two 8-byte salts are fresh random bytes. Unlike /U, the owner entry
        // mixes /U into both hashes.
        self.compute_salted_value_r6(file_encryption_key, owner_password, Some(&self.user_value))
    }

    /// Build the salted 48-byte value and the file-key blob that Algorithms 8
    /// and 9 pair together: a 32-byte Algorithm 2.B hash followed by a fresh
    /// validation salt and key salt, plus the file encryption key encrypted
    /// (AES-256 CBC, zero IV, no padding) under the key-salt hash.
    ///
    /// `user_key` is `/U` for the owner entry and absent for the user entry.
    fn compute_salted_value_r6(
        &self, file_encryption_key: &[u8], password: &[u8], user_key: Option<&[u8]>,
    ) -> Result<(Vec<u8>, Vec<u8>), DecryptionError> {
        let mut value = [0u8; 48];
        rand::rng().fill(&mut value[32..]);

        let validation_salt = &value[32..][..8];
        let hashed = self.compute_hash(password, validation_salt, user_key)?;
        value[..32].copy_from_slice(&hashed);

        // Compute the 32-byte hash using algorithm 2.B with an input string consisting of the
        // UTF-8 password concatenated with the key salt.
        let key_salt = &value[40..][..8];
        let key = aes256_key(&self.compute_hash(password, key_salt, user_key)?);

        let mut encrypted = file_encryption_key.to_vec();
        aes256_cbc(&mut encrypted, key, true);

        Ok((value.to_vec(), encrypted))
    }

    /// Compute the encryption dictionary's Perms (permissions) value (revision 6 and later).
    ///
    /// This implements Algorithm 10 as described in ISO 32000-2:2020 (PDF 2.0).
    pub(crate) fn compute_permissions<K>(&self, file_encryption_key: K) -> Result<Vec<u8>, DecryptionError>
    where
        K: AsRef<[u8]>,
    {
        let file_encryption_key = file_encryption_key.as_ref();
        let mut bytes = [0u8; 16];

        // Record the 8 bytes of permission in the bytes 0-7 of the block, low order byte first.
        bytes[..8].copy_from_slice(&u64::to_le_bytes(self.permissions.bits()));

        // Set byte 8 to ASCII character "T" or "F" according to the EncryptMetadata boolean.
        bytes[8] = if self.encrypt_metadata { b'T' } else { b'F' };

        // Set bytes 9-11 to the ASCII characters "a", "d", "b".
        bytes[9..][..3].copy_from_slice(b"adb");

        // Set bytes 12-15 to 4 bytes of random data, which will be ignored.
        let mut rng = rand::rng();
        rng.fill(&mut bytes[12..][..4]);

        // Encrypt the 16-byte block using AES-256 in ECB mode with an initialization vector of
        // zero, using the file encryption key as the key.
        aes256_ecb(&mut bytes, aes256_key(file_encryption_key), true);

        // The result (16 bytes) is stored as the Perms string, and checked for validity when the
        // file is opened.
        Ok(bytes.to_vec())
    }

    /// Authenticate the user password (revision 6 and later).
    ///
    /// This implements Algorithm 11 as described in ISO 32000-2:2020 (PDF 2.0).
    fn authenticate_user_password_r6<U>(&self, user_password: U) -> Result<(), DecryptionError>
    where
        U: AsRef<[u8]>,
    {
        // Algorithm 2.B over password + user validation salt; a match with U is the user password.
        self.authenticate_password_r6(user_password.as_ref(), &self.user_value, None)
    }

    /// Authenticate the owner password (revision 6 and later).
    ///
    /// This implements Algorithm 12 as described in ISO 32000-2:2020 (PDF 2.0).
    fn authenticate_owner_password_r6<O>(&self, owner_password: O) -> Result<(), DecryptionError>
    where
        O: AsRef<[u8]>,
    {
        // Algorithm 2.B over password + owner validation salt + U; a match with O is the owner password.
        self.authenticate_password_r6(owner_password.as_ref(), &self.owner_value, Some(&self.user_value))
    }

    /// Shared implementation of Algorithms 11 and 12: hash the password with
    /// `value`'s validation salt and compare against `value`'s leading 32
    /// bytes. `user_key` is `/U` for the owner entry and absent for the user one.
    fn authenticate_password_r6(
        &self, password: &[u8], value: &[u8], user_key: Option<&[u8]>,
    ) -> Result<(), DecryptionError> {
        let hash = self.compute_hash(truncate_password(password), &value[32..][..8], user_key)?;

        if hash != value[..32] {
            return Err(DecryptionError::IncorrectPassword);
        }

        Ok(())
    }

    /// Validate the permissions (revision 6 and later).
    ///
    /// This implements Algorithm 13 as described in ISO 32000-2:2020 (PDF 2.0).
    fn validate_permissions<K>(&self, file_encryption_key: K) -> Result<(), DecryptionError>
    where
        K: AsRef<[u8]>,
    {
        let file_encryption_key = file_encryption_key.as_ref();

        // Decrypt the 16 byte Perms string using AES-256 in ECB mode with an initialization vector
        // of zero and the file encryption key as the key.
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&self.permission_encrypted);
        aes256_ecb(&mut bytes, aes256_key(file_encryption_key), false);

        // Verify that bytes 9-11 of the result are the characters "a", "d", "b".
        if &bytes[9..][..3] != b"adb" {
            return Err(DecryptionError::IncorrectPassword);
        }

        // Bytes 0-3 of the decrypted Perms entry, treated as a little-endian integer, are the
        // user permissions. They should match the value in the P key.
        if bytes[..3] != u64::to_le_bytes(self.permissions.bits())[..3] {
            return Err(DecryptionError::IncorrectPassword);
        }

        // Byte 8 should match the ASCII character "T" or "F" according to the boolean value of the
        // EncryptMetadata key.
        if bytes[8] != if self.encrypt_metadata { b'T' } else { b'F' } {
            return Err(DecryptionError::IncorrectPassword);
        }

        Ok(())
    }

    /// Sanitize the password.
    pub fn sanitize_password(&self, password: &str) -> Result<Vec<u8>, DecryptionError> {
        match self.revision {
            2..=4 => self.sanitize_password_r4(password),
            5..=6 => self.sanitize_password_r6(password),
            _ => Err(DecryptionError::UnsupportedRevision),
        }
    }

    /// Compute the file encryption key used to encrypt/decrypt the document.
    pub fn compute_file_encryption_key<P>(&self, doc: &Document, password: P) -> Result<Vec<u8>, DecryptionError>
    where
        P: AsRef<[u8]>,
    {
        match self.revision {
            2..=4 => self.compute_file_encryption_key_r4(doc, password),
            5..=6 => self.compute_file_encryption_key_r6(password),
            _ => Err(DecryptionError::UnsupportedRevision),
        }
    }

    /// Resolve a caller-supplied password to the bytes that must actually be
    /// passed to [`Self::compute_file_encryption_key`].
    ///
    /// The owner and user passwords are independent credentials — either is
    /// sufficient to open a document — but for revisions 4 and earlier the
    /// file encryption key (Algorithm 2) is always derived from the *user*
    /// password specifically. If the caller supplied the user password
    /// directly, it is returned as-is. If it only authenticates as the
    /// *owner* password, the true user password recovered from `/O`
    /// (Algorithm 7) is returned instead of the literal input.
    ///
    /// Skipping this resolution — deriving the file key straight from
    /// whatever password authenticated — silently produces the wrong key
    /// whenever the owner and user passwords differ: `/O`, `/U` and every
    /// unencrypted structure (dictionaries, names, integers, references)
    /// still parse correctly, so decryption reports success, but every
    /// decrypted string and stream comes out as garbage. Revisions 5 and 6
    /// have no equivalent gap — their file key is derived from `/OE`/`/UE`
    /// via the password hash directly — so the input passes through
    /// unchanged.
    pub fn resolve_password_for_key_derivation<P>(
        &self, doc: &Document, password: P,
    ) -> Result<Vec<u8>, DecryptionError>
    where
        P: AsRef<[u8]>,
    {
        let password = password.as_ref();
        match self.revision {
            2..=4 => {
                if self.authenticate_user_password_r4(doc, password).is_ok() {
                    Ok(password.to_vec())
                } else {
                    self.recover_user_password_r4(doc, password)
                }
            }
            5..=6 => Ok(password.to_vec()),
            _ => Err(DecryptionError::UnsupportedRevision),
        }
    }

    /// Authenticate the owner password.
    pub fn authenticate_user_password<U>(&self, doc: &Document, user_password: U) -> Result<(), DecryptionError>
    where
        U: AsRef<[u8]>,
    {
        match self.revision {
            2..=4 => self.authenticate_user_password_r4(doc, user_password),
            5..=6 => self.authenticate_user_password_r6(user_password),
            _ => Err(DecryptionError::UnsupportedRevision),
        }
    }

    /// Authenticate the owner password.
    pub fn authenticate_owner_password<O>(&self, doc: &Document, owner_password: O) -> Result<(), DecryptionError>
    where
        O: AsRef<[u8]>,
    {
        match self.revision {
            2..=4 => self.authenticate_owner_password_r4(doc, owner_password),
            5..=6 => self.authenticate_owner_password_r6(owner_password),
            _ => Err(DecryptionError::UnsupportedRevision),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Permissions;
    use crate::creator::tests::create_document;
    use crate::encryption::PasswordAlgorithm;
    use rand::RngExt as _;

    #[test]
    fn authenticate_password_r2() {
        let document = create_document();

        let mut algorithm = PasswordAlgorithm {
            encrypt_metadata: true,
            length: None,
            version: 1,
            revision: 2,
            permissions: Permissions::all(),
            ..Default::default()
        };

        let owner_password = "owner";
        let user_password = "user";

        // Sanitize the passwords.
        let owner_password = algorithm.sanitize_password_r4(owner_password).unwrap();
        let user_password = algorithm.sanitize_password_r4(user_password).unwrap();

        // Compute the hashed values.
        algorithm.owner_value = algorithm
            .compute_hashed_owner_password_r4(Some(&owner_password), &user_password)
            .unwrap();

        algorithm.user_value = algorithm
            .compute_hashed_user_password_r2(&document, &user_password)
            .unwrap();

        // Assert that the correct passwords authenticate.
        assert!(
            algorithm
                .authenticate_owner_password_r4(&document, &owner_password)
                .is_ok()
        );
        assert!(
            algorithm
                .authenticate_user_password_r4(&document, &user_password)
                .is_ok()
        );

        // Assert that the swapped passwords do not authenticate.
        assert!(
            algorithm
                .authenticate_owner_password_r4(&document, user_password)
                .is_err()
        );
        assert!(
            algorithm
                .authenticate_user_password_r4(&document, owner_password)
                .is_err()
        );
    }

    #[test]
    fn authenticate_password_r3() {
        let document = create_document();

        let mut algorithm = PasswordAlgorithm {
            encrypt_metadata: true,
            length: Some(40),
            version: 2,
            revision: 3,
            permissions: Permissions::all(),
            ..Default::default()
        };

        let owner_password = "owner";
        let user_password = "user";

        // Sanitize the passwords.
        let owner_password = algorithm.sanitize_password_r4(owner_password).unwrap();
        let user_password = algorithm.sanitize_password_r4(user_password).unwrap();

        // Compute the hashed values.
        algorithm.owner_value = algorithm
            .compute_hashed_owner_password_r4(Some(&owner_password), &user_password)
            .unwrap();

        algorithm.user_value = algorithm
            .compute_hashed_user_password_r3_r4(&document, &user_password)
            .unwrap();

        // Assert that the correct passwords authenticate.
        assert!(
            algorithm
                .authenticate_owner_password_r4(&document, &owner_password)
                .is_ok()
        );
        assert!(
            algorithm
                .authenticate_user_password_r4(&document, &user_password)
                .is_ok()
        );

        // Assert that the swapped passwords do not authenticate.
        assert!(
            algorithm
                .authenticate_owner_password_r4(&document, user_password)
                .is_err()
        );
        assert!(
            algorithm
                .authenticate_user_password_r4(&document, owner_password)
                .is_err()
        );
    }

    #[test]
    fn authenticate_password_r4() {
        let document = create_document();

        let mut algorithm = PasswordAlgorithm {
            encrypt_metadata: true,
            length: Some(128),
            version: 4,
            revision: 4,
            permissions: Permissions::all(),
            ..Default::default()
        };

        let owner_password = "owner";
        let user_password = "user";

        // Sanitize the passwords.
        let owner_password = algorithm.sanitize_password_r4(owner_password).unwrap();
        let user_password = algorithm.sanitize_password_r4(user_password).unwrap();

        // Compute the hashed values.
        algorithm.owner_value = algorithm
            .compute_hashed_owner_password_r4(Some(&owner_password), &user_password)
            .unwrap();

        algorithm.user_value = algorithm
            .compute_hashed_user_password_r3_r4(&document, &user_password)
            .unwrap();

        // Assert that the correct passwords authenticate.
        assert!(
            algorithm
                .authenticate_owner_password_r4(&document, &owner_password)
                .is_ok()
        );
        assert!(
            algorithm
                .authenticate_user_password_r4(&document, &user_password)
                .is_ok()
        );

        // Assert that the swapped passwords do not authenticate.
        assert!(
            algorithm
                .authenticate_owner_password_r4(&document, user_password)
                .is_err()
        );
        assert!(
            algorithm
                .authenticate_user_password_r4(&document, owner_password)
                .is_err()
        );
    }

    /// Revisions 5 and 6 share the hash machinery and differ only in it:
    /// revision 5 uses the simplified hash of Algorithm 2.B, revision 6 the
    /// round-based one.
    #[test]
    fn authenticate_password_r5_r6() {
        for revision in [5, 6] {
            let mut algorithm = PasswordAlgorithm {
                encrypt_metadata: true,
                version: 5,
                revision,
                permissions: Permissions::all(),
                ..Default::default()
            };

            let owner_password = "owner";
            let user_password = "user";

            // Sanitize the passwords.
            let owner_password = algorithm.sanitize_password_r6(owner_password).unwrap();
            let user_password = algorithm.sanitize_password_r6(user_password).unwrap();

            // Compute the hashed values.
            let mut file_encryption_key = [0u8; 32];

            let mut rng = rand::rng();
            rng.fill(&mut file_encryption_key);

            let (user_value, user_encrypted) = algorithm
                .compute_hashed_user_password_r6(file_encryption_key, &user_password)
                .unwrap();

            algorithm.user_value = user_value;
            algorithm.user_encrypted = user_encrypted;

            let (owner_value, owner_encrypted) = algorithm
                .compute_hashed_owner_password_r6(file_encryption_key, &owner_password)
                .unwrap();

            algorithm.owner_value = owner_value;
            algorithm.owner_encrypted = owner_encrypted;

            algorithm.permission_encrypted = algorithm.compute_permissions(file_encryption_key).unwrap();

            // Assert that the correct passwords authenticate.
            assert!(algorithm.authenticate_owner_password_r6(&owner_password).is_ok());
            assert!(algorithm.authenticate_user_password_r6(&user_password).is_ok());

            // Assert that the swapped passwords do not authenticate.
            assert!(algorithm.authenticate_owner_password_r6(&user_password).is_err());
            assert!(algorithm.authenticate_user_password_r6(&owner_password).is_err());

            // Assert that the permissions validate correctly.
            assert!(algorithm.validate_permissions(file_encryption_key).is_ok());

            // Assert that the file encryption key is equal for both passwords.
            for password in [&owner_password, &user_password] {
                let key = algorithm.compute_file_encryption_key_r6(password).unwrap();
                assert_eq!(&file_encryption_key[..], key, "revision {revision}");
            }
        }
    }

    /// Some PDF writers (e.g. Adobe) pad /O and /U to 127 bytes with trailing
    /// zeros instead of the spec-required 48. Verify that `try_from` accepts
    /// these and truncates to the correct 48-byte length.
    #[test]
    fn r6_padded_owner_user_values_accepted() {
        use crate::{Document, Object, StringFormat, dictionary};

        let mut doc = Document::with_version("2.0");

        // Build valid 48-byte /O and /U (contents don't matter for parsing).
        let o_48 = vec![0xAAu8; 48];
        let u_48 = vec![0xBBu8; 48];

        // Pad to 127 bytes with trailing zeros — mimics real-world Adobe PDFs.
        let mut o_127 = o_48.clone();
        o_127.resize(127, 0u8);
        let mut u_127 = u_48.clone();
        u_127.resize(127, 0u8);

        let encrypt_dict = dictionary! {
            "Filter" => "Standard",
            "V" => Object::Integer(5),
            "R" => Object::Integer(6),
            "Length" => Object::Integer(256),
            "O" => Object::String(o_127, StringFormat::Literal),
            "OE" => Object::String(vec![0xCCu8; 32], StringFormat::Literal),
            "U" => Object::String(u_127, StringFormat::Literal),
            "UE" => Object::String(vec![0xDDu8; 32], StringFormat::Literal),
            "P" => Object::Integer(-3388),
            "Perms" => Object::String(vec![0xEEu8; 16], StringFormat::Literal)
        };

        let encrypt_id = doc.add_object(encrypt_dict);
        doc.trailer.set("Encrypt", Object::Reference(encrypt_id));

        let algo = PasswordAlgorithm::try_from(&doc).expect("should accept padded /O and /U longer than 48 bytes");

        // Verify the values were truncated to the spec-required 48 bytes.
        assert_eq!(algo.owner_value.len(), 48);
        assert_eq!(algo.user_value.len(), 48);
        assert_eq!(&algo.owner_value, &o_48);
        assert_eq!(&algo.user_value, &u_48);
    }

    /// Verify that /O and /U values shorter than 48 bytes are still rejected
    /// for R >= 5.
    #[test]
    fn r6_short_owner_user_values_rejected() {
        use crate::{Document, Object, StringFormat, dictionary};

        let mut doc = Document::with_version("2.0");

        let encrypt_dict = dictionary! {
            "Filter" => "Standard",
            "V" => Object::Integer(5),
            "R" => Object::Integer(6),
            "Length" => Object::Integer(256),
            "O" => Object::String(vec![0xAAu8; 47], StringFormat::Literal),
            "OE" => Object::String(vec![0xCCu8; 32], StringFormat::Literal),
            "U" => Object::String(vec![0xBBu8; 48], StringFormat::Literal),
            "UE" => Object::String(vec![0xDDu8; 32], StringFormat::Literal),
            "P" => Object::Integer(-3388),
            "Perms" => Object::String(vec![0xEEu8; 16], StringFormat::Literal)
        };

        let encrypt_id = doc.add_object(encrypt_dict);
        doc.trailer.set("Encrypt", Object::Reference(encrypt_id));

        assert!(
            PasswordAlgorithm::try_from(&doc).is_err(),
            "should reject /O shorter than 48 bytes"
        );
    }
}
