use pom::char_class::{alpha, hex_digit, oct_digit, multispace};
use pom::{parser, Parser};
use pom::parser::*;
use std::collections::BTreeMap;
use std::str::FromStr;
use super::{Object, ObjectId, Dictionary, Stream, StringFormat};
use reader::Reader;
use content::*;

fn eol() -> Parser<u8, u8> {
	sym(b'\r') * sym(b'\n') | sym(b'\n') | sym(b'\r')
}

fn comment() -> Parser<u8, ()> {
	sym(b'%') * none_of(b"\r\n").repeat(0..) * eol().discard()
}

fn space() -> Parser<u8, ()> {
	( one_of(b" \t\n\r\0\x0C").repeat(1..).discard()
	| comment()
	).repeat(0..).discard()
}

fn integer() -> Parser<u8, i64> {
	let number = one_of(b"+-").opt() + one_of(b"0123456789").repeat(1..);
	number.collect().convert(|v|String::from_utf8(v)).convert(|s|i64::from_str(&s))
}

fn real() -> Parser<u8, f64> {
	let number = one_of(b"+-").opt() +
		( one_of(b"0123456789").repeat(1..) * sym(b'.') - one_of(b"0123456789").repeat(0..)
		| sym(b'.') - one_of(b"0123456789").repeat(1..)
		);
	number.collect().convert(|v|String::from_utf8(v)).convert(|s|f64::from_str(&s))
}

fn hex_char() -> Parser<u8, u8> {
	let number = is_a(hex_digit).repeat(2..3);
	number.collect().convert(|v|u8::from_str_radix(&String::from_utf8(v).unwrap(), 16))
}

fn oct_char() -> Parser<u8, u8> {
	let number = is_a(oct_digit).repeat(1..4);
	number.collect().convert(|v|u8::from_str_radix(&String::from_utf8(v).unwrap(), 8))
}

fn name() -> Parser<u8, String> {
	let name = sym(b'/') * (none_of(b" \t\n\r\x0C()<>[]{}/%#") | sym(b'#') * hex_char()).repeat(0..);
	name.convert(|v|String::from_utf8(v))
}

fn escape_sequence() -> Parser<u8, Vec<u8>> {
	sym(b'\\') *
	( sym(b'\\').map(|_| vec![b'\\'])
	| sym(b'(').map(|_| vec![b'('])
	| sym(b')').map(|_| vec![b')'])
	| sym(b'n').map(|_| vec![b'\n'])
	| sym(b'r').map(|_| vec![b'\r'])
	| sym(b't').map(|_| vec![b'\t'])
	| sym(b'b').map(|_| vec![b'\x08'])
	| sym(b'f').map(|_| vec![b'\x0C'])
	| oct_char().map(|c| vec![c])
	| eol()     .map(|_| vec![])
	| empty()   .map(|_| vec![])
	)
}

fn nested_literal_string() -> Parser<u8, Vec<u8>> {
	sym(b'(') *
	( none_of(b"\\()").repeat(1..)
	| escape_sequence()
	| call(nested_literal_string)
	).repeat(0..).map(|segments| {
		let mut bytes = segments.into_iter().fold(
			vec![b'('],
			|mut bytes, mut segment| {
				bytes.append(&mut segment);
				bytes
			});
		bytes.push(b')');
		bytes
	})
	- sym(b')')
}

fn literal_string() -> Parser<u8, Vec<u8>> {
	sym(b'(') *
	( none_of(b"\\()").repeat(1..)
	| escape_sequence()
	| nested_literal_string()
	).repeat(0..).map(|segments|segments.concat())
	- sym(b')')
}

fn hexadecimal_string() -> Parser<u8, Vec<u8>> {
	sym(b'<') * hex_char().repeat(0..) - sym(b'>')
}

fn array() -> Parser<u8, Vec<Object>> {
	sym(b'[') * space() * call(direct_object).repeat(0..) - sym(b']')
}

