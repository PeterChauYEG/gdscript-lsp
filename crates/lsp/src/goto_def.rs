use gdscript_parser::ParsedDocument;
use tower_lsp::lsp_types::{Location, Position, Range, Url};

/// Node kinds that introduce a named definition in `GDScript`.
const DEFINITION_KINDS: &[&str] = &[
    "function_definition",
    "variable_statement",
    "const_statement",
    "signal_statement",
    "class_name_statement",
    "class_definition",
];

/// Search `doc` for a definition of `target` and return its `Location` if found.
#[must_use]
pub fn find_definition(doc: &ParsedDocument, uri: &Url, target: &str) -> Option<Location> {
    let src = doc.source.as_bytes();
    let mut cursor = doc.tree.walk();
    find_in_subtree(&mut cursor, src, uri, target)
}

fn find_in_subtree(
    cursor: &mut tree_sitter::TreeCursor<'_>,
    src: &[u8],
    uri: &Url,
    target: &str,
) -> Option<Location> {
    let node = cursor.node();

    if DEFINITION_KINDS.contains(&node.kind()) {
        if let Some(loc) = name_child_location(node, src, uri, target) {
            return Some(loc);
        }
    }

    if cursor.goto_first_child() {
        loop {
            if let Some(loc) = find_in_subtree(cursor, src, uri, target) {
                cursor.goto_parent();
                return Some(loc);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }

    None
}

/// Return the `Location` of the `name` child of `node` if its text equals `target`.
fn name_child_location(
    node: tree_sitter::Node<'_>,
    src: &[u8],
    uri: &Url,
    target: &str,
) -> Option<Location> {
    for i in 0..node.child_count() {
        let child = node.child(i)?;
        if child.kind() != "name" {
            continue;
        }
        let text = child.utf8_text(src).ok()?;
        if text != target {
            return None;
        }
        let start = child.start_position();
        let end = child.end_position();
        return Some(Location {
            uri: uri.clone(),
            range: Range {
                start: Position {
                    line: u32::try_from(start.row).unwrap_or(u32::MAX),
                    character: u32::try_from(start.column).unwrap_or(u32::MAX),
                },
                end: Position {
                    line: u32::try_from(end.row).unwrap_or(u32::MAX),
                    character: u32::try_from(end.column).unwrap_or(u32::MAX),
                },
            },
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use gdscript_parser::parse::parse;
    use tower_lsp::lsp_types::Url;

    use super::*;

    fn test_uri() -> Url {
        Url::parse("file:///test.gd").unwrap()
    }

    #[test]
    fn finds_function_definition() {
        let src = "func my_func():\n\tpass\n";
        let doc = parse(src).unwrap();
        let loc = find_definition(&doc, &test_uri(), "my_func").unwrap();
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, 5); // "func |my_func"
    }

    #[test]
    fn finds_variable_definition() {
        let src = "var my_var: int = 0\n";
        let doc = parse(src).unwrap();
        let loc = find_definition(&doc, &test_uri(), "my_var").unwrap();
        assert_eq!(loc.range.start.line, 0);
    }

    #[test]
    fn finds_const_definition() {
        let src = "const MY_CONST = 42\n";
        let doc = parse(src).unwrap();
        let loc = find_definition(&doc, &test_uri(), "MY_CONST").unwrap();
        assert_eq!(loc.range.start.line, 0);
    }

    #[test]
    fn finds_signal_definition() {
        let src = "signal my_signal(v)\n";
        let doc = parse(src).unwrap();
        let loc = find_definition(&doc, &test_uri(), "my_signal").unwrap();
        assert_eq!(loc.range.start.line, 0);
    }

    #[test]
    fn returns_none_for_unknown_symbol() {
        let src = "func my_func():\n\tpass\n";
        let doc = parse(src).unwrap();
        assert!(find_definition(&doc, &test_uri(), "does_not_exist").is_none());
    }

    #[test]
    fn finds_nested_function() {
        let src = "func outer():\n\tpass\n\nfunc inner():\n\tpass\n";
        let doc = parse(src).unwrap();
        let loc = find_definition(&doc, &test_uri(), "inner").unwrap();
        assert_eq!(loc.range.start.line, 3);
    }
}
