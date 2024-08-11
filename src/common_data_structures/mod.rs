use crate::{encodings, Object, StringFormat};

/// Creates a text string.
/// If the input only contains ASCII characters, the string is encoded
/// in PDFDocEncoding, otherwise in UTF-16BE.
pub fn text_string(text: &str) -> Object {
    if text.is_ascii() {
        return Object::String(text.into(), StringFormat::Literal);
    }
    Object::String(encodings::encode_utf16_be(text), StringFormat::Hexadecimal)
}
