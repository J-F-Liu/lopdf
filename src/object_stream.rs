use super::parser;
use super::{Object, ObjectId, Stream};
use std::collections::BTreeMap;
use std::str::FromStr;

use rayon::prelude::*;

#[derive(Debug)]
pub struct ObjectStream {
	pub objects: BTreeMap<ObjectId, Object>,
}

impl ObjectStream {
	pub fn new(stream: &mut Stream) -> Option<ObjectStream> {
		stream.decompress();

		if stream.content.is_empty() {
			return Some(ObjectStream { objects: BTreeMap::new() });
		}

		let first_offset = stream.dict.get(b"First").and_then(Object::as_i64)? as usize;
		let _count = stream.dict.get(b"N").and_then(Object::as_i64)? as usize;

		let index_block = stream.content.get(..first_offset)?;

		let numbers_str = std::str::from_utf8(index_block).ok()?;
		let numbers: Vec<_> = numbers_str.split_whitespace().map(|number| u32::from_str(number).ok()).collect();
		let len = numbers.len() / 2 * 2; // Ensure only pairs.

		let objects = numbers[..len].par_chunks(2).filter_map(|chunk| {
			let id = chunk[0]?;
			let offset = first_offset + chunk[1]? as usize;

			let object = parser::direct_object(&stream.content[offset..])?;

			Some(((id, 0), object))
		}).collect();

		Some(ObjectStream{ objects })
	}
}
