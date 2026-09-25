//! An object stream whose `/Length` can only be resolved by reading an object
//! stream that depends on it. Before the fix every such file overflowed the
//! stack while loading, which aborts the whole process instead of returning
//! an error.

use lopdf::Document;

/// Uncompressed object stream holding the given `(number, value)` integers.
fn object_stream(number: u32, length_ref: u32, objects: &[(u32, i64)]) -> Vec<u8> {
    let mut header = String::new();
    let mut body = String::new();
    for (id, value) in objects {
        header.push_str(&format!("{id} {} ", body.len()));
        body.push_str(&format!("{value} "));
    }
    let content = format!("{header}{body}");
    format!(
        "{number} 0 obj\n<< /Type /ObjStm /N {} /First {} /Length {length_ref} 0 R >>\nstream\n{content}\nendstream\nendobj\n",
        objects.len(),
        header.len(),
    )
    .into_bytes()
}

/// Cross-reference stream entry: `(type, field 2, field 3)` with `/W [1 4 2]`.
fn xref_row(kind: u8, second: u32, third: u16) -> [u8; 7] {
    let [a, b, c, d] = second.to_be_bytes();
    let [e, f] = third.to_be_bytes();
    [kind, a, b, c, d, e, f]
}

/// An object stream of the test document.
struct Container<'a> {
    number: u32,
    /// Number of the object holding the `/Length` of this stream.
    length_ref: u32,
    /// `(number, value)` of the integers stored in this stream.
    objects: &'a [(u32, i64)],
}

/// Catalog, page tree and page as plain objects 1–3, then the object streams.
fn pdf_with_object_streams(streams: &[Container]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for body in [
        &b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n"[..],
        b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>\nendobj\n",
    ] {
        offsets.push(pdf.len());
        pdf.extend_from_slice(body);
    }

    let size = 4 + streams.iter().map(|stream| 1 + stream.objects.len()).sum::<usize>() as u32 + 1;
    let mut rows = vec![[0u8; 7]; size as usize];
    rows[0] = xref_row(0, 0, 0xFFFF);
    for (number, offset) in offsets.iter().enumerate() {
        rows[number + 1] = xref_row(1, *offset as u32, 0);
    }
    for stream in streams {
        rows[stream.number as usize] = xref_row(1, pdf.len() as u32, 0);
        for (index, (id, _)) in stream.objects.iter().enumerate() {
            rows[*id as usize] = xref_row(2, stream.number, index as u16);
        }
        pdf.extend_from_slice(&object_stream(stream.number, stream.length_ref, stream.objects));
    }

    let xref_number = size - 1;
    let xref_offset = pdf.len();
    rows[xref_number as usize] = xref_row(1, xref_offset as u32, 0);
    let table: Vec<u8> = rows.concat();
    pdf.extend_from_slice(
        format!(
            "{xref_number} 0 obj\n<< /Type /XRef /Size {size} /W [1 4 2] /Root 1 0 R /Length {} >>\nstream\n",
            table.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&table);
    pdf.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes());
    pdf
}

#[test]
fn object_stream_whose_length_lives_inside_itself_does_not_overflow_the_stack() {
    // Object stream 5 holds object 4, which is its own /Length.
    let pdf = pdf_with_object_streams(&[Container {
        number: 5,
        length_ref: 4,
        objects: &[(4, 0)],
    }]);
    let document = Document::load_mem(&pdf).expect("the rest of the document is intact");
    assert_eq!(document.get_pages().len(), 1);
}

#[test]
fn object_streams_holding_each_others_length_do_not_overflow_the_stack() {
    // Object stream 5 holds the /Length of stream 7 (object 6) and stream 7
    // holds the /Length of stream 5 (object 4).
    let pdf = pdf_with_object_streams(&[
        Container {
            number: 5,
            length_ref: 4,
            objects: &[(6, 0)],
        },
        Container {
            number: 7,
            length_ref: 6,
            objects: &[(4, 0)],
        },
    ]);
    let document = Document::load_mem(&pdf).expect("the rest of the document is intact");
    assert_eq!(document.get_pages().len(), 1);
}
