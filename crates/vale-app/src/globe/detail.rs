//! The detail of the raster layers in the map frames.
//!
//! A frame draws a raster layer from its copy on the GPU. Where a frame
//! needs more texels than the copy has, the layer gets a detail map: a
//! finer reduced copy of the whole heightmap, with tiles only where the
//! frames show it. The detail map holds level 0 and level 1, so a frame
//! that reads it smooths from these two levels alone.
//!
//! A frame uses its detail map when each tile under the frame is there.
//! Until then it uses the copy.

use std::collections::HashMap;
use std::f64::consts::FRAC_PI_2;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use vale_sphere::{Point, Projection, ProjectionSpec};
use vale_store::RasterId;
use vale_terrain::{FACES, Heightmap, TILE_SIZE, TexelRect, face_of, level_size, warp};

use super::gpu::{LayerKey, Link, MapKey, MapKind, Op};
use super::math::{V3, add, angle, cross, lonlat_to_dir, normalize, scale};
use super::{FrameDraw, FramePlace};

/// The memory of the GPU for the detail maps, in bytes.
pub const DETAIL_BUDGET: u64 = 256 << 20;

/// The most tiles that wait for the project file at one time.
pub const DETAIL_IN_FLIGHT: usize = 32;

/// A detail map that no frame used for this number of frames goes away.
const IDLE_FRAMES: u64 = 60;

/// Pixels between two points of the grid of a frame.
const GRID_PIXELS: u32 = 32;

/// The most points along one side of the grid of a frame.
const GRID_MAX: usize = 48;

/// The most points along one side of a cell of the grid that the window
/// of a frame reads.
const CELL_MAX: usize = 8;

/// A tile of a level: its face, its column, and its row.
pub type TileKey = (usize, usize, usize);

/// The number of texels along one side of a tile of a detail map with a
/// face of `face_size` texels.
pub fn tile_size(face_size: u32) -> usize {
    TILE_SIZE.max(face_size as usize / 32)
}

/// A new number for the texels of a raster layer. Each change of the
/// texels gets a new number, so a detail map of other texels is stale.
pub fn next_edits() -> u64 {
    static EDITS: AtomicU64 = AtomicU64::new(1);
    EDITS.fetch_add(1, Ordering::Relaxed)
}

/// One tile of a detail map that the globe asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetailRequest {
    pub raster: RasterId,
    /// The generation of the detail map on the GPU.
    pub generation: u64,
    /// The level of the whole heightmap. A texel of the detail map is the
    /// mean of `1 << level` by `1 << level` texels of the whole heightmap.
    pub level: u32,
    /// Texels along one side of a face of the detail map.
    pub face_size: u32,
    /// Texels along one side of a face of the whole heightmap.
    pub full_face_size: u32,
    pub face: usize,
    /// The column and the row of the tile in the detail map.
    pub column: usize,
    pub row: usize,
    /// The texels of the tile, in the texels of the detail map.
    pub rect: TexelRect,
    /// The texels of the whole heightmap under the tile.
    pub source: TexelRect,
}

impl DetailRequest {
    /// The columns and the rows of the tiles of the whole heightmap under
    /// the tile, for tiles of `tile` texels.
    pub fn source_tiles(&self, tile: usize) -> (Range<usize>, Range<usize>) {
        let s = self.source;
        (
            s.x0 / tile..s.x1.div_ceil(tile),
            s.y0 / tile..s.y1.div_ceil(tile),
        )
    }

    fn key(&self) -> TileKey {
        (self.face, self.column, self.row)
    }
}

/// The texels of one tile of a detail map: level 0, and the block of level
/// 1 under it. `map` is the whole heightmap. It needs only the tiles under
/// `req.source`.
pub fn detail_tile(map: &Heightmap, req: &DetailRequest) -> (Vec<u16>, Vec<u16>) {
    let mut level0 = Vec::new();
    map.read_reduced(req.face, req.rect, 1 << req.level, &mut level0);
    let (_, level1) = next_block(&level0, req.rect, req.face_size);
    (level0, level1)
}

/// The rectangle of level 1 under a rectangle of level 0, on a face of
/// `face_size` texels at level 0. `rect` starts at even texels.
fn next_rect(rect: TexelRect, face_size: u32) -> TexelRect {
    let half = level_size(face_size, 1) as usize;
    TexelRect {
        x0: rect.x0 / 2,
        y0: rect.y0 / 2,
        x1: rect.x1.div_ceil(2).min(half),
        y1: rect.y1.div_ceil(2).min(half),
    }
}

/// The texels of level 1 under the texels of a rectangle of level 0. Each
/// is the rounded mean of 2 by 2 texels, as in `smooth::next_level`.
fn next_block(data: &[u16], rect: TexelRect, face_size: u32) -> (TexelRect, Vec<u16>) {
    let next = next_rect(rect, face_size);
    let width = rect.x1 - rect.x0;
    let mut out = Vec::with_capacity((next.x1 - next.x0) * (next.y1 - next.y0));
    for y in next.y0..next.y1 {
        for x in next.x0..next.x1 {
            let mut sum = 0u32;
            for (bx, by) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sy) = (2 * x + bx - rect.x0, 2 * y + by - rect.y0);
                sum += u32::from(data[sy * width + sx]);
            }
            out.push(((sum + 2) / 4) as u16);
        }
    }
    (next, out)
}

