//! Regression guard for the repo's comment policy within `crates/`.
//!
//! Only two kinds of comments are allowed: a doc block at the top of a
//! script/module and a doc block directly above a function/method (both
//! written with `///` or `//!`). Everything else — inline comments, block
//! comments, commented-out code, TODO/FIXME notes — is disallowed.

use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs")
            && path.file_name().and_then(|n| n.to_str()) != Some("comment_policy.rs")
        {
            out.push(path);
        }
    }
}

/// Returns the byte offset of a disallowed comment marker on `line`, or
/// `None` if the line contains no comment or only an allowed doc comment.
///
/// Treats `//` preceded by `:` (e.g. inside a `"res://..."` string literal)
/// as not a comment, and `///`/`//!` as allowed doc-comment markers.
fn disallowed_comment_offset(line: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(idx) = line[search_from..].find("//") {
        let idx = search_from + idx;
        if idx > 0 && line.as_bytes()[idx - 1] == b':' {
            search_from = idx + 2;
            continue;
        }
        let rest = &line[idx..];
        if rest.starts_with("///") || rest.starts_with("//!") {
            return None;
        }
        return Some(idx);
    }
    None
}

#[test]
fn detects_inline_comment() {
    assert_eq!(disallowed_comment_offset("let x = 1; // inline"), Some(11));
}

#[test]
fn detects_todo_comment() {
    assert_eq!(disallowed_comment_offset("// TODO: fix this"), Some(0));
}

#[test]
fn allows_outer_doc_comment() {
    assert_eq!(
        disallowed_comment_offset("/// Doc for the next item."),
        None
    );
}

#[test]
fn allows_module_doc_comment() {
    assert_eq!(disallowed_comment_offset("//! Module-level doc."), None);
}

#[test]
fn allows_res_url_literal() {
    assert_eq!(
        disallowed_comment_offset("let path = \"res://player.gd\";"),
        None
    );
}

#[test]
fn crates_contain_only_doc_comments() {
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli crate has a parent directory");

    let mut files = Vec::new();
    collect_rs_files(crates_dir, &mut files);
    assert!(
        !files.is_empty(),
        "expected to find .rs files under crates/"
    );

    let mut violations = Vec::new();
    for path in files {
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (lineno, line) in contents.lines().enumerate() {
            if disallowed_comment_offset(line).is_some() {
                violations.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    lineno + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "found disallowed non-doc comments:\n{}",
        violations.join("\n")
    );
}
