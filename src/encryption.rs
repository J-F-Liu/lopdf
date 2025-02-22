use crate::encodings;
use crate::rc4::Rc4;
use crate::{Document, Error, Object, ObjectId};
use aes::cipher::{
    block_padding::{PadType, RawPadding, UnpadError},
    BlockDecryptMut, BlockEncryptMut, KeyIvInit,
};
use md5::{Digest as _, Md5};
use rand::Rng as _;
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;

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

// If the password string is less than 32 bytes long, pad it by appending the required number of
// additional bytes from the beginning of the following padding string.
const PAD_BYTES: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08, 0x2E, 0x2E, 0x00,
    0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

const DEFAULT_KEY_LEN: Object = Object::Integer(40);
const DEFAULT_ALGORITHM: Object = Object::Integer(0);

/// Pad block with bytes with value equal to the number of bytes added.
///
/// PKCS#5 is described in [RFC 2898](https://tools.ietf.org/html/rfc2898).
#[derive(Clone, Copy, Debug)]
pub struct Pkcs5;

impl Pkcs5 {
    #[inline]
    fn unpad(block: &[u8], strict: bool) -> Result<&[u8], UnpadError> {
        // TODO: use bounds to check it at compile time
        if block.len() > 16 {
            panic!("block size is too big for PKCS#5");
        }
        let bs = block.len();
        let n = block[bs - 1];
        if n == 0 || n as usize > bs {
            return Err(UnpadError);
        }
        let s = bs - n as usize;
        if strict && block[s..bs - 1].iter().any(|&v| v != n) {
            return Err(UnpadError);
        }
        Ok(&block[..s])
    }
}

impl RawPadding for Pkcs5 {
    const TYPE: PadType = PadType::Reversible;

    #[inline]
    fn raw_pad(block: &mut [u8], pos: usize) {
        // TODO: use bounds to check it at compile time for Padding<B>
        if block.len() > 16 {
            panic!("block size is too big for PKCS#5");
        }
        if pos >= block.len() {
            panic!("`pos` is bigger or equal to block size");
        }
        let n = (block.len() - pos) as u8;
        for b in &mut block[pos..] {
            *b = n;
        }
    }

    #[inline]
    fn raw_unpad(block: &[u8]) -> Result<&[u8], UnpadError> {
        Pkcs5::unpad(block, true)
    }
}

#[derive(Clone, Debug)]
pub struct EncryptionState {
    pub crypt_filters: BTreeMap<Vec<u8>, Arc<dyn CryptFilter>>,
    pub key: Vec<u8>,
    pub stream_filter: Arc<dyn CryptFilter>,
    pub string_filter: Arc<dyn CryptFilter>,
}

