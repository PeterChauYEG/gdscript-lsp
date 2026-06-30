use gdscript_core::symbol::SymbolDef;

use crate::ParsedDocument;

/// Extract all top-level symbol declarations from a parsed document.
#[must_use]
pub fn extract_symbols(_doc: &ParsedDocument) -> Vec<SymbolDef> {
    // TODO(LAB-654): walk tree-sitter AST for func/var/const/signal/class nodes
    vec![]
}
