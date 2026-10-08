use lopdf::{Document, Object, Stream, dictionary};

#[cfg(not(feature = "async"))]
use lopdf::LoadOptions;

/// A document with one page per entry of `texts`, each page showing its text,
/// and the trailer `/ID` that encryption requires.
fn document_with_pages(texts: &[&str]) -> Document {
    let mut doc = Document::with_version("1.5");
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::string_literal(b"0123456789abcdef"),
            Object::string_literal(b"fedcba9876543210"),
        ]),
    );

    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });

    let kids: Vec<Object> = texts
        .iter()
        .map(|text| {
            let content = format!("BT\n/F1 12 Tf\n100 700 Td\n({text}) Tj\nET\n");
            let contents_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
            Object::Reference(doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
                "Contents" => contents_id,
            }))
        })
        .collect();

    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => texts.len() as i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc
}

/// Encrypts `doc` in place, ready to be saved.
fn encrypt(doc: &mut Document, owner_password: &str, user_password: &str) {
    let state = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V2 {
        document: doc,
        owner_password,
        user_password,
        key_length: 128,
        permissions: lopdf::Permissions::all(),
    })
    .unwrap();
    doc.encrypt(&state).unwrap();
}

/// The content stream of every page, in page order.
#[cfg(not(feature = "async"))]
fn content_streams(doc: &Document) -> Vec<lopdf::ObjectId> {
    doc.get_pages()
        .values()
        .map(|page| doc.get_page_contents(*page)[0])
        .collect()
}

/// The text of every page, concatenated.
fn extract_text(doc: &Document) -> String {
    let page_numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
    doc.extract_text(&page_numbers).unwrap()
}

#[cfg(not(feature = "async"))]
#[test]
fn test_load_encrypted_pdf_from_assets() {
    let doc = Document::load("assets/encrypted.pdf").unwrap();

    assert!(
        !doc.is_encrypted(),
        "Document should not appear encrypted after decryption"
    );
    assert!(doc.encryption_state.is_some());

    let pages = doc.get_pages();
    assert_eq!(pages.len(), 1, "Should have exactly one page");

    let text = extract_text(&doc);
    assert!(text.contains("USCIS"), "Should contain USCIS text from the form");
    assert!(text.contains("Form G-1145"), "Should contain form number");

    assert!(doc.trailer.get(b"Root").is_ok(), "Trailer should have Root entry");
    assert!(
        doc.trailer.get(b"Encrypt").is_err(),
        "Encrypt entry should be removed after decryption"
    );
    assert!(doc.trailer.get(b"Info").is_ok(), "Trailer should have Info entry");
}

#[cfg(not(feature = "async"))]
#[test]
fn test_decrypt_pdf_with_empty_password() {
    let mut doc = document_with_pages(&["Hello, Encrypted World!"]);
    encrypt(&mut doc, "", "");

    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test_encrypted.pdf");
    doc.save(&path).unwrap();

    // An empty password opens the document without any hint from the caller.
    let loaded_doc = Document::load(&path).unwrap();

    assert!(
        !loaded_doc.is_encrypted(),
        "Should not appear encrypted after decryption"
    );
    assert!(loaded_doc.encryption_state.is_some());
    assert_eq!(loaded_doc.get_pages().len(), 1);
    assert!(extract_text(&loaded_doc).contains("Hello, Encrypted World!"));
}

#[cfg(feature = "async")]
#[tokio::test]
async fn test_decrypt_pdf_with_empty_password_async() {
    let mut doc = document_with_pages(&["Hello, Async Encrypted World!"]);
    encrypt(&mut doc, "", "");

    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test_encrypted_async.pdf");
    doc.save(&path).unwrap();

    let loaded_doc = Document::load(&path).await.unwrap();

    assert!(
        !loaded_doc.is_encrypted(),
        "Should not appear encrypted after decryption"
    );
    assert!(loaded_doc.encryption_state.is_some());
    assert_eq!(loaded_doc.get_pages().len(), 1);
    assert!(extract_text(&loaded_doc).contains("Hello, Async Encrypted World!"));
}

#[cfg(not(feature = "async"))]
#[test]
fn test_load_with_password_correct_password() {
    let mut doc = document_with_pages(&["Password Protected Content!"]);
    encrypt(&mut doc, "owner_secret", "user_secret");

    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test_password_protected.pdf");
    doc.save(&path).unwrap();

    // The empty password does not open the document, but it is still readable.
    let loaded_without_password = Document::load(&path).unwrap();
    assert!(
        loaded_without_password.is_encrypted(),
        "Should still appear encrypted when auth fails"
    );

    let loaded_with_password = Document::load_with_password(&path, "user_secret").unwrap();
    assert!(
        !loaded_with_password.is_encrypted(),
        "Should not appear encrypted after successful decryption"
    );
    assert!(loaded_with_password.encryption_state.is_some());
    assert_eq!(loaded_with_password.get_pages().len(), 1);
    assert!(extract_text(&loaded_with_password).contains("Password Protected Content!"));
}

