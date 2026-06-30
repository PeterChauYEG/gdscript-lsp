use std::collections::HashMap;
use std::path::PathBuf;

use gdscript_core::symbol::SymbolDef;

/// Project-wide symbol index.
#[derive(Debug, Default)]
pub struct ProjectIndex {
    /// Maps `class_name` declarations to the file that declares them.
    pub class_names: HashMap<String, PathBuf>,
    /// Maps file paths to their top-level symbol declarations.
    pub file_symbols: HashMap<PathBuf, Vec<SymbolDef>>,
    /// Autoloads from project.godot: name → script path.
    pub autoloads: HashMap<String, PathBuf>,
    /// Godot version extracted from project.godot (e.g. "4.7").
    pub godot_version: Option<String>,
}

impl ProjectIndex {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}
