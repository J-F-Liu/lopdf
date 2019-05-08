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
use nom::multi::{many0, fold_many0};
use nom::combinator::{opt, map, map_res, map_opt};
use nom::character::complete::{one_of as nom_one_of};
use nom::sequence::{pair, preceded, terminated, tuple};

// Change this to something else that implements ParseError to get a
// different error type out of nom.
type NomError = ();
type NomResult<'a, O, E=NomError> = IResult<&'a [u8], O, E>;

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

// TODO: make this a part of nom
fn contained<I, O1, O2, O3, E: ParseError<I>, F, G, H>(start: F, value: G, end: H) -> impl Fn(I) -> IResult<I, O2, E>
	where
	F: Fn(I) -> IResult<I, O1, E>,
	G: Fn(I) -> IResult<I, O2, E>,
	H: Fn(I) -> IResult<I, O3, E>,
{
	move |input: I| {
		let (input, _) = start(input)?;
		let (input, v) = value(input)?;
		end(input).map(|(i, _)| (i, v))
	}
}


fn eol<'a>(input: &'a [u8]) -> NomResult<'a, ()> {
	map(alt((tag(b"\r\n"), tag(b"\n"), tag(b"\r"))), |_| ())(input)
}

fn comment<'a>(input: &'a [u8]) -> NomResult<'a, ()> {
	map(tuple((tag(b"%"), take_while(|c: u8| !b"\r\n".contains(&c)), eol)), |_| ())(input)
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

fn white_space<'a>(input: &'a [u8]) -> NomResult<'a, ()> {
	map(take_while(is_whitespace), |_| ())(input)
}

fn space<'a>(input: &'a [u8]) -> NomResult<'a, ()> {
	fold_many0(alt((
		map(take_while1(is_whitespace), |_| ()),
		comment
	)), (),	|_, _| ())(input)
}

fn integer<'a>(input: &'a [u8]) -> NomResult<'a, i64> {
	opt(nom_one_of("+-"))(input)
		.and_then(|(i, sign)| {
			map_res(take_while1(|c: u8| c.is_ascii_digit()),
					|m: &[u8]| {
						let len = sign.map(|_| 1).unwrap_or(0) + m.len();
						i64::from_str(str::from_utf8(&input[..len]).unwrap())
					})(i)
		})
}

fn real<'a>(input: &'a [u8]) -> NomResult<'a, f64> {
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

	f64::from_str(float_str).map(|v| (i, v)).map_err(|_| nom::Err::Error(NomError::from_error_kind(i, ErrorKind::Digit)))
}

fn hex_char<'a>(input: &'a [u8]) -> NomResult<'a, u8> {
	map_res(take_while_m_n(2, 2, |c: u8| c.is_ascii_hexdigit()),
			|x| u8::from_str_radix(str::from_utf8(x).unwrap(), 16)
	)(input)
}

fn oct_char<'a>(input: &'a [u8]) -> NomResult<'a, u8> {
	map_res(take_while_m_n(1, 3, |c: u8| c.is_ascii_hexdigit()),
			// Spec requires us to ignore any overflow.
			|x| u16::from_str_radix(str::from_utf8(x).unwrap(), 8).map(|o| o as u8)
	)(input)
}

fn name<'a>(input: &'a [u8]) -> NomResult<'a, Vec<u8>> {
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

fn escape_sequence<'a>(input: &'a [u8]) -> NomResult<'a, Option<u8>> {
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

fn inner_literal_string<'a>(input: &'a [u8]) -> NomResult<'a, Vec<u8>> {
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

fn nested_literal_string<'a>(input: &'a [u8]) -> NomResult<'a, Vec<u8>> {
	map(contained(tag(b"("), inner_literal_string, tag(b")")),
		|mut content| {
			content.insert(0, b'(');
			content.push(b')');
			content
		})(input)
}

fn literal_string<'a>(input: &'a [u8]) -> NomResult<'a, Vec<u8>> {
	contained(tag(b"("), inner_literal_string, tag(b")"))(input)
}

fn hexadecimal_string<'a>(input: &'a [u8]) -> NomResult<'a, Object> {
	let (i, bytes) = contained(tag(b"<"),
							   terminated(many0(|i| white_space(i).and_then(|(i, _)| hex_char(i))),
										  white_space),
							   tag(b">"))(input)?;

	Ok((i, Object::String(bytes, StringFormat::Hexadecimal)))
}

