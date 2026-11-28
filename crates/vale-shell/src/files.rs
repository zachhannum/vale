//! The file services of a folder that a sync service owns.

use std::io;
use std::path::{Path, PathBuf};
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

/// The extension of the file that iCloud Drive keeps in place of a file
/// that is not on the device. The name of that file starts with a dot.
const PLACEHOLDER: &str = "icloud";

/// The path with no symbolic link in its folder. The path of a file from
/// the system can name the same folder in a different way.
pub fn real_path(path: &Path) -> PathBuf {
    if let Ok(real) = std::fs::canonicalize(path) {
        return real;
    }
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    match (absolute.parent(), absolute.file_name()) {
        (Some(dir), Some(name)) => match std::fs::canonicalize(dir) {
            Ok(dir) => dir.join(name),
            Err(_) => absolute,
        },
        _ => absolute,
    }
}

/// The file with the extension that a placeholder of iCloud Drive stands
/// for. `None`: the path is not such a placeholder.
pub fn placeholder_target(path: &Path, extension: &str) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let name = name.strip_prefix('.')?;
    let name = name.strip_suffix(PLACEHOLDER)?.strip_suffix('.')?;
    let target = path.with_file_name(name);
    (target.extension().is_some_and(|e| e == extension)).then_some(target)
}

/// The placeholder of iCloud Drive for a file.
pub fn placeholder_of(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    Some(path.with_file_name(format!(".{name}.{PLACEHOLDER}")))
}

/// The files of a folder with the extension, and where their bytes are. The
/// path of a placeholder of iCloud Drive is the path of its file. A folder
/// that is not there gives an empty list.
pub fn listed(dir: &Path, extension: &str, files: &dyn Files) -> Vec<(PathBuf, Availability)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let paths = entries.filter_map(|entry| Some(entry.ok()?.path()));
    let listed = |path: PathBuf| match placeholder_target(&path, extension) {
        // A placeholder of a file that is in the folder is not in the list.
        Some(target) if target.exists() => None,
        Some(target) => {
            let availability = match files.availability(&target) {
                Availability::Here => Availability::InCloud,
                other => other,
            };
            Some((target, availability))
        }
        None if path.extension().is_some_and(|e| e == extension) && path.is_file() => {
            let availability = files.availability(&path);
            Some((path, availability))
        }
        None => None,
    };
    paths.filter_map(listed).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placeholder_is_listed_as_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = |name: &str| dir.path().join(name);
        for name in [
            "a.gpkg",
            ".b.gpkg.icloud",
            ".a.gpkg.icloud",
            "c.txt",
            ".d.txt.icloud",
        ] {
            std::fs::write(path(name), "x").unwrap();
        }
        std::fs::create_dir(path("e.gpkg")).unwrap();
        assert_eq!(
            placeholder_target(&path(".b.gpkg.icloud"), "gpkg"),
            Some(path("b.gpkg"))
        );
        assert_eq!(
            placeholder_of(&path("b.gpkg")),
            Some(path(".b.gpkg.icloud"))
        );

        let mut list = listed(dir.path(), "gpkg", &PlainFiles);
        list.sort_by(|a, b| a.0.cmp(&b.0));
        let expected = [
            (path("a.gpkg"), Availability::Here),
            (path("b.gpkg"), Availability::InCloud),
        ];
        assert_eq!(list, expected);
        assert_eq!(listed(&path("none"), "gpkg", &PlainFiles), []);
    }
}
