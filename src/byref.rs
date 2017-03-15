use super::{Document, Object, Dictionary};

pub trait ByRef<'a> {
	fn get_dict_by_ref(&self, doc: &'a Document) -> Option<&'a Dictionary>;
}

impl<'a> ByRef<'a> for Option<&'a Object> {
	fn get_dict_by_ref(&self, doc: &'a Document) -> Option<&'a Dictionary> {
		self.and_then(|obj|obj.as_reference())
			.and_then(|id|doc.get_object(id))
			.and_then(|obj|obj.as_dict())
	}
}
