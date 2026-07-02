use crate::encodings;
use crate::encodings::cmap::ToUnicodeCMap;
use crate::encodings::{Differences, Encoding, Glyph};
use crate::error::DecompressError;
use crate::{Document, Error, Result};
use indexmap::IndexMap;
use log::warn;
use std::cmp::max;
use std::collections::HashSet;
use std::fmt;
use std::str;

/// Object identifier consists of two parts: object number and generation number.
pub type ObjectId = (u32, u16);

/// Dictionary object.
#[derive(Clone, Default, PartialEq)]
pub struct Dictionary(IndexMap<Vec<u8>, Object>);

/// Stream object
/// Warning - all streams must be indirect objects, while
/// the stream dictionary may be a direct object
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    /// Associated stream dictionary
    pub dict: Dictionary,
    /// Contents of the stream in bytes
    pub content: Vec<u8>,
    /// Can the stream be compressed by the `Document::compress()` function?
    /// Font streams may not be compressed, for example
    pub allows_compression: bool,
    /// Stream data's position in PDF file.
    pub start_position: Option<usize>,
}

/// Basic PDF object types defined in an enum.
#[derive(Clone, PartialEq)]
pub enum Object {
    Null,
    Boolean(bool),
    Integer(i64),
    Real(f32),
    Name(Vec<u8>),
    String(Vec<u8>, StringFormat),
    Array(Vec<Object>),
    Dictionary(Dictionary),
    Stream(Stream),
    Reference(ObjectId),
}

/// String objects can be written in two formats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StringFormat {
    #[default]
    Literal,
    Hexadecimal,
}

impl From<bool> for Object {
    fn from(value: bool) -> Self {
        Object::Boolean(value)
    }
}

impl From<i64> for Object {
    fn from(number: i64) -> Self {
        Object::Integer(number)
    }
}

macro_rules! from_smaller_ints {
	($( $Int: ty )+) => {
		$(
			impl From<$Int> for Object {
				fn from(number: $Int) -> Self {
					Object::Integer(i64::from(number))
				}
			}
		)+
	}
}

from_smaller_ints! {
    i8 i16 i32
    u8 u16 u32
}

impl From<f64> for Object {
    fn from(number: f64) -> Self {
        Object::Real(number as f32)
    }
}

impl From<f32> for Object {
    fn from(number: f32) -> Self {
        Object::Real(number)
    }
}

impl From<String> for Object {
    fn from(name: String) -> Self {
        Object::Name(name.into_bytes())
    }
}

impl<'a> From<&'a str> for Object {
    fn from(name: &'a str) -> Self {
        Object::Name(name.as_bytes().to_vec())
    }
}

impl From<Vec<Object>> for Object {
    fn from(array: Vec<Object>) -> Self {
        Object::Array(array)
    }
}

impl From<Dictionary> for Object {
    fn from(dict: Dictionary) -> Self {
        Object::Dictionary(dict)
    }
}

impl From<Stream> for Object {
    fn from(stream: Stream) -> Self {
        Object::Stream(stream)
    }
}

impl From<ObjectId> for Object {
    fn from(id: ObjectId) -> Self {
        Object::Reference(id)
    }
}

impl Object {
    pub fn string_literal<S: Into<Vec<u8>>>(s: S) -> Self {
        Object::String(s.into(), StringFormat::Literal)
    }

    pub fn is_null(&self) -> bool {
        matches!(*self, Object::Null)
    }

    pub fn as_bool(&self) -> Result<bool> {
        match self {
            Object::Boolean(value) => Ok(*value),
            _ => Err(Error::ObjectType {
                expected: "Boolean",
                found: self.enum_variant(),
            }),
        }
    }

    pub fn as_i64(&self) -> Result<i64> {
        match self {
            Object::Integer(value) => Ok(*value),
            _ => Err(Error::ObjectType {
                expected: "Integer",
                found: self.enum_variant(),
            }),
        }
    }

    pub fn as_f32(&self) -> Result<f32> {
        match self {
            Object::Real(value) => Ok(*value),
            _ => Err(Error::ObjectType {
                expected: "Real",
                found: self.enum_variant(),
            }),
        }
    }

    /// Get the object value as a float.
    /// Unlike [`Object::as_f32`] this will also cast an Integer to a Real.
    pub fn as_float(&self) -> Result<f32> {
        match self {
            Object::Integer(value) => Ok(*value as f32),
            Object::Real(value) => Ok(*value),
            _ => Err(Error::ObjectType {
                expected: "Integer or Real",
                found: self.enum_variant(),
            }),
        }
    }

    pub fn as_name(&self) -> Result<&[u8]> {
        match self {
            Object::Name(name) => Ok(name),
            _ => Err(Error::ObjectType {
                expected: "Name",
                found: self.enum_variant(),
            }),
        }
    }

    pub fn as_str(&self) -> Result<&[u8]> {
        match self {
            Object::String(string, _) => Ok(string),
            _ => Err(Error::ObjectType {
                expected: "String",
                found: self.enum_variant(),
            }),
        }
    }

    pub fn as_str_mut(&mut self) -> Result<&mut Vec<u8>> {
        match self {
            Object::String(string, _) => Ok(string),
            _ => Err(Error::ObjectType {
                expected: "String",
                found: self.enum_variant(),
            }),
        }
    }

