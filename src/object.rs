use std::collections::BTreeMap;

/// Object identifier consists of two parts: object number and generation number.
pub type ObjectId = (u32, u16);

/// Dictionary object.
#[derive(Debug)]
pub struct Dictionary(BTreeMap<String, Object>);

/// Stream Object.
#[derive(Debug)]
pub struct Stream {
	pub dict: Dictionary,
	pub content: Vec<u8>,
}

/// Basic PDF object types defined in an enum.
#[derive(Debug)]
pub enum Object {
	Null,
	Boolean(bool),
	Integer(i64),
	Real(f64),
	Name(String),
	String(Vec<u8>, StringFormat),
	Array(Vec<Object>),
	Dictionary(Dictionary),
	Stream(Stream),
	Reference(ObjectId),
}

/// String objects can be written in two formats.
#[derive(Debug)]
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

impl From<f64> for Object {
	fn from(number: f64) -> Self {
		Object::Real(number)
	}
}

impl From<String> for Object {
	fn from(name: String) -> Self {
		Object::Name(name)
	}
}

impl<'a> From<&'a str> for Object {
	fn from(name: &'a str) -> Self {
		Object::Name(name.to_owned())
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

impl Object {
	pub fn is_null(&self) -> bool {
		match *self {
			Object::Null => true,
			_ => false
		}
	}

	pub fn as_i64(&self) -> Option<i64> {
		match *self {
			Object::Integer(ref value) => Some(*value),
			_ => None
		}
	}

	pub fn as_name(&self) -> Option<&str> {
		match *self {
			Object::Name(ref name) => Some(name),
			_ => None
		}
	}

	pub fn as_reference(&self) -> Option<ObjectId> {
		match *self {
			Object::Reference(ref id) => Some(*id),
			_ => None
		}
	}

	pub fn as_array(&self) -> Option<&Vec<Object>> {
		match *self {
			Object::Array(ref arr) => Some(arr),
			_ => None
		}
	}
}

impl Dictionary {
	pub fn new() -> Dictionary {
		Dictionary(BTreeMap::new())
	}

	pub fn get<K>(&self, key: K) -> Option<&Object>
		where K: Into<String>
	{
		self.0.get(&key.into())
	}

	pub fn set<K, V>(&mut self, key: K, value: V)
		where K: Into<String>,
		      V: Into<Object>
	{
		self.0.insert(key.into(), value.into());
	}

	pub fn len(&self) -> usize {
		self.0.len()
	}

	pub fn remove<K>(&mut self, key: K) -> Option<Object>
		where K: Into<String>
	{
		self.0.remove(&key.into())
	}
}

impl<'a> IntoIterator for &'a Dictionary {
	type Item = (&'a String, &'a Object);
	type IntoIter = ::std::collections::btree_map::Iter<'a, String, Object>;

	fn into_iter(self) -> Self::IntoIter {
		self.0.iter()
	}
}

impl Stream {
	pub fn new(mut dict: Dictionary, content: Vec<u8>) -> Stream {
		dict.set("Length", content.len() as i64);
		Stream {
			dict: dict,
			content: content,
		}
	}

	pub fn filter(&self) -> Option<String> {
		if let Some(filter) = self.dict.get("Filter") {
			if let Some(filter) = filter.as_name() {
				return Some(filter.to_owned()); // so as to pass borrow checker
			}
		}
		return None;
	}

	pub fn compress(&mut self) {
		use std::io::prelude::*;
		use flate2::Compression;
		use flate2::write::ZlibEncoder;

		if self.dict.get("Filter").is_none() {
			let mut encoder = ZlibEncoder::new(Vec::new(), Compression::Best);
			encoder.write(self.content.as_slice()).unwrap();
			let compressed = encoder.finish().unwrap();
			if compressed.len() + 19 < self.content.len() {
				self.content = compressed;
				self.dict.set("Filter", "FlateDecode");
				self.dict.set("Length", self.content.len() as i64);
			}
		}
	}

	pub fn decompress(&mut self) {
		use std::io::prelude::*;
		use flate2::read::ZlibDecoder;

		if let Some(filter) = self.filter() {
			match filter.as_str() {
				"FlateDecode" => {
					let mut data = Vec::new();
					{
						let mut decoder = ZlibDecoder::new(self.content.as_slice());
						decoder.read_to_end(&mut data).unwrap();
					}
					self.content = data;
					self.dict.remove("Filter");
					self.dict.set("Length", self.content.len() as i64);
				},
				_ => ()
			}
		}
	}
}