/// The memory of a pool of tiles on the GPU with `tiles` tiles at its
/// peak, in bytes. The pool grows by chunks that double it.
fn pool_bytes(tiles: usize, tile: usize) -> u64 {
    const LAYER_TILES: usize = 8;
    let layers = tiles.div_ceil(LAYER_TILES * LAYER_TILES);
    let layers = if layers == 0 {
        0
    } else {
        layers.next_power_of_two()
    };
    let side = (LAYER_TILES * tile) as u64;
    layers as u64 * side * side * 2
}

/// The memory of the table of a detail map, in bytes.
fn table_bytes(face_size: u32, tile: usize) -> u64 {
    let per_side = (face_size as usize).div_ceil(tile) as u64;
    per_side * per_side * FACES as u64 * 2 * 4
}

/// The memory of a detail map with `tiles` tiles of level 0, in bytes.
fn map_bytes(face_size: u32, tiles: &[TileKey]) -> u64 {
    let tile = tile_size(face_size);
    let mut parents: Vec<TileKey> = tiles.iter().map(|&(f, c, r)| (f, c / 2, r / 2)).collect();
    parents.sort_unstable();
    parents.dedup();
    table_bytes(face_size, tile) + pool_bytes(tiles.len() + parents.len(), tile)
}

/// The places that a frame shows: a grid of pixels of its target, with the
/// direction under each.
#[derive(Debug)]
pub(super) struct Grid {
    cols: usize,
    rows: usize,
    /// Pixels between two points along x and along y.
    step: [f64; 2],
    dirs: Vec<Option<V3>>,
}

impl Grid {
    /// The grid of a frame, with `projection` of radius 1.
    pub(super) fn new(place: &FramePlace, size: [u32; 2], projection: &Projection) -> Grid {
        let count = |pixels: u32| ((pixels / GRID_PIXELS) as usize + 2).min(GRID_MAX);
        let (cols, rows) = (count(size[0]), count(size[1]));
        let step = [
            f64::from(size[0]) / (cols - 1) as f64,
            f64::from(size[1]) / (rows - 1) as f64,
        ];
        let (sin, cos) = place.rotation.sin_cos();
        let units = f64::from(place.units_to_pixels);
        let mut dirs = Vec::with_capacity(cols * rows);
        for row in 0..rows {
            for col in 0..cols {
                let tx = (col as f64 * step[0] - f64::from(place.center[0])) / units;
                let ty = (row as f64 * step[1] - f64::from(place.center[1])) / units;
                let point = Point::new(
                    tx * cos + ty * sin + f64::from(place.map_center[0]),
                    tx * sin - ty * cos + f64::from(place.map_center[1]),
                );
                let dir = projection.inverse(point);
                dirs.push(dir.map(|[lon, lat]| lonlat_to_dir(lon, lat)));
            }
        }
        Grid {
            cols,
            rows,
            step,
            dirs,
        }
    }

    fn at(&self, col: usize, row: usize) -> Option<V3> {
        self.dirs[row * self.cols + col]
    }

    /// The fewest radians of the world in one pixel. A pixel spans more
    /// radians along one axis than along the other on most projections,
    /// and the larger of the two counts. `None`: the frame shows no world.
    fn radians_per_pixel(&self) -> Option<f64> {
        let mut least: Option<f64> = None;
        for row in 0..self.rows - 1 {
            for col in 0..self.cols - 1 {
                let (Some(a), Some(b), Some(c)) = (
                    self.at(col, row),
                    self.at(col + 1, row),
                    self.at(col, row + 1),
                ) else {
                    continue;
                };
                let along = (angle(a, b) / self.step[0]).max(angle(a, c) / self.step[1]);
                least = Some(least.map_or(along, |l| l.min(along)));
            }
        }
        least.filter(|l| *l > 0.0)
    }

