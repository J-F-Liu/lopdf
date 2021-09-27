pub mod cmap;
mod glyphnames;
mod mappings;

use crate::Error;
use crate::Result;
use cmap::ToUnicodeCMap;
use encoding::EncoderTrap;
use encoding::{all::UTF_16BE, DecoderTrap, Encoding as _};

pub use self::mappings::*;

pub fn bytes_to_string(encoding: &ByteToGlyphMap, bytes: &[u8]) -> String {
    let code_points = bytes
        .iter()
        .filter_map(|&byte| encoding[byte as usize])
        .collect::<Vec<u16>>();
    String::from_utf16_lossy(&code_points)
}

pub fn string_to_bytes(encoding: &ByteToGlyphMap, text: &str) -> Vec<u8> {
    text.encode_utf16()
        .filter_map(|ch| encoding.iter().position(|&code| code == Some(ch)))
        .map(|byte| byte as u8)
        .collect()
}

pub enum Encoding<'a> {
    OneByteEncoding(&'a ByteToGlyphMap),
    SimpleEncoding(&'a str),
    UnicodeMapEncoding(ToUnicodeCMap),
}

impl<'a> std::fmt::Debug for Encoding<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // UnicodeCMap and Bytes encoding ommitted to not bloat debug log
            Self::OneByteEncoding(_arg0) => f.debug_tuple("OneByteEncoding").finish(),
            Self::SimpleEncoding(arg0) => f.debug_tuple("SimpleEncoding").field(arg0).finish(),
            Self::UnicodeMapEncoding(_arg0) => f.debug_tuple("UnicodeMapEncoding").finish(),
        }
    }
}

impl<'a> Encoding<'a> {
    pub fn bytes_to_string(&self, bytes: &[u8]) -> Result<String> {
        match self {
            Self::OneByteEncoding(map) => Ok(bytes_to_string(map, bytes)),
            Self::SimpleEncoding(name) if ["UniGB-UCS2-H", "UniGB−UTF16−H"].contains(name) => UTF_16BE
                .decode(bytes, DecoderTrap::Ignore)
                .map_err(|_| Error::ContentDecode),
            Self::UnicodeMapEncoding(unicode_map) => {
                let utf16_str: Vec<u8> = bytes
                    .chunks_exact(2)
                    .map(|chunk| chunk[0] as u16 * 256 + chunk[1] as u16)
                    .flat_map(|cp| unicode_map.get_or_replacement_char(cp))
                    .flat_map(|it| [(it / 256) as u8, (it % 256) as u8])
                    .collect();
                UTF_16BE
                    .decode(&utf16_str, DecoderTrap::Ignore)
                    .map_err(|_| Error::ContentDecode)
            }
            _ => Err(Error::ContentDecode),
        }
    }
    pub fn string_to_bytes(&self, text: &str) -> Vec<u8> {
        match self {
            Self::OneByteEncoding(map) => string_to_bytes(map, text),
            Self::SimpleEncoding(name) if ["UniGB-UCS2-H", "UniGB-UTF16-H"].contains(name) => {
                UTF_16BE.encode(text, EncoderTrap::Ignore).unwrap()
            }
            Self::UnicodeMapEncoding(_unicode_map) => {
                //maybe only possible if the unicode map is an identity?
                unimplemented!()
            }
            _ => string_to_bytes(&STANDARD_ENCODING, text),
        }
    }
}
