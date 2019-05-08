use log::{error, warn};
use std::cmp;
use std::fs::File;
use std::io::{Error, ErrorKind, Read, Result};
use std::path::Path;
use std::sync::Mutex;

use rayon::prelude::*;

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
			buffer,
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
		if xref_start > self.buffer.len() {
			return Err(Error::new(ErrorKind::InvalidData, format!("Not a valid PDF file (xref_start)")));
		}

		let (mut xref, mut trailer) = parser::xref_and_trailer(&self)
			.parse(&self.buffer[xref_start..])
			.map_err(|err| Error::new(ErrorKind::InvalidData, format!("Not a valid PDF file (xref_and_trailer).\n{:?}", err)))?;

		// Read previous Xrefs of linearized or incremental updated document.
		let mut prev_xref_start = trailer.remove(b"Prev");
		while let Some(prev) = prev_xref_start.and_then(|offset| offset.as_i64()) {
			let prev = prev as usize;
			if prev > self.buffer.len() {
				return Err(Error::new(ErrorKind::InvalidData, format!("Not a valid PDF file (prev_xref_start)")));
			}
			let (prev_xref, mut prev_trailer) = parser::xref_and_trailer(&self)
				.parse(&self.buffer[prev..])
				.map_err(|err| Error::new(ErrorKind::InvalidData, format!("Not a valid PDF file (prev xref_and_trailer).\n{:?}", err)))?;
			xref.extend(prev_xref);

			// Read xref stream in hybrid-reference file
			let prev_xref_stream_start = trailer.remove(b"XRefStm");
			if let Some(prev) = prev_xref_stream_start.and_then(|offset| offset.as_i64()) {
				let prev = prev as usize;
				if prev > self.buffer.len() {
					return Err(Error::new(ErrorKind::InvalidData, format!("Not a valid PDF file (prev_xref_stream_start)")));
				}
				let (prev_xref, _) = parser::xref_and_trailer(&self)
					.parse(&self.buffer[prev..])
					.map_err(|_| Error::new(ErrorKind::InvalidData, "Not a valid PDF file (prev xref_and_trailer)."))?;
				xref.extend(prev_xref);
			}

			prev_xref_start = prev_trailer.remove(b"Prev");
		}

		let xref_entry_count = xref.max_id() + 1;
		if xref.size != xref_entry_count {
			warn!("Size entry of trailer dictionary is {}, correct value is {}.", xref.size, xref_entry_count);
			xref.size = xref_entry_count;
		}

		self.document.version = version;
		self.document.max_id = xref.size - 1;
		self.document.trailer = trailer;
		self.document.reference_table = xref;

		let zero_length_streams = Mutex::new(vec![]);
		let object_streams = Mutex::new(vec![]);

		self.document.objects = self.document.reference_table.entries.par_iter().filter_map(|(_, entry)| {
			if let XrefEntry::Normal { offset, .. } = *entry {
				let read_result = self.read_object(offset as usize);
				match read_result {
					Ok((object_id, mut object)) => {
						if let Object::Stream(ref mut stream) = object {
							if stream.dict.type_is(b"ObjStm") {
								let obj_stream = ObjectStream::new(stream);
								let mut object_streams = object_streams.lock().unwrap();
								object_streams.extend(obj_stream?.objects);
							} else if stream.content.is_empty() {
								let mut zero_length_streams = zero_length_streams.lock().unwrap();
								zero_length_streams.push(object_id);
							}
						}
						Some((object_id, object))
					}
					Err(err) => {
						error!("{:?}", err);
						None
					}
				}
			} else {
				None
			}
		}).collect();

		self.document.objects.extend(object_streams.into_inner().unwrap());

		for object_id in zero_length_streams.into_inner().unwrap() {
			if let Some(length) = self.get_stream_length(object_id) {
				if let Some(ref mut object) = self.document.get_object_mut(object_id) {
					if let Object::Stream(ref mut stream) = object {
						if let Some(start) = stream.start_position {
							let end = start + length as usize;
							stream.set_content(self.buffer[start..end].to_vec());
						}
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
					return self.document.get_object(id).and_then(Object::as_i64);
				}
				value.as_i64()
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
		if offset > self.buffer.len() {
			return Err(Error::new(ErrorKind::InvalidData, format!("Not a valid PDF file (read at offset {})", offset)));
		}
		let (id, mut object) = parser::indirect_object(self)
			.parse(&self.buffer[offset..])
			.map_err(|err| Error::new(ErrorKind::InvalidData, format!("Not a valid PDF file (read object at {}).\n{:?}", offset, err)))?;

		// Parser is invoked relative to offset, add it back here.
		if let Object::Stream(ref mut stream) = object {
			stream.offset_position(offset);
		}

		Ok((id, object))
	}

	fn get_xref_start(buffer: &[u8]) -> Result<usize> {
		let seek_pos = buffer.len() - cmp::min(buffer.len(), 512);
		Self::search_substring(buffer, b"%%EOF", seek_pos)
			.and_then(|eof_pos| Self::search_substring(buffer, b"startxref", eof_pos - 25))
			.ok_or_else(|| Error::new(ErrorKind::InvalidData, "Not a valid PDF file (xref_start)."))
			.and_then(|xref_pos| if xref_pos <= buffer.len() {
				match parser::xref_start().parse(&buffer[xref_pos..]) {
					Ok(startxref) => Ok(startxref as usize),
					Err(_) => Err(Error::new(ErrorKind::InvalidData, "Not a valid PDF file (xref_start).")),
				}
			} else {
				Err(Error::new(ErrorKind::InvalidData, "Not a valid PDF file (xref_pos)"))
			})
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
