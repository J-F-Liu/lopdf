use pom::{DataInput, Result};
use std::io::{self, Write};
use super::{Object, Stream};
use super::parser;
use writer::Writer;

#[derive(Debug, Clone)]
pub struct Operation {
	pub operator: String,
	pub operands: Vec<Object>,
}

impl Operation {
	pub fn new(operator: &str, operands: Vec<Object>) -> Operation {
		Operation{
			operator: operator.to_string(),
			operands: operands,
		}
	}
}

#[derive(Debug, Clone)]
pub struct Content {
	pub operations: Vec<Operation>,
}

impl Content {
	/// Encode content operations.
	pub fn encode(&self) -> io::Result<Vec<u8>> {
		let mut buffer = Vec::new();
		for operation in &self.operations {
			for operand in &operation.operands {
				Writer::write_object(&mut buffer, operand)?;
				buffer.write_all(b" ")?;
			}
			buffer.write_all(operation.operator.as_bytes())?;
			buffer.write_all(b"\n")?;
		}
		Ok(buffer)
	}

	/// Decode content operations.
	pub fn decode(data: &[u8]) -> Result<Content> {
		let mut input = DataInput::new(data);
		parser::content().parse(&mut input)
	}
}

impl Stream {
	/// Decode content after decoding all stream filters.
	pub fn decode_content(&self) -> Result<Content> {
		Content::decode(&self.content)
	}
}
