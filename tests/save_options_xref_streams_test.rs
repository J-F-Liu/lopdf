//! `SaveOptions::use_xref_streams` alone. The two features are independent in the PDF
//! specification, so one must not require the other.

#![cfg(not(feature = "async"))]

use lopdf::xref::XrefType;
use lopdf::{Document, Object, SaveOptions, Stream, dictionary};

/// A one page document written with a classic cross-reference table, as a document
/// loaded from a pre-1.5 file would be.
fn sample_document() -> Document {
    let mut doc = Document::with_version("1.4");
    doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;

    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, b"BT ET".to_vec()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => Object::Reference(content_id),
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));

    doc
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

#[test]
fn xref_streams_are_written_without_object_streams() {
    let mut doc = sample_document();

    let options = SaveOptions::builder()
        .use_object_streams(false)
        .use_xref_streams(true)
        .build();
    let mut buffer = Vec::new();
    doc.save_with_options(&mut buffer, options).unwrap();

    assert!(
        contains(&buffer, b"/Type/XRef") || contains(&buffer, b"/Type /XRef"),
        "a requested cross-reference stream should be written even with object streams off"
    );
    assert!(
        !contains(&buffer, b"\nxref\n"),
        "the classic cross-reference table should not be written as well"
    );
    assert!(
        !contains(&buffer, b"/ObjStm"),
        "object streams were not requested and must not appear"
    );

    // A cross-reference stream is a PDF 1.5 construct, so the header has to say so.
    assert!(
        buffer.starts_with(b"%PDF-1.5"),
        "expected the version to be raised to 1.5, got {:?}",
        String::from_utf8_lossy(&buffer[..8.min(buffer.len())])
    );

    let reloaded = Document::load_mem(&buffer).unwrap();
    assert_eq!(reloaded.get_pages().len(), 1, "the saved document should still load");
}

#[test]
fn cross_reference_table_is_kept_when_xref_streams_are_not_requested() {
    let mut doc = sample_document();

    let options = SaveOptions::builder()
        .use_object_streams(false)
        .use_xref_streams(false)
        .build();
    let mut buffer = Vec::new();
    doc.save_with_options(&mut buffer, options).unwrap();

    assert!(
        contains(&buffer, b"\nxref\n"),
        "without the option the document keeps its cross-reference table"
    );
    assert!(
        !contains(&buffer, b"/Type/XRef") && !contains(&buffer, b"/Type /XRef"),
        "no cross-reference stream should appear when it was not requested"
    );
    assert!(
        buffer.starts_with(b"%PDF-1.4"),
        "the version should be left alone when no 1.5 feature is used"
    );
}

#[test]
fn both_options_together_still_write_object_and_xref_streams() {
    let mut doc = sample_document();

    let options = SaveOptions::builder()
        .use_object_streams(true)
        .use_xref_streams(true)
        .build();
    let mut buffer = Vec::new();
    doc.save_with_options(&mut buffer, options).unwrap();

    assert!(
        contains(&buffer, b"/ObjStm"),
        "object streams were requested and should still be written"
    );
    assert!(
        contains(&buffer, b"/Type/XRef") || contains(&buffer, b"/Type /XRef"),
        "cross-reference streams were requested and should still be written"
    );

    let reloaded = Document::load_mem(&buffer).unwrap();
    assert_eq!(reloaded.get_pages().len(), 1, "the saved document should still load");
}

/// A classic cross-reference table is a sequence of subsections, each headed
/// by the first object id it covers. Documents with gaps in their object ids
/// produce more than one subsection, and every one of them must carry its own
/// starting id: writing `0` for all of them makes the reader resolve the wrong
/// objects, silently dropping the ones past the first gap.
#[test]
fn xref_table_sections_record_their_starting_id() {
    let mut doc = Document::with_version("1.4");
    doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;

    // Deliberately non-contiguous ids: 1, 3 and 7, inserted directly so the
    // gaps are not closed by `add_object`.
    for id in [1_u32, 3, 7] {
        doc.objects.insert(
            (id, 0),
            Object::Dictionary(dictionary! { "Marker" => Object::Integer(id as i64) }),
        );
        doc.max_id = doc.max_id.max(id);
    }

    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).unwrap();

    let text = String::from_utf8_lossy(&buffer);
    let table = &text[text.find("xref").unwrap()..text.find("trailer").unwrap()];

    // The first subsection absorbs the mandatory free entry for object 0; the
    // later ones start at the first id they actually cover.
    assert!(
        table.contains("3 1\n"),
        "subsection for object 3 lost its starting id:\n{table}"
    );
    assert!(
        table.contains("7 1\n"),
        "subsection for object 7 lost its starting id:\n{table}"
    );

    let reloaded = Document::load_mem(&buffer).unwrap();
    for id in [1_u32, 3, 7] {
        let marker = reloaded
            .get_object((id, 0))
            .unwrap_or_else(|e| panic!("object {id} did not survive the round trip: {e}"))
            .as_dict()
            .unwrap()
            .get(b"Marker")
            .unwrap()
            .as_i64()
            .unwrap();
        assert_eq!(marker, id as i64, "object {id} resolved to the wrong entry");
    }
}

/// The mandatory `0` free entry only fits in a subsection of its own when the
/// lowest object id in the file is not 1. Squeezing it into the subsection
/// that starts at, say, object 3 would claim that subsection covers objects 0
/// and 1 and shift every entry in it by two.
#[test]
fn xref_table_free_entry_is_its_own_section_when_object_1_is_missing() {
    let mut doc = Document::with_version("1.4");
    doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;

    // A leading gap: the file has no objects 1 or 2 at all.
    for id in [3_u32, 7] {
        doc.objects.insert(
            (id, 0),
            Object::Dictionary(dictionary! { "Marker" => Object::Integer(id as i64) }),
        );
        doc.max_id = doc.max_id.max(id);
    }

    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).unwrap();

    let text = String::from_utf8_lossy(&buffer);
    let table = &text[text.find("xref").unwrap()..text.find("trailer").unwrap()];

    assert!(
        table.contains("0 1\n0000000000 65535 f"),
        "the free entry for object 0 is missing or shares its subsection:\n{table}"
    );
    assert!(
        table.contains("3 1\n") && table.contains("7 1\n"),
        "the subsections lost their starting ids:\n{table}"
    );

    let reloaded = Document::load_mem(&buffer).unwrap();
    for id in [3_u32, 7] {
        let marker = reloaded
            .get_object((id, 0))
            .unwrap_or_else(|e| panic!("object {id} did not survive the round trip: {e}"))
            .as_dict()
            .unwrap()
            .get(b"Marker")
            .unwrap()
            .as_i64()
            .unwrap();
        assert_eq!(marker, id as i64, "object {id} resolved to the wrong entry");
    }
}

/// A document with no objects still needs the mandatory `0 1` free subsection.
#[test]
fn empty_document_still_writes_the_free_entry() {
    let mut doc = Document::with_version("1.4");
    doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;

    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).unwrap();

    let text = String::from_utf8_lossy(&buffer);
    let table = &text[text.find("xref").unwrap()..text.find("trailer").unwrap()];
    assert!(
        table.contains("0 1\n"),
        "the free entry for object 0 is missing:\n{table}"
    );
    assert!(
        table.contains("0000000000 65535 f"),
        "the free entry is malformed:\n{table}"
    );
}
