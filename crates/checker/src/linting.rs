use gdscript_parser::ParsedDocument;

use crate::diagnostics::{Diagnostic, Severity};

/// Run all lint passes on a parsed document. Returns warnings for:
/// - W0001: unused local variable
/// - W0002: function with declared return type missing a return on some path
/// - W0003: unreachable code after return/break/continue
/// - W0004: file missing `class_name` declaration (plugin.gd exempt)
/// - W0005: match statement missing enum variants (non-exhaustive)
#[must_use]
pub fn lint(doc: &ParsedDocument) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let source = doc.source.as_bytes();
    let root = doc.tree.root_node();

    let mut has_class_name = false;
    let mut is_editor_plugin = false;

    // Build a map of enum_name → Vec<variant_name> for exhaustiveness checks.
    let enum_map = collect_enum_definitions(&root, source);

    for i in 0..root.child_count() as u32 {
        let Some(node) = root.child(i) else { continue };
        match node.kind() {
            "class_name_statement" => has_class_name = true,
            "extends_statement" => {
                // Exempt EditorPlugin scripts — they're registered differently.
                if let Ok(text) = node.utf8_text(source) {
                    if text.contains("EditorPlugin") {
                        is_editor_plugin = true;
                    }
                }
            }
            "function_definition" => {
                lint_function(&node, source, &mut out);
                lint_match_exhaustiveness_in_func(&node, source, &enum_map, &mut out);
            }
            _ => {}
        }
    }

    if !has_class_name && !is_editor_plugin {
        out.push(Diagnostic {
            line: 0,
            col: 0,
            end_line: 0,
            end_col: 0,
            severity: Severity::Warning,
            code: Some("W0004".to_owned()),
            message: "File is missing a class_name declaration".to_owned(),
        });
    }

    out
}

/// Collect all `enum MyEnum { A, B, C }` definitions at file scope.
/// Returns a map from enum name → list of variant name strings.
fn collect_enum_definitions(
    root: &tree_sitter::Node,
    source: &[u8],
) -> std::collections::HashMap<String, Vec<String>> {
    let mut map = std::collections::HashMap::new();
    for i in 0..root.child_count() as u32 {
        let Some(node) = root.child(i) else { continue };
        if node.kind() != "enum_definition" {
            continue;
        }
        // Named enum: `enum Dir { UP, DOWN }` — anonymous enums are ignored.
        let name_node = (0..node.child_count() as u32)
            .filter_map(|j| node.child(j))
            .find(|n| n.kind() == "name");
        let Some(name_node) = name_node else { continue };
        let Ok(enum_name) = name_node.utf8_text(source) else {
            continue;
        };

        let mut variants = Vec::new();
        for j in 0..node.child_count() as u32 {
            let Some(child) = node.child(j) else { continue };
            if child.kind() == "enumerator_list" {
                for k in 0..child.child_count() as u32 {
                    let Some(enumerator) = child.child(k) else {
                        continue;
                    };
                    if enumerator.kind() == "enumerator" {
                        if let Some(ident) = (0..enumerator.child_count() as u32)
                            .filter_map(|m| enumerator.child(m))
                            .find(tree_sitter::Node::is_named)
                        {
                            if let Ok(v) = ident.utf8_text(source) {
                                variants.push(v.to_owned());
                            }
                        }
                    }
                }
            }
        }
        map.insert(enum_name.to_owned(), variants);
    }
    map
}

/// Check all `match` statements inside a function for non-exhaustive enum coverage.
fn lint_match_exhaustiveness_in_func(
    func: &tree_sitter::Node,
    source: &[u8],
    enum_map: &std::collections::HashMap<String, Vec<String>>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(body) = (0..func.child_count() as u32)
        .filter_map(|i| func.child(i))
        .find(|n| n.kind() == "body")
    else {
        return;
    };
    for i in 0..body.child_count() as u32 {
        let Some(stmt) = body.child(i) else { continue };
        if stmt.kind() == "match_statement" {
            check_match_exhaustiveness(&stmt, source, enum_map, out);
        }
    }
}

