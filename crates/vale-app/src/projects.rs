//! The projects folders: the list of the project files, and the rename,
//! copy, and delete operations on a file that is not open.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use vale_store::{ProjectFile, StoreError, Thumbnail};

use crate::files::{Availability, Files, Version};
use crate::session::{EXTENSION, new_project_path};

/// The file in the state folder that holds the recent list.
const RECENT_FILE: &str = "recent.txt";
/// The file in the state folder that holds the location of the last new
/// world.
const CHOICE_FILE: &str = "location.txt";
/// The extension of the file that iCloud Drive keeps in place of a file
/// that is not on the device. The name of that file starts with a dot.
const PLACEHOLDER: &str = "icloud";

/// The folder in the state folder that holds the working copies.
const WORK_DIR: &str = "work";
/// The folder of a Mac that holds each folder of iCloud Drive.
const SYNC_ROOT: &str = "Mobile Documents";

/// One of the two folders that hold the projects.
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

    /// The name of the location. `pad`: the device is an iPad.
    pub fn label(self, pad: bool) -> &'static str {
        match (self, pad) {
            (Location::Cloud, _) => "iCloud Drive",
            (Location::Local, true) => "On this iPad",
            (Location::Local, false) => "On this computer",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Location::Local => "local",
            Location::Cloud => "cloud",
        }
    }
}

/// The folders of the projects, and the folder of the state of the app.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Locations {
    local: Option<PathBuf>,
    cloud: Option<PathBuf>,
    /// The folder of the recent list and of the last choice.
    state: Option<PathBuf>,
    choice: Location,
}

impl Locations {
    /// A local folder, and the folder of the state.
    pub fn new(local: Option<PathBuf>, state: Option<PathBuf>) -> Locations {
        let read = |dir: &PathBuf| std::fs::read_to_string(dir.join(CHOICE_FILE)).ok();
        let choice = match state.as_ref().and_then(read) {
            Some(text) if text.trim() == Location::Local.key() => Location::Local,
            // A device with iCloud Drive starts with it.
            _ => Location::Cloud,
        };
        Locations {
            local,
            cloud: None,
            state,
            choice,
        }
    }

    /// One folder that holds the projects and the state.
    pub fn single(dir: impl Into<PathBuf>) -> Locations {
        let dir = dir.into();
        Locations::new(Some(dir.clone()), Some(dir))
    }

    /// Sets the iCloud folder. `None`: the device has no iCloud Drive.
    pub fn set_cloud(&mut self, dir: Option<PathBuf>) {
        self.cloud = dir;
    }

    pub fn dir(&self, location: Location) -> Option<&Path> {
        match location {
            Location::Local => self.local.as_deref(),
            Location::Cloud => self.cloud.as_deref(),
        }
    }

    /// True if the app has a folder for projects.
    pub fn any(&self) -> bool {
        self.local.is_some() || self.cloud.is_some()
    }

    /// The locations that have a folder, in the order of the form.
    pub fn available(&self) -> Vec<Location> {
        let has = |location: &Location| self.dir(*location).is_some();
        Location::ALL.into_iter().filter(has).collect()
    }

    pub fn state_dir(&self) -> Option<&Path> {
        self.state.as_deref()
    }

    /// The location of the next new world: the last choice, if it has a
    /// folder.
    pub fn choice(&self) -> Location {
        let other = match self.choice {
            Location::Local => Location::Cloud,
            Location::Cloud => Location::Local,
        };
        if self.dir(self.choice).is_none() && self.dir(other).is_some() {
            other
        } else {
            self.choice
        }
    }

    /// Keeps the location of the next new world.
    pub fn set_choice(&mut self, location: Location) {
        self.choice = location;
        if let Some(dir) = &self.state {
            // The choice is a convenience, so its error has no message.
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(dir.join(CHOICE_FILE), location.key());
        }
    }

