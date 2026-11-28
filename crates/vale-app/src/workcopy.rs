//! The working copy of a Vale project file.

use std::io;
use std::path::{Path, PathBuf};

use vale_store::ProjectFile;

use crate::session::new_project_path;

pub use vale_shell::workcopy::*;

/// The project files of Vale: GeoPackage files.
pub struct ValeProject;

impl Project for ValeProject {
    fn settle(&self, work: &Path) -> io::Result<()> {
        // An open of the database ends a transaction that the app left open.
        drop(ProjectFile::open(work).map_err(|e| io::Error::other(e.to_string()))?);
        Ok(())
    }

    fn conflict_path(&self, source: &Path) -> PathBuf {
        let dir = source.parent().unwrap_or(Path::new("."));
        let stem = source.file_stem().unwrap_or_default().to_string_lossy();
        new_project_path(dir, &format!("{stem} conflict"))
    }
}
