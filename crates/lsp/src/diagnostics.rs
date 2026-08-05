use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use gdscript_api_db::ApiDb;
use gdscript_checker::diagnostics::Severity;
use gdscript_parser::parse::parse;
use tokio::sync::RwLock;
use tower_lsp::Client;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range, Url};

use crate::type_resolver::TypeMap;
use gdscript_indexer::ProjectIndex;

/// Parse `source`, extract syntax errors + lint warnings + `extra` diagnostics, and publish.
///
/// `autoload_paths` — files registered as Godot autoloads. W0004 (missing
/// `class_name`) is suppressed for these: Godot treats autoload names as global
/// identifiers and refuses to compile scripts that re-declare them with `class_name`.
#[allow(clippy::implicit_hasher)]
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
        .is_some_and(|p| autoload_paths.contains(p));

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
                        start: Position {
                            line: d.line,
                            character: d.col,
                        },
                        end: Position {
                            line: d.end_line,
                            character: d.end_col,
                        },
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

/// Compute diagnostics for a document without publishing (used by pull model).
#[allow(clippy::implicit_hasher)]
pub async fn compute_diagnostics(
    uri: &Url,
    source: &str,
    api_db: &Arc<RwLock<Option<ApiDb>>>,
    type_maps: &Arc<RwLock<std::collections::HashMap<Url, TypeMap>>>,
    project_index: &Arc<RwLock<ProjectIndex>>,
) -> Vec<Diagnostic> {
    let mut diags: Vec<Diagnostic> = match parse(source) {
        Ok(doc) => {
            let errors = gdscript_checker::syntax::syntax_errors(&doc);
            let warnings = gdscript_checker::linting::lint(&doc);
            errors
                .into_iter()
                .chain(warnings)
                .map(|d| Diagnostic {
                    range: Range {
                        start: Position {
                            line: d.line,
                            character: d.col,
                        },
                        end: Position {
                            line: d.end_line,
                            character: d.end_col,
                        },
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

    // Append type-check diagnostics from call checker.
    if let Ok(doc) = parse(source) {
        let db = api_db.read().await;
        if let Some(db) = db.as_ref() {
            let type_maps_guard = type_maps.read().await;
            let empty = TypeMap::default();
            let type_map = type_maps_guard.get(uri).unwrap_or(&empty);
            let index = project_index.read().await;
            let extra = crate::call_checker::check_calls(&doc, type_map, db, &index);
            let type_diags = crate::type_check::check_type_mismatches(&doc, db);
            diags.extend(extra);
            diags.extend(type_diags);
        }
    }

    diags
}
