# lopdf

[![Crates.io](https://img.shields.io/crates/v/lopdf.svg)](https://crates.io/crates/lopdf)

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
