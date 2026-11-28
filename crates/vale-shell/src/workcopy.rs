//! The working copy of a project file in a folder that a sync service owns.
//! The app edits the copy in its own storage, and it writes the copy back
//! as one whole file.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::files::Files;

/// What the working copy needs to know about the project files of an app.
pub trait Project {
    /// Makes the file at `work` whole after a stop of the app, for example
    /// by the end of a transaction that the app left open.
    fn settle(&self, work: &Path) -> io::Result<()>;

    /// A free path beside `source` for a copy that conflicts with it.
    fn conflict_path(&self, source: &Path) -> PathBuf;
}

/// The time with no change after which the working copy goes back to the
/// source.
pub const WRITE_BACK_IDLE: Duration = Duration::from_secs(15);

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
    /// The time of the last change.
    last: Option<Instant>,
}

impl WorkCopy {
    /// Copies the source into `dir`. If no file is at the source, the copy
    /// has no file. If `dir` holds a copy of an earlier run with the name
    /// of the source, the source of that copy gets it first.
    pub fn open(
        source: &Path,
        dir: &Path,
        files: &dyn Files,
        project: &dyn Project,
    ) -> io::Result<WorkCopy> {
        std::fs::create_dir_all(dir)?;
        let name = source.file_name().ok_or(io::ErrorKind::InvalidInput)?;
        let work = dir.join(name);
        if origin_path(&work).exists() {
            recover_one(&work, files, project)?;
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
            last: None,
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
        self.last = Some(now);
        if self.dirty.is_none() {
            self.dirty = Some(now);
            // The copy keeps the change if the app stops here.
            let _ = self.write_origin();
        }
    }

    /// True if the copy has a change that the source does not hold.
    pub fn is_dirty(&self) -> bool {
        self.dirty.is_some()
    }

    /// True if the source changed after the copy or the last write-back.
    pub fn source_changed(&self) -> bool {
        self.source.exists() && stamp_of(&self.source) != self.stamp
    }

    /// The time to the next write-back. `None`: the copy has no change.
    pub fn due_in(&self, now: Instant) -> Option<Duration> {
        let first = self.dirty? + WRITE_BACK_DELAY;
        let idle = self.last.map_or(first, |last| last + WRITE_BACK_IDLE);
        Some(first.min(idle).saturating_duration_since(now))
    }

    /// Drops the changes that the source does not hold.
    pub fn forget(&mut self) {
        self.dirty = None;
    }

    /// True if the copy has a change, and the last change is older than
    /// `WRITE_BACK_IDLE` or the first change is older than
    /// `WRITE_BACK_DELAY`.
    pub fn due(&self, now: Instant) -> bool {
        let older =
            |at: Option<Instant>, limit| at.is_some_and(|at| now.duration_since(at) >= limit);
        self.dirty.is_some()
            && (older(self.last, WRITE_BACK_IDLE) || older(self.dirty, WRITE_BACK_DELAY))
    }

    /// Writes the copy to the source as one whole file, if the copy has a
    /// change. The database of the copy must have no open transaction. If
    /// the source changed after the copy, the copy goes to a new file
    /// beside the source, and that file is the source from then on.
    pub fn write_back(&mut self, files: &dyn Files, project: &dyn Project) -> io::Result<()> {
        if self.dirty.is_none() {
            return Ok(());
        }
        let (source, stamp) = put(&self.work, &self.source, self.stamp, files, project)?;
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
    project: &dyn Project,
) -> io::Result<(PathBuf, Option<Stamp>)> {
    let dir = source.parent().unwrap_or(Path::new("."));
    let to = if source.exists() && stamp_of(source) != stamp {
        project.conflict_path(source)
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
fn recover_one(
    work: &Path,
    files: &dyn Files,
    project: &dyn Project,
) -> io::Result<Option<PathBuf>> {
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
        project.settle(work)?;
        to = Some(put(work, &source, stamp, files, project)?.0);
    }
    remove(work);
    Ok(to)
}

/// Gives each copy of an earlier run in `dir` to its source. Returns the
/// files that got a copy, and the text of each error.
pub fn recover(
    dir: &Path,
    files: &dyn Files,
    project: &dyn Project,
) -> (Vec<PathBuf>, Vec<String>) {
    let (mut saved, mut errors) = (Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (saved, errors);
    };
    for path in entries.filter_map(|entry| Some(entry.ok()?.path())) {
        if path.extension().is_some_and(|e| e == ORIGIN) {
            let work = path.with_extension("");
            match recover_one(&work, files, project) {
                Ok(to) => saved.extend(to),
                Err(e) => errors.push(format!("{}: {e}", work.display())),
            }
        }
    }
    (saved, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::PlainFiles;

    /// Project files that need no repair, with a conflict name of a suffix.
    struct Plain;

    impl Project for Plain {
        fn settle(&self, _work: &Path) -> io::Result<()> {
            Ok(())
        }

        fn conflict_path(&self, source: &Path) -> PathBuf {
            source.with_extension("conflict")
        }
    }

    fn copy(dir: &Path) -> WorkCopy {
        let source = dir.join("World.gpkg");
        std::fs::write(&source, "x").unwrap();
        WorkCopy::open(&source, &dir.join("work"), &PlainFiles, &Plain).unwrap()
    }

    #[test]
    fn a_change_goes_back_to_the_source_or_beside_a_changed_source() {
        let dir = tempfile::tempdir().unwrap();
        let mut copy = copy(dir.path());
        let source = dir.path().join("World.gpkg");
        std::fs::write(copy.path(), "y").unwrap();
        copy.mark_changed(Instant::now());
        copy.write_back(&PlainFiles, &Plain).unwrap();
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "y");
        assert!(!copy.is_dirty());

        // Another device changes the source.
        std::fs::write(&source, "other device").unwrap();
        std::fs::write(copy.path(), "z").unwrap();
        copy.mark_changed(Instant::now());
        copy.write_back(&PlainFiles, &Plain).unwrap();
        let conflict = dir.path().join("World.conflict");
        assert_eq!(copy.source(), conflict);
        assert_eq!(std::fs::read_to_string(&conflict).unwrap(), "z");
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "other device");
    }

    #[test]
    fn a_copy_with_a_change_goes_back_at_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        let mut copy = copy(dir.path());
        std::fs::write(copy.path(), "y").unwrap();
        copy.mark_changed(Instant::now());
        // The app stops with the change in the copy.
        drop(copy);
        let (saved, errors) = recover(&work, &PlainFiles, &Plain);
        let source = dir.path().join("World.gpkg");
        assert_eq!(
            (saved, errors),
            (vec![source.clone()], Vec::<String>::new())
        );
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "y");
        assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0);
    }

    #[test]
    fn a_change_is_due_when_it_is_idle_for_the_idle_time() {
        let dir = tempfile::tempdir().unwrap();
        let mut copy = copy(dir.path());
        let t = Instant::now();
        assert!(!copy.due(t + WRITE_BACK_DELAY));
        copy.mark_changed(t);
        assert!(!copy.due(t + WRITE_BACK_IDLE - Duration::from_secs(1)));
        assert!(copy.due(t + WRITE_BACK_IDLE));
    }

    #[test]
    fn steady_changes_are_due_after_the_longest_delay() {
        let dir = tempfile::tempdir().unwrap();
        let mut copy = copy(dir.path());
        let t = Instant::now();
        let step = Duration::from_secs(5);
        let mut now = t;
        while now < t + WRITE_BACK_DELAY {
            copy.mark_changed(now);
            assert!(!copy.due(now));
            now += step;
        }
        copy.mark_changed(now);
        assert!(copy.due(now));
    }
}
