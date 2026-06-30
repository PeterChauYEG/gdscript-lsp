use gdscript_api_db::ApiDb;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, CompletionResponse};

/// Build a flat list of all engine class names as completion items.
///
/// Context-aware completions (e.g. member access after `.`) require type
/// inference and are handled separately once the type checker is implemented.
#[must_use]
pub fn class_name_completions(api_db: &ApiDb) -> CompletionResponse {
    let items = api_db
        .class_names()
        .map(|name| CompletionItem {
            label: name.to_owned(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some("Godot engine class".to_owned()),
            ..Default::default()
        })
        .collect();

    CompletionResponse::Array(items)
}
