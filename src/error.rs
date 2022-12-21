use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// Could not decode content.
    ContentDecode,
    /// Dictionary key was not found.
    DictKey,
    /// Invalid file header
    Header,
    /// IO error
    IO(std::io::Error),
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
    /// Dereferencing object reached the limit.
    /// This might indicate a reference loop.
    ReferenceLimit,
    /// Brackets limit reached.
    /// To many brackets nested.
    // TODO: This does not seem to be used.
    BracketLimit,
    /// The file trailer was invalid.
    Trailer,
    /// The object does not have the expected type.
    Type,
    /// Decoding byte vector to UTF8 String failed.
    UTF8,
    /// Syntax error while parsing the file.
    Syntax(String),
    /// Error while parsing cross reference table.
    Xref(XrefError),
    /// Invalid command.
    Invalid(String),
    /// PDF document has no Outlines.
    NoOutlines,
    /// Error when handling images.
    #[cfg(feature = "embed_image")]
    Image(image::ImageError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::ContentDecode => write!(f, "Could not decode content"),
            Error::DictKey => write!(f, "A required dictionary key was not found"),
            Error::Header => write!(f, "Invalid file header"),
            Error::IO(e) => e.fmt(f),
            Error::ObjectIdMismatch => write!(f, "The object id found did not match the requested object"),
            Error::ObjectNotFound => write!(f, "A required object was not found"),
            Error::Offset(o) => write!(f, "Invalid file offset: {}", o),
            Error::PageNumberNotFound(p) => write!(f, "Page number {} could not be found", p),
            Error::Parse { offset, .. } => write!(f, "Invalid object at byte {}", offset),
            Error::ReferenceLimit => write!(f, "Could not dereference an object; possible reference loop"),
            Error::BracketLimit => write!(f, "Too deep embedding of ()'s."),
            Error::Trailer => write!(f, "Invalid file trailer"),
            Error::Type => write!(f, "An object does not have the expected type"),
            Error::UTF8 => write!(f, "UTF-8 error"),
            Error::Syntax(msg) => write!(f, "Syntax error: {}", msg),
            Error::Xref(e) => write!(f, "Invalid cross-reference table ({})", e),
            Error::Invalid(msg) => write!(f, "Invalid command: {}", msg),
            Error::NoOutlines => write!(f, "PDF document has no Outlines"),
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
