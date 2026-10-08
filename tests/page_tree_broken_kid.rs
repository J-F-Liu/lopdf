use lopdf::Document;

/// Append objects, returning each one's `(number, physical offset)`.
fn append_objects(pdf: &mut Vec<u8>, bodies: &[(u32, String)]) -> Vec<(u32, usize)> {
    let mut offsets = Vec::new();
    for (number, body) in bodies {
        offsets.push((*number, pdf.len()));
        pdf.extend_from_slice(format!("{number} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    offsets
}

fn append_xref_trailer(pdf: &mut Vec<u8>, offsets: &[(u32, usize)], size: u32, root: &str) {
    let startxref = pdf.len();
    pdf.extend_from_slice(b"xref\n");
    pdf.extend_from_slice(format!("0 {size}\n0000000000 65535 f \n").as_bytes());
    let mut by_number: Vec<(u32, usize)> = offsets.to_vec();
    by_number.sort_by_key(|(n, _)| *n);
    // Fill every slot [1, size) so a deliberately-missing object number still
    // gets a free-entry placeholder (its reference stays dangling).
    let mut next = by_number.into_iter().peekable();
    for number in 1..size {
        if next.peek().map(|(n, _)| *n) == Some(number) {
            let (_, offset) = next.next().unwrap();
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        } else {
            pdf.extend_from_slice(b"0000000000 65535 f \n");
        }
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root {root} >>\nstartxref\n{startxref}\n%%EOF\n").as_bytes(),
    );
}

fn new_pdf() -> Vec<u8> {
    b"%PDF-1.4\n".to_vec()
}

/// 4-page document (objects 10..=13 are the pages) whose /Kids array and
/// page objects can be individually broken by the caller.
fn four_page_bodies(kids: &str, page_overrides: &[(u32, &str)]) -> Vec<(u32, String)> {
    let mut bodies = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()),
        (2, format!("<< /Type /Pages /Kids [{kids}] /Count 4 >>")),
    ];
    for i in 0..4u32 {
        let num = 10 + i;
        let default = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >>".to_string();
        let body = page_overrides
            .iter()
            .find(|(n, _)| *n == num)
            .map(|(_, b)| b.to_string())
            .unwrap_or(default);
        bodies.push((num, body));
    }
    bodies
}

#[test]
fn kid_referencing_missing_object_is_kept_as_blank_page() {
    // /Kids references object 99, which does not exist anywhere in the file.
    let bodies = four_page_bodies("10 0 R 11 0 R 99 0 R 12 0 R 13 0 R", &[]);
    let mut pdf = new_pdf();
    let offsets = append_objects(&mut pdf, &bodies);
    append_xref_trailer(&mut pdf, &offsets, 14, "1 0 R");

    let doc = Document::load_mem(&pdf).unwrap();
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 5, "the missing-object kid must still occupy a page slot");
    assert!(doc.get_object((99, 0)).is_err());
}

#[test]
fn out_of_range_unparseable_page_dict_is_kept_as_blank_page() {
    // Page object 10 (the first page) is corrupted: a bare integer sits where
    // a dictionary key must be, so it can no longer be read as a /Page dict.
    let bodies = four_page_bodies(
        "10 0 R 11 0 R 12 0 R 13 0 R",
        &[(10, "<< /Type /Page /Parent 2 0 R 7 /MediaBox [0 0 300 200] >>")],
    );
    let mut pdf = new_pdf();
    let offsets = append_objects(&mut pdf, &bodies);
    append_xref_trailer(&mut pdf, &offsets, 14, "1 0 R");

    let doc = Document::load_mem(&pdf).unwrap();
    assert_eq!(
        doc.get_pages().len(),
        4,
        "a corrupted page dict must still occupy a page slot"
    );
}

#[test]
fn in_range_unparseable_page_dict_is_kept_as_blank_page() {
    // Same corruption as above, but on page object 11 (the second page)
    // instead of the first.
    let bodies = four_page_bodies(
        "10 0 R 11 0 R 12 0 R 13 0 R",
        &[(11, "<< /Type /Page /Parent 2 0 R 7 /MediaBox [0 0 300 200] >>")],
    );
    let mut pdf = new_pdf();
    let offsets = append_objects(&mut pdf, &bodies);
    append_xref_trailer(&mut pdf, &offsets, 14, "1 0 R");

    let doc = Document::load_mem(&pdf).unwrap();
    assert_eq!(
        doc.get_pages().len(),
        4,
        "a corrupted page dict must still occupy a page slot"
    );
}

#[test]
fn page_iter_order_keeps_broken_kid_in_place() {
    let bodies = four_page_bodies("10 0 R 11 0 R 99 0 R 12 0 R 13 0 R", &[]);
    let mut pdf = new_pdf();
    let offsets = append_objects(&mut pdf, &bodies);
    append_xref_trailer(&mut pdf, &offsets, 14, "1 0 R");

    let doc = Document::load_mem(&pdf).unwrap();
    let ids: Vec<_> = doc.page_iter().collect();
    assert_eq!(ids.len(), 5);
    // Index 2 (third slot) is the dangling reference itself, kept in place
    // so every later page's index still lines up with the original /Kids order.
    assert_eq!(ids[2], (99, 0));
    assert_eq!(ids[3], (12, 0));
}
