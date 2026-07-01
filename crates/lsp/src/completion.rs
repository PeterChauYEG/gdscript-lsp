use gdscript_api_db::ApiDb;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, Documentation, MarkupContent,
    MarkupKind,
};

use crate::type_resolver::TypeMap;

/// Build completion items for member access after `.`.
///
/// Resolves the receiver type from `type_map`, then returns all methods and
/// properties from that class and every ancestor in the inheritance chain.
#[must_use]
pub fn member_completions(
    receiver: &str,
    type_map: &TypeMap,
    api_db: &ApiDb,
) -> Option<CompletionResponse> {
    let type_name = type_map.resolve(receiver)?;
    let chain = api_db.inheritance_chain(type_name);

    let mut items: Vec<CompletionItem> = Vec::new();

    for class_name in &chain {
        let Some(class) = api_db.get_class(class_name) else {
            continue;
        };

        for method in &class.methods {
            let args: Vec<String> = method
                .arguments
                .iter()
                .map(|a| {
                    if let Some(def) = &a.default_value {
                        format!("{}: {} = {}", a.name, a.type_name, def)
                    } else {
                        format!("{}: {}", a.name, a.type_name)
                    }
                })
                .collect();
            let ret = method
                .return_value
                .as_ref()
                .map_or("void", |r| r.type_name.as_str());
            let signature = format!("{}({}) -> {}", method.name, args.join(", "), ret);

            let mut item = CompletionItem {
                label: method.name.clone(),
                kind: Some(if method.is_static {
                    CompletionItemKind::FUNCTION
                } else {
                    CompletionItemKind::METHOD
                }),
                detail: Some(format!("[{}] {}", class_name, signature)),
                insert_text: Some(format!("{}(", method.name)),
                ..Default::default()
            };

            if !method.description.is_empty() {
                item.documentation = Some(Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: method.description.clone(),
                }));
            }

            items.push(item);
        }

        for prop in &class.properties {
            items.push(CompletionItem {
                label: prop.name.clone(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(format!("[{}] {}: {}", class_name, prop.name, prop.type_name)),
                documentation: if prop.description.is_empty() {
                    None
                } else {
                    Some(Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: prop.description.clone(),
                    }))
                },
                ..Default::default()
            });
        }

        for constant in &class.constants {
            items.push(CompletionItem {
                label: constant.name.clone(),
                kind: Some(CompletionItemKind::CONSTANT),
                detail: Some(format!("[{}] {} = {}", class_name, constant.name, constant.value)),
                ..Default::default()
            });
        }
    }

    Some(CompletionResponse::Array(items))
}

/// Build a flat list of all engine class names as completion items.
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
