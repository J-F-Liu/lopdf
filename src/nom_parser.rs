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
use nom::multi::{many0, many0_count, fold_many0, fold_many1};
use nom::combinator::{opt, map, map_res, map_opt};
use nom::character::complete::{one_of as nom_one_of};
use nom::sequence::{pair, preceded, terminated, tuple, separated_pair};

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

#[inline]
fn convert_result<O, E>(result: Result<O, E>, input: &[u8], error_kind: ErrorKind) -> NomResult<O> {
	result.map(|o| (input, o)).map_err(|_| nom::Err::Error(NomError::from_error_kind(input, error_kind)))
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


fn eol(input: &[u8]) -> NomResult<()> {
	map(alt((tag(b"\r\n"), tag(b"\n"), tag(b"\r"))), |_| ())(input)
}

fn comment(input: &[u8]) -> NomResult<()> {
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

fn white_space(input: &[u8]) -> NomResult<()> {
	map(take_while(is_whitespace), |_| ())(input)
}

fn space(input: &[u8]) -> NomResult<()> {
	fold_many0(alt((
		map(take_while1(is_whitespace), |_| ()),
		comment
	)), (),	|_, _| ())(input)
}

fn integer(input: &[u8]) -> NomResult<i64> {
	opt(nom_one_of("+-"))(input)
		.and_then(|(i, sign)| {
			map_res(take_while1(|c: u8| c.is_ascii_digit()),
					|m: &[u8]| {
						let len = sign.map(|_| 1).unwrap_or(0) + m.len();
						i64::from_str(str::from_utf8(&input[..len]).unwrap())
					})(i)
		})
}

fn real(input: &[u8]) -> NomResult<f64> {
	let (i, _) = pair(opt(nom_one_of("+-")), alt((
		map(tuple((take_while1(|c: u8| c.is_ascii_digit()),
				   tag(b"."),
				   take_while(|c: u8| c.is_ascii_digit()))),
			|_| ()),
		map(pair(tag(b"."), take_while1(|c: u8| c.is_ascii_digit())),
			|_| ())
	)))(input)?;

	let float_input = &input[..input.len()-i.len()];
	convert_result(f64::from_str(str::from_utf8(float_input).unwrap()), i, ErrorKind::Digit)
}

fn hex_char(input: &[u8]) -> NomResult<u8> {
	map_res(take_while_m_n(2, 2, |c: u8| c.is_ascii_hexdigit()),
			|x| u8::from_str_radix(str::from_utf8(x).unwrap(), 16)
	)(input)
}

fn oct_char(input: &[u8]) -> NomResult<u8> {
	map_res(take_while_m_n(1, 3, |c: u8| c.is_ascii_hexdigit()),
			// Spec requires us to ignore any overflow.
			|x| u16::from_str_radix(str::from_utf8(x).unwrap(), 8).map(|o| o as u8)
	)(input)
}

fn name(input: &[u8]) -> NomResult<Vec<u8>> {
	preceded(tag(b"/"), many0(alt((
		preceded(tag(b"#"), hex_char),

		map_opt(nom_take(1usize), |c: &[u8]| {
			if c[0] != b'#' && is_regular(c[0]) {
				Some(c[0])
			} else {
				None
			}
		})
	))))(input)
}

fn escape_sequence(input: &[u8]) -> NomResult<Option<u8>> {
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
			// Any end of line in a string literal is treated as a line feed.
			ILS::EOL => output.push(b'\n'),
			ILS::Nested(n) => output.extend_from_slice(n),
		}
	}
}

fn inner_literal_string(input: &[u8]) -> NomResult<Vec<u8>> {
	fold_many0(
		alt((
			map(take_while1(is_direct_literal_string), ILS::Direct),
			map(escape_sequence, ILS::Escape),
			map(eol, |_| ILS::EOL),
			map(nested_literal_string, ILS::Nested),
		)),
		Vec::new(),
		|mut out: Vec<u8>, value| { value.push(&mut out); out }
	)(input)
}

fn nested_literal_string(input: &[u8]) -> NomResult<Vec<u8>> {
	map(contained(tag(b"("), inner_literal_string, tag(b")")),
		|mut content| {
			content.insert(0, b'(');
			content.push(b')');
			content
		})(input)
}

fn literal_string(input: &[u8]) -> NomResult<Vec<u8>> {
	contained(tag(b"("), inner_literal_string, tag(b")"))(input)
}

fn hexadecimal_string(input: &[u8]) -> NomResult<Object> {
	map(contained(tag(b"<"),
				  terminated(many0(preceded(white_space, hex_char)), white_space),
				  tag(b">")),
		|bytes| Object::String(bytes, StringFormat::Hexadecimal))(input)
}

fn boolean(input: &[u8]) -> NomResult<Object> {
	alt((
		map(tag(b"true"), |_| Object::Boolean(true)),
		map(tag(b"false"), |_| Object::Boolean(false))
	))(input)
}

fn null(input: &[u8]) -> NomResult<Object> {
	map(tag(b"null"), |_| Object::Null)(input)
}

