//! Saving writes the document out without changing it. The object streams and the
//! cross-reference stream that a save generates are numbered after `Document::max_id` without
//! raising it, and the trailer they need is written from a copy, so saving the same document
//! again writes the same file.

use lopdf::xref::XrefType;
use lopdf::{Document, IncrementalDocument, Object, SaveOptions, Stream, dictionary};

/// A one-page document with an `/ID`, so that two saves have nothing to differ in.
fn one_page_document(cross_reference_type: XrefType) -> Document {
    let mut doc = Document::with_version("1.5");
    doc.reference_table.cross_reference_type = cross_reference_type;
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::string_literal(b"0123456789abcdef"),
            Object::string_literal(b"fedcba9876543210"),
        ]),
    );
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, b"BT (Hello) Tj ET".to_vec()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content_id,
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
    doc
}

fn object_streams() -> SaveOptions {
    SaveOptions::builder()
        .use_object_streams(true)
        .use_xref_streams(true)
        .build()
}

fn xref_stream() -> SaveOptions {
    SaveOptions::builder().use_xref_streams(true).build()
}

/// A way to save a document, writing to a fresh buffer.
type Save = fn(&mut Document) -> Vec<u8>;

/// Every way to save a document.
fn saves() -> Vec<(&'static str, Save)> {
    vec![
        ("save_to", |doc| {
            let mut buffer = Vec::new();
            doc.save_to(&mut buffer).unwrap();
            buffer
        }),
        ("save_with_options (xref stream)", |doc| {
            let mut buffer = Vec::new();
            doc.save_with_options(&mut buffer, xref_stream()).unwrap();
            buffer
        }),
        ("save_with_options (object streams)", |doc| {
            let mut buffer = Vec::new();
            doc.save_with_options(&mut buffer, object_streams()).unwrap();
            buffer
        }),
        ("save_modern", |doc| {
            let mut buffer = Vec::new();
            doc.save_modern(&mut buffer).unwrap();
            buffer
        }),
    ]
}

#[test]
fn saving_leaves_max_id_and_the_trailer_as_they_were() {
    for cross_reference_type in [XrefType::CrossReferenceTable, XrefType::CrossReferenceStream] {
        for (name, save) in saves() {
            let mut doc = one_page_document(cross_reference_type);
            let max_id = doc.max_id;
            let trailer = doc.trailer.clone();

            save(&mut doc);

            assert_eq!(doc.max_id, max_id, "{name}, {cross_reference_type:?}");
            assert_eq!(doc.trailer, trailer, "{name}, {cross_reference_type:?}");
        }
    }
}

#[test]
fn saving_twice_writes_the_same_file() {
    for cross_reference_type in [XrefType::CrossReferenceTable, XrefType::CrossReferenceStream] {
        for (name, save) in saves() {
            let mut doc = one_page_document(cross_reference_type);
            let first = save(&mut doc);
            let second = save(&mut doc);
            assert!(first == second, "{name}, {cross_reference_type:?}");
        }
    }
}

#[test]
fn a_table_saved_after_a_cross_reference_stream_has_no_stream_entries() {
    let mut doc = one_page_document(XrefType::CrossReferenceTable);
    doc.save_with_options(&mut Vec::new(), xref_stream()).unwrap();
    doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).unwrap();

    let trailer = &buffer[buffer.windows(7).rposition(|window| window == b"trailer").unwrap()..];
    let trailer = String::from_utf8_lossy(trailer);
    for key in ["/Type", "/W", "/Index", "/Length"] {
        assert!(!trailer.contains(key), "{key} in {trailer}");
    }
}

#[test]
fn objects_added_after_a_save_take_the_next_number() {
    for (name, save) in saves() {
        let mut doc = one_page_document(XrefType::CrossReferenceStream);
        let max_id = doc.max_id;
        save(&mut doc);
        assert_eq!(doc.new_object_id(), (max_id + 1, 0), "{name}");
    }
}

#[test]
fn the_size_counts_the_generated_streams() {
    // Object streams and the cross-reference stream take the numbers after `max_id`; `/Size`
    // still has to cover them, and the file has to load with every object in place.
    let mut doc = one_page_document(XrefType::CrossReferenceStream);
    let max_id = doc.max_id;
    let mut buffer = Vec::new();
    doc.save_with_options(&mut buffer, object_streams()).unwrap();

    let loaded = Document::load_mem(&buffer).unwrap();
    // One object stream and the cross-reference stream.
    assert_eq!(
        loaded.trailer.get(b"Size").unwrap().as_i64().unwrap(),
        i64::from(max_id) + 3
    );
    for id in doc.objects.keys() {
        assert_eq!(loaded.get_object(*id).unwrap(), doc.get_object(*id).unwrap());
    }
}

#[test]
fn incremental_saving_leaves_the_new_document_as_it_was() {
    let mut doc = one_page_document(XrefType::CrossReferenceStream);
    let mut prev_bytes = Vec::new();
    doc.save_to(&mut prev_bytes).unwrap();
    let loaded = Document::load_mem(&prev_bytes).unwrap();

    let mut incremental = IncrementalDocument::create_from(prev_bytes, loaded);
    incremental.new_document.add_object(dictionary! { "Type" => "Example" });
    let max_id = incremental.new_document.max_id;
    let trailer = incremental.new_document.trailer.clone();

    let mut first = Vec::new();
    incremental.save_to(&mut first).unwrap();
    assert_eq!(incremental.new_document.max_id, max_id);
    assert_eq!(incremental.new_document.trailer, trailer);

    let mut second = Vec::new();
    incremental.save_to(&mut second).unwrap();
    assert!(first == second);
    Document::load_mem(&second).unwrap();
}

#[test]
fn a_document_with_revisions_saves_and_loads_again() {
    // #575: a file whose cross-reference streams chain through `/Prev`, loaded, changed in an
    // object of the first revision, and saved, has to load again.
    let mut doc = one_page_document(XrefType::CrossReferenceStream);
    let mut prev_bytes = Vec::new();
    doc.save_to(&mut prev_bytes).unwrap();
    let loaded = Document::load_mem(&prev_bytes).unwrap();
    let mut incremental = IncrementalDocument::create_from(prev_bytes, loaded);
    incremental.new_document.add_object(dictionary! { "Type" => "Example" });
    let mut bytes = Vec::new();
    incremental.save_to(&mut bytes).unwrap();

    let mut doc = Document::load_mem(&bytes).unwrap();
    let catalog_id = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    doc.get_dictionary_mut(catalog_id)
        .unwrap()
        .set("Lang", Object::string_literal("en"));
    for (name, save) in saves() {
        let saved = save(&mut doc);
        let reloaded = Document::load_mem(&saved).unwrap();
        assert!(!reloaded.trailer.has(b"Prev"), "{name}");
        assert_eq!(
            reloaded.get_dictionary(catalog_id).unwrap().get(b"Lang").unwrap(),
            &Object::string_literal("en"),
            "{name}"
        );
    }
}
