//! Streams whose `/Length` is an indirect reference to an integer inside an object stream:
//! the decoded container is reused for every such length rather than re-inflated each time.

use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use lopdf::{Document, Object};

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// One page whose `/Contents` is `count` streams. The `/Length` of content
/// stream `i` is object `length_of(i)`, and all lengths live in one compressed
/// object stream.
fn pdf_with_lengths_in_an_object_stream(count: u32) -> Vec<u8> {
    let first_content = 4;
    let first_length = first_content + count;
    let container = first_length + count;
    let xref = container + 1;
    let size = xref + 1;

    let contents: Vec<String> = (0..count).map(|i| format!("BT /F1 12 Tf ({i}) Tj ET")).collect();
    let refs: Vec<String> = (0..count).map(|i| format!("{} 0 R", first_content + i)).collect();

    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0usize; size as usize];
    let mut put = |pdf: &mut Vec<u8>, number: u32, body: Vec<u8>| {
        offsets[number as usize] = pdf.len();
        pdf.extend_from_slice(&body);
    };
    put(
        &mut pdf,
        1,
        b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_vec(),
    );
    put(
        &mut pdf,
        2,
        b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".to_vec(),
    );
    put(
        &mut pdf,
        3,
        format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents [{}] >>\nendobj\n",
            refs.join(" ")
        )
        .into_bytes(),
    );
    for (i, content) in contents.iter().enumerate() {
        let number = first_content + i as u32;
        let length = first_length + i as u32;
        put(
            &mut pdf,
            number,
            format!("{number} 0 obj\n<< /Length {length} 0 R >>\nstream\n{content}\nendstream\nendobj\n").into_bytes(),
        );
    }

    let mut header = String::new();
    let mut body = String::new();
    for (i, content) in contents.iter().enumerate() {
        header.push_str(&format!("{} {} ", first_length + i as u32, body.len()));
        body.push_str(&format!("{} ", content.len()));
    }
    let packed = deflate(format!("{header}{body}").as_bytes());
    let mut stream = format!(
        "{container} 0 obj\n<< /Type /ObjStm /N {count} /First {} /Filter /FlateDecode /Length {} >>\nstream\n",
        header.len(),
        packed.len()
    )
    .into_bytes();
    stream.extend_from_slice(&packed);
    stream.extend_from_slice(b"\nendstream\nendobj\n");
    put(&mut pdf, container, stream);

    let xref_offset = pdf.len();
    let mut table = Vec::new();
    for number in 0..size {
        let row: (u8, u32, u16) = match number {
            0 => (0, 0, 0xFFFF),
            n if (first_length..container).contains(&n) => (2, container, (n - first_length) as u16),
            n if n == xref => (1, xref_offset as u32, 0),
            n => (1, offsets[n as usize] as u32, 0),
        };
        table.push(row.0);
        table.extend_from_slice(&row.1.to_be_bytes());
        table.extend_from_slice(&row.2.to_be_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "{xref} 0 obj\n<< /Type /XRef /Size {size} /W [1 4 2] /Root 1 0 R /Length {} >>\nstream\n",
            table.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&table);
    pdf.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes());
    pdf
}

#[test]
fn every_stream_gets_its_length_from_the_object_stream() {
    let count = 50;
    let document = Document::load_mem(&pdf_with_lengths_in_an_object_stream(count)).unwrap();
    for i in 0..count {
        let stream = document.get_object((4 + i, 0)).unwrap().as_stream().unwrap();
        let expected = format!("BT /F1 12 Tf ({i}) Tj ET");
        assert_eq!(stream.content, expected.as_bytes());
        assert_eq!(
            stream.dict.get(b"Length").unwrap(),
            &Object::Integer(expected.len() as i64)
        );
    }
}
