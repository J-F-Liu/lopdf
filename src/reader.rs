#[cfg(feature = "nom_parser")]
use log::{error, warn};
use std::cmp;
use std::collections::{BTreeMap, HashSet};
use std::convert::TryInto;
#[cfg(not(feature = "async"))]
use std::fs::File;
#[cfg(not(feature = "async"))]
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

#[cfg(feature = "rayon")]
use rayon::prelude::*;
#[cfg(feature = "async")]
use tokio::fs::File;
#[cfg(feature = "async")]
use tokio::io::{AsyncRead, AsyncReadExt};
#[cfg(feature = "async")]
use tokio::pin;

use crate::error::XrefError;
use crate::object_stream::ObjectStream;
use crate::parser::{self, ParserInput};
use crate::xref::XrefEntry;
use crate::{Document, Error, IncrementalDocument, Object, ObjectId, Result};

type FilterFunc = fn((u32, u16), &mut Object) -> Option<((u32, u16), Object)>;

#[cfg(not(feature = "async"))]
impl Document {
    /// Load a PDF document from a specified file path.
    #[inline]
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Document> {
        let file = File::open(path)?;
        let capacity = Some(file.metadata()?.len() as usize);
        Self::load_internal(file, capacity, None)
    }

    #[inline]
    pub fn load_filtered<P: AsRef<Path>>(path: P, filter_func: FilterFunc) -> Result<Document> {
        let file = File::open(path)?;
        let capacity = Some(file.metadata()?.len() as usize);
        Self::load_internal(file, capacity, Some(filter_func))
    }

    /// Load a PDF document from an arbitrary source.
    #[inline]
    pub fn load_from<R: Read>(source: R) -> Result<Document> {
        Self::load_internal(source, None, None)
    }

    fn load_internal<R: Read>(
        mut source: R, capacity: Option<usize>, filter_func: Option<FilterFunc>,
    ) -> Result<Document> {
        let mut buffer = capacity.map(Vec::with_capacity).unwrap_or_default();
        source.read_to_end(&mut buffer)?;

        Reader {
            buffer: &buffer,
            document: Document::new(),
        }
        .read(filter_func)
    }

    /// Load a PDF document from a memory slice.
    pub fn load_mem(buffer: &[u8]) -> Result<Document> {
        buffer.try_into()
    }
}

#[cfg(feature = "async")]
impl Document {
    pub async fn load<P: AsRef<Path>>(path: P) -> Result<Document> {
        let file = File::open(path).await?;
        let metadata = file.metadata().await?;
        let capacity = Some(metadata.len() as usize);
        Self::load_internal(file, capacity, None).await
    }

    pub async fn load_filtered<P: AsRef<Path>>(path: P, filter_func: FilterFunc) -> Result<Document> {
        let file = File::open(path).await?;
        let metadata = file.metadata().await?;
        let capacity = Some(metadata.len() as usize);
        Self::load_internal(file, capacity, Some(filter_func)).await
    }

    async fn load_internal<R: AsyncRead>(
        source: R, capacity: Option<usize>, filter_func: Option<FilterFunc>,
    ) -> Result<Document> {
        pin!(source);

        let mut buffer = capacity.map(Vec::with_capacity).unwrap_or_default();
        source.read_to_end(&mut buffer).await?;

        Reader {
            buffer: &buffer,
            document: Document::new(),
        }
        .read(filter_func)
    }

    /// Load a PDF document from a memory slice.
    pub fn load_mem(buffer: &[u8]) -> Result<Document> {
        buffer.try_into()
    }
}

impl TryInto<Document> for &[u8] {
    type Error = Error;

    fn try_into(self) -> Result<Document> {
        Reader {
            buffer: self,
            document: Document::new(),
        }
        .read(None)
    }
}

