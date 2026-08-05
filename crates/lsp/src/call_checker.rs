use gdscript_api_db::ApiDb;
use gdscript_indexer::ProjectIndex;
use gdscript_parser::ParsedDocument;
use tower_lsp::lsp_types::Diagnostic;

use crate::type_resolver::TypeMap;
use crate::type_util::{error_diag, infer_literal_type, node_range, types_compatible};

/// Check all engine method calls and autoload member calls in a document for
/// argument count/type errors (engine API) and unknown-member errors (autoloads).
#[must_use]
pub fn check_calls(
    doc: &ParsedDocument,
    type_map: &TypeMap,
    api_db: &ApiDb,
    project_index: &ProjectIndex,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let source = doc.source.as_bytes();
    let root = doc.tree.root_node();

    walk(&root, source, type_map, api_db, project_index, &mut diags);
    diags
}

fn walk(
    node: &tree_sitter::Node,
    source: &[u8],
    type_map: &TypeMap,
    api_db: &ApiDb,
    project_index: &ProjectIndex,
    out: &mut Vec<Diagnostic>,
) {
    match node.kind() {
        "attribute" => {
            check_attribute_call(node, source, type_map, api_db, project_index, out);
        }
        "call" => {
            check_bare_call(node, source, type_map, api_db, out);
        }
        _ => {}
    }

    for i in 0..node.child_count() as u32 {
        let Some(child) = node.child(i) else { continue };
        walk(&child, source, type_map, api_db, project_index, out);
    }
}

