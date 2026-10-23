//! The linearization parameter dictionary of a file (ISO 32000-2 Annex F).

use crate::{Dictionary, Object, ObjectId};

/// The linearization parameter dictionary that a file opens with, as it was
/// read (ISO 32000-2 Annex F.2.2, Table F.1).
///
/// A file keeps the dictionary through later incremental updates, which
/// make the parameters stale: compare [`file_length`](Self::file_length)
/// with the length of the file and [`Document::revision_count`] with 1 to
/// tell whether they still describe it.
#[derive(Debug, Clone, PartialEq)]
pub struct Linearization {
    /// The object that holds the dictionary.
    pub id: ObjectId,
    /// `/Linearized`: the version of the linearization format.
    pub version: f32,
    /// `/L`: the length of the file in bytes when it was linearized.
    pub file_length: u64,
    /// `/H`: the offset and length of the primary hint stream, then of the
    /// overflow hint stream when there is one.
    pub hint_streams: Vec<(u64, u64)>,
    /// `/O`: the object number of the first page's page object.
    pub first_page_object: u32,
    /// `/E`: the offset of the end of the first page.
    pub first_page_end: u64,
    /// `/N`: the number of pages.
    pub page_count: u32,
    /// `/T`: the offset of the first entry of the main cross-reference table,
    /// or of the main cross-reference stream.
    pub main_xref_offset: u64,
    /// `/P`: the zero-based number of the first page, 0 when absent.
    pub first_page: u32,
}

impl Linearization {
    /// Read the parameters from `dict`, or `None` when it is no
    /// linearization parameter dictionary or a required entry is missing.
    pub fn from_dictionary(id: ObjectId, dict: &Dictionary) -> Option<Self> {
        let version = match dict.get(b"Linearized").ok()? {
            Object::Integer(version) => *version as f32,
            Object::Real(version) => *version,
            _ => return None,
        };
        let unsigned = |key: &[u8]| -> Option<u64> {
            dict.get(key)
                .ok()?
                .as_i64()
                .ok()
                .and_then(|value| u64::try_from(value).ok())
        };
        let number = |key: &[u8]| -> Option<u32> { unsigned(key).and_then(|value| u32::try_from(value).ok()) };
        let hints = dict.get(b"H").ok()?.as_array().ok()?;
        if !matches!(hints.len(), 2 | 4) {
            return None;
        }
        let value = |hint: &Object| hint.as_i64().ok().and_then(|value| u64::try_from(value).ok());
        let mut hint_streams = Vec::with_capacity(hints.len() / 2);
        for pair in [&hints[..2], &hints[2..]] {
            if let [offset, length] = pair {
                hint_streams.push((value(offset)?, value(length)?));
            }
        }
        Some(Self {
            id,
            version,
            file_length: unsigned(b"L")?,
            hint_streams,
            first_page_object: number(b"O")?,
            first_page_end: unsigned(b"E")?,
            page_count: number(b"N")?,
            main_xref_offset: unsigned(b"T")?,
            first_page: match dict.get(b"P") {
                Ok(_) => number(b"P")?,
                Err(_) => 0,
            },
        })
    }
}
