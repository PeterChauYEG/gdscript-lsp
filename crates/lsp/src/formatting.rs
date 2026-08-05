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
                        start: Position {
                            line: old_line,
                            character: 0,
                        },
                        end: Position {
                            line: old_line + 1,
                            character: 0,
                        },
                    },
                    new_text: String::new(),
                });
                old_line += 1;
            }
            ChangeTag::Insert => {
                // Insert before the current old_line position
                edits.push(TextEdit {
                    range: Range {
                        start: Position {
                            line: old_line,
                            character: 0,
                        },
                        end: Position {
                            line: old_line,
                            character: 0,
                        },
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

    // --- format_document async path ---

    #[tokio::test]
    async fn binary_not_found_returns_none() {
        let result = format_document("var x = 1\n", "/nonexistent/gdformat").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn binary_exits_nonzero_returns_none() {
        // /bin/false always exits with code 1.
        let result = format_document("var x = 1\n", "/bin/false").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn binary_outputs_invalid_utf8_returns_none() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake_gdformat.py");
        std::fs::write(
            &script,
            "#!/usr/bin/env python3\nimport sys\nsys.stdout.buffer.write(b'\\xff\\xfe')\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let result = format_document("var x = 1\n", script.to_str().unwrap()).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn binary_echoes_input_returns_empty_edits() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake_gdformat.sh");
        // cat reads stdin and writes it back unchanged.
        std::fs::write(&script, "#!/bin/sh\ncat\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let src = "var x = 1\n";
        let result = format_document(src, script.to_str().unwrap()).await;
        assert_eq!(result, Some(vec![]));
    }
}
