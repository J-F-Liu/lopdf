#[cfg(not(feature = "async"))]
use lopdf::{Document, Object};

#[test]
#[cfg(all(test, not(feature = "async")))]
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

    let obj1_exists = doc.get_object(id).is_ok();
    let obj2_exists = doc.get_object(id2).is_ok();

    assert!(obj1_exists);
    assert!(obj2_exists);
}

#[cfg(all(test, not(feature = "async")))]
mod tests_with_parsing {
    use super::*;
    use lopdf::Result;

    fn modify_text() -> Result<bool> {
        let mut doc = Document::load("assets/example.pdf")?;
        doc.version = "1.4".to_string();
        if let Some(Object::Stream(stream)) = doc.objects.get_mut(&(4, 0)) {
            let mut content = stream.decode_content().unwrap();
            content.operations[3].operands[0] = Object::string_literal("Modified text!");
            stream.set_content(content.encode().unwrap());
        }

        // Create temporary folder to store file.
        let temp_dir = tempfile::tempdir()?;
        let file_path = temp_dir.path().join("test_3_modify.pdf");
        doc.save(file_path)?;
        Ok(true)
    }

    #[test]
    fn test_modify() {
        assert!(modify_text().unwrap());
    }

    fn replace_text() -> Result<Document> {
        let mut doc = Document::load("assets/example.pdf")?;
        doc.replace_text(1, "Hello World!", "Modified text!", None)?;

        // Create temporary folder to store file.
        let temp_dir = tempfile::tempdir()?;
        let file_path = temp_dir.path().join("test_4_unicode_replace.pdf");
        doc.save(&file_path)?;

        let doc = Document::load(file_path)?;
        Ok(doc)
    }

    #[test]
    fn test_replace() {
        assert_eq!(replace_text().unwrap().extract_text(&[1]).unwrap(), "Modified text!\n");
    }

    fn replace_unicode_text() -> Result<Document> {
        let mut doc = Document::load("assets/unicode.pdf")?;
        doc.replace_text(1, "😀", "🔧2", Some("🔨"))?;

        let temp_dir = tempfile::tempdir()?;
        let file_path = temp_dir.path().join("test_4_unicode_replace.pdf");
        doc.save(&file_path)?;

        let doc = Document::load(file_path)?;
        Ok(doc)
    }

    #[test]
    fn test_unicode_replace() {
        let text = replace_unicode_text().unwrap().extract_text(&[1]).unwrap();
        assert_eq!(text, "🔧🔨\n🔧\n🔨\n");
    }

    fn get_mut() -> Result<bool> {
        let mut doc = Document::load("assets/example.pdf")?;
        let arr = doc
            .get_object_mut((5, 0))?
            .as_dict_mut()?
            .get_mut(b"Contents")?
            .as_array_mut()?;
        arr[0] = arr[0].clone();
        Ok(true)
    }

    #[test]
    fn test_get_mut() {
        assert!(get_mut().unwrap());
    }
}
