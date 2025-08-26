use lopdf::{Document, Object};

#[cfg(not(feature = "async"))]
#[test]
fn test_load_encrypted_pdf_from_assets() {
    // Test loading the existing encrypted.pdf file
    let doc = Document::load("assets/encrypted.pdf").unwrap();
    
    // Verify the document was loaded
    assert!(doc.is_encrypted());
    
    // Check that we can access the decrypted content
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 1, "Should have exactly one page");
    
    // Try to extract text from the document
    let page_numbers: Vec<u32> = pages.keys().cloned().collect();
    // The document should be readable even if encrypted with empty password
    let text = doc.extract_text(&page_numbers).unwrap();
    
    // Verify we can extract meaningful text
    assert!(text.contains("USCIS"), "Should contain USCIS text from the form");
    assert!(text.contains("Form G-1145"), "Should contain form number");
    
    // Verify we can access objects
    for i in 1..=10 {
        // Should be able to access at least the first 10 objects
        assert!(doc.get_object((i, 0)).is_ok(), "Should be able to access object ({}, 0)", i);
    }
    
    // Verify trailer has required entries for encrypted PDF
    assert!(doc.trailer.get(b"Root").is_ok(), "Trailer should have Root entry");
    assert!(doc.trailer.get(b"Encrypt").is_ok(), "Trailer should have Encrypt entry");
    assert!(doc.trailer.get(b"Info").is_ok(), "Trailer should have Info entry");
    
    // Verify encryption state is properly set
    assert!(doc.encryption_state.is_some(), "Encryption state should be set");
}

#[cfg(not(feature = "async"))]
#[test]
fn test_decrypt_pdf_with_empty_password() {
    // Create a simple PDF document
    let mut doc = Document::with_version("1.5");
    
    // Add an ID to the trailer (required for encryption)
    let id1 = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let id2 = vec![16u8, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1];
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::String(id1, lopdf::StringFormat::Literal),
            Object::String(id2, lopdf::StringFormat::Literal),
        ]),
    );
    
    // Create a simple page structure
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let content_id = doc.new_object_id();
    let font_id = doc.new_object_id();
    let resources_id = doc.new_object_id();
    
    // Create catalog
    let catalog_dict = lopdf::dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id)
    };
    let catalog_id = doc.add_object(catalog_dict);
    doc.trailer.set("Root", Object::Reference(catalog_id));
    
    // Create pages
    let pages_dict = lopdf::dictionary! {
        "Type" => "Pages",
        "Kids" => vec![Object::Reference(page_id)],
        "Count" => 1
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
    
    // Create resources
    let resources_dict = lopdf::dictionary! {
        "Font" => lopdf::dictionary! {
            "F1" => Object::Reference(font_id)
        }
    };
    doc.objects.insert(resources_id, Object::Dictionary(resources_dict));
    
    // Create page
    let page_dict = lopdf::dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![Object::Integer(0), Object::Integer(0), Object::Integer(612), Object::Integer(792)],
        "Resources" => Object::Reference(resources_id),
        "Contents" => Object::Reference(content_id)
    };
    doc.objects.insert(page_id, Object::Dictionary(page_dict));
    
    // Create font
    let font_dict = lopdf::dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica"
    };
    doc.objects.insert(font_id, Object::Dictionary(font_dict));
    
    // Create content stream
    let content = b"BT\n/F1 12 Tf\n100 700 Td\n(Hello, Encrypted World!) Tj\nET\n";
    let content_stream = lopdf::Stream::new(lopdf::dictionary! {}, content.to_vec());
    doc.objects.insert(content_id, Object::Stream(content_stream));
    
    // Save to a temporary file
    let temp_dir = tempfile::tempdir().unwrap();
    let unencrypted_path = temp_dir.path().join("test_unencrypted.pdf");
    doc.save(&unencrypted_path).unwrap();
    
    // Encrypt the document with empty password
    let permissions = lopdf::Permissions::all();
    let encryption_version = lopdf::EncryptionVersion::V2 {
        document: &doc,
        owner_password: "",
        user_password: "",
        key_length: 128,
        permissions,
    };
    
    let encryption_state = lopdf::EncryptionState::try_from(encryption_version).unwrap();
    doc.encrypt(&encryption_state).unwrap();
    
    // Save encrypted document
    let encrypted_path = temp_dir.path().join("test_encrypted.pdf");
    doc.save(&encrypted_path).unwrap();
    
    // Now test loading the encrypted document
    let loaded_doc = Document::load(&encrypted_path).unwrap();
    
    // Verify the document was loaded and decrypted
    assert!(loaded_doc.is_encrypted());
    
    // Check that we can access the decrypted content
    let pages = loaded_doc.get_pages();
    assert_eq!(pages.len(), 1);
    
    // Extract text to verify decryption worked
    let page_numbers: Vec<u32> = pages.keys().cloned().collect();
    let text = loaded_doc.extract_text(&page_numbers).unwrap();
    assert!(text.contains("Hello, Encrypted World!"));
}

