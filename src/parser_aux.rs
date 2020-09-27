#![cfg(any(feature = "pom_parser", feature = "nom_parser"))]

use crate::{
    content::{Content, Operation},
    document::Document,
    error::XrefError,
    object::Object::Name,
    xref::{Xref, XrefEntry},
    Error, Result,
};
use crate::{parser, Dictionary, Object, ObjectId, Stream};
use log::info;
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};

impl Content<Vec<Operation>> {
    /// Decode content operations.
    pub fn decode(data: &[u8]) -> Result<Self> {
        parser::content(data).ok_or(Error::ContentDecode)
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

    pub fn extract_text(&self, page_numbers: &[u32]) -> Result<String> {
        fn collect_text(text: &mut String, encoding: Option<&str>, operands: &[Object]) {
            for operand in operands.iter() {
                match *operand {
                    Object::String(ref bytes, _) => {
                        let decoded_text = Document::decode_text(encoding, bytes);
                        text.push_str(&decoded_text);
                    }
                    Object::Array(ref arr) => {
                        collect_text(text, encoding, arr);
                    }
                    _ => {}
                }
            }
        }
        let mut text = String::new();
        let pages = self.get_pages();
        for page_number in page_numbers {
            let page_id = *pages.get(page_number).ok_or(Error::PageNumberNotFound(*page_number))?;
            let fonts = self.get_page_fonts(page_id);
            let encodings = fonts
                .into_iter()
                .map(|(name, font)| (name, font.get_font_encoding()))
                .collect::<BTreeMap<Vec<u8>, &str>>();
            let content_data = self.get_page_content(page_id)?;
            let content = Content::decode(&content_data)?;
            let mut current_encoding = None;
            for operation in &content.operations {
                match operation.operator.as_ref() {
                    "Tf" => {
                        let current_font = operation
                            .operands
                            .get(0)
                            .ok_or(Error::Syntax("missing font operand".to_string()))?
                            .as_name()?;
                        current_encoding = encodings.get(current_font).cloned();
                    }
                    "Tj" | "TJ" => {
                        collect_text(&mut text, current_encoding, &operation.operands);
                    }
                    "ET" => {
                        if !text.ends_with('\n') {
                            text.push('\n')
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(text)
    }

    pub fn replace_text(&mut self, page_number: u32, text: &str, other_text: &str) -> Result<()> {
        let page_id = self
            .page_iter()
            .nth(page_number as usize - 1)
            .ok_or(Error::PageNumberNotFound(page_number))?;
        let encodings = self
            .get_page_fonts(page_id)
            .into_iter()
            .map(|(name, font)| (name, font.get_font_encoding().to_owned()))
            .collect::<BTreeMap<Vec<u8>, String>>();
        let content_data = self.get_page_content(page_id)?;
        let mut content = Content::decode(&content_data)?;
        let mut current_encoding = None;
        for operation in &mut content.operations {
            match operation.operator.as_ref() {
                "Tf" => {
                    let current_font = operation
                        .operands
                        .get(0)
                        .ok_or(Error::Syntax("missing font operand".to_string()))?
                        .as_name()?;
                    current_encoding = encodings.get(current_font).map(std::string::String::as_str);
                }
                "Tj" => {
                    for bytes in operation.operands.iter_mut().flat_map(Object::as_str_mut) {
                        let decoded_text = Document::decode_text(current_encoding, bytes);
                        info!("{}", decoded_text);
                        if decoded_text == text {
                            let encoded_bytes = Document::encode_text(current_encoding, other_text);
                            *bytes = encoded_bytes;
                        }
                    }
                }
                _ => {}
            }
        }
        let modified_content = content.encode()?;
        self.change_page_content(page_id, modified_content)
    }

    pub fn insert_image(
        &mut self, page_id: ObjectId, img_object: Stream, position: (f64, f64), size: (f64, f64),
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
        content.operations.push(Operation::new("Q", vec![]));

        self.change_page_content(page_id, content.encode()?)
    }

    pub fn insert_form_object(&mut self, page_id: ObjectId, form_obj: Stream) -> Result<()> {
        let form_id = self.add_object(form_obj);
        let form_name = format!("X{}", form_id.0);

        let mut content = self.get_and_decode_page_content(page_id)?;
        content.operations.insert(0, Operation::new("q", vec![]));
        content.operations.push(Operation::new("Q", vec![]));
        // content.operations.push(Operation::new("q", vec![]));
        content
            .operations
            .push(Operation::new("Do", vec![Name(form_name.as_bytes().to_vec())]));
        // content.operations.push(Operation::new("Q", vec![]));
        let modified_content = content.encode()?;
        self.add_xobject(page_id, form_name, form_id)?;

        self.change_page_content(page_id, modified_content)
    }
}

pub fn decode_xref_stream(mut stream: Stream) -> Result<(Xref, Dictionary)> {
    stream.decompress();
    let mut dict = stream.dict;
    let mut reader = Cursor::new(stream.content);
    let size = dict
        .get(b"Size")
        .and_then(Object::as_i64)
        .map_err(|_| Error::Xref(XrefError::Parse))?;
    let mut xref = Xref::new(size as u32);
    {
        let section_indice = dict
            .get(b"Index")
            .and_then(parse_integer_array)
            .unwrap_or_else(|_| vec![0, size]);
        let field_widths = dict
            .get(b"W")
            .and_then(parse_integer_array)
            .map_err(|_| Error::Xref(XrefError::Parse))?;

        if field_widths.len() < 3 {
            return Err(Error::Xref(XrefError::Parse));
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
                        //free object
                        read_big_endian_integer(&mut reader, bytes2.as_mut_slice())?;
                        read_big_endian_integer(&mut reader, bytes3.as_mut_slice())?;
                    }
                    1 => {
                        //normal object
                        let offset = read_big_endian_integer(&mut reader, bytes2.as_mut_slice())?;
                        let generation = if !bytes3.is_empty() {
                            read_big_endian_integer(&mut reader, bytes3.as_mut_slice())?
                        } else {
                            0
                        } as u16;
                        xref.insert((start + j) as u32, XrefEntry::Normal { offset, generation });
                    }
                    2 => {
                        //compressed object
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

#[test]
fn load_and_save() {
    // test load_from() and save_to()
    use std::fs::File;
    use std::io::Cursor;

    let in_file = File::open("test_1_create.pdf").unwrap();
    let mut in_doc = Document::load_from(in_file).unwrap();

    let out_buf = Vec::new();
    let mut memory_cursor = Cursor::new(out_buf);
    in_doc.save_to(&mut memory_cursor).unwrap();
    assert!(!memory_cursor.get_ref().is_empty());
}