pub trait CryptFilter: std::fmt::Debug {
    fn compute_key(&self, key: &[u8], obj_id: ObjectId) -> Result<Vec<u8>, DecryptionError>;
    fn encrypt(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, DecryptionError>;
    fn decrypt(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, DecryptionError>;
}

#[derive(Clone, Copy, Debug)]
pub struct IdentityCryptFilter;

impl CryptFilter for IdentityCryptFilter {
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

    fn encrypt(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
        Ok(Rc4::new(key).encrypt(plaintext))
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

    fn encrypt(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
        // The ciphertext needs to be a multiple of 16 bytes to include the padding.
        let ciphertext_len = (plaintext.len() + 15) / 16 * 16;

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

        // Use the 128-bit AES-CBC algorithm with PKCS#5 padding to encrypt the plaintext.
        //
        // Strings and streams encrypted with AES shall use a padding scheme that is described in
        // the Internet RFC 2898, PKCS #5: Password-Based Cryptography Specification Version 2.0;
        // see the Bibliography. For an original message length of M, the pad shall consist of 16 -
        // (M mod 16) bytes whose value shall also be 16 - (M mod 16).
        Aes128CbcEnc::new(key.into(), &iv.into())
            .encrypt_padded_mut::<Pkcs5>(&mut ciphertext[16..], plaintext.len())
            .unwrap();

        Ok(ciphertext)
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

        // Use the 128-bit AES-CBC algorithm with PKCS#5 padding to decrypt the ciphertext.
        //
        // Strings and streams encrypted with AES shall use a padding scheme that is described in
        // the Internet RFC 2898, PKCS #5: Password-Based Cryptography Specification Version 2.0;
        // see the Bibliography. For an original message length of M, the pad shall consist of 16 -
        // (M mod 16) bytes whose value shall also be 16 - (M mod 16).
        let data = &mut ciphertext[16..].to_vec();

        Ok(Aes128CbcDec::new(key.into(), &iv.into())
            .decrypt_padded_mut::<Pkcs5>(data)
            .unwrap()
            .to_vec())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Aes256CryptFilter;

impl CryptFilter for Aes256CryptFilter {
    fn compute_key(&self, key: &[u8], _obj_id: ObjectId) -> Result<Vec<u8>, DecryptionError> {
        // Use the 32-byte file encryption key for the AES-256 symmetric key algorithm.
        Ok(key.to_vec())
    }

    fn encrypt(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
        // The ciphertext needs to be a multiple of 16 bytes to include the padding.
        let ciphertext_len = (plaintext.len() + 15) / 16 * 16;

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

        // Use the 256-bit AES-CBC algorithm with PKCS#5 padding to encrypt the plaintext.
        //
        // Strings and streams encrypted with AES shall use a padding scheme that is described in
        // the Internet RFC 2898, PKCS #5: Password-Based Cryptography Specification Version 2.0;
        // see the Bibliography. For an original message length of M, the pad shall consist of 16 -
        // (M mod 16) bytes whose value shall also be 16 - (M mod 16).
        Aes256CbcEnc::new(key.into(), &iv.into())
            .encrypt_padded_mut::<Pkcs5>(&mut ciphertext[16..], plaintext.len())
            .unwrap();

        Ok(ciphertext)
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

        // Use the 256-bit AES-CBC algorithm with PKCS#7 padding to decrypt the ciphertext.
        //
        // Strings and streams encrypted with AES shall use a padding scheme that is described in
        // the Internet RFC 2898, PKCS #5: Password-Based Cryptography Specification Version 2.0;
        // see the Bibliography. For an original message length of M, the pad shall consist of 16 -
        // (M mod 16) bytes whose value shall also be 16 - (M mod 16).
        let data = &mut ciphertext[16..].to_vec();

        Ok(Aes256CbcDec::new(key.into(), &iv.into())
            .decrypt_padded_mut::<Pkcs5>(data)
            .unwrap()
            .to_vec())
    }
}

/// Sanitize the password (revision 4 and earlier).
///
/// This implements the first step of Algorithm 2 as described in ISO 32000-2:2020 (PDF 2.0).
///
/// This algorithm is deprecated in PDF 2.0.
fn sanitize_password_r4(
    password: &str,
) -> Result<Vec<u8>, DecryptionError> {
    // The password string is generated from host system codepage characters (or system scripts) by
    // first converting the string to PDFDocEncoding. If the input is Unicode, first convert to a
    // codepage encoding, and then to PDFDocEncoding for backward compatibility.
    let password = encodings::string_to_bytes(&encodings::PDF_DOC_ENCODING, password);

    Ok(password)
}

/// Compute a file encryption key in order to encrypt a document (revision 4 and earlier).
///
/// This implements Algorithm 2 as described in ISO 32000-2:2020 (PDF 2.0).
///
/// This algorithm is deprecated in PDF 2.0.
fn compute_file_encryption_key_r4<P>(
    doc: &Document,
    password: P,
    length: Option<usize>,
    revision: i64,
) -> Result<Vec<u8>, DecryptionError>
where
    P: AsRef<[u8]>,
{
    let password = password.as_ref();

    let encrypted = doc
        .get_encrypted()
        .map_err(|_| DecryptionError::MissingEncryptDictionary)?;

    let encrypt_metadata = encrypted
        .get(b"EncryptMetadata")
        .unwrap_or(&Object::Boolean(true))
        .as_bool()
        .map_err(|_| DecryptionError::InvalidType)?;

    // Pad or truncate the resulting password string to exactly 32 bytes. If the password string is
    // more than 32 bytes long, use only its first 32 bytes; if it is less than 32 bytes long, pad
    // it by appending the required number of additional bytes from the beginning of the following
    // padding string (see `PAD_BYTES`).
    //
    // That is, if the password is n bytes long, append the first 32 - n bytes of the padding
    // string to the end of the password string. If the password string is empty (zero-length),
    // meaning there is no user password, substitute the entire padding string in its place.
    //
    // i.e., we will simply calculate `len = min(password length, 32)` and use the first len bytes
    // of password and the last len bytes of `PAD_BYTES`.
    let len = password.len().min(32);

    // Initialize the MD5 hash function and pass the result as input to this function.
    let mut hasher = Md5::new();

    hasher.update(&password[..len]);
    hasher.update(&PAD_BYTES[len..]);

    // Pass the value of the encryption dictionary's O entry (owner password hash) to the MD5 hash
    // function.
    let hashed_owner_password = encrypted
        .get(b"O")
        .map_err(|_| DecryptionError::MissingOwnerPassword)?
        .as_str()
        .map_err(|_| DecryptionError::InvalidType)?;
    hasher.update(hashed_owner_password);

    // Convert the integer value of the P entry (permissions) to a 32-bit unsigned binary number
    // and pass these bytes to the MD5 hash function, low-order byte first.
    //
    // We don't actually care about the permissions, but we need the correct value to derive the
    // correct key.
    let permissions = encrypted
        .get(b"P")
        .map_err(|_| DecryptionError::MissingPermissions)?
        .as_i64()
        .map_err(|_| DecryptionError::InvalidType)? as u32;
    hasher.update(permissions.to_le_bytes());

    // Pass the first element of the file's file identifier array (the value of the ID entry in the
    // document's trailer dictionary to the MD5 hash function.
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
    hasher.update(file_id_0);

    // (Security handlers of revision 4 or greater) If document metadata is not being encrypted,
    // pass 4 bytes with the value 0xFFFFFFFF to the MD5 hash function.
    if revision >= 4 && !encrypt_metadata {
        hasher.update(b"\xff\xff\xff\xff");
    }

    // Finish the hash.
    let mut hash = hasher.finalize();

    // (Security handlers of revision 3 or greater) Do the following 50 times: take the output from
    // the previous MD5 hash and pass the first n bytes of the output as input into a new MD5 hash,
    // where n is the number of bytes of the file encryption key as defined by the value of the
    // encryption dictionary's Length entry.
    let n = if revision >= 3 {
        length.ok_or(DecryptionError::MissingKeyLength)? / 8
    } else {
        5
    };

    // The maximum supported key length is 16 bytes (128 bits) due to the use of MD5.
    if n > 16 {
        return Err(DecryptionError::InvalidKeyLength);
    }

    if revision >= 3 {
        for _ in 0..50 {
            hash = Md5::digest(&hash[..n]);
        }
    }

    // Set the file encryption key to the first n bytes of the output from the final MD5 hash,
    // where n shall always be 5 for security handlers of revision 2 but, for security handlers of
    // revision 3 or greater, shall depend on the value of the encrpytion dictionary's Length
    // entry.
    Ok(hash[..n].to_vec())
}

/// Compute the encryption dictionary's O-entry value (revision 4 and earlier).
///
/// This implements Algorithm 3 as described in ISO 32000-2:2020 (PDF 2.0).
///
/// This algorithm is deprecated in PDF 2.0.
fn compute_hashed_owner_password_r4<O, U>(
    owner_password: Option<O>,
    user_password: U,
    length: Option<usize>,
    revision: i64,
) -> Result<Vec<u8>, DecryptionError>
where
    O: AsRef<[u8]>,
    U: AsRef<[u8]>,
{
    let user_password = user_password.as_ref();

    // Pad or truncate the owner string. If there is no owner password, use the user password
    // instead.
    let password = owner_password.as_ref().map(|password| password.as_ref()).unwrap_or(user_password);

    // Pad or truncate the resulting password string to exactly 32 bytes. If the password string is
    // more than 32 bytes long, use only its first 32 bytes; if it is less than 32 bytes long, pad
    // it by appending the required number of additional bytes from the beginning of the following
    // padding string (see `PAD_BYTES`).
    //
    // That is, if the password is n bytes long, append the first 32 - n bytes of the padding
    // string to the end of the password string. If the password string is empty (zero-length),
    // meaning there is no user password, substitute the entire padding string in its place.
    //
    // i.e., we will simply calculate `len = min(password length, 32)` and use the first len bytes
    // of password and the last len bytes of `PAD_BYTES`.
    let len = password.len().min(32);

    // Initialize the MD5 hash function and pass the result as input to this function.
    let mut hasher = Md5::new();

    hasher.update(&password[..len]);
    hasher.update(&PAD_BYTES[len..]);

    let mut hash = hasher.finalize();

    // (Security handlers of revision 3 or greater) Do the following 50 times: take the output from
    // the previous MD5 hash and pass it as input into a new MD5 hash.
    if revision >= 3 {
        for _ in 0..50 {
            hash = Md5::digest(hash);
        }
    }

    // Create an RC4 file encryption key using the first n bytes of the output from the final MD5
    // hash, where n shall always be 5 for security handlers of revision 2 but, for security
    // handlers of revision 3 or greater, shall depend on the value of the encryption dictionary's
    // Length entry.
    let n = if revision >= 3 {
        length.ok_or(DecryptionError::MissingKeyLength)? / 8
    } else {
        5
    };

    // The maximum supported key length is 16 bytes (128 bits) due to the use of MD5.
    if n > 16 {
        return Err(DecryptionError::InvalidKeyLength);
    }

    // Pad or truncate the user password string to exactly 32 bytes. If the user password string is
    // more than 32 bytes long, use only its first 32 bytes; if it is less than 32 bytes long, pad
    // it by appending the required number of additional bytes from the beginning of the following
    // padding string (see `PAD_BYTES`).
    //
    // That is, if the password is n bytes long, append the first 32 - n bytes of the padding
    // string to the end of the password string. If the password string is empty (zero-length),
    // meaning there is no user password, substitute the entire padding string in its place.
    //
    // i.e., we will simply calculate `len = min(password length, 32)` and use the first len bytes
    // of password and the last len bytes of `PAD_BYTES`.
    let len = password.len().min(32);

    // Encrypt the result of the previous step using an RC4 encryption function with the RC4 file
    // encryption key obtained in the step before the previous step.
    let mut bytes = [0u8; 32];

    bytes[..len].copy_from_slice(&user_password[..len]);
    bytes[len..].copy_from_slice(&PAD_BYTES[len..]);

    let mut result = Rc4::new(&hash[..n]).encrypt(bytes);

    // (Security handlers of revision 3 or greater) Do the following 19 times: Take the output from
    // the previous invocation of the RC4 function and pass it as input to a new invocation of the
    // function; use a file encryption key generated by taking each byte of the RC4 file encryption
    // key and performing an XOR (exclusive or) operation between that byte and the single-byte
    // value of the iteration counter (from 1 to 19).
    if revision >= 3 {
        let mut key = vec![0u8; n];

        for i in 1..=19 {
            for (in_byte, out_byte) in hash[..n].iter().zip(key.iter_mut()) {
                *out_byte = in_byte ^ i;
            }

            result = Rc4::new(&key).encrypt(&result);
        }
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
fn compute_hashed_user_password_r2<U>(
    doc: &Document,
    user_password: U,
    length: Option<usize>,
    revision: i64,
) -> Result<Vec<u8>, DecryptionError>
where
    U: AsRef<[u8]>,
{
    // Create a file encryption key based on the user password string.
    let file_encryption_key = compute_file_encryption_key_r4(doc, user_password, length, revision)?;

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
fn compute_hashed_user_password_r3_r4<U>(
    doc: &Document,
    user_password: U,
    length: Option<usize>,
    revision: i64,
) -> Result<Vec<u8>, DecryptionError>
where
    U: AsRef<[u8]>,
{
    // Create a file encryption key based on the user password string.
    let file_encryption_key = compute_file_encryption_key_r4(doc, user_password, length, revision)?;

    // Initialize the MD5 hash function and pass the 32-byte padding string.
    let mut hasher = Md5::new();

    hasher.update(PAD_BYTES);

    // Pass the first element of the file's file identifier array (the value of the ID entry in the
    // document's trailer dictionary) to the hash function and finish the hash.
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
    hasher.update(file_id_0);

    let hash = hasher.finalize();

    // Encrypt the 16-byte result of the hash, using an RC4 encryption function with the file
    // encryption key.
    let mut result = Rc4::new(&file_encryption_key).encrypt(hash);

    // Do the following 19 times: Take the output from the previous invocation of the RC4 function
    // and pass it as input to a new invocation of the function; use a file encryption key
    // generated by taking each byte of the RC4 file encryption key and performing an XOR
    // (exclusive or) operation between that byte and the single-byte value of the iteration
    // counter (from 1 to 19).
    let mut key = vec![0u8; file_encryption_key.len()];

    for i in 1..=19 {
        for (in_byte, out_byte) in file_encryption_key.iter().zip(key.iter_mut()) {
            *out_byte = in_byte ^ i;
        }

        result = Rc4::new(&key).encrypt(&result);
    }

    // Append 16 bytes of arbitrary padding to the output from the final invocation of the RC4
    // function and store the 32-byte result as the value of the U entry in the encryption
    // dictionary.
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
fn authenticate_user_password_r4<U>(
    doc: &Document,
    user_password: U,
    length: Option<usize>,
    revision: i64,
) -> Result<(), DecryptionError>
where
    U: AsRef<[u8]>,
{
    // Perform all but the last step of Algorithm 4 (security handlers of revision 2) or Algorithm
    // 5 (security handlers of revision 3 or 4) using the supplied password string to compute the
    // encryption dictionary's U-entry value.
    let hashed_user_password = match revision {
        2 => compute_hashed_user_password_r2(doc, &user_password, length, revision)?,
        3 | 4 => compute_hashed_user_password_r3_r4(doc, &user_password, length, revision)?,
        _ => return Err(DecryptionError::InvalidRevision),
    };

    // If the result of the previous step is equal to the value of the encryption dictionary's U
    // entry (comparing on the first 16 bytes in the case of security handlers of revision 3 or
    // greater), the password supplied is the correct user password.
    let len = match revision {
        3 | 4 => 16,
        _ => hashed_user_password.len(),
    };

    let encrypted = doc
        .get_encrypted()
        .map_err(|_| DecryptionError::MissingEncryptDictionary)?;

    let stored_hashed_user_password = encrypted
        .get(b"U")
        .map_err(|_| DecryptionError::MissingUserPassword)?
        .as_str()
        .map_err(|_| DecryptionError::InvalidType)?;

    if hashed_user_password[..len] != stored_hashed_user_password[..len] {
        return Err(DecryptionError::IncorrectPassword);
    }

    Ok(())
}

/// Authenticate the owner password (revision 4 and earlier).
///
/// This implements Algorithm 7 as described in ISO 32000-2:2020 (PDF 2.0).
///
/// This algorithm is deprecated in PDF 2.0.
fn authenticate_owner_password_r4<O>(
    doc: &Document,
    owner_password: O,
    length: Option<usize>,
    revision: i64,
) -> Result<(), DecryptionError>
where
    O: AsRef<[u8]>,
{
    // Pad or truncate the owner string. If there is no owner password, use the user password
    // instead.
    let password = owner_password.as_ref();

    // Pad or truncate the resulting password string to exactly 32 bytes. If the password string is
    // more than 32 bytes long, use only its first 32 bytes; if it is less than 32 bytes long, pad
    // it by appending the required number of additional bytes from the beginning of the following
    // padding string (see `PAD_BYTES`).
    //
    // That is, if the password is n bytes long, append the first 32 - n bytes of the padding
    // string to the end of the password string. If the password string is empty (zero-length),
    // meaning there is no user password, substitute the entire padding string in its place.
    //
    // i.e., we will simply calculate `len = min(password length, 32)` and use the first len bytes
    // of password and the last len bytes of `PAD_BYTES`.
    let len = password.len().min(32);

    // Initialize the MD5 hash function and pass the result as input to this function.
    let mut hasher = Md5::new();

    hasher.update(&password[..len]);
    hasher.update(&PAD_BYTES[len..]);

    let mut hash = hasher.finalize();

    // (Security handlers of revision 3 or greater) Do the following 50 times: take the output from
    // the previous MD5 hash and pass it as input into a new MD5 hash.
    if revision >= 3 {
        for _ in 0..50 {
            hash = Md5::digest(hash);
        }
    }

    // Create an RC4 file encryption key using the first n bytes of the output from the final MD5
    // hash, where n shall always be 5 for security handlers of revision 2 but, for security
    // handlers of revision 3 or greater, shall depend on the value of the encryption dictionary's
    // Length entry.
    let n = if revision >= 3 {
        length.ok_or(DecryptionError::MissingKeyLength)? / 8
    } else {
        5
    };

    // The maximum supported key length is 16 bytes (128 bits) due to the use of MD5.
    if n > 16 {
        return Err(DecryptionError::InvalidKeyLength);
    }

    // Decrypt the value of the encryption dictionary's O entry, using an RC4 encryption function
    // with the file encryption key to retrieve the user password.
    let encrypted = doc
        .get_encrypted()
        .map_err(|_| DecryptionError::MissingEncryptDictionary)?;

    let mut result = encrypted
        .get(b"O")
        .map_err(|_| DecryptionError::MissingUserPassword)?
        .as_str()
        .map_err(|_| DecryptionError::InvalidType)?
        .to_vec();

    // (Security handlers of revision 3 or greater) Do the following 19 times: Take the output from
    // the previous invocation of the RC4 function and pass it as input to a new invocation of the
    // function; use a file encryption key generated by taking each byte of the RC4 file encryption
    // key and performing an XOR (exclusive or) operation between that byte and the single-byte
    // value of the iteration counter (from 19 to 1).
    if revision >= 3 {
        let mut key = vec![0u8; n];

        for i in (1..=19).rev() {
            for (in_byte, out_byte) in hash[..n].iter().zip(key.iter_mut()) {
                *out_byte = in_byte ^ i;
            }

            result = Rc4::new(&key).decrypt(&result);
        }
    }

    // (Security handler of revision 2 and the final step for revision 3 or greater) Decrypt the
    // value of the encryption dictionary's O entry, using an RC4 encryption function with the file
    // encryption key.
    result = Rc4::new(&hash[..n]).decrypt(&result);

    // The result of the previous step purports to be the user password. Authenticate this user
    // password using Algorithm 5. If it is correct, the password supplied is the correct owner
    // password.
    authenticate_user_password_r4(doc, &result, length, revision)
}

#[derive(Clone, Debug)]
pub struct PasswordAlgorithm {
    pub length: Option<usize>,
    pub revision: i64,
}

impl TryFrom<&Document> for PasswordAlgorithm {
    type Error = Error;

    fn try_from(value: &Document) -> Result<Self, Self::Error> {
        // Get the encrypted dictionary.
        let encrypted = value
            .get_encrypted()
            .map_err(|_| DecryptionError::MissingEncryptDictionary)?;

        // Get the Length field if any. Make sure that if it is present that it is a 64-bit integer and
        // that it can be converted to an unsigned size.
        let length = if encrypted.get(b"Length").is_ok() {
            Some(encrypted
                .get(b"Length")?
                .as_i64()?
                .try_into()?)
        } else {
            None
        };

        // Get the R field.
        let revision = encrypted
            .get(b"R")
            .map_err(|_| DecryptionError::MissingRevision)?
            .as_i64()
            .map_err(|_| DecryptionError::InvalidType)?;

        Ok(Self {
            length,
            revision,
        })
    }
}

impl PasswordAlgorithm {
    /// Sanitize the password.
    pub fn sanitize_password(
        &self,
        password: &str,
    ) -> Result<Vec<u8>, DecryptionError> {
        match self.revision {
            2..=4 => sanitize_password_r4(password),
            _ => Err(DecryptionError::UnsupportedRevision),
        }
    }

    /// Compute the file encryption key used to encrypt/decrypt the document.
    pub fn compute_file_encryption_key<P>(
        &self,
        doc: &Document,
        password: P,
    ) -> Result<Vec<u8>, DecryptionError>
    where
        P: AsRef<[u8]>,
    {
        match self.revision {
            2..=4 => compute_file_encryption_key_r4(doc, password, self.length, self.revision),
            _ => Err(DecryptionError::UnsupportedRevision),
        }
    }

    /// Compute the encryption dictionary's O-entry value.
    pub fn compute_hashed_owner_password<O, U>(
        &self,
        owner_password: Option<O>,
        user_password: U,
    ) -> Result<Vec<u8>, DecryptionError>
    where
        O: AsRef<[u8]>,
        U: AsRef<[u8]>,
    {
        match self.revision {
            2..=4 => compute_hashed_owner_password_r4(owner_password, user_password, self.length, self.revision),
            _ => Err(DecryptionError::UnsupportedRevision),
        }
    }

    /// Compute the encryption dictionary's U-entry value.
    pub fn compute_hashed_user_password<U>(
        &self,
        doc: &Document,
        user_password: U,
    ) -> Result<Vec<u8>, DecryptionError>
    where
        U: AsRef<[u8]>,
    {
        match self.revision {
            2 => compute_hashed_user_password_r2(doc, user_password, self.length, self.revision),
            3..=4 => compute_hashed_user_password_r3_r4(doc, user_password, self.length, self.revision),
            _ => Err(DecryptionError::UnsupportedRevision),
        }
    }

    /// Authenticate the owner password.
    pub fn authenticate_user_password<U>(
        &self,
        doc: &Document,
        user_password: U,
    ) -> Result<(), DecryptionError>
    where
        U: AsRef<[u8]>,
    {
        match self.revision {
            2..=4 => authenticate_user_password_r4(doc, user_password, self.length, self.revision),
            _ => Err(DecryptionError::UnsupportedRevision),
        }
    }

    /// Authenticate the owner password.
    pub fn authenticate_owner_password<O>(
        &self,
        doc: &Document,
        owner_password: O,
    ) -> Result<(), DecryptionError>
    where
        O: AsRef<[u8]>,
    {
        match self.revision {
            2..=4 => authenticate_owner_password_r4(doc, owner_password, self.length, self.revision),
            _ => Err(DecryptionError::UnsupportedRevision),
        }
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
