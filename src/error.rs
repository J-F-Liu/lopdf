#[derive(Debug)]
pub enum Error {
	IO(std::io::Error),
	Header,
	Trailer,
	Xref(XrefError),
	Offset(usize),
	Parse {offset: usize},
}

#[derive(Debug)]
pub enum XrefError {
	Parse,
	Start,
	PrevStart,
	StreamStart,
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<std::io::Error> for Error {
	fn from(err: std::io::Error) -> Error {
		Error::IO(err)
	}
}
