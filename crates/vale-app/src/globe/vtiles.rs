//! The tiles of the vector layers that draw from tiles: the tiles that the
//! view needs, their render on worker threads, and their slots in the atlas
//! on the GPU.
//!
//! A tile is a picture of a part of a cube face at a level. Level `l` cuts
//! each face into `2^l` by `2^l` tiles of `TILE` texels, in the coordinates
//! of `place` in `field.wgsl`. The view wants the tiles under it at the level
//! with one texel or more in each pixel, and the 6 tiles of level 0. Until a
//! tile is there, the shader draws the nearest coarser tile that is. A change
//! of the layer or of its style renders each tile again, and the old tile
//! stays on screen until its new one arrives.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::time::Instant;

use vale_store::{Geometry, Layer, LayerId};
use vale_terrain::{face_dir, face_of, unwarp, warp};

use super::detail::Grid;
use super::gpu::{Link, VTilePage, VTileUpload};
use super::math::{V3, angle};
use crate::vector_tiles::{Snapshot, TilePaints, render_tile};
pub use crate::vector_tiles::{TILE, TILE_PX, TileKey};

/// The finest level: a face of 65,536 texels.
pub const MAX_LEVEL: u8 = 8;
pub const LEVELS: usize = MAX_LEVEL as usize + 1;
/// The slots of the atlas along x and along y. 360 slots of 258 squared
/// RGBA texels take 96 MB.
pub const SLOT_COLS: u32 = 20;
pub const SLOT_ROWS: u32 = 18;
pub const SLOTS: u32 = SLOT_COLS * SLOT_ROWS;
pub const ATLAS: [u32; 2] = [SLOT_COLS * TILE_PX as u32, SLOT_ROWS * TILE_PX as u32];
/// The size of the page table of a layer: an entry for each tile of each
/// level.
pub const PAGE_SIZE: [u32; 2] = [1024, 512];
/// A layer with more points draws from tiles.
pub const MIN_POINTS: usize = 1_000_000;

/// The index of the entry of `key` in the page table.
pub fn page_index(key: TileKey) -> u32 {
    let level = u32::from(key.level);
    let side = 1u32 << level;
    let base = 6 * ((1u32 << (2 * level)) - 1) / 3;
    base + (u32::from(key.face) * side + key.row) * side + key.col
}

/// The coarsest level with one texel or more in a pixel of `px` radians.
pub fn level_for(px: f64) -> u8 {
    let texels = std::f64::consts::FRAC_PI_2 / (TILE as f64 * px.max(1e-12));
    (texels.max(1.0).log2().ceil() as i64).clamp(0, i64::from(MAX_LEVEL)) as u8
}

/// The tile of `level` under direction `d`.
pub fn tile_at(d: V3, level: u8) -> TileKey {
    let (face, a, b) = face_of(d);
    let side = 1i64 << level;
    let cell = |s: f64| (((warp(s) * 0.5 + 0.5) * side as f64).floor() as i64).clamp(0, side - 1);
    TileKey {
        face: face as u8,
        level,
        col: cell(a) as u32,
        row: cell(b) as u32,
    }
}

/// The direction of the middle of a tile.
fn tile_middle(key: TileKey) -> V3 {
    let side = f64::from(1u32 << key.level);
    let at = |c: u32| unwarp((f64::from(c) + 0.5) / side * 2.0 - 1.0);
    face_dir(usize::from(key.face), at(key.col), at(key.row))
}

/// The tiles of level 0.
fn roots() -> impl Iterator<Item = TileKey> {
    (0..6u8).map(|face| TileKey {
        face,
        level: 0,
        col: 0,
        row: 0,
    })
}

enum Job {
    Snapshot {
        id: LayerId,
        geometries: Arc<Vec<Geometry>>,
    },
    Tile {
        id: LayerId,
        generation: u64,
        key: TileKey,
        snapshot: Arc<Snapshot>,
        paints: Arc<TilePaints>,
    },
}

