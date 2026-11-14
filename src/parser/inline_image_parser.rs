//! Inline Images Parser
//!
//! The essential part here covers the detection of the image data, since in ISO 32000-1 8.9.7
//! "Inline Images" no length entry is provided. This module handles the inline image operations
//! BI, ID and EI as one BI operator together with the dictionary and un-decoded stream.
use super::{content_space, eol, inner_dictionary, is_whitespace, Dictionary, NomResult, Object, ParserInput, Stream};
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
        Ok((after_input, mut stream)) => {
            if let Some(rest) = (content_space, tag(&b"EI"[..]), content_space)
                .parse(after_input)
                .ok()
                .map(|(rest, _)| rest)
            {
                return Ok((rest, (vec![Object::Stream(stream)], String::from("BI"))));
            }
            // If EI does not follow the data, this searches the end of the inline image.
            if let Some((missing_rest, rest)) = search_end_of_inline_image(after_input) {
                log::warn!("Inline image data longer than computed withelp of the dict, adding the missing rest.");
                let mut content = std::mem::take(&mut stream.content);
                content.extend_from_slice(&after_input[..missing_rest]);
                stream.set_content(content);
                return Ok((rest, (vec![Object::Stream(stream)], String::from("BI"))));
            }
            log::warn!("Inline image data shorter than resolved by its dictionary");
            skip_stream(input)
        }
        Err(e) => {
            log::warn!("Skip inline image: {e}");
            skip_stream(input)
        }
    }
}

fn skip_stream(input: ParserInput) -> NomResult<(Vec<Object>, String)> {
    let (_, rest) = search_end_of_inline_image(input)
        .ok_or_else(|| nom::Err::Failure(nom::error::Error::from_error_kind(input, ErrorKind::Fail)))?;
    Ok((rest, (vec![], String::from("BI"))))
}

// EI surrounded by whitespace
fn search_end_of_inline_image(input: ParserInput) -> Option<(usize, ParserInput)> {
    let i = input.windows(3).enumerate().position(|(i, w)| {
        is_whitespace(w[0]) && w[1..] == *b"EI" && input.get(i + 3).is_none_or(|&c| is_whitespace(c))
    })?;
    let (rest, _) = content_space(&input[i + 3..]).ok()?;
    Some((i, rest))
}

