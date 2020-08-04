use super::parser;
use super::{Object, Stream};
use std::io::Write;
use crate::{Error, Result};
use crate::writer::Writer;

#[derive(Debug, Clone)]
pub struct Operation {
	pub operator: String,
	pub operands: Vec<Object>,
}

impl Operation {
	pub fn new(operator: &str, operands: Vec<Object>) -> Operation {
		Operation {
			operator: operator.to_string(),
			operands,
		}
	}
}

#[derive(Debug, Clone)]
pub struct Content<Operations: AsRef<[Operation]> = Vec<Operation>> {
	pub operations: Operations,
}

impl<Operations: AsRef<[Operation]>> Content<Operations> {
	/// Encode content operations.
	pub fn encode(&self) -> Result<Vec<u8>> {
		let mut buffer = Vec::new();
		for operation in self.operations.as_ref() {
			for operand in &operation.operands {
				Writer::write_object(&mut buffer, operand)?;
				buffer.write_all(b" ")?;
			}
			buffer.write_all(operation.operator.as_bytes())?;
			buffer.write_all(b"\n")?;
		}
		Ok(buffer)
	}
}

impl Content<Vec<Operation>> {
	/// Decode content operations.
	pub fn decode(data: &[u8]) -> Result<Self> {
		parser::content(data).ok_or(Error::ContentDecode)
	}
}

impl Stream {
	/// Decode content after decoding all stream filters.
	pub fn decode_content(&self) -> Result<Content<Vec<Operation>>> {
		Content::decode(&self.content)
	}
}
