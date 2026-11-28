//! The file services of a folder that a sync service owns.

use std::io;
use std::path::Path;
use std::time::SystemTime;

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
    /// The device has an older version of the file, and the sync service
    /// has a newer one.
    Stale,
}

impl Availability {
    /// True if the device has a version of the file.
    pub fn on_device(self) -> bool {
        matches!(self, Availability::Here | Availability::Stale)
    }
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
