use super::encodings::{self, bytes_to_string, string_to_bytes};
use super::{Bookmark, Dictionary, Object, ObjectId};
use crate::encryption;
use crate::xref::{Xref, XrefType};
use crate::{Error, Result, Stream};
use encoding_rs::UTF_16BE;
use log::info;
use std::cmp::max;
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::str;

/// A PDF document.
///
/// This can both be a combination of multiple incremental updates
/// or just one (the last) incremental update in a PDF file.
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

    /// Current maximum object id within Bookmarks.
    pub max_bookmark_id: u32,

    /// The bookmarks in the document. Render at the very end of document after renumbering objects.
    pub bookmarks: Vec<u32>,

    /// used to locate a stored Bookmark so children can be appended to it via its id. Otherwise we
    /// need to do recursive lookups and returns on the bookmarks internal layout Vec
    pub bookmark_table: HashMap<u32, Bookmark>,

    /// The byte the cross-reference table starts at.
    /// This value is only set during reading, but not when writing the file.
    /// It is used to support incremental updates in PDFs.
    /// Default value is `0`.
    pub xref_start: usize,
}

impl Document {
    /// Create new PDF document.
    pub fn new() -> Self {
        Self {
            version: "1.4".to_string(),
            trailer: Dictionary::new(),
            reference_table: Xref::new(0, XrefType::CrossReferenceStream),
            objects: BTreeMap::new(),
            max_id: 0,
            max_bookmark_id: 0,
            bookmarks: Vec::new(),
            bookmark_table: HashMap::new(),
            xref_start: 0,
        }
    }

    /// Create a new PDF document that is an incremental update to a previous document.
    pub fn new_from_prev(prev: &Document) -> Self {
        let mut new_trailer = prev.trailer.clone();
        new_trailer.set("Prev", Object::Integer(prev.xref_start as i64));
        Self {
            version: "1.4".to_string(),
            trailer: new_trailer,
            reference_table: Xref::new(0, prev.reference_table.cross_reference_type),
            objects: BTreeMap::new(),
            max_id: prev.max_id,
            max_bookmark_id: prev.max_bookmark_id,
            bookmarks: Vec::new(),
            bookmark_table: HashMap::new(),
            xref_start: 0,
        }
    }

    const DEREF_LIMIT: usize = 128;

    fn recursive_fix_pages(&mut self, bookmarks: &[u32], first: bool) -> ObjectId {
        if !bookmarks.is_empty() {
            for id in bookmarks {
                let (children, mut page) = match self.bookmark_table.get(id) {
                    Some(n) => (n.children.clone(), n.page),
                    None => return (0, 0),
                };

                if 0 == page.0 && !children.is_empty() {
                    let objectid = self.recursive_fix_pages(&children[..], false);

                    let bookmark = self.bookmark_table.get_mut(id).unwrap();
                    bookmark.page = objectid;
                    page = objectid;
                }

                if !first && 0 != page.0 {
                    return page;
                }

                if first && !children.is_empty() {
                    self.recursive_fix_pages(&children[..], first);
                }
            }
        }

        (0, 0)
    }

    /// Adjusts the Parents that have a ObjectId of (0,_) to that
    /// of their first child. will recurse through all entries
    /// till all parents of children are set. This should be
    /// ran before building the final bookmark objects but after
    /// renumbering of objects.
    pub fn adjust_zero_pages(&mut self) {
        self.recursive_fix_pages(&self.bookmarks.clone(), true);
    }

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

    /// Determines if an object exists in the current document (or incremental update.)
    /// with the given `ObjectId`.
    /// `true` if the object exists, `false` if it does not exist.
    pub fn has_object(&self, id: ObjectId) -> bool {
        self.objects.get(&id).is_some()
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
            let mut objects_ids = annots.iter().map(Object::as_reference);

            let contains = objects_ids.any(|object_id| {
                if let Ok(object_id) = object_id {
                    return id == object_id;
                }
                false
            });
            if contains {
                return Ok(object_id);
            }
        }

