mod glyphnames;
mod mappings;

pub use self::mappings::*;

pub fn bytes_to_string(encoding: [Option<u16>; 256], bytes: &[u8]) -> String {
	let code_points = bytes
		.iter()
		.map(|byte| encoding[*byte as usize])
		.filter(Option::is_some)
		.map(Option::unwrap)
		.collect::<Vec<u16>>();
	String::from_utf16_lossy(&code_points)
}

pub fn string_to_bytes(encoding: [Option<u16>; 256], text: &str) -> Vec<u8> {
	text
		.chars()
		.map(|ch| encoding.iter().position(|&code| code == Some(ch as u16)))
		.filter(Option::is_some)
		.map(|byte| byte.unwrap() as u8)
		.collect::<Vec<u8>>()
}
