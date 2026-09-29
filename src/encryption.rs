mod algorithms;
pub mod crypt_filters;
mod pkcs5;
mod rc4;

use crate::{Dictionary, Document, Error, Object, ObjectId};
use bitflags::bitflags;
use crypt_filters::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;

pub use algorithms::PasswordAlgorithm;

#[derive(Error, Debug)]
pub enum DecryptionError {
    #[error("the /Encrypt dictionary is missing")]
    MissingEncryptDictionary,
    #[error("missing encryption version")]
    MissingVersion,
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

    #[error("invalid hash length")]
    InvalidHashLength,
    #[error("invalid key length")]
    InvalidKeyLength,
    #[error("invalid ciphertext length")]
    InvalidCipherTextLength,
    #[error("invalid permission length")]
    InvalidPermissionLength,
    #[error("invalid version")]
    InvalidVersion,
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
    #[error("the encryption version is not implemented in lopdf")]
    UnsupportedVersion,
    #[error("the encryption revision is not implemented in lopdf")]
    UnsupportedRevision,

    #[error(transparent)]
    StringPrep(#[from] stringprep::Error),
    #[error("invalid padding encountered when decrypting, key might be incorrect")]
    Padding,
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
    pub struct Permissions: u64 {
        /// (Security handlers of revision 2) Print the document.
        /// (Security handlers of revision 3 or greater) Print the document (possibly not at the
        /// highest quality level, depending on whether [`Permissions::PRINTABLE_IN_HIGH_QUALITY`]
        /// is also set).
        const PRINTABLE = 1 << 2;

        /// Modify the contents of the document by operations other than those controlled by
        /// [`Permissions::ANNOTABLE`], [`Permissions::FILLABLE`] and [`Permissions::ASSEMBLABLE`].
        const MODIFIABLE = 1 << 3;

        /// Copy or otherwise extract text and graphics from the document. However, for the limited
        /// purpose of providing this content to assistive technology, a PDF reader should behave
        /// as if this bit was set to 1.
        const COPYABLE = 1 << 4;

        /// Add or modify text annotations, fill in interactive form fields, and if
        /// [`Permissions::MODIFIABLE`] is also set, create or modify interactive form fields
        /// (including signature fields).
        const ANNOTABLE = 1 << 5;

        /// Fill in existing interactive fields (including signature fields), even if
        /// [`Permissions::ANNOTABLE`] is clear.
        const FILLABLE = 1 << 8;

        /// Copy or otherwise extract text and graphics from the document for the purpose of
        /// providing this content to assistive technology.
        ///
        /// Deprecated since PDF 2.0: must always be set for backward compatibility with PDF
        /// viewers following earlier specifications.
        const COPYABLE_FOR_ACCESSIBILITY = 1 << 9;

        /// (Security handlers of revision 3 or greater) Assemble the document (insert, rotate, or
        /// delete pages and create document outline items or thumbnail images), even if
        /// [`Permissions::MODIFIABLE`] is not set.
        const ASSEMBLABLE = 1 << 10;

        /// (Security handlers of revision 3 or greater) Print the document to a representation
        /// from which a faithful copy of the PDF content could be generated, based on an
        /// implementation-dependent algorithm. When this bit is clear (and
        /// [`Permissions::PRINTABLE`] is set), printing shall be limited to a low-level
        /// representation of the appearance, possibly of degraded quality.
        const PRINTABLE_IN_HIGH_QUALITY = 1 << 11;
    }
}

impl Default for Permissions {
    fn default() -> Self {
        Self::all()
    }
}

impl Permissions {
    fn correct_bits(self) -> Self {
        let mut bits = self.bits();

        // 7-8: Reserved. Must be 1.
        bits |= 0b11 << 6;

        // 13-32: Reserved. Must be 1.
        bits |= 0b1111 << 12 | 0xffff << 16;

        // Extend the permissions (contents of the P integer) to 64 bits by setting the upper 32
        // bits to all 1s.
        bits |= 0xffffffff << 32;

        Permissions::from_bits_retain(bits)
    }
}

#[derive(Clone, Debug)]
pub enum EncryptionVersion<'a> {
    /// (PDF 1.4; deprecated in PDF 2.0) Indicates the use of encryption of data using the RC4 or
    /// AES algorithms with a file encryption key length of 40 bits.
    V1 {
        document: &'a Document,
        owner_password: &'a str,
        user_password: &'a str,
        permissions: Permissions,
    },
    /// (PDF 1.4; deprecated in PDF 2.0) Indicates the use of encryption of data using the RC4 or
    /// AES algorithms but permitting file encryption key lengths greater or 40 bits.
    V2 {
        document: &'a Document,
        owner_password: &'a str,
        user_password: &'a str,
        key_length: usize,
        permissions: Permissions,
    },
    /// (PDF 1.5; deprecated in PDF 2.0) The security handler defines the use of encryption and
    /// decryption in the document, using the rules specified by the CF, StmF and StrF entries
    /// using encryption of data using the RC4 or AES algorithms (deprecated in PDF  2.0) with a
    /// file encryption key length of 128 bits.
    V4 {
        document: &'a Document,
        encrypt_metadata: bool,
        crypt_filters: BTreeMap<Vec<u8>, Arc<dyn CryptFilter>>,
        stream_filter: Vec<u8>,
        string_filter: Vec<u8>,
        owner_password: &'a str,
        user_password: &'a str,
        permissions: Permissions,
    },
    /// (PDF 2.0; deprecated in PDF 2.0) Shall not be used. This value was used by a deprecated
    /// proprietary Adobe extension.
    ///
    /// This exists for testing purposes to guarantee improved compatibility.
    #[deprecated(
        note = "R5 is a proprietary Adobe extension and should not be used in newly produced documents other than for testing purposes."
    )]
    R5 {
        encrypt_metadata: bool,
        crypt_filters: BTreeMap<Vec<u8>, Arc<dyn CryptFilter>>,
        file_encryption_key: &'a [u8],
        stream_filter: Vec<u8>,
        string_filter: Vec<u8>,
        owner_password: &'a str,
        user_password: &'a str,
        permissions: Permissions,
    },
    /// (PDF 2.0) The security handler defines the use of encryption and decryption in the
    /// document, using the rules specified by the CF, StmF, StrF and EFF entries using encryption
    /// of data using the AES algorithms with a file encryption key length of 256 bits.
    V5 {
        encrypt_metadata: bool,
        crypt_filters: BTreeMap<Vec<u8>, Arc<dyn CryptFilter>>,
        file_encryption_key: &'a [u8],
        stream_filter: Vec<u8>,
        string_filter: Vec<u8>,
        owner_password: &'a str,
        user_password: &'a str,
        permissions: Permissions,
    },
}

