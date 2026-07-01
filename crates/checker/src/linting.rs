use gdscript_parser::ParsedDocument;

use crate::diagnostics::{Diagnostic, Severity};

/// Run all lint passes on a parsed document. Returns warnings for:
/// - W0001: unused local variable
/// - W0002: function with declared return type missing a return on some path
/// - W0003: unreachable code after return/break/continue
#[must_use]
pub fn lint(doc: &ParsedDocument) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let source = doc.source.as_bytes();
    let root = doc.tree.root_node();

    for i in 0..root.child_count() {
        let Some(node) = root.child(i) else { continue };
        if node.kind() == "function_definition" {
            lint_function(&node, source, &mut out);
        }
    }

    out
}

fn lint_function(func: &tree_sitter::Node, source: &[u8], out: &mut Vec<Diagnostic>) {
    let Some(body) = (0..func.child_count())
        .filter_map(|i| func.child(i))
        .find(|n| n.kind() == "body")
    else {
        return;
    };

    let stmts: Vec<tree_sitter::Node> = (0..body.child_count())
        .filter_map(|i| body.child(i))
        .filter(|n| n.is_named())
        .collect();

    check_unreachable(&stmts, out);
    check_missing_return(func, &stmts, source, out);
    check_unused_locals(&body, source, out);
}

/// Warn on statements after return/break/continue.
fn check_unreachable(stmts: &[tree_sitter::Node], out: &mut Vec<Diagnostic>) {
    let mut terminated = false;
    for stmt in stmts {
        if terminated {
            let start = stmt.start_position();
            let end = stmt.end_position();
            out.push(Diagnostic {
                line: start.row as u32,
                col: start.column as u32,
                end_line: end.row as u32,
                end_col: end.column as u32,
                severity: Severity::Warning,
                code: Some("W0003".to_owned()),
                message: "Unreachable code".to_owned(),
            });
        }
        if matches!(
            stmt.kind(),
            "return_statement" | "break_statement" | "continue_statement"
        ) {
            terminated = true;
        }
    }
}

/// Warn when a non-void function's last reachable statement isn't a return.
/// Skips the check when the last statement is a branch (if/match) to avoid
/// false positives on exhaustive branches — control-flow analysis is out of scope.
fn check_missing_return(
    func: &tree_sitter::Node,
    stmts: &[tree_sitter::Node],
    source: &[u8],
    out: &mut Vec<Diagnostic>,
) {
    let Some(ret_type) = get_return_type(func, source) else {
        return;
    };
    if ret_type == "void" {
        return;
    }

    let last = stmts
        .iter()
        .rev()
        .find(|n| n.kind() != "pass_statement");

    let needs_warning = match last {
        None => true,
        Some(n) => !matches!(
            n.kind(),
            "return_statement" | "if_statement" | "match_statement"
        ),
    };

    if needs_warning {
        if let Some(name_node) = (0..func.child_count())
            .filter_map(|i| func.child(i))
            .find(|n| n.kind() == "name")
        {
            let start = name_node.start_position();
            let name_text = name_node.utf8_text(source).unwrap_or("?");
            out.push(Diagnostic {
                line: start.row as u32,
                col: start.column as u32,
                end_line: start.row as u32,
                end_col: (start.column + name_text.len()) as u32,
                severity: Severity::Warning,
                code: Some("W0002".to_owned()),
                message: format!(
                    "Function '{}' has return type '{}' but not all paths return a value",
                    name_text, ret_type
                ),
            });
        }
    }
}

/// Warn on local variables declared inside a function body that are never read.
/// Variables prefixed with `_` are exempt (intentional unused convention).
fn check_unused_locals(body: &tree_sitter::Node, source: &[u8], out: &mut Vec<Diagnostic>) {
    let locals: Vec<(String, tree_sitter::Node)> = (0..body.child_count())
        .filter_map(|i| body.child(i))
        .filter(|n| n.kind() == "variable_statement")
        .filter_map(|stmt| {
            let name_node = (0..stmt.child_count())
                .filter_map(|j| stmt.child(j))
                .find(|n| n.kind() == "name")?;
            let name = name_node.utf8_text(source).ok()?.to_owned();
            Some((name, stmt))
        })
        .collect();

    for (name, decl) in &locals {
        if name.starts_with('_') {
            continue;
        }
        // Count `identifier` nodes (usages) — `name` nodes (declarations) are a different kind.
        if count_identifier_uses(body, source, name) == 0 {
            let start = decl.start_position();
            let end = decl.end_position();
            out.push(Diagnostic {
                line: start.row as u32,
                col: start.column as u32,
                end_line: end.row as u32,
                end_col: end.column as u32,
                severity: Severity::Warning,
                code: Some("W0001".to_owned()),
                message: format!("Unused variable '{name}'"),
            });
        }
    }
}

