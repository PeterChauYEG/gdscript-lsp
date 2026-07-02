use std::collections::HashSet;
use std::path::PathBuf;

use gdscript_checker::diagnostics::Severity;
use gdscript_parser::parse::parse;
use tower_lsp::Client;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range, Url};

/// Parse `source`, extract syntax errors + lint warnings + `extra` diagnostics, and publish.
///
/// `autoload_paths` — files registered as Godot autoloads. W0004 (missing
/// class_name) is suppressed for these: Godot treats autoload names as global
/// identifiers and refuses to compile scripts that re-declare them with class_name.
pub async fn publish_diagnostics(
    client: &Client,
    uri: Url,
    version: i32,
    source: &str,
    extra: Vec<Diagnostic>,
    autoload_paths: &HashSet<PathBuf>,
) {
    let file_path = uri.to_file_path().ok();
    let is_autoload = file_path
        .as_deref()
        .map(|p| autoload_paths.contains(p))
        .unwrap_or(false);

    let mut diags: Vec<Diagnostic> = match parse(source) {
        Ok(doc) => {
            let errors = gdscript_checker::syntax::syntax_errors(&doc);
            let warnings = gdscript_checker::linting::lint(&doc);
            errors
                .into_iter()
                .chain(warnings)
                .filter(|d| {
                    // Autoload scripts must NOT declare class_name — suppress W0004 for them.
                    !(is_autoload && d.code.as_deref() == Some("W0004"))
                })
                .map(|d| Diagnostic {
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
                })
                .collect()
        }
        Err(_) => vec![],
    };

    diags.extend(extra);
    client.publish_diagnostics(uri, diags, Some(version)).await;
}