fn dictionary() -> Parser<u8, Dictionary> {
	let entry = name() - space() + call(direct_object);
	let entries = seq(b"<<") * space() * entry.repeat(0..) - seq(b">>");
	entries.map(|entries| entries.into_iter().fold(
		Dictionary::new(),
		|mut dict: Dictionary, (key, value)| { dict.set(key, value); dict }
	))
}

fn stream<'a>(reader: &'a Reader) -> parser::Parser<'a, u8, Stream> {
	dictionary() - space() - seq(b"stream") - eol() >>
	move |dict: Dictionary| {
		let length = dict.get("Length").and_then(|value| {
			if let Some(id) = value.as_reference() {
				return reader.get_object(id).and_then(|value|value.as_i64());
			}
			return value.as_i64();
		}).expect("Stream Length should be an integer.");
		let stream = take(length as usize) - eol().opt() - seq(b"endstream");
		stream.map(move |data|Stream::new(dict.clone(), data))
	}
}

fn object_id() -> Parser<u8, ObjectId> {
	let id = one_of(b"0123456789").repeat(1..).convert(|v|u32::from_str(&String::from_utf8(v).unwrap()));
	let gen = one_of(b"0123456789").repeat(1..).convert(|v|u16::from_str(&String::from_utf8(v).unwrap()));
	id - space() + gen - space()
}

fn direct_object() -> Parser<u8, Object> {
	( seq(b"null").map(|_|Object::Null)
	| seq(b"true").map(|_|Object::Boolean(true))
	| seq(b"false").map(|_|Object::Boolean(false))
	| object_id().map(|id|Object::Reference(id)) - sym(b'R')
	| real().map(|num|Object::Real(num))
	| integer().map(|num|Object::Integer(num))
	| name().map(|text| Object::Name(text))
	| literal_string().map(|bytes| Object::String(bytes, StringFormat::Literal))
	| hexadecimal_string().map(|bytes| Object::String(bytes, StringFormat::Hexadecimal))
	| array().map(|items|Object::Array(items))
	| dictionary().map(|dict|Object::Dictionary(dict))
	) - space()
}

fn object<'a>(reader: &'a Reader) -> parser::Parser<'a, u8, Object> {
	( seq(b"null").map(|_|Object::Null)
	| seq(b"true").map(|_|Object::Boolean(true))
	| seq(b"false").map(|_|Object::Boolean(false))
	| object_id().map(|id|Object::Reference(id)) - sym(b'R')
	| real().map(|num|Object::Real(num))
	| integer().map(|num|Object::Integer(num))
	| name().map(|text| Object::Name(text))
	| literal_string().map(|bytes| Object::String(bytes, StringFormat::Literal))
	| hexadecimal_string().map(|bytes| Object::String(bytes, StringFormat::Hexadecimal))
	| array().map(|items|Object::Array(items))
	| stream(reader).map(|stream|Object::Stream(stream))
	| dictionary().map(|dict|Object::Dictionary(dict))
	) - space()
}

pub fn indirect_object<'a>(reader: &'a Reader) -> parser::Parser<'a, u8, (ObjectId, Object)> {
	object_id() - seq(b"obj") - space() + object(reader) - space() - seq(b"endobj") - space()
}

pub fn header() -> Parser<u8, String> {
	seq(b"%PDF-") * none_of(b"\r\n").repeat(0..).convert(|v|String::from_utf8(v)) - eol() - comment().repeat(0..)
}

pub fn xref() -> Parser<u8, BTreeMap<u32, (u16, u64)>> {
	let xref_entry = integer().map(|i|i as u64) - sym(b' ') + integer().map(|i|i as u16) - sym(b' ') + one_of(b"nf").map(|k|k==b'n') - take(2);
	let xref_section = integer().map(|i|i as usize) - sym(b' ') + integer() - eol() + xref_entry.repeat(1..);
	let xref = seq(b"xref") * eol() * xref_section.repeat(1..) - space();
	xref.map(|sections| {
		sections.into_iter().fold(
		BTreeMap::new(),
		|mut acc: BTreeMap<_, _>, ((start, _count), entries): ((usize, i64), Vec<((u64, u16), bool)>)| {
			for (index, ((offset, generation), is_normal)) in entries.into_iter().enumerate() {
				if is_normal {
					acc.insert((start + index) as u32, (generation, offset));
				}
			}
			acc
		})
	})
}

