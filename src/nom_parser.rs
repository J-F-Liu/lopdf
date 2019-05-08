use super::{Dictionary, Object, ObjectId, Stream, StringFormat};
use crate::content::*;
use crate::reader::Reader;
use crate::xref::*;
use pom::parser::*;
use std::str::{self, FromStr};

use nom::IResult;
use nom::bytes::complete::{tag, take as nom_take, take_while, take_while1, take_while_m_n};
use nom::branch::alt;
use nom::error::{ParseError, ErrorKind};
use nom::multi::{many0, many0_count, fold_many0};
use nom::combinator::{opt, map, map_res, map_opt};
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

#[inline]
fn is_delimiter(c: u8) -> bool {
	b"()<>[]{}/%".contains(&c)
}

#[inline]
fn is_regular(c: u8) -> bool {
	!is_whitespace(c) && !is_delimiter(c)
}

#[inline]
fn is_direct_literal_string(c: u8) -> bool {
	!b"()\\\r\n".contains(&c)
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

fn real<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], f64, E> {
	let (i, _) = opt(nom_one_of("+-"))(input)?;
	let (i, _) = alt((
		|i| take_while1(|c: u8| c.is_ascii_digit())(i)
			.and_then(|(i, _)| tag(b".")(i))
			.and_then(|(i, _)| take_while(|c: u8| c.is_ascii_digit())(i)),
		|i| tag(b".")(i)
			.and_then(|(i, _)| take_while1(|c: u8| c.is_ascii_digit())(i)),
	))(i)?;

	let float_input = &input[..input.len()-i.len()];
	let float_str = str::from_utf8(float_input).unwrap();

	f64::from_str(float_str).map(|v| (i, v)).map_err(|_| nom::Err::Error(E::from_error_kind(i, ErrorKind::Digit)))
}

fn hex_char<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], u8, E> {
	map_res(take_while_m_n(2, 2, |c: u8| c.is_ascii_hexdigit()),
			|x| u8::from_str_radix(str::from_utf8(x).unwrap(), 16)
	)(input)
}

fn oct_char<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], u8, E> {
	map_res(take_while_m_n(1, 3, |c: u8| c.is_ascii_hexdigit()),
			// Spec requires us to ignore any overflow.
			|x| u16::from_str_radix(str::from_utf8(x).unwrap(), 8).map(|o| o as u8)
	)(input)
}

fn name<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Vec<u8>, E> {
	tag(b"/")(input).and_then(|(i, _)| {
		many0(alt((
			|i| tag(b"#")(i).and_then(|(i, _)| hex_char(i)),

			map_opt(nom_take(1usize), |c: &[u8]| {
				if c[0] != b'#' && is_regular(c[0]) {
					Some(c[0])
				} else {
					None
				}
			})
		)))(i)
	})
}

fn escape_sequence<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Option<u8>, E> {
	tag(b"\\")(input).and_then(|(i, _)| {
		alt((
			map(|i| map_opt(nom_take(1usize), |c: &[u8]| {
				match c[0] {
					b'(' | b')' => Some(c[0]),
					b'n' => Some(b'\n'),
					b'r' => Some(b'\r'),
					b't' => Some(b'\t'),
					b'b' => Some(b'\x08'),
					b'f' => Some(b'\x0C'),
					b'\\' => Some(b'\\'),
					_ => None,
				}
			})(i), Some),

			map(oct_char, Some),
			map(eol, |_| None),
		))(i)
	})
}

enum ILS<'a> {
	Direct(&'a [u8]),
	Escape(Option<u8>),
	EOL,
	Nested(Vec<u8>)
}

impl <'a> ILS<'a> {
	fn push(&self, output: &mut Vec<u8>) {
		match self {
			ILS::Direct(d) => output.extend_from_slice(*d),
			ILS::Escape(e) => output.extend(e.into_iter()),
			ILS::EOL => output.extend(b"\n"),
			ILS::Nested(n) => output.extend_from_slice(n),
		}
	}
}

fn inner_literal_string<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Vec<u8>, E> {
	fold_many0(
		alt((
			map(take_while1(is_direct_literal_string), ILS::Direct),
			map(escape_sequence, ILS::Escape),
			// Any end of line in a string literal is treated as a line feed.
			map(eol, |_| ILS::EOL),
			map(nested_literal_string, ILS::Nested),
		)),
		Vec::new(),
		|mut out: Vec<u8>, value| { value.push(&mut out); out }
	)(input)
}

