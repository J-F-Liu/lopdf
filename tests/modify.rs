use lopdf::{Document, Object};

#[test]
fn test_get_object() {
    use self::Object;
    use lopdf::Dictionary as LoDictionary;
    use lopdf::Stream as LoStream;

    let mut doc = Document::new();
    let id = doc.add_object(Object::string_literal("test"));
    let id2 = doc.add_object(Object::Stream(LoStream::new(
        LoDictionary::new(),
        "stream".as_bytes().to_vec(),
    )));

    println!("{:?}", id);
    println!("{:?}", id2);
    assert!(doc.get_object(id).is_ok());
    assert!(doc.get_object(id2).is_ok());
}

#[cfg(any(feature = "pom_parser", feature = "nom_parser"))]
mod tests_with_parsing {
    use super::*;
    use lopdf::Result;

    fn modify_text() -> Result<Document> {
        let mut doc = Document::load("assets/example.pdf")?;
        doc.version = "1.4".to_string();
        if let Some(content_stream) = doc.objects.get_mut(&(4, 0)) {
            match *content_stream {
                Object::Stream(ref mut stream) => {
                    let mut content = stream.decode_content().unwrap();
                    content.operations[3].operands[0] = Object::string_literal("Modified text!");
                    stream.set_content(content.encode().unwrap());
                }
                _ => (),
            }
        }

        doc.save("test_3_modify.pdf")?;
        Ok(doc)
    }

    #[test]
    fn test_modify() {
        assert_eq!(modify_text().is_ok(), true);
    }

    fn replace_text() -> Result<Document> {
        let mut doc = Document::load("assets/example.pdf")?;
        doc.replace_text(1, "Hello World!", "Modified text!")?;
        doc.save("test_4_replace.pdf")?;

        let doc = Document::load("test_4_replace.pdf")?;
        Ok(doc)
    }

    #[test]
    fn test_replace() {
        assert_eq!(replace_text().unwrap().extract_text(&[1]).unwrap(), "Modified text!\n");
    }

    fn get_mut() -> Result<()> {
        let mut doc = Document::load("assets/example.pdf")?;
        let arr = doc
            .get_object_mut((5, 0))?
            .as_dict_mut()?
            .get_mut(b"Contents")?
            .as_array_mut()?;
        arr[0] = arr[0].clone();
        Ok(())
    }

    #[test]
    fn test_get_mut() {
        assert!(get_mut().is_ok());
    }
}
