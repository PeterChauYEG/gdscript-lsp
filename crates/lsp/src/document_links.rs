use gdscript_parser::ParsedDocument;
use tower_lsp::lsp_types::{DocumentLink, Position, Range, Url};

/// Find all `res://` string literals in a `GDScript` document and return them
/// as [`DocumentLink`]s that resolve to absolute filesystem paths.
///
/// Only emits links for paths that exist on disk.
#[must_use]
pub fn document_links(doc: &ParsedDocument, workspace_root: &std::path::Path) -> Vec<DocumentLink> {
    let source = doc.source.as_bytes();
    let root = doc.tree.root_node();
    let mut out = Vec::new();
    collect_links(&root, source, workspace_root, &mut out);
    out
}

fn collect_links(
    node: &tree_sitter::Node,
    source: &[u8],
    workspace_root: &std::path::Path,
    out: &mut Vec<DocumentLink>,
) {
    if node.kind() == "string" {
        let start = node.start_position();
        let end = node.end_position();

        if let Ok(raw) = node.utf8_text(source) {
            // Strip surrounding quotes.
            let inner = raw.trim_matches('"').trim_matches('\'');
            if let Some(rel) = inner.strip_prefix("res://") {
                let abs = workspace_root.join(rel);
                if abs.exists() {
                    if let Ok(target) = Url::from_file_path(&abs) {
                        out.push(DocumentLink {
                            range: Range {
                                start: Position {
                                    line: start.row as u32,
                                    character: start.column as u32,
                                },
                                end: Position {
                                    line: end.row as u32,
                                    character: end.column as u32,
                                },
                            },
                            target: Some(target),
                            tooltip: None,
                            data: None,
                        });
                    }
                }
            }
        }
        return; // Don't recurse into string children.
    }

    for i in 0..node.child_count() as u32 {
        let Some(child) = node.child(i) else { continue };
        collect_links(&child, source, workspace_root, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscript_parser::parse::parse;
    use std::fs;

    #[test]
    fn res_path_that_exists_produces_link() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("scene.tscn");
        fs::write(&file, "").unwrap();

        let src = "const PATH = \"res://scene.tscn\"\n";
        let doc = parse(src).unwrap();
        let links = document_links(&doc, dir.path());
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn res_path_nonexistent_no_link() {
        let dir = tempfile::tempdir().unwrap();
        let src = "const PATH = \"res://missing.tscn\"\n";
        let doc = parse(src).unwrap();
        let links = document_links(&doc, dir.path());
        assert!(links.is_empty());
    }

    #[test]
    fn non_res_string_no_link() {
        let dir = tempfile::tempdir().unwrap();
        let src = "const MSG = \"hello world\"\n";
        let doc = parse(src).unwrap();
        let links = document_links(&doc, dir.path());
        assert!(links.is_empty());
    }

    #[test]
    fn multiple_res_paths() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.tscn"), "").unwrap();
        fs::write(dir.path().join("b.tscn"), "").unwrap();

        let src = "const A = \"res://a.tscn\"\nconst B = \"res://b.tscn\"\n";
        let doc = parse(src).unwrap();
        let links = document_links(&doc, dir.path());
        assert_eq!(links.len(), 2);
    }
}
