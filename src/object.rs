use linked_hash_map::{self, Iter, IterMut, LinkedHashMap};
use log::warn;
use std::fmt;
use std::str;

/// Object identifier consists of two parts: object number and generation number.
pub type ObjectId = (u32, u16);

/// Dictionary object.
#[derive(Clone, Default)]
pub struct Dictionary(LinkedHashMap<Vec<u8>, Object>);

/// Stream object
/// Warning - all streams must be indirect objects, while
/// the stream dictionary may be a direct object
#[derive(Debug, Clone)]
pub struct Stream {
	/// Associated stream dictionary
	pub dict: Dictionary,
	/// Contents of the stream in bytes
	pub content: Vec<u8>,
	/// Can the stream be compressed by the `Document::compress()` function?
	/// Font streams may not be compressed, for example
	pub allows_compression: bool,
	/// Stream data's position in PDF file.
	pub start_position: Option<usize>,
}

/// Basic PDF object types defined in an enum.
#[derive(Clone)]
pub enum Object {
	Null,
	Boolean(bool),
	Integer(i64),
	Real(f64),
	Name(Vec<u8>),
	String(Vec<u8>, StringFormat),
	Array(Vec<Object>),
	Dictionary(Dictionary),
	Stream(Stream),
	Reference(ObjectId),
}

/// String objects can be written in two formats.
#[derive(Debug, Clone)]
pub enum StringFormat {
	Literal,
	Hexadecimal,
}

impl Default for StringFormat {
	fn default() -> StringFormat {
		StringFormat::Literal
	}
}

impl From<bool> for Object {
	fn from(value: bool) -> Self {
		Object::Boolean(value)
	}
}

impl From<i64> for Object {
	fn from(number: i64) -> Self {
		Object::Integer(number)
	}
}

macro_rules! from_smaller_ints {
	($( $Int: ty )+) => {
		$(
			impl From<$Int> for Object {
				fn from(number: $Int) -> Self {
					Object::Integer(i64::from(number))
				}
			}
		)+
	}
}

from_smaller_ints! {
	i8 i16 i32
	u8 u16 u32
}

impl From<f64> for Object {
	fn from(number: f64) -> Self {
		Object::Real(number)
	}
}

impl From<f32> for Object {
	fn from(number: f32) -> Self {
		Object::Real(f64::from(number))
	}
}

impl From<String> for Object {
	fn from(name: String) -> Self {
		Object::Name(name.into_bytes())
	}
}

impl<'a> From<&'a str> for Object {
	fn from(name: &'a str) -> Self {
		Object::Name(name.as_bytes().to_vec())
	}
}

impl From<Vec<Object>> for Object {
	fn from(array: Vec<Object>) -> Self {
		Object::Array(array)
	}
}

impl From<Dictionary> for Object {
	fn from(dcit: Dictionary) -> Self {
		Object::Dictionary(dcit)
	}
}

impl From<Stream> for Object {
	fn from(stream: Stream) -> Self {
		Object::Stream(stream)
	}
}

impl From<ObjectId> for Object {
	fn from(id: ObjectId) -> Self {
		Object::Reference(id)
	}
}

impl Object {
	pub fn string_literal<S: Into<Vec<u8>>>(s: S) -> Self {
		Object::String(s.into(), StringFormat::Literal)
	}

	pub fn is_null(&self) -> bool {
		match *self {
			Object::Null => true,
			_ => false,
		}
	}

	pub fn as_i64(&self) -> Option<i64> {
		match *self {
			Object::Integer(ref value) => Some(*value),
			_ => None,
		}
	}

	pub fn as_f64(&self) -> Option<f64> {
		match *self {
			Object::Real(ref value) => Some(*value),
			_ => None,
		}
	}

	pub fn as_name(&self) -> Option<&[u8]> {
		match *self {
			Object::Name(ref name) => Some(name),
			_ => None,
		}
	}

	pub fn as_name_str(&self) -> Option<&str> {
		match *self {
			Object::Name(ref name) => str::from_utf8(name).ok(),
			_ => None,
		}
	}

	pub fn as_reference(&self) -> Option<ObjectId> {
		match *self {
			Object::Reference(ref id) => Some(*id),
			_ => None,
		}
	}

	pub fn as_array(&self) -> Option<&Vec<Object>> {
		match *self {
			Object::Array(ref arr) => Some(arr),
			_ => None,
		}
	}

