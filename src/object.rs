use std::collections::BTreeMap;

pub type ObjectId = (u32, u16);

pub type Dictionary = BTreeMap<String, Object>;

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

impl Stream {
	pub fn new(content: Vec<u8>) -> Stream {
		let mut dict = Dictionary::new();
		dict.insert("Length".to_string(), Object::Integer(content.len() as i64));
		Stream {
			dict: dict,
			content: content,
		}
	}
}
