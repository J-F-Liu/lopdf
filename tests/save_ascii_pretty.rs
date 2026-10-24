//! `SaveOptions::ascii` writes only 7-bit ASCII and `SaveOptions::pretty` indents dictionaries
//! and arrays. Neither changes what a reader reads: every object loads back as it was, and a
//! stream that ASCII output encodes with `/ASCIIHexDecode` decodes to the same data.

#![cfg(not(feature = "async"))]

use lopdf::xref::XrefType;
use lopdf::{
    Dictionary, Document, EncryptionState, EncryptionVersion, Object, Permissions, SaveOptions, Stream, StringFormat,
    dictionary,
};

/// Every byte value, so that strings and streams hold binary data and every delimiter.
fn all_bytes() -> Vec<u8> {
    (0..=255).collect()
}

/// A document with an object of every kind: scalars, names and strings with bytes that need
/// escaping, nested arrays and dictionaries, and streams with and without filters.
fn document(cross_reference_type: XrefType) -> Document {
    let mut doc = Document::with_version("1.5");
    doc.reference_table.cross_reference_type = cross_reference_type;

    let compressed = Stream::new(dictionary! {}, all_bytes().repeat(4)).with_compression(true);
    let mut compressed = compressed;
    compressed.compress().unwrap();
    assert!(compressed.dict.has(b"Filter"));

    let streams = vec![
        // Text that ASCII output keeps as it is.
        Stream::new(dictionary! {}, b"BT /F1 12 Tf (Hello) Tj ET\r\n\t".to_vec()),
        // Binary data without a filter.
        Stream::new(dictionary! {}, all_bytes()),
        // A form feed is not one of the bytes ASCII output keeps.
        Stream::new(dictionary! {}, b"a\x0Cb".to_vec()),
        // A single filter given as a name, with parameters in a dictionary.
        Stream::new(
            dictionary! {
                "Filter" => "FlateDecode",
                "DecodeParms" => dictionary! { "Predictor" => 1 },
            },
            compressed.content.clone(),
        ),
        // Filters and parameters given as arrays.
        Stream::new(
            dictionary! {
                "Filter" => vec![Object::Name(b"FlateDecode".to_vec())],
                "DecodeParms" => vec![Object::Null],
            },
            compressed.content.clone(),
        ),
        // Filters as an array, parameters as a dictionary.
        Stream::new(
            dictionary! {
                "Filter" => vec![Object::Name(b"FlateDecode".to_vec())],
                "DecodeParms" => dictionary! { "Predictor" => 1 },
            },
            compressed.content.clone(),
        ),
        // A filter without parameters.
        compressed,
        // Data that is already ASCII hexadecimal.
        Stream::new(dictionary! { "Filter" => "ASCIIHexDecode" }, b"48656C6C6F>".to_vec()),
        // No data at all.
        Stream::new(dictionary! {}, Vec::new()),
    ];
    let stream_ids: Vec<Object> = streams
        .into_iter()
        .map(|stream| doc.add_object(stream).into())
        .collect();

    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => stream_ids[0].clone(),
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let mut nested = Dictionary::new();
    nested.set("Null", Object::Null);
    nested.set("True", true);
    nested.set("False", false);
    nested.set("Integer", -42);
    nested.set("Real", 0.5);
    nested.set("Name", Object::Name(b"A B#/()<>[]{}%\x00\x7F\x80\xFF".to_vec()));
    nested.set("Literal", Object::String(all_bytes(), StringFormat::Literal));
    nested.set(
        "Unbalanced",
        Object::String(b"a(b\\c)d)e(".to_vec(), StringFormat::Literal),
    );
    nested.set("Octal", Object::String(b"\x01234\x7F5".to_vec(), StringFormat::Literal));
    nested.set("Hexadecimal", Object::String(all_bytes(), StringFormat::Hexadecimal));
    nested.set("Empty", Object::String(Vec::new(), StringFormat::Literal));
    nested.set("EmptyArray", Object::Array(Vec::new()));
    nested.set("EmptyDictionary", Dictionary::new());
    nested.set(
        "Array",
        vec![
            Object::Integer(1),
            Object::Array(vec![Object::Array(vec![Object::Real(-3.25)]), Object::Null]),
            Object::Dictionary(dictionary! { "Key" => Object::Name(b"Value".to_vec()) }),
            Object::Reference(page_id),
        ],
    );
    nested.set("Streams", stream_ids);
    nested.set("Dictionary", dictionary! { "Inner" => dictionary! { "Deepest" => 1 } });
    let nested_id = doc.add_object(nested);
    doc.add_object(Object::String(all_bytes(), StringFormat::Literal));
    doc.add_object(Object::Name(b"\x80 \xFF".to_vec()));
    doc.add_object(Object::Real(1.1));
    doc.add_object(Object::Null);

    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::String(all_bytes()[..16].to_vec(), StringFormat::Literal),
            Object::String(all_bytes()[240..].to_vec(), StringFormat::Hexadecimal),
        ]),
    );
    doc.trailer.set("Info", nested_id);
    doc
}