#[cfg(not(feature = "async"))]
impl IncrementalDocument {
    /// Load a PDF document from a specified file path.
    #[inline]
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let capacity = Some(file.metadata()?.len() as usize);
        Self::load_internal(file, capacity)
    }

    /// Load a PDF document from an arbitrary source.
    #[inline]
    pub fn load_from<R: Read>(source: R) -> Result<Self> {
        Self::load_internal(source, None)
    }

    fn load_internal<R: Read>(mut source: R, capacity: Option<usize>) -> Result<Self> {
        let mut buffer = capacity.map(Vec::with_capacity).unwrap_or_default();
        source.read_to_end(&mut buffer)?;

        let document = Reader {
            buffer: &buffer,
            document: Document::new(),
        }
        .read(None)?;

        Ok(IncrementalDocument::create_from(buffer, document))
    }

    /// Load a PDF document from a memory slice.
    pub fn load_mem(buffer: &[u8]) -> Result<Document> {
        buffer.try_into()
    }
}

#[cfg(feature = "async")]
impl IncrementalDocument {
    /// Load a PDF document from a specified file path.
    #[inline]
    pub async fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path).await?;
        let metadata = file.metadata().await?;
        let capacity = Some(metadata.len() as usize);
        Self::load_internal(file, capacity).await
    }

    /// Load a PDF document from an arbitrary source.
    #[inline]
    pub async fn load_from<R: AsyncRead>(source: R) -> Result<Self> {
        Self::load_internal(source, None).await
    }

    async fn load_internal<R: AsyncRead>(source: R, capacity: Option<usize>) -> Result<Self> {
        pin!(source);

        let mut buffer = capacity.map(Vec::with_capacity).unwrap_or_default();
        source.read_to_end(&mut buffer).await?;

        let document = Reader {
            buffer: &buffer,
            document: Document::new(),
        }
        .read(None)?;

        Ok(IncrementalDocument::create_from(buffer, document))
    }

    /// Load a PDF document from a memory slice.
    pub fn load_mem(buffer: &[u8]) -> Result<Document> {
        buffer.try_into()
    }
}

impl TryInto<IncrementalDocument> for &[u8] {
    type Error = Error;

    fn try_into(self) -> Result<IncrementalDocument> {
        let document = Reader {
            buffer: self,
            document: Document::new(),
        }
        .read(None)?;

        Ok(IncrementalDocument::create_from(self.to_vec(), document))
    }
}

pub struct Reader<'a> {
    pub buffer: &'a [u8],
    pub document: Document,
}

/// Maximum allowed embedding of literal strings.
pub const MAX_BRACKET: usize = 100;

