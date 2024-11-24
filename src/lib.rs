#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(clippy::all)]

pub mod content;
pub mod encryption;
pub mod filters;
pub mod xobject;
pub mod xref;

#[macro_use]
mod object;
mod document;
mod incremental_document;

mod bookmarks;
mod cmap_section;
mod common_data_structures;
mod creator;
mod datetime;
mod destinations;
mod encodings;
mod error;
mod outlines;
mod processor;
mod rc4;
mod toc;
mod writer;

mod object_stream;
mod parser;
mod parser_aux;
mod reader;

pub use document::Document;
pub use object::{Dictionary, Object, ObjectId, Stream, StringFormat};

pub use bookmarks::Bookmark;
pub use common_data_structures::{decode_text_string, text_string};
pub use destinations::Destination;
pub use encodings::{encode_utf16_be, encode_utf8, Encoding};
pub use error::{Error, Result, XrefError};
pub use incremental_document::IncrementalDocument;
pub use object_stream::ObjectStream;
pub use outlines::Outline;
#[cfg(feature = "nom_parser")]
pub use reader::Reader;
pub use toc::Toc;
