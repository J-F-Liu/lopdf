use super::{
    _direct_object, content_space, eol, is_whitespace, name, space, Dictionary, NomError, NomResult, Object,
    ParserInput, Stream,
};
use crate::Error;
use nom::branch::alt;
use nom::bytes::complete::{tag, take, take_while_m_n};
use nom::combinator::cut;
use nom::combinator::{map, opt};
use nom::error::{ErrorKind, ParseError};
use nom::multi::fold_many0;
use nom::sequence::{pair, preceded, terminated};
use nom::Parser;

fn inner_dictionary(depth: usize) -> impl Fn(ParserInput) -> NomResult<Dictionary> {
    move |input| {
        fold_many0(
            pair(terminated(name, space), _direct_object(depth)),
            Dictionary::new,
            |mut dict, (key, value)| {
                dict.set(key, value);
                dict
            },
        )
        .parse(input)
    }
}

pub(crate) fn inline_image(input: ParserInput) -> NomResult<(Vec<Object>, String)> {
    preceded(pair(tag(&b"BI"[..]), content_space), cut(inline_image_impl)).parse(input)
}

/// The single white-space character between `ID` and the image data (ISO 32000-2, 8.9.7),
/// with a CR LF pair read as one end-of-line. White space after it is image data: a gray
/// level of 32 is a space byte.
fn inline_image_data_separator(input: ParserInput) -> NomResult<()> {
    map(opt(alt((eol, take_while_m_n(1, 1, is_whitespace)))), |_| ()).parse(input)
}

fn inline_image_impl(input: ParserInput) -> NomResult<(Vec<Object>, String)> {
    let (input, stream_dict) = inner_dictionary(crate::reader::MAX_NESTING_DEPTH).parse(input)?;
    let (input, _) = pair(tag(&b"ID"[..]), inline_image_data_separator).parse(input)?;
    match image_data_stream(input, stream_dict) {
        Ok((input, stream)) => {
            let (input, _) = (content_space, tag(&b"EI"[..]), content_space).parse(input)?;
            Ok((input, (vec![Object::Stream(stream)], String::from("BI"))))
        }
        Err(e) => {
            // Skip to EI marker so the rest of the content stream can still be parsed.
            log::warn!("Skipping unparseable inline image: {e}");
            let bytes = input;
            // EI must appear after whitespace to distinguish from data bytes, and be followed
            // by whitespace or the end of the stream.
            let ei_pos = bytes
                .windows(3)
                .enumerate()
                .find(|&(i, w)| {
                    is_whitespace(w[0]) && w[1..] == *b"EI" && bytes.get(i + 3).is_none_or(|&c| is_whitespace(c))
                })
                .map(|(i, _)| i)
                .ok_or_else(|| {
                    let err: NomError = nom::error::Error::from_error_kind(input, ErrorKind::Fail);
                    nom::Err::Failure(err)
                })?;
            let (input, _) = take(ei_pos + 3).parse(input).map_err(|_: nom::Err<()>| {
                let err: NomError = nom::error::Error::from_error_kind(input, ErrorKind::Fail);
                nom::Err::Failure(err)
            })?;
            let (input, _) = content_space(input)?;
            Ok((input, (vec![], String::from("BI"))))
        }
    }
}

