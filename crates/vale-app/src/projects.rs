//! The projects folder: the list of the project files, and the rename, copy,
//! and delete operations on a file that is not open.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use vale_store::{ProjectFile, StoreError, Thumbnail};

use crate::session::{EXTENSION, new_project_path};

/// The file in the projects folder that holds the recent list.
const RECENT_FILE: &str = "recent.txt";
/// The largest count of paths in the recent list.
const RECENT_MAX: usize = 8;

/// One project file in the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    /// The name of the world. If the app cannot read the file, this is the
    /// name of the file.
    pub name: String,
    pub thumbnail: Option<Thumbnail>,
    /// The time of the last change of the file.
    pub modified: Option<SystemTime>,
    /// The text of the error, if the app cannot read the file.
    pub error: Option<String>,
}

/// The projects folder of this device. `None`: the system gives no home
/// folder.
pub fn default_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if cfg!(any(target_os = "ios", target_os = "macos")) {
        // On iOS, `HOME` is the container of the app.
        let home = PathBuf::from(var("HOME")?);
        Some(home.join("Library/Application Support/Vale"))
    } else if cfg!(windows) {
        Some(PathBuf::from(var("APPDATA")?).join("Vale"))
    } else {
        let data = match var("XDG_DATA_HOME") {
            Some(data) => PathBuf::from(data),
            None => PathBuf::from(var("HOME")?).join(".local/share"),
        };
        Some(data.join("vale"))
    }
}

/// The entry of one project file.
pub fn entry(path: &Path) -> Entry {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let stem = || {
        let stem = path.file_stem().unwrap_or(path.as_os_str());
        stem.to_string_lossy().into_owned()
    };
    match vale_store::summary(path) {
        Ok(summary) => Entry {
            path: path.to_path_buf(),
            name: summary.name,
            thumbnail: summary.thumbnail,
            modified,
            error: None,
        },
        Err(e) => Entry {
            path: path.to_path_buf(),
            name: stem(),
            thumbnail: None,
            modified,
            error: Some(e.to_string()),
        },
    }
}

/// The project files of the folder. The file with the newest change is the
/// first. A folder that is not there gives an empty list.
pub fn list(dir: &Path) -> Vec<Entry> {
    let Ok(files) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = files
        .filter_map(|file| Some(file.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|e| e == EXTENSION) && path.is_file())
        .map(|path| entry(&path))
        .collect();
    entries.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
    entries
}

/// Gives the world of a project file a new name. The file must not be open.
/// The rename is one step of the undo log of the file.
pub fn rename(path: &Path, name: &str) -> Result<(), StoreError> {
    let (mut file, mut project) = ProjectFile::open(path)?;
    if project.world.name != name {
        project.world.name = name.to_string();
        file.save(&project)?;
    }
    Ok(())
}

/// Makes a copy of a project file in the folder of the file. The file must
/// hold each change of its project. The name of the new world is the old
/// name with " copy". Returns the path of the copy.
pub fn copy(path: &Path) -> Result<PathBuf, StoreError> {
    let name = format!("{} copy", vale_store::summary(path)?.name);
    let dir = path.parent().unwrap_or(Path::new("."));
    let to = new_project_path(dir, &name);
    std::fs::copy(path, &to)?;
    if let Err(e) = rename(&to, &name) {
        let _ = std::fs::remove_file(&to);
        return Err(e);
    }
    Ok(to)
}

/// Removes a project file. The file must not be open.
pub fn delete(path: &Path) -> Result<(), StoreError> {
    std::fs::remove_file(path)?;
    let mut journal = path.as_os_str().to_owned();
    journal.push("-journal");
    let _ = std::fs::remove_file(journal);
    Ok(())
}

/// The date of a time as `YYYY-MM-DD`, in UTC.
pub fn date_text(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    // The civil date of a day count, after Howard Hinnant's `civil_from_days`.
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The recent list of the folder: the paths of the projects that the app
/// opened, the newest first. A path with no file is not in the list.
pub fn recent(dir: &Path) -> Vec<PathBuf> {
    let text = std::fs::read_to_string(dir.join(RECENT_FILE)).unwrap_or_default();
    let paths = text.lines().filter(|line| !line.is_empty());
    paths.map(PathBuf::from).filter(|p| p.is_file()).collect()
}

/// Puts a path at the start of the recent list of the folder.
pub fn push_recent(dir: &Path, path: &Path) -> std::io::Result<()> {
    let path = std::path::absolute(path)?;
    let mut paths = recent(dir);
    paths.retain(|p| *p != path);
    paths.insert(0, path);
    paths.truncate(RECENT_MAX);
    let lines: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(RECENT_FILE), lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vale_store::{Project, World};

    fn create(dir: &Path, name: &str) -> PathBuf {
        let path = new_project_path(dir, name);
        let world = World {
            name: name.to_string(),
            ..World::default()
        };
        ProjectFile::create(&path, &Project::new(world)).unwrap();
        path
    }

    #[test]
    fn a_project_is_in_the_list_with_its_name_and_date() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(list(&dir.path().join("none")), []);
        let a = create(dir.path(), "Aurelia");
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        std::fs::write(dir.path().join("broken.gpkg"), "x").unwrap();

        let entries = list(dir.path());
        assert_eq!(entries.len(), 2);
        let found = entries.iter().find(|e| e.path == a).unwrap();
        assert_eq!(found.name, "Aurelia");
        assert_eq!(found.error, None);
        let today = date_text(SystemTime::now());
        assert_eq!(date_text(found.modified.unwrap()), today);
        let broken = entries.iter().find(|e| e.name == "broken").unwrap();
        assert!(broken.error.is_some());
    }

    #[test]
    fn rename_copy_and_delete_change_the_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = create(dir.path(), "Aurelia");
        rename(&path, "Borea").unwrap();
        assert_eq!(vale_store::summary(&path).unwrap().name, "Borea");

        let copied = copy(&path).unwrap();
        assert_eq!(copied.file_name().unwrap(), "Borea copy.gpkg");
        assert_eq!(vale_store::summary(&copied).unwrap().name, "Borea copy");
        assert_eq!(vale_store::summary(&path).unwrap().name, "Borea");

        delete(&path).unwrap();
        let names: Vec<String> = list(dir.path()).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["Borea copy"]);
    }

    #[test]
    fn dates() {
        let at = |secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        assert_eq!(date_text(at(0)), "1970-01-01");
        assert_eq!(date_text(at(951_782_400)), "2000-02-29");
        assert_eq!(date_text(at(1_791_763_199)), "2026-10-11");
        assert_eq!(date_text(at(1_791_763_200)), "2026-10-12");
    }

    #[test]
    fn the_recent_list_has_the_newest_path_first() {
        let dir = tempfile::tempdir().unwrap();
        let a = create(dir.path(), "A");
        let b = create(dir.path(), "B");
        push_recent(dir.path(), &a).unwrap();
        push_recent(dir.path(), &b).unwrap();
        push_recent(dir.path(), &a).unwrap();
        assert_eq!(recent(dir.path()), [a.clone(), b.clone()]);
        delete(&a).unwrap();
        assert_eq!(recent(dir.path()), [b]);
    }
}
