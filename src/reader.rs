use log::{error, warn};
use std::cmp;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::TryInto;
#[cfg(not(feature = "async"))]
use std::fs::File;
#[cfg(not(feature = "async"))]
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[cfg(feature = "rayon")]
use rayon::prelude::*;
#[cfg(feature = "async")]
use tokio::fs::File;
#[cfg(feature = "async")]
use tokio::io::{AsyncRead, AsyncReadExt};
#[cfg(feature = "async")]
use tokio::pin;

use crate::common_data_structures;
use crate::encryption::{self, EncryptionState, PasswordAlgorithm};
use crate::error::{DecompressError, ParseError, XrefError};
use crate::linearization::Linearization;
use crate::load_options::{FilterFunc, LoadOptions};
use crate::object_stream::ObjectStream;
use crate::parser;
use crate::xref::{Xref, XrefEntry, XrefRecovery, XrefType, decode_xref_stream_with_limit};
use crate::{Dictionary, Document, Error, IncrementalDocument, Object, ObjectId, Result};

/// Open `path` and report its size, so the caller can pre-size the buffer it
/// reads the file into.
#[cfg(not(feature = "async"))]
fn open_file<P: AsRef<Path>>(path: P) -> Result<(File, usize)> {
    let file = File::open(path)?;
    let capacity = file.metadata()?.len() as usize;
    Ok((file, capacity))
}

#[cfg(not(feature = "async"))]
impl Document {
    /// Load a PDF document from a specified file path.
    #[inline]
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Document> {
        Self::load_with_options(path, LoadOptions::default())
    }

    /// Load a PDF document from a specified file path with the given options.
    #[inline]
    pub fn load_with_options<P: AsRef<Path>>(path: P, options: LoadOptions) -> Result<Document> {
        let (file, capacity) = open_file(path)?;
        Self::load_internal(file, Some(capacity), options)
    }

    /// Load a PDF document from a specified file path with a password for encrypted PDFs.
    #[inline]
    pub fn load_with_password<P: AsRef<Path>>(path: P, password: &str) -> Result<Document> {
        Self::load_with_options(path, LoadOptions::with_password(password))
    }

    #[deprecated(since = "0.41.0", note = "Use load_with_options instead")]
    #[inline]
    pub fn load_filtered<P: AsRef<Path>>(path: P, filter_func: FilterFunc) -> Result<Document> {
        Self::load_with_options(path, LoadOptions::with_filter(filter_func))
    }

    /// Load a PDF document from an arbitrary source.
    #[inline]
    pub fn load_from<R: Read>(source: R) -> Result<Document> {
        Self::load_from_with_options(source, LoadOptions::default())
    }

    /// Load a PDF document from an arbitrary source with the given options.
    #[inline]
    pub fn load_from_with_options<R: Read>(source: R, options: LoadOptions) -> Result<Document> {
        Self::load_internal(source, None, options)
    }

    /// Load a PDF document from an arbitrary source with a password for encrypted PDFs.
    #[deprecated(since = "0.41.0", note = "Use load_from_with_options instead")]
    #[inline]
    pub fn load_from_with_password<R: Read>(source: R, password: &str) -> Result<Document> {
        Self::load_from_with_options(source, LoadOptions::with_password(password))
    }

    fn load_internal<R: Read>(mut source: R, capacity: Option<usize>, options: LoadOptions) -> Result<Document> {
        let mut buffer = capacity.map(Vec::with_capacity).unwrap_or_default();
        source.read_to_end(&mut buffer)?;

        let filter = options.filter;
        Reader::with_options(&buffer, options).read(filter)
    }

    /// Load a PDF document from a memory slice.
    pub fn load_mem(buffer: &[u8]) -> Result<Document> {
        Self::load_mem_with_options(buffer, LoadOptions::default())
    }

    /// Load a PDF document from a memory slice with the given options.
    pub fn load_mem_with_options(buffer: &[u8], options: LoadOptions) -> Result<Document> {
        let filter = options.filter;
        Reader::with_options(buffer, options).read(filter)
    }

    /// Load a PDF document from a memory slice with a password for encrypted PDFs.
    #[deprecated(since = "0.41.0", note = "Use load_mem_with_options instead")]
    pub fn load_mem_with_password(buffer: &[u8], password: &str) -> Result<Document> {
        Self::load_mem_with_options(buffer, LoadOptions::with_password(password))
    }

    /// Load PDF metadata (title and page count) without loading the entire document.
    /// This is much faster for large PDFs when you only need basic information.
    #[inline]
    pub fn load_metadata<P: AsRef<Path>>(path: P) -> Result<PdfMetadata> {
        let (file, capacity) = open_file(path)?;
        Self::load_metadata_internal(file, Some(capacity), None)
    }

    /// Load PDF metadata from a file path with a password for encrypted PDFs.
    #[inline]
    pub fn load_metadata_with_password<P: AsRef<Path>>(path: P, password: &str) -> Result<PdfMetadata> {
        let (file, capacity) = open_file(path)?;
        Self::load_metadata_internal(file, Some(capacity), Some(password.to_string()))
    }

    /// Load PDF metadata from an arbitrary source without loading the entire document.
    #[inline]
    pub fn load_metadata_from<R: Read>(source: R) -> Result<PdfMetadata> {
        Self::load_metadata_internal(source, None, None)
    }

    /// Load PDF metadata from an arbitrary source with a password for encrypted PDFs.
    #[inline]
    pub fn load_metadata_from_with_password<R: Read>(source: R, password: &str) -> Result<PdfMetadata> {
        Self::load_metadata_internal(source, None, Some(password.to_string()))
    }

    /// Load PDF metadata from a memory slice without loading the entire document.
    #[inline]
    pub fn load_metadata_mem(buffer: &[u8]) -> Result<PdfMetadata> {
        Reader::for_metadata(buffer, None).read_metadata()
    }

    /// Load PDF metadata from a memory slice with a password for encrypted PDFs.
    #[inline]
    pub fn load_metadata_mem_with_password(buffer: &[u8], password: &str) -> Result<PdfMetadata> {
        Reader::for_metadata(buffer, Some(password.to_string())).read_metadata()
    }

    fn load_metadata_internal<R: Read>(
        mut source: R, capacity: Option<usize>, password: Option<String>,
    ) -> Result<PdfMetadata> {
        let mut buffer = capacity.map(Vec::with_capacity).unwrap_or_default();
        source.read_to_end(&mut buffer)?;

        Reader::for_metadata(&buffer, password).read_metadata()
    }
}

/// Open `path` and report its size, so the caller can pre-size the buffer it
/// reads the file into.
#[cfg(feature = "async")]
async fn open_file<P: AsRef<Path>>(path: P) -> Result<(File, usize)> {
    let file = File::open(path).await?;
    let capacity = file.metadata().await?.len() as usize;
    Ok((file, capacity))
}

#[cfg(feature = "async")]
impl Document {
    pub async fn load<P: AsRef<Path>>(path: P) -> Result<Document> {
        Self::load_with_options(path, LoadOptions::default()).await
    }

    /// Load a PDF document from a specified file path with the given options.
    pub async fn load_with_options<P: AsRef<Path>>(path: P, options: LoadOptions) -> Result<Document> {
        let (file, capacity) = open_file(path).await?;
        Self::load_internal(file, Some(capacity), options).await
    }

    /// Load a PDF document from a specified file path with a password for encrypted PDFs.
    pub async fn load_with_password<P: AsRef<Path>>(path: P, password: &str) -> Result<Document> {
        Self::load_with_options(path, LoadOptions::with_password(password)).await
    }

    #[deprecated(since = "0.41.0", note = "Use load_with_options instead")]
    pub async fn load_filtered<P: AsRef<Path>>(path: P, filter_func: FilterFunc) -> Result<Document> {
        Self::load_with_options(path, LoadOptions::with_filter(filter_func)).await
    }

    async fn load_internal<R: AsyncRead>(source: R, capacity: Option<usize>, options: LoadOptions) -> Result<Document> {
        pin!(source);

        let mut buffer = capacity.map(Vec::with_capacity).unwrap_or_default();
        source.read_to_end(&mut buffer).await?;

        let filter = options.filter;
        Reader::with_options(&buffer, options).read(filter)
    }

    /// Load a PDF document from a memory slice.
    pub fn load_mem(buffer: &[u8]) -> Result<Document> {
        Self::load_mem_with_options(buffer, LoadOptions::default())
    }

    /// Load a PDF document from a memory slice with the given options.
    pub fn load_mem_with_options(buffer: &[u8], options: LoadOptions) -> Result<Document> {
        let filter = options.filter;
        Reader::with_options(buffer, options).read(filter)
    }

    /// Load PDF metadata (title and page count) without loading the entire document.
    /// This is much faster for large PDFs when you only need basic information.
    #[inline]
    pub async fn load_metadata<P: AsRef<Path>>(path: P) -> Result<PdfMetadata> {
        let (file, capacity) = open_file(path).await?;
        Self::load_metadata_internal(file, Some(capacity), None).await
    }

    /// Load PDF metadata from a file path with a password for encrypted PDFs.
    #[inline]
    pub async fn load_metadata_with_password<P: AsRef<Path>>(path: P, password: &str) -> Result<PdfMetadata> {
        let (file, capacity) = open_file(path).await?;
        Self::load_metadata_internal(file, Some(capacity), Some(password.to_string())).await
    }

    /// Load PDF metadata from an arbitrary source without loading the entire document.
    #[inline]
    pub async fn load_metadata_from<R: AsyncRead>(source: R) -> Result<PdfMetadata> {
        Self::load_metadata_internal(source, None, None).await
    }

    /// Load PDF metadata from an arbitrary source with a password for encrypted PDFs.
    #[inline]
    pub async fn load_metadata_from_with_password<R: AsyncRead>(source: R, password: &str) -> Result<PdfMetadata> {
        Self::load_metadata_internal(source, None, Some(password.to_string())).await
    }

    /// Load PDF metadata from a memory slice without loading the entire document.
    #[inline]
    pub fn load_metadata_mem(buffer: &[u8]) -> Result<PdfMetadata> {
        Reader::for_metadata(buffer, None).read_metadata()
    }

    /// Load PDF metadata from a memory slice with a password for encrypted PDFs.
    #[inline]
    pub fn load_metadata_mem_with_password(buffer: &[u8], password: &str) -> Result<PdfMetadata> {
        Reader::for_metadata(buffer, Some(password.to_string())).read_metadata()
    }

    async fn load_metadata_internal<R: AsyncRead>(
        source: R, capacity: Option<usize>, password: Option<String>,
    ) -> Result<PdfMetadata> {
        pin!(source);

        let mut buffer = capacity.map(Vec::with_capacity).unwrap_or_default();
        source.read_to_end(&mut buffer).await?;

        Reader::for_metadata(&buffer, password).read_metadata()
    }
}

impl TryInto<Document> for &[u8] {
    type Error = Error;

    fn try_into(self) -> Result<Document> {
        Reader::new(self).read(None)
    }
}

#[cfg(not(feature = "async"))]
impl IncrementalDocument {
    /// Load a PDF document from a specified file path.
    #[inline]
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let (file, capacity) = open_file(path)?;
        Self::load_internal(file, Some(capacity))
    }

    /// Load a PDF document from an arbitrary source.
    #[inline]
    pub fn load_from<R: Read>(source: R) -> Result<Self> {
        Self::load_internal(source, None)
    }

    fn load_internal<R: Read>(mut source: R, capacity: Option<usize>) -> Result<Self> {
        let mut buffer = capacity.map(Vec::with_capacity).unwrap_or_default();
        source.read_to_end(&mut buffer)?;

        let document = Reader::new(&buffer).read(None)?;

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
        let (file, capacity) = open_file(path).await?;
        Self::load_internal(file, Some(capacity)).await
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

        let document = Reader::new(&buffer).read(None)?;

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
        let document = Reader::new(self).read(None)?;

        Ok(IncrementalDocument::create_from(self.to_vec(), document))
    }
}

pub struct Reader<'a> {
    pub buffer: &'a [u8],
    pub document: Document,
    pub encryption_state: Option<EncryptionState>,
    pub raw_objects: BTreeMap<ObjectId, Vec<u8>>, // Store raw bytes for encrypted objects
    pub password: Option<String>,                 // Password for encrypted PDFs
    pub strict: bool,                             // Reject non-conforming PDFs when true
    /// Maximum decompressed size, in bytes, allowed for any single stream
    /// (object streams and cross-reference streams) decoded during loading.
    /// `None` (the default) applies no limit; set it to guard against
    /// decompression bombs. See [`crate::LoadOptions`].
    pub max_decompressed_size: Option<usize>,
    /// Rebuild the cross-reference table by scanning before reading it as written. See
    /// [`LoadOptions::force_reconstruct`].
    pub force_reconstruct: bool,
    /// Sorted unique byte offsets of every `XrefEntry::Normal` entry in the
    /// final cross-reference table. Built once when the table is complete so
    /// object-boundary lookups can binary-search the successor offset instead
    /// of scanning all xref entries on each call.
    normal_offsets: Vec<usize>,
    /// Object streams decoded while resolving compressed objects, keyed by
    /// container number; `None` records one that failed to decode. Without it
    /// every indirect `/Length` stored in an object stream re-reads and
    /// re-decodes the whole container, which is quadratic in the number of
    /// such streams.
    object_streams: Mutex<HashMap<u32, Option<Arc<ObjectStream>>>>,
    /// Puts the objects on the calling thread (`Some(false)`) or rayon's pool
    /// (`Some(true)`) whatever the document's size.
    #[cfg(all(test, feature = "rayon"))]
    parallel: Option<bool>,
}

