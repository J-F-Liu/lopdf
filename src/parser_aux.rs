#![cfg(feature = "nom_parser")]
use log::warn;

use crate::{
    content::{Content, Operation},
    document::Document,
    encodings::Encoding,
    error::ParseError,
    object::Object::Name,
    parser::ParserInput,
    xref::{Xref, XrefEntry, XrefType},
    Error, Result,
};
use crate::{parser, Dictionary, Object, ObjectId, Stream};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};

impl Content<Vec<Operation>> {
    /// Decode content operations.
    pub fn decode(data: &[u8]) -> Result<Self> {
        parser::content(ParserInput::new_extra(data, "content operations"))
            .ok_or(ParseError::InvalidContentStream.into())
    }
}

impl Stream {
    /// Decode content after decoding all stream filters.
    pub fn decode_content(&self) -> Result<Content<Vec<Operation>>> {
        Content::decode(&self.content)
    }
}

impl Document {
    /// Get decoded page content;
    pub fn get_and_decode_page_content(&self, page_id: ObjectId) -> Result<Content<Vec<Operation>>> {
        let content_data = self.get_page_content(page_id)?;
        Content::decode(&content_data)
    }

    /// Add content to a page. All existing content will be unchanged.
    pub fn add_to_page_content(&mut self, page_id: ObjectId, content: Content<Vec<Operation>>) -> Result<()> {
        let content_data = Content::encode(&content)?;
        self.add_page_contents(page_id, content_data)?;
        Ok(())
    }

    pub fn extract_text(&self, page_numbers: &[u32]) -> Result<String> {
        let text_fragments = self.extract_text_chunks(page_numbers);
        let mut text = String::new();
        for maybe_text_fragment in text_fragments.into_iter() {
            let text_fragment = maybe_text_fragment?;
            text.push_str(&text_fragment);
        }

        Ok(text)
    }

    pub fn extract_text_chunks(&self, page_numbers: &[u32]) -> Vec<Result<String>> {
        let pages: BTreeMap<u32, (u32, u16)> = self.get_pages();
        page_numbers
            .iter()
            .flat_map(|page_number| {
                let result = self.extract_text_chunks_from_page(&pages, *page_number);
                match result {
                    Ok(text_chunks) => text_chunks,
                    Err(err) => vec![Err(err)],
                }
            })
            .collect()
    }

    fn extract_text_chunks_from_page(
        &self, pages: &BTreeMap<u32, (u32, u16)>, page_number: u32,
    ) -> Result<Vec<Result<String>>> {
        fn collect_text(text: &mut String, encoding: &Encoding, operands: &[Object]) -> Result<()> {
            for operand in operands.iter() {
                match *operand {
                    Object::String(ref bytes, _) => {
                        text.push_str(&Document::decode_text(encoding, bytes)?);
                    }
                    Object::Array(ref arr) => {
                        collect_text(text, encoding, arr)?;
                        text.push(' ');
                    }
                    Object::Integer(i) => {
                        if i < -100 {
                            text.push(' ');
                        }
                    }
                    _ => {}
                }
            }
            Ok(())
        }
        let mut collected_chunks_and_errs: Vec<std::result::Result<String, Error>> = Vec::new();

        let page_id = *pages.get(&page_number).ok_or(Error::PageNumberNotFound(page_number))?;
        let fonts = self.get_page_fonts(page_id)?;
        let encodings: BTreeMap<Vec<u8>, Encoding> = fonts
            .into_iter()
            .filter_map(|(name, font)| match font.get_font_encoding(self) {
                Ok(it) => Some((name, it)),
                Err(err) => {
                    collected_chunks_and_errs.push(Err(err));
                    None
                }
            })
            .collect();
        let content_data = self.get_page_content(page_id)?;
        let content = Content::decode(&content_data)?;

        // each text with different encoding is extracted as separate chunk
        let mut current_encoding = None;
        let mut current_text = String::new();
        for operation in &content.operations {
            match operation.operator.as_ref() {
                "Tf" => {
                    let current_font = operation
                        .operands
                        .first()
                        .ok_or_else(|| Error::Syntax("missing font operand".to_string()))?
                        .as_name();
                    current_encoding = match current_font {
                        Ok(font) => encodings.get(font),
                        Err(err) => {
                            collected_chunks_and_errs.push(Err(err));
                            None
                        }
                    };

                    if !current_text.is_empty() {
                        collected_chunks_and_errs.push(Ok(current_text));
                        current_text = String::new();
                    }
                }
                "Tj" | "TJ" => match current_encoding {
                    Some(encoding) => {
                        let res = collect_text(&mut current_text, encoding, &operation.operands);
                        if let Err(err) = res {
                            collected_chunks_and_errs.push(Err(err));
                        }
                    }
                    None => warn!("Could not decode extracted text"),
                },
                "ET" => {
                    if !current_text.ends_with('\n') {
                        current_text.push('\n')
                    }
                }
                _ => {}
            }
        }
        if !current_text.is_empty() {
            collected_chunks_and_errs.push(Ok(current_text));
        }

        Ok(collected_chunks_and_errs)
    }

