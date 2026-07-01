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
