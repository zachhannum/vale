//! The folder of the world files, and the world that the next start opens.

use std::path::{Path, PathBuf};

use vale_shell::resume::{Lines, Resume, ResumeView};

/// The extension of a world file.
pub const EXTENSION: &str = "gpkg";

/// The folder of the world files of this device. On iOS, it is the
/// Documents folder, which the Files app shows. `None`: the system gives no
/// home folder.
pub fn default_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if cfg!(target_os = "ios") {
        Some(PathBuf::from(var("HOME")?).join("Documents"))
    } else if cfg!(target_os = "macos") {
        Some(PathBuf::from(var("HOME")?).join("Library/Application Support/Isotherm"))
    } else if cfg!(windows) {
        Some(PathBuf::from(var("APPDATA")?).join("Isotherm"))
    } else {
        let data = match var("XDG_DATA_HOME") {
            Some(data) => PathBuf::from(data),
            None => PathBuf::from(var("HOME")?).join(".local/share"),
        };
        Some(data.join("isotherm"))
    }
}

/// A path in `dir` for a new world file named after `name`. If the file
/// exists, the name gets " 2", " 3", and so on.
pub fn new_path(dir: &Path, name: &str) -> PathBuf {
    let name = name.trim();
    let name = if name.is_empty() { "World" } else { name };
    let mut n = 1;
    loop {
        let stem = match n {
            1 => name.to_owned(),
            n => format!("{name} {n}"),
        };
        let path = dir.join(format!("{stem}.{EXTENSION}"));
        if !path.exists() {
            return path;
        }
        n += 1;
    }
}

/// The state of a world has no view yet.
#[derive(Clone, Debug, PartialEq)]
struct NoView;

impl ResumeView for NoView {
    fn lines(&self, _out: &mut Vec<String>) {}

    fn parse(_lines: &Lines) -> Option<NoView> {
        None
    }
}

/// The world file of the last run, if it still exists.
pub fn last_world(dir: &Path) -> Option<PathBuf> {
    let resume = Resume::<NoView>::read(dir)?;
    resume.project.filter(|path| path.is_file())
}

/// Keeps `path` as the world that the next start opens.
pub fn remember(dir: &Path, path: &Path) -> std::io::Result<()> {
    let resume = Resume::<NoView> {
        project: Some(path.to_path_buf()),
        view: None,
    };
    resume.write(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_name_does_not_take_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let first = new_path(dir.path(), "Pangaea");
        assert_eq!(first, dir.path().join("Pangaea.gpkg"));
        std::fs::write(&first, b"").unwrap();
        assert_eq!(
            new_path(dir.path(), "Pangaea"),
            dir.path().join("Pangaea 2.gpkg")
        );
        assert_eq!(new_path(dir.path(), " "), dir.path().join("World.gpkg"));
    }

    #[test]
    fn the_last_world_comes_back_while_its_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(last_world(dir.path()), None);
        let path = dir.path().join("A.gpkg");
        remember(dir.path(), &path).unwrap();
        assert_eq!(last_world(dir.path()), None);
        std::fs::write(&path, b"").unwrap();
        assert_eq!(last_world(dir.path()), Some(path));
    }
}