    pub fn replace_text(&mut self, page_number: u32, text: &str, other_text: &str) -> Result<()> {
        let page = page_number.saturating_sub(1) as usize;
        let page_id = self
            .page_iter()
            .nth(page)
            .ok_or(Error::PageNumberNotFound(page_number))?;
        let encodings: BTreeMap<Vec<u8>, Encoding> = self
            .get_page_fonts(page_id)?
            .into_iter()
            .map(|(name, font)| font.get_font_encoding(self).map(|it| (name, it)))
            .collect::<Result<BTreeMap<Vec<u8>, Encoding>>>()?;
        let content_data = self.get_page_content(page_id)?;
        let mut content = Content::decode(&content_data)?;
        let mut current_encoding = None;
        for operation in &mut content.operations {
            match operation.operator.as_ref() {
                "Tf" => {
                    let current_font = operation
                        .operands
                        .first()
                        .ok_or_else(|| Error::Syntax("missing font operand".to_string()))?
                        .as_name()?;
                    current_encoding = encodings.get(current_font);
                }
                "Tj" => match current_encoding {
                    Some(encoding) => try_to_replace_encoded_text(operation, encoding, text, other_text)?,
                    None => {
                        warn!("Could not decode extracted text, some of the occurances might not be properly replaced")
                    }
                },
                _ => {}
            }
        }
        let modified_content = content.encode()?;
        self.change_page_content(page_id, modified_content)
    }

    pub fn insert_image(
        &mut self, page_id: ObjectId, img_object: Stream, position: (f32, f32), size: (f32, f32),
    ) -> Result<()> {
        let img_id = self.add_object(img_object);
        let img_name = format!("X{}", img_id.0);

        self.add_xobject(page_id, img_name.as_bytes(), img_id)?;

        let mut content = self.get_and_decode_page_content(page_id)?;
        content.operations.push(Operation::new("q", vec![]));
        content.operations.push(Operation::new(
            "cm",
            vec![
                size.0.into(),
                0.into(),
                0.into(),
                size.1.into(),
                position.0.into(),
                position.1.into(),
            ],
        ));
        content
            .operations
            .push(Operation::new("Do", vec![Name(img_name.as_bytes().to_vec())]));
        content.operations.push(Operation::new("Q", vec![]));

        self.change_page_content(page_id, content.encode()?)
    }

    pub fn insert_form_object(&mut self, page_id: ObjectId, form_obj: Stream) -> Result<()> {
        let form_id = self.add_object(form_obj);
        let form_name = format!("X{}", form_id.0);

        let mut content = self.get_and_decode_page_content(page_id)?;
        content.operations.insert(0, Operation::new("q", vec![]));
        content.operations.push(Operation::new("Q", vec![]));
        content
            .operations
            .push(Operation::new("Do", vec![Name(form_name.as_bytes().to_vec())]));
        let modified_content = content.encode()?;
        self.add_xobject(page_id, form_name, form_id)?;

        self.change_page_content(page_id, modified_content)
    }
}

fn try_to_replace_encoded_text(
    operation: &mut Operation, encoding: &Encoding, text_to_replace: &str, replacement: &str,
) -> Result<()> {
    for bytes in operation.operands.iter_mut().flat_map(Object::as_str_mut) {
        let decoded_text = Document::decode_text(encoding, bytes)?;
        if decoded_text == text_to_replace {
            let encoded_bytes = Document::encode_text(encoding, replacement);
            *bytes = encoded_bytes;
        }
    }
    Ok(())
}