/// Check a single `match_statement` for missing enum variants.
///
/// Strategy: look at all `pattern_section` children. If any pattern is a bare
/// `_` identifier (wildcard arm), the match is trivially exhaustive — skip.
/// Otherwise, collect all patterns of the form `EnumName.VARIANT`. If they all
/// share the same enum name AND that enum exists in `enum_map`, warn about any
/// variants that are not covered.
fn check_match_exhaustiveness(
    match_stmt: &tree_sitter::Node,
    source: &[u8],
    enum_map: &std::collections::HashMap<String, Vec<String>>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(match_body) = match_stmt.child_by_field_name("body").or_else(|| {
        (0..match_stmt.child_count() as u32)
            .filter_map(|i| match_stmt.child(i))
            .find(|n| n.kind() == "match_body")
    }) else {
        return;
    };

    // Gather all pattern_section nodes.
    let sections: Vec<tree_sitter::Node> = (0..match_body.child_count() as u32)
        .filter_map(|i| match_body.child(i))
        .filter(|n| n.kind() == "pattern_section")
        .collect();

    // If any pattern is a wildcard `_`, the match is exhaustive.
    let has_wildcard = sections.iter().any(|sec| {
        (0..sec.child_count() as u32)
            .filter_map(|i| sec.child(i))
            .any(|p| {
                (p.kind() == "identifier" && p.utf8_text(source).ok() == Some("_"))
                    || p.kind() == "pattern_open_ending"
            })
    });
    if has_wildcard {
        return;
    }

    // Collect all `EnumName.VARIANT` member-access patterns.
    let mut enum_name: Option<String> = None;
    let mut covered: std::collections::HashSet<String> = std::collections::HashSet::new();

    for sec in &sections {
        for i in 0..sec.child_count() as u32 {
            let Some(pattern) = sec.child(i) else {
                continue;
            };
            // A member access pattern looks like `identifier "." identifier`.
            if pattern.kind() == "attribute" || pattern.kind() == "member_access" {
                // Try to extract `lhs.rhs` from the node text.
                if let Ok(text) = pattern.utf8_text(source) {
                    if let Some(dot) = text.find('.') {
                        let lhs = &text[..dot];
                        let rhs = &text[dot + 1..];
                        if let Some(ref en) = enum_name {
                            if en == lhs {
                                covered.insert(rhs.to_owned());
                            }
                        } else if enum_map.contains_key(lhs) {
                            enum_name = Some(lhs.to_owned());
                            covered.insert(rhs.to_owned());
                        }
                    }
                }
            }
        }
    }

    let Some(enum_name) = enum_name else { return };
    let Some(all_variants) = enum_map.get(&enum_name) else {
        return;
    };

    let missing: Vec<&String> = all_variants
        .iter()
        .filter(|v| !covered.contains(*v))
        .collect();

    if !missing.is_empty() {
        let start = match_stmt.start_position();
        let end = match_stmt.end_position();
        let missing_list: Vec<&str> = missing.iter().map(|s| s.as_str()).collect();
        out.push(Diagnostic {
            line: start.row as u32,
            col: start.column as u32,
            end_line: end.row as u32,
            end_col: end.column as u32,
            severity: Severity::Warning,
            code: Some("W0005".to_owned()),
            message: format!(
                "Non-exhaustive match on '{}': missing variants: {}",
                enum_name,
                missing_list.join(", ")
            ),
        });
    }
}