enum Done {
    Snapshot {
        id: LayerId,
        geometries: Arc<Vec<Geometry>>,
        snapshot: Arc<Snapshot>,
    },
    Tile {
        id: LayerId,
        generation: u64,
        key: TileKey,
        rgba: Vec<u8>,
    },
}

type Queue = Arc<(Mutex<VecDeque<Job>>, Condvar)>;

/// The worker threads. They end when the pool goes away.
struct Pool {
    queue: Queue,
    done: mpsc::Receiver<Done>,
    stop: Arc<AtomicBool>,
}

impl Pool {
    fn new() -> Pool {
        let queue: Queue = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, done) = mpsc::channel();
        let threads = std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .saturating_sub(1)
            .max(1);
        for _ in 0..threads {
            let (queue, stop, tx) = (queue.clone(), stop.clone(), tx.clone());
            let run = move || {
                loop {
                    let job = {
                        let (lock, ready) = &*queue;
                        let mut jobs = lock.lock().unwrap_or_else(PoisonError::into_inner);
                        loop {
                            if stop.load(Ordering::Relaxed) {
                                return;
                            }
                            if let Some(job) = jobs.pop_front() {
                                break job;
                            }
                            jobs = ready.wait(jobs).unwrap_or_else(PoisonError::into_inner);
                        }
                    };
                    let done = match job {
                        Job::Snapshot { id, geometries } => Done::Snapshot {
                            id,
                            snapshot: Arc::new(Snapshot::new(geometries.clone())),
                            geometries,
                        },
                        Job::Tile {
                            id,
                            generation,
                            key,
                            snapshot,
                            paints,
                        } => Done::Tile {
                            id,
                            generation,
                            key,
                            rgba: render_tile(&snapshot, &paints, key),
                        },
                    };
                    if tx.send(done).is_err() {
                        return;
                    }
                }
            };
            let spawned = std::thread::Builder::new()
                .name("vale-vtiles".to_string())
                .spawn(run);
            if spawned.is_err() {
                break;
            }
        }
        Pool { queue, done, stop }
    }

    /// Runs `change` on the jobs that wait.
    fn jobs<R>(&self, change: impl FnOnce(&mut VecDeque<Job>) -> R) -> R {
        let (lock, ready) = &*self.queue;
        let mut jobs = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let out = change(&mut jobs);
        ready.notify_all();
        out
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.jobs(VecDeque::clear);
    }
}

/// A tile in a slot of the atlas.
struct Resident {
    slot: u32,
    generation: u64,
}

/// What the paints of a layer come from.
#[derive(Clone, PartialEq)]
struct StyleKey {
    style: vale_style::Style,
    pixels_per_point: f32,
    radius_m: f64,
}

/// The state of one layer that draws from tiles.
struct Tiled {
    /// The geometry of the snapshot, or of the snapshot that builds.
    geometries: Option<Arc<Vec<Geometry>>>,
    snapshot: Option<Arc<Snapshot>>,
    /// The snapshot is the same as the layer. After an undo, the next use
    /// compares them.
    checked: bool,
    /// The time of the last edit of the layer, while it waits for the quiet
    /// time.
    edited: Option<Instant>,
    style: Option<StyleKey>,
    /// The paints of each level. Empty: a paint of the style does not draw
    /// from tiles.
    paints: Vec<Arc<TilePaints>>,
    /// A new number for each new snapshot and each new style.
    generation: u64,
    tiles: HashMap<TileKey, Resident>,
}

/// The tiles of the vector layers that draw from tiles.
pub struct VectorTiles {
    layers: HashMap<LayerId, Tiled>,
    /// The layers that draw from tiles in this frame.
    shown: Vec<LayerId>,
    /// The tile in each slot, and the frame of its last use.
    slots: Vec<Option<(LayerId, TileKey, u64)>>,
    /// The tiles that wait for a worker or that a worker renders.
    pending: HashSet<(LayerId, TileKey, u64)>,
    frame: u64,
    pool: Option<Pool>,
    pub min_points: usize,
    /// The points of each layer, for its features at the count.
    counts: HashMap<LayerId, ((usize, usize), usize)>,
}

