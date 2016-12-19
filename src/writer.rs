use std::fs::File;
use std::io::{self, Seek, Write, SeekFrom};
use std::path::Path;

use super::{Document, Object, Dictionary, Stream, StringFormat};
use super::Object::*;

impl Document {
	pub fn save(&mut self, path: &Path) -> Result<File, io::Error> {
		let mut file = File::create(path)?;

		file.write_all(format!("%PDF-{}\n", self.version).as_bytes())?;
		self.reference_table.clear();

		for (id, object) in &self.objects {
			let offset = file.seek(SeekFrom::Current(0)).unwrap();
			self.reference_table.insert(id.0, (id.1, offset));

			file.write_all(format!("{} {} obj{}", id.0, id.1, if Document::need_separator(object) {" "} else {""}).as_bytes())?;
			Document::write_object(&mut file, object)?;
			file.write_all(format!("{}endobj\n", if Document::need_end_separator(object) {" "} else {""}).as_bytes())?;
		}

		let xref_start = file.seek(SeekFrom::Current(0)).unwrap();
		self.write_xref(&mut file)?;
		self.write_trailer(&mut file)?;
		file.write_all(format!("\nstartxref\n{}\n%%EOF", xref_start).as_bytes())?;

		Ok(file)
	}

	fn need_separator(object: &Object) -> bool {
		match *object {
			Null => true,
			Boolean(_) => true,
			Integer(_) => true,
			Real(_) => true,
			Reference(_) => true,
			_ => false,
		}
	}

	fn need_end_separator(object: &Object) -> bool {
		match *object {
			Null => true,
			Boolean(_) => true,
			Integer(_) => true,
			Real(_) => true,
			Name(_) => true,
			Object::Stream(_) => true,
			_ => false,
		}
	}