impl<'a> Reader<'a> {
    /// Read whole document.
    pub fn read(mut self, filter_func: Option<FilterFunc>) -> Result<Document> {
        // The document structure can be expressed in PEG as:
        //   document <- header indirect_object* xref trailer xref_start
        let version = parser::header(ParserInput::new_extra(self.buffer, "header")).ok_or(Error::Header)?;

        let xref_start = Self::get_xref_start(self.buffer)?;
        if xref_start > self.buffer.len() {
            return Err(Error::Xref(XrefError::Start));
        }
        self.document.xref_start = xref_start;

        let (mut xref, mut trailer) =
            parser::xref_and_trailer(ParserInput::new_extra(&self.buffer[xref_start..], "xref"), &self)?;

        // Read previous Xrefs of linearized or incremental updated document.
        let mut already_seen = HashSet::new();
        let mut prev_xref_start = trailer.remove(b"Prev").ok_or(Error::DictKey);
        while let Ok(prev) = prev_xref_start.and_then(|offset| offset.as_i64()) {
            if already_seen.contains(&prev) {
                break;
            }
            already_seen.insert(prev);
            if prev < 0 || prev as usize > self.buffer.len() {
                return Err(Error::Xref(XrefError::PrevStart));
            }

            let (prev_xref, prev_trailer) =
                parser::xref_and_trailer(ParserInput::new_extra(&self.buffer[prev as usize..], ""), &self)?;
            xref.merge(prev_xref);

            // Read xref stream in hybrid-reference file
            let prev_xref_stream_start = trailer.remove(b"XRefStm");
            if let Some(prev) = prev_xref_stream_start.and_then(|offset| offset.as_i64().ok()) {
                if prev < 0 || prev as usize > self.buffer.len() {
                    return Err(Error::Xref(XrefError::StreamStart));
                }

                let (prev_xref, _) =
                    parser::xref_and_trailer(ParserInput::new_extra(&self.buffer[prev as usize..], ""), &self)?;
                xref.merge(prev_xref);
            }

            prev_xref_start = prev_trailer.get(b"Prev").cloned();
        }
        let xref_entry_count = xref.max_id().checked_add(1).ok_or(Error::Xref(XrefError::Parse))?;
        if xref.size != xref_entry_count {
            warn!(
                "Size entry of trailer dictionary is {}, correct value is {}.",
                xref.size, xref_entry_count
            );
            xref.size = xref_entry_count;
        }

        self.document.version = version;
        self.document.max_id = xref.size - 1;
        self.document.trailer = trailer;
        self.document.reference_table = xref;

        let zero_length_streams = Mutex::new(vec![]);
        let object_streams = Mutex::new(vec![]);

        let entries_filter_map = |(_, entry): (&_, &_)| {
            if let XrefEntry::Normal { offset, .. } = *entry {
                let (object_id, mut object) = self
                    .read_object(offset as usize, None, &mut HashSet::new())
                    .map_err(|e| error!("Object load error: {:?}", e))
                    .ok()?;
                if let Some(filter_func) = filter_func {
                    filter_func(object_id, &mut object)?;
                }
                if let Ok(ref mut stream) = object.as_stream_mut() {
                    if stream.dict.type_is(b"ObjStm") {
                        let obj_stream = ObjectStream::new(stream).ok()?;
                        let mut object_streams = object_streams.lock().unwrap();
                        // TODO: Is insert and replace intended behavior?
                        // See https://github.com/J-F-Liu/lopdf/issues/160 for more info
                        if let Some(filter_func) = filter_func {
                            let objects: BTreeMap<(u32, u16), Object> = obj_stream
                                .objects
                                .into_iter()
                                .filter_map(|(object_id, mut object)| filter_func(object_id, &mut object))
                                .collect();
                            object_streams.extend(objects);
                        } else {
                            object_streams.extend(obj_stream.objects);
                        }
                    } else if stream.content.is_empty() {
                        let mut zero_length_streams = zero_length_streams.lock().unwrap();
                        zero_length_streams.push(object_id);
                    }
                }
                Some((object_id, object))
            } else {
                None
            }
        };
        #[cfg(feature = "rayon")]
        {
            self.document.objects = self
                .document
                .reference_table
                .entries
                .par_iter()
                .filter_map(entries_filter_map)
                .collect();
        }
        #[cfg(not(feature = "rayon"))]
        {
            self.document.objects = self
                .document
                .reference_table
                .entries
                .iter()
                .filter_map(entries_filter_map)
                .collect();
        }
        // Only add entries, but never replace entries
        for (id, entry) in object_streams.into_inner().unwrap() {
            self.document.objects.entry(id).or_insert(entry);
        }

        for object_id in zero_length_streams.into_inner().unwrap() {
            let _ = self.set_stream_content(object_id);
        }

        Ok(self.document)
    }

    fn set_stream_content(&mut self, object_id: ObjectId) -> Result<()> {
        let length = self.get_stream_length(object_id)?;
        let stream = self
            .document
            .get_object_mut(object_id)
            .and_then(Object::as_stream_mut)?;
        let start = stream.start_position.ok_or(Error::ObjectNotFound)?;

        if length < 0 {
            return Err(Error::Syntax("Negative stream length.".to_string()));
        }

        let end = start + length as usize;

        if end > self.buffer.len() {
            return Err(Error::Syntax("Stream extends after document end.".to_string()));
        }

        stream.set_content(self.buffer[start..end].to_vec());
        Ok(())
    }

