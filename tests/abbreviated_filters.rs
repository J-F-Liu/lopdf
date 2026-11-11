use lopdf::{Dictionary, Document, Object, SaveOptions, Stream, dictionary};

fn stream_with_filter(filter: &str, content: Vec<u8>) -> Stream {
    let mut dict = Dictionary::new();
    dict.set("Filter", filter);
    Stream::new(dict, content)
}

fn flate_compressed(content: &[u8]) -> Vec<u8> {
    let mut stream = Stream::new(Dictionary::new(), content.to_vec());
    stream.compress().unwrap();
    stream.content
}

#[test]
fn abbreviated_filter_names_decode_like_the_full_names() {
    // LZW vector from PDF 32000-1 7.4.4.2.
    let cases: [(&str, &str, Vec<u8>, &[u8]); 5] = [
        ("Fl", "FlateDecode", flate_compressed(&[b'a'; 64]), &[b'a'; 64]),
        (
            "LZW",
            "LZWDecode",
            vec![0x80, 0x0B, 0x60, 0x50, 0x22, 0x0C, 0x0C, 0x85, 0x01],
            b"-----A---B",
        ),
        ("A85", "ASCII85Decode", b"87cURDZ~>".to_vec(), b"Hello"),
        ("AHx", "ASCIIHexDecode", b"48656C6C6F>".to_vec(), b"Hello"),
        (
            "RL",
            "RunLengthDecode",
            [&[0x04][..], b"Hello", &[0x80]].concat(),
            b"Hello",
        ),
    ];

    for (abbreviation, name, content, expected) in cases {
        let full = stream_with_filter(name, content.clone());
        let short = stream_with_filter(abbreviation, content);

        assert_eq!(full.decompressed_content().unwrap(), expected, "/{name}");
        assert_eq!(short.decompressed_content().unwrap(), expected, "/{abbreviation}");
    }
}

#[test]
fn abbreviated_filter_names_decode_in_a_filter_array() {
    let mut dict = Dictionary::new();
    dict.set("Filter", vec![Object::from("AHx"), Object::from("Fl")]);
    let hex = flate_compressed(&[b'a'; 64])
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        + ">";
    let stream = Stream::new(dict, hex.into_bytes());

    assert_eq!(stream.decompressed_content().unwrap(), [b'a'; 64]);
}

fn pdf_with_page_tree_in_object_streams() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
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

    let options = SaveOptions::builder()
        .use_object_streams(true)
        .use_xref_streams(true)
        .compression_level(6)
        .build();
    let mut output = Vec::new();
    doc.save_with_options(&mut output, options).unwrap();
    output
}

/// Rewrite `/FlateDecode` as `/Fl` in every object stream dictionary, padding with spaces so
/// that the byte offsets in the cross-reference stream stay valid.
fn abbreviate_object_stream_filters(pdf: &mut [u8]) -> usize {
    let mut rewritten = 0;
    let mut search_from = 0;
    while let Some(position) = find(&pdf[search_from..], b"/ObjStm").map(|offset| search_from + offset) {
        let start = rfind(&pdf[..position], b"<<").unwrap();
        let end = position + find(&pdf[position..], b">>").unwrap();
        if let Some(offset) = find(&pdf[start..end], b"/FlateDecode") {
            let filter = start + offset;
            pdf[filter..filter + b"/FlateDecode".len()].copy_from_slice(b"/Fl         ");
            rewritten += 1;
        }
        search_from = end;
    }
    rewritten
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|window| window == needle)
}

#[test]
fn object_streams_with_an_abbreviated_filter_load_their_objects() {
    let mut pdf = pdf_with_page_tree_in_object_streams();
    assert!(
        abbreviate_object_stream_filters(&mut pdf) > 0,
        "fixture has no FlateDecode object stream"
    );

    let document = Document::load_mem(&pdf).unwrap();

    assert_eq!(document.get_pages().len(), 1);
}
