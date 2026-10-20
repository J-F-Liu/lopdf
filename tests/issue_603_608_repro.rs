//! Regression tests for #603 (strings in stream dictionaries) and #608
//! (signature `/Contents`), which must not be encrypted or decrypted.

use std::collections::BTreeMap;
use std::sync::Arc;

use lopdf::{
    Document, EncryptionState, EncryptionVersion, LoadOptions, Object, ObjectId, Permissions, SaveOptions, Stream,
    dictionary,
    encryption::{
        crypt_filters::{Aes128CryptFilter, Aes256CryptFilter, CryptFilter},
        decrypt_object, encrypt_object,
    },
};

const PALETTE: [u8; 6] = [0x28, 0xaf, 0xb9, 0xff, 0xff, 0xff];
const CONTENTS: [u8; 37] = [0x30; 37];

#[derive(Clone, Copy, Debug)]
enum Cipher {
    Aes128,
    Aes256,
    Rc4,
}

/// A minimal document with the trailer `/ID` that encryption requires.
fn minimal_document() -> Document {
    let mut doc = Document::with_version("1.5");
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::string_literal(b"0123456789abcdef"),
            Object::string_literal(b"fedcba9876543210"),
        ]),
    );

    let catalog = doc.add_object(dictionary! { "Type" => "Catalog" });
    doc.trailer.set("Root", catalog);

    doc
}

fn encryption_state(cipher: Cipher, doc: &Document) -> EncryptionState {
    let crypt_filters = |filter: Arc<dyn CryptFilter>| BTreeMap::from([(b"StdCF".to_vec(), filter)]);

    let version = match cipher {
        Cipher::Aes128 => EncryptionVersion::V4 {
            document: doc,
            encrypt_metadata: true,
            crypt_filters: crypt_filters(Arc::new(Aes128CryptFilter)),
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password: "owner",
            user_password: "user",
            permissions: Permissions::all(),
        },
        Cipher::Aes256 => EncryptionVersion::V5 {
            encrypt_metadata: true,
            crypt_filters: crypt_filters(Arc::new(Aes256CryptFilter)),
            file_encryption_key: &[0x42; 32],
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password: "owner",
            user_password: "user",
            permissions: Permissions::all(),
        },
        Cipher::Rc4 => EncryptionVersion::V2 {
            document: doc,
            key_length: 128,
            owner_password: "owner",
            user_password: "user",
            permissions: Permissions::all(),
        },
    };

    EncryptionState::try_from(version).unwrap()
}

/// A 1×1 image whose `/Indexed` colour space holds a string in the stream dictionary.
fn image_stream() -> Object {
    Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image",
            "Width" => 1, "Height" => 1, "BitsPerComponent" => 1,
            "ColorSpace" => vec!["Indexed".into(), "DeviceRGB".into(), 1.into(),
                                 Object::string_literal(PALETTE.to_vec())],
        },
        vec![0x00],
    ))
}

/// `/Contents` is in clear next to encrypted strings, as a writer must produce it.
fn signature_dictionary() -> Object {
    Object::Dictionary(dictionary! {
        "Type" => "Sig",
        "ByteRange" => vec![0.into(), 0.into(), 0.into(), 0.into()],
        "Contents" => Object::string_literal(CONTENTS.to_vec()),
        "M" => Object::string_literal("D:20261016120000Z"),
        "Reason" => Object::string_literal("Testing"),
    })
}

fn palette_of(object: &Object) -> Vec<u8> {
    object
        .as_stream()
        .unwrap()
        .dict
        .get(b"ColorSpace")
        .unwrap()
        .as_array()
        .unwrap()[3]
        .as_str()
        .unwrap()
        .to_vec()
}

/// Encrypts a document holding both an image stream and a signature dictionary,
/// saves it, and loads it back with the user password.
fn reloaded_document(cipher: Cipher) -> (Document, ObjectId, ObjectId) {
    let mut doc = minimal_document();
    let image = doc.add_object(image_stream());
    let signature = doc.add_object(signature_dictionary());

    let state = encryption_state(cipher, &doc);
    doc.encrypt(&state).unwrap();

    let mut pdf = Vec::new();
    doc.save_with_options(
        &mut pdf,
        SaveOptions::builder()
            .use_object_streams(false)
            .use_xref_streams(false)
            .build(),
    )
    .unwrap();

    let reloaded = Document::load_mem_with_options(&pdf, LoadOptions::with_password("user")).unwrap();

    (reloaded, image, signature)
}

/// #603: both directions cover the strings in a stream dictionary, not just its content.
#[test]
fn stream_dictionary_strings_are_encrypted_and_decrypted() {
    let mut doc = minimal_document();
    let id = doc.add_object(image_stream());
    let state = encryption_state(Cipher::Aes128, &doc);

    let mut object = doc.get_object(id).unwrap().clone();
    encrypt_object(&state, id, &mut object).unwrap();
    assert_ne!(
        palette_of(&object),
        PALETTE,
        "the stream dictionary string was left in clear"
    );

    decrypt_object(&state, id, &mut object).unwrap();
    assert_eq!(
        palette_of(&object),
        PALETTE,
        "the stream dictionary string was not decrypted"
    );
}

