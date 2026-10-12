//! Files that the file picker of the system gives to the app.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[cfg(target_os = "ios")]
pub mod uikit;

/// The folder of the export files that wait for the Files app.
pub fn export_dir() -> PathBuf {
    std::env::temp_dir().join("vale-export")
}

/// Removes the export files that did not go to the Files app.
pub fn clear_exports() {
    clear_dir(&export_dir());
}

/// Removes the files of a folder. The folder stays.
pub fn clear_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
}

/// The end of the hand-off of an export file to the Files app.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExportEvent {
    /// The file is at the place that the user picked.
    Saved,
    /// The user closed the Files app with no place.
    Cancelled,
}

/// The ends of the hand-offs that wait for the app.
#[derive(Clone, Default)]
pub struct ExportQueue(Arc<Mutex<Vec<ExportEvent>>>);

impl ExportQueue {
    pub fn push(&self, event: ExportEvent) {
        if let Ok(mut queue) = self.0.lock() {
            queue.push(event);
        }
    }

    pub fn take(&self) -> Vec<ExportEvent> {
        self.0
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_dir_removes_the_files_of_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("vale-export");
        // A folder that is not there is not an error.
        clear_dir(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        for name in ["a.tif", "b.tif"] {
            std::fs::write(folder.join(name), "x").unwrap();
        }
        clear_dir(&folder);
        assert!(folder.is_dir());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    }

    #[test]
    fn the_export_queue_gives_each_event_one_time() {
        let queue = ExportQueue::default();
        queue.clone().push(ExportEvent::Saved);
        queue.push(ExportEvent::Cancelled);
        assert_eq!(queue.take(), [ExportEvent::Saved, ExportEvent::Cancelled]);
        assert!(queue.take().is_empty());
    }
}
