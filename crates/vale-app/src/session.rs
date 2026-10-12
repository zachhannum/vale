//! The open project file. It saves the project and the changed tiles of the
//! heightmap a short time after a change. Each save is one step of the undo
//! log of the file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use vale_store::{
    Project, ProjectFile, Raster, Restored, SaveStats, StoreError, TileChanges, TileKey,
};
use vale_terrain::{Bands, ELEV_MAX, ELEV_MIN, Heightmap, Ramp};

use crate::document::Document;
use crate::globe::thumbnail;
use crate::globe::view::GlobeView;

/// The time from a change to its save. One drag of a control changes the
/// project in each frame, and this delay turns the drag into a few saves.
pub const SAVE_DELAY: Duration = Duration::from_millis(500);

/// The name of the raster that the app makes for its heightmap.
const HEIGHT_NAME: &str = "Height";
/// The largest face size of a heightmap that the app loads.
const MAX_FACE_SIZE: u32 = 16384;

/// The extension of a project file.
pub const EXTENSION: &str = "gpkg";

/// The name of a world in a form that is safe as a file name. An empty name
/// gives an empty text.
pub fn file_stem(name: &str) -> String {
    let safe = |c: char| match c {
        c if c.is_alphanumeric() || " -_".contains(c) => c,
        _ => '_',
    };
    name.trim().chars().map(safe).collect()
}

/// The path of a new project file in `dir` for a world with this name. No
/// file is at the path.
pub fn new_project_path(dir: &Path, name: &str) -> PathBuf {
    let stem = file_stem(name);
    let stem = if stem.is_empty() { "World" } else { &stem };
    let mut path = dir.join(format!("{stem}.{EXTENSION}"));
    let mut number = 2;
    while path.exists() {
        path = dir.join(format!("{stem} {number}.{EXTENSION}"));
        number += 1;
    }
    path
}

/// The project files in `dir`, each with the time of its last change.
fn project_files(dir: &Path) -> Vec<(std::time::SystemTime, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let files = entries.filter_map(|entry| {
        let path = entry.ok()?.path();
        let project = path.extension().is_some_and(|e| e == EXTENSION);
        let changed = path.metadata().ok()?.modified().ok()?;
        project.then_some((changed, path))
    });
    files.collect()
}

/// The project file in `dir` with the newest change.
pub fn latest_project(dir: &Path) -> Option<PathBuf> {
    project_files(dir).into_iter().max().map(|(_, path)| path)
}

/// A project file and the name of its world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldFile {
    pub path: PathBuf,
    pub name: String,
}

/// The worlds of the project files in `dir`, in the order of their names.
/// A file has the name of the file in place of the name of its world if it
/// does not give its world, or if another file has a world of that name.
pub fn worlds(dir: &Path) -> Vec<WorldFile> {
    let stem = |path: &Path| {
        let stem = path.file_stem().unwrap_or_default();
        stem.to_string_lossy().into_owned()
    };
    let files = project_files(dir).into_iter().map(|(_, path)| {
        let name = ProjectFile::world_name(&path).unwrap_or_else(|_| stem(&path));
        WorldFile { path, name }
    });
    let mut worlds: Vec<WorldFile> = files.collect();
    let names: Vec<String> = worlds.iter().map(|world| world.name.clone()).collect();
    for world in &mut worlds {
        if names.iter().filter(|name| **name == world.name).count() > 1 {
            world.name = stem(&world.path);
        }
    }
    worlds.sort_by_cached_key(|world| (world.name.to_lowercase(), world.path.clone()));
    worlds
}

