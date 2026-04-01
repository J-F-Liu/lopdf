use lopdf::Document;
use std::time::Instant;

#[test]
fn page_count_meta_data_performance_test() {
    let document_path = "tests/fixtures/test.pdf";
    let start_time = Instant::now();
    let doc = Document::load_metadata(document_path).expect("Failed to load document");
    let result = doc.page_count;
    let elapsed_time = start_time.elapsed();
    println!("--- Meta Page Count Stats ---");
    println!(
        "Page count: {} in {:.2}s",
        result,
        elapsed_time.as_secs_f64()
    );
    assert_eq!(result, 100);
}

#[test]
fn page_count_performance_test() {
    let document_path = "tests/fixtures/test.pdf";
    let start_time = Instant::now();
    let doc = Document::load(document_path).expect("Failed to load document");
    let result = doc.get_pages().len();
    let elapsed_time = start_time.elapsed();
    println!("--- Page Count Stats ---");
    println!(
        "Page count: {} in {:.2}s",
        result,
        elapsed_time.as_secs_f64()
    );
    assert_eq!(result, 100);
}
