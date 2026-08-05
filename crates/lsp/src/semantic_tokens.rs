use gdscript_parser::ParsedDocument;
use tower_lsp::lsp_types::{SemanticToken, SemanticTokens};

// Token type indices matching capabilities::semantic_token_types().
const TT_CLASS: u32 = 2;
const TT_ENUM: u32 = 3;
const TT_PARAMETER: u32 = 7;
const TT_VARIABLE: u32 = 8;
const TT_ENUM_MEMBER: u32 = 10;
const TT_FUNCTION: u32 = 12;
const TT_DECORATOR: u32 = 22;

const BUILTIN_CONSTANTS: &[&str] = &["PI", "TAU", "INF", "NAN", "OK", "FAILED", "HALTED"];

/// Compute semantic tokens for an entire document (full request).
#[must_use]
pub fn semantic_tokens(doc: &ParsedDocument) -> SemanticTokens {
    let source = doc.source.as_bytes();
    let root = doc.tree.root_node();

    let mut raw: Vec<(u32, u32, u32, u32)> = Vec::new();
    collect_tokens(&root, source, &mut raw);

    raw.sort_by_key(|&(line, col, _, _)| (line, col));
    raw.dedup_by_key(|&mut (line, col, _, _)| (line, col));

    let mut tokens = Vec::with_capacity(raw.len());
    let mut prev_line = 0u32;
    let mut prev_col = 0u32;

    for (line, col, len, tt) in raw {
        let delta_line = line - prev_line;
        let delta_col = if delta_line == 0 { col - prev_col } else { col };
        tokens.push(SemanticToken {
            delta_line,
            delta_start: delta_col,
            length: len,
            token_type: tt,
            token_modifiers_bitset: 0,
        });
        prev_line = line;
        prev_col = col;
    }

    SemanticTokens {
        result_id: None,
        data: tokens,
    }
}

fn collect_tokens(node: &tree_sitter::Node, source: &[u8], out: &mut Vec<(u32, u32, u32, u32)>) {
    match node.kind() {
        "function_definition" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .or_else(|| find_child_kind(node, "name"))
            {
                push_node(&name, TT_FUNCTION, out);
            }
        }
        "lambda" => {
            // Optional name (named lambda: `func my_lambda(x): ...`)
            if let Some(name) = node
                .child_by_field_name("name")
                .or_else(|| find_child_kind(node, "name"))
            {
                push_node(&name, TT_FUNCTION, out);
            }
            // Parameters are highlighted by the "parameters" branch below via recursion.
        }
        "parameters" => {
            for i in 0..node.child_count() as u32 {
                let Some(child) = node.child(i) else { continue };
                match child.kind() {
                    "typed_parameter" | "parameter" => {
                        let ident = child
                            .child_by_field_name("name")
                            .or_else(|| first_named_child(&child));
                        if let Some(n) = ident {
                            push_node(&n, TT_PARAMETER, out);
                        }
                    }
                    "identifier" => push_node(&child, TT_PARAMETER, out),
                    _ => {}
                }
            }
        }
        "variable_statement" | "const_statement" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .or_else(|| find_child_kind(node, "name"))
            {
                push_node(&name, TT_VARIABLE, out);
            }
        }
        "class_name_statement" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .or_else(|| find_child_kind(node, "name"))
            {
                push_node(&name, TT_CLASS, out);
            }
        }
        "class_definition" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .or_else(|| find_child_kind(node, "name"))
            {
                push_node(&name, TT_CLASS, out);
            }
        }
        "annotation" => {
            let start = node.start_position();
            let end_col = node.end_position().column;
            let len = end_col.saturating_sub(start.column);
            if len > 0 && node.start_position().row == node.end_position().row {
                out.push((
                    start.row as u32,
                    start.column as u32,
                    len as u32,
                    TT_DECORATOR,
                ));
            }
        }
        "enum_definition" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .or_else(|| find_child_kind(node, "name"))
            {
                push_node(&name, TT_ENUM, out);
            }
            // Enumerators live inside enumerator_list { enumerator* }
            collect_enum_member_tokens(node, out);
            // Don't recurse into enum body.
            return;
        }
        "identifier" => {
            if let Ok(text) = node.utf8_text(source) {
                if BUILTIN_CONSTANTS.contains(&text) {
                    push_node(node, TT_ENUM_MEMBER, out);
                }
            }
            return;
        }
        _ => {}
    }

    for i in 0..node.child_count() as u32 {
        let Some(child) = node.child(i) else { continue };
        collect_tokens(&child, source, out);
    }
}

