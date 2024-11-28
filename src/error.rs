use thiserror::Error;

use crate::encryption;
use std::fmt;

use crate::encodings::cmap::UnicodeCMapError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    /// An Object has the wrong type, e.g. the Object is an Array where a Name would be expected.
    #[error("object has wrong type; expected type {expected} but found type {found}")]
    ObjectType {
        expected: &'static str,
        found: &'static str,
    },
    #[error("dictionary has wrong type: ")]
    DictType { expected: &'static str, found: String },
    /// Lopdf does not (yet) implement a needed feature.
    #[error("missing feature of lopdf: {0}. Please open an issue at https://github.com/J-F-Liu/lopdf/ to let the developers know of your usecase")]
    Unimplemented(&'static str),
    /// The encountered character encoding is invalid.
    #[error("invalid character encoding")]
    CharacterEncoding,
    /// The stream couldn't be decompressed.
    #[error("couldn't decompress stream {0}")]
    Decompress(#[from] DecompressError),
    /// Failed to parse input.
    #[error("couldn't parse input: {0}")]
    Parse(#[from] ParseError),
    /// Failed to parse content stream.
    #[error("couldn't parse content stream")]
    ContentStream,
    /// Error when decrypting the contents of the file
    #[error("decryption error: {0}")]
    Decryption(#[from] encryption::DecryptionError),
    /// Dictionary key was not found.
    #[error("missing required dictionary key \"{0}\"")]
    DictKey(String),
    /// Invalid inline image.
    #[error("invalid inline image: {0}")]
    InvalidInlineImage(String),
    /// Invalid document outline.
    #[error("invalid document outline: {0}")]
    InvalidOutline(String),

    /// Invalid object while parsing at offset.
    #[error("")]
    OldParse { offset: usize },
    /// IO error
    #[error("")]
    IO(std::io::Error),
    /// PDF document has no Outlines.
    #[error("")]
    NoOutlines,
    /// Found Object ID does not match Expected Object ID.
    #[error("")]
    ObjectIdMismatch,
    /// The Object ID was not found.
    #[error("")]
    ObjectNotFound,
    /// Offset in file is invalid.
    #[error("")]
    Offset(usize),
    /// Page number was not found in document.
    #[error("")]
    PageNumberNotFound(u32),
    /// Dereferencing object failed due to a reference cycle.
    #[error("")]
    ReferenceCycle,
    /// Dereferencing object reached the limit.
    /// This might indicate a reference loop.
    #[error("")]
    ReferenceLimit,
    /// Decoding byte vector failed.
    #[error("")]
    StringDecode,
    /// Syntax error while parsing the file.
    #[error("")]
    Syntax(String),
    /// Could not parse ToUnicodeCMap.
    #[error("")]
    ToUnicodeCMap(UnicodeCMapError),
    /// The file trailer was invalid.
    #[error("")]
    Trailer,
    /// Decoding byte vector to UTF8 String failed.
    #[error("")]
    UTF8,
    /// Error while parsing cross reference table.
    #[error("")]
    Xref(XrefError),
    /// Error when handling images.
    #[cfg(feature = "embed_image")]
    #[error("")]
    Image(image::ImageError),
}

#[derive(Error, Debug)]
pub enum DecompressError {
    #[error("decoding ASCII85 failed: {0}")]
    Ascii85(&'static str),
}

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("unexpected end of input")]
    EndOfInput,
    #[error("invalid file header")]
    InvalidFileHeader,
}

// impl fmt::Display for PDFError {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         match self {
//             PDFError::BracketLimit => write!(f, "Too deep embedding of ()'s."),
//             PDFError::ContentDecode => write!(f, "Could not decode content"),
//             PDFError::Decryption(d) => d.fmt(f),
//             PDFError::DictKey => write!(f, "A required dictionary key was not found"),
//             PDFError::Header => write!(f, "Invalid file header"),
//             PDFError::Invalid(msg) => write!(f, "Invalid command: {}", msg),
//             PDFError::IO(e) => e.fmt(f),
//             PDFError::NoOutlines => write!(f, "PDF document has no Outlines"),
//             PDFError::ObjectIdMismatch => write!(f, "The object id found did not match the requested object"),
//             PDFError::ObjectNotFound => write!(f, "A required object was not found"),
//             PDFError::Offset(o) => write!(f, "Invalid file offset: {}", o),
//             PDFError::PageNumberNotFound(p) => write!(f, "Page number {} could not be found", p),
//             PDFError::Parse { offset, .. } => write!(f, "Invalid object at byte {}", offset),
//             PDFError::ReferenceCycle => write!(f, "Could not dereference an object; reference cycle detected"),
//             PDFError::ReferenceLimit => write!(f, "Could not dereference an object; possible reference cycle"),
//             PDFError::StringDecode => write!(f, "Could not decode string"),
//             PDFError::Syntax(msg) => write!(f, "Syntax error: {}", msg),
//             PDFError::ToUnicodeCMap(err) => write!(f, "ToUnicode CMap error: {}", err),
//             PDFError::Trailer => write!(f, "Invalid file trailer"),
//             PDFError::Type => write!(f, "An object does not have the expected type"),
//             PDFError::UTF8 => write!(f, "UTF-8 error"),
//             PDFError::Xref(e) => write!(f, "Invalid cross-reference table ({})", e),
//             #[cfg(feature = "embed_image")]
//             PDFError::Image(e) => e.fmt(f),
//             _ => unimplemented!(),
//         }
//     }
// }

// impl std::error::Error for PDFError {}

#[derive(Debug)]
pub enum XrefError {
    /// Could not parse cross reference table.
    Parse,
    /// Could not find start of cross reference table.
    Start,
    /// The trailer's "Prev" field was invalid.
    PrevStart,
    /// The trailer's "XRefStm" field was invalid.
    StreamStart,
}

impl fmt::Display for XrefError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            XrefError::Parse => write!(f, "could not parse xref"),
            XrefError::Start => write!(f, "invalid start value"),
            XrefError::PrevStart => write!(f, "invalid start value in Prev field"),
            XrefError::StreamStart => write!(f, "invalid stream start value"),
        }
    }
}

impl std::error::Error for XrefError {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::IO(err)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(_err: std::string::FromUtf8Error) -> Self {
        Error::UTF8
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(_err: std::str::Utf8Error) -> Self {
        Error::UTF8
    }
}

impl From<UnicodeCMapError> for Error {
    fn from(cmap_err: UnicodeCMapError) -> Self {
        Error::ToUnicodeCMap(cmap_err)
    }
}

#[cfg(feature = "embed_image")]
impl From<image::ImageError> for Error {
    fn from(err: image::ImageError) -> Self {
        Error::Image(err)
    }
}
