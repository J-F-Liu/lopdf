use std::collections::BTreeMap;
use xref::{Xref, XrefEntry};
use super::{Object, ObjectId, Dictionary};
use object_stream::ObjectStream;

/// PDF document.
pub struct Document {
	/// The version of the PDF specification to which the file conforms.
	pub version: String,

	/// The trailer gives the location of the cross-reference table and of certain special objects.
	pub trailer: Dictionary,

	/// The cross-reference table contains locations of the indirect objects.
	pub reference_table: Xref,

	/// The objects that make up the document contained in the file.
	pub objects: BTreeMap<ObjectId, Object>,

	/// The object streams which contains compressed objects.
	pub streams: BTreeMap<u32, ObjectStream>,

	/// Current maximum object id within the document.
	pub max_id: u32,
}

impl Document {
	/// Create new PDF document.
	pub fn new() -> Document {
		Document {
			version: "1.4".to_string(),
			trailer: Dictionary::new(),
			reference_table: Xref::new(0),
			objects: BTreeMap::new(),
			streams: BTreeMap::new(),
			max_id: 0,
		}
	}

	/// Get object by object id, will recursively dereference a referenced object.
	pub fn get_object(&self, id: ObjectId) -> Option<&Object> {
		if let Some(entry) = self.reference_table.get(id.0) {
			match *entry {
				XrefEntry::Normal { .. } => {
					if let Some(object) = self.objects.get(&id) {
						if let Some(id) = object.as_reference() {
							return self.get_object(id);
						} else {
							return Some(object);
						}
					}
				}
				XrefEntry::Compressed { container, index } => {
					if let Some(stream) = self.streams.get(&container) {
						if let Some(&(_id, ref object)) = stream.get_object(index as usize) {
							return Some(object);
						}
					}
				}
				_ => {},
			}
		}
		return None;
	}

	/// Traverse objects from trailer recursively, return all referenced object IDs.
	pub fn traverse_objects<A: Fn(&mut Object) -> ()>(&mut self, action: A) -> Vec<ObjectId> {
		fn traverse_array<A: Fn(&mut Object) -> ()>(array: &mut Vec<Object>, action: &A, refs: &mut Vec<ObjectId>) {
			for item in array.iter_mut() {
				traverse_object(item, action, refs);
			}
		}
		fn traverse_dictionary<A: Fn(&mut Object) -> ()>(dict: &mut Dictionary, action: &A, refs: &mut Vec<ObjectId>) {
			for (_, v) in dict.iter_mut() {
				traverse_object(v, action, refs);
			}
		}
		fn traverse_object<A: Fn(&mut Object) -> ()>(object: &mut Object, action: &A, refs: &mut Vec<ObjectId>) {
			action(object);
			match *object {
				Object::Array(ref mut array) => traverse_array(array, action, refs),
				Object::Dictionary(ref mut dict) => traverse_dictionary(dict, action, refs),
				Object::Stream(ref mut stream) => traverse_dictionary(&mut stream.dict, action, refs),
				Object::Reference(id) => {
					if !refs.contains(&id) {
						refs.push(id);
					}
				},
				_ => {}
			}
		}
		let mut refs = vec![];
		traverse_dictionary(&mut self.trailer, &action, &mut refs);
		let mut index = 0;
		while index < refs.len() {
			if let Some(object) = self.objects.get_mut(&refs[index]) {
				traverse_object(object, &action, &mut refs);
			}
			index += 1;
		}
		refs
	}
}
