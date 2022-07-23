#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(clippy::all)]

#[macro_use]
mod object;
mod datetime;
pub use crate::object::{Dictionary, Object, ObjectId, Stream, StringFormat};

mod document;
mod incremental_document;
mod object_stream;
mod xref;
pub use crate::document::Document;
pub use crate::incremental_document::IncrementalDocument;

mod bookmarks;
pub use crate::bookmarks::Bookmark;
pub mod content;
mod creator;
mod encodings;
mod error;
pub mod filters;
#[cfg(not(feature = "nom_parser"))]
#[cfg(feature = "pom_parser")]
mod parser;
#[cfg(feature = "nom_parser")]
#[path = "nom_parser.rs"]
mod parser;
mod parser_aux;
mod processor;
mod reader;
mod writer;
pub mod xobject;
pub use error::{Error, Result};
