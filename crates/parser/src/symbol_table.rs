use gdscript_core::symbol::{SymbolDef, SymbolKind};
use tree_sitter::Node;

use crate::ParsedDocument;

/// Extract all top-level symbol declarations from a parsed document,
/// including members of inner classes with qualified names (e.g. `MyInner.do_something`).
#[must_use]
pub fn extract_symbols(doc: &ParsedDocument) -> Vec<SymbolDef> {
    let source = doc.source.as_bytes();
    let root = doc.tree.root_node();
    let mut symbols = Vec::new();
    collect_symbols(source, root, None, &mut symbols);
    symbols
}

/// Recursively collect symbols from `parent`'s children.
///
/// `class_prefix` is `Some("ClassName")` when collecting inside an inner class body,
/// causing member names to be qualified as `ClassName.member`.
fn collect_symbols<'tree>(
    source: &[u8],
    parent: Node<'tree>,
    class_prefix: Option<&str>,
    symbols: &mut Vec<SymbolDef>,
) {
    for i in 0..parent.child_count() {
        let node = parent.child(i).unwrap();
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
        let Ok(raw_name) = name_node.utf8_text(source) else {
            continue;
        };

        let qualified_name = match class_prefix {
            Some(prefix) => format!("{prefix}.{raw_name}"),
            None => raw_name.to_owned(),
        };

        let pos = name_node.start_position();
        symbols.push(SymbolDef {
            name: qualified_name.clone(),
            kind: kind.clone(),
            line: pos.row as u32,
            col: pos.column as u32,
            type_annotation: None,
        });

        // For class definitions, recurse into the body to extract member symbols.
        if matches!(kind, SymbolKind::Class) {
            if let Some(body) = node.child_by_field_name("body") {
                collect_symbols(source, body, Some(&qualified_name), symbols);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    #[test]
    fn extracts_signals_vars_and_funcs() {
        let src = "class_name EventBusManager\nextends Node\nsignal game_start\nvar score: int = 0\nfunc _ready() -> void:\n\tpass\n";
        let doc = parse(src).unwrap();
        let syms = extract_symbols(&doc);
        assert!(syms.iter().any(|s| s.name == "game_start"));
        assert!(syms.iter().any(|s| s.name == "score"));
        assert!(syms.iter().any(|s| s.name == "_ready"));
    }

    #[test]
    fn signal_extracted_as_signal_kind() {
        let src = "signal my_signal(arg1: int)\n";
        let doc = parse(src).unwrap();
        let syms = extract_symbols(&doc);
        let sig = syms.iter().find(|s| s.name == "my_signal").expect("signal not found");
        assert_eq!(sig.kind, SymbolKind::Signal);
    }

    #[test]
    fn inner_class_members_extracted_with_qualified_names() {
        let src = "\
class MyInner:
\tvar x: int = 0
\tfunc do_something():
\t\tpass
func top_level():
\tpass
";
        let doc = parse(src).unwrap();
        let syms = extract_symbols(&doc);

        // The inner class itself should appear
        assert!(syms.iter().any(|s| s.name == "MyInner" && s.kind == SymbolKind::Class));

        // Inner class members should appear with qualified names
        assert!(syms.iter().any(|s| s.name == "MyInner.x" && s.kind == SymbolKind::Variable));
        assert!(syms.iter().any(|s| s.name == "MyInner.do_something" && s.kind == SymbolKind::Function));

        // Top-level function should still appear unqualified
        assert!(syms.iter().any(|s| s.name == "top_level" && s.kind == SymbolKind::Function));
    }

    #[test]
    fn nested_inner_classes_extracted() {
        let src = "\
class Outer:
\tvar outer_var: int = 0
\tclass Inner:
\t\tfunc inner_func():
\t\t\tpass
";
        let doc = parse(src).unwrap();
        let syms = extract_symbols(&doc);

        assert!(syms.iter().any(|s| s.name == "Outer" && s.kind == SymbolKind::Class));
        assert!(syms.iter().any(|s| s.name == "Outer.outer_var" && s.kind == SymbolKind::Variable));
        assert!(syms.iter().any(|s| s.name == "Outer.Inner" && s.kind == SymbolKind::Class));
        assert!(syms.iter().any(|s| s.name == "Outer.Inner.inner_func" && s.kind == SymbolKind::Function));
    }
}
