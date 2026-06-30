#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod error;
pub mod parse;
pub mod symbol_table;

pub use parse::ParsedDocument;
