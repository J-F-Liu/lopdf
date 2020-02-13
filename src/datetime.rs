use super::Object;
#[cfg(feature = "chrono_time")]
use chrono::prelude::*;
#[cfg(not(feature = "chrono_time"))]
use time::strptime;
use time::{strftime, Tm};

#[cfg(feature = "chrono_time")]
impl From<DateTime<Local>> for Object {
	fn from(date: DateTime<Local>) -> Self {
		let mut timezone_str = date.format("D:%Y%m%d%H%M%S%:z'").to_string().into_bytes();
		convert_utc_offset(&mut timezone_str);
		Object::string_literal(timezone_str)
	}
}

// Find the last `:` and turn it into an `'` to account for PDF weirdness
fn convert_utc_offset(bytes: &mut [u8]) {
	let mut index = bytes.len();
	while let Some(last) = bytes[..index].last_mut() {
		if *last == b':' {
			*last = b'\'';
			break;
		}
		index -= 1;
	}
}

#[cfg(feature = "chrono_time")]
impl From<DateTime<Utc>> for Object {
	fn from(date: DateTime<Utc>) -> Self {
		Object::string_literal(date.format("D:%Y%m%d%H%M%SZ").to_string())
	}
}

impl From<Tm> for Object {
	fn from(date: Tm) -> Self {
		// can only fail if the TIME_FMT_ENCODE_STR would be invalid
		Object::string_literal(if date.tm_utcoff != 0 {
			// D:%Y%m%d%H%M%S:%z'
			//
			// UTC offset in the form +HHMM or -HHMM (empty string if the the object is naive).
			let timezone = strftime("%z", &date).unwrap();
			let timezone_str_start = strftime("%Y%m%d%H%M%S", &date).unwrap();
			let mut timezone_str = format!("D:{}{}:{}'", timezone_str_start, &timezone[..3], &timezone[3..]).into_bytes();
			convert_utc_offset(&mut timezone_str);
			timezone_str
		} else {
			format!("D:{}", strftime("%Y%m%d%H%M%SZ", &date).unwrap()).into_bytes()
		})
	}
}

impl Object {
	// Parses the `D`, `:` and `\` out of a `Object::String` to parse the date time
	fn datetime_string(&self) -> Option<String> {
		if let Object::String(ref bytes, _) = self {
			String::from_utf8(bytes.iter().filter(|b| ![b'D', b':', b'\''].contains(b)).cloned().collect()).ok()
		} else {
			None
		}
	}

	#[cfg(feature = "chrono_time")]
	pub fn as_datetime(&self) -> Option<DateTime<Local>> {
	#[cfg(feature = "chrono_time")]
		const TIME_FMT_DECODE_STR: &str = "%Y%m%d%H%M%S%#z";
		let text = self.datetime_string()?;
		DateTime::parse_from_str(&text, TIME_FMT_DECODE_STR).map(|date| date.with_timezone(&Local)).ok()
	}

	/// WARNING: `tm_wday` (weekday), `tm_yday` (day index in year), `tm_isdst`
	/// (daylight saving time) and `tm_nsec` (nanoseconds of the date from 1970)
	/// are set to 0 since they aren't available in the PDF time format. They could,
	/// however, be calculated manually
	#[cfg(not(feature = "chrono_time"))]
	pub fn as_datetime(&self) -> Option<Tm> {
		const TIME_FMT_DECODE_STR: &str = "%Y%m%d%H%M%S%z";
		let text = self.datetime_string()?;
		strptime(&text, TIME_FMT_DECODE_STR).ok()
	}
}

#[cfg(feature = "chrono_time")]
#[test]
fn parse_datetime_local() {
	let time = Local::now().with_nanosecond(0).unwrap();
	let text: Object = time.into();
	let time2 = text.as_datetime();
	assert_eq!(time2, Some(time));
}

#[cfg(feature = "chrono_time")]
#[test]
fn parse_datetime_utc() {
	let time = Utc::now().with_nanosecond(0).unwrap();
	let text: Object = time.into();
	let time2 = text.as_datetime();
	assert_eq!(time2, Some(time.with_timezone(&Local)));
}

#[cfg(not(feature = "chrono_time"))]
#[test]
fn parse_datetime() {
	// Tm-based: Ignore tm_wday, tm_yday, tm_isdst and tm_nsec
	// - not important in the date parsing
	let time = Tm {
		tm_wday: 0,
		tm_yday: 0,
		tm_isdst: 0,
		tm_nsec: 0,
		..::time::now()
	};

	let text: Object = time.into();
	let time2 = text.as_datetime();
	assert_eq!(time2, Some(time));
}
