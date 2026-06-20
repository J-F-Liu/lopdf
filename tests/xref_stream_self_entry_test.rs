//! Regression test: the cross-reference stream must include an xref entry for
//! its own object.
//!
//! The cross-reference stream object is created late in
//! `write_cross_reference_stream`, after `Xref::size` was first computed. If
//! `Xref::insert` doesn't keep `size` up to date (its documented invariant —
//! "highest object number plus 1"), then `create_xref_steam`, which iterates
//! `1..size`, silently drops the highest id(s) — including the cross-reference
//! stream itself. qpdf reports this as:
//!   "xref entry for the xref stream itself is missing".

use lopdf::{dictionary, Document, Object, Stream};

fn parse_u64_after(text: &str, key: &str) -> Option<i64> {
    let rest = &text[text.find(key)? + key.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn parse_index(text: &str) -> Option<Vec<i64>> {
    let open = text.find("/Index[")? + "/Index[".len();
    let close = text[open..].find(']')? + open;
    text[open..close]
        .split_whitespace()
        .map(|t| t.parse::<i64>().ok())
        .collect()
}

#[test]
fn xref_stream_index_covers_the_xref_stream_itself() {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    // A handful of compressible (non-stream) objects plus one stream, so an
    // ObjStm is created and the max object id grows past the initial size.
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let content_id = doc.add_object(Stream::new(dictionary! {}, b"BT ET".to_vec()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let mut buffer = Vec::new();
    doc.save_modern(&mut buffer).expect("save_modern");

    let text = String::from_utf8_lossy(&buffer);
    let size = parse_u64_after(&text, "/Size ").expect("xref stream /Size");
    let index = parse_index(&text).expect("xref stream /Index");
    assert!(!index.is_empty() && index.len() % 2 == 0, "malformed /Index");

    // /Index is a sequence of (first_id, count) subsections. The highest id it
    // covers must be the highest object number, i.e. /Size - 1 — otherwise the
    // top object (the cross-reference stream itself) has no xref entry.
    let max_covered = index
        .chunks(2)
        .map(|pair| pair[0] + pair[1] - 1)
        .max()
        .unwrap();
    assert_eq!(
        max_covered,
        size - 1,
        "xref /Index must cover the cross-reference stream's own object: \
         /Size={size} but /Index only covers up to id {max_covered}"
    );
}
