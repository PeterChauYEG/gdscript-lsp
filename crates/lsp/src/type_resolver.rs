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
                if let Some(type_node) = node.child_by_field_name("type") {
                    if let Some(name) = type_ident(&type_node, source) {
                        map.self_type = Some(name.to_owned());
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
