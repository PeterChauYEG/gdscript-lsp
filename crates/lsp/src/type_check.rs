use std::collections::HashMap;

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

    // Collect declared types for reassignment checking.
    let declared_types = collect_declared_types(&root, source);

    for i in 0..root.child_count() {
        let Some(node) = root.child(i) else { continue };
        match node.kind() {
            "variable_statement" => {
                check_var_assignment(&node, source, api_db, &mut out);
            }
            "function_definition" => {
                check_function(&node, source, api_db, &mut out);
            }
            "expression_statement" => {
                check_reassignment(&node, source, api_db, &declared_types, &mut out);
            }
            _ => {}
        }
    }

    out
}

/// Collect `var name: Type` declarations at the top level of a script.
fn collect_declared_types<'a>(
    root: &tree_sitter::Node,
    source: &'a [u8],
) -> HashMap<String, &'a str> {
    let mut map = HashMap::new();
    for i in 0..root.child_count() {
        let Some(node) = root.child(i) else { continue };
        if node.kind() == "variable_statement" {
            if let (Some(name), Some(ty)) =
                (var_name_text(&node, source), declared_var_type(&node, source))
            {
                map.insert(name.to_owned(), ty);
            }
        }
    }
    map
}

/// Get the `name` child text of a `variable_statement`.
fn var_name_text<'a>(stmt: &tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    (0..stmt.child_count())
        .filter_map(|i| stmt.child(i))
        .find(|n| n.kind() == "name")
        .and_then(|n| n.utf8_text(source).ok())
}

/// Check `x = literal` against the declared type of `x`.
fn check_reassignment(
    stmt: &tree_sitter::Node,
    source: &[u8],
    api_db: &ApiDb,
    declared_types: &HashMap<String, &str>,
    out: &mut Vec<Diagnostic>,
) {
    let assignment = match (0..stmt.child_count())
        .filter_map(|i| stmt.child(i))
        .find(|n| n.kind() == "assignment")
    {
        Some(a) => a,
        None => return,
    };

    // LHS must be a bare identifier.
    let lhs = match (0..assignment.child_count())
        .filter_map(|i| assignment.child(i))
        .find(|n| n.is_named() && n.kind() == "identifier")
    {
        Some(n) => n,
        None => return,
    };

    let var_name = match lhs.utf8_text(source) {
        Ok(s) => s,
        Err(_) => return,
    };

    let declared = match declared_types.get(var_name) {
        Some(t) => *t,
        None => return,
    };

    // RHS is the first named node after `=`.
    let mut after_eq = false;
    let rhs = (0..assignment.child_count()).find_map(|i| {
        let child = assignment.child(i)?;
        if child.kind() == "=" {
            after_eq = true;
            return None;
        }
        if after_eq && child.is_named() {
            return Some(child);
        }
        None
    });

    let Some(rhs) = rhs else { return };
    let Some(inferred) = infer_literal_type(&rhs) else { return };

    if !types_compatible(declared, inferred, api_db) {
        out.push(error_diag(
            node_range(&rhs),
            "E0003",
            format!("Cannot assign `{inferred}` to variable of type `{declared}`"),
        ));
    }
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
            "if_statement" | "while_statement" | "for_statement" => {
                // Direct body children.
                for j in 0..stmt.child_count() {
                    let Some(child) = stmt.child(j) else { continue };
                    if child.kind() == "body" {
                        check_returns_in_body(&child, ret_type, source, api_db, out);
                    }
                }
            }
            "match_statement" => {
                // match_statement → match_body → pattern_section → body
                for j in 0..stmt.child_count() {
                    let Some(match_body) = stmt.child(j) else { continue };
                    if match_body.kind() != "match_body" {
                        continue;
                    }
                    for k in 0..match_body.child_count() {
                        let Some(section) = match_body.child(k) else { continue };
                        if section.kind() != "pattern_section" {
                            continue;
                        }
                        for l in 0..section.child_count() {
                            let Some(body) = section.child(l) else { continue };
                            if body.kind() == "body" {
                                check_returns_in_body(&body, ret_type, source, api_db, out);
                            }
                        }
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

    // --- reassignment ---

    #[test]
    fn reassignment_wrong_type_is_error() {
        let src = "var x: int = 1\nx = \"bad\"\n";
        assert!(codes(src).contains(&"E0003".to_owned()));
    }

    #[test]
    fn reassignment_same_type_is_ok() {
        let src = "var x: int = 1\nx = 2\n";
        assert!(codes(src).is_empty());
    }

    #[test]
    fn reassignment_undeclared_var_no_diag() {
        // No `var y: int` declaration — can't check, should be silent.
        let src = "y = \"bad\"\n";
        assert!(codes(src).is_empty());
    }

    // --- non-literal RHS is silently skipped ---

    #[test]
    fn expression_rhs_is_silently_skipped() {
        // Intentional: we don't infer types for non-literal RHS, so no E0003.
        let src = "var x: int = get_node(\"/root\")\n";
        assert!(codes(src).is_empty());
    }

    // --- nested control flow ---

    #[test]
    fn return_in_nested_while_checked() {
        let src = "func foo() -> int:\n\twhile true:\n\t\treturn \"bad\"\n";
        assert!(codes(src).contains(&"E0003".to_owned()));
    }

    #[test]
    fn return_in_match_branch_checked() {
        let src = "func foo() -> int:\n\tmatch 1:\n\t\t1:\n\t\t\treturn \"bad\"\n";
        assert!(codes(src).contains(&"E0003".to_owned()));
    }

    #[test]
    fn correct_return_in_match_branch_no_diag() {
        let src = "func foo() -> int:\n\tmatch 1:\n\t\t1:\n\t\t\treturn 42\n";
        assert!(codes(src).is_empty());
    }

    // --- multiple return statements ---

    #[test]
    fn multiple_returns_all_correct() {
        let src = "func foo(x: int) -> int:\n\tif x > 0:\n\t\treturn 1\n\treturn 0\n";
        assert!(codes(src).is_empty());
    }

    #[test]
    fn multiple_returns_one_wrong_type() {
        let src = "func foo(x: int) -> int:\n\tif x > 0:\n\t\treturn \"bad\"\n\treturn 0\n";
        assert!(codes(src).contains(&"E0003".to_owned()));
    }
}
