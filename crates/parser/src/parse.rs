use tree_sitter::{Parser, Tree};

use crate::error::ParseError;

pub struct ParsedDocument {
    pub tree: Tree,
    pub source: String,
}

/// Parse a `GDScript` source string into a `ParsedDocument`.
///
/// # Errors
///
/// Returns [`ParseError::LanguageError`] if the tree-sitter language fails to
/// initialize or the parser produces no tree (should not happen in practice).
pub fn parse(source: &str) -> Result<ParsedDocument, ParseError> {
    let mut parser = Parser::new();
    let language = tree_sitter_gdscript::LANGUAGE;
    parser
        .set_language(&language.into())
        .map_err(|_| ParseError::LanguageError)?;

    let tree = parser
        .parse(source, None)
        .ok_or(ParseError::LanguageError)?;

    Ok(ParsedDocument {
        tree,
        source: source.to_owned(),
    })
}
