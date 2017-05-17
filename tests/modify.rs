extern crate lopdf;

use lopdf::{Document, Object, StringFormat};
use std::io::Result;

fn modify_text() -> Result<Document> {
	let mut doc = Document::load("assets/example.pdf")?;
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

	doc.save("test_3_modify.pdf")?;
	Ok(doc)
}


#[test]
fn test_modify() {
	assert_eq!(modify_text().is_ok(), true);
}

#[test]
fn test_get_object() {
	use lopdf::Dictionary as LoDictionary;
	use lopdf::Stream as LoStream;
	use self::Object;

	let mut doc = Document::new();
	let id = doc.add_object(Object::String("test".as_bytes().to_vec(), StringFormat::Literal));
	let id2 = doc.add_object(Object::Stream(LoStream::new(LoDictionary::new(), "stream".as_bytes().to_vec())));

	println!("{:?}", id);
	println!("{:?}", id2);
	assert!(doc.get_object(id).is_some());
	assert!(doc.get_object(id2).is_some());
}