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

		let mut reader = Reader {
			buffer: buffer,
			document: Document::new(),
		};

		reader.read()?;
		Ok(reader.document)
	}
}

pub struct Reader {
	buffer: Vec<u8>,
	document: Document,
}

impl Reader {
	// fn new()
	fn read(&mut self) -> Result<()> {
		let mut input = DataInput::new(&self.buffer);
		// The document structure can be expressed in PEG as:
		//   document <- header indirect_object* xref trailer xref_start
		let version = parser::header().parse(&mut input)
			.map_err(|_|Error::new(ErrorKind::InvalidData, "Not a valid PDF file (header)."))?;

		let xref_start = Self::get_xref_start(&self.buffer, &mut input)?;
		input.jump_to(xref_start);

		let xref = parser::xref().parse(&mut input)
			.map_err(|_|Error::new(ErrorKind::InvalidData, "Not a valid PDF file (xref)."))?;

		let trailer = parser::trailer().parse(&mut input)
			.map_err(|_|Error::new(ErrorKind::InvalidData, "Not a valid PDF file (trailer)."))?;

		self.document.version = version;
		self.document.max_id = trailer.get("Size").and_then(|value| value.as_i64()).unwrap() as u32 - 1;
		self.document.trailer = trailer;
		self.document.reference_table = xref;

		for (_id, &(_gen, offset)) in &self.document.reference_table {
			let (object_id, object) = self.read_object(offset as usize)?;
			self.document.objects.insert(object_id, object);
		}

		Ok(())
	}

	/// Get object offset by object id.
	fn get_offset(&self, id: ObjectId) -> Option<u64> {
		if let Some(&(gen, offset)) = self.document.reference_table.get(&id.0) {
			if gen == id.1 { Some(offset) } else { None }
		} else {
			None
		}
	}

	pub fn get_object(&self, id: ObjectId) -> Option<Object> {
		if let Some(offset) = self.get_offset(id) {
			if let Ok((_, obj)) = self.read_object(offset as usize) {
				return Some(obj);
			}
		}
		return None;
	}

	pub fn print_xref_size(&self) {
		println!("xref has {} entires", self.document.reference_table.len());
	}

	fn read_object(&self, offset: usize) -> Result<(ObjectId, Object)> {
		let mut input = DataInput::new(&self.buffer);
		input.jump_to(offset);
		parser::indirect_object(self).parse(&mut input)
			.map_err(|err|Error::new(ErrorKind::InvalidData, format!("Not a valid PDF file (read object at {}).\n{:?}", offset, err)))
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
