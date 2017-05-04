use pom::{Input, DataInput};
use std::cmp;
use std::io::{Result, Read, Error, ErrorKind};

use super::{Document, Object, ObjectId};
use super::parser;
use xref::XrefEntry;
use object_stream::ObjectStream;

impl Document {

	/// Load PDF document from specified file path.
	pub fn load<R: Read>(mut source: R) -> Result<Document> {

		let mut buffer = Vec::new();
		source.read_to_end(&mut buffer)?;

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
	/// Read whole document.
	fn read(&mut self) -> Result<()> {
		let mut input = DataInput::new(&self.buffer);
		// The document structure can be expressed in PEG as:
		//   document <- header indirect_object* xref trailer xref_start
		let version = parser::header().parse(&mut input)
			.map_err(|_|Error::new(ErrorKind::InvalidData, "Not a valid PDF file (header)."))?;

		let xref_start = Self::get_xref_start(&self.buffer, &mut input)?;
		input.jump_to(xref_start);

		let (mut xref, mut trailer) = parser::xref_and_trailer(&self).parse(&mut input)
			.map_err(|_|Error::new(ErrorKind::InvalidData, "Not a valid PDF file (xref_and_trailer)."))?;

		// Read previous Xrefs of linearized or incremental updated document.
		let mut prev_xref_start = trailer.remove("Prev");
		while let Some(prev) = prev_xref_start.and_then(|offset|offset.as_i64()) {
			input.jump_to(prev as usize);
			let (prev_xref, mut prev_trailer) = parser::xref_and_trailer(&self).parse(&mut input)
				.map_err(|_|Error::new(ErrorKind::InvalidData, "Not a valid PDF file (prev xref_and_trailer)."))?;
			xref.extend(prev_xref);

			// Read xref stream in hybrid-reference file
			let prev_xref_stream_start = trailer.remove("XRefStm");
			if let Some(prev) = prev_xref_stream_start.and_then(|offset|offset.as_i64()) {
				input.jump_to(prev as usize);
				let (prev_xref, _) = parser::xref_and_trailer(&self).parse(&mut input)
					.map_err(|_|Error::new(ErrorKind::InvalidData, "Not a valid PDF file (prev xref_and_trailer)."))?;
				xref.extend(prev_xref);
			}

			prev_xref_start = prev_trailer.remove("Prev");
		}

		self.document.version = version;
		self.document.max_id = xref.size - 1;
		self.document.trailer = trailer;
		self.document.reference_table = xref;

		for entry in self.document.reference_table.entries.values().filter(|entry|entry.is_normal()) {
			match *entry {
				XrefEntry::Normal{offset, ..} => {
					let (object_id, mut object) = self.read_object(offset as usize)?;

					match object {
						Object::Stream(ref mut stream) => if stream.dict.type_is(b"ObjStm") {
							self.document.streams.insert(object_id.0, ObjectStream::new(stream));
						},
						_ => {}
					}

					self.document.objects.insert(object_id, object);
				},
				_ => {},
			};
		}

		Ok(())
	}

	/// Get object offset by object id.
	fn get_offset(&self, id: ObjectId) -> Option<u32> {
		if let Some(entry) = self.document.reference_table.get(id.0) {
			match *entry {
				XrefEntry::Normal{offset, generation} => {
					if id.1 == generation { Some(offset) } else { None }
				},
				_ => None,
			}
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

	use std::fs::File;
	let file = File::open("assets/example.pdf").unwrap();
	let mut doc = Document::load(file).unwrap();
	assert_eq!(doc.version, "1.5");
	let mut file = File::create("test_2_load.pdf").unwrap();
	doc.save(&mut file).unwrap();
}