/// With the `rayon` feature, a document with fewer cross-reference entries
/// than this and less input than [`MIN_PARALLEL_BYTES`] is parsed on the
/// calling thread. Handing the parse to rayon's global pool has a fixed cost:
/// the caller wakes a parked worker and blocks until the work comes back, and
/// the first load in a process also spawns the pool. For a document this small
/// that costs more than parsing it in parallel saves.
#[cfg(feature = "rayon")]
const MIN_PARALLEL_ENTRIES: usize = 16;

/// Input from which a document is parsed on rayon's pool whatever its number
/// of entries: copying a few large streams in parallel pays for the handoff.
#[cfg(feature = "rayon")]
const MIN_PARALLEL_BYTES: usize = 1 << 20;

/// Maximum allowed embedding of literal strings.
pub const MAX_BRACKET: usize = 100;

pub const MAX_NESTING_DEPTH: usize = 100;

/// Cap on reconstructed cross-reference entries, bounding memory on hostile inputs.
const MAX_RECONSTRUCTED_OBJECTS: usize = 1_000_000;

/// Cap on how many trailing `trailer` dictionaries, and then cross-reference
/// streams and catalogs, are inspected for a `/Root`.
const MAX_TRAILER_CANDIDATES: usize = 16;

const UNREADABLE_PAGE_TREE: &str = "the catalog or the root of the page tree did not load; \
     rebuilt the cross-reference table by scanning for indirect objects";

/// PDF metadata extracted without loading the entire document.
/// This is useful for quickly getting basic information about large PDFs.
#[derive(Debug, Clone)]
pub struct PdfMetadata {
    /// Document title from Info dictionary
    pub title: Option<String>,
    /// Document author from Info dictionary
    pub author: Option<String>,
    /// Document subject from Info dictionary
    pub subject: Option<String>,
    /// Document keywords from Info dictionary
    pub keywords: Option<String>,
    /// Application that created the document
    pub creator: Option<String>,
    /// Application that produced the document
    pub producer: Option<String>,
    /// Document creation date (PDF date format: D:YYYYMMDDHHmmSSOHH'mm')
    pub creation_date: Option<String>,
    /// Document modification date (PDF date format: D:YYYYMMDDHHmmSSOHH'mm')
    pub modification_date: Option<String>,
    /// Whether the document has been modified to include trapping information: `True`,
    /// `False` or `Unknown`. The value is a name in the Info dictionary; a text string,
    /// which some producers write instead, is decoded as the other fields are.
    pub trapped: Option<String>,
    /// Custom Info dictionary entries that are not part of the standard set above.
    ///
    /// Producers commonly use the Info dictionary to attach application-specific
    /// metadata (for example, Microsoft Information Protection labels stored as
    /// `MSIP_Label_{GUID}_{Property}` keys). Such entries used to be discarded
    /// by the metadata-only loader; they are now preserved here as raw `Object`
    /// values so callers can inspect or decode them as needed.
    pub custom: HashMap<Vec<u8>, Object>,
    /// Number of pages in the document
    pub page_count: u32,
    /// PDF version
    pub version: String,
    /// Whether the document declares encryption (an `/Encrypt` entry in the
    /// trailer). When `true`, string and stream contents are encrypted; the
    /// standard Info fields above are populated only if the document could be
    /// decrypted — e.g. an empty user password, or a password supplied via a
    /// `*_with_password` loader.
    pub encrypted: bool,
    /// How the cross-reference data was repaired to read this metadata, or
    /// `None` when it was read as written. See [`Document::xref_recovery`].
    pub xref_recovery: Option<XrefRecovery>,
}

struct InfoMetadata {
    title: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    keywords: Option<String>,
    creator: Option<String>,
    producer: Option<String>,
    creation_date: Option<String>,
    modification_date: Option<String>,
    trapped: Option<String>,
    custom: HashMap<Vec<u8>, Object>,
}

impl InfoMetadata {
    fn empty() -> Self {
        Self {
            title: None,
            author: None,
            subject: None,
            keywords: None,
            creator: None,
            producer: None,
            creation_date: None,
            modification_date: None,
            trapped: None,
            custom: HashMap::new(),
        }
    }
}

/// Standard Info dictionary keys defined in PDF 1.7 §14.3.3 ("Document
/// Information Dictionary"). Any other key is preserved on
/// `PdfMetadata::custom` rather than dropped.
const STANDARD_INFO_KEYS: &[&[u8]] = &[
    b"Title",
    b"Author",
    b"Subject",
    b"Keywords",
    b"Creator",
    b"Producer",
    b"CreationDate",
    b"ModDate",
    b"Trapped",
];

impl<'a> Reader<'a> {
    /// A reader with the default load options: no password, lenient, unlimited.
    ///
    /// This deliberately defers to `LoadOptions::default()`, so a field added
    /// there later is picked up here rather than silently reset to a stale
    /// value the way an explicit struct literal would.
    fn new(buffer: &'a [u8]) -> Self {
        Self::with_options(buffer, LoadOptions::default())
    }

    /// The metadata-only loaders accept a password but never the strictness,
    /// filter or decompression-limit knobs, so they use their own defaults.
    fn for_metadata(buffer: &'a [u8], password: Option<String>) -> Self {
        Self {
            password,
            ..Self::new(buffer)
        }
    }

    fn with_options(buffer: &'a [u8], options: LoadOptions) -> Self {
        Self {
            buffer,
            document: Document::new(),
            encryption_state: None,
            raw_objects: BTreeMap::new(),
            password: options.password,
            strict: options.strict,
            max_decompressed_size: options.max_decompressed_size,
            force_reconstruct: options.force_reconstruct,
            normal_offsets: Vec::new(),
            object_streams: Mutex::default(),
            #[cfg(all(test, feature = "rayon"))]
            parallel: None,
        }
    }

    /// Read metadata (title and page count) without loading the entire document.
    /// This is much faster for large PDFs when you only need basic information.
    ///
    /// For encrypted PDFs, use `Document::load_metadata_with_password()` instead.
    pub fn read_metadata(mut self) -> Result<PdfMetadata> {
        let metadata = self.read_metadata_once()?;
        if self.should_rebuild(metadata.xref_recovery) && !self.page_tree_readable() {
            let mut rebuilt = Reader::for_metadata(self.buffer, self.password.clone());
            rebuilt.force_reconstruct = true;
            if let Ok(rebuilt_metadata) = rebuilt.read_metadata_once()
                && rebuilt_metadata.xref_recovery == Some(XrefRecovery::Reconstructed)
                && rebuilt.page_tree_readable()
            {
                warn!("{UNREADABLE_PAGE_TREE}");
                return Ok(rebuilt_metadata);
            }
        }
        Ok(metadata)
    }

    fn read_metadata_once(&mut self) -> Result<PdfMetadata> {
        let offset = self.buffer.windows(5).position(|w| w == b"%PDF-").unwrap_or(0);
        self.buffer = &self.buffer[offset..];

        let version = parser::header(self.buffer, self.strict).ok_or(ParseError::InvalidFileHeader)?;

        let (xref, trailer) = self.load_xref_and_trailer()?;

        self.set_reference_table(xref);
        self.document.trailer = trailer.clone();
        let encrypted = self.document.trailer.get(b"Encrypt").is_ok();
        // For encrypted PDFs, set up decryption so the Info dictionary can be read. Without a
        // usable password, report what we have rather than failing the whole call; callers
        // needing the Info fields should use a `*_with_password` loader.
        let needs_password = if encrypted {
            match self.setup_encryption_for_metadata() {
                Ok(()) => false,
                // No usable password: report `encrypted` without the Info fields.
                Err(Error::Unimplemented(_)) => true,
                Err(e) => return Err(e),
            }
        } else {
            false
        };

        let info_metadata = if needs_password {
            InfoMetadata::empty()
        } else {
            self.extract_info_metadata()?
        };
        let page_count = if needs_password { 0 } else { self.extract_page_count()? };

        Ok(PdfMetadata {
            title: info_metadata.title,
            author: info_metadata.author,
            subject: info_metadata.subject,
            keywords: info_metadata.keywords,
            creator: info_metadata.creator,
            producer: info_metadata.producer,
            creation_date: info_metadata.creation_date,
            modification_date: info_metadata.modification_date,
            trapped: info_metadata.trapped,
            custom: info_metadata.custom,
            page_count,
            version,
            encrypted,
            xref_recovery: self.document.xref_recovery,
        })
    }

    fn extract_info_metadata(&self) -> Result<InfoMetadata> {
        let info_ref = match self.document.trailer.get(b"Info") {
            Ok(obj) => obj.as_reference().ok(),
            Err(_) => return Ok(InfoMetadata::empty()),
        };

        let info_id = match info_ref {
            Some(id) => id,
            None => return Ok(InfoMetadata::empty()),
        };

        let mut already_seen = HashSet::new();
        let info_obj = match self.get_object(info_id, &mut already_seen) {
            Ok(obj) => obj,
            Err(_) => return Ok(InfoMetadata::empty()),
        };

        let info_dict = match info_obj.as_dict() {
            Ok(dict) => dict,
            Err(_) => return Ok(InfoMetadata::empty()),
        };

        let mut custom = HashMap::new();
        for (key, value) in info_dict.iter() {
            if STANDARD_INFO_KEYS.contains(&key.as_slice()) {
                continue;
            }
            custom.insert(key.clone(), value.clone());
        }

        Ok(InfoMetadata {
            title: Self::extract_string_field(info_dict, b"Title"),
            author: Self::extract_string_field(info_dict, b"Author"),
            subject: Self::extract_string_field(info_dict, b"Subject"),
            keywords: Self::extract_string_field(info_dict, b"Keywords"),
            creator: Self::extract_string_field(info_dict, b"Creator"),
            producer: Self::extract_string_field(info_dict, b"Producer"),
            creation_date: Self::extract_string_field(info_dict, b"CreationDate"),
            modification_date: Self::extract_string_field(info_dict, b"ModDate"),
            trapped: Self::extract_trapped_field(info_dict),
            custom,
        })
    }

    fn extract_string_field(dict: &Dictionary, key: &[u8]) -> Option<String> {
        match dict.get(key) {
            Ok(obj) => match obj {
                Object::String(_bytes, _) => common_data_structures::decode_text_string(obj).ok(),
                _ => None,
            },
            Err(_) => None,
        }
    }

    /// `/Trapped` is a name, unlike the other standard fields; accept a text string too.
    fn extract_trapped_field(dict: &Dictionary) -> Option<String> {
        match dict.get(b"Trapped") {
            Ok(Object::Name(name)) => String::from_utf8(name.clone()).ok(),
            _ => Self::extract_string_field(dict, b"Trapped"),
        }
    }

    fn extract_page_count(&self) -> Result<u32> {
        let root_ref = match self.document.trailer.get(b"Root").and_then(Object::as_reference) {
            Ok(id) => id,
            Err(_) => return Ok(0),
        };

        let mut already_seen = HashSet::new();
        let catalog_obj = match self.get_object(root_ref, &mut already_seen) {
            Ok(obj) => obj,
            Err(_) => return Ok(0),
        };

        let catalog_dict = match catalog_obj.as_dict() {
            Ok(dict) => dict,
            Err(_) => return Ok(0),
        };

        let pages_ref = match catalog_dict.get(b"Pages").and_then(Object::as_reference) {
            Ok(id) => id,
            Err(_) => return Ok(0),
        };

        self.get_pages_tree_count(pages_ref, &mut HashSet::new(), 0).or(Ok(0))
    }

    fn get_pages_tree_count(&self, pages_id: ObjectId, seen: &mut HashSet<ObjectId>, depth: usize) -> Result<u32> {
        // `seen` catches cycles but not a long non-cyclic /Pages chain with no /Count,
        // which recurses once per level.
        if depth >= MAX_NESTING_DEPTH {
            return Ok(0);
        }
        if seen.contains(&pages_id) {
            return Err(Error::ReferenceCycle(pages_id));
        }
        seen.insert(pages_id);

        let mut already_seen = HashSet::new();
        let pages_obj = match self.get_object(pages_id, &mut already_seen) {
            Ok(obj) => obj,
            Err(_) => return Ok(0),
        };

        let pages_dict = match pages_obj.as_dict() {
            Ok(dict) => dict,
            Err(_) => return Ok(0),
        };

        match pages_dict.get_type() {
            Ok(type_name) if type_name == b"Page" => Ok(1),
            Ok(type_name) if type_name == b"Pages" => {
                if let Ok(count_obj) = pages_dict.get(b"Count")
                    && let Ok(count) = count_obj.as_i64()
                    && count >= 0
                {
                    return Ok(count as u32);
                }

                let kids = match pages_dict.get(b"Kids").and_then(Object::as_array) {
                    Ok(arr) => arr,
                    Err(_) => return Ok(0),
                };

                let mut total = 0u32;
                for kid in kids.iter() {
                    if let Ok(kid_ref) = kid.as_reference()
                        && let Ok(count) = self.get_pages_tree_count(kid_ref, seen, depth + 1)
                    {
                        total += count;
                    }
                }
                Ok(total)
            }
            _ => Ok(1),
        }
    }