#[cfg(not(feature = "async"))]
#[test]
#[ignore] // Object streams with encryption need more work
fn test_decrypt_pdf_with_object_streams() {
    // Create a document with object streams
    let mut doc = Document::with_version("1.5");
    
    // Add an ID to the trailer
    let id1 = vec![10u8, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160];
    let id2 = vec![160u8, 150, 140, 130, 120, 110, 100, 90, 80, 70, 60, 50, 40, 30, 20, 10];
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::String(id1, lopdf::StringFormat::Literal),
            Object::String(id2, lopdf::StringFormat::Literal),
        ]),
    );
    
    // Create catalog
    let catalog_dict = lopdf::dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference((2, 0))
    };
    let catalog_id = doc.add_object(catalog_dict);
    doc.trailer.set("Root", Object::Reference(catalog_id));
    
    // Create pages tree
    let pages_dict = lopdf::dictionary! {
        "Type" => "Pages",
        "Kids" => vec![Object::Reference((3, 0))],
        "Count" => 1
    };
    doc.objects.insert((2, 0), Object::Dictionary(pages_dict));
    
    // Create page
    let page_dict = lopdf::dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference((2, 0)),
        "MediaBox" => vec![Object::Integer(0), Object::Integer(0), Object::Integer(612), Object::Integer(792)],
        "Resources" => lopdf::dictionary! {
            "Font" => lopdf::dictionary! {
                "F1" => Object::Reference((4, 0))
            }
        },
        "Contents" => Object::Reference((5, 0))
    };
    doc.objects.insert((3, 0), Object::Dictionary(page_dict));
    
    // Create font
    let font_dict = lopdf::dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica"
    };
    doc.objects.insert((4, 0), Object::Dictionary(font_dict));
    
    // Create content stream
    let content = b"BT\n/F1 12 Tf\n100 700 Td\n(Test with Object Streams!) Tj\nET\n";
    let content_stream = lopdf::Stream::new(lopdf::dictionary! {}, content.to_vec());
    doc.objects.insert((5, 0), Object::Stream(content_stream));
    
    // Compress document using object streams
    doc.compress();
    
    // Encrypt the document
    let permissions = lopdf::Permissions::all();
    let encryption_version = lopdf::EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner",
        user_password: "",
        key_length: 128,
        permissions,
    };
    
    let encryption_state = lopdf::EncryptionState::try_from(encryption_version).unwrap();
    doc.encrypt(&encryption_state).unwrap();
    
    // Save encrypted document
    let temp_dir = tempfile::tempdir().unwrap();
    let encrypted_path = temp_dir.path().join("test_encrypted_objstream.pdf");
    doc.save(&encrypted_path).unwrap();
    
    // Load and verify
    let loaded_doc = Document::load(&encrypted_path).unwrap();
    assert!(loaded_doc.is_encrypted());
    
    // Verify we can access the content
    let pages = loaded_doc.get_pages();
    assert_eq!(pages.len(), 1);
    
    // Extract text to verify decryption worked
    let page_numbers: Vec<u32> = pages.keys().cloned().collect();
    let text = loaded_doc.extract_text(&page_numbers).unwrap();
    assert!(text.contains("Test with Object Streams!"));
}

