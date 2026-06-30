use std::collections::HashMap;

/// Node tree extracted from a `.tscn` file: `NodePath` → Godot class name.
pub type SceneNodeMap = HashMap<String, String>;

/// Parse a `.tscn` file and extract the node hierarchy.
#[must_use]
pub fn parse(_content: &str) -> SceneNodeMap {
    // TODO(LAB-663): implement .tscn state-machine parser
    HashMap::new()
}
