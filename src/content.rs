use super::{Object, Stream};
use super::parser;
use pom::{DataInput, Result};

#[derive(Debug, Clone)]
pub struct Operation {
	pub operator: String,
	pub operands: Vec<Object>,
}

#[derive(Debug, Clone)]
pub struct Content {
	pub operations: Vec<Operation>,
}

impl Stream {
	pub fn decode_content(&self) -> Result<Content> {
		let mut input = DataInput::new(&self.content);
		parser::content().parse(&mut input)
	}
}