fn nested_literal_string<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Vec<u8>, E> {
	let (i, _) = tag(b"(")(input)?;
	let (i, mut content) = inner_literal_string(i)?;
	let (i, _) = tag(b")")(i)?;

	content.insert(0, b'(');
	content.push(b')');

	Ok((i, content))
}

fn literal_string<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Vec<u8>, E> {
	let (i, _) = tag(b"(")(input)?;
	let (i, content) = inner_literal_string(i)?;
	let (i, _) = tag(b")")(i)?;

	Ok((i, content))
}

fn hexadecimal_string<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Object, E> {
	let (i, _) = tag(b"<")(input)?;
	let (i, bytes) = many0(|i| white_space(i).and_then(|(i, _)| hex_char(i)))(i)?;
	let (i, _) = white_space(i)?;
	let (i, _) = tag(b">")(i)?;

	Ok((i, Object::String(bytes, StringFormat::Hexadecimal)))
}

fn boolean<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Object, E> {
	alt((
		map(tag(b"true"), |_| Object::Boolean(true)),
		map(tag(b"false"), |_| Object::Boolean(false))
	))(input)
}

fn null<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Object, E> {
	map(tag(b"null"), |_| Object::Null)(input)
}

fn array<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Vec<Object>, E> {
	let (i, _) = tag(b"[")(input)?;
	let (i, _) = space(i)?;
	let (i, objects) = many0(_direct_object)(i)?;
	let (i, _) = tag(b"]")(i)?;

	Ok((i, objects))
}

fn dict_entry<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], (Vec<u8>, Object), E> {
	let (i, name) = name(input)?;
	let (i, _) = space(i)?;
	let (i, object) = _direct_object(i)?;

	Ok((i, (name, object)))
}

fn dictionary<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Dictionary, E> {
	let (i, _) = tag(b"<<")(input)?;
	let (i, _) = space(i)?;
	let (i, dict) = fold_many0(dict_entry, Dictionary::new(),
							   |mut dict, (key, value)| {
								   dict.set(key, value);
								   dict
							   })(i)?;
	let (i, _) = tag(b">>")(i)?;

	Ok((i, dict))
}

