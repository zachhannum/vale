//! The folders of the world files, the list of the worlds, and the world
//! that the next start opens.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use isotherm_store::{StoreError, Thumbnail, WorldFile};
use vale_shell::files::{Availability, Files, listed, placeholder_of, real_path};
use vale_shell::workcopy::Project;

use crate::resume::{Resume, View};

/// The extension of a world file.
pub const EXTENSION: &str = "gpkg";

/// The file in the state folder that holds the location of the next new
/// world.
const CHOICE_FILE: &str = "location.txt";
/// The folder in the state folder that holds the working copies.
const WORK_DIR: &str = "work";
/// The folder of a Mac or an iPad that holds each folder of iCloud Drive.
const SYNC_ROOT: &str = "Mobile Documents";

/// One of the two folders of the world files.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Location {
    /// The folder of the device.
    #[default]
    Local,
    /// The folder that iCloud Drive syncs.
    Cloud,
}

impl Location {
    pub const ALL: [Location; 2] = [Location::Cloud, Location::Local];

    /// The name of the location. `phone`: the device is an iPhone.
    pub fn label(self, phone: bool) -> &'static str {
        match (self, cfg!(target_os = "ios"), phone) {
            (Location::Cloud, _, _) => "iCloud Drive",
            (Location::Local, true, true) => "On this iPhone",
            (Location::Local, true, false) => "On this iPad",
            (Location::Local, false, _) => "On this computer",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Location::Local => "local",
            Location::Cloud => "cloud",
        }
    }
}

/// The folders of the world files, and the folder of the state of the app.
#[derive(Clone, Debug, PartialEq)]
pub struct Folders {
    local: PathBuf,
    cloud: Option<PathBuf>,
    state: PathBuf,
    choice: Location,
}

/// One world file in the list.
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
    pub location: Location,
    pub availability: Availability,
}

impl Folders {
    /// A local folder, and the folder of the state.
    pub fn new(local: PathBuf, state: PathBuf) -> Folders {
        let text = std::fs::read_to_string(state.join(CHOICE_FILE)).unwrap_or_default();
        // A device with iCloud Drive starts with it.
        let choice = match text.trim() {
            "local" => Location::Local,
            _ => Location::Cloud,
        };
        Folders {
            local,
            cloud: None,
            state,
            choice,
        }
    }

    /// Sets the iCloud folder. `None`: the device has no iCloud Drive.
    pub fn set_cloud(&mut self, dir: Option<PathBuf>) {
        self.cloud = dir;
    }

    pub fn cloud(&self) -> Option<&Path> {
        self.cloud.as_deref()
    }

    pub fn dir(&self, location: Location) -> Option<&Path> {
        match location {
            Location::Local => Some(&self.local),
            Location::Cloud => self.cloud.as_deref(),
        }
    }

    /// The location of the next new world: the last choice, if it has a
    /// folder.
    pub fn choice(&self) -> Location {
        match self.cloud {
            Some(_) => self.choice,
            None => Location::Local,
        }
    }

    /// Keeps the location of the next new world.
    pub fn set_choice(&mut self, location: Location) {
        self.choice = location;
        // The choice is a convenience, so its error has no message.
        let _ = std::fs::create_dir_all(&self.state);
        let _ = std::fs::write(self.state.join(CHOICE_FILE), location.key());
    }

    /// The folder of the next new world.
    pub fn new_world_dir(&self) -> &Path {
        match (self.choice, &self.cloud) {
            (Location::Cloud, Some(cloud)) => cloud,
            _ => &self.local,
        }
    }

    /// The location that holds a file, at each depth.
    pub fn location_of(&self, path: &Path) -> Option<Location> {
        let path = real_path(path);
        let holds = |location: &Location| {
            let dir = self.dir(*location).map(real_path);
            dir.is_some_and(|dir| path.starts_with(dir))
        };
        Location::ALL.into_iter().find(holds)
    }

    /// The folder of the working copy of a world file. `None`: the app
    /// edits the file in place. A file in a folder of iCloud Drive has a
    /// working copy, because the sync service must see only whole files.
    pub fn work_dir(&self, path: &Path) -> Option<PathBuf> {
        let synced = self.location_of(path) == Some(Location::Cloud)
            || path.components().any(|part| part.as_os_str() == SYNC_ROOT);
        synced.then(|| self.state.join(WORK_DIR))
    }

    /// The world files of each folder. The file with the newest change is
    /// the first. A Vale project is not in the list.
    pub fn list(&self, files: &dyn Files) -> Vec<Entry> {
        let mut entries = Vec::new();
        for location in Location::ALL {
            let Some(dir) = self.dir(location) else {
                continue;
            };
            for (path, availability) in listed(dir, EXTENSION, files) {
                let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                let mut entry = Entry {
                    name: stem(&path),
                    thumbnail: None,
                    modified,
                    error: None,
                    location,
                    availability,
                    path,
                };
                // A read of a file that is not on the device waits for its
                // download.
                if availability.on_device() {
                    match isotherm_store::summary(&entry.path) {
                        Ok(summary) => {
                            entry.name = summary.name;
                            entry.thumbnail = summary.thumbnail;
                        }
                        Err(StoreError::NotAWorld) => continue,
                        Err(e) => entry.error = Some(e.to_string()),
                    }
                }
                entries.push(entry);
            }
        }
        entries.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
        entries
    }

