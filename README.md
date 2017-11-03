# lopdf

[![Crates.io](https://img.shields.io/crates/v/lopdf.svg)](https://crates.io/crates/lopdf)
[![Build Status](https://travis-ci.org/J-F-Liu/lopdf.png)](https://travis-ci.org/J-F-Liu/lopdf)

A Rust library for PDF document manipulation.

## Example Code

- Create PDF document

```rust
extern crate lopdf;
use lopdf::{Document, Object, Stream};
use lopdf::content::{Content, Operation};

let mut doc = Document::with_version("1.5");
let pages_id = doc.new_object_id();
let font_id = doc.add_object(dictionary! {
	"Type" => "Font",
	"Subtype" => "Type1",
	"BaseFont" => "Courier",,
});
let resources_id = doc.add_object(dictionary! {
	"Font" => dictionary! {
		"F1" => font_id,
	},
});
let content = Content{operations: vec![
	Operation::new("BT", vec![]),
	Operation::new("Tf", vec!["F1".into(), 48.into()]),
	Operation::new("Td", vec![100.into(), 600.into()]),
	Operation::new("Tj", vec![Object::string_literal("Hello World!")]),
	Operation::new("ET", vec![]),
]};
let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
let page_id = doc.add_object(dictionary! {
	"Type" => "Page",
	"Parent" => pages_id,
	"Contents" => vec![content_id.into()],
});
let pages = dictionary! {
	"Type" => "Pages",
	"Kids" => vec![page_id.into()],
	"Count" => 1,
	"Resources" => resources_id,
	"MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
};
doc.objects.insert(pages_id, Object::Dictionary(pages));
let catalog_id = doc.add_object(dictionary! {
	"Type" => "Catalog",
	"Pages" => pages_id,
});
doc.trailer.set("Root", catalog_id);
doc.compress();
doc.save("example.pdf").unwrap();
```

- Modify PDF document

```rust
let mut doc = Document::load("example.pdf")?;
doc.version = "1.4".to_string();
if let Some(content_stream) = doc.objects.get_mut(&(3, 0)) {
	match *content_stream {
		Object::Stream(ref mut stream) => {
			let mut content = stream.decode_content().unwrap();
			content.operations[3].operands[0] = Object::string_literal("Modified text!");
			stream.set_content(content.encode().unwrap());
		},
		_ => ()
	}
}
doc.save("modified.pdf")?;
```

## FAQ

- Why keeping everything in memory as high-level objects until finallay serializing the entire document?

	Normally a PDF document won't be very large, ranging form tens of KB to hundreds of MB. Memory size is not a bottle neck for today's computer.
	By keep the whole document in memory, stream length can be pre-calculated, no need to use a reference object for the Length entry,
	the resulting PDF file is smaller for distribution and faster for PDF consumers to process.

	Producing is a one-time effort, while consuming is many more.
