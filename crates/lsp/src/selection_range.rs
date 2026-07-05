use gdscript_parser::ParsedDocument;
use tower_lsp::lsp_types::{Position, Range, SelectionRange};

/// Build nested selection ranges for each requested position.
///
/// For each position, finds the tightest AST node containing it, then builds
/// a chain of parent ranges up to the root.
#[must_use]
pub fn selection_ranges(doc: &ParsedDocument, positions: &[Position]) -> Vec<SelectionRange> {
    let root = doc.tree.root_node();
    positions.iter().map(|pos| range_at(&root, pos)).collect()
}

fn range_at(root: &tree_sitter::Node, pos: &Position) -> SelectionRange {
    let ts_point = tree_sitter::Point {
        row: pos.line as usize,
        column: pos.character as usize,
    };

    // Descend to the leaf node at the position.
    let mut node = root.descendant_for_point_range(ts_point, ts_point)
        .unwrap_or(*root);

    // Walk up, building the nested chain.
    let mut chain: Vec<Range> = Vec::new();
    loop {
        chain.push(ts_range_to_lsp(&node));
        match node.parent() {
            Some(parent) => node = parent,
            None => break,
        }
    }

    // Convert the chain (leaf-first) into nested SelectionRange (leaf innermost).
    chain.into_iter().rev().fold(None, |parent, range| {
        Some(SelectionRange { range, parent: parent.map(Box::new) })
    }).unwrap_or(SelectionRange {
        range: Range {
            start: Position { line: pos.line, character: pos.character },
            end: Position { line: pos.line, character: pos.character },
        },
        parent: None,
    })
}

fn ts_range_to_lsp(node: &tree_sitter::Node) -> Range {
    let start = node.start_position();
    let end = node.end_position();
    Range {
        start: Position { line: start.row as u32, character: start.column as u32 },
        end: Position { line: end.row as u32, character: end.column as u32 },
    }
}

#[cfg(test)]
mod tests {
    use gdscript_parser::parse::parse;
    use super::*;

    #[test]
    fn cursor_inside_identifier_returns_ranges() {
        let src = "var x = 1\n";
        let doc = parse(src).unwrap();
        let pos = Position { line: 0, character: 4 }; // inside 'x'
        let ranges = selection_ranges(&doc, &[pos]);
        assert_eq!(ranges.len(), 1);
        // Leaf range should be narrow (just the identifier).
        let sr = &ranges[0];
        assert!(sr.range.end.character > sr.range.start.character
            || sr.parent.is_some(),
            "expected a non-empty range or a parent");
    }

    #[test]
    fn multiple_positions_returns_correct_count() {
        let src = "var x = 1\nvar y = 2\n";
        let doc = parse(src).unwrap();
        let positions = vec![
            Position { line: 0, character: 4 },
            Position { line: 1, character: 4 },
        ];
        let ranges = selection_ranges(&doc, &positions);
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn has_parent_ranges() {
        let src = "func foo():\n\tvar x = 1\n";
        let doc = parse(src).unwrap();
        let pos = Position { line: 1, character: 5 }; // inside 'x'
        let ranges = selection_ranges(&doc, &[pos]);
        // The deepest range should have a parent (enclosing expression/statement).
        assert!(ranges[0].parent.is_some());
    }
}