/// Check `receiver.method(args)` calls against the engine API and, when the
/// receiver is an autoload singleton name, against its indexed file symbols.
fn check_attribute_call(
    node: &tree_sitter::Node,
    source: &[u8],
    type_map: &TypeMap,
    api_db: &ApiDb,
    project_index: &ProjectIndex,
    out: &mut Vec<Diagnostic>,
) {
    // Children: identifier(receiver) . attribute_call
    let mut receiver_name: Option<&str> = None;
    let mut call_node: Option<tree_sitter::Node> = None;

    for i in 0..node.child_count() as u32 {
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

    // --- Engine API path ---
    // Resolve receiver to an engine class type (via type_map variable lookup or
    // direct class-name lookup in the API db), then check the API db for the
    // method signature.
    let engine_type_name = type_map
        .resolve(receiver)
        .or_else(|| api_db.get_class(receiver).map(|c| c.name.as_str()))
        .and_then(|t| api_db.get_class(t).map(|c| c.name.as_str()));

    if let Some(type_name) = engine_type_name {
        let mut method_name: Option<&str> = None;
        let mut args_node: Option<tree_sitter::Node> = None;

        for i in 0..call.child_count() as u32 {
            let Some(child) = call.child(i) else { continue };
            match child.kind() {
                "identifier" => method_name = child.utf8_text(source).ok(),
                "arguments" => args_node = Some(child),
                _ => {}
            }
        }

        if let (Some(method_name), Some(args_node)) = (method_name, args_node) {
            check_args(type_name, method_name, &args_node, source, api_db, out);
        }
        return;
    }

    // --- Autoload path ---
    // When the receiver is a known autoload singleton name, verify that the
    // called member actually exists in that singleton's indexed file symbols.
    check_autoload_member(receiver, &call, source, project_index, out);
}

/// Verify that `method_name` (extracted from `call`) exists among the symbols
/// indexed for `receiver`'s autoload script.  Emits E0004 when absent.
fn check_autoload_member(
    receiver: &str,
    call: &tree_sitter::Node,
    source: &[u8],
    project_index: &ProjectIndex,
    out: &mut Vec<Diagnostic>,
) {
    let Some(path) = project_index.autoloads.get(receiver) else {
        return;
    };
    let Some(symbols) = project_index.file_symbols.get(path) else {
        return;
    };

    // Extract the method/property identifier from the attribute_call node.
    let mut method_node: Option<tree_sitter::Node> = None;
    for i in 0..call.child_count() as u32 {
        let Some(child) = call.child(i) else { continue };
        if child.kind() == "identifier" {
            method_node = Some(child);
            break;
        }
    }
    let Some(method_node) = method_node else {
        return;
    };
    let Ok(method_name) = method_node.utf8_text(source) else {
        return;
    };

    // A member is valid if any symbol in the autoload's file has the same name.
    // We match by name only regardless of SymbolKind: a variable holding a
    // Callable is a legitimate call target.  Kind-level checks are deferred.
    let member_exists = symbols.iter().any(|s| s.name == method_name);

    if !member_exists {
        out.push(error_diag(
            node_range(&method_node),
            "E0004",
            format!("`{receiver}` has no member `{method_name}`"),
        ));
    }
}

/// Check bare `method(args)` calls using the script's self type.
fn check_bare_call(
    node: &tree_sitter::Node,
    source: &[u8],
    type_map: &TypeMap,
    api_db: &ApiDb,
    out: &mut Vec<Diagnostic>,
) {
    let Some(self_type) = type_map.self_type.as_deref() else {
        return;
    };

    let mut method_name: Option<&str> = None;
    let mut args_node: Option<tree_sitter::Node> = None;

    for i in 0..node.child_count() as u32 {
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
    let arg_nodes: Vec<tree_sitter::Node> = (0..args_node.child_count() as u32)
        .filter_map(|i| args_node.child(i))
        .filter(tree_sitter::Node::is_named)
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
            format!("`{method_name}` expects {required}-{expected} arguments, got {got}")
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
    use super::*;
    use crate::type_resolver::extract_types;
    use gdscript_api_db::ApiDb;
    use gdscript_core::symbol::{SymbolDef, SymbolKind};
    use gdscript_indexer::ProjectIndex;
    use gdscript_parser::parse::parse;
    use std::path::PathBuf;

    fn db() -> ApiDb {
        ApiDb::bundled().unwrap()
    }

    fn diags(src: &str) -> Vec<Diagnostic> {
        let db = db();
        let doc = parse(src).unwrap();
        let type_map = extract_types(&doc);
        let index = ProjectIndex::new();
        check_calls(&doc, &type_map, &db, &index)
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

    // --- Autoload member checks ---

    fn make_index(autoload_name: &str, path: &str, symbols: Vec<SymbolDef>) -> ProjectIndex {
        let mut index = ProjectIndex::new();
        let pb = PathBuf::from(path);
        index.autoloads.insert(autoload_name.to_owned(), pb.clone());
        index.file_symbols.insert(pb, symbols);
        index
    }

    #[test]
    fn unknown_member_on_autoload_flagged() {
        let db = db();
        let src = "func _ready():\n\tEventBus.nonexistent_method()\n";
        let doc = parse(src).unwrap();
        let type_map = extract_types(&doc);
        // EventBus autoload exists but has no symbols at all.
        let index = make_index("EventBus", "/res/event_bus.gd", vec![]);
        let d = check_calls(&doc, &type_map, &db, &index);
        assert!(!d.is_empty(), "expected a diagnostic for unknown member");
        assert!(d[0].message.contains("nonexistent_method"));
        assert!(d[0].message.contains("EventBus"));
    }

    #[test]
    fn known_function_on_autoload_no_diag() {
        let db = db();
        let src = "func _ready():\n\tEventBus.emit_event()\n";
        let doc = parse(src).unwrap();
        let type_map = extract_types(&doc);
        let index = make_index(
            "EventBus",
            "/res/event_bus.gd",
            vec![SymbolDef {
                name: "emit_event".to_owned(),
                kind: SymbolKind::Function,
                line: 1,
                col: 0,
                type_annotation: None,
            }],
        );
        let d = check_calls(&doc, &type_map, &db, &index);
        assert!(d.is_empty(), "unexpected diagnostic: {d:?}");
    }

    #[test]
    fn unknown_receiver_not_flagged() {
        // A receiver that is neither an engine class nor an autoload should be
        // silently skipped (no false positives for user-defined class instances).
        let src = "extends Node\nvar x: SomeUserClass\nfunc _ready():\n\tx.do_something()\n";
        assert!(diags(src).is_empty());
    }

    #[test]
    fn autoload_not_in_index_not_flagged() {
        // If the autoload has no entry in file_symbols yet (e.g. still indexing),
        // we should not emit spurious diagnostics.
        let db = db();
        let src = "func _ready():\n\tEventBus.emit_event()\n";
        let doc = parse(src).unwrap();
        let type_map = extract_types(&doc);
        // Autoload registered but file_symbols not yet populated.
        let mut index = ProjectIndex::new();
        index
            .autoloads
            .insert("EventBus".to_owned(), PathBuf::from("/res/event_bus.gd"));
        // file_symbols intentionally left empty.
        let d = check_calls(&doc, &type_map, &db, &index);
        assert!(d.is_empty(), "should not flag when symbols not indexed yet");
    }
}
