use std::collections::HashMap;

/// Node tree extracted from a `.tscn` file: node name → Godot class name.
pub type SceneNodeMap = HashMap<String, String>;

/// Parse a `.tscn` file and extract the node-name → type mapping.
///
/// Only reads `[node ...]` header lines; ignores properties and sub-resources.
#[must_use]
pub fn parse(content: &str) -> SceneNodeMap {
    let mut map = SceneNodeMap::new();

    for line in content.lines() {
        let line = line.trim();
        if !line.starts_with("[node ") {
            continue;
        }
        let name = attr(line, "name");
        let type_ = attr(line, "type");
        if let (Some(name), Some(type_)) = (name, type_) {
            map.insert(name, type_);
        }
    }

    map
}

/// Extract a quoted attribute value from a `.tscn` header line.
/// e.g. `attr([node name="Foo" type="Node2D"], "name")` → `Some("Foo")`
fn attr<'a>(line: &'a str, key: &str) -> Option<String> {
    let needle = format!("{}=\"", key);
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_node() {
        let src = "[node name=\"Sprite2D\" type=\"Sprite2D\"]\n";
        let map = parse(src);
        assert_eq!(map.get("Sprite2D").map(String::as_str), Some("Sprite2D"));
    }

    #[test]
    fn parses_multiple_nodes() {
        let src = "[node name=\"Root\" type=\"Node2D\"]\n[node name=\"Player\" type=\"CharacterBody2D\"]\n";
        let map = parse(src);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("Root").map(String::as_str), Some("Node2D"));
        assert_eq!(map.get("Player").map(String::as_str), Some("CharacterBody2D"));
    }

    #[test]
    fn ignores_non_node_lines() {
        let src = "[ext_resource type=\"Script\" path=\"res://player.gd\" id=\"1\"]\n[node name=\"Root\" type=\"Node\"]\n[sub_resource ...]\n";
        let map = parse(src);
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn node_without_type_is_skipped() {
        let src = "[node name=\"Orphan\"]\n";
        let map = parse(src);
        assert!(map.is_empty());
    }

    #[test]
    fn node_without_name_is_skipped() {
        let src = "[node type=\"Node2D\"]\n";
        let map = parse(src);
        assert!(map.is_empty());
    }

    #[test]
    fn empty_file_returns_empty_map() {
        let map = parse("");
        assert!(map.is_empty());
    }
}