    pub fn as_reference(&self) -> Result<ObjectId> {
        match self {
            Object::Reference(id) => Ok(*id),
            _ => Err(Error::ObjectType {
                expected: "Reference",
                found: self.enum_variant(),
            }),
        }
    }

    pub fn as_array(&self) -> Result<&Vec<Object>> {
        match self {
            Object::Array(arr) => Ok(arr),
            _ => Err(Error::ObjectType {
                expected: "Array",
                found: self.enum_variant(),
            }),
        }
    }

    pub fn as_array_mut(&mut self) -> Result<&mut Vec<Object>> {
        match self {
            Object::Array(arr) => Ok(arr),
            _ => Err(Error::ObjectType {
                expected: "Array",
                found: self.enum_variant(),
            }),
        }
    }

    pub fn as_dict(&self) -> Result<&Dictionary> {
        match self {
            Object::Dictionary(dict) => Ok(dict),
            _ => Err(Error::ObjectType {
                expected: "Dictionary",
                found: self.enum_variant(),
            }),
        }
    }

    pub fn as_dict_mut(&mut self) -> Result<&mut Dictionary> {
        match self {
            Object::Dictionary(dict) => Ok(dict),
            _ => Err(Error::ObjectType {
                expected: "Dictionary",
                found: self.enum_variant(),
            }),
        }
    }

    pub fn as_stream(&self) -> Result<&Stream> {
        match self {
            Object::Stream(stream) => Ok(stream),
            _ => Err(Error::ObjectType {
                expected: "Stream",
                found: self.enum_variant(),
            }),
        }
    }

    pub fn as_stream_mut(&mut self) -> Result<&mut Stream> {
        match self {
            Object::Stream(stream) => Ok(stream),
            _ => Err(Error::ObjectType {
                expected: "Stream",
                found: self.enum_variant(),
            }),
        }
    }

    // TODO: maybe remove
    pub fn type_name(&self) -> Result<&[u8]> {
        match self {
            Object::Dictionary(dict) => dict.get_type(),
            Object::Stream(stream) => stream.dict.get_type(),
            obj => Err(Error::ObjectType {
                expected: "Dictionary or Stream",
                found: obj.enum_variant(),
            }),
        }
    }

    pub fn enum_variant(&self) -> &'static str {
        match self {
            Object::Null => "Null",
            Object::Boolean(_) => "Boolean",
            Object::Integer(_) => "Integer",
            Object::Real(_) => "Real",
            Object::Name(_) => "Name",
            Object::String(_, _) => "String",
            Object::Array(_) => "Array",
            Object::Dictionary(_) => "Dictionary",
            Object::Stream(_) => "Stream",
            Object::Reference(_) => "Reference",
        }
    }
}

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Object::Null => write!(f, "Null"),
            Object::Boolean(value) => write!(f, "{value}"),
            Object::Integer(value) => write!(f, "{value}"),
            Object::Real(value) => write!(f, "{value}"),
            Object::Name(name) => write!(f, "/{}", String::from_utf8_lossy(name)),
            Object::String(text, StringFormat::Literal) => write!(f, "({})", String::from_utf8_lossy(text)),
            Object::String(text, StringFormat::Hexadecimal) => {
                write!(f, "<")?;
                for b in text {
                    write!(f, "{b:02x}")?
                }
                write!(f, ">")
            }
            Object::Array(array) => {
                let items = array.iter().map(|item| format!("{item:?}")).collect::<Vec<String>>();
                write!(f, "[{}]", items.join(" "))
            }
            Object::Dictionary(dict) => write!(f, "{dict:?}"),
            Object::Stream(stream) => write!(f, "{:?}stream...endstream", stream.dict),
            Object::Reference(id) => write!(f, "{} {} R", id.0, id.1),
        }
    }
}

impl Dictionary {
    pub fn new() -> Dictionary {
        Dictionary(IndexMap::new())
    }

    pub fn has(&self, key: &[u8]) -> bool {
        self.0.contains_key(key)
    }

    pub fn get(&self, key: &[u8]) -> Result<&Object> {
        self.0
            .get(key)
            .ok_or(Error::DictKey(String::from_utf8_lossy(key).to_string()))
    }

    /// Extract object from dictionary, dereferencing
    /// the object if it is a reference.
    pub fn get_deref<'a>(&'a self, key: &[u8], doc: &'a Document) -> Result<&'a Object> {
        doc.dereference(self.get(key)?).map(|(_, object)| object)
    }

    pub fn get_mut(&mut self, key: &[u8]) -> Result<&mut Object> {
        self.0
            .get_mut(key)
            .ok_or(Error::DictKey(String::from_utf8_lossy(key).to_string()))
    }

    pub fn set<K, V>(&mut self, key: K, value: V)
    where
        K: Into<Vec<u8>>,
        V: Into<Object>,
    {
        self.0.insert(key.into(), value.into());
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.len() == 0
    }

    pub fn remove(&mut self, key: &[u8]) -> Option<Object> {
        self.0.swap_remove(key)
    }

    pub fn has_type(&self, type_name: &[u8]) -> bool {
        self.get(b"Type").and_then(|s| s.as_name()).ok() == Some(type_name)
    }

