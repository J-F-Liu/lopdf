// Only run test when parser is enabled
#![cfg(any(feature = "pom_parser", feature = "nom_parser"))]

use lopdf::{Document, IncrementalDocument, Result};
use tempfile::tempdir;

#[test]
fn load_incremental_file_as_linear_file() -> Result<()> {
    let doc = Document::load("assets/Incremental.pdf")?;
    assert_eq!(doc.version, "1.5".to_string());

    Ok(())
}

#[test]
fn load_incremental_file() -> Result<()> {
    let mut doc = IncrementalDocument::load("assets/Incremental.pdf")?;
    assert_eq!(doc.get_prev_documents().version, "1.5".to_string());

    // Create temporary folder to store file.
    let temp_dir = tempdir()?;
    let file_path = temp_dir.path().join("test_4_incremental.pdf");
    doc.save(file_path)?;

    Ok(())
}