        Err(Error::ObjectNotFound)
    }

    /// Get dictionary object by id.
    pub fn get_dictionary(&self, id: ObjectId) -> Result<&Dictionary> {
        self.get_object(id).and_then(Object::as_dict)
    }

    /// Get a mutable dictionary object by id.
    pub fn get_dictionary_mut(&mut self, id: ObjectId) -> Result<&mut Dictionary> {
        self.get_object_mut(id).and_then(Object::as_dict_mut)
    }

    /// Get dictionary in dictionary by key.
    pub fn get_dict_in_dict(&self, node: &Dictionary, key: &[u8]) -> Result<&Dictionary> {
        node.get(key)
            .and_then(Object::as_reference)
            .and_then(move |id| self.get_dictionary(id))
    }

    /// Traverse objects from trailer recursively, return all referenced object IDs.
    pub fn traverse_objects<A: Fn(&mut Object)>(&mut self, action: A) -> Vec<ObjectId> {
        fn traverse_array<A: Fn(&mut Object)>(array: &mut [Object], action: &A, refs: &mut Vec<ObjectId>) {
            for item in array.iter_mut() {
                traverse_object(item, action, refs);
            }
        }
        fn traverse_dictionary<A: Fn(&mut Object)>(dict: &mut Dictionary, action: &A, refs: &mut Vec<ObjectId>) {
            for (_, v) in dict.iter_mut() {
                traverse_object(v, action, refs);
            }
        }
        fn traverse_object<A: Fn(&mut Object)>(object: &mut Object, action: &A, refs: &mut Vec<ObjectId>) {
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

    /// Return dictionary with encryption information
    pub fn get_encrypted(&self) -> Result<&Dictionary> {
        self.trailer
            .get(b"Encrypt")
            .and_then(Object::as_reference)
            .and_then(|id| self.get_dictionary(id))
    }

    /// Return true is PDF document is encrypted
    pub fn is_encrypted(&self) -> bool {
        self.get_encrypted().is_ok()
    }

    /// Replaces all encrypted Strings and Streams with their decrypted contents
    pub fn decrypt<P: AsRef<[u8]>>(&mut self, password: P) -> Result<()> {
        // Find the ID of the encryption dict; we'll want to skip it when decrypting
        let encryption_obj_id = self.trailer.get(b"Encrypt").and_then(Object::as_reference)?;

        // Since PDF 1.5, metadata may or may not be encrypted; defaults to true
        let metadata_is_encrypted = self
            .get_object(encryption_obj_id)?
            .as_dict()?
            .get(b"EncryptMetadata")
            .and_then(|o| o.as_bool())
            .unwrap_or(true);

        let key = encryption::get_encryption_key(self, &password, true)?;
        for (&id, obj) in self.objects.iter_mut() {
            // The encryption dictionary is not encrypted, leave it alone
            if id == encryption_obj_id {
                continue;
            }

            // If a Metadata stream but metadata isn't encrypted, leave it alone
            if obj.type_name().unwrap_or("") == "Metadata" && !metadata_is_encrypted {
                continue;
            }

            let decrypted = match encryption::decrypt_object(&key, id, &*obj) {
                Ok(content) => content,
                Err(encryption::DecryptionError::NotDecryptable) => {
                    continue;
                }
                Err(_err) => {
                    return Err(_err.into());
                }
            };

            // Only strings and streams are encrypted
            match obj {
                Object::Stream(stream) => stream.set_content(decrypted),
                Object::String(ref mut content, _) => *content = decrypted,
                _ => {}
            }
        }

        if let Ok(info_obj_id) = self.trailer.get(b"Info").and_then(Object::as_reference) {
            if let Ok(info_dict) = self.get_object_mut(info_obj_id).and_then(Object::as_dict_mut) {
                for (_, info_obj) in info_dict.iter_mut() {
                    if let Ok(content) = encryption::decrypt_object(&key, info_obj_id, &*info_obj) {
                        info_obj.as_str_mut().unwrap().clear();
                        info_obj.as_str_mut().unwrap().extend(content);
                    };
                }
            }
        }

        self.trailer.remove(b"Encrypt");
        Ok(())
    }

    /// Return the PDF document catalog, which is the root of the document's object graph.
    pub fn catalog(&self) -> Result<&Dictionary> {
        self.trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .and_then(|id| self.get_dictionary(id))
    }

    /// Return a mutable reference to the PDF document catalog, which is the root of the document's
    /// object graph.
    pub fn catalog_mut(&mut self) -> Result<&mut Dictionary> {
        self.trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .and_then(move |id| self.get_dictionary_mut(id))
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
            let mut nb_deref = 0;
            // Since we're looking for object ids, we can't use get_deref
            // so manually walk any references in contents object
            if let Ok(mut contents) = page.get(b"Contents") {
                loop {
                    match *contents {
                        Object::Reference(id) => match self.objects.get(&id) {
                            None | Some(Object::Stream(_)) => {
                                streams.push(id);
                            }
                            Some(o) => {
                                nb_deref += 1;
                                if nb_deref < Self::DEREF_LIMIT {
                                    contents = o;
                                    continue;
                                }
                            }
                        },
                        Object::Array(ref arr) => {
                            for content in arr {
                                if let Ok(id) = content.as_reference() {
                                    streams.push(id)
                                }
                            }
                        }
                        _ => {}
                    }
                    break;
                }
            }
        }
        streams
    }

    /// Add content to a page. All existing content will be unchanged.
    pub fn add_page_contents(&mut self, page_id: ObjectId, content: Vec<u8>) -> Result<()> {
        if let Ok(page) = self.get_dictionary(page_id) {
            // Prepare new value
            let mut current_content_list: Vec<Object> = match page.get(b"Contents") {
                Ok(Object::Reference(ref id)) => {
                    // Covert reference to array
                    vec![Object::Reference(*id)]
                }
                Ok(Object::Array(ref arr)) => arr.clone(),
                Err(Error::DictKey) => vec![],
                _ => vec![],
            };
            let content_object_id = self.add_object(Object::Stream(Stream::new(Dictionary::new(), content)));
            current_content_list.push(Object::Reference(content_object_id));
            // Set data

            let page_mut = self.get_object_mut(page_id).and_then(Object::as_dict_mut).unwrap();
            page_mut.set("Contents", current_content_list);
            Ok(())
        } else {
            Err(Error::ObjectNotFound)
        }
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
        }

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
            if let Ok(font) = resources.get(b"Font") {
                let font_dict = match font {
                    Object::Reference(ref id) => {
                        doc.get_object(*id).and_then(Object::as_dict).ok()
                    },
                    Object::Dictionary(ref dict) => {
                        Some(dict)
                    },
                    _ => {
                        None
                    }
                };
                if let Some(font_dict) = font_dict {
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
            }
        }

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

    /// Get the PDF annotations of a page. The /Subtype of each annotation dictionary defines the
    /// annotation type (Text, Link, Highlight, Underline, Ink, Popup, Widget, etc.). The /Rect of
    /// an annotation dictionary defines its location on the page.
    pub fn get_page_annotations(&self, page_id: ObjectId) -> Vec<&Dictionary> {
        let mut annotations = vec![];
        if let Ok(page) = self.get_dictionary(page_id) {
            match page.get(b"Annots") {
                Ok(Object::Reference(ref id)) => self
                    .get_object(*id)
                    .and_then(Object::as_array)
                    .unwrap()
                    .iter()
                    .flat_map(Object::as_reference)
                    .flat_map(|id| self.get_dictionary(id))
                    .for_each(|a| annotations.push(a)),
                Ok(Object::Array(ref a)) => a
                    .iter()
                    .flat_map(Object::as_reference)
                    .flat_map(|id| self.get_dictionary(id))
                    .for_each(|a| annotations.push(a)),
                _ => {}
            }
        }
        annotations
    }

    pub fn decode_text(encoding: Option<&str>, bytes: &[u8]) -> String {
        if let Some(encoding) = encoding {
            info!("{}", encoding);
            match encoding {
                "StandardEncoding" => bytes_to_string(encodings::STANDARD_ENCODING, bytes),
                "MacRomanEncoding" => bytes_to_string(encodings::MAC_ROMAN_ENCODING, bytes),
                "MacExpertEncoding" => bytes_to_string(encodings::MAC_EXPERT_ENCODING, bytes),
                "WinAnsiEncoding" => bytes_to_string(encodings::WIN_ANSI_ENCODING, bytes),
                "UniGB-UCS2-H" | "UniGB−UTF16−H" => UTF_16BE.decode(bytes).0.to_string(),
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
                "UniGB-UCS2-H" | "UniGB−UTF16−H" => UTF_16BE.encode(text).0.to_vec(),
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
