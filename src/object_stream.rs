use super::parser;
use super::{Object, ObjectId, Stream};
use pom::{DataInput, Input};
use std::collections::BTreeMap;
use std::io::Read;
use std::str::FromStr;

#[derive(Debug)]
pub struct ObjectStream {
	pub objects: BTreeMap<ObjectId, Object>,
}

impl ObjectStream {
	pub fn new(stream: &mut Stream) -> ObjectStream {
		let mut objects = BTreeMap::new();
		stream.decompress();
		if stream.content.len() > 0 {
			let first_offset = stream.dict.get(b"First").and_then(|obj| obj.as_i64()).unwrap() as usize;
			let _count = stream.dict.get(b"N").and_then(|obj| obj.as_i64()).unwrap() as usize;

			let mut index_block = vec![0_u8; first_offset];
			stream.content.as_slice().read_exact(index_block.as_mut_slice()).unwrap();

			let numbers = String::from_utf8(index_block).unwrap();
			let numbers = numbers.split_whitespace().map(|number| u32::from_str(number).unwrap());
			let mut numbers = numbers.into_iter();

			while let Some(id) = numbers.next() {
				let offset = first_offset + numbers.next().unwrap() as usize;
				let mut data = DataInput::new(stream.content.as_slice());
				data.jump_to(offset);
				if let Ok(object) = parser::direct_object().parse(&mut data) {
					objects.insert((id, 0), object);
				}
			}
		}
		ObjectStream { objects }
	}
}
