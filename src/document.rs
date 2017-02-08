use std::collections::BTreeMap;
use xref::Xref;
use super::{Object, ObjectId, Dictionary};

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

	/// Current maximum object id within the document.
	pub max_id: u32,
}

impl Document {
	/// Create new PDF document.
	pub fn new() -> Document {
		Document {
			version: "1.4".to_string(),
			trailer: Dictionary::new(),
			reference_table: Xref::new(),
			objects: BTreeMap::new(),
			max_id: 0,
		}
	}

	/// Get object by object id, will recursively dereference a referenced object.
	pub fn get_object(&self, id: ObjectId) -> Option<&Object> {
		if let Some(object) = self.objects.get(&id) {
			if let Some(id) = object.as_reference() {
				self.get_object(id)
			} else {
				Some(object)
			}
		} else {
			None
		}
	}
}
