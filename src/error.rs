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
    /// Lopdf does not (yet) implement a needed feature.
    #[error("missing feature of lopdf: {0}")]
    Unimplemented(String),
    /// Brackets limit reached.
    /// To many brackets nested.
    // TODO: This does not seem to be used.
    #[error("")]
    BracketLimit,
    /// Could not decode content.
    #[error("")]
    ContentDecode,
    /// Error when decrypting the contents of the file
    #[error("")]
    Decryption(encryption::DecryptionError),
    /// Dictionary key was not found.
    #[error("")]
    DictKey,
    /// Invalid file header
    #[error("")]
    Header,
    /// Invalid command.
    #[error("")]
    Invalid(String),
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
    /// Invalid object while parsing at offset.
    #[error("")]
    Parse { offset: usize },
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

impl From<encryption::DecryptionError> for Error {
    fn from(_err: encryption::DecryptionError) -> Self {
        Error::Decryption(_err)
    }
}
