use chrono::prelude::*;
use super::{Object, StringFormat};

impl From<DateTime<Local>> for Object {
	fn from(date: DateTime<Local>) -> Self {
		let mut text = date.format("%Y%m%d%H%M%S%:z'").to_string().replace(':', "'");
		text.insert_str(0, "D:");
		Object::String(text.into_bytes(), StringFormat::Literal)
	}
}

impl From<DateTime<UTC>> for Object {
	fn from(date: DateTime<UTC>) -> Self {
		Object::String(date.format("D:%Y%m%d%H%M%SZ").to_string().into_bytes(), StringFormat::Literal)
	}
}
