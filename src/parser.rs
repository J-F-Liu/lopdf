use nom::{IResult, ErrorKind, Needed, digit, not_line_ending, is_hex_digit, is_oct_digit};
use std::collections::BTreeMap;
use std::str::{self, FromStr};
use super::{Document, Object, ObjectId, Dictionary, Stream, StringFormat};

fn hex_digit(input: &[u8]) -> IResult<&[u8], u8> {
	if input.is_empty() {
		IResult::Incomplete(Needed::Size(1))
	} else if is_hex_digit(input[0]) {
		IResult::Done(&input[1..], input[0])
	} else {
		IResult::Error(error_position!(ErrorKind::Custom(0), input))
	}
}

fn oct_digit(input: &[u8]) -> IResult<&[u8], u8> {
	if input.is_empty() {
		IResult::Incomplete(Needed::Size(1))
	} else if is_oct_digit(input[0]) {
		IResult::Done(&input[1..], input[0])
	} else {
		IResult::Error(error_position!(ErrorKind::Custom(0), input))
	}
}

fn regular_name_char(input: &[u8]) -> IResult<&[u8], u8> {
	if input.is_empty() {
		IResult::Incomplete(Needed::Size(1))
	} else if b" \t\n\r\x0C()<>[]{}/%#".contains(&input[0]) {
		IResult::Error(error_position!(ErrorKind::Custom(0), input))
	} else {
		IResult::Done(&input[1..], input[0])
	}
}

named!(eol, alt!(tag!("\r\n") | tag!("\n") | tag!("\r")));

named!(comment<&[u8], ()>, do_parse!(tag!("%")>>not_line_ending>>eol>>()));

named!(white_space_or_comment<&[u8], ()>, alt!(
	is_a!(" \t\n\r\0\x0C") => {|_|()}
	| comment
));

named!(space<&[u8], ()>, do_parse!(
	many1!(white_space_or_comment) >> ()
));

named!(sign<&[u8], i64>, alt!(
	tag!("+") => {|_|1}
	| tag!("-")=> {|_|-1}
));

named!(integer<&[u8], i64>, map!(
	pair!(
		opt!(sign),
		map_res!(map_res!(digit, str::from_utf8), i64::from_str)
	),
	|(sign, value): (Option<i64>, i64)| { sign.unwrap_or(1) * value }
));

named!(real<&[u8], f64>, map!(
	pair!(
		opt!(sign),
		map_res!(map_res!(
			recognize!(
				alt!(
					delimited!(digit, tag!("."), opt!(complete!(digit))) |
					preceded!(tag!("."), digit)
				)
			),
		str::from_utf8), f64::from_str)
	),
	|(sign, value): (Option<i64>, f64)| {
		(sign.unwrap_or(1) as f64) * value
	}
));

named!(hex_char<&[u8], u8>, map_res!(
	do_parse!(
		tag!("#") >>
		b1: hex_digit >>
		b2: hex_digit >>
		(b1, b2)
	), |(b1, b2)| {
		u8::from_str_radix(&format!("{}{}", b1 as char, b2 as char), 16)
	}
));

named!(oct_char<&[u8], u8>, map_res!(
	many_m_n!(1, 3, oct_digit), |bytes:Vec<u8>| {
		u8::from_str_radix(&String::from_utf8(bytes).unwrap(), 8)
	}
));

named!(name<&[u8], String>, do_parse!(
	tag!("/") >>
	bytes: fold_many0!(
		alt!(
			regular_name_char
			| hex_char
		),
		vec![],
		|mut acc: Vec<_>, item| { acc.push(item); acc }
	) >>
	(String::from_utf8(bytes).unwrap())
));

named!(escape_sequence<&[u8], Vec<u8>>, do_parse!(
	tag!("\\") >>
	bytes: alt!(
	  tag!("\\")       => { |_| vec![b'\\'] }
	| tag!("(")        => { |_| vec![b'('] }
	| tag!(")")        => { |_| vec![b')'] }
	| tag!("n")        => { |_| vec![b'\n'] }
	| tag!("r")        => { |_| vec![b'\r'] }
	| tag!("t")        => { |_| vec![b'\t'] }
	| tag!("b")        => { |_| vec![b'\x08'] }
	| tag!("f")        => { |_| vec![b'\x0C'] }
	| oct_char         => { |c| vec![c] }
	| eol              => { |_| vec![] }
	) >>
	(bytes)
));

