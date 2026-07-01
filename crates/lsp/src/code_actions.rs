use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic, NumberOrString, Position, Range,
    TextEdit, Url, WorkspaceEdit,
};
use tree_sitter_gdscript;

use crate::type_util::infer_literal_type;

/// Build quick-fix code actions for diagnostics at the given range.
#[must_use]
pub fn code_actions_for(
    uri: &Url,
    source: &str,
    _range: &Range,
    diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    for diag in diagnostics {
        let code = match &diag.code {
            Some(NumberOrString::String(s)) => s.as_str(),
            _ => continue,
        };

        match code {
            "W0001" => {
                // Unused variable — offer to remove the declaration line.
                if let Some(action) = remove_var_action(uri, source, diag) {
                    actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
            "W0002" => {
                // Missing return — offer to append a bare `return`.
                if let Some(action) = add_return_action(uri, diag) {
                    actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
            _ => {}
        }
    }

    // Offer "Add type annotation" for untyped vars with literal RHS at the range.
    if let Some(action) = add_type_annotation_action(uri, source, _range, diagnostics) {
        actions.push(CodeActionOrCommand::CodeAction(action));
    }

    actions
}

/// "Remove unused variable" — deletes the entire declaration line.
fn remove_var_action(uri: &Url, source: &str, diag: &Diagnostic) -> Option<CodeAction> {
    let line = diag.range.start.line;
    let line_count = source.lines().count() as u32;
    let end_line = (line + 1).min(line_count);

    let edit = single_file_edit(
        uri,
        Range {
            start: Position { line, character: 0 },
            end: Position { line: end_line, character: 0 },
        },
        String::new(),
    );

    Some(CodeAction {
        title: "Remove unused variable".to_owned(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(edit),
        ..Default::default()
    })
}

/// "Add return statement" — appends `\treturn` after the function body's last line.
fn add_return_action(uri: &Url, diag: &Diagnostic) -> Option<CodeAction> {
    let line = diag.range.end.line + 1;
    let edit = single_file_edit(
        uri,
        Range {
            start: Position { line, character: 0 },
            end: Position { line, character: 0 },
        },
        "\treturn\n".to_owned(),
    );

    Some(CodeAction {
        title: "Add return statement".to_owned(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(edit),
        ..Default::default()
    })
}

/// "Add type annotation" — offered for untyped `var x = <literal>` where we can
/// infer the type. Scans all `variable_statement` nodes in the viewport range.
fn add_type_annotation_action(
    uri: &Url,
    source: &str,
    range: &Range,
    _diagnostics: &[Diagnostic],
) -> Option<CodeAction> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_gdscript::LANGUAGE.into()).ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();
    let bytes = source.as_bytes();

    find_untyped_var_in_range(&root, bytes, range, uri)
}

fn find_untyped_var_in_range(
    node: &tree_sitter::Node,
    source: &[u8],
    range: &Range,
    uri: &Url,
) -> Option<CodeAction> {
    if node.kind() == "variable_statement" {
        let stmt_line = node.start_position().row as u32;
        if stmt_line >= range.start.line && stmt_line <= range.end.line {
            if let Some(action) = type_annotation_for_var(node, source, uri) {
                return Some(action);
            }
        }
    }

    for i in 0..node.child_count() {
        let child = node.child(i)?;
        if let Some(action) = find_untyped_var_in_range(&child, source, range, uri) {
            return Some(action);
        }
    }
    None
}

fn type_annotation_for_var(
    stmt: &tree_sitter::Node,
    source: &[u8],
    uri: &Url,
) -> Option<CodeAction> {
    // Skip if already typed
    let has_type = (0..stmt.child_count())
        .filter_map(|i| stmt.child(i))
        .any(|n| n.kind() == "type");
    if has_type {
        return None;
    }

    // Find name node
    let name_node = (0..stmt.child_count())
        .filter_map(|i| stmt.child(i))
        .find(|n| n.kind() == "name")?;
    let name = name_node.utf8_text(source).ok()?;

    // Find RHS literal
    let mut after_eq = false;
    let value = (0..stmt.child_count()).find_map(|i| {
        let child = stmt.child(i)?;
        if child.kind() == "=" {
            after_eq = true;
            return None;
        }
        if after_eq && child.is_named() {
            return Some(child);
        }
        None
    })?;

    let type_name = infer_literal_type(&value)?;

    // Insert `: TypeName` right after the name node
    let name_end = name_node.end_position();
    let insert_pos = Position {
        line: name_end.row as u32,
        character: name_end.column as u32,
    };

    let edit = single_file_edit(
        uri,
        Range { start: insert_pos, end: insert_pos },
        format!(": {type_name}"),
    );

    Some(CodeAction {
        title: format!("Add type annotation: var {name}: {type_name}"),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(edit),
        ..Default::default()
    })
}

fn single_file_edit(uri: &Url, range: Range, new_text: String) -> WorkspaceEdit {
    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), vec![TextEdit { range, new_text }]);
    WorkspaceEdit { changes: Some(changes), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::{DiagnosticSeverity, NumberOrString, Position, Range};

    use super::*;

    fn uri() -> Url {
        "file:///test.gd".parse().unwrap()
    }

    fn diag(code: &str, line: u32) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position { line, character: 0 },
                end: Position { line, character: 10 },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String(code.to_owned())),
            message: "test".to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn remove_var_action_produced_for_w0001() {
        let src = "var x = 1\nvar y = 2\n";
        let diags = vec![diag("W0001", 0)];
        let range = Range {
            start: Position { line: 0, character: 0 },
            end: Position { line: 0, character: 10 },
        };
        let actions = code_actions_for(&uri(), src, &range, &diags);
        assert!(actions.iter().any(|a| {
            matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Remove"))
        }));
    }

    #[test]
    fn add_return_action_produced_for_w0002() {
        let src = "func foo() -> int:\n\tvar x = 1\n";
        let diags = vec![diag("W0002", 1)];
        let range = Range {
            start: Position { line: 1, character: 0 },
            end: Position { line: 1, character: 10 },
        };
        let actions = code_actions_for(&uri(), src, &range, &diags);
        assert!(actions.iter().any(|a| {
            matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("return"))
        }));
    }

    #[test]
    fn type_annotation_offered_for_untyped_int_var() {
        let src = "var x = 42\n";
        let range = Range {
            start: Position { line: 0, character: 0 },
            end: Position { line: 0, character: 10 },
        };
        let actions = code_actions_for(&uri(), src, &range, &[]);
        assert!(actions.iter().any(|a| {
            matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("int"))
        }));
    }

    #[test]
    fn no_type_annotation_for_already_typed_var() {
        let src = "var x: int = 42\n";
        let range = Range {
            start: Position { line: 0, character: 0 },
            end: Position { line: 0, character: 15 },
        };
        let actions = code_actions_for(&uri(), src, &range, &[]);
        let has_type_action = actions.iter().any(|a| {
            matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("type annotation"))
        });
        assert!(!has_type_action);
    }

    #[test]
    fn no_type_annotation_for_expression_rhs() {
        let src = "var x = get_node(\"/root\")\n";
        let range = Range {
            start: Position { line: 0, character: 0 },
            end: Position { line: 0, character: 25 },
        };
        let actions = code_actions_for(&uri(), src, &range, &[]);
        let has_type_action = actions.iter().any(|a| {
            matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("type annotation"))
        });
        assert!(!has_type_action);
    }
}
