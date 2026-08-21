use lopdf::{Document, LoadOptions, Object};

fn pdf_with_stream(declared_length: usize, content: &[u8], trailer: &[u8]) -> Vec<u8> {
    let mut pdf = Vec::new();
    let mut offsets = Vec::new();

    pdf.extend_from_slice(b"%PDF-1.7\n");
    offsets.push(pdf.len());
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    offsets.push(pdf.len());
    pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    offsets.push(pdf.len());
    pdf.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /Im0 4 0 R >> >> >>\nendobj\n",
    );
    offsets.push(pdf.len());
    pdf.extend_from_slice(
        format!(
            "4 0 obj\n<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter [/FlateDecode /DCTDecode] /Length {declared_length} >>\nstream\n"
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(content);
    pdf.extend_from_slice(trailer);

    let xref_offset = pdf.len();
    pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes());
    pdf
}

#[test]
fn lenient_load_recovers_an_unambiguous_stream_length_mismatch() {
    let content = b"actual stream bytes";
    let pdf = pdf_with_stream(0, content, b"\nendstream\nendobj\n");

    let document = Document::load_mem(&pdf).unwrap();
    let Object::Stream(stream) = document.get_object((4, 0)).unwrap() else {
        panic!("recovered object must remain a stream");
    };
    assert_eq!(stream.content, content);
    assert_eq!(
        stream.dict.get(b"Length").unwrap().as_i64().unwrap(),
        content.len() as i64
    );
}

#[test]
fn strict_load_rejects_a_stream_length_mismatch() {
    let content = b"actual stream bytes";
    let malformed = pdf_with_stream(0, content, b"\nendstream\nendobj\n");
    let conforming = pdf_with_stream(content.len(), content, b"\nendstream\nendobj\n");
    let strict = LoadOptions {
        strict: true,
        ..Default::default()
    };

    assert!(Document::load_mem_with_options(&conforming, strict.clone()).is_ok());
    assert!(Document::load_mem_with_options(&malformed, strict).is_err());
}

#[test]
fn lenient_load_does_not_guess_an_ambiguous_stream_boundary() {
    let pdf = pdf_with_stream(
        0,
        b"first candidate\nendstream\nendobj\nbytes after the false boundary",
        b"\nendstream\nendobj\n",
    );

    let document = Document::load_mem(&pdf).unwrap();
    assert!(document.get_object((4, 0)).is_err());
}

#[test]
fn lenient_load_does_not_recover_without_endstream() {
    let pdf = pdf_with_stream(0, b"unterminated stream bytes", b"\nendobj\n");

    let document = Document::load_mem(&pdf).unwrap();
    assert!(document.get_object((4, 0)).is_err());
}
