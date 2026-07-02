use std::collections::HashMap;

use tower_lsp::lsp_types::Url;

/// Stores the current text content of all open documents.
#[derive(Debug, Default)]
pub struct DocumentStore {
    documents: HashMap<Url, String>,
}

impl DocumentStore {
    pub fn open(&mut self, uri: Url, text: String) {
        self.documents.insert(uri, text);
    }

    pub fn update(&mut self, uri: &Url, text: String) {
        if let Some(doc) = self.documents.get_mut(uri) {
            *doc = text;
        }
    }

    pub fn close(&mut self, uri: &Url) {
        self.documents.remove(uri);
    }

    pub fn get(&self, uri: &Url) -> Option<&str> {
        self.documents.get(uri).map(String::as_str)
    }

    /// Return a snapshot of all open documents.
    pub fn all(&self) -> HashMap<Url, String> {
        self.documents.clone()
    }
}
