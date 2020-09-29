use super::encodings::{self, bytes_to_string, string_to_bytes};
use super::{Dictionary, Object, ObjectId};
use crate::xref::Xref;
use crate::{Error, Result};
use encoding::all::UTF_16BE;
use encoding::types::{DecoderTrap, EncoderTrap, Encoding};
use log::info;
use std::cmp::max;
use std::collections::BTreeMap;
use std::io::Write;
use std::str;

/// PDF document.
#[derive(Debug, Clone)]
pub struct Document {
    /// The version of the PDF specification to which the file conforms.
    pub version: String,

    /// The trailer gives the location of the cross-reference table and of certain special objects.
    pub trailer: Dictionary,

    /// The cross-reference table contains locations of the indirect objects.
    pub reference_table: Xref,

    /// The objects that make up the document contained in the file.
    pub objects: BTreeMap<ObjectId, Object>,

    /// Current maximum object id within the document.
    pub max_id: u32,
}

impl Document {
    /// Create new PDF document.
    pub fn new() -> Document {
        Document {
            version: "1.4".to_string(),
            trailer: Dictionary::new(),
            reference_table: Xref::new(0),
            objects: BTreeMap::new(),
            max_id: 0,
        }
    }

    const DEREF_LIMIT: usize = 128;

    /// Follow references if the supplied object is a reference.
    ///
    /// Returns a tuple of an optional object id and final object.
    /// The object id will be None if the object was not a
    /// reference. Otherwise, it will be the last object id in the
    /// reference chain.
    pub fn dereference<'a>(&'a self, mut object: &'a Object) -> Result<(Option<ObjectId>, &'a Object)> {
        let mut nb_deref = 0;
        let mut id = None;

        while let Ok(ref_id) = object.as_reference() {
            id = Some(ref_id);
            object = self.objects.get(&ref_id).ok_or(Error::ObjectNotFound)?;

            nb_deref += 1;
            if nb_deref > Self::DEREF_LIMIT {
                return Err(Error::ReferenceLimit);
            }
        }

        Ok((id, object))
    }

    /// Get object by object id, will iteratively dereference a referenced object.
    pub fn get_object(&self, id: ObjectId) -> Result<&Object> {
        let object = self.objects.get(&id).ok_or(Error::ObjectNotFound)?;
        self.dereference(object).map(|(_, object)| object)
    }

    /// Get mutable reference to object by object id, will iteratively dereference a referenced object.
    pub fn get_object_mut(&mut self, id: ObjectId) -> Result<&mut Object> {
        let object = self.objects.get(&id).ok_or(Error::ObjectNotFound)?;
        let (ref_id, _) = self.dereference(object)?;

        Ok(self.objects.get_mut(&ref_id.unwrap_or(id)).unwrap())
    }

    /// Get page object_id of the specified object object_id
    pub fn get_object_page(&self, id: ObjectId) -> Result<ObjectId> {
        for (_, object_id) in self.get_pages() {
            let page = self.get_object(object_id)?.as_dict()?;
            let annots = page.get(b"Annots")?.as_array()?;
            let objects_ids = annots.iter().map(|object| object.as_reference()).collect::<Vec<_>>();

            if objects_ids.iter().any(|object_id| {
                if let Ok(object_id) = object_id {
                    return id == *object_id;
                }

                false
            }) {
                return Ok(object_id);
            }
        }

        Err(Error::ObjectNotFound)
    }

    /// Get dictionary object by id.
    pub fn get_dictionary(&self, id: ObjectId) -> Result<&Dictionary> {
        self.get_object(id).and_then(Object::as_dict)
    }

    /// Traverse objects from trailer recursively, return all referenced object IDs.
    pub fn traverse_objects<A: Fn(&mut Object) -> ()>(&mut self, action: A) -> Vec<ObjectId> {
        fn traverse_array<A: Fn(&mut Object) -> ()>(array: &mut Vec<Object>, action: &A, refs: &mut Vec<ObjectId>) {
            for item in array.iter_mut() {
                traverse_object(item, action, refs);
            }
        }
        fn traverse_dictionary<A: Fn(&mut Object) -> ()>(dict: &mut Dictionary, action: &A, refs: &mut Vec<ObjectId>) {
            for (_, v) in dict.iter_mut() {
                traverse_object(v, action, refs);
            }
        }
        fn traverse_object<A: Fn(&mut Object) -> ()>(object: &mut Object, action: &A, refs: &mut Vec<ObjectId>) {
            action(object);
            match *object {
                Object::Array(ref mut array) => traverse_array(array, action, refs),
                Object::Dictionary(ref mut dict) => traverse_dictionary(dict, action, refs),
                Object::Stream(ref mut stream) => traverse_dictionary(&mut stream.dict, action, refs),
                Object::Reference(id) => {
                    if !refs.contains(&id) {
                        refs.push(id);
                    }
                }
                _ => {}
            }
        }
        let mut refs = vec![];
        traverse_dictionary(&mut self.trailer, &action, &mut refs);
        let mut index = 0;
        while index < refs.len() {
            if let Some(object) = self.objects.get_mut(&refs[index]) {
                traverse_object(object, &action, &mut refs);
            }
            index += 1;
        }
        refs
    }

    /// Get catalog dictionary.
    pub fn catalog(&self) -> Result<&Dictionary> {
        self.trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .and_then(|id| self.get_dictionary(id))
    }

    /// Get page numbers and corresponding object ids.
    pub fn get_pages(&self) -> BTreeMap<u32, ObjectId> {
        self.page_iter().enumerate().map(|(i, p)| ((i + 1) as u32, p)).collect()
    }

    pub fn page_iter(&self) -> impl Iterator<Item = ObjectId> + '_ {
        PageTreeIter::new(self)
    }

    /// Get content stream object ids of a page.
    pub fn get_page_contents(&self, page_id: ObjectId) -> Vec<ObjectId> {
        let mut streams = vec![];
        if let Ok(page) = self.get_dictionary(page_id) {
            if let Ok(contents) = page.get(b"Contents") {
                match *contents {
                    Object::Reference(ref id) => {
                        streams.push(*id);
                    }
                    Object::Array(ref arr) => {
                        for content in arr {
                            if let Ok(id) = content.as_reference() {
                                streams.push(id)
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        streams
    }

    /// Get content of a page.
    pub fn get_page_content(&self, page_id: ObjectId) -> Result<Vec<u8>> {
        let mut content = Vec::new();
        let content_streams = self.get_page_contents(page_id);
        for object_id in content_streams {
            if let Ok(content_stream) = self.get_object(object_id).and_then(Object::as_stream) {
                match content_stream.decompressed_content() {
                    Ok(data) => content.write_all(&data)?,
                    Err(_) => content.write_all(&content_stream.content)?,
                };
            }
        }
        Ok(content)
    }

    /// Get resources used by a page.
    pub fn get_page_resources(&self, page_id: ObjectId) -> (Option<&Dictionary>, Vec<ObjectId>) {
        fn collect_resources(page_node: &Dictionary, resource_ids: &mut Vec<ObjectId>, doc: &Document) {
            if let Ok(resources_id) = page_node.get(b"Resources").and_then(Object::as_reference) {
                resource_ids.push(resources_id);
            }
            if let Ok(page_tree) = page_node
                .get(b"Parent")
                .and_then(Object::as_reference)
                .and_then(|id| doc.get_dictionary(id))
            {
                collect_resources(page_tree, resource_ids, doc);
            }
        };

        let mut resource_dict = None;
        let mut resource_ids = Vec::new();
        if let Ok(page) = self.get_dictionary(page_id) {
            resource_dict = page.get(b"Resources").and_then(Object::as_dict).ok();
            collect_resources(page, &mut resource_ids, self);
        }
        (resource_dict, resource_ids)
    }

    /// Get fonts used by a page.
    pub fn get_page_fonts(&self, page_id: ObjectId) -> BTreeMap<Vec<u8>, &Dictionary> {
        fn collect_fonts_from_resources<'a>(
            resources: &'a Dictionary, fonts: &mut BTreeMap<Vec<u8>, &'a Dictionary>, doc: &'a Document,
        ) {
            if let Ok(font_dict) = resources.get(b"Font").and_then(Object::as_dict) {
                for (name, value) in font_dict.iter() {
                    let font = match *value {
                        Object::Reference(id) => doc.get_dictionary(id).ok(),
                        Object::Dictionary(ref dict) => Some(dict),
                        _ => None,
                    };
                    if !fonts.contains_key(name) {
                        font.map(|font| fonts.insert(name.clone(), font));
                    }
                }
            }
        };

        let mut fonts = BTreeMap::new();
        let (resource_dict, resource_ids) = self.get_page_resources(page_id);
        if let Some(resources) = resource_dict {
            collect_fonts_from_resources(resources, &mut fonts, self);
        }
        for resource_id in resource_ids {
            if let Ok(resources) = self.get_dictionary(resource_id) {
                collect_fonts_from_resources(resources, &mut fonts, self);
            }
        }
        fonts
    }

    pub fn decode_text(encoding: Option<&str>, bytes: &[u8]) -> String {
        if let Some(encoding) = encoding {
            info!("{}", encoding);
            match encoding {
                "StandardEncoding" => bytes_to_string(encodings::STANDARD_ENCODING, bytes),
                "MacRomanEncoding" => bytes_to_string(encodings::MAC_ROMAN_ENCODING, bytes),
                "MacExpertEncoding" => bytes_to_string(encodings::MAC_EXPERT_ENCODING, bytes),
                "WinAnsiEncoding" => bytes_to_string(encodings::WIN_ANSI_ENCODING, bytes),
                "UniGB-UCS2-H" | "UniGB−UTF16−H" => UTF_16BE.decode(bytes, DecoderTrap::Ignore).unwrap(),
                "Identity-H" => "?Identity-H Unimplemented?".to_string(), // Unimplemented
                _ => String::from_utf8_lossy(bytes).to_string(),
            }
        } else {
            bytes_to_string(encodings::STANDARD_ENCODING, bytes)
        }
    }

    pub fn encode_text(encoding: Option<&str>, text: &str) -> Vec<u8> {
        if let Some(encoding) = encoding {
            match encoding {
                "StandardEncoding" => string_to_bytes(encodings::STANDARD_ENCODING, text),
                "MacRomanEncoding" => string_to_bytes(encodings::MAC_ROMAN_ENCODING, text),
                "MacExpertEncoding" => string_to_bytes(encodings::MAC_EXPERT_ENCODING, text),
                "WinAnsiEncoding" => string_to_bytes(encodings::WIN_ANSI_ENCODING, text),
                "UniGB-UCS2-H" | "UniGB−UTF16−H" => UTF_16BE.encode(text, EncoderTrap::Ignore).unwrap(),
                "Identity-H" => vec![], // Unimplemented
                _ => text.as_bytes().to_vec(),
            }
        } else {
            string_to_bytes(encodings::STANDARD_ENCODING, text)
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

struct PageTreeIter<'a> {
    doc: &'a Document,
    stack: Vec<&'a [Object]>,
    kids: Option<&'a [Object]>,
    iter_limit: usize,
}

impl<'a> PageTreeIter<'a> {
    const PAGE_TREE_DEPTH_LIMIT: usize = 256;

    fn new(doc: &'a Document) -> Self {
        if let Ok(page_tree_id) = doc
            .catalog()
            .and_then(|cat| cat.get(b"Pages"))
            .and_then(Object::as_reference)
        {
            Self {
                doc,
                kids: Self::kids(doc, page_tree_id),
                stack: Vec::with_capacity(32),
                iter_limit: doc.objects.len(),
            }
        } else {
            Self {
                doc,
                kids: None,
                stack: Vec::new(),
                iter_limit: doc.objects.len(),
            }
        }
    }

    fn kids(doc: &Document, page_tree_id: ObjectId) -> Option<&[Object]> {
        doc.get_dictionary(page_tree_id)
            .and_then(|page_tree| page_tree.get(b"Kids"))
            .and_then(Object::as_array)
            .map(|k| k.as_slice())
            .ok()
    }
}

impl Iterator for PageTreeIter<'_> {
    type Item = ObjectId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            while let Some((kid, new_kids)) = self.kids.and_then(|k| k.split_first()) {
                if self.iter_limit == 0 {
                    return None;
                }
                self.iter_limit -= 1;

                self.kids = Some(new_kids);

                if let Ok(kid_id) = kid.as_reference() {
                    if let Ok(type_name) = self.doc.get_dictionary(kid_id).and_then(Dictionary::type_name) {
                        match type_name {
                            "Page" => {
                                return Some(kid_id);
                            }
                            "Pages" => {
                                if self.stack.len() < Self::PAGE_TREE_DEPTH_LIMIT {
                                    let kids = self.kids.unwrap();
                                    if !kids.is_empty() {
                                        self.stack.push(kids);
                                    }
                                    self.kids = Self::kids(self.doc, kid_id);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Current level exhausted, try to pop.
            if let kids @ Some(_) = self.stack.pop() {
                self.kids = kids;
            } else {
                return None;
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let kids = self.kids.unwrap_or(&[]);

        let nb_pages: usize = kids
            .iter()
            .chain(self.stack.iter().flat_map(|k| k.iter()))
            .map(|kid| {
                if let Ok(dict) = kid.as_reference().and_then(|id| self.doc.get_dictionary(id)) {
                    if let Ok("Pages") = dict.type_name() {
                        let count = dict.get_deref(b"Count", self.doc).and_then(Object::as_i64).unwrap_or(0);
                        // Don't let page count go backwards in case of an invalid document.
                        max(0, count) as usize
                    } else {
                        1
                    }
                } else {
                    1
                }
            })
            .sum();

        (nb_pages, Some(nb_pages))
    }
}

impl std::iter::FusedIterator for PageTreeIter<'_> {}
