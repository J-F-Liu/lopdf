use super::{Dictionary, Stream};
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Xref {
	/// Entries for indirect object.
	pub entries: BTreeMap<u32, XrefEntry>,

	/// Total number of entries (including free entries), equal to the highest object number plus 1.
	pub size: u32,
}

#[derive(Debug, Clone)]
pub enum XrefEntry {
	Free,
	Normal { offset: u32, generation: u16 },
	Compressed { container: u32, index: u16 },
}

impl Xref {
	pub fn new(size: u32) -> Xref {
		Xref { entries: BTreeMap::new(), size }
	}

	pub fn get(&self, id: u32) -> Option<&XrefEntry> {
		self.entries.get(&id)
	}

	pub fn insert(&mut self, id: u32, entry: XrefEntry) {
		self.entries.insert(id, entry);
	}

	pub fn extend(&mut self, xref: Xref) {
		for (id, entry) in xref.entries {
			self.entries.entry(id).or_insert(entry);
		}
	}

	pub fn clear(&mut self) {
		self.entries.clear()
	}

	pub fn max_id(&self) -> u32 {
		match self.entries.keys().max() {
			Some(&id) => id,
			None => 0,
		}
	}
}

use crate::object::Object;
use self::XrefEntry::*;
impl XrefEntry {
	pub fn is_normal(&self) -> bool {
		match *self {
			Normal { .. } => true,
			_ => false,
		}
	}

	pub fn is_compressed(&self) -> bool {
		match *self {
			Compressed { .. } => true,
			_ => false,
		}
	}
}

pub fn decode_xref_stream(mut stream: Stream) -> Result<(Xref, Dictionary)> {
	stream.decompress();
	let mut dict = stream.dict;
	let mut reader = Cursor::new(stream.content);
	let size = dict.get(b"Size").and_then(Object::as_i64).ok_or(Error::Trailer)?;
	let mut xref = Xref::new(size as u32);
	{
		let section_indice = dict
			.get(b"Index")
			.and_then(Object::as_array)
			.map(|array| array.iter().map(|n| n.as_i64().unwrap()).collect())
			.unwrap_or_else(|| vec![0, size]);
		let field_widths: Vec<usize> = dict
			.get(b"W")
			.and_then(Object::as_array)
			.map(|array| array.iter().map(|n| n.as_i64().unwrap() as usize).collect())
			.ok_or(Error::Trailer)?;
		let mut bytes1 = vec![0_u8; field_widths[0]];
		let mut bytes2 = vec![0_u8; field_widths[1]];
		let mut bytes3 = vec![0_u8; field_widths[2]];

		for i in 0..section_indice.len() / 2 {
			let start = section_indice[2 * i];
			let count = section_indice[2 * i + 1];

			for j in 0..count {
				let entry_type = if !bytes1.is_empty() { read_big_endian_interger(&mut reader, bytes1.as_mut_slice()) } else { 1 };
				match entry_type {
					0 => {
						//free object
						read_big_endian_interger(&mut reader, bytes2.as_mut_slice());
						read_big_endian_interger(&mut reader, bytes3.as_mut_slice());
					}
					1 => {
						//normal object
						let offset = read_big_endian_interger(&mut reader, bytes2.as_mut_slice());
						let generation = if !bytes3.is_empty() { read_big_endian_interger(&mut reader, bytes3.as_mut_slice()) } else { 0 } as u16;
						xref.insert(
							(start + j) as u32,
							XrefEntry::Normal {
								offset,
								generation,
							},
						);
					}
					2 => {
						//compressed object
						let container = read_big_endian_interger(&mut reader, bytes2.as_mut_slice());
						let index = read_big_endian_interger(&mut reader, bytes3.as_mut_slice()) as u16;
						xref.insert((start + j) as u32, XrefEntry::Compressed { container, index });
					}
					_ => {}
				}
			}
		}
	}
	dict.remove(b"Length");
	dict.remove(b"W");
	dict.remove(b"Index");
	Ok((xref, dict))
}

fn read_big_endian_interger(reader: &mut Cursor<Vec<u8>>, buffer: &mut [u8]) -> u32 {
	reader.read_exact(buffer).unwrap();
	let mut value = 0;
	for &mut byte in buffer {
		value = (value << 8) + u32::from(byte);
	}
	value
}
