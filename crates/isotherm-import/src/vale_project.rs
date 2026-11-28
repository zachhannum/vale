//! The import of a height layer of a Vale project file.

use std::path::Path;

use isotherm_world::{Heightmap, Terrain};
use vale_store::{Project, StoreError, TILE_SIZE, TileReader};

pub use vale_store::LayerId;

use crate::ImportError;

/// The largest face size of a raster that a read accepts.
const MAX_FACE_SIZE: u32 = 16384;

/// A height layer of a Vale project.
#[derive(Clone, Debug, PartialEq)]
pub struct HeightLayer {
    pub id: LayerId,
    pub name: String,
    pub face_size: usize,
    /// The height of level 0, in meters.
    pub low_m: f64,
    /// The height of level 65,535, in meters.
    pub high_m: f64,
}

/// The world of a Vale project.
#[derive(Clone, Debug, PartialEq)]
pub struct ValeWorld {
    pub name: String,
    pub radius_m: f64,
}

/// The name and the radius of the world of a Vale project file.
pub fn world(path: &Path) -> Result<ValeWorld, ImportError> {
    let (_, project) = open(path)?;
    Ok(ValeWorld {
        name: project.world.name.clone(),
        radius_m: project.world.radius_km * 1000.0,
    })
}

/// The height layers of a Vale project file, in the order of its layers.
pub fn height_layers(path: &Path) -> Result<Vec<HeightLayer>, ImportError> {
    let (_, project) = open(path)?;
    let layers = project.layers().iter().filter_map(|layer| {
        let raster = project.raster(layer.raster()?)?;
        raster.is_height().then(|| HeightLayer {
            id: layer.id,
            name: layer.name.clone(),
            face_size: raster.face_size as usize,
            low_m: raster.value_min,
            high_m: raster.value_max,
        })
    });
    Ok(layers.collect())
}

/// Reads a height layer of a Vale project file. The terrain has the face
/// size and the levels of the layer.
pub fn read_layer(path: &Path, id: LayerId) -> Result<Terrain, ImportError> {
    let (reader, project) = open(path)?;
    let parse = |message: &str| ImportError::Parse {
        name: name_of(path),
        message: message.to_string(),
    };
    let raster = project
        .layer(id)
        .and_then(|layer| project.raster(layer.raster()?))
        .filter(|raster| raster.is_height())
        .ok_or_else(|| parse("the project has no such height layer"))?;
    if raster.face_size == 0 || raster.face_size > MAX_FACE_SIZE {
        return Err(parse("the face size of the layer is not valid"));
    }
    let (low_m, high_m) = (raster.value_min, raster.value_max);
    if !(low_m.is_finite() && high_m.is_finite() && low_m < high_m) {
        return Err(parse("the range of the heights is not valid"));
    }
    let face_size = raster.face_size as usize;
    let per_side = face_size.div_ceil(TILE_SIZE);
    let mut heights = Heightmap::new(face_size, raster.base);
    reader
        .load_tiles(raster, |key, pixels| {
            let (face, column, row) = (key.face as usize, key.column as usize, key.row as usize);
            if key.level == 0 && face < 6 && column < per_side && row < per_side {
                heights.load_tile(face, column, row, &pixels);
            }
        })
        .map_err(|e| store_error(path, e))?;
    Ok(Terrain::new(heights, low_m, high_m))
}

fn open(path: &Path) -> Result<(TileReader, Project), ImportError> {
    vale_store::read_project(path).map_err(|e| store_error(path, e))
}

fn store_error(path: &Path, e: StoreError) -> ImportError {
    match e {
        StoreError::Io(e) => ImportError::Io {
            path: path.to_path_buf(),
            message: e.to_string(),
        },
        other => ImportError::Parse {
            name: name_of(path),
            message: other.to_string(),
        },
    }
}

fn name_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_string())
}
