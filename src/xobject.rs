use super::content::*;
use super::Object::*;
use super::{Dictionary, Document, Object, Stream};

pub fn form(boundingbox: Vec<f64>, matrix: Vec<f64>, content: Vec<u8>) -> Stream {
	let mut dict = Dictionary::new();
	dict.set("Type", Name(b"XObject".to_vec()));
	dict.set("Subtype", Name(b"Form".to_vec()));
	dict.set("BBox", Array(boundingbox.into_iter().map(Real).collect()));
	dict.set("Matrix", Array(matrix.into_iter().map(Real).collect()));
	return Stream::new(dict, content);
}

impl Document {
	pub fn insert_form_object(&mut self, page_number: u32, form_obj: Stream) {
		let pages = self.get_pages();
		let page_id = *pages.get(&page_number).expect(&format!("Page {} not exist.", page_number));

		let form_id = self.add_object(form_obj);
		let form_name = format!("M{}", form_id.0);

		let mut content = self.get_and_decode_page_content(page_id);
		// content.operations.push(Operation::new("q", vec![]));
		content.operations.push(Operation::new("Do", vec![Name(form_name.as_bytes().to_vec())]));
		// content.operations.push(Operation::new("Q", vec![]));
		let modified_contnet = content.encode().unwrap();
		self.change_page_content(page_id, modified_contnet);
		self.add_xobject(page_id, form_name, form_id);
	}
}
