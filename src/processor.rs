use super::{Document, Object, ObjectId};

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
}
