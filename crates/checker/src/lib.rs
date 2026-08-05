#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::cast_possible_truncation)]

pub mod diagnostics;
pub mod linting;
pub mod resolver;
pub mod syntax;
