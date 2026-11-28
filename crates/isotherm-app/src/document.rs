//! The open world, its world file, and the import of a heightmap.

use std::path::{Path, PathBuf};

use isotherm_import::vale_project::{self, HeightLayer, LayerId};
use isotherm_import::{Samples, Scale};
use isotherm_store::{Restored, WorldFile};
use isotherm_world::{Heightmap, Planet, Terrain, World};
use vale_terrain::{FACES, TexelRect};

use crate::worlds;

/// Texels on one edge of a cube face of the sample world.
pub const SAMPLE_FACE_SIZE: usize = 256;

/// The largest face size of an imported terrain.
pub const MAX_FACE_SIZE: usize = 2048;

/// The sample heightmap of the Earth: 1,024 by 512 levels, a level minus
/// 32,768 is the height in meters.
const SAMPLE: &[u8] = include_bytes!("../data/earth.png");

/// The open world. A world with no file is the sample world, in memory.
pub struct Document {
    pub world: World,
    pub file: Option<WorldFile>,
    pub path: Option<PathBuf>,
}

/// What the read of an import file gives.
pub enum Read {
    /// A terrain that is ready.
    Terrain { name: String, terrain: Box<Terrain> },
    /// An image with no units. The import needs the heights of black and
    /// white.
    Scale { name: String, samples: Samples },
    /// A Vale project with more than one height layer. The user picks one.
    Layers {
        path: PathBuf,
        name: String,
        layers: Vec<HeightLayer>,
    },
}

/// What an undo or a redo changed in the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// The heightmap changed, so the globe needs its texels again.
    Terrain,
    /// The sea level or the planet changed.
    Other,
}

impl Document {
    /// The sample world in memory.
    pub fn sample() -> Result<Document, String> {
        let samples =
            isotherm_import::read_image("earth.png", SAMPLE).map_err(|e| e.to_string())?;
        let samples = stretch_signed_16(samples);
        let terrain = isotherm_import::to_terrain(&samples, Scale::default(), SAMPLE_FACE_SIZE)
            .map_err(|e| e.to_string())?;
        Ok(Document {
            world: World {
                name: "Earth".to_owned(),
                planet: Planet::earth(),
                terrain,
            },
            file: None,
            path: None,
        })
    }

    /// Opens a world file.
    pub fn open(path: &Path) -> Result<Document, String> {
        let (file, world) = WorldFile::open(path)
            .map_err(|e| format!("Could not open {}: {e}.", file_name(path)))?;
        Ok(Document {
            world,
            file: Some(file),
            path: Some(path.to_path_buf()),
        })
    }

    /// Puts an imported terrain in the world. With an open world file, the
    /// terrain replaces the terrain of the file as one undo step. Else the
    /// import makes a new world file in `dir`, named `name`.
    pub fn import(
        &mut self,
        name: &str,
        terrain: Terrain,
        dir: Option<&Path>,
    ) -> Result<(), String> {
        if let Some(file) = &mut self.file {
            file.replace_terrain(&terrain)
                .map_err(|e| format!("Could not save the world: {e}."))?;
            self.world.terrain = terrain;
            return Ok(());
        }
        let dir = dir.ok_or("This device has no folder for worlds.")?;
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Could not make the folder of worlds: {e}."))?;
        let path = worlds::new_path(dir, name);
        let world = World {
            name: name.to_owned(),
            planet: self.world.planet.clone(),
            terrain,
        };
        let file = WorldFile::create(&path, &world)
            .map_err(|e| format!("Could not make the world: {e}."))?;
        *self = Document {
            world,
            file: Some(file),
            path: Some(path),
        };
        Ok(())
    }

    /// Saves the sea level at the end of a drag, as one undo step. A world
    /// with no file keeps the sea level in memory.
    pub fn save_sea_level(&mut self, old_m: f64) -> Result<(), String> {
        let new_m = self.world.terrain.sea_level_m;
        match &mut self.file {
            Some(file) if old_m != new_m => file
                .set_sea_level(old_m, new_m)
                .map_err(|e| format!("Could not save the sea level: {e}.")),
            _ => Ok(()),
        }
    }

    pub fn can_undo(&self) -> bool {
        self.file.as_ref().is_some_and(WorldFile::can_undo)
    }

    pub fn can_redo(&self) -> bool {
        self.file.as_ref().is_some_and(WorldFile::can_redo)
    }

    /// Undoes the newest step. `None`: nothing changed.
    pub fn undo(&mut self) -> Result<Option<Change>, String> {
        let Some(file) = &mut self.file else {
            return Ok(None);
        };
        let restored = file.undo().map_err(|e| format!("Could not undo: {e}."))?;
        Ok(restored.map(|r| self.restore(r)))
    }

    /// Redoes the step that was undone last. `None`: nothing changed.
    pub fn redo(&mut self) -> Result<Option<Change>, String> {
        let Some(file) = &mut self.file else {
            return Ok(None);
        };
        let restored = file.redo().map_err(|e| format!("Could not redo: {e}."))?;
        Ok(restored.map(|r| self.restore(r)))
    }

