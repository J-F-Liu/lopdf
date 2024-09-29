use crate::cmap_parser::parse;
use crate::cmap_section::{CMapParseError, CMapSection, CodeLen, SourceCode};
use crate::parser::ParserInput;

use log::error;
use rangemap::RangeInclusiveMap;
use std::fmt;

/// Unicode Cmap is implemented by 4 maps.
/// Each map contains a mappings from source codes to unicode values for a different length of codes.
/// Codes vary from 1 byte to 4 bytes so they are always in limits of u32.
/// However to map a code to a unicode value an additional knowledge about the number of bytes is needed,
/// as 2 byte code <0000> shouldn't be matched with a single byte <00> even though they have the same integer value.
#[derive(Debug, Default)]
pub struct ToUnicodeCMap {
    bf_ranges: [RangeInclusiveMap<SourceCode, BfRangeTarget>; 4],
}

#[derive(Debug)]
pub enum UnicodeCMapError {
    Parse(CMapParseError),
    InvalidCodeRange,
}

impl fmt::Display for UnicodeCMapError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use UnicodeCMapError::*;
        match self {
            Parse(cmap_parse_error) => write!(f, "Could not parse ToUnicodeCMap: {:#?}!", cmap_parse_error),
            InvalidCodeRange => write!(f, "Invalid code range given!"),
        }
    }
}
impl From<CMapParseError> for UnicodeCMapError {
    fn from(err: CMapParseError) -> Self {
        UnicodeCMapError::Parse(err)
    }
}

impl ToUnicodeCMap {
    const REPLACEMENT_CHAR: u16 = 0xfffd;

    pub fn new() -> ToUnicodeCMap {
        ToUnicodeCMap {
            bf_ranges: [(); 4].map(|_| RangeInclusiveMap::new()),
        }
    }

    pub(crate) fn parse(stream_content: Vec<u8>) -> Result<ToUnicodeCMap, UnicodeCMapError> {
        let cmap_sections = parse(ParserInput::new_extra(&stream_content[..], "cmap"))?;
        Self::from_sections(cmap_sections)
    }

    fn from_sections(cmap_sections: Vec<CMapSection>) -> Result<ToUnicodeCMap, UnicodeCMapError> {
        let mut cmap = Self::new();
        for section in cmap_sections {
            match section {
                CMapSection::CsRange(_) => (), // currently no additional validation is implemented for code ranges
                CMapSection::BfChar(char_mappings) => {
                    for ((code, code_len), dst) in char_mappings {
                        cmap.put_char(code, code_len, dst);
                    }
                }
                CMapSection::BfRange(range_mappings) => {
                    for ((start, end, code_len), dst_vec) in range_mappings {
                        if end < start {
                            return Err(UnicodeCMapError::InvalidCodeRange);
                        }
                        match dst_vec.len() {
                            1 if dst_vec[0].len() == 1 => cmap.put(
                                start,
                                end,
                                code_len,
                                BfRangeTarget::UTF16CodePoint {
                                    offset: u32::wrapping_sub(dst_vec[0][0] as u32, start),
                                },
                            ),
                            1 => cmap.put(start, end, code_len, BfRangeTarget::HexString(dst_vec[0].clone())),
                            0 => return Err(UnicodeCMapError::InvalidCodeRange),
                            _ => cmap.put(start, end, code_len, BfRangeTarget::ArrayOfHexStrings(dst_vec.clone())),
                        }
                    }
                }
            }
        }
        Ok(cmap)
    }

    pub fn get(&self, code: SourceCode, code_len: CodeLen) -> Option<Vec<u16>> {
        if code_len > 4 || code_len == 0 {
            error!("Code lenght should be between l and 4 bytes, got {code_len}");
            return None;
        }
        use BfRangeTarget::*;

        let bf_ranges_map = &self.bf_ranges[(code_len - 1) as usize];

        bf_ranges_map.get_key_value(&code).map(|(range, value)| match value {
            HexString(ref vec) => {
                let mut ret_vec = vec.clone();
                *(ret_vec.last_mut().unwrap()) += (code - range.start()) as u16;
                ret_vec
            }
            UTF16CodePoint { offset } => vec![u32::wrapping_add(code, *offset) as u16],
            ArrayOfHexStrings(ref vec_of_strings) => vec_of_strings[(code - range.start()) as usize].clone(),
        })
    }

    pub fn get_or_replacement_char(&self, code: SourceCode, code_len: CodeLen) -> Vec<u16> {
        self.get(code, code_len)
            .unwrap_or(vec![ToUnicodeCMap::REPLACEMENT_CHAR])
    }

    pub fn put(&mut self, src_code_lo: SourceCode, src_code_hi: SourceCode, code_len: CodeLen, target: BfRangeTarget) {
        if code_len > 4 || code_len == 0 {
            error!("Code lenght should be between l and 4 bytes, got {code_len}, ignoring");
            return;
        }
        self.bf_ranges[(code_len - 1) as usize].insert(src_code_lo..=src_code_hi, target)
    }

    pub fn put_char(&mut self, code: SourceCode, code_len: CodeLen, dst: Vec<u16>) {
        let target = if dst.len() == 1 {
            BfRangeTarget::UTF16CodePoint {
                offset: u32::wrapping_sub(dst[0] as u32, code),
            }
        } else {
            BfRangeTarget::HexString(dst)
        };
        self.put(code, code, code_len, target)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BfRangeTarget {
    // UTF16-BE encoding is used
    HexString(Vec<u16>),
    // don't store the actual codepoint but rather an offset to the src_code_lo
    // so that consecutive ranges can be mapped to the same value in the range map
    UTF16CodePoint { offset: u32 },
    ArrayOfHexStrings(Vec<Vec<u16>>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_char_can_be_retrieved() {
        let mut cmap = ToUnicodeCMap::new();
        let char_code = 0x01;
        let char_value = vec![0x1234];
        cmap.put_char(char_code, 2, char_value.clone());

        assert_eq!(cmap.get(char_code, 2), Some(char_value))
    }

    #[test]
    fn char_can_be_retrieved_only_by_appropriate_len() {
        let mut cmap = ToUnicodeCMap::new();
        let char_code = 0x1;
        let code_len = 4;
        let char_value = vec![0x1234];
        cmap.put_char(char_code, code_len, char_value.clone());

        for i in 1..=3 {
            assert_eq!(cmap.get(char_code, i), None);
        }

        assert_eq!(cmap.get(char_code, code_len), Some(char_value));
    }

    #[test]
    fn wrong_code_len_does_not_panic() {
        let mut cmap = ToUnicodeCMap::new();
        let char_code = 0x1;
        let char_value = vec![0x1234];

        cmap.put_char(char_code, 5, char_value.clone());
        cmap.put_char(char_code, 0, char_value.clone());
    }
}
