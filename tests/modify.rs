extern crate lopdf;

use lopdf::{Document, Object, StringFormat};
use std::io::Result;
use std::fs::File;

fn modify_text() -> Result<Document> {
	let file = File::open("assets/example.pdf").unwrap();
	let mut doc = Document::load(file)?;
	doc.version = "1.4".to_string();
	if let Some(content_stream) = doc.objects.get_mut(&(4, 0)) {
		match *content_stream {
			Object::Stream(ref mut stream) => {
				let mut content = stream.decode_content().unwrap();
				content.operations[3].operands[0] = Object::String(
					b"Modified text!".to_vec(),
					StringFormat::Literal);
				stream.set_content(content.encode().unwrap());
			},
			_ => ()
		}
	}

	let mut file = File::create("test_3_modify.pdf")?;
	doc.save(&mut file)?;
	Ok(doc)
}


#[test]
fn test_modify() {
	assert_eq!(modify_text().is_ok(), true);
}
