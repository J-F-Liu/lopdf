use crate::encryption;
use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// Brackets limit reached.
    /// To many brackets nested.
    // TODO: This does not seem to be used.
    BracketLimit,
    /// Could not decode content.
    ContentDecode,
    /// Error when decrypting the contents of the file
    Decryption(encryption::DecryptionError),
    /// Dictionary key was not found.
    DictKey,
    /// Invalid file header
    Header,
    /// Invalid command.
    Invalid(String),
    /// IO error
    IO(std::io::Error),
    /// PDF document has no Outlines.
    NoOutlines,
    /// Found Object ID does not match Expected Object ID.
    ObjectIdMismatch,
    /// The Object ID was not found.
    ObjectNotFound,
    /// Offset in file is invalid.
    Offset(usize),
    /// Page number was not found in document.
    PageNumberNotFound(u32),
    /// Invalid object while parsing at offset.
    Parse { offset: usize },
    /// Dereferencing object failed due to a reference cycle.
    ReferenceCycle,
    /// Dereferencing object reached the limit.
    /// This might indicate a reference loop.
    ReferenceLimit,
    /// Decoding byte vector failed.
    StringDecode,
    /// Syntax error while parsing the file.
    Syntax(String),
    /// The file trailer was invalid.
    Trailer,
    /// The object does not have the expected type.
    Type,
    /// Decoding byte vector to UTF8 String failed.
    UTF8,
    /// Error while parsing cross reference table.
    Xref(XrefError),
    /// Error when handling images.
    #[cfg(feature = "embed_image")]
    Image(image::ImageError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::BracketLimit => write!(f, "Too deep embedding of ()'s."),
            Error::ContentDecode => write!(f, "Could not decode content"),
            Error::Decryption(d) => d.fmt(f),
            Error::DictKey => write!(f, "A required dictionary key was not found"),
            Error::Header => write!(f, "Invalid file header"),
            Error::Invalid(msg) => write!(f, "Invalid command: {}", msg),
            Error::IO(e) => e.fmt(f),
            Error::NoOutlines => write!(f, "PDF document has no Outlines"),
            Error::ObjectIdMismatch => write!(f, "The object id found did not match the requested object"),
            Error::ObjectNotFound => write!(f, "A required object was not found"),
            Error::Offset(o) => write!(f, "Invalid file offset: {}", o),
            Error::PageNumberNotFound(p) => write!(f, "Page number {} could not be found", p),
            Error::Parse { offset, .. } => write!(f, "Invalid object at byte {}", offset),
            Error::ReferenceCycle => write!(f, "Could not dereference an object; reference cycle detected"),
            Error::ReferenceLimit => write!(f, "Could not dereference an object; possible reference cycle"),
            Error::StringDecode => write!(f, "Could not decode string"),
            Error::Syntax(msg) => write!(f, "Syntax error: {}", msg),
            Error::Trailer => write!(f, "Invalid file trailer"),
            Error::Type => write!(f, "An object does not have the expected type"),
            Error::UTF8 => write!(f, "UTF-8 error"),
            Error::Xref(e) => write!(f, "Invalid cross-reference table ({})", e),
            #[cfg(feature = "embed_image")]
            Error::Image(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

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

pub type Result<T> = std::result::Result<T, Error>;

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
