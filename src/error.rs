#[derive(Debug)]
pub enum Error {
	IO(std::io::Error),
	InvalidTrailer,
	InvalidXref,
}

pub type Result<T> = std::result::Result<T, Error>;