    fn restore(&mut self, restored: Restored) -> Change {
        match restored {
            Restored::SeaLevel(m) => {
                self.world.terrain.sea_level_m = m;
                Change::Other
            }
            Restored::Planet(planet) => {
                self.world.planet = planet;
                Change::Other
            }
            Restored::Terrain(terrain) => {
                self.world.terrain = *terrain;
                Change::Terrain
            }
        }
    }
}

/// The name of a file with no folder.
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "the file".to_owned())
}

/// The name of a world from the file that it comes from.
fn world_name(path: &Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "World".to_owned())
}

/// Reads an import file: a PNG or TIFF heightmap, or a Vale project.
pub fn read_import(path: &Path) -> Result<Read, String> {
    let file = file_name(path);
    let name = world_name(path);
    let is_project = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(worlds::EXTENSION));
    if is_project {
        let layers = vale_project::height_layers(path)
            .map_err(|e| format!("Could not read {file}: {e}."))?;
        return match layers.as_slice() {
            [] => Err(format!("{file} has no height layers.")),
            [layer] => Ok(Read::Terrain {
                terrain: Box::new(layer_terrain(path, layer.id)?),
                name,
            }),
            _ => Ok(Read::Layers {
                path: path.to_path_buf(),
                name,
                layers,
            }),
        };
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read {file}: {e}."))?;
    let samples = isotherm_import::read_image(&file, &bytes)
        .map_err(|e| format!("Could not read {file}: {e}."))?;
    if samples.has_units() {
        let terrain = scale_terrain(&samples, Scale::default())?;
        return Ok(Read::Terrain {
            name,
            terrain: Box::new(terrain),
        });
    }
    Ok(Read::Scale { name, samples })
}

/// The terrain of an image, with the heights of black and white.
pub fn scale_terrain(samples: &Samples, scale: Scale) -> Result<Terrain, String> {
    let face_size = isotherm_import::face_size_for(samples.width()).min(MAX_FACE_SIZE);
    isotherm_import::to_terrain(samples, scale, face_size)
        .map_err(|e| format!("Could not import the heightmap: {e}."))
}

/// The terrain of a height layer of a Vale project, with at most
/// `MAX_FACE_SIZE` texels on the edge of a face.
pub fn layer_terrain(path: &Path, id: LayerId) -> Result<Terrain, String> {
    let file = file_name(path);
    let terrain =
        vale_project::read_layer(path, id).map_err(|e| format!("Could not read {file}: {e}."))?;
    Ok(Terrain {
        heights: reduced(terrain.heights, MAX_FACE_SIZE),
        ..terrain
    })
}

/// A heightmap with at most `max` texels on the edge of a face. Each texel
/// of a smaller heightmap is the mean of a block of texels.
pub fn reduced(map: Heightmap, max: usize) -> Heightmap {
    let n = map.face_size();
    if n <= max {
        return map;
    }
    let factor = n.div_ceil(max).next_power_of_two();
    let size = n.div_ceil(factor);
    let mut out = Heightmap::new(size, 0);
    let rect = TexelRect {
        x0: 0,
        y0: 0,
        x1: size,
        y1: size,
    };
    let mut levels = Vec::with_capacity(size * size);
    for face in 0..FACES {
        levels.clear();
        map.read_reduced(face, rect, factor, &mut levels);
        out.store_face(face, &levels);
    }
    out
}

/// Heights in meters from the levels of a signed 16-bit heightmap, where a
/// level minus 32,768 is the height. The range of the heights is that of the
/// data, so the sea level slider spans only the heights of the world.
fn stretch_signed_16(samples: Samples) -> Samples {
    let Samples::Levels {
        width,
        height,
        levels,
    } = samples
    else {
        return samples;
    };
    let low = levels.iter().copied().min().unwrap_or(0);
    let high = levels.iter().copied().max().unwrap_or(0).max(low + 1);
    let span = f64::from(high - low);
    let levels = levels
        .iter()
        .map(|&l| (f64::from(l - low) / span * 65535.0).round() as u16)
        .collect();
    let meters = |l: u16| f64::from(i32::from(l) - 32768);
    Samples::Meters {
        width,
        height,
        levels,
        low_m: meters(low),
        high_m: meters(high),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sea_level_of_the_sample_spans_the_heights_of_the_earth() {
        let terrain = Document::sample().unwrap().world.terrain;
        assert!(
            (-12_000.0..-9_000.0).contains(&terrain.low_m),
            "{}",
            terrain.low_m
        );
        assert!(
            (5_000.0..9_000.0).contains(&terrain.high_m),
            "{}",
            terrain.high_m
        );
        assert!(terrain.sea_level_m.abs() < 1.0);
    }

    #[test]
    fn a_large_heightmap_is_reduced_to_the_largest_face_size() {
        let mut map = Heightmap::new(16, 100);
        map.set(0, 0, 0, 300);
        let small = reduced(map, 4);
        assert_eq!(small.face_size(), 4);
        // The first texel is the mean of a block of 4 by 4 texels.
        let mut data = Vec::new();
        let rect = TexelRect {
            x0: 0,
            y0: 0,
            x1: 1,
            y1: 1,
        };
        small.read_rect(0, rect, &mut data);
        assert_eq!(data, [113]);
        assert_eq!(reduced(Heightmap::new(16, 0), 16).face_size(), 16);
    }
}
