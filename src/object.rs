use std::collections::BTreeMap;

pub type ObjectId = (u32, u16);

pub struct Dictionary(BTreeMap<String, Object>);

/// Stream Object
pub struct Stream {
	pub dict: Dictionary,
	pub content: Vec<u8>,
}

///  basic types of PDF objects
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

/// String objects can be written in two ways
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

impl Dictionary {
	pub fn new() -> Dictionary {
		Dictionary(BTreeMap::new())
	}

	pub fn set<K, V>(&mut self, key: K, value: V)
		where K: Into<String>,
		      V: Into<Object>
	{
		self.0.insert(key.into(), value.into());
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
	pub fn new(content: Vec<u8>) -> Stream {
		let mut dict = Dictionary::new();
		dict.set("Length", content.len() as i64);
		Stream {
			dict: dict,
			content: content,
		}
	}
}