    /// The folder of the next new world.
    pub fn new_world_dir(&self) -> Option<&Path> {
        self.dir(self.choice())
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

    /// The folder of the working copy of a project file. `None`: the app
    /// edits the file in place. A file in a folder of iCloud Drive has a
    /// working copy, because the sync service must see only whole files.
    pub fn work_dir(&self, path: &Path) -> Option<PathBuf> {
        let synced = self.location_of(path) == Some(Location::Cloud)
            || path.components().any(|part| part.as_os_str() == SYNC_ROOT);
        let state = self.state.as_ref().filter(|_| synced)?;
        Some(state.join(WORK_DIR))
    }

    /// Moves each project file of the state folder to the local folder. An
    /// older version of the iPad app kept its projects in the state folder,
    /// which the Files app does not show. Returns the count of the files.
    pub fn adopt_old_projects(&self) -> usize {
        let (Some(state), Some(local)) = (&self.state, &self.local) else {
            return 0;
        };
        if real_path(state) == real_path(local) || std::fs::create_dir_all(local).is_err() {
            return 0;
        }
        let moved = |path: &PathBuf| {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            std::fs::rename(path, new_project_path(local, &stem)).is_ok()
        };
        let old = project_paths(state).into_iter();
        old.filter(|path| placeholder_target(path).is_none())
            .filter(moved)
            .count()
    }

    /// The folder of the working copies.
    pub fn all_work_dir(&self) -> Option<PathBuf> {
        Some(self.state.as_ref()?.join(WORK_DIR))
    }

    /// The project files of each folder. The file with the newest change is
    /// the first.
    pub fn list(&self, files: &dyn Files) -> Vec<Entry> {
        let mut entries = Vec::new();
        for location in Location::ALL {
            if let Some(dir) = self.dir(location) {
                entries.extend(list_dir(dir, location, files));
            }
        }
        sort(&mut entries);
        entries
    }

    /// The project files of each folder that are on the device.
    pub fn paths(&self, files: &dyn Files) -> Vec<PathBuf> {
        let here = |entry: &Entry| entry.availability == Availability::Here;
        let entries = self.list_names(files).into_iter().filter(here);
        entries.map(|entry| entry.path).collect()
    }

    /// The entries of each folder with no read of a project file.
    fn list_names(&self, files: &dyn Files) -> Vec<Entry> {
        let mut entries = Vec::new();
        for location in Location::ALL {
            if let Some(dir) = self.dir(location) {
                for path in project_paths(dir) {
                    let availability = availability(&path, files);
                    let path = placeholder_target(&path).unwrap_or(path);
                    entries.push(Entry::unread(path, location, availability));
                }
            }
        }
        entries
    }
}

/// The path with no symbolic link in its folder. The path of a file from
/// the system can name the same folder in a different way.
fn real_path(path: &Path) -> PathBuf {
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

/// The project file that a placeholder of iCloud Drive stands for.
fn placeholder_target(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let name = name.strip_prefix('.')?;
    let name = name.strip_suffix(PLACEHOLDER)?.strip_suffix('.')?;
    let target = path.with_file_name(name);
    (target.extension().is_some_and(|e| e == EXTENSION)).then_some(target)
}

/// The placeholder of iCloud Drive for a project file.
fn placeholder_of(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    Some(path.with_file_name(format!(".{name}.{PLACEHOLDER}")))
}

/// The project files and the placeholders of a folder. A placeholder of a
/// file that is in the folder is not in the list.
fn project_paths(dir: &Path) -> Vec<PathBuf> {
    let Ok(files) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let project = |path: &PathBuf| match placeholder_target(path) {
        Some(target) => !target.exists(),
        None => path.extension().is_some_and(|e| e == EXTENSION) && path.is_file(),
    };
    let paths = files.filter_map(|file| Some(file.ok()?.path()));
    paths.filter(project).collect()
}

fn availability(path: &Path, files: &dyn Files) -> Availability {
    match placeholder_target(path) {
        Some(target) => match files.availability(&target) {
            Availability::Here => Availability::InCloud,
            other => other,
        },
        None => files.availability(path),
    }
}

fn list_dir(dir: &Path, location: Location, files: &dyn Files) -> Vec<Entry> {
    let entry_of = |path: PathBuf| match availability(&path, files) {
        Availability::Here => Entry {
            location,
            conflicts: files.conflicts(&path),
            ..entry(&path)
        },
        // A read of a file that is not on the device waits for its download.
        other => Entry::unread(placeholder_target(&path).unwrap_or(path), location, other),
    };
    project_paths(dir).into_iter().map(entry_of).collect()
}

fn sort(entries: &mut [Entry]) {
    entries.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
}
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
    pub location: Location,
    pub availability: Availability,
    /// The other versions of the file, if two devices changed it.
    pub conflicts: Vec<Version>,
}

impl Entry {
    /// The entry of a file that the app did not read. The name is the name
    /// of the file.
    fn unread(path: PathBuf, location: Location, availability: Availability) -> Entry {
        let stem = path.file_stem().unwrap_or(path.as_os_str());
        Entry {
            name: stem.to_string_lossy().into_owned(),
            thumbnail: None,
            modified: std::fs::metadata(&path).and_then(|m| m.modified()).ok(),
            error: None,
            location,
            availability,
            conflicts: Vec::new(),
            path,
        }
    }
}

fn var(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name).filter(|v| !v.is_empty())
}

