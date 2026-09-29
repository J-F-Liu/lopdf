use lopdf::{DateTime, Object};

#[test]
fn test_datetime_is_exported_and_nameable() {
    let obj = Object::string_literal("D:20260710143000+02'00'");
    let dt: Option<DateTime> = obj.as_datetime();
    assert!(dt.is_some());
    let dt: DateTime = dt.unwrap();
    assert_eq!(dt.as_str(), "20260710143000+0200");
}