	pub fn as_array_mut(&mut self) -> Option<&mut Vec<Object>> {
		match *self {
			Object::Array(ref mut arr) => Some(arr),
			_ => None,
		}
	}

	pub fn as_dict(&self) -> Option<&Dictionary> {
		match *self {
			Object::Dictionary(ref dict) => Some(dict),
			_ => None,
		}
	}

	pub fn as_dict_mut(&mut self) -> Option<&mut Dictionary> {
		match *self {
			Object::Dictionary(ref mut dict) => Some(dict),
			_ => None,
		}
	}

	pub fn as_stream(&self) -> Option<&Stream> {
		match *self {
			Object::Stream(ref stream) => Some(stream),
			_ => None,
		}
	}

	pub fn type_name(&self) -> Option<&str> {
		match *self {
			Object::Dictionary(ref dict) => dict.type_name(),
			Object::Stream(ref stream) => stream.dict.type_name(),
			_ => None,
		}
	}
}

impl fmt::Debug for Object {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match *self {
			Object::Null => f.write_str("null"),
			Object::Boolean(ref value) => {
				if *value {
					f.write_str("true")
				} else {
					f.write_str("false")
				}
			}
			Object::Integer(ref value) => write!(f, "{}", *value),
			Object::Real(ref value) => write!(f, "{}", *value),
			Object::Name(ref name) => write!(f, "/{}", str::from_utf8(name).unwrap()),
			Object::String(ref text, _) => write!(f, "({})", String::from_utf8_lossy(text)),
			Object::Array(ref array) => {
				let items = array.iter().map(|item| format!("{:?}", item)).collect::<Vec<String>>();
				write!(f, "[{}]", items.join(" "))
			}
			Object::Dictionary(ref dict) => write!(f, "{:?}", dict),
			Object::Stream(ref stream) => write!(f, "{:?}stream...endstream", stream.dict),
			Object::Reference(ref id) => write!(f, "{} {} R", id.0, id.1),
		}
	}
}

impl Dictionary {
	pub fn new() -> Dictionary {
		Dictionary(LinkedHashMap::new())
	}

	pub fn has(&self, key: &[u8]) -> bool {
		self.0.contains_key(key)
	}

	pub fn get(&self, key: &[u8]) -> Option<&Object> {
		self.0.get(key)
	}

	pub fn get_mut(&mut self, key: &[u8]) -> Option<&mut Object> {
		self.0.get_mut(key)
	}

	pub fn set<K, V>(&mut self, key: K, value: V)
	where
		K: Into<Vec<u8>>,
		V: Into<Object>,
	{
		self.0.insert(key.into(), value.into());
	}

	pub fn len(&self) -> usize {
		self.0.len()
	}

	pub fn is_empty(&self) -> bool {
		self.0.len() == 0
	}

	pub fn remove(&mut self, key: &[u8]) -> Option<Object> {
		self.0.remove(key)
	}

	pub fn type_name(&self) -> Option<&str> {
		self.get(b"Type").and_then(Object::as_name_str).or_else(|| self.get(b"Linearized").and(Some("Linearized")))
	}

	pub fn type_is(&self, type_name: &[u8]) -> bool {
		self.get(b"Type").and_then(Object::as_name) == Some(type_name)
	}

	pub fn iter(&self) -> Iter<'_, Vec<u8>, Object> {
		self.0.iter()
	}

	pub fn iter_mut(&mut self) -> IterMut<'_, Vec<u8>, Object> {
		self.0.iter_mut()
	}
}

#[macro_export]
macro_rules! dictionary {
	() => {
		$crate::Dictionary::new()
	};
	($( $key: expr => $value: expr ),+ ,) => {
		dictionary!( $($key => $value),+ )
	};
	($( $key: expr => $value: expr ),*) => {{
		let mut dict = $crate::Dictionary::new();
		$(
			dict.set($key, $value);
		)*
		dict
	}}
}

impl fmt::Debug for Dictionary {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let entries = self.into_iter().map(|(key, value)| format!("/{} {:?}", String::from_utf8_lossy(key), value)).collect::<Vec<String>>();
		write!(f, "<<{}>>", entries.concat())
	}
}

impl<'a> IntoIterator for &'a Dictionary {
	type Item = (&'a Vec<u8>, &'a Object);
	type IntoIter = linked_hash_map::Iter<'a, Vec<u8>, Object>;