    /// The tiles of a detail map with a face of `face_size` texels under the
    /// grid, with one tile more on each side, sorted.
    fn tiles(&self, face_size: u32) -> Vec<TileKey> {
        let tile = tile_size(face_size);
        // The angle of one tile.
        let span = FRAC_PI_2 * tile as f64 / f64::from(face_size);
        let mut out = Vec::new();
        let mut add_point = |d: V3| {
            let helper = if d[2].abs() < 0.9 {
                [0.0, 0.0, 1.0]
            } else {
                [1.0, 0.0, 0.0]
            };
            let e1 = normalize(cross(d, helper));
            let e2 = cross(d, e1);
            for i in -1..=1 {
                for j in -1..=1 {
                    let offset = add(
                        scale(e1, f64::from(i) * span),
                        scale(e2, f64::from(j) * span),
                    );
                    out.push(tile_of(normalize(add(d, offset)), face_size, tile));
                }
            }
        };
        for row in 0..self.rows - 1 {
            for col in 0..self.cols - 1 {
                let corners = [
                    self.at(col, row),
                    self.at(col + 1, row),
                    self.at(col, row + 1),
                    self.at(col + 1, row + 1),
                ];
                let [Some(a), Some(b), Some(c), Some(d)] = corners else {
                    corners.into_iter().flatten().for_each(&mut add_point);
                    continue;
                };
                let widest = angle(a, b)
                    .max(angle(a, c))
                    .max(angle(b, d))
                    .max(angle(c, d));
                let n = ((2.0 * widest / span).ceil() as usize).clamp(1, CELL_MAX);
                for i in 0..=n {
                    for j in 0..=n {
                        let (u, v) = (i as f64 / n as f64, j as f64 / n as f64);
                        let top = add(scale(a, 1.0 - u), scale(b, u));
                        let bottom = add(scale(c, 1.0 - u), scale(d, u));
                        add_point(normalize(add(scale(top, 1.0 - v), scale(bottom, v))));
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// The tile of a detail map under a direction.
fn tile_of(d: V3, face_size: u32, tile: usize) -> TileKey {
    let (face, a, b) = face_of(d);
    let n = face_size as usize;
    let texel = |s: f64| (((warp(s) + 1.0) / 2.0 * n as f64) as usize).min(n - 1);
    (face, texel(a) / tile, texel(b) / tile)
}

/// One raster layer in one frame, for `Details::want`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Shown {
    /// The index of the frame.
    pub frame: usize,
    pub raster: RasterId,
    /// Texels along one side of a face of the whole heightmap.
    pub full_face: u32,
    /// Texels along one side of a face of the copy that the frame reads
    /// without a detail map.
    pub copy_face: u32,
    /// The level of the texels that no tile holds.
    pub base: u16,
    /// The number of the texels of the layer, from `next_edits`.
    pub edits: u64,
    /// The level of the smooth field of the frame, for the whole heightmap.
    pub lod: f64,
}

/// The level of the whole heightmap that a frame reads a layer at, from 0
/// to `coarsest`. `lod` is the level of the smooth field, and
/// `radians_per_pixel` is the fewest radians of the world in one pixel.
/// The level has one texel or more in each pixel.
pub(super) fn frame_level(full_face: u32, coarsest: u32, lod: f64, radians_per_pixel: f64) -> u32 {
    let texels = radians_per_pixel * f64::from(full_face) / FRAC_PI_2;
    let level = lod.max(texels.log2()).floor();
    if level.is_nan() || level < 0.0 {
        return 0;
    }
    (level as u32).min(coarsest)
}

/// The tiles of the detail map that one frame needs for one raster layer.
#[derive(Clone, Debug)]
struct Need {
    frame: u32,
    raster: RasterId,
    full_face: u32,
    /// The level of the copy. A need at this level is no need.
    coarsest: u32,
    level: u32,
    base: u16,
    edits: u64,
    tiles: Vec<TileKey>,
    /// A detail map of another level that has each tile of the window, for
    /// the time until the map of `level` has them.
    fallback: Option<MapKey>,
}

impl Need {
    fn face_size(&self) -> u32 {
        self.full_face.div_ceil(1 << self.level)
    }
}

/// One detail map on the GPU.
#[derive(Debug)]
struct DetailMap {
    key: MapKey,
    raster: RasterId,
    edits: u64,
    full_face: u32,
    level: u32,
    /// Each tile of level 0 that the GPU has, with the tick of its last
    /// use.
    tiles: HashMap<TileKey, u64>,
    /// Each tile of level 1 that the GPU has, with the number of its tiles
    /// of level 0 that the GPU has.
    parents: HashMap<TileKey, usize>,
    /// The most tiles of both levels that the GPU had at one time.
    peak: usize,
    /// The tick of the last use.
    used: u64,
}

impl DetailMap {
    fn bytes(&self) -> u64 {
        let tile = tile_size(self.key.face_size);
        table_bytes(self.key.face_size, tile) + pool_bytes(self.peak, tile)
    }

    fn held(&self) -> usize {
        self.tiles.len() + self.parents.len()
    }

    fn has_all(&self, tiles: &[TileKey]) -> bool {
        tiles.iter().all(|t| self.tiles.contains_key(t))
    }

    fn request(&self, (face, column, row): TileKey) -> DetailRequest {
        let size = self.key.face_size as usize;
        let tile = tile_size(self.key.face_size);
        let rect = TexelRect {
            x0: column * tile,
            y0: row * tile,
            x1: ((column + 1) * tile).min(size),
            y1: ((row + 1) * tile).min(size),
        };
        let factor = 1 << self.level;
        let full = self.full_face as usize;
        let source = TexelRect {
            x0: rect.x0 * factor,
            y0: rect.y0 * factor,
            x1: (rect.x1 * factor).min(full),
            y1: (rect.y1 * factor).min(full),
        };
        DetailRequest {
            raster: self.raster,
            generation: self.key.generation,
            level: self.level,
            face_size: self.key.face_size,
            full_face_size: self.full_face,
            face,
            column,
            row,
            rect,
            source,
        }
    }

    /// Takes one tile of level 0 from the GPU, and its tile of level 1 when
    /// no other tile under it is there.
    fn release(&mut self, key: TileKey, link: Option<&Link>) {
        if self.tiles.remove(&key).is_none() {
            return;
        }
        let (face, column, row) = key;
        if let Some(link) = link {
            let face = face as u32;
            link.push(
                self.key,
                Op::ReleaseTile {
                    level: 0,
                    face,
                    column,
                    row,
                },
            );
        }
        let parent = (face, column / 2, row / 2);
        let count = self.parents.entry(parent).or_insert(1);
        *count -= 1;
        if *count == 0 {
            self.parents.remove(&parent);
            if let Some(link) = link {
                let (column, row) = (parent.1, parent.2);
                let face = face as u32;
                link.push(
                    self.key,
                    Op::ReleaseTile {
                        level: 1,
                        face,
                        column,
                        row,
                    },
                );
            }
        }
    }
}

/// The key of the grid of a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct GridKey {
    place: FramePlace,
    size: [u32; 2],
    spec: ProjectionSpec,
}

/// The detail maps of the raster layers, and what the frames need.
#[derive(Default)]
pub(super) struct Details {
    maps: Vec<DetailMap>,
    needs: Vec<Need>,
    /// The frames of the last `want`.
    frames: Vec<u32>,
    /// The frames of the last `want` that read a raster layer at a coarser
    /// level than their pixels need, because of the budget.
    coarser: Vec<u32>,
    /// The grid of each frame of the last `want`.
    grids: Vec<(u32, GridKey, Arc<Grid>)>,
    projections: Vec<(ProjectionSpec, Rc<Projection>)>,
    /// The tiles that wait for the project file.
    in_flight: Vec<DetailRequest>,
    /// The count of the calls of `want`.
    tick: u64,
    /// The count of the frames since the last `want`.
    idle: u64,
    /// The memory of the GPU for the detail maps, in bytes. `None`:
    /// `DETAIL_BUDGET`.
    pub budget: Option<u64>,
}

impl Details {
    fn budget(&self) -> u64 {
        self.budget.unwrap_or(DETAIL_BUDGET)
    }

    fn projection(&mut self, spec: ProjectionSpec) -> Option<Rc<Projection>> {
        if let Some((_, p)) = self.projections.iter().find(|(s, _)| *s == spec) {
            return Some(p.clone());
        }
        let projection = Rc::new(Projection::new(spec, 1.0).ok()?);
        self.projections.push((spec, projection.clone()));
        Some(projection)
    }

    /// The grid of a frame, from the last `want` if the frame did not move.
    fn grid(&mut self, frame: &FrameDraw) -> Option<Arc<Grid>> {
        let key = GridKey {
            place: frame.place,
            size: [frame.rect[2], frame.rect[3]],
            spec: frame.mesh.spec,
        };
        let old = self
            .grids
            .iter()
            .find(|(id, k, _)| *id == frame.id && *k == key);
        if let Some((_, _, grid)) = old {
            return Some(grid.clone());
        }
        let projection = self.projection(key.spec)?;
        Some(Arc::new(Grid::new(&frame.place, key.size, &projection)))
    }

    /// Finds the tiles that each frame needs for each raster layer in
    /// `shown`, and makes the detail maps for them. A tile that no frame
    /// needs any more stays until the maps go over the budget. A tile that
    /// waits for the project file and that no frame needs is cancelled.
    /// `link`: the GPU gets the changes. `generations` counts the maps on
    /// the GPU.
    pub(super) fn want(
        &mut self,
        frames: &[FrameDraw],
        shown: &[Shown],
        link: Option<&Link>,
        generations: &mut u64,
    ) {
        self.tick += 1;
        self.idle = 0;
        let mut grids = Vec::new();
        for frame in frames {
            if let Some(grid) = self.grid(frame) {
                let key = GridKey {
                    place: frame.place,
                    size: [frame.rect[2], frame.rect[3]],
                    spec: frame.mesh.spec,
                };
                grids.push((frame.id, key, grid));
            }
        }
        self.grids = grids;
        self.frames = frames.iter().map(|frame| frame.id).collect();

        // The maps of texels that changed go away.
        let stale = |map: &DetailMap| {
            shown
                .iter()
                .any(|s| s.raster == map.raster && s.edits != map.edits)
        };
        self.maps.retain(|map| !stale(map));

        let mut needs = Vec::new();
        for s in shown {
            let frame = &frames[s.frame];
            let Some(grid) = self.grid_of(frame.id) else {
                continue;
            };
            let Some(radians) = grid.radians_per_pixel() else {
                continue;
            };
            let coarsest = (s.full_face / s.copy_face.max(1)).max(1).ilog2();
            let level = frame_level(s.full_face, coarsest, s.lod, radians);
            if level >= coarsest {
                continue;
            }
            let face = s.full_face.div_ceil(1 << level);
            needs.push(Need {
                frame: frame.id,
                raster: s.raster,
                full_face: s.full_face,
                coarsest,
                level,
                base: s.base,
                edits: s.edits,
                tiles: grid.tiles(face),
                fallback: None,
            });
        }
        let wanted: Vec<(u32, RasterId, u32)> =
            needs.iter().map(|n| (n.frame, n.raster, n.level)).collect();
        self.fit_budget(&mut needs);
        self.coarser = wanted
            .into_iter()
            .filter(|&(frame, raster, level)| {
                let kept = needs
                    .iter()
                    .find(|n| n.frame == frame && n.raster == raster);
                kept.is_none_or(|n| n.level != level)
            })
            .map(|(frame, ..)| frame)
            .collect();

        // A map for each need.
        for need in &needs {
            let found = self.maps.iter_mut().find(|m| {
                m.raster == need.raster && m.level == need.level && m.edits == need.edits
            });
            let map = match found {
                Some(map) => map,
                None => {
                    *generations += 1;
                    let key = MapKey {
                        layer: LayerKey::Raster(need.raster),
                        generation: *generations,
                        kind: MapKind::Detail { level: need.level },
                        face_size: need.face_size(),
                    };
                    if let Some(link) = link {
                        link.push(key, Op::Reset(need.base));
                    }
                    self.maps.push(DetailMap {
                        key,
                        raster: need.raster,
                        edits: need.edits,
                        full_face: need.full_face,
                        level: need.level,
                        tiles: HashMap::new(),
                        parents: HashMap::new(),
                        peak: 0,
                        used: self.tick,
                    });
                    self.maps.last_mut().expect("pushed above")
                }
            };
            map.used = self.tick;
            for tile in &need.tiles {
                if let Some(used) = map.tiles.get_mut(tile) {
                    *used = self.tick;
                }
            }
        }

        // A frame whose map is not ready keeps the map that it used, if
        // that map has each tile of the window.
        let old_needs = std::mem::take(&mut self.needs);
        for need in &mut needs {
            if self.map_of(need).is_some_and(|m| m.has_all(&need.tiles)) {
                continue;
            }
            let before = old_needs
                .iter()
                .find(|n| n.frame == need.frame && n.raster == need.raster)
                .and_then(|n| match self.ready_key(n) {
                    Some(key) => Some(key),
                    None => n.fallback,
                });
            let Some(key) = before else { continue };
            let Some(grid) = self.grid_of(need.frame) else {
                continue;
            };
            let tiles = grid.tiles(key.face_size);
            let tick = self.tick;
            let map = self.maps.iter_mut().find(|m| m.key == key);
            if let Some(map) = map.filter(|m| m.edits == need.edits && m.has_all(&tiles)) {
                map.used = tick;
                for tile in &tiles {
                    if let Some(used) = map.tiles.get_mut(tile) {
                        *used = tick;
                    }
                }
                need.fallback = Some(key);
            }
        }
        self.needs = needs;

        // The tiles that no frame needs any more do not wait for the file.
        let in_flight = std::mem::take(&mut self.in_flight);
        self.in_flight = in_flight.into_iter().filter(|r| self.needed(r)).collect();

        // A map that no frame used for a time goes away.
        let tick = self.tick;
        self.maps.retain(|map| tick - map.used <= IDLE_FRAMES);
        self.trim();
    }

    /// Raises the level of the frame with the most tiles until the needs
    /// fit in the budget.
    fn fit_budget(&self, needs: &mut Vec<Need>) {
        loop {
            let mut maps: Vec<(RasterId, u32, u32, Vec<TileKey>)> = Vec::new();
            for need in needs.iter() {
                let found = maps
                    .iter_mut()
                    .find(|(r, l, _, _)| *r == need.raster && *l == need.level);
                match found {
                    Some((_, _, _, tiles)) => tiles.extend(&need.tiles),
                    None => maps.push((
                        need.raster,
                        need.level,
                        need.face_size(),
                        need.tiles.clone(),
                    )),
                }
            }
            let bytes: u64 = maps
                .iter_mut()
                .map(|(_, _, face, tiles)| {
                    tiles.sort_unstable();
                    tiles.dedup();
                    map_bytes(*face, tiles)
                })
                .sum();
            if bytes <= self.budget() {
                return;
            }
            let Some(most) = (0..needs.len()).max_by_key(|&i| needs[i].tiles.len()) else {
                return;
            };
            let need = &mut needs[most];
            need.level += 1;
            if need.level >= need.coarsest {
                needs.remove(most);
                continue;
            }
            let face = need.face_size();
            match self.grid_of(need.frame) {
                Some(grid) => need.tiles = grid.tiles(face),
                None => {
                    needs.remove(most);
                }
            }
        }
    }

    fn grid_of(&self, frame: u32) -> Option<Arc<Grid>> {
        let found = self.grids.iter().find(|(id, _, _)| *id == frame);
        found.map(|(_, _, grid)| grid.clone())
    }

    fn map_of(&self, need: &Need) -> Option<&DetailMap> {
        self.maps
            .iter()
            .find(|m| m.raster == need.raster && m.level == need.level && m.edits == need.edits)
    }

    /// The key of the map of a need, if it has each tile of the need.
    fn ready_key(&self, need: &Need) -> Option<MapKey> {
        let map = self.map_of(need)?;
        map.has_all(&need.tiles).then_some(map.key)
    }

    /// True if a frame needs the tile of a request.
    fn needed(&self, req: &DetailRequest) -> bool {
        let Some(map) = self
            .maps
            .iter()
            .find(|m| m.key.generation == req.generation)
        else {
            return false;
        };
        self.needs.iter().any(|n| {
            n.raster == map.raster
                && n.level == map.level
                && n.edits == map.edits
                && n.tiles.binary_search(&req.key()).is_ok()
        })
    }

    /// Each tile that a frame needs and that the GPU does not have, and
    /// that does not wait for the project file.
    pub(super) fn missing(&self) -> Vec<DetailRequest> {
        let mut out = Vec::new();
        for need in &self.needs {
            let Some(map) = self.map_of(need) else {
                continue;
            };
            for tile in &need.tiles {
                if map.tiles.contains_key(tile) {
                    continue;
                }
                let req = map.request(*tile);
                if !out.contains(&req) && !self.in_flight.contains(&req) {
                    out.push(req);
                }
            }
        }
        out
    }

    /// Marks requests as waiting for the project file.
    pub(super) fn start(&mut self, reqs: &[DetailRequest]) {
        self.in_flight.extend_from_slice(reqs);
    }

    /// The number of requests that wait for the project file.
    pub(super) fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    /// True if a request waits for the project file and a frame needs it.
    pub(super) fn wants(&self, req: &DetailRequest) -> bool {
        self.in_flight.contains(req)
    }

    /// Takes a request from the list of the requests that wait for the
    /// project file. Returns `false` if it was not there.
    pub(super) fn finish(&mut self, req: &DetailRequest) -> bool {
        let before = self.in_flight.len();
        self.in_flight.retain(|r| r != req);
        before != self.in_flight.len()
    }

    /// Puts the texels of one tile into its detail map: level 0 and the
    /// block of level 1 under it. `None`: each texel has the base level.
    pub(super) fn store(
        &mut self,
        req: &DetailRequest,
        data: Option<(Vec<u16>, Vec<u16>)>,
        link: Option<&Link>,
    ) {
        let budget = self.budget();
        let tick = self.tick;
        let others: u64 = self
            .maps
            .iter()
            .filter(|m| m.key.generation != req.generation)
            .map(DetailMap::bytes)
            .sum();
        let Some(map) = self
            .maps
            .iter_mut()
            .find(|m| m.key.generation == req.generation)
        else {
            return;
        };
        let key = req.key();
        if map.tiles.contains_key(&key) {
            return;
        }
        // A new tile that makes the pool grow over the budget takes the
        // place of the oldest tile that no frame needs.
        let parent = (req.face, req.column / 2, req.row / 2);
        let grows = 1 + usize::from(!map.parents.contains_key(&parent));
        while map.held() + grows > map.peak {
            let tile = tile_size(map.key.face_size);
            let after = table_bytes(map.key.face_size, tile) + pool_bytes(map.held() + grows, tile);
            if others + after <= budget {
                break;
            }
            let oldest = map
                .tiles
                .iter()
                .filter(|(_, used)| **used < tick)
                .min_by_key(|(_, used)| **used)
                .map(|(t, _)| *t);
            match oldest {
                Some(t) if t != key => map.release(t, link),
                _ => break,
            }
        }
        if let (Some(link), Some((level0, level1))) = (link, data) {
            let face = req.face as u32;
            let rect = req.rect;
            link.push(
                map.key,
                Op::UploadLevel {
                    level: 0,
                    face,
                    rect,
                    data: level0,
                },
            );
            let rect = next_rect(req.rect, req.face_size);
            link.push(
                map.key,
                Op::UploadLevel {
                    level: 1,
                    face,
                    rect,
                    data: level1,
                },
            );
        }
        map.tiles.insert(key, tick);
        *map.parents.entry(parent).or_insert(0) += 1;
        map.peak = map.peak.max(map.held());
    }

    /// Takes whole maps that no frame uses now from the GPU while the maps
    /// hold more memory than the budget, the oldest first.
    fn trim(&mut self) {
        let budget = self.budget();
        loop {
            let bytes: u64 = self.maps.iter().map(DetailMap::bytes).sum();
            if bytes <= budget {
                return;
            }
            let idle = self
                .maps
                .iter()
                .enumerate()
                .filter(|(_, m)| m.used < self.tick);
            let Some((at, _)) = idle.min_by_key(|(_, m)| m.used) else {
                return;
            };
            self.maps.remove(at);
        }
    }

    /// Counts one frame. With no `want` for a time, each map goes away.
    pub(super) fn idle_frame(&mut self) {
        self.idle += 1;
        if self.idle > IDLE_FRAMES {
            self.clear();
        }
    }

    /// Takes each map and each need.
    pub(super) fn clear(&mut self) {
        self.maps.clear();
        self.needs.clear();
        self.in_flight.clear();
        self.frames.clear();
        self.coarser.clear();
    }

    /// True if a frame of the last `want` reads each raster layer at the
    /// level that its pixels need.
    pub(super) fn fits(&self, frame: u32) -> bool {
        self.frames.contains(&frame) && !self.coarser.contains(&frame)
    }

    /// The detail map that a frame draws a raster layer from. `None`: the
    /// frame draws the copy.
    pub(super) fn source(&self, frame: u32, raster: RasterId) -> Option<MapKey> {
        let need = self
            .needs
            .iter()
            .find(|n| n.frame == frame && n.raster == raster)?;
        self.ready_key(need).or(need.fallback)
    }

    /// True if each tile that a frame of the last `want` needs is on the
    /// GPU. `false` for a frame that the last `want` did not have.
    pub(super) fn ready(&self, frame: u32) -> bool {
        let mut needs = self.needs.iter().filter(|n| n.frame == frame);
        self.frames.contains(&frame) && needs.all(|n| self.ready_key(n).is_some())
    }

    /// True while a frame waits for tiles.
    pub(super) fn loading(&self) -> bool {
        self.needs.iter().any(|n| self.ready_key(n).is_none())
    }

    /// The key of each detail map on the GPU.
    pub(super) fn keys(&self) -> Vec<MapKey> {
        self.maps.iter().map(|map| map.key).collect()
    }

    /// The memory of the detail maps on the GPU, in bytes, by the estimate.
    pub(super) fn bytes(&self) -> u64 {
        self.maps.iter().map(DetailMap::bytes).sum()
    }

    /// The level of the detail map that a frame needs for a raster layer.
    /// `None`: the frame reads the copy.
    pub(super) fn level(&self, frame: u32, raster: RasterId) -> Option<u32> {
        let need = self
            .needs
            .iter()
            .find(|n| n.frame == frame && n.raster == raster);
        need.map(|n| n.level)
    }

    /// The tiles that a frame needs for a raster layer.
    pub(super) fn tiles(&self, frame: u32, raster: RasterId) -> Vec<TileKey> {
        let need = self
            .needs
            .iter()
            .find(|n| n.frame == frame && n.raster == raster);
        need.map(|n| n.tiles.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use vale_store::{BlendMode, Hillshade, RasterDisplay, Smoothing};
    use vale_terrain::next_level;

    use super::super::flat::{DEFAULT_SPEC, FlatMesh};
    use super::super::{FrameDraw, FramePlace, Globe, StackItem};
    use super::*;

    const RASTER: RasterId = RasterId(7);
    const FACE: usize = 8192;

    /// A globe with one visible raster layer of 8,192 texels in a face,
    /// with no smoothing. `memory`: the whole heightmap is in the main
    /// memory. Else the project file holds its tiles.
    fn globe(memory: bool) -> Globe {
        let mut globe = Globe::with_face_size(64);
        let map = Heightmap::new(FACE, 1000);
        if memory {
            assert!(globe.add_raster(RASTER, map, true));
        } else {
            let reduced = super::super::reduce_map(&map);
            assert!(globe.add_reduced(RASTER, reduced, FACE, true));
        }
        globe.set_stack(vec![StackItem::Raster {
            id: RASTER,
            opacity: 1.0,
            display: RasterDisplay::Grey,
            smoothing: Smoothing {
                on: false,
                ..Smoothing::default()
            },
            hillshade: Hillshade::default(),
            rivers: false,
            lines: false,
            blend: BlendMode::Normal,
        }]);
        globe
    }

    /// A frame of `size` pixels with `pixels_per_radian` pixels in one unit
    /// of the projection, centered on the point `map_center` of the map.
    fn frame(id: u32, size: u32, pixels_per_radian: f32, map_center: [f32; 2]) -> FrameDraw {
        let projection = Projection::new(DEFAULT_SPEC, 1.0).unwrap();
        FrameDraw {
            id,
            place: FramePlace {
                center: [size as f32 / 2.0; 2],
                map_center,
                units_to_pixels: pixels_per_radian,
                rotation: 0.0,
                points_per_radian: f64::from(pixels_per_radian),
                pixels_per_page_point: 1.0,
            },
            rect: [0, 0, size, size],
            mesh: Arc::new(FlatMesh::new(&projection)),
            mask: None,
            shown: vec![RASTER],
            images: Vec::new(),
            raster_looks: Vec::new(),
            image_looks: Vec::new(),
        }
    }

    #[test]
    fn a_zoomed_frame_needs_the_full_texels_under_it_only() {
        let mut globe = globe(true);
        globe.want_frames(&[frame(1, 500, 20000.0, [0.0, 0.0])]);
        assert_eq!(globe.frame_detail_level(1, RASTER), Some(0));
        let tiles = globe.frame_detail_tiles(1, RASTER);
        // The middle of the map is the middle of face 0.
        let middle = tile_of([1.0, 0.0, 0.0], FACE as u32, tile_size(FACE as u32));
        assert!(tiles.contains(&middle), "{tiles:?}");
        // The frame is on the corner of four tiles, and each side has one
        // tile more.
        assert!(tiles.len() <= 16, "{tiles:?}");
        for (face, column, row) in tiles {
            assert_eq!(face, middle.0);
            assert!(column.abs_diff(middle.1) <= 2 && row.abs_diff(middle.2) <= 2);
        }
    }

    #[test]
    fn a_frame_of_the_whole_world_reads_the_copy() {
        let mut globe = globe(true);
        let world = frame(1, 1000, 160.0, [0.0, 0.0]);
        globe.want_frames(std::slice::from_ref(&world));
        assert_eq!(globe.frame_detail_level(1, RASTER), None);
        assert!(globe.frame_detail_tiles(1, RASTER).is_empty());
        assert_eq!(globe.detail_bytes(), 0);
        assert!(globe.frame_ready(&world));
    }

    #[test]
    fn a_frame_over_the_budget_takes_a_coarser_level() {
        let mut globe = globe(true);
        let big = frame(1, 1400, 3000.0, [0.0, 0.0]);
        globe.want_frames(std::slice::from_ref(&big));
        assert_eq!(globe.frame_detail_level(1, RASTER), Some(0));
        assert!(globe.frame_fits(1));
        let budget = 20 << 20;
        let mut globe = super::tests::globe(true);
        globe.set_detail_budget(Some(budget));
        globe.want_frames(std::slice::from_ref(&big));
        assert_eq!(globe.frame_detail_level(1, RASTER), Some(1));
        assert!(!globe.frame_fits(1));
        for _ in 0..10 {
            globe.want_frames(std::slice::from_ref(&big));
        }
        assert!(globe.frame_ready(&big));
        assert!(globe.detail_bytes() <= budget, "{}", globe.detail_bytes());
    }

    #[test]
    fn a_tile_from_the_main_memory_has_the_reduced_texels_and_their_next_level() {
        let n = 1024;
        let mut map = Heightmap::new(n, 1000);
        for y in 0..600 {
            for x in 0..700 {
                map.set(2, x, y, ((x * 31 + y * 17) % 5000) as u16 * 13);
            }
        }
        let level = 1;
        let size = (n >> level) as u32;
        let rect = TexelRect {
            x0: 256,
            y0: 0,
            x1: 512,
            y1: 256,
        };
        let req = DetailRequest {
            raster: RASTER,
            generation: 1,
            level,
            face_size: size,
            full_face_size: n as u32,
            face: 2,
            column: 1,
            row: 0,
            rect,
            source: TexelRect {
                x0: 512,
                y0: 0,
                x1: 1024,
                y1: 512,
            },
        };
        let (level0, level1) = detail_tile(&map, &req);
        let mut want = Vec::new();
        map.read_reduced(2, rect, 2, &mut want);
        assert_eq!(level0, want);

        // The block of level 1 is that of the next level of the whole face.
        let all = TexelRect {
            x0: 0,
            y0: 0,
            x1: size as usize,
            y1: size as usize,
        };
        let mut face = Vec::new();
        map.read_reduced(2, all, 2, &mut face);
        let next = next_level(&face, size as usize);
        let half = size as usize / 2;
        let want: Vec<u16> = (0..128)
            .flat_map(|y| (128..256).map(move |x| (x, y)))
            .map(|(x, y)| next[y * half + x])
            .collect();
        assert_eq!(level1, want);
        assert_eq!(req.source_tiles(256), (2..4, 0..2));
    }

    #[test]
    fn a_tile_that_no_frame_needs_any_more_is_cancelled() {
        let mut globe = globe(false);
        let here = frame(1, 500, 20000.0, [0.0, 0.0]);
        globe.want_frames(std::slice::from_ref(&here));
        let reqs = globe.detail_requests();
        assert!(!reqs.is_empty() && reqs.len() <= DETAIL_IN_FLIGHT);
        assert!(!globe.frame_ready(&here));
        assert!(reqs.iter().all(|r| globe.wants_detail(r)));

        // The frame moves far away.
        let there = frame(1, 500, 20000.0, [1.0, 0.5]);
        globe.want_frames(std::slice::from_ref(&there));
        assert!(reqs.iter().all(|r| !globe.wants_detail(r)));
        globe.give_detail_tile(reqs[0], None);
        assert!(!globe.frame_ready(&there));

        // The tiles of the frame arrive.
        for _ in 0..10 {
            for req in globe.detail_requests() {
                globe.give_detail_tile(req, None);
            }
            globe.want_frames(std::slice::from_ref(&there));
        }
        assert!(globe.frame_ready(&there));
        assert!(globe.detail_requests().is_empty());
    }
}