// Resolves the data stream between ID and EI.
fn data_stream(input: ParserInput, dict: Dictionary) -> Result<(ParserInput, Stream)> {
    let filter = match dict_get_with_abbr(&dict, b"F", b"Filter") {
        None => None,
        Some(Object::Name(filter)) => Some(filter.as_slice()),
        Some(Object::Array(filters)) => match filters.first() {
            // An empty array applies no filters.
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

    let decode_parms = |index: usize| match dict_get_with_abbr(&dict, b"DP", b"DecodeParms") {
        Some(Object::Dictionary(parms)) if index == 0 => Some(parms),
        Some(Object::Array(parms)) => parms.get(index).and_then(|p| p.as_dict().ok()),
        _ => None,
    };

    let length = match filter {
        // No filter provided
        None => unfiltered_length(&dict),
        Some(filter) => match filter {
            b"ASCIIHexDecode" | b"AHx" => ascii_hex_length(input),
            b"ASCII85Decode" | b"A85" => ascii85_length(input),
            b"LZWDecode" | b"LZW" => lzw_length(input, decode_parms(0)),
            b"FlateDecode" | b"Fl" => flate_length(input),
            b"RunLengthDecode" | b"RL" => run_length_decode_length(input),
            _ => {
                log::warn!("No all filters for inline images are implemented");
                return Err(Error::Unimplemented(
                    "now all filters for inline images are implemented",
                ));
            }
        },
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

// ASCII Hex Decode Filter ISO 32000-1 7.4.2 with EOD '>'
fn ascii_hex_length(input: &[u8]) -> Result<usize> {
    input
        .iter()
        .position(|&c| c == b'>')
        .map(|i| i + 1)
        .ok_or_else(|| Error::InvalidInlineImage(String::from("ASCIIHex inline image data has no end-of-data marker")))
}

// ASCII85 Decode Filter ISO 32000-1, 7.4.3 with EOD '~>'. White-space characters are ignored.
fn ascii85_length(input: &[u8]) -> Result<usize> {
    let err = || Error::InvalidInlineImage(String::from("ASCII85 inline image data has no end-of-data marker"));
    let tilde = input.iter().position(|&c| c == b'~').ok_or_else(err)?;
    let tilde_rest = tilde + 1 + input[tilde + 1..].iter().take_while(|&&c| is_whitespace(c)).count();
    (input.get(tilde_rest) == Some(&b'>'))
        .then_some(tilde_rest + 1)
        .ok_or_else(err)
}

/// LZW Decode Filter ISO 32000-1 7.4.4 with EOD 257.
fn lzw_length(data: &[u8], decode_parms: Option<&Dictionary>) -> Result<usize> {
    use weezl::{decode::Decoder, BitOrder, LzwStatus};

    let err = Error::InvalidInlineImage(String::from("LZW inline image data length could not be evaluated"));

    let early_change = decode_parms
        .and_then(|p| p.get(b"EarlyChange").ok())
        .and_then(|p| p.as_i64().ok())
        .is_none_or(|v| v != 0);

    let mut decoder = if early_change {
        Decoder::with_tiff_size_switch(BitOrder::Msb, 8)
    } else {
        Decoder::new(BitOrder::Msb, 8)
    };

    let mut sink = [0u8; 8192];
    let mut produced = 0u64;
    for i in 0..data.len() {
        let mut input = &data[i..=i];
        loop {
            let result = decoder.decode_bytes(input, &mut sink);

            input = &input[result.consumed_in..];
            produced += result.consumed_out as u64;

            // In inline images section in ISO 32000-1 it stands, that inline images should be < 4KB.
            // So this limit is far away from that.
            if produced > crate::DEFAULT_MAX_DECOMPRESSED_SIZE as u64 {
                return Err(err);
            }

            match result
                .status
                .map_err(|_| Error::InvalidInlineImage(String::from("Lzw status in inline image has an error")))?
            {
                LzwStatus::Done => return Ok(i + 1),
                LzwStatus::Ok if result.consumed_out == sink.len() => continue,
                _ if input.is_empty() => break,
                LzwStatus::NoProgress => return Err(err),
                LzwStatus::Ok => {}
            }
        }
    }
    Err(err)
}

/// Flate Decode Filter ISO 32000-1 7.4.4
fn flate_length(data: &[u8]) -> Result<usize> {
    use flate2::{Decompress, FlushDecompress, Status};

    let mut decompress = Decompress::new(true);
    let mut sink = [0u8; 8192];

    loop {
        let consumed = usize::try_from(decompress.total_in())
            .map_err(|_| Error::InvalidInlineImage(String::from("Flate decode decompress total in is negative")))?;
        let produced = decompress.total_out();

        let status = decompress
            .decompress(
                data.get(consumed..).ok_or_else(|| {
                    Error::InvalidInlineImage(String::from("Flate decode data could not be consumed"))
                })?,
                &mut sink,
                FlushDecompress::None,
            )
            .map_err(|_| Error::InvalidInlineImage(String::from("Flate decode image data can not be parsed.")))?;

        match status {
            Status::StreamEnd => {
                return usize::try_from(decompress.total_in())
                    .map_err(|_| Error::InvalidInlineImage(String::from("Flate decode decompress")))
            }
            _ if decompress.total_out() > crate::DEFAULT_MAX_DECOMPRESSED_SIZE as u64 => {
                return Err(Error::InvalidInlineImage(String::from(
                    "Flate decode image data length could not be resolved.",
                )))
            }
            _ if decompress.total_in() as usize == consumed && decompress.total_out() == produced => {
                return Err(Error::InvalidInlineImage(String::from(
                    "Flate decode image data length could not be resolved.",
                )))
            }
            _ => {}
        }
    }
}

/// RunLengthDecode Filter ISO 32000-1 7.4.5
fn run_length_decode_length(input: &[u8]) -> Result<usize> {
    let mut i = 0usize;
    loop {
        let length = *input.get(i).ok_or_else(|| {
            Error::InvalidInlineImage(String::from(
                "Run length decode input entry not found in inline image data",
            ))
        })?;
        i += 1;
        match length {
            0..=127 => {
                i = i.checked_add(usize::from(length) + 1).ok_or_else(|| {
                    Error::InvalidInlineImage(String::from(
                        "Run length decode inline image data could not be resolved",
                    ))
                })?
            }
            128 => return Ok(i),
            _ => i += 1,
        }
        if i > input.len() {
            return Err(Error::InvalidInlineImage(String::from(
                "Run length decode inline image data could not be resolved",
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{content, content_strict};
    use super::*;

    fn test_span(s: &'_ [u8]) -> ParserInput<'_> {
        s
    }

    fn zlib(data: &[u8], level: flate2::Compression) -> Vec<u8> {
        use std::io::Write;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), level);
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn inline_image_content(dict: &str, data: &[u8], after: &[u8]) -> Vec<u8> {
        let mut input = format!("q BI {dict} ID ").into_bytes();
        input.extend_from_slice(data);
        input.extend_from_slice(after);
        input
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
            let mut input = b"q BI /W 1 /H 1 /BPC 8 /CS /G /F /CCF ID 00>".to_vec();
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

    #[test]
    fn all_inline_image_bytes_before_ei_kept_as_data() {
        // A 2x1 gray image with one byte more than the data length resolvers calculates.
        // It should not fail, instead it should return the data.
        let input = b"q BI /W 2 /H 1 /BPC 8 /CS /G ID \x00\xff\x7f EI Q";
        let out = content_strict(test_span(input)).unwrap();
        let ops: Vec<&str> = out.operations.iter().map(|o| o.operator.as_str()).collect();
        assert_eq!(ops, vec!["q", "BI", "Q"]);
        let image = out.operations[1].operands[0].as_stream().unwrap();
        assert_eq!(image.content, b"\x00\xff\x7f");
        assert_eq!(image.dict.get(b"Length").unwrap().as_i64().unwrap(), 3);
    }

    #[test]
    fn inline_image_without_ei_fails() {
        let input = b"q BI /W 2 /H 1 /BPC 8 /CS /G ID \x00\xff\x7f Q";
        assert!(content_strict(test_span(input)).is_err());
    }

    #[test]
    fn ascii_hex_and_ascii85_length() {
        assert_eq!(
            ascii_hex_length(&[104, 101, 108, 108, 111, 32, 119, 111, 114, 108, 100, 62]).unwrap(),
            12
        );
        assert!(ascii_hex_length(b"01234").is_err());
        assert_eq!(ascii85_length(b"Ci=9'Aftf/+CQC&Df9G7@rc\"\"AH~>").unwrap(), 29);
        assert_eq!(ascii85_length(b"abc~ \n>").unwrap(), 7);
        // No non whitespace between ~ and > allowed.
        assert!(ascii85_length(b"z!!~x >").is_err());
        assert!(ascii85_length(b"z!!~     x >").is_err());
    }

    #[test]
    fn ascii_hex_inline_image() {
        let input = b"q BI /W 1 /H 1 /BPC 1 /CS /G /F /AHx ID 31> EI Q";
        assert!(content_strict(test_span(input)).is_ok());
    }

    #[test]
    fn ascii_85_inline_image() {
        let input = b"q BI /W 1 /H 1 /BPC 1 /CS /G /F /A85 ID 0`~> EI Q";
        assert!(content_strict(test_span(input)).is_ok());
    }

    #[test]
    fn inline_image_lzw() {
        // "hello world" as an 11x1 gray image, LZW-encoded, followed by a space, EI and Q.
        let mut input = b"q BI /W 11 /H 1 /BPC 8 /CS /G /F /LZW ID ".to_vec();
        let data = b"\x80\x1a\x0c\xa6\xc3\x61\xbc\x40\x77\x37\x9c\x8d\x86\x48\x08";
        input.extend_from_slice(data);
        input.extend_from_slice(b" EI Q");
        let out = content_strict(test_span(&input)).unwrap();
        let image = out.operations[1].operands[0].as_stream().unwrap();
        assert_eq!(image.content, data);
    }

    #[test]
    fn inline_image_flate_data_containing_ei() {
        let pixels = b"\x80 EI Q\n".repeat(10);
        let data = zlib(&pixels, flate2::Compression::none());
        for dict in [
            "/W 10 /H 7 /BPC 8 /CS /G /F /Fl",
            "/W 10 /H 7 /BPC 8 /CS /G /Filter /FlateDecode",
        ] {
            let input = inline_image_content(dict, &data, b"\nEI Q");
            let out = content_strict(test_span(&input)).unwrap();
            let ops: Vec<&str> = out.operations.iter().map(|o| o.operator.as_str()).collect();
            assert_eq!(ops, vec!["q", "BI", "Q"], "{dict}");
            assert_eq!(
                out.operations[1].operands[0].as_stream().unwrap().content,
                data,
                "{dict}"
            );
        }
    }

    #[test]
    fn inline_image_flate_with_predictor() {
        let data = zlib(&[0; 110], flate2::Compression::default());
        let dict = "/W 10 /H 10 /BPC 8 /CS /G /F /Fl /DP << /Predictor 15 /Columns 10 >>";
        let input = inline_image_content(dict, &data, b" EI Q");
        let out = content_strict(test_span(&input)).unwrap();
        assert_eq!(out.operations[1].operands[0].as_stream().unwrap().content, data);
    }

    #[test]
    fn inline_image_run_length() {
        let data = b"\x04\x80 EI \xfe\x41\x80";
        let input = inline_image_content("/W 8 /H 1 /BPC 8 /CS /G /F /RL", data, b" EI Q");
        let out = content_strict(test_span(&input)).unwrap();
        assert_eq!(out.operations[1].operands[0].as_stream().unwrap().content, data);
    }
}
