#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod backend;
pub mod call_checker;
pub mod capabilities;
pub mod code_actions;
pub mod completion;
pub mod diagnostics;
pub mod document_store;
pub mod formatting;
pub mod goto_def;
pub mod hover;
pub mod inlay_hints;
pub mod signature_help;
pub mod text_util;
pub mod type_check;
pub mod type_resolver;
pub mod type_util;
