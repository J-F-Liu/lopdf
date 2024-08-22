pub mod cmap;
mod glyphnames;
mod mappings;

use crate::Error;
use crate::Result;
use cmap::ToUnicodeCMap;
use encoding_rs::UTF_16BE;
use log::debug;

pub use self::mappings::*;

pub fn bytes_to_string(encoding: &CodedCharacterSet, bytes: &[u8]) -> String {
    let code_points = bytes
        .iter()
        .filter_map(|&byte| encoding[byte as usize])
        .collect::<Vec<u16>>();
    String::from_utf16_lossy(&code_points)
}

pub fn string_to_bytes(encoding: &CodedCharacterSet, text: &str) -> Vec<u8> {
    text.encode_utf16()
        .filter_map(|ch| encoding.iter().position(|&code| code == Some(ch)))
        .map(|byte| byte as u8)
        .collect()
}

pub enum Encoding<'a> {
    OneByteEncoding(&'a CodedCharacterSet),
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
            Self::SimpleEncoding(name) if ["UniGB-UCS2-H", "UniGB−UTF16−H"].contains(name) => {
                Ok(UTF_16BE.decode(bytes).0.to_string())
            }
            Self::UnicodeMapEncoding(unicode_map) => {
                let utf16_str: Vec<u8> = bytes
                    .chunks_exact(2)
                    .map(|chunk| chunk[0] as u16 * 256 + chunk[1] as u16)
                    .flat_map(|cp| unicode_map.get_or_replacement_char(cp))
                    .flat_map(|it| [(it / 256) as u8, (it % 256) as u8])
                    .collect();
                Ok(UTF_16BE.decode(&utf16_str).0.to_string())
            }
            Self::SimpleEncoding(_) => Err(Error::ContentDecode),
        }
    }
    pub fn string_to_bytes(&self, text: &str) -> Vec<u8> {
        match self {
            Self::OneByteEncoding(map) => string_to_bytes(map, text),
            Self::SimpleEncoding(name) if ["UniGB-UCS2-H", "UniGB-UTF16-H"].contains(name) => encode_utf16_be(text),
            Self::UnicodeMapEncoding(_unicode_map) => {
                // maybe only possible if the unicode map is an identity?
                unimplemented!()
            }
            Self::SimpleEncoding(_) => {
                debug!("Unknown encoding used to encode text {self:?}");
                text.as_bytes().to_vec()
            }
        }
    }
}

/// Encodes the given `str` to UTF-16BE.
/// The recommended way to encode text strings, as it supports all of
/// unicode and all major PDF readers support it.
pub fn encode_utf16_be(text: &str) -> Vec<u8> {
    // Prepend BOM to the mark string as UTF-16BE encoded.
    let bom: u16 = 0xFEFF;
    let mut bytes = vec![];
    bytes.extend([bom].iter().flat_map(|b| b.to_be_bytes()));
    bytes.extend(text.encode_utf16().flat_map(|b| b.to_be_bytes()));
    bytes
}

/// Encodes the given `str` to UTF-8. This method of encoding text strings
/// is first specified in PDF2.0 and reader support is still lacking
/// (notably, Adobe Acrobat Reader doesn't support it at the time of writing).
/// Thus, using it is **NOT RECOMMENDED**.
pub fn encode_utf8(text: &str) -> Vec<u8> {
    // Prepend BOM to the mark string as UTF-8 encoded.
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(text.bytes());
    bytes
}