	fn write_object<'a>(file: &mut File, object: &'a Object) -> Result<(), io::Error> {
		match *object {
			Null => file.write_all(b"null"),
			Boolean(ref value) => file.write_all(format!("{}", value).as_bytes()),
			Integer(ref value) => file.write_all(format!("{}", value).as_bytes()),
			Real(ref value) => file.write_all(format!("{}", value).as_bytes()),
			Name(ref name) => Document::write_name(file, name),
			String(ref text, ref format) => Document::write_string(file, text, format),
			Array(ref array) => Document::write_array(file, array),
			Object::Dictionary(ref dict) => Document::write_dictionary(file, dict),
			Object::Stream(ref stream) => Document::write_stream(file, stream),
			Reference(ref id) => file.write_all(format!("{} {} R", id.0, id.1).as_bytes()),
		}
	}

	fn write_name<'a>(file: &mut File, name: &'a str) -> Result<(), io::Error> {
		file.write_all(b"/")?;
		for &byte in name.as_bytes() {
			// white-space and delimiter chars are encoded to # sequences
			if b" \t\x0C\r\n()<>[]{}/%#".contains(&byte) {
				file.write_all(format!("#{:02X}", byte).as_bytes())?;
			} else {
				file.write_all(&[byte])?;
			}
		}
		Ok(())
	}

	fn write_string<'a>(file: &mut File, text: &'a [u8], format: &'a StringFormat) -> Result<(), io::Error> {
		match *format {
			// Within a Literal string, backslash (\) and unbalanced parentheses should be escaped.
			// This rule apply to each individual byte in a string object,
			// whether the string is interpreted as single-byte or multiple-byte character codes.
			// If an end-of-line marker appears within a literal string without a preceding backslash, the result is equivalent to \n.
			// So \r also need be escaped.
			StringFormat::Literal => {
				let mut escape_indice = Vec::new();
				let mut parentheses = Vec::new();
				for (index, &byte) in text.into_iter().enumerate() {
					match byte {
						b'(' => parentheses.push(index),
						b')' => {
							if parentheses.len() > 0 {
								parentheses.pop();
							} else {
								escape_indice.push(index);
							}
						}
						b'\\' | b'\r' => escape_indice.push(index),
						_ => continue,
					}
				}
				escape_indice.append(&mut parentheses);

				file.write_all(b"(")?;
				if escape_indice.len() > 0 {
					for (index, &byte) in text.into_iter().enumerate() {
						if escape_indice.contains(&index) {
							file.write_all(b"\\")?;
							file.write_all(&[if byte == b'\r' { b'r' } else { byte }])?;
						} else {
							file.write_all(&[byte])?;
						}
					}
				} else {
					file.write_all(text)?;
				}
				file.write_all(b")")?;
			}
			StringFormat::Hexadecimal => {
				file.write_all(b"<")?;
				for &byte in text {
					file.write_all(format!("{:02X}", byte).as_bytes())?;
				}
				file.write_all(b">")?;
			}
		}
		Ok(())
	}

	fn write_array<'a>(file: &mut File, array: &'a Vec<Object>) -> Result<(), io::Error> {
		file.write_all(b"[")?;
		let mut first = true;
		for object in array {
			if first {
				first = false;
			} else if Document::need_separator(object) {
				file.write_all(b" ")?;
			}
			Document::write_object(file, object)?;
		}
		file.write_all(b"]")?;
		Ok(())
	}

	fn write_dictionary<'a>(file: &mut File, dictionary: &'a Dictionary) -> Result<(), io::Error> {
		file.write_all(b"<<")?;
		for (key, value) in dictionary {
			Document::write_name(file, key)?;
			if Document::need_separator(value) {
				file.write_all(b" ")?;
			}
			Document::write_object(file, value)?;
		}
		file.write_all(b">>")?;
		Ok(())
	}

	fn write_stream<'a>(file: &mut File, stream: &'a Stream) -> Result<(), io::Error> {
		Document::write_dictionary(file, &stream.dict)?;
		file.write_all(b"stream\n")?;
		file.write_all(&stream.content)?;
		file.write_all(b"endstream")?;
		Ok(())
	}

	fn write_xref(&self, file: &mut File) -> Result<(), io::Error> {
		file.write_all(b"xref\n")?;
		file.write_all(format!("0 {}\n", self.max_id + 1).as_bytes())?;

		let mut write_xref_entry = |offset: u64, generation: u16, kind: char| {
			file.write_all(format!("{:>010} {:>05} {} \n", offset, generation, kind).as_bytes())
		};
		write_xref_entry(0, 65535, 'f')?;

		let mut obj_id = 1;
		while obj_id <= self.max_id {
			if let Some(&(generation, offset)) = self.reference_table.get(&obj_id) {
				write_xref_entry(offset, generation, 'n')?;
			} else {
				write_xref_entry(0, 65535, 'f')?;
			}
			obj_id += 1;
		}
		Ok(())
	}

	fn write_trailer(&mut self, file: &mut File) -> Result<(), io::Error> {
		self.trailer.set("Size", (self.max_id + 1) as i64);
		file.write_all(b"trailer\n")?;
		Document::write_dictionary(file, &self.trailer)?;
		Ok(())
	}
}

#[test]
fn save_document() {
	let mut doc = Document::new();
	doc.version = "1.5".to_string();
	doc.objects.insert((1,0), Null);
	doc.objects.insert((2,0), Boolean(true));
	doc.objects.insert((3,0), Integer(3));
	doc.objects.insert((4,0), Real(0.5));
	doc.objects.insert((5,0), String("text((\r)".as_bytes().to_vec(), StringFormat::Literal));
	doc.objects.insert((6,0), String("text((\r)".as_bytes().to_vec(), StringFormat::Hexadecimal));
	doc.objects.insert((7,0), Name("name \t".to_string()));
	doc.objects.insert((8,0), Reference((1,0)));
	doc.objects.insert((9,2), Array(vec![Integer(1), Integer(2), Integer(3)]));
	doc.objects.insert((11,0), Stream(Stream::new(vec![0x41, 0x42, 0x43])));
	let mut dict = Dictionary::new();
	dict.set("A", Null);
	dict.set("B", false);
	dict.set("C", Name("name".to_string()));
	doc.objects.insert((12,0), Object::Dictionary(dict));
	doc.max_id = 12;
	doc.save(Path::new("test.pdf")).unwrap();
}