    pub fn get_type(&self) -> Result<&[u8]> {
        self.get(b"Type")
            .and_then(Object::as_name)
            .or_else(|_| self.get(b"Linearized").and(Ok(b"Linearized")))
    }

    pub fn iter(&'_ self) -> indexmap::map::Iter<'_, Vec<u8>, Object> {
        self.0.iter()
    }

    pub fn iter_mut(&'_ mut self) -> indexmap::map::IterMut<'_, Vec<u8>, Object> {
        self.0.iter_mut()
    }

    pub fn get_font_encoding<'a>(&'a self, doc: &'a Document) -> Result<Encoding<'a>> {
        if !self.has_type(b"Font") {
            return Err(Error::DictType {
                expected: "Font",
                found: String::from_utf8_lossy(self.get_type().unwrap_or(b"None")).to_string(),
            });
        }

        // Note: currently not all encodings are handled, not implemented:
        // - TrueType cmap tables
        // - DescendantFonts in CID-Keyed fonts
        // - Predefined CJK CMAP other than indicated in SimpleEncoding
        // - Deciding what should be the fallback font if no such encoding is defined in difference encoding (see Table
        //   114 in 9.6.6.1 General under `BaseEncoding`).
        let result = (|| {
            if let Ok(object) = self.get(b"Encoding") {
                return self.get_base_encoding(object, doc);
            }

            if let Ok(stream) = self.get_deref(b"ToUnicode", doc).and_then(Object::as_stream) {
                return self.get_to_unicode_encoding(stream);
            }

            Ok(Encoding::OneByteEncoding(&encodings::STANDARD_ENCODING))
        })();

        match result {
            Ok(encoding) => Ok(encoding),
            Err(err) => {
                warn!(
                    "Could not parse the encoding, error: {err:#?}\nFont: {self:#?}. Using standard encoding as a fallback!"
                );
                Ok(Encoding::OneByteEncoding(&encodings::STANDARD_ENCODING))
            }
        }
    }

    /// Get a simple encoding from the /Encoding entry of a font dictionary.
    fn get_base_encoding<'a>(&'a self, mut object: &'a Object, doc: &'a Document) -> Result<Encoding<'a>> {
        // Set of visited to detect circular references.
        let mut visited = HashSet::new();

        loop {
            match *object {
                Object::Name(ref name) => {
                    return self.base_encoding(doc, name);
                }
                Object::Reference(id) => {
                    if !visited.insert(id) {
                        return Err(Error::ReferenceCycle(id));
                    }

                    let Ok(o) = doc.get_object(id) else {
                        return Err(Error::ObjectNotFound(id));
                    };

                    object = o;
                }
                Object::Dictionary(ref dict) => {
                    let ty = dict.get(b"Type")?.as_name()?;

                    match ty {
                        b"Encoding" => {
                            let mut base = None;

                            if let Ok(base_encoding) = dict.get(b"BaseEncoding")
                                && let Ok(name) = base_encoding.as_name()
                            {
                                base = Some(self.base_encoding(doc, name)?);
                            }

                            let base = match base {
                                Some(base) => base,
                                None => Encoding::OneByteEncoding(&encodings::STANDARD_ENCODING),
                            };

                            let differences = dict.get(b"Differences")?.as_array()?;
                            let differences = self.differences(base, differences)?;
                            return Ok(Encoding::Differences(differences));
                        }
                        _ => {
                            return Err(Error::ObjectType {
                                expected: "Encoding Dictionary",
                                found: "Dictionary with Type other than /Encoding",
                            });
                        }
                    }
                }
                ref object => {
                    return Err(Error::ObjectType {
                        expected: "Name or Reference or Dictionary",
                        found: object.enum_variant(),
                    });
                }
            }
        }
    }