/// A filter's name and its parameters, `null` for none.
type Filter = (Vec<u8>, Object);

/// What a reader reads from a stream: its dictionary without `/Length`, its filters with their
/// parameters, and its stored data. A stream that starts with one more `/ASCIIHexDecode` than
/// `original` is read through that filter first, so that comparing what this returns compares
/// what a reader decodes.
fn read_stream(stream: &Stream, original: &Stream) -> (Dictionary, Vec<Filter>, Vec<u8>) {
    fn filters(stream: &Stream) -> Vec<Filter> {
        let names: Vec<Vec<u8>> = stream
            .filters()
            .map(|names| names.into_iter().map(<[u8]>::to_vec).collect())
            .unwrap_or_default();
        let params = |index: usize| match stream.dict.get(b"DecodeParms") {
            Ok(Object::Array(params)) => params.get(index).cloned().unwrap_or(Object::Null),
            Ok(params @ Object::Dictionary(_)) => params.clone(),
            _ => Object::Null,
        };
        names
            .into_iter()
            .enumerate()
            .map(|(index, name)| (name, params(index)))
            .collect()
    }

    let mut chain = filters(stream);
    let mut content = stream.content.clone();
    if chain.len() == filters(original).len() + 1 {
        let (name, params) = chain.remove(0);
        assert_eq!(name, b"ASCIIHexDecode");
        assert_eq!(params, Object::Null);
        content = Stream::new(dictionary! { "Filter" => "ASCIIHexDecode" }, content)
            .get_plain_content()
            .unwrap();
    }

    let mut dict = stream.dict.clone();
    for key in [&b"Length"[..], b"Filter", b"DecodeParms"] {
        dict.remove(key);
    }
    (dict, chain, content)
}

/// Assert that `saved`, read back, holds every object of `doc` as it was.
fn assert_reads_back(doc: &Document, saved: &[u8], label: &str) {
    let loaded = Document::load_mem(saved).unwrap_or_else(|err| panic!("{label}: {err}"));
    // The cross-reference data has to locate every object: a rebuilt table would hide wrong
    // offsets.
    assert_eq!(loaded.xref_recovery, None, "{label}");

    for (id, object) in &doc.objects {
        let read = loaded
            .objects
            .get(id)
            .unwrap_or_else(|| panic!("{label}: {id:?} is missing"));
        match (object, read) {
            (Object::Stream(original), Object::Stream(read)) => {
                let (dict, filters, content) = read_stream(read, original);
                let (original_dict, original_filters, original_content) = read_stream(original, original);
                assert_eq!(dict, original_dict, "{label}: {id:?}");
                assert_eq!(filters, original_filters, "{label}: {id:?}");
                assert_eq!(content, original_content, "{label}: {id:?}");
            }
            _ => assert_eq!(read, object, "{label}: {id:?}"),
        }
    }
    // Anything else is a stream that the save generated.
    for (id, object) in &loaded.objects {
        if !doc.objects.contains_key(id) {
            let kind = object
                .as_stream()
                .unwrap()
                .dict
                .get(b"Type")
                .unwrap()
                .as_name()
                .unwrap();
            assert!(kind == b"ObjStm" || kind == b"XRef", "{label}: {id:?}");
        }
    }
    for key in [&b"Root"[..], b"Info", b"ID"] {
        assert_eq!(
            loaded.trailer.get(key).unwrap(),
            doc.trailer.get(key).unwrap(),
            "{label}"
        );
    }
}

