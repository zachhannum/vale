//! The open project file. It saves the project and the changed tiles of the
//! heightmap a short time after a change.

use std::path::Path;
use std::time::{Duration, Instant};

use vale_store::{Project, ProjectFile, Raster, SaveStats, StoreError, TileChanges, TileKey};
use vale_terrain::{Bands, ELEV_MAX, ELEV_MIN, Heightmap, Ramp};

use crate::document::Document;

/// The time from a change to its save. One drag of a control changes the
/// project in each frame, and this delay turns the drag into a few saves.
pub const SAVE_DELAY: Duration = Duration::from_millis(500);

/// The name of the raster that the app makes for its heightmap.
const HEIGHT_NAME: &str = "Height";
/// The largest face size of a heightmap that the app loads.
const MAX_FACE_SIZE: u32 = 16384;

pub struct Session {
    file: ProjectFile,
    /// The project that the file holds.
    saved: Project,
    /// The time of the first change that the file does not hold.
    changed_at: Option<Instant>,
    /// The tile rows that the last save changed.
    pub last_save: SaveStats,
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
            last_save: SaveStats::default(),
        })
    }

    /// The heightmap of the file, with the face size, the base level, the
    /// bands, and the tiles that the file holds. The heightmap is the first
    /// raster of the project. `None`: the project has no raster.
    pub fn load_map(&self) -> Result<Option<Heightmap>, StoreError> {
        let Some(raster) = self.saved.rasters().first() else {
            return Ok(None);
        };
        if raster.face_size > MAX_FACE_SIZE {
            return Err(StoreError::Corrupt(format!(
                "the face size of raster {:?} is {}",
                raster.name, raster.face_size
            )));
        }
        let mut map = Heightmap::new(raster.face_size as usize, raster.base);
        let ramp = Ramp {
            sea: raster.ramp_sea.clone(),
            land: raster.ramp_land.clone(),
        };
        if let Some(bands) = Bands::from_parts(&raster.limits, ramp) {
            map.bands = bands;
        }
        self.file.load_tiles(raster, |key, pixels| {
            map.load_tile(
                key.face as usize,
                key.column as usize,
                key.row as usize,
                &pixels,
            );
        })?;
        Ok(Some(map))
    }

    /// True if a save writes the heightmap. A heightmap with a face size
    /// other than the face size in the file does not go to the file, and the
    /// file keeps its tiles.
    pub fn saves_map(&self, map: &Heightmap) -> bool {
        let raster = self.saved.rasters().first();
        raster.is_none_or(|r| r.face_size as usize == map.face_size())
    }

    /// Call this in each frame. It saves a change that is older than
    /// `SAVE_DELAY`. It returns the time to the save of a newer change.
    ///
    /// The heightmap gets a raster record in the project. While `busy`, the
    /// heightmap waits for texels of a stroke, and there is no save.
    pub fn tick(
        &mut self,
        project: &mut Project,
        map: &mut Heightmap,
        busy: bool,
        now: Instant,
    ) -> Result<Option<Duration>, StoreError> {
        if !self.changed(project, map) {
            self.changed_at = None;
            return Ok(None);
        }
        let age = now.duration_since(*self.changed_at.get_or_insert(now));
        if age < SAVE_DELAY {
            return Ok(Some(SAVE_DELAY - age));
        }
        if busy {
            return Ok(Some(SAVE_DELAY));
        }
        self.flush(project, map)?;
        Ok(None)
    }

    /// Saves a change now.
    pub fn flush(&mut self, project: &mut Project, map: &mut Heightmap) -> Result<(), StoreError> {
        self.changed_at = None;
        if !self.changed(project, map) {
            return Ok(());
        }
        let saves_map = self.saves_map(map);
        let changes = match project.rasters().first() {
            Some(raster) if saves_map => {
                // The file has no tile of a raster that is new to it.
                let new = self.saved.rasters().first().map(|r| r.id) != Some(raster.id);
                vec![tile_changes(raster, map, new)]
            }
            _ => Vec::new(),
        };
        self.last_save = self.file.save_tiles(project, &changes)?;
        self.saved = project.clone();
        if saves_map {
            map.clear_changes();
        }
        Ok(())
    }

    /// Updates the raster record of the heightmap. Returns `true` if the
    /// file does not hold the project or the heightmap.
    fn changed(&self, project: &mut Project, map: &Heightmap) -> bool {
        let saves_map = self.saves_map(map);
        if saves_map {
            sync_raster(project, map);
        }
        *project != self.saved || (saves_map && map.has_changes())
    }
}

/// Makes the first raster of the project the record of the heightmap.
fn sync_raster(project: &mut Project, map: &Heightmap) {
    let id = match project.rasters().first() {
        Some(raster) => raster.id,
        None => project.add_raster(HEIGHT_NAME.to_string(), map.face_size() as u32),
    };
    let raster = project.raster_mut(id).expect("the raster of the ID");
    let record = Raster {
        id,
        name: raster.name.clone(),
        face_size: map.face_size() as u32,
        unit: "m".to_string(),
        value_min: ELEV_MIN,
        value_max: ELEV_MAX,
        base: map.base(),
        limits: map.bands.limits().to_vec(),
        ramp_sea: map.bands.ramp.sea.clone(),
        ramp_land: map.bands.ramp.land.clone(),
    };
    if *raster != record {
        *raster = record;
    }
}

/// The tiles of the heightmap that the file does not hold. With `all`, they
/// are all the tiles.
fn tile_changes<'a>(raster: &Raster, map: &'a Heightmap, all: bool) -> TileChanges<'a> {
    assert_eq!(map.tile_size(), vale_store::TILE_SIZE, "the tile size");
    let all = all || map.changed_all();
    let indexes: Vec<usize> = if all {
        map.allocated().collect()
    } else {
        map.changed().collect()
    };
    let mut changes = TileChanges {
        raster: raster.id,
        all,
        put: Vec::new(),
        delete: Vec::new(),
    };
    for index in indexes {
        let (face, column, row) = map.tile_key(index);
        let key = TileKey {
            level: 0,
            face: face as u8,
            column: column as u32,
            row: row as u32,
        };
        match map.tile(index) {
            Some(pixels) => changes.put.push((key, pixels)),
            None => changes.delete.push(key),
        }
    }
    changes
}
