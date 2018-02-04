mod glyphnames;
pub mod encodings;

pub fn bytes_to_unicode(encoding: [Option<u16>; 256], bytes: &[u8]) -> String {
	let code_points = bytes
		.iter()
		.map(|byte| encoding[*byte as usize])
		.filter(|code| code.is_some())
		.map(|code| code.unwrap())
		.collect::<Vec<u16>>();
	String::from_utf16_lossy(&code_points)
}
