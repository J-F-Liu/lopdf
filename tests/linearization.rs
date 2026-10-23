use lopdf::xref::XrefRecovery;
use lopdf::{Document, Linearization};

/// Append `body` as indirect object `number` and return its offset.
fn object(buf: &mut Vec<u8>, number: u32, body: &str) -> usize {
    let offset = buf.len();
    buf.extend_from_slice(format!("{number} 0 obj\n{body}\nendobj\n").as_bytes());
    offset
}

/// Append a cross-reference section of the given `(number, offset)` entries,
/// with `trailer` entries, and return its offset.
fn section(buf: &mut Vec<u8>, entries: &[(u32, usize)], trailer: &str) -> usize {
    let offset = buf.len();
    buf.extend_from_slice(b"xref\n");
    for &(number, object_offset) in entries {
        buf.extend_from_slice(format!("{number} 1\n{object_offset:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(format!("trailer\n<< {trailer} >>\n").as_bytes());
    offset
}

fn end(buf: &mut Vec<u8>, startxref: usize) {
    buf.extend_from_slice(format!("startxref\n{startxref}\n%%EOF\n").as_bytes());
}

/// A file laid out as ISO 32000-2 Annex F lays out a linearized one: the
/// parameter dictionary first, then the first-page section, whose `/Prev`
/// names the main section at the end, which the final `startxref` does not
/// name. `linearized` is the body of object 5.
fn linearized_file(linearized: &str) -> (Vec<u8>, usize) {
    let mut buf = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let parameters = object(&mut buf, 5, linearized);
    let first_page_xref_placeholder = buf.len();
    let catalog = object(&mut buf, 1, "<< /Type /Catalog /Pages 2 0 R >>");
    let page = object(&mut buf, 3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>");
    let pages = object(&mut buf, 2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    let main = section(&mut buf, &[(2, pages)], "/Size 6");
    end(&mut buf, 0);
    let first = section(
        &mut buf,
        &[(1, catalog), (3, page), (5, parameters)],
        &format!("/Size 6 /Root 1 0 R /Prev {main}"),
    );
    end(&mut buf, first);
    assert!(first_page_xref_placeholder < 1024);
    (buf, first)
}

const PARAMETERS: &str = "<< /Linearized 1 /L 1000 /H [ 120 30 400 20 ] /O 3 /E 300 /N 1 /T 500 /P 0 >>";

#[test]
fn reads_the_linearization_parameters() {
    let doc = Document::load_mem(&linearized_file(PARAMETERS).0).unwrap();

    let linearization = doc.linearization.clone().expect("the file is linearized");
    assert_eq!(
        linearization,
        Linearization {
            id: (5, 0),
            version: 1.0,
            file_length: 1000,
            hint_streams: vec![(120, 30), (400, 20)],
            first_page_object: 3,
            first_page_end: 300,
            page_count: 1,
            main_xref_offset: 500,
            first_page: 0,
        }
    );
    assert_eq!(doc.xref_section_count, 2);
    assert_eq!(doc.revision_count(), 1);
}

#[test]
fn an_incremental_update_adds_a_revision_and_keeps_the_parameters() {
    let (mut buf, previous) = linearized_file(PARAMETERS);
    let info = object(&mut buf, 6, "<< /Title (Updated) >>");
    let update = section(
        &mut buf,
        &[(6, info)],
        &format!("/Size 7 /Root 1 0 R /Info 6 0 R /Prev {previous}"),
    );
    end(&mut buf, update);

    let doc = Document::load_mem(&buf).unwrap();

    assert!(doc.linearization.is_some());
    assert_eq!(doc.xref_section_count, 3);
    assert_eq!(doc.revision_count(), 2);
}

#[test]
fn an_update_that_replaces_the_dictionary_leaves_the_file_linearized_as_written() {
    let (mut buf, first) = linearized_file(PARAMETERS);
    let replacement = object(&mut buf, 5, "<< /Replaced true >>");
    let update = section(
        &mut buf,
        &[(5, replacement)],
        &format!("/Size 6 /Root 1 0 R /Prev {first}"),
    );
    end(&mut buf, update);

    let doc = Document::load_mem(&buf).unwrap();

    assert!(doc.get_dictionary((5, 0)).unwrap().has(b"Replaced"));
    assert_eq!(
        doc.linearization.as_ref().map(|linearization| linearization.page_count),
        Some(1)
    );
    assert_eq!(doc.revision_count(), 2);
}

#[test]
fn incomplete_parameters_are_no_linearization() {
    for parameters in [
        "<< /Linearized 1 /L 1000 /O 3 /E 300 /N 1 /T 500 >>",
        "<< /Linearized 1 /L 1000 /H [ 120 ] /O 3 /E 300 /N 1 /T 500 >>",
        "<< /Linearized 1 /L -1 /H [ 120 30 ] /O 3 /E 300 /N 1 /T 500 >>",
        "<< /Linearized /Yes /L 1000 /H [ 120 30 ] /O 3 /E 300 /N 1 /T 500 >>",
        "<< /Type /Font >>",
    ] {
        let doc = Document::load_mem(&linearized_file(parameters).0).unwrap();
        assert_eq!(doc.linearization, None, "{parameters}");
        assert_eq!(doc.xref_section_count, 2);
        assert_eq!(doc.revision_count(), 2);
    }
}

#[test]
fn a_plain_file_has_one_revision_and_no_parameters() {
    let doc = Document::load_mem(&std::fs::read("assets/example.pdf").unwrap()).unwrap();

    assert_eq!(doc.linearization, None);
    assert_eq!((doc.xref_section_count, doc.revision_count()), (1, 1));
    assert_eq!(Document::new().revision_count(), 1);
}

#[test]
fn the_assets_report_their_revisions() {
    let incremental = Document::load_mem(&std::fs::read("assets/Incremental.pdf").unwrap()).unwrap();
    assert_eq!((incremental.xref_section_count, incremental.revision_count()), (3, 3));

    let linearized = Document::load_mem(&std::fs::read("assets/encrypted.pdf").unwrap()).unwrap();
    let linearization = linearized
        .linearization
        .as_ref()
        .expect("assets/encrypted.pdf is linearized");
    assert_eq!(
        linearization.file_length,
        std::fs::metadata("assets/encrypted.pdf").unwrap().len()
    );
    assert_eq!((linearized.xref_section_count, linearized.revision_count()), (2, 1));
}

#[test]
fn a_prev_chain_that_returns_to_the_start_counts_each_section_once() {
    let mut buf = b"%PDF-1.4\n".to_vec();
    let catalog = object(&mut buf, 1, "<< /Type /Catalog /Pages 2 0 R >>");
    let pages = object(&mut buf, 2, "<< /Type /Pages /Kids [] /Count 0 >>");
    let first = section(&mut buf, &[(1, catalog)], "/Size 3 /Root 1 0 R /Prev 0000");
    let second_offset = buf.len();
    // The first section's /Prev is patched below to name the second, whose /Prev names the first.
    let second = section(&mut buf, &[(2, pages)], &format!("/Size 3 /Root 1 0 R /Prev {first}"));
    assert_eq!(second, second_offset);
    end(&mut buf, first);
    let text = String::from_utf8(buf)
        .unwrap()
        .replacen("/Prev 0000", &format!("/Prev {second:04}"), 1);

    let doc = Document::load_mem(text.as_bytes()).unwrap();

    assert_eq!((doc.xref_section_count, doc.revision_count()), (2, 2));
}

#[test]
fn a_rebuilt_table_counts_as_one_revision() {
    let (mut buf, _) = linearized_file(PARAMETERS);
    let tail = buf.len() - 12;
    buf.truncate(tail);

    let doc = Document::load_mem(&buf).unwrap();

    assert_eq!(doc.xref_recovery, Some(XrefRecovery::Reconstructed));
    assert_eq!((doc.xref_section_count, doc.revision_count()), (0, 1));
    assert!(doc.linearization.is_some());
}
