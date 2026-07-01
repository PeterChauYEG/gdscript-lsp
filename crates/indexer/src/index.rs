use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gdscript_core::symbol::SymbolDef;

use crate::error::IndexerError;

/// Project-wide symbol index.
#[derive(Debug, Default)]
pub struct ProjectIndex {
    /// Maps `class_name` declarations to the file that declares them.
    pub class_names: HashMap<String, PathBuf>,
    /// Maps file paths to their top-level symbol declarations.
    pub file_symbols: HashMap<PathBuf, Vec<SymbolDef>>,
    /// Autoloads from project.godot: singleton name → absolute script path.
    pub autoloads: HashMap<String, PathBuf>,
    /// Godot version extracted from project.godot (e.g. "4.3").
    pub godot_version: Option<String>,
    /// Scene node maps: scene path → (node name → Godot class).
    pub scenes: HashMap<PathBuf, crate::scene::SceneNodeMap>,
}

impl ProjectIndex {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Extract the `class_name` declared in a GDScript source, if any.
#[must_use]
pub fn extract_class_name(source: &str) -> Option<String> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_gdscript::LANGUAGE.into()).ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();
    let bytes = source.as_bytes();

    for i in 0..root.child_count() {
        let Some(node) = root.child(i) else { continue };
        if node.kind() == "class_name_statement" {
            let name = node.child_by_field_name("name")?;
            return Some(name.utf8_text(bytes).ok()?.to_owned());
        }
    }
    None
}

/// Walk `root` recursively, index all `.gd` and `.tscn` files, parse
/// `project.godot`, and return a populated [`ProjectIndex`].
///
/// # Errors
///
/// Returns [`IndexerError::Io`] if the directory cannot be read.
pub fn index_workspace(root: &Path) -> Result<ProjectIndex, IndexerError> {
    let mut index = ProjectIndex::new();

    // Parse project.godot for autoloads and version.
    if let Some(project_file) = crate::project_godot::find(root) {
        if let Ok(content) = std::fs::read_to_string(&project_file) {
            let config = crate::project_godot::parse(&content);
            index.godot_version = config.godot_version;

            for (name, res_path) in config.autoloads {
                let abs = res_to_abs(&res_path, root);
                index.autoloads.insert(name, abs);
            }
        }
    }

    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("");

        match ext {
            "gd" => {
                let Ok(source) = std::fs::read_to_string(path) else { continue };

                if let Some(class_name) = extract_class_name(&source) {
                    index.class_names.insert(class_name, path.to_path_buf());
                }

                if let Ok(doc) = gdscript_parser::parse::parse(&source) {
                    let symbols = gdscript_parser::symbol_table::extract_symbols(&doc);
                    index.file_symbols.insert(path.to_path_buf(), symbols);
                }
            }
            "tscn" => {
                let Ok(content) = std::fs::read_to_string(path) else { continue };
                let node_map = crate::scene::parse(&content);
                if !node_map.is_empty() {
                    index.scenes.insert(path.to_path_buf(), node_map);
                }
            }
            _ => {}
        }
    }

    Ok(index)
}

/// Convert a `res://` path to an absolute filesystem path.
fn res_to_abs(res_path: &str, root: &Path) -> PathBuf {
    let rel = res_path.trim_start_matches("res://");
    root.join(rel)
}
