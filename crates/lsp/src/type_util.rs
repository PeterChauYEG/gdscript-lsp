use gdscript_api_db::ApiDb;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

/// Infer the GDScript type name of a literal AST node.
#[must_use]
pub fn infer_literal_type(node: &tree_sitter::Node) -> Option<&'static str> {
    match node.kind() {
        "integer" => Some("int"),
        "float" => Some("float"),
        "string" | "string_name" => Some("String"),
        "true" | "false" => Some("bool"),
        "null" => Some("Nil"),
        _ => None,
    }
}

/// Return true if `actual` is assignable to `expected`.
#[must_use]
pub fn types_compatible(expected: &str, actual: &str, api_db: &ApiDb) -> bool {
    if expected == actual {
        return true;
    }
    if expected == "float" && actual == "int" {
        return true;
    }
    if expected == "Variant" {
        return true;
    }
    if api_db.get_class(expected).is_some() && api_db.get_class(actual).is_some() {
        return api_db.is_subclass(actual, expected);
    }
    false
}

#[must_use]
pub fn node_range(node: &tree_sitter::Node) -> Range {
    let start = node.start_position();
    let end = node.end_position();
    Range {
        start: Position { line: start.row as u32, character: start.column as u32 },
        end: Position { line: end.row as u32, character: end.column as u32 },
    }
}

pub fn error_diag(range: Range, code: &str, message: String) -> Diagnostic {
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(code.to_owned())),
        source: Some("gdscript-lsp".to_owned()),
        message,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use gdscript_api_db::ApiDb;
    use gdscript_parser::parse::parse;

    use super::*;

    fn db() -> ApiDb { ApiDb::bundled().unwrap() }

    fn literal_type(src: &str) -> Option<&'static str> {
        let doc = parse(src).unwrap();
        let root = doc.tree.root_node();
        // The variable_statement's value is under the RHS — find it
        for i in 0..root.child_count() {
            let Some(stmt) = root.child(i) else { continue };
            if stmt.kind() != "variable_statement" { continue }
            let mut after_eq = false;
            for j in 0..stmt.child_count() {
                let Some(child) = stmt.child(j) else { continue };
                if child.kind() == "=" { after_eq = true; }
                else if after_eq && child.is_named() { return infer_literal_type(&child); }
            }
        }
        None
    }

    #[test]
    fn int_literal() { assert_eq!(literal_type("var x = 5\n"), Some("int")); }

    #[test]
    fn float_literal() { assert_eq!(literal_type("var x = 3.14\n"), Some("float")); }

    #[test]
    fn string_literal() { assert_eq!(literal_type("var x = \"hi\"\n"), Some("String")); }

    #[test]
    fn bool_true() { assert_eq!(literal_type("var x = true\n"), Some("bool")); }

    #[test]
    fn bool_false() { assert_eq!(literal_type("var x = false\n"), Some("bool")); }

    #[test]
    fn null_literal() { assert_eq!(literal_type("var x = null\n"), Some("Nil")); }

    #[test]
    fn expression_not_literal() { assert_eq!(literal_type("var x = foo()\n"), None); }

    // types_compatible
    #[test]
    fn same_type_compatible() { let d = db(); assert!(types_compatible("int", "int", &d)); }

    #[test]
    fn int_to_float() { let d = db(); assert!(types_compatible("float", "int", &d)); }

    #[test]
    fn variant_accepts_string() { let d = db(); assert!(types_compatible("Variant", "String", &d)); }

    #[test]
    fn string_to_int_incompatible() { let d = db(); assert!(!types_compatible("int", "String", &d)); }

    #[test]
    fn subclass_compatible() {
        let d = db();
        // Node2D inherits Node — Node2D is a subclass of Node
        assert!(types_compatible("Node", "Node2D", &d));
    }
}
