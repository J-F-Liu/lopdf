use super::{Document, Object, ObjectId};

impl Document {
	/// Create an object ID.
	pub fn new_object_id(&mut self) -> ObjectId {
		self.max_id += 1;
		(self.max_id, 0)
	}

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
	use super::{Dictionary, Stream, StringFormat};
	use super::content::*;
	use Object::Reference;
	use std::iter::FromIterator;
	use chrono::prelude::Local;

	let mut doc = Document::new();
	doc.version = "1.5".to_string();
	let info_id = doc.add_object(
		Dictionary::from_iter(vec![
			("Title", Object::String(b"Create PDF document example".to_vec(), StringFormat::Literal)),
			("Creator", Object::String(b"https://crates.io/crates/lopdf".to_vec(), StringFormat::Literal)),
			("CreationDate", Local::now().into()),
		])
	);
	let pages_id = doc.new_object_id();
	let font_id = doc.add_object(
		Dictionary::from_iter(vec![
			("Type", "Font".into()),
			("Subtype", "Type1".into()),
			("BaseFont", "Courier".into()),
		])
	);
	let resources_id = doc.add_object(
		Dictionary::from_iter(vec![
			("Font", Dictionary::from_iter(vec![
				("F1", Reference(font_id)),
			]).into()),
		])
	);
	let content = Content{operations: vec![
		Operation::new("BT", vec![]),
		Operation::new("Tf", vec!["F1".into(), 48.into()]),
		Operation::new("Td", vec![100.into(), 600.into()]),
		Operation::new("Tj", vec![Object::String(b"Hello World!".to_vec(), StringFormat::Literal)]),
		Operation::new("ET", vec![]),
	]};
	let content_id = doc.add_object(Stream::new(Dictionary::new(), content.encode().unwrap()));
	let page_id = doc.add_object(
		Dictionary::from_iter(vec![
			("Type", "Page".into()),
			("Parent", Reference(pages_id)),
			("Contents", vec![Reference(content_id)].into()),
		])
	);
	let pages = Dictionary::from_iter(vec![
		("Type", "Pages".into()),
		("Kids", vec![Reference(page_id)].into()),
		("Count", 1.into()),
		("Resources", Reference(resources_id)),
		("MediaBox", vec![0.into(), 0.into(), 595.into(), 842.into()].into()),
	]);
	doc.objects.insert(pages_id, Object::Dictionary(pages));
	let catalog_id = doc.add_object(
		Dictionary::from_iter(vec![
			("Type", "Catalog".into()),
			("Pages", Reference(pages_id)),
		])
	);
	doc.trailer.set("Root", Reference(catalog_id));
	doc.trailer.set("Info", Reference(info_id));
	doc.compress();

	let mut file = ::std::fs::File::create("test_1_create.pdf").unwrap();
	doc.save(&mut file).unwrap();
}
