use pom::{Input, DataInput};
use std::cmp;
use std::fs::File;
use std::io::{Result, Read, Error, ErrorKind};
use std::path::Path;

use super::{Document, Object, ObjectId};
use super::parser;

impl Document {
	/// Load PDF document from specified file path.
	pub fn load<P: AsRef<Path>>(path: P) -> Result<Document> {
		let mut file = File::open(path)?;
		let mut buffer = Vec::with_capacity(file.metadata()?.len() as usize);
		file.read_to_end(&mut buffer)?;
		let mut input = DataInput::new(&buffer);

		// The document structure can be expressed in PEG as:
		//   document <- header indirect_object* xref trailer xref_start
		let version = parser::header().parse(&mut input)
			.map_err(|_|Error::new(ErrorKind::InvalidData, "Not a valid PDF file (header)."))?;

		let xref_start = Self::get_xref_start(&buffer, &mut input)?;
		input.jump_to(xref_start);

		let xref = parser::xref().parse(&mut input)
			.map_err(|_|Error::new(ErrorKind::InvalidData, "Not a valid PDF file (xref)."))?;

		let trailer = parser::trailer().parse(&mut input)
			.map_err(|_|Error::new(ErrorKind::InvalidData, "Not a valid PDF file (trailer)."))?;

		let mut doc = Document::new();
		doc.version = version;

		for (_id, &(_gen, offset)) in &xref {
			let (object_id, object) = doc.read_object(&mut input, offset as usize)?;
			doc.objects.insert(object_id, object);
		}

		doc.reference_table = xref;
		doc.trailer = trailer;
		doc.max_id = doc.trailer.get("Size").and_then(|value| value.as_i64()).unwrap() as u32 - 1;
		Ok(doc)
	}

	fn read_object(&mut self, input: &mut Input<u8>, offset: usize) -> Result<(ObjectId, Object)> {
		input.jump_to(offset);
		parser::indirect_object().parse(input)
			.map_err(|_|Error::new(ErrorKind::InvalidData, format!("Not a valid PDF file (read object at {}).", offset)))
	}

	fn get_xref_start(buffer: &[u8], input: &mut Input<u8>) -> Result<usize> {
		let seek_pos = buffer.len() - cmp::min(buffer.len(), 512);
		Self::search_substring(buffer, b"%%EOF", seek_pos)
			.and_then(|eof_pos| Self::search_substring(buffer, b"startxref", eof_pos - 25))
			.and_then(|xref_pos| {
				input.jump_to(xref_pos);
				match parser::xref_start().parse(input) {
					Ok(startxref) => Some(startxref as usize),
					_ => None,
				}
			})
			.ok_or(Error::new(ErrorKind::InvalidData, "Not a valid PDF file (xref_start)."))
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
