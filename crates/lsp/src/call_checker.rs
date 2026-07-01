use gdscript_api_db::ApiDb;
use gdscript_parser::ParsedDocument;
use tower_lsp::lsp_types::Diagnostic;

use crate::type_resolver::TypeMap;
use crate::type_util::{error_diag, infer_literal_type, node_range, types_compatible};

/// Check all engine method calls in a document for argument count/type errors.
#[must_use]
pub fn check_calls(doc: &ParsedDocument, type_map: &TypeMap, api_db: &ApiDb) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let source = doc.source.as_bytes();
    let root = doc.tree.root_node();

    walk(&root, source, type_map, api_db, &mut diags);
    diags
}

fn walk(
    node: &tree_sitter::Node,
    source: &[u8],
    type_map: &TypeMap,
    api_db: &ApiDb,
    out: &mut Vec<Diagnostic>,
) {
    match node.kind() {
        "attribute" => {
            check_attribute_call(node, source, type_map, api_db, out);
        }
        "call" => {
            check_bare_call(node, source, type_map, api_db, out);
        }
        _ => {}
    }

    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else { continue };
        walk(&child, source, type_map, api_db, out);
    }
}

/// Check `receiver.method(args)` calls.
fn check_attribute_call(
    node: &tree_sitter::Node,
    source: &[u8],
    type_map: &TypeMap,
    api_db: &ApiDb,
    out: &mut Vec<Diagnostic>,
) {
    // Children: identifier(receiver) . attribute_call
    let mut receiver_name: Option<&str> = None;
    let mut call_node: Option<tree_sitter::Node> = None;

    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else { continue };
        match child.kind() {
            "identifier" if receiver_name.is_none() => {
                receiver_name = child.utf8_text(source).ok();
            }
            "attribute_call" => {
                call_node = Some(child);
            }
            _ => {}
        }
    }

    let (Some(receiver), Some(call)) = (receiver_name, call_node) else {
        return;
    };

    let type_name = type_map
        .resolve(receiver)
        .or_else(|| api_db.get_class(receiver).map(|c| c.name.as_str()));
    let Some(type_name) = type_name else { return };

    let mut method_name: Option<&str> = None;
    let mut args_node: Option<tree_sitter::Node> = None;

    for i in 0..call.child_count() {
        let Some(child) = call.child(i) else { continue };
        match child.kind() {
            "identifier" => method_name = child.utf8_text(source).ok(),
            "arguments" => args_node = Some(child),
            _ => {}
        }
    }

    let (Some(method_name), Some(args_node)) = (method_name, args_node) else {
        return;
    };

    check_args(type_name, method_name, &args_node, source, api_db, out);
}

/// Check bare `method(args)` calls using the script's self type.
fn check_bare_call(
    node: &tree_sitter::Node,
    source: &[u8],
    type_map: &TypeMap,
    api_db: &ApiDb,
    out: &mut Vec<Diagnostic>,
) {
    let self_type = match type_map.self_type.as_deref() {
        Some(t) => t,
        None => return,
    };

    let mut method_name: Option<&str> = None;
    let mut args_node: Option<tree_sitter::Node> = None;

    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else { continue };
        match child.kind() {
            "identifier" => method_name = child.utf8_text(source).ok(),
            "arguments" => args_node = Some(child),
            _ => {}
        }
    }

    let (Some(method_name), Some(args_node)) = (method_name, args_node) else {
        return;
    };

    check_args(self_type, method_name, &args_node, source, api_db, out);
}

