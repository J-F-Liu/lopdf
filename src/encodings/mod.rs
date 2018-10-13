pub mod encodings;
mod glyphnames;

pub fn bytes_to_string(encoding: [Option<u16>; 256], bytes: &[u8]) -> String {
	let code_points = bytes
		.iter()
		.map(|byte| encoding[*byte as usize])
		.filter(|code| code.is_some())
		.map(|code| code.unwrap())
		.collect::<Vec<u16>>();
	String::from_utf16_lossy(&code_points)
}

pub fn string_to_bytes(encoding: [Option<u16>; 256], text: &str) -> Vec<u8> {
	text
		.chars()
		.map(|ch| encoding.iter().position(|&code| code == Some(ch as u16)))
		.filter(|byte| byte.is_some())
		.map(|byte| byte.unwrap() as u8)
		.collect::<Vec<u8>>()
}
