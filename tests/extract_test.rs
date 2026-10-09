use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, Stream, StringFormat, dictionary};

// Taken from a real PDF. The `0 beginbfrange ... endbfrange` sections triggered parse failures
// and broke text extraction before v0.38.0.
const FONT1_TOUNICODE: &str = r#"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo
<< /Registry (Adobe)
/Ordering (UCS)
/Supplement 0
>> def
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
0 beginbfrange
endbfrange
2 beginbfchar
<0003> <0020>
<0006> <0023>
endbfchar
3 beginbfrange
<0008> <0009> <0025>
<000b> <000c> <0028>
<000f> <0016> <002c>
endbfrange
3 beginbfchar
<001a> <0037>
<001d> <003a>
<0022> <003f>
endbfchar
3 beginbfrange
<0024> <002a> <0041>
<002c> <002d> <0049>
<002f> <0031> <004c>
endbfrange
1 beginbfchar
<0033> <0050>
endbfchar
1 beginbfrange
<0035> <0038> <0052>
endbfrange
2 beginbfchar
<003a> <0057>
<003c> <0059>
endbfchar
2 beginbfrange
<0044> <004c> <0061>
<004e> <005d> <006b>
endbfrange
endcmap
CMapName currentdict /CMap defineresource pop
end
end
"#;

const FONT2_TOUNICODE: &str = r#"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo
<< /Registry (Adobe)
/Ordering (UCS)
/Supplement 0
>> def
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
0 beginbfrange
endbfrange
2 beginbfchar
<0003> <0020>
<0007> <0024>
endbfchar
2 beginbfrange
<000a> <000c> <0027>
<000e> <0019> <002b>
endbfrange
1 beginbfchar
<001b> <0038>
endbfchar
5 beginbfrange
<001d> <001f> <003a>
<0021> <0022> <003e>
<0024> <002c> <0041>
<002e> <003b> <004b>
<0044> <005d> <0061>
endbfrange
2 beginbfchar
<0061> <007e>
<00bc> <20ac>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end
"#;

fn build_doc_with_tounicode(tounicode: &str, encoded_text: Vec<u8>) -> Document {
    let mut doc = Document::with_version("1.5");

    let pages_id = doc.new_object_id();

    let tounicode_bytes = tounicode.as_bytes().to_vec();
    let tounicode_stream_id = doc.add_object(Stream::new(
        dictionary! { "Length" => tounicode_bytes.len() as i64 },
        tounicode_bytes,
    ));

    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "DummyFont",
        "Encoding" => "Identity-H",
        "ToUnicode" => Object::Reference(tounicode_stream_id),
    });

    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    });

    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 12.into()]),
            Operation::new("Td", vec![50.into(), 700.into()]),
            Operation::new("Tj", vec![Object::String(encoded_text, StringFormat::Hexadecimal)]),
            Operation::new("ET", vec![]),
        ],
    };

    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Resources" => resources_id,
        "Contents" => content_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
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

    doc
}

#[test]
fn extract_text_does_not_error_with_empty_bfrange_font2() {
    // 2-byte codes (Identity-H): space, $, ' (bfrange), 8, ~ and € (bfchar).
    let encoded_text = vec![
        0x00, 0x03, // space
        0x00, 0x07, // $
        0x00, 0x0a, // '
        0x00, 0x1b, // 8
        0x00, 0x61, // ~
        0x00, 0xbc, // €
    ];

    let doc = build_doc_with_tounicode(FONT2_TOUNICODE, encoded_text);

    let text = doc.extract_text(&[1]).expect("extract_text should not error");
    assert_eq!(text.trim_end(), " $'8~€");
}

#[test]
fn extract_text_does_not_error_with_empty_bfrange_font1() {
    // Exercises both bfchar and bfrange plus an initial empty bfrange: space, #, % (bfrange
    // <0008> <0009> <0025>), 7 (bfchar), a (bfrange <0044> <004c> <0061>).
    let encoded_text = vec![
        0x00, 0x03, // space
        0x00, 0x06, // #
        0x00, 0x08, // %
        0x00, 0x1a, // 7
        0x00, 0x44, // a
    ];

    let doc = build_doc_with_tounicode(FONT1_TOUNICODE, encoded_text);

    let text = doc.extract_text(&[1]).expect("extract_text should not error");
    assert_eq!(text.trim_end(), " #%7a");
}

fn tounicode_with_codespaces(codespaces: &str, mappings: &str) -> String {
    format!(
        "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Test def
/CMapType 2 def
{} begincodespacerange
{codespaces}
endcodespacerange
{} beginbfchar
{mappings}
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end",
        codespaces.lines().count(),
        mappings.lines().count(),
    )
}

#[test]
fn extract_text_preserves_characters_after_unmapped_codes() {
    let cmap = tounicode_with_codespaces("<0000> <FFFF>", "<0001> <0041>\n<0002> <0042>");
    for (codes, expected) in [
        (vec![1, 2], "AB"),
        (vec![0, 1, 2], "\u{fffd}AB"),
        (vec![1, 0, 2], "A\u{fffd}B"),
        (vec![1, 3, 2], "A\u{fffd}B"),
        (vec![1, 0, 0, 2], "A\u{fffd}\u{fffd}B"),
        (vec![1, 2, 0], "AB\u{fffd}"),
    ] {
        let encoded_text = codes.into_iter().flat_map(u16::to_be_bytes).collect();
        let mut doc = build_doc_with_tounicode(&cmap, encoded_text);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let loaded = Document::load_mem(&bytes).unwrap();
        assert_eq!(loaded.extract_text(&[1]).unwrap(), format!("{expected}\n"));
    }
}

#[test]
fn decode_text_preserves_mixed_length_codes_after_unmapped_codes() {
    let cmap = tounicode_with_codespaces(
        "<00> <7F>\n<8140> <82FE>\n<820000> <8200FF>\n<83000000> <83FFFFFF>",
        "<41> <0041>\n<8140> <0042>\n<820001> <0043>\n<83000001> <0044>",
    );
    let mut doc = Document::new();
    let cmap_id = doc.add_object(Stream::new(dictionary! {}, cmap.into_bytes()));
    let font = dictionary! { "Type" => "Font", "ToUnicode" => cmap_id };
    let encoding = font.get_font_encoding(&doc).unwrap();
    let encoded_text = [
        0x00, 0x41, 0x81, 0x41, 0x81, 0x40, 0x82, 0x00, 0x00, 0x82, 0x00, 0x01, 0x83, 0x00, 0x00, 0x00, 0x83, 0x00,
        0x00, 0x01,
    ];
    assert_eq!(
        Document::decode_text(&encoding, &encoded_text).unwrap(),
        "\u{fffd}A\u{fffd}B\u{fffd}C\u{fffd}D"
    );
}

#[test]
fn encoding_dictionary_entries_are_optional() {
    // ISO 32000-1, Table 114: /Type, /BaseEncoding and /Differences may all be omitted.
    let doc = Document::new();
    for (encoding, expected) in [
        (dictionary! { "Differences" => vec![65.into(), "B".into()] }, "B\u{d8}"),
        (dictionary! { "BaseEncoding" => "WinAnsiEncoding" }, "A\u{e9}"),
        (
            dictionary! { "Type" => "Encoding", "BaseEncoding" => "WinAnsiEncoding" },
            "A\u{e9}",
        ),
    ] {
        let font = dictionary! { "Type" => "Font", "Encoding" => encoding };
        let encoding = font.get_font_encoding(&doc).unwrap();
        assert_eq!(Document::decode_text(&encoding, b"A\xe9").unwrap(), expected);
    }
}
