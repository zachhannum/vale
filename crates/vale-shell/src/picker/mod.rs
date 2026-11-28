//! The file picker. On iOS it shows the Files app, and the paths come back
//! through a [`PickQueue`] in a later frame. On the desktop it is a dialog
//! that blocks.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "ios")]
mod uikit;
#[cfg(target_os = "ios")]
pub use uikit::Picker;

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

/// Shows a dialog for one file with one of `extensions`, for example
/// `pick_file("Heightmap", &["png", "tif"])`. `filter` names the files in
/// the dialog. Returns `None` if the user cancels.
#[cfg(not(target_os = "ios"))]
pub fn pick_file(filter: &str, extensions: &[&str]) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(filter, extensions)
        .pick_file()
}

/// Shows a dialog for one or more files with one of `extensions`.
#[cfg(not(target_os = "ios"))]
pub fn pick_files(filter: &str, extensions: &[&str]) -> Option<Vec<PathBuf>> {
    rfd::FileDialog::new()
        .add_filter(filter, extensions)
        .pick_files()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_export_queue_gives_each_event_one_time() {
        let queue = ExportQueue::default();
        queue.clone().push(ExportEvent::Saved);
        queue.push(ExportEvent::Cancelled);
        assert_eq!(queue.take(), [ExportEvent::Saved, ExportEvent::Cancelled]);
        assert!(queue.take().is_empty());
    }

    #[test]
    fn the_pick_queue_gives_each_path_one_time() {
        let queue = PickQueue::default();
        queue.clone().push([PathBuf::from("a.png")]);
        queue.push([PathBuf::from("b.tif")]);
        assert_eq!(
            queue.take(),
            [PathBuf::from("a.png"), PathBuf::from("b.tif")]
        );
        assert!(queue.take().is_empty());
    }
}