    fn base_encoding<'a>(&'a self, doc: &'a Document, name: &'a [u8]) -> Result<Encoding<'a>> {
        match name {
            b"StandardEncoding" => Ok(Encoding::OneByteEncoding(&encodings::STANDARD_ENCODING)),
            b"MacRomanEncoding" => Ok(Encoding::OneByteEncoding(&encodings::MAC_ROMAN_ENCODING)),
            b"MacExpertEncoding" => Ok(Encoding::OneByteEncoding(&encodings::MAC_EXPERT_ENCODING)),
            b"WinAnsiEncoding" => Ok(Encoding::OneByteEncoding(&encodings::WIN_ANSI_ENCODING)),
            b"PDFDocEncoding" => {
                log::warn!("PDFDocEncoding is not a valid character encoding for a font");
                Ok(Encoding::OneByteEncoding(&encodings::PDF_DOC_ENCODING))
            }
            b"Identity-H" | b"Identity-V" => {
                let stream = self.get_deref(b"ToUnicode", doc)?.as_stream()?;
                self.get_to_unicode_encoding(stream)
            }
            name => Ok(Encoding::SimpleEncoding(name)),
        }
    }

    fn differences<'a>(&'a self, base: Encoding<'a>, array: &[Object]) -> Result<Differences<'a>> {
        let mut map = IndexMap::new();
        let mut inverse = IndexMap::new();
        let mut current_code = 0;

        for obj in array {
            match *obj {
                Object::Integer(code) => {
                    if !(0..=255).contains(&code) {
                        return Err(Error::InvalidEncodingDifferenceCode { code });
                    }

                    current_code = code as u8;
                }
                Object::Name(ref name) => {
                    let Some(glyph) = Glyph::from_name(name) else {
                        return Err(Error::InvalidEncodingDifferenceGlyph {
                            name: String::from_utf8_lossy(name).into_owned(),
                        });
                    };

                    map.insert(current_code, glyph);
                    inverse.insert(glyph, current_code);
                    current_code = current_code.wrapping_add(1);
                }
                _ => {
                    return Err(Error::ObjectType {
                        expected: "Integer or Name",
                        found: obj.enum_variant(),
                    });
                }
            }
        }

        Ok(Differences {
            base: Box::new(base),
            map,
            inverse,
        })
    }

    fn get_to_unicode_encoding(&'_ self, stream: &Stream) -> Result<Encoding<'_>> {
        let content = stream.get_plain_content()?;
        let cmap = ToUnicodeCMap::parse(content)?;
        Ok(Encoding::UnicodeMapEncoding(cmap))
    }

    pub fn extend(&mut self, other: &Dictionary) {
        let keep_both_objects =
            |new_dict: &mut IndexMap<Vec<u8>, Object>, key: &Vec<u8>, value: &Object, old_value: Object| {
                let mut final_array;

                match value {
                    Object::Array(array) => {
                        final_array = Vec::with_capacity(array.len() + 1);
                        final_array.push(old_value);
                        final_array.extend(array.to_owned());
                    }
                    _ => {
                        final_array = vec![value.to_owned(), old_value];
                    }
                }

                new_dict.insert(key.to_owned(), Object::Array(final_array));
            };

        let mut new_dict = std::mem::take(&mut self.0);
        new_dict.reserve_exact(other.0.len());

        for (key, value) in other.0.iter() {
            if let Some(old_value) = new_dict.get(key) {
                let old_value = old_value.to_owned();
                match (&old_value, value) {
                    (Object::Dictionary(old_dict), Object::Dictionary(dict)) => {
                        let mut replaced_dict = old_dict.to_owned();
                        replaced_dict.extend(dict);
                        new_dict.insert(key.to_owned(), Object::Dictionary(replaced_dict));
                    }
                    (Object::Array(old_array), Object::Array(array)) => {
                        let mut replaced_array = old_array.to_owned();
                        replaced_array.extend(array.to_owned());
                        new_dict.insert(key.to_owned(), Object::Array(replaced_array));
                    }
                    (Object::Integer(old_id), Object::Integer(id)) => {
                        let array = vec![Object::Integer(*old_id), Object::Integer(*id)];
                        new_dict.insert(key.to_owned(), Object::Array(array));
                    }
                    (Object::Real(old_id), Object::Real(id)) => {
                        let array = vec![Object::Real(*old_id), Object::Real(*id)];
                        new_dict.insert(key.to_owned(), Object::Array(array));
                    }
                    (Object::String(old_ids, old_format), Object::String(ids, format)) => {
                        let array = vec![
                            Object::String(old_ids.to_owned(), old_format.to_owned()),
                            Object::String(ids.to_owned(), format.to_owned()),
                        ];
                        new_dict.insert(key.to_owned(), Object::Array(array));
                    }
                    (Object::Reference(old_object_id), Object::Reference(object_id)) => {
                        let array = vec![Object::Reference(*old_object_id), Object::Reference(*object_id)];
                        new_dict.insert(key.to_owned(), Object::Array(array));
                    }
                    (Object::Null, _) | (Object::Boolean(_), _) | (Object::Name(_), _) | (Object::Stream(_), _) => {
                        new_dict.insert(key.to_owned(), old_value);
                    }
                    (_, _) => keep_both_objects(&mut new_dict, key, value, old_value),
                }
            } else {
                new_dict.insert(key.to_owned(), value.to_owned());
            }
        }

        self.0 = new_dict;
    }

    /// Return a reference to the inner  [`IndexMap`].
    pub fn as_hashmap(&self) -> &IndexMap<Vec<u8>, Object> {
        &self.0
    }

    /// Return a mut reference to the inner [`IndexMap`].
    pub fn as_hashmap_mut(&mut self) -> &mut IndexMap<Vec<u8>, Object> {
        &mut self.0
    }
}

#[macro_export]
macro_rules! dictionary {
	() => {
		$crate::Dictionary::new()
	};
	($( $key: expr => $value: expr ),+ ,) => {
		dictionary!( $($key => $value),+ )
	};
	($( $key: expr => $value: expr ),*) => {{
		let mut dict = $crate::Dictionary::new();
		$(
			dict.set($key, $value);
		)*
		dict
	}}
}

impl fmt::Debug for Dictionary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entries = self
            .into_iter()
            .map(|(key, value)| format!("/{} {:?}", String::from_utf8_lossy(key), value))
            .collect::<Vec<String>>();
        write!(f, "<<{}>>", entries.concat())
    }
}

