//! An object stream whose `/Length` needs reading an object stream that depends on it. Such
//! files used to overflow the stack while loading, aborting the process instead of erroring.

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

/// Catalog, page tree and page as plain objects 1–3, then `bodies` as objects
/// 4 and up. Returns the bytes written and each object's offset.
fn pdf_with_bodies(bodies: &[Vec<u8>]) -> (Vec<u8>, Vec<u32>) {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for body in [
        &b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n"[..],
        b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>\nendobj\n",
    ]
    .into_iter()
    .chain(bodies.iter().map(|body| &body[..]))
    {
        offsets.push(pdf.len() as u32);
        pdf.extend_from_slice(body);
    }
    (pdf, offsets)
}

/// Appends the cross-reference stream for `rows`, adding its own entry, and
/// the `startxref` footer.
fn append_xref_stream(pdf: &mut Vec<u8>, number: u32, rows: &mut Vec<[u8; 7]>) {
    let offset = pdf.len() as u32;
    rows.push(xref_row(1, offset, 0));
    let table: Vec<u8> = rows.concat();
    pdf.extend_from_slice(
        format!(
            "{number} 0 obj\n<< /Type /XRef /Size {} /W [1 4 2] /Root 1 0 R /Length {} >>\nstream\n",
            rows.len(),
            table.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&table);
    pdf.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{offset}\n%%EOF\n").as_bytes());
}

/// An object stream of the test document.
struct Container<'a> {
    number: u32,
    /// Number of the object holding the `/Length` of this stream.
    length_ref: u32,
    /// `(number, value)` of the integers stored in this stream.
    objects: &'a [(u32, i64)],
}

/// A document whose objects 4 and up are the given object streams.
fn pdf_with_object_streams(streams: &[Container]) -> Vec<u8> {
    let bodies: Vec<Vec<u8>> = streams
        .iter()
        .map(|stream| object_stream(stream.number, stream.length_ref, stream.objects))
        .collect();
    let (mut pdf, offsets) = pdf_with_bodies(&bodies);
    let size = 4 + bodies.len() + streams.iter().map(|stream| stream.objects.len()).sum::<usize>() + 1;
    let xref_number = (size - 1) as u32;

    let mut rows = vec![[0u8; 7]; size - 1];
    rows[0] = xref_row(0, 0, 0xFFFF);
    for (number, offset) in offsets.iter().enumerate() {
        rows[number + 1] = xref_row(1, *offset, 0);
    }
    for (position, stream) in streams.iter().enumerate() {
        rows[stream.number as usize] = xref_row(1, offsets[3 + position], 0);
        for (index, (id, _)) in stream.objects.iter().enumerate() {
            rows[*id as usize] = xref_row(2, stream.number, index as u16);
        }
    }

    append_xref_stream(&mut pdf, xref_number, &mut rows);
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

/// A document whose cross-reference table claims that object stream 5 is
/// stored inside object stream 5.
fn pdf_marked_as_compressed_in_itself() -> Vec<u8> {
    let content = "4 0 12 ";
    let (mut pdf, offsets) = pdf_with_bodies(&[
        format!(
            "5 0 obj\n<< /Type /ObjStm /N 1 /First 4 /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
            content.len()
        )
        .into_bytes(),
        b"6 0 obj\n<< /Length 4 0 R >>\nstream\nBT ET\nendstream\nendobj\n".to_vec(),
    ]);

    let mut rows = vec![
        xref_row(0, 0, 0xFFFF),
        xref_row(1, offsets[0], 0),
        xref_row(1, offsets[1], 0),
        xref_row(1, offsets[2], 0),
        // Object 4 and object stream 5 are both claimed to live in stream 5.
        xref_row(2, 5, 0),
        xref_row(2, 5, 1),
        xref_row(1, offsets[4], 0),
    ];
    append_xref_stream(&mut pdf, 7, &mut rows);
    pdf
}

#[test]
fn object_stream_marked_as_compressed_in_itself_does_not_overflow_the_stack() {
    // The xref says object stream 5 is stored inside itself. Its /Length is direct, so no
    // cycle is involved; stream 6's /Length does come from an object inside stream 5.
    let pdf = pdf_marked_as_compressed_in_itself();

    let document = Document::load_mem(&pdf).expect("the rest of the document is intact");
    assert_eq!(document.get_pages().len(), 1);
}
