use gdscript_api_db::ApiDb;
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use crate::type_resolver::TypeMap;

fn make_hover(md: String) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: None,
    }
}

/// Build a hover response given the cursor position within a source line.
///
/// Checks (in order):
/// 1. Method/property on a typed receiver (`receiver.member`)
/// 2. Method/property on `self` (bare call in a typed script)
/// 3. Class name
#[must_use]
pub fn hover_at(
    word: &str,
    line: &str,
    char_pos: usize,
    type_map: &TypeMap,
    api_db: &ApiDb,
) -> Option<Hover> {
    // Find the byte offset of `word` start on this line.
    // Check whether there's a `.receiver` pattern before it.
    let word_start = find_word_start(line, char_pos);

    if let Some(dot_pos) = word_start.checked_sub(1).filter(|&i| line.as_bytes().get(i) == Some(&b'.')) {
        // Find the receiver identifier before the dot.
        let before_dot: String = line.chars().take(dot_pos).collect();
        let receiver = before_dot
            .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .filter(|s| !s.is_empty());

        if let Some(receiver) = receiver {
            // Resolve receiver type: try type map, then direct class name.
            let type_name = type_map
                .resolve(receiver)
                .or_else(|| api_db.get_class(receiver).map(|c| c.name.as_str()));

            if let Some(type_name) = type_name {
                if let Some(h) = hover_member(type_name, word, api_db) {
                    return Some(h);
                }
            }
        }
    } else {
        // Bare call — could be a method on self.
        if let Some(self_type) = type_map.self_type.as_deref() {
            if let Some(h) = hover_member(self_type, word, api_db) {
                return Some(h);
            }
        }
    }

    // Fall back: class name hover.
    hover_class(word, api_db)
}

fn hover_member(type_name: &str, member: &str, api_db: &ApiDb) -> Option<Hover> {
    let chain = api_db.inheritance_chain(type_name);
    for class_name in &chain {
        let Some(class) = api_db.get_class(class_name) else { continue };

        if let Some(method) = class.methods.iter().find(|m| m.name == member) {
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
            let mut md = format!(
                "```gdscript\nfunc {}.{}({}) -> {}\n```",
                class_name,
                method.name,
                args.join(", "),
                ret
            );
            if !method.description.is_empty() {
                md.push_str("\n\n");
                md.push_str(&method.description);
            }
            return Some(make_hover(md));
        }

        if let Some(prop) = class.properties.iter().find(|p| p.name == member) {
            let mut md = format!(
                "```gdscript\nvar {}.{}: {}\n```",
                class_name, prop.name, prop.type_name
            );
            if !prop.description.is_empty() {
                md.push_str("\n\n");
                md.push_str(&prop.description);
            }
            return Some(make_hover(md));
        }
    }
    None
}

fn hover_class(word: &str, api_db: &ApiDb) -> Option<Hover> {
    let class = api_db.get_class(word)?;
    let chain = api_db.inheritance_chain(word);
    let inherits_str = if chain.len() > 1 {
        format!(" *({})*", chain[1..].join(" → "))
    } else {
        String::new()
    };

    let mut lines = vec![format!("**{}**{}", class.name, inherits_str), String::new()];

    if !class.properties.is_empty() {
        lines.push(format!("**Properties** ({})", class.properties.len()));
        for p in class.properties.iter().take(5) {
            lines.push(format!("- `{}`: {}", p.name, p.type_name));
        }
        if class.properties.len() > 5 {
            lines.push(format!("- *…{} more*", class.properties.len() - 5));
        }
        lines.push(String::new());
    }

    if !class.methods.is_empty() {
        lines.push(format!("**Methods** ({})", class.methods.len()));
        for m in class.methods.iter().take(5) {
            let ret = m.return_value.as_ref().map_or("void", |r| r.type_name.as_str());
            let args: Vec<_> = m.arguments.iter().map(|a| a.name.as_str()).collect();
            lines.push(format!("- `{}({})` → {}", m.name, args.join(", "), ret));
        }
        if class.methods.len() > 5 {
            lines.push(format!("- *…{} more*", class.methods.len() - 5));
        }
    }

    Some(make_hover(lines.join("\n")))
}

/// Find the byte offset of the start of the word that contains `char_pos`.
fn find_word_start(line: &str, char_pos: usize) -> usize {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let chars: Vec<char> = line.chars().collect();
    let pos = char_pos.min(chars.len().saturating_sub(1));
    let mut start = pos;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    // Convert char index to byte offset.
    chars[..start].iter().collect::<String>().len()
}

#[cfg(test)]
mod tests {
    use gdscript_api_db::ApiDb;

    use super::*;
    use crate::type_resolver::TypeMap;

    fn db() -> ApiDb { ApiDb::bundled().unwrap() }
    fn empty_map() -> TypeMap { TypeMap::default() }

    fn hover_text(word: &str, line: &str, char_pos: usize, map: &TypeMap, db: &ApiDb) -> String {
        let h = hover_at(word, line, char_pos, map, db).unwrap();
        match h.contents {
            HoverContents::Markup(m) => m.value,
            _ => String::new(),
        }
    }

    #[test]
    fn class_hover_shows_name() {
        let db = db();
        let text = hover_text("Node2D", "Node2D", 3, &empty_map(), &db);
        assert!(text.contains("Node2D"));
    }

    #[test]
    fn class_hover_shows_inheritance() {
        let db = db();
        let text = hover_text("Node2D", "Node2D", 3, &empty_map(), &db);
        assert!(text.contains("Node"));
    }

    #[test]
    fn method_hover_on_receiver() {
        let db = db();
        let mut map = TypeMap::default();
        map.types.insert("n".to_owned(), "Node2D".to_owned());
        let line = "n.add_child(x)";
        // char_pos points into "add_child"
        let text = hover_text("add_child", line, 6, &map, &db);
        assert!(text.contains("add_child"));
        assert!(text.contains("func"));
    }

    #[test]
    fn property_hover_on_receiver() {
        let db = db();
        let mut map = TypeMap::default();
        map.types.insert("n".to_owned(), "Node2D".to_owned());
        let line = "n.position";
        let text = hover_text("position", line, 3, &map, &db);
        assert!(text.contains("position"));
    }

    #[test]
    fn bare_method_hover_uses_self_type() {
        let db = db();
        let mut map = TypeMap::default();
        map.self_type = Some("Node2D".to_owned());
        let line = "add_child(x)";
        let text = hover_text("add_child", line, 3, &map, &db);
        assert!(text.contains("add_child"));
    }

    #[test]
    fn unknown_word_returns_none() {
        let db = db();
        let result = hover_at("totally_unknown_xyz", "totally_unknown_xyz", 5, &empty_map(), &db);
        assert!(result.is_none());
    }
}