    /// Read whole document.
    pub fn read(mut self, filter_func: Option<FilterFunc>) -> Result<Document> {
        let offset = self.buffer.windows(5).position(|w| w == b"%PDF-").unwrap_or(0);
        self.buffer = &self.buffer[offset..];

        // The document structure can be expressed in PEG as:
        //   document <- header indirect_object* xref trailer xref_start
        let version = parser::header(self.buffer, self.strict).ok_or(ParseError::InvalidFileHeader)?;

        //The binary_mark is in line 2 after the pdf version. If at other line number, then will be declared as invalid
        // pdf.
        if let Some(pos) = self.buffer.iter().position(|&byte| byte == b'\n')
            && let Some(binary_mark) = parser::binary_mark(&self.buffer[pos + 1..])
            && binary_mark.iter().all(|&byte| byte >= 128)
        {
            self.document.binary_mark = binary_mark;
        }

        let (xref, trailer) = self.load_xref_and_trailer()?;

        self.document.version = version;
        self.document.max_id = xref.size - 1;
        self.document.trailer = trailer;
        self.set_reference_table(xref);

        // Check if encrypted
        let is_encrypted = self.document.trailer.get(b"Encrypt").is_ok();

        if is_encrypted {
            // For encrypted PDFs, use a special loading strategy
            self.load_encrypted_document(filter_func)?;
        } else {
            // For non-encrypted PDFs, use the normal loading
            self.load_objects_raw(filter_func)?;
        }

        // Object-stream members join `objects` after `max_id` was derived from the xref size,
        // so raise the ceiling to the highest loaded object.
        if let Some(&max_loaded_id) = self.document.objects.keys().next_back() {
            self.document.max_id = self.document.max_id.max(max_loaded_id.0);
        }
        self.document.linearization = self.read_linearization();

        // A filter may drop the page tree on purpose, so only rebuild for an unfiltered load.
        if filter_func.is_none()
            && self.should_rebuild(self.document.xref_recovery)
            && !Self::page_tree_loaded(&self.document)
        {
            let options = LoadOptions {
                password: self.password.clone(),
                max_decompressed_size: self.max_decompressed_size,
                force_reconstruct: true,
                ..LoadOptions::default()
            };
            if let Ok(rebuilt) = Reader::with_options(self.buffer, options).read(None)
                && rebuilt.xref_recovery == Some(XrefRecovery::Reconstructed)
                && Self::page_tree_loaded(&rebuilt)
            {
                warn!("{UNREADABLE_PAGE_TREE}");
                return Ok(rebuilt);
            }
        }

        Ok(self.document)
    }

    /// Resolve the cross-reference data and trailer, or rebuild them by scanning for indirect
    /// objects once all standard resolution fails. With `force_reconstruct`, rebuild first, and
    /// resolve only if the scan finds no usable trailer.
    fn load_xref_and_trailer(&mut self) -> Result<(Xref, Dictionary)> {
        if self.force_reconstruct
            && let Some(reconstructed) = self.reconstruct_xref_and_trailer()
        {
            warn!("rebuilt the cross-reference table by scanning for indirect objects, as requested");
            return Ok(reconstructed);
        }
        match self.resolve_xref_and_trailer() {
            Ok(resolved) => Ok(resolved),
            // The scan already found nothing usable.
            Err(err) if self.force_reconstruct => Err(err),
            Err(err) => match self.reconstruct_xref_and_trailer() {
                Some(reconstructed) => {
                    warn!("cross-reference resolution failed ({err}); recovered by scanning for indirect objects");
                    Ok(reconstructed)
                }
                None => Err(err),
            },
        }
    }

    /// Whether a load whose cross-reference data was read as described by `recovery` may be
    /// repeated with the table rebuilt. Resolution trusts the cross-reference data it reads, and
    /// in a file that lost or gained bytes that data can point past the objects it names, so the
    /// catalog or the root of the page tree does not load although the objects are all there.
    ///
    /// An encrypted document that waits for its password has no objects to load yet, which is
    /// not damage.
    fn should_rebuild(&self, recovery: Option<XrefRecovery>) -> bool {
        let needs_password = self.document.trailer.has(b"Encrypt") && self.encryption_state.is_none();
        !self.strict && !self.force_reconstruct && !needs_password && recovery != Some(XrefRecovery::Reconstructed)
    }

    /// Whether the loaded catalog leads to the root of a page tree, a dictionary with `/Kids`,
    /// which is what [`Document::page_iter`] walks.
    fn page_tree_loaded(document: &Document) -> bool {
        document
            .catalog()
            .and_then(|catalog| catalog.get(b"Pages"))
            .and_then(Object::as_reference)
            .and_then(|root| document.get_dictionary(root))
            .and_then(|root| root.get_deref(b"Kids", document))
            .and_then(Object::as_array)
            .is_ok()
    }

    /// [`Self::page_tree_loaded`] for the metadata loaders, which read objects on demand.
    fn page_tree_readable(&self) -> bool {
        let read = |object: &Object| match object {
            Object::Reference(id) => self.get_object(*id, &mut HashSet::new()).ok(),
            object => Some(object.clone()),
        };
        let dictionary = |object: Option<Object>| match object {
            Some(Object::Dictionary(dictionary)) => Some(dictionary),
            _ => None,
        };
        let catalog = self.document.trailer.get(b"Root").ok().and_then(read);
        let root = dictionary(catalog).and_then(|catalog| catalog.get(b"Pages").ok().and_then(read));
        let kids = dictionary(root).and_then(|root| root.get(b"Kids").ok().and_then(read));
        matches!(kids, Some(Object::Array(_)))
    }

    fn load_encrypted_document(&mut self, _filter_func: Option<FilterFunc>) -> Result<()> {
        // First, extract all raw object bytes without parsing
        let entries: Vec<_> = self
            .document
            .reference_table
            .entries
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();

        let mut object_streams = Vec::new();

        for (obj_num, entry) in entries {
            match entry {
                XrefEntry::Normal { offset, .. } => {
                    if let Ok((obj_id, raw_bytes)) = self.extract_raw_object(offset as usize) {
                        self.raw_objects.insert(obj_id, raw_bytes);
                    }
                }
                XrefEntry::Compressed { container, index } => {
                    // Store compressed object info for later processing
                    object_streams.push((obj_num, container, index));
                }
                XrefEntry::Free | XrefEntry::UnusableFree => {
                    // Skip free entries
                }
            }
        }

        self.parse_encryption_dictionary()?;

        if self.authenticate_and_setup_encryption(false)?.is_none() {
            return Ok(());
        }

        if let Some(ref state) = self.encryption_state {
            let encrypt_ref = self
                .document
                .trailer
                .get(b"Encrypt")
                .ok()
                .and_then(|o| o.as_reference().ok());

            for (obj_id, raw_bytes) in &self.raw_objects {
                if let Some(enc_ref) = encrypt_ref
                    && *obj_id == enc_ref
                {
                    continue;
                }

                if let Ok((id, mut obj)) = self.parse_raw_object(raw_bytes) {
                    // Keep the partial object, but do not drop the error silently.
                    if let Err(error) = encryption::decrypt_object(state, *obj_id, &mut obj) {
                        log::warn!("object {obj_id:?} could not be fully decrypted: {error}");
                    }

                    self.document.objects.insert(id, obj);
                }
            }

            let mut streams_to_process: std::collections::HashMap<u32, Vec<(u32, u16)>> =
                std::collections::HashMap::new();
            for (obj_num, container_id, index) in object_streams {
                streams_to_process
                    .entry(container_id)
                    .or_default()
                    .push((obj_num, index));
            }

            for (container_id, objects_in_stream) in streams_to_process {
                if let Some(container_obj) = self.document.objects.get(&(container_id, 0))
                    && let Ok(stream) = container_obj.as_stream()
                {
                    match ObjectStream::new_with_limit(stream, self.max_decompressed_size) {
                        Ok(object_stream) => {
                            for (obj_num, _index) in objects_in_stream {
                                let obj_id = (obj_num, 0);
                                if let Some(obj) = object_stream.objects.get(&obj_id) {
                                    self.document.objects.insert(obj_id, obj.clone());
                                }
                            }
                        }
                        Err(error @ Error::Decompress(DecompressError::MemoryLimitExceeded { .. })) => {
                            return Err(error);
                        }
                        Err(error) => {
                            if self.strict {
                                return Err(error);
                            }
                            warn!("Object stream load error for {container_id} 0 R: {error:?}");
                        }
                    }
                }
            }

            let mut stored_state = state.clone();
            // Record the /Encrypt id so a later incremental save can restore it in the trailer.
            stored_state.encrypt_object_id = encrypt_ref;
            self.document.encryption_state = Some(stored_state);

            if let Some(enc_ref) = encrypt_ref {
                self.document.objects.remove(&enc_ref);
            }
            self.document.trailer.remove(b"Encrypt");
        }

        Ok(())
    }

    fn parse_raw_object(&self, raw_bytes: &[u8]) -> Result<(ObjectId, Object)> {
        // Parse the raw bytes as an indirect object
        parser::indirect_object(raw_bytes, 0, None, self, &mut HashSet::new(), None)
    }

    /// Install a fully merged cross-reference table and derive its sorted
    /// offset index. All later object-boundary lookups go through that index,
    /// so it must be rebuilt whenever the table is replaced.
    fn set_reference_table(&mut self, xref: Xref) {
        let mut normal_offsets: Vec<usize> = xref
            .entries
            .values()
            .filter_map(|entry| match entry {
                XrefEntry::Normal { offset, .. } => Some(*offset as usize),
                _ => None,
            })
            .collect();
        normal_offsets.sort_unstable();
        normal_offsets.dedup();
        self.normal_offsets = normal_offsets;
        self.document.reference_table = xref;
    }

    fn load_objects_raw(&mut self, filter_func: Option<FilterFunc>) -> Result<()> {
        let is_encrypted = self.document.trailer.get(b"Encrypt").is_ok();
        let zero_length_streams = Mutex::new(vec![]);
        let object_streams = Mutex::new(vec![]);

        // Map each compressed object to its xref container, so stale ObjStm copies (e.g. from
        // linearization first-page sections) cannot override the correct version.
        let compressed_obj_containers: BTreeMap<u32, u32> = self
            .document
            .reference_table
            .entries
            .iter()
            .filter_map(|(&id, entry)| {
                if let XrefEntry::Compressed { container, .. } = entry {
                    Some((id, *container))
                } else {
                    None
                }
            })
            .collect();
        let normal_offsets = &self.normal_offsets;

        let entries_filter_map = |(_, entry): (&_, &_)| -> Result<Option<(ObjectId, Object)>> {
            if let XrefEntry::Normal { offset, .. } = *entry {
                // read_object now handles decryption internally
                let offset = offset as usize;
                let next_index = normal_offsets.partition_point(|&next| next <= offset);
                let next_object = normal_offsets.get(next_index).copied();
                let end = self.object_end(offset, next_object);
                let result = self.read_object_to(offset, end, None, &mut HashSet::new());
                let (object_id, mut object) = match result {
                    Ok(obj) => obj,
                    Err(e) => {
                        if is_encrypted {
                            // Non-fatal even in strict mode: an encrypted object does not
                            // indicate a malformed file.
                            warn!("Skipping encrypted object at offset {}: {:?}", offset, e);
                            return Ok(None);
                        }
                        // Lenient loading logs and skips malformed objects; strict loading fails.
                        error!("Object load error at offset {}: {e:?}", offset);
                        return if self.strict { Err(e) } else { Ok(None) };
                    }
                };
                if let Some(filter_func) = filter_func
                    && filter_func(object_id, &mut object).is_none()
                {
                    return Ok(None);
                }

                if let Ok(stream) = object.as_stream() {
                    if stream.dict.has_type(b"ObjStm") && !is_encrypted {
                        let obj_stream = match ObjectStream::new_with_limit(stream, self.max_decompressed_size) {
                            Ok(obj_stream) => obj_stream,
                            // Resource limits apply even when malformed objects may be skipped.
                            Err(e @ Error::Decompress(DecompressError::MemoryLimitExceeded { .. })) => return Err(e),
                            // Not an encryption-related loss, so it obeys the same
                            // strict-mode policy as any other load error.
                            Err(e) => {
                                error!("Object stream load error for {object_id:?}: {e:?}");
                                return if self.strict { Err(e) } else { Ok(None) };
                            }
                        };
                        let container_id = object_id.0;
                        let mut object_streams = object_streams.lock().unwrap();
                        if let Some(filter_func) = filter_func {
                            let objects: BTreeMap<(u32, u16), Object> = obj_stream
                                .objects
                                .into_iter()
                                .filter(|((obj_num, _), _)| {
                                    compressed_obj_containers
                                        .get(obj_num)
                                        .is_none_or(|&c| c == container_id)
                                })
                                .filter_map(|(object_id, mut object)| filter_func(object_id, &mut object))
                                .collect();
                            object_streams.extend(objects);
                        } else {
                            object_streams.extend(obj_stream.objects.into_iter().filter(|((obj_num, _), _)| {
                                compressed_obj_containers
                                    .get(obj_num)
                                    .is_none_or(|&c| c == container_id)
                            }));
                        }
                    } else if stream.content.is_empty() {
                        let mut zero_length_streams = zero_length_streams.lock().unwrap();
                        zero_length_streams.push(object_id);
                    }
                }

                Ok(Some((object_id, object)))
            } else {
                Ok(None)
            }
        };

        let entries = &self.document.reference_table.entries;
        #[cfg(feature = "rayon")]
        let objects: Result<BTreeMap<_, _>> = if self.load_in_parallel() {
            entries
                .par_iter()
                .map(entries_filter_map)
                .filter_map(Result::transpose)
                .collect()
        } else {
            entries
                .iter()
                .map(entries_filter_map)
                .filter_map(Result::transpose)
                .collect()
        };
        #[cfg(not(feature = "rayon"))]
        let objects: Result<BTreeMap<_, _>> = entries
            .iter()
            .map(entries_filter_map)
            .filter_map(Result::transpose)
            .collect();
        self.document.objects = objects?;

        // Only add entries, but never replace entries
        for (id, entry) in object_streams.into_inner().unwrap() {
            self.document.objects.entry(id).or_insert(entry);
        }

        for object_id in zero_length_streams.into_inner().unwrap() {
            let _ = self.read_stream_content(object_id);
        }

        Ok(())
    }