fn assert_ascii(saved: &[u8], label: &str) {
    if let Some(position) = saved
        .iter()
        .position(|&byte| !matches!(byte, b'\t' | b'\n' | b'\r' | b' '..=b'~'))
    {
        panic!(
            "{label}: byte {:#04x} at {position} in {:?}",
            saved[position],
            String::from_utf8_lossy(&saved[position.saturating_sub(40)..saved.len().min(position + 40)])
        );
    }
}

/// Every way to save: a cross-reference table, a cross-reference stream, and object streams
/// with and without compression, each with `ascii`, `pretty`, or both.
fn saves() -> Vec<(String, XrefType, SaveOptions)> {
    let mut saves = Vec::new();
    for (ascii, pretty) in [(true, false), (false, true), (true, true)] {
        let options = || SaveOptions::builder().ascii(ascii).pretty(pretty);
        let label = format!("ascii {ascii}, pretty {pretty}");
        saves.push((
            format!("{label}, table"),
            XrefType::CrossReferenceTable,
            options().build(),
        ));
        saves.push((
            format!("{label}, stream"),
            XrefType::CrossReferenceTable,
            options().use_xref_streams(true).build(),
        ));
        for compression_level in [0, 6] {
            saves.push((
                format!("{label}, object streams at level {compression_level}"),
                XrefType::CrossReferenceStream,
                options()
                    .use_xref_streams(true)
                    .use_object_streams(true)
                    .compression_level(compression_level)
                    .build(),
            ));
        }
    }
    saves
}

#[test]
fn every_object_reads_back_as_it_was() {
    for (label, cross_reference_type, options) in saves() {
        let mut doc = document(cross_reference_type);
        let ascii = options.ascii;
        let mut saved = Vec::new();
        doc.save_with_options(&mut saved, options).unwrap();

        assert_reads_back(&doc, &saved, &label);
        if ascii {
            assert_ascii(&saved, &label);
        } else {
            assert!(saved.iter().any(|&byte| byte >= 0x80), "{label}");
        }
    }
}

#[test]
fn saving_leaves_the_document_as_it_was() {
    for (label, cross_reference_type, options) in saves() {
        let mut doc = document(cross_reference_type);
        let before = doc.clone();
        let mut first = Vec::new();
        doc.save_with_options(&mut first, options.clone()).unwrap();
        assert_eq!(doc.objects, before.objects, "{label}");
        assert_eq!(doc.trailer, before.trailer, "{label}");
        assert_eq!(doc.max_id, before.max_id, "{label}");

        let mut second = Vec::new();
        doc.save_with_options(&mut second, options).unwrap();
        assert!(first == second, "{label}");
    }
}

#[test]
fn ascii_output_has_no_binary_mark() {
    let mut doc = document(XrefType::CrossReferenceTable);
    let mut saved = Vec::new();
    doc.save_with_options(&mut saved, SaveOptions::builder().ascii(true).build())
        .unwrap();
    assert!(saved.starts_with(b"%PDF-1.5\n1 0 obj\n"));

    let mut saved = Vec::new();
    doc.save_to(&mut saved).unwrap();
    assert!(saved.starts_with(b"%PDF-1.5\n%\xBB\xAD\xC0\xDE\n"));
}

