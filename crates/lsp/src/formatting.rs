use similar::{ChangeTag, TextDiff};
use tower_lsp::lsp_types::{Position, Range, TextEdit};

/// Format `source` by shelling out to `gdformat`, then diff the result to
/// produce minimal `TextEdit`s. Returns `None` if gdformat is not found or
/// returns an error.
pub async fn format_document(source: &str, gdformat_path: &str) -> Option<Vec<TextEdit>> {
    let source_owned = source.to_owned();
    let path_owned = gdformat_path.to_owned();

    tokio::task::spawn_blocking(move || run_gdformat(&source_owned, &path_owned))
        .await
        .ok()
        .flatten()
}

fn run_gdformat(source: &str, gdformat_path: &str) -> Option<Vec<TextEdit>> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let mut child = Command::new(gdformat_path)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    child.stdin.take()?.write_all(source.as_bytes()).ok()?;

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }

    let formatted = String::from_utf8(output.stdout).ok()?;
    if formatted == source {
        return Some(vec![]);
    }

    Some(diff_to_edits(source, &formatted))
}

/// Convert a line-level diff between `old` and `new` into LSP `TextEdit`s.
fn diff_to_edits(old: &str, new: &str) -> Vec<TextEdit> {
    let diff = TextDiff::from_lines(old, new);
    let mut edits = Vec::new();
    let mut old_line: u32 = 0;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                old_line += 1;
            }
            ChangeTag::Delete => {
                // Remove this line
                edits.push(TextEdit {
                    range: Range {
                        start: Position { line: old_line, character: 0 },
                        end: Position { line: old_line + 1, character: 0 },
                    },
                    new_text: String::new(),
                });
                old_line += 1;
            }
            ChangeTag::Insert => {
                // Insert before the current old_line position
                edits.push(TextEdit {
                    range: Range {
                        start: Position { line: old_line, character: 0 },
                        end: Position { line: old_line, character: 0 },
                    },
                    new_text: change.value().to_owned(),
                });
            }
        }
    }

    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_change_returns_empty_edits() {
        let src = "var x = 1\n";
        let edits = diff_to_edits(src, src);
        assert!(edits.is_empty());
    }

    #[test]
    fn added_line_produces_insert_edit() {
        let old = "var x = 1\n";
        let new = "var x = 1\nvar y = 2\n";
        let edits = diff_to_edits(old, new);
        assert!(!edits.is_empty());
        assert!(edits.iter().any(|e| e.new_text.contains("var y")));
    }

    #[test]
    fn removed_line_produces_delete_edit() {
        let old = "var x = 1\nvar y = 2\n";
        let new = "var x = 1\n";
        let edits = diff_to_edits(old, new);
        assert!(!edits.is_empty());
        assert!(edits.iter().any(|e| e.new_text.is_empty()));
    }

    #[test]
    fn changed_line_produces_delete_then_insert() {
        let old = "var x=1\n";
        let new = "var x = 1\n";
        let edits = diff_to_edits(old, new);
        assert!(!edits.is_empty());
    }
}