    /// Whether [`Self::load_objects_raw`] parses on rayon's pool; see
    /// [`MIN_PARALLEL_ENTRIES`].
    #[cfg(feature = "rayon")]
    fn load_in_parallel(&self) -> bool {
        #[cfg(test)]
        if let Some(parallel) = self.parallel {
            return parallel;
        }
        self.document.reference_table.entries.len() >= MIN_PARALLEL_ENTRIES || self.buffer.len() >= MIN_PARALLEL_BYTES
    }

    fn read_stream_content(&mut self, object_id: ObjectId) -> Result<()> {
        let length = self.get_stream_length(object_id)?;
        let stream = self
            .document
            .get_object_mut(object_id)
            .and_then(Object::as_stream_mut)?;
        let start = stream
            .start_position
            .ok_or(Error::InvalidStream("missing start position".to_string()))?;

        if length < 0 {
            return Err(Error::InvalidStream("negative stream length.".to_string()));
        }

        let length = usize::try_from(length).map_err(|e| Error::NumericCast(e.to_string()))?;
        let end = start + length;

        if end > self.buffer.len() {
            return Err(Error::InvalidStream("stream extends after document end.".to_string()));
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
            .and_then(|value| self.document.dereference(value))
            .and_then(|(_id, obj)| match obj.as_i64() {
                Ok(length) => Ok(length),
                // s7.3.8.2 requires an integer /Length, but some producers write a real ("42.");
                // accept one whose value is integral and in range.
                Err(err) => match obj.as_f32() {
                    Ok(value) if value.fract() == 0.0 && value >= -(2f32.powi(63)) && value < 2f32.powi(63) => {
                        Ok(value as i64)
                    }
                    _ => Err(err),
                },
            })
            .inspect_err(|_err| {
                error!(
                    "stream dictionary of '{} {} R' is missing the Length entry",
                    object_id.0, object_id.1
                );
            })
    }

    /// Get object offset by object ID.
    fn get_offset(&self, id: ObjectId) -> Result<u32> {
        let entry = self.document.reference_table.get(id.0).ok_or(Error::MissingXrefEntry)?;
        match *entry {
            XrefEntry::Normal { offset, generation } if generation == id.1 => Ok(offset),
            _ => Err(Error::MissingXrefEntry),
        }
    }

    /// Load a compressed object from an object stream (for lightweight metadata extraction)
    ///
    /// `already_seen` is the resolution path of the caller. Reading the
    /// container may resolve its own indirect `/Length`, and if that length
    /// lives in the same container, a fresh set would recurse until the stack
    /// overflows.
    fn get_compressed_object(&self, id: ObjectId, already_seen: &mut HashSet<ObjectId>) -> Result<Object> {
        let entry = self.document.reference_table.get(id.0).ok_or(Error::MissingXrefEntry)?;

        let container_id = match entry {
            XrefEntry::Compressed { container, .. } => *container,
            _ => return Err(Error::MissingXrefEntry),
        };

        let object_stream = self.object_stream(container_id, already_seen)?;
        object_stream.objects.get(&id).cloned().ok_or(Error::MissingXrefEntry)
    }

    /// Decode object stream `container` once per load and reuse it afterwards.
    ///
    /// The result depends only on the buffer, the cross-reference table and
    /// the decompression limit, all fixed while the reader lives, so caching it
    /// changes nothing but the cost. The lock is not held while decoding: two
    /// threads may decode the same container, and both get the same result.
    ///
    /// A reference cycle is the one failure that is not cached: it depends on
    /// the caller's resolution path in `already_seen`, not on the container,
    /// and a caller coming by another path may read the same container fine.
    fn object_stream(&self, container: u32, already_seen: &mut HashSet<ObjectId>) -> Result<Arc<ObjectStream>> {
        if let Some(cached) = self.object_streams.lock().unwrap().get(&container) {
            return cached
                .clone()
                .ok_or_else(|| Error::InvalidObjectStream(format!("object stream {container} 0 R failed to decode")));
        }
        let decoded = self
            .get_object((container, 0), already_seen)
            .and_then(|object| ObjectStream::new_with_limit(object.as_stream()?, self.max_decompressed_size))
            .map(Arc::new);
        if !matches!(decoded, Err(Error::ReferenceCycle(_))) {
            self.object_streams
                .lock()
                .unwrap()
                .insert(container, decoded.as_ref().ok().cloned());
        }
        decoded
    }

    pub fn get_object(&self, id: ObjectId, already_seen: &mut HashSet<ObjectId>) -> Result<Object> {
        if already_seen.contains(&id) {
            warn!("reference cycle detected resolving object {} {}", id.0, id.1);
            return Err(Error::ReferenceCycle(id));
        }
        already_seen.insert(id);

        if let Some(entry) = self.document.reference_table.get(id.0)
            && matches!(entry, XrefEntry::Compressed { .. })
        {
            return self.get_compressed_object(id, already_seen);
        }

        let offset = self.get_offset(id)?;
        let (_, mut obj) = self.read_object(offset as usize, Some(id), already_seen)?;

        if let Some(ref state) = self.encryption_state {
            let encrypt_ref = self
                .document
                .trailer
                .get(b"Encrypt")
                .ok()
                .and_then(|o| o.as_reference().ok());
            if let Some(enc_ref) = encrypt_ref
                && id != enc_ref
            {
                encryption::decrypt_object(state, id, &mut obj).map_err(Error::Decryption)?;
            }
        }

        Ok(obj)
    }

    fn parse_encryption_dictionary(&mut self) -> Result<()> {
        if let Ok(encrypt_ref) = self.document.trailer.get(b"Encrypt").and_then(|o| o.as_reference()) {
            if self.raw_objects.is_empty() {
                let offset = self.get_offset(encrypt_ref)?;
                let (_, encrypt_obj) = self.read_object(offset as usize, Some(encrypt_ref), &mut HashSet::new())?;
                self.document.objects.insert(encrypt_ref, encrypt_obj);
            } else if let Some(raw_bytes) = self.raw_objects.get(&encrypt_ref)
                && let Ok((_, obj)) = self.parse_raw_object(raw_bytes)
            {
                self.document.objects.insert(encrypt_ref, obj);
            }
        }
        Ok(())
    }

    fn authenticate_and_setup_encryption(&mut self, require_password: bool) -> Result<Option<String>> {
        let password_to_use: Option<String> = if self.document.authenticate_password("").is_ok() {
            Some(String::new())
        } else if let Some(ref pwd) = self.password {
            if self.document.authenticate_password(pwd).is_ok() {
                Some(pwd.clone())
            } else if require_password {
                return Err(Error::InvalidPassword);
            } else {
                warn!("Invalid password provided for encrypted PDF");
                return Err(Error::InvalidPassword);
            }
        } else if require_password {
            return Err(Error::Unimplemented(
                "PDF is encrypted and requires a password. Use Document::load_metadata_with_password() instead.",
            ));
        } else {
            warn!("PDF is encrypted and requires a password");
            return Ok(None);
        };

        if let Some(ref password) = password_to_use {
            // Derive the key from the same sanitized bytes `authenticate_password` checked.
            let password = PasswordAlgorithm::try_from(&self.document)?.sanitize_password(password)?;
            let state = EncryptionState::decode(&self.document, &password)?;
            self.encryption_state = Some(state);
            // Containers decoded before decryption was set up hold ciphertext.
            if let Ok(cache) = self.object_streams.get_mut() {
                cache.clear();
            }
        }

        Ok(password_to_use)
    }

    fn setup_encryption_for_metadata(&mut self) -> Result<()> {
        self.parse_encryption_dictionary()?;
        self.authenticate_and_setup_encryption(true)?;
        Ok(())
    }

    fn extract_raw_object(&mut self, offset: usize) -> Result<(ObjectId, Vec<u8>)> {
        if offset > self.buffer.len() {
            return Err(Error::InvalidOffset(offset));
        }

        // Find object header (e.g., "19 0 obj")
        let slice = &self.buffer[offset..];

        let pos = skip_whitespace(slice, 0);

        // Get object number
        let (obj_num, pos) = scan_number::<u32>(slice, pos)?;
        let pos = skip_whitespace(slice, pos);

        // Get generation number
        let (obj_gen, pos) = scan_number::<u16>(slice, pos)?;

        // Skip to "obj"
        let pos = skip_whitespace(slice, pos);
        if pos + 3 > slice.len() || &slice[pos..pos + 3] != b"obj" {
            return Err(Error::Parse(ParseError::InvalidXref));
        }
        let body_start = pos + 3;

        // Find "endobj"
        let end_pos = find_from(slice, body_start, b"endobj").map_or(0, |end| end + b"endobj".len());

        // A truncated file may lack the keyword. Returning the rest of the buffer
        // would claim the object runs to the end of the file, so reject it and let
        // the caller decide how much of the damage to tolerate.
        if end_pos == 0 {
            return Err(Error::Parse(ParseError::InvalidXref));
        }

        // Extract raw object bytes (including header and trailer)
        let raw_bytes = slice[0..end_pos].to_vec();

        Ok(((obj_num, obj_gen), raw_bytes))
    }

    fn read_object(
        &self, offset: usize, expected_id: Option<ObjectId>, already_seen: &mut HashSet<ObjectId>,
    ) -> Result<(ObjectId, Object)> {
        let next_index = self.normal_offsets.partition_point(|&next| next <= offset);
        let end = self.object_end(offset, self.normal_offsets.get(next_index).copied());
        self.read_object_to(offset, end, expected_id, already_seen)
    }

    fn object_end(&self, offset: usize, next_object: Option<usize>) -> usize {
        let xref_start = (self.document.xref_start > offset).then_some(self.document.xref_start);
        next_object
            .into_iter()
            .chain(xref_start)
            .min()
            .unwrap_or(self.buffer.len())
            .min(self.buffer.len())
    }

    fn read_object_to(
        &self, offset: usize, end: usize, expected_id: Option<ObjectId>, already_seen: &mut HashSet<ObjectId>,
    ) -> Result<(ObjectId, Object)> {
        if offset > end || end > self.buffer.len() {
            return Err(Error::InvalidOffset(offset));
        }

        // Parse against the full buffer so a wrong xref offset cannot truncate a well-formed
        // object; `end` only bounds stream length recovery.
        parser::indirect_object(self.buffer, offset, expected_id, self, already_seen, Some(end))
    }

    /// Parse the cross-reference section recorded at `offset`, and return it with
    /// the offset it was read from. In lenient mode a slightly miswritten offset
    /// is corrected first, but only when the section found there locates its own
    /// objects: after bytes were inserted or removed before it, a nearby section
    /// points at shifted objects, and failing lets reconstruction find them.
    fn xref_and_trailer_at(&mut self, offset: usize) -> Result<(usize, Xref, Dictionary)> {
        let corrected = self.correct_xref_offset(offset);
        if corrected != offset
            && let Ok((xref, trailer)) = parser::xref_and_trailer(&self.buffer[corrected..], self)
            && self.locates_objects(&xref)
        {
            warn!(
                "Cross-reference offset {} does not point at an xref section; using nearby offset {} instead.",
                offset, corrected
            );
            self.document.xref_recovery = Some(XrefRecovery::OffsetCorrected);
            return Ok((corrected, xref, trailer));
        }
        let (xref, trailer) = parser::xref_and_trailer(&self.buffer[offset..], self)?;
        Ok((offset, xref, trailer))
    }

    /// Whether most normal entries of a cross-reference section point at the
    /// header of their own object, as they do in a section describing this file.
    fn locates_objects(&self, xref: &Xref) -> bool {
        let (mut normal, mut located) = (0usize, 0usize);
        for (&number, entry) in &xref.entries {
            if let XrefEntry::Normal { offset, generation } = *entry {
                normal += 1;
                let rest = self.buffer.get(offset as usize..).unwrap_or_default();
                if Self::parse_object_header(&rest[skip_whitespace(rest, 0)..]) == Some((number, generation)) {
                    located += 1;
                }
            }
        }
        normal == 0 || located * 2 > normal
    }

    /// Resolve the cross-reference table/stream and trailer, including the
    /// `/Prev` chain, and record the resolved start offset on the document.
    fn resolve_xref_and_trailer(&mut self) -> Result<(Xref, Dictionary)> {
        let xref_start = Self::get_xref_start(self.buffer)?;
        if xref_start > self.buffer.len() {
            return Err(Error::Xref(XrefError::Start));
        }
        let (xref_start, mut xref, mut trailer) = self.xref_and_trailer_at(xref_start)?;
        self.document.xref_start = xref_start;
        self.merge_hybrid_xref_stream(&mut xref, trailer.remove(b"XRefStm"))?;
        let mut sections = 1;

        // Read previous Xrefs of linearized or incremental updated document. A `/Prev` that leads
        // back to the section `startxref` names ends the chain like any other cycle.
        let mut already_seen = HashSet::new();
        already_seen.insert(xref_start as i64);
        let mut prev_xref_start = trailer.remove(b"Prev");
        while let Some(prev) = prev_xref_start.and_then(|offset| offset.as_i64().ok()) {
            if already_seen.contains(&prev) {
                break;
            }
            already_seen.insert(prev);
            if prev < 0 || prev as usize > self.buffer.len() {
                return Err(Error::Xref(XrefError::PrevStart));
            }

            let (_, mut prev_xref, mut prev_trailer) = self.xref_and_trailer_at(prev as usize)?;
            sections += 1;
            self.merge_hybrid_xref_stream(&mut prev_xref, prev_trailer.remove(b"XRefStm"))?;
            xref.merge(prev_xref);

            prev_xref_start = prev_trailer.remove(b"Prev");
        }
        let xref_entry_count = xref.max_id().checked_add(1).ok_or(ParseError::InvalidXref)?;
        if xref.size != xref_entry_count {
            warn!(
                "Size entry of trailer dictionary is {}, correct value is {}.",
                xref.size, xref_entry_count
            );
            xref.size = xref_entry_count;
        }
        self.document.xref_section_count = sections;

        Ok((xref, trailer))
    }

    /// Merge the cross-reference stream that a section of a hybrid-reference file points to
    /// with `/XRefStm`. Its entries come after the section's table and before older sections.
    fn merge_hybrid_xref_stream(&mut self, xref: &mut Xref, xref_stream_start: Option<Object>) -> Result<()> {
        let Some(start) = xref_stream_start.and_then(|offset| offset.as_i64().ok()) else {
            return Ok(());
        };
        if start < 0 || start as usize > self.buffer.len() {
            return Err(Error::Xref(XrefError::StreamStart));
        }
        match self.xref_and_trailer_at(start as usize) {
            Ok((_, stream_xref, _)) => xref.merge_hybrid(stream_xref),
            // The table alone still describes a hybrid-reference file for older readers.
            Err(e) if !self.strict => warn!("ignoring unreadable cross-reference stream at {start}: {e}"),
            Err(e) => return Err(e),
        }
        Ok(())
    }

    /// Last-resort recovery: rebuild the cross-reference table by scanning the
    /// raw bytes for indirect-object headers and locating a trailer with a
    /// usable `/Root`. Returns `None` (caller keeps the original error) when
    /// strict mode forbids recovery or nothing usable was found.
    fn reconstruct_xref_and_trailer(&mut self) -> Option<(Xref, Dictionary)> {
        if self.strict {
            return None;
        }
        u32::try_from(self.buffer.len()).ok()?;

        let markers = Self::scan_object_markers(self.buffer);
        if markers.is_empty() {
            return None;
        }

        let mut xref = Xref::new(markers.len() as u32, XrefType::CrossReferenceTable);
        for &(offset, (number, generation)) in &markers {
            // Incremental updates append, so later revisions win.
            xref.insert(number, XrefEntry::Normal { offset, generation });
        }
        // Normalize like `resolve_xref_and_trailer`: size spans object numbers
        // up to the highest, not physical copies across incremental updates.
        xref.size = xref.max_id().saturating_add(1);

        let mut trailer = match self.find_latest_trailer(&xref) {
            Some((_trailer_pos, trailer)) => trailer,
            // Files written with cross-reference streams only have no `trailer` keyword, and a
            // file cut off before its last cross-reference section may have neither.
            None => {
                let mut over_limit = false;
                match self.find_latest_xref_stream(&markers, &mut xref, &mut over_limit) {
                    Some(trailer) => trailer,
                    // A cross-reference stream that the decompression limit kept from decoding
                    // is there; the load fails with the limit rather than go on without it.
                    None if over_limit => return None,
                    None => self.find_latest_catalog(&markers, &xref)?,
                }
            }
        };
        // As in resolution, drop the links to other cross-reference sections: the rebuilt
        // table replaces them, and their offsets would go out unchanged on the next save.
        trailer.remove(b"Prev");
        trailer.remove(b"XRefStm");
        // Zero marks the offset "unknown" so `Document::new_from_prev` omits `/Prev`.
        self.document.xref_start = 0;
        self.document.xref_recovery = Some(XrefRecovery::Reconstructed);
        self.document.xref_section_count = 0;

        warn!(
            "reconstructed cross-reference table with {} objects by scanning for indirect objects",
            xref.entries.len()
        );
        Some((xref, trailer))
    }

    /// Collect `(offset, id)` of every indirect-object header in one pass.
    /// Only headers starting a line (optional leading blanks allowed) count,
    /// stream payloads are skipped wholesale, and object numbers beyond
    /// [`MAX_RECONSTRUCTED_OBJECTS`] are rejected so a forged header can
    /// neither shadow a genuine entry nor poison the reconstructed size.
    fn scan_object_markers(buffer: &[u8]) -> Vec<(u32, ObjectId)> {
        const STREAM_KEYWORD: &[u8] = b"stream";
        const END_STREAM_KEYWORD: &[u8] = b"endstream";

        let mut markers = Vec::new();
        let mut oversized_number_warned = false;
        let mut at_line_start = true;
        // Where the dictionary of the next stream can start: past the previous
        // `stream` keyword and the payload skipped after it.
        let mut dictionary_start = 0;
        // No `endstream` follows a later keyword either once one keyword has
        // none, so searching again would rescan the rest of the buffer.
        let mut unterminated = false;
        let mut pos = 0;
        while pos < buffer.len() {
            // Skip raw stream data: a payload may embed convincing `N G obj` lines whose
            // offsets would otherwise override the genuine entries.
            if buffer[pos..].starts_with(STREAM_KEYWORD)
                && !buffer[..pos].ends_with(b"end")
                && matches!(buffer.get(pos + STREAM_KEYWORD.len()), Some(b'\r' | b'\n'))
            {
                let after_keyword = pos + STREAM_KEYWORD.len();
                let dictionary = std::mem::replace(&mut dictionary_start, after_keyword);
                let endstream = if unterminated {
                    None
                } else {
                    find_from(buffer, after_keyword, END_STREAM_KEYWORD)
                };
                unterminated = endstream.is_none();
                // Damaged stream without terminator: fall back to the /Length hint so the
                // payload cannot hide pseudo headers, while later objects stay reachable.
                let payload_end = endstream
                    .map(|end| end + END_STREAM_KEYWORD.len())
                    .or_else(|| Self::payload_end_by_length(buffer, dictionary, pos));
                if let Some(end) = payload_end {
                    pos = end;
                    dictionary_start = end;
                    at_line_start = false;
                    continue;
                }
                // Unusable /Length too: nothing bounds the payload, so keep
                // scanning byte by byte rather than dropping what follows.
            }
            // Anchor at line starts so pseudo headers buried after another
            // token (string literal, comment) are never mistaken for markers.
            if at_line_start
                && buffer[pos].is_ascii_digit()
                && let Some(id) = Self::parse_object_header(&buffer[pos..])
            {
                // A huge bogus number would inflate `size` (and thus `max_id`) via `Xref::insert`.
                if id.0 > MAX_RECONSTRUCTED_OBJECTS as u32 {
                    if !oversized_number_warned {
                        warn!("ignoring object headers numbered above {MAX_RECONSTRUCTED_OBJECTS}");
                        oversized_number_warned = true;
                    }
                } else {
                    if markers.len() == MAX_RECONSTRUCTED_OBJECTS {
                        warn!(
                            "object marker scan stopped at the {MAX_RECONSTRUCTED_OBJECTS}-marker cap; reconstruction may be incomplete"
                        );
                        break;
                    }
                    markers.push((pos as u32, id));
                }
            }
            match buffer[pos] {
                b'\r' | b'\n' => at_line_start = true,
                b' ' | b'\t' => {}
                _ => at_line_start = false,
            }
            pos += 1;
        }
        markers
    }

    /// Resume offset past a stream payload according to a *direct* `/Length`
    /// integer in the dictionary preceding the `stream` keyword at
    /// `stream_pos`. The search is scoped to the current object (after the
    /// nearest preceding `obj` keyword) so a missing `/Length` cannot latch
    /// onto a previous object's value. Nor does it reach back before
    /// `dictionary_start`, where the previous keyword or its payload ended:
    /// that dictionary belongs to another stream, and stopping there keeps
    /// the searches of all keywords to one pass over the buffer. Returns
    /// `None` when the hint is absent, indirect, or points outside the
    /// buffer — damaged files tend to carry wrong `/Length` values, so it
    /// must stay a hint, never a hard boundary.
    fn payload_end_by_length(buffer: &[u8], dictionary_start: usize, stream_pos: usize) -> Option<usize> {
        const LENGTH_KEY: &[u8] = b"/Length";
        const OBJ_KEYWORD: &[u8] = b"obj";
        const STREAM_KEYWORD_LEN: usize = b"stream".len();

        let head = buffer.get(dictionary_start..stream_pos)?;
        let obj_pos = dictionary_start
            + head
                .windows(OBJ_KEYWORD.len())
                .rposition(|window| window == OBJ_KEYWORD)?;
        let dict_region = &buffer[obj_pos + OBJ_KEYWORD.len()..stream_pos];
        // Nearest `/Length` in this object's dictionary; parsing it validates
        // that the match really is a key with an integer value.
        let key_pos = dict_region
            .windows(LENGTH_KEY.len())
            .rposition(|window| window == LENGTH_KEY)?;
        let rest = &dict_region[key_pos + LENGTH_KEY.len()..];
        let digits_start = rest.iter().position(u8::is_ascii_digit)?;
        let digits_len = rest[digits_start..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits_len > 10 {
            return None;
        }
        // An indirect reference (`/Length 5 0 R`) continues with another
        // number token; a direct integer ends at a name, `>>`, or the keyword.
        match rest[digits_start + digits_len..]
            .iter()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace())
        {
            Some(b'/') | Some(b'>') => {}
            _ => return None,
        }
        let length: usize = std::str::from_utf8(&rest[digits_start..digits_start + digits_len])
            .ok()?
            .parse()
            .ok()?;
        // Spec: the EOL after the `stream` keyword counts as CRLF or LF.
        let eol_len = match (
            buffer.get(stream_pos + STREAM_KEYWORD_LEN),
            buffer.get(stream_pos + STREAM_KEYWORD_LEN + 1),
        ) {
            (Some(b'\r'), Some(b'\n')) => 2,
            _ => 1,
        };
        let end = (stream_pos + STREAM_KEYWORD_LEN + eol_len).checked_add(length)?;
        (buffer.len() >= end).then_some(end)
    }

    /// Read the linearization parameter dictionary: the first indirect object of the file, when
    /// its header starts within the first 1024 bytes (ISO 32000-2 Annex F.2.2). The object is
    /// read where it stands, whatever the cross-reference data says, because an incremental
    /// update may free or replace it without making the file any less linearized as written.
    fn read_linearization(&self) -> Option<Linearization> {
        const FIRST_OBJECT_WINDOW: usize = 1024;
        let window = &self.buffer[..self.buffer.len().min(FIRST_OBJECT_WINDOW)];
        let start = (0..window.len()).find(|&position| {
            window[position].is_ascii_digit()
                && position > 0
                && matches!(window[position - 1], b' ' | b'\t' | b'\r' | b'\n')
                && Self::parse_object_header(&self.buffer[position..]).is_some()
        })?;
        let (id, object) = self.read_object(start, None, &mut HashSet::new()).ok()?;
        Linearization::from_dictionary(id, object.as_dict().ok()?)
    }

    /// Parse an `N G obj` header; the byte after `obj` must end the token.
    fn parse_object_header(input: &[u8]) -> Option<ObjectId> {
        // deliberate: caps at u32/u16 width bound work on pathological padding.
        fn digits(input: &[u8], cap: usize) -> Option<(&[u8], &[u8])> {
            let n = input.iter().take_while(|b| b.is_ascii_digit()).count();
            (0 < n && n <= cap).then(|| input.split_at(n))
        }
        fn spaces(input: &[u8]) -> Option<&[u8]> {
            let n = input
                .iter()
                .take_while(|&&b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                .count();
            (n > 0).then(|| &input[n..])
        }

        let (number, rest) = digits(input, 10)?;
        let number: u32 = std::str::from_utf8(number).ok()?.parse().ok()?;
        let rest = spaces(rest)?;
        let (generation, rest) = digits(rest, 5)?;
        let generation: u16 = std::str::from_utf8(generation).ok()?.parse().ok()?;
        let rest = spaces(rest)?;
        let tail = rest.strip_prefix(b"obj")?;
        match tail.first() {
            None => {}
            Some(&byte) if !byte.is_ascii_alphanumeric() => {}
            _ => return None,
        }
        Some((number, generation))
    }

    /// Newest backwards-scanned `trailer` whose `/Root` references a scanned object.
    fn find_latest_trailer(&self, xref: &Xref) -> Option<(usize, Dictionary)> {
        let mut bound = self.buffer.len();
        for _ in 0..MAX_TRAILER_CANDIDATES {
            let Some(pos) = Reader::search_substring(&self.buffer[..bound], b"trailer", 0) else {
                break;
            };
            bound = pos;

            let after_keyword = &self.buffer[pos + b"trailer".len()..];
            let Some(dict_start) = after_keyword.iter().position(|byte| !byte.is_ascii_whitespace()) else {
                continue;
            };
            let Ok((_, dict)) = parser::dictionary(&after_keyword[dict_start..]) else {
                continue;
            };
            if dict
                .get(b"Root")
                .and_then(Object::as_reference)
                .is_ok_and(|root| xref.entries.contains_key(&root.0))
            {
                return Some((pos, dict));
            }
        }
        None
    }

    /// Dictionary of the newest cross-reference stream whose `/Root` the rebuilt
    /// table can resolve, for files without a `trailer` keyword. Like trailers,
    /// the streams are found by searching backwards, here for their `/XRef`
    /// type. Objects inside object streams have no header for the scan to
    /// find, so the compressed entries of these streams are added for object
    /// numbers the scan did not see, newer streams first, as resolution merges
    /// `/Prev` sections.
    /// `over_limit` is set when a stream does not decode within the decompression limit.
    fn find_latest_xref_stream(
        &self, markers: &[(u32, ObjectId)], xref: &mut Xref, over_limit: &mut bool,
    ) -> Option<Dictionary> {
        let mut dictionaries = Vec::new();
        let mut compressed = BTreeMap::new();
        let mut bound = self.buffer.len();
        for _ in 0..MAX_TRAILER_CANDIDATES {
            let Some(pos) = Reader::search_substring(&self.buffer[..bound], b"/XRef", 0) else {
                break;
            };
            // Not `/XRefStm`, the trailer key of a hybrid-reference file.
            if matches!(self.buffer.get(pos + b"/XRef".len()), Some(byte) if byte.is_ascii_alphanumeric()) {
                bound = pos;
                continue;
            }
            // The match lies in the object whose header comes last before it,
            // which is read once however many matches it holds.
            let Some(index) = markers
                .partition_point(|&(offset, _)| (offset as usize) < pos)
                .checked_sub(1)
            else {
                break;
            };
            let (offset, id) = markers[index];
            bound = offset as usize;
            let end = markers
                .get(index + 1)
                .map_or(self.buffer.len(), |&(next, _)| next as usize);
            let Ok((_, Object::Stream(stream))) =
                self.read_object_to(offset as usize, end, Some(id), &mut HashSet::new())
            else {
                continue;
            };
            if !stream.dict.has_type(b"XRef") {
                continue;
            }
            let (stream_xref, dictionary) = match decode_xref_stream_with_limit(stream, self.max_decompressed_size) {
                Ok(decoded) => decoded,
                Err(Error::Decompress(DecompressError::MemoryLimitExceeded { .. })) => {
                    *over_limit = true;
                    continue;
                }
                Err(_) => continue,
            };
            for (number, entry) in stream_xref.entries {
                if entry.is_compressed()
                    && number <= MAX_RECONSTRUCTED_OBJECTS as u32
                    && !xref.entries.contains_key(&number)
                {
                    compressed.entry(number).or_insert(entry);
                }
            }
            dictionaries.push(dictionary);
        }

        let resolvable = |number| match compressed.get(&number) {
            Some(XrefEntry::Compressed { container, .. }) => xref.entries.contains_key(container),
            _ => xref.entries.contains_key(&number),
        };
        let trailer = dictionaries.into_iter().find(|dictionary| {
            dictionary
                .get(b"Root")
                .and_then(Object::as_reference)
                .is_ok_and(|root| resolvable(root.0))
        })?;
        for (number, entry) in compressed {
            xref.insert(number, entry);
        }
        xref.cross_reference_type = XrefType::CrossReferenceStream;
        Some(trailer)
    }

    /// A trailer naming the newest scanned catalog as `/Root`, for a file in which no trailer
    /// survives, as when it was cut off before its last cross-reference section. The catalog
    /// has to be a top-level object that the rebuilt table resolves to, as it does to the
    /// newest copy of each object, and its `/Pages` has to resolve to the root of a page tree.
    /// The trailer has no `/Info` or `/ID`, and without `/Encrypt` and `/ID` an encrypted file
    /// could not be decrypted, so an encrypted file gets none.
    fn find_latest_catalog(&self, markers: &[(u32, ObjectId)], xref: &Xref) -> Option<Dictionary> {
        if self.has_encryption(markers) {
            return None;
        }
        let mut bound = self.buffer.len();
        for _ in 0..MAX_TRAILER_CANDIDATES {
            let pos = Reader::search_substring(&self.buffer[..bound], b"/Catalog", 0)?;
            // As for cross-reference streams, the match lies in the object whose header comes
            // last before it.
            let index = markers
                .partition_point(|&(offset, _)| (offset as usize) < pos)
                .checked_sub(1)?;
            let (offset, id) = markers[index];
            bound = offset as usize;
            // A newer copy of the object replaced this one.
            if !matches!(xref.get(id.0), Some(&XrefEntry::Normal { offset: newest, generation })
                if newest == offset && generation == id.1)
            {
                continue;
            }
            if let Some(Object::Dictionary(catalog)) = self.read_scanned(markers, xref, id)
                && catalog.has_type(b"Catalog")
                && let Ok(pages) = catalog.get(b"Pages").and_then(Object::as_reference)
                && let Some(Object::Dictionary(root)) = self.read_scanned(markers, xref, pages)
                && matches!(root.get(b"Kids"), Ok(Object::Array(_) | Object::Reference(_)))
            {
                warn!("no trailer survives; using catalog {} {} as /Root", id.0, id.1);
                return Some(dictionary! { "Root" => Object::Reference(id) });
            }
        }
        None
    }

    /// The object that the rebuilt table resolves `id` to, read between its header and the
    /// next one the scan found.
    fn read_scanned(&self, markers: &[(u32, ObjectId)], xref: &Xref, id: ObjectId) -> Option<Object> {
        let Some(&XrefEntry::Normal { offset, generation }) = xref.get(id.0) else {
            return None;
        };
        if generation != id.1 {
            return None;
        }
        let index = markers.partition_point(|&(marker, _)| marker < offset);
        let end = markers
            .get(index + 1)
            .map_or(self.buffer.len(), |&(next, _)| next as usize);
        let (_, object) = self
            .read_object_to(offset as usize, end, Some(id), &mut HashSet::new())
            .ok()?;
        Some(object)
    }

    /// Whether the file names `/Encrypt` or holds an encryption dictionary. Cutting a file off
    /// before its trailer takes `/Encrypt` away, but not the dictionary it referenced.
    fn has_encryption(&self, markers: &[(u32, ObjectId)]) -> bool {
        if Reader::search_substring(self.buffer, b"/Encrypt", 0).is_some() {
            return true;
        }
        markers.iter().enumerate().any(|(index, &(offset, id))| {
            let end = markers
                .get(index + 1)
                .map_or(self.buffer.len(), |&(next, _)| next as usize);
            let bytes = &self.buffer[offset as usize..end];
            // An encryption dictionary names its security handler with `/Filter`, and it is
            // never a stream; read only the objects that could be one.
            find_from(bytes, 0, b"/Filter").is_some()
                && find_from(bytes, 0, b"stream").is_none()
                && matches!(
                    self.read_object_to(offset as usize, end, Some(id), &mut HashSet::new()),
                    Ok((_, Object::Dictionary(dictionary))) if Self::is_encryption_dictionary(&dictionary)
                )
        })
    }

    /// `/O` and `/U` belong to the standard security handler, `/Recipients` to public-key ones,
    /// and `/CF` to both from version 4.
    fn is_encryption_dictionary(dictionary: &Dictionary) -> bool {
        dictionary.get(b"Filter").and_then(Object::as_name).is_ok()
            && (dictionary.has(b"O") && dictionary.has(b"U") || dictionary.has(b"Recipients") || dictionary.has(b"CF"))
    }

    /// Some generators write `startxref` (or trailer `Prev`) values that are
    /// slightly off — most commonly the offset of the line *after* the `xref`
    /// keyword instead of the keyword itself. Such files are otherwise intact,
    /// and common readers (Acrobat, poppler, mupdf, pdf.js) recover from them.
    /// In lenient mode, if `offset` does not point at a cross-reference table
    /// or an indirect object (cross-reference stream), scan a small window
    /// around it for the `xref` keyword and return the nearest match instead,
    /// which `xref_and_trailer_at` checks before using it. Without one, as in
    /// a file with cross-reference streams only, return the nearest object
    /// header that starts a cross-reference stream.
    fn correct_xref_offset(&self, offset: usize) -> usize {
        const RECOVERY_WINDOW: usize = 64;

        if self.strict || offset >= self.buffer.len() {
            return offset;
        }
        let rest = &self.buffer[offset..];
        if rest.starts_with(b"xref") || Self::starts_indirect_object(rest) {
            return offset;
        }

        let window_start = offset.saturating_sub(RECOVERY_WINDOW);
        let window_end = cmp::min(self.buffer.len(), offset + RECOVERY_WINDOW);
        let mut corrected: Option<usize> = None;
        for pos in window_start..window_end.saturating_sub(4) {
            if !self.buffer[pos..].starts_with(b"xref") {
                continue;
            }
            // `startxref` contains `xref`; never match inside it.
            if pos >= 5 && &self.buffer[pos - 5..pos] == b"start" {
                continue;
            }
            if corrected.is_none_or(|best: usize| pos.abs_diff(offset) < best.abs_diff(offset)) {
                corrected = Some(pos);
            }
        }
        corrected
            .or_else(|| self.nearest_xref_stream(offset, window_start, window_end))
            .unwrap_or(offset)
    }

    /// Start of the object header between `window_start` and `window_end` that
    /// is nearest to `offset` and begins a cross-reference stream. The stream
    /// parser itself skips white-space before a header, so an `offset` there
    /// needs no correction.
    fn nearest_xref_stream(&self, offset: usize, window_start: usize, window_end: usize) -> Option<usize> {
        let rest = &self.buffer[offset..];
        if Self::starts_indirect_object(&rest[skip_whitespace(rest, 0)..]) {
            return None;
        }
        // A header starts a token: `12 0 obj` must not also match as `2 0 obj`.
        let mut headers: Vec<usize> = (window_start..window_end)
            .filter(|&pos| pos == 0 || !self.buffer[pos - 1].is_ascii_alphanumeric())
            .filter(|&pos| Self::starts_indirect_object(&self.buffer[pos..]))
            .collect();
        headers.sort_by_key(|pos| pos.abs_diff(offset));
        headers.into_iter().find(|&pos| {
            matches!(
                parser::indirect_object(self.buffer, pos, None, self, &mut HashSet::new(), None),
                Ok((_, Object::Stream(stream))) if stream.dict.has_type(b"XRef")
            )
        })
    }

    /// Whether `input` begins with an indirect-object header (`N G obj`), the
    /// form a cross-reference stream starts with.
    fn starts_indirect_object(input: &[u8]) -> bool {
        Self::parse_object_header(input).is_some()
    }

    fn get_xref_start(buffer: &[u8]) -> Result<usize> {
        // Like other readers, tolerate white-space padding (often NUL bytes) after `%%EOF` and
        // look for the marker within the last 1024 bytes of the rest.
        let end = buffer
            .iter()
            .rposition(|c| !b" \t\n\r\0\x0C".contains(c))
            .map_or(0, |pos| pos + 1);
        let seek_pos = end - cmp::min(end, 1024);
        Self::search_substring(&buffer[..end], b"%%EOF", seek_pos)
            .filter(|&eof_pos| eof_pos > 25)
            .and_then(|eof_pos| Self::search_substring(&buffer[..eof_pos], b"startxref", eof_pos - 25))
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
        buffer
            .get(start_pos..)?
            .windows(pattern.len())
            .rposition(|window| window == pattern)
            .map(|pos| start_pos + pos)
    }
}

/// Advance past ASCII whitespace, returning the first index that is not one (or
/// `input.len()` when the rest is all whitespace).
fn skip_whitespace(input: &[u8], mut pos: usize) -> usize {
    while pos < input.len() && input[pos].is_ascii_whitespace() {
        pos += 1;
    }
    pos
}

/// Read the run of ASCII digits at `pos` as `N`, together with the index just
/// past it. An empty or unparsable run is a malformed file, not a short read.
fn scan_number<N: std::str::FromStr>(input: &[u8], mut pos: usize) -> Result<(N, usize)> {
    let start = pos;
    while pos < input.len() && input[pos].is_ascii_digit() {
        pos += 1;
    }

    let value = std::str::from_utf8(&input[start..pos])
        .ok()
        .and_then(|digits| digits.parse().ok())
        .ok_or(Error::Parse(ParseError::InvalidXref))?;

    Ok((value, pos))
}

/// The first index at or after `start` where `pattern` begins, or `None`.
fn find_from(input: &[u8], start: usize, pattern: &[u8]) -> Option<usize> {
    input
        .get(start..)?
        .windows(pattern.len())
        .position(|window| window == pattern)
        .map(|pos| start + pos)
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
fn load_document_with_preceding_bytes() {
    let mut content = Vec::new();
    content.extend(b"garbage");
    content.extend(include_bytes!("../assets/example.pdf"));
    let doc = Document::load_mem(&content).unwrap();
    assert_eq!(doc.version, "1.5");
}

#[test]
fn load_many_shallow_brackets() {
    let content: String = std::iter::repeat_n("()", MAX_BRACKET * 10)
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
    let content: Vec<u8> = std::iter::repeat_n(b'(', MAX_BRACKET + 1)
        .chain(std::iter::repeat_n(b')', MAX_BRACKET + 1))
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

#[cfg(all(test, not(feature = "async")))]
#[test]
fn search_substring_finds_last_occurrence() {
    assert_eq!(Reader::search_substring(b"hello world", b"xyz", 0), None);
    assert_eq!(Reader::search_substring(b"hello world", b"world", 0), Some(6));

    let buffer = b"%%EOF\ntest%%EOF\nend";
    assert_eq!(Reader::search_substring(buffer, b"%%EOF", 0), Some(10));
    assert_eq!(Reader::search_substring(buffer, b"%%EOF", 6), Some(10));
    assert_eq!(Reader::search_substring(buffer, b"%%EOF", 15), None);
    assert_eq!(Reader::search_substring(b"%%EOF", b"%%EOF", 0), Some(0));

    let buffer_with_many_percents = b"%%%PDF-1.3%%%comment%%%more%%EOF";
    assert_eq!(
        Reader::search_substring(buffer_with_many_percents, b"%%EOF", 0),
        Some(27)
    );
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn get_xref_start_ignores_startxref_past_eof() {
    // Two revisions, the second with a corrupted %%EOF: the startxref *before* the valid
    // %%EOF must win over the one after it.
    let mut buf = Vec::new();
    // Padding so the buffer is large enough
    buf.extend_from_slice(&[b' '; 200]);
    // First (correct) startxref + xref offset + valid %%EOF
    let xref_offset = 100usize;
    let startxref_block = format!("startxref\n{}\n%%EOF\n", xref_offset);
    let _startxref_pos = buf.len();
    buf.extend_from_slice(startxref_block.as_bytes());
    // Second (corrupted) revision: another startxref pointing elsewhere, with %%EO\0
    let bad_block = "startxref\n999\n%%EO\x00\n".to_string();
    buf.extend_from_slice(bad_block.as_bytes());

    let result = Reader::get_xref_start(&buf).unwrap();
    // Should find the xref_offset from the first startxref (before valid %%EOF)
    assert_eq!(result, xref_offset);
    // Verify it did NOT pick up 999 from the corrupted revision
    assert_ne!(result, 999);
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn get_xref_start_tolerates_data_after_eof() {
    let mut doc = Document::load("assets/example.pdf").unwrap();
    // A file with only a cross-reference stream has no `trailer` keyword, so reconstruction
    // cannot recover it when `startxref` is not found.
    doc.reference_table.cross_reference_type = XrefType::CrossReferenceStream;
    let mut buffer = Vec::new();
    doc.save_to(&mut buffer).unwrap();
    let xref_start = Reader::get_xref_start(&buffer).unwrap();

    // NUL and white-space padding of any length.
    let mut padded = buffer.clone();
    padded.extend_from_slice(&[0; 20_000]);
    padded.extend_from_slice(b"\r\n \x0C\t");
    assert_eq!(Reader::get_xref_start(&padded).unwrap(), xref_start);

    let options = LoadOptions {
        strict: true,
        ..Default::default()
    };
    let loaded = Document::load_mem_with_options(&padded, options).unwrap();
    assert_eq!(loaded.get_pages().len(), 1);

    // Other trailing data, as long as `%%EOF` is within the last 1024 bytes.
    let mut trailing = buffer.clone();
    trailing.extend_from_slice(&[b'x'; 1000]);
    assert_eq!(Reader::get_xref_start(&trailing).unwrap(), xref_start);
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn recovers_from_miswritten_startxref_offset() {
    // Some generators point `startxref` at the line *after* the `xref` keyword: lenient
    // loading must recover, strict loading must still reject.
    let header = "%PDF-1.5\n";
    let obj1 = "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n";
    let obj2 = "2 0 obj<</Type/Pages/Kids[]/Count 0>>endobj\n";
    let o1 = header.len();
    let o2 = o1 + obj1.len();
    let body = format!("{header}{obj1}{obj2}");
    let xref_pos = body.len();
    let doc = format!(
        "{body}xref
0 3
0000000000 65535 f 
{o1:010} 00000 n 
{o2:010} 00000 n 
trailer
<</Root 1 0 R/Size 3>>
startxref
{}
%%EOF",
        xref_pos + 4 // past the `xref` keyword, onto the line after it
    );

    let loaded = Document::load_mem(doc.as_bytes()).unwrap();
    assert!(loaded.trailer.get(b"Root").is_ok());
    assert_eq!(loaded.xref_start, xref_pos);
    assert_eq!(loaded.xref_recovery, Some(XrefRecovery::OffsetCorrected));
    let metadata = Document::load_metadata_mem(doc.as_bytes()).unwrap();
    assert_eq!(metadata.xref_recovery, Some(XrefRecovery::OffsetCorrected));

    let strict = Document::load_mem_with_options(
        doc.as_bytes(),
        LoadOptions {
            strict: true,
            ..Default::default()
        },
    );
    assert!(strict.is_err());
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn reports_a_corrected_prev_offset() {
    // An incremental update whose `/Prev` points past the `xref` keyword of the first section.
    let header = "%PDF-1.5\n";
    let obj1 = "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n";
    let obj2 = "2 0 obj<</Type/Pages/Kids[]/Count 0>>endobj\n";
    let o1 = header.len();
    let o2 = o1 + obj1.len();
    let body = format!("{header}{obj1}{obj2}");
    let prev = body.len();
    let first = format!(
        "{body}xref
0 3
0000000000 65535 f 
{o1:010} 00000 n 
{o2:010} 00000 n 
trailer
<</Root 1 0 R/Size 3>>
startxref
{prev}
%%EOF
"
    );
    let update = "1 0 obj<</Type/Catalog/Pages 2 0 R/Lang(en)>>endobj\n";
    let o1_update = first.len();
    let xref_pos = o1_update + update.len();
    let doc = format!(
        "{first}{update}xref
1 1
{o1_update:010} 00000 n 
trailer
<</Root 1 0 R/Size 3/Prev {}>>
startxref
{xref_pos}
%%EOF",
        prev + 4
    );

    let loaded = Document::load_mem(doc.as_bytes()).unwrap();
    assert!(loaded.catalog().unwrap().has(b"Lang"));
    assert!(loaded.get_object((2, 0)).is_ok());
    assert_eq!(loaded.xref_recovery, Some(XrefRecovery::OffsetCorrected));

    // The same file with the right offset is read as written.
    let intact = doc.replace(&format!("/Prev {}", prev + 4), &format!("/Prev {prev}"));
    assert_eq!(Document::load_mem(intact.as_bytes()).unwrap().xref_recovery, None);
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn recovers_from_miswritten_startxref_offset_of_a_cross_reference_stream() {
    // Without an `xref` keyword nearby, the header of the cross-reference stream is used,
    // whether `startxref` stops short of it or points into it.
    let mut document = Document::load("assets/example.pdf").unwrap();
    document.reference_table.cross_reference_type = XrefType::CrossReferenceStream;
    let mut buffer = Vec::new();
    document.save_to(&mut buffer).unwrap();
    let xref_start = Reader::get_xref_start(&buffer).unwrap();
    let keyword = buffer.windows(9).rposition(|window| window == b"startxref").unwrap();
    let with_startxref = |offset: usize| {
        let mut damaged = buffer[..keyword].to_vec();
        damaged.extend_from_slice(format!("startxref\n{offset}\n%%EOF").as_bytes());
        damaged
    };

    for offset in [xref_start - 2, xref_start + 3] {
        let damaged = with_startxref(offset);
        let loaded = Document::load_mem(&damaged).unwrap();
        assert_eq!(loaded.xref_start, xref_start);
        assert_eq!(loaded.get_pages().len(), 1);

        let strict = Document::load_mem_with_options(
            &damaged,
            LoadOptions {
                strict: true,
                ..Default::default()
            },
        );
        assert!(strict.is_err());
    }

    // The parser skips white-space before the header, so that offset is kept.
    let loaded = Document::load_mem(&with_startxref(xref_start - 1)).unwrap();
    assert_eq!(loaded.xref_start, xref_start - 1);
    assert_eq!(loaded.get_pages().len(), 1);
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn rebuilds_rather_than_correcting_to_a_table_of_shifted_objects() {
    // The table and `startxref` were written for a file with 10 more bytes before the
    // objects, as when bytes are later removed. The nearby `xref` keyword leads to a table
    // whose offsets all miss their objects; scanning finds the objects where they are.
    let header = "%PDF-1.5\n";
    let obj1 = "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n";
    let obj2 = "2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n";
    let obj3 = "3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n";
    let removed = 10;
    let o1 = header.len() + removed;
    let o2 = o1 + obj1.len();
    let o3 = o2 + obj2.len();
    let body = format!("{header}{obj1}{obj2}{obj3}");
    let doc = format!(
        "{body}xref
0 4
0000000000 65535 f 
{o1:010} 00000 n 
{o2:010} 00000 n 
{o3:010} 00000 n 
trailer
<</Root 1 0 R/Size 4>>
startxref
{}
%%EOF",
        body.len() + removed
    );

    let loaded = Document::load_mem(doc.as_bytes()).unwrap();
    assert_eq!(loaded.get_pages().len(), 1);
    // Zero marks a table rebuilt by scanning rather than read at a corrected offset.
    assert_eq!(loaded.xref_start, 0);
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn stream_length_written_as_real() {
    // "/Length 42." must still resolve to the stream content rather than load as empty.
    let obj4 = "4 0 obj\n<< /Length 42. >>\nstream\nBT /F1 12 Tf 20 100 Td (Hello World) Tj ET\nendstream\nendobj\n";
    let header = "%PDF-1.7\n";
    let obj1 = "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n";
    let obj2 = "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n";
    let obj3 = "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>\nendobj\n";
    let o1 = header.len();
    let o2 = o1 + obj1.len();
    let o3 = o2 + obj2.len();
    let o4 = o3 + obj3.len();
    let body = format!("{header}{obj1}{obj2}{obj3}{obj4}");
    let xref_pos = body.len();
    let doc = format!(
        "{body}xref
0 5
0000000000 65535 f 
{o1:010} 00000 n 
{o2:010} 00000 n 
{o3:010} 00000 n 
{o4:010} 00000 n 
trailer
<< /Size 5 /Root 1 0 R >>
startxref
{xref_pos}
%%EOF
"
    );

    let loaded = Document::load_mem(doc.as_bytes()).unwrap();
    let stream = loaded.get_object((4, 0)).unwrap().as_stream().unwrap();
    assert_eq!(stream.content, b"BT /F1 12 Tf 20 100 Td (Hello World) Tj ET");
}

#[test]
fn indirect_dictionary_is_a_stream_only_before_the_stream_keyword() {
    let read = |input: &[u8]| parser::indirect_object(input, 0, None, &Reader::new(input), &mut HashSet::new(), None);

    // Without the keyword and an end-of-line marker after it, the dictionary is the object.
    for input in [
        &b"1 0 obj <</A 1>> endobj"[..],
        b"1 0 obj <</A 1>>\nendobj",
        b"1 0 obj <</A 1>>stream endobj",
        b"1 0 obj <</A 1>> /B 2",
    ] {
        let (id, object) = read(input).unwrap();
        assert_eq!(id, (1, 0));
        assert_eq!(object, Object::Dictionary(crate::dictionary! { "A" => 1 }), "{input:?}");
    }

    let (_, object) = read(b"1 0 obj <</Length 3>> stream\r\nabc\nendstream endobj").unwrap();
    assert_eq!(object.as_stream().unwrap().content, b"abc");

    // A stream whose data cannot be read fails rather than loading as its dictionary.
    assert!(read(b"1 0 obj <</Length -1>>stream\nabc\nendstream endobj").is_err());
    // So does an unterminated dictionary.
    assert!(read(b"1 0 obj <</A 1 endobj").is_err());
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn object_marker_parsing_accepts_only_real_headers() {
    assert_eq!(Reader::parse_object_header(b"12 0 obj<<"), Some((12, 0)));
    assert_eq!(Reader::parse_object_header(b"007 0 obj\n"), Some((7, 0)));
    assert_eq!(Reader::parse_object_header(b"5 2 obj>>"), Some((5, 2)));
    assert_eq!(Reader::parse_object_header(b"12\t3\r\nobj["), Some((12, 3)));
    assert_eq!(Reader::parse_object_header(b"2 0 objects"), None);
    assert_eq!(Reader::parse_object_header(b"99 88 objx"), None);
    assert_eq!(Reader::parse_object_header(b"12345678901 0 obj"), None);
    assert_eq!(Reader::parse_object_header(b"1 70000 obj"), None);
    assert_eq!(Reader::parse_object_header(b"1 0obj"), None);
    assert_eq!(Reader::parse_object_header(b"1  obj"), None);
    assert_eq!(Reader::parse_object_header(b"x 0 obj"), None);
}

/// A hybrid-reference file: the page tree lives in an object stream that only the
/// cross-reference stream at `/XRefStm` lists. Returns the file and the offset of its
/// `xref` table.
#[cfg(all(test, not(feature = "async")))]
fn hybrid_reference_file() -> (Vec<u8>, usize) {
    let header = "%PDF-1.5\n";
    let obj1 = "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n";
    let pages = "<< /Type /Pages /Kids [3 0 R] /Count 1 >>";
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> >>";
    let offsets = format!("2 0 3 {} ", pages.len() + 1);
    let content = format!("{offsets}{pages} {page}");
    let obj4 = format!(
        "4 0 obj\n<< /Type /ObjStm /N 2 /First {} /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
        offsets.len(),
        content.len(),
    );
    let o1 = header.len();
    let o4 = o1 + obj1.len();
    let o5 = o4 + obj4.len();

    // Entries for objects 2 to 5 with /W [1 2 1].
    let [o4_hi, o4_lo] = u16::try_from(o4).unwrap().to_be_bytes();
    let [o5_hi, o5_lo] = u16::try_from(o5).unwrap().to_be_bytes();
    let entries = [2, 0, 4, 0, 2, 0, 4, 1, 1, o4_hi, o4_lo, 0, 1, o5_hi, o5_lo, 0];
    let mut buffer = format!("{header}{obj1}{obj4}").into_bytes();
    buffer.extend_from_slice(
        format!(
            "5 0 obj\n<< /Type /XRef /Size 6 /Index [2 4] /W [1 2 1] /Length {} >>\nstream\n",
            entries.len()
        )
        .as_bytes(),
    );
    buffer.extend_from_slice(&entries);
    buffer.extend_from_slice(b"\nendstream\nendobj\n");

    let xref_pos = buffer.len();
    buffer.extend_from_slice(
        format!(
            "xref
0 2
0000000000 65535 f 
{o1:010} 00000 n 
trailer
<< /Size 6 /Root 1 0 R /XRefStm {o5} >>
startxref
{xref_pos}
%%EOF
"
        )
        .as_bytes(),
    );
    (buffer, xref_pos)
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn reads_xref_stream_of_hybrid_reference_file() {
    let (buffer, _) = hybrid_reference_file();

    let loaded = Document::load_mem(&buffer).unwrap();
    assert_eq!(loaded.get_pages().len(), 1);
    // The offset is meaningless once the document is written again.
    assert!(!loaded.trailer.has(b"XRefStm"));
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn reads_xref_stream_of_earlier_hybrid_reference_section() {
    // An incremental update with a plain `xref` table on top of a hybrid-reference file.
    let (mut buffer, prev) = hybrid_reference_file();
    let obj1 = buffer.len();
    buffer.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R /Lang (en) >>\nendobj\n");
    let xref_pos = buffer.len();
    buffer.extend_from_slice(
        format!(
            "xref
1 1
{obj1:010} 00000 n 
trailer
<< /Size 6 /Root 1 0 R /Prev {prev} >>
startxref
{xref_pos}
%%EOF
"
        )
        .as_bytes(),
    );

    let loaded = Document::load_mem(&buffer).unwrap();
    assert_eq!(loaded.get_pages().len(), 1);
    assert!(loaded.catalog().unwrap().has(b"Lang"));
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn reads_xref_stream_of_hybrid_reference_file_with_free_table_entries() {
    // Hybrid-reference files usually list the objects of their object streams as free in
    // the table, so that readers without cross-reference stream support skip them.
    let (buffer, xref_pos) = hybrid_reference_file();
    let (head, table) = buffer.split_at(xref_pos);
    let table = String::from_utf8(table.to_vec()).unwrap().replacen(
        " n \ntrailer",
        &format!(" n \n2 4\n{}trailer", "0000000000 00001 f \n".repeat(4)),
        1,
    );
    assert!(table.contains("2 4\n"));
    let buffer = [head, table.as_bytes()].concat();

    let loaded = Document::load_mem(&buffer).unwrap();
    assert_eq!(loaded.xref_recovery, None);
    assert!(matches!(
        loaded.reference_table.get(2),
        Some(XrefEntry::Compressed { .. })
    ));
    assert_eq!(loaded.get_pages().len(), 1);
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn free_entry_of_incremental_update_deletes_object() {
    let header = "%PDF-1.4\n";
    let obj1 = "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n";
    let obj2 = "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n";
    let obj3 = "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>\nendobj\n";
    let obj4 = "4 0 obj\n<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] >>\nendobj\n";
    let o1 = header.len();
    let o2 = o1 + obj1.len();
    let o3 = o2 + obj2.len();
    let o4 = o3 + obj3.len();
    let prev = o4 + obj4.len();
    let mut buffer = format!(
        "{header}{obj1}{obj2}{obj3}{obj4}xref
0 5
0000000000 65535 f 
{o1:010} 00000 n 
{o2:010} 00000 n 
{o3:010} 00000 n 
{o4:010} 00000 n 
trailer
<< /Size 5 /Root 1 0 R >>
startxref
{prev}
%%EOF
"
    );
    // The update drops the annotation from the page and deletes object 4.
    let new3 = buffer.len();
    buffer.push_str("3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>\nendobj\n");
    let xref_pos = buffer.len();
    buffer.push_str(&format!(
        "xref
0 1
0000000000 65535 f 
3 2
{new3:010} 00000 n 
0000000000 00001 f 
trailer
<< /Size 5 /Root 1 0 R /Prev {prev} >>
startxref
{xref_pos}
%%EOF
"
    ));

    let loaded = Document::load_mem(buffer.as_bytes()).unwrap();
    assert!(!loaded.get_object((3, 0)).unwrap().as_dict().unwrap().has(b"Annots"));
    assert!(loaded.get_object((4, 0)).is_err());
    assert!(!loaded.objects.contains_key(&(4, 0)));
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn saving_a_reconstructed_hybrid_reference_file_writes_no_stale_xref_stm() {
    // With `startxref` past the end, the file is rebuilt by scanning, and its trailer
    // still names the cross-reference stream by an offset into the damaged file.
    let (mut buffer, xref_pos) = hybrid_reference_file();
    let tail = format!("startxref\n{xref_pos}\n%%EOF\n");
    buffer.truncate(buffer.len() - tail.len());
    buffer.extend_from_slice(format!("startxref\n{}\n%%EOF\n", buffer.len() + 4096).as_bytes());

    let mut loaded = Document::load_mem(&buffer).unwrap();
    assert_eq!(loaded.get_pages().len(), 1);
    assert!(!loaded.trailer.has(b"XRefStm"));

    let mut saved = Vec::new();
    loaded.save_to(&mut saved).unwrap();
    let strict = LoadOptions {
        strict: true,
        ..Default::default()
    };
    let reloaded = Document::load_mem_with_options(&saved, strict).unwrap();
    assert_eq!(reloaded.get_pages().len(), 1);
}

#[cfg(all(test, not(feature = "async")))]
#[test]
fn ignores_unreadable_hybrid_xref_stream_when_lenient() {
    // `/XRefStm` pointing at an object that is not a cross-reference stream.
    let header = "%PDF-1.5\n";
    let obj1 = "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n";
    let obj2 = "2 0 obj<</Type/Pages/Kids[]/Count 0>>endobj\n";
    let o1 = header.len();
    let o2 = o1 + obj1.len();
    let body = format!("{header}{obj1}{obj2}");
    let xref_pos = body.len();
    let doc = format!(
        "{body}xref
0 3
0000000000 65535 f 
{o1:010} 00000 n 
{o2:010} 00000 n 
trailer
<</Root 1 0 R/Size 3/XRefStm {o2}>>
startxref
{xref_pos}
%%EOF"
    );

    let loaded = Document::load_mem(doc.as_bytes()).unwrap();
    // Read through the table rather than by reconstructing the cross-reference table.
    assert_eq!(loaded.xref_start, xref_pos);
    assert!(loaded.get_object((2, 0)).is_ok());

    let strict = Document::load_mem_with_options(
        doc.as_bytes(),
        LoadOptions {
            strict: true,
            ..Default::default()
        },
    );
    assert!(strict.is_err());
}

/// Load `buffer` with its objects parsed on the calling thread and again on
/// rayon's pool, whatever the number of cross-reference entries.
#[cfg(all(test, feature = "rayon"))]
fn load_on_both_paths(buffer: &[u8], strict: bool) -> [Result<Document>; 2] {
    [false, true].map(|parallel| {
        let options = LoadOptions {
            strict,
            ..Default::default()
        };
        Reader {
            parallel: Some(parallel),
            ..Reader::with_options(buffer, options)
        }
        .read(None)
    })
}

#[cfg(all(test, feature = "rayon"))]
fn assert_same_document(sequential: &Document, parallel: &Document) {
    assert_eq!(sequential.version, parallel.version);
    assert_eq!(sequential.max_id, parallel.max_id);
    assert_eq!(sequential.trailer, parallel.trailer);
    assert_eq!(sequential.objects, parallel.objects);
}

/// A saved document of `pages` pages, each drawing its own content stream.
#[cfg(all(test, feature = "rayon"))]
fn saved_document(pages: usize, options: crate::SaveOptions) -> Vec<u8> {
    let texts: Vec<String> = (0..pages).map(|page| format!("Page {page}")).collect();
    let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
    let mut buffer = Vec::new();
    crate::creator::tests::create_document_with_texts(&texts)
        .save_with_options(&mut buffer, options)
        .unwrap();
    buffer
}

#[cfg(all(test, feature = "rayon"))]
#[test]
fn sequential_and_parallel_loads_read_the_same_document() {
    // 2 pages load as 10 objects and 300 pages as 606, on either side of
    // MIN_PARALLEL_ENTRIES. With object streams, one stream holds 7 or 305 of
    // them, on either side of MIN_PARALLEL_OBJECTS.
    let object_streams = crate::SaveOptions::builder()
        .use_object_streams(true)
        .use_xref_streams(true)
        .max_objects_per_stream(1000)
        .build();
    for pages in [2, 300] {
        for options in [crate::SaveOptions::default(), object_streams.clone()] {
            let buffer = saved_document(pages, options);
            let [sequential, parallel] = load_on_both_paths(&buffer, true);
            let (sequential, parallel) = (sequential.unwrap(), parallel.unwrap());
            assert_eq!(sequential.get_pages().len(), pages);
            assert_same_document(&sequential, &parallel);
        }
    }
}

#[cfg(all(test, feature = "rayon"))]
#[test]
fn sequential_and_parallel_loads_treat_a_malformed_object_alike() {
    for pages in [2, 300] {
        let mut buffer = saved_document(pages, crate::SaveOptions::default());
        let intact = Document::load_mem(&buffer).unwrap();
        let page = intact.page_iter().next().unwrap();
        let Some(XrefEntry::Normal { offset, .. }) = intact.reference_table.get(page.0) else {
            panic!("page {page:?} has no offset");
        };
        // The object header no longer starts with a number.
        buffer[*offset as usize] = b'x';

        let [sequential, parallel] = load_on_both_paths(&buffer, true);
        let sequential = sequential.map(|_| ()).unwrap_err();
        assert_eq!(
            format!("{sequential:?}"),
            format!("{:?}", parallel.map(|_| ()).unwrap_err())
        );

        let [sequential, parallel] = load_on_both_paths(&buffer, false);
        let (sequential, parallel) = (sequential.unwrap(), parallel.unwrap());
        assert!(!sequential.objects.contains_key(&page));
        assert_eq!(sequential.objects.len(), intact.objects.len() - 1);
        assert_same_document(&sequential, &parallel);
    }
}
