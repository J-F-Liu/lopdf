use super::Object;
#[cfg(feature = "chrono_time")]
use chrono::prelude::*;

use time::OffsetDateTime;
use time::Time;

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

impl From<Time> for Object {
    fn from(date: Time) -> Self {
        // can only fail if the TIME_FMT_ENCODE_STR would be invalid
        Object::string_literal(format!("D:{}", date.format("%Y%m%d%H%M%SZ")).into_bytes())
    }
}

impl From<OffsetDateTime> for Object {
    fn from(date: OffsetDateTime) -> Self {
        // can only fail if the TIME_FMT_ENCODE_STR would be invalid
        Object::string_literal({
            // D:%Y%m%d%H%M%S:%z'
            //
            // UTC offset in the form +HHMM or -HHMM (empty string if the the object is naive).
            let timezone = date.format("%z");
            let timezone_str_start = date.format("%Y%m%d%H%M%S");
            let mut timezone_str =
                format!("D:{}{}:{}'", timezone_str_start, &timezone[..3], &timezone[3..]).into_bytes();
            convert_utc_offset(&mut timezone_str);
            timezone_str
        })
    }
}

impl Object {
    // Parses the `D`, `:` and `\` out of a `Object::String` to parse the date time
    fn datetime_string(&self) -> Option<String> {
        if let Object::String(ref bytes, _) = self {
            String::from_utf8(
                bytes
                    .iter()
                    .filter(|b| ![b'D', b':', b'\''].contains(b))
                    .cloned()
                    .collect(),
            )
            .ok()
        } else {
            None
        }
    }

    #[cfg(feature = "chrono_time")]
    pub fn as_datetime(&self) -> Option<DateTime<Local>> {
        let text = self.datetime_string()?;
        let from_date = |date| FixedOffset::east(0).from_utc_date(&date).and_hms(0, 0, 0);
        DateTime::parse_from_str(&text, "%Y%m%d%H%M%S%#z")
            .or_else(|_| DateTime::parse_from_str(&text, "%Y%m%d%H%M%#z"))
            .or_else(|_| NaiveDate::parse_from_str(&text, "%Y%m%d").map(from_date))
            .map(|date| date.with_timezone(&Local))
            .ok()
    }

    /// WARNING: `tm_wday` (weekday), `tm_yday` (day index in year), `tm_isdst`
    /// (daylight saving time) and `tm_nsec` (nanoseconds of the date from 1970)
    /// are set to 0 since they aren't available in the PDF time format. They could,
    /// however, be calculated manually
    #[cfg(not(feature = "chrono_time"))]
    pub fn as_datetime(&self) -> Option<OffsetDateTime> {
        const TIME_FMT_DECODE_STR: &str = "%Y%m%d%H%M%S%z";
        let text = self.datetime_string()?;
        OffsetDateTime::parse(&text, TIME_FMT_DECODE_STR).ok()
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

#[cfg(feature = "chrono_time")]
#[test]
fn parse_datetime_seconds_missing() {
    // this is the example from the PDF reference, version 1.7, chapter 3.8.3
    let text = Object::string_literal("D:199812231952-08'00'");
    assert!(text.as_datetime().is_some());
}

#[cfg(feature = "chrono_time")]
#[test]
fn parse_datetime_time_missing() {
    let text = Object::string_literal("D:20040229");
    assert!(text.as_datetime().is_some());
}

#[cfg(not(feature = "chrono_time"))]
#[test]
fn parse_datetime() {
    let time = time::OffsetDateTime::now_utc();

    let text: Object = time.into();
    let time2 = text.as_datetime().unwrap();

    assert_eq!(time2.date(), time.date());

    // Ignore nanoseconds
    // - not important in the date parsing
    assert_eq!(time2.time().hour(), time.time().hour());
    assert_eq!(time2.time().minute(), time.time().minute());
    assert_eq!(time2.time().second(), time.time().second());
}