fn image_data_stream(input: ParserInput, stream_dict: Dictionary) -> crate::Result<(ParserInput, Stream)> {
    let get_abbr = |key_abbr: &[u8], key: &[u8]| stream_dict.get(key_abbr).or_else(|_| stream_dict.get(key));
    let invalid_size = || Error::InvalidInlineImage(String::from("image size out of range"));
    let get_size = |key_abbr: &[u8], key: &[u8]| -> crate::Result<usize> {
        usize::try_from(get_abbr(key_abbr, key)?.as_i64()?).map_err(|_| invalid_size())
    };
    let width = get_size(b"W", b"Width")?;
    let height = get_size(b"H", b"Height")?;
    let im = get_abbr(b"IM", b"ImageMask").and_then(|x| x.as_bool());
    let bpc = match get_size(b"BPC", b"BitsPerComponent") {
        // An image mask has one bit per sample, so BitsPerComponent is optional for it.
        Err(Error::DictKey(_)) if matches!(im, Ok(true)) => 1,
        bpc => bpc?,
    };
    let num_colors = match im {
        // If we have an image mask then we don't have a colorspace
        Ok(true) => 1,
        _ => match get_abbr(b"CS", b"ColorSpace")? {
            // An Indexed colour space is an array, and each sample is an index into its table.
            Object::Array(array) if matches!(array.first().map(Object::as_name), Some(Ok(b"Indexed" | b"I"))) => 1,
            colorspace => match colorspace.as_name()? {
                b"DeviceGray" | b"G" | b"Gray" => 1,
                b"DeviceRGB" | b"RGB" => 3,
                b"DeviceRGBA" | b"RGBA" => 4,
                b"DeviceCMYK" | b"CMYK" => 4,
                b"Pattern" => {
                    log::warn!("Pattern colorspace is not allowed in inline images");
                    return Err(Error::InvalidInlineImage(String::from(
                        "Pattern colorspace is not allowed in inline images",
                    )));
                }
                _ => {
                    log::warn!("Colorspace of inline image not recognized / not yet implemented");
                    return Err(Error::Unimplemented("inline image colorspaces"));
                }
            },
        },
    };

    let length = bpc
        .checked_mul(num_colors)
        .and_then(|bits_per_pixel| width.checked_mul(bits_per_pixel))
        .and_then(|row_bits| row_bits.div_ceil(8).checked_mul(height))
        .ok_or_else(invalid_size)?;

    let (input, content) = match get_abbr(b"F", b"Filter") {
        Err(_) => {
            // no decompression needed as no filter was applied
            take(length)
                .parse(input)
                .map_err(|_: nom::Err<()>| crate::error::ParseError::EndOfInput)?
        }
        Ok(Object::Name(_filter)) => {
            log::warn!("Filters for inline images are not yet implemented");
            return Err(Error::Unimplemented("filters for inline images"));
        }
        Ok(Object::Array(_filters)) => {
            log::warn!("Filters for inline images are not yet implemented");
            return Err(Error::Unimplemented("filters for inline images"));
        }
        Ok(obj) => {
            log::warn!("Filter must be either a Name or and Array.");
            return Err(Error::ObjectType {
                expected: "Name or Array",
                found: obj.enum_variant(),
            });
        }
    };
    Ok((input, Stream::new(stream_dict, content.to_vec())))
}

#[cfg(test)]
mod tests {
    use super::super::{content, content_strict};
    use super::*;

