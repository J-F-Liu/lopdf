use super::content::Content;
use super::encodings::{self, bytes_to_string, string_to_bytes};
use super::{Dictionary, Object, ObjectId};
use crate::xref::Xref;
use crate::{Error, Result};
use encoding::all::UTF_16BE;
use encoding::types::{DecoderTrap, EncoderTrap, Encoding};
use log::info;
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

	/// Get object by object id, will recursively dereference a referenced object.
	pub fn get_object(&self, id: ObjectId) -> Result<&Object> {
		if let Some(object) = self.objects.get(&id) {
			if let Ok(id) = object.as_reference() {
				return self.get_object(id);
			} else {
				return Ok(object);
			}
		}
		Err(Error::ObjectNotFound)
	}

	/// Get mutable reference to object by object id, will recursively dereference a referenced object.
	pub fn get_object_mut(&mut self, id: ObjectId) -> Result<&mut Object> {
		unsafe {
			let s = self as *mut Self;
			if let Some(object) = (*s).objects.get_mut(&id) {
				if let Ok(id) = object.as_reference() {
					return (*s).get_object_mut(id);
				} else {
					return Ok(object);
				}
			}
			Err(Error::ObjectNotFound)
		}
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
		self.trailer.get(b"Root").and_then(Object::as_reference).and_then(|id| self.get_dictionary(id))
	}

	/// Get page numbers and corresponding object ids.
	pub fn get_pages(&self) -> BTreeMap<u32, ObjectId> {
		fn collect_pages(doc: &Document, page_tree_id: ObjectId, page_number: &mut u32, pages: &mut BTreeMap<u32, ObjectId>) {
			if let Ok(kids) = doc.get_dictionary(page_tree_id).and_then(|page_tree| page_tree.get(b"Kids")).and_then(Object::as_array) {
				for kid in kids {
					if let Ok(kid_id) = kid.as_reference() {
						if let Ok(type_name) = doc.get_dictionary(kid_id).and_then(Dictionary::type_name) {
							match type_name {
								"Page" => {
									pages.insert(*page_number, kid_id);
									*page_number += 1;
								}
								"Pages" => {
									collect_pages(doc, kid_id, page_number, pages);
								}
								_ => {}
							}
						}
					}
				}
			}
		}

		let mut pages = BTreeMap::new();
		let mut page_number = 1;
		if let Ok(page_tree_id) = self.catalog().and_then(|cat| cat.get(b"Pages")).and_then(Object::as_reference) {
			collect_pages(self, page_tree_id, &mut page_number, &mut pages);
		}
		pages
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
							if let Ok(id) = content.as_reference() { streams.push(id) }
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

	/// Get decoded page content;
	pub fn get_and_decode_page_content(&self, page_id: ObjectId) -> Result<Content> {
		let content_data = self.get_page_content(page_id)?;
		Content::decode(&content_data)
	}

	/// Get resources used by a page.
	pub fn get_page_resources(&self, page_id: ObjectId) -> (Option<&Dictionary>, Vec<ObjectId>) {
		fn collect_resources(page_node: &Dictionary, resource_ids: &mut Vec<ObjectId>, doc: &Document) {
			if let Ok(resources_id) = page_node.get(b"Resources").and_then(Object::as_reference) {
				resource_ids.push(resources_id);
			}
			if let Ok(page_tree) = page_node.get(b"Parent").and_then(Object::as_reference).and_then(|id| doc.get_dictionary(id)) {
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
		fn collect_fonts_from_resources<'a>(resources: &'a Dictionary, fonts: &mut BTreeMap<Vec<u8>, &'a Dictionary>, doc: &'a Document) {
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

	pub fn get_font_encoding<'a>(&self, font: &'a Dictionary) -> &'a str {
		font.get(b"Encoding")
			.and_then(Object::as_name_str)
			.unwrap_or("StandardEncoding")
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
