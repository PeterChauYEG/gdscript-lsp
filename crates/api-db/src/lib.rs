#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::cast_possible_truncation)]

pub mod db;
pub mod error;
pub mod types;

pub use db::ApiDb;

pub const BUNDLED_API: &[u8] = include_bytes!("../data/extension_api.json");
