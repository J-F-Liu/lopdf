//! A new object takes the number after `Document::max_id`. Once that is `u32::MAX` there is
//! none left: allocation fails instead of wrapping around to object 0 and then replacing
//! object 1, which is often the catalog.

use lopdf::xref::XrefType;
use lopdf::{Bookmark, Document, Error, Object, SaveOptions, Stream, dictionary};

/// A one-page document whose catalog is object 1, with `max_id` raised to `max_id` as if the
/// file also had an object with that number.
fn document_with_max_id(max_id: u32) -> Document {
    let mut doc = Document::with_version("1.5");
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::string_literal(b"0123456789abcdef"),
            Object::string_literal(b"fedcba9876543210"),
        ]),
    );
    let catalog_id = doc.new_object_id();
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
    doc.objects.insert(
        catalog_id,
        Object::Dictionary(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        }),
    );
    doc.trailer.set("Root", catalog_id);
    doc.max_id = max_id;
    doc
}

fn assert_catalog_is_object_1(doc: &Document) {
    let catalog = doc.get_dictionary((1, 0)).unwrap();
    assert_eq!(catalog.get(b"Type").unwrap().as_name().unwrap(), b"Catalog");
}

#[test]
fn the_last_object_number_is_allocated_and_then_allocation_fails() {
    let mut doc = document_with_max_id(u32::MAX - 1);

    assert_eq!(doc.try_add_object(dictionary! {}).unwrap(), (u32::MAX, 0));
    let object_count = doc.objects.len();

    assert!(matches!(
        doc.try_add_object(dictionary! {}),
        Err(Error::ObjectIdOverflow)
    ));
    assert!(matches!(doc.try_new_object_id(), Err(Error::ObjectIdOverflow)));
    assert_eq!(doc.max_id, u32::MAX);
    assert_eq!(doc.objects.len(), object_count);
    assert!(!doc.objects.contains_key(&(0, 0)));
    assert_catalog_is_object_1(&doc);
}

#[test]
#[should_panic(expected = "object ID overflow")]
fn add_object_panics_rather_than_wrapping() {
    let mut doc = document_with_max_id(u32::MAX);
    doc.add_object(dictionary! {});
}

#[test]
#[should_panic(expected = "object ID overflow")]
fn new_object_id_panics_rather_than_wrapping() {
    let mut doc = document_with_max_id(u32::MAX);
    doc.new_object_id();
}

#[test]
#[should_panic(expected = "object ID overflow")]
fn build_outline_panics_rather_than_wrapping() {
    // The outline dictionary takes the last number; its item would wrap around to object 0 and
    // its action to object 1.
    let mut doc = document_with_max_id(u32::MAX - 1);
    doc.add_bookmark(Bookmark::new("Page 1".to_string(), [0.0, 0.0, 0.0], 0, (4, 0)), None);
    doc.build_outline();
}

#[test]
fn encrypt_fails_before_encrypting_any_object() {
    let mut doc = document_with_max_id(u32::MAX);
    let state = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner",
        user_password: "",
        key_length: 128,
        permissions: lopdf::Permissions::all(),
    })
    .unwrap();
    let objects = doc.objects.clone();

    assert!(matches!(doc.encrypt(&state), Err(Error::ObjectIdOverflow)));
    assert!(!doc.is_encrypted());
    assert_eq!(doc.objects, objects);
    assert_eq!(doc.max_id, u32::MAX);
}

#[test]
fn saving_fails_when_an_object_or_xref_stream_has_no_number_left() {
    let object_streams = SaveOptions::builder()
        .use_object_streams(true)
        .use_xref_streams(true)
        .build();
    let xref_stream = SaveOptions::builder().use_xref_streams(true).build();
    // The object stream needs one number and the cross-reference stream another.
    for (max_id, options) in [
        (u32::MAX, object_streams.clone()),
        (u32::MAX - 1, object_streams),
        (u32::MAX, xref_stream),
    ] {
        let mut doc = document_with_max_id(max_id);
        let error = doc.save_with_options(&mut Vec::new(), options).unwrap_err();
        let source = error.get_ref().and_then(|source| source.downcast_ref::<Error>());
        assert!(
            matches!(source, Some(Error::ObjectIdOverflow)),
            "max_id {max_id}: {error:?}"
        );
        assert_catalog_is_object_1(&doc);
    }
}

#[test]
fn a_table_trailer_counts_the_last_object_number() {
    let mut doc = document_with_max_id(u32::MAX);
    doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).unwrap();

    let trailer = &buffer[buffer.windows(7).rposition(|window| window == b"trailer").unwrap()..];
    let trailer = String::from_utf8_lossy(trailer);
    assert!(trailer.contains("/Size 4294967296"), "{trailer}");
}