    fn test_span(s: &'_ [u8]) -> ParserInput<'_> {
        s
    }

    #[test]
    fn inline_image_unknown_colorspace_skipped() {
        // Inline image with an unrecognized colorspace ("ICCBased" is not handled).
        // The parser should skip it and still parse the surrounding operations.
        let input = b"q 100 100 moveto
BI /W 2 /H 2 /CS /ICCBased /BPC 8
ID
\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00
EI
(Hello) Tj Q";
        let out = content(test_span(input)).unwrap();
        // Should have: q, moveto, BI (skipped), Tj, Q = 5 operations
        let ops: Vec<&str> = out.operations.iter().map(|o| o.operator.as_str()).collect();
        assert!(ops.contains(&"q"), "missing q, got: {:?}", ops);
        assert!(ops.contains(&"Tj"), "missing Tj, got: {:?}", ops);
        assert!(ops.contains(&"Q"), "missing Q, got: {:?}", ops);
    }

    #[test]
    fn inline_image_with_invalid_dimensions_skipped() {
        // Negative dimensions or a byte count that doesn't fit in usize must not overflow.
        for dims in [
            &b"/W -1 /H 1 /BPC 8"[..],
            b"/W 4294967296 /H 4294967296 /BPC 8",
            b"/W 1 /H 1 /BPC 9223372036854775807",
        ] {
            let mut input = b"q BI ".to_vec();
            input.extend_from_slice(dims);
            input.extend_from_slice(b" /CS /DeviceRGB ID \x00\x00\x00 EI (Hello) Tj Q");
            let out = content(test_span(&input)).unwrap();
            let ops: Vec<&str> = out.operations.iter().map(|o| o.operator.as_str()).collect();
            assert_eq!(ops, vec!["q", "BI", "Tj", "Q"]);
            assert!(out.operations[1].operands.is_empty());
        }
    }

    #[test]
    fn inline_image_skipped_to_ei_after_any_white_space() {
        // Filters aren't decoded yet, so an image with one is skipped by looking for EI.
        // Any white-space character can delimit EI, and the stream can end right after it.
        for (end, expected) in [
            (&b"\tEI\tQ"[..], &["q", "BI", "Q"][..]),
            (b"\x0cEI\x00Q", &["q", "BI", "Q"]),
            (b" EI", &["q", "BI"]),
        ] {
            let mut input = b"q BI /W 1 /H 1 /BPC 8 /CS /G /F /AHx ID 00>".to_vec();
            input.extend_from_slice(end);
            let out = content_strict(test_span(&input)).unwrap();
            let ops: Vec<&str> = out.operations.iter().map(|o| o.operator.as_str()).collect();
            assert_eq!(ops, expected, "stream: {:?}", String::from_utf8_lossy(&input));
        }
    }

    #[test]
    fn inline_image() {
        let _ = env_logger::try_init();
        let input = b"BI /W 4 /H 4 /CS /RGB /BPC 8
ID
00000z0z00zzz00z0zzz0zzzEI aazazaazzzaazazzzazzz
EI";
        let out = super::inline_image(test_span(input)).unwrap().1;
        assert_eq!(&out.1, "BI");
        assert_eq!(
            &out.0[0].as_stream().unwrap().content,
            b"00000z0z00zzz00z0zzz0zzzEI aazazaazzzaazazzzazzz"
        )
    }

    #[test]
    fn inline_image_abbreviated_color_spaces() {
        // 2x1 images using the abbreviated colour space names allowed in inline images.
        for (color_space, data) in [
            (&b"/G"[..], &b"\x00\xff"[..]),
            (b"/RGB", b"\x00\x00\x00\xff\xff\xff"),
            (b"/CMYK", b"\x00\x00\x00\x00\xff\xff\xff\xff"),
            (b"[/I /RGB 1 <000000FFFFFF>]", b"\x00\x01"),
            (b"[/Indexed /DeviceRGB 1 <000000FFFFFF>]", b"\x00\x01"),
        ] {
            let mut input = b"q BI /W 2 /H 1 /BPC 8 /CS ".to_vec();
            input.extend_from_slice(color_space);
            input.extend_from_slice(b" ID ");
            input.extend_from_slice(data);
            input.extend_from_slice(b" EI Q");
            let out = content(test_span(&input)).unwrap();
            let ops: Vec<&str> = out.operations.iter().map(|o| o.operator.as_str()).collect();
            assert_eq!(ops, vec!["q", "BI", "Q"]);
            let image = out.operations[1].operands[0].as_stream().unwrap();
            assert_eq!(image.content, data);
        }
    }

    #[test]
    fn inline_image_mask_without_bits_per_component() {
        // A 9x2 image mask: one bit per sample, each row padded to two bytes.
        for image_mask in [&b"/IM true"[..], b"/ImageMask true"] {
            let mut input = b"q BI ".to_vec();
            input.extend_from_slice(image_mask);
            input.extend_from_slice(b" /W 9 /H 2 ID \x80\x00\xff\x80 EI Q");
            let out = content(test_span(&input)).unwrap();
            let ops: Vec<&str> = out.operations.iter().map(|o| o.operator.as_str()).collect();
            assert_eq!(ops, vec!["q", "BI", "Q"]);
            let image = out.operations[1].operands[0].as_stream().unwrap();
            assert_eq!(image.content, b"\x80\x00\xff\x80");
        }
    }

    #[test]
    fn inline_image_data_starting_with_white_space() {
        // A 3x1 gray image whose three pixels have level 32, i.e. space bytes. Only the
        // first white-space character after ID separates it from the data.
        let input = b"q\rBI\r/CS/DeviceGray/W 3/H 1/BPC 8\rID\r   \nEI\rQ\r";
        let out = content_strict(test_span(input)).unwrap();
        let ops: Vec<&str> = out.operations.iter().map(|o| o.operator.as_str()).collect();
        assert_eq!(ops, vec!["q", "BI", "Q"]);
        assert_eq!(&out.operations[1].operands[0].as_stream().unwrap().content, b"   ");
    }

    #[test]
    fn inline_image_data_after_crlf() {
        // A CR LF after ID is one end-of-line, so the data starts after the LF.
        let input = b"BI /CS /DeviceGray /W 2 /H 1 /BPC 8 ID\r\n\n\x80\nEI";
        let out = super::inline_image(test_span(input)).unwrap().1;
        assert_eq!(&out.0[0].as_stream().unwrap().content, b"\n\x80");
    }
}
