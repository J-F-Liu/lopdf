//! A cross-reference stream declares how many entries it holds in its `/Index`
//! array, but the entries themselves live in the (decoded) stream body. If the
//! declared count is allowed to run past what the body can hold, `load` spins on
//! an attacker-controlled number instead of the bytes actually present.

#![cfg(not(feature = "async"))]

use lopdf::{Document, Error, ParseError, SaveOptions, Stream, dictionary};

/// A PDF whose only cross-reference is an xref stream with the given `/W` widths,
/// `/Index [0 count]`, and `body` as an uncompressed stream body.
fn xref_stream_pdf(w: [i64; 3], count: i64, body: &[u8]) -> Vec<u8> {
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.5\n");
    let obj_offset = pdf.len();
    pdf.extend_from_slice(b"1 0 obj\n");
    pdf.extend_from_slice(
        format!(
            "<< /Type /XRef /Size 4 /W [{} {} {}] /Index [0 {count}] /Root 1 0 R /Length {} >>\n",
            w[0],
            w[1],
            w[2],
            body.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(b"stream\n");
    pdf.extend_from_slice(body);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    pdf.extend_from_slice(format!("startxref\n{obj_offset}\n%%EOF").as_bytes());
    pdf
}

// With `/W [0 0 0]` every entry reads zero bytes, so before the bound the loop ran
// purely on `/Index`. A 137-byte file claiming a million entries returned Ok with a
// million-node xref table; `i64::MAX` never returned at all. It must be rejected,
// and quickly.
#[test]
fn zero_width_xref_stream_is_rejected() {
    let pdf = xref_stream_pdf([0, 0, 0], 1_000_000, b"");
    match Document::load_mem(&pdf) {
        Err(Error::Parse(ParseError::InvalidXref)) => {}
        Err(other) => panic!("expected InvalidXref, got {other:?}"),
        Ok(doc) => panic!(
            "a 137-byte file claiming 1,000,000 xref entries loaded with {} entries",
            doc.reference_table.entries.len()
        ),
    }
}

// Non-zero widths already terminate once the reader runs dry, but only after
// inserting one map entry per byte first. Bounding the count against the bytes
// present stops that up front: three body bytes at `/W [1 1 1]` hold one entry, so
// a section claiming a million is malformed.
#[test]
fn index_count_past_stream_length_is_rejected() {
    let pdf = xref_stream_pdf([1, 1, 1], 1_000_000, &[1, 0, 0]);
    match Document::load_mem(&pdf) {
        Err(Error::Parse(ParseError::InvalidXref)) => {}
        Err(other) => panic!("expected InvalidXref, got {other:?}"),
        Ok(_) => panic!("an /Index count far past the stream length was accepted"),
    }
}

// The bound is derived from the real per-entry width, so a genuine xref stream
// whose `/Index` matches its body still loads. Written by lopdf's own writer to be
// sure the shape is exactly what the reader expects.
#[test]
fn valid_xref_stream_still_loads() {
    let mut doc = Document::with_version("1.5");
    let content_id = doc.add_object(Stream::new(dictionary! {}, b"BT ET".to_vec()));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => lopdf::Object::Reference(pages_id),
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => lopdf::Object::Reference(content_id),
    });
    doc.objects.insert(
        pages_id,
        lopdf::Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![lopdf::Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => lopdf::Object::Reference(pages_id),
    });
    doc.trailer.set("Root", lopdf::Object::Reference(catalog_id));

    let options = SaveOptions::builder()
        .use_object_streams(false)
        .use_xref_streams(true)
        .build();
    let mut buffer = Vec::new();
    doc.save_with_options(&mut buffer, options).unwrap();

    let reloaded = Document::load_mem(&buffer).unwrap();
    assert!(reloaded.get_object(catalog_id).is_ok());
    assert_eq!(reloaded.get_pages().len(), 1);
}
