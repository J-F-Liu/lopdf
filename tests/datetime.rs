use lopdf::{DateTime, Object};

#[test]
fn test_datetime_is_exported_and_nameable() {
    let obj = Object::string_literal("D:20260710143000+02'00'");
    let dt: Option<DateTime> = obj.as_datetime();
    assert!(dt.is_some());
    let dt: DateTime = dt.unwrap();
    assert_eq!(dt.as_str(), "20260710143000+0200");
}

/// The `time` crate spells PDF's offset separator as `'` directly, so its
/// bytes must not be run through the chrono/jiff fixup. Applying it anyway
/// finds no `:` after the offset and corrupts the `D:` prefix into `D'`.
#[cfg(feature = "time")]
#[test]
fn test_time_offset_date_keeps_date_separator() {
    use time::OffsetDateTime;

    // 2024-01-11T12:00:00Z
    let date = OffsetDateTime::from_unix_timestamp(1_704_974_400).unwrap();
    let object: Object = date.into();

    let text = String::from_utf8(object.as_str().unwrap().to_vec()).unwrap();
    assert_eq!(text, "D:20240111120000+00'00'");
}

/// The `time` crate's naive form renders as UTC, with the `Z` suffix.
#[cfg(feature = "time")]
#[test]
fn test_time_naive_date_is_utc() {
    use time::PrimitiveDateTime;

    let date = PrimitiveDateTime::new(
        time::Date::from_calendar_date(2024, time::Month::January, 11).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );
    let object: Object = date.into();

    let text = String::from_utf8(object.as_str().unwrap().to_vec()).unwrap();
    assert_eq!(text, "D:20240111120000Z");
}