#[cfg(not(feature = "async"))]
#[test]
fn test_load_with_password_owner_password_recovers_correct_content() {
    // The owner and user passwords are independent and either opens the document, but the file
    // key (Algorithm 2) is always derived from the *user* password. Deriving it from the literal
    // owner password yields a wrong key that still parses, since structure is never encrypted.
    let mut doc = document_with_pages(&["Password Protected Content!"]);
    encrypt(&mut doc, "owner_secret", "user_secret");

    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test_owner_password.pdf");
    doc.save(&path).unwrap();

    // Load with the OWNER password, not the user password.
    let loaded = Document::load_with_password(&path, "owner_secret").unwrap();
    assert!(
        !loaded.is_encrypted(),
        "Should not appear encrypted after successful owner-password decryption"
    );
    assert_eq!(loaded.get_pages().len(), 1);

    // Only the decrypted *content* reveals a wrong key.
    assert!(
        extract_text(&loaded).contains("Password Protected Content!"),
        "Owner-password decryption must recover the same content as user-password \
         decryption, not garbage derived from the literal owner password"
    );
}

#[cfg(not(feature = "async"))]
#[test]
fn test_load_with_password_rejected_passwords() {
    let mut doc = document_with_pages(&[]);
    encrypt(&mut doc, "secret", "secret");

    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test_rejected_passwords.pdf");
    doc.save(&path).unwrap();

    for password in ["wrong_password", ""] {
        let result = Document::load_with_password(&path, password);
        assert!(
            matches!(result, Err(lopdf::Error::InvalidPassword)),
            "{password:?} should be rejected, got {result:?}"
        );
    }
}

#[cfg(not(feature = "async"))]
#[test]
fn test_load_with_password_non_ascii_password() {
    // R2-R4 passwords are converted to PDFDocEncoding before use, so their bytes differ from
    // the UTF-8 the caller passes in as soon as they contain a non-ASCII character.
    let mut doc = document_with_pages(&["Non-ASCII Password!"]);
    encrypt(&mut doc, "pröprietär", "geheimnis-ä");

    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).unwrap();

    for password in ["geheimnis-ä", "pröprietär"] {
        let loaded = Document::load_mem_with_options(&buffer, LoadOptions::with_password(password)).unwrap();
        assert!(!loaded.is_encrypted(), "{password:?} should open the document");
        assert!(extract_text(&loaded).contains("Non-ASCII Password!"));
    }
}

#[cfg(not(feature = "async"))]
#[test]
fn test_load_with_password_saslprep_password() {
    use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    // R6 passwords are normalized with SASLprep, which maps the "ﬁ" ligature to "fi".
    let mut doc = document_with_pages(&["SASLprep Password!"]);
    let crypt_filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
    let state = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), crypt_filter)]),
        file_encryption_key: &[7u8; 32],
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: "owner-\u{FB01}le",
        user_password: "user-\u{FB01}le",
        permissions: lopdf::Permissions::all(),
    })
    .unwrap();
    doc.encrypt(&state).unwrap();

    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).unwrap();

    for password in ["user-\u{FB01}le", "owner-\u{FB01}le", "user-file"] {
        let loaded = Document::load_mem_with_options(&buffer, LoadOptions::with_password(password)).unwrap();
        assert!(!loaded.is_encrypted(), "{password:?} should open the document");
        assert!(extract_text(&loaded).contains("SASLprep Password!"));
    }
}

#[cfg(not(feature = "async"))]
#[test]
fn test_load_mem_with_password() {
    let mut doc = document_with_pages(&["Memory Loaded!"]);
    encrypt(&mut doc, "mem_owner", "mem_user");

    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).unwrap();

    let loaded_doc = Document::load_mem_with_options(&buffer, LoadOptions::with_password("mem_user")).unwrap();
    assert!(
        !loaded_doc.is_encrypted(),
        "Should not appear encrypted after decryption"
    );
    assert!(loaded_doc.encryption_state.is_some());
    assert_eq!(loaded_doc.get_pages().len(), 1);
}

#[cfg(feature = "async")]
#[tokio::test]
async fn test_load_with_password_async() {
    let mut doc = document_with_pages(&["Async Password Protected!"]);
    encrypt(&mut doc, "async_owner", "async_user");

    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test_async_password.pdf");
    doc.save(&path).unwrap();

    let loaded_doc = Document::load_with_password(&path, "async_user").await.unwrap();
    assert!(
        !loaded_doc.is_encrypted(),
        "Should not appear encrypted after decryption"
    );
    assert!(loaded_doc.encryption_state.is_some());
    assert_eq!(loaded_doc.get_pages().len(), 1);
}