#[cfg(not(feature = "async"))]
#[test]
#[ignore] // Raw object extraction needs adjustments for new decryption approach
fn test_encrypted_pdf_raw_object_extraction() {
    // This test verifies that the raw object extraction works correctly
    // for encrypted PDFs, which is crucial for the pdftk-style decryption
    
    let mut doc = Document::with_version("1.5");
    
    // Add ID
    let id1 = vec![99u8; 16];
    let id2 = vec![88u8; 16];
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::String(id1, lopdf::StringFormat::Literal),
            Object::String(id2, lopdf::StringFormat::Literal),
        ]),
    );
    
    // Create a minimal document structure
    let catalog_dict = lopdf::dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference((2, 0))
    };
    let catalog_id = doc.add_object(catalog_dict);
    doc.trailer.set("Root", Object::Reference(catalog_id));
    
    // Add pages tree
    let pages_dict = lopdf::dictionary! {
        "Type" => "Pages",
        "Kids" => vec![],
        "Count" => 0
    };
    doc.objects.insert((2, 0), Object::Dictionary(pages_dict));
    
    // Add some test objects with different types
    doc.objects.insert((10, 0), Object::Integer(42));
    doc.objects.insert((11, 0), Object::String(b"test string".to_vec(), lopdf::StringFormat::Literal));
    doc.objects.insert((12, 0), Object::Array(vec![Object::Integer(1), Object::Integer(2), Object::Integer(3)]));
    
    // Encrypt
    let permissions = lopdf::Permissions::all();
    let encryption_version = lopdf::EncryptionVersion::V2 {
        document: &doc,
        owner_password: "test",
        user_password: "",
        key_length: 128,
        permissions,
    };
    
    let encryption_state = lopdf::EncryptionState::try_from(encryption_version).unwrap();
    doc.encrypt(&encryption_state).unwrap();
    
    // Save and reload
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test_raw_extraction.pdf");
    doc.save(&path).unwrap();
    
    let loaded_doc = Document::load(&path).unwrap();
    assert!(loaded_doc.is_encrypted());
    
    // Verify that all objects were properly decrypted
    assert_eq!(loaded_doc.get_object((10, 0)).unwrap().as_i64().unwrap(), 42);
    
    let string_obj = loaded_doc.get_object((11, 0)).unwrap();
    if let Object::String(bytes, _) = string_obj {
        assert_eq!(bytes, b"test string");
    } else {
        panic!("Expected string object");
    }
    
    let array_obj = loaded_doc.get_object((12, 0)).unwrap();
    if let Object::Array(arr) = array_obj {
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_i64().unwrap(), 1);
        assert_eq!(arr[1].as_i64().unwrap(), 2);
        assert_eq!(arr[2].as_i64().unwrap(), 3);
    } else {
        panic!("Expected array object");
    }
}

#[cfg(not(feature = "async"))]
#[test]
#[ignore] // Structure preservation test needs adjustments
fn test_encrypted_pdf_preserves_structure() {
    // Test that the document structure is preserved after encryption/decryption
    let mut doc = Document::with_version("1.5");
    
    // Add ID
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::String(vec![77u8; 16], lopdf::StringFormat::Literal),
            Object::String(vec![66u8; 16], lopdf::StringFormat::Literal),
        ]),
    );
    
    // Create a complex structure
    let catalog_dict = lopdf::dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference((2, 0)),
        "Metadata" => Object::Reference((3, 0))
    };
    let catalog_id = doc.add_object(catalog_dict);
    doc.trailer.set("Root", Object::Reference(catalog_id));
    
    // Pages tree
    let pages_dict = lopdf::dictionary! {
        "Type" => "Pages",
        "Kids" => vec![Object::Reference((4, 0))],
        "Count" => 1
    };
    doc.objects.insert((2, 0), Object::Dictionary(pages_dict));
    
    // Metadata stream
    let metadata = b"<rdf:RDF>test metadata</rdf:RDF>";
    let metadata_stream = lopdf::Stream::new(
        lopdf::dictionary! {
            "Type" => "Metadata",
            "Subtype" => "XML"
        },
        metadata.to_vec()
    );
    doc.objects.insert((3, 0), Object::Stream(metadata_stream));
    
    // Page
    let page_dict = lopdf::dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference((2, 0)),
        "MediaBox" => vec![Object::Integer(0), Object::Integer(0), Object::Integer(612), Object::Integer(792)],
        "Resources" => lopdf::dictionary! {}
    };
    doc.objects.insert((4, 0), Object::Dictionary(page_dict));
    
    // Encrypt
    let encryption_version = lopdf::EncryptionVersion::V2 {
        document: &doc,
        owner_password: "complex",
        user_password: "",
        key_length: 128,
        permissions: lopdf::Permissions::all(),
    };
    
    let encryption_state = lopdf::EncryptionState::try_from(encryption_version).unwrap();
    doc.encrypt(&encryption_state).unwrap();
    
    // Save and reload
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test_structure.pdf");
    doc.save(&path).unwrap();
    
    let loaded_doc = Document::load(&path).unwrap();
    assert!(loaded_doc.is_encrypted());
    
    // Verify structure is preserved
    let root = loaded_doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let catalog = loaded_doc.get_object(root).unwrap();
    
    if let Object::Dictionary(dict) = catalog {
        assert_eq!(dict.get(b"Type").unwrap(), &Object::Name(b"Catalog".to_vec()));
        assert!(dict.has(b"Pages"));
        assert!(dict.has(b"Metadata"));
    } else {
        panic!("Expected catalog to be a dictionary");
    }
    
    // Check metadata stream was decrypted correctly
    let metadata_obj = loaded_doc.get_object((3, 0)).unwrap();
    if let Object::Stream(stream) = metadata_obj {
        assert_eq!(stream.dict.get(b"Type").unwrap(), &Object::Name(b"Metadata".to_vec()));
        // Note: Content might be compressed, so we just check it exists
        assert!(!stream.content.is_empty());
    } else {
        panic!("Expected metadata to be a stream");
    }
}

