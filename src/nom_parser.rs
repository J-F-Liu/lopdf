use super::{Dictionary, Object, ObjectId, Reader, Stream, StringFormat};
use crate::content::*;
use crate::error::XrefError;
use crate::xref::*;
use crate::Error;
use std::collections::HashSet;
use std::str::{self, FromStr};

use nom::branch::alt;
use nom::bytes::complete::{tag, take, take_while, take_while1, take_while_m_n};
use nom::character::complete::multispace1;
use nom::character::complete::{digit0, digit1, one_of};
use nom::character::complete::{space0, space1};
use nom::character::{is_hex_digit, is_oct_digit};
use nom::combinator::{map, map_opt, map_res, opt, verify};
use nom::error::{ErrorKind, ParseError};
use nom::multi::{fold_many0, fold_many1, many0, many0_count};
use nom::sequence::{delimited, pair, preceded, separated_pair, terminated, tuple};
use nom::AsBytes;
use nom::IResult;
use nom::Slice;
use nom_locate::LocatedSpan;

pub(crate) type ParserInput<'a> = LocatedSpan<&'a [u8], &'a str>;
// Change this to something else that implements ParseError to get a
// different error type out of nom.
pub(crate) type NomError<'a> = nom::error::Error<ParserInput<'a>>;

pub(crate) type NomResult<'a, O, E = NomError<'a>> = IResult<ParserInput<'a>, O, E>;

#[inline]
fn strip_nom<O>(r: NomResult<O>) -> Option<O> {
    r.ok().map(|(_, o)| o)
}

#[inline]
fn convert_result<O, E>(result: Result<O, E>, input: ParserInput, error_kind: ErrorKind) -> NomResult<O> {
    result.map(|o| (input, o)).map_err(|_| {
        // this is a unit bind if NomError = ()
        #[allow(clippy::let_unit_value)]
        let err: NomError = nom::error::Error::from_error_kind(input, error_kind);
        nom::Err::Error(err)
    })
}

#[inline]
fn offset_stream(object: &mut Object, offset: usize) {
    if let Object::Stream(ref mut stream) = object {
        stream.start_position = stream.start_position.and_then(|sp| sp.checked_add(offset));
    }
}

pub(crate) fn eol(input: ParserInput) -> NomResult<ParserInput> {
    alt((tag(b"\r\n"), tag(b"\n"), tag(b"\r")))(input)
}