    fn get_stream_length(&self, object_id: ObjectId) -> Result<i64> {
        let object = self.document.get_object(object_id)?;
        let stream = object.as_stream()?;

        stream
            .dict
            .get(b"Length")
            .and_then(|value| {
                if let Ok(id) = value.as_reference() {
                    return self.document.get_object(id).and_then(Object::as_i64);
                }
                value.as_i64()
            })
            .map_err(|err| {
                error!(
                    "Stream dictionary of '{} {} R' is missing the Length entry",
                    object_id.0, object_id.1
                );
                err
            })
    }

    /// Get object offset by object id.
    fn get_offset(&self, id: ObjectId) -> Result<u32> {
        let entry = self.document.reference_table.get(id.0).ok_or(Error::ObjectNotFound)?;
        match *entry {
            XrefEntry::Normal { offset, generation } => {
                if id.1 == generation {
                    Ok(offset)
                } else {
                    Err(Error::ObjectNotFound)
                }
            }
            _ => Err(Error::ObjectNotFound),
        }
    }

    pub fn get_object(&self, id: ObjectId, already_seen: &mut HashSet<ObjectId>) -> Result<Object> {
        if already_seen.contains(&id) {
            warn!("reference cycle detected resolving object {} {}", id.0, id.1);
            return Err(Error::ReferenceCycle);
        }
        already_seen.insert(id);
        let offset = self.get_offset(id)?;
        let (_, obj) = self.read_object(offset as usize, Some(id), already_seen)?;

        Ok(obj)
    }

    fn read_object(
        &self, offset: usize, expected_id: Option<ObjectId>, already_seen: &mut HashSet<ObjectId>,
    ) -> Result<(ObjectId, Object)> {
        if offset > self.buffer.len() {
            return Err(Error::Offset(offset));
        }

        parser::indirect_object(
            ParserInput::new_extra(self.buffer, "indirect object"),
            offset,
            expected_id,
            self,
            already_seen,
        )
    }

    fn get_xref_start(buffer: &[u8]) -> Result<usize> {
        let seek_pos = buffer.len() - cmp::min(buffer.len(), 512);
        Self::search_substring(buffer, b"%%EOF", seek_pos)
            .and_then(|eof_pos| if eof_pos > 25 { Some(eof_pos) } else { None })
            .and_then(|eof_pos| Self::search_substring(buffer, b"startxref", eof_pos - 25))
            .ok_or(Error::Xref(XrefError::Start))
            .and_then(|xref_pos| {
                if xref_pos <= buffer.len() {
                    match parser::xref_start(ParserInput::new_extra(&buffer[xref_pos..], "xref")) {
                        Some(startxref) => Ok(startxref as usize),
                        None => Err(Error::Xref(XrefError::Start)),
                    }
                } else {
                    Err(Error::Xref(XrefError::Start))
                }
            })
    }