	fn into_iter(self) -> Self::IntoIter {
		self.0.iter()
	}
}

use std::iter::FromIterator;
impl<K: Into<Vec<u8>>> FromIterator<(K, Object)> for Dictionary {
	fn from_iter<I: IntoIterator<Item = (K, Object)>>(iter: I) -> Self {
		let mut dict = Dictionary::new();
		for (k, v) in iter {
			dict.set(k, v);
		}
		dict
	}
}

impl Stream {
	pub fn new(mut dict: Dictionary, content: Vec<u8>) -> Stream {
		dict.set("Length", content.len() as i64);
		Stream {
			dict,
			content,
			allows_compression: true,
			start_position: None,
		}
	}

	pub fn with_position(dict: Dictionary, position: usize) -> Stream {
		Stream {
			dict,
			content: vec![],
			allows_compression: true,
			start_position: Some(position),
		}
	}

	/// Default is that the stream may be compressed. On font streams,
	/// set this to false, otherwise the font will be corrupt
	#[inline]
	pub fn with_compression(mut self, allows_compression: bool) -> Stream {
		self.allows_compression = allows_compression;
		self
	}

	pub fn filter(&self) -> Option<String> {
		if let Some(filter) = self.dict.get(b"Filter") {
			if let Some(filter) = filter.as_name() {
				return Some(String::from_utf8(filter.to_vec()).unwrap()); // so as to pass borrow checker
			}
		}
		None
	}

	pub fn set_content(&mut self, content: Vec<u8>) {
		self.content = content;
		self.dict.set("Length", self.content.len() as i64);
	}

	pub fn set_plain_content(&mut self, content: Vec<u8>) {
		self.dict.remove(b"DecodeParms");
		self.dict.remove(b"Filter");
		self.dict.set("Length", content.len() as i64);
		self.content = content;
	}

	pub fn compress(&mut self) {
		use flate2::write::ZlibEncoder;
		use flate2::Compression;
		use std::io::prelude::*;

		if self.dict.get(b"Filter").is_none() {
			let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
			encoder.write_all(self.content.as_slice()).unwrap();
			let compressed = encoder.finish().unwrap();
			if compressed.len() + 19 < self.content.len() {
				self.dict.set("Filter", "FlateDecode");
				self.set_content(compressed);
			}
		}
	}

	pub fn decompressed_content(&self) -> Option<Vec<u8>> {
		if let Some(filter) = self.filter() {
			if self.dict.get(b"Subtype").and_then(Object::as_name_str) == Some("Image") {
				return None;
			}

			let params = self.dict.get(b"DecodeParms").and_then(Object::as_dict);
			if filter.as_str() == "FlateDecode" {
				Some(Self::decompress_zlib(self.content.as_slice(), params))
			} else {
				None
			}
		} else {
			None
		}
	}

	fn decompress_zlib(input: &[u8], params: Option<&Dictionary>) -> Vec<u8> {
		use flate2::read::ZlibDecoder;
		use std::io::prelude::*;

		let mut output = Vec::with_capacity(input.len() * 2);
		let mut decoder = ZlibDecoder::new(input);

		if !input.is_empty() {
			decoder.read_to_end(&mut output).unwrap_or_else(|err| {
				warn!("{}", err);
				0
			});
		}
		Self::decompress_predictor(output, params)
	}

	fn decompress_predictor(mut data: Vec<u8>, params: Option<&Dictionary>) -> Vec<u8> {
		use crate::filters::png;

		if let Some(params) = params {
			let predictor = params.get(b"Predictor").and_then(Object::as_i64).unwrap_or(1);
			if predictor >= 10 && predictor <= 15 {
				let pixels_per_row = params.get(b"Columns").and_then(Object::as_i64).unwrap_or(1) as usize;
				let colors = params.get(b"Colors").and_then(Object::as_i64).unwrap_or(1) as usize;
				let bits = params.get(b"BitsPerComponent").and_then(Object::as_i64).unwrap_or(8) as usize;
				let bytes_per_pixel = colors * bits / 8;
				data = png::decode_frame(data.as_slice(), bytes_per_pixel, pixels_per_row).unwrap();
			}
			data
		} else {
			data
		}
	}

	pub fn decompress(&mut self) {
		if let Some(data) = self.decompressed_content() {
			self.dict.remove(b"DecodeParms");
			self.dict.remove(b"Filter");
			self.set_content(data);
		}
	}
}