/// The folder of the state of the app on Apple platforms.
fn apple_support_dir() -> Option<PathBuf> {
    // On iOS, `HOME` is the container of the app.
    let home = PathBuf::from(var("HOME")?);
    Some(home.join("Library/Application Support/Vale"))
}

/// The folders of this device. The iCloud folder comes later on iPad,
/// because the system gives it on a worker thread.
pub fn default_locations() -> Locations {
    let mut locations = Locations::new(default_dir(), default_state_dir());
    if cfg!(target_os = "macos") {
        locations.set_cloud(mac_cloud_dir());
    }
    locations
}

/// The folder of the recent list and of the last choice. On iPad, the
/// Files app does not show this folder.
fn default_state_dir() -> Option<PathBuf> {
    if cfg!(target_os = "ios") {
        apple_support_dir()
    } else {
        default_dir()
    }
}

/// The folder that the Finder shows as "iCloud Drive > Vale", if this Mac
/// has it.
fn mac_cloud_dir() -> Option<PathBuf> {
    let home = PathBuf::from(var("HOME")?);
    let dir = home.join("Library/Mobile Documents/iCloud~dev~vale~app/Documents");
    dir.is_dir().then_some(dir)
}

/// The local projects folder of this device. `None`: the system gives no
/// home folder.
pub fn default_dir() -> Option<PathBuf> {
    if cfg!(target_os = "ios") {
        // The Files app shows this folder as "On My iPad > Vale".
        Some(PathBuf::from(var("HOME")?).join("Documents"))
    } else if cfg!(target_os = "macos") {
        apple_support_dir()
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
            location: Location::Local,
            availability: Availability::Here,
            conflicts: Vec::new(),
        },
        Err(e) => Entry {
            path: path.to_path_buf(),
            name: stem(),
            thumbnail: None,
            modified,
            error: Some(e.to_string()),
            location: Location::Local,
            availability: Availability::Here,
            conflicts: Vec::new(),
        },
    }
}

/// The project files of one plain folder. The file with the newest change
/// is the first. A folder that is not there gives an empty list.
pub fn list(dir: &Path) -> Vec<Entry> {
    Locations::new(Some(dir.to_path_buf()), None).list(&crate::files::PlainFiles)
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

fn io_error(e: StoreError) -> std::io::Error {
    match e {
        StoreError::Io(e) => e,
        other => std::io::Error::other(other.to_string()),
    }
}

/// The path of a scratch file in the folder of the working copies.
fn scratch(work_dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(work_dir)?;
    Ok(work_dir.join(format!("scratch-{}.{EXTENSION}", std::process::id())))
}

/// `rename` for a file in a folder that a sync service owns. The change is
/// on a copy in `work_dir`, and the file gets the copy as one whole file.
pub fn rename_synced(
    path: &Path,
    name: &str,
    work_dir: &Path,
    files: &dyn Files,
) -> Result<(), StoreError> {
    let temp = scratch(work_dir)?;
    let result = (|| {
        files.read(path, &mut |from| std::fs::copy(from, &temp).map(drop))?;
        rename(&temp, name)?;
        files.write(path, &mut |to| std::fs::copy(&temp, to).map(drop))?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&temp);
    result
}

/// `copy` for a file in a folder that a sync service owns.
pub fn copy_synced(path: &Path, work_dir: &Path, files: &dyn Files) -> Result<PathBuf, StoreError> {
    let temp = scratch(work_dir)?;
    let result = (|| {
        files.read(path, &mut |from| std::fs::copy(from, &temp).map(drop))?;
        let name = format!("{} copy", vale_store::summary(&temp)?.name);
        rename(&temp, &name)?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let to = new_project_path(dir, &name);
        files.write(&to, &mut |to| std::fs::copy(&temp, to).map(drop))?;
        Ok(to)
    })();
    let _ = std::fs::remove_file(&temp);
    result
}

/// `delete` for a file in a folder that a sync service owns.
pub fn delete_synced(path: &Path, files: &dyn Files) -> Result<(), StoreError> {
    files.write(path, &mut |path| delete(path).map_err(io_error))?;
    Ok(())
}

/// Removes a project file. The file must not be open.
pub fn delete(path: &Path) -> Result<(), StoreError> {
    if !path.exists()
        && let Some(placeholder) = placeholder_of(path).filter(|p| p.exists())
    {
        std::fs::remove_file(placeholder)?;
        return Ok(());
    }
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

/// The date and the time of a time as `YYYY-MM-DD HH:MM`, in UTC.
pub fn time_text(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let (hour, minute) = (secs / 3600 % 24, secs / 60 % 60);
    format!("{} {hour:02}:{minute:02}", date_text(time))
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
        assert_eq!(time_text(at(1_791_763_199)), "2026-10-11 23:59");
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