    /// Gives the world of a file that is not open a new name. The name of
    /// the file stays.
    pub fn rename(&self, path: &Path, name: &str, files: &dyn Files) -> Result<(), String> {
        let error = |e: String| format!("Could not rename {}: {e}.", file_name(path));
        let Some(work) = self.work_dir(path) else {
            return set_name(path, name).map_err(error);
        };
        let temp = scratch(&work).map_err(|e| error(e.to_string()))?;
        let result = (|| {
            files
                .read(path, &mut |from| std::fs::copy(from, &temp).map(drop))
                .map_err(|e| e.to_string())?;
            set_name(&temp, name)?;
            files
                .write(path, &mut |to| std::fs::copy(&temp, to).map(drop))
                .map_err(|e| e.to_string())
        })();
        remove_world(&temp);
        result.map_err(error)
    }

    /// Makes a copy of a world file that is not open, in its folder. The
    /// name of the copy is the old name with " copy". Returns the path of
    /// the copy.
    pub fn duplicate(&self, path: &Path, files: &dyn Files) -> Result<PathBuf, String> {
        let error = |e: String| format!("Could not duplicate {}: {e}.", file_name(path));
        let dir = path.parent().unwrap_or(Path::new("."));
        let temp = scratch(&self.state.join(WORK_DIR)).map_err(|e| error(e.to_string()))?;
        let result = (|| {
            files
                .read(path, &mut |from| std::fs::copy(from, &temp).map(drop))
                .map_err(|e| e.to_string())?;
            let summary = isotherm_store::summary(&temp).map_err(|e| e.to_string())?;
            let name = format!("{} copy", summary.name);
            set_name(&temp, &name)?;
            let to = new_path(dir, &name);
            files
                .write(&to, &mut |to| std::fs::copy(&temp, to).map(drop))
                .map_err(|e| e.to_string())?;
            Ok(to)
        })();
        remove_world(&temp);
        result.map_err(error)
    }

    /// Removes a world file that is not open, or its placeholder.
    pub fn delete(&self, path: &Path, files: &dyn Files) -> Result<(), String> {
        let delete = |path: &Path| {
            if !path.exists()
                && let Some(placeholder) = placeholder_of(path).filter(|p| p.exists())
            {
                return std::fs::remove_file(placeholder);
            }
            std::fs::remove_file(path)?;
            remove_world(path);
            Ok(())
        };
        let result = match self.work_dir(path) {
            Some(_) => files.write(path, &mut |path| delete(path)),
            None => delete(path),
        };
        result.map_err(|e| format!("Could not delete {}: {e}.", file_name(path)))
    }
}

/// The world files of Isotherm, for a working copy.
pub struct IsothermWorld;

impl Project for IsothermWorld {
    fn settle(&self, work: &Path) -> io::Result<()> {
        // An open of the database ends a transaction that the app left open.
        drop(WorldFile::open(work).map_err(|e| io::Error::other(e.to_string()))?);
        Ok(())
    }

    fn conflict_path(&self, source: &Path) -> PathBuf {
        let dir = source.parent().unwrap_or(Path::new("."));
        new_path(dir, &format!("{} conflict", stem(source)))
    }
}

