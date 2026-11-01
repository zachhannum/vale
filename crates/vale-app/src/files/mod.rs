//! Files that the system gives to the app, and the file services of a
//! folder that a sync service owns.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

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

/// Where the bytes of a file are.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Availability {
    /// The file is on the device.
    #[default]
    Here,
    /// The sync service has the file, and the device has only its name.
    InCloud,
    /// The device gets the file from the sync service.
    Downloading,
}

/// One of the versions of a file that two devices changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    /// The name of the device that saved the version.
    pub device: String,
    pub modified: Option<SystemTime>,
}

/// The file services of the system for a folder that a sync service owns.
/// Each default is the answer of a plain folder.
pub trait Files {
    fn availability(&self, _path: &Path) -> Availability {
        Availability::Here
    }

    /// Asks the sync service for the bytes of a file.
    fn start_download(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    /// The other versions of a file that two devices changed. An empty list:
    /// the file has one version.
    fn conflicts(&self, _path: &Path) -> Vec<Version> {
        Vec::new()
    }

    /// Keeps one version of a file and removes the others. `keep` is an
    /// index into `conflicts`. `None` keeps the version at the path.
    fn resolve(&self, _path: &Path, _keep: Option<usize>) -> io::Result<()> {
        Ok(())
    }

    /// Runs `f` while the sync service does not write the file. `f` gets
    /// the path to read.
    fn read(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
        f(path)
    }

    /// Runs `f` while the sync service does not read or write the file. `f`
    /// gets the path to write.
    fn write(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
        f(path)
    }

    /// Runs `f` with the right to read a file that the Files app gave to
    /// the app from outside its folders.
    fn with_access(&self, _path: &Path, f: &mut dyn FnMut() -> io::Result<()>) -> io::Result<()> {
        f()
    }

    /// The same file services for another thread. `None`: only this thread
    /// can use them.
    fn for_thread(&self) -> Option<Box<dyn Files + Send>> {
        None
    }
}

/// The files of a plain folder.
pub struct PlainFiles;

impl Files for PlainFiles {
    fn for_thread(&self) -> Option<Box<dyn Files + Send>> {
        Some(Box::new(PlainFiles))
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

    #[test]
    fn the_export_queue_gives_each_event_one_time() {
        let queue = ExportQueue::default();
        queue.clone().push(ExportEvent::Saved);
        queue.push(ExportEvent::Cancelled);
        assert_eq!(queue.take(), [ExportEvent::Saved, ExportEvent::Cancelled]);
        assert!(queue.take().is_empty());
    }
}
