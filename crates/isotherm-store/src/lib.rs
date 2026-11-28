//! The world file of Isotherm: a GeoPackage with `isotherm_` tables.
//!
//! The `isotherm_` tables hold the name, the planet and its moons, the
//! terrain and its tiles, and the undo log. They are in `gpkg_contents` with
//! the data type `isotherm`. GDAL lists a table that is not in
//! `gpkg_contents` as a layer, and it does not list a table with a data type
//! that it does not know.

mod tile;
mod world_file;

pub use world_file::{
    FORMAT_VERSION, Restored, StoreError, UNDO_STEPS, UNDO_TERRAIN_BYTES, WorldFile,
};

/// The prefix of each table that Isotherm adds to the GeoPackage.
pub const TABLE_PREFIX: &str = "isotherm_";
