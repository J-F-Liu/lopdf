//! A real keeps every digit it was written with. `Object::Real` held an `f32`, about seven
//! significant digits, so a font width such as `666.99219` loaded as `666.9922` and was written
//! back that way, and a program that measured or re-serialised the file saw a different number.

use lopdf::content::Content;
use lopdf::{Document, Object, Writer, dictionary};

/// Values an `f32` cannot hold: eight and nine significant digits, as LibreOffice and other
/// producers write font widths and matrix entries.
const LONG_REALS: [(&str, f64); 4] = [
    ("666.99219", 666.99219),
    ("12.3456789", 12.3456789),
    ("-0.004142502", -0.004142502),
    ("1234567.89", 1234567.89),
];

fn written(object: &Object) -> String {
    let mut out = Vec::new();
    Writer::write_object(&mut out, object).unwrap();
    String::from_utf8(out).unwrap()
}

/// A one-page document whose page dictionary holds `value` under `/Probe`.
fn saved_with(value: Object) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Probe" => value,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn a_long_real_in_a_content_stream_keeps_every_digit() {
    for (text, value) in LONG_REALS {
        let content = Content::decode(format!("{text} 0 Td").as_bytes()).unwrap();
        assert_eq!(content.operations[0].operands[0], Object::Real(value), "{text}");
    }
}

#[test]
fn a_long_real_in_a_document_loads_and_saves_unchanged() {
    for (text, value) in LONG_REALS {
        let bytes = saved_with(Object::Real(value));
        let doc = Document::load_mem(&bytes).unwrap();
        let page = doc.page_iter().next().unwrap();
        let probe = doc.get_dictionary(page).unwrap().get(b"Probe").unwrap();
        assert_eq!(probe, &Object::Real(value), "{text}");
        assert_eq!(written(probe), text);
    }
}

#[test]
fn an_f32_widens_to_the_decimal_it_prints_as() {
    assert_eq!(Object::from(0.1f32), Object::Real(0.1));
    assert_eq!(written(&Object::from(0.1f32)), "0.1");
    assert_eq!(written(&Object::from(666.9922f32)), "666.9922");
    assert_eq!(written(&Object::from(-2.5f32)), "-2.5");
    assert_eq!(written(&Object::from(3.0f32)), "3.0");
    assert!(matches!(Object::from(f32::NAN), Object::Real(v) if v.is_nan()));
    assert_eq!(Object::from(f32::INFINITY), Object::Real(f64::INFINITY));
    assert_eq!(Object::from(f32::NEG_INFINITY), Object::Real(f64::NEG_INFINITY));
}

#[test]
fn an_f64_is_stored_as_given() {
    assert_eq!(Object::from(666.99219f64), Object::Real(666.99219));
    assert_eq!(Object::from(0.1f64), Object::Real(0.1));
}

#[test]
fn the_f32_accessors_round_and_the_f64_accessors_do_not() {
    let real = Object::Real(666.99219);
    assert_eq!(real.as_f64().unwrap(), 666.99219);
    assert_eq!(real.as_float_f64().unwrap(), 666.99219);
    assert_eq!(real.as_f32().unwrap(), 666.9922f32);
    assert_eq!(real.as_float().unwrap(), 666.9922f32);

    let integer = Object::Integer(7);
    assert_eq!(integer.as_float_f64().unwrap(), 7.0);
    assert_eq!(integer.as_float().unwrap(), 7.0);
    assert!(integer.as_f64().is_err());
    assert!(integer.as_f32().is_err());

    let name = Object::Name(b"Width".to_vec());
    assert!(name.as_f64().is_err());
    assert!(name.as_f32().is_err());
    assert!(name.as_float().is_err());
    assert!(name.as_float_f64().is_err());
}
