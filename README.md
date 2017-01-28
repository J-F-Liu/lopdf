# lopdf

[![Crates.io](https://img.shields.io/crates/v/lopdf.svg)](https://crates.io/crates/lopdf)
[![Build Status](https://travis-ci.org/J-F-Liu/lopdf.png)](https://travis-ci.org/J-F-Liu/lopdf)

A Rust library for PDF document manipulation.

## Example Code

- Create new PDF document

```rust
extern crate lopdf;
use lopdf::{Document, Object, Dictionary, Stream, StringFormat};
use Object::{Null, Integer, Name, String, Reference};

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
doc.save("test.pdf").unwrap();
```

- Read PDF document

```rust
let mut doc = Document::load("test.pdf").unwrap();
```

## FAQ

- Why keeping everything in memory as high-level objects until finallay serializing the entire document?

	Normally a PDF document won't be very large, ranging form tens of KB to hundreds of MB. Memory size is not a bottle neck for today's computer.
	By keep the whole document in memory, stream length can be pre-calculated, no need to use a reference object for the Length entry,
	the resulting PDF file is smaller for distribution and faster for PDF consumers to process.

	Producing is a one-time effort, while consuming is many more.
