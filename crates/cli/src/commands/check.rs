use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use tower_lsp::lsp_types::NumberOrString;

#[derive(Args)]
pub struct CheckArgs {
    /// Path to a file or directory to check
    pub path: PathBuf,

    /// Output format
    #[arg(long, default_value = "text", value_parser = ["text", "json", "github"])]
    pub format: String,

    /// Promote warnings to errors and enable stricter rules (untyped params/returns/vars)
    #[arg(long)]
    pub strict: bool,
}

#[derive(serde::Serialize)]
struct JsonDiag {
    file: String,
    line: u32,
    col: u32,
    severity: String,
    code: String,
    message: String,
}

pub fn run(args: CheckArgs) -> Result<()> {
    let api_db = gdscript_api_db::ApiDb::bundled().map_err(|e| anyhow::anyhow!("{e}"))?;
    let files = collect_gd_files(&args.path);

    let mut all_diags: Vec<JsonDiag> = Vec::new();
    let mut has_error = false;

    for file in &files {
        let source = std::fs::read_to_string(file)?;
        let doc = match gdscript_parser::parse::parse(&source) {
            Ok(d) => d,
            Err(_) => {
                eprintln!("failed to parse {}", file.display());
                has_error = true;
                continue;
            }
        };

        let type_map = gdscript_lsp::type_resolver::extract_types(&doc);

        // Collect checker-crate diagnostics (syntax + lint) and convert to LSP
        let checker_diags = {
            use gdscript_checker::diagnostics::Severity;
            use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
            let mut raw = gdscript_checker::syntax::syntax_errors(&doc);
            raw.extend(gdscript_checker::linting::lint(&doc));
            raw.into_iter().map(|d| Diagnostic {
                range: Range {
                    start: Position { line: d.line, character: d.col },
                    end: Position { line: d.end_line, character: d.end_col },
                },
                severity: Some(match d.severity {
                    Severity::Error => DiagnosticSeverity::ERROR,
                    Severity::Warning => DiagnosticSeverity::WARNING,
                    Severity::Hint => DiagnosticSeverity::HINT,
                }),
                code: d.code.map(NumberOrString::String),
                message: d.message,
                source: Some("gdscript-lsp".to_owned()),
                ..Default::default()
            }).collect::<Vec<_>>()
        };

        let mut diags = checker_diags;
        diags.extend(gdscript_lsp::type_check::check_type_mismatches(&doc, &api_db));
        diags.extend(gdscript_lsp::call_checker::check_calls(&doc, &type_map, &api_db));

        if args.strict {
            diags.extend(strict_checks(&doc));
        }

        for d in &diags {
            let sev = match d.severity {
                Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR) => "error",
                Some(tower_lsp::lsp_types::DiagnosticSeverity::WARNING) => "warning",
                _ => "info",
            };
            if sev == "error" || (args.strict && sev == "warning") {
                has_error = true;
            }
            let code = match &d.code {
                Some(NumberOrString::String(s)) => s.clone(),
                Some(NumberOrString::Number(n)) => n.to_string(),
                None => String::new(),
            };
            all_diags.push(JsonDiag {
                file: file.display().to_string(),
                line: d.range.start.line + 1,
                col: d.range.start.character + 1,
                severity: sev.to_owned(),
                code,
                message: d.message.clone(),
            });
        }
    }

    match args.format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&all_diags)?),
        "github" => {
            for d in &all_diags {
                let level = if d.severity == "error" { "error" } else { "warning" };
                println!(
                    "::{level} file={},line={},col={}::{}",
                    d.file, d.line, d.col, d.message
                );
            }
        }
        _ => {
            for d in &all_diags {
                println!("{}:{}:{}: {}: {}", d.file, d.line, d.col, d.severity, d.message);
            }
        }
    }

    if has_error {
        std::process::exit(1);
    }
    Ok(())
}

fn collect_gd_files(path: &std::path::Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_owned()];
    }
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                files.extend(collect_gd_files(&p));
            } else if p.extension().and_then(|e| e.to_str()) == Some("gd") {
                files.push(p);
            }
        }
    }
    files
}

