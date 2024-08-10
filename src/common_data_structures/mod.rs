use crate::{encodings, Object, StringFormat};

pub fn text_string(text: &str) -> Object {
    if text.is_ascii() {
        return Object::String(text.into(), StringFormat::Literal);
    }
    Object::String(encodings::encode_utf16_be(text), StringFormat::Hexadecimal)
}
