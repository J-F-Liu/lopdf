use super::{Dictionary, Object, ObjectId, Stream, StringFormat};
use crate::content::*;
use crate::error::XrefError;
use crate::reader::Reader;
use crate::xref::*;
use crate::{Error, Result};
use pom::char_class::{alpha, hex_digit, multispace, oct_digit};
use pom::parser::*;
use std::str::{self, FromStr};

fn eol<'a>() -> Parser<'a, u8, u8> {
    (sym(b'\r') * sym(b'\n')) | sym(b'\n') | sym(b'\r')
}

fn comment<'a>() -> Parser<'a, u8, ()> {
    sym(b'%') * none_of(b"\r\n").repeat(0..) * eol().discard()
}

fn white_space<'a>() -> Parser<'a, u8, ()> {
    one_of(b" \t\n\r\0\x0C").repeat(0..).discard()
}

fn space<'a>() -> Parser<'a, u8, ()> {
    (one_of(b" \t\n\r\0\x0C").repeat(1..).discard() | comment())
        .repeat(0..)
        .discard()
}

fn integer<'a>() -> Parser<'a, u8, i64> {
    let number = one_of(b"+-").opt() + one_of(b"0123456789").repeat(1..);
    number.collect().convert(str::from_utf8).convert(|s| i64::from_str(&s))
}

fn real<'a>() -> Parser<'a, u8, f64> {
    let number = one_of(b"+-").opt()
        + ((one_of(b"0123456789").repeat(1..) * sym(b'.') - one_of(b"0123456789").repeat(0..))
            | (sym(b'.') - one_of(b"0123456789").repeat(1..)));
    number.collect().convert(str::from_utf8).convert(|s| f64::from_str(&s))
}

fn hex_char<'a>() -> Parser<'a, u8, u8> {
    let number = is_a(hex_digit).repeat(2);
    number
        .collect()
        .convert(|v| u8::from_str_radix(str::from_utf8(v).unwrap(), 16))
}

fn oct_char<'a>() -> Parser<'a, u8, u8> {
    let number = is_a(oct_digit).repeat(1..4);
    number
        .collect()
        .convert(|v| u8::from_str_radix(str::from_utf8(v).unwrap(), 8))
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
            | eol().map(|_| vec![])
            | empty().map(|_| vec![]))
}

fn nested_literal_string<'a>(depth: usize) -> Parser<'a, u8, Vec<u8>> {
    if depth == 0 {
        return Parser::new(move |_: &'a [u8], pos: usize| {
            Err(pom::Error::Custom {
                message: "Brackets embedded to deep.".to_string(),
                position: pos,
                inner: None,
            })
        });
    }

    sym(b'(')
        * (none_of(b"\\()").repeat(1..) | escape_sequence() | call(move || nested_literal_string(depth - 1)))
            .repeat(0..)
            .map(|segments| {
                let mut bytes = segments.into_iter().fold(vec![b'('], |mut bytes, mut segment| {
                    bytes.append(&mut segment);
                    bytes
                });
                bytes.push(b')');
                bytes
            })
        - sym(b')')
}

fn literal_string<'a>() -> Parser<'a, u8, Vec<u8>> {
    sym(b'(')
        * (none_of(b"\\()").repeat(1..) | escape_sequence() | nested_literal_string(crate::reader::MAX_BRACKET))
            .repeat(0..)
            .map(|segments| segments.concat())
        - sym(b')')
}

fn hexadecimal_string<'a>() -> Parser<'a, u8, Vec<u8>> {
    sym(b'<') * (white_space() * hex_char()).repeat(0..) - (white_space() * sym(b'>'))
}

fn array<'a>() -> Parser<'a, u8, Vec<Object>> {
    sym(b'[') * space() * call(_direct_object).repeat(0..) - sym(b']')
}

fn dictionary<'a>() -> Parser<'a, u8, Dictionary> {
    let entry = name() - space() + call(_direct_object);
    let entries = seq(b"<<") * space() * entry.repeat(0..) - seq(b">>");
    entries.map(|entries| {
        entries
            .into_iter()
            .fold(Dictionary::new(), |mut dict: Dictionary, (key, value)| {
                dict.set(key, value);
                dict
            })
    })
}

fn stream<'a>(reader: &'a Reader) -> Parser<'a, u8, Stream> {
    (dictionary() - space() - seq(b"stream") - eol())
        >> move |dict: Dictionary| {
            if let Ok(length) = dict.get(b"Length").and_then(|value| {
                if let Ok(id) = value.as_reference() {
                    return reader.get_object(id).and_then(|value| value.as_i64());
                }
                value.as_i64()
            }) {
                let stream = take(length as usize) - eol().opt() - seq(b"endstream").expect("endstream");
                stream.map(move |data| Stream::new(dict.clone(), data.to_vec()))
            } else {
                empty().pos().map(move |pos| Stream::with_position(dict.clone(), pos))
            }
        }
}

fn object_id<'a>() -> Parser<'a, u8, ObjectId> {
    let id = one_of(b"0123456789")
        .repeat(1..)
        .convert(|v| u32::from_str(&str::from_utf8(&v).unwrap()));
    let gen = one_of(b"0123456789")
        .repeat(1..)
        .convert(|v| u16::from_str(&str::from_utf8(&v).unwrap()));
    id - space() + gen - space()
}

