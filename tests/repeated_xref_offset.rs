//! A cross-reference table can mark many entries in use at the same offset. Loading must read
//! that offset a bounded number of times, not once per entry: a real file with ~1.9M such
//! entries, all pointing at an object stream, exhausted 49 GiB in `Document::load_mem`.

use lopdf::{Document, Object};

/// A PDF whose xref stream marks `repeats` unused object numbers as in use at the offset of its
/// object stream. The stream holds a catalog, a page tree, an Info dictionary and a
/// 10,000-integer array, so each redundant load of it is expensive.
fn pdf_repeating_object_stream_offset(repeats: usize) -> Vec<u8> {
    let filler = format!("[{}]", (0..10_000).map(|n| n.to_string()).collect::<Vec<_>>().join(" "));
    let objects = [
        "<</Type/Catalog/Pages 3 0 R>>",
        "<</Type/Pages/Kids[]/Count 0>>",
        "<</Title(Planning notice)/Author(Council)>>",
        filler.as_str(),
    ];
    let mut index = String::new();
    let mut body = String::new();
    for (number, object) in (2..).zip(objects) {
        index.push_str(&format!("{number} {} ", body.len()));
        body.push_str(object);
        body.push(' ');
    }
    let content = format!("{index}{body}");

    let mut pdf = b"%PDF-1.7\n".to_vec();
    let object_stream_offset = pdf.len();
    pdf.extend(
        format!(
            "1 0 obj\n<</Type/ObjStm/N 4/First {}/Length {}>>\nstream\n{content}\nendstream\nendobj\n",
            index.len(),
            content.len()
        )
        .as_bytes(),
    );
    let xref_offset = pdf.len();

    let row = |kind: u8, field: usize, position: u16| {
        let mut row = vec![kind];
        row.extend(u32::try_from(field).unwrap().to_be_bytes());
        row.extend(position.to_be_bytes());
        row
    };
    let mut rows = vec![row(0, 0, 65535), row(1, object_stream_offset, 0)];
    rows.extend((0..4).map(|position| row(2, 1, position)));
    rows.push(row(1, xref_offset, 0));
    rows.extend(std::iter::repeat_n(row(1, object_stream_offset, 0), repeats));
    let xref = rows.concat();

    pdf.extend(
        format!(
            "6 0 obj\n<</Type/XRef/Size {}/W[1 4 2]/Root 2 0 R/Info 4 0 R/Length {}>>\nstream\n",
            rows.len(),
            xref.len()
        )
        .as_bytes(),
    );
    pdf.extend(xref);
    pdf.extend(format!("\nendstream\nendobj\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes());
    pdf
}

fn title(document: &Document) -> Vec<u8> {
    let info = document.trailer.get(b"Info").and_then(Object::as_reference).unwrap();
    let info = document.get_dictionary(info).unwrap();
    info.get(b"Title").and_then(Object::as_str).unwrap().to_vec()
}

#[test]
fn entries_repeating_an_offset_load_the_same_objects_as_one_entry() {
    let few = Document::load_mem(&pdf_repeating_object_stream_offset(10)).unwrap();

    let many = Document::load_mem(&pdf_repeating_object_stream_offset(200_000)).unwrap();

    let without_xref_stream = |document: &Document| {
        let mut objects = document.objects.clone();
        objects.remove(&(6, 0));
        objects
    };
    assert_eq!(many.objects.len(), 6);
    assert_eq!(without_xref_stream(&many), without_xref_stream(&few));
    assert_eq!(title(&many), b"Planning notice");
}