fn set_name(path: &Path, name: &str) -> Result<(), String> {
    let (mut file, world) = WorldFile::open(path).map_err(|e| e.to_string())?;
    if world.name != name {
        file.set_name(name).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The path of a scratch file in the folder of the working copies.
fn scratch(work: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(work)?;
    Ok(work.join(format!("scratch-{}.{EXTENSION}", std::process::id())))
}

/// Removes a database and the files of its journal.
fn remove_world(path: &Path) {
    let _ = std::fs::remove_file(path);
    for end in ["-journal", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(end);
        let _ = std::fs::remove_file(name);
    }
}

fn stem(path: &Path) -> String {
    let stem = path.file_stem().unwrap_or(path.as_os_str());
    stem.to_string_lossy().into_owned()
}

fn file_name(path: &Path) -> String {
    crate::document::file_name(path)
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

/// The folder of the resume file and the working copies. On iOS, it is
/// a folder that the Files app does not show. Elsewhere, it is the folder of
/// the world files.
pub fn state_dir() -> Option<PathBuf> {
    if cfg!(target_os = "ios") {
        let home = std::env::var_os("HOME").filter(|v| !v.is_empty())?;
        Some(PathBuf::from(home).join("Library/Application Support/Isotherm"))
    } else {
        default_dir()
    }
}

/// The world file of the last run, if it still exists, and its view.
pub fn last_world(dir: &Path) -> Option<(PathBuf, Option<View>)> {
    let resume = Resume::read(dir)?;
    let path = resume.project.filter(|path| path.is_file())?;
    Some((path, resume.view))
}

/// The world file of the last run, also if it does not exist.
pub fn last_path(dir: &Path) -> Option<PathBuf> {
    Resume::read(dir)?.project
}

/// Keeps `path` and its view as the world that the next start opens.
pub fn remember(dir: &Path, path: &Path, view: View) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let resume = Resume {
        project: Some(path.to_path_buf()),
        view: Some(view),
    };
    resume.write(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create(dir: &Path, name: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = new_path(dir, name);
        let world = isotherm_world::World::flat(name, isotherm_world::Planet::earth(), 8);
        WorldFile::create(&path, &world).unwrap();
        path
    }

    fn folders() -> (tempfile::TempDir, Folders) {
        let dir = tempfile::tempdir().unwrap();
        let mut folders = Folders::new(dir.path().join("local"), dir.path().join("state"));
        folders.set_cloud(Some(dir.path().join("cloud")));
        (dir, folders)
    }

    fn set_modified(path: &Path, secs: u64) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn the_list_has_both_folders_newest_first_and_no_vale_project() {
        let (_dir, folders) = folders();
        let local = folders.dir(Location::Local).unwrap().to_path_buf();
        let cloud = folders.cloud().unwrap().to_path_buf();
        let old = create(&local, "Old");
        let new = create(&cloud, "New");
        let middle = create(&local, "Middle");
        set_modified(&old, 1_000);
        set_modified(&middle, 2_000);
        set_modified(&new, 3_000);
        let vale = local.join("Vale.gpkg");
        vale_store::ProjectFile::create(
            &vale,
            &vale_store::Project::new(vale_store::World::default()),
        )
        .unwrap();
        std::fs::write(cloud.join(".Far.gpkg.icloud"), b"x").unwrap();
        std::fs::write(local.join("Broken.gpkg"), b"").unwrap();

        let entries = folders.list(&vale_shell::files::PlainFiles);
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.starts_with(&["New", "Middle", "Old"]), "{names:?}");
        assert!(!names.contains(&"Vale"), "{names:?}");
        assert_eq!(entries[0].location, Location::Cloud);
        assert_eq!(entries[1].location, Location::Local);
        let far = entries.iter().find(|e| e.name == "Far").unwrap();
        assert_eq!(far.availability, Availability::InCloud);
        assert_eq!(far.path, cloud.join("Far.gpkg"));
    }

    #[test]
    fn only_a_file_in_the_cloud_has_a_working_copy() {
        let (dir, folders) = folders();
        let local = create(folders.dir(Location::Local).unwrap(), "A");
        let cloud = create(folders.cloud().unwrap(), "B");
        assert_eq!(folders.work_dir(&local), None);
        assert_eq!(
            folders.work_dir(&cloud),
            Some(dir.path().join("state/work"))
        );
        assert_eq!(folders.location_of(&cloud), Some(Location::Cloud));
    }

    #[test]
    fn the_next_new_world_goes_to_the_chosen_folder() {
        let (dir, mut folders) = folders();
        assert_eq!(folders.new_world_dir(), dir.path().join("cloud"));
        folders.set_choice(Location::Local);
        assert_eq!(folders.new_world_dir(), dir.path().join("local"));
        let mut again = Folders::new(dir.path().join("local"), dir.path().join("state"));
        assert_eq!(again.choice(), Location::Local);
        again.set_choice(Location::Cloud);
        // With no iCloud Drive, a new world goes to the local folder.
        assert_eq!(again.new_world_dir(), dir.path().join("local"));
    }

    #[test]
    fn rename_duplicate_and_delete_change_the_files() {
        let (_dir, folders) = folders();
        let files = vale_shell::files::PlainFiles;
        for location in Location::ALL {
            let path = create(folders.dir(location).unwrap(), "Aurelia");
            folders.rename(&path, "Borea", &files).unwrap();
            assert_eq!(isotherm_store::summary(&path).unwrap().name, "Borea");

            let copy = folders.duplicate(&path, &files).unwrap();
            assert_eq!(copy.file_name().unwrap(), "Borea copy.gpkg");
            assert_eq!(isotherm_store::summary(&copy).unwrap().name, "Borea copy");

            folders.delete(&path, &files).unwrap();
            assert!(!path.exists());
            let placeholder = placeholder_of(&copy).unwrap();
            std::fs::remove_file(&copy).unwrap();
            std::fs::write(&placeholder, b"x").unwrap();
            folders.delete(&copy, &files).unwrap();
            assert!(!placeholder.exists());
        }
        assert_eq!(folders.list(&files), []);
    }

    #[test]
    fn dates() {
        let at = |secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        assert_eq!(date_text(at(0)), "1970-01-01");
        assert_eq!(date_text(at(951_782_400)), "2000-02-29");
        assert_eq!(date_text(at(1_791_763_200)), "2026-10-12");
    }

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
        let view = crate::resume::tests::view();
        remember(dir.path(), &path, view).unwrap();
        assert_eq!(last_world(dir.path()), None);
        assert_eq!(last_path(dir.path()), Some(path.clone()));
        std::fs::write(&path, b"").unwrap();
        assert_eq!(last_world(dir.path()), Some((path, Some(view))));
    }
}