impl IntoIterator for Dictionary {
    type Item = (Vec<u8>, Object);
    type IntoIter = indexmap::map::IntoIter<Vec<u8>, Object>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Dictionary {
    type Item = (&'a Vec<u8>, &'a Object);
    type IntoIter = indexmap::map::Iter<'a, Vec<u8>, Object>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a mut Dictionary {
    type Item = (&'a Vec<u8>, &'a mut Object);
    type IntoIter = indexmap::map::IterMut<'a, Vec<u8>, Object>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

use std::iter::FromIterator;
impl<K: Into<Vec<u8>>> FromIterator<(K, Object)> for Dictionary {
    fn from_iter<I: IntoIterator<Item = (K, Object)>>(iter: I) -> Self {
        let mut dict = Dictionary::new();
        for (k, v) in iter {
            dict.set(k, v);
        }
        dict
    }
}

/// A [`std::io::Write`] adapter that appends to a `Vec<u8>` but refuses to grow
/// it past `limit` bytes. Used to bound decoders (e.g. LZW) that write into a
/// sink rather than being read from, so a decompression bomb cannot allocate an
/// unbounded amount of memory. On overflow it fills up to exactly `limit` bytes
/// and then returns an error, so the caller can detect the overflow by length.
struct LimitedWriter<'a> {
    inner: &'a mut Vec<u8>,
    limit: usize,
}

impl<'a> LimitedWriter<'a> {
    fn new(inner: &'a mut Vec<u8>, limit: usize) -> Self {
        LimitedWriter { inner, limit }
    }
}

impl std::io::Write for LimitedWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let remaining = self.limit.saturating_sub(self.inner.len());
        if buf.len() > remaining {
            // Fill up to the limit so the caller's `len() > max` check trips,
            // then signal that no more may be written.
            self.inner.extend_from_slice(&buf[..remaining]);
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "decompression output exceeded limit",
            ));
        }
        self.inner.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Stream {
    pub fn new(mut dict: Dictionary, content: Vec<u8>) -> Stream {
        dict.set("Length", content.len() as i64);
        Stream {
            dict,
            content,
            allows_compression: true,
            start_position: None,
        }
    }

    pub fn with_position(dict: Dictionary, position: usize) -> Stream {
        Stream {
            dict,
            content: vec![],
            allows_compression: true,
            start_position: Some(position),
        }
    }

    /// Default is that the stream may be compressed. On font streams,
    /// set this to false, otherwise the font will be corrupt
    #[inline]
    pub fn with_compression(mut self, allows_compression: bool) -> Stream {
        self.allows_compression = allows_compression;
        self
    }

    pub fn filters(&self) -> Result<Vec<&[u8]>> {
        let filter = self.dict.get(b"Filter")?;

        if let Ok(name) = filter.as_name() {
            Ok(vec![name])
        } else if let Ok(names) = filter.as_array() {
            names.iter().map(Object::as_name).collect()
        } else {
            Err(Error::ObjectType {
                expected: "Name or Array",
                found: filter.enum_variant(),
            })
        }
    }

    pub fn set_content(&mut self, content: Vec<u8>) {
        self.content = content;
        self.dict.set("Length", self.content.len() as i64);
    }

    pub fn set_plain_content(&mut self, content: Vec<u8>) {
        self.dict.remove(b"DecodeParms");
        self.dict.remove(b"Filter");
        self.dict.set("Length", content.len() as i64);
        self.content = content;
    }

    pub fn get_plain_content(&self) -> Result<Vec<u8>> {
        match self.filters() {
            Ok(vec) if !vec.is_empty() => self.decompressed_content(),
            _ => Ok(self.content.clone()),
        }
    }

    pub fn compress(&mut self) -> Result<()> {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::prelude::*;

        if self.dict.get(b"Filter").is_err() {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
            encoder.write_all(self.content.as_slice())?;
            let compressed = encoder.finish()?;
            if compressed.len() + 19 < self.content.len() {
                self.dict.set("Filter", "FlateDecode");
                self.set_content(compressed);
            }
        }
        Ok(())
    }

    /// Decode this stream's content, applying its filters in order.
    ///
    /// # Warning: unbounded output
    ///
    /// This method places **no limit** on the size of the decompressed output.
    /// A small compressed stream can inflate to an enormous size (a
    /// "decompression bomb"), so calling this on untrusted input can exhaust
    /// all available memory. When processing PDFs from an untrusted source,
    /// prefer [`Stream::decompressed_content_with_limit`] or
    /// [`Stream::decompress_to_writer`], and load documents with
    /// [`crate::LoadOptions::max_decompressed_size`] set.
    pub fn decompressed_content(&self) -> Result<Vec<u8>> {
        self.decode_filters(None)
    }

    /// Decode this stream's content, rejecting the stream with
    /// [`DecompressError::MemoryLimitExceeded`] if the decoded output would
    /// exceed `max_output` bytes.
    ///
    /// This is the bomb-safe counterpart to [`Stream::decompressed_content`].
    /// Each filter in the chain is bounded individually, so a stream with nested
    /// filters (e.g. `/Filter [FlateDecode FlateDecode]`) can never allocate
    /// more than roughly `max_output` bytes per layer before being rejected,
    /// rather than expanding without limit.
    pub fn decompressed_content_with_limit(&self, max_output: usize) -> Result<Vec<u8>> {
        self.decode_filters(Some(max_output))
    }

    /// Decode this stream's content into a caller-provided writer, rejecting the
    /// stream if the decoded output would exceed `max_output` bytes.
    ///
    /// The result is decoded with the same bomb-safe bound as
    /// [`Stream::decompressed_content_with_limit`] (internal buffering is capped
    /// at roughly `max_output` bytes) and then written to `writer`, which lets
    /// callers direct the output straight into a file or a fixed-capacity buffer
    /// of their choosing. Returns the number of bytes written on success, or
    /// [`DecompressError::MemoryLimitExceeded`] if the limit would be exceeded.
    pub fn decompress_to_writer<W: std::io::Write>(&self, writer: &mut W, max_output: usize) -> Result<usize> {
        let data = self.decompressed_content_with_limit(max_output)?;
        writer.write_all(&data)?;
        Ok(data.len())
    }

    /// Shared decoder for [`Stream::decompressed_content`] and its bounded
    /// variants. `limit` is `None` to decode without a size limit, or
    /// `Some(max)` to cap the decoded output at `max` bytes per filter layer.
    fn decode_filters(&self, limit: Option<usize>) -> Result<Vec<u8>> {
        let params = self.dict.get(b"DecodeParms").and_then(Object::as_dict).ok();
        let filters = match self.filters() {
            Ok(f) => f,
            // No /Filter key means the stream is uncompressed. The raw content is
            // already in memory, but still honor the caller's limit.
            Err(_) => {
                if let Some(max) = limit
                    && self.content.len() > max
                {
                    return Err(DecompressError::MemoryLimitExceeded { limit: max }.into());
                }
                return Ok(self.content.clone());
            }
        };

        let mut input = self.content.as_slice();
        let mut output = vec![];

        // Filters are in decoding order.
        for filter in filters {
            output = match filter {
                b"FlateDecode" => Self::decompress_zlib(input, params, limit)?,
                b"LZWDecode" => Self::decompress_lzw(input, params, limit)?,
                b"ASCII85Decode" => Self::decode_ascii85(input, limit)?,
                _ => return Err(Error::Unimplemented("decompression algorithms")),
            };
            if let Some(max) = limit
                && output.len() > max
            {
                return Err(DecompressError::MemoryLimitExceeded { limit: max }.into());
            }
            input = &output;
        }
        Ok(output)
    }

    /// Read `reader` to end, appending to `output`. When `limit` is `Some(max)`,
    /// at most `max + 1` bytes are read, so an oversized (bomb) stream is
    /// detected via the `> max` check by the caller without ever allocating the
    /// full decompressed output.
    fn read_capped<R: std::io::Read>(reader: R, output: &mut Vec<u8>, limit: Option<usize>) -> std::io::Result<()> {
        use std::io::Read;
        match limit {
            Some(max) => Read::take(reader, (max as u64).saturating_add(1))
                .read_to_end(output)
                .map(|_| ()),
            None => {
                let mut reader = reader;
                reader.read_to_end(output).map(|_| ())
            }
        }
    }

    fn decompress_lzw(input: &[u8], params: Option<&Dictionary>, limit: Option<usize>) -> Result<Vec<u8>> {
        use weezl::{BitOrder, decode::Decoder};
        const MIN_BITS: u8 = 9;

        let early_change = params
            .and_then(|p| p.get(b"EarlyChange").ok())
            .and_then(|p| Object::as_i64(p).ok())
            .map(|v| v != 0)
            .unwrap_or(true);

        let mut decoder = if early_change {
            Decoder::with_tiff_size_switch(BitOrder::Msb, MIN_BITS - 1)
        } else {
            Decoder::new(BitOrder::Msb, MIN_BITS - 1)
        };

        let output = Self::decompress_lzw_loop(input, &mut decoder, limit);
        if let Some(max) = limit
            && output.len() > max
        {
            return Err(DecompressError::MemoryLimitExceeded { limit: max }.into());
        }
        Self::decompress_predictor(output, params)
    }

    fn decompress_lzw_loop(input: &[u8], decoder: &mut weezl::decode::Decoder, limit: Option<usize>) -> Vec<u8> {
        let mut output = vec![];

        // When a limit is set, decode into a writer that stops accepting bytes
        // once `max + 1` have been produced, so a bomb cannot allocate the full
        // (potentially enormous) output; the caller rejects it via the `> max`
        // check. Without a limit, decode straight into the output vector.
        let status = match limit {
            Some(max) => {
                let mut sink = LimitedWriter::new(&mut output, max.saturating_add(1));
                decoder.into_stream(&mut sink).decode_all(input).status
            }
            None => decoder.into_stream(&mut output).decode_all(input).status,
        };
        if let Err(err) = status {
            warn!("{err}");
        }

        output
    }

    fn decompress_zlib(input: &[u8], params: Option<&Dictionary>, limit: Option<usize>) -> Result<Vec<u8>> {
        use flate2::read::ZlibDecoder;

        // Reserve a starting capacity, but never pre-allocate beyond the limit so
        // a bomb cannot force a huge allocation up front.
        let initial_capacity = match limit {
            Some(max) => input.len().saturating_mul(2).min(max.saturating_add(1)),
            None => input.len().saturating_mul(2),
        };
        let mut output = Vec::with_capacity(initial_capacity);

        if !input.is_empty()
            && let Err(err) = Self::read_capped(ZlibDecoder::new(input), &mut output, limit)
        {
            warn!("{err}");
            // Zlib decompression failed (e.g. corrupt adler32 checksum in
            // encrypted PDFs). Retry with raw deflate, skipping the 2-byte
            // zlib header and ignoring the checksum.
            if output.is_empty() && input.len() > 2 {
                use flate2::read::DeflateDecoder;
                if let Err(raw_err) = Self::read_capped(DeflateDecoder::new(&input[2..]), &mut output, limit) {
                    warn!("raw deflate fallback also failed: {raw_err}");
                }
            }
        }
        if let Some(max) = limit
            && output.len() > max
        {
            return Err(DecompressError::MemoryLimitExceeded { limit: max }.into());
        }
        Self::decompress_predictor(output, params)
    }

    fn decode_ascii85(input: &[u8], limit: Option<usize>) -> Result<Vec<u8>> {
        let mut output = vec![];
        let mut buffer: u32 = 0;
        let mut count = 0;
        // Check for EOD marker
        let input_no_eod = if input.len() >= 2 && &input[input.len() - 2..] == b"~>" {
            &input[..input.len() - 2]
        } else {
            log::warn!("ASCII85 stream is missing its EOD marker");
            input
        };
        for &ch in input_no_eod {
            // ASCII85 can amplify up to 4x (each `z` expands to four zero bytes),
            // so bound the output here rather than only after the fact; each
            // iteration appends at most 4 bytes, so output never exceeds max + 4.
            if let Some(max) = limit
                && output.len() > max
            {
                return Err(DecompressError::MemoryLimitExceeded { limit: max }.into());
            }
            if ch == b'z' {
                if count != 0 {
                    return Err(DecompressError::Ascii85("z character is not allowed in the middle of a group").into());
                }
                output.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }

            if ch.is_ascii_whitespace() {
                continue;
            }

            if !(b'!'..=b'u').contains(&ch) {
                break;
            }
            buffer = buffer
                .checked_mul(85)
                .ok_or(DecompressError::Ascii85("multiplication overflow"))?;
            buffer += (ch - b'!') as u32;
            count += 1;

            if count == 5 {
                output.extend_from_slice(&buffer.to_be_bytes());
                buffer = 0;
                count = 0;
            }
        }

        if count > 0 {
            for _ in count..5 {
                buffer = buffer
                    .checked_mul(85)
                    .ok_or(DecompressError::Ascii85("multiplication overflow"))?;
                buffer += 84;
            }

            let bytes = buffer.to_be_bytes();
            output.extend_from_slice(&bytes[..count - 1]);
        }

        Ok(output)
    }

    fn decompress_predictor(mut data: Vec<u8>, params: Option<&Dictionary>) -> Result<Vec<u8>> {
        use crate::filters::png;

        if let Some(params) = params {
            let predictor = params.get(b"Predictor").and_then(Object::as_i64).unwrap_or(1);
            if (10..=15).contains(&predictor) {
                let pixels_per_row = max(1, params.get(b"Columns").and_then(Object::as_i64).unwrap_or(1)) as usize;
                let colors = max(1, params.get(b"Colors").and_then(Object::as_i64).unwrap_or(1)) as usize;
                let bits = max(8, params.get(b"BitsPerComponent").and_then(Object::as_i64).unwrap_or(8)) as usize;
                let bytes_per_pixel = colors * bits / 8;
                data = png::decode_frame(data.as_slice(), bytes_per_pixel, pixels_per_row)?;
            }
            Ok(data)
        } else {
            Ok(data)
        }
    }

    pub fn decompress(&mut self) -> Result<()> {
        let data = self.decompressed_content()?;
        self.dict.remove(b"DecodeParms");
        self.dict.remove(b"Filter");
        self.set_content(data);
        Ok(())
    }

    /// Decompress this stream in place like [`Stream::decompress`], but reject it
    /// with [`DecompressError::MemoryLimitExceeded`] if the decoded output would
    /// exceed `max_output` bytes. Used on the load path to bound the memory a
    /// single object/xref stream can consume.
    pub fn decompress_with_limit(&mut self, max_output: usize) -> Result<()> {
        let data = self.decompressed_content_with_limit(max_output)?;
        self.dict.remove(b"DecodeParms");
        self.dict.remove(b"Filter");
        self.set_content(data);
        Ok(())
    }

    pub fn is_compressed(&self) -> bool {
        self.dict.get(b"Filter").is_ok()
    }
}

