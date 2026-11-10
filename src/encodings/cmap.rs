use crate::cmap_section::{CMapParseError, CMapSection, CodeLen, SourceCode};
use crate::parser::cmap_parser::parse;

use log::error;
use rangemap::RangeInclusiveMap;
use std::collections::HashMap;
use thiserror::Error;

/// Unicode Cmap is implemented by 4 maps.
/// Each map contains a mappings from source codes to unicode values for a different length of codes.
/// Codes vary from 1 byte to 4 bytes so they are always in limits of u32.
/// However to map a code to a unicode value an additional knowledge about the number of bytes is needed,
/// as 2 byte code <0000> shouldn't be matched with a single byte <00> even though they have the same integer value.
#[derive(Debug, Default)]
pub struct ToUnicodeCMap {
    pub bf_ranges: [RangeInclusiveMap<SourceCode, BfRangeTarget>; 4],
    codespace_ranges: Vec<(SourceCode, SourceCode, CodeLen)>,
    reverse_map: Option<HashMap<Vec<u16>, Vec<ReverseCMapEntry>>>,
}
/// Represents the information needed to map a Unicode sequence back to a source code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReverseCMapEntry {
    pub source_code: SourceCode,
    pub code_len: CodeLen,
    // Optionally, add priority if multiple source codes map to the same Unicode sequence
    // pub priority: u8,
}

#[derive(Debug, Error)]
pub enum UnicodeCMapError {
    #[error("could not parse ToUnicode CMap: {0:#?}")]
    Parse(CMapParseError),
    #[error("invalid code range")]
    InvalidCodeRange,
}

impl From<CMapParseError> for UnicodeCMapError {
    fn from(err: CMapParseError) -> Self {
        UnicodeCMapError::Parse(err)
    }
}

impl ToUnicodeCMap {
    const REPLACEMENT_CHAR: u16 = 0xfffd;

    /// The most entries [`build_reverse_map`](Self::build_reverse_map) will create.
    ///
    /// The reverse map holds one entry per source code of every `bfrange`. A `bfrange`
    /// may declare a source-code span up to the full 4-byte range (`<00000000>
    /// <FFFFFFFF>`), so a stream of a few hundred bytes can request billions of entries
    /// and exhaust memory. The reverse map is only consulted when *encoding* text with a
    /// ToUnicode CMap; text extraction and decoding never read it. Capping the number of
    /// entries bounds the allocation while leaving every real font — whose codes number
    /// in the thousands at most — fully mapped. 2^16 is the size of the whole two-byte
    /// code space.
    const MAX_REVERSE_MAP_ENTRIES: usize = 1 << 16;

    pub fn new() -> ToUnicodeCMap {
        ToUnicodeCMap {
            bf_ranges: [(); 4].map(|_| RangeInclusiveMap::new()),
            codespace_ranges: Vec::new(),
            reverse_map: None,
        }
    }

    pub(crate) fn parse(stream_content: Vec<u8>) -> Result<ToUnicodeCMap, UnicodeCMapError> {
        let cmap_sections = parse(&stream_content[..])?;
        Self::from_sections(cmap_sections)
    }

