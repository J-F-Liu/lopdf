use nom::IResult;
use std::fs::File;
use std::io::{Result, Seek, Read, SeekFrom};
use std::path::Path;

use super::{Document, Object, Dictionary, Stream, StringFormat};
use super::parser;

impl Document {
	pub fn load<P: AsRef<Path>>(path: P) -> Result<Document> {
		let mut file = File::open(path)?;
		let mut buffer = vec![];
		file.read_to_end(&mut buffer)?;

		match parser::document(&buffer) {
			IResult::Done(_, document) => Ok(document),
			IResult::Incomplete(x) => panic!("incomplete: {:?}", x),
			IResult::Error(e) => panic!("error: {:?}", e),
		}
	}
}

#[test]
fn load_document() {
	let mut doc = Document::load("test.pdf").unwrap();
	assert_eq!(doc.version, "1.5");
	doc.save("test_2_load.pdf").unwrap();
}