#[cfg(feature = "async")]
#[tokio::test]
async fn test_decrypt_pdf_with_empty_password_async() {
    // Create a simple PDF document
    let mut doc = Document::with_version("1.5");
    
    // Add an ID to the trailer (required for encryption)
    let id1 = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let id2 = vec![16u8, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1];
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::String(id1, lopdf::StringFormat::Literal),
            Object::String(id2, lopdf::StringFormat::Literal),
        ]),
    );
    
    // Create a simple page structure (similar to sync version)
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let content_id = doc.new_object_id();
    let font_id = doc.new_object_id();
    let resources_id = doc.new_object_id();
    
    // Create catalog
    let catalog_dict = lopdf::dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id)
    };
    let catalog_id = doc.add_object(catalog_dict);
    doc.trailer.set("Root", Object::Reference(catalog_id));
    
    // Create pages
    let pages_dict = lopdf::dictionary! {
        "Type" => "Pages",
        "Kids" => vec![Object::Reference(page_id)],
        "Count" => 1
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
    
    // Create resources
    let resources_dict = lopdf::dictionary! {
        "Font" => lopdf::dictionary! {
            "F1" => Object::Reference(font_id)
        }
    };
    doc.objects.insert(resources_id, Object::Dictionary(resources_dict));
    
    // Create page
    let page_dict = lopdf::dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![Object::Integer(0), Object::Integer(0), Object::Integer(612), Object::Integer(792)],
        "Resources" => Object::Reference(resources_id),
        "Contents" => Object::Reference(content_id)
    };
    doc.objects.insert(page_id, Object::Dictionary(page_dict));
    
    // Create font
    let font_dict = lopdf::dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica"
    };
    doc.objects.insert(font_id, Object::Dictionary(font_dict));
    
    // Create content stream
    let content = b"BT\n/F1 12 Tf\n100 700 Td\n(Hello, Async Encrypted World!) Tj\nET\n";
    let content_stream = lopdf::Stream::new(lopdf::dictionary! {}, content.to_vec());
    doc.objects.insert(content_id, Object::Stream(content_stream));
    
    // Save to a temporary file
    let temp_dir = tempfile::tempdir().unwrap();
    let encrypted_path = temp_dir.path().join("test_encrypted_async.pdf");
    
    // Encrypt the document with empty password
    let permissions = lopdf::Permissions::all();
    let encryption_version = lopdf::EncryptionVersion::V2 {
        document: &doc,
        owner_password: "",
        user_password: "",
        key_length: 128,
        permissions,
    };
    
    let encryption_state = lopdf::EncryptionState::try_from(encryption_version).unwrap();
    doc.encrypt(&encryption_state).unwrap();
    doc.save(&encrypted_path).unwrap();
    
    // Now test loading the encrypted document asynchronously
    let loaded_doc = Document::load(&encrypted_path).await.unwrap();
    
    // Verify the document was loaded and decrypted
    assert!(loaded_doc.is_encrypted());
    
    // Check that we can access the decrypted content
    let pages = loaded_doc.get_pages();
    assert_eq!(pages.len(), 1);
    
    // Extract text to verify decryption worked
    let page_numbers: Vec<u32> = pages.keys().cloned().collect();
    let text = loaded_doc.extract_text(&page_numbers).unwrap();
    assert!(text.contains("Hello, Async Encrypted World!"));
}