/// Walk a subtree counting `identifier` nodes (usages, not declarations) matching `name`.
fn count_identifier_uses(node: &tree_sitter::Node, source: &[u8], name: &str) -> usize {
    let mut count = 0;
    if node.kind() == "identifier" && node.utf8_text(source).ok() == Some(name) {
        count += 1;
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            count += count_identifier_uses(&child, source, name);
        }
    }
    count
}

/// Get the declared return type text of a function, if present.
fn get_return_type<'a>(func: &tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    let mut after_arrow = false;
    for i in 0..func.child_count() {
        let Some(child) = func.child(i) else { continue };
        match child.kind() {
            "->" => after_arrow = true,
            "type" if after_arrow => {
                for j in 0..child.child_count() {
                    let Some(c) = child.child(j) else { continue };
                    if c.is_named() {
                        return c.utf8_text(source).ok();
                    }
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use gdscript_parser::parse::parse;

    use super::*;

    fn diags(src: &str) -> Vec<Diagnostic> {
        let doc = parse(src).unwrap();
        lint(&doc)
    }

    fn codes(src: &str) -> Vec<String> {
        diags(src)
            .into_iter()
            .filter_map(|d| d.code)
            .collect()
    }

    // --- unused variables ---

    #[test]
    fn unused_local_var_warned() {
        let src = "func _ready():\n\tvar x: int = 5\n";
        assert!(codes(src).contains(&"W0001".to_owned()));
    }

    #[test]
    fn used_local_var_clean() {
        let src = "func _ready():\n\tvar x: int = 5\n\tprint(x)\n";
        assert!(!codes(src).contains(&"W0001".to_owned()));
    }

    #[test]
    fn underscore_prefix_suppresses_warning() {
        let src = "func _ready():\n\tvar _x: int = 5\n";
        assert!(!codes(src).contains(&"W0001".to_owned()));
    }

    #[test]
    fn var_used_in_rhs_of_another_is_not_unused() {
        let src = "func _ready():\n\tvar x: int = 1\n\tvar y: int = x\n\tprint(y)\n";
        assert!(!codes(src).contains(&"W0001".to_owned()));
    }

    // --- missing return ---

    #[test]
    fn missing_return_on_non_void() {
        let src = "func foo() -> int:\n\tvar x = 5\n";
        assert!(codes(src).contains(&"W0002".to_owned()));
    }

    #[test]
    fn return_present_no_warning() {
        let src = "func foo() -> int:\n\treturn 42\n";
        assert!(!codes(src).contains(&"W0002".to_owned()));
    }

    #[test]
    fn void_function_no_warning() {
        let src = "func foo() -> void:\n\tpass\n";
        assert!(!codes(src).contains(&"W0002".to_owned()));
    }

    #[test]
    fn no_return_type_no_warning() {
        let src = "func foo():\n\tvar x = 5\n";
        assert!(!codes(src).contains(&"W0002".to_owned()));
    }

    #[test]
    fn if_as_last_stmt_no_false_positive() {
        // We can't prove branches are exhaustive, so we don't warn on if-as-last-stmt.
        let src = "func foo() -> int:\n\tif true:\n\t\treturn 1\n\telse:\n\t\treturn 2\n";
        assert!(!codes(src).contains(&"W0002".to_owned()));
    }

    // --- unreachable code ---

    #[test]
    fn code_after_return_is_unreachable() {
        let src = "func foo():\n\treturn\n\tvar x = 1\n";
        assert!(codes(src).contains(&"W0003".to_owned()));
    }

    #[test]
    fn code_after_break_is_unreachable() {
        let src = "func foo():\n\tbreak\n\tvar x = 1\n";
        assert!(codes(src).contains(&"W0003".to_owned()));
    }

    #[test]
    fn normal_code_not_unreachable() {
        let src = "func foo():\n\tvar x = 1\n\tvar y = 2\n\treturn\n";
        assert!(!codes(src).contains(&"W0003".to_owned()));
    }
}
