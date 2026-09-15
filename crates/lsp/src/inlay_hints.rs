use gdscript_parser::ParsedDocument;
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position, Range};

use crate::type_util::infer_literal_type;

/// Compute inlay type hints for untyped variable declarations with literal RHS,
/// and implicit `-> void` on functions without a declared return type.
///
/// Shows `: int` after `var x = 5`, `: float` after `var y = 3.14`, etc.
/// Skips declarations that already have an explicit type annotation.
/// Shows ` -> void` after `func foo()` when no return type is declared.
#[must_use]
pub fn inlay_hints(doc: &ParsedDocument, range: &Range) -> Vec<InlayHint> {
    let mut hints = Vec::new();
    let source = doc.source.as_bytes();
    let root = doc.tree.root_node();

    collect_hints(&root, source, range, &mut hints);
    hints
}

fn collect_hints(node: &tree_sitter::Node, source: &[u8], range: &Range, out: &mut Vec<InlayHint>) {
    match node.kind() {
        "variable_statement" => {
            hint_for_var(node, source, range, out);
            return;
        }
        "function_definition" => {
            hint_for_func(node, source, range, out);
            for i in 0..node.child_count() as u32 {
                let Some(child) = node.child(i) else { continue };
                if child.kind() == "body" {
                    collect_hints(&child, source, range, out);
                }
            }
            return;
        }
        _ => {}
    }

    for i in 0..node.child_count() as u32 {
        let Some(child) = node.child(i) else { continue };
        collect_hints(&child, source, range, out);
    }
}

fn hint_for_var(stmt: &tree_sitter::Node, _source: &[u8], range: &Range, out: &mut Vec<InlayHint>) {
    let has_type = (0..stmt.child_count() as u32)
        .filter_map(|i| stmt.child(i))
        .any(|n| n.kind() == "type");
    if has_type {
        return;
    }

    let name_node = (0..stmt.child_count() as u32)
        .filter_map(|i| stmt.child(i))
        .find(|n| n.kind() == "name");
    let Some(name_node) = name_node else { return };

    let name_end = name_node.end_position();
    let hint_pos = Position {
        line: name_end.row as u32,
        character: name_end.column as u32,
    };

    if hint_pos.line < range.start.line || hint_pos.line > range.end.line {
        return;
    }

    let value = rhs_value(stmt);
    let Some(value) = value else { return };

    let Some(type_name) = infer_literal_type(&value) else {
        return;
    };

    out.push(InlayHint {
        position: hint_pos,
        label: InlayHintLabel::String(format!(": {type_name}")),
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: Some(false),
        padding_right: Some(true),
        data: None,
    });
}

/// Emit a ` -> void` inlay hint for functions that have no declared return type.
///
/// The hint is placed right after the closing `)` of the parameter list so
/// the reader can see the implicit return type without having to remember
/// `GDScript`'s default.
fn hint_for_func(
    func: &tree_sitter::Node,
    _source: &[u8],
    range: &Range,
    out: &mut Vec<InlayHint>,
) {
    let has_return_type = (0..func.child_count() as u32)
        .filter_map(|i| func.child(i))
        .any(|n| n.kind() == "->");
    if has_return_type {
        return;
    }

    let params_node = (0..func.child_count() as u32)
        .filter_map(|i| func.child(i))
        .find(|n| n.kind() == "parameters");
    let Some(params_node) = params_node else {
        return;
    };

    let params_end = params_node.end_position();
    let hint_pos = Position {
        line: params_end.row as u32,
        character: params_end.column as u32,
    };

    if hint_pos.line < range.start.line || hint_pos.line > range.end.line {
        return;
    }

    out.push(InlayHint {
        position: hint_pos,
        label: InlayHintLabel::String(" -> void".to_owned()),
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: Some(false),
        padding_right: Some(false),
        data: None,
    });
}

