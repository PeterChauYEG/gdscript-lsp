use gdscript_core::symbol::{SymbolDef, SymbolKind};

use crate::ParsedDocument;

/// Extract all top-level symbol declarations from a parsed document.
#[must_use]
pub fn extract_symbols(doc: &ParsedDocument) -> Vec<SymbolDef> {
    let source = doc.source.as_bytes();
    let root = doc.tree.root_node();
    let mut symbols = Vec::new();

    for i in 0..root.child_count() {
        let node = root.child(i).unwrap();
        let kind = match node.kind() {
            "function_definition" => Some(SymbolKind::Function),
            "variable_statement" => Some(SymbolKind::Variable),
            "const_statement" => Some(SymbolKind::Constant),
            "signal_statement" => Some(SymbolKind::Signal),
            "class_definition" => Some(SymbolKind::Class),
            "enum_definition" => Some(SymbolKind::Enum),
            _ => None,
        };

        let Some(kind) = kind else { continue };

        let Some(name_node) = node.child_by_field_name("name") else {
            continue;
        };
        let Ok(name) = name_node.utf8_text(source) else {
            continue;
        };

        let pos = name_node.start_position();
        symbols.push(SymbolDef {
            name: name.to_owned(),
            kind,
            line: pos.row as u32,
            col: pos.column as u32,
            type_annotation: None,
        });
    }

    symbols
}