    fn from_sections(cmap_sections: Vec<CMapSection>) -> Result<ToUnicodeCMap, UnicodeCMapError> {
        let mut cmap = Self::new();
        for section in cmap_sections {
            match section {
                CMapSection::CsRange(ranges) => cmap.codespace_ranges.extend(ranges),
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

        cmap.build_reverse_map();

        Ok(cmap)
    }

    /// The CMap to decode a simple font with. A simple font's character codes are always
    /// one byte (ISO 32000-1, 9.6.1), but its ToUnicode CMap is often written with a two-byte
    /// codespace and two-byte keys such as `<0041>`; read as is, two codes would be taken
    /// for one. Each code 0..=255 is therefore looked up as a one-byte key, then as a two-byte
    /// key, and a code the CMap does not map is taken from `fallback` (the font's /Encoding),
    /// since ToUnicode CMaps often cover only some of a font's codes.
    pub(crate) fn for_simple_font(&self, mut fallback: impl FnMut(u8) -> Option<Vec<u16>>) -> ToUnicodeCMap {
        let mut cmap = Self::new();
        for code in 0..=u8::MAX {
            let unicode = self
                .get(code as SourceCode, 1)
                .or_else(|| self.get(code as SourceCode, 2))
                .or_else(|| fallback(code));
            if let Some(unicode) = unicode {
                cmap.put_char(code as SourceCode, 1, unicode);
            }
        }
        cmap.codespace_ranges.push((0x00, 0xFF, 1));
        cmap.build_reverse_map();
        cmap
    }

    fn build_reverse_map(&mut self) {
        let mut rev_map = HashMap::new();
        let mut entries = 0usize;

        'ranges: for code_len_idx in 0..self.bf_ranges.len() {
            let code_len = (code_len_idx + 1) as u8;
            for (range, target) in self.bf_ranges[code_len_idx].iter() {
                for src_code in range.clone() {
                    let unicode_sequence: Option<Vec<u16>> = match target {
                        BfRangeTarget::UTF16CodePoint { offset } => {
                            Some(vec![u32::wrapping_add(src_code, *offset) as u16])
                        }
                        BfRangeTarget::HexString(hex_str_vec) => {
                            // If the hex_str_vec itself is the target for a single src_code in a bfchar-like mapping
                            // or if it's a base for a bfrange where only the last element increments.
                            if src_code == *range.start() {
                                // Simplified: assume direct mapping for start of range
                                Some(hex_str_vec.clone())
                            } else if hex_str_vec.len() == 1 {
                                // For ranges like <01> <05> <0041>
                                Some(vec![hex_str_vec[0].wrapping_add((src_code - range.start()) as u16)])
                            } else if !hex_str_vec.is_empty() {
                                // For ranges like <01> <05> [<0041> <0042> ...]
                                let mut current_hex_str = hex_str_vec.clone();
                                if let Some(last_val) = current_hex_str.last_mut() {
                                    *last_val = last_val.wrapping_add((src_code - range.start()) as u16);
                                    Some(current_hex_str)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        BfRangeTarget::ArrayOfHexStrings(array_of_hex_str) => {
                            let index = (src_code - range.start()) as usize;
                            if index < array_of_hex_str.len() {
                                Some(array_of_hex_str[index].clone())
                            } else {
                                None
                            }
                        }
                    };

                    if let Some(uni_seq) = unicode_sequence
                        && !uni_seq.is_empty()
                    {
                        rev_map.entry(uni_seq).or_insert_with(Vec::new).push(ReverseCMapEntry {
                            source_code: src_code,
                            code_len,
                        });
                        entries += 1;
                        if entries >= Self::MAX_REVERSE_MAP_ENTRIES {
                            error!(
                                "ToUnicode CMap bfrange reverse map reached the {} entry cap; \
                                 the remaining source codes are left unmapped for reverse lookup",
                                Self::MAX_REVERSE_MAP_ENTRIES
                            );
                            break 'ranges;
                        }
                    }
                }
            }
        }
        self.reverse_map = Some(rev_map);
    }

    pub(super) fn get_for_decoding(&self, code: SourceCode, code_len: CodeLen) -> Option<Vec<u16>> {
        if self.codespace_ranges.is_empty() {
            return self.get(code, code_len);
        }

        // Codespace bounds apply to each byte independently (ISO 32000-1, 9.7.5.1).
        self.codespace_ranges
            .iter()
            .any(|&(low, high, len)| {
                len == code_len
                    && (0..len).all(|i| {
                        let shift = 8 * i;
                        let byte = (code >> shift) as u8;
                        (low >> shift) as u8 <= byte && byte <= (high >> shift) as u8
                    })
            })
            .then(|| self.get_or_replacement_char(code, code_len))
    }

    pub fn get(&self, code: SourceCode, code_len: CodeLen) -> Option<Vec<u16>> {
        if code_len > 4 || code_len == 0 {
            error!("Code lenght should be between l and 4 bytes, got {code_len}");
            return None;
        }
        use BfRangeTarget::*;

        let bf_ranges_map = &self.bf_ranges[(code_len - 1) as usize];

        bf_ranges_map.get_key_value(&code).map(|(range, value)| match value {
            HexString(vec) => {
                let mut ret_vec = vec.clone();
                if let Some(last) = ret_vec.last_mut() {
                    *last = last.wrapping_add((code - range.start()) as u16);
                }
                ret_vec
            }
            UTF16CodePoint { offset } => vec![u32::wrapping_add(code, *offset) as u16],
            ArrayOfHexStrings(vec_of_strings) => {
                let idx = (code - range.start()) as usize;
                if idx < vec_of_strings.len() {
                    vec_of_strings[idx].clone()
                } else {
                    vec![ToUnicodeCMap::REPLACEMENT_CHAR]
                }
            }
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

    /// Gets the source code(s) for a given Unicode sequence.
    /// Prioritizes shorter byte sequences if multiple mappings exist.
    pub fn get_source_codes_for_unicode(&self, unicode_sequence: &[u16]) -> Option<&[ReverseCMapEntry]> {
        if let Some(map) = &self.reverse_map {
            // TODO: Add prioritization logic if needed (e.g., prefer shorter code_len)
            map.get(unicode_sequence).map(|v| v.as_slice())
        } else {
            None
        }
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

    fn reverse_map_entries(cmap: &ToUnicodeCMap) -> usize {
        cmap.reverse_map.as_ref().unwrap().values().map(|v| v.len()).sum()
    }

    fn cmap_with_bfrange(lo_hi: &str) -> ToUnicodeCMap {
        let data = format!(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
             /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
             1 begincodespacerange\n<00000000> <FFFFFFFF>\nendcodespacerange\n\
             1 beginbfrange\n{lo_hi} <0041>\nendbfrange\n\
             endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend"
        );
        ToUnicodeCMap::parse(data.into_bytes()).unwrap()
    }

    // Regression: a `bfrange` declares a source-code span, and `build_reverse_map` used
    // to create one entry per source code with the only guard being `end < start`. A
    // span covering the whole 4-byte code space (`<00000000> <FFFFFFFF>`) therefore
    // requested ~4.29e9 entries (tens of GiB) from a ~150-byte stream, reachable from
    // `Document::extract_text`. The reverse map is now capped.
    #[test]
    fn bfrange_reverse_map_is_bounded() {
        // A small range is mapped in full.
        assert_eq!(reverse_map_entries(&cmap_with_bfrange("<00000000> <00000FFF>")), 4096);
        // The full 4-byte range would be ~4.29e9 entries without the cap; it must not be.
        let full = cmap_with_bfrange("<00000000> <FFFFFFFF>");
        assert!(
            reverse_map_entries(&full) <= ToUnicodeCMap::MAX_REVERSE_MAP_ENTRIES,
            "reverse map must be bounded by MAX_REVERSE_MAP_ENTRIES"
        );
    }

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

    #[test]
    fn array_of_hex_strings_out_of_bounds_returns_replacement() {
        // Simulate a malformed CMap bfrange where the array has fewer entries
        // than the declared source code range covers.
        let mut cmap = ToUnicodeCMap::new();
        let array = BfRangeTarget::ArrayOfHexStrings(vec![
            vec![0x0041], // 'A' — for code 0x10
            vec![0x0042], // 'B' — for code 0x11
        ]);
        // Range 0x10..=0x14 but only 2 entries in array (needs 5)
        cmap.put(0x10, 0x14, 2, array);

        // In-bounds lookups work
        assert_eq!(cmap.get(0x10, 2), Some(vec![0x0041]));
        assert_eq!(cmap.get(0x11, 2), Some(vec![0x0042]));

        // Out-of-bounds lookups return replacement char instead of panicking
        assert_eq!(cmap.get(0x12, 2), Some(vec![ToUnicodeCMap::REPLACEMENT_CHAR]));
        assert_eq!(cmap.get(0x13, 2), Some(vec![ToUnicodeCMap::REPLACEMENT_CHAR]));
        assert_eq!(cmap.get(0x14, 2), Some(vec![ToUnicodeCMap::REPLACEMENT_CHAR]));
    }

    #[test]
    fn bfrange_target_overflow_wraps_instead_of_panicking() {
        // The last UTF-16 unit of a multi-unit bfrange target is incremented per source code,
        // so a crafted target ending in <FFFF> overflows for the second code of the range.
        let data = b"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<00> <FF>
endcodespacerange
1 beginbfrange
<01> <02> <D83DFFFF>
endbfrange
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
        let cmap = ToUnicodeCMap::parse(data.to_vec()).unwrap();

        assert_eq!(cmap.get(0x01, 1), Some(vec![0xD83D, 0xFFFF]));
        assert_eq!(cmap.get(0x02, 1), Some(vec![0xD83D, 0x0000]));
    }
}
