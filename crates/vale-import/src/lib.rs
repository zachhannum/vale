//! Vector and raster data import.

pub mod geojson;
pub mod raster;

use std::fmt;
use std::path::PathBuf;

/// An import failure.
#[derive(Debug)]
pub enum ImportError {
    Io { path: PathBuf, message: String },
    Parse { name: String, message: String },
    NotGeoJson { name: String },
    NotRaster { name: String },
    Empty { name: String },
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Io { path, message } => {
                write!(f, "cannot read {}: {message}", path.display())
            }
            ImportError::Parse { name, message } => write!(f, "cannot parse {name}: {message}"),
            ImportError::NotGeoJson { name } => write!(f, "{name} is not GeoJSON"),
            ImportError::NotRaster { name } => write!(f, "{name} is not a PNG or TIFF image"),
            ImportError::Empty { name } => write!(f, "{name} has no usable features"),
        }
    }
}

impl std::error::Error for ImportError {}

pub use geojson::{Imported, ImportedLayer};
pub use raster::Greyscale;