#[cfg(test)]
mod test {
    use crate::{Error, error::DecompressError};

    use super::Stream;

    #[test]
    fn test_decode_ascii85() {
        let input = r#"9jqo^BlbD-BleB1DJ+*+F(f,q/0JhKF<GL>Cj@.4Gp$d7F!,L7@<6@)/0JDEF<G%<+EV:2F!,O<
            DJ+*.@<*K0@<6L(Df-\0Ec5e;DffZ(EZee.Bl.9pF"AGXBPCsi+DGm>@3BB/F*&OCAfu2/AKYi(
            DIb:@FD,*)+C]U=@3BN#EcYf8ATD3s@q?d$AftVqCh[NqF<G:8+EV:.+Cf>-FD5W8ARlolDIal(
            DId<j@<?3r@:F%a+D58'ATD4$Bl@l3De:,-DJs`8ARoFb/0JMK@qB4^F!,R<AKZ&-DfTqBG%G>u
            D.RTpAKYo'+CT/5+Cei#DII?(E,9)oF*2M7/c~>"#;
        let expected = "Man is distinguished, not only by his reason, but by this singular passion from other animals, which is a lust of the mind, that by a perseverance of delight in the continued and indefatigable generation of knowledge, exceeds the short vehemence of any carnal pleasure.";
        let output = Stream::decode_ascii85(input.as_bytes(), None).unwrap();
        println!("{}", String::from_utf8(output.clone()).unwrap());
        assert_eq!(&output, expected.as_bytes());
    }

