use std::collections::BTreeMap;
use xref::{Xref, XrefEntry};
use super::{Object, ObjectId, Dictionary};
use object_stream::ObjectStream;
use byref::ByRef;

/// PDF document.
#[derive(Debug)]
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

	/// Create new PDF document.
	pub fn with_version<S: Into<String>>(version: S) -> Document {
		let mut document = Self::new();
		document.version = version.into();
		document
	}

	/// Get object by object id, will recursively dereference a referenced object.
	pub fn get_object(&self, id: ObjectId) -> Option<&Object> {
		if let Some(object) = self.objects.get(&id) {
			return Some(object);
		}
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

	/// Get catalog dictionary.
	pub fn catalog(&self) -> Option<&Dictionary> {
		self.trailer.get("Root").get_dict_by_ref(self)
	}

	/// Get page numbers and corresponding object ids.
	pub fn get_pages(&self) -> BTreeMap<u32, ObjectId> {
		fn collect_pages(doc: &Document, page_tree_id: ObjectId, page_number: &mut u32, pages: &mut BTreeMap<u32, ObjectId>) {
			if let Some(kids) = doc.get_object(page_tree_id).and_then(|obj|obj.as_dict()).and_then(|page_tree|page_tree.get("Kids")).and_then(|obj|obj.as_array()) {
				for kid in kids {
					if let Some(kid_id) = kid.as_reference() {
						if let Some(type_name) = doc.get_object(kid_id).and_then(|obj|obj.as_dict()).and_then(|dict|dict.type_name()) {
							match type_name {
								"Page" => {
									pages.insert(*page_number, kid_id);
									*page_number += 1;
								}
								"Pages" => {
									collect_pages(doc, kid_id, page_number, pages);
								}
								_ => {}
							}
						}
					}
				}
			}
		}

		let mut pages = BTreeMap::new();
		let mut page_number = 1;
		if let Some(page_tree_id) = self.catalog().and_then(|cat|cat.get("Pages")).and_then(|pages|pages.as_reference()) {
			collect_pages(self, page_tree_id, &mut page_number, &mut pages);
		}
		pages
	}
}
