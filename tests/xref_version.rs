//! A cross-reference stream requires PDF 1.5 even when no SaveOptions were supplied.

use lopdf::{Document, IncrementalDocument, Object, dictionary, xref::XrefType};

fn document(version: &str, xref: XrefType) -> Document {
    let mut doc = Document::with_version(version);
    doc.reference_table.cross_reference_type = xref;
    let root = doc.add_object(dictionary! { "Type" => "Catalog" });
    doc.trailer.set("Root", root);
    doc
}

#[test]
fn plain_save_to_raises_version_for_default_cross_reference_stream() {
    let mut doc = Document::new();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.5"));
    assert_eq!(doc.version, "1.5");
    assert!(bytes.windows(b"/Type/XRef".len()).any(|w| w == b"/Type/XRef"));
}

#[test]
fn plain_save_to_file_raises_version_for_cross_reference_stream() {
    let mut doc = document("1.4", XrefType::CrossReferenceStream);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("xref-stream.pdf");
    doc.save(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.5"));
    assert_eq!(Document::load_mem(&bytes).unwrap().version, "1.5");
}

#[test]
fn plain_save_keeps_classic_cross_reference_tables_and_later_versions() {
    for (version, xref) in [
        ("1.4", XrefType::CrossReferenceTable),
        ("1.7", XrefType::CrossReferenceStream),
        ("2.0", XrefType::CrossReferenceStream),
    ] {
        let mut doc = document(version, xref);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        assert!(bytes.starts_with(format!("%PDF-{version}").as_bytes()));
        assert_eq!(Document::load_mem(&bytes).unwrap().version, version);
    }
}

#[test]
fn incremental_save_preserves_existing_header_and_cross_reference_format() {
    for (version, xref) in [
        ("1.4", XrefType::CrossReferenceTable),
        ("1.7", XrefType::CrossReferenceStream),
    ] {
        let mut doc = document(version, xref);
        let mut original = Vec::new();
        doc.save_to(&mut original).unwrap();
        let previous = Document::load_mem(&original).unwrap();
        let mut update = IncrementalDocument::create_from(original.clone(), previous);
        let marker = update.new_document.add_object(Object::string_literal("appended"));
        let mut bytes = Vec::new();
        update.save_to(&mut bytes).unwrap();
        assert!(bytes.starts_with(&original));
        assert!(!bytes[original.len()..].windows(b"%PDF-".len()).any(|w| w == b"%PDF-"));
        let reloaded = Document::load_mem(&bytes).unwrap();
        assert_eq!(reloaded.version, version);
        assert_eq!(reloaded.get_object(marker).unwrap().as_str().unwrap(), b"appended");
    }
}
