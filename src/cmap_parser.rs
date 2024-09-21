use log::warn;

use crate::cmap_section::{CMapParseError, CMapSection};

pub(crate) fn parse(_stream_content: &[u8]) -> Result<Vec<CMapSection>, CMapParseError> {
    warn!(
        "Unicode cmap parsing is not supported in pom version, \
text extraction might contain missing characters"
    );
    Ok(Vec::new())
}
