extern crate rangemap;
use crate::cmap_parser::{cmap_stream, CMapSection};
use rangemap::RangeInclusiveMap;
use std::fmt;

#[derive(Debug)]
pub struct ToUnicodeCMap {
    bf_ranges: RangeInclusiveMap<u16, BfRangeTarget>,
}

#[derive(Debug)]
pub enum UnicodeCMapError {
    Parse(pom::Error),
    UnsupportedCodeSpaceRange,
    InvalidCodeRange,
}

impl fmt::Display for UnicodeCMapError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use UnicodeCMapError::*;
        match self {
            Parse(pom_err) => write!(f, "Could not parse ToUnicodeCMap: {}!", pom_err),
            UnsupportedCodeSpaceRange => write!(f, "Unsupported codespace range given!"),
            InvalidCodeRange => write!(f, "Invalid code range given!"),
        }
    }
}
impl From<pom::Error> for UnicodeCMapError {
    fn from(err: pom::Error) -> Self {
        UnicodeCMapError::Parse(err)
    }
}

impl ToUnicodeCMap {
    pub fn new() -> ToUnicodeCMap {
        ToUnicodeCMap {
            bf_ranges: RangeInclusiveMap::new(),
        }
    }

    pub(crate) fn parse(stream_content: Vec<u8>) -> Result<ToUnicodeCMap, UnicodeCMapError> {
        let cmap_sections = cmap_stream().parse(&stream_content[..])?;
        Self::from_sections(cmap_sections)
    }

    fn from_sections(cmap_sections: Vec<CMapSection>) -> Result<ToUnicodeCMap, UnicodeCMapError> {
        let mut cmap = Self::new();
        for section in cmap_sections {
            match section {
                CMapSection::CsRangeSection(ranges) => match ranges.len() {
                    1 if ranges[0] == (0x0000, 0xffff) => {}
                    _ => return Err(UnicodeCMapError::UnsupportedCodeSpaceRange),
                },
                CMapSection::BfCharSection(char_mappings) => {
                    for (code, dst) in char_mappings {
                        cmap.put_char(code, dst);
                    }
                }
                CMapSection::BfRangeSection(range_mappings) => {
                    for ((start, end), dst_vec) in range_mappings {
                        if end < start {
                            return Err(UnicodeCMapError::InvalidCodeRange);
                        }
                        match dst_vec.len() {
                            1 if dst_vec[0].len() == 1 => cmap.put(
                                start,
                                end,
                                BfRangeTarget::UTF16CodePoint {
                                    offset: u16::wrapping_sub(dst_vec[0][0], start),
                                },
                            ),
                            1 => cmap.put(start, end, BfRangeTarget::HexString(dst_vec[0].clone())),
                            0 => return Err(UnicodeCMapError::InvalidCodeRange),
                            _ => cmap.put(start, end, BfRangeTarget::ArrayOfHexStrings(dst_vec.clone())),
                        }
                    }
                }
            }
        }
        Ok(cmap)
    }

    pub fn get(&self, code: u16) -> Option<Vec<u16>> {
        use BfRangeTarget::*;
        self.bf_ranges.get_key_value(&code).map(|(range, value)| match value {
            HexString(ref vec) => {
                let mut ret_vec = vec.clone();
                *(ret_vec.last_mut().unwrap()) += code - range.start();
                ret_vec
            }
            UTF16CodePoint { offset } => vec![u16::wrapping_add(code, *offset)],
            ArrayOfHexStrings(ref vec_of_strings) => vec_of_strings[(code - range.start()) as usize].clone(),
        })
    }

    pub fn get_or_replacement_char(&self, code: u16) -> Vec<u16> {
        self.get(code).unwrap_or(vec![0xfffd])
    }

    pub fn put(&mut self, src_code_lo: u16, src_code_hi: u16, target: BfRangeTarget) {
        self.bf_ranges.insert(src_code_lo..=src_code_hi, target)
    }

    pub fn put_char(&mut self, code: u16, dst: Vec<u16>) {
        let target = if dst.len() == 1 {
            BfRangeTarget::UTF16CodePoint {
                offset: u16::wrapping_sub(dst[0], code),
            }
        } else {
            BfRangeTarget::HexString(dst)
        };
        self.put(code, code, target)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BfRangeTarget {
    // UTF16-BE encoding is used
    HexString(Vec<u16>),
    // don't store the actual codepoint but rather an offset to the src_code_lo
    // so that consecutive ranges can be mapped to the same value in the range map
    UTF16CodePoint { offset: u16 },
    ArrayOfHexStrings(Vec<Vec<u16>>),
}
