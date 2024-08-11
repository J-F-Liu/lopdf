mod glyphnames;
mod mappings;
pub use self::mappings::*;

pub fn bytes_to_string(encoding: [Option<u16>; 256], bytes: &[u8]) -> String {
    let code_points = bytes
        .iter()
        .filter_map(|&byte| encoding[byte as usize])
        .collect::<Vec<u16>>();
    String::from_utf16_lossy(&code_points)
}

pub fn string_to_bytes(encoding: [Option<u16>; 256], text: &str) -> Vec<u8> {
    text.encode_utf16()
        .filter_map(|ch| encoding.iter().position(|&code| code == Some(ch)))
        .map(|byte| byte as u8)
        .collect()
}

/// Encodes the given `str` to UTF-16BE.
/// The recommended way to encode text strings, as it supports all of
/// unicode and all major PDF readers support it.
pub fn encode_utf16_be(text: &str) -> Vec<u8> {
    // Prepend BOM to the mark string as UTF-16BE encoded.
    let bom: u16 = 0xFEFF;
    let mut bytes = vec![];
    bytes.extend([bom].iter().flat_map(|b| b.to_be_bytes()));
    bytes.extend(text.encode_utf16().flat_map(|b| b.to_be_bytes()));
    bytes
}

/// Encodes the given `str` to UTF-8. This method of encoding text strings
/// is first specified in PDF2.0 and reader support is still lacking
/// (notably, Adobe Acrobat Reader doesn't support it at the time of writing).
/// Thus, using it is **NOT RECOMMENDED**.
pub fn encode_utf8(text: &str) -> Vec<u8> {
    // Prepend BOM to the mark string as UTF-8 encoded.
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(text.bytes());
    bytes
}
