//! The open project file. It saves the project and the changed tiles of each
//! raster layer a short time after a change. Each save is one step of the undo
//! log of the file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use vale_store::{
    Project, ProjectFile, Raster, RasterId, Restored, SaveStats, StoreError, TileChanges, TileKey,
};
use vale_terrain::{Bands, FACES, Heightmap, Ramp, reduced_face_size};

use crate::document::Document;
use crate::files::Files;
use crate::globe::view::GlobeView;
use crate::globe::{DISPLAY_CAP, reduce_tile, thumbnail};
use crate::workcopy::WorkCopy;

/// The time from a change to its save. One drag of a control changes the
/// project in each frame, and this delay turns the drag into a few saves.
pub const SAVE_DELAY: Duration = Duration::from_millis(500);

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
    worlds_of(project_files(dir).into_iter().map(|(_, path)| path))
}

/// The worlds of these project files, with the names and the order of
/// `worlds`.
pub fn worlds_of(paths: impl IntoIterator<Item = PathBuf>) -> Vec<WorldFile> {
    let stem = |path: &Path| {
        let stem = path.file_stem().unwrap_or_default();
        stem.to_string_lossy().into_owned()
    };
    let files = paths.into_iter().map(|path| {
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
    /// The working copy that holds the database, for a project in a folder
    /// that a sync service owns.
    work: Option<WorkCopy>,
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
    /// The tiles that the last undo or redo put back in the file for the
    /// raster layers with no heightmap in its `maps`.
    pub last_restored: Vec<(RasterId, TileKey)>,
}

impl Session {
    /// Opens the project file and puts its project into the document. If no
    /// file is at the path, this makes the file from the document.
    pub fn open(path: &Path, doc: &mut Document) -> Result<Session, StoreError> {
        Session::open_at(path, path, None, doc)
    }

    /// Opens a project file in a folder that a sync service owns. The
    /// database is a working copy in `work_dir`. If no file is at the path,
    /// this makes the file from the document.
    pub fn open_copy(
        path: &Path,
        work_dir: &Path,
        files: &dyn Files,
        doc: &mut Document,
    ) -> Result<Session, StoreError> {
        let work = WorkCopy::open(path, work_dir, files)?;
        let new = !work.path().exists();
        let database = work.path().to_path_buf();
        let mut session = Session::open_at(&database, path, Some(work), doc)?;
        if new {
            session.mark_changed();
            session.write_back(files)?;
        }
        Ok(session)
    }

    fn open_at(
        database: &Path,
        path: &Path,
        work: Option<WorkCopy>,
        doc: &mut Document,
    ) -> Result<Session, StoreError> {
        let file = if database.exists() {
            let (file, project) = ProjectFile::open(database)?;
            doc.set_project(project);
            file
        } else {
            if let Some(parent) = database.parent() {
                std::fs::create_dir_all(parent)?;
            }
            ProjectFile::create(database, &doc.project)?
        };
        let thumbnail = vale_store::summary(database)?.thumbnail;
        Ok(Session {
            file,
            work,
            path: path.to_path_buf(),
            thumbnail_stale: thumbnail.is_none(),
            changed_at: None,
            held: false,
            last_save: SaveStats::default(),
            saves: 0,
            last_restored: Vec::new(),
        })
    }

    /// The path of the project file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Closes the project file. The project file does not get the changes
    /// that the working copy holds.
    pub fn discard(mut self) {
        if let Some(work) = &mut self.work {
            work.forget();
        }
    }

    fn mark_changed(&mut self) {
        if let Some(work) = &mut self.work {
            work.mark_changed(Instant::now());
        }
    }

    /// True if the working copy holds a change for longer than
    /// `WRITE_BACK_DELAY`.
    pub fn write_back_due(&self, now: Instant) -> bool {
        self.work.as_ref().is_some_and(|work| work.due(now))
    }

    /// Writes the working copy to the project file, if it has a change. If
    /// another device changed the project file, the copy goes to a new file
    /// beside it. That file is the project file from then on, and this
    /// returns `true`.
    pub fn write_back(&mut self, files: &dyn Files) -> Result<bool, StoreError> {
        let Some(work) = &mut self.work else {
            return Ok(false);
        };
        work.write_back(files)?;
        let moved = work.source() != self.path;
        self.path = work.source().to_path_buf();
        Ok(moved)
    }

    /// Writes the picture of the globe for the list of projects, if the
    /// file changed after the last picture. `layers` has the heightmap and
    /// the opacity of each visible raster layer, from the bottom.
    pub fn save_thumbnail(
        &mut self,
        layers: &[(&Heightmap, f32)],
        view: &GlobeView,
    ) -> Result<(), StoreError> {
        if !self.thumbnail_stale {
            return Ok(());
        }
        let picture = thumbnail::render(layers, view, thumbnail::SIZE);
        self.file.set_thumbnail(&picture)?;
        self.thumbnail_stale = false;
        self.mark_changed();
        Ok(())
    }

    /// The heightmap of each raster layer of the file, in the order of the
    /// project. A heightmap has the face size, the base level, the bands,
    /// and the tiles that the file holds.
    pub fn load_maps(&self) -> Result<Vec<(RasterId, Heightmap)>, StoreError> {
        let rasters = self.file.project().rasters().iter();
        rasters
            .map(|raster| Ok((raster.id, self.load_map(raster)?)))
            .collect()
    }

    /// The heightmap of a raster layer with no tile: the face size, the
    /// base level, and the bands of the record.
    pub fn empty_map(raster: &Raster) -> Result<Heightmap, StoreError> {
        if raster.face_size > MAX_FACE_SIZE {
            return Err(StoreError::Corrupt(format!(
                "the face size of raster {} is {}",
                raster.id.0, raster.face_size
            )));
        }
        let mut map = Heightmap::new(raster.face_size as usize, raster.base);
        map.bands = bands_of(raster);
        Ok(map)
    }

    /// One tile of a raster layer of the file. `None`: the file has no such
    /// tile.
    pub fn load_tile(
        &self,
        raster: RasterId,
        face: usize,
        column: usize,
        row: usize,
    ) -> Result<Option<Vec<u16>>, StoreError> {
        let key = TileKey {
            level: 0,
            face: face as u8,
            column: column as u32,
            row: row as u32,
        };
        self.file.load_tile(raster, key)
    }

    /// The heightmap of the picture of each raster layer of the file: the
    /// whole heightmap of a layer with a face of `DISPLAY_CAP` or less, or
    /// else the reduced heightmap.
    pub fn load_display_maps(&self) -> Result<Vec<(RasterId, Heightmap)>, StoreError> {
        let rasters = self.file.project().rasters().iter();
        rasters
            .map(|raster| Ok((raster.id, self.load_display_map(raster)?)))
            .collect()
    }

    /// The heightmap of the picture of one raster layer of the file. A
    /// reduced heightmap comes from the tiles one by one, so the main memory
    /// does not hold the whole heightmap.
    pub fn load_display_map(&self, raster: &Raster) -> Result<Heightmap, StoreError> {
        let face_size = raster.face_size as usize;
        if raster.face_size <= DISPLAY_CAP {
            return self.load_map(raster);
        }
        let whole = Session::empty_map(raster)?;
        let reduced = reduced_face_size(raster.face_size, DISPLAY_CAP) as usize;
        let mut map = Heightmap::new(reduced, whole.base());
        map.bands = whole.bands;
        let factor = raster.face_size.div_ceil(reduced as u32) as usize;
        let tile = vale_store::TILE_SIZE;
        self.file.load_tiles(raster, |key, pixels| {
            let (face, column, row) = (key.face as usize, key.column as usize, key.row as usize);
            let base = map.base();
            let block = reduce_tile(Some(&pixels), base, tile, face_size, factor, column, row);
            if let Some((rect, data)) = block.filter(|_| face < FACES) {
                map.store_rect(face, rect, &data);
            }
        })?;
        map.clear_changes();
        Ok(map)
    }

    /// The heightmap of one raster layer of the file.
    pub fn load_map(&self, raster: &Raster) -> Result<Heightmap, StoreError> {
        let mut map = Session::empty_map(raster)?;
        self.file.load_tiles(raster, |key, pixels| {
            map.load_tile(
                key.face as usize,
                key.column as usize,
                key.row as usize,
                &pixels,
            );
        })?;
        Ok(map)
    }

    /// Call this in each frame. It saves a change that is older than
    /// `SAVE_DELAY`. It returns the time to the save of a newer change.
    ///
    /// `maps` has the heightmap of each raster layer. A raster layer with no
    /// heightmap there keeps the tiles of the file. With `hold`, there is no
    /// save: a heightmap waits for texels of a stroke, or a control is held.
    /// The changes of one hold are one save, at the end of the hold.
    pub fn tick(
        &mut self,
        project: &mut Project,
        maps: &mut Maps,
        hold: bool,
        now: Instant,
    ) -> Result<Option<Duration>, StoreError> {
        let released = self.held && !hold;
        self.held = hold;
        if !self.changed(project, maps) {
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
        self.flush(project, maps)?;
        Ok(None)
    }

    /// Saves a change now, as one undo step.
    pub fn flush(&mut self, project: &mut Project, maps: &mut Maps) -> Result<(), StoreError> {
        self.changed_at = None;
        if !self.changed(project, maps) {
            return Ok(());
        }
        let mut changes = Vec::new();
        for (id, map) in maps.iter() {
            let Some(raster) = record(project, *id, map) else {
                continue;
            };
            // The file has no tile of a raster that is new to it.
            let stored = self.file.project().raster(*id);
            let new = stored.is_none_or(|stored| stored.face_size != raster.face_size);
            if new || map.has_changes() {
                changes.push(tile_changes(raster, map, new));
            }
        }
        self.last_save = self.file.save_tiles(project, &changes)?;
        drop(changes);
        self.saves += 1;
        self.thumbnail_stale = true;
        self.mark_changed();
        for (id, map) in maps.iter_mut() {
            if record(project, *id, map).is_some() {
                map.clear_changes();
            }
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

    /// Puts back the project and the heightmaps from before the last edit.
    /// A change that waits for its save is saved first, so it is the edit.
    /// Returns `false` if the file has no edit to undo. No stroke must be
    /// open.
    ///
    /// A raster layer that the step brings back has no heightmap in `maps`.
    /// `load_map` gives it.
    pub fn undo(&mut self, doc: &mut Document, maps: &mut Maps) -> Result<bool, StoreError> {
        self.flush(&mut doc.project, maps)?;
        self.mark_changed();
        self.last_restored.clear();
        match self.file.undo()? {
            Some(restored) => self.apply(restored, doc, maps).map(|()| true),
            None => Ok(false),
        }
    }

    /// Puts back the edit that the last `undo` took away.
    pub fn redo(&mut self, doc: &mut Document, maps: &mut Maps) -> Result<bool, StoreError> {
        self.flush(&mut doc.project, maps)?;
        self.mark_changed();
        self.last_restored.clear();
        match self.file.redo()? {
            Some(restored) => self.apply(restored, doc, maps).map(|()| true),
            None => Ok(false),
        }
    }

    /// Puts the project of the file into the document, and the record and
    /// the restored tiles of each raster into its heightmap.
    fn apply(
        &mut self,
        restored: Restored,
        doc: &mut Document,
        maps: &mut Maps,
    ) -> Result<(), StoreError> {
        doc.set_project(self.file.project().clone());
        let project = self.file.project();
        for (id, map) in maps.iter_mut() {
            if let Some(raster) = record(project, *id, map) {
                load_record(map, raster);
            }
        }
        self.last_restored.clear();
        for (id, key) in restored.tiles {
            let Some((_, map)) = maps.iter_mut().find(|(of, _)| *of == id) else {
                self.last_restored.push((id, key));
                continue;
            };
            let (face, column, row) = (key.face as usize, key.column as usize, key.row as usize);
            let side = map.face_size().div_ceil(map.tile_size());
            if face >= FACES || column >= side || row >= side {
                continue;
            }
            // A raster that the step took away leaves no tile in its
            // heightmap.
            let pixels = match project.raster(id) {
                Some(raster) if raster.face_size as usize != map.face_size() => continue,
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

    /// Updates the record of each raster from its heightmap. Returns `true`
    /// if the file does not hold the project or a heightmap.
    fn changed(&self, project: &mut Project, maps: &Maps) -> bool {
        let mut painted = false;
        for (id, map) in maps {
            let raster = project.raster_mut(*id);
            let Some(raster) = raster.filter(|r| r.face_size as usize == map.face_size()) else {
                continue;
            };
            sync_record(raster, map);
            painted |= map.has_changes();
        }
        painted || project != self.file.project()
    }
}

/// The heightmap of each raster layer that a save writes.
pub type Maps<'a> = [(RasterId, &'a mut Heightmap)];

/// The record of a heightmap in the project. `None`: the project has no
/// raster with this ID, or the raster has another face size. A save then
/// does not write the heightmap.
fn record<'a>(project: &'a Project, id: RasterId, map: &Heightmap) -> Option<&'a Raster> {
    let raster = project.raster(id);
    raster.filter(|raster| raster.face_size as usize == map.face_size())
}

fn ramp_of(raster: &Raster) -> Ramp {
    Ramp {
        sea: raster.ramp_sea.clone(),
        land: raster.ramp_land.clone(),
    }
}

/// The bands of a raster record. A record with no limits or with no ramp
/// gets the bands that a new raster of its range has. A record with an empty
/// range gets the range of a height in meters.
pub fn bands_of(raster: &Raster) -> Bands {
    let (min, max) = (raster.value_min, raster.value_max);
    let sea = crate::ui::is_height(raster);
    Bands::from_range_parts(min, max, sea, &raster.limits, ramp_of(raster))
        .or_else(|| Bands::with_range(min, max, sea))
        .or_else(|| Bands::from_parts(&raster.limits, ramp_of(raster)))
        .unwrap_or_default()
}

/// Puts the base level, the band limits, and the ramp of a raster record
/// into the heightmap. The call makes no change for a save.
fn load_record(map: &mut Heightmap, raster: &Raster) {
    map.load_base(raster.base);
    let (min, max) = (raster.value_min, raster.value_max);
    let sea = crate::ui::is_height(raster);
    let bands = Bands::from_range_parts(min, max, sea, &raster.limits, ramp_of(raster));
    if let Some(bands) = bands {
        map.bands = bands;
    }
}

/// Puts the base level, the value range, the band limits, and the ramp of
/// a heightmap into its raster record. The unit, the opacity, and the face
/// size of the record stay.
pub fn sync_record(raster: &mut Raster, map: &Heightmap) {
    raster.value_min = map.bands.min();
    raster.value_max = map.bands.max();
    raster.base = map.base();
    if raster.limits != map.bands.limits() {
        raster.limits = map.bands.limits().to_vec();
    }
    if raster.ramp_sea != map.bands.ramp.sea {
        raster.ramp_sea = map.bands.ramp.sea.clone();
    }
    if raster.ramp_land != map.bands.ramp.land {
        raster.ramp_land = map.bands.ramp.land.clone();
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
