#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod backend;
pub mod capabilities;
pub mod completion;
pub mod diagnostics;
pub mod document_store;
pub mod goto_def;
pub mod hover;
pub mod text_util;
