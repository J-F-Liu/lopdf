use std::env;
use std::path::{Path, PathBuf};

use lopdf::Document;

#[cfg(feature = "async")]
use tokio::runtime::Builder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = match env::args_os().nth(1) {
        Some(input) => PathBuf::from(input),
        None => panic!("Please provide a PDF file as first argument"),
    };

    let output = output_path(&input);
    let mut doc = load_document(&input)?;
    doc.delete_outlines()?;
    doc.save(&output)?;

    println!("Saved {}", output.display());

    Ok(())
}

fn output_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .expect("input file has no file stem")
        .to_string_lossy();

    input.with_file_name(format!("{stem}-NOTOC.pdf"))
}

#[cfg(not(feature = "async"))]
fn load_document<P: AsRef<Path>>(path: P) -> Result<Document, lopdf::Error> {
    Document::load(path)
}

#[cfg(feature = "async")]
fn load_document<P: AsRef<Path>>(path: P) -> Result<Document, lopdf::Error> {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move { Document::load(path).await })
}