#[derive(Clone, Debug, Default)]
pub struct EncryptionState {
    pub(crate) version: i64,
    pub(crate) revision: i64,
    pub(crate) key_length: Option<usize>,
    pub(crate) encrypt_metadata: bool,
    pub(crate) crypt_filters: BTreeMap<Vec<u8>, Arc<dyn CryptFilter>>,
    pub(crate) file_encryption_key: Vec<u8>,
    pub(crate) stream_filter: Vec<u8>,
    pub(crate) string_filter: Vec<u8>,
    pub(crate) owner_value: Vec<u8>,
    pub(crate) owner_encrypted: Vec<u8>,
    pub(crate) user_value: Vec<u8>,
    pub(crate) user_encrypted: Vec<u8>,
    pub(crate) permissions: Permissions,
    pub(crate) permission_encrypted: Vec<u8>,
    /// Object id of the `/Encrypt` dictionary in the previous revision, recorded
    /// during `Document::decrypt_raw` before the reference is stripped from the
    /// trailer. Used by `IncrementalDocument` to restore the `/Encrypt` reference
    /// in the appended trailer.
    pub(crate) encrypt_object_id: Option<ObjectId>,
}

/// Fields shared by the AES-256 variants (`R5` and `V5`), which differ only
/// in the revision they record.
struct Aes256Params<'a> {
    encrypt_metadata: bool,
    crypt_filters: BTreeMap<Vec<u8>, Arc<dyn CryptFilter>>,
    file_encryption_key: &'a [u8],
    stream_filter: Vec<u8>,
    string_filter: Vec<u8>,
    owner_password: &'a str,
    user_password: &'a str,
    permissions: Permissions,
}

