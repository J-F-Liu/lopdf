use lopdf::Document;

#[test]
fn test_metadata_extraction_basic() {
    let metadata = Document::load_metadata("assets/example.pdf").unwrap();

    assert_eq!(metadata.version, "1.5");
    assert!(metadata.page_count > 0);
}

#[test]
fn test_metadata_extraction_page_count() {
    let metadata = Document::load_metadata("assets/example.pdf").unwrap();
    assert!(metadata.page_count > 0);

    let doc = Document::load("assets/example.pdf").unwrap();
    let pages = doc.get_pages();
    assert_eq!(metadata.page_count, pages.len() as u32);
}

#[test]
fn test_metadata_extraction_unicode() {
    let metadata = Document::load_metadata("assets/unicode.pdf").unwrap();
    assert!(metadata.page_count > 0);
}

#[test]
fn test_metadata_extraction_from_memory() {
    let buffer = std::fs::read("assets/example.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();

    assert_eq!(metadata.version, "1.5");
    assert!(metadata.page_count > 0);

    let file_metadata = Document::load_metadata("assets/example.pdf").unwrap();
    assert_eq!(metadata.version, file_metadata.version);
    assert_eq!(metadata.page_count, file_metadata.page_count);
}

#[test]
fn test_metadata_extraction_incremental() {
    let metadata = Document::load_metadata("assets/Incremental.pdf").unwrap();
    assert!(metadata.page_count > 0);
}

#[test]
fn test_metadata_extraction_annotation_demo() {
    let metadata = Document::load_metadata("assets/AnnotationDemo.pdf").unwrap();
    assert!(metadata.page_count > 0);
}