    #[test]
    fn test_decode_ascii85_overflow() {
        let input = b"uuuuu~>";
        let output = Stream::decode_ascii85(input, None);
        // let expected: Result<Vec<u8>, Error> = Err(Error::ContentDecode);
        assert!(matches!(output, Err(Error::Decompress(DecompressError::Ascii85(_)))));
    }

    #[test]
    fn test_decompress_zlib_corrupt_checksum() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        let original = b"BT /F1 12 Tf (Hello World) Tj ET";

        // Compress with valid zlib
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let mut compressed = encoder.finish().unwrap();

        // Corrupt the adler32 checksum (last 4 bytes)
        let len = compressed.len();
        assert!(len >= 4);
        for byte in &mut compressed[len - 4..] {
            *byte ^= 0xFF;
        }

        // Normal zlib should fail, but our fallback should recover
        let result = Stream::decompress_zlib(&compressed, None, None).unwrap();
        assert_eq!(result, original);
    }

    #[test]
    fn test_uncompressed_stream_returns_raw_content() {
        use crate::Dictionary;

        // A stream with no /Filter should return its raw content from decompressed_content()
        let content = b"/FullPage Do
"
        .to_vec();
        let mut dict = Dictionary::new();
        dict.set("Length", content.len() as i64);
        let stream = Stream::new(dict, content.clone());

        let result = stream
            .decompressed_content()
            .expect("should succeed for uncompressed stream");
        assert_eq!(result, content);
    }

    #[test]
    fn test_uncompressed_stream_honors_limit() {
        use crate::Dictionary;

        // Even with no filter, an over-limit raw stream must be rejected so the
        // caller's memory bound is never silently exceeded.
        let content = vec![0u8; 1024];
        let mut dict = Dictionary::new();
        dict.set("Length", content.len() as i64);
        let stream = Stream::new(dict, content);

        assert!(matches!(
            stream.decompressed_content_with_limit(512),
            Err(Error::Decompress(DecompressError::MemoryLimitExceeded { limit: 512 }))
        ));
        assert!(stream.decompressed_content_with_limit(4096).is_ok());
    }

    #[test]
    fn test_limited_writer_fills_to_limit_then_errors() {
        use super::LimitedWriter;
        use std::io::Write;

        // The push-based (LZW) guard: the sink accepts bytes up to `limit`, then
        // fills the remaining room and refuses further writes, so the caller can
        // detect the overflow via `len() > max` without unbounded allocation.
        let mut buf = Vec::new();
        let mut writer = LimitedWriter::new(&mut buf, 4);

        assert_eq!(writer.write(b"ab").unwrap(), 2);
        // This write would cross the limit: it fills to exactly 4 bytes, errors.
        let err = writer.write(b"cdef").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::WriteZero);

        assert_eq!(buf, b"abcd");
    }

    #[test]
    fn test_ascii85_honors_limit() {
        // Each `z` expands to four zero bytes, so 1 MiB of `z` decodes to 4 MiB.
        // Without a limit that full 4x amplification is allocated; with a limit
        // the decoder must stop rather than expand past it (guards the chained
        // `[FlateDecode ASCII85Decode]` bomb vector on the load path).
        let mut input = vec![b'z'; 1024 * 1024];
        input.extend_from_slice(b"~>");

        let full = Stream::decode_ascii85(&input, None).unwrap();
        assert_eq!(full.len(), 4 * 1024 * 1024, "unbounded ASCII85 amplifies 4x");

        assert!(matches!(
            Stream::decode_ascii85(&input, Some(1024 * 1024)),
            Err(Error::Decompress(DecompressError::MemoryLimitExceeded { limit })) if limit == 1024 * 1024
        ));
    }

    #[test]
    fn test_lzw_bomb_rejected_with_limit() {
        use crate::Dictionary;
        use weezl::{BitOrder, encode::Encoder};

        // A tiny LZW stream that decodes back to 8 MiB of zeros. The encoder must
        // match lopdf's default decoder (`with_tiff_size_switch`, code size 8).
        let plain = vec![0u8; 8 * 1024 * 1024];
        let compressed = Encoder::with_tiff_size_switch(BitOrder::Msb, 8).encode(&plain).unwrap();
        assert!(compressed.len() < 128 * 1024, "LZW bomb input should be tiny");

        let mut dict = Dictionary::new();
        dict.set("Filter", "LZWDecode");
        let stream = Stream::new(dict, compressed);

        // Bounded: the push-based LZW path (LimitedWriter + weezl) rejects it.
        assert!(matches!(
            stream.decompressed_content_with_limit(1024 * 1024),
            Err(Error::Decompress(DecompressError::MemoryLimitExceeded { limit })) if limit == 1024 * 1024
        ));
        // Unbounded default still round-trips fully (proves the pairing is valid,
        // so the rejection above is real and not a decode failure).
        assert_eq!(stream.decompressed_content().unwrap().len(), 8 * 1024 * 1024);
    }
}