fn boolean<'a>(input: &'a [u8]) -> NomResult<'a, Object> {
	alt((
		map(tag(b"true"), |_| Object::Boolean(true)),
		map(tag(b"false"), |_| Object::Boolean(false))
	))(input)
}

fn null<'a>(input: &'a [u8]) -> NomResult<'a, Object> {
	map(tag(b"null"), |_| Object::Null)(input)
}

fn array<'a>(input: &'a [u8]) -> NomResult<'a, Vec<Object>> {
	let (i, _) = tag(b"[")(input)?;
	let (i, _) = space(i)?;
	let (i, objects) = many0(_direct_object)(i)?;
	let (i, _) = tag(b"]")(i)?;

	Ok((i, objects))
}

fn dict_entry<'a>(input: &'a [u8]) -> NomResult<'a, (Vec<u8>, Object)> {
	pair(terminated(name, space), _direct_object)(input)
}

fn dictionary<'a>(input: &'a [u8]) -> NomResult<'a, Dictionary> {
	let (i, _) = terminated(tag(b"<<"), space)(input)?;
	let (i, dict) = fold_many0(dict_entry, Dictionary::new(),
							   |mut dict, (key, value)| {
								   dict.set(key, value);
								   dict
							   })(i)?;
	let (i, _) = tag(b">>")(i)?;

	Ok((i, dict))
}

fn stream<'a>(input: &'a [u8], reader: &Reader) -> NomResult<'a, Object> {
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

fn unsigned_int<'a, I: FromStr>(input: &'a [u8]) -> NomResult<'a, I> {
	let (i, digits) = take_while1(|c: u8| c.is_ascii_digit())(input)?;

	I::from_str(str::from_utf8(&digits).unwrap()).map(|v| (i, v)).map_err(|_| nom::Err::Error(NomError::from_error_kind(i, ErrorKind::Digit)))
}

fn object_id<'a>(input: &'a [u8]) -> NomResult<'a, ObjectId> {
	pair(terminated(unsigned_int, space), terminated(unsigned_int, space))(input)
}

fn reference<'a>(input: &'a [u8]) -> NomResult<'a, Object> {
	map(terminated(object_id, tag(b"R")), Object::Reference)(input)
}

fn _direct_objects<'a>(input: &'a [u8]) -> NomResult<'a, Object> {
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

fn _direct_object<'a>(input: &'a [u8]) -> NomResult<'a, Object> {
	terminated(_direct_objects, space)(input)
}

pub fn direct_object<'a>() -> Parser<'a, u8, Object> {
	nom_to_pom(_direct_object)
}

fn object<'a>(input: &'a [u8], reader: &Reader) -> NomResult<'a, Object> {
	terminated(alt((|input| stream(input, reader), _direct_objects)), space)(input)
}

pub fn indirect_object(reader: &Reader) -> Parser<u8, (ObjectId, Object)> {
	nom_to_pom(move |input| _indirect_object(input, reader))
}

fn _indirect_object<'a>(input: &'a [u8], reader: &Reader) -> NomResult<'a, (ObjectId, Object)> {
	let (i, object_id) = terminated(object_id, pair(tag(b"obj"), space))(input)?;

	let object_offset = input.len() - i.len();
	let (i, mut object) = terminated(|i| object(i, reader), tuple((space, opt(tag(b"endobj")), space)))(i)?;

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

fn content_space<'a>(input: &'a [u8]) -> NomResult<'a, ()> {
	take_while(|c| b" \t\r\n".contains(&c))(input)
		.map(|(i, _)| (i, ()))
}

fn operator<'a>(input: &'a [u8]) -> NomResult<'a, String> {
	map_res(take_while1(|c: u8| c.is_ascii_alphabetic() || b"*'\"".contains(&c)),
			|op| str::from_utf8(op).map(Into::into))(input)
}

fn operand<'a>(input: &'a [u8]) -> NomResult<'a, Object> {
	terminated(alt((
		null,
		boolean,
		map(real, Object::Real),
		map(integer, Object::Integer),
		map(name, Object::Name),
		map(literal_string, Object::string_literal),
		hexadecimal_string,
		map(array, Object::Array),
		map(dictionary, Object::Dictionary),
	)), content_space)(input)
}

fn operation<'a>(input: &'a [u8]) -> NomResult<'a, Operation> {
	let (i, (operands, operator)) = terminated(pair(many0(operand), operator), content_space)(input)?;

	Ok((i, Operation { operator, operands }))
}

fn _content<'a>(input: &'a [u8]) -> NomResult<'a, Content> {
	preceded(content_space, map(many0(operation), |operations| Content { operations }))(input)
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