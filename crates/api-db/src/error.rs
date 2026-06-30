#[derive(Debug, thiserror::Error)]
pub enum ApiDbError {
    #[error("failed to parse extension_api.json: {0}")]
    Parse(#[from] serde_json::Error),
}
