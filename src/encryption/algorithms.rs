use crate::encodings;
use crate::{Document, Error, Object};
use md5::{Digest as _, Md5};
use rand::Rng as _;
use super::DecryptionError;
use super::rc4::Rc4;

// If the password string is less than 32 bytes long, pad it by appending the required number of
// additional bytes from the beginning of the following padding string.
const PAD_BYTES: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08, 0x2E, 0x2E, 0x00,
    0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

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
    /// Sanitize the password (revision 4 and earlier).
    ///
    /// This implements the first step of Algorithm 2 as described in ISO 32000-2:2020 (PDF 2.0).
    ///
    /// This algorithm is deprecated in PDF 2.0.
    fn sanitize_password_r4(
        &self,
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
        &self,
        doc: &Document,
        password: P,
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
        if self.revision >= 4 && !encrypt_metadata {
            hasher.update(b"\xff\xff\xff\xff");
        }

        // Finish the hash.
        let mut hash = hasher.finalize();

        // (Security handlers of revision 3 or greater) Do the following 50 times: take the output from
        // the previous MD5 hash and pass the first n bytes of the output as input into a new MD5 hash,
        // where n is the number of bytes of the file encryption key as defined by the value of the
        // encryption dictionary's Length entry.
        let n = if self.revision >= 3 {
            self.length.ok_or(DecryptionError::MissingKeyLength)? / 8
        } else {
            5
        };

        // The maximum supported key length is 16 bytes (128 bits) due to the use of MD5.
        if n > 16 {
            return Err(DecryptionError::InvalidKeyLength);
        }

        if self.revision >= 3 {
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
        &self,
        owner_password: Option<O>,
        user_password: U,
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
        if self.revision >= 3 {
            for _ in 0..50 {
                hash = Md5::digest(hash);
            }
        }

        // Create an RC4 file encryption key using the first n bytes of the output from the final MD5
        // hash, where n shall always be 5 for security handlers of revision 2 but, for security
        // handlers of revision 3 or greater, shall depend on the value of the encryption dictionary's
        // Length entry.
        let n = if self.revision >= 3 {
            self.length.ok_or(DecryptionError::MissingKeyLength)? / 8
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
        if self.revision >= 3 {
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
        &self,
        doc: &Document,
        user_password: U,
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
    fn compute_hashed_user_password_r3_r4<U>(
        &self,
        doc: &Document,
        user_password: U,
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
        &self,
        doc: &Document,
        user_password: U,
    ) -> Result<(), DecryptionError>
    where
        U: AsRef<[u8]>,
    {
        // Perform all but the last step of Algorithm 4 (security handlers of revision 2) or Algorithm
        // 5 (security handlers of revision 3 or 4) using the supplied password string to compute the
        // encryption dictionary's U-entry value.
        let hashed_user_password = match self.revision {
            2 => self.compute_hashed_user_password_r2(doc, &user_password)?,
            3 | 4 => self.compute_hashed_user_password_r3_r4(doc, &user_password)?,
            _ => return Err(DecryptionError::InvalidRevision),
        };

        // If the result of the previous step is equal to the value of the encryption dictionary's U
        // entry (comparing on the first 16 bytes in the case of security handlers of revision 3 or
        // greater), the password supplied is the correct user password.
        let len = match self.revision {
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
        &self,
        doc: &Document,
        owner_password: O,
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
        if self.revision >= 3 {
            for _ in 0..50 {
                hash = Md5::digest(hash);
            }
        }

        // Create an RC4 file encryption key using the first n bytes of the output from the final MD5
        // hash, where n shall always be 5 for security handlers of revision 2 but, for security
        // handlers of revision 3 or greater, shall depend on the value of the encryption dictionary's
        // Length entry.
        let n = if self.revision >= 3 {
            self.length.ok_or(DecryptionError::MissingKeyLength)? / 8
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
        if self.revision >= 3 {
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
        self.authenticate_user_password_r4(doc, &result)
    }

    /// Sanitize the password.
    pub fn sanitize_password(
        &self,
        password: &str,
    ) -> Result<Vec<u8>, DecryptionError> {
        match self.revision {
            2..=4 => self.sanitize_password_r4(password),
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
            2..=4 => self.compute_file_encryption_key_r4(doc, password),
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
            2..=4 => self.compute_hashed_owner_password_r4(owner_password, user_password),
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
            2 => self.compute_hashed_user_password_r2(doc, user_password),
            3..=4 => self.compute_hashed_user_password_r3_r4(doc, user_password),
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
            2..=4 => self.authenticate_user_password_r4(doc, user_password),
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
            2..=4 => self.authenticate_owner_password_r4(doc, owner_password),
            _ => Err(DecryptionError::UnsupportedRevision),
        }
    }
}
