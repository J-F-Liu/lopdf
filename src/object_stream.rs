#![cfg(any(feature = "pom_parser", feature = "nom_parser"))]

use crate::parser;
use crate::{Error, Object, ObjectId, Result, Stream};
use std::collections::BTreeMap;
use std::str::FromStr;

use log::warn;
#[cfg(feature = "rayon")]
use rayon::prelude::*;

#[derive(Debug)]
pub struct ObjectStream {
    pub objects: BTreeMap<ObjectId, Object>,
}

impl ObjectStream {
    pub fn new(stream: &mut Stream) -> Result<ObjectStream> {
        stream.decompress();

        if stream.content.is_empty() {
            return Ok(ObjectStream {
                objects: BTreeMap::new(),
            });
        }

        let first_offset = stream
            .dict
            .get(b"First")
            .and_then(Object::as_i64)?
            .try_into()
            .map_err(|_| Error::Offset(0))?;
        let index_block = stream.content.get(..first_offset).ok_or(Error::Offset(first_offset))?;

        let numbers_str = std::str::from_utf8(index_block)?;
        let numbers: Vec<_> = numbers_str
            .split_whitespace()
            .map(|number| u32::from_str(number).ok())
            .collect();
        let len = numbers.len() / 2 * 2; // Ensure only pairs.

        let n = stream.dict.get(b"N").and_then(Object::as_i64)?;
        if numbers.len().try_into().ok() != n.checked_mul(2) {
            warn!("object stream: the object stream dictionary specifies a wrong number of objects")
        }

        let chunks_filter_map = |chunk: &[_]| {
            let id = chunk[0]?;
            let offset = first_offset + chunk[1]? as usize;

            if offset >= stream.content.len() {
                warn!("out-of-bounds offset in object stream");
                return None;
            }
            let object = parser::direct_object(&stream.content[offset..])?;

            Some(((id, 0), object))
        };
        #[cfg(feature = "rayon")]
        let objects = numbers[..len].par_chunks(2).filter_map(chunks_filter_map).collect();
        #[cfg(not(feature = "rayon"))]
        let objects = numbers[..len].chunks(2).filter_map(chunks_filter_map).collect();

        Ok(ObjectStream { objects })
    }
}
