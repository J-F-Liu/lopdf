use lopdf::{Document, Object, SaveOptions, dictionary};

#[test]
fn test_catalog_included_in_object_stream_output() {
    // Create a simple PDF
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });

    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });

    let info_id = doc.add_object(dictionary! {
        "Title" => "Integration Test PDF",
        "Author" => "lopdf test suite",
    });

    doc.trailer.set("Root", catalog_id);
    doc.trailer.set("Info", info_id);

    // Save with object streams
    let options = SaveOptions::builder()
        .use_object_streams(true)
        .use_xref_streams(true)
        .build();

    let mut output = Vec::new();
    doc.save_with_options(&mut output, options).unwrap();

    // Parse the output to verify catalog is in object stream
    let saved_doc = Document::load_mem(&output).unwrap();

    // Count object streams
    let obj_stream_count = saved_doc
        .objects
        .iter()
        .filter(|(_, obj)| {
            if let Object::Stream(s) = obj {
                s.dict.get(b"Type").ok() == Some(&Object::Name(b"ObjStm".to_vec()))
            } else {
                false
            }
        })
        .count();

    // Verify object streams were created and that the catalog went into one
    // rather than being written as an individual object.
    assert!(obj_stream_count > 0, "Should have created object streams");

    let content = String::from_utf8_lossy(&output);
    assert!(
        !content.contains(&format!("{} 0 obj\n<</Type/Catalog", catalog_id.0)),
        "Catalog should be in object stream, not as individual object"
    );
    assert!(
        !content.contains(&format!("{} 0 obj\n<</Title", info_id.0)),
        "Info dictionary should be in object stream, not as individual object"
    );
}
