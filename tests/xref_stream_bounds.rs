//! A cross-reference stream declares how many entries it holds in its `/Index`
//! array, but the entries themselves live in the (decoded) stream body. If the
//! declared count is allowed to run past what the body can hold, `load` spins on
//! an attacker-controlled number instead of the bytes actually present.

#![cfg(not(feature = "async"))]

use std::io::{Cursor, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use flate2::{Compression, write::ZlibEncoder};
use lopdf::{
    DecompressError, Document, Error, IncrementalDocument, LoadOptions, Object, ParseError, SaveOptions, Stream,
    dictionary,
};

static OBJECT_FILTER_CALLED: AtomicBool = AtomicBool::new(false);

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

fn compressed_xref_stream_pdf(entry_count: usize) -> Vec<u8> {
    assert!(entry_count >= 3);

    let mut pdf = b"%PDF-1.5\n".to_vec();
    let catalog_offset = pdf.len();
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog >>\nendobj\n");
    let xref_offset = pdf.len();

    let mut body = Vec::with_capacity(entry_count * 7);
    for id in 0..entry_count {
        let (entry_type, field_two, field_three) = match id {
            0 => (0, 0, u16::MAX),
            1 => (1, catalog_offset as u32, 0),
            2 => (1, xref_offset as u32, 0),
            _ => (0, 0, 0),
        };
        body.push(entry_type);
        body.extend_from_slice(&field_two.to_be_bytes());
        body.extend_from_slice(&field_three.to_be_bytes());
    }

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&body).unwrap();
    let compressed_body = encoder.finish().unwrap();

    pdf.extend_from_slice(b"2 0 obj\n");
    pdf.extend_from_slice(
        format!(
            "<< /Type /XRef /Size {entry_count} /W [1 4 2] /Index [0 {entry_count}] \
             /Root 1 0 R /Filter /FlateDecode /Length {} >>\n",
            compressed_body.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(b"stream\n");
    pdf.extend_from_slice(&compressed_body);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    pdf.extend_from_slice(format!("startxref\n{xref_offset}\n%%EOF").as_bytes());
    pdf
}

fn incremental_xref_pdf() -> Vec<u8> {
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let object_one_offset = pdf.len();
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog >>\nendobj\n");
    let object_two_offset = pdf.len();
    pdf.extend_from_slice(b"2 0 obj\n42\nendobj\n");

    let first_xref_offset = pdf.len();
    pdf.extend_from_slice(b"xref\n0 3\n0000000000 65535 f \n");
    pdf.extend_from_slice(format!("{object_one_offset:010} 00000 n \n").as_bytes());
    pdf.extend_from_slice(format!("{object_two_offset:010} 00000 n \n").as_bytes());
    pdf.extend_from_slice(b"trailer\n<< /Size 3 /Root 1 0 R >>\n");
    pdf.extend_from_slice(format!("startxref\n{first_xref_offset}\n%%EOF\n").as_bytes());

    let object_three_offset = pdf.len();
    pdf.extend_from_slice(b"3 0 obj\n43\nendobj\n");
    let object_four_offset = pdf.len();
    pdf.extend_from_slice(b"4 0 obj\n44\nendobj\n");
    let second_xref_offset = pdf.len();
    pdf.extend_from_slice(b"xref\n3 2\n");
    pdf.extend_from_slice(format!("{object_three_offset:010} 00000 n \n").as_bytes());
    pdf.extend_from_slice(format!("{object_four_offset:010} 00000 n \n").as_bytes());
    pdf.extend_from_slice(format!("trailer\n<< /Size 5 /Root 1 0 R /Prev {first_xref_offset} >>\n").as_bytes());
    pdf.extend_from_slice(format!("startxref\n{second_xref_offset}\n%%EOF").as_bytes());
    pdf
}

fn record_object_filter(id: (u32, u16), object: &mut Object) -> Option<((u32, u16), Object)> {
    OBJECT_FILTER_CALLED.store(true, Ordering::SeqCst);
    Some((id, object.clone()))
}

fn assert_invalid_xref<T>(result: lopdf::Result<T>, context: &str) {
    match result {
        Err(Error::Parse(ParseError::InvalidXref)) => {}
        Err(other) => panic!("{context}: expected InvalidXref, got {other:?}"),
        Ok(_) => panic!("{context}: expected the xref entry limit to reject the PDF"),
    }
}

fn temporary_pdf(pdf: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(pdf).unwrap();
    file.flush().unwrap();
    file
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

#[test]
fn compressed_xref_stream_over_configured_entry_limit_is_rejected() {
    let pdf = compressed_xref_stream_pdf(4);
    Document::load_mem(&pdf).expect("the compressed xref stream should be valid without a limit");

    let result = Document::load_mem_with_options(&pdf, LoadOptions::with_max_xref_entries(3));
    assert!(
        matches!(result, Err(Error::Parse(ParseError::InvalidXref))),
        "expected an xref entry limit error, got {result:?}"
    );
}

#[test]
fn metadata_load_options_propagate_resource_limits() {
    let pdf = compressed_xref_stream_pdf(4);
    Document::load_metadata_mem(&pdf).expect("the compressed xref stream should provide valid metadata");
    let file = temporary_pdf(&pdf);

    assert_invalid_xref(
        Document::load_metadata_mem_with_options(&pdf, LoadOptions::with_max_xref_entries(3)),
        "memory metadata loader",
    );
    assert_invalid_xref(
        Document::load_metadata_from_with_options(Cursor::new(pdf.clone()), LoadOptions::with_max_xref_entries(3)),
        "source metadata loader",
    );
    assert_invalid_xref(
        Document::load_metadata_with_options(file.path(), LoadOptions::with_max_xref_entries(3)),
        "file metadata loader",
    );

    let result = Document::load_metadata_mem_with_options(&pdf, LoadOptions::with_max_decompressed_size(1));
    assert!(
        matches!(
            result,
            Err(Error::Decompress(DecompressError::MemoryLimitExceeded { limit: 1 }))
        ),
        "metadata loading should propagate the decompressed-size limit, got {result:?}"
    );
}

#[test]
fn incremental_load_options_propagate_resource_limits() {
    let pdf = compressed_xref_stream_pdf(4);
    let file = temporary_pdf(&pdf);

    assert_invalid_xref(
        IncrementalDocument::load_mem_with_options(&pdf, LoadOptions::with_max_xref_entries(3)),
        "memory incremental loader",
    );
    assert_invalid_xref(
        IncrementalDocument::load_from_with_options(Cursor::new(pdf.clone()), LoadOptions::with_max_xref_entries(3)),
        "source incremental loader",
    );
    assert_invalid_xref(
        IncrementalDocument::load_with_options(file.path(), LoadOptions::with_max_xref_entries(3)),
        "file incremental loader",
    );

    let result =
        IncrementalDocument::load_from_with_options(Cursor::new(pdf), LoadOptions::with_max_decompressed_size(1));
    assert!(
        matches!(
            result,
            Err(Error::Decompress(DecompressError::MemoryLimitExceeded { limit: 1 }))
        ),
        "incremental loading should propagate the decompressed-size limit, got {result:?}"
    );
}

#[test]
fn cumulative_incremental_xref_entries_are_rejected_before_objects_are_parsed() {
    let pdf = incremental_xref_pdf();
    Document::load_mem(&pdf).expect("each incremental xref table should be valid on its own");

    OBJECT_FILTER_CALLED.store(false, Ordering::SeqCst);
    let options = LoadOptions {
        filter: Some(record_object_filter),
        max_xref_entries: Some(3),
        ..Default::default()
    };
    let result = Document::load_mem_with_options(&pdf, options);

    assert!(
        matches!(result, Err(Error::Parse(ParseError::InvalidXref))),
        "expected a cumulative xref entry limit error, got {result:?}"
    );
    assert!(
        !OBJECT_FILTER_CALLED.load(Ordering::SeqCst),
        "normal objects must not be parsed before the cumulative limit is enforced"
    );

    assert_invalid_xref(
        Document::load_metadata_mem_with_options(&pdf, LoadOptions::with_max_xref_entries(3)),
        "metadata cumulative merge",
    );
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
