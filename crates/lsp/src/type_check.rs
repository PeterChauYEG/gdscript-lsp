use gdscript_api_db::ApiDb;
use gdscript_parser::ParsedDocument;
use tower_lsp::lsp_types::Diagnostic;

use crate::type_util::{error_diag, infer_literal_type, node_range, types_compatible};

/// Check type mismatches on variable assignments and return statements.
///
/// Only checks cases where both sides are statically known — declared type annotation
/// on the left and a literal on the right. Expressions/variables are skipped.
#[must_use]
pub fn check_type_mismatches(doc: &ParsedDocument, api_db: &ApiDb) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let source = doc.source.as_bytes();
    let root = doc.tree.root_node();

    for i in 0..root.child_count() {
        let Some(node) = root.child(i) else { continue };
        match node.kind() {
            "variable_statement" => {
                check_var_assignment(&node, source, api_db, &mut out);
            }
            "function_definition" => {
                check_function(&node, source, api_db, &mut out);
            }
            _ => {}
        }
    }

    out
}

/// Check `var x: Type = literal` for type mismatches.
fn check_var_assignment(
    stmt: &tree_sitter::Node,
    source: &[u8],
    api_db: &ApiDb,
    out: &mut Vec<Diagnostic>,
) {
    let declared = match declared_var_type(stmt, source) {
        Some(t) => t,
        None => return,
    };

    let value = match rhs_value(stmt) {
        Some(v) => v,
        None => return,
    };

    let inferred = match infer_literal_type(&value) {
        Some(t) => t,
        None => return,
    };

    if !types_compatible(declared, inferred, api_db) {
        out.push(error_diag(
            node_range(&value),
            "E0003",
            format!("Cannot assign `{inferred}` to variable of type `{declared}`"),
        ));
    }
}

/// Check return statements inside a function for type mismatches.
fn check_function(
    func: &tree_sitter::Node,
    source: &[u8],
    api_db: &ApiDb,
    out: &mut Vec<Diagnostic>,
) {
    let ret_type = match function_return_type(func, source) {
        Some(t) if t != "void" => t,
        _ => return,
    };

    let body = match (0..func.child_count())
        .filter_map(|i| func.child(i))
        .find(|n| n.kind() == "body")
    {
        Some(b) => b,
        None => return,
    };

    check_returns_in_body(&body, ret_type, source, api_db, out);
}

fn check_returns_in_body(
    body: &tree_sitter::Node,
    ret_type: &str,
    source: &[u8],
    api_db: &ApiDb,
    out: &mut Vec<Diagnostic>,
) {
    for i in 0..body.child_count() {
        let Some(stmt) = body.child(i) else { continue };
        match stmt.kind() {
            "return_statement" => {
                check_return_value(&stmt, ret_type, source, api_db, out);
            }
            "if_statement" | "while_statement" | "for_statement" | "match_statement" => {
                // Recurse into nested bodies
                for j in 0..stmt.child_count() {
                    let Some(child) = stmt.child(j) else { continue };
                    if child.kind() == "body" {
                        check_returns_in_body(&child, ret_type, source, api_db, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn check_return_value(
    ret_stmt: &tree_sitter::Node,
    ret_type: &str,
    _source: &[u8],
    api_db: &ApiDb,
    out: &mut Vec<Diagnostic>,
) {
    // The return value is the first named child of the return statement.
    let value = (0..ret_stmt.child_count())
        .filter_map(|i| ret_stmt.child(i))
        .find(|n| n.is_named() && n.kind() != "return");

    let Some(value) = value else { return };
    let Some(inferred) = infer_literal_type(&value) else { return };

    if !types_compatible(ret_type, inferred, api_db) {
        out.push(error_diag(
            node_range(&value),
            "E0003",
            format!("Return type mismatch: expected `{ret_type}`, got `{inferred}`"),
        ));
    }
}

/// Get the declared type name from a `variable_statement`'s `: Type` annotation.
fn declared_var_type<'a>(stmt: &tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    let type_node = (0..stmt.child_count())
        .filter_map(|i| stmt.child(i))
        .find(|n| n.kind() == "type")?;
    first_named_text(&type_node, source)
}

/// Get the RHS value node of a `variable_statement` (the node after `=`).
fn rhs_value<'a>(stmt: &'a tree_sitter::Node) -> Option<tree_sitter::Node<'a>> {
    let mut after_eq = false;
    for i in 0..stmt.child_count() {
        let Some(child) = stmt.child(i) else { continue };
        if child.kind() == "=" {
            after_eq = true;
        } else if after_eq && child.is_named() {
            return Some(child);
        }
    }
    None
}

/// Get the declared return type of a `function_definition`.
fn function_return_type<'a>(func: &tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    let mut after_arrow = false;
    for i in 0..func.child_count() {
        let Some(child) = func.child(i) else { continue };
        match child.kind() {
            "->" => after_arrow = true,
            "type" if after_arrow => return first_named_text(&child, source),
            _ => {}
        }
    }
    None
}

fn first_named_text<'a>(node: &tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else { continue };
        if child.is_named() {
            return child.utf8_text(source).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use gdscript_api_db::ApiDb;
    use gdscript_parser::parse::parse;

    use super::*;

    fn db() -> ApiDb {
        ApiDb::bundled().unwrap()
    }

    fn codes(src: &str) -> Vec<String> {
        let db = db();
        let doc = parse(src).unwrap();
        check_type_mismatches(&doc, &db)
            .into_iter()
            .filter_map(|d| {
                if let Some(tower_lsp::lsp_types::NumberOrString::String(s)) = d.code {
                    Some(s)
                } else {
                    None
                }
            })
            .collect()
    }

    fn msgs(src: &str) -> Vec<String> {
        let db = db();
        let doc = parse(src).unwrap();
        check_type_mismatches(&doc, &db)
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    // --- variable assignment ---

    #[test]
    fn string_to_int_is_error() {
        let src = "var x: int = \"hello\"\n";
        assert!(codes(src).contains(&"E0003".to_owned()));
    }

    #[test]
    fn int_literal_to_int_is_ok() {
        let src = "var x: int = 42\n";
        assert!(codes(src).is_empty());
    }

    #[test]
    fn int_to_float_is_ok() {
        let src = "var x: float = 1\n";
        assert!(codes(src).is_empty());
    }

    #[test]
    fn bool_to_int_is_error() {
        let src = "var x: int = true\n";
        assert!(codes(src).contains(&"E0003".to_owned()));
    }

    #[test]
    fn unannotated_var_no_diag() {
        let src = "var x = \"hello\"\n";
        assert!(codes(src).is_empty());
    }

    #[test]
    fn variant_accepts_anything() {
        let src = "var x: Variant = \"hello\"\n";
        assert!(codes(src).is_empty());
    }

    // --- return type ---

    #[test]
    fn wrong_return_literal() {
        let src = "func foo() -> int:\n\treturn \"oops\"\n";
        let m = msgs(src);
        assert!(!m.is_empty());
        assert!(m[0].contains("Return type mismatch"));
    }

    #[test]
    fn correct_return_literal() {
        let src = "func foo() -> int:\n\treturn 42\n";
        assert!(codes(src).is_empty());
    }

    #[test]
    fn void_function_no_diag() {
        let src = "func foo() -> void:\n\treturn\n";
        assert!(codes(src).is_empty());
    }

    #[test]
    fn return_in_if_branch_checked() {
        let src = "func foo() -> int:\n\tif true:\n\t\treturn \"bad\"\n";
        assert!(codes(src).contains(&"E0003".to_owned()));
    }
}
