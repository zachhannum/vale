//! Files that the file picker of the system gives to the app.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "ios")]
pub mod uikit;

/// The picked files that wait for the app.
#[derive(Clone, Default)]
pub struct PickQueue(Arc<Mutex<Vec<PathBuf>>>);

impl PickQueue {
    pub fn push(&self, paths: impl IntoIterator<Item = PathBuf>) {
        if let Ok(mut queue) = self.0.lock() {
            queue.extend(paths);
        }
    }

    pub fn take(&self) -> Vec<PathBuf> {
        self.0
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }
}
