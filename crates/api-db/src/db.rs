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