pub(crate) fn comment(input: ParserInput) -> NomResult<()> {
    map(
        tuple((tag(b"%"), take_while(|c: u8| !b"\r\n".contains(&c)), eol)),
        |_| (),
    )(input)
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

fn white_space(input: ParserInput) -> NomResult<()> {
    map(take_while(is_whitespace), |_| ())(input)
}

fn space(input: ParserInput) -> NomResult<()> {
    fold_many0(
        alt((map(take_while1(is_whitespace), |_| ()), comment)),
        || {},
        |_, _| (),
    )(input)
}

fn integer(input: ParserInput) -> NomResult<i64> {
    let (i, _) = pair(opt(one_of("+-")), digit1)(input)?;

    let int_input = &input[..input.len() - i.len()];
    convert_result(i64::from_str(str::from_utf8(int_input).unwrap()), i, ErrorKind::Digit)
}

fn real(input: ParserInput) -> NomResult<f32> {
    let (i, _) = pair(
        opt(one_of("+-")),
        alt((
            map(tuple((digit1, tag(b"."), digit0)), |_| ()),
            map(pair(tag(b"."), digit1), |_| ()),
        )),
    )(input)?;

    let float_input = &input[..input.len() - i.len()];
    convert_result(f32::from_str(str::from_utf8(float_input).unwrap()), i, ErrorKind::Digit)
}

pub(crate) fn hex_char(input: ParserInput) -> NomResult<u8> {
    map_res(
        verify(take(2usize), |h: &ParserInput| {
            h.as_bytes().iter().copied().all(is_hex_digit)
        }),
        |x: ParserInput| u8::from_str_radix(str::from_utf8(&x).unwrap(), 16),
    )(input)
}

fn oct_char(input: ParserInput) -> NomResult<u8> {
    map_res(
        take_while_m_n(1, 3, is_oct_digit),
        // Spec requires us to ignore any overflow.
        |x: ParserInput| u16::from_str_radix(str::from_utf8(&x).unwrap(), 8).map(|o| o as u8),
    )(input)
}

pub(crate) fn name(input: ParserInput) -> NomResult<Vec<u8>> {
    preceded(
        tag(b"/"),
        many0(alt((
            preceded(tag(b"#"), hex_char),
            map_opt(take(1usize), |c: ParserInput| {
                if c[0] != b'#' && is_regular(c[0]) {
                    Some(c[0])
                } else {
                    None
                }
            }),
        ))),
    )(input)
}

fn escape_sequence(input: ParserInput) -> NomResult<Option<u8>> {
    preceded(
        tag(b"\\"),
        alt((
            map(oct_char, Some),
            map(eol, |_| None),
            map(tag(b"n"), |_| Some(b'\n')),
            map(tag(b"r"), |_| Some(b'\r')),
            map(tag(b"t"), |_| Some(b'\t')),
            map(tag(b"b"), |_| Some(b'\x08')),
            map(tag(b"f"), |_| Some(b'\x0C')),
            map(take(1usize), |c: ParserInput| Some(c[0])),
        )),
    )(input)
}

enum InnerLiteralString<'a> {
    Direct(ParserInput<'a>),
    Escape(Option<u8>),
    Eol(ParserInput<'a>),
    Nested(Vec<u8>),
}

impl<'a> InnerLiteralString<'a> {
    fn push(&self, output: &mut Vec<u8>) {
        match self {
            InnerLiteralString::Direct(s) | InnerLiteralString::Eol(s) => output.extend_from_slice(s),
            InnerLiteralString::Escape(e) => output.extend(e),
            InnerLiteralString::Nested(n) => output.extend_from_slice(n),
        }
    }
}

fn inner_literal_string(depth: usize) -> impl Fn(ParserInput) -> NomResult<Vec<u8>> {
    move |input| {
        fold_many0(
            alt((
                map(take_while1(is_direct_literal_string), InnerLiteralString::Direct),
                map(escape_sequence, InnerLiteralString::Escape),
                map(eol, InnerLiteralString::Eol),
                map(nested_literal_string(depth), InnerLiteralString::Nested),
            )),
            Vec::new,
            |mut out: Vec<u8>, value| {
                value.push(&mut out);
                out
            },
        )(input)
    }
}

fn nested_literal_string(depth: usize) -> impl Fn(ParserInput) -> NomResult<Vec<u8>> {
    move |input| {
        if depth == 0 {
            map(verify(tag(b"too deep"), |_| false), |_| vec![])(input)
        } else {
            map(
                delimited(tag(b"("), inner_literal_string(depth - 1), tag(b")")),
                |mut content| {
                    content.insert(0, b'(');
                    content.push(b')');
                    content
                },
            )(input)
        }
    }
}

fn literal_string(input: ParserInput) -> NomResult<Vec<u8>> {
    delimited(tag(b"("), inner_literal_string(crate::reader::MAX_BRACKET), tag(b")"))(input)
}

#[inline]
fn hex_digit(input: ParserInput) -> NomResult<u8> {
    map_opt(take(1usize), |c: ParserInput| {
        str::from_utf8(&c).ok().and_then(|c| u8::from_str_radix(c, 16).ok())
    })(input)
}

fn hexadecimal_string(input: ParserInput) -> NomResult<Object> {
    map(
        delimited(
            tag(b"<"),
            terminated(
                fold_many0(
                    preceded(white_space, hex_digit),
                    || -> (Vec<u8>, bool) { (Vec::new(), false) },
                    |state, c| match state {
                        (mut out, false) => {
                            out.push(c << 4);
                            (out, true)
                        }
                        (mut out, true) => {
                            *out.last_mut().unwrap() |= c;
                            (out, false)
                        }
                    },
                ),
                white_space,
            ),
            tag(b">"),
        ),
        |(bytes, _)| Object::String(bytes, StringFormat::Hexadecimal),
    )(input)
}

fn boolean(input: ParserInput) -> NomResult<Object> {
    alt((
        map(tag(b"true"), |_| Object::Boolean(true)),
        map(tag(b"false"), |_| Object::Boolean(false)),
    ))(input)
}

fn null(input: ParserInput) -> NomResult<Object> {
    map(tag(b"null"), |_| Object::Null)(input)
}

fn array(input: ParserInput) -> NomResult<Vec<Object>> {
    delimited(pair(tag(b"["), space), many0(_direct_object), tag(b"]"))(input)
}

pub(crate) fn dictionary(input: ParserInput) -> NomResult<Dictionary> {
    delimited(
        pair(tag(b"<<"), space),
        fold_many0(
            pair(terminated(name, space), _direct_object),
            Dictionary::new,
            |mut dict, (key, value)| {
                dict.set(key, value);
                dict
            },
        ),
        tag(b">>"),
    )(input)
}

pub(crate) fn dict_dup(input: ParserInput) -> NomResult<Dictionary> {
    delimited(
        tuple((
            digit1,
            space1,
            tag(b"dict"),
            space1,
            tag(b"dup"),
            space1,
            tag(b"begin"),
            multispace1,
        )),
        fold_many0(
            terminated(
                pair(terminated(name, space), _direct_object),
                pair(tag(b"def"), multispace1),
            ),
            Dictionary::new,
            |mut dict, (key, value)| {
                dict.set(key, value);
                dict
            },
        ),
        tag(b"end"),
    )(input)
}

fn stream<'a>(input: ParserInput<'a>, reader: &Reader, already_seen: &mut HashSet<ObjectId>) -> NomResult<'a, Object> {
    let (i, dict) = terminated(dictionary, tuple((space, tag(b"stream"), space0, eol)))(input)?;

    if let Ok(length) = dict.get(b"Length").and_then(|value| {
        if let Ok(id) = value.as_reference() {
            reader.get_object(id, already_seen).and_then(|value| value.as_i64())
        } else {
            value.as_i64()
        }
    }) {
        if length < 0 {
            // artificial error kind is created to allow descriptive nom errors
            #[allow(clippy::unit_arg)]
            return Err(nom::Err::Failure(NomError::from_error_kind(i, ErrorKind::LengthValue)));
        }
        let (i, data) = terminated(take(length as usize), pair(opt(eol), tag(b"endstream")))(i)?;
        Ok((i, Object::Stream(Stream::new(dict, data.to_vec()))))
    } else {
        // Return position relative to the start of the stream dictionary.
        Ok((i, Object::Stream(Stream::with_position(dict, input.len() - i.len()))))
    }
}

