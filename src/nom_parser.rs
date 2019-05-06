use super::{Dictionary, Object, ObjectId, Stream, StringFormat};
use crate::content::*;
use crate::reader::Reader;
use crate::xref::*;
use pom::char_class::{alpha, hex_digit, multispace, oct_digit};
use pom::parser::*;
use std::str::{self, FromStr};

use nom::IResult;
use nom::bytes::complete::{tag, take_while, take_while1};
use nom::branch::alt;
use nom::error::ParseError;
use nom::multi::{many0, many1, many0_count, many1_count};
use nom::combinator::{opt, map_res};
use nom::character::complete::{one_of as nom_one_of};

fn nom_to_pom<'a, O, NP>(f: NP) -> Parser<'a, u8, O>
	where NP: Fn(&'a [u8]) -> IResult<&'a [u8], O, ()> + 'a
{
	Parser::new(move |input, inpos| {
		let nom_input = &input[inpos..];

		match f(nom_input) {
			Ok((rem, out)) => {
				let parsed_len = nom_input.len() - rem.len();
				let outpos = inpos + parsed_len;

				Ok((out, outpos))
			},
			Err(nom_err) => Err(match nom_err {
				nom::Err::Incomplete(_) => pom::Error::Incomplete,
				_ => pom::Error::Mismatch{ message: "nom error".into(), position: inpos },
			}),
		}
	})
}

fn eol<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], u8, E> {
	alt((|i| tag(b"\r\n")(i).map(|(i, _)| (i, b'\n')),
		 |i| tag(b"\n")(i).map(|(i, _)| (i, b'\n')),
		 |i| tag(b"\r")(i).map(|(i, _)| (i, b'\r')))
	)(input)
}

fn comment<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], (), E> {
	tag(b"%")(input)
		.and_then(|(i, _)| take_while(|c: u8| !b"\r\n".contains(&c))(i))
		.and_then(|(i, _)| eol(i))
		.map(|(i, _)| (i, ()))
}

#[inline]
fn is_whitespace(c: u8) -> bool {
	b" \t\n\r\0\x0C".contains(&c)
}

fn white_space<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], (), E> {
	take_while(is_whitespace)(input)
		.map(|(i, _)| (i, ()))
}

fn space<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], (), E> {
	many0_count(alt((
		|i| take_while1(is_whitespace)(i).map(|(i, _)| (i, ())),
		comment
	)))(input).map(|(i, _)| (i, ()))
}

fn integer<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], i64, E> {
	opt(nom_one_of("+-"))(input)
		.and_then(|(i, sign)| {
			map_res(take_while1(|c: u8| c.is_ascii_digit()),
					|m: &[u8]| {
						let len = sign.map(|_| 1).unwrap_or(0) + m.len();
						i64::from_str(str::from_utf8(&input[..len]).unwrap())
					})(i)
		})
}

fn real<'a>() -> Parser<'a, u8, f64> {
	let number = one_of(b"+-").opt() + ((one_of(b"0123456789").repeat(1..) * sym(b'.') - one_of(b"0123456789").repeat(0..)) | (sym(b'.') - one_of(b"0123456789").repeat(1..)));
	number.collect().convert(str::from_utf8).convert(|s| f64::from_str(&s))
}

fn hex_char<'a>() -> Parser<'a, u8, u8> {
	let number = is_a(hex_digit).repeat(2);
	number.collect().convert(|v| u8::from_str_radix(str::from_utf8(v).unwrap(), 16))
}

fn oct_char<'a>() -> Parser<'a, u8, u8> {
	let number = is_a(oct_digit).repeat(1..4);
	number.collect().convert(|v| u8::from_str_radix(str::from_utf8(v).unwrap(), 8))
}

fn name<'a>() -> Parser<'a, u8, Vec<u8>> {
	sym(b'/') * (none_of(b" \t\n\r\x0C()<>[]{}/%#") | (sym(b'#') * hex_char())).repeat(0..)
}

fn escape_sequence<'a>() -> Parser<'a, u8, Vec<u8>> {
	sym(b'\\')
		* (sym(b'\\').map(|_| vec![b'\\'])
			| sym(b'(').map(|_| vec![b'('])
			| sym(b')').map(|_| vec![b')'])
			| sym(b'n').map(|_| vec![b'\n'])
			| sym(b'r').map(|_| vec![b'\r'])
			| sym(b't').map(|_| vec![b'\t'])
			| sym(b'b').map(|_| vec![b'\x08'])
			| sym(b'f').map(|_| vec![b'\x0C'])
			| oct_char().map(|c| vec![c])
			| nom_to_pom(eol).map(|_| vec![])
			| empty().map(|_| vec![]))
}

