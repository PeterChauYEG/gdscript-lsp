use gdscript_parser::ParsedDocument;

use crate::diagnostics::{Diagnostic, Severity};

/// Walk the tree-sitter parse tree and return one diagnostic per ERROR or MISSING node.
#[must_use]
pub fn syntax_errors(doc: &ParsedDocument) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut cursor = doc.tree.walk();
    collect_errors(&mut cursor, &mut out);
    out
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn collect_errors(cursor: &mut tree_sitter::TreeCursor<'_>, out: &mut Vec<Diagnostic>) {
    let node = cursor.node();

    if node.is_error() {
        let start = node.start_position();
        let end = node.end_position();
        out.push(Diagnostic {
            line: to_u32(start.row),
            col: to_u32(start.column),
            end_line: to_u32(end.row),
            end_col: to_u32(end.column),
            severity: Severity::Error,
            code: Some("E0001".to_owned()),
            message: "Syntax error".to_owned(),
        });
        // Don't descend into error nodes — children are just recovery fragments.
        return;
    }

    if node.is_missing() {
        let pos = node.start_position();
        out.push(Diagnostic {
            line: to_u32(pos.row),
            col: to_u32(pos.column),
            end_line: to_u32(pos.row),
            end_col: to_u32(pos.column),
            severity: Severity::Error,
            code: Some("E0002".to_owned()),
            message: format!("Missing `{}`", node.kind()),
        });
    }

    if cursor.goto_first_child() {
        loop {
            collect_errors(cursor, out);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

#[cfg(test)]
mod tests {
    use gdscript_parser::parse::parse;

    use super::*;

    #[test]
    fn valid_script_has_no_errors() {
        let src = "extends Node\n\nfunc _ready():\n\tpass\n";
        let doc = parse(src).unwrap();
        assert!(syntax_errors(&doc).is_empty());
    }

    #[test]
    fn broken_script_reports_error() {
        let src = "func (\n";
        let doc = parse(src).unwrap();
        assert!(!syntax_errors(&doc).is_empty());
    }

    #[test]
    fn error_position_is_accurate() {
        // The error is on the first line (row 0).
        let src = "func (\n";
        let doc = parse(src).unwrap();
        let errors = syntax_errors(&doc);
        assert!(!errors.is_empty());
        assert_eq!(errors[0].line, 0);
    }
}
