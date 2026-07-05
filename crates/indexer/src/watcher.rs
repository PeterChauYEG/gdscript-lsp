use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use tokio::sync::RwLock;

use crate::{ProjectIndex, error::IndexerError, index::index_workspace};

/// Spawn a background task that watches `root` for `.gd` changes and updates `index`.
///
/// # Errors
///
/// Returns [`IndexerError::Watcher`] if the file watcher cannot be created.
pub fn watch(
    root: &Path,
    index: Arc<RwLock<ProjectIndex>>,
) -> Result<(), IndexerError> {
    let root = root.to_path_buf();

    tokio::task::spawn_blocking(move || {
        let (tx, rx) = std::sync::mpsc::channel();

        let mut debouncer = new_debouncer(Duration::from_millis(300), tx)
            .map_err(|e| IndexerError::Watcher(e.to_string()))?;

        debouncer
            .watcher()
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| IndexerError::Watcher(e.to_string()))?;

        for result in rx {
            let events = match result {
                Ok(events) => events,
                Err(_) => continue,
            };

            let any_gd = events.iter().any(|e| {
                e.path.extension().and_then(|x| x.to_str()) == Some("gd")
            });

            if any_gd {
                let handle = tokio::runtime::Handle::try_current();
                if let Ok(handle) = handle {
                    let index = index.clone();
                    let root = root.clone();
                    handle.spawn(async move {
                        if let Ok(new_index) = index_workspace(&root) {
                            *index.write().await = new_index;
                        }
                    });
                }
            }
        }

        Ok::<_, IndexerError>(())
    });

    Ok(())
}
