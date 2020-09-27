#![cfg(any(feature = "pom_parser", feature = "nom_parser"))]

use log::{error, warn};
use std::cmp;
use std::convert::TryInto;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

#[cfg(feature = "rayon")]
use rayon::prelude::*;

use super::parser;
use super::{Document, Object, ObjectId};
use crate::error::XrefError;
use crate::object_stream::ObjectStream;
use crate::xref::XrefEntry;
use crate::{Error, Result};

impl Document {
    /// Load a PDF document from a specified file path.
    #[inline]
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Document> {
        let file = File::open(path)?;
        let capacity = Some(file.metadata()?.len() as usize);
        Self::load_internal(file, capacity)
    }

    /// Load a PDF document from an arbitrary source.
    #[inline]
    pub fn load_from<R: Read>(source: R) -> Result<Document> {
        Self::load_internal(source, None)
    }

    fn load_internal<R: Read>(mut source: R, capacity: Option<usize>) -> Result<Document> {
        let mut buffer = capacity.map(Vec::with_capacity).unwrap_or_else(Vec::new);
        source.read_to_end(&mut buffer)?;

        Reader {
            buffer: &buffer,
            document: Document::new(),
        }
        .read()
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
        .read()
    }
}

pub struct Reader<'a> {
    buffer: &'a [u8],
    document: Document,
}

/// Maximum allowed embedding of literal strings.
pub const MAX_BRACKET: usize = 100;

impl<'a> Reader<'a> {
    /// Read whole document.
    fn read(mut self) -> Result<Document> {
        // The document structure can be expressed in PEG as:
        //   document <- header indirect_object* xref trailer xref_start
        let version = parser::header(&self.buffer).ok_or(Error::Header)?;

        let xref_start = Self::get_xref_start(&self.buffer)?;
        if xref_start > self.buffer.len() {
            return Err(Error::Xref(XrefError::Start));
        }

        let (mut xref, mut trailer) = parser::xref_and_trailer(&self.buffer[xref_start..], &self)?;

        // Read previous Xrefs of linearized or incremental updated document.
        let mut prev_xref_start = trailer.remove(b"Prev");
        while let Some(prev) = prev_xref_start.and_then(|offset| offset.as_i64().ok()) {
            let prev = prev as usize;
            if prev > self.buffer.len() {
                return Err(Error::Xref(XrefError::PrevStart));
            }
            let (prev_xref, mut prev_trailer) = parser::xref_and_trailer(&self.buffer[prev..], &self)?;
            xref.extend(prev_xref);

            // Read xref stream in hybrid-reference file
            let prev_xref_stream_start = trailer.remove(b"XRefStm");
            if let Some(prev) = prev_xref_stream_start.and_then(|offset| offset.as_i64().ok()) {
                let prev = prev as usize;
                if prev > self.buffer.len() {
                    return Err(Error::Xref(XrefError::StreamStart));
                }
                let (prev_xref, _) = parser::xref_and_trailer(&self.buffer[prev..], &self)?;
                xref.extend(prev_xref);
            }

            prev_xref_start = prev_trailer.remove(b"Prev");
        }

        let xref_entry_count = xref.max_id() + 1;
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
                    .read_object(offset as usize, None)
                    .map_err(|e| error!("Object load error: {:?}", e))
                    .ok()?;
                if let Ok(ref mut stream) = object.as_stream_mut() {
                    if stream.dict.type_is(b"ObjStm") {
                        let obj_stream = ObjectStream::new(stream).ok()?;
                        let mut object_streams = object_streams.lock().unwrap();
                        object_streams.extend(obj_stream.objects);
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
        self.document.objects.extend(object_streams.into_inner().unwrap());

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

        stream.dict.get(b"Length").and_then(|value| {
            if let Ok(id) = value.as_reference() {
                return self.document.get_object(id).and_then(Object::as_i64);
            }
            value.as_i64()
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

    pub fn get_object(&self, id: ObjectId) -> Result<Object> {
        let offset = self.get_offset(id)?;
        let (_, obj) = self.read_object(offset as usize, Some(id))?;

        Ok(obj)
    }

    fn read_object(&self, offset: usize, expected_id: Option<ObjectId>) -> Result<(ObjectId, Object)> {
        if offset > self.buffer.len() {
            return Err(Error::Offset(offset));
        }

        parser::indirect_object(&self.buffer, offset, expected_id, self)
    }

    fn get_xref_start(buffer: &[u8]) -> Result<usize> {
        let seek_pos = buffer.len() - cmp::min(buffer.len(), 512);
        Self::search_substring(buffer, b"%%EOF", seek_pos)
            .and_then(|eof_pos| if eof_pos > 25 { Some(eof_pos) } else { None })
            .and_then(|eof_pos| Self::search_substring(buffer, b"startxref", eof_pos - 25))
            .ok_or(Error::Xref(XrefError::Start))
            .and_then(|xref_pos| {
                if xref_pos <= buffer.len() {
                    match parser::xref_start(&buffer[xref_pos..]) {
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

#[test]
fn load_document() {
    let mut doc = Document::load("assets/example.pdf").unwrap();
    assert_eq!(doc.version, "1.5");
    doc.save("test_2_load.pdf").unwrap();
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
        .map(|x| x.chars())
        .flatten()
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
    let pages = doc.get_pages().keys().map(|r| *r).collect::<Vec<_>>();
    assert_eq!("Hello World!\n", doc.extract_text(&pages).unwrap());
}