impl TryFrom<EncryptionVersion<'_>> for EncryptionState {
    type Error = Error;

    fn try_from(version: EncryptionVersion) -> Result<EncryptionState, Self::Error> {
        Ok(match version {
            EncryptionVersion::V1 {
                document,
                owner_password,
                user_password,
                permissions,
            } => r4_state(
                R4Params {
                    document,
                    owner_password,
                    user_password,
                    permissions,
                    key_length: None,
                    version: 1,
                    revision: 2,
                    user_hash: UserHash::R2,
                    encrypt_metadata: true,
                },
                None,
            )?,
            EncryptionVersion::V2 {
                document,
                owner_password,
                user_password,
                key_length,
                permissions,
            } => r4_state(
                R4Params {
                    document,
                    owner_password,
                    user_password,
                    permissions,
                    key_length: Some(key_length),
                    version: 2,
                    revision: 3,
                    user_hash: UserHash::R3R4,
                    encrypt_metadata: true,
                },
                None,
            )?,
            EncryptionVersion::V4 {
                document,
                encrypt_metadata,
                crypt_filters,
                stream_filter,
                string_filter,
                owner_password,
                user_password,
                permissions,
            } => r4_state(
                R4Params {
                    document,
                    owner_password,
                    user_password,
                    permissions,
                    key_length: Some(128),
                    version: 4,
                    revision: 4,
                    user_hash: UserHash::R3R4,
                    encrypt_metadata,
                },
                Some((crypt_filters, stream_filter, string_filter)),
            )?,
            #[allow(deprecated)]
            EncryptionVersion::R5 {
                encrypt_metadata,
                crypt_filters,
                file_encryption_key,
                stream_filter,
                string_filter,
                owner_password,
                user_password,
                permissions,
            } => aes256_state(
                5,
                Aes256Params {
                    encrypt_metadata,
                    crypt_filters,
                    file_encryption_key,
                    stream_filter,
                    string_filter,
                    owner_password,
                    user_password,
                    permissions,
                },
            )?,
            EncryptionVersion::V5 {
                encrypt_metadata,
                crypt_filters,
                file_encryption_key,
                stream_filter,
                string_filter,
                owner_password,
                user_password,
                permissions,
            } => aes256_state(
                6,
                Aes256Params {
                    encrypt_metadata,
                    crypt_filters,
                    file_encryption_key,
                    stream_filter,
                    string_filter,
                    owner_password,
                    user_password,
                    permissions,
                },
            )?,
        })
    }
}

/// Which Algorithm 4 variant a revision uses to derive `/U`: revision 2 hashes
/// the padded password on its own, revision 3 and above also mix in the file
/// identifier. Named explicitly so that adding a revision cannot silently land
/// on the wrong hash.
#[derive(Clone, Copy)]
enum UserHash {
    /// Algorithm 4: MD5 of the padded password, then the RC4 rounds of
    /// revision 2's own key derivation.
    R2,
    /// Algorithm 5: as above with the first `/ID` element mixed in.
    R3R4,
}

