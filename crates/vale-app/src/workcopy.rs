//! The working copy of a project file in a folder that a sync service owns.
//! The app edits the copy in its own storage, and it writes the copy back
//! as one whole file.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use vale_store::ProjectFile;

use crate::files::Files;
use crate::session::new_project_path;

/// The longest time that a change waits in the working copy while the
/// project is open.
pub const WRITE_BACK_DELAY: Duration = Duration::from_secs(120);

/// The extension of the file that holds the source of a working copy.
const ORIGIN: &str = "origin";

/// The time of the last change of a file, and its size.
type Stamp = (SystemTime, u64);

fn stamp_of(path: &Path) -> Option<Stamp> {
    let data = std::fs::metadata(path).ok()?;
    Some((data.modified().ok()?, data.len()))
}

pub struct WorkCopy {
    /// The project file in the folder of the sync service.
    source: PathBuf,
    work: PathBuf,
    /// The stamp of the source when the copy and the source were equal.
    /// `None`: the source was not there.
    stamp: Option<Stamp>,
    /// The time of the first change that the source does not hold.
    dirty: Option<Instant>,
}

impl WorkCopy {
    /// Copies the source into `dir`. If no file is at the source, the copy
    /// has no file. If `dir` holds a copy of an earlier run with the name
    /// of the source, the source of that copy gets it first.
    pub fn open(source: &Path, dir: &Path, files: &dyn Files) -> io::Result<WorkCopy> {
        std::fs::create_dir_all(dir)?;
        let name = source.file_name().ok_or(io::ErrorKind::InvalidInput)?;
        let work = dir.join(name);
        if origin_path(&work).exists() {
            recover_one(&work, files)?;
        }
        let mut stamp = None;
        if source.exists() {
            files.read(source, &mut |from| {
                std::fs::copy(from, &work)?;
                stamp = stamp_of(from);
                Ok(())
            })?;
        }
        let copy = WorkCopy {
            source: source.to_path_buf(),
            work,
            stamp,
            dirty: None,
        };
        copy.write_origin()?;
        Ok(copy)
    }

    /// The path of the copy.
    pub fn path(&self) -> &Path {
        &self.work
    }

    /// The file that gets the copy at the next write-back.
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Call this after each change of the copy.
    pub fn mark_changed(&mut self, now: Instant) {
        if self.dirty.is_none() {
            self.dirty = Some(now);
            // The copy keeps the change if the app stops here.
            let _ = self.write_origin();
        }
    }

    /// Drops the changes that the source does not hold.
    pub fn forget(&mut self) {
        self.dirty = None;
    }

    /// True if a change waits longer than `WRITE_BACK_DELAY`.
    pub fn due(&self, now: Instant) -> bool {
        self.dirty
            .is_some_and(|at| now.duration_since(at) >= WRITE_BACK_DELAY)
    }

    /// Writes the copy to the source as one whole file, if the copy has a
    /// change. The database of the copy must have no open transaction. If
    /// the source changed after the copy, the copy goes to a new file
    /// beside the source, and that file is the source from then on.
    pub fn write_back(&mut self, files: &dyn Files) -> io::Result<()> {
        if self.dirty.is_none() {
            return Ok(());
        }
        let (source, stamp) = put(&self.work, &self.source, self.stamp, files)?;
        self.source = source;
        self.stamp = stamp;
        self.dirty = None;
        self.write_origin()
    }

    fn write_origin(&self) -> io::Result<()> {
        let stamp = match self.stamp {
            Some((time, size)) => {
                let since = time.duration_since(SystemTime::UNIX_EPOCH);
                let since = since.unwrap_or(Duration::ZERO);
                format!("{} {} {size}", since.as_secs(), since.subsec_nanos())
            }
            None => "none".to_string(),
        };
        let state = if self.dirty.is_some() {
            "dirty"
        } else {
            "clean"
        };
        let text = format!("{}\n{stamp}\n{state}\n", self.source.display());
        std::fs::write(origin_path(&self.work), text)
    }
}

impl Drop for WorkCopy {
    fn drop(&mut self) {
        // A copy with a change stays for the next start of the app.
        if self.dirty.is_none() {
            remove(&self.work);
        }
    }
}

fn origin_path(work: &Path) -> PathBuf {
    let mut name = work.as_os_str().to_owned();
    name.push(".");
    name.push(ORIGIN);
    PathBuf::from(name)
}

fn remove(work: &Path) {
    let mut journal = work.as_os_str().to_owned();
    journal.push("-journal");
    for path in [
        work.to_path_buf(),
        PathBuf::from(journal),
        origin_path(work),
    ] {
        let _ = std::fs::remove_file(path);
    }
}

/// Writes the copy to the source. If the source does not have this stamp,
/// the copy goes to a new file beside it. Returns the file that got the
/// copy, and its stamp.
fn put(
    work: &Path,
    source: &Path,
    stamp: Option<Stamp>,
    files: &dyn Files,
) -> io::Result<(PathBuf, Option<Stamp>)> {
    let dir = source.parent().unwrap_or(Path::new("."));
    let to = if source.exists() && stamp_of(source) != stamp {
        let stem = source.file_stem().unwrap_or_default().to_string_lossy();
        new_project_path(dir, &format!("{stem} conflict"))
    } else {
        source.to_path_buf()
    };
    std::fs::create_dir_all(dir)?;
    let mut stamp = None;
    files.write(&to, &mut |to| {
        // The sync service must see only the whole file.
        let name = to.file_name().unwrap_or_default().to_string_lossy();
        let part = to.with_file_name(format!(".{name}.part"));
        std::fs::copy(work, &part)?;
        std::fs::rename(&part, to)?;
        stamp = stamp_of(to);
        Ok(())
    })?;
    Ok((to, stamp))
}

/// Gives a copy of an earlier run to its source, and removes the copy.
/// Returns the file that got the copy. `None`: the copy had no change.
fn recover_one(work: &Path, files: &dyn Files) -> io::Result<Option<PathBuf>> {
    let text = std::fs::read_to_string(origin_path(work))?;
    let mut lines = text.lines();
    let source = PathBuf::from(lines.next().unwrap_or_default());
    let stamp = lines.next().and_then(|line| {
        let mut parts = line.split(' ').map(str::parse::<u64>);
        let (secs, nanos, size) = (
            parts.next()?.ok()?,
            parts.next()?.ok()?,
            parts.next()?.ok()?,
        );
        let time = SystemTime::UNIX_EPOCH + Duration::new(secs, nanos as u32);
        Some((time, size))
    });
    let dirty = lines.next() == Some("dirty");
    let mut to = None;
    if dirty && work.exists() && source.file_name().is_some() {
        // An open of the database ends a transaction that the app left open.
        drop(ProjectFile::open(work).map_err(|e| io::Error::other(e.to_string()))?);
        to = Some(put(work, &source, stamp, files)?.0);
    }
    remove(work);
    Ok(to)
}

/// Gives each copy of an earlier run in `dir` to its source. Returns the
/// files that got a copy, and the text of each error.
pub fn recover(dir: &Path, files: &dyn Files) -> (Vec<PathBuf>, Vec<String>) {
    let (mut saved, mut errors) = (Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (saved, errors);
    };
    for path in entries.filter_map(|entry| Some(entry.ok()?.path())) {
        if path.extension().is_some_and(|e| e == ORIGIN) {
            let work = path.with_extension("");
            match recover_one(&work, files) {
                Ok(to) => saved.extend(to),
                Err(e) => errors.push(format!("{}: {e}", work.display())),
            }
        }
    }
    (saved, errors)
}
