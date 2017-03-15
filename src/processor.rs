use super::{Document, Object, ObjectId};
use std::collections::BTreeMap;

impl Document {
	/// Compress PDF stream objects.
	pub fn compress(&mut self) {
		for (_, object) in self.objects.iter_mut() {
			match *object {
				Object::Stream(ref mut stream) => stream.compress(),
				_ => ()
			}
		}
	}

	/// Decompress PDF stream objects.
	pub fn decompress(&mut self) {
		for (_, object) in self.objects.iter_mut() {
			match *object {
				Object::Stream(ref mut stream) => stream.decompress(),
				_ => ()
			}
		}
	}

	/// Delete unused objects.
	pub fn delete_unused_objects(&mut self) {
		let refs = self.traverse_objects(|_|{});
		for id in self.objects.keys().cloned().collect::<Vec<ObjectId>>() {
			if !refs.contains(&id) {
				self.objects.remove(&id);
			}
		}
	}

	/// Delete object by object ID.
	pub fn delete_object(&mut self, id: ObjectId) -> Option<Object> {
		let action = |object: &mut Object| {
			match *object {
				Object::Array(ref mut array) => {
					if let Some(index) = array.iter().position(|item: &Object| {
						match *item {
							Object::Reference(ref_id) => ref_id == id,
							_ => false
						}
					}) {
						array.remove(index);
					}
				},
				Object::Dictionary(ref mut dict) => {
					let keys: Vec<String> = dict.iter().filter(|&(_, item): &(&String, &Object)| {
						match *item {
							Object::Reference(ref_id) => ref_id == id,
							_ => false
						}
					}).map(|(k, _)| k.clone()).collect();
					for key in keys {
						dict.remove(key.as_str());
					}
				},
				_ => {}
			}
		};
		self.traverse_objects(action);
		self.objects.remove(&id)
	}

	/// Renumber objects, normally called after delete_unused_objects.
	pub fn renumber_objects(&mut self) {
		let mut replace = BTreeMap::new();
		let mut new_id = 1;
		let mut ids = self.objects.keys().cloned().collect::<Vec<ObjectId>>();
		ids.sort();

		for id in ids {
			if id.0 != new_id {
				replace.insert(id, (new_id, id.1));
			}
			new_id += 1;
		}

		// replace order is from small to big
		for (old, new) in replace.iter() {
			if let Some(object) = self.objects.remove(old) {
				self.objects.insert(new.clone(), object);
			}
		}

		let action = |object: &mut Object| {
			match *object {
				Object::Reference(ref mut id) => {
					if replace.contains_key(&id) {
						*id = replace.get(id).unwrap().clone();
					}
				},
				_ => {}
			}
		};
		
		self.traverse_objects(action);
		self.max_id = new_id - 1;
	}
}
