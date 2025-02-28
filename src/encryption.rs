mod algorithms;
pub mod crypt_filters;
mod pkcs5;
mod rc4;

use bitflags::bitflags;
use crate::{Document, Error, Object, ObjectId};
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

    #[error(transparent)]
    StringPrep(#[from] stringprep::Error),
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
    pub struct Permissions: u64 {
        /// (Security handlers of revision 2) Print the document.
        /// (Security handlers of revision 3 or greater) Print the document (possibly not at the
        /// highest quality level, depending on whether [`Permissions::PRINTABLE_IN_HIGH_QUALITY`]
        /// is also set).
        const PRINTABLE = 1 << 3;

        /// Modify the contents of the document by operations other than those controlled by
        /// [`Permissions::ANNOTABLE`], [`Permissions::FILLABLE`] and [`Permissions::ASSEMBLABLE`].
        const MODIFIABLE = 1 << 4;

        /// Copy or otherwise extract text and graphics from the document. However, for the limited
        /// purpose of providing this content to assistive technology, a PDF reader should behave
        /// as if this bit was set to 1.
        const COPYABLE = 1 << 5;

        /// Add or modify text annotations, fill in interactive form fields, and if
        /// [`Permissions::MODIFIABLE`] is also set, create or modify interactive form fields
        /// (including signature fields).
        const ANNOTABLE = 1 << 6;

        /// Fill in existing interactive fields (including signature fields), even if
        /// [`Permissions::ANNOTABLE`] is clear.
        const FILLABLE = 1 << 9;

        /// Copy or otherwise extract text and graphics from the document for the purpose of
        /// providing this content to assistive technology.
        ///
        /// Deprecated since PDF 2.0: must always be set for backward compatibility with PDF
        /// viewers following earlier specifications.
        const COPYABLE_FOR_ACCESSIBILITY = 1 << 10;

        /// (Security handlers of revision 3 or greater) Assemble the document (insert, rotate, or
        /// delete pages and create document outline items or thumbnail images), even if
        /// [`Permissions::MODIFIABLE`] is not set.
        const ASSEMBLABLE = 1 << 11;

        /// (Security handlers of revision 3 or greater) Print the document to a representation
        /// from which a faithful copy of the PDF content could be generated, based on an
        /// implementation-dependent algorithm. When this bit is clear (and
        /// [`Permissions::PRINTABLE`] is set), printing shall be limited to a low-level
        /// representation of the appearance, possibly of degraded quality.
        const PRINTABLE_IN_HIGH_QUALITY = 1 << 12;
    }
}

impl Permissions {
    pub fn p_value(&self) -> u64 {
        self.bits() |
        // 7-8: Reserved. Must be 1.
        (0b11 << 7) |
        // 13-32: Reserved. Must be 1.
        (0b111 << 13) | (0xffff << 16) |
        // Extend the permissions (contents of the P integer) to 64 bits by setting the upper 32
        // bits to all 1s.
        (0xffffffff << 32)
    }
}

#[derive(Clone, Debug)]
pub struct EncryptionState {
    pub crypt_filters: BTreeMap<Vec<u8>, Arc<dyn CryptFilter>>,
    pub file_encryption_key: Vec<u8>,
    pub stream_filter: Vec<u8>,
    pub string_filter: Vec<u8>,
    pub owner_value: Vec<u8>,
    pub owner_encrypted: Vec<u8>,
    pub user_value: Vec<u8>,
    pub user_encrypted: Vec<u8>,
    pub permissions: Permissions,
}

impl EncryptionState {
    pub fn decode<P>(
        document: &Document,
        password: P,
    ) -> Result<Self, Error>
    where
        P: AsRef<[u8]>,
    {
        if !document.is_encrypted() {
            return Err(Error::NotEncrypted);
        }

        // The name of the preferred security handler for this document. It shall be the name of
        // the security handler that was used to encrypt the document.
        //
        // Standard shall be the name of the built-in password-based security handler.
        let filter = document.get_encrypted()
            .and_then(|dict| dict.get(b"Filter"))
            .and_then(|object| object.as_name())
            .map_err(|_| Error::DictKey("Filter".to_string()))?;

        if filter != b"Standard" {
            return Err(Error::UnsupportedSecurityHandler(filter.to_vec()));
        }

        let algorithm = PasswordAlgorithm::try_from(document)?;
        let file_encryption_key = algorithm.compute_file_encryption_key(document, password)?;

        // Get the owner value and owner encrypted blobs.
        let owner_value = document.get_encrypted()
            .and_then(|dict| dict.get(b"O"))
            .map_err(|_| DecryptionError::MissingOwnerPassword)?
            .as_str()
            .map_err(|_| DecryptionError::InvalidType)?
            .to_vec();

        let owner_encrypted = document.get_encrypted()
            .and_then(|dict| dict.get(b"OE"))
            .and_then(Object::as_str)
            .map(|s| s.to_vec())
            .ok()
            .unwrap_or_default();

        // Get the user value and user encrypted blobs.
        let user_value = document.get_encrypted()
            .and_then(|dict| dict.get(b"U"))
            .map_err(|_| DecryptionError::MissingUserPassword)?
            .as_str()
            .map_err(|_| DecryptionError::InvalidType)?
            .to_vec();

        let user_encrypted = document.get_encrypted()
            .and_then(|dict| dict.get(b"UE"))
            .and_then(Object::as_str)
            .map(|s| s.to_vec())
            .ok()
            .unwrap_or_default();

        // Get the permission value.
        let permission_value = document.get_encrypted()
            .and_then(|dict| dict.get(b"P"))
            .map_err(|_| DecryptionError::MissingPermissions)?
            .as_i64()
            .map_err(|_| DecryptionError::InvalidType)?
            as u64;

        let permissions = Permissions::from_bits_truncate(permission_value);

        let crypt_filters = document.get_crypt_filters();

        let mut state = Self {
            crypt_filters,
            file_encryption_key,
            stream_filter: vec![],
            string_filter: vec![],
            owner_value,
            owner_encrypted,
            user_value,
            user_encrypted,
            permissions,
        };

        if let Ok(stream_filter) = document.get_encrypted()
            .and_then(|dict| dict.get(b"StmF"))
            .and_then(|object| object.as_name()) {
            state.stream_filter = stream_filter.to_vec();
        }

        if let Ok(string_filter) = document.get_encrypted()
            .and_then(|dict| dict.get(b"StrF"))
            .and_then(|object| object.as_name()) {
            state.string_filter = string_filter.to_vec();
        }

        Ok(state)
    }
}

impl EncryptionState {
    pub fn get_stream_filter(&self) -> Arc<dyn CryptFilter> {
        self.crypt_filters.get(&self.stream_filter).cloned().unwrap_or(Arc::new(Rc4CryptFilter))
    }

    pub fn get_string_filter(&self) -> Arc<dyn CryptFilter> {
        self.crypt_filters.get(&self.string_filter).cloned().unwrap_or(Arc::new(Rc4CryptFilter))
    }
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
        Object::String(content, _) => (state.get_string_filter(), &*content),
        Object::Stream(stream) => (state.get_stream_filter(), &stream.content),
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
    let key = crypt_filter.compute_key(&state.file_encryption_key, obj_id)?;

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
        Object::String(content, _) => (state.get_string_filter(), &*content),
        Object::Stream(stream) => (state.get_stream_filter(), &stream.content),
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
    let key = crypt_filter.compute_key(&state.file_encryption_key, obj_id)?;

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
    use super::rc4::Rc4;

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