/// Fields shared by the MD5-based revisions (`V1`, `V2` and `V4`), which run
/// the same Algorithms 2 through 5 and differ only in version, revision, key
/// length, user hash and metadata handling.
struct R4Params<'a> {
    document: &'a Document,
    owner_password: &'a str,
    user_password: &'a str,
    permissions: Permissions,
    key_length: Option<usize>,
    version: i64,
    revision: i64,
    user_hash: UserHash,
    encrypt_metadata: bool,
}

/// The per-object crypt filter triple `V4` and later carry, and the earlier
/// revisions leave unset.
type CryptFilters = (
    BTreeMap<Vec<u8>, Arc<dyn CryptFilter>>,
    Vec<u8>, // StmF
    Vec<u8>, // StrF
);

/// Build the [`EncryptionState`] for one of the MD5-based revisions. The
/// per-object crypt filters are present only from `V4` on.
fn r4_state(params: R4Params<'_>, crypt_filters: Option<CryptFilters>) -> Result<EncryptionState, DecryptionError> {
    let R4Params {
        document,
        owner_password,
        user_password,
        permissions,
        key_length,
        version,
        revision,
        user_hash,
        encrypt_metadata,
    } = params;

    let mut algorithm = r4_algorithm(key_length, version, revision, encrypt_metadata, permissions);

    let owner_password = algorithm.sanitize_password_r4(owner_password)?;
    let user_password = algorithm.sanitize_password_r4(user_password)?;

    algorithm.owner_value = algorithm.compute_hashed_owner_password_r4(Some(&owner_password), &user_password)?;

    algorithm.user_value = match user_hash {
        UserHash::R2 => algorithm.compute_hashed_user_password_r2(document, &user_password)?,
        UserHash::R3R4 => algorithm.compute_hashed_user_password_r3_r4(document, &user_password)?,
    };

    let file_encryption_key = algorithm.compute_file_encryption_key_r4(document, &user_password)?;

    let (crypt_filters, stream_filter, string_filter) = crypt_filters.unwrap_or_default();

    Ok(EncryptionState {
        version: algorithm.version,
        revision: algorithm.revision,
        key_length: algorithm.length,
        encrypt_metadata: algorithm.encrypt_metadata,
        file_encryption_key,
        crypt_filters,
        stream_filter,
        string_filter,
        owner_value: algorithm.owner_value,
        user_value: algorithm.user_value,
        // The corrected bits, which `r4_algorithm` recorded on the algorithm.
        permissions: algorithm.permissions,
        ..Default::default()
    })
}

/// An [`PasswordAlgorithm`] seeded for one of the MD5-based revisions
/// (2 through 4), which derive the file encryption key the same way and
/// differ only in version, revision and key length. The permissions are
/// corrected here, because every revision records them in `/P`.
fn r4_algorithm(
    key_length: Option<usize>, version: i64, revision: i64, encrypt_metadata: bool, permissions: Permissions,
) -> PasswordAlgorithm {
    PasswordAlgorithm {
        encrypt_metadata,
        length: key_length,
        version,
        revision,
        permissions: permissions.correct_bits(),
        ..Default::default()
    }
}

