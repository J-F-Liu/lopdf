use lopdf::{dictionary, Document, Object, ObjectStream, Stream};

#[test]
fn test_object_stream_builder() {
    let builder = ObjectStream::builder();
    assert_eq!(builder.get_max_objects(), 100);
    assert_eq!(builder.get_compression_level(), 6);
}

#[test]
fn test_object_stream_add_and_build() {
    let mut obj_stream = ObjectStream::builder().build();
    
    // Add some objects
    obj_stream.add_object((1, 0), Object::Integer(42)).unwrap();
    obj_stream.add_object((2, 0), Object::Boolean(true)).unwrap();
    obj_stream.add_object((3, 0), Object::Name(b"Test".to_vec())).unwrap();
    
    // Build the stream content
    let content = obj_stream.build_stream_content().unwrap();
    assert!(!content.is_empty());
    
    // Convert to stream object
    let stream_obj = obj_stream.to_stream_object().unwrap();
    assert_eq!(stream_obj.dict.get(b"Type").unwrap(), &Object::Name(b"ObjStm".to_vec()));
    assert_eq!(stream_obj.dict.get(b"N").unwrap(), &Object::Integer(3));
}

#[test]
fn test_save_with_object_streams() {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica"
    });
    
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    });
    
    let content = lopdf::content::Content {
        operations: vec![
            lopdf::content::Operation::new("BT", vec![]),
            lopdf::content::Operation::new("Tf", vec!["F1".into(), 12.into()]),
            lopdf::content::Operation::new("Td", vec![50.into(), 700.into()]),
            lopdf::content::Operation::new("Tj", vec![Object::string_literal("Test Document")]),
            lopdf::content::Operation::new("ET", vec![]),
        ],
    };
    
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        content.encode().unwrap()
    ));
    
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    }));
    
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    
    doc.trailer.set("Root", catalog_id);
    
    // Test save_modern
    let mut buffer = Vec::new();
    let result = doc.save_modern(&mut buffer);
    assert!(result.is_ok());
    
    // Verify PDF was created
    let content = String::from_utf8_lossy(&buffer);
    assert!(content.starts_with("%PDF-1.5"));
    
    // Verify object streams were created
    assert!(content.contains("/ObjStm"), "Object streams should be created");
    
    // Verify that structural objects are NOT present as individual objects
    assert!(!content.contains("2 0 obj\n<</Type/Pages"), "Pages object should be compressed");
    assert!(!content.contains("3 0 obj\n<</Type/Page"), "Page object should be compressed");
}