use std::fmt;
use crate::{Document, Object, ObjectId};
use crate::rc4::Rc4;

#[derive(Debug)]
pub enum DecryptionError {
    MissingEncryptDictionary,
    MissingRevision,
    MissingOwnerPassword,
    MissingPermissions,
    MissingFileID,

    InvalidKeyLength,
    InvalidRevision,
    // Used generically when the object type violates the spec
    InvalidType,

    NotDecryptable,
    IncorrectPassword,

    UnsupportedEncryption,
}

impl std::error::Error for DecryptionError {}

impl fmt::Display for DecryptionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DecryptionError::MissingEncryptDictionary => write!(f, "the /Encrypt dictionary is missing"),
            DecryptionError::MissingRevision => write!(f, "missing encryption revision"),
            DecryptionError::MissingOwnerPassword => write!(f, "missing the owner password (/O)"),
            DecryptionError::MissingPermissions => write!(f, "missing the permissions field (/P)"),
            DecryptionError::InvalidKeyLength => write!(f, "unsupported key length"),
            DecryptionError::InvalidRevision => write!(f, "unsupported revision"),
            DecryptionError::InvalidType => write!(f, "unexpected type; document does not comply with the spec"),
            DecryptionError::MissingFileID => write!(f, "missing the file /ID elements"),
            DecryptionError::NotDecryptable => write!(f, "the object is not capable of being decrypted"),
            DecryptionError::IncorrectPassword => write!(f, "the supplied password is incorrect"),
            DecryptionError::UnsupportedEncryption=> write!(f, "the document uses an encryption scheme that is not supported"),
        }
    }
}

const PAD_BYTES: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41,
    0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80,
    0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

const DEFAULT_KEY_LEN: Object = Object::Integer(40);
const DEFAULT_ALGORITHM: Object = Object::Integer(0);

