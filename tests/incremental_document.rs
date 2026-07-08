use lopdf::encryption::crypt_filters::{Aes128CryptFilter, Aes256CryptFilter, CryptFilter};
use lopdf::{
    Document, EncryptionState, EncryptionVersion, IncrementalDocument, LoadOptions, Object, Permissions, Result,
    Stream, dictionary,
};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::sync::Arc;
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

/// Prepare an encrypted PDF: build a minimal encryptable document, apply the
/// supplied `EncryptionState`, and save it to bytes.
fn build_encrypted_pdf(state: &EncryptionState) -> Vec<u8> {
    let mut doc = minimal_encryptable_document();
    doc.encrypt(state).unwrap();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

/// Return the `/Encrypt` object id currently stored in the on-disk trailer
/// of `bytes` (the file must not be decrypted before calling this).
fn read_encrypt_id(bytes: &[u8]) -> lopdf::ObjectId {
    Document::load_mem(bytes)
        .unwrap()
        .trailer
        .get(b"Encrypt")
        .unwrap()
        .as_reference()
        .unwrap()
}

/// Load `bytes` and immediately decrypt using `password`, returning the
/// fully-populated `Document`. `Document::load_mem` defers object parsing for
/// encrypted files that arrive without a password, so tests that need the
/// content objects present must go through `load_mem_with_options`.
fn load_and_decrypt(bytes: &[u8], password: &str) -> Document {
    Document::load_mem_with_options(bytes, LoadOptions::with_password(password)).unwrap()
}

/// End-to-end verification of an incremental save on a decrypted document:
/// build encrypted `prev_bytes`, decrypt them, append a new plaintext-marker
/// stream, save incrementally, then check that the appended region does not
/// leak the plaintext, that decrypt+read still round-trips, and that the
/// trailer's `/Encrypt` reference resolves to the original id.
fn encrypted_incremental_round_trip(label: &str, state: EncryptionState) {
    let prev_bytes = build_encrypted_pdf(&state);
    let original_encrypt_id = read_encrypt_id(&prev_bytes);

    let loaded = load_and_decrypt(&prev_bytes, "user");
    assert!(
        loaded.encryption_state.as_ref().unwrap().encrypt_object_id().is_some(),
        "{label}: encrypt_object_id must be recorded during load-and-decrypt"
    );

    let mut incremental = IncrementalDocument::create_from(prev_bytes.clone(), loaded);
    let marker: Vec<u8> = format!("PR520-MARKER-{label}").into_bytes();
    let stream = Stream::new(dictionary! {}, marker.clone()).with_compression(false);
    let new_stream_id = incremental.new_document.add_object(Object::Stream(stream));

    let mut out = Vec::new();
    incremental.save_to(&mut out).unwrap();

    // Assert A: the plaintext marker must not appear anywhere in the
    // appended (encrypted) region.
    let appended = &out[prev_bytes.len()..];
    assert!(
        !appended.windows(marker.len()).any(|w| w == marker.as_slice()),
        "{label}: plaintext marker leaked in the appended region"
    );

    // Assert B: round-trip. Reloading and decrypting the appended file must
    // reveal the marker again.
    let reloaded = load_and_decrypt(&out, "user");
    let stream = reloaded.get_object(new_stream_id).unwrap().as_stream().unwrap();
    assert_eq!(
        stream.content, marker,
        "{label}: appended stream content mismatch after round-trip"
    );

    // Assert C: the trailer's /Encrypt reference in the appended file resolves
    // to the same object id as the original encrypted revision. The dictionary
    // bytes themselves live in the previous revision and are still intact.
    let round_encrypt_id = read_encrypt_id(&out);
    assert_eq!(
        round_encrypt_id, original_encrypt_id,
        "{label}: trailer /Encrypt reference mismatch after round-trip"
    );
}

/// Incremental save of a document decrypted from V1 (RC4-40) round-trips —
/// replaces the earlier PR #521 rejection test now that #520 supports this.
#[test]
fn incremental_save_of_decrypted_document_v1_round_trip() {
    let doc = minimal_encryptable_document();
    let state = EncryptionState::try_from(EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner",
        user_password: "user",
        permissions: Permissions::all(),
    })
    .unwrap();
    encrypted_incremental_round_trip("V1", state);
}

#[test]
fn incremental_save_of_decrypted_document_v2_round_trip() {
    let doc = minimal_encryptable_document();
    let state = EncryptionState::try_from(EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner",
        user_password: "user",
        key_length: 128,
        permissions: Permissions::all(),
    })
    .unwrap();
    encrypted_incremental_round_trip("V2", state);
}