fn lint_function(func: &tree_sitter::Node, source: &[u8], out: &mut Vec<Diagnostic>) {
    let Some(body) = (0..func.child_count() as u32)
        .filter_map(|i| func.child(i))
        .find(|n| n.kind() == "body")
    else {
        return;
    };

    let stmts: Vec<tree_sitter::Node> = (0..body.child_count() as u32)
        .filter_map(|i| body.child(i))
        .filter(tree_sitter::Node::is_named)
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

    let last = stmts.iter().rev().find(|n| n.kind() != "pass_statement");

    let needs_warning = match last {
        None => true,
        Some(n) => !matches!(
            n.kind(),
            "return_statement" | "if_statement" | "match_statement"
        ),
    };

    if needs_warning {
        if let Some(name_node) = (0..func.child_count() as u32)
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
                    "Function '{name_text}' has return type '{ret_type}' but not all paths return a value"
                ),
            });
        }
    }
}

/// Warn on local variables declared inside a function body that are never read.
/// Variables prefixed with `_` are exempt (intentional unused convention).
fn check_unused_locals(body: &tree_sitter::Node, source: &[u8], out: &mut Vec<Diagnostic>) {
    let locals: Vec<(String, tree_sitter::Node)> = (0..body.child_count() as u32)
        .filter_map(|i| body.child(i))
        .filter(|n| n.kind() == "variable_statement")
        .filter_map(|stmt| {
            let name_node = (0..stmt.child_count() as u32)
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
    for i in 0..node.child_count() as u32 {
        if let Some(child) = node.child(i) {
            count += count_identifier_uses(&child, source, name);
        }
    }
    count
}

/// Get the declared return type text of a function, if present.
fn get_return_type<'a>(func: &tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    let mut after_arrow = false;
    for i in 0..func.child_count() as u32 {
        let Some(child) = func.child(i) else { continue };
        match child.kind() {
            "->" => after_arrow = true,
            "type" if after_arrow => {
                for j in 0..child.child_count() as u32 {
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
        diags(src).into_iter().filter_map(|d| d.code).collect()
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

    // --- missing class_name ---

    #[test]
    fn missing_class_name_warned() {
        let src = "extends Node\nfunc _ready():\n\tpass\n";
        assert!(codes(src).contains(&"W0004".to_owned()));
    }

    #[test]
    fn class_name_present_no_warning() {
        let src = "class_name MyClass\nextends Node\nfunc _ready():\n\tpass\n";
        assert!(!codes(src).contains(&"W0004".to_owned()));
    }

    #[test]
    fn editor_plugin_exempt_from_class_name_warning() {
        let src = "@tool\nextends EditorPlugin\nfunc _enter_tree():\n\tpass\n";
        assert!(!codes(src).contains(&"W0004".to_owned()));
    }

    // --- match exhaustiveness (W0005) ---

    #[test]
    fn match_exhaustiveness_warns_when_variant_missing() {
        let src = "class_name T\nenum Dir { UP, DOWN, LEFT }\nfunc go(d):\n\tmatch d:\n\t\tDir.UP:\n\t\t\tpass\n\t\tDir.DOWN:\n\t\t\tpass\n";
        assert!(
            codes(src).contains(&"W0005".to_owned()),
            "should warn about missing LEFT"
        );
    }

    #[test]
    fn match_exhaustiveness_no_warn_when_all_covered() {
        let src = "class_name T\nenum Dir { UP, DOWN }\nfunc go(d):\n\tmatch d:\n\t\tDir.UP:\n\t\t\tpass\n\t\tDir.DOWN:\n\t\t\tpass\n";
        assert!(
            !codes(src).contains(&"W0005".to_owned()),
            "all variants covered — no warning"
        );
    }

    #[test]
    fn match_exhaustiveness_no_warn_with_wildcard() {
        let src = "class_name T\nenum Dir { UP, DOWN, LEFT }\nfunc go(d):\n\tmatch d:\n\t\tDir.UP:\n\t\t\tpass\n\t\t_:\n\t\t\tpass\n";
        assert!(
            !codes(src).contains(&"W0005".to_owned()),
            "wildcard arm present — exhaustive"
        );
    }
}
