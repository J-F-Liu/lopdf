use chrono::prelude::*;
use super::Object;

impl From<DateTime<Local>> for Object {
	fn from(date: DateTime<Local>) -> Self {
		let mut bytes = date.format("D:%Y%m%d%H%M%S%:z'").to_string().into_bytes();
		let mut index = bytes.len();
		while let Some(last) = bytes[..index].last_mut() {
			if *last == b':' {
				*last = b'\'';
				break;
			}
			index -= 1;
		}
		Object::string_literal(bytes)
	}
}

impl From<DateTime<UTC>> for Object {
	fn from(date: DateTime<UTC>) -> Self {
		Object::string_literal(date.format("D:%Y%m%d%H%M%SZ").to_string())
	}
}

impl Object {
	pub fn as_datetime(&self) -> Option<DateTime<Local>> {
		match *self {
			Object::String(ref bytes, _) => {
				let text = String::from_utf8(
					bytes.iter().filter(|b| ![b'D', b':', b'\''].contains(b)).map(|b|*b).collect()
				).unwrap();
				Local.datetime_from_str(&text, "%Y%m%d%H%M%S%z").ok()
			},
			_ => None
		}
	}
}

#[test]
fn parse_datetime() {
	let time = Local::now().with_nanosecond(0).unwrap();
	let text: Object = time.into();
	let time2 = text.as_datetime();
	assert_eq!(time2, Some(time));
}