#[test]
fn incremental_save_of_decrypted_document_v4_round_trip() {
    let doc = minimal_encryptable_document();
    let crypt_filter: Arc<dyn CryptFilter> = Arc::new(Aes128CryptFilter);
    let state = EncryptionState::try_from(EncryptionVersion::V4 {
        document: &doc,
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), crypt_filter)]),
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: "owner",
        user_password: "user",
        permissions: Permissions::all(),
    })
    .unwrap();
    encrypted_incremental_round_trip("V4", state);
}

#[test]
fn incremental_save_of_decrypted_document_v5_round_trip() {
    // Fixed key: this is a deterministic test; `Aes256CryptFilter` combined
    // with V5's per-object IV keeps the outputs distinct anyway.
    let file_encryption_key = [7u8; 32];
    let crypt_filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
    let state = EncryptionState::try_from(EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), crypt_filter)]),
        file_encryption_key: &file_encryption_key,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: "owner",
        user_password: "user",
        permissions: Permissions::all(),
    })
    .unwrap();
    encrypted_incremental_round_trip("V5", state);
}

/// Regression guard for the "clone before encrypt" invariant: two successive
/// incremental saves on the same `IncrementalDocument` must not double-encrypt
/// the objects added in the earlier round. If we mutated in-place, the second
/// save would re-encrypt already-encrypted bytes and decrypt would return
/// garbage instead of the marker.
#[test]
fn incremental_save_of_decrypted_document_does_not_double_encrypt_on_repeated_save() {
    let doc = minimal_encryptable_document();
    let state = EncryptionState::try_from(EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner",
        user_password: "user",
        permissions: Permissions::all(),
    })
    .unwrap();
    let prev_bytes = build_encrypted_pdf(&state);

    let loaded = load_and_decrypt(&prev_bytes, "user");
    let mut incremental = IncrementalDocument::create_from(prev_bytes, loaded);

    let marker_a: Vec<u8> = b"PR520-A".to_vec();
    let stream_a = Stream::new(dictionary! {}, marker_a.clone()).with_compression(false);
    let id_a = incremental.new_document.add_object(Object::Stream(stream_a));

    let mut buf1 = Vec::new();
    incremental.save_to(&mut buf1).unwrap();

    let marker_b: Vec<u8> = b"PR520-B".to_vec();
    let stream_b = Stream::new(dictionary! {}, marker_b.clone()).with_compression(false);
    let id_b = incremental.new_document.add_object(Object::Stream(stream_b));

    let mut buf2 = Vec::new();
    incremental.save_to(&mut buf2).unwrap();

    let reloaded = load_and_decrypt(&buf2, "user");
    let content_a = reloaded.get_object(id_a).unwrap().as_stream().unwrap().content.clone();
    let content_b = reloaded.get_object(id_b).unwrap().as_stream().unwrap().content.clone();
    assert_eq!(
        content_a, marker_a,
        "stream A must survive a second incremental save without double-encryption"
    );
    assert_eq!(
        content_b, marker_b,
        "stream B must be readable after the second incremental save"
    );
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

/// `IncrementalDocument::save(path)` must not truncate a pre-existing file
/// when the incremental save is unsupported. Prior to this check the guard
/// ran only after `File::create` had already zeroed the target.
#[test]
fn incremental_save_to_path_does_not_truncate_on_unsupported_input() {
    let mut doc = minimal_encryptable_document();
    let encryption_state = EncryptionState::try_from(EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner",
        user_password: "user",
        permissions: Permissions::all(),
    })
    .unwrap();
    doc.encrypt(&encryption_state).unwrap();

    let mut prev_bytes = Vec::new();
    doc.save_to(&mut prev_bytes).unwrap();

    // Load without decrypting: this is the still-encrypted (unsupported) case.
    let loaded = Document::load_mem(&prev_bytes).unwrap();
    let mut incremental = IncrementalDocument::create_from(prev_bytes, loaded);

    let temp_dir = tempdir().unwrap();
    let target_path = temp_dir.path().join("existing.pdf");
    let sentinel: &[u8] = b"do-not-truncate-me";
    fs::write(&target_path, sentinel).unwrap();

    let err = incremental.save(&target_path).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Unsupported);

    // File must be untouched on disk.
    let on_disk = fs::read(&target_path).unwrap();
    assert_eq!(
        on_disk, sentinel,
        "unsupported save must not truncate or overwrite the target file"
    );
}