fn array(input: &[u8]) -> NomResult<Vec<Object>> {
	contained(pair(tag(b"["), space), many0(_direct_object), tag(b"]"))(input)
}

fn dictionary(input: &[u8]) -> NomResult<Dictionary> {
	contained(pair(tag(b"<<"), space),
			  fold_many0(pair(terminated(name, space), _direct_object),
						 Dictionary::new(),
						 |mut dict, (key, value)| { dict.set(key, value); dict }),
			  tag(b">>"))(input)
}

fn stream<'a>(input: &'a [u8], reader: &Reader) -> NomResult<'a, Object> {
	let (i, dict) = terminated(dictionary, tuple((space, tag(b"stream"), eol)))(input)?;

	if let Some(length) = dict.get(b"Length").and_then(|value|
		if let Some(id) = value.as_reference() {
			reader.get_object(id).and_then(|value| value.as_i64())
		} else {
			value.as_i64()
		}) {

		let (i, data) = terminated(nom_take(length as usize), pair(opt(eol), tag(b"endstream")))(i)?;
		Ok((i, Object::Stream(Stream::new(dict, data.to_vec()))))
	} else {
		// Return position relative to the start of the stream dictionary.
		Ok((i, Object::Stream(Stream::with_position(dict, input.len() - i.len()))))
	}
}

fn unsigned_int<I: FromStr>(input: &[u8]) -> NomResult<I> {
	map_res(take_while1(|c: u8| c.is_ascii_digit()),
			|digits| I::from_str(str::from_utf8(digits).unwrap()))(input)
}

fn object_id(input: &[u8]) -> NomResult<ObjectId> {
	pair(terminated(unsigned_int, space), terminated(unsigned_int, space))(input)
}

fn reference(input: &[u8]) -> NomResult<Object> {
	map(terminated(object_id, tag(b"R")), Object::Reference)(input)
}

fn _direct_objects(input: &[u8]) -> NomResult<Object> {
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

fn _direct_object(input: &[u8]) -> NomResult<Object> {
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
	nom_to_pom(map_res(contained(tag(b"%PDF-"), take_while(|c: u8| !b"\r\n".contains(&c)), pair(eol, many0_count(comment))),
					   |v| str::from_utf8(v).map(Into::into)))
}

fn xref(input: &[u8]) -> NomResult<Xref> {
	let xref_eol = map(alt((tag(b" \r"), tag(b" \n"), tag("\r\n"))), |_| ());
	let xref_entry = pair(separated_pair(unsigned_int, tag(b" "), unsigned_int),
						  contained(tag(b" "), map(nom_one_of("nf"), |k| k == 'n'), xref_eol));

	let xref_section = pair(separated_pair(unsigned_int::<usize>, tag(b" "), unsigned_int::<u32>),
							preceded(pair(opt(tag(b" ")), eol), many0(xref_entry)));

	contained(pair(tag(b"xref"), eol),
			  fold_many1(xref_section, Xref::new(0),
						 |mut xref, ((start, _count), entries)| {
							 for (index, ((offset, generation), is_normal)) in entries.into_iter().enumerate() {
								 if is_normal {
									 xref.insert((start + index) as u32, XrefEntry::Normal { offset, generation });
								 }
							 }
							 xref
						 }),
			  space)(input)
}

fn trailer(input: &[u8]) -> NomResult<Dictionary> {
	contained(pair(tag(b"trailer"), space), dictionary, space)(input)
}

pub fn xref_and_trailer(reader: &Reader) -> Parser<u8, (Xref, Dictionary)> {
	(nom_to_pom(xref) + nom_to_pom(trailer)).map(|(mut xref, trailer)| {
		xref.size = trailer.get(b"Size").and_then(Object::as_i64).expect("Size is absent in trailer.") as u32;
		(xref, trailer)
	}) | indirect_object(reader).convert(|(_, obj)| match obj {
		Object::Stream(stream) => Ok(decode_xref_stream(stream)),
		_ => Err("Xref is not a stream object."),
	})
}

pub fn xref_start<'a>() -> Parser<'a, u8, i64> {
	nom_to_pom(contained(
		pair(tag(b"startxref"), eol),
		integer,
		tuple((eol, tag(b"%%EOF"), space))
	))
}

// The following code create parser to parse content stream.

fn content_space(input: &[u8]) -> NomResult<()> {
	map(take_while(|c| b" \t\r\n".contains(&c)), |_| ())(input)
}

fn operator(input: &[u8]) -> NomResult<String> {
	map_res(take_while1(|c: u8| c.is_ascii_alphabetic() || b"*'\"".contains(&c)),
			|op| str::from_utf8(op).map(Into::into))(input)
}

fn operand(input: &[u8]) -> NomResult<Object> {
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

fn operation(input: &[u8]) -> NomResult<Operation> {
	map(terminated(pair(many0(operand), operator), content_space),
		|(operands, operator)| Operation { operator, operands })(input)
}

fn _content(input: &[u8]) -> NomResult<Content> {
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