fn unsigned_int<I: FromStr>(input: ParserInput) -> NomResult<I> {
    map_res(digit1, |digits: ParserInput| {
        I::from_str(str::from_utf8(&digits).unwrap())
    })(input)
}

fn object_id(input: ParserInput) -> NomResult<ObjectId> {
    pair(terminated(unsigned_int, space), terminated(unsigned_int, space))(input)
}

fn reference(input: ParserInput) -> NomResult<Object> {
    map(terminated(object_id, tag(b"R")), Object::Reference)(input)
}

fn _direct_objects(input: ParserInput) -> NomResult<Object> {
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

fn _direct_object(input: ParserInput) -> NomResult<Object> {
    terminated(_direct_objects, space)(input)
}

pub fn direct_object(input: ParserInput) -> Option<Object> {
    strip_nom(_direct_object(input))
}

fn object<'a>(input: ParserInput<'a>, reader: &Reader, already_seen: &mut HashSet<ObjectId>) -> NomResult<'a, Object> {
    terminated(
        alt((|input| stream(input, reader, already_seen), _direct_objects)),
        space,
    )(input)
}

pub fn indirect_object(
    input: ParserInput, offset: usize, expected_id: Option<ObjectId>, reader: &Reader,
    already_seen: &mut HashSet<ObjectId>,
) -> crate::Result<(ObjectId, Object)> {
    let (id, mut object) = _indirect_object(input.slice(offset..), offset, expected_id, reader, already_seen)?;

    offset_stream(&mut object, offset);

    Ok((id, object))
}

fn _indirect_object<'a>(
    input: ParserInput<'a>, offset: usize, expected_id: Option<ObjectId>, reader: &Reader,
    already_seen: &mut HashSet<ObjectId>,
) -> crate::Result<(ObjectId, Object)> {
    let (i, (_, object_id)) =
        terminated(tuple((space, object_id)), pair(tag(b"obj"), space))(input).map_err(|_| Error::Parse { offset })?;
    if let Some(expected_id) = expected_id {
        if object_id != expected_id {
            return Err(crate::error::Error::ObjectIdMismatch);
        }
    }

    let object_offset = input.len() - i.len();
    let (_, mut object) = terminated(
        |i: ParserInput<'a>| object(i, reader, already_seen),
        tuple((space, opt(tag(b"endobj")), space)),
    )(i)
    .map_err(|_| Error::Parse { offset })?;

    offset_stream(&mut object, object_offset);

    Ok((object_id, object))
}

