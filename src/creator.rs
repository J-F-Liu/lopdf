use super::{Document, Object, ObjectId};

impl Document {
	/// Add PDF object into document's object list.
	pub fn add_object<T: Into<Object>>(&mut self, object: T) -> ObjectId {
		self.max_id += 1;
		let id = (self.max_id, 0);
		self.objects.insert(id, object.into());
		id
	}
}

#[test]
fn create_document() {
	use std::path::Path;
	use Object::{Null, Integer, Name, String, Reference};
	use super::{Dictionary, Stream, StringFormat};

	let mut doc = Document::new();
	doc.version = "1.5".to_string();
	doc.add_object(Null);
	doc.add_object(true);
	doc.add_object(3);
	doc.add_object(0.5);
	doc.add_object(String("text((\r)".as_bytes().to_vec(), StringFormat::Literal));
	doc.add_object(String("text((\r)".as_bytes().to_vec(), StringFormat::Hexadecimal));
	doc.add_object(Name("name \t".to_string()));
	doc.add_object(Reference((1,0)));
	doc.add_object(vec![Integer(1), Integer(2), Integer(3)]);
	doc.add_object(Stream::new(Dictionary::new(), vec![0x41, 0x42, 0x43]));
	let mut dict = Dictionary::new();
	dict.set("A", Null);
	dict.set("B", false);
	dict.set("C", Name("name".to_string()));
	doc.add_object(dict);
	doc.save(Path::new("test_1_create.pdf")).unwrap();
}
