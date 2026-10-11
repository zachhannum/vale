//! The open project file. It saves the project a short time after a change.

use std::path::Path;
use std::time::{Duration, Instant};

use vale_store::{Project, ProjectFile, StoreError};

use crate::document::Document;

/// The time from a change to its save. One drag of a control changes the
/// project in each frame, and this delay turns the drag into a few saves.
pub const SAVE_DELAY: Duration = Duration::from_millis(500);

pub struct Session {
    file: ProjectFile,
    /// The project that the file holds.
    saved: Project,
    /// The time of the first change that the file does not hold.
    changed_at: Option<Instant>,
}

impl Session {
    /// Opens the project file and puts its project into the document. If no
    /// file is at the path, this makes the file from the document.
    pub fn open(path: &Path, doc: &mut Document) -> Result<Session, StoreError> {
        let file = if path.exists() {
            let (file, project) = ProjectFile::open(path)?;
            doc.set_project(project);
            file
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            ProjectFile::create(path, &doc.project)?
        };
        Ok(Session {
            file,
            saved: doc.project.clone(),
            changed_at: None,
        })
    }

    /// Call this in each frame. It saves a change that is older than
    /// `SAVE_DELAY`. It returns the time to the save of a newer change.
    pub fn tick(
        &mut self,
        project: &Project,
        now: Instant,
    ) -> Result<Option<Duration>, StoreError> {
        if *project == self.saved {
            self.changed_at = None;
            return Ok(None);
        }
        let age = now.duration_since(*self.changed_at.get_or_insert(now));
        if age < SAVE_DELAY {
            return Ok(Some(SAVE_DELAY - age));
        }
        self.flush(project)?;
        Ok(None)
    }

    /// Saves a change now.
    pub fn flush(&mut self, project: &Project) -> Result<(), StoreError> {
        self.changed_at = None;
        if *project != self.saved {
            self.file.save(project)?;
            self.saved = project.clone();
        }
        Ok(())
    }
}
