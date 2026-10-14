use lopdf::{
    Document, Stream,
    content::{Content, Operation},
    dictionary,
};

#[test]
fn get_page_content_separates_streams_at_token_boundary() {
    let mut doc = Document::with_version("1.5");
    // Producer writes stream that ends without trailing whitespace
    let s1 = doc.add_object(Stream::new(dictionary! {}, b"1 0 0 1 50".to_vec()));
    let s2 = doc.add_object(Stream::new(dictionary! {}, b"100 cm".to_vec()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Contents" => vec![s1.into(), s2.into()],
    });

    let bytes = doc.get_page_content(page_id);
    let content = Content::decode(&bytes).unwrap();

    assert_eq!(content.operations.len(), 1);
    let op = &content.operations[0];
    assert_eq!(op.operator, "cm");
    let operands: Vec<f32> = op.operands.iter().map(|o| o.as_float().unwrap()).collect();

    assert_eq!(operands, [1.0, 0.0, 0.0, 1.0, 50.0, 100.0]);
}

#[test]
fn get_page_content_terminates_trailing_comment() {
    let mut doc = Document::with_version("1.5");
    // Producer writes stream that ends with a comment without EOL
    let s1 = doc.add_object(Stream::new(dictionary! {}, b"% a comment".to_vec()));
    let s2 = doc.add_object(Stream::new(dictionary! {}, b"1 0 0 1 50 100 cm".to_vec()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Contents" => vec![s1.into(), s2.into()],
    });

    let bytes = doc.get_page_content(page_id);
    let content = Content::decode(&bytes).unwrap();

    assert_eq!(content.operations.len(), 1);
    let op = &content.operations[0];
    assert_eq!(op.operator, "cm");
    let operands: Vec<f32> = op.operands.iter().map(|o| o.as_float().unwrap()).collect();

    assert_eq!(operands, [1.0, 0.0, 0.0, 1.0, 50.0, 100.0]);
}

#[test]
fn encode_writes_inline_images_back_in_line() {
    let data = b"q BI /W 2 /H 1 /CS /G /BPC 8 ID \x00\xff\nEI Q";
    let content = Content::decode(data).unwrap();

    let encoded = content.encode().unwrap();
    assert_eq!(encoded, b"q\nBI /W 2 /H 1 /CS /G /BPC 8 ID \x00\xff\nEI\nQ");

    let decoded = Content::decode(&encoded).unwrap();
    let ops: Vec<&str> = decoded.operations.iter().map(|o| o.operator.as_str()).collect();
    assert_eq!(ops, ["q", "BI", "Q"]);
    let image = decoded.operations[1].operands[0].as_stream().unwrap();
    assert_eq!(image.content, b"\x00\xff");
    assert_eq!(image.dict.get(b"CS").unwrap().as_name().unwrap(), b"G");
}

#[test]
fn encode_writes_constructed_inline_image() {
    let image = Stream::new(
        dictionary! { "W" => 1, "H" => 1, "CS" => "RGB", "BPC" => 8 },
        b"EI ".to_vec(),
    );
    let content = Content {
        operations: vec![Operation::new("BI", vec![image.into()])],
    };

    let encoded = content.encode().unwrap();
    assert_eq!(encoded, b"BI /W 1 /H 1 /CS /RGB /BPC 8 ID EI \nEI");

    let decoded = Content::decode(&encoded).unwrap();
    assert_eq!(decoded.operations.len(), 1);
    assert_eq!(decoded.operations[0].operands[0].as_stream().unwrap().content, b"EI ");
}
