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