fn rhs_value<'a>(stmt: &'a tree_sitter::Node) -> Option<tree_sitter::Node<'a>> {
    let mut after_eq = false;
    for i in 0..stmt.child_count() as u32 {
        let Some(child) = stmt.child(i) else { continue };
        if child.kind() == "=" {
            after_eq = true;
        } else if after_eq && child.is_named() {
            return Some(child);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use gdscript_parser::parse::parse;
    use tower_lsp::lsp_types::{Position, Range};

    use super::*;

    fn full_range() -> Range {
        Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 9999,
                character: 0,
            },
        }
    }

    fn hint_labels(src: &str) -> Vec<String> {
        let doc = parse(src).unwrap();
        inlay_hints(&doc, &full_range())
            .into_iter()
            .map(|h| match h.label {
                InlayHintLabel::String(s) => s,
                InlayHintLabel::LabelParts(_) => String::new(),
            })
            .collect()
    }

    #[test]
    fn int_literal_gets_hint() {
        let labels = hint_labels("var x = 5\n");
        assert_eq!(labels, vec![": int"]);
    }

    #[test]
    fn float_literal_gets_hint() {
        let labels = hint_labels("var x = 3.14\n");
        assert_eq!(labels, vec![": float"]);
    }

    #[test]
    fn string_literal_gets_hint() {
        let labels = hint_labels("var x = \"hello\"\n");
        assert_eq!(labels, vec![": String"]);
    }

    #[test]
    fn bool_literal_gets_hint() {
        let labels = hint_labels("var x = true\n");
        assert_eq!(labels, vec![": bool"]);
    }

    #[test]
    fn typed_var_no_hint() {
        let labels = hint_labels("var x: int = 5\n");
        assert!(labels.is_empty());
    }

    #[test]
    fn expression_rhs_no_hint() {
        let labels = hint_labels("var x = get_parent()\n");
        assert!(labels.is_empty());
    }

    #[test]
    fn hint_position_is_after_name() {
        let doc = parse("var foo = 42\n").unwrap();
        let hints = inlay_hints(&doc, &full_range());
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].position.character, 7);
    }

    #[test]
    fn local_var_in_function_gets_hint() {
        let src = "func _ready():\n\tvar x = 1\n";
        let labels = hint_labels(src);
        assert!(
            labels.contains(&": int".to_owned()),
            "expected local var hint"
        );
    }

    #[test]
    fn viewport_range_filters_hints() {
        let src = "var a = 1\nvar b = 2\nvar c = 3\n";
        let doc = parse(src).unwrap();
        let range = Range {
            start: Position {
                line: 1,
                character: 0,
            },
            end: Position {
                line: 1,
                character: 0,
            },
        };
        let hints = inlay_hints(&doc, &range);
        assert_eq!(hints.len(), 1);
    }

    #[test]
    fn func_without_return_type_gets_void_hint() {
        let src = "func _ready():\n\tpass\n";
        let labels = hint_labels(src);
        assert!(
            labels.iter().any(|l| l == " -> void"),
            "expected a void hint, got: {labels:?}"
        );
    }

    #[test]
    fn func_with_return_type_no_hint() {
        let src = "func get_count() -> int:\n\treturn 0\n";
        let labels = hint_labels(src);
        assert!(
            !labels.iter().any(|l| l.contains("void")),
            "should not emit void hint for typed function, got: {labels:?}",
        );
    }

    #[test]
    fn func_with_return_type_body_var_still_gets_hint() {
        let src = "func compute() -> int:\n\tvar x = 5\n\treturn x\n";
        let labels = hint_labels(src);
        assert!(
            labels.contains(&": int".to_owned()),
            "body var hint expected"
        );
        assert!(
            !labels.iter().any(|l| l.contains("void")),
            "no void hint expected"
        );
    }

    #[test]
    fn multiple_funcs_each_get_void_hint_when_untyped() {
        let src = "func a():\n\tpass\n\nfunc b():\n\tpass\n";
        let labels = hint_labels(src);
        assert_eq!(
            labels.iter().filter(|l| l.as_str() == " -> void").count(),
            2,
            "expected two void hints, got: {labels:?}"
        );
    }

    #[test]
    fn void_hint_position_is_after_closing_paren() {
        let src = "func foo():\n\tpass\n";
        let doc = parse(src).unwrap();
        let hints = inlay_hints(&doc, &full_range());
        let void_hint = hints
            .iter()
            .find(|h| match &h.label {
                InlayHintLabel::String(s) => s.contains("void"),
                InlayHintLabel::LabelParts(_) => false,
            })
            .expect("void hint should be present");
        assert_eq!(void_hint.position.line, 0);
        assert_eq!(void_hint.position.character, 10);
    }

    #[test]
    fn local_var_in_untyped_func_shows_both_hints() {
        let src = "func _ready():\n\tvar x = 1\n";
        let labels = hint_labels(src);
        assert!(labels.contains(&" -> void".to_owned()), "void hint missing");
        assert!(labels.contains(&": int".to_owned()), "var hint missing");
    }
}