pub fn header(input: ParserInput) -> Option<String> {
    strip_nom(map_res(
        delimited(
            tag(b"%PDF-"),
            take_while(|c: u8| !b"\r\n".contains(&c)),
            pair(eol, many0_count(comment)),
        ),
        |v: ParserInput| str::from_utf8(&v).map(Into::into),
    )(input))
}

/// Decode CrossReferenceTable
fn xref(input: ParserInput) -> NomResult<Xref> {
    let xref_eol = map(alt((tag(b" \r"), tag(b" \n"), tag(b"\r\n"))), |_| ());
    let xref_entry = pair(
        separated_pair(unsigned_int, tag(b" "), unsigned_int::<u32>),
        delimited(tag(b" "), map(one_of("nf"), |k| k == 'n'), xref_eol),
    );

    let xref_section = pair(
        separated_pair(unsigned_int::<usize>, tag(b" "), unsigned_int::<u32>),
        preceded(pair(opt(tag(b" ")), eol), many0(xref_entry)),
    );

    delimited(
        pair(tag(b"xref"), eol),
        fold_many1(
            xref_section,
            || -> Xref { Xref::new(0, XrefType::CrossReferenceTable) },
            |mut xref, ((start, _count), entries)| {
                for (index, ((offset, generation), is_normal)) in entries.into_iter().enumerate() {
                    if is_normal {
                        if let Ok(generation) = generation.try_into() {
                            xref.insert((start + index) as u32, XrefEntry::Normal { offset, generation });
                        }
                    }
                }
                xref
            },
        ),
        space,
    )(input)
}

fn trailer(input: ParserInput) -> NomResult<Dictionary> {
    delimited(pair(tag(b"trailer"), space), dictionary, space)(input)
}

pub fn xref_and_trailer(input: ParserInput, reader: &Reader) -> crate::Result<(Xref, Dictionary)> {
    let xref_trailer = map(pair(xref, trailer), |(mut xref, trailer)| {
        xref.size = trailer
            .get(b"Size")
            .and_then(Object::as_i64)
            .map_err(|_| Error::Trailer)? as u32;
        Ok((xref, trailer))
    });
    alt((
        xref_trailer,
        (|input| {
            _indirect_object(input, 0, None, reader, &mut HashSet::new())
                .map(|(_, obj)| {
                    let res = match obj {
                        Object::Stream(stream) => decode_xref_stream(stream),
                        _ => Err(Error::Xref(XrefError::Parse)),
                    };
                    (input, res)
                })
                .map_err(|_| {
                    // artificial error kind is created to allow descriptive nom errors
                    #[allow(clippy::unit_arg)]
                    nom::Err::Error(NomError::from_error_kind(input, ErrorKind::Fail))
                })
        }),
    ))(input)
    .map(|(_, o)| o)
    .unwrap_or(Err(Error::Trailer))
}

pub fn xref_start(input: ParserInput) -> Option<i64> {
    strip_nom(delimited(
        pair(tag(b"startxref"), eol),
        integer,
        tuple((eol, tag(b"%%EOF"), space)),
    )(input))
}

// The following code create parser to parse content stream.

fn content_space(input: ParserInput) -> NomResult<()> {
    map(take_while(|c| b" \t\r\n".contains(&c)), |_| ())(input)
}

fn operator(input: ParserInput) -> NomResult<String> {
    map_res(
        take_while1(|c: u8| c.is_ascii_alphabetic() || b"*'\"".contains(&c)),
        |op: ParserInput| str::from_utf8(&op).map(Into::into),
    )(input)
}

fn operand(input: ParserInput) -> NomResult<Object> {
    terminated(
        alt((
            null,
            boolean,
            map(real, Object::Real),
            map(integer, Object::Integer),
            map(name, Object::Name),
            map(literal_string, Object::string_literal),
            hexadecimal_string,
            map(array, Object::Array),
            map(dictionary, Object::Dictionary),
        )),
        content_space,
    )(input)
}

fn operation(input: ParserInput) -> NomResult<Operation> {
    map(
        preceded(
            many0(comment),
            terminated(pair(many0(operand), operator), content_space),
        ),
        |(operands, operator)| Operation { operator, operands },
    )(input)
}

