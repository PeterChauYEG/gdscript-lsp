use std::collections::HashMap;

use gdscript_parser::ParsedDocument;

/// Annotation-based type map for a single GDScript file.
#[derive(Debug, Default)]
pub struct TypeMap {
    pub types: HashMap<String, String>,
    pub self_type: Option<String>,
}

impl TypeMap {
    #[must_use]
    pub fn resolve<'a>(&'a self, name: &str) -> Option<&'a str> {
        if name == "self" {
            return self.self_type.as_deref();
        }
        self.types.get(name).map(String::as_str)
    }
}

/// Extract explicit type annotations from a parsed GDScript document.
#[must_use]
pub fn extract_types(doc: &ParsedDocument) -> TypeMap {
    let mut map = TypeMap::default();
    let source = doc.source.as_bytes();
    let root = doc.tree.root_node();

    for i in 0..root.child_count() {
        let Some(node) = root.child(i) else { continue };
        match node.kind() {
            "extends_statement" => {
                // The type node is the first named child (no field name in the grammar).
                for i in 0..node.child_count() {
                    let Some(child) = node.child(i) else { continue };
                    if child.kind() == "type" {
                        if let Some(name) = type_ident(&child, source) {
                            map.self_type = Some(name.to_owned());
                        }
                        break;
                    }
                }
            }
            "variable_statement" | "const_statement" => {
                extract_var_type(&node, source, &mut map.types);
            }
            "function_definition" => {
                extract_func_types(&node, source, &mut map.types);
            }
            _ => {}
        }
    }

    map
}

fn extract_var_type(node: &tree_sitter::Node, source: &[u8], out: &mut HashMap<String, String>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let Ok(name) = name_node.utf8_text(source) else {
        return;
    };
    if let Some(type_node) = node.child_by_field_name("type") {
        if let Some(type_name) = type_ident(&type_node, source) {
            out.insert(name.to_owned(), type_name.to_owned());
        }
    }
}

fn extract_func_types(func_node: &tree_sitter::Node, source: &[u8], out: &mut HashMap<String, String>) {
    for i in 0..func_node.child_count() {
        let Some(child) = func_node.child(i) else { continue };
        if child.kind() == "parameters" {
            for j in 0..child.child_count() {
                let Some(param) = child.child(j) else { continue };
                if param.kind() == "typed_parameter" {
                    let mut ident: Option<String> = None;
                    let mut type_name: Option<String> = None;
                    for k in 0..param.child_count() {
                        let Some(p) = param.child(k) else { continue };
                        if p.kind() == "identifier" && ident.is_none() {
                            ident = p.utf8_text(source).ok().map(str::to_owned);
                        } else if p.kind() == "type" {
                            type_name = type_ident(&p, source).map(str::to_owned);
                        }
                    }
                    if let (Some(name), Some(ty)) = (ident, type_name) {
                        out.insert(name, ty);
                    }
                }
            }
        }
        if child.kind() == "body" {
            extract_body_var_types(&child, source, out);
        }
    }
}

fn extract_body_var_types(body: &tree_sitter::Node, source: &[u8], out: &mut HashMap<String, String>) {
    for i in 0..body.child_count() {
        let Some(child) = body.child(i) else { continue };
        if child.kind() == "variable_statement" {
            extract_var_type(&child, source, out);
        }
    }
}

fn type_ident<'a>(type_node: &tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    for i in 0..type_node.child_count() {
        let Some(child) = type_node.child(i) else { continue };
        if child.is_named() {
            return child.utf8_text(source).ok();
        }
    }
    None
}

/// Resolve the Godot class name for a `$NodePath` expression.
///
/// `node_text` is the raw source text of the expression (e.g. `"$Sprite2D"` or
/// `"$UI/HealthBar"`). The leading `$` is stripped and the last path component
/// is used as the lookup key in `scene_map`, which maps node names to their
/// Godot class names. Returns `Some(class_name)` when found.
///
/// # LAB-696
#[must_use]
pub fn resolve_dollar_path(
    node_text: &str,
    scene_map: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let path = node_text.strip_prefix('$').unwrap_or(node_text);
    let simple = path.split('/').next_back().unwrap_or(path);
    scene_map.get(simple).cloned()
}

/// Resolve the narrowed type produced by an `expr as TypeName` cast expression.
///
/// Returns `Some(type_name)` when `node` is a `cast_expression` node and its
/// right-hand type child can be read from `source`. The returned string is the
/// bare identifier text of the target type (e.g. `"Sprite2D"`).
///
/// # LAB-708 / F-1
#[must_use]
pub fn resolve_as_cast(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    // tree-sitter-gdscript uses binary_operator for `expr as Type`, not cast_expression.
    if node.kind() != "binary_operator" && node.kind() != "cast_expression" {
        return None;
    }
    let mut found_as = false;
    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else { continue };
        if !found_as {
            if child.kind() == "as" {
                found_as = true;
            }
            continue;
        }
        // First node after "as" — could be a "type" wrapper or a bare identifier.
        if child.kind() == "type" {
            return type_ident(&child, source).map(str::to_owned);
        }
        if child.is_named() {
            return child.utf8_text(source).ok().map(str::to_owned);
        }
    }
    None
}

