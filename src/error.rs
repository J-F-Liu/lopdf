use std::fmt;

#[derive(Debug)]
pub enum Error {
	IO(std::io::Error),
	Header,
	Trailer,
	Xref(XrefError),
	Offset(usize),
	Parse {offset: usize},
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Error::IO(e) => e.fmt(f),
			Error::Header => write!(f, "Invalid file header"),
			Error::Trailer => write!(f, "Invalid file trailer"),
			Error::Xref(e) => write!(f, "Invalid cross-reference table ({})", e),
			Error::Offset(o) => write!(f, "Invalid file offset: {}", o),
			Error::Parse{offset, ..} => write!(f, "Invalid object at byte {}", offset),
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
	fn from(err: std::io::Error) -> Error {
		Error::IO(err)
	}
}
