use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub uri: Url,
    pub line: u32,
    pub col: u32,
}