pub struct Session {
    file: ProjectFile,
    path: PathBuf,
    /// The thumbnail of the file is older than the project, or the file has
    /// no thumbnail.
    thumbnail_stale: bool,
    /// The time of the first change that the file does not hold.
    changed_at: Option<Instant>,
    /// The last `tick` had `hold`.
    held: bool,
    /// The tile rows that the last save changed.
    pub last_save: SaveStats,
    /// The count of the saves that changed the file.
    saves: u64,
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
        let thumbnail = vale_store::summary(path)?.thumbnail;
        Ok(Session {
            file,
            path: path.to_path_buf(),
            thumbnail_stale: thumbnail.is_none(),
            changed_at: None,
            held: false,
            last_save: SaveStats::default(),
            saves: 0,
        })
    }

    /// The path of the project file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes the picture of the globe for the list of projects, if the
    /// file changed after the last picture. With `!saves_map(map)`, the app
    /// does not show the heightmap of the file, and the file keeps its
    /// picture.
    pub fn save_thumbnail(&mut self, map: &Heightmap, view: &GlobeView) -> Result<(), StoreError> {
        if !self.thumbnail_stale || !self.saves_map(map) {
            return Ok(());
        }
        let picture = thumbnail::render(map, view, thumbnail::SIZE);
        self.file.set_thumbnail(&picture)?;
        self.thumbnail_stale = false;
        Ok(())
    }

    /// The heightmap of the file, with the face size, the base level, the
    /// bands, and the tiles that the file holds. The heightmap is the first
    /// raster of the project. `None`: the project has no raster.
    pub fn load_map(&self) -> Result<Option<Heightmap>, StoreError> {
        let Some(raster) = self.file.project().rasters().first() else {
            return Ok(None);
        };
        if raster.face_size > MAX_FACE_SIZE {
            return Err(StoreError::Corrupt(format!(
                "the face size of raster {:?} is {}",
                raster.name, raster.face_size
            )));
        }
        let mut map = Heightmap::new(raster.face_size as usize, raster.base);
        load_record(&mut map, raster);
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
        let raster = self.file.project().rasters().first();
        raster.is_none_or(|r| r.face_size as usize == map.face_size())
    }

    /// Call this in each frame. It saves a change that is older than
    /// `SAVE_DELAY`. It returns the time to the save of a newer change.
    ///
    /// A painted heightmap gets a raster record in the project. With `hold`,
    /// there is no save: the heightmap waits for texels of a stroke, or a
    /// control is held. The changes of one hold are one save, at the end of
    /// the hold.
    pub fn tick(
        &mut self,
        project: &mut Project,
        map: &mut Heightmap,
        hold: bool,
        now: Instant,
    ) -> Result<Option<Duration>, StoreError> {
        let released = self.held && !hold;
        self.held = hold;
        if !self.changed(project, map) {
            self.changed_at = None;
            return Ok(None);
        }
        let age = now.duration_since(*self.changed_at.get_or_insert(now));
        if hold {
            return Ok(Some(SAVE_DELAY));
        }
        if !released && age < SAVE_DELAY {
            return Ok(Some(SAVE_DELAY - age));
        }
        self.flush(project, map)?;
        Ok(None)
    }

    /// Saves a change now, as one undo step.
    pub fn flush(&mut self, project: &mut Project, map: &mut Heightmap) -> Result<(), StoreError> {
        self.changed_at = None;
        if !self.changed(project, map) {
            return Ok(());
        }
        let saves_map = self.saves_map(map);
        let changes = match project.rasters().first() {
            Some(raster) if saves_map => {
                // The file has no tile of a raster that is new to it.
                let stored = self.file.project().rasters().first();
                let new = stored.map(|r| r.id) != Some(raster.id);
                vec![tile_changes(raster, map, new)]
            }
            _ => Vec::new(),
        };
        self.last_save = self.file.save_tiles(project, &changes)?;
        self.saves += 1;
        self.thumbnail_stale = true;
        if saves_map {
            map.clear_changes();
        }
        Ok(())
    }

    /// The count of the saves that changed the file.
    pub fn saves(&self) -> u64 {
        self.saves
    }

    /// True if a change waits for its save.
    pub fn waits(&self) -> bool {
        self.changed_at.is_some()
    }

    /// True if the file has an edit to undo, or a change waits for its save.
    pub fn can_undo(&self) -> bool {
        self.file.can_undo() || self.changed_at.is_some()
    }

    /// True if the file has an edit to redo. A change that waits for its
    /// save makes the redo steps invalid.
    pub fn can_redo(&self) -> bool {
        self.file.can_redo() && self.changed_at.is_none()
    }

    /// Puts back the project and the heightmap from before the last edit.
    /// A change that waits for its save is saved first, so it is the edit.
    /// Returns `false` if the file has no edit to undo. No stroke must be
    /// open.
    pub fn undo(&mut self, doc: &mut Document, map: &mut Heightmap) -> Result<bool, StoreError> {
        self.flush(&mut doc.project, map)?;
        match self.file.undo()? {
            Some(restored) => self.apply(restored, doc, map).map(|()| true),
            None => Ok(false),
        }
    }

    /// Puts back the edit that the last `undo` took away.
    pub fn redo(&mut self, doc: &mut Document, map: &mut Heightmap) -> Result<bool, StoreError> {
        self.flush(&mut doc.project, map)?;
        match self.file.redo()? {
            Some(restored) => self.apply(restored, doc, map).map(|()| true),
            None => Ok(false),
        }
    }

    /// Puts the project of the file into the document, and the record and
    /// the restored tiles of the heightmap into `map`.
    fn apply(
        &mut self,
        restored: Restored,
        doc: &mut Document,
        map: &mut Heightmap,
    ) -> Result<(), StoreError> {
        doc.set_project(self.file.project().clone());
        if !self.saves_map(map) {
            return Ok(());
        }
        // An undo of the first stroke leaves a project with no raster. The
        // tiles of that stroke then leave the heightmap.
        let raster = self.file.project().rasters().first();
        if let Some(raster) = raster {
            load_record(map, raster);
        }
        for (id, key) in restored.tiles {
            if raster.is_some_and(|r| r.id != id) {
                continue;
            }
            let (face, column, row) = (key.face as usize, key.column as usize, key.row as usize);
            let pixels = match raster {
                Some(_) => self.file.load_tile(id, key)?,
                None => None,
            };
            match pixels {
                Some(pixels) => map.load_tile(face, column, row, &pixels),
                None => map.clear_tile(face, column, row),
            }
        }
        Ok(())
    }

    /// The size of the undo log of the file, in bytes.
    pub fn undo_bytes(&self) -> u64 {
        self.file.undo_bytes()
    }

    /// The limit of the size of the undo log of the file, in bytes.
    pub fn undo_limit(&self) -> u64 {
        self.file.undo_limit()
    }

    /// Updates the raster record of the heightmap. Returns `true` if the
    /// file does not hold the project or the heightmap.
    fn changed(&self, project: &mut Project, map: &Heightmap) -> bool {
        let saves_map = self.saves_map(map);
        if saves_map {
            sync_raster(project, map);
        }
        project != self.file.project() || (saves_map && map.has_changes())
    }
}

/// Puts the base level, the band limits, and the ramp of a raster record
/// into the heightmap. The call makes no change for a save.
fn load_record(map: &mut Heightmap, raster: &Raster) {
    map.load_base(raster.base);
    let ramp = Ramp {
        sea: raster.ramp_sea.clone(),
        land: raster.ramp_land.clone(),
    };
    if let Some(bands) = Bands::from_parts(&raster.limits, ramp) {
        map.bands = bands;
    }
}

/// Makes the first raster of the project the record of the heightmap. A
/// project with no raster gets one when the heightmap has a painted tile.
fn sync_raster(project: &mut Project, map: &Heightmap) {
    let id = match project.rasters().first() {
        Some(raster) => raster.id,
        None if map.allocated().next().is_none() => return,
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
