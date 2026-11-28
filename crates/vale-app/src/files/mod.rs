//! Files that the system gives to the app, and the file services of a
//! folder that a sync service owns.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub use vale_shell::files::{Availability, Files, PlainFiles, Version};
pub use vale_shell::picker::{ExportEvent, ExportQueue, PickQueue};

#[cfg(target_os = "ios")]
pub mod cloud;
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

/// What the system tells the app between two frames.
#[derive(Clone, Default)]
pub struct SystemEvents(Arc<Mutex<Events>>);

#[derive(Default)]
struct Events {
    opened: Vec<PathBuf>,
    cloud: Option<Option<PathBuf>>,
    wake: Option<Box<dyn Fn() + Send>>,
}

impl SystemEvents {
    fn change(&self, f: impl FnOnce(&mut Events)) {
        if let Ok(mut events) = self.0.lock() {
            f(&mut events);
            if let Some(wake) = &events.wake {
                wake();
            }
        }
    }

    /// Sets the function that asks the app for a new frame.
    pub fn set_wake(&self, wake: Box<dyn Fn() + Send>) {
        self.change(|events| events.wake = Some(wake));
    }

    /// The system asks the app to open a project file.
    pub fn open(&self, path: PathBuf) {
        self.change(|events| {
            if !events.opened.contains(&path) {
                events.opened.push(path);
            }
        });
    }

    /// The system gives the iCloud folder. `None`: the device has no iCloud
    /// Drive for the app.
    pub fn set_cloud(&self, dir: Option<PathBuf>) {
        self.change(|events| events.cloud = Some(dir));
    }

    pub fn take_opened(&self) -> Vec<PathBuf> {
        let mut events = self.0.lock();
        events
            .as_mut()
            .map(|events| std::mem::take(&mut events.opened))
            .unwrap_or_default()
    }

    pub fn take_cloud(&self) -> Option<Option<PathBuf>> {
        self.0.lock().ok()?.cloud.take()
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
}