fn _content(input: ParserInput) -> NomResult<Content<Vec<Operation>>> {
    preceded(
        content_space,
        map(many0(operation), |operations| Content { operations }),
    )(input)
}

pub fn content(input: ParserInput) -> Option<Content<Vec<Operation>>> {
    strip_nom(_content(input))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_span(s: &[u8]) -> ParserInput {
        LocatedSpan::new_extra(s, "test")
    }

    fn tstrip<O>(r: NomResult<O>) -> Option<O> {
        r.ok().and_then(|(i, o)| if !i.is_empty() { None } else { Some(o) })
    }

    #[test]
    fn parse_real_number() {
        let real = |i| tstrip(real(i));

        assert_eq!(real(test_span(b"0.12")), Some(0.12));
        assert_eq!(real(test_span(b"-.12")), Some(-0.12));
        assert_eq!(real(test_span(b"10.")), Some(10.0));
    }

    #[test]
    fn parse_string() {
        let literal_string = |i| tstrip(literal_string(i));

        let data = vec![
            ("()", ""),
            ("(text())", "text()"),
            ("(text\r\n\\\\(nested\\t\\b\\f))", "text\r\n\\(nested\t\x08\x0C)"),
            ("(text\\0\\53\\053\\0053)", "text\0++\x053"),
            ("(text line\\\n())", "text line()"),
        ];

        for (input, expected) in data {
            assert_eq!(
                literal_string(test_span(input.as_bytes())),
                Some(expected.as_bytes().to_vec()),
                "input: {:?} output: {:?}",
                input,
                expected,
            );
        }
    }

    #[test]
    fn parse_name() {
        let (text, expected) = (b"/ABC#5f", b"ABC\x5F");
        let result = tstrip(name(test_span(text)));
        assert_eq!(result, Some(expected.to_vec()));

        let (text, expected) = (b"/#cb#ce#cc#e5", b"\xcb\xce\xcc\xe5");
        let result = tstrip(name(test_span(text)));
        assert_eq!(result, Some(expected.to_vec()));
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
        let content = tstrip(_content(test_span(stream)));
        println!("{:?}", content);
        assert!(content.is_some());
    }

    #[test]
    fn hex_partial() {
        // Example from PDF specification.
        let out = tstrip(hexadecimal_string(test_span(b"<901FA>")));

        match out {
            Some(Object::String(s, _)) => assert_eq!(s, b"\x90\x1F\xA0".to_vec()),
            _ => panic!("unexpected {:?}", out),
        }
    }

    #[test]
    fn hex_separated() {
        let out = tstrip(hexadecimal_string(test_span(b"<9 01F A>")));

        match out {
            Some(Object::String(s, _)) => assert_eq!(s, b"\x90\x1F\xA0".to_vec()),
            _ => panic!("unexpected {:?}", out),
        }
    }

    #[test]
    fn big_generation_value() {
        let input = b"xref
0 1
0000000000 65536 f 
0 16
0000000000 65535 f 
0000153238 00000 n 
0000000019 00000 n 
0000000313 00000 n 
0000000333 00000 n 
0000145531 00000 n 
0000153407 00000 n 
0000145554 00000 n 
0000152303 00000 n 
0000152324 00000 n 
0000152514 00000 n 
0000152880 00000 n 
0000153106 00000 n 
0000153139 00000 n 
0000153532 00000 n 
0000153629 00000 n 
trailer
<</Size 16/Root 14 0 R
/Info 15 0 R
/ID [ <9DDC4B621B3F485FF5ED0F57D00A028F>
<9DDC4B621B3F485FF5ED0F57D00A028F> ]
/DocChecksum /2BCC3C7DE26E6BF3573E4A6E8362221F
>>
startxref
153804
%%EOF
";
        match xref(test_span(input)) {
            Ok((_, re)) => assert_eq!(re.entries.len(), 15),
            Err(err) => panic!("unexpected {:?}", err),
        }
    }

    #[test]
    fn content_with_comments() {
        // It should be processed as usual but ignoring the comments
        let input = b"0.5 0.5 0.5 setrgbcolor
% This is a comment
100 100 moveto
(Hello, world!) show
% Another comment
";
        let out = content(test_span(input)).unwrap();
        assert_eq!(out.operations.len(), 3);
    }
}