/// An [`EncryptionState`] for the AES-256 revisions (5 and 6), which run the
/// same Algorithms 8, 9 and 10 and differ only in the revision recorded.
fn aes256_state(revision: i64, params: Aes256Params<'_>) -> Result<EncryptionState, DecryptionError> {
    let Aes256Params {
        encrypt_metadata,
        crypt_filters,
        file_encryption_key,
        stream_filter,
        string_filter,
        owner_password,
        user_password,
        permissions,
    } = params;

    if file_encryption_key.len() != 32 {
        return Err(DecryptionError::InvalidKeyLength);
    }

    let permissions = permissions.correct_bits();

    let mut algorithm = PasswordAlgorithm {
        encrypt_metadata,
        version: 5,
        revision,
        permissions,
        ..Default::default()
    };

    let owner_password = algorithm.sanitize_password_r6(owner_password)?;
    let user_password = algorithm.sanitize_password_r6(user_password)?;

    let (user_value, user_encrypted) = algorithm.compute_hashed_user_password_r6(file_encryption_key, user_password)?;

    algorithm.user_value = user_value;
    algorithm.user_encrypted = user_encrypted;

    let (owner_value, owner_encrypted) =
        algorithm.compute_hashed_owner_password_r6(file_encryption_key, owner_password)?;

    algorithm.owner_value = owner_value;
    algorithm.owner_encrypted = owner_encrypted;

    algorithm.permission_encrypted = algorithm.compute_permissions(file_encryption_key)?;

    Ok(EncryptionState {
        version: algorithm.version,
        revision: algorithm.revision,
        key_length: algorithm.length,
        encrypt_metadata: algorithm.encrypt_metadata,
        crypt_filters,
        file_encryption_key: file_encryption_key.to_vec(),
        stream_filter,
        string_filter,
        owner_value: algorithm.owner_value,
        owner_encrypted: algorithm.owner_encrypted,
        user_value: algorithm.user_value,
        user_encrypted: algorithm.user_encrypted,
        permissions: algorithm.permissions,
        permission_encrypted: algorithm.permission_encrypted,
        encrypt_object_id: None,
    })
}

impl EncryptionState {
    pub fn version(&self) -> i64 {
        self.version
    }

    pub fn revision(&self) -> i64 {
        self.revision
    }

    pub fn key_length(&self) -> Option<usize> {
        self.key_length
    }

    pub fn encrypt_metadata(&self) -> bool {
        self.encrypt_metadata
    }

    pub fn crypt_filters(&self) -> &BTreeMap<Vec<u8>, Arc<dyn CryptFilter>> {
        &self.crypt_filters
    }

    pub fn file_encryption_key(&self) -> &[u8] {
        self.file_encryption_key.as_ref()
    }

    pub fn default_stream_filter(&self) -> &[u8] {
        self.stream_filter.as_ref()
    }

    pub fn default_string_filter(&self) -> &[u8] {
        self.string_filter.as_ref()
    }

    pub fn owner_value(&self) -> &[u8] {
        self.owner_value.as_ref()
    }

    pub fn owner_encrypted(&self) -> &[u8] {
        self.owner_encrypted.as_ref()
    }

    pub fn user_value(&self) -> &[u8] {
        self.user_value.as_ref()
    }

    pub fn user_encrypted(&self) -> &[u8] {
        self.user_encrypted.as_ref()
    }

    pub fn permissions(&self) -> Permissions {
        self.permissions
    }

    pub fn permission_encrypted(&self) -> &[u8] {
        self.permission_encrypted.as_ref()
    }

    /// Object id of the original `/Encrypt` dictionary, when known. This is
    /// recorded during `Document::decrypt_raw`; the dictionary bytes themselves
    /// are left intact in the previous revision so that an incremental save can
    /// point back at them via `/Encrypt N G R` without re-emitting the dictionary.
    pub fn encrypt_object_id(&self) -> Option<ObjectId> {
        self.encrypt_object_id
    }

