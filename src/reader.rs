use log::error;
use std::cmp;
use std::fs::File;
use std::io::{Error, ErrorKind, Read, Result};
use std::path::Path;

use super::parser;
use super::{Document, Object, ObjectId};
use crate::object_stream::ObjectStream;
use crate::xref::XrefEntry;

impl Document {
	/// Load PDF document from specified file path.
	#[inline]
	pub fn load<P: AsRef<Path>>(path: P) -> Result<Document> {
		let file = File::open(path)?;
		let buffer = Vec::with_capacity(file.metadata()?.len() as usize);
		Self::load_internal(file, buffer)
	}

	/// Load PDF document from arbitrary source
	#[inline]
	pub fn load_from<R: Read>(source: R) -> Result<Document> {
		let buffer = Vec::<u8>::new();
		Self::load_internal(source, buffer)
	}

	fn load_internal<R: Read>(mut source: R, mut buffer: Vec<u8>) -> Result<Document> {
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
		// The document structure can be expressed in PEG as:
		//   document <- header indirect_object* xref trailer xref_start
		let version = parser::header().parse(&self.buffer).map_err(|_| Error::new(ErrorKind::InvalidData, "Not a valid PDF file (header)."))?;

		let xref_start = Self::get_xref_start(&self.buffer)?;

		let (mut xref, mut trailer) = parser::xref_and_trailer(&self)
			.parse(&self.buffer[xref_start..])
			.map_err(|err| Error::new(ErrorKind::InvalidData, format!("Not a valid PDF file (xref_and_trailer).\n{:?}", err)))?;

		// Read previous Xrefs of linearized or incremental updated document.
		let mut prev_xref_start = trailer.remove(b"Prev");
		while let Some(prev) = prev_xref_start.and_then(|offset| offset.as_i64()) {
			let (prev_xref, mut prev_trailer) = parser::xref_and_trailer(&self)
				.parse(&self.buffer[prev as usize..])
				.map_err(|err| Error::new(ErrorKind::InvalidData, format!("Not a valid PDF file (prev xref_and_trailer).\n{:?}", err)))?;
			xref.extend(prev_xref);

			// Read xref stream in hybrid-reference file
			let prev_xref_stream_start = trailer.remove(b"XRefStm");
			if let Some(prev) = prev_xref_stream_start.and_then(|offset| offset.as_i64()) {
				let (prev_xref, _) = parser::xref_and_trailer(&self)
					.parse(&self.buffer[prev as usize..])
					.map_err(|_| Error::new(ErrorKind::InvalidData, "Not a valid PDF file (prev xref_and_trailer)."))?;
				xref.extend(prev_xref);
			}

			prev_xref_start = prev_trailer.remove(b"Prev");
		}

		self.document.version = version;
		self.document.max_id = xref.size - 1;
		self.document.trailer = trailer;
		self.document.reference_table = xref;

		let mut zero_length_streams = vec![];
		for entry in self.document.reference_table.entries.values().filter(|entry| entry.is_normal()) {
			match *entry {
				XrefEntry::Normal { offset, .. } => {
					let read_result = self.read_object(offset as usize);
					match read_result {
						Ok((object_id, mut object)) => {
							match object {
								Object::Stream(ref mut stream) => {
									if stream.dict.type_is(b"ObjStm") {
										let mut obj_stream = ObjectStream::new(stream);
										self.document.objects.append(&mut obj_stream.objects);
									} else if stream.content.is_empty() {
										zero_length_streams.push(object_id);
									}
								}
								_ => {}
							}
							self.document.objects.insert(object_id, object);
						}
						Err(err) => {
							error!("{:?}", err);
						}
					}
				}
				_ => {}
			};
		}

		for object_id in zero_length_streams {
			if let Some(length) = self.get_stream_length(object_id) {
				if let Some(ref mut object) = self.document.get_object_mut(object_id) {
					match object {
						Object::Stream(ref mut stream) => {
							if let Some(start) = stream.start_position {
								let end = start + length as usize;
								stream.set_content(self.buffer[start..end].to_vec());
							}
						}
						_ => {}
					}
				}
			}
		}

		Ok(())
	}

	fn get_stream_length(&self, object_id: ObjectId) -> Option<i64> {
		let object = self.document.get_object(object_id).unwrap();
		match object {
			Object::Stream(ref stream) => stream.dict.get(b"Length").and_then(|value| {
				if let Some(id) = value.as_reference() {
					return self.document.get_object(id).and_then(|value| value.as_i64());
				}
				return value.as_i64();
			}),
			_ => None,
		}
	}

	/// Get object offset by object id.
	fn get_offset(&self, id: ObjectId) -> Option<u32> {
		if let Some(entry) = self.document.reference_table.get(id.0) {
			match *entry {
				XrefEntry::Normal { offset, generation } => {
					if id.1 == generation {
						Some(offset)
					} else {
						None
					}
				}
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
		None
	}

	fn read_object(&self, offset: usize) -> Result<(ObjectId, Object)> {
		parser::indirect_object(self)
			.parse(&self.buffer[offset..])
			.map_err(|err| Error::new(ErrorKind::InvalidData, format!("Not a valid PDF file (read object at {}).\n{:?}", offset, err)))
	}

	fn get_xref_start(buffer: &[u8]) -> Result<usize> {
		let seek_pos = buffer.len() - cmp::min(buffer.len(), 512);
		Self::search_substring(buffer, b"%%EOF", seek_pos)
			.and_then(|eof_pos| Self::search_substring(buffer, b"startxref", eof_pos - 25))
			.and_then(|xref_pos| match parser::xref_start().parse(&buffer[xref_pos..]) {
				Ok(startxref) => Some(startxref as usize),
				_ => None,
			})
			.ok_or_else(|| Error::new(ErrorKind::InvalidData, "Not a valid PDF file (xref_start)."))
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

		None
	}
}

#[test]
fn load_document() {
	let mut doc = Document::load("assets/example.pdf").unwrap();
	assert_eq!(doc.version, "1.5");
	doc.save("test_2_load.pdf").unwrap();
}