    fn search_substring(buffer: &[u8], pattern: &[u8], start_pos: usize) -> Option<usize> {
        let mut seek_pos = start_pos;
        let mut index = 0;

        while seek_pos < buffer.len() && index < pattern.len() {
            if buffer[seek_pos] == pattern[index] {
                index += 1;
            } else if index > 0 {
                seek_pos -= index;
                index = 0;
            }
            seek_pos += 1;

            if index == pattern.len() {
                let res = seek_pos - index;
                return Self::search_substring(buffer, pattern, res + 1).or(Some(res));
            }
        }

        None
    }
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn load_document() {
    let mut doc = Document::load("assets/example.pdf").unwrap();
    assert_eq!(doc.version, "1.5");

    // Create temporary folder to store file.
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test_2_load.pdf");
    doc.save(file_path).unwrap();
}

#[cfg(all(test, feature = "async"))]
#[tokio::test]
async fn load_document() {
    let mut doc = Document::load("assets/example.pdf").await.unwrap();
    assert_eq!(doc.version, "1.5");

    // Create temporary folder to store file.
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test_2_load.pdf");
    doc.save(file_path).unwrap();
}

#[test]
#[should_panic(expected = "Xref(Start)")]
fn load_short_document() {
    let _doc = Document::load_mem(b"%PDF-1.5\n%%EOF\n").unwrap();
}

#[test]
fn load_many_shallow_brackets() {
    let content: String = std::iter::repeat("()")
        .take(MAX_BRACKET * 10)
        .flat_map(|x| x.chars())
        .collect();
    const STREAM_CRUFT: usize = 33;
    let doc = format!(
        "%PDF-1.5
1 0 obj<</Type/Pages/Kids[5 0 R]/Count 1/Resources 3 0 R/MediaBox[0 0 595 842]>>endobj
2 0 obj<</Type/Font/Subtype/Type1/BaseFont/Courier>>endobj
3 0 obj<</Font<</F1 2 0 R>>>>endobj
5 0 obj<</Type/Page/Parent 1 0 R/Contents[4 0 R]>>endobj
6 0 obj<</Type/Catalog/Pages 1 0 R>>endobj
4 0 obj<</Length {}>>stream
BT
/F1 48 Tf
100 600 Td
({}) Tj
ET
endstream endobj\n",
        content.len() + STREAM_CRUFT,
        content
    );
    let doc = format!(
        "{}xref
0 7
0000000000 65535 f 
0000000009 00000 n 
0000000096 00000 n 
0000000155 00000 n 
0000000291 00000 n 
0000000191 00000 n 
0000000248 00000 n 
trailer
<</Root 6 0 R/Size 7>>
startxref
{}
%%EOF",
        doc,
        doc.len()
    );

    let _doc = Document::load_mem(doc.as_bytes()).unwrap();
}

#[test]
fn load_too_deep_brackets() {
    let content: Vec<u8> = std::iter::repeat(b'(')
        .take(MAX_BRACKET + 1)
        .chain(std::iter::repeat(b')').take(MAX_BRACKET + 1))
        .collect();
    let content = String::from_utf8(content).unwrap();
    const STREAM_CRUFT: usize = 33;
    let doc = format!(
        "%PDF-1.5
1 0 obj<</Type/Pages/Kids[5 0 R]/Count 1/Resources 3 0 R/MediaBox[0 0 595 842]>>endobj
2 0 obj<</Type/Font/Subtype/Type1/BaseFont/Courier>>endobj
3 0 obj<</Font<</F1 2 0 R>>>>endobj
5 0 obj<</Type/Page/Parent 1 0 R/Contents[7 0 R 4 0 R]>>endobj
6 0 obj<</Type/Catalog/Pages 1 0 R>>endobj
7 0 obj<</Length 45>>stream
BT /F1 48 Tf 100 600 Td (Hello World!) Tj ET
endstream
endobj
4 0 obj<</Length {}>>stream
BT
/F1 48 Tf
100 600 Td
({}) Tj
ET
endstream endobj\n",
        content.len() + STREAM_CRUFT,
        content
    );
    let doc = format!(
        "{}xref
0 7
0000000000 65535 f 
0000000009 00000 n 
0000000096 00000 n 
0000000155 00000 n 
0000000387 00000 n 
0000000191 00000 n 
0000000254 00000 n 
0000000297 00000 n 
trailer
<</Root 6 0 R/Size 7>>
startxref
{}
%%EOF",
        doc,
        doc.len()
    );

    let doc = Document::load_mem(doc.as_bytes()).unwrap();
    let pages = doc.get_pages().keys().cloned().collect::<Vec<_>>();
    assert_eq!("Hello World!\n", doc.extract_text(&pages).unwrap());
}