    pub fn decode<P>(document: &Document, password: P) -> Result<Self, Error>
    where
        P: AsRef<[u8]>,
    {
        if !document.is_encrypted() {
            return Err(Error::NotEncrypted);
        }

        // `/Filter` names the security handler; only the built-in password-based
        // "Standard" handler is supported.
        let filter = document
            .get_encrypted()
            .and_then(|dict| dict.get(b"Filter"))
            .and_then(|object| object.as_name())
            .map_err(|_| Error::DictKey("Filter".to_string()))?;

        if filter != b"Standard" {
            return Err(Error::UnsupportedSecurityHandler(filter.to_vec()));
        }

        let algorithm = PasswordAlgorithm::try_from(document)?;
        // Either password may be given, but only the user password (or, for an owner password,
        // the value Algorithm 7 recovers from `/O`) is valid input to Algorithm 2.
        // See `PasswordAlgorithm::resolve_password_for_key_derivation`.
        let resolved_password = algorithm.resolve_password_for_key_derivation(document, password)?;
        let file_encryption_key = algorithm.compute_file_encryption_key(document, resolved_password)?;

        let mut crypt_filters = document.get_crypt_filters();

        // CF is meaningful only when the value of V is 4 (PDF 1.5) or 5 (PDF 2.0).
        if algorithm.version < 4 {
            crypt_filters.clear();
        }

        let mut state = Self {
            version: algorithm.version,
            revision: algorithm.revision,
            key_length: algorithm.length,
            encrypt_metadata: algorithm.encrypt_metadata,
            crypt_filters,
            file_encryption_key,
            owner_value: algorithm.owner_value,
            owner_encrypted: algorithm.owner_encrypted,
            user_value: algorithm.user_value,
            user_encrypted: algorithm.user_encrypted,
            permissions: algorithm.permissions,
            permission_encrypted: algorithm.permission_encrypted,
            ..Default::default()
        };

        // StmF and StrF are meaningful only when the value of V is 4 (PDF 1.5) or 5 (PDF 2.0).
        if algorithm.version == 4 || algorithm.version == 5 {
            if let Ok(stream_filter) = document
                .get_encrypted()
                .and_then(|dict| dict.get(b"StmF"))
                .and_then(|object| object.as_name())
            {
                state.stream_filter = stream_filter.to_vec();
            }

            if let Ok(string_filter) = document
                .get_encrypted()
                .and_then(|dict| dict.get(b"StrF"))
                .and_then(|object| object.as_name())
            {
                state.string_filter = string_filter.to_vec();
            }
        }

        Ok(state)
    }

    pub fn encode(&self) -> Result<Dictionary, DecryptionError> {
        let mut encrypted = Dictionary::new();

        encrypted.set(b"Filter", Object::Name(b"Standard".to_vec()));

        encrypted.set(b"V", Object::Integer(self.version));
        encrypted.set(b"R", Object::Integer(self.revision));

        if let Some(key_length) = self.key_length {
            encrypted.set(b"Length", Object::Integer(key_length as i64));
        }

        // Optional; meaningful only when the value of V is 4 (PDF 1.5) or 5 (PDF 2.0)). Indicates
        // whether the document-level metadata stream shall be encrypted. Default value: true.
        if self.version >= 4 {
            encrypted.set(b"EncryptMetadata", Object::Boolean(self.encrypt_metadata));
        }

        encrypted.set(b"O", Object::string_literal(self.owner_value.clone()));
        encrypted.set(b"U", Object::string_literal(self.user_value.clone()));
        encrypted.set(b"P", Object::Integer(self.permissions.bits() as i64));

        if self.revision >= 4 {
            let mut filters = Dictionary::new();

            for (name, crypt_filter) in &self.crypt_filters {
                let mut filter = Dictionary::new();

                filter.set(b"Type", Object::Name(b"CryptFilter".to_vec()));
                filter.set(b"CFM", Object::Name(crypt_filter.method().to_vec()));

                filters.set(name.to_vec(), Object::Dictionary(filter));
            }

            encrypted.set(b"CF", Object::Dictionary(filters));
            encrypted.set(b"StmF", Object::Name(self.stream_filter.clone()));
            encrypted.set(b"StrF", Object::Name(self.string_filter.clone()));
        }

        if self.revision >= 5 {
            encrypted.set(b"OE", Object::string_literal(self.owner_encrypted.clone()));
            encrypted.set(b"UE", Object::string_literal(self.user_encrypted.clone()));
            encrypted.set(b"Perms", Object::string_literal(self.permission_encrypted.clone()));
        }

        Ok(encrypted)
    }