named!(regular_chars<&[u8], Vec<u8>>, do_parse!(
	bytes: is_not!("\\()") >>
	(bytes.to_vec())
));

named!(nested_literal_string<&[u8], Vec<u8>>, map!(
	do_parse!(
		tag!("(") >>
		bytes: fold_many0!(
			alt!(
				regular_chars
				| escape_sequence
				| nested_literal_string
			),
			vec![b'('],
			|mut acc: Vec<_>, mut item| { acc.append(&mut item); acc }
		) >>
		tag!(")") >>
		(bytes)
	), |mut bytes: Vec<u8>| {
		bytes.push(b')');
		bytes
	}
));

named!(literal_string<&[u8], Vec<u8>>, do_parse!(
	tag!("(") >>
	bytes: fold_many0!(
		alt!(
			regular_chars
			| escape_sequence
			| nested_literal_string
		),
		Vec::new(),
		|mut acc: Vec<_>, mut item| { acc.append(&mut item); acc }
	) >>
	tag!(")") >>
	(bytes)
));

named!(hexadecimal_string<&[u8], Vec<u8>>, do_parse!(
	tag!("<") >>
	bytes: fold_many0!(
		map_res!(pair!(
			hex_digit,
			hex_digit
		), |(b1, b2)| {
			u8::from_str_radix(&format!("{}{}", b1 as char, b2 as char), 16)
		}),
		Vec::new(),
		|mut acc: Vec<_>, item| { acc.push(item); acc }
	) >>
	tag!(">") >>
	(bytes)
));

named!(array<Vec<Object>>, do_parse!(
	tag!("[") >>
	opt!(space) >>
	objects: separated_list!(space, object) >>
	opt!(space) >>
	tag!("]") >>
	(objects)
));

named!(dictionary<Dictionary>, do_parse!(
	tag!("<<") >>
	opt!(space) >>
	dict: fold_many0!(
		do_parse!(
			key: name >>
			opt!(space) >>
			value: object >>
			opt!(space) >>
			(key, value)
		),
		Dictionary::new(),
		|mut acc: Dictionary, (key, value)| { acc.set(key, value); acc }
	) >>
	tag!(">>") >>
	(dict)
));

named!(dictionary_or_stream<Object>, do_parse!(
	dict: dictionary >>
	stream: opt!(do_parse!(
		opt!(space) >>
		tag!("stream") >>
		eol >>
		data: take!(dict.get("Length").and_then(|value|value.as_i64()).unwrap() as usize) >>
		opt!(eol) >>
		tag!("endstream") >>
		opt!(eol) >>
		(data.to_vec())
	)) >>
	// (stream.map_or(Object::Dictionary(dict), |data|Object::Stream(Stream::new(dict, data))))
	(match stream {
		None => Object::Dictionary(dict),
		Some(data) => Object::Stream(Stream::new(dict, data))
	})
));

named!(object_id<&[u8], ObjectId>, do_parse!(
	id: map_res!(map_res!(digit, str::from_utf8), u32::from_str) >>
	space >>
	gen: map_res!(map_res!(digit, str::from_utf8), u16::from_str) >>
	space >>
	(id, gen)
));

named!(object<&[u8], Object>, alt!(
	tag!("null") => {|_| Object::Null }
	| tag!("false") => {|_| Object::Boolean(false) }
	| tag!("true") => {|_| Object::Boolean(true) }
	| do_parse!(id: object_id >> tag!("R") >> (id)) => {|id| Object::Reference(id) }
	| real => {|num| Object::Real(num) }
	| integer => {|num| Object::Integer(num) }
	| literal_string => {|bytes| Object::String(bytes, StringFormat::Literal) }
	| hexadecimal_string => {|bytes| Object::String(bytes, StringFormat::Hexadecimal) }
	| name => {|text| Object::Name(text) }
	| array => {|items| Object::Array(items) }
	| dictionary_or_stream => {|dict_or_stream| dict_or_stream }
));