/// #608: the signature `/Contents` is stored in clear, so neither direction may touch it.
/// This does not go through `Document::encrypt`, whose output is itself under test.
#[test]
fn signature_contents_is_never_transformed() {
    let doc = minimal_document();
    let state = encryption_state(Cipher::Aes128, &doc);
    let obj_id = (7u32, 0u16);

    let mut signature = signature_dictionary();
    encrypt_object(&state, obj_id, &mut signature).unwrap();

    let dict = signature.as_dict().unwrap();
    assert_eq!(
        dict.get(b"Contents").unwrap().as_str().unwrap(),
        &CONTENTS[..],
        "encrypt_object encrypted the signature /Contents"
    );
    assert_ne!(
        dict.get(b"M").unwrap().as_str().unwrap(),
        b"D:20261016120000Z",
        "encrypt_object left /M in clear"
    );

    decrypt_object(&state, obj_id, &mut signature).unwrap();

    let dict = signature.as_dict().unwrap();
    assert_eq!(
        dict.get(b"Contents").unwrap().as_str().unwrap(),
        &CONTENTS[..],
        "decrypt_object touched the signature /Contents"
    );
    assert_eq!(
        dict.get(b"M").unwrap().as_str().unwrap(),
        b"D:20261016120000Z",
        "/M did not survive the round trip"
    );
}

/// #603 and #608 end to end: the palette string comes back, the signature
/// `/Contents` is untouched and its neighbouring strings are decrypted.
#[test]
fn round_trip_keeps_dictionary_strings_intact() {
    for cipher in [Cipher::Aes128, Cipher::Aes256, Cipher::Rc4] {
        let (doc, image, signature) = reloaded_document(cipher);

        assert_eq!(
            palette_of(doc.get_object(image).unwrap()),
            PALETTE,
            "{cipher:?}: the image palette did not survive the round trip"
        );

        let signature = doc.get_object(signature).unwrap().as_dict().unwrap();
        assert_eq!(
            signature.get(b"Contents").unwrap().as_str().unwrap(),
            &CONTENTS[..],
            "{cipher:?}: the signature /Contents was altered"
        );
        assert_eq!(
            signature.get(b"M").unwrap().as_str().unwrap(),
            b"D:20261016120000Z",
            "{cipher:?}: /M was not decrypted"
        );
        assert_eq!(
            signature.get(b"Reason").unwrap().as_str().unwrap(),
            b"Testing",
            "{cipher:?}: /Reason was not decrypted"
        );
    }
}

/// #608: an entry that fails to decrypt must not leave the entries after it encrypted.
#[test]
fn failing_entry_does_not_abandon_the_rest() {
    let doc = minimal_document();
    let state = encryption_state(Cipher::Aes128, &doc);

    let obj_id = (99u32, 0u16);
    let mut encrypted = signature_dictionary();
    encrypt_object(&state, obj_id, &mut encrypted).unwrap();
    let encrypted = encrypted.as_dict().unwrap();

    // Without `/Type` or `/ByteRange` this is no signature dictionary, so `/Contents`
    // is visited and fails; `/M` and `/Reason` must still be decrypted.
    let mut object = Object::Dictionary(dictionary! {
        "Contents" => Object::string_literal(CONTENTS.to_vec()),
        "M" => encrypted.get(b"M").unwrap().clone(),
        "Reason" => encrypted.get(b"Reason").unwrap().clone(),
    });

    assert!(
        decrypt_object(&state, obj_id, &mut object).is_err(),
        "the failure must be reported"
    );

    let dict = object.as_dict().unwrap();
    assert_eq!(dict.get(b"M").unwrap().as_str().unwrap(), b"D:20261016120000Z");
    assert_eq!(dict.get(b"Reason").unwrap().as_str().unwrap(), b"Testing");
}

/// #608 at document level: a failing object must not leave the later ones encrypted.
#[test]
fn undecryptable_object_does_not_abort_document_decryption() {
    let mut doc = minimal_document();
    let broken = doc.add_object(Object::string_literal("broken"));
    let intact = doc.add_object(Object::string_literal("intact"));

    let state = encryption_state(Cipher::Aes128, &doc);
    doc.encrypt(&state).unwrap();

    // Ciphertext that is not a whole number of AES blocks can never be decrypted.
    *doc.get_object_mut(broken).unwrap().as_str_mut().unwrap() = vec![0x30; 37];

    assert!(doc.decrypt("user").is_err(), "the failure must be reported");
    assert_eq!(
        doc.get_object(intact).unwrap().as_str().unwrap(),
        b"intact",
        "the object after the failing one was left encrypted"
    );
}
