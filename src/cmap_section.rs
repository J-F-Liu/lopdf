/*
ToUnicode CMaps are special CMaps and thus can be parsed simpler. Assumptions:
- /CMapType is always 2
- codespace range always from <0000> to <ffff>
- only bfchar and bfrange sections allowed
- no glyph names as target allowed, only hex strings
- source character codes always 2 bytes
- target encoded in UTF16-BE
 */

pub(crate) type ArrayOfTargetStrings = Vec<Vec<u16>>;
pub(crate) type SourceRangeMapping = ((u16, u16), ArrayOfTargetStrings);
pub(crate) type SourceCharMapping = (u16, Vec<u16>);
#[derive(Debug, PartialEq)]
pub enum CMapSection {
    CsRange(Vec<(u16, u16)>),
    BfChar(Vec<SourceCharMapping>),
    BfRange(Vec<SourceRangeMapping>),
}

#[derive(Debug)]
pub enum CMapParseError {
    Incomplete,
    Error,
}