fn stream<'a, E: ParseError<&'a [u8]>>(input: &'a [u8], reader: &Reader) -> IResult<&'a [u8], Object, E> {
	let (i, dict) = dictionary(input)?;
	let (i, _) = space(i)?;
	let (i, _) = tag(b"stream")(i)?;
	let (i, _) = eol(i)?;

	if let Some(length) = dict.get(b"Length").and_then(|value|
		if let Some(id) = value.as_reference() {
			reader.get_object(id).and_then(|value| value.as_i64())
		} else {
			value.as_i64()
		}) {

		let (i, data) = nom_take(length as usize)(i)?;
		let (i, _) = opt(eol)(i)?;
		let (i, _) = tag(b"endstream")(i)?;

		Ok((i, Object::Stream(Stream::new(dict, data.to_vec()))))
	} else {
		// Return position relative to the start of the stream dictionary.
		Ok((i, Object::Stream(Stream::with_position(dict, input.len() - i.len()))))
	}
}

fn unsigned_int<'a, E: ParseError<&'a [u8]>, I: FromStr>(input: &'a [u8]) -> IResult<&'a [u8], I, E> {
	let (i, digits) = take_while1(|c: u8| c.is_ascii_digit())(input)?;

	I::from_str(str::from_utf8(&digits).unwrap()).map(|v| (i, v)).map_err(|_| nom::Err::Error(E::from_error_kind(i, ErrorKind::Digit)))
}

fn object_id<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], ObjectId, E> {
	let (i, id) = unsigned_int(input)?;
	let (i, _) = space(i)?;
	let (i, gen) = unsigned_int(i)?;
	let (i, _) = space(i)?;

	Ok((i, (id, gen)))
}

fn reference<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Object, E> {
	let (i, id) = object_id(input)?;
	let (i, _) = tag(b"R")(i)?;

	Ok((i, Object::Reference(id)))
}

fn _direct_objects<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Object, E> {
	alt((
		null,
		boolean,
		reference,
		map(real, Object::Real),
		map(integer, Object::Integer),
		map(name, Object::Name),
		map(literal_string, Object::string_literal),
		hexadecimal_string,
		map(array, Object::Array),
		map(dictionary, Object::Dictionary),
	))(input)
}

fn _direct_object<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Object, E> {
	let (i, object) = _direct_objects(input)?;
	let (i, _) = space(i)?;

	Ok((i, object))
}

pub fn direct_object<'a>() -> Parser<'a, u8, Object> {
	nom_to_pom(_direct_object)
}

fn object<'a, E: ParseError<&'a [u8]>>(input: &'a [u8], reader: &Reader) -> IResult<&'a [u8], Object, E> {
	let (i, object) = alt((|input| stream(input, reader), _direct_objects))(input)?;
	let (i, _) = space(i)?;

	Ok((i, object))
}

pub fn indirect_object(reader: &Reader) -> Parser<u8, (ObjectId, Object)> {
	nom_to_pom(move |input| _indirect_object(input, reader))
}

fn _indirect_object<'a, E: ParseError<&'a [u8]>>(input: &'a [u8], reader: &Reader) -> IResult<&'a [u8], (ObjectId, Object), E> {
	let (i, object_id) = object_id(input)?;
	let (i, _) = tag(b"obj")(i)?;
	let (i, _) = space(i)?;

	let object_offset = input.len() - i.len();
	let (i, mut object) = object(i, reader)?;
	let (i, _) = space(i)?;
	let (i, _) = opt(tag(b"endobj"))(i)?;
	let (i, _) = space(i)?;

	if let Object::Stream(ref mut stream) = object {
		stream.offset_position(object_offset);
	}

	Ok((i, (object_id, object)))
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
	seq(b"trailer") * nom_to_pom(space) * nom_to_pom(dictionary) - nom_to_pom(space)
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

fn content_space<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], (), E> {
	take_while(|c| b" \t\r\n".contains(&c))(input)
		.map(|(i, _)| (i, ()))
}

fn operator<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], String, E> {
	map_res(take_while1(|c: u8| c.is_ascii_alphabetic() || b"*'\"".contains(&c)),
			|op| str::from_utf8(op).map(Into::into))(input)
}

fn operand<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Object, E> {
	let (i, object) = alt((
		null,
		boolean,
		map(real, Object::Real),
		map(integer, Object::Integer),
		map(name, Object::Name),
		map(literal_string, Object::string_literal),
		hexadecimal_string,
		map(array, Object::Array),
		map(dictionary, Object::Dictionary),
	))(input)?;

	let (i, _) = content_space(i)?;

	Ok((i, object))
}

fn operation<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Operation, E> {
	let (i, operands) = many0(operand)(input)?;
	let (i, operator) = operator(i)?;
	let (i, _) = content_space(i)?;

	Ok((i, Operation { operator, operands }))
}

fn _content<'a, E: ParseError<&'a [u8]>>(input: &'a [u8]) -> IResult<&'a [u8], Content, E> {
	let (i, _) = content_space(input)?;

	map(many0(operation), |operations| Content { operations })(i)
}


pub fn content<'a>() -> Parser<'a, u8, Content> {
	nom_to_pom(_content)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_real_number() {
		let r0 = nom_to_pom(real).parse(b"0.12");
		assert_eq!(r0, Ok(0.12));
		let r1 = nom_to_pom(real).parse(b"-.12");
		assert_eq!(r1, Ok(-0.12));
		let r2 = nom_to_pom(real).parse(b"10.");
		assert_eq!(r2, Ok(10.0));
	}

	#[test]
	fn parse_string() {
		assert_eq!(nom_to_pom(literal_string).parse(b"()"), Ok(b"".to_vec()));
		assert_eq!(nom_to_pom(literal_string).parse(b"(text())"), Ok(b"text()".to_vec()));
		assert_eq!(nom_to_pom(literal_string).parse(b"(text\r\n\\\\(nested\\t\\b\\f))"), Ok(b"text\n\\(nested\t\x08\x0C)".to_vec()));
		assert_eq!(nom_to_pom(literal_string).parse(b"(text\\0\\53\\053\\0053)"), Ok(b"text\0++\x053".to_vec()));
		assert_eq!(nom_to_pom(literal_string).parse(b"(text line\\\n())"), Ok(b"text line()".to_vec()));
		assert_eq!(nom_to_pom(name).parse(b"/ABC#5f"), Ok(b"ABC\x5F".to_vec()));
	}

	#[test]
	fn parse_name() {
		let text = b"/#cb#ce#cc#e5";
		let name = nom_to_pom(name).parse(text);
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