/// Generates the encryption key for the document and, if `check_password` is
///  true, verifies that the key is correct.
pub fn get_encryption_key<P>(doc: &Document, password: P, check_password: bool) -> Result<Vec<u8>, DecryptionError>
where
    P: AsRef<[u8]>,
{
    let password = password.as_ref();

    let encryption_dict = doc.get_encrypted()
        .map_err(|_| DecryptionError::MissingEncryptDictionary)?;

    // Very early versions of PDF assume a key length of 40 bits
    let key_len = encryption_dict.get(b"Length")
                    .unwrap_or(&DEFAULT_KEY_LEN)
                    .as_i64()
                    .map_err(|_| DecryptionError::InvalidType)?
                    as usize / 8; // Length is in bits, convert to bytes

    // MD5 produces 128bit digests, so key_len must not be greater
    if key_len > 128/8 {
        return Err(DecryptionError::InvalidKeyLength);
    }

    // Make sure we support the encryption algorithm
    let algorithm = encryption_dict.get(b"V")
                    .unwrap_or(&DEFAULT_ALGORITHM)
                    .as_i64()
                    .map_err(|_| DecryptionError::InvalidType)?;
    // Currently only support V = 1 or 2
    if !(1..=2).contains(&algorithm) {
        return Err(DecryptionError::UnsupportedEncryption);
    }

    // Revision number dictates hashing strategy
    let revision = encryption_dict.get(b"R")
                    .map_err(|_| DecryptionError::MissingRevision)?
                    .as_i64()
                    .map_err(|_| DecryptionError::InvalidType)?;
    if !(2..=3).contains(&revision) {
        return Err(DecryptionError::UnsupportedEncryption);
    }

    // Algorithm 3.2

    // 3.2.1 Start building up the key, starting with the user password plaintext,
    //  padding as needed to 32 bytes
    let mut key = Vec::with_capacity(128);
    let password_len = std::cmp::min(password.len(), 32);
    key.extend_from_slice(&password[0..password_len]);
    key.extend_from_slice(&PAD_BYTES[0..32-password_len]);

    // 3.2.3 Append hashed owner password
    let hashed_owner_password = encryption_dict.get(b"O")
        .map_err(|_| DecryptionError::MissingOwnerPassword)?
        .as_str()
        .map_err(|_| DecryptionError::InvalidType)?;
    key.extend_from_slice(hashed_owner_password);

    // 3.2.4 Append the permissions (4 bytes)
    let permissions = encryption_dict.get(b"P")
        // we don't actually care about the permissions but we need the correct
        //  value to get the correct key
        .map_err(|_| DecryptionError::MissingPermissions)?
        .as_i64()
        .map_err(|_| DecryptionError::InvalidType)? as u32;
    key.extend_from_slice(&permissions.to_le_bytes());

    // 3.2.5 Append the first element of the file identifier
    let file_id_0 = doc.trailer.get(b"ID")
        .map_err(|_| DecryptionError::MissingFileID)?
        .as_array()
        .map_err(|_| DecryptionError::InvalidType)?
        .first().ok_or(DecryptionError::InvalidType)?
        .as_str()
        .map_err(|_| DecryptionError::InvalidType)?;
    key.extend_from_slice(file_id_0);

    // 3.2.6 Revision >=4
    if revision >= 4 {
        key.extend_from_slice(&[0xFF_u8, 0xFF, 0xFF, 0xFF]);
    }

    // 3.2.7+8
    // Hash the contents of key and take the first key_len bytes
    let n_hashes = if revision < 3 { 1 } else { 51 };
    for _ in 0..n_hashes {
        let digest = md5::compute(&key);
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
        let mut ctx = md5::Context::new();
        ctx.consume(PAD_BYTES);

        // 3.5.3
        ctx.consume(file_id_0);
        let hash = ctx.compute();

        // 3.5.4
        let mut encrypted_hash = encryptor.encrypt(&hash[..]);

        // 3.5.5
        let mut temp_key = Vec::new();
        temp_key.resize(key.len(), 0_u8);
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
pub fn decrypt_object<Key>(key: Key, obj_id: ObjectId, obj: &Object) -> Result<Vec<u8>, DecryptionError>
where
    Key: AsRef<[u8]>,
{
    let key = key.as_ref();
    let mut builder = Vec::<u8>::with_capacity(key.len() + 5);
    builder.extend_from_slice(key.as_ref());

    // Extend the key with the lower 3 bytes of the object number
    builder.extend_from_slice(&obj_id.0.to_le_bytes()[..3]);
    // and the lower 2 bytes of the generation number
    builder.extend_from_slice(&obj_id.1.to_le_bytes()[..2]);

    // Now construct the rc4 key
    let key_len = std::cmp::min(key.len() + 5, 16);
    let rc4_key = &md5::compute(builder)[..key_len];

    let encrypted = match obj {
        Object::String(content, _) => content,
        Object::Stream(stream) => &stream.content,
        _ => { return Err(DecryptionError::NotDecryptable); },
    };

    // Decrypt using the rc4 algorithm
    Ok(Rc4::new(rc4_key).decrypt(encrypted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rc4_works() {
        let cases = [
            // Key, Plain, Cipher
            (String::from("Key"), String::from("Plaintext"), String::from("BBF316E8D940AF0AD3")),
            (String::from("Wiki"), String::from("pedia"), String::from("1021BF0420")),
        ];

        for (key, plain, cipher) in cases {
            // Reencode cipher from a hex string to a Vec<u8>
            let cipher = cipher.as_bytes();
            let mut cipher_bytes = Vec::with_capacity(cipher.len()/2);
            for hex_pair in cipher.chunks_exact(2) {
                cipher_bytes.push(
                    u8::from_str_radix(std::str::from_utf8(hex_pair).unwrap(), 16).unwrap()
                );
            }

            let decryptor = Rc4::new(key);
            let decrypted = decryptor.decrypt(&cipher_bytes);
            assert_eq!(plain.as_bytes(), &decrypted[..]);
        }
    }
}
