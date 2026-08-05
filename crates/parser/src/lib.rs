#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::cast_possible_truncation)]

pub mod error;
pub mod parse;
pub mod symbol_table;

pub use parse::ParsedDocument;
