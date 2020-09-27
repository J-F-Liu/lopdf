use std::fmt;

#[derive(Debug)]
pub enum Error {
    ContentDecode,
    DictKey,
    Header,
    IO(std::io::Error),
    ObjectIdMismatch,
    ObjectNotFound,
    Offset(usize),
    PageNumberNotFound(u32),
    Parse {
        offset: usize,
    },
    ReferenceLimit,
    BracketLimit,
    Trailer,
    Type,
    UTF8,
    Syntax(String),
    Xref(XrefError),
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
            #[cfg(feature = "embed_image")]
            Error::Image(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug)]
pub enum XrefError {
    Parse,
    Start,
    PrevStart,
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
