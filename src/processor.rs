use std::collections::BTreeMap;
use super::{Document, Object, ObjectId, StringFormat};

impl Document {
	/// Change producer of document information dictionary.
	pub fn change_producer(&mut self, producer: &str) {
		if let Some(info) = self.trailer.get_mut("Info") {
			if let Some(dict) = match *info {
				Object::Dictionary(ref mut dict) => Some(dict),
				Object::Reference(ref id) => self.objects.get_mut(id).and_then(|obj|obj.as_dict_mut()),
				_ => None,
			} {
				dict.set("Producer", Object::String(producer.as_bytes().to_vec(), StringFormat::Literal));
			}
		}
	}

	/// Compress PDF stream objects.
	pub fn compress(&mut self) {
		for (_, object) in self.objects.iter_mut() {
			match *object {
				Object::Stream(ref mut stream) => {
                    if stream.allows_compression {
                        stream.compress()
                    }
                },
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

	/// Delete pages.
	pub fn delete_pages(&mut self, page_numbers: &[u32]) {
		let pages = self.get_pages();
		for page_number in page_numbers {
			if let Some(page) = pages.get(&page_number).and_then(|page_id|self.delete_object(page_id)) {
				let mut page_tree_ref = page.as_dict().and_then(|dict|dict.get("Parent")).and_then(|obj|obj.as_reference());
				while let Some(page_tree_id) = page_tree_ref {
					if let Some(page_tree) = self.objects.get_mut(&page_tree_id).and_then(|obj|obj.as_dict_mut()) {
						page_tree.get("Count").and_then(|obj|obj.as_i64()).map(|count|{
							page_tree.set("Count", count - 1);
						});
						page_tree_ref = page_tree.get("Parent").and_then(|obj|obj.as_reference());
					} else {
						break;
					}
				}
			}
		}
	}

	/// Prune all unused objects.
	pub fn prune_objects(&mut self) -> Vec<ObjectId> {
		let mut ids = vec![];
		let refs = self.traverse_objects(|_|{});
		for id in self.objects.keys().cloned().collect::<Vec<ObjectId>>() {
			if !refs.contains(&id) {
				self.objects.remove(&id);
				ids.push(id);
			}
		}
		ids
	}

	/// Delete object by object ID.
	pub fn delete_object(&mut self, id: &ObjectId) -> Option<Object> {
		let action = |object: &mut Object| {
			match *object {
				Object::Array(ref mut array) => {
					if let Some(index) = array.iter().position(|item: &Object| {
						match *item {
							Object::Reference(ref_id) => ref_id == *id,
							_ => false
						}
					}) {
						array.remove(index);
					}
				},
				Object::Dictionary(ref mut dict) => {
					let keys: Vec<String> = dict.iter().filter(|&(_, item): &(&String, &Object)| {
						match *item {
							Object::Reference(ref_id) => ref_id == *id,
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
		self.objects.remove(id)
	}

	/// Delete zero length stream objects.
	pub fn delete_zero_length_streams(&mut self) -> Vec<ObjectId> {
		let mut ids = vec![];
		for id in self.objects.keys().cloned().collect::<Vec<ObjectId>>() {
			if self.objects.get(&id).and_then(|obj|obj.as_stream()).map(|stream|stream.content.len()==0) == Some(true) {
				self.delete_object(&id);
				ids.push(id);
			}
		}
		ids
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
