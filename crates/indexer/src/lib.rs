#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod error;
pub mod index;
pub mod project_godot;
pub mod scene;
pub mod watcher;

pub use index::ProjectIndex;
