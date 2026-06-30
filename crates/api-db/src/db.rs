use std::collections::HashMap;

use serde::Deserialize;

use crate::{error::ApiDbError, types::ClassDef};

#[derive(Debug, Deserialize)]
struct ExtensionApi {
    #[serde(default)]
    classes: Vec<ClassDef>,
}

/// In-memory database of all Godot engine built-in classes.
pub struct ApiDb {
    classes: HashMap<String, ClassDef>,
}

impl ApiDb {
    /// Load the `extension_api.json` bundled at compile time.
    ///
    /// # Errors
    ///
    /// Returns [`ApiDbError::Parse`] if the bundled JSON is malformed (should never happen).
    pub fn bundled() -> Result<Self, ApiDbError> {
        Self::from_json(crate::BUNDLED_API)
    }

    /// Parse an `extension_api.json` payload into the database.
    ///
    /// # Errors
    ///
    /// Returns [`ApiDbError::Parse`] if the JSON is malformed or missing expected fields.
    pub fn from_json(json: &[u8]) -> Result<Self, ApiDbError> {
        let api: ExtensionApi = serde_json::from_slice(json)?;
        let classes = api
            .classes
            .into_iter()
            .map(|c| (c.name.clone(), c))
            .collect();
        Ok(Self { classes })
    }

    #[must_use]
    pub fn get_class(&self, name: &str) -> Option<&ClassDef> {
        self.classes.get(name)
    }

    pub fn class_names(&self) -> impl Iterator<Item = &str> {
        self.classes.keys().map(String::as_str)
    }

    /// Resolve the full inheritance chain for a class (inclusive, from most to least derived).
    #[must_use]
    pub fn inheritance_chain<'a>(&'a self, name: &'a str) -> Vec<&'a str> {
        let mut chain = vec![];
        let mut current = name;
        loop {
            chain.push(current);
            match self
                .classes
                .get(current)
                .and_then(|c| c.inherits.as_deref())
            {
                Some(parent) => current = parent,
                None => break,
            }
        }
        chain
    }

    /// Check if `candidate` is the same class as or a subclass of `base`.
    #[must_use]
    pub fn is_subclass(&self, candidate: &str, base: &str) -> bool {
        self.inheritance_chain(candidate).contains(&base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_api_parses() {
        let db = ApiDb::bundled().expect("bundled extension_api.json must parse");
        assert!(
            db.class_names().count() > 1000,
            "expected 1000+ engine classes"
        );
        assert!(db.get_class("Node").is_some(), "Node must exist");
        assert!(db.get_class("Node2D").is_some(), "Node2D must exist");
        assert!(db.is_subclass("Node2D", "Node"), "Node2D inherits Node");
    }

    #[test]
    fn inheritance_chain_terminates() {
        let db = ApiDb::bundled().unwrap();
        let chain = db.inheritance_chain("Sprite2D");
        assert!(chain.contains(&"Node2D"));
        assert!(chain.contains(&"Node"));
        assert!(chain.contains(&"Object"));
    }
}