/// Decode CrossReferenceStream
pub fn decode_xref_stream(mut stream: Stream) -> Result<(Xref, Dictionary)> {
    if stream.is_compressed() {
        stream.decompress()?;
    }
    let mut dict = stream.dict;
    let mut reader = Cursor::new(stream.content);
    let size = dict
        .get(b"Size")
        .and_then(Object::as_i64)
        .map_err(|_| ParseError::InvalidXref)?;
    let mut xref = Xref::new(size as u32, XrefType::CrossReferenceStream);
    {
        let section_indice = dict
            .get(b"Index")
            .and_then(parse_integer_array)
            .unwrap_or_else(|_| vec![0, size]);
        let field_widths = dict
            .get(b"W")
            .and_then(parse_integer_array)
            .map_err(|_| ParseError::InvalidXref)?;

        if field_widths.len() < 3
            || field_widths[0].is_negative()
            || field_widths[1].is_negative()
            || field_widths[2].is_negative()
        {
            return Err(ParseError::InvalidXref.into());
        }

        let mut bytes1 = vec![0_u8; field_widths[0] as usize];
        let mut bytes2 = vec![0_u8; field_widths[1] as usize];
        let mut bytes3 = vec![0_u8; field_widths[2] as usize];

        for i in 0..section_indice.len() / 2 {
            let start = section_indice[2 * i];
            let count = section_indice[2 * i + 1];

            for j in 0..count {
                let entry_type = if !bytes1.is_empty() {
                    read_big_endian_integer(&mut reader, bytes1.as_mut_slice())?
                } else {
                    1
                };
                match entry_type {
                    0 => {
                        // free object
                        read_big_endian_integer(&mut reader, bytes2.as_mut_slice())?;
                        read_big_endian_integer(&mut reader, bytes3.as_mut_slice())?;
                    }
                    1 => {
                        // normal object
                        let offset = read_big_endian_integer(&mut reader, bytes2.as_mut_slice())?;
                        let generation = if !bytes3.is_empty() {
                            read_big_endian_integer(&mut reader, bytes3.as_mut_slice())?
                        } else {
                            0
                        } as u16;
                        xref.insert((start + j) as u32, XrefEntry::Normal { offset, generation });
                    }
                    2 => {
                        // compressed object
                        let container = read_big_endian_integer(&mut reader, bytes2.as_mut_slice())?;
                        let index = read_big_endian_integer(&mut reader, bytes3.as_mut_slice())? as u16;
                        xref.insert((start + j) as u32, XrefEntry::Compressed { container, index });
                    }
                    _ => {}
                }
            }
        }
    }
    dict.remove(b"Length");
    dict.remove(b"W");
    dict.remove(b"Index");
    Ok((xref, dict))
}

fn read_big_endian_integer(reader: &mut Cursor<Vec<u8>>, buffer: &mut [u8]) -> Result<u32> {
    reader.read_exact(buffer)?;
    let mut value = 0;
    for &mut byte in buffer {
        value = (value << 8) + u32::from(byte);
    }
    Ok(value)
}

fn parse_integer_array(array: &Object) -> Result<Vec<i64>> {
    let array = array.as_array()?;
    let mut out = Vec::with_capacity(array.len());

    for n in array {
        out.push(n.as_i64()?);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creator::tests::{create_document, create_document_with_texts, save_document};

    #[cfg(not(feature = "async"))]
    #[test]
    fn load_and_save() {
        // test load_from() and save_to()
        use std::fs::File;
        use std::io::Cursor;
        // Create temporary folder to store file.
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test_1_load_and_save.pdf");

        let mut doc = create_document();

        save_document(&file_path, &mut doc);

        let in_file = File::open(file_path).unwrap();
        let mut in_doc = Document::load_from(in_file).unwrap();

        let out_buf = Vec::new();
        let mut memory_cursor = Cursor::new(out_buf);
        in_doc.save_to(&mut memory_cursor).unwrap();
        // Check if saved file is not an empty bytes vector.
        assert!(!memory_cursor.get_ref().is_empty());
    }

    #[test]
    fn extract_text_chunks() {
        let text1 = "Hello world!";
        let text2 = "Ferris is the best!";
        let doc = create_document_with_texts(&[text1, text2]);
        let extracted_texts = doc.extract_text_chunks(&[1, 2]);
        assert_eq!(extracted_texts.len(), 2);
        assert_eq!(
            [
                extracted_texts[0].as_ref().unwrap().trim(),
                extracted_texts[1].as_ref().unwrap().trim()
            ],
            [text1, text2]
        );
    }

    #[test]
    fn extract_text_concatenates_text_from_multiple_pages() {
        let text1 = "Hello world!";
        let text2 = "Ferris is the best!";
        let doc = create_document_with_texts(&[text1, text2]);
        let extracted_text = doc.extract_text(&[1, 2]);
        assert_eq!(extracted_text.unwrap(), format!("{text1}\n{text2}\n"));
    }
}
