use lopdf::{Document, EncryptionState, EncryptionVersion, IncrementalDocument, Object, Permissions, Result};
use std::io;
use tempfile::tempdir;

mod utils;

#[test]
fn load_incremental_file_as_linear_file() -> Result<()> {
    let doc = utils::load_document("assets/Incremental.pdf")?;
    assert_eq!(doc.version, "1.5".to_string());

    Ok(())
}

#[test]
fn load_incremental_file() -> Result<()> {
    let mut doc = utils::load_incremental_document("assets/Incremental.pdf")?;
    assert_eq!(doc.get_prev_documents().version, "1.5".to_string());

    // Create temporary folder to store file.
    let temp_dir = tempdir()?;
    let file_path = temp_dir.path().join("test_4_incremental.pdf");
    doc.save(file_path)?;

    Ok(())
}

/// Build a minimal document that has everything `EncryptionVersion::V1`
/// needs to derive an encryption key (a `Root` entry and a file `ID`).
fn minimal_encryptable_document() -> Document {
    let mut doc = Document::with_version("1.5");
    let catalog_id = doc.add_object(lopdf::dictionary! { "Type" => "Catalog" });
    doc.trailer.set("Root", Object::Reference(catalog_id));
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::string_literal(vec![1u8; 16]),
            Object::string_literal(vec![2u8; 16]),
        ]),
    );
    doc
}

/// After `decrypt`, `EncryptionState::encrypt_object_id()` must expose the
/// object id of the `/Encrypt` dictionary that was in the trailer before
/// decryption — an incremental save needs it to restore `/Encrypt` in the
/// appended trailer.
#[test]
fn decrypt_records_encrypt_object_id() {
    let mut doc = minimal_encryptable_document();

    let encryption_version = EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner",
        user_password: "user",
        permissions: Permissions::all(),
    };
    let encryption_state = EncryptionState::try_from(encryption_version).unwrap();
    doc.encrypt(&encryption_state).unwrap();

    let mut prev_bytes = Vec::new();
    doc.save_to(&mut prev_bytes).unwrap();

    // Capture the /Encrypt reference id from the on-disk trailer before decrypt.
    let expected_id = Document::load_mem(&prev_bytes)
        .unwrap()
        .trailer
        .get(b"Encrypt")
        .unwrap()
        .as_reference()
        .unwrap();

    let mut loaded = Document::load_mem(&prev_bytes).unwrap();
    loaded.decrypt("user").unwrap();

    let recorded = loaded
        .encryption_state
        .as_ref()
        .expect("encryption_state set after decrypt")
        .encrypt_object_id()
        .expect("encrypt_object_id recorded during decrypt");
    assert_eq!(recorded, expected_id);
}

/// Incremental save of a *decrypted* document (regression test for
/// https://github.com/J-F-Liu/lopdf/issues/520): the appended trailer would
/// lack `/Encrypt` while the original bytes remain ciphertext, so this must
/// be rejected rather than silently producing a corrupt file.
#[test]
fn incremental_save_of_decrypted_document_is_rejected() {
    let mut doc = minimal_encryptable_document();

    let encryption_version = EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner",
        user_password: "user",
        permissions: Permissions::all(),
    };
    let encryption_state = EncryptionState::try_from(encryption_version).unwrap();
    doc.encrypt(&encryption_state).unwrap();

    let mut prev_bytes = Vec::new();
    doc.save_to(&mut prev_bytes).unwrap();

    let mut loaded = Document::load_mem(&prev_bytes).unwrap();
    loaded.decrypt("user").unwrap();
    assert!(!loaded.is_encrypted());
    assert!(loaded.was_encrypted());

    let mut incremental = IncrementalDocument::create_from(prev_bytes, loaded);
    incremental.new_document.add_object(Object::Integer(42));

    let mut out = Vec::new();
    let err = incremental.save_to(&mut out).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
}

/// Incremental save of a still-*encrypted* (never decrypted) document must
/// also be rejected: the trailer still carries `/Encrypt`, but any newly
/// appended objects would be written as plaintext, which readers would
/// misinterpret as ciphertext.
#[test]
fn incremental_save_of_still_encrypted_document_is_rejected() {
    let mut doc = minimal_encryptable_document();

    let encryption_version = EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner",
        user_password: "user",
        permissions: Permissions::all(),
    };
    let encryption_state = EncryptionState::try_from(encryption_version).unwrap();
    doc.encrypt(&encryption_state).unwrap();

    let mut prev_bytes = Vec::new();
    doc.save_to(&mut prev_bytes).unwrap();

    // Load without decrypting: `is_encrypted()` stays true.
    let loaded = Document::load_mem(&prev_bytes).unwrap();
    assert!(loaded.is_encrypted());
    assert!(!loaded.was_encrypted());

    let mut incremental = IncrementalDocument::create_from(prev_bytes, loaded);
    incremental.new_document.add_object(Object::Integer(42));

    let mut out = Vec::new();
    let err = incremental.save_to(&mut out).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
}
