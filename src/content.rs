use super::{Object, Stream};
use crate::Result;
pub use crate::parser::ContentOperations;
use crate::writer::Writer;
use std::io::Write;

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
        let mut first_operation = true;
        for operation in self.operations.as_ref() {
            // Add new line after each operation except the last one.
            if first_operation {
                first_operation = false;
            } else {
                buffer.write_all(b"\n")?;
            }
            // A decoded inline image is a BI operation holding the image as a stream.
            if let ("BI", [Object::Stream(image)]) = (operation.operator.as_str(), operation.operands.as_slice()) {
                encode_inline_image(&mut buffer, image)?;
                continue;
            }
            for operand in &operation.operands {
                Writer::write_object(&mut buffer, operand)?;
                buffer.write_all(b" ")?;
            }
            buffer.write_all(operation.operator.as_bytes())?;
        }
        Ok(buffer)
    }
}

/// Write an inline image as `BI`, its dictionary entries, `ID`, its data and `EI`.
fn encode_inline_image(buffer: &mut Vec<u8>, image: &Stream) -> Result<()> {
    buffer.write_all(b"BI")?;
    for (key, value) in &image.dict {
        // Readers find the end of the data from the image size, and `Stream::new` adds a
        // Length the source dictionary did not have.
        if key == b"Length" {
            continue;
        }
        buffer.write_all(b" ")?;
        Writer::write_name(buffer, key)?;
        buffer.write_all(b" ")?;
        Writer::write_object(buffer, value)?;
    }
    buffer.write_all(b" ID ")?;
    buffer.write_all(&image.content)?;
    buffer.write_all(b"\nEI")?;
    Ok(())
}