pub fn trailer() -> Parser<u8, Dictionary> {
	seq(b"trailer") * space() * dictionary() - space()
}

pub fn xref_start() -> Parser<u8, i64> {
	seq(b"startxref") * eol() * integer() - eol() - seq(b"%%EOF") - space()
}

fn content_space() -> Parser<u8, ()> {
	is_a(multispace).repeat(0..).discard()
}

fn operator() -> Parser<u8, String> {
	(is_a(alpha) | one_of(b"*'\"")).repeat(1..).convert(|v|String::from_utf8(v))
}

fn operand() -> Parser<u8, Object> {
	( seq(b"null").map(|_|Object::Null)
	| seq(b"true").map(|_|Object::Boolean(true))
	| seq(b"false").map(|_|Object::Boolean(false))
	| real().map(|num|Object::Real(num))
	| integer().map(|num|Object::Integer(num))
	| name().map(|text| Object::Name(text))
	| literal_string().map(|bytes| Object::String(bytes, StringFormat::Literal))
	| hexadecimal_string().map(|bytes| Object::String(bytes, StringFormat::Hexadecimal))
	| array().map(|items|Object::Array(items))
	| dictionary().map(|dict|Object::Dictionary(dict))
	) - content_space()
}

fn operation() -> Parser<u8, Operation> {
	let operation = operand().repeat(0..) + operator() - content_space();
	operation.map(|(operands, operator)| {
		Operation {
			operator: operator,
			operands: operands,
		}
	})
}

pub fn content() -> Parser<u8, Content> {
	content_space() * operation().repeat(0..).map(|operations| Content{operations: operations})
}

#[cfg(test)]
mod tests {
	use super::*;
	use pom::DataInput;

	#[test]
	fn parse_real_number() {
		let r0 = real().parse(&mut DataInput::new(b"0.12"));
		assert_eq!(r0, Ok(0.12));
		let r1 = real().parse(&mut DataInput::new(b"-.12"));
		assert_eq!(r1, Ok(-0.12));
		let r2 = real().parse(&mut DataInput::new(b"10."));
		assert_eq!(r2, Ok(10.0));
	}

	#[test]
	fn parse_string() {
		assert_eq!(
			literal_string().parse(&mut DataInput::new(b"()")),
			Ok(b"".to_vec()));
		assert_eq!(
			literal_string().parse(&mut DataInput::new(b"(text())")),
			Ok(b"text()".to_vec()));
		assert_eq!(
			literal_string().parse(&mut DataInput::new(b"(text\r\n\\\\(nested\\t\\b\\f))")),
			Ok(b"text\r\n\\(nested\t\x08\x0C)".to_vec()));
		assert_eq!(
			literal_string().parse(&mut DataInput::new(b"(text\\0\\53\\053\\0053)")),
			Ok(b"text\0++\x053".to_vec()));
		assert_eq!(
			literal_string().parse(&mut DataInput::new(b"(text line\\\n())")),
			Ok(b"text line()".to_vec()));
		assert_eq!(
			name().parse(&mut DataInput::new(b"/ABC#5f")),
			Ok("ABC\x5F".to_string()));
	}

	#[test]
	/// Run `cargo test -- --nocapture` to see output
	fn parse_content() {
		let stream = b"
2 J
BT
/F1 12 Tf
0 Tc
0 Tw
72.5 712 TD
[(Unencoded streams can be read easily) 65 (,) ] TJ
0 -14 TD
[(b) 20 (ut generally tak) 10 (e more space than \\311)] TJ
T* (encoded streams.) Tj
		";
		let content = content().parse(&mut DataInput::new(stream));
		println!("{:?}", content);
		assert_eq!(content.is_ok(), true);
	}
}
