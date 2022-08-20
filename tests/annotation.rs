// Only run test when parser is enabled
#![cfg(any(feature = "pom_parser", feature = "nom_parser"))]

use lopdf::{Document, Result};


#[test]
fn annotation_count() -> Result<()> {
    // This test file from the pdfcpu repository,
    // https://github.com/pdfcpu/pdfcpu/blob/master/pkg/samples/basic/AnnotationDemo.pdf
    let doc = Document::load("assets/AnnotationDemo.pdf")?;
    assert_eq!(doc.version, "1.7".to_string());
    assert_eq!(doc.page_iter().count(), 1);
    assert_eq!(doc.get_page_annotations(doc.page_iter().next().unwrap()).len(), 33);
    Ok(())
}


