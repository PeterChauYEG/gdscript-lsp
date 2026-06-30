#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod db;
pub mod error;
pub mod types;

pub use db::ApiDb;

/// The `extension_api.json` bundled at compile time for Godot 4.7.
pub const BUNDLED_API: &[u8] = include_bytes!("../data/extension_api.json");
