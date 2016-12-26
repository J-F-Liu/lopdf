use nom::IResult;
use std::cmp;
use std::fs::File;
use std::io::{Result, Read, Error, ErrorKind};
use std::path::Path;

use super::{Document, Object, ObjectId, Dictionary};
use super::parser;

impl Document {
	/// Load PDF document from specified file path.
	pub fn load<P: AsRef<Path>>(path: P) -> Result<Document> {
		let mut file = File::open(path)?;
		let mut buffer = Vec::with_capacity(file.metadata()?.len() as usize);
		file.read_to_end(&mut buffer)?;

		// The document structure can be expressed in PEG as:
		//   document <- header indirect_object* xref trailer xref_start
		let version = match parser::header(&buffer) {
				IResult::Done(_, version) => Some(version),
				_ => None,
			}.ok_or(Error::new(ErrorKind::InvalidData, "Not a valid PDF file."))?;

		let xref_start = Self::get_xref_start(&buffer)?;

		let (input, xref) = match parser::xref(&buffer[xref_start..]) {
				IResult::Done(input, table) => Some((input, table)),
				_ => None,
			}.ok_or(Error::new(ErrorKind::InvalidData, "Not a valid PDF file."))?;

		let trailer = match parser::trailer(input) {
				IResult::Done(_, dict) => Some(dict),
				_ => None,
			}.ok_or(Error::new(ErrorKind::InvalidData, "Not a valid PDF file."))?;

		let mut doc = Document::new();
		doc.version = version;

		for (_id, &(_gen, offset)) in &xref {
			let (object_id, object) = doc.read_object(&buffer, offset as usize)?;
			doc.objects.insert(object_id, object);
		}

		doc.reference_table = xref;
		doc.trailer = trailer;
		doc.max_id = doc.trailer.get("Size").and_then(|value| value.as_i64()).unwrap() as u32 - 1;
		Ok(doc)
	}

	fn read_object(&mut self, buffer: &[u8], offset: usize) -> Result<(ObjectId, Object)> {
		match parser::indirect_object(&buffer[offset..]) {
			IResult::Done(_, (object_id, object)) => Ok((object_id, object)),
			_ => Err(Error::new(ErrorKind::InvalidData, "Not a valid PDF file.")),
		}
	}

	fn get_xref_start(buffer: &[u8]) -> Result<usize> {
		let seek_pos = buffer.len() - cmp::min(buffer.len(), 512);
		Self::search_substring(buffer, b"%%EOF", seek_pos)
			.and_then(|eof_pos| Self::search_substring(buffer, b"startxref", eof_pos - 25))
			.and_then(|xref_pos| match parser::xref_start(&buffer[xref_pos..]) {
				IResult::Done(_, startxref) => Some(startxref as usize),
				_ => None,
			})
			.ok_or(Error::new(ErrorKind::InvalidData, "Not a valid PDF file."))
	}

	fn search_substring(buffer: &[u8], pattern: &[u8], start_pos: usize) -> Option<usize> {
		let mut seek_pos = start_pos;
		let mut index = 0;

		while seek_pos < buffer.len() && index < pattern.len() {
			if buffer[seek_pos] == pattern[index] {
				index += 1;
			} else if index > 0 {
				seek_pos -= index;
				index = 0;
			}
			seek_pos += 1;

			if index == pattern.len() {
				return Some(seek_pos - index);
			}
		}

		return None;
	}
}

#[test]
fn load_document() {
	let mut doc = Document::load("test.pdf").unwrap();
	assert_eq!(doc.version, "1.5");
	doc.save("test_2_load.pdf").unwrap();
}