    pub fn get_stream_filter(&self) -> Arc<dyn CryptFilter> {
        self.crypt_filters
            .get(&self.stream_filter)
            .cloned()
            .unwrap_or(Arc::new(Rc4CryptFilter))
    }

    pub fn get_string_filter(&self) -> Arc<dyn CryptFilter> {
        self.crypt_filters
            .get(&self.string_filter)
            .cloned()
            .unwrap_or(Arc::new(Rc4CryptFilter))
    }
}

/// Which way a crypt filter is applied to an object's payload.
#[derive(Clone, Copy)]
enum Direction {
    Encrypt,
    Decrypt,
}

/// Applies the document's crypt filter to every string and stream in `obj`,
/// recursing through arrays and dictionaries to reach the ones nested inside.
///
/// Encryption and decryption are the same walk over the object with the
/// filter run forwards or backwards, so they share this traversal; the spec
/// rules about which objects are exempt are identical in both directions
/// (ISO 32000-1, 7.6). A direction that ever needs a different exemption
/// belongs in its own wrapper rather than in a test of `direction` here.
fn transform_object(
    state: &EncryptionState, obj_id: ObjectId, obj: &mut Object, direction: Direction,
) -> Result<(), DecryptionError> {
    // The cross-reference stream shall not be encrypted and strings appearing in the
    // cross-reference stream dictionary shall not be encrypted.
    let is_xref_stream = obj
        .as_stream()
        .map(|stream| stream.dict.has_type(b"XRef"))
        .unwrap_or(false);

    if is_xref_stream {
        return Ok(());
    }

    // The Metadata stream shall only be encrypted if EncryptMetadata is set to true.
    if obj.type_name().ok() == Some(b"Metadata") && !state.encrypt_metadata {
        return Ok(());
    }

    // A stream's DecodeParms /Crypt /Name may override the document's default crypt filter;
    // Identity is used when absent.
    let override_crypt_filter = obj
        .as_stream()
        .ok()
        .filter(|stream| {
            stream
                .filters()
                .map(|filters| filters.contains(&&b"Crypt"[..]))
                .unwrap_or(false)
        })
        .and_then(|stream| stream.dict.get(b"DecodeParms").ok())
        .and_then(|object| object.as_dict().ok())
        .map(|dict| {
            dict.get(b"Name")
                .and_then(|object| object.as_name())
                .ok()
                .and_then(|name| state.crypt_filters.get(name).cloned())
                .unwrap_or(Arc::new(IdentityCryptFilter))
        });

    // Retrieve the payload and the crypt filter to use to transform it.
    let (mut crypt_filter, payload) = match obj {
        // Every string and stream is covered, so recurse into arrays and dictionaries
        // to reach the ones nested inside.
        Object::Array(objects) => {
            for obj in objects {
                transform_object(state, obj_id, obj, direction)?;
            }

            return Ok(());
        }
        Object::Dictionary(objects) => {
            for (_, obj) in objects.iter_mut() {
                transform_object(state, obj_id, obj, direction)?;
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

    let transformed = match direction {
        Direction::Encrypt => crypt_filter.encrypt(&key, payload)?,
        Direction::Decrypt => crypt_filter.decrypt(&key, payload)?,
    };

    // Store the result in the object.
    match obj {
        Object::Stream(stream) => stream.set_content(transformed),
        Object::String(content, _) => *content = transformed,
        _ => (),
    }

    Ok(())
}

/// Encrypts `obj`.
pub fn encrypt_object(state: &EncryptionState, obj_id: ObjectId, obj: &mut Object) -> Result<(), DecryptionError> {
    transform_object(state, obj_id, obj, Direction::Encrypt)
}

/// Decrypts `obj`.
pub fn decrypt_object(state: &EncryptionState, obj_id: ObjectId, obj: &mut Object) -> Result<(), DecryptionError> {
    transform_object(state, obj_id, obj, Direction::Decrypt)
}

#[cfg(test)]
mod tests {
    use super::rc4::Rc4;
    use crate::creator::tests::create_document;
    use crate::encryption::{Aes128CryptFilter, Aes256CryptFilter, CryptFilter};
    use crate::{EncryptionState, EncryptionVersion, Permissions};
    use rand::RngExt as _;
    use std::collections::BTreeMap;
    use std::sync::Arc;

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
            for hex_pair in cipher.as_chunks::<2>().0 {
                cipher_bytes.push(u8::from_str_radix(std::str::from_utf8(hex_pair).unwrap(), 16).unwrap());
            }

            let decryptor = Rc4::new(key);
            let decrypted = decryptor.decrypt(&cipher_bytes);
            assert_eq!(plain.as_bytes(), &decrypted[..]);
        }
    }

    /// Every revision of the standard security handler must round-trip:
    /// encrypting a document and decrypting it again with the user password
    /// both succeed.
    #[test]
    fn encrypt_and_decrypt_every_revision() {
        let document = create_document();
        let crypt_filters = |filter: Arc<dyn CryptFilter>| BTreeMap::from([(b"StdCF".to_vec(), filter)]);

        let mut file_encryption_key = [0u8; 32];
        rand::rng().fill(&mut file_encryption_key);

        #[allow(deprecated)]
        let versions = [
            EncryptionVersion::V1 {
                document: &document,
                owner_password: "owner",
                user_password: "user",
                permissions: Permissions::all(),
            },
            EncryptionVersion::V2 {
                document: &document,
                owner_password: "owner",
                user_password: "user",
                key_length: 40,
                permissions: Permissions::all(),
            },
            EncryptionVersion::V4 {
                document: &document,
                encrypt_metadata: true,
                crypt_filters: crypt_filters(Arc::new(Aes128CryptFilter)),
                stream_filter: b"StdCF".to_vec(),
                string_filter: b"StdCF".to_vec(),
                owner_password: "owner",
                user_password: "user",
                permissions: Permissions::all(),
            },
            EncryptionVersion::R5 {
                encrypt_metadata: true,
                crypt_filters: crypt_filters(Arc::new(Aes256CryptFilter)),
                file_encryption_key: &file_encryption_key,
                stream_filter: b"StdCF".to_vec(),
                string_filter: b"StdCF".to_vec(),
                owner_password: "owner",
                user_password: "user",
                permissions: Permissions::all(),
            },
            EncryptionVersion::V5 {
                encrypt_metadata: true,
                crypt_filters: crypt_filters(Arc::new(Aes256CryptFilter)),
                file_encryption_key: &file_encryption_key,
                stream_filter: b"StdCF".to_vec(),
                string_filter: b"StdCF".to_vec(),
                owner_password: "owner",
                user_password: "user",
                permissions: Permissions::all(),
            },
        ];

        for version in versions {
            let mut document = document.clone();
            let state = EncryptionState::try_from(version).unwrap();
            assert!(document.encrypt(&state).is_ok());
            assert!(document.decrypt("user").is_ok());
        }
    }

    #[test]
    fn decrypt_v4_without_length() {
        let mut document = create_document();

        let crypt_filter: Arc<dyn CryptFilter> = Arc::new(Aes128CryptFilter);

        let version = EncryptionVersion::V4 {
            document: &document,
            encrypt_metadata: true,
            crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), crypt_filter)]),
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password: "owner",
            user_password: "user",
            permissions: Permissions::all(),
        };

        let state = EncryptionState::try_from(version).unwrap();
        document.encrypt(&state).unwrap();

        // Length is optional for V4, which fixes the file encryption key at 128
        // bits, and plenty of producers leave the entry out.
        let encrypt_id = document.trailer.get(b"Encrypt").unwrap().as_reference().unwrap();
        document
            .get_object_mut(encrypt_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .remove(b"Length");

        assert!(document.decrypt("user").is_ok());
    }
}