fn check_args(
    type_name: &str,
    method_name: &str,
    args_node: &tree_sitter::Node,
    _source: &[u8],
    api_db: &ApiDb,
    out: &mut Vec<Diagnostic>,
) {
    let chain = api_db.inheritance_chain(type_name);
    let method = chain.iter().find_map(|cls| {
        api_db
            .get_class(cls)
            .and_then(|c| c.methods.iter().find(|m| m.name == method_name))
    });
    let Some(method) = method else { return };

    // Collect actual argument nodes (skip punctuation).
    let arg_nodes: Vec<tree_sitter::Node> = (0..args_node.child_count())
        .filter_map(|i| args_node.child(i))
        .filter(|n| n.is_named())
        .collect();

    let expected = method.arguments.len();
    let got = arg_nodes.len();

    // Count required params (those without defaults).
    let required = method
        .arguments
        .iter()
        .filter(|a| a.default_value.is_none())
        .count();

    if !method.is_vararg && (got < required || got > expected) {
        let range = node_range(args_node);
        let msg = if required == expected {
            format!(
                "`{}` expects {} argument{}, got {}",
                method_name,
                expected,
                if expected == 1 { "" } else { "s" },
                got
            )
        } else {
            format!(
                "`{}` expects {}-{} arguments, got {}",
                method_name, required, expected, got
            )
        };
        out.push(diag(range, msg));
        return;
    }

    // Type-check arguments where we can infer types from literals.
    for (i, (arg_node, param)) in arg_nodes.iter().zip(method.arguments.iter()).enumerate() {
        let inferred = infer_literal_type(arg_node);
        let Some(inferred) = inferred else { continue };

        if !types_compatible(&param.type_name, inferred, api_db) {
            let range = node_range(arg_node);
            out.push(diag(
                range,
                format!(
                    "argument {} `{}`: expected `{}`, got `{}`",
                    i + 1,
                    param.name,
                    param.type_name,
                    inferred
                ),
            ));
        }
    }
}

fn diag(range: tower_lsp::lsp_types::Range, message: String) -> Diagnostic {
    error_diag(range, "E0002", message)
}

#[cfg(test)]
mod tests {
    use gdscript_api_db::ApiDb;
    use gdscript_parser::parse::parse;
    use super::*;
    use crate::type_resolver::extract_types;

    fn db() -> ApiDb { ApiDb::bundled().unwrap() }

    fn diags(src: &str) -> Vec<Diagnostic> {
        let db = db();
        let doc = parse(src).unwrap();
        let type_map = extract_types(&doc);
        check_calls(&doc, &type_map, &db)
    }

    #[test]
    fn no_diag_for_correct_call() {
        let src = "extends Node2D\nvar n: Node\nfunc _ready():\n\tadd_child(n)\n";
        assert!(diags(src).is_empty());
    }

    #[test]
    fn wrong_arg_count_too_few() {
        let src = "extends Node2D\nfunc _ready():\n\tadd_child()\n";
        let d = diags(src);
        assert!(!d.is_empty());
        assert!(d[0].message.contains("expects"));
    }

    #[test]
    fn wrong_arg_count_on_receiver() {
        let src = "extends Node\nvar n: Node2D\nfunc _ready():\n\tn.add_child()\n";
        let d = diags(src);
        assert!(!d.is_empty());
        assert!(d[0].message.contains("add_child"));
    }

    #[test]
    fn wrong_literal_type_flagged() {
        let src = "extends Node2D\nvar n: Node2D\nfunc _ready():\n\tn.add_child(42)\n";
        let d = diags(src);
        assert!(!d.is_empty());
        assert!(d[0].message.contains("int"));
    }

    #[test]
    fn correct_literal_no_diag() {
        // set_visible(bool) — passing true is fine
        let src = "extends Node\nvar s: Sprite2D\nfunc _ready():\n\ts.set_visible(true)\n";
        assert!(diags(src).is_empty());
    }

    #[test]
    fn inherited_method_checked() {
        // add_child is on Node, called on a Node2D (subclass) receiver
        let src = "extends Node\nvar n: Node2D\nfunc _ready():\n\tn.add_child(42)\n";
        let d = diags(src);
        assert!(!d.is_empty());
    }

    #[test]
    fn vararg_method_not_flagged() {
        // print() is vararg — any number of args is fine
        let src = "extends Node\nfunc _ready():\n\tprint(1, 2, 3, 4)\n";
        assert!(diags(src).is_empty());
    }
}
