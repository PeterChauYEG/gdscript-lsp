#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("tree-sitter language error")]
    LanguageError,
}