fn nested_literal_string<'a>() -> Parser<'a, u8, Vec<u8>> {
	sym(b'(')
		* (none_of(b"\\()").repeat(1..) | escape_sequence() | call(nested_literal_string)).repeat(0..).map(|segments| {
			let mut bytes = segments.into_iter().fold(vec![b'('], |mut bytes, mut segment| {
				bytes.append(&mut segment);
				bytes
			});
			bytes.push(b')');
			bytes
		}) - sym(b')')
}

fn literal_string<'a>() -> Parser<'a, u8, Vec<u8>> {
	sym(b'(')
		* (none_of(b"\\()").repeat(1..) | escape_sequence() | nested_literal_string())
			.repeat(0..)
			.map(|segments| segments.concat())
		- sym(b')')
}

fn hexadecimal_string<'a>() -> Parser<'a, u8, Vec<u8>> {
	sym(b'<') * (nom_to_pom(white_space) * hex_char()).repeat(0..) - (nom_to_pom(white_space) * sym(b'>'))
}

fn array<'a>() -> Parser<'a, u8, Vec<Object>> {
	sym(b'[') * nom_to_pom(space) * call(direct_object).repeat(0..) - sym(b']')
}

fn dictionary<'a>() -> Parser<'a, u8, Dictionary> {
	let entry = name() - nom_to_pom(space) + call(direct_object);
	let entries = seq(b"<<") * nom_to_pom(space) * entry.repeat(0..) - seq(b">>");
	entries.map(|entries| {
		entries.into_iter().fold(Dictionary::new(), |mut dict: Dictionary, (key, value)| {
			dict.set(key, value);
			dict
		})
	})
}

fn stream(reader: &Reader) -> Parser<u8, Stream> {
	(dictionary() - nom_to_pom(space) - seq(b"stream") - nom_to_pom(eol))
		>> move |dict: Dictionary| {
			if let Some(length) = dict.get(b"Length").and_then(|value| {
				if let Some(id) = value.as_reference() {
					return reader.get_object(id).and_then(|value| value.as_i64());
				}
				value.as_i64()
			}) {
				let stream = take(length as usize) - nom_to_pom(eol).opt() - seq(b"endstream").expect("endstream");
				stream.map(move |data| Stream::new(dict.clone(), data.to_vec()))
			} else {
				empty().pos().map(move |pos| Stream::with_position(dict.clone(), pos))
			}
		}
}

fn object_id<'a>() -> Parser<'a, u8, ObjectId> {
	let id = one_of(b"0123456789").repeat(1..).convert(|v| u32::from_str(&str::from_utf8(&v).unwrap()));
	let gen = one_of(b"0123456789").repeat(1..).convert(|v| u16::from_str(&str::from_utf8(&v).unwrap()));
	id - nom_to_pom(space) + gen - nom_to_pom(space)
}

pub fn direct_object<'a>() -> Parser<'a, u8, Object> {
	(seq(b"null").map(|_| Object::Null)
		| seq(b"true").map(|_| Object::Boolean(true))
		| seq(b"false").map(|_| Object::Boolean(false))
		| (object_id().map(Object::Reference) - sym(b'R'))
		| real().map(Object::Real)
		| nom_to_pom(integer).map(Object::Integer)
		| name().map(Object::Name)
		| literal_string().map(Object::string_literal)
		| hexadecimal_string().map(|bytes| Object::String(bytes, StringFormat::Hexadecimal))
		| array().map(Object::Array)
		| dictionary().map(Object::Dictionary))
		- nom_to_pom(space)
}

fn object(reader: &Reader) -> Parser<u8, Object> {
	(seq(b"null").map(|_| Object::Null)
		| seq(b"true").map(|_| Object::Boolean(true))
		| seq(b"false").map(|_| Object::Boolean(false))
		| (object_id().map(Object::Reference) - sym(b'R'))
		| real().map(Object::Real)
		| nom_to_pom(integer).map(Object::Integer)
		| name().map(Object::Name)
		| literal_string().map(Object::string_literal)
		| hexadecimal_string().map(|bytes| Object::String(bytes, StringFormat::Hexadecimal))
		| array().map(Object::Array)
		| stream(reader).map(Object::Stream)
		| dictionary().map(Object::Dictionary))
		- nom_to_pom(space)
}

pub fn indirect_object(reader: &Reader) -> Parser<u8, (ObjectId, Object)> {
	object_id() - seq(b"obj") - nom_to_pom(space) + object(reader) - nom_to_pom(space) - seq(b"endobj").opt() - nom_to_pom(space)
}

pub fn header<'a>() -> Parser<'a, u8, String> {
	seq(b"%PDF-") * none_of(b"\r\n").repeat(0..).convert(String::from_utf8) - nom_to_pom(eol) - nom_to_pom(comment).repeat(0..)
}

