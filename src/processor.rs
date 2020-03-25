use crate::content::Content;
use crate::{Document, Object, ObjectId};
use crate::{Error, Result};
use log::info;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;

impl Document {
	/// Change producer of document information dictionary.
	pub fn change_producer(&mut self, producer: &str) {
		if let Ok(info) = self.trailer.get_mut(b"Info") {
			if let Some(dict) = match *info {
				Object::Dictionary(ref mut dict) => Some(dict),
				Object::Reference(ref id) => self.objects.get_mut(id).and_then(|o| o.as_dict_mut().ok()),
				_ => None,
			} {
				dict.set("Producer", Object::string_literal(producer));
			}
		}
	}

	/// Compress PDF stream objects.
	pub fn compress(&mut self) {
		for object in self.objects.values_mut() {
			if let Object::Stream(ref mut stream) = *object {
				if stream.allows_compression {
					// Ignore any error and continue to compress other streams.
					let _ = stream.compress();
				}
			}
		}
	}

	/// Decompress PDF stream objects.
	pub fn decompress(&mut self) {
		for object in self.objects.values_mut() {
			if let Object::Stream(ref mut stream) = *object {
				stream.decompress()
			}
		}
	}

	/// Delete pages.
	pub fn delete_pages(&mut self, page_numbers: &[u32]) {
		let pages = self.get_pages();
		for page_number in page_numbers {
			if let Some(page) = pages.get(&page_number).and_then(|page_id| self.delete_object(*page_id)) {
				let mut page_tree_ref = page.as_dict().and_then(|dict| dict.get(b"Parent")).and_then(Object::as_reference);
				while let Ok(page_tree_id) = page_tree_ref {
					if let Some(page_tree) = self.objects.get_mut(&page_tree_id).and_then(|pt| pt.as_dict_mut().ok()) {
						if let Ok(count) = page_tree.get(b"Count").and_then(Object::as_i64) {
							page_tree.set("Count", count - 1);
						}
						page_tree_ref = page_tree.get(b"Parent").and_then(Object::as_reference);
					} else {
						break;
					}
				}
			}
		}
	}

	/// Prune all unused objects.
	pub fn prune_objects(&mut self) -> Vec<ObjectId> {
		let mut ids = vec![];
		let refs = self.traverse_objects(|_| {});
		for id in self.objects.keys() {
			if !refs.contains(id) {
				ids.push(*id);
			}
		}

		for id in &ids {
			self.objects.remove(id);
		}

		ids
	}

	/// Delete object by object ID.
	pub fn delete_object(&mut self, id: ObjectId) -> Option<Object> {
		let action = |object: &mut Object| match *object {
			Object::Array(ref mut array) => {
				if let Some(index) = array.iter().position(|item: &Object| match *item {
					Object::Reference(ref_id) => ref_id == id,
					_ => false,
				}) {
					array.remove(index);
				}
			}
			Object::Dictionary(ref mut dict) => {
				let keys: Vec<Vec<u8>> = dict
					.iter()
					.filter(|&(_, item): &(&Vec<u8>, &Object)| match *item {
						Object::Reference(ref_id) => ref_id == id,
						_ => false,
					})
					.map(|(k, _)| k.clone())
					.collect();
				for key in keys {
					dict.remove(&key);
				}
			}
			_ => {}
		};
		self.traverse_objects(action);
		self.objects.remove(&id)
	}

	/// Delete zero length stream objects.
	pub fn delete_zero_length_streams(&mut self) -> Vec<ObjectId> {
		let mut ids = vec![];
		for id in self.objects.keys() {
			if self.objects.get(id).and_then(|o| Object::as_stream(o).ok()).map(|stream| stream.content.is_empty()).unwrap_or(false) {
				ids.push(*id);
			}
		}

		for id in &ids {
			self.delete_object(*id);
		}

		ids
	}

