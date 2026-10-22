#![cfg(not(feature = "async"))]

use flate2::{Compression, write::ZlibEncoder};
use lopdf::{
    DecompressError, Document, EncryptionState, EncryptionVersion, Error, LoadOptions, Object, Permissions, Stream,
    StringFormat, dictionary, encryption::encrypt_object,
};
use std::io::Write;

fn object_stream_pdf(encrypted: bool, oversized: bool, malformed: bool) -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    document.trailer.set(
        "ID",
        vec![
            Object::string_literal("0123456789abcdef"),
            Object::string_literal("fedcba9876543210"),
        ],
    );
    let state = encrypted.then(|| {
        EncryptionState::try_from(EncryptionVersion::V2 {
            document: &document,
            owner_password: "owner",
            user_password: "user",
            key_length: 128,
            permissions: Permissions::all(),
        })
        .unwrap()
    });
    let text = if oversized {
        "x".repeat(2048)
    } else {
        "small".to_owned()
    };
    let decoded = format!("5 0 << /Value ({text}) >>");
    let mut compressor = ZlibEncoder::new(Vec::new(), Compression::default());
    compressor.write_all(decoded.as_bytes()).unwrap();
    let mut container = Object::Stream(Stream::new(
        dictionary! {
            "Type" => "ObjStm", "N" => 1, "First" => if malformed { 999999 } else { 4 }, "Filter" => "FlateDecode",
        },
        compressor.finish().unwrap(),
    ));
    if let Some(state) = &state {
        encrypt_object(state, (4, 0), &mut container).unwrap();
    }
    let mut pdf = b"%PDF-1.5\n".to_vec();
    let mut offsets = [0usize; 8];
    for (number, body) in [
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
    ] {
        offsets[number] = pdf.len();
        write!(pdf, "{number} 0 obj\n{body}\nendobj\n").unwrap();
    }
    offsets[4] = pdf.len();
    let stream = container.as_stream().unwrap();
    write!(pdf, "4 0 obj\n{:?}\nstream\n", stream.dict).unwrap();
    pdf.extend_from_slice(&stream.content);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    if let Some(state) = &state {
        offsets[7] = pdf.len();
        let mut encryption_dictionary = state.encode().unwrap();
        for key in [b"O", b"U"] {
            let bytes = encryption_dictionary.get(key).unwrap().as_str().unwrap().to_vec();
            encryption_dictionary.set(key, Object::String(bytes, StringFormat::Hexadecimal));
        }
        write!(pdf, "7 0 obj\n{encryption_dictionary:?}\nendobj\n").unwrap();
    }
    offsets[6] = pdf.len();
    let size = if encrypted { 8 } else { 7 };
    let mut xref = Vec::new();
    for (number, offset) in offsets.iter().enumerate().take(size) {
        let (kind, field, generation): (u8, u32, u16) = match number {
            0 => (0, 0, 65535),
            5 => (2, 4, 0),
            _ => (1, *offset as u32, 0),
        };
        xref.push(kind);
        xref.extend_from_slice(&field.to_be_bytes());
        xref.extend_from_slice(&generation.to_be_bytes());
    }
    let mut trailer = dictionary! { "Type" => "XRef", "Size" => size as i64, "W" => vec![1.into(), 4.into(), 2.into()], "Root" => Object::Reference((1, 0)), "Length" => xref.len() as i64 };
    if encrypted {
        trailer.set("Encrypt", Object::Reference((7, 0)));
        trailer.set("ID", document.trailer.get(b"ID").unwrap().clone());
    }
    write!(pdf, "6 0 obj\n{trailer:?}\nstream\n").unwrap();
    pdf.extend_from_slice(&xref);
    write!(pdf, "\nendstream\nendobj\nstartxref\n{}\n%%EOF\n", offsets[6]).unwrap();
    pdf
}

fn assert_object_stream_limit(encrypted: bool, strict: bool) {
    let bytes = object_stream_pdf(encrypted, true, false);
    let result = Document::load_mem_with_options(
        &bytes,
        LoadOptions {
            password: encrypted.then(|| "user".to_owned()),
            strict,
            max_decompressed_size: Some(512),
            ..Default::default()
        },
    );
    assert!(
        matches!(
            result,
            Err(Error::Decompress(DecompressError::MemoryLimitExceeded { limit: 512 }))
        ),
        "encrypted={encrypted}, strict={strict}: expected the configured limit error, got {:?}",
        result.map(|doc| doc.objects.len())
    );
}

#[test]
fn plain_lenient_load_propagates_object_stream_limit() {
    assert_object_stream_limit(false, false);
}
#[test]
fn plain_strict_load_propagates_object_stream_limit() {
    assert_object_stream_limit(false, true);
}
#[test]
fn encrypted_lenient_load_propagates_object_stream_limit() {
    assert_object_stream_limit(true, false);
}
#[test]
fn encrypted_strict_load_propagates_object_stream_limit() {
    assert_object_stream_limit(true, true);
}

#[test]
fn object_stream_under_limit_preserves_members() {
    for encrypted in [false, true] {
        for strict in [false, true] {
            let bytes = object_stream_pdf(encrypted, false, false);
            let doc = Document::load_mem_with_options(
                &bytes,
                LoadOptions {
                    password: encrypted.then(|| "user".to_owned()),
                    strict,
                    max_decompressed_size: Some(512),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                doc.get_dictionary((5, 0))
                    .unwrap()
                    .get(b"Value")
                    .unwrap()
                    .as_str()
                    .unwrap(),
                b"small"
            );
            assert_eq!(doc.get_pages().len(), 1);
        }
    }
}

#[test]
fn malformed_object_stream_still_obeys_strictness() {
    for encrypted in [false, true] {
        for strict in [false, true] {
            let bytes = object_stream_pdf(encrypted, false, true);
            let result = Document::load_mem_with_options(
                &bytes,
                LoadOptions {
                    password: encrypted.then(|| "user".to_owned()),
                    strict,
                    ..Default::default()
                },
            );
            assert_eq!(result.is_err(), strict, "encrypted={encrypted}, strict={strict}");
        }
    }
}