named!(pub indirect_object<&[u8], (ObjectId, Object)>, do_parse!(
	id: object_id >>
	tag!("obj") >>
	opt!(space) >>
	object: object >>
	opt!(space) >>
	tag!("endobj") >>
	space >>
	(id, object)
));

named!(pub header<&[u8], String>, do_parse!(
	tag!("%PDF-") >>
	version: map_res!(not_line_ending, str::from_utf8) >>
	eol >>
	many0!(comment) >>
	(version.to_string())
));

named!(xref_entry<&[u8], (u64, u16, bool)>, do_parse!(
	offset: integer >>
	tag!(" ") >>
	generation: integer >>
	tag!(" ") >>
	kind: alt!(tag!("n") | tag!("f")) >>
	alt!(tag!("\r\n") | tag!(" \n") | tag!(" \r")) >>
	(offset as u64, generation as u16, kind[0] == b'n')
));

named!(pub xref<&[u8], BTreeMap<u32, (u16, u64)>>, do_parse!(
	tag!("xref") >>
	eol >>
	table: fold_many1!(
		do_parse!(
			start: integer >>
			tag!(" ") >>
			count: integer >>
			eol >>
			entries: many1!(xref_entry) >>
			(start as usize, count, entries)
		),
		BTreeMap::new(),
		|mut acc: BTreeMap<_, _>, (start, count, entries): (usize, i64, Vec<(u64, u16, bool)>)| {
			for (index, (offset, generation, is_normal)) in entries.into_iter().enumerate() {
				if is_normal {
					acc.insert((start + index) as u32, (generation, offset));
				}
			}
			acc
		}
	) >>
	opt!(space) >>
	(table)
));

named!(pub trailer<Dictionary>, do_parse!(
	tag!("trailer") >>
	opt!(space) >>
	dict: dictionary >>
	opt!(space) >>
	(dict)
));

named!(pub xref_start<i64>, do_parse!(
	tag!("startxref") >>
	eol >>
	offset: integer >>
	eol >>
	tag!("%%EOF") >>
	opt!(space) >>
	(offset)
));

// named!(pub document<Document>, map!(
// 	tuple!(
// 		header,
// 		many0!(indirect_object),
// 		xref,
// 		trailer,
// 		xref_start
// 	),
// 	|(version, objects, xref, trailer, xref_start)| {
// 		let mut doc = Document::new();
// 		doc.version = version;
// 		for (id, object) in objects {
// 			doc.objects.insert(id, object);
// 		}
// 		doc.reference_table = xref;
// 		doc.trailer = trailer;
// 		doc.max_id = doc.trailer.get("Size").and_then(|value|value.as_i64()).unwrap() as u32 - 1;
// 		doc
// 	}
// ));

#[test]
fn parse_real_number() {
	let r0 = real(&b"0.12"[..]);
	assert_eq!(r0, IResult::Done(&b""[..], 0.12));
	let r1 = real(&b"-.12"[..]);
	assert_eq!(r1, IResult::Done(&b""[..], -0.12));
	let r2 = real(&b"10."[..]);
	assert_eq!(r2, IResult::Done(&b""[..], f64::from_str("10.").unwrap()));
}

#[test]
fn parse_string() {
	assert_eq!(
		literal_string(&b"()"[..]),
		IResult::Done(&b""[..], b"".to_vec()));
	assert_eq!(literal_string(
		&b"(text())"[..]),
		IResult::Done(&b""[..], b"text()".to_vec()));
	assert_eq!(literal_string(
		&b"(text\r\n\\\\(nested\\t\\b\\f))"[..]),
		IResult::Done(&b""[..], b"text\r\n\\(nested\t\x08\x0C)".to_vec()));
	assert_eq!(literal_string(
		&b"(text\\0\\53\\053\\0053)"[..]),
		IResult::Done(&b""[..], b"text\0++\x053".to_vec()));
	assert_eq!(literal_string(
		&b"(text line\\\n())"[..]),
		IResult::Done(&b""[..], b"text line()".to_vec()));
	assert_eq!(name(&b"/ABC#5f"[..]), IResult::Done(&b""[..], "ABC\x5F".to_string()));
}