pub fn direct_object(input: &[u8]) -> Option<Object> {
    _direct_object().parse(input).ok()
}

fn _direct_object<'a>() -> Parser<'a, u8, Object> {
    (seq(b"null").map(|_| Object::Null)
        | seq(b"true").map(|_| Object::Boolean(true))
        | seq(b"false").map(|_| Object::Boolean(false))
        | (object_id().map(Object::Reference) - sym(b'R'))
        | real().map(Object::Real)
        | integer().map(Object::Integer)
        | name().map(Object::Name)
        | literal_string().map(Object::string_literal)
        | hexadecimal_string().map(|bytes| Object::String(bytes, StringFormat::Hexadecimal))
        | array().map(Object::Array)
        | dictionary().map(Object::Dictionary))
        - space()
}

fn object<'a>(reader: &'a Reader) -> Parser<'a, u8, Object> {
    (seq(b"null").map(|_| Object::Null)
        | seq(b"true").map(|_| Object::Boolean(true))
        | seq(b"false").map(|_| Object::Boolean(false))
        | (object_id().map(Object::Reference) - sym(b'R'))
        | real().map(Object::Real)
        | integer().map(Object::Integer)
        | name().map(Object::Name)
        | literal_string().map(Object::string_literal)
        | hexadecimal_string().map(|bytes| Object::String(bytes, StringFormat::Hexadecimal))
        | array().map(Object::Array)
        | stream(reader).map(Object::Stream)
        | dictionary().map(Object::Dictionary))
        - space()
}

pub fn indirect_object(
    input: &[u8], offset: usize, expected_id: Option<ObjectId>, reader: &Reader,
) -> Result<(ObjectId, Object)> {
    _indirect_object(expected_id, reader)
        .parse_at(input, offset)
        .map(|(out, _)| out)
        .map_err(|_| Error::Parse { offset })
}

fn _indirect_object<'a>(expected_id: Option<ObjectId>, reader: &'a Reader) -> Parser<'a, u8, (ObjectId, Object)> {
    object_id().convert(move |id| match expected_id {
        Some(expected_id) if expected_id == id => Ok(id),
        Some(_) => Err(()),
        None => Ok(id),
    }) - seq(b"obj")
        - space()
        + object(reader)
        - space()
        - seq(b"endobj").opt()
        - space()
}

pub fn header(input: &[u8]) -> Option<String> {
    (seq(b"%PDF-") * none_of(b"\r\n").repeat(0..).convert(String::from_utf8) - eol() - comment().repeat(0..))
        .parse(input)
        .ok()
}

fn xref<'a>() -> Parser<'a, u8, Xref> {
    let xref_entry = integer().map(|i| i as u32) - sym(b' ') + integer().map(|i| i as u16) - sym(b' ')
        + one_of(b"nf").map(|k| k == b'n')
        - take(2);
    let xref_section =
        integer().map(|i| i as usize) - sym(b' ') + integer() - sym(b' ').opt() - eol() + xref_entry.repeat(0..);
    let xref = seq(b"xref") * eol() * xref_section.repeat(1..) - space();
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
    seq(b"trailer") * space() * dictionary() - space()
}

pub fn xref_and_trailer<'a>(input: &'a [u8], reader: &'a Reader) -> Result<(Xref, Dictionary)> {
    _xref_and_trailer(reader)
        .parse(input)
        .map_err(|_| Error::Xref(XrefError::Parse))
}

fn _xref_and_trailer<'a>(reader: &'a Reader) -> Parser<'a, u8, (Xref, Dictionary)> {
    (xref() + trailer()).convert(|(mut xref, trailer)| -> Result<_> {
        xref.size = trailer
            .get(b"Size")
            .and_then(Object::as_i64)
            .map_err(|_| Error::Trailer)? as u32;
        Ok((xref, trailer))
    }) | _indirect_object(None, reader).convert(|(_, obj)| match obj {
        Object::Stream(stream) => decode_xref_stream(stream),
        _ => Err(Error::Xref(XrefError::Parse)),
    })
}

pub fn xref_start(input: &[u8]) -> Option<i64> {
    (seq(b"startxref") * white_space() * integer() - white_space() - seq(b"%%EOF") - space())
        .parse(input)
        .ok()
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
        | integer().map(Object::Integer)
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

pub fn content(input: &[u8]) -> Option<Content<Vec<Operation>>> {
    (content_space() * operation().repeat(0..).map(|operations| Content { operations }))
        .parse(input)
        .ok()
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
        assert_eq!(
            literal_string().parse(b"(text\r\n\\\\(nested\\t\\b\\f))"),
            Ok(b"text\r\n\\(nested\t\x08\x0C)".to_vec())
        );
        assert_eq!(
            literal_string().parse(b"(text\\0\\53\\053\\0053)"),
            Ok(b"text\0++\x053".to_vec())
        );
        assert_eq!(
            literal_string().parse(b"(text line\\\n())"),
            Ok(b"text line()".to_vec())
        );
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
        let content = content(stream);
        println!("{:?}", content);
        assert!(content.is_some());
    }
}
