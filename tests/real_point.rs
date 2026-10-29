//! A real is written so that it reads back as a real: a whole-number real keeps its point. Rust's
//! `Display` prints `0.0` as `0` and `5.0` as `5`, so a real written that way reloaded as an
//! `Object::Integer` and changed type on every load and save.

use lopdf::content::{Content, Operation};
use lopdf::{Document, IncrementalDocument, Object, dictionary};

/// A one-page document whose page dictionary holds `value` under `/Probe`, alone and in an array.
fn document_with(value: Object) -> Document {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Probe" => value.clone(),
        "ProbeArray" => vec![1.into(), value, 2.5.into(), 3.into()],
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
    doc
}

/// The `/Probe` value and the `/ProbeArray` element it was put in, as `bytes` loads back.
fn probes(bytes: &[u8]) -> (Object, Object) {
    let doc = Document::load_mem(bytes).unwrap();
    let page = doc.page_iter().next().unwrap();
    let dict = doc.get_dictionary(page).unwrap();
    let probe = dict.get(b"Probe").unwrap().clone();
    let array = dict.get(b"ProbeArray").and_then(Object::as_array).unwrap();
    (probe, array[1].clone())
}

/// `reloaded` is the real `value`, its sign included.
fn assert_same_real(reloaded: &Object, value: Object) {
    match (reloaded, &value) {
        (Object::Real(got), Object::Real(want)) => {
            assert_eq!(got, want, "{reloaded:?} for {value:?}");
            assert_eq!(
                got.is_sign_negative(),
                want.is_sign_negative(),
                "{reloaded:?} for {value:?}"
            );
        }
        _ => panic!("{value:?} reloaded as {reloaded:?}, not a real"),
    }
}

fn saved(mut doc: Document) -> Vec<u8> {
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

fn saved_modern(mut doc: Document) -> Vec<u8> {
    let mut bytes = Vec::new();
    doc.save_modern(&mut bytes).unwrap();
    bytes
}

/// `object` as the writer spells it. `Writer` is private; a content stream's operands go through
/// the same `Writer::write_object` as every saved object.
fn written(object: &Object) -> String {
    let content = Content {
        operations: vec![Operation::new("", vec![object.clone()])],
    };
    let text = String::from_utf8(content.encode().unwrap()).unwrap();
    text.trim_end().to_string()
}

#[test]
fn a_real_is_written_with_its_point() {
    assert_eq!(written(&Object::Real(0.0)), "0.0");
    assert_eq!(written(&Object::Real(-0.0)), "-0.0");
    assert_eq!(written(&Object::Real(5.0)), "5.0");
    assert_eq!(written(&Object::Real(-5.0)), "-5.0");
    // Full digits, never an exponent, which PDF syntax does not have.
    assert_eq!(written(&Object::Real(1e20)), "100000000000000000000.0");
    assert_eq!(written(&Object::Real(0.1)), "0.1");
    assert_eq!(written(&Object::Real(2.5)), "2.5");
}

#[test]
fn a_real_reloads_as_the_same_real() {
    for value in [0.0, -0.0, 5.0, 1e20, 0.1] {
        let value = Object::Real(value);
        for bytes in [
            saved(document_with(value.clone())),
            saved_modern(document_with(value.clone())),
        ] {
            let (probe, in_array) = probes(&bytes);
            assert_same_real(&probe, value.clone());
            assert_same_real(&in_array, value.clone());
        }
    }
}

#[test]
fn an_integer_is_still_written_as_an_integer() {
    assert_eq!(written(&Object::Integer(5)), "5");
    assert_eq!(written(&Object::Integer(0)), "0");
    let bytes = saved(document_with(Object::Integer(5)));
    let (probe, in_array) = probes(&bytes);
    assert_eq!(probe, Object::Integer(5));
    assert_eq!(in_array, Object::Integer(5));
}

#[test]
fn an_incremental_update_writes_a_real_with_its_point() {
    let prev = saved(document_with(Object::Integer(1)));
    let loaded = Document::load_mem(&prev).unwrap();
    let page = loaded.page_iter().next().unwrap();
    let mut incremental = IncrementalDocument::create_from(prev.clone(), loaded);
    incremental.opt_clone_object_to_new_document(page).unwrap();
    incremental
        .new_document
        .get_dictionary_mut(page)
        .unwrap()
        .set("Probe", Object::Real(5.0));
    let mut out = Vec::new();
    incremental.save_to(&mut out).unwrap();

    let (probe, _) = probes(&out);
    assert_same_real(&probe, Object::Real(5.0));
}

#[test]
fn a_content_stream_operand_keeps_its_point() {
    let content = Content {
        operations: vec![Operation::new(
            "cm",
            vec![
                Object::Real(1.0),
                Object::Integer(0),
                Object::Real(0.0),
                1.into(),
                Object::Real(2.5),
                Object::Real(-3.0),
            ],
        )],
    };
    let encoded = String::from_utf8(content.encode().unwrap()).unwrap();
    assert_eq!(encoded.trim_end(), "1.0 0 0.0 1 2.5 -3.0 cm");

    let decoded = Content::decode(encoded.as_bytes()).unwrap();
    assert_eq!(decoded.operations[0].operands, content.operations[0].operands);
    assert!(matches!(decoded.operations[0].operands[0], Object::Real(_)));
    assert!(matches!(decoded.operations[0].operands[2], Object::Real(_)));
}

/// NaN and the infinities have no PDF spelling. They are written as `Display` prints them, as
/// before: the point is added only to a finite value.
#[test]
fn a_non_finite_real_is_written_as_before() {
    assert_eq!(written(&Object::from(f32::NAN)), "NaN");
    assert_eq!(written(&Object::from(f32::INFINITY)), "inf");
    assert_eq!(written(&Object::from(f32::NEG_INFINITY)), "-inf");
}