/// Resolve the type of a ternary `x if cond else y` expression.
///
/// Returns `Some(type_name)` only when both branches resolve to the same type
/// via `type_map`. When the branches differ or either is unknown, returns `None`.
///
/// # LAB-711 / F-4
#[must_use]
pub fn resolve_ternary_type(
    node: &tree_sitter::Node,
    source: &[u8],
    type_map: &TypeMap,
) -> Option<String> {
    if node.kind() != "if_expression" {
        return None;
    }
    // tree-sitter-gdscript grammar for ternary:
    //   if_expression: <value_if_true> "if" <condition> "else" <value_if_false>
    // Named children (is_named == true, non-keyword) in order: true_branch, cond, false_branch.
    let named_children: Vec<tree_sitter::Node> =
        (0..node.child_count())
            .filter_map(|i| node.child(i))
            .filter(|c| c.is_named())
            .collect();

    // We expect at least 3 named children: true_expr, condition, false_expr.
    if named_children.len() < 3 {
        return None;
    }

    let true_branch = &named_children[0];
    let false_branch = &named_children[2];

    let resolve_branch = |branch: &tree_sitter::Node| -> Option<String> {
        if branch.kind() == "identifier" {
            let name = branch.utf8_text(source).ok()?;
            return type_map.resolve(name).map(str::to_owned);
        }
        // For cast expressions nested in the branch, delegate.
        if branch.kind() == "cast_expression" {
            return resolve_as_cast(branch, source);
        }
        None
    };

    let true_ty = resolve_branch(true_branch)?;
    let false_ty = resolve_branch(false_branch)?;

    if true_ty == false_ty {
        Some(true_ty)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use gdscript_parser::parse::parse;
    use super::*;

    fn types(src: &str) -> TypeMap {
        let doc = parse(src).unwrap();
        extract_types(&doc)
    }

    #[test]
    fn extracts_self_type_from_extends() {
        let map = types("extends Node2D\n");
        assert_eq!(map.self_type.as_deref(), Some("Node2D"));
    }

    #[test]
    fn extracts_variable_annotation() {
        let map = types("extends Node\nvar target: Node2D\n");
        assert_eq!(map.resolve("target"), Some("Node2D"));
    }

    #[test]
    fn extracts_function_parameter_annotation() {
        let map = types("extends Node\nfunc foo(x: Sprite2D) -> void:\n\tpass\n");
        assert_eq!(map.resolve("x"), Some("Sprite2D"));
    }

    #[test]
    fn resolves_self_keyword() {
        let map = types("extends RigidBody2D\n");
        assert_eq!(map.resolve("self"), Some("RigidBody2D"));
    }

    #[test]
    fn unannotated_var_is_absent() {
        let map = types("extends Node\nvar x = 42\n");
        assert!(map.resolve("x").is_none());
    }

    #[test]
    fn extracts_local_var_in_function() {
        let map = types("extends Node\nfunc _ready():\n\tvar label: Label\n");
        assert_eq!(map.resolve("label"), Some("Label"));
    }

    // --- LAB-696: $NodePath type inference ---

    #[test]
    fn resolve_dollar_path_simple() {
        let mut scene_map = HashMap::new();
        scene_map.insert("Sprite2D".to_owned(), "Sprite2D".to_owned());
        assert_eq!(
            resolve_dollar_path("$Sprite2D", &scene_map),
            Some("Sprite2D".to_owned())
        );
    }

    #[test]
    fn resolve_dollar_path_nested() {
        let mut scene_map = HashMap::new();
        scene_map.insert("HealthBar".to_owned(), "ProgressBar".to_owned());
        assert_eq!(
            resolve_dollar_path("$UI/HealthBar", &scene_map),
            Some("ProgressBar".to_owned())
        );
    }

    #[test]
    fn resolve_dollar_path_missing_returns_none() {
        let scene_map: HashMap<String, String> = HashMap::new();
        assert!(resolve_dollar_path("$NonExistent", &scene_map).is_none());
    }

    #[test]
    fn resolve_dollar_path_no_dollar_prefix() {
        let mut scene_map = HashMap::new();
        scene_map.insert("Label".to_owned(), "Label".to_owned());
        // Should still work when the caller already stripped the `$`.
        assert_eq!(
            resolve_dollar_path("Label", &scene_map),
            Some("Label".to_owned())
        );
    }

    // --- LAB-708 / F-1: as cast type narrowing ---

    fn find_as_binary<'a>(node: tree_sitter::Node<'a>, source: &[u8]) -> Option<tree_sitter::Node<'a>> {
        if node.kind() == "binary_operator" || node.kind() == "cast_expression" {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "as" {
                        return Some(node);
                    }
                }
            }
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                if let Some(found) = find_as_binary(child, source) {
                    return Some(found);
                }
            }
        }
        None
    }

    #[test]
    fn resolve_as_cast_returns_target_type() {
        let src = "func _ready():\n\tvar x = node as Sprite2D\n";
        let doc = parse(src).unwrap();
        let source = doc.source.as_bytes();
        let cast_node = find_as_binary(doc.tree.root_node(), source)
            .expect("binary_operator with 'as' not found in AST");
        assert_eq!(resolve_as_cast(&cast_node, source), Some("Sprite2D".to_owned()));
    }

    #[test]
    fn resolve_as_cast_non_cast_returns_none() {
        let src = "var x: Node2D\n";
        let doc = parse(src).unwrap();
        let root = doc.tree.root_node();
        assert!(resolve_as_cast(&root, doc.source.as_bytes()).is_none());
    }
}