#[cfg(not(feature = "async"))]
#[test]
fn test_load_with_password_multipage_roundtrip() {
    let texts = ["Page 1 Content!", "Page 2 Content!", "Page 3 Content!"];
    let mut doc = document_with_pages(&texts);

    let temp_dir = tempfile::tempdir().unwrap();
    let unencrypted_path = temp_dir.path().join("multipage_unencrypted.pdf");
    doc.save(&unencrypted_path).unwrap();
    let unencrypted_size = std::fs::metadata(&unencrypted_path).unwrap().len();

    encrypt(&mut doc, "owner_password", "test_password");
    let encrypted_path = temp_dir.path().join("multipage_encrypted.pdf");
    doc.save(&encrypted_path).unwrap();

    let mut loaded_doc = Document::load_with_password(&encrypted_path, "test_password").unwrap();
    assert!(
        !loaded_doc.is_encrypted(),
        "Should not appear encrypted after decryption"
    );
    assert!(loaded_doc.encryption_state.is_some());
    assert_eq!(loaded_doc.get_pages().len(), 3);

    let text = extract_text(&loaded_doc);
    for expected in texts {
        assert!(text.contains(expected), "Should contain {expected:?}, got {text:?}");
    }

    // Saving a decrypted document keeps its content: a round-trip through the
    // writer used to shrink a 197 KB file to a few hundred bytes.
    let resaved_path = temp_dir.path().join("multipage_resaved.pdf");
    loaded_doc.save(&resaved_path).unwrap();
    assert!(
        std::fs::metadata(&resaved_path).unwrap().len() > unencrypted_size / 2,
        "Re-saved file is unexpectedly small"
    );

    let reloaded = Document::load(&resaved_path).unwrap();
    assert_eq!(reloaded.get_pages().len(), 3);
}

#[cfg(not(feature = "async"))]
#[test]
fn test_load_with_password_with_compressed_streams() {
    let mut doc = document_with_pages(&["Compressed Page 1!", "Compressed Page 2!"]);
    for content_id in content_streams(&doc) {
        let Object::Stream(stream) = doc.get_object_mut(content_id).unwrap() else {
            unreachable!("page contents are streams");
        };
        stream.compress().unwrap();
    }
    encrypt(&mut doc, "owner_secret", "user_secret");

    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test_compressed_encrypted.pdf");
    doc.save(&path).unwrap();

    let loaded_doc = Document::load_with_password(&path, "user_secret").unwrap();
    assert!(
        !loaded_doc.is_encrypted(),
        "Should not appear encrypted after decryption"
    );
    assert!(loaded_doc.encryption_state.is_some());
    assert_eq!(loaded_doc.get_pages().len(), 2);
    assert!(extract_text(&loaded_doc).contains("Compressed Page 1!"));
}

#[cfg(not(feature = "async"))]
#[test]
fn test_load_with_password_stream_with_endobj_bytes() {
    // Binary stream data may contain "endobj", which must not be mistaken for
    // the end of the stream.
    let mut doc = document_with_pages(&["Test"]);
    let Object::Stream(stream) = doc.get_object_mut(content_streams(&doc)[0]).unwrap() else {
        unreachable!("page contents are streams");
    };
    stream.content.extend_from_slice(b"endobj fake marker");

    let second_obj_id = doc.new_object_id();
    doc.objects.insert(second_obj_id, Object::Integer(42));
    encrypt(&mut doc, "owner_secret", "user_secret");

    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test_stream_with_endobj.pdf");
    doc.save(&path).unwrap();

    let loaded_doc = Document::load_with_password(&path, "user_secret").unwrap();
    assert!(
        loaded_doc.get_object(second_obj_id).is_ok(),
        "Object behind the stream should be loaded"
    );
}

#[cfg(not(feature = "async"))]
#[test]
fn test_was_encrypted_method() {
    let mut doc = document_with_pages(&["Hello, Encrypted World!"]);
    assert!(!doc.is_encrypted(), "Unencrypted doc should not be encrypted");
    assert!(!doc.was_encrypted(), "Unencrypted doc was not originally encrypted");

    encrypt(&mut doc, "owner", "user");
    assert!(doc.is_encrypted(), "Should be encrypted after encrypt()");

    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test_was_encrypted.pdf");
    doc.save(&path).unwrap();

    // After loading with the correct password the document is decrypted, but
    // it remembers that it was originally encrypted.
    let loaded = Document::load_with_password(&path, "user").unwrap();
    assert!(!loaded.is_encrypted(), "Should not appear encrypted after decryption");
    assert!(loaded.was_encrypted(), "Should remember it was originally encrypted");

    let loaded_locked = Document::load(&path).unwrap();
    assert!(
        loaded_locked.is_encrypted(),
        "Should still appear encrypted without password"
    );
    assert!(
        !loaded_locked.was_encrypted(),
        "encryption_state not set when auth failed"
    );
}