fn collect_enum_member_tokens(node: &tree_sitter::Node, out: &mut Vec<(u32, u32, u32, u32)>) {
    for i in 0..node.child_count() as u32 {
        let Some(child) = node.child(i) else { continue };
        if child.kind() == "enumerator_list" {
            for j in 0..child.child_count() as u32 {
                let Some(enumerator) = child.child(j) else {
                    continue;
                };
                if enumerator.kind() == "enumerator" {
                    if let Some(ident) = first_named_child(&enumerator) {
                        push_node(&ident, TT_ENUM_MEMBER, out);
                    }
                }
            }
        } else if child.kind() == "enumerator" {
            if let Some(ident) = first_named_child(&child) {
                push_node(&ident, TT_ENUM_MEMBER, out);
            }
        }
    }
}

fn push_node(node: &tree_sitter::Node, token_type: u32, out: &mut Vec<(u32, u32, u32, u32)>) {
    let start = node.start_position();
    let end = node.end_position();
    if start.row != end.row {
        return;
    }
    let len = end.column.saturating_sub(start.column);
    if len == 0 {
        return;
    }
    out.push((
        start.row as u32,
        start.column as u32,
        len as u32,
        token_type,
    ));
}

fn find_child_kind<'a>(node: &'a tree_sitter::Node, kind: &str) -> Option<tree_sitter::Node<'a>> {
    (0..node.child_count() as u32)
        .filter_map(|i| node.child(i))
        .find(|c| c.kind() == kind)
}

fn first_named_child<'a>(node: &'a tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
    (0..node.child_count() as u32)
        .filter_map(|i| node.child(i))
        .find(tree_sitter::Node::is_named)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscript_parser::parse::parse;

    fn token_types_in(src: &str) -> Vec<u32> {
        let doc = parse(src).unwrap();
        semantic_tokens(&doc)
            .data
            .iter()
            .map(|t| t.token_type)
            .collect()
    }

    #[test]
    fn function_name_gets_function_token() {
        let src = "func my_func():\n\tpass\n";
        assert!(token_types_in(src).contains(&TT_FUNCTION));
    }

    #[test]
    fn parameter_gets_parameter_token() {
        let src = "func foo(x: int):\n\tpass\n";
        assert!(token_types_in(src).contains(&TT_PARAMETER));
    }

    #[test]
    fn decorator_gets_decorator_token() {
        let src = "@export\nvar x: int = 0\n";
        assert!(token_types_in(src).contains(&TT_DECORATOR));
    }

    #[test]
    fn pi_constant_gets_enum_member_token() {
        let src = "var x = PI\n";
        assert!(token_types_in(src).contains(&TT_ENUM_MEMBER));
    }

    #[test]
    fn enum_member_gets_enum_member_token() {
        let src = "enum Dir { UP, DOWN }\n";
        assert!(token_types_in(src).contains(&TT_ENUM_MEMBER));
    }

    #[test]
    fn variable_gets_variable_token() {
        let src = "var my_var: int = 0\n";
        assert!(token_types_in(src).contains(&TT_VARIABLE));
    }

    #[test]
    fn lambda_parameter_gets_parameter_token() {
        // LAB-694: lambda `func(x: int): ...` — x should get a parameter token.
        let src = "class_name T\nfunc _ready():\n\tvar f = func(x: int): return x\n";
        assert!(
            token_types_in(src).contains(&TT_PARAMETER),
            "lambda parameter should emit TT_PARAMETER"
        );
    }

    #[test]
    fn multi_line_delta_encoding_is_correct() {
        let src = "func foo():\n\tpass\nfunc bar():\n\tpass\n";
        let doc = parse(src).unwrap();
        let tokens = semantic_tokens(&doc).data;
        let func_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TT_FUNCTION)
            .collect();
        assert!(func_tokens.len() >= 2);
        assert_eq!(func_tokens[0].delta_line, 0);
        assert!(
            func_tokens[1].delta_line > 0,
            "second func should have non-zero deltaLine"
        );
    }
}
