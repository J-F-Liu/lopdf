//! `Writer::write_object` is public so that callers writing a document
//! incrementally can serialize single objects with lopdf's own formatting.

use lopdf::{Dictionary, Object, Stream, StringFormat, Writer};

fn write(object: &Object) -> Vec<u8> {
    let mut out = Vec::new();
    Writer::write_object(&mut out, object).unwrap();
    out
}

#[test]
fn writes_scalars() {
    assert_eq!(write(&Object::Null), b"null");
    assert_eq!(write(&Object::Boolean(true)), b"true");
    assert_eq!(write(&Object::Boolean(false)), b"false");
    assert_eq!(write(&Object::Integer(-42)), b"-42");
    assert_eq!(write(&Object::Real(1.5)), b"1.5");
}

#[test]
fn writes_names_and_strings() {
    assert_eq!(write(&Object::Name(b"Type".to_vec())), b"/Type");
    // A space is escaped as #20.
    assert_eq!(write(&Object::Name(b"Two Words".to_vec())), b"/Two#20Words");
    assert_eq!(
        write(&Object::String(b"hi".to_vec(), StringFormat::Literal)),
        b"(hi)"
    );
    assert_eq!(
        write(&Object::String(b"hi".to_vec(), StringFormat::Hexadecimal)),
        b"<6869>"
    );
}

#[test]
fn writes_dictionaries_and_arrays() {
    let mut dict = Dictionary::new();
    dict.set("Type", "Catalog");
    dict.set("Count", 3);
    assert_eq!(write(&Object::Dictionary(dict)), b"<</Type/Catalog/Count 3>>");

    assert_eq!(
        write(&Object::Array(vec![
            Object::Integer(1),
            Object::Integer(2)
        ])),
        b"[1 2]"
    );
}

#[test]
fn writes_streams_with_length() {
    let stream = Stream::new(Dictionary::new(), b"abc".to_vec());
    assert_eq!(
        write(&Object::Stream(stream)),
        b"<</Length 3>>stream\nabc\nendstream"
    );
}

#[test]
fn matches_document_save_bytes() {
    // The point of exposing the writer: hand-assembled output is byte-identical
    // to what `Document::save` produces for the same object.
    let mut dict = Dictionary::new();
    dict.set("Type", "Catalog");
    dict.set("Pages", Object::Reference((2, 0)));
    let object = Object::Dictionary(dict);

    let mut doc = lopdf::Document::with_version("1.5");
    let id = doc.add_object(object.clone());
    let mut saved = Vec::new();
    doc.save_to(&mut saved).unwrap();

    let needle = format!("{} {} obj\n", id.0, id.1);
    let start = saved
        .windows(needle.len())
        .position(|w| w == needle.as_bytes())
        .expect("object header in saved document")
        + needle.len();
    let end = saved[start..]
        .windows(b"\nendobj".len())
        .position(|w| w == b"\nendobj")
        .expect("endobj in saved document")
        + start;

    assert_eq!(&saved[start..end], write(&object).as_slice());
}