impl Default for VectorTiles {
    fn default() -> Self {
        VectorTiles {
            layers: HashMap::new(),
            shown: Vec::new(),
            slots: (0..SLOTS).map(|_| None).collect(),
            pending: HashSet::new(),
            frame: 0,
            pool: None,
            min_points: MIN_POINTS,
            counts: HashMap::new(),
        }
    }
}

impl VectorTiles {
    /// The number of points of `layer`, for the features that it has now.
    pub fn points(&mut self, layer: &Layer) -> usize {
        let key = (layer.features.as_ptr() as usize, layer.features.len());
        match self.counts.get(&layer.id) {
            Some(&(k, n)) if k == key => n,
            _ => {
                let n = (layer.features.iter())
                    .map(|f| crate::lod::points(&f.geometry))
                    .sum();
                self.counts.insert(layer.id, (key, n));
                n
            }
        }
    }

    /// Starts a frame: no layer draws from tiles until `show`.
    pub fn begin(&mut self) {
        self.shown.clear();
        self.frame += 1;
    }

    /// Draws `layer` from tiles in this frame, with its style. `paints`
    /// gives the paints of each level, or `None` if a paint does not draw
    /// from tiles, and runs only when the style changes. False: the layer
    /// does not draw from tiles.
    pub fn show(
        &mut self,
        layer: &Layer,
        style: &vale_style::Style,
        (pixels_per_point, radius_m): (f32, f64),
        paints: impl FnOnce() -> Option<Vec<TilePaints>>,
    ) -> bool {
        let id = layer.id;
        let tiled = self.layers.entry(id).or_insert_with(|| Tiled {
            geometries: None,
            snapshot: None,
            checked: true,
            edited: None,
            style: None,
            paints: Vec::new(),
            generation: 0,
            tiles: HashMap::new(),
        });
        let key = StyleKey {
            style: style.clone(),
            pixels_per_point,
            radius_m,
        };
        if tiled.style.as_ref() != Some(&key) {
            tiled.paints = (paints().into_iter().flatten()).map(Arc::new).collect();
            tiled.style = Some(key);
            tiled.generation += 1;
        }
        if tiled.paints.is_empty() {
            return false;
        }
        self.shown.push(id);
        // A new snapshot starts after an edit stops.
        if tiled
            .edited
            .is_some_and(|at| at.elapsed() < crate::lod::QUIET)
        {
            return true;
        }
        tiled.edited = None;
        let same = tiled.geometries.as_ref().is_some_and(|g| {
            if g.len() != layer.features.len() {
                return false;
            }
            if !tiled.checked {
                tiled.checked = g.iter().zip(&layer.features).all(|(g, f)| *g == f.geometry);
            }
            tiled.checked
        });
        if !same {
            let geometries: Arc<Vec<Geometry>> =
                Arc::new(layer.features.iter().map(|f| f.geometry.clone()).collect());
            tiled.geometries = Some(geometries.clone());
            tiled.checked = true;
            let pool = self.pool.get_or_insert_with(Pool::new);
            pool.jobs(|jobs| jobs.push_front(Job::Snapshot { id, geometries }));
        }
        true
    }

    /// The layers that draw from tiles in this frame.
    pub fn shown(&self) -> &[LayerId] {
        &self.shown
    }

    /// Renders the tiles of layer `id` again after the quiet time.
    pub fn forget(&mut self, id: LayerId) {
        if let Some(tiled) = self.layers.get_mut(&id) {
            tiled.checked = false;
            tiled.edited = Some(Instant::now());
        }
    }

    /// Compares each layer with its snapshot at its next use. An undo can
    /// change any layer.
    pub fn uncheck(&mut self) {
        for tiled in self.layers.values_mut() {
            tiled.checked = false;
        }
    }

