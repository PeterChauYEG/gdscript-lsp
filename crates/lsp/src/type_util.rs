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