/// Strict-mode checks: warn on untyped variable declarations, function
/// parameters without type annotations, and functions missing return types.
fn strict_checks(doc: &gdscript_parser::ParsedDocument) -> Vec<tower_lsp::lsp_types::Diagnostic> {
    use gdscript_lsp::type_util::{error_diag, node_range};
    use tower_lsp::lsp_types::DiagnosticSeverity;

    let mut out = Vec::new();
    let source = doc.source.as_bytes();
    let root = doc.tree.root_node();

    for i in 0..root.child_count() {
        let Some(node) = root.child(i) else { continue };
        match node.kind() {
            "variable_statement" => {
                let has_type = (0..node.child_count())
                    .filter_map(|j| node.child(j))
                    .any(|n| n.kind() == "type");
                if !has_type {
                    let mut d = error_diag(
                        node_range(&node),
                        "W0010",
                        "untyped variable declaration".to_owned(),
                    );
                    d.severity = Some(DiagnosticSeverity::WARNING);
                    out.push(d);
                }
            }
            "function_definition" => {
                let has_return_type = (0..node.child_count())
                    .filter_map(|j| node.child(j))
                    .any(|n| n.kind() == "->");
                if !has_return_type {
                    let mut d = error_diag(
                        node_range(&node),
                        "W0011",
                        "function missing return type annotation".to_owned(),
                    );
                    d.severity = Some(DiagnosticSeverity::WARNING);
                    out.push(d);
                }
                for j in 0..node.child_count() {
                    let Some(params_node) = node.child(j) else { continue };
                    if params_node.kind() != "parameters" { continue }
                    // Parameters node contains: `(`, identifier/typed_parameter, `,`, `)`
                    for k in 0..params_node.child_count() {
                        let Some(param) = params_node.child(k) else { continue };
                        match param.kind() {
                            "identifier" => {
                                // Bare untyped parameter
                                let name = param.utf8_text(source).unwrap_or("_");
                                let mut d = error_diag(
                                    node_range(&param),
                                    "W0012",
                                    format!("parameter `{name}` missing type annotation"),
                                );
                                d.severity = Some(DiagnosticSeverity::WARNING);
                                out.push(d);
                            }
                            "typed_parameter" => {
                                // `name: Type` — already typed, skip
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> gdscript_parser::ParsedDocument {
        gdscript_parser::parse::parse(src).expect("parse failed")
    }

    #[test]
    fn strict_warns_on_untyped_var() {
        let doc = parse("var x = 1\n");
        let diags = strict_checks(&doc);
        assert!(diags.iter().any(|d| d.message.contains("untyped variable")));
    }

    #[test]
    fn strict_no_warn_on_typed_var() {
        let doc = parse("var x: int = 1\n");
        let diags = strict_checks(&doc);
        assert!(!diags.iter().any(|d| d.message.contains("untyped variable")));
    }

    #[test]
    fn strict_warns_on_missing_return_type() {
        let doc = parse("func foo():\n\tpass\n");
        let diags = strict_checks(&doc);
        assert!(diags.iter().any(|d| d.message.contains("return type")));
    }

    #[test]
    fn strict_no_warn_when_return_type_present() {
        let doc = parse("func foo() -> void:\n\tpass\n");
        let diags = strict_checks(&doc);
        assert!(!diags.iter().any(|d| d.message.contains("return type")));
    }

    #[test]
    fn strict_warns_on_untyped_param() {
        let doc = parse("func foo(x):\n\tpass\n");
        let diags = strict_checks(&doc);
        assert!(diags.iter().any(|d| d.message.contains("parameter")));
    }

    #[test]
    fn strict_no_warn_on_typed_param() {
        let doc = parse("func foo(x: int) -> void:\n\tpass\n");
        let diags = strict_checks(&doc);
        assert!(!diags.iter().any(|d| d.message.contains("parameter")));
    }

    #[test]
    fn collect_gd_files_single_file() {
        let tmp = std::env::temp_dir().join("test_collect.gd");
        std::fs::write(&tmp, "").unwrap();
        let files = collect_gd_files(&tmp);
        assert_eq!(files.len(), 1);
        std::fs::remove_file(tmp).ok();
    }
}