#[test]
fn ascii_output_encodes_binary_streams_with_ascii_hex_decode() {
    let mut doc = Document::with_version("1.5");
    let id = doc.add_object(Stream::new(
        dictionary! {
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 1 },
        },
        vec![0x00, 0x9C, 0xFF],
    ));
    let mut saved = Vec::new();
    doc.save_with_options(&mut saved, SaveOptions::builder().ascii(true).build())
        .unwrap();

    let text = String::from_utf8(saved).unwrap();
    assert!(text.contains(&format!(
        "{} 0 obj\n<</Filter[/ASCIIHexDecode/FlateDecode]/DecodeParms[null<</Predictor 1>>]/Length 7>>stream\n009CFF>\nendstream",
        id.0
    )));
}

#[test]
fn ascii_output_writes_octal_escapes_in_literal_strings() {
    let mut doc = Document::with_version("1.5");
    doc.add_object(Object::String(
        b"a\x00\x0C\x7F\xFF9\n\r\t(".to_vec(),
        StringFormat::Literal,
    ));
    let mut saved = Vec::new();
    doc.save_with_options(&mut saved, SaveOptions::builder().ascii(true).build())
        .unwrap();
    let text = String::from_utf8(saved).unwrap();
    assert!(
        text.contains("1 0 obj\n(a\\000\\014\\177\\3779\n\\r\t\\()\nendobj"),
        "{text}"
    );
}

#[test]
fn ascii_cross_reference_stream_counts_the_encoded_data() {
    let mut doc = document(XrefType::CrossReferenceStream);
    let mut saved = Vec::new();
    doc.save_with_options(&mut saved, SaveOptions::builder().ascii(true).build())
        .unwrap();

    let loaded = Document::load_mem(&saved).unwrap();
    assert_eq!(loaded.xref_recovery, None);
    let xref = loaded
        .objects
        .values()
        .filter_map(|object| object.as_stream().ok())
        .find(|stream| stream.dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"XRef"))
        .unwrap();
    assert_eq!(xref.filters().unwrap(), vec![&b"ASCIIHexDecode"[..]]);
    assert_eq!(
        xref.dict.get(b"Length").unwrap().as_i64().unwrap(),
        xref.content.len() as i64
    );
    assert!(xref.content.ends_with(b">"));
}

#[test]
fn pretty_output_indents_dictionaries_and_arrays() {
    let mut doc = Document::with_version("1.5");
    doc.add_object(dictionary! {
        "Type" => "Page",
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Annots" => vec![Object::Dictionary(dictionary! { "Subtype" => "Link" }), Object::Null],
        "Resources" => dictionary! { "Font" => dictionary! {} },
    });
    doc.add_object(Stream::new(dictionary! {}, b"q Q".to_vec()));
    let mut saved = Vec::new();
    doc.save_with_options(&mut saved, SaveOptions::builder().pretty(true).build())
        .unwrap();
    let text = String::from_utf8_lossy(&saved);
    let expected = concat!(
        "1 0 obj\n",
        "<<\n",
        "  /Type /Page\n",
        "  /MediaBox [0 0 612 792]\n",
        "  /Annots [\n",
        "    <<\n",
        "      /Subtype /Link\n",
        "    >>\n",
        "    null\n",
        "  ]\n",
        "  /Resources <<\n",
        "    /Font <<>>\n",
        "  >>\n",
        ">>\n",
        "endobj\n",
        "2 0 obj\n",
        "<<\n",
        "  /Length 3\n",
        ">>\n",
        "stream\n",
        "q Q\n",
        "endstream \n",
        "endobj\n",
    );
    assert!(text.contains(expected), "{text}");
}

#[test]
fn an_encrypted_document_cannot_be_saved_as_ascii() {
    let mut doc = document(XrefType::CrossReferenceTable);
    let version = EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner",
        user_password: "",
        key_length: 128,
        permissions: Permissions::PRINTABLE,
    };
    let state = EncryptionState::try_from(version).unwrap();
    doc.encrypt(&state).unwrap();

    let mut saved = Vec::new();
    let error = doc
        .save_with_options(&mut saved, SaveOptions::builder().ascii(true).build())
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(saved.is_empty());

    // Pretty printing does not need the data to change, so it still works.
    doc.save_with_options(&mut saved, SaveOptions::builder().pretty(true).build())
        .unwrap();
    assert!(!saved.is_empty());
}
