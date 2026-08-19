#![cfg(feature = "async")]

use std::io::Write;

use flate2::{Compression, write::ZlibEncoder};
use lopdf::{DecompressError, Document, Error, IncrementalDocument, LoadOptions, ParseError};

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

fn temporary_pdf(pdf: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(pdf).unwrap();
    file.flush().unwrap();
    file
}

fn assert_xref_limit_exceeded<T>(result: lopdf::Result<T>, context: &str) {
    match result {
        Err(Error::Parse(ParseError::XrefEntryLimitExceeded { limit: 3 })) => {}
        Err(other) => panic!("{context}: expected the three-entry xref limit to be exceeded, got {other:?}"),
        Ok(_) => panic!("{context}: expected the xref entry limit to reject the PDF"),
    }
}

#[tokio::test]
async fn async_metadata_load_options_propagate_resource_limits() {
    let pdf = compressed_xref_stream_pdf(4);
    let file = temporary_pdf(&pdf);

    assert_xref_limit_exceeded(
        Document::load_metadata_mem_with_options(&pdf, LoadOptions::with_max_xref_entries(3)),
        "async-build memory metadata loader",
    );
    assert_xref_limit_exceeded(
        Document::load_metadata_with_options(file.path(), LoadOptions::with_max_xref_entries(3)).await,
        "async file metadata loader",
    );

    let source = tokio::fs::File::open(file.path()).await.unwrap();
    assert_xref_limit_exceeded(
        Document::load_metadata_from_with_options(source, LoadOptions::with_max_xref_entries(3)).await,
        "async source metadata loader",
    );

    let result = Document::load_metadata_with_options(file.path(), LoadOptions::with_max_decompressed_size(1)).await;
    assert!(
        matches!(
            result,
            Err(Error::Decompress(DecompressError::MemoryLimitExceeded { limit: 1 }))
        ),
        "async metadata loading should propagate the decompressed-size limit, got {result:?}"
    );
}

#[tokio::test]
async fn async_incremental_load_options_propagate_resource_limits() {
    let pdf = compressed_xref_stream_pdf(4);
    let file = temporary_pdf(&pdf);

    assert_xref_limit_exceeded(
        IncrementalDocument::load_mem_with_options(&pdf, LoadOptions::with_max_xref_entries(3)),
        "async-build memory incremental loader",
    );
    assert_xref_limit_exceeded(
        IncrementalDocument::load_with_options(file.path(), LoadOptions::with_max_xref_entries(3)).await,
        "async file incremental loader",
    );

    let source = tokio::fs::File::open(file.path()).await.unwrap();
    assert_xref_limit_exceeded(
        IncrementalDocument::load_from_with_options(source, LoadOptions::with_max_xref_entries(3)).await,
        "async source incremental loader",
    );

    let result = IncrementalDocument::load_with_options(file.path(), LoadOptions::with_max_decompressed_size(1)).await;
    assert!(
        matches!(
            result,
            Err(Error::Decompress(DecompressError::MemoryLimitExceeded { limit: 1 }))
        ),
        "async incremental loading should propagate the decompressed-size limit, got {result:?}"
    );
}