    /// True while a tile or a snapshot waits.
    pub fn loading(&self) -> bool {
        !self.pending.is_empty()
            || (self.shown.iter())
                .any(|id| self.layers.get(id).is_some_and(|t| t.snapshot.is_none()))
    }

    /// The number of the tiles in the atlas.
    pub fn resident(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// The tiles of layer `id` in the atlas.
    pub fn tiles(&self, id: LayerId) -> Vec<TileKey> {
        let mut out: Vec<TileKey> = self
            .layers
            .get(&id)
            .map_or(Vec::new(), |t| t.tiles.keys().copied().collect());
        out.sort_unstable();
        out
    }

    /// Finds the tiles that the view of `grid` needs, sends the missing
    /// ones to the workers, and puts the tiles that the workers finished in
    /// the atlas. With `link`, the tiles and the page tables go to the GPU.
    /// `block`: waits for each tile that the view needs, for the tests.
    pub(super) fn plan(&mut self, grid: &Grid, link: Option<&Link>, block: bool) {
        // The layers that no longer draw from tiles free their slots.
        let shown = &self.shown;
        self.layers.retain(|id, _| shown.contains(id));
        for slot in &mut self.slots {
            if slot.is_some_and(|(id, _, _)| !shown.contains(&id)) {
                *slot = None;
            }
        }
        self.pending.retain(|(id, _, _)| shown.contains(id));
        if self.shown.is_empty() {
            self.pool = None;
            self.pending.clear();
            return;
        }
        let wanted = wanted_tiles(grid);
        let frame = self.frame;
        for tiled in self.layers.values() {
            for key in &wanted {
                let resident = tiled.tiles.get(key);
                if let Some(slot) = resident.and_then(|r| self.slots[r.slot as usize].as_mut()) {
                    slot.2 = frame;
                }
            }
        }
        // The tiles that the workers have not started go back, and the
        // queue gets the missing tiles of this frame, coarse levels first.
        let Some(pool) = &self.pool else {
            return;
        };
        let dropped: Vec<(LayerId, TileKey, u64)> = pool.jobs(|jobs| {
            let mut dropped = Vec::new();
            jobs.retain(|job| match job {
                Job::Snapshot { .. } => true,
                Job::Tile {
                    id,
                    generation,
                    key,
                    ..
                } => {
                    dropped.push((*id, *key, *generation));
                    false
                }
            });
            dropped
        });
        for d in dropped {
            self.pending.remove(&d);
        }
        let mut jobs = Vec::new();
        for (&id, tiled) in &self.layers {
            let Some(snapshot) = &tiled.snapshot else {
                continue;
            };
            for &key in &wanted {
                let fresh = tiled
                    .tiles
                    .get(&key)
                    .is_some_and(|r| r.generation == tiled.generation);
                let token = (id, key, tiled.generation);
                if fresh || self.pending.contains(&token) {
                    continue;
                }
                let Some(paints) = tiled.paints.get(usize::from(key.level)) else {
                    continue;
                };
                self.pending.insert(token);
                jobs.push(Job::Tile {
                    id,
                    generation: tiled.generation,
                    key,
                    snapshot: snapshot.clone(),
                    paints: paints.clone(),
                });
            }
        }
        pool.jobs(|queue| queue.extend(jobs));
        self.receive(link, block, &wanted);
    }

    /// Takes the snapshots and the tiles that the workers finished.
    fn receive(&mut self, link: Option<&Link>, block: bool, wanted: &[TileKey]) {
        loop {
            let Some(pool) = &self.pool else {
                return;
            };
            let waiting =
                !self.pending.is_empty() || self.layers.values().any(|t| t.snapshot.is_none());
            let done = if block && waiting {
                match pool.done.recv() {
                    Ok(done) => done,
                    Err(_) => return,
                }
            } else {
                match pool.done.try_recv() {
                    Ok(done) => done,
                    Err(_) => return,
                }
            };
            match done {
                Done::Snapshot {
                    id,
                    geometries,
                    snapshot,
                } => {
                    let Some(tiled) = self.layers.get_mut(&id) else {
                        continue;
                    };
                    let current = tiled
                        .geometries
                        .as_ref()
                        .is_some_and(|g| Arc::ptr_eq(g, &geometries));
                    if current {
                        tiled.snapshot = Some(snapshot);
                        tiled.generation += 1;
                    }
                    if block {
                        // The tiles of the new snapshot go to the workers.
                        self.requeue(wanted);
                    }
                }
                Done::Tile {
                    id,
                    generation,
                    key,
                    rgba,
                } => {
                    self.pending.remove(&(id, key, generation));
                    self.store(id, generation, key, rgba, link);
                }
            }
        }
    }

    /// Sends the missing tiles of `wanted` to the workers, for `receive`
    /// with `block`.
    fn requeue(&mut self, wanted: &[TileKey]) {
        let Some(pool) = &self.pool else {
            return;
        };
        let mut jobs = Vec::new();
        for (&id, tiled) in &self.layers {
            let Some(snapshot) = &tiled.snapshot else {
                continue;
            };
            for &key in wanted {
                let token = (id, key, tiled.generation);
                let fresh =
                    (tiled.tiles.get(&key)).is_some_and(|r| r.generation == tiled.generation);
                if fresh || self.pending.contains(&token) {
                    continue;
                }
                let Some(paints) = tiled.paints.get(usize::from(key.level)) else {
                    continue;
                };
                self.pending.insert(token);
                jobs.push(Job::Tile {
                    id,
                    generation: tiled.generation,
                    key,
                    snapshot: snapshot.clone(),
                    paints: paints.clone(),
                });
            }
        }
        pool.jobs(|queue| queue.extend(jobs));
    }

    /// Puts a finished tile in a slot: the slot of the old tile of the same
    /// place, a free slot, or the slot that the views used longest ago.
    fn store(
        &mut self,
        id: LayerId,
        generation: u64,
        key: TileKey,
        rgba: Vec<u8>,
        link: Option<&Link>,
    ) {
        let frame = self.frame;
        let Some(tiled) = self.layers.get_mut(&id) else {
            return;
        };
        if generation != tiled.generation {
            return;
        }
        let slot = match tiled.tiles.get(&key) {
            Some(r) => r.slot,
            None => {
                let free = self.slots.iter().position(Option::is_none);
                let oldest = || {
                    (self.slots.iter().enumerate())
                        .filter(|(_, s)| s.is_some_and(|(_, k, used)| used < frame && k.level > 0))
                        .min_by_key(|(_, s)| s.map(|(_, _, used)| used))
                        .map(|(i, _)| i)
                };
                let Some(slot) = free.or_else(oldest) else {
                    return;
                };
                if let Some((old_id, old_key, _)) = self.slots[slot].take() {
                    if let Some(old) = self.layers.get_mut(&old_id) {
                        old.tiles.remove(&old_key);
                    }
                    if let Some(link) = link {
                        link.push_vpage(VTilePage {
                            layer: old_id,
                            index: page_index(old_key),
                            value: 0,
                        });
                    }
                }
                let slot = slot as u32;
                if let Some(link) = link {
                    link.push_vpage(VTilePage {
                        layer: id,
                        index: page_index(key),
                        value: slot + 1,
                    });
                }
                slot
            }
        };
        let Some(tiled) = self.layers.get_mut(&id) else {
            return;
        };
        tiled.tiles.insert(key, Resident { slot, generation });
        self.slots[slot as usize] = Some((id, key, frame));
        if let Some(link) = link {
            link.push_vtile(VTileUpload { slot, rgba });
        }
    }
}

/// The tiles under the view of `grid`, each at the level of the pixels
/// over it, and the 6 tiles of level 0, coarse levels first and then by
/// the distance to the middle of the view.
fn wanted_tiles(grid: &Grid) -> Vec<TileKey> {
    let mut out: Vec<TileKey> = roots().collect();
    grid.sample(
        |d, px| out.push(tile_at(d, level_for(px))),
        |px| {
            // The angle of a tile at the level of `px`.
            std::f64::consts::FRAC_PI_2 / f64::from(1u32 << level_for(px))
        },
    );
    out.sort_unstable();
    out.dedup();
    let middle = grid.middle();
    out.sort_by(|a, b| {
        let far = |k: &TileKey| middle.map_or(0.0, |m| angle(m, tile_middle(*k)));
        (a.level, far(a))
            .partial_cmp(&(b.level, far(b)))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // The finest tiles go first when the view needs more than the atlas
    // holds.
    out.truncate(SLOTS as usize * 3 / 4);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::globe::math::lonlat_to_dir;

    /// A layer of `count` squares along the equator, each with `steps`
    /// points on each side.
    fn squares(doc: &mut crate::document::Document, count: usize, steps: usize) -> LayerId {
        let mut features = Vec::new();
        for i in 0..count {
            let (x, y) = (-40.0 + 10.0 * i as f64, -5.0);
            let mut ring = Vec::new();
            let at = |t: usize| t as f64 * 8.0 / steps as f64;
            ring.extend((0..steps).map(|t| format!("[{}, {}]", x + at(t), y)));
            ring.extend((0..steps).map(|t| format!("[{}, {}]", x + 8.0, y + at(t))));
            ring.extend((0..steps).map(|t| format!("[{}, {}]", x + 8.0 - at(t), y + 8.0)));
            ring.extend((0..=steps).map(|t| format!("[{}, {}]", x, y + 8.0 - at(t))));
            features.push(format!(
                r#"{{"type": "Feature", "properties": {{}}, "geometry": {{"type": "Polygon", "coordinates": [[{}]]}}}}"#,
                ring.join(", ")
            ));
        }
        let text = format!(
            r#"{{"type": "FeatureCollection", "features": [{}]}}"#,
            features.join(", ")
        );
        let imported = vale_import::geojson::read_str("squares", &text).unwrap();
        doc.add_imported(imported, None)[0]
    }

    /// The grid of a view of 512 pixels toward longitude 0 and latitude 0,
    /// `zoom` times closer than the whole globe.
    fn view(zoom: f64) -> Grid {
        let center = lonlat_to_dir(0.0, 0.0);
        // Two directions across the view.
        let east = lonlat_to_dir(90.0, 0.0);
        let north = lonlat_to_dir(0.0, 90.0);
        Grid::from_fn([512, 512], |x, y| {
            let (u, v) = ((x - 256.0) / 256.0 / zoom, (256.0 - y) / 256.0 / zoom);
            let w = 1.0 - u * u - v * v;
            (w > 0.0).then(|| {
                let w = w.sqrt();
                std::array::from_fn(|i| center[i] * w + east[i] * u + north[i] * v)
            })
        })
    }

    fn tiled(
        zoom: f64,
    ) -> (
        crate::document::Document,
        crate::pipeline::Pipeline,
        LayerId,
        Grid,
    ) {
        let mut doc = crate::document::Document::empty();
        let id = squares(&mut doc, 8, 200);
        let mut pipeline = crate::pipeline::Pipeline::new();
        pipeline.vtiles_mut().min_points = 100;
        pipeline.vtiles_mut().begin();
        assert!(pipeline.show_tiles(&doc, id, 1.0));
        (doc, pipeline, id, view(zoom))
    }

    #[test]
    fn a_layer_draws_from_the_tiles_under_the_view() {
        let (_, mut pipeline, id, grid) = tiled(1.0);
        let tiles = pipeline.vtiles_mut();
        tiles.plan(&grid, None, true);
        assert!(!tiles.loading());
        let got = tiles.tiles(id);
        assert!(roots().all(|r| got.contains(&r)));
        // A whole globe of 512 pixels needs level 1 at most.
        assert!(got.iter().all(|k| k.level <= 1), "{got:?}");

        // Near the ground, the view needs finer tiles, and only near it.
        let near = view(64.0);
        tiles.begin();
        tiles.shown.push(id);
        tiles.plan(&near, None, true);
        let fine: Vec<TileKey> = (tiles.tiles(id).into_iter())
            .filter(|k| k.level >= 5)
            .collect();
        assert!(!fine.is_empty());
        let center = lonlat_to_dir(0.0, 0.0);
        assert!(fine.iter().all(|k| angle(tile_middle(*k), center) < 0.1));
    }

    #[test]
    fn a_style_change_keeps_the_old_tiles_until_the_new_ones_arrive() {
        let (mut doc, mut pipeline, id, grid) = tiled(1.0);
        pipeline.vtiles_mut().plan(&grid, None, true);
        let before = pipeline.vtiles().tiles(id);
        let style = &mut doc.entry_mut(id).unwrap().style.style;
        style.rules[0]
            .symbolizers
            .retain(|s| !matches!(s, vale_style::Symbolizer::Stroke(_)));
        pipeline.vtiles_mut().begin();
        assert!(pipeline.show_tiles(&doc, id, 1.0));
        assert_eq!(pipeline.vtiles().tiles(id), before);
        pipeline.vtiles_mut().plan(&grid, None, true);
        assert!(!pipeline.vtiles().loading());
        let tiled = &pipeline.vtiles().layers[&id];
        assert!(
            tiled
                .tiles
                .values()
                .all(|r| r.generation == tiled.generation)
        );
    }

    #[test]
    fn a_small_layer_draws_no_tiles() {
        let mut doc = crate::document::Document::empty();
        let id = squares(&mut doc, 2, 10);
        let mut pipeline = crate::pipeline::Pipeline::new();
        pipeline.vtiles_mut().begin();
        assert!(!pipeline.show_tiles(&doc, id, 1.0));
        assert!(pipeline.vtiles().shown().is_empty());
    }

    #[test]
    fn the_page_index_of_each_tile_is_its_own() {
        let mut seen = HashSet::new();
        for level in 0..=4u8 {
            let side = 1u32 << level;
            for face in 0..6u8 {
                for row in 0..side {
                    for col in 0..side {
                        assert!(seen.insert(page_index(TileKey {
                            face,
                            level,
                            col,
                            row
                        })));
                    }
                }
            }
        }
        assert_eq!(seen.len(), (0..=4).map(|l| 6 << (2 * l)).sum::<usize>());
        let last = TileKey {
            face: 5,
            level: MAX_LEVEL,
            col: 255,
            row: 255,
        };
        assert!(page_index(last) < PAGE_SIZE[0] * PAGE_SIZE[1]);
    }

    #[test]
    fn a_finer_pixel_takes_a_finer_level() {
        // A texel of level 0 covers a quarter turn over 256 texels.
        let texel = std::f64::consts::FRAC_PI_2 / 256.0;
        assert_eq!(level_for(texel * 2.0), 0);
        assert_eq!(level_for(texel), 0);
        assert_eq!(level_for(texel / 2.0), 1);
        assert_eq!(level_for(texel / 3.0), 2);
        assert_eq!(level_for(1e-9), MAX_LEVEL);
    }

    #[test]
    fn the_tile_of_a_direction_holds_it() {
        let d = lonlat_to_dir(10.0, 20.0);
        for level in 0..=MAX_LEVEL {
            let key = tile_at(d, level);
            let middle = tile_middle(key);
            // A tile of level l is at most a quarter turn over 2^l wide.
            let span = std::f64::consts::FRAC_PI_2 / f64::from(1u32 << level);
            assert!(angle(d, middle) < span, "{level}");
        }
    }
}
