use pom::{Input, DataInput};
use std::io::Read;
use std::str::FromStr;
use super::{Object, ObjectId, Stream};
use super::parser;

#[derive(Debug)]
pub struct ObjectStream {
	pub objects: Vec<(ObjectId, Object)>,
}

impl ObjectStream {
	pub fn new(stream: &mut Stream) -> ObjectStream {
		stream.decompress();
		let first_offset = stream.dict.get("First").and_then(|obj|obj.as_i64()).unwrap() as usize;
		let count = stream.dict.get("N").and_then(|obj|obj.as_i64()).unwrap() as usize;

		let mut index_block = vec![0_u8; first_offset];
		stream.content.as_slice().read_exact(index_block.as_mut_slice()).unwrap();

		let numbers = String::from_utf8(index_block).unwrap();
		let numbers = numbers.split_whitespace().map(|number|u32::from_str(number).unwrap());
		let mut numbers = numbers.into_iter();

		let mut objects = Vec::with_capacity(count);
		while let Some(id) = numbers.next() {
			let offset = first_offset + numbers.next().unwrap() as usize;
			let mut data = DataInput::new(stream.content.as_slice());
			data.jump_to(offset);
			if let Ok(object) = parser::direct_object().parse(&mut data) {
				objects.push(((id, 0), object));
			}
		}
		ObjectStream { objects }
	}

	pub fn get_object(&self, index: usize) -> Option<&(ObjectId, Object)> {
		if index < self.objects.len() {
			Some(&self.objects[index])
		} else {
			None
		}
	}
}
