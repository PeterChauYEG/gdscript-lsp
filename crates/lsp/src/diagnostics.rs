use gdscript_checker::diagnostics::Severity;
use gdscript_parser::parse::parse;
use tower_lsp::Client;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range, Url};

/// Parse `source`, extract syntax errors + any extra diagnostics, and publish.
pub async fn publish_diagnostics(
    client: &Client,
    uri: Url,
    version: i32,
    source: &str,
    extra: Vec<Diagnostic>,
) {
    let mut diags: Vec<Diagnostic> = match parse(source) {
        Ok(doc) => {
            let errors = gdscript_checker::syntax::syntax_errors(&doc);
            let warnings = gdscript_checker::linting::lint(&doc);
            errors
                .into_iter()
                .chain(warnings)
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
