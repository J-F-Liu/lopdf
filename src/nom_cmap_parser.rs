use crate::cmap_section::{CMapParseError, CMapSection};

pub(crate) fn parse(stream_content: &[u8]) -> Result<Vec<CMapSection>, CMapParseError> {
    unimplemented!();
}

impl<E> From<nom::Err<E>> for CMapParseError {
    fn from(err: nom::Err<E>) -> Self {
        match err {
            nom::Err::Incomplete(_) => CMapParseError::Incomplete,
            _ => CMapParseError::Error,
        }
    }
}
