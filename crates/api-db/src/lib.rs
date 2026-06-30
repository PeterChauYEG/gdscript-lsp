#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod db;
pub mod error;
pub mod types;

pub use db::ApiDb;
