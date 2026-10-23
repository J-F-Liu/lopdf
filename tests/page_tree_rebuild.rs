//! Lenient loading rebuilds the cross-reference table not only when it cannot be read, but also
//! when it reads and the catalog or the root of the page tree then does not load, as in a file
//! that gained or lost bytes before its objects. `LoadOptions::force_reconstruct` rebuilds it
//! whatever it reads, and a file cut off before any trailer is rebuilt around its catalog.

use lopdf::xref::XrefRecovery;
use lopdf::{Document, LoadOptions, Object, ObjectId};

/// Append objects, returning each one's `(number, physical offset)`.
fn append_objects(pdf: &mut Vec<u8>, bodies: &[(u32, String)]) -> Vec<(u32, usize)> {
    let mut offsets = Vec::new();
    for (number, body) in bodies {
        offsets.push((*number, pdf.len()));
        pdf.extend_from_slice(format!("{number} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    offsets
}

fn append_xref_trailer(pdf: &mut Vec<u8>, offsets: &[(u32, usize)], trailer: &str) {
    let startxref = pdf.len();
    pdf.extend_from_slice(b"xref\n");
    pdf.extend_from_slice(format!("0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for (_, offset) in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(format!("trailer\n{trailer}\nstartxref\n{startxref}\n%%EOF\n").as_bytes());
}

/// A two-page document: catalog 1, page tree 2, pages 3 and 4, and an Info dictionary 5.
fn bodies() -> Vec<(u32, String)> {
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_string();
    vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()),
        (2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string()),
        (3, page.clone()),
        (4, page),
        (5, "<< /Title (Two pages) >>".to_string()),
    ]
}

const TRAILER: &str = "<< /Size 6 /Root 1 0 R /Info 5 0 R >>";

fn intact_pdf() -> Vec<u8> {
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let offsets = append_objects(&mut pdf, &bodies());
    append_xref_trailer(&mut pdf, &offsets, TRAILER);
    pdf
}

/// A file with a line inserted after its header once the offsets were recorded: the
/// cross-reference table and `startxref` read as written, but every entry points before its
/// object, and none of the objects load.
fn shifted_pdf() -> Vec<u8> {
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let offsets = append_objects(&mut pdf, &bodies());
    let inserted = b"% a line added after the cross-reference table was written\n";
    pdf.splice(9..9, inserted.iter().copied());
    append_xref_trailer(&mut pdf, &offsets, TRAILER);
    pdf
}

/// A file cut off after its objects, before its cross-reference table and trailer.
fn truncated_pdf(bodies: &[(u32, String)]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.4\n".to_vec();
    append_objects(&mut pdf, bodies);
    pdf
}

fn page_ids(document: &Document) -> Vec<ObjectId> {
    document.get_pages().into_values().collect()
}

#[test]
fn a_table_that_misses_its_objects_is_rebuilt() {
    let pdf = shifted_pdf();

    let document = Document::load_mem(&pdf).unwrap();
    assert_eq!(page_ids(&document), [(3, 0), (4, 0)]);
    assert_eq!(document.xref_recovery, Some(XrefRecovery::Reconstructed));
    assert!(document.trailer.get(b"Info").is_ok());

    let metadata = Document::load_metadata_mem(&pdf).unwrap();
    assert_eq!(metadata.page_count, 2);
    assert_eq!(metadata.title.as_deref(), Some("Two pages"));
    assert_eq!(metadata.xref_recovery, Some(XrefRecovery::Reconstructed));
}

#[test]
fn strict_loading_does_not_rebuild_a_table_that_misses_its_objects() {
    let strict = LoadOptions {
        strict: true,
        ..Default::default()
    };
    assert!(Document::load_mem_with_options(&shifted_pdf(), strict).is_err());
}

#[test]
fn a_filtered_load_is_not_rebuilt() {
    // A filter can drop the page tree on purpose.
    let options = LoadOptions::with_filter(|id, object| Some((id, object.clone())));
    let document = Document::load_mem_with_options(&shifted_pdf(), options).unwrap();
    assert_eq!(document.xref_recovery, None);
}

#[test]
fn a_table_whose_objects_load_is_kept() {
    let document = Document::load_mem(&intact_pdf()).unwrap();
    assert_eq!(page_ids(&document), [(3, 0), (4, 0)]);
    assert_eq!(document.xref_recovery, None);

    // A page tree that loads and is empty is kept too.
    let mut bodies = bodies();
    bodies[1].1 = "<< /Type /Pages /Kids [] /Count 0 >>".to_string();
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let offsets = append_objects(&mut pdf, &bodies);
    append_xref_trailer(&mut pdf, &offsets, TRAILER);
    let document = Document::load_mem(&pdf).unwrap();
    assert!(document.get_pages().is_empty());
    assert_eq!(document.xref_recovery, None);
}

#[test]
fn force_reconstruct_rebuilds_a_table_that_reads() {
    let force = LoadOptions {
        force_reconstruct: true,
        ..Default::default()
    };
    let document = Document::load_mem_with_options(&intact_pdf(), force.clone()).unwrap();
    assert_eq!(page_ids(&document), [(3, 0), (4, 0)]);
    assert_eq!(document.xref_recovery, Some(XrefRecovery::Reconstructed));
    assert_eq!(document.objects, Document::load_mem(&intact_pdf()).unwrap().objects);

    let document = Document::load_mem_with_options(&shifted_pdf(), force).unwrap();
    assert_eq!(page_ids(&document), [(3, 0), (4, 0)]);
    assert_eq!(document.xref_recovery, Some(XrefRecovery::Reconstructed));
}

#[test]
fn force_reconstruct_reads_the_table_when_the_scan_finds_no_trailer() {
    // The trailer's `/Root` names an object that has no header in the file, and no catalog
    // can stand in for it, so the scan finds no trailer to use.
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let offsets = append_objects(&mut pdf, &bodies()[1..]);
    pdf.extend_from_slice(b"% the catalog follows without an object header\n");
    let force = LoadOptions {
        force_reconstruct: true,
        ..Default::default()
    };
    append_xref_trailer(&mut pdf, &offsets, "<< /Size 6 /Root 9 0 R >>");
    let document = Document::load_mem_with_options(&pdf, force).unwrap();
    assert_eq!(document.xref_recovery, None);
}

#[test]
fn force_reconstruct_is_ignored_by_strict_loading() {
    let options = LoadOptions {
        strict: true,
        force_reconstruct: true,
        ..Default::default()
    };
    let document = Document::load_mem_with_options(&intact_pdf(), options).unwrap();
    assert_eq!(document.xref_recovery, None);
}

#[test]
fn a_file_cut_off_before_its_trailer_is_rebuilt_around_its_catalog() {
    let pdf = truncated_pdf(&bodies());

    let document = Document::load_mem(&pdf).unwrap();
    assert_eq!(page_ids(&document), [(3, 0), (4, 0)]);
    assert_eq!(document.xref_recovery, Some(XrefRecovery::Reconstructed));
    assert_eq!(document.trailer.get(b"Root").unwrap(), &Object::Reference((1, 0)));
    // Nothing says which object was the Info dictionary.
    assert!(document.trailer.get(b"Info").is_err());

    let metadata = Document::load_metadata_mem(&pdf).unwrap();
    assert_eq!(metadata.page_count, 2);
    assert_eq!(metadata.xref_recovery, Some(XrefRecovery::Reconstructed));

    // The rebuilt document saves to a file that loads as written.
    let mut document = document;
    let mut saved = Vec::new();
    document.save_to(&mut saved).unwrap();
    let reloaded = Document::load_mem(&saved).unwrap();
    assert_eq!(reloaded.xref_recovery, None);
    assert_eq!(reloaded.get_pages().len(), 2);
}

#[test]
fn the_newest_catalog_whose_pages_were_found_stands_in_for_the_trailer() {
    let mut bodies = bodies();
    // An incremental update appended a newer catalog, then the file was cut off. A catalog
    // after it names a page tree that is not in the file.
    bodies.push((1, "<< /Type /Catalog /Pages 2 0 R /Lang (en) >>".to_string()));
    bodies.push((6, "<< /Type /Catalog /Pages 9 0 R >>".to_string()));
    let document = Document::load_mem(&truncated_pdf(&bodies)).unwrap();
    assert_eq!(document.trailer.get(b"Root").unwrap(), &Object::Reference((1, 0)));
    assert!(document.catalog().unwrap().has(b"Lang"));
    assert_eq!(document.get_pages().len(), 2);
}

#[test]
fn a_file_that_names_encrypt_is_not_rebuilt_around_its_catalog() {
    // Without `/Encrypt` and `/ID` in the trailer, its strings and streams could not be
    // decrypted.
    let mut bodies = bodies();
    bodies.push((6, "<< /Filter /Standard /V 1 /R 2 /O <00> /U <00> /P -4 >>".to_string()));
    bodies.push((7, "<< /Encrypt 6 0 R >>".to_string()));
    assert!(Document::load_mem(&truncated_pdf(&bodies)).is_err());
}

#[test]
fn a_file_without_a_catalog_still_fails_to_load() {
    let bodies = bodies();
    assert!(Document::load_mem(&truncated_pdf(&bodies[1..])).is_err());
    assert!(Document::load_metadata_mem(&truncated_pdf(&bodies[1..])).is_err());
}
