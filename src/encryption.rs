mod algorithms;
pub mod crypt_filters;
mod pkcs5;
mod rc4;

use crate::{Object, ObjectId};
use crypt_filters::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;

pub use algorithms::PasswordAlgorithm;

#[derive(Error, Debug)]
pub enum DecryptionError {
    #[error("the /Encrypt dictionary is missing")]
    MissingEncryptDictionary,
    #[error("missing encryption revision")]
    MissingRevision,
    #[error("missing the owner password (/O)")]
    MissingOwnerPassword,
    #[error("missing the user password (/U)")]
    MissingUserPassword,
    #[error("missing the permissions field (/P)")]
    MissingPermissions,
    #[error("missing the file /ID elements")]
    MissingFileID,
    #[error("missing the key length (/Length)")]
    MissingKeyLength,

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
    #[error("the encryption revision is not implemented in lopdf")]
    UnsupportedRevision,
}

#[derive(Clone, Debug)]
pub struct EncryptionState {
    pub crypt_filters: BTreeMap<Vec<u8>, Arc<dyn CryptFilter>>,
    pub key: Vec<u8>,
    pub stream_filter: Arc<dyn CryptFilter>,
    pub string_filter: Arc<dyn CryptFilter>,
}

/// Encrypts `obj`.
pub fn encrypt_object(state: &EncryptionState, obj_id: ObjectId, obj: &mut Object) -> Result<(), DecryptionError> {
    // The cross-reference stream shall not be encrypted and strings appearing in the
    // cross-reference stream dictionary shall not be encrypted.
    let is_xref_stream = obj.as_stream()
        .map(|stream| stream.dict.has_type(b"XRef"))
        .unwrap_or(false);

    if is_xref_stream {
        return Ok(());
    }

    // A stream filter type, the Crypt filter can be specified for any stream in the document to
    // override the default filter for streams. The stream's DecodeParms entry shall contain a
    // Crypt filter decode parameters dictionary whose Name entry specifies the particular crypt
    // filter that shell be used (if missing, Identity is used).
    let override_crypt_filter = obj.as_stream().ok()
        .filter(|stream| stream.filters().map(|filters| filters.contains(&&b"Crypt"[..])).unwrap_or(false))
        .and_then(|stream| stream.dict.get(b"DecodeParms").ok())
        .and_then(|object| object.as_dict().ok())
        .map(|dict| dict.get(b"Name")
            .and_then(|object| object.as_name())
            .ok()
            .and_then(|name| state.crypt_filters.get(name).cloned())
            .unwrap_or(Arc::new(IdentityCryptFilter))
        );

    // Retrieve the plaintext and the crypt filter to use to decrypt the ciphertext from the given
    // object.
    let (mut crypt_filter, plaintext) = match obj {
        // Encryption applies to all strings and streams in the document's PDF file, i.e., we have to
        // recursively process array and dictionary objects to decrypt any string and stream objects
        // stored inside of those.
        Object::Array(objects) => {
            for obj in objects {
                encrypt_object(state, obj_id, obj)?;
            }

            return Ok(());
        }
        Object::Dictionary(objects) => {
            for (_, obj) in objects.iter_mut() {
                encrypt_object(state, obj_id, obj)?;
            }

            return Ok(());
        }
        // Encryption applies to all strings and streams in the document's PDF file. We return the
        // crypt filter and the content here.
        Object::String(content, _) => (state.string_filter.clone(), &*content),
        Object::Stream(stream) => (state.stream_filter.clone(), &stream.content),
        // Encryption is not applied to other object types such as integers and boolean values.
        _ => {
            return Ok(());
        }
    };

    // If the stream object specifies its own crypt filter, override the default one with the one
    // from this stream object.
    if let Some(filter) = override_crypt_filter {
        crypt_filter = filter;
    }

    // Compute the key from the original file encryption key and the object identifier to use for
    // the corresponding object.
    let key = crypt_filter.compute_key(&state.key, obj_id)?;

    // Encrypt the plaintext.
    let ciphertext = crypt_filter.encrypt(&key, plaintext)?;

    // Store the ciphertext in the object.
    match obj {
        Object::Stream(stream) => stream.set_content(ciphertext),
        Object::String(content, _) => *content = ciphertext,
        _ => (),
    }

    Ok(())
}

/// Decrypts `obj`.
pub fn decrypt_object(state: &EncryptionState, obj_id: ObjectId, obj: &mut Object) -> Result<(), DecryptionError> {
    // The cross-reference stream shall not be encrypted and strings appearing in the
    // cross-reference stream dictionary shall not be encrypted.
    let is_xref_stream = obj.as_stream()
        .map(|stream| stream.dict.has_type(b"XRef"))
        .unwrap_or(false);

    if is_xref_stream {
        return Ok(());
    }

    // A stream filter type, the Crypt filter can be specified for any stream in the document to
    // override the default filter for streams. The stream's DecodeParms entry shall contain a
    // Crypt filter decode parameters dictionary whose Name entry specifies the particular crypt
    // filter that shell be used (if missing, Identity is used).
    let override_crypt_filter = obj.as_stream().ok()
        .filter(|stream| stream.filters().map(|filters| filters.contains(&&b"Crypt"[..])).unwrap_or(false))
        .and_then(|stream| stream.dict.get(b"DecodeParms").ok())
        .and_then(|object| object.as_dict().ok())
        .map(|dict| dict.get(b"Name")
            .and_then(|object| object.as_name())
            .ok()
            .and_then(|name| state.crypt_filters.get(name).cloned())
            .unwrap_or(Arc::new(IdentityCryptFilter))
        );

    // Retrieve the ciphertext and the crypt filter to use to decrypt the ciphertext from the given
    // object.
    let (mut crypt_filter, ciphertext) = match obj {
        // Encryption applies to all strings and streams in the document's PDF file, i.e., we have to
        // recursively process array and dictionary objects to decrypt any string and stream objects
        // stored inside of those.
        Object::Array(objects) => {
            for obj in objects {
                decrypt_object(state, obj_id, obj)?;
            }

            return Ok(());
        }
        Object::Dictionary(objects) => {
            for (_, obj) in objects.iter_mut() {
                decrypt_object(state, obj_id, obj)?;
            }

            return Ok(());
        }
        // Encryption applies to all strings and streams in the document's PDF file. We return the
        // crypt filter and the content here.
        Object::String(content, _) => (state.string_filter.clone(), &*content),
        Object::Stream(stream) => (state.stream_filter.clone(), &stream.content),
        // Encryption is not applied to other object types such as integers and boolean values.
        _ => {
            return Ok(());
        }
    };

    // If the stream object specifies its own crypt filter, override the default one with the one
    // from this stream object.
    if let Some(filter) = override_crypt_filter {
        crypt_filter = filter;
    }

    // Compute the key from the original file encryption key and the object identifier to use for
    // the corresponding object.
    let key = crypt_filter.compute_key(&state.key, obj_id)?;

    // Decrypt the ciphertext.
    let plaintext = crypt_filter.decrypt(&key, ciphertext)?;

    // Store the plaintext in the object.
    match obj {
        Object::Stream(stream) => stream.set_content(plaintext),
        Object::String(content, _) => *content = plaintext,
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