	/// Renumber objects, normally called after delete_unused_objects.
	pub fn renumber_objects(&mut self) {
		let mut replace = BTreeMap::new();
		let mut new_id = 1;
		let mut ids = self.objects.keys().cloned().collect::<Vec<ObjectId>>();
		ids.sort();

		for id in ids {
			if id.0 != new_id {
				replace.insert(id, (new_id, id.1));
			}
			new_id += 1;
		}

		// replace order is from small to big
		for (old, new) in &replace {
			if let Some(object) = self.objects.remove(old) {
				self.objects.insert(new.clone(), object);
			}
		}

		let action = |object: &mut Object| if let Object::Reference(ref mut id) = *object {
			if replace.contains_key(&id) {
				*id = replace[id];
			}
		};

		self.traverse_objects(action);
		self.max_id = new_id - 1;
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
			let encodings = fonts.into_iter().map(|(name, font)| (name, font.get_font_encoding())).collect::<BTreeMap<Vec<u8>, &str>>();
			let content_data = self.get_page_content(page_id)?;
			let content = Content::decode(&content_data)?;
			let mut current_encoding = None;
			for operation in &content.operations {
				match operation.operator.as_ref() {
					"Tf" => {
						let current_font =
							operation.operands.get(0)
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

	pub fn change_content_stream(&mut self, stream_id: ObjectId, content: Vec<u8>) {
		if let Some(content_stream) = self.objects.get_mut(&stream_id) {
			if let Object::Stream(ref mut stream) = *content_stream {
				stream.set_plain_content(content);
				// Ignore any compression error.
				let _ = stream.compress();
			}
		}
	}

	pub fn change_page_content(&mut self, page_id: ObjectId, content: Vec<u8>) -> Result<()> {
		let contents = self.get_dictionary(page_id).and_then(|page| page.get(b"Contents"))?;
		match *contents {
			Object::Reference(id) => self.change_content_stream(id, content),
			Object::Array(ref arr) => {
				if arr.len() == 1 {
					if let Ok(id) = arr[0].as_reference() { self.change_content_stream(id, content) }
				} else {
					let new_stream = self.add_object(super::Stream::new(dictionary!{}, content));
					if let Ok(page) = self.get_object_mut(page_id) {
						if let Object::Dictionary(ref mut dict) = *page {
							dict.set("Contents", new_stream);
						}
					}
				}
			}
			_ => {}
		}
		Ok(())
	}

	pub fn replace_text(&mut self, page_number: u32, text: &str, other_text: &str) -> Result<()>{
		let page_id = self.page_iter().nth(page_number as usize - 1).ok_or(Error::PageNumberNotFound(page_number))?;
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
					let current_font =
						operation.operands.get(0)
						.ok_or(Error::Syntax("missing font operand".to_string()))?
						.as_name()?;
					current_encoding = encodings.get(current_font).map(std::string::String::as_str);
				}
				"Tj" => {
					for operand in &mut operation.operands {
						if let Object::String(ref mut bytes, _) = *operand {
							let decoded_text = Document::decode_text(current_encoding, bytes);
							info!("{}", decoded_text);
							if decoded_text == text {
								let encoded_bytes = Document::encode_text(current_encoding, other_text);
								*bytes = encoded_bytes;
							}
						}
					}
				}
				_ => {}
			}
		}
		let modified_content = content.encode()?;
		self.change_page_content(page_id, modified_content)
	}

	pub fn extract_stream(&self, stream_id: ObjectId, decompress: bool) -> Result<()> {
		let mut file = File::create(format!("{:?}.bin", stream_id))?;
		if let Ok(stream_obj) = self.get_object(stream_id) {
			if let Object::Stream(ref stream) = *stream_obj {
				if decompress {
					if let Ok(data) = stream.decompressed_content() {
						file.write_all(&data)?;
					} else {
						file.write_all(&stream.content)?;
					}
				} else {
					file.write_all(&stream.content)?;
				}
			}
		}
		Ok(())
	}
}