fn xref<'a>() -> Parser<'a, u8, Xref> {
	let xref_entry = nom_to_pom(integer).map(|i| i as u32) - sym(b' ') + nom_to_pom(integer).map(|i| i as u16) - sym(b' ') + one_of(b"nf").map(|k| k == b'n') - take(2);
	let xref_section = nom_to_pom(integer).map(|i| i as usize) - sym(b' ') + nom_to_pom(integer) - sym(b' ').opt() - nom_to_pom(eol) + xref_entry.repeat(0..);
	let xref = seq(b"xref") * nom_to_pom(eol) * xref_section.repeat(1..) - nom_to_pom(space);
	xref.map(|sections| {
		sections
			.into_iter()
			.fold(Xref::new(0), |mut xref: Xref, ((start, _count), entries): _| {
				for (index, ((offset, generation), is_normal)) in entries.into_iter().enumerate() {
					if is_normal {
						xref.insert((start + index) as u32, XrefEntry::Normal { offset, generation });
					}
				}
				xref
			})
	})
}

fn trailer<'a>() -> Parser<'a, u8, Dictionary> {
	seq(b"trailer") * nom_to_pom(space) * dictionary() - nom_to_pom(space)
}

pub fn xref_and_trailer(reader: &Reader) -> Parser<u8, (Xref, Dictionary)> {
	(xref() + trailer()).map(|(mut xref, trailer)| {
		xref.size = trailer.get(b"Size").and_then(Object::as_i64).expect("Size is absent in trailer.") as u32;
		(xref, trailer)
	}) | indirect_object(reader).convert(|(_, obj)| match obj {
		Object::Stream(stream) => Ok(decode_xref_stream(stream)),
		_ => Err("Xref is not a stream object."),
	})
}

pub fn xref_start<'a>() -> Parser<'a, u8, i64> {
	seq(b"startxref") * nom_to_pom(eol) * nom_to_pom(integer) - nom_to_pom(eol) - seq(b"%%EOF") - nom_to_pom(space)
}

// The following code create parser to parse content stream.

fn content_space<'a>() -> Parser<'a, u8, ()> {
	is_a(multispace).repeat(0..).discard()
}

fn operator<'a>() -> Parser<'a, u8, String> {
	(is_a(alpha) | one_of(b"*'\"")).repeat(1..).convert(String::from_utf8)
}

fn operand<'a>() -> Parser<'a, u8, Object> {
	(seq(b"null").map(|_| Object::Null)
		| seq(b"true").map(|_| Object::Boolean(true))
		| seq(b"false").map(|_| Object::Boolean(false))
		| real().map(Object::Real)
		| nom_to_pom(integer).map(Object::Integer)
		| name().map(Object::Name)
		| literal_string().map(Object::string_literal)
		| hexadecimal_string().map(|bytes| Object::String(bytes, StringFormat::Hexadecimal))
		| array().map(Object::Array)
		| dictionary().map(Object::Dictionary))
		- content_space()
}

fn operation<'a>() -> Parser<'a, u8, Operation> {
	let operation = operand().repeat(0..) + operator() - content_space();
	operation.map(|(operands, operator)| Operation { operator, operands })
}

pub fn content<'a>() -> Parser<'a, u8, Content> {
	content_space() * operation().repeat(0..).map(|operations| Content { operations })
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_real_number() {
		let r0 = real().parse(b"0.12");
		assert_eq!(r0, Ok(0.12));
		let r1 = real().parse(b"-.12");
		assert_eq!(r1, Ok(-0.12));
		let r2 = real().parse(b"10.");
		assert_eq!(r2, Ok(10.0));
	}

	#[test]
	fn parse_string() {
		assert_eq!(literal_string().parse(b"()"), Ok(b"".to_vec()));
		assert_eq!(literal_string().parse(b"(text())"), Ok(b"text()".to_vec()));
		assert_eq!(literal_string().parse(b"(text\r\n\\\\(nested\\t\\b\\f))"), Ok(b"text\r\n\\(nested\t\x08\x0C)".to_vec()));
		assert_eq!(literal_string().parse(b"(text\\0\\53\\053\\0053)"), Ok(b"text\0++\x053".to_vec()));
		assert_eq!(literal_string().parse(b"(text line\\\n())"), Ok(b"text line()".to_vec()));
		assert_eq!(name().parse(b"/ABC#5f"), Ok(b"ABC\x5F".to_vec()));
	}

	#[test]
	fn parse_name() {
		let text = b"/#cb#ce#cc#e5";
		let name = name().parse(text);
		println!("{:?}", name);
		assert_eq!(name.is_ok(), true);
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
		let content = content().parse(stream);
		println!("{:?}", content);
		assert_eq!(content.is_ok(), true);
	}
}
