use lopdf::{Document, Object, ObjectStream, Stream, StringFormat, dictionary};

/// The rules that keep an object out of object streams: streams themselves,
/// objects with a non-zero generation, the dictionary the trailer points at
/// with `/Encrypt`, the cross-reference and object streams themselves, and the
/// catalog of a linearized document.
#[test]
fn can_be_compressed_excludes_the_documented_objects() {
    let mut doc = Document::new();

    let stream_id = doc.add_object(Stream::new(dictionary! { "Type" => "XObject" }, vec![1, 2, 3]));
    let xref_id = doc.add_object(Object::Dictionary(dictionary! { "Type" => "XRef" }));
    let objstm_id = doc.add_object(Object::Dictionary(dictionary! { "Type" => "ObjStm" }));
    let encrypt_id = doc.add_object(Object::Dictionary(dictionary! {
        "Filter" => "Standard",
        "V" => 1,
        "R" => 2,
    }));
    let generation_id = (1, 5);
    doc.objects.insert(generation_id, Object::Integer(42));
    let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference((2, 0)),
    }));

    doc.trailer.set("Encrypt", encrypt_id);
    // Any dictionary with a `/Linearized` entry marks the document linearized,
    // which in turn excludes the catalog.
    doc.add_object(Object::Dictionary(dictionary! { "Linearized" => 1 }));

    for id in [stream_id, xref_id, objstm_id, encrypt_id, generation_id, catalog_id] {
        let object = doc.objects.get(&id).unwrap();
        assert!(
            !ObjectStream::can_be_compressed(id, object, &doc),
            "{id:?} must not be compressed"
        );
    }

    // The catalog of a document that is not linearized is compressible.
    let mut doc = Document::new();
    let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference((2, 0)),
    }));
    let catalog = doc.objects.get(&catalog_id).unwrap();
    assert!(ObjectStream::can_be_compressed(catalog_id, catalog, &doc));
}

/// Every other object is compressible: the plain object types as well as the
/// structural dictionaries a trailer points at.
#[test]
fn can_be_compressed_accepts_everything_else() {
    let mut doc = Document::with_version("1.5");

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference((2, 0)),
    });
    let pages_id = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => vec![Object::Reference((3, 0))],
        "Count" => 1,
    });
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    let info_id = doc.add_object(dictionary! {
        "Title" => "Test PDF",
        "Author" => "Test Author",
        "CreationDate" => "D:20250807120000Z",
    });

    doc.trailer.set("Root", catalog_id);
    doc.trailer.set("Info", info_id);

    let ordinary = [
        doc.add_object(Object::Integer(42)),
        doc.add_object(Object::Boolean(true)),
        doc.add_object(Object::String(b"Hello".to_vec(), StringFormat::Literal)),
        doc.add_object(Object::Name(b"Test".to_vec())),
        doc.add_object(Object::Array(vec![Object::Integer(1), Object::Integer(2)])),
        doc.add_object(dictionary! { "Key" => "Value" }),
    ];

    for id in ordinary.into_iter().chain([catalog_id, pages_id, page_id, info_id]) {
        let object = doc.objects.get(&id).unwrap();
        assert!(
            ObjectStream::can_be_compressed(id, object, &doc),
            "{id:?} must be compressed"
        );
    }
}

#[test]
fn test_object_stream_builder_custom_config() {
    let builder = ObjectStream::builder().max_objects(50).compression_level(9);

    assert_eq!(builder.get_max_objects(), 50);
    assert_eq!(builder.get_compression_level(), 9);
}

#[test]
fn test_object_stream_add_max_objects() {
    let mut obj_stream = ObjectStream::builder().max_objects(3).build();

    assert!(obj_stream.add_object((1, 0), Object::Integer(1)).is_ok());
    assert!(obj_stream.add_object((2, 0), Object::Integer(2)).is_ok());
    assert!(obj_stream.add_object((3, 0), Object::Integer(3)).is_ok());

    // Should fail when exceeding max
    assert!(obj_stream.add_object((4, 0), Object::Integer(4)).is_err());
}

#[test]
fn test_parse_existing_object_stream() {
    // Create a simple object stream content
    let content = b"1 0 2 50 3 100\n\
                    <</Type/Font/Subtype/Type1/BaseFont/Helvetica>>\n\
                    <</Type/Annot/Subtype/Text/Rect[100 100 200 200]>>\n\
                    42";

    let stream = Stream::new(
        dictionary! {
            "Type" => "ObjStm",
            "N" => 3,
            "First" => 15
        },
        content.to_vec(),
    );

    let obj_stream = ObjectStream::new(&stream).unwrap();
    assert_eq!(obj_stream.objects.len(), 3);
    assert!(obj_stream.objects.contains_key(&(1, 0)));
    assert!(obj_stream.objects.contains_key(&(2, 0)));
    assert!(obj_stream.objects.contains_key(&(3, 0)));
}

/// Saving in the modern format packs the compressible objects into object
/// streams while streams stay top-level, and the result still loads.
#[test]
fn test_save_modern_packs_objects_into_object_streams() {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let xobject_id = doc.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Image" },
        vec![0; 100],
    ));
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
    doc.trailer.set("Root", catalog_id);

    let mut buffer = Vec::new();
    doc.save_modern(&mut buffer).unwrap();
    let content = String::from_utf8_lossy(&buffer);

    assert!(content.contains("/ObjStm"), "object streams should be created");
    // Compressible objects must not be written as individual objects.
    for (id, type_name) in [
        (catalog_id, "Catalog"),
        (pages_id, "Pages"),
        (page_id, "Page"),
        (font_id, "Font"),
    ] {
        assert!(
            !content.contains(&format!("{} 0 obj\n<</Type/{type_name}", id.0)),
            "{type_name} object should be in an object stream"
        );
    }
    // Stream objects are not compressible and must remain top-level.
    assert!(
        content.contains(&format!("{} 0 obj", xobject_id.0)),
        "stream objects should remain top-level objects"
    );

    let reloaded = Document::load_mem(&buffer).unwrap();
    assert_eq!(reloaded.get_pages().len(), 1);
}
