use crate::Object;

/// Type alias for the filter function used during PDF loading.
///
/// The function receives an object ID and a mutable reference to the object,
/// and returns `Some((id, object))` to keep it or `None` to discard it.
pub type FilterFunc = fn((u32, u16), &mut Object) -> Option<((u32, u16), Object)>;

/// The default bound, in bytes, on how large a single stream may decompress to
/// when loading a document or extracting its text: 256 MiB.
///
/// This is far above what real object, cross-reference, and page content streams
/// need, and it keeps a small crafted file from inflating to gigabytes. Pass a
/// larger value, or `None` in [`LoadOptions::max_decompressed_size`], to lift it.
pub const DEFAULT_MAX_DECOMPRESSED_SIZE: usize = 256 * 1024 * 1024;

/// Options for loading PDF documents.
///
/// Use this struct to configure password, object filtering, and strictness
/// when loading a PDF. The default is lenient parsing with no password or filter,
/// and with stream decompression bounded by [`DEFAULT_MAX_DECOMPRESSED_SIZE`].
///
/// # Examples
///
/// ```no_run
/// use lopdf::{Document, LoadOptions};
///
/// // Load with a password
/// let doc = Document::load_with_options(
///     "encrypted.pdf",
///     LoadOptions::with_password("secret"),
/// );
///
/// // Load with strict parsing
/// let doc = Document::load_with_options(
///     "document.pdf",
///     LoadOptions { strict: true, ..Default::default() },
/// );
/// ```
#[derive(Clone)]
pub struct LoadOptions {
    /// Password for encrypted PDFs.
    pub password: Option<String>,
    /// Object filter applied during loading.
    pub filter: Option<FilterFunc>,
    /// When `true`, reject non-conforming PDFs instead of silently accepting them.
    /// Defaults to `false` (lenient parsing), which also repairs damaged cross-reference
    /// data and reports it in [`Document::xref_recovery`](crate::Document::xref_recovery).
    pub strict: bool,
    /// Maximum number of bytes any single stream may decompress to during
    /// loading (object streams and cross-reference streams).
    ///
    /// Compression filters can inflate a tiny input into an enormous output (a
    /// "decompression bomb"). Because object and xref streams are decoded eagerly
    /// while the document is loaded, an unbounded stream can exhaust memory
    /// before any of your code runs. This bounds that per-stream cost; a stream
    /// that would exceed it fails with [`crate::DecompressError::MemoryLimitExceeded`].
    ///
    /// Defaults to [`DEFAULT_MAX_DECOMPRESSED_SIZE`]. `None` applies no limit; use it
    /// only for PDFs from a trusted source.
    pub max_decompressed_size: Option<usize>,
    /// When `true`, rebuild the cross-reference table by scanning the file for indirect
    /// objects instead of reading it as written, as lenient loading does when the
    /// cross-reference data cannot be read. Use it for a file whose cross-reference data
    /// reads but describes the wrong objects. If the scan finds no usable trailer, the
    /// cross-reference data is read as written after all; [`Document::xref_recovery`]
    /// tells the two apart. Strict loading never rebuilds, so `strict` ignores this.
    ///
    /// [`Document::xref_recovery`]: crate::Document::xref_recovery
    pub force_reconstruct: bool,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            password: None,
            filter: None,
            strict: false,
            max_decompressed_size: Some(DEFAULT_MAX_DECOMPRESSED_SIZE),
            force_reconstruct: false,
        }
    }
}

impl std::fmt::Debug for LoadOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadOptions")
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("filter", &self.filter.map(|_| "fn(..)"))
            .field("strict", &self.strict)
            .field("max_decompressed_size", &self.max_decompressed_size)
            .field("force_reconstruct", &self.force_reconstruct)
            .finish()
    }
}

impl LoadOptions {
    /// Create options with a password for encrypted PDFs.
    pub fn with_password(password: &str) -> Self {
        Self {
            password: Some(password.to_string()),
            ..Default::default()
        }
    }

    /// Create options with an object filter.
    pub fn with_filter(filter: FilterFunc) -> Self {
        Self {
            filter: Some(filter),
            ..Default::default()
        }
    }

    /// Create options that bound how large any single stream may decompress to
    /// during loading, to defend against decompression bombs in untrusted PDFs.
    /// See [`LoadOptions::max_decompressed_size`].
    pub fn with_max_decompressed_size(max_decompressed_size: usize) -> Self {
        Self {
            max_decompressed_size: Some(max_decompressed_size),
            ..Default::default()
        }
    }
}
