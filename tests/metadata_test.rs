use lopdf::Document;

#[test]
fn test_metadata_extraction_basic() {
    let buffer = std::fs::read("assets/example.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();

    assert_eq!(metadata.version, "1.5");
    assert!(metadata.page_count > 0);
}

#[test]
fn test_metadata_extraction_page_count() {
    let buffer = std::fs::read("assets/example.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();
    assert!(metadata.page_count > 0);

    let buffer = std::fs::read("assets/example.pdf").unwrap();
    let doc = Document::load_mem(&buffer).unwrap();
    let pages = doc.get_pages();
    assert_eq!(metadata.page_count, pages.len() as u32);
}

#[test]
fn test_metadata_extraction_unicode() {
    let buffer = std::fs::read("assets/unicode.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();
    assert!(metadata.page_count > 0);
}

#[test]
fn test_metadata_extraction_from_memory() {
    let buffer = std::fs::read("assets/example.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();

    assert_eq!(metadata.version, "1.5");
    assert!(metadata.page_count > 0);
}

#[test]
fn test_metadata_extraction_incremental() {
    let buffer = std::fs::read("assets/Incremental.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();
    assert!(metadata.page_count > 0);
}

#[test]
fn test_metadata_extraction_annotation_demo() {
    let buffer = std::fs::read("assets/AnnotationDemo.pdf").unwrap();
    let metadata = Document::load_metadata_mem(&buffer).unwrap();
    assert!(metadata.page_count > 0);
}
