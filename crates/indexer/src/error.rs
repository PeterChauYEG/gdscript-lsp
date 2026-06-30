#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("file watcher error: {0}")]
    Watcher(String),
}
