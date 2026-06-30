use gdscript_api_db::ApiDb;
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

/// Build a hover response for `word` if it names a known engine class.
#[must_use]
pub fn hover_for_word(word: &str, api_db: &ApiDb) -> Option<Hover> {
    let class = api_db.get_class(word)?;

    let chain = api_db.inheritance_chain(word);
    let inherits_str = if chain.len() > 1 {
        format!(" *({})*", chain[1..].join(" → "))
    } else {
        String::new()
    };

    let mut lines = vec![format!("**{}**{}", class.name, inherits_str), String::new()];

    if !class.properties.is_empty() {
        lines.push(format!("**Properties** ({})  ", class.properties.len()));
        for p in class.properties.iter().take(5) {
            lines.push(format!("- `{}`: {}", p.name, p.type_name));
        }
        if class.properties.len() > 5 {
            lines.push(format!("- *…{} more*", class.properties.len() - 5));
        }
        lines.push(String::new());
    }

    if !class.methods.is_empty() {
        lines.push(format!("**Methods** ({})  ", class.methods.len()));
        for m in class.methods.iter().take(5) {
            let ret = m
                .return_value
                .as_ref()
                .map_or("void", |r| r.type_name.as_str());
            let args: Vec<_> = m.arguments.iter().map(|a| a.name.as_str()).collect();
            lines.push(format!("- `{}({})` → {}", m.name, args.join(", "), ret));
        }
        if class.methods.len() > 5 {
            lines.push(format!("- *…{} more*", class.methods.len() - 5));
        }
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: None,
    })
}
