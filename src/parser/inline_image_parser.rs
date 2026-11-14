//! Inline Images Parser
//!
//! The essential part here covers the detection of the image data, since in ISO 32000-1 8.9.7
//! "Inline Images" no length entry is provided. This module handles the inline image operations
//! BI, ID and EI as one BI operator together with the dictionary and un-decoded stream.
use super::{
    content_space, eol, inner_dictionary, is_whitespace, Dictionary, NomError, NomResult, Object, ParserInput, Stream,
};
use crate::{Error, Result};
use nom::branch::alt;
use nom::bytes::complete::{tag, take, take_while_m_n};
use nom::combinator::cut;
use nom::combinator::{map, opt};
use nom::error::{ErrorKind, ParseError};
use nom::sequence::{pair, preceded};
use nom::Parser;

pub(crate) fn inline_image_parser(input: ParserInput) -> NomResult<(Vec<Object>, String)> {
    preceded(pair(tag(&b"BI"[..]), content_space), cut(inline_image)).parse(input)
}

fn dict_get_with_abbr<'a>(dict: &'a Dictionary, key_abbr: &[u8], key: &[u8]) -> Option<&'a Object> {
    dict.get(key_abbr).or_else(|_| dict.get(key)).ok()
}

fn inline_image(input: ParserInput) -> NomResult<(Vec<Object>, String)> {
    let (input, dict) = inner_dictionary(crate::reader::MAX_NESTING_DEPTH).parse(input)?;
    let (input, _) = pair(tag(&b"ID"[..]), inline_image_data_separator).parse(input)?;
    match data_stream(input, dict) {
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

// Resolves the data stream between ID and EI.
fn data_stream(input: ParserInput, dict: Dictionary) -> Result<(ParserInput, Stream)> {
    let filter = match dict_get_with_abbr(&dict, b"F", b"Filter") {
        None => None,
        Some(Object::Name(filter)) => Some(filter.as_slice()),
        Some(Object::Array(filters)) => match filters.first() {
            // An empty array applies no filters (ISO 32000-1, Table 5).
            None => None,
            Some(filter) => Some(filter.as_name()?),
        },
        Some(obj) => {
            log::warn!("Filter must be either a Name or an Array.");
            return Err(Error::ObjectType {
                expected: "Name or Array",
                found: obj.enum_variant(),
            });
        }
    };

    let length = match filter {
        // No filter provided
        None => unfiltered_length(&dict),
        Some(_) => {
            log::warn!("Filters for inline images are not yet implemented");
            return Err(Error::Unimplemented("filters for inline images"));
        }
    }?;

    let (input, content) = take(length)
        .parse(input)
        .map_err(|_: nom::Err<()>| crate::error::ParseError::EndOfInput)?;
    Ok((input, Stream::new(dict, content.to_vec())))
}

/// The single white-space character between `ID` and the image data (ISO 32000-2, 8.9.7),
/// with a CR LF pair read as one end-of-line. White space after it is image data: a gray
/// level of 32 is a space byte.
fn inline_image_data_separator(input: ParserInput) -> NomResult<()> {
    map(opt(alt((eol, take_while_m_n(1, 1, is_whitespace)))), |_| ()).parse(input)
}

fn unfiltered_length(dict: &Dictionary) -> Result<usize> {
    let dict_get_size = |abbr: &[u8], key: &[u8]| -> Result<usize> {
        usize::try_from(
            dict_get_with_abbr(dict, abbr, key)
                .ok_or_else(|| Error::InvalidInlineImage(String::from("dimension entry missing in inline image")))?
                .as_i64()?,
        )
        .map_err(|_| Error::InvalidInlineImage(String::from("dimension is negative in inline image")))
    };

    // See ISO 32000-1 Table 89 for key value definitions of image dictionary entries.
    // Width and height are required
    let width = dict_get_size(b"W", b"Width")?;
    let height = dict_get_size(b"H", b"Height")?;

    // Optional boolean with default value false.
    let is_mask = match dict_get_with_abbr(dict, b"IM", b"ImageMask") {
        None => false,
        Some(Object::Boolean(is_mask)) => *is_mask,
        Some(_) => {
            return Err(Error::InvalidInlineImage(String::from(
                "Image mask of an inline image must be a boolean",
            )));
        }
    };

    let bits_per_component = match dict_get_with_abbr(dict, b"BPC", b"BitsPerComponent") {
        None if is_mask => 1,
        Some(obj) => usize::try_from(obj.as_i64()?)?,
        _ => {
            log::warn!("Bits per component is not provided in inline image.");
            return Err(Error::InvalidInlineImage(String::from(
                "Bits per component is not provided in inline image",
            )));
        }
    };

    let components = if is_mask {
        1
    } else {
        match dict_get_with_abbr(dict, b"CS", b"ColorSpace") {
            Some(Object::Array(array)) if matches!(array.first().map(Object::as_name), Some(Ok(b"Indexed" | b"I"))) => {
                // No hard error if base colorspace, hival or lookup type is not ISO spec conform.
                1
            }
            Some(Object::Name(name)) => match name.as_slice() {
                b"DeviceGray" | b"G" => 1,
                b"DeviceRGB" | b"RGB" => 3,
                b"DeviceCMYK" | b"CMYK" => 4,
                b"Pattern" => {
                    log::warn!("Pattern colorspace is not allowed in inline images");
                    return Err(Error::InvalidInlineImage(String::from(
                        "Pattern colorspace is not allowed in inline images",
                    )));
                }
                _name => {
                    log::warn!(
                        "Colorspace name lookup in resource dictionary for inline images is not yet implemented"
                    );
                    return Err(Error::Unimplemented(
                        "Colorspace name lookup in resource dictionary for inline images is not yet implemented",
                    ));
                }
            },
            _ => {
                return Err(Error::InvalidInlineImage(String::from(
                    "Missing colorspace in inline image",
                )))
            }
        }
    };

    bits_per_component
        .checked_mul(components)
        .and_then(|bits_per_pixel| bits_per_pixel.checked_mul(width))
        .and_then(|bits_per_row| bits_per_row.div_ceil(8).checked_mul(height))
        .ok_or_else(|| Error::InvalidInlineImage(String::from("image size out of range")))
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
        let out = super::inline_image_parser(test_span(input)).unwrap().1;
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
        let out = super::inline_image_parser(test_span(input)).unwrap().1;
        assert_eq!(&out.0[0].as_stream().unwrap().content, b"\n\x80");
    }
}
