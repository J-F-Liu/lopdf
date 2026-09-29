use lopdf::{Document, Object, ObjectStream, dictionary};

/// Trailer entries, and what they say about the document they describe.
type Variant = (&'static str, Vec<(Vec<u8>, Object)>);

/// Nothing the trailer may hold can keep a plain dictionary out of an object
/// stream: no entries at all, an `/Encrypt` that points elsewhere or holds no
/// reference, entries of every object type, or non-ASCII keys.
#[test]
fn trailer_contents_never_block_compression() {
    let variants: [Variant; 5] = [
        ("an empty trailer", vec![]),
        (
            "an /Encrypt reference to another object",
            vec![(b"Encrypt".to_vec(), Object::Reference((999, 99)))],
        ),
        (
            "entries of every type",
            vec![
                (b"Null".to_vec(), Object::Null),
                (b"Bool".to_vec(), Object::Boolean(true)),
                (b"Int".to_vec(), Object::Integer(42)),
                (b"Real".to_vec(), Object::Real(1.25)),
                (
                    b"String".to_vec(),
                    Object::String(b"test".to_vec(), lopdf::StringFormat::Literal),
                ),
                (b"Name".to_vec(), Object::Name(b"Test".to_vec())),
                (b"Array".to_vec(), Object::Array(vec![Object::Integer(1)])),
                (b"Dict".to_vec(), Object::Dictionary(dictionary! { "Key" => "Value" })),
            ],
        ),
        (
            "non-ASCII keys",
            vec![
                (
                    "Ünïcödé".as_bytes().to_vec(),
                    Object::String(b"test".to_vec(), lopdf::StringFormat::Literal),
                ),
                ("日本語".as_bytes().to_vec(), Object::Integer(42)),
                ("🎯".as_bytes().to_vec(), Object::Boolean(true)),
            ],
        ),
        (
            "1000 custom entries",
            (0..1000)
                .map(|i| (format!("Custom{i:04}").into_bytes(), Object::Integer(i)))
                .collect(),
        ),
    ];

    for (label, entries) in variants {
        let mut doc = Document::with_version("1.5");
        let obj_id = doc.add_object(dictionary! { "Type" => "Test" });
        doc.trailer.set("Root", obj_id);
        for (key, value) in entries {
            doc.trailer.set(key, value);
        }

        assert!(
            ObjectStream::can_be_compressed(obj_id, doc.objects.get(&obj_id).unwrap(), &doc),
            "the object should stay compressible with {label}"
        );
    }
}

#[test]
fn test_self_referencing_object() {
    let mut doc = Document::with_version("1.5");

    let obj_id = (5, 0);
    doc.objects.insert(
        obj_id,
        Object::Dictionary(dictionary! {
            "Type" => "Test",
            "Self" => Object::Reference(obj_id)  // Self reference
        }),
    );

    doc.trailer.set("Test", obj_id);

    // Should be compressible (not encryption dict)
    assert!(
        ObjectStream::can_be_compressed(obj_id, doc.objects.get(&obj_id).unwrap(), &doc),
        "Self-referencing object should be compressible"
    );
}

#[test]
fn test_circular_references() {
    let mut doc = Document::with_version("1.5");

    let obj1_id = (1, 0);
    let obj2_id = (2, 0);

    doc.objects.insert(
        obj1_id,
        Object::Dictionary(dictionary! {
            "Next" => Object::Reference(obj2_id)
        }),
    );

    doc.objects.insert(
        obj2_id,
        Object::Dictionary(dictionary! {
            "Next" => Object::Reference(obj1_id)  // Circular reference
        }),
    );

    doc.trailer.set("Start", obj1_id);

    // Both should be compressible
    assert!(
        ObjectStream::can_be_compressed(obj1_id, doc.objects.get(&obj1_id).unwrap(), &doc),
        "First object in circular reference should be compressible"
    );
    assert!(
        ObjectStream::can_be_compressed(obj2_id, doc.objects.get(&obj2_id).unwrap(), &doc),
        "Second object in circular reference should be compressible"
    );
}
