use lopdf::{Object, ObjectId, ObjectStream, Stream, dictionary};

#[test]
fn test_object_stream_builder() {
    let builder = ObjectStream::builder();
    assert_eq!(builder.get_max_objects(), 100);
    assert_eq!(builder.get_compression_level(), 6);
}

/// Construct a raw ObjStm whose embedded objects are preceded by newlines
/// (as seen in real PDFs like MAECI documents). Without the whitespace-
/// skipping fix in ObjectStream::new, all objects would silently fail to
/// parse and the returned ObjectStream would be empty.
#[test]
fn test_object_stream_parses_objects_with_leading_whitespace() {
    // Two dictionaries preceded by \n: object 10 a Font, object 11 a FontDescriptor.
    let obj10_bytes = b"\n<< /Type /Font /Subtype /TrueType /BaseFont /Calibri >>";
    let obj11_bytes = b"\n<< /Type /FontDescriptor /FontName /Calibri >>";

    let obj10_offset = 0usize;
    let obj11_offset = obj10_bytes.len();

    // Build the index block: "obj_num offset obj_num offset "
    let index = format!("10 {} 11 {} ", obj10_offset, obj11_offset);
    let first_offset = index.len();

    // Assemble the full stream content: index block + object data
    let mut content = Vec::new();
    content.extend_from_slice(index.as_bytes());
    content.extend_from_slice(obj10_bytes);
    content.extend_from_slice(obj11_bytes);

    let dict = dictionary! {
        "Type" => "ObjStm",
        "N" => 2,
        "First" => first_offset as i64,
    };

    let stream = Stream::new(dict, content);
    let obj_stream = ObjectStream::new(&stream).expect("should parse object stream");

    // Both objects must be present
    assert_eq!(
        obj_stream.objects.len(),
        2,
        "expected 2 objects but got {}; leading whitespace was not skipped",
        obj_stream.objects.len()
    );

    // Verify object 10 is a Font dictionary
    let obj10 = obj_stream
        .objects
        .get(&(10u32, 0u16) as &ObjectId)
        .expect("object 10 missing");
    if let Object::Dictionary(dict) = obj10 {
        assert_eq!(dict.get(b"BaseFont").unwrap().as_name().unwrap(), b"Calibri");
    } else {
        panic!("object 10 should be a Dictionary, got {:?}", obj10);
    }

    // Verify object 11 is a FontDescriptor dictionary
    let obj11 = obj_stream
        .objects
        .get(&(11u32, 0u16) as &ObjectId)
        .expect("object 11 missing");
    if let Object::Dictionary(dict) = obj11 {
        assert_eq!(dict.get(b"FontName").unwrap().as_name().unwrap(), b"Calibri");
    } else {
        panic!("object 11 should be a Dictionary, got {:?}", obj11);
    }
}

/// Comments are white space. qpdf writes one before every object of an object stream in
/// QDF files, the first of them still in the index; another one here splits a pair.
#[test]
fn test_object_stream_parses_objects_after_comments() {
    let obj2 = "<< /Type /Catalog /Pages 3 0 R >>\n";
    let obj3 = "%% Object stream: object 3, index 1\n%% Page 1\n<< /Type /Page >>\n";
    let index = format!(
        "2 % the catalog\r0\n3 {}\n%% Object stream: object 2, index 0\n",
        obj2.len()
    );
    let dict = dictionary! {
        "Type" => "ObjStm",
        "N" => 2,
        "First" => index.len() as i64,
    };
    let stream = Stream::new(dict, format!("{index}{obj2}{obj3}").into_bytes());
    let obj_stream = ObjectStream::new(&stream).expect("should parse object stream");

    assert_eq!(obj_stream.objects.len(), 2);
    let page = obj_stream.objects[&(3, 0)].as_dict().unwrap();
    assert_eq!(page.get(b"Type").unwrap().as_name().unwrap(), b"Page");
}
