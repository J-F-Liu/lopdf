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
doc.add_object(Null);
doc.add_object(true);
doc.add_object(3);
doc.add_object(0.5);
doc.add_object(String("text".as_bytes().to_vec(), StringFormat::Literal));
doc.add_object(Name("name".to_string()));
doc.add_object(Reference((1,0)));
doc.add_object(vec![Integer(1), Integer(2), Integer(3)]);
doc.add_object(Stream::new(Dictionary::new(), vec![0x41; 100]));
let mut dict = Dictionary::new();
dict.set("A", Null);
dict.set("B", false);
dict.set("C", Name("name".to_string()));
doc.add_object(dict);
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
