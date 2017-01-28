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
	use Object::{String, Reference};
	use super::{Dictionary, Stream, StringFormat};
	use super::content::*;
	use std::iter::FromIterator;

	let mut doc = Document::new();
	doc.version = "1.5".to_string();
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
		Operation{operator: "BT".into(), operands: vec![]},
		Operation{operator: "Tf".into(), operands: vec!["F1".into(), 48.into()]},
		Operation{operator: "Td".into(), operands: vec![100.into(), 600.into()]},
		Operation{operator: "Tj".into(), operands: vec![String("Hello World!".as_bytes().to_vec(), StringFormat::Literal)]},
		Operation{operator: "ET".into(), operands: vec![]},
	]};
	let content_id = doc.add_object(Stream::new(Dictionary::new(), content.encode().unwrap()));
	let page_id = doc.add_object(
		Dictionary::from_iter(vec![
			("Type", "Page".into()),
			("Parent", Reference((5,0))),
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
	let pages_id = doc.add_object(pages);
	doc.trailer.set("Root", Dictionary::from_iter(vec![
		("Type", "Catalog".into()),
		("Pages", Reference(pages_id)),
	]));
	doc.compress();
	doc.save("test_1_create.pdf").unwrap();
}
