use lopdf::Document;

#[test]
fn test_metadata_extraction_basic() {
    let buffer = std::fs::read("assets/example.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();

    assert_eq!(metadata.version, "1.5");
    assert!(metadata.page_count > 0);
}

#[test]
fn test_metadata_extraction_page_count() {
    let buffer = std::fs::read("assets/example.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();
    assert!(metadata.page_count > 0);

    let buffer = std::fs::read("assets/example.pdf").unwrap();
    let doc = Document::load_mem(&buffer).unwrap();
    let pages = doc.get_pages();
    assert_eq!(metadata.page_count, pages.len() as u32);
}

#[test]
fn test_metadata_extraction_unicode() {
    let buffer = std::fs::read("assets/unicode.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();
    assert!(metadata.page_count > 0);
}

#[test]
fn test_metadata_extraction_from_memory() {
    let buffer = std::fs::read("assets/example.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();

    assert_eq!(metadata.version, "1.5");
    assert!(metadata.page_count > 0);
}

#[test]
fn test_metadata_extraction_incremental() {
    let buffer = std::fs::read("assets/Incremental.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();
    assert!(metadata.page_count > 0);
}

#[test]
fn test_metadata_extraction_annotation_demo() {
    let buffer = std::fs::read("assets/AnnotationDemo.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();
    assert!(metadata.page_count > 0);
}

#[cfg(not(feature = "async"))]
#[test]
fn test_metadata_extraction_encrypted_empty_password() {
    let mut doc = Document::with_version("1.5");

    let id1 = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let id2 = vec![16u8, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1];
    doc.trailer.set(
        "ID",
        lopdf::Object::Array(vec![
            lopdf::Object::String(id1, lopdf::StringFormat::Literal),
            lopdf::Object::String(id2, lopdf::StringFormat::Literal),
        ]),
    );

    let catalog_dict = lopdf::dictionary! {
        "Type" => "Catalog",
        "Pages" => lopdf::Object::Reference((2, 0))
    };
    let catalog_id = doc.add_object(catalog_dict);
    doc.trailer.set("Root", lopdf::Object::Reference(catalog_id));

    let pages_dict = lopdf::dictionary! {
        "Type" => "Pages",
        "Kids" => vec![lopdf::Object::Reference((3, 0))],
        "Count" => 1
    };
    doc.objects.insert((2, 0), lopdf::Object::Dictionary(pages_dict));

    let page_dict = lopdf::dictionary! {
        "Type" => "Page",
        "Parent" => lopdf::Object::Reference((2, 0)),
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()]
    };
    doc.objects.insert((3, 0), lopdf::Object::Dictionary(page_dict));

    let info_dict = lopdf::dictionary! {
        "Title" => lopdf::Object::String(b"Test Encrypted PDF".to_vec(), lopdf::StringFormat::Literal),
        "Author" => lopdf::Object::String(b"Test Author".to_vec(), lopdf::StringFormat::Literal),
        "Subject" => lopdf::Object::String(b"Test Subject".to_vec(), lopdf::StringFormat::Literal)
    };
    let info_id = doc.add_object(info_dict);
    doc.trailer.set("Info", lopdf::Object::Reference(info_id));

    let encryption_version = lopdf::EncryptionVersion::V2 {
        document: &doc,
        owner_password: "",
        user_password: "",
        key_length: 128,
        permissions: lopdf::Permissions::all(),
    };

    let encryption_state = lopdf::EncryptionState::try_from(encryption_version).unwrap();
    doc.encrypt(&encryption_state).unwrap();

    let temp_dir = tempfile::tempdir().unwrap();
    let encrypted_path = temp_dir.path().join("test_encrypted_metadata.pdf");
    doc.save(&encrypted_path).unwrap();

    let metadata = Document::load_metadata(&encrypted_path).unwrap();
    assert_eq!(metadata.title, Some("Test Encrypted PDF".to_string()));
    assert_eq!(metadata.author, Some("Test Author".to_string()));
    assert_eq!(metadata.subject, Some("Test Subject".to_string()));
    assert_eq!(metadata.page_count, 1);
}

#[cfg(not(feature = "async"))]
#[test]
fn test_metadata_extraction_encrypted_with_password() {
    let mut doc = Document::with_version("1.5");

    let id1 = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let id2 = vec![16u8, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1];
    doc.trailer.set(
        "ID",
        lopdf::Object::Array(vec![
            lopdf::Object::String(id1, lopdf::StringFormat::Literal),
            lopdf::Object::String(id2, lopdf::StringFormat::Literal),
        ]),
    );

    let catalog_dict = lopdf::dictionary! {
        "Type" => "Catalog",
        "Pages" => lopdf::Object::Reference((2, 0))
    };
    let catalog_id = doc.add_object(catalog_dict);
    doc.trailer.set("Root", lopdf::Object::Reference(catalog_id));

    let pages_dict = lopdf::dictionary! {
        "Type" => "Pages",
        "Kids" => vec![lopdf::Object::Reference((3, 0))],
        "Count" => 1
    };
    doc.objects.insert((2, 0), lopdf::Object::Dictionary(pages_dict));

    let page_dict = lopdf::dictionary! {
        "Type" => "Page",
        "Parent" => lopdf::Object::Reference((2, 0)),
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()]
    };
    doc.objects.insert((3, 0), lopdf::Object::Dictionary(page_dict));

    let info_dict = lopdf::dictionary! {
        "Title" => lopdf::Object::String(b"Password Protected PDF".to_vec(), lopdf::StringFormat::Literal),
        "Author" => lopdf::Object::String(b"Protected Author".to_vec(), lopdf::StringFormat::Literal)
    };
    let info_id = doc.add_object(info_dict);
    doc.trailer.set("Info", lopdf::Object::Reference(info_id));

    let encryption_version = lopdf::EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner_pass",
        user_password: "user_pass",
        key_length: 128,
        permissions: lopdf::Permissions::all(),
    };

    let encryption_state = lopdf::EncryptionState::try_from(encryption_version).unwrap();
    doc.encrypt(&encryption_state).unwrap();

    let temp_dir = tempfile::tempdir().unwrap();
    let encrypted_path = temp_dir.path().join("test_password_metadata.pdf");
    doc.save(&encrypted_path).unwrap();

    let metadata = Document::load_metadata_with_password(&encrypted_path, "user_pass").unwrap();
    assert_eq!(metadata.title, Some("Password Protected PDF".to_string()));
    assert_eq!(metadata.author, Some("Protected Author".to_string()));
    assert_eq!(metadata.page_count, 1);

    let buffer = std::fs::read(&encrypted_path).unwrap();
    let metadata_mem = Document::load_metadata_mem_with_password(&buffer, "user_pass").unwrap();
    assert_eq!(metadata_mem.title, Some("Password Protected PDF".to_string()));
    assert_eq!(metadata_mem.author, Some("Protected Author".to_string()));
    assert_eq!(metadata_mem.page_count, 1);
}

#[cfg(not(feature = "async"))]
#[test]
fn test_metadata_extraction_encrypted_wrong_password() {
    let mut doc = Document::with_version("1.5");

    let id1 = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let id2 = vec![16u8, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1];
    doc.trailer.set(
        "ID",
        lopdf::Object::Array(vec![
            lopdf::Object::String(id1, lopdf::StringFormat::Literal),
            lopdf::Object::String(id2, lopdf::StringFormat::Literal),
        ]),
    );

    let catalog_dict = lopdf::dictionary! {
        "Type" => "Catalog",
        "Pages" => lopdf::Object::Reference((2, 0))
    };
    let catalog_id = doc.add_object(catalog_dict);
    doc.trailer.set("Root", lopdf::Object::Reference(catalog_id));

    let pages_dict = lopdf::dictionary! {
        "Type" => "Pages",
        "Kids" => vec![lopdf::Object::Reference((3, 0))],
        "Count" => 1
    };
    doc.objects.insert((2, 0), lopdf::Object::Dictionary(pages_dict));

    let page_dict = lopdf::dictionary! {
        "Type" => "Page",
        "Parent" => lopdf::Object::Reference((2, 0)),
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()]
    };
    doc.objects.insert((3, 0), lopdf::Object::Dictionary(page_dict));

    let encryption_version = lopdf::EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner",
        user_password: "user",
        key_length: 128,
        permissions: lopdf::Permissions::all(),
    };

    let encryption_state = lopdf::EncryptionState::try_from(encryption_version).unwrap();
    doc.encrypt(&encryption_state).unwrap();

    let temp_dir = tempfile::tempdir().unwrap();
    let encrypted_path = temp_dir.path().join("test_wrong_password.pdf");
    doc.save(&encrypted_path).unwrap();

    let result = Document::load_metadata_with_password(&encrypted_path, "wrong_password");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), lopdf::Error::InvalidPassword));
}
