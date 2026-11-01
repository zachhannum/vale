//! The tile store and the brush.
//!
//! Each face splits into square tiles. A tile takes memory from the first
//! change of one of its texels. Each other tile reads as the base level.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, TAU};
use std::sync::Arc;

use crate::bands::{Bands, SEA_LEVEL};
use crate::channel::{ChannelCut, ChannelShape};
use crate::cube::{ELEV_MAX, ELEV_MIN, FACES, face_dir, face_of, meters_to_level, unwarp, warp};
use crate::erode::ErodeStep;
use crate::flow::{ChannelMap, ChannelWindow};
use crate::line::{LinePlace, LinePlan, LinePoint};
use crate::math::{V3, add, cross, dot, length, normalize, scale};
use crate::range::RangePlan;
use crate::ridge::RidgePlan;

/// The side of a tile, in texels.
pub const TILE_SIZE: usize = 256;

/// The largest brush radius, in radians. The face test in `stamp` needs it.
pub const MAX_BRUSH_RADIUS: f64 = 0.3;

/// The default limit of the memory of the undo and redo steps, in bytes.
pub const UNDO_MEMORY_LIMIT: usize = 256 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Raise,
    Lower,
    Smooth,
    Flatten,
    /// Cuts a channel along each river of the channel lines, with the width
    /// and the floodplain of `Stamp::channel`. The bed of a channel is
    /// `Stamp::strength` below the floodplain level for the largest rivers,
    /// and a quarter of that for the smallest. The ground beside a large
    /// river on a low slope goes down to the floodplain level. The mode only
    /// lowers the ground, and land does not go below sea level.
    Carve,
    /// Raises the land by an amount that grows with the distance from the
    /// nearest river of the channel map, up to `RIDGE_REACH`. The ground on a
    /// river and the sea floor do not change.
    Ridge,
    /// Raises the ground toward the fill level of the channel map. That
    /// level is the rim of a pit, or a level between the bed and the banks
    /// of a river. Ground at or above the level and the sea floor do not
    /// change.
    Deposit,
    /// The mode of the erode brush. A stamp of this mode changes nothing.
    /// `Heightmap::erode` makes the change.
    Erode,
    /// Moves ground from a texel to each lower neighbor where the slope
    /// between them is more than `Stamp::talus`. The sum of the levels stays.
    Weather,
    /// Raises the ground as `Raise` does. The app also runs erode steps and
    /// weather stamps under the brush.
    Uplift,
    /// Moves the ground sideways. Each texel takes the level of the ground at
    /// an offset place, and a noise of the direction of the texel gives the
    /// offset. Each new level is between the lowest level and the highest
    /// level under the brush.
    Warp,
    /// The mode of the ocean ridge brush. A stamp of this mode changes
    /// nothing. `Heightmap::ridge` makes the change.
    OceanRidge,
    /// The mode of the range brush. A stamp of this mode changes nothing.
    /// `Heightmap::range` makes the change.
    Range,
    /// Moves the ground in the direction of `Stamp::motion`. Each texel takes
    /// the level of the ground behind it, at the motion times the effect of
    /// the brush. Each new level is between the lowest level and the highest
    /// level under the brush.
    Push,
    /// Adds fractal noise of the direction of each texel, times the effect
    /// of the brush and the strength. `Stamp::ridged` and `Stamp::relief`
    /// set its shape.
    Noise,
}

/// The elevation limits of a stamp, in 16-bit steps. A stamp changes only
/// the texels from the floor to the ceiling, and it does not move them past
/// a limit. A texel below the floor or above the ceiling does not change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub floor: u16,
    pub ceiling: u16,
}

impl Default for Limits {
    /// Limits with no effect.
    fn default() -> Limits {
        Limits {
            floor: 0,
            ceiling: 65535,
        }
    }
}

impl Limits {
    /// Whether a texel at `level` can change.
    pub fn holds(self, level: f64) -> bool {
        f64::from(self.floor) <= level && level <= f64::from(self.ceiling)
    }

    /// The level of a texel after a change from `old` to `new`.
    pub fn apply(self, old: f64, new: f64) -> f64 {
        if !self.holds(old) {
            old
        } else if new < old {
            new.max(f64::from(self.floor))
        } else {
            new.min(f64::from(self.ceiling))
        }
    }

    /// The largest number of levels that one pair of a weather stamp moves
    /// from a texel at `high` to a texel at `low`. A texel is in 4 pairs at
    /// most, so the sum of its moves does not cross a limit. A pair with a
    /// texel outside the limits does not move.
    fn pair_room(self, high: f64, low: f64) -> f64 {
        if !self.holds(high) || !self.holds(low) {
            return 0.0;
        }
        let down = high - f64::from(self.floor);
        let up = f64::from(self.ceiling) - low;
        (down.min(up) * 0.25).floor()
    }
}

/// One touch of the brush on the sphere.
#[derive(Clone, Copy, Debug)]
pub struct Stamp {
    /// The brush center, a unit vector.
    pub center: V3,
    /// The brush radius as an angle, in radians.
    pub radius: f64,
    /// The part of the radius with full effect, from 0 to 1.
    pub hardness: f64,
    /// The effect at the center, from 0 to 1. Pressure sets this value.
    pub flow: f64,
    pub mode: Mode,
    /// The target of the flatten mode.
    pub level: u16,
    /// The largest change of the raise, lower, and noise modes, in 16-bit
    /// steps. In the warp mode, the largest offset as an angle, in radians.
    pub strength: f64,
    /// The steepest slope that the weather mode leaves, in 16-bit steps for
    /// each texel step. `talus_levels` gives it for an angle.
    pub talus: f64,
    /// The size of the features of the noise of the warp mode, or of the
    /// largest features of the noise mode, as an angle in radians.
    pub scale: f64,
    /// The seed of the noise of the warp mode and the noise mode.
    pub seed: u32,
    /// The move of the pen to this stamp from the stamp before it, in the
    /// push mode. It is square to the center, and its length is the angle in
    /// radians.
    pub motion: V3,
    /// Whether the noise mode folds each octave of its noise at zero, which
    /// gives sharp crests.
    pub ridged: bool,
    /// Whether the noise mode turns its ridged noise upside down, which gives
    /// sharp valleys. The soft noise does not change.
    pub invert: bool,
    /// How much the noise of the noise mode grows with the height of the
    /// ground above sea level, from 0 to 1. At 1, the noise is zero at sea
    /// level and full at `NOISE_RELIEF_M` above it.
    pub relief: f64,
    /// The width and the floodplain of the channels of the carve mode.
    pub channel: ChannelShape,
    pub limits: Limits,
}

/// The height above sea level, in meters, where the noise mode has its full
/// effect at each relief setting.
pub const NOISE_RELIEF_M: f64 = 2000.0;

/// The most octaves of the noise mode.
pub const NOISE_OCTAVES: u32 = 8;

/// The offset of the warp mode and of the push mode is zero at this number of
/// texel steps from the brush edge. Each texel that a warp stamp or a push
/// stamp reads is then in the brush.
const WARP_MARGIN: f64 = 3.0;

/// The slope of an angle on a world, in 16-bit steps for each texel step.
/// The texel step is the one at the center of a face.
pub fn talus_levels(angle_deg: f64, face_size: usize, world_radius_m: f64) -> f64 {
    let texel_m = world_radius_m * FRAC_PI_2 / face_size as f64;
    angle_deg.to_radians().tan() * texel_m / ((ELEV_MAX - ELEV_MIN) / 65535.0)
}

/// The effect of one stamp at each place on one face.
struct Falloff {
    /// The brush center along the face axis and along the two face
    /// coordinates.
    center: [f64; 3],
    radius: f64,
    cos_radius: f64,
    hardness: f64,
    flow: f64,
}

impl Falloff {
    fn new(face: usize, stamp: &Stamp) -> Falloff {
        let axis = face / 2;
        let sign = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
        let c = stamp.center;
        Falloff {
            center: [c[axis] * sign, c[(axis + 1) % 3], c[(axis + 2) % 3]],
            radius: stamp.radius,
            cos_radius: stamp.radius.cos(),
            hardness: stamp.hardness.clamp(0.0, 0.999),
            flow: stamp.flow,
        }
    }

    /// The effect at a flat face coordinate, or `None` outside the brush
    /// circle.
    fn amount(&self, fu: f64, fv: f64) -> Option<f64> {
        let [cn, cu, cv] = self.center;
        let cos_angle = (cn + fu * cu + fv * cv) / (1.0 + fu * fu + fv * fv).sqrt();
        if cos_angle <= self.cos_radius {
            return None;
        }
        let k = cos_angle.min(1.0).acos() / self.radius;
        Some(falloff(k, self.hardness) * self.flow)
    }
}

/// A rectangle of texels on one face. `x1` and `y1` are exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TexelRect {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}

impl TexelRect {
    pub(crate) fn union(self, o: TexelRect) -> TexelRect {
        TexelRect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    /// The smallest rectangle of reduced texels that covers this rectangle.
    /// A reduced texel covers `factor` by `factor` texels.
    pub fn reduced(self, factor: usize) -> TexelRect {
        TexelRect {
            x0: self.x0 / factor,
            y0: self.y0 / factor,
            x1: self.x1.div_ceil(factor),
            y1: self.y1.div_ceil(factor),
        }
    }
}

/// The texels that one stamp can touch.
#[derive(Clone, Copy, Debug)]
pub struct StampPlan {
    /// The stamp, with the radius inside the limits of the brush.
    pub stamp: Stamp,
    /// The rectangle of each face that the brush circle can touch.
    pub rects: [Option<TexelRect>; FACES],
    /// The distance from a texel to the neighbors that the smooth mode reads,
    /// in texels.
    pub reach: usize,
}

/// The texels of one tile. A snapshot shares it, and the first write after
/// that makes a copy.
type Tile = Arc<[u16]>;

/// One undo or redo step: the base level and the tiles that one stroke
/// changed. An undo step holds them from before the stroke, and a redo step
/// holds them from after it. A tile that had no memory is `None`.
struct Saved {
    base: u16,
    tiles: HashMap<usize, Option<Tile>>,
}

impl Saved {
    /// The memory of the tiles, in bytes.
    fn bytes(&self) -> usize {
        let texels: usize = self.tiles.values().flatten().map(|tile| tile.len()).sum();
        texels * size_of::<u16>()
    }
}

pub struct Heightmap {
    n: usize,
    tile: usize,
    /// The number of tiles along one side of a face.
    per_side: usize,
    /// The level of each texel in a tile that has no memory.
    base: u16,
    /// The tiles of all faces, face by face, in row order.
    tiles: Vec<Option<Tile>>,
    /// The flat face coordinate at each texel center.
    flat: Vec<f64>,
    dirty: [Option<TexelRect>; FACES],
    /// The base level or the full set of tiles is new.
    reset: bool,
    stroke: Option<Saved>,
    /// The undo steps, from the oldest to the newest.
    undo: VecDeque<Saved>,
    /// The redo steps. The last one is the next redo.
    redo: VecDeque<Saved>,
    undo_limit: usize,
    /// With `false`, no undo and redo steps are kept.
    keep_steps: bool,
    /// The tiles that changed after the last `clear_changes`.
    changed: BTreeSet<usize>,
    /// The base level changed after the last `clear_changes`.
    changed_all: bool,
    /// The rivers that the carve mode and the ridge mode follow.
    channels: Option<Arc<ChannelMap>>,
    /// The rivers of a part of the sphere at a small scale. The carve mode
    /// and the ridge mode follow them where the window has them.
    window: Option<Arc<ChannelWindow>>,
    /// The color stops of the preview.
    pub bands: Bands,
}

impl Heightmap {
    /// Makes a heightmap with `n` by `n` texels on each face, all at `level`.
    /// It starts with all faces dirty.
    pub fn new(n: usize, level: u16) -> Heightmap {
        Heightmap::with_tile_size(n, TILE_SIZE, level)
    }

    /// Makes a heightmap with tiles of `tile` by `tile` texels.
    pub fn with_tile_size(n: usize, tile: usize, level: u16) -> Heightmap {
        assert!(n > 0 && tile > 0, "the sizes must not be zero");
        let per_side = n.div_ceil(tile);
        let mut map = Heightmap {
            n,
            tile,
            per_side,
            base: level,
            tiles: (0..FACES * per_side * per_side).map(|_| None).collect(),
            flat: (0..n)
                .map(|i| unwarp((i as f64 + 0.5) / n as f64 * 2.0 - 1.0))
                .collect(),
            dirty: [None; FACES],
            reset: true,
            stroke: None,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            undo_limit: UNDO_MEMORY_LIMIT,
            keep_steps: true,
            changed: BTreeSet::new(),
            changed_all: false,
            channels: None,
            window: None,
            bands: Bands::default(),
        };
        map.mark_all_dirty();
        map
    }

    /// A copy that shares the tiles. It has no undo steps and no dirty
    /// state. A later change to one of the two copies a tile at its first
    /// write, and the other keeps its levels.
    pub fn snapshot(&self) -> Heightmap {
        Heightmap {
            n: self.n,
            tile: self.tile,
            per_side: self.per_side,
            base: self.base,
            tiles: self.tiles.clone(),
            flat: self.flat.clone(),
            dirty: [None; FACES],
            reset: false,
            stroke: None,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            undo_limit: self.undo_limit,
            keep_steps: self.keep_steps,
            changed: BTreeSet::new(),
            changed_all: false,
            channels: self.channels.clone(),
            window: self.window.clone(),
            bands: self.bands.clone(),
        }
    }

    pub fn face_size(&self) -> usize {
        self.n
    }

    /// The level of each texel in a tile that has no memory.
    pub fn base(&self) -> u16 {
        self.base
    }

    /// Sets the rivers that the carve mode and the ridge mode follow, and the
    /// fill levels of the deposit mode. With `None`, no river is near each
    /// place, and the deposit mode changes nothing.
    pub fn set_channels(&mut self, channels: Option<Arc<ChannelMap>>) {
        self.channels = channels;
    }

    pub fn channels(&self) -> Option<&Arc<ChannelMap>> {
        self.channels.as_ref()
    }

    /// Sets the window of small rivers. The carve mode, the ridge mode, and
    /// the deposit mode follow it on the ground that `Window::covers`, and the channel map on
    /// the other ground.
    pub fn set_window(&mut self, window: Option<Arc<ChannelWindow>>) {
        self.window = window;
    }

    pub fn window(&self) -> Option<&Arc<ChannelWindow>> {
        self.window.as_ref()
    }

    /// The face and the texels of each tile that holds memory.
    pub fn allocated_rects(&self) -> impl Iterator<Item = (usize, TexelRect)> + '_ {
        self.tiles
            .iter()
            .enumerate()
            .filter(|(_, tile)| tile.is_some())
            .map(|(index, _)| self.tile_rect(index))
    }

    /// The index of each tile that holds memory.
    pub fn allocated(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.tiles.len()).filter(|&index| self.tiles[index].is_some())
    }

    /// The side of a tile, in texels.
    pub fn tile_size(&self) -> usize {
        self.tile
    }

    /// The face, the column, and the row of a tile.
    pub fn tile_key(&self, index: usize) -> (usize, usize, usize) {
        let side = self.per_side;
        (index / (side * side), index % side, index / side % side)
    }

    /// The texels of a tile, row by row. A tile at the far side of a face
    /// can go past the face, and those texels have the base level. `None`: the
    /// tile holds no memory.
    pub fn tile(&self, index: usize) -> Option<&[u16]> {
        self.tiles[index].as_deref()
    }

    /// Sets the texels of one tile from a saved copy. The call makes no undo
    /// step, and it does not mark the tile for a save.
    pub fn load_tile(&mut self, face: usize, column: usize, row: usize, data: &[u16]) {
        assert!(
            face < FACES && column < self.per_side && row < self.per_side,
            "the tile key"
        );
        assert_eq!(data.len(), self.tile * self.tile, "the data size");
        let index = (face * self.per_side + row) * self.per_side + column;
        self.tiles[index] = Some(data.into());
        let (face, rect) = self.tile_rect(index);
        self.mark_dirty(face, rect);
    }

    /// Frees one tile, so that its texels have the base level. As with
    /// `load_tile`, the call makes no undo step and no mark for a save.
    pub fn clear_tile(&mut self, face: usize, column: usize, row: usize) {
        assert!(
            face < FACES && column < self.per_side && row < self.per_side,
            "the tile key"
        );
        let index = (face * self.per_side + row) * self.per_side + column;
        if self.tiles[index].take().is_some() {
            let (face, rect) = self.tile_rect(index);
            self.mark_dirty(face, rect);
        }
    }

    /// Sets the base level from a saved copy. The call makes no undo step,
    /// and it does not mark the heightmap for a save.
    pub fn load_base(&mut self, level: u16) {
        if self.base != level {
            self.base = level;
            self.mark_all_dirty();
            self.reset = true;
        }
    }

    /// True if the texels changed after the last `clear_changes`.
    pub fn has_changes(&self) -> bool {
        self.changed_all || !self.changed.is_empty()
    }

    /// True if the base level changed after the last `clear_changes`. A saved
    /// copy then needs each tile again.
    pub fn changed_all(&self) -> bool {
        self.changed_all
    }

    /// The index of each tile that changed after the last `clear_changes`. A
    /// tile that lost its memory is one of them.
    pub fn changed(&self) -> impl Iterator<Item = usize> + '_ {
        self.changed.iter().copied()
    }

    /// Call this after a save of the changed tiles.
    pub fn clear_changes(&mut self) {
        self.changed.clear();
        self.changed_all = false;
    }

    /// The number of tiles that hold memory.
    pub fn allocated_tiles(&self) -> usize {
        self.tiles.iter().flatten().count()
    }

    /// The memory of the texels, in bytes. The undo and redo steps are not
    /// included.
    pub fn memory_bytes(&self) -> usize {
        self.allocated_tiles() * self.tile * self.tile * size_of::<u16>()
    }

    fn tile_index(&self, face: usize, x: usize, y: usize) -> usize {
        (face * self.per_side + y / self.tile) * self.per_side + x / self.tile
    }

    fn offset(&self, x: usize, y: usize) -> usize {
        (y % self.tile) * self.tile + x % self.tile
    }

    /// The texels of one tile, clipped to the face.
    fn tile_rect(&self, index: usize) -> (usize, TexelRect) {
        let (tx, ty) = (index % self.per_side, index / self.per_side % self.per_side);
        let rect = TexelRect {
            x0: tx * self.tile,
            y0: ty * self.tile,
            x1: ((tx + 1) * self.tile).min(self.n),
            y1: ((ty + 1) * self.tile).min(self.n),
        };
        (index / (self.per_side * self.per_side), rect)
    }

    fn mark_dirty(&mut self, face: usize, rect: TexelRect) {
        self.dirty[face] = Some(match self.dirty[face] {
            Some(d) => d.union(rect),
            None => rect,
        });
    }

    fn mark_all_dirty(&mut self) {
        let all = TexelRect {
            x0: 0,
            y0: 0,
            x1: self.n,
            y1: self.n,
        };
        self.dirty = [Some(all); FACES];
    }

    /// Marks each texel for a new upload, so the next `take_reset` is `true`.
    pub fn mark_reset(&mut self) {
        self.mark_all_dirty();
        self.reset = true;
    }

    /// Takes the rectangle of changed texels of each face.
    pub fn take_dirty(&mut self) -> [Option<TexelRect>; FACES] {
        std::mem::take(&mut self.dirty)
    }

    /// Returns `true` one time after the base level or the full set of tiles
    /// changed. A new heightmap, `fill`, the undo or the redo of a `fill`, and
    /// `mark_reset` do this. A reader then needs the base level and each tile
    /// again, so the call also clears the changed rectangles.
    pub fn take_reset(&mut self) -> bool {
        if self.reset {
            self.dirty = [None; FACES];
        }
        std::mem::take(&mut self.reset)
    }

    /// Sets all texels to `level` and frees all tiles.
    pub fn fill(&mut self, level: u16) {
        let mut saved = Saved {
            base: self.base,
            tiles: HashMap::new(),
        };
        for (index, tile) in self.tiles.iter_mut().enumerate() {
            if tile.is_some() {
                saved.tiles.insert(index, tile.take());
                self.changed.insert(index);
            }
        }
        self.stroke = None;
        if saved.base != level || !saved.tiles.is_empty() {
            self.changed_all |= saved.base != level;
            self.base = level;
            self.push_undo(saved);
            self.mark_all_dirty();
            self.reset = true;
        }
    }

    /// The texel index for an equal-angle coordinate.
    fn index(&self, s: f64) -> usize {
        texel_index(self.n, s)
    }

    fn texel(&self, face: usize, x: usize, y: usize) -> u16 {
        match &self.tiles[self.tile_index(face, x, y)] {
            Some(tile) => tile[self.offset(x, y)],
            None => self.base,
        }
    }

    /// The face and the texel that hold a direction.
    pub fn texel_of(&self, d: V3) -> (usize, usize, usize) {
        let (face, a, b) = face_of(d);
        (face, self.index(warp(a)), self.index(warp(b)))
    }

    /// The level at a direction, from the nearest texel.
    pub fn sample(&self, d: V3) -> u16 {
        let (face, x, y) = self.texel_of(d);
        self.texel(face, x, y)
    }

    /// The level of a texel. `x` and `y` can be outside the face by less than
    /// half the face size. The texel grid then continues past the face edge,
    /// and the read gets the nearest texel of the face that is there.
    pub fn get(&self, face: usize, x: i64, y: i64) -> u16 {
        let n = self.n as i64;
        if (0..n).contains(&x) && (0..n).contains(&y) {
            return self.texel(face, x as usize, y as usize);
        }
        let flat =
            |i: i64| unwarp(((i as f64 + 0.5) / self.n as f64 * 2.0 - 1.0).clamp(-1.99, 1.99));
        self.sample(face_dir(face, flat(x), flat(y)))
    }

    /// Sets the level of one texel.
    pub fn set(&mut self, face: usize, x: usize, y: usize, level: u16) {
        if self.texel(face, x, y) == level {
            return;
        }
        let (index, offset) = (self.tile_index(face, x, y), self.offset(x, y));
        let (len, base) = (self.tile * self.tile, self.base);
        let tile = &mut self.tiles[index];
        save_tile(&mut self.stroke, &mut self.changed, index, tile);
        Arc::make_mut(tile.get_or_insert_with(|| vec![base; len].into()))[offset] = level;
        self.mark_dirty(
            face,
            TexelRect {
                x0: x,
                y0: y,
                x1: x + 1,
                y1: y + 1,
            },
        );
    }

    /// Adds the texels of a rectangle to `out`, row by row.
    pub fn read_rect(&self, face: usize, rect: TexelRect, out: &mut Vec<u16>) {
        for y in rect.y0..rect.y1 {
            let mut x = rect.x0;
            while x < rect.x1 {
                let end = ((x / self.tile + 1) * self.tile).min(rect.x1);
                match &self.tiles[self.tile_index(face, x, y)] {
                    Some(tile) => {
                        let offset = self.offset(x, y);
                        out.extend_from_slice(&tile[offset..offset + end - x]);
                    }
                    None => out.resize(out.len() + end - x, self.base),
                }
                x = end;
            }
        }
    }

    /// The texels of the face under a rectangle of reduced texels.
    fn reduced_source(&self, rect: TexelRect, factor: usize) -> TexelRect {
        assert!(factor.is_power_of_two(), "the factor is a power of two");
        let size = self.n.div_ceil(factor);
        assert!(rect.x1 <= size && rect.y1 <= size, "the rectangle");
        TexelRect {
            x0: rect.x0 * factor,
            y0: rect.y0 * factor,
            x1: (rect.x1 * factor).min(self.n),
            y1: (rect.y1 * factor).min(self.n),
        }
    }

    /// True if no tile under a rectangle of reduced texels holds memory. Each
    /// reduced texel then has the base level. `rect` is in reduced texels, as
    /// in `read_reduced`.
    pub fn reduced_is_base(&self, face: usize, rect: TexelRect, factor: usize) -> bool {
        let source = self.reduced_source(rect, factor);
        let t = self.tile;
        (source.y0 / t..source.y1.div_ceil(t)).all(|ty| {
            (source.x0 / t..source.x1.div_ceil(t))
                .all(|tx| self.tiles[(face * self.per_side + ty) * self.per_side + tx].is_none())
        })
    }

    /// Adds the reduced texels of a rectangle to `out`, row by row. A reduced
    /// texel is the rounded mean of `factor` by `factor` texels, and a face
    /// has `face_size().div_ceil(factor)` of them along one side. `rect` is
    /// in reduced texels. `factor` is a power of two, and 1 gives the texels
    /// of `read_rect`.
    ///
    /// A reduced texel at the far side of a face can go past the face. It is
    /// the mean of its texels on the face.
    pub fn read_reduced(&self, face: usize, rect: TexelRect, factor: usize, out: &mut Vec<u16>) {
        if factor == 1 {
            return self.read_rect(face, rect, out);
        }
        let source = self.reduced_source(rect, factor);
        let (width, t) = (rect.x1 - rect.x0, self.tile);
        let start = out.len();
        out.resize(start + width * (rect.y1 - rect.y0), self.base);
        // The mean of the texels of the face in one block.
        let mean = |x: usize, y: usize, level: &dyn Fn(usize, usize) -> u16| {
            let (x0, x1) = (x * factor, ((x + 1) * factor).min(self.n));
            let (y0, y1) = (y * factor, ((y + 1) * factor).min(self.n));
            let mut sum = 0u64;
            for sy in y0..y1 {
                sum += (x0..x1).map(|sx| u64::from(level(sx, sy))).sum::<u64>();
            }
            let count = ((x1 - x0) * (y1 - y0)) as u64;
            ((sum + count / 2) / count) as u16
        };
        if !t.is_multiple_of(factor) {
            // A block can have texels in more than one tile.
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    out[start + (y - rect.y0) * width + x - rect.x0] =
                        mean(x, y, &|sx, sy| self.texel(face, sx, sy));
                }
            }
            return;
        }
        // Each block is in one tile. A tile with no memory keeps the base
        // level in `out`.
        let per_tile = t / factor;
        for ty in source.y0 / t..source.y1.div_ceil(t) {
            for tx in source.x0 / t..source.x1.div_ceil(t) {
                let index = (face * self.per_side + ty) * self.per_side + tx;
                let Some(tile) = &self.tiles[index] else {
                    continue;
                };
                let level = |sx: usize, sy: usize| tile[(sy - ty * t) * t + sx - tx * t];
                for y in rect.y0.max(ty * per_tile)..rect.y1.min((ty + 1) * per_tile) {
                    for x in rect.x0.max(tx * per_tile)..rect.x1.min((tx + 1) * per_tile) {
                        out[start + (y - rect.y0) * width + x - rect.x0] = mean(x, y, &level);
                    }
                }
            }
        }
    }

    /// Writes the texels of a rectangle, row by row. The texels come from a
    /// copy of the heightmap that is already up to date, so the call marks
    /// nothing as changed. A tile with no changed texel takes no memory.
    pub fn store_rect(&mut self, face: usize, rect: TexelRect, data: &[u16]) {
        let (t, base) = (self.tile, self.base);
        let width = rect.x1 - rect.x0;
        assert_eq!(data.len(), width * (rect.y1 - rect.y0), "the data size");
        if data.is_empty() {
            return;
        }
        for ty in rect.y0 / t..=(rect.y1 - 1) / t {
            for tx in rect.x0 / t..=(rect.x1 - 1) / t {
                let index = (face * self.per_side + ty) * self.per_side + tx;
                let (x0, x1) = (rect.x0.max(tx * t), rect.x1.min((tx + 1) * t));
                let (y0, y1) = (rect.y0.max(ty * t), rect.y1.min((ty + 1) * t));
                // The row of `data` and the row of the tile, for each `y`.
                let rows = (y0..y1).map(|y| {
                    let from = (y - rect.y0) * width + (x0 - rect.x0);
                    let to = (y - ty * t) * t + (x0 - tx * t);
                    (&data[from..from + x1 - x0], to..to + x1 - x0)
                });
                let tile = &mut self.tiles[index];
                let same = rows.clone().all(|(new, to)| match tile.as_deref() {
                    Some(tile) => tile[to] == *new,
                    None => new.iter().all(|&level| level == base),
                });
                if same {
                    continue;
                }
                save_tile(&mut self.stroke, &mut self.changed, index, tile);
                let tile = Arc::make_mut(tile.get_or_insert_with(|| vec![base; t * t].into()));
                for (new, to) in rows {
                    tile[to].copy_from_slice(new);
                }
            }
        }
    }

    /// Replaces one whole face, row by row, and marks it as changed. In a
    /// stroke, the old tiles go into the undo entry.
    pub fn store_face(&mut self, face: usize, levels: &[u16]) {
        let full = TexelRect {
            x0: 0,
            y0: 0,
            x1: self.n,
            y1: self.n,
        };
        self.store_rect(face, full, levels);
        self.mark_dirty(face, full);
    }

    /// The texels of a rectangle that can go past the face edges, row by row.
    fn read_past_edges(&self, face: usize, x0: i64, y0: i64, w: usize, h: usize) -> Vec<u16> {
        let n = self.n as i64;
        let (x1, y1) = (x0 + w as i64, y0 + h as i64);
        // The part of each row that is on the face.
        let (xa, xb) = (x0.clamp(0, n), x1.clamp(0, n));
        let mut out = Vec::with_capacity(w * h);
        for y in y0..y1 {
            if !(0..n).contains(&y) || xa >= xb {
                out.extend((x0..x1).map(|x| self.get(face, x, y)));
                continue;
            }
            out.extend((x0..xa).map(|x| self.get(face, x, y)));
            let row = TexelRect {
                x0: xa as usize,
                y0: y as usize,
                x1: xb as usize,
                y1: y as usize + 1,
            };
            self.read_rect(face, row, &mut out);
            out.extend((xb..x1).map(|x| self.get(face, x, y)));
        }
        out
    }

    /// The direction of a texel center.
    pub fn texel_dir(&self, face: usize, x: usize, y: usize) -> V3 {
        face_dir(face, self.flat[x], self.flat[y])
    }

    /// The solid angle of one texel, in steradians.
    pub fn texel_solid_angle(&self, x: usize, y: usize) -> f64 {
        // d(flat)/d(index) at the texel center, then the flat-face area factor.
        let n = self.n as f64;
        let (fu, fv) = (self.flat[x], self.flat[y]);
        let step = |f: f64| (1.0 + f * f) * FRAC_PI_4 * 2.0 / n;
        step(fu) * step(fv) / (1.0 + fu * fu + fv * fv).powf(1.5)
    }

    pub fn begin_stroke(&mut self) {
        self.stroke = Some(Saved {
            base: self.base,
            tiles: HashMap::new(),
        });
    }

    /// Ends the stroke. Returns `true` if the stroke changed the heightmap.
    pub fn end_stroke(&mut self) -> bool {
        match self.stroke.take() {
            Some(saved) if !saved.tiles.is_empty() => {
                self.push_undo(saved);
                true
            }
            _ => false,
        }
    }

    /// Ends the stroke and puts back the tiles from before it. The undo and
    /// redo steps stay as they are.
    pub fn cancel_stroke(&mut self) {
        if let Some(mut saved) = self.stroke.take() {
            self.swap(&mut saved);
        }
    }

    /// Adds an undo step. The redo steps are no longer valid.
    fn push_undo(&mut self, saved: Saved) {
        if !self.keep_steps {
            return;
        }
        self.redo.clear();
        self.undo.push_back(saved);
        self.trim();
    }

    /// Removes steps until the memory is inside the limit. The oldest undo
    /// steps go first. The redo steps go after them, and the next redo goes
    /// last.
    fn trim(&mut self) {
        let mut bytes = self.undo_memory_bytes();
        while bytes > self.undo_limit {
            let Some(step) = self.undo.pop_front().or_else(|| self.redo.pop_front()) else {
                break;
            };
            bytes -= step.bytes();
        }
    }

    /// The memory of the undo and redo steps, in bytes.
    pub fn undo_memory_bytes(&self) -> usize {
        self.undo.iter().chain(&self.redo).map(Saved::bytes).sum()
    }

    /// The limit of the memory of the undo and redo steps, in bytes.
    pub fn undo_memory_limit(&self) -> usize {
        self.undo_limit
    }

    /// With `false`, the heightmap keeps no undo and redo steps, and the
    /// steps that it has go. Use this if a project file keeps the steps. A
    /// stroke can still be cancelled.
    pub fn set_undo_steps(&mut self, keep: bool) {
        self.keep_steps = keep;
        if !keep {
            self.undo.clear();
            self.redo.clear();
        }
    }

    /// Sets the limit of the memory of the undo and redo steps. A step that
    /// is larger than the limit is not kept.
    pub fn set_undo_memory_limit(&mut self, bytes: usize) {
        self.undo_limit = bytes;
        self.trim();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Puts back the tiles from before the last stroke.
    pub fn undo(&mut self) -> bool {
        let Some(mut step) = self.undo.pop_back() else {
            return false;
        };
        self.swap(&mut step);
        self.redo.push_back(step);
        self.trim();
        true
    }

    /// Puts back the tiles from after the last stroke that `undo` took away.
    pub fn redo(&mut self) -> bool {
        let Some(mut step) = self.redo.pop_back() else {
            return false;
        };
        self.swap(&mut step);
        self.undo.push_back(step);
        self.trim();
        true
    }

    /// Exchanges the base level and the tiles of a step with the heightmap.
    fn swap(&mut self, step: &mut Saved) {
        if step.base != self.base {
            std::mem::swap(&mut self.base, &mut step.base);
            self.mark_all_dirty();
            self.reset = true;
            self.changed_all = true;
        }
        for (&index, tile) in &mut step.tiles {
            std::mem::swap(&mut self.tiles[index], tile);
            self.changed.insert(index);
            let (face, rect) = self.tile_rect(index);
            self.mark_dirty(face, rect);
        }
    }

    /// The texels that a stamp can touch.
    pub fn stamp_plan(&self, stamp: &Stamp) -> StampPlan {
        let radius = stamp.radius.clamp(1e-6, MAX_BRUSH_RADIUS);
        // The shader works with 32-bit numbers. A slope minus a multiple of
        // 1/16 is exact there. No slope is more than 65535.
        let talus = ((stamp.talus * 16.0).round() / 16.0).clamp(0.0, 65536.0);
        let step = FRAC_PI_2 / self.n as f64;
        let (strength, scale) = if matches!(stamp.mode, Mode::Warp | Mode::Noise) {
            // The noise changes too fast for the texels below 2 texel steps.
            // The shader gets the same 32-bit feature size.
            let scale = f64::from(stamp.scale.max(2.0 * step) as f32);
            let strength = match stamp.mode {
                Mode::Warp => stamp.strength.clamp(0.0, radius),
                _ => stamp.strength,
            };
            (strength, scale)
        } else {
            (stamp.strength, stamp.scale)
        };
        StampPlan {
            stamp: Stamp {
                radius,
                talus,
                strength,
                scale,
                ..*stamp
            },
            rects: std::array::from_fn(|face| brush_rect(self.n, face, stamp.center, radius)),
            reach: ((radius / (FRAC_PI_2 / self.n as f64)) * 0.25).max(1.0) as usize,
        }
    }

    /// Applies one stamp. Returns the number of texels that it visited.
    pub fn stamp(&mut self, stamp: &Stamp) -> usize {
        if matches!(stamp.mode, Mode::Erode | Mode::OceanRidge | Mode::Range) {
            return 0;
        }
        let plan = self.stamp_plan(stamp);
        if matches!(stamp.mode, Mode::Warp | Mode::Push) {
            return self.offset_stamp(&plan);
        }
        let mut visited = 0;
        for (face, rect) in plan.rects.into_iter().enumerate() {
            if let Some(rect) = rect {
                visited += match plan.stamp.mode {
                    Mode::Weather => self.weather_face(face, rect, &plan.stamp),
                    _ => self.stamp_face(face, rect, &plan.stamp, plan.reach),
                };
                self.mark_dirty(face, rect);
            }
        }
        visited
    }

    fn stamp_face(&mut self, face: usize, rect: TexelRect, stamp: &Stamp, reach: usize) -> usize {
        let (t, base) = (self.tile, self.base);
        let falloff = Falloff::new(face, stamp);
        // The smooth mode reads neighbors, so it needs the values from before.
        // Near a face edge, the neighbors are on the next face.
        let (bx, by) = (rect.x0 as i64 - reach as i64, rect.y0 as i64 - reach as i64);
        let (bw, bh) = (rect.x1 - rect.x0 + 2 * reach, rect.y1 - rect.y0 + 2 * reach);
        let before = if stamp.mode == Mode::Smooth {
            self.read_past_edges(face, bx, by, bw, bh)
        } else {
            Vec::new()
        };
        let old_at = |x: usize, y: usize| f64::from(before[y * bw + x]);
        let channels = self.channels.clone();
        let window = self.window.clone();
        let sea = f64::from(meters_to_level(SEA_LEVEL));
        let channel_size = channels.as_ref().map_or(0.0, |c| c.size() as f64);
        let n = self.n as f64;
        let cut = ChannelCut {
            shape: stamp.channel,
            depth: stamp.strength,
            texel: FRAC_PI_2 / n,
        };
        // The level of a texel after the cut of the channels. The window
        // comes first. With no channel map, no river is near.
        let carve = |x: usize, y: usize, fu: f64, fv: f64, old: f64, rate: f64| {
            let p = face_dir(face, fu, fv);
            let in_window = window.as_ref().and_then(|channels| {
                let window = channels.window();
                let (u, v) = if window.face == face {
                    window.channel_of_texel(x as i64, y as i64)
                } else {
                    window.channel_of_dir(p)?
                };
                if !window.covers(u, v) {
                    return None;
                }
                let lines = channels.lines();
                let bin = lines.window_bin(0.5 * (u + 0.5), 0.5 * (v + 0.5))?;
                Some(lines.lowered(bin, p, old, rate, &cut))
            });
            in_window.or_else(|| {
                let lines = channels.as_ref()?.lines();
                let bin = lines.texel_bin(face, x, y, self.n)?;
                Some(lines.lowered(bin, p, old, rate, &cut))
            })
        };
        let octaves = noise_octaves(stamp.scale, self.n);
        let relief_span = f64::from(meters_to_level(SEA_LEVEL + NOISE_RELIEF_M)) - sea;
        // The ridge height at a texel. The window comes first. With no
        // channel map, no river is near, so the ridge has its full height.
        let ridge = |x: usize, y: usize, fu: f64, fv: f64| {
            // The place of the texel in the window. On the face of the
            // window, the place is exact.
            let in_window = window.as_ref().and_then(|channels| {
                let window = channels.window();
                let (u, v) = if window.face == face {
                    window.channel_of_texel(x as i64, y as i64)
                } else {
                    window.channel_of_dir(face_dir(face, fu, fv))?
                };
                channels.ridge(u, v)
            });
            in_window.or_else(|| {
                // The place of the texel in the channel map.
                let at = |x: usize| (x as f64 + 0.5) * channel_size / n - 0.5;
                match &channels {
                    Some(channels) => Some(channels.ridge(face, at(x), at(y))),
                    None => Some(1.0),
                }
            })
        };
        // The fill level at a texel. The window comes first.
        let fill = |x: usize, y: usize, fu: f64, fv: f64| {
            let in_window = window.as_ref().and_then(|channels| {
                let window = channels.window();
                let (u, v) = if window.face == face {
                    window.channel_of_texel(x as i64, y as i64)
                } else {
                    window.channel_of_dir(face_dir(face, fu, fv))?
                };
                channels.fill_level(u, v)
            });
            in_window.or_else(|| {
                let at = |x: usize| (x as f64 + 0.5) * channel_size / n - 0.5;
                Some(channels.as_ref()?.fill_level(face, at(x), at(y)))
            })
        };
        for ty in rect.y0 / t..=(rect.y1 - 1) / t {
            for tx in rect.x0 / t..=(rect.x1 - 1) / t {
                let index = (face * self.per_side + ty) * self.per_side + tx;
                let tile = &mut self.tiles[index];
                let mut texels = Texels::Shared(tile.as_deref());
                for y in rect.y0.max(ty * t)..rect.y1.min((ty + 1) * t) {
                    let fv = self.flat[y];
                    for x in rect.x0.max(tx * t)..rect.x1.min((tx + 1) * t) {
                        let fu = self.flat[x];
                        let Some(amount) = falloff.amount(fu, fv) else {
                            continue;
                        };
                        let offset = (y - ty * t) * t + (x - tx * t);
                        let old_level = texels.get(offset, base);
                        let old = f64::from(old_level);
                        let new = match stamp.mode {
                            Mode::Raise | Mode::Uplift => old + amount * stamp.strength,
                            Mode::Lower => old - amount * stamp.strength,
                            Mode::Flatten => old + (f64::from(stamp.level) - old) * amount.min(1.0),
                            Mode::Erode | Mode::OceanRidge | Mode::Range => continue,
                            Mode::Noise => {
                                let p = face_dir(face, fu, fv);
                                let value = fractal(p, stamp, octaves);
                                let height = ((old - sea) / relief_span).clamp(0.0, 1.0);
                                let relief = stamp.relief.clamp(0.0, 1.0);
                                let gain = 1.0 - relief + relief * height;
                                old + amount * stamp.strength * value * gain
                            }
                            Mode::Carve => {
                                let rate = amount * stamp.strength;
                                let Some(new) = carve(x, y, fu, fv, old, rate) else {
                                    continue;
                                };
                                new.round().max(old.min(sea))
                            }
                            Mode::Ridge => {
                                if old <= sea {
                                    continue;
                                }
                                let Some(ridge) = ridge(x, y, fu, fv) else {
                                    continue;
                                };
                                old + amount * stamp.strength * ridge
                            }
                            Mode::Deposit => {
                                if old < sea {
                                    continue;
                                }
                                let Some(level) = fill(x, y, fu, fv) else {
                                    continue;
                                };
                                old + (level - old).max(0.0) * amount.min(1.0)
                            }
                            Mode::Weather => unreachable!("`stamp` sends it to `weather_face`"),
                            Mode::Warp | Mode::Push => {
                                unreachable!("`stamp` sends it to `offset_stamp`")
                            }
                            Mode::Smooth => {
                                // The place of the texel in `before`.
                                let (x, y) = (x - rect.x0 + reach, y - rect.y0 + reach);
                                let (xa, xb) = (x - reach, x + reach);
                                let (ya, yb) = (y - reach, y + reach);
                                let mean = (old_at(xa, ya)
                                    + old_at(x, ya)
                                    + old_at(xb, ya)
                                    + old_at(xa, y)
                                    + old_at(xb, y)
                                    + old_at(xa, yb)
                                    + old_at(x, yb)
                                    + old_at(xb, yb)
                                    + old)
                                    / 9.0;
                                old + (mean - old) * amount.min(1.0)
                            }
                        };
                        let new = new.round().clamp(0.0, 65535.0);
                        let new = stamp.limits.apply(old, new) as u16;
                        if new == old_level {
                            continue;
                        }
                        if let Texels::Shared(_) = texels {
                            save_tile(&mut self.stroke, &mut self.changed, index, tile);
                            let tile = tile.get_or_insert_with(|| vec![base; t * t].into());
                            texels = Texels::Owned(Arc::make_mut(tile));
                        }
                        texels.set(offset, new);
                    }
                }
            }
        }
        (rect.x1 - rect.x0) * (rect.y1 - rect.y0)
    }

    /// Lowers the texels by the drops of one step of the erode brush. A texel
    /// takes the drops of the 4 cells around it, in the ratio of its
    /// distances to their centers. The lowest level is 0. A texel does not go
    /// below the floor of the step, and a texel outside the limits of the
    /// step stays. Returns the number of texels that changed.
    pub fn erode(&mut self, step: &ErodeStep) -> usize {
        let (t, base, n) = (self.tile, self.base, self.n);
        let limits = step.limits();
        let mut changed = 0;
        for (face, rect) in step.rects().into_iter().enumerate() {
            let Some(rect) = rect else { continue };
            assert!(rect.x1 <= n && rect.y1 <= n, "the face size");
            let before = changed;
            for ty in rect.y0 / t..=(rect.y1 - 1) / t {
                for tx in rect.x0 / t..=(rect.x1 - 1) / t {
                    let index = (face * self.per_side + ty) * self.per_side + tx;
                    let tile = &mut self.tiles[index];
                    let mut texels = Texels::Shared(tile.as_deref());
                    for y in rect.y0.max(ty * t)..rect.y1.min((ty + 1) * t) {
                        for x in rect.x0.max(tx * t)..rect.x1.min((tx + 1) * t) {
                            let drop = step.drop_at(n, face, x, y).round();
                            if drop <= 0.0 {
                                continue;
                            }
                            let offset = (y - ty * t) * t + (x - tx * t);
                            let old = texels.get(offset, base);
                            if !limits.holds(f64::from(old)) {
                                continue;
                            }
                            let new = (f64::from(old) - drop).max(f64::from(limits.floor)) as u16;
                            if new == old {
                                continue;
                            }
                            if let Texels::Shared(_) = texels {
                                save_tile(&mut self.stroke, &mut self.changed, index, tile);
                                let tile = tile.get_or_insert_with(|| vec![base; t * t].into());
                                texels = Texels::Owned(Arc::make_mut(tile));
                            }
                            texels.set(offset, new);
                            changed += 1;
                        }
                    }
                }
            }
            if changed > before {
                self.mark_dirty(face, rect);
            }
        }
        changed
    }

    /// Applies one weather stamp to one face. Each pair of texels that are
    /// side by side in the rectangle moves a whole number of levels from the
    /// higher texel to the lower one. All pairs read the levels from before
    /// the stamp. No ground moves across a face edge. A pair moves a quarter
    /// at most of the room from the higher texel down to the floor, and of
    /// the room from the lower texel up to the ceiling. A pair with a texel
    /// outside the limits does not move.
    fn weather_face(&mut self, face: usize, rect: TexelRect, stamp: &Stamp) -> usize {
        let falloff = Falloff::new(face, stamp);
        let (w, h) = (rect.x1 - rect.x0, rect.y1 - rect.y0);
        // The effect at each texel, with a margin of one texel of zeros.
        let row = w + 2;
        let mut amounts = vec![0.0; row * (h + 2)];
        for y in 0..h {
            let fv = self.flat[rect.y0 + y];
            for x in 0..w {
                let amount = falloff.amount(self.flat[rect.x0 + x], fv);
                amounts[(y + 1) * row + x + 1] = amount.unwrap_or(0.0);
            }
        }
        let mut old = Vec::with_capacity(w * h);
        self.read_rect(face, rect, &mut old);
        for y in 0..h {
            for x in 0..w {
                let at = (y + 1) * row + x + 1;
                let level = f64::from(old[y * w + x]);
                let mut new = level;
                // The neighbor in `amounts`, and its distance in `old`.
                let across = w as isize;
                let steps = [
                    (at - 1, -1),
                    (at + 1, 1),
                    (at - row, -across),
                    (at + row, across),
                ];
                for (near, step) in steps {
                    // A pair with a texel outside the rectangle has no weight.
                    let weight = amounts[at].min(amounts[near]).min(1.0);
                    if weight <= 0.0 {
                        continue;
                    }
                    let other = f64::from(old[(y * w + x).wrapping_add_signed(step)]);
                    let drop = level - other;
                    let over = drop.abs() - stamp.talus;
                    if over > 0.0 {
                        let room = stamp.limits.pair_room(level.max(other), level.min(other));
                        new -= drop.signum() * (weight * over * 0.125).floor().min(room);
                    }
                }
                // A texel loses at most half of each drop, so the new level
                // is between the old levels of the pair texels.
                self.set(face, rect.x0 + x, rect.y0 + y, new as u16);
            }
        }
        w * h
    }

    /// Applies one warp stamp or push stamp. Each face reads the levels from
    /// before the stamp. Returns the number of texels that it visited.
    fn offset_stamp(&mut self, plan: &StampPlan) -> usize {
        let faces = plan.rects.iter().enumerate();
        let faces = faces.filter_map(|(face, rect)| Some((face, (*rect)?)));
        let new: Vec<_> = faces
            .map(|(face, rect)| (face, rect, self.offset_face(face, rect, &plan.stamp)))
            .collect();
        let mut visited = 0;
        for (face, rect, levels) in new {
            let w = rect.x1 - rect.x0;
            for (i, level) in levels.into_iter().enumerate() {
                self.set(face, rect.x0 + i % w, rect.y0 + i / w, level);
            }
            visited += w * (rect.y1 - rect.y0);
        }
        visited
    }

    /// The levels of a rectangle of one face after one warp stamp or push
    /// stamp, row by row.
    fn offset_face(&self, face: usize, rect: TexelRect, stamp: &Stamp) -> Vec<u16> {
        let mut levels = Vec::with_capacity((rect.x1 - rect.x0) * (rect.y1 - rect.y0));
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                let p = self.texel_dir(face, x, y);
                let offset = match stamp.mode {
                    Mode::Push => push_offset(stamp, self.n, p),
                    _ => warp_offset(stamp, self.n, p),
                };
                let old = self.texel(face, x, y);
                levels.push(if offset == [0.0; 3] {
                    old
                } else {
                    let new = self.level_between(normalize(add(p, offset)), true).round();
                    stamp.limits.apply(f64::from(old), new) as u16
                });
            }
        }
        levels
    }

    /// The level of a texel from before the stroke. With no stroke, it is the
    /// level of the texel.
    fn level_before(&self, face: usize, x: usize, y: usize) -> u16 {
        let saved = self.stroke.as_ref();
        match saved.and_then(|saved| Some((saved, saved.tiles.get(&self.tile_index(face, x, y))?)))
        {
            Some((_, Some(tile))) => tile[self.offset(x, y)],
            Some((saved, None)) => saved.base,
            None => self.texel(face, x, y),
        }
    }

    /// Lays the ocean ridge of a plan on the ground from before the stroke.
    /// A texel near the new part of the line gets the level of the ridge, and
    /// a texel that the ridge does not reach gets its level from before the
    /// stroke. Thus a second call with the same plan changes nothing. Call
    /// this in a stroke. Returns the number of texels that changed.
    pub fn ridge(&mut self, plan: &RidgePlan) -> usize {
        self.lay(&plan.line, |start, at, p| {
            plan.level(start, at, plan.noise(p))
        })
    }

    /// Lays the mountain range of a plan on the ground from before the
    /// stroke, as `ridge` lays an ocean ridge. Returns the number of texels
    /// that changed.
    pub fn range(&mut self, plan: &RangePlan) -> usize {
        self.lay(&plan.line, |start, at, _| plan.level(start, at))
    }

    /// Lays a line brush on the ground from before the stroke. A texel near
    /// the new part of the line gets `level` of its level from before the
    /// stroke, its place on the line, and its direction. A texel that the
    /// brush does not reach gets its level from before the stroke. Returns
    /// the number of texels that changed.
    fn lay(&mut self, line: &LinePlan, level: impl Fn(u16, LinePoint, V3) -> u16) -> usize {
        assert_eq!(line.face_size, self.n, "the face size of the plan");
        let mut changed = 0;
        for (face, rect) in line.rects.into_iter().enumerate() {
            let Some(rect) = rect else { continue };
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    let p = self.texel_dir(face, x, y);
                    let place = match line.place(p) {
                        LinePlace::Far => continue,
                        LinePlace::Off => None,
                        LinePlace::On(place) => Some(place),
                    };
                    let start = self.level_before(face, x, y);
                    let new = place.map_or(start, |place| level(start, place, p));
                    if new != self.texel(face, x, y) {
                        self.set(face, x, y, new);
                        changed += 1;
                    }
                }
            }
        }
        changed
    }

    /// The level at a direction, from the 4 texels around it on its face in
    /// the ratio of the distances to their centers. If `past` is true, the
    /// texel grid continues past the face edges, and the level of a texel
    /// there comes from this function without `past`. If not, the texels at
    /// the face edge continue.
    fn level_between(&self, d: V3, past: bool) -> f64 {
        let (face, a, b) = face_of(d);
        let at = |a: f64| (warp(a) + 1.0) * 0.5 * self.n as f64 - 0.5;
        self.level_on_face(face, at(a), at(b), past)
    }

    /// `level_between` for a place on a face in texels. The center of texel
    /// `i` is at `i`.
    pub(crate) fn level_on_face(&self, face: usize, x: f64, y: f64, past: bool) -> f64 {
        let n = self.n as i64;
        let (x0, y0) = (x.floor(), y.floor());
        let (tx, ty) = (x - x0, y - y0);
        let (xi, yi) = (x0 as i64, y0 as i64);
        let mix = |l: [f64; 4]| {
            // With 4 equal levels, each step gives that level.
            let top = l[0] + (l[1] - l[0]) * tx;
            let bottom = l[2] + (l[3] - l[2]) * tx;
            top + (bottom - top) * ty
        };
        if xi >= 0 && yi >= 0 && xi + 1 < n && yi + 1 < n {
            let (x, y, t) = (xi as usize, yi as usize, self.tile);
            let (column, row) = (x / t, y / t);
            if (x + 1) / t == column && (y + 1) / t == row {
                // The 4 texels are in one tile.
                let index = (face * self.per_side + row) * self.per_side + column;
                let Some(tile) = &self.tiles[index] else {
                    return f64::from(self.base);
                };
                let at = (y - row * t) * t + (x - column * t);
                let level = |at: usize| f64::from(tile[at]);
                return mix([level(at), level(at + 1), level(at + t), level(at + t + 1)]);
            }
        }
        let level = |i: i64, j: i64| {
            let (x, y) = (xi + i, yi + j);
            let on_face = (x.clamp(0, n - 1), y.clamp(0, n - 1));
            if past && (x, y) != on_face {
                let flat =
                    |i: i64| unwarp(((i as f64 + 0.5) / n as f64 * 2.0 - 1.0).clamp(-1.99, 1.99));
                self.level_between(face_dir(face, flat(x), flat(y)), false)
            } else {
                f64::from(self.texel(face, on_face.0 as usize, on_face.1 as usize))
            }
        };
        mix([level(0, 0), level(1, 0), level(0, 1), level(1, 1)])
    }
}

/// A hash of a lattice point. The shader has the same steps on 32-bit whole
/// numbers, so it gives the same bits.
pub(crate) fn hash(ix: u32, iy: u32, iz: u32, seed: u32) -> u32 {
    let mut h = seed
        ^ 0x9e37_79b9
        ^ ix.wrapping_mul(0x8da6_b343)
        ^ iy.wrapping_mul(0xd816_3841)
        ^ iz.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

/// A noise vector at a point, with each part from -1 to 1. Each lattice
/// point with whole coordinates has a vector from `hash`, and the noise
/// changes smoothly between the 8 lattice points around the point.
pub(crate) fn noise3(p: V3, seed: u32) -> V3 {
    let cell = p.map(f64::floor);
    let [ix, iy, iz] = cell.map(|c| c as i32 as u32);
    let [tx, ty, tz] = [0, 1, 2].map(|k| {
        let t = p[k] - cell[k];
        t * t * (3.0 - 2.0 * t)
    });
    let corner = |dx: u32, dy: u32, dz: u32| {
        let h = hash(
            ix.wrapping_add(dx),
            iy.wrapping_add(dy),
            iz.wrapping_add(dz),
            seed,
        );
        // Three numbers of 10 bits each.
        [h, h >> 10, h >> 20].map(|k| (f64::from(k & 1023) - 511.5) / 511.5)
    };
    let between = |a: V3, b: V3, t: f64| add(a, scale(add(b, scale(a, -1.0)), t));
    let along_x = |dy: u32, dz: u32| between(corner(0, dy, dz), corner(1, dy, dz), tx);
    let along_y = |dz: u32| between(along_x(0, dz), along_x(1, dz), ty);
    between(along_y(0), along_y(1), tz)
}

/// The number of octaves of the noise mode on a face with `n` texels along
/// one side. The smallest octave has features of 2 texel steps or more.
pub(crate) fn noise_octaves(scale: f64, n: usize) -> u32 {
    let step = FRAC_PI_2 / n as f64;
    let count = (scale / (2.0 * step)).log2().floor() as i64 + 1;
    count.clamp(1, i64::from(NOISE_OCTAVES)) as u32
}

/// The fractal noise of a noise stamp at a unit direction, from -1 to 1. It
/// is a sum of `octaves` noises. Each octave has features of half the size
/// and half the weight of the one before it. In the ridged variant, each
/// octave is folded at zero, so its value is high where the noise is near
/// zero. The inverted ridged noise is the ridged noise times -1.
pub(crate) fn fractal(p: V3, stamp: &Stamp, octaves: u32) -> f64 {
    let (mut sum, mut total) = (0.0, 0.0);
    let (mut size, mut weight) = (stamp.scale, 1.0);
    for octave in 0..octaves {
        let seed = stamp.seed ^ octave.wrapping_add(1).wrapping_mul(0x6d2b_79f5);
        let v = noise3(p.map(|c| c / size), seed)[0];
        let v = if stamp.ridged { 1.0 - 2.0 * v.abs() } else { v };
        sum += weight * v;
        total += weight;
        size *= 0.5;
        weight *= 0.5;
    }
    if stamp.ridged && stamp.invert {
        -sum / total
    } else {
        sum / total
    }
}

/// The offset of a warp stamp at a unit direction, on a face with `n` texels
/// along one side. The offset is square to the direction. Its length is the
/// angle in radians, and it is no more than the strength times the effect of
/// the brush. The offset is zero outside the brush and near the brush edge,
/// so the offset place is in the brush.
fn warp_offset(stamp: &Stamp, n: usize, p: V3) -> V3 {
    let cos_angle = dot(stamp.center, p);
    if cos_angle <= stamp.radius.cos() {
        return [0.0; 3];
    }
    let angle = cos_angle.min(1.0).acos();
    let hard = stamp.hardness.clamp(0.0, 0.999);
    let amount = falloff(angle / stamp.radius, hard) * stamp.flow;
    let margin = WARP_MARGIN * FRAC_PI_2 / n as f64;
    let len = (stamp.strength * amount).min(stamp.radius - angle - margin);
    if len <= 0.0 {
        return [0.0; 3];
    }
    let v = noise3(p.map(|c| c / stamp.scale), stamp.seed);
    // The part of the noise vector along the ground.
    let t = add(v, scale(p, -dot(v, p)));
    scale(t, len / length(t).max(1.0))
}

/// The offset of a push stamp at a unit direction, on a face with `n` texels
/// along one side. The offset is square to the direction and points back
/// along the motion. Its length is the angle in radians: the length of the
/// motion times the effect of the brush. The offset is zero outside the
/// brush and near the brush edge, so the offset place is in the brush.
fn push_offset(stamp: &Stamp, n: usize, p: V3) -> V3 {
    let cos_angle = dot(stamp.center, p);
    if cos_angle <= stamp.radius.cos() {
        return [0.0; 3];
    }
    let angle = cos_angle.min(1.0).acos();
    let hard = stamp.hardness.clamp(0.0, 0.999);
    let amount = falloff(angle / stamp.radius, hard) * stamp.flow;
    // The part of the motion along the ground at `p`.
    let t = add(stamp.motion, scale(p, -dot(stamp.motion, p)));
    let full = length(t);
    let margin = WARP_MARGIN * FRAC_PI_2 / n as f64;
    let len = (length(stamp.motion) * amount).min(stamp.radius - angle - margin);
    if len <= 0.0 || full == 0.0 {
        return [0.0; 3];
    }
    scale(t, -len / full)
}

/// The texel index for an equal-angle coordinate, on a face with `n` texels
/// along one side.
fn texel_index(n: usize, s: f64) -> usize {
    (((s + 1.0) * 0.5 * n as f64).floor().max(0.0) as usize).min(n - 1)
}

/// The effect of a brush at `k` radii from its center, from 0 to 1. `hard`
/// is the part of the radius with full effect.
pub(crate) fn falloff(k: f64, hard: f64) -> f64 {
    if k <= hard {
        return 1.0;
    }
    let k = (k - hard) / (1.0 - hard);
    1.0 - k * k * (3.0 - 2.0 * k)
}

/// The texels of one face that a brush circle can touch. The face has `n`
/// texels along one side.
pub(crate) fn brush_rect(n: usize, face: usize, center: V3, radius: f64) -> Option<TexelRect> {
    let axis = face / 2;
    let sign = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
    let (ua, va) = ((axis + 1) % 3, (axis + 2) % 3);
    // A face point is at most 54.74 degrees from the face center.
    if center[axis] * sign < (0.9554 + radius).cos() {
        return None;
    }
    // Two vectors that are square to the center and to each other.
    let other = if center[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let e1 = normalize(cross(center, other));
    let e2 = cross(center, e1);
    let (sr, cr) = radius.sin_cos();
    let (mut a0, mut a1, mut b0, mut b1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    const SAMPLES: usize = 32;
    for i in 0..=SAMPLES {
        // The last sample is the center.
        let p = if i == SAMPLES {
            center
        } else {
            let (s, c) = (i as f64 / SAMPLES as f64 * TAU).sin_cos();
            [
                center[0] * cr + (e1[0] * c + e2[0] * s) * sr,
                center[1] * cr + (e1[1] * c + e2[1] * s) * sr,
                center[2] * cr + (e1[2] * c + e2[2] * s) * sr,
            ]
        };
        let depth = (p[axis] * sign).max(0.02);
        let (a, b) = (p[ua] / depth, p[va] / depth);
        a0 = a0.min(a);
        a1 = a1.max(a);
        b0 = b0.min(b);
        b1 = b1.max(b);
    }
    // The samples cut the corners of the true outline, so add a margin.
    let margin = 0.02 * (a1 - a0).max(b1 - b0);
    let (a0, a1, b0, b1) = (a0 - margin, a1 + margin, b0 - margin, b1 + margin);
    if a0 > 1.0 || a1 < -1.0 || b0 > 1.0 || b1 < -1.0 {
        return None;
    }
    let lo = |a: f64| texel_index(n, warp(a.clamp(-1.0, 1.0))).saturating_sub(1);
    let hi = |a: f64| (texel_index(n, warp(a.clamp(-1.0, 1.0))) + 2).min(n);
    Some(TexelRect {
        x0: lo(a0),
        y0: lo(b0),
        x1: hi(a1),
        y1: hi(b1),
    })
}

/// The texels of one tile in a loop that writes some of them. The tile is
/// `Shared` until the first write, so a tile with no write gets no copy.
enum Texels<'a> {
    /// `None`: the tile has no memory.
    Shared(Option<&'a [u16]>),
    Owned(&'a mut [u16]),
}

impl Texels<'_> {
    fn get(&self, offset: usize, base: u16) -> u16 {
        match self {
            Texels::Shared(tile) => tile.map_or(base, |tile| tile[offset]),
            Texels::Owned(tile) => tile[offset],
        }
    }

    /// Call this only on `Owned`.
    fn set(&mut self, offset: usize, level: u16) {
        if let Texels::Owned(tile) = self {
            tile[offset] = level;
        }
    }
}

/// Call this before a change to a tile. It marks the tile for a save, and
/// at the first change in a stroke it keeps a copy for undo.
fn save_tile(
    stroke: &mut Option<Saved>,
    changed: &mut BTreeSet<usize>,
    index: usize,
    tile: &Option<Tile>,
) {
    changed.insert(index);
    if let Some(saved) = stroke {
        saved.tiles.entry(index).or_insert_with(|| tile.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{ChannelLines, channel_size};
    use crate::flow::tests::equator_valley;
    use crate::flow::{
        CoarseHeights, FlowMap, NONE, OUT, Window, WindowHeights, channel_map, drain, sea,
        window_channels,
    };
    use crate::math::{angle, arc_motion, dir_to_lonlat, lonlat_to_dir};
    use crate::range::{RangeLine, RangeSpec};
    use crate::ridge::{Ridge, RidgeLine};

    fn raise(center: V3, radius: f64) -> Stamp {
        Stamp {
            center,
            radius,
            hardness: 1.0,
            flow: 1.0,
            mode: Mode::Raise,
            level: 0,
            strength: 1000.0,
            talus: 0.0,
            scale: 0.1,
            seed: 1,
            motion: [0.0; 3],
            ridged: false,
            relief: 0.0,
            channel: ChannelShape::default(),
            invert: false,
            limits: Limits::default(),
        }
    }

    #[test]
    fn a_paint_after_a_snapshot_leaves_the_snapshot_and_copies_only_its_tiles() {
        fn shared<T: Send + Sync>() {}
        shared::<Heightmap>();

        let n = 256;
        let mut map = Heightmap::with_tile_size(n, 32, 100);
        let (a, b) = (lonlat_to_dir(0.0, 0.0), lonlat_to_dir(180.0, 0.0));
        map.begin_stroke();
        map.stamp(&raise(a, 0.05));
        map.stamp(&raise(b, 0.05));
        map.end_stroke();
        map.take_reset();
        map.take_dirty();
        map.clear_changes();

        let snapshot = map.snapshot();
        assert!(!snapshot.can_undo() && !snapshot.has_changes());
        assert_eq!(snapshot.dirty, [None; FACES]);
        assert!(!snapshot.reset);
        let same = |x: &Option<Tile>, y: &Option<Tile>| match (x, y) {
            (Some(x), Some(y)) => Arc::ptr_eq(x, y),
            (None, None) => true,
            _ => false,
        };
        assert!(map.allocated_tiles() > 0);
        assert!(
            map.tiles
                .iter()
                .zip(&snapshot.tiles)
                .all(|(x, y)| same(x, y))
        );
        let before: Vec<Option<Vec<u16>>> = (0..map.tiles.len())
            .map(|index| map.tile(index).map(<[u16]>::to_vec))
            .collect();

        // A stroke, a stamp with no stroke, and one texel.
        map.begin_stroke();
        map.stamp(&raise(a, 0.05));
        map.end_stroke();
        map.stamp(&raise(lonlat_to_dir(90.0, 0.0), 0.05));
        map.set(5, 0, 0, 7);
        let painted: Vec<usize> = map.changed().collect();
        assert!(painted.len() > 2);
        for (index, before) in before.iter().enumerate() {
            assert_eq!(snapshot.tile(index), before.as_deref(), "tile {index}");
            let kept = same(&map.tiles[index], &snapshot.tiles[index]);
            assert_eq!(kept, !painted.contains(&index), "tile {index}");
        }
        assert_eq!(snapshot.sample(a), map.sample(b));
        assert!(map.sample(a) > snapshot.sample(a));

        // A paint on the snapshot leaves the first heightmap.
        let mut snapshot = snapshot;
        let level = map.sample(b);
        snapshot.stamp(&raise(b, 0.05));
        assert_eq!(map.sample(b), level);
        assert!(snapshot.sample(b) > level);
    }

    #[test]
    fn solid_angles_sum_to_the_sphere() {
        let map = Heightmap::new(64, 0);
        let mut sum = 0.0;
        for y in 0..64 {
            for x in 0..64 {
                sum += map.texel_solid_angle(x, y);
            }
        }
        let sphere = 4.0 * std::f64::consts::PI;
        assert!((sum * 6.0 - sphere).abs() / sphere < 1e-3, "{}", sum * 6.0);
    }

    /// The ground area that one stamp changes, in steradians. The tiles are
    /// small, so the stamp crosses many tile edges.
    fn painted_area(center: V3, radius: f64) -> f64 {
        let n = 256;
        let mut map = Heightmap::with_tile_size(n, 32, 0);
        map.stamp(&raise(center, radius));
        let mut area = 0.0;
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    if map.get(face, x as i64, y as i64) > 0 {
                        area += map.texel_solid_angle(x, y);
                    }
                }
            }
        }
        area
    }

    /// The design goal: the brush is the same circle at every place on the
    /// sphere, with no stretch at a pole, a face edge, or a cube corner.
    #[test]
    fn brush_covers_the_same_ground_everywhere() {
        let radius: f64 = 0.12;
        let cap = TAU * (1.0 - radius.cos());
        let places = [
            ("equator", lonlat_to_dir(0.0, 0.0)),
            ("north pole", lonlat_to_dir(0.0, 90.0)),
            ("face edge", lonlat_to_dir(45.0, 0.0)),
            ("cube corner", lonlat_to_dir(45.0, 35.264)),
            ("mid latitude", lonlat_to_dir(-73.0, 61.0)),
            ("south", lonlat_to_dir(160.0, -80.0)),
        ];
        for (name, center) in places {
            let area = painted_area(center, radius);
            assert!(
                (area - cap).abs() / cap < 0.02,
                "{name}: painted {area}, cap {cap}"
            );
        }
    }

    /// The texel rectangle must not cut the brush. Compare with a stamp that
    /// tests every texel of every face.
    #[test]
    fn stamp_rect_holds_the_full_brush() {
        let n = 128;
        let map = Heightmap::new(n, 0);
        let radii = [0.01, 0.08, MAX_BRUSH_RADIUS];
        for lat in (-90..=90).step_by(15) {
            for lon in (-180..180).step_by(15) {
                let center = lonlat_to_dir(f64::from(lon) + 3.0, f64::from(lat));
                for radius in radii {
                    for face in 0..FACES {
                        let rect = brush_rect(map.face_size(), face, center, radius);
                        for y in 0..n {
                            for x in 0..n {
                                if angle(map.texel_dir(face, x, y), center) >= radius {
                                    continue;
                                }
                                let inside = rect.is_some_and(|r| {
                                    x >= r.x0 && x < r.x1 && y >= r.y0 && y < r.y1
                                });
                                assert!(inside, "lon {lon} lat {lat} r {radius} face {face}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn large_heightmap_uses_memory_only_for_painted_tiles() {
        let n = 8192;
        let mut map = Heightmap::new(n, 100);
        assert_eq!(map.allocated_tiles(), 0);
        assert_eq!(map.memory_bytes(), 0);
        assert_eq!(map.sample(lonlat_to_dir(20.0, 10.0)), 100);

        // The brush is about 100 texels wide, so it touches 4 tiles at most.
        let center = lonlat_to_dir(20.0, 10.0);
        map.begin_stroke();
        map.stamp(&raise(center, 0.01));
        map.end_stroke();
        assert!(map.sample(center) > 100);
        assert!((1..=4).contains(&map.allocated_tiles()));
        assert!(map.memory_bytes() <= 4 * TILE_SIZE * TILE_SIZE * 2);
        assert!(map.memory_bytes() * 1000 < FACES * n * n * 2);

        // A stamp that changes no texel takes no memory.
        let flat = lonlat_to_dir(-100.0, -40.0);
        let tiles = map.allocated_tiles();
        map.stamp(&Stamp {
            mode: Mode::Smooth,
            ..raise(flat, 0.01)
        });
        assert_eq!(map.allocated_tiles(), tiles);

        assert!(map.undo());
        assert_eq!(map.allocated_tiles(), 0);
    }

    /// A code that is different for each texel of a map with faces of 64.
    fn code(face: usize, x: usize, y: usize) -> u16 {
        (face * 4096 + y * 64 + x) as u16
    }

    /// Each row is one face. The four numbers are the faces past the +u, -u,
    /// +v, and -v edges. This follows from the layout in `cube.rs`.
    const NEXT_FACE: [[usize; 4]; FACES] = [
        [2, 3, 4, 5],
        [2, 3, 4, 5],
        [4, 5, 0, 1],
        [4, 5, 0, 1],
        [0, 1, 2, 3],
        [0, 1, 2, 3],
    ];

    /// The test reads along the middle row or column of each face, where the
    /// texel grids of two faces line up.
    #[test]
    fn read_past_a_face_edge_gets_the_next_face() {
        let n = 64;
        let mut map = Heightmap::with_tile_size(n, 16, 0);
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    map.set(face, x, y, code(face, x, y));
                }
            }
        }
        let (n, mid) = (n as i64, n / 2);
        for (face, &[pu, nu, pv, nv]) in NEXT_FACE.iter().enumerate() {
            for k in 0..16 {
                // The texel `k` steps past the shared edge, on the next face.
                // A positive face meets its neighbors at their far edges.
                let depth = if face % 2 == 0 { 63 - k } else { k };
                let k = k as i64;
                let m = mid as i64;
                // Past a u edge, the depth runs along v of the next face.
                assert_eq!(map.get(face, n + k, m), code(pu, mid, depth));
                assert_eq!(map.get(face, -1 - k, m), code(nu, mid, depth));
                // Past a v edge, the depth runs along u of the next face.
                assert_eq!(map.get(face, m, n + k), code(pv, depth, mid));
                assert_eq!(map.get(face, m, -1 - k), code(nv, depth, mid));
            }
        }
    }

    #[test]
    fn read_rect_joins_tiles() {
        let n = 64;
        let mut map = Heightmap::with_tile_size(n, 16, 7);
        map.set(1, 20, 30, 500);
        let rect = TexelRect {
            x0: 10,
            y0: 28,
            x1: 40,
            y1: 34,
        };
        let mut out = Vec::new();
        map.read_rect(1, rect, &mut out);
        assert_eq!(out.len(), 30 * 6);
        for (i, &level) in out.iter().enumerate() {
            let (x, y) = (10 + i % 30, 28 + i / 30);
            assert_eq!(level, if (x, y) == (20, 30) { 500 } else { 7 });
        }
    }

    /// The mean of the texels of the face in one block, from single reads.
    fn block_mean(map: &Heightmap, face: usize, x: usize, y: usize, factor: usize) -> u16 {
        let n = map.face_size();
        let (mut sum, mut count) = (0u64, 0u64);
        for sy in y * factor..((y + 1) * factor).min(n) {
            for sx in x * factor..((x + 1) * factor).min(n) {
                sum += u64::from(map.get(face, sx as i64, sy as i64));
                count += 1;
            }
        }
        ((sum + count / 2) / count) as u16
    }

    fn reduced(map: &Heightmap, face: usize, rect: TexelRect, factor: usize) -> Vec<u16> {
        let mut out = Vec::new();
        map.read_reduced(face, rect, factor, &mut out);
        out
    }

    #[test]
    fn a_reduced_texel_is_the_mean_of_its_texels() {
        let mut map = Heightmap::with_tile_size(64, 16, 10);
        // The block of the reduced texel at 5, 2 with a factor of 4.
        for (i, level) in [3u16, 1000, 65535, 22].into_iter().enumerate() {
            map.set(2, 20 + i, 9, level);
        }
        let rect = TexelRect {
            x0: 4,
            y0: 2,
            x1: 7,
            y1: 3,
        };
        // The sum is 66,560 plus 12 texels at 10, and 66,680 / 16 is 4167.5.
        assert_eq!(reduced(&map, 2, rect, 4), [10, 4168, 10]);
        // With a factor of 2, a block has 2 of those texels and 2 at 10.
        let rect = TexelRect {
            x0: 10,
            y0: 4,
            x1: 12,
            y1: 5,
        };
        assert_eq!(reduced(&map, 2, rect, 2), [256, 16394]);
    }

    #[test]
    fn a_tile_with_no_memory_reads_as_the_base() {
        let mut map = Heightmap::with_tile_size(64, 16, 777);
        map.set(1, 40, 40, 900);
        let all = TexelRect {
            x0: 0,
            y0: 0,
            x1: 16,
            y1: 16,
        };
        assert_eq!(reduced(&map, 0, all, 4), [777; 256]);
        assert!(map.reduced_is_base(0, all, 4));
        assert!(!map.reduced_is_base(1, all, 4));
        // The tile with memory is at the reduced texels from 8 to 12.
        let near = TexelRect {
            x0: 4,
            y0: 8,
            x1: 8,
            y1: 12,
        };
        assert!(map.reduced_is_base(1, near, 4));
        assert_eq!(reduced(&map, 1, near, 4), [777; 16]);
        let at = TexelRect {
            x0: 7,
            y0: 11,
            x1: 9,
            y1: 12,
        };
        assert!(!map.reduced_is_base(1, at, 4));
        assert_eq!(map.allocated_tiles(), 1);
    }

    #[test]
    fn a_factor_of_1_reads_the_texels() {
        let mut map = Heightmap::with_tile_size(64, 16, 7);
        for i in 0..40 {
            map.set(3, 10 + i, 5 + i, 100 + i as u16);
        }
        let rect = TexelRect {
            x0: 3,
            y0: 2,
            x1: 60,
            y1: 50,
        };
        let mut plain = Vec::new();
        map.read_rect(3, rect, &mut plain);
        assert_eq!(reduced(&map, 3, rect, 1), plain);
        assert_eq!(rect.reduced(1), rect);
    }

    /// A face of 70 has 18 reduced texels along one side with a factor of 4,
    /// and the last one has 2 texels along that side. With a factor of 32, a
    /// block has texels in four tiles.
    #[test]
    fn a_reduced_rect_crosses_tiles_and_the_face_edge() {
        let n = 70;
        let mut map = Heightmap::with_tile_size(n, 16, 50);
        for y in 0..n {
            for x in (y % 3..n).step_by(3) {
                // The tiles of one row and one column keep no memory.
                if x / 16 != 2 && y / 16 != 1 {
                    map.set(4, x, y, (x * 331 + y * 7919) as u16);
                }
            }
        }
        for factor in [2, 4, 16, 32] {
            let size = n.div_ceil(factor);
            let rects = [
                TexelRect {
                    x0: 0,
                    y0: 0,
                    x1: size,
                    y1: size,
                },
                TexelRect {
                    x0: size / 3,
                    y0: size / 2,
                    x1: size,
                    y1: size,
                },
            ];
            for rect in rects {
                let out = reduced(&map, 4, rect, factor);
                let width = rect.x1 - rect.x0;
                assert_eq!(out.len(), width * (rect.y1 - rect.y0));
                for (i, &level) in out.iter().enumerate() {
                    let (x, y) = (rect.x0 + i % width, rect.y0 + i / width);
                    assert_eq!(level, block_mean(&map, 4, x, y, factor), "{factor} {x} {y}");
                }
            }
        }
    }

    #[test]
    fn a_dirty_rect_maps_to_the_reduced_texels_that_cover_it() {
        let rect = TexelRect {
            x0: 9,
            y0: 16,
            x1: 17,
            y1: 24,
        };
        let covering = TexelRect {
            x0: 1,
            y0: 2,
            x1: 3,
            y1: 3,
        };
        assert_eq!(rect.reduced(8), covering);
    }

    /// Face 2 is high and face 0 is low. A smooth stamp on face 0, at the
    /// shared edge, must raise face 0.
    #[test]
    fn smooth_reads_the_next_face() {
        let n = 64;
        let mut map = Heightmap::with_tile_size(n, 16, 0);
        for y in 0..n {
            for x in 0..n {
                map.set(2, x, y, 10000);
            }
        }
        map.stamp(&Stamp {
            mode: Mode::Smooth,
            ..raise(lonlat_to_dir(43.0, 0.0), 0.1)
        });
        assert!(map.get(0, n as i64 - 1, n as i64 / 2) > 0);
    }

    #[test]
    fn undo_puts_back_the_stroke() {
        let mut map = Heightmap::with_tile_size(64, 16, 100);
        let center = lonlat_to_dir(45.0, 0.0);
        map.begin_stroke();
        map.stamp(&Stamp {
            hardness: 0.5,
            strength: 500.0,
            ..raise(center, 0.2)
        });
        assert!(map.end_stroke());
        assert!(map.sample(center) > 100);
        assert!(map.undo());
        assert_eq!(map.allocated_tiles(), 0);
        assert_eq!(map.sample(center), 100);
    }

    #[test]
    fn stamp_plan_lists_the_texels_that_stamp_marks() {
        let mut map = Heightmap::with_tile_size(64, 16, 100);
        let stamp = raise(lonlat_to_dir(45.0, 35.264), 5.0);
        let plan = map.stamp_plan(&stamp);
        assert_eq!(plan.stamp.radius, MAX_BRUSH_RADIUS);
        // A quarter of the radius, in texels of 90 / 64 degrees.
        assert_eq!(plan.reach, 3);
        assert_eq!(plan.rects.iter().flatten().count(), 3);
        map.take_dirty();
        map.stamp(&stamp);
        assert_eq!(map.take_dirty(), plan.rects);
        assert_eq!(map.stamp_plan(&raise(stamp.center, 0.001)).reach, 1);
    }

    #[test]
    fn base_is_the_level_of_a_fill() {
        let mut map = Heightmap::with_tile_size(64, 16, 100);
        assert_eq!(map.base(), 100);
        map.set(0, 1, 1, 5);
        assert_eq!(map.base(), 100);
        map.fill(300);
        assert_eq!(map.base(), 300);
        map.undo();
        assert_eq!(map.base(), 100);
    }

    #[test]
    fn take_reset_is_true_one_time_after_a_new_base() {
        let mut map = Heightmap::with_tile_size(64, 16, 100);
        assert!(map.take_reset());
        assert_eq!(map.take_dirty(), [None; FACES]);
        assert!(!map.take_reset());

        // A stamp and its undo keep the base level.
        map.begin_stroke();
        map.stamp(&raise(lonlat_to_dir(0.0, 0.0), 0.1));
        map.end_stroke();
        map.undo();
        assert!(!map.take_reset());
        assert!(map.take_dirty()[0].is_some());

        map.fill(300);
        assert!(map.take_reset());
        assert!(!map.take_reset());
        // A fill to the same level changes nothing.
        map.fill(300);
        assert!(!map.take_reset());

        map.undo();
        assert!(map.take_reset());
        assert_eq!(map.take_dirty(), [None; FACES]);

        // `take_dirty` does not clear the reset.
        map.fill(500);
        map.take_dirty();
        assert!(map.take_reset());

        map.mark_reset();
        assert!(map.take_reset());
        assert!(!map.take_reset());
    }

    #[test]
    fn allocated_rects_lists_the_tiles_with_memory() {
        let mut map = Heightmap::with_tile_size(40, 16, 0);
        assert_eq!(map.allocated_rects().count(), 0);
        map.set(1, 3, 20, 9);
        map.set(4, 39, 39, 9);
        let rect = |x0, y0, x1, y1| TexelRect { x0, y0, x1, y1 };
        // The last tile of a face stops at the face edge.
        let expected = vec![(1, rect(0, 16, 16, 32)), (4, rect(32, 32, 40, 40))];
        assert_eq!(map.allocated_rects().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn store_rect_writes_texels_and_undo_puts_them_back() {
        let mut map = Heightmap::with_tile_size(64, 16, 100);
        map.set(2, 20, 20, 7);
        map.take_dirty();
        // The rectangle covers parts of 9 tiles. Only one new texel is not
        // equal to the texel in the map.
        let rect = TexelRect {
            x0: 10,
            y0: 12,
            x1: 40,
            y1: 36,
        };
        let mut data = Vec::new();
        map.read_rect(2, rect, &mut data);
        data[(33 - 12) * 30 + (35 - 10)] = 900;
        map.begin_stroke();
        map.store_rect(2, rect, &data);
        assert!(map.end_stroke());
        assert_eq!(map.take_dirty(), [None; FACES]);
        assert_eq!(map.allocated_tiles(), 2);
        assert_eq!(map.get(2, 35, 33), 900);
        assert_eq!(map.get(2, 20, 20), 7);
        let mut back = Vec::new();
        map.read_rect(2, rect, &mut back);
        assert_eq!(back, data);

        assert!(map.undo());
        assert_eq!(map.allocated_tiles(), 1);
        assert_eq!(map.get(2, 35, 33), 100);
        assert_eq!(map.get(2, 20, 20), 7);

        // Equal texels are not a change of the stroke.
        map.begin_stroke();
        map.store_rect(2, rect, &vec![100; 30 * 24][..]);
        assert!(map.end_stroke());
        map.begin_stroke();
        map.store_rect(2, rect, &vec![100; 30 * 24][..]);
        assert!(!map.end_stroke());
    }

    /// A stroke with each mode across a cube corner gives a fixed result. The
    /// sum is a hash of the levels of all texels.
    #[test]
    fn stamp_results_do_not_change() {
        let mut map = Heightmap::with_tile_size(64, 16, 30000);
        for (i, mode) in [Mode::Raise, Mode::Lower, Mode::Flatten, Mode::Smooth]
            .into_iter()
            .enumerate()
        {
            for step in 0..6 {
                map.stamp(&Stamp {
                    hardness: 0.4,
                    flow: 0.7,
                    mode,
                    level: 41000,
                    strength: 900.0,
                    ..raise(
                        lonlat_to_dir(40.0 + 2.0 * step as f64, 30.0 + i as f64),
                        0.5,
                    )
                });
            }
        }
        let mut sum = 0u64;
        for face in 0..FACES {
            for y in 0..64 {
                for x in 0..64 {
                    let level = u64::from(map.get(face, x, y));
                    sum = sum.wrapping_mul(31).wrapping_add(level);
                }
            }
        }
        assert_eq!(sum, 3970950523126258947);
    }

    /// The latitude of the river in `valley`.
    const RIVER_LAT: f64 = 0.18;

    /// Land on face 0 with a valley along the equator. The river in the
    /// valley goes west to the sea at longitude -30. The map has the channel
    /// map of its rivers.
    fn valley() -> Heightmap {
        let mut map = valley_land(256);
        let channels = channel_map(&CoarseHeights::new(&map), 6371.0);
        assert!(channels.has_rivers());
        map.set_channels(Some(Arc::new(channels)));
        map
    }

    /// The land of `valley` with `n` texels along one side of a face, and
    /// with no channel map.
    fn valley_land(n: usize) -> Heightmap {
        let mut map = Heightmap::new(n, meters_to_level(-1000.0));
        let data: Vec<u16> = (0..n * n)
            .map(|i| {
                let (lon, lat) = dir_to_lonlat(map.texel_dir(0, i % n, i / n));
                let across = (lat - RIVER_LAT).abs();
                meters_to_level(if lon.abs() < 30.0 && across < 20.0 {
                    200.0 + 20.0 * (lon + 30.0) + 60.0 * across
                } else {
                    -1000.0
                })
            })
            .collect();
        map.store_rect(0, face_rect(n), &data);
        map
    }

    fn face_rect(n: usize) -> TexelRect {
        TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        }
    }

    /// The levels of face 0, row by row.
    fn face_levels(map: &Heightmap) -> Vec<u16> {
        let mut levels = Vec::new();
        map.read_rect(0, face_rect(map.face_size()), &mut levels);
        levels
    }

    fn carve(lon: f64, lat: f64, radius: f64) -> Stamp {
        Stamp {
            mode: Mode::Carve,
            ..raise(lonlat_to_dir(lon, lat), radius)
        }
    }

    /// The place of the texel at a direction in the result of `face_levels`.
    fn place(map: &Heightmap, lon: f64, lat: f64) -> usize {
        let (face, a, b) = face_of(lonlat_to_dir(lon, lat));
        assert_eq!(face, 0);
        map.index(warp(b)) * map.face_size() + map.index(warp(a))
    }

    #[test]
    fn carve_lowers_the_ground_along_a_river() {
        let mut map = valley();
        let before = face_levels(&map);
        map.stamp(&carve(0.0, RIVER_LAT, 0.05));
        let after = face_levels(&map);
        assert!(after.iter().zip(&before).all(|(new, old)| new <= old));
        let lowered = |lat: f64| {
            let at = place(&map, 0.0, lat);
            before[at] - after[at]
        };
        // The stamp lowers the ground by 1000 steps at most. The ground
        // 2 degrees from the large river is in the brush, on a small river.
        let (river, side) = (lowered(RIVER_LAT), lowered(RIVER_LAT + 2.0));
        assert!(river > 600 && river <= 1000, "{river}");
        assert!(side > 0 && side + 200 < river, "{side} {river}");
    }

    #[test]
    fn carve_does_not_lower_land_below_sea_level() {
        let mut map = valley();
        let sea = meters_to_level(SEA_LEVEL);
        let before = face_levels(&map);
        // The river is about 1300 steps above the sea at this place. The
        // bed of the smallest channel is a quarter of the strength deep.
        for _ in 0..5 {
            map.stamp(&Stamp {
                strength: 8000.0,
                ..carve(-28.0, RIVER_LAT, 0.05)
            });
        }
        let after = face_levels(&map);
        for (new, old) in after.iter().zip(&before) {
            assert_eq!(*new >= sea, *old >= sea);
        }
        let at = place(&map, -28.0, RIVER_LAT);
        assert!(before[at] > sea + 1000);
        assert_eq!(after[at], sea);
    }

    #[test]
    fn carve_leaves_the_sea_and_ground_far_from_a_river() {
        let mut map = valley();
        let sea = meters_to_level(SEA_LEVEL);
        let before = face_levels(&map);
        // The brush covers the mouth of the river and the sea next to it.
        map.stamp(&carve(-30.0, RIVER_LAT, 0.05));
        let after = face_levels(&map);
        assert!(after != before);
        for (new, old) in after.iter().zip(&before) {
            assert!(*old >= sea || new == old);
        }

        // At the top of the slope, too little land drains for a river.
        let far = carve(28.0, 19.0, 0.01);
        let channels = map.channels().expect("the map has channels").clone();
        assert_eq!(channels.at(far.center).1, 0);
        assert!(map.stamp(&far) > 0);
        assert!(face_levels(&map) == after);
    }

    /// `valley` with a channel map of cells that are 4 texels wide, and a
    /// window of `cells` cells of one texel around the river at longitude 0.
    fn valley_with_window(cells: usize) -> Heightmap {
        let mut map = valley();
        let flow = FlowMap::new(&CoarseHeights::from_fn(64, |d| map.sample(d)));
        map.set_channels(Some(Arc::new(flow.channels(6371.0))));
        let window = Window::centered(lonlat_to_dir(0.0, RIVER_LAT), 256, 1, cells);
        let channels = window_channels(&WindowHeights::new(&map, window), &flow, 6371.0);
        assert!(channels.has_rivers());
        map.set_window(Some(Arc::new(channels)));
        map
    }

    /// The distance from a direction to the nearest curve of `lines` that
    /// drains `min_area` or more, and the level and the area of that curve
    /// there.
    fn nearest_curve(lines: &ChannelLines, p: V3, min_area: f64) -> (f64, f64, f64) {
        let large = lines.curves().iter().filter(|c| c.areas[1] >= min_area);
        let (d, [level, area, _]) = large
            .map(|curve| curve.nearest(p))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .expect("the lines have curves");
        (d, level, area)
    }

    /// Half of the largest drained area of the curves near `p`. The curves
    /// that drain this area or more are the main river there.
    fn main_river(lines: &ChannelLines, p: V3) -> f64 {
        let near = lines.curves().iter().filter(|c| c.nearest(p).0 < 0.01);
        0.5 * near.map(|c| c.areas[1]).fold(0.0, f64::max)
    }

    /// A carve stroke along the river of `valley`, from `from` to `to`
    /// degrees of longitude.
    fn carve_along(map: &mut Heightmap, from: f64, to: f64, shape: ChannelShape, strength: f64) {
        let steps = ((to - from) / 0.5).round() as i32;
        for step in 0..=steps {
            map.stamp(&Stamp {
                channel: shape,
                strength,
                ..carve(from + 0.5 * f64::from(step), RIVER_LAT, 0.05)
            });
        }
    }

    /// A shape with no floodplain whose channel is `width` radians wide at
    /// the area of the river at `p`.
    fn shape_at(lines: &ChannelLines, p: V3, width: f64) -> ChannelShape {
        ChannelShape {
            width,
            reference: nearest_curve(lines, p, main_river(lines, p)).2,
            floodplain: 0.0,
        }
    }

    /// The width of the bed of the channel across the river at longitude 0,
    /// in radians, and the drained area there. The bed is the ground below
    /// the floodplain level by more than half of the depth of the channel.
    /// `lines` are the lines that cut the channel there.
    fn bed_width(
        map: &Heightmap,
        before: &[u16],
        lines: &ChannelLines,
        strength: f64,
    ) -> (f64, f64) {
        let n = map.face_size();
        let after = face_levels(map);
        let column = place(map, 0.0, RIVER_LAT) % n;
        let main = main_river(lines, lonlat_to_dir(0.0, RIVER_LAT));
        let near = |y: usize| nearest_curve(lines, map.texel_dir(0, column, y), main);
        let middle = place(map, 0.0, RIVER_LAT) / n;
        let middle = (middle - 20..middle + 20)
            .min_by(|&a, &b| near(a).0.total_cmp(&near(b).0))
            .unwrap();
        let in_bed = |y: usize| {
            let (_, level, area) = near(y);
            let at = y * n + column;
            let half = level - 0.5 * strength * channel_size(area);
            after[at] < before[at] && f64::from(after[at]) <= half
        };
        let up = (middle..n).take_while(|&y| in_bed(y)).count();
        let down = (0..middle).rev().take_while(|&y| in_bed(y)).count();
        assert!(up + down > 2, "{up} {down}");
        ((up + down) as f64 * FRAC_PI_2 / n as f64, near(middle).2)
    }

    #[test]
    fn the_channel_has_one_width_in_km_at_each_cell_size() {
        let strength = 3000.0;
        let width = 0.012;
        let mut results = Vec::new();
        // A global map with cells of 2 texels.
        let mut map = valley_land(1024);
        let channels = channel_map(&CoarseHeights::new(&map), 6371.0);
        let river = lonlat_to_dir(0.0, RIVER_LAT);
        let shape = shape_at(channels.lines(), river, width);
        map.set_channels(Some(Arc::new(channels)));
        let before = face_levels(&map);
        carve_along(&mut map, -3.0, 3.0, shape, strength);
        let lines = map.channels().unwrap().lines().clone();
        results.push((bed_width(&map, &before, &lines, strength), 1024));

        // A global map with cells of 16 texels, and a window with cells of
        // one texel.
        let mut map = valley_land(2048);
        let flow = FlowMap::new(&CoarseHeights::from_fn(128, |d| map.sample(d)));
        map.set_channels(Some(Arc::new(flow.channels(6371.0))));
        let before = face_levels(&map);
        carve_along(&mut map, -3.0, 3.0, shape, strength);
        let lines = map.channels().unwrap().lines().clone();
        results.push((bed_width(&map, &before, &lines, strength), 2048));
        map.store_rect(0, face_rect(2048), &before);
        let window = Window::centered(river, 2048, 1, 512);
        let channels = window_channels(&WindowHeights::new(&map, window), &flow, 6371.0);
        let lines = channels.lines().clone();
        map.set_window(Some(Arc::new(channels)));
        carve_along(&mut map, -3.0, 3.0, shape, strength);
        results.push((bed_width(&map, &before, &lines, strength), 2048));

        for &((measured, area), n) in &results {
            // The bed reaches half a texel past its edge, on each side.
            let texel = FRAC_PI_2 / n as f64;
            let expected = shape.width_at(area) + texel;
            assert!((measured - expected).abs() <= texel, "{results:?}");
            let first = results[0].0.1;
            assert!(area / first > 0.7 && area / first < 1.4, "{results:?}");
        }
    }

    #[test]
    fn the_cut_width_follows_the_channel_width_setting() {
        let strength = 3000.0;
        let mut widths = Vec::new();
        for width in [0.006, 0.012, 0.024] {
            let mut map = valley_land(1024);
            let channels = channel_map(&CoarseHeights::new(&map), 6371.0);
            let shape = shape_at(channels.lines(), lonlat_to_dir(0.0, RIVER_LAT), width);
            map.set_channels(Some(Arc::new(channels)));
            let before = face_levels(&map);
            carve_along(&mut map, -3.0, 3.0, shape, strength);
            let lines = map.channels().unwrap().lines().clone();
            let (measured, area) = bed_width(&map, &before, &lines, strength);
            widths.push((measured, shape.width_at(area)));
        }
        let texel = FRAC_PI_2 / 1024.0;
        for &(measured, expected) in &widths {
            assert!((measured - expected - texel).abs() <= texel, "{widths:?}");
        }
        assert!(widths[2].0 > 3.0 * widths[0].0, "{widths:?}");
    }

    #[test]
    fn carve_with_a_floodplain_flattens_the_ground_beside_a_large_river() {
        let n = 1024;
        let texel = FRAC_PI_2 / n as f64;
        let land = valley_land(n);
        let channels = Arc::new(channel_map(&CoarseHeights::new(&land), 6371.0));
        let river = lonlat_to_dir(0.0, RIVER_LAT);
        let plain = channels.lines().curves().iter().map(|c| c.nearest(river));
        let (_, [_, _, part]) = plain.min_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
        assert!(part > 0.9, "{part}");
        let floodplain = 30.0 * texel;
        let result = |floodplain: f64| {
            let mut map = land.snapshot();
            map.set_channels(Some(channels.clone()));
            let shape = ChannelShape {
                floodplain,
                ..shape_at(channels.lines(), river, 4.0 * texel)
            };
            carve_along(&mut map, -3.0, 3.0, shape, 2000.0);
            (face_levels(&map), shape)
        };
        let before = face_levels(&land);
        let (flat, shape) = result(floodplain);
        let (none, _) = result(0.0);
        let main = main_river(channels.lines(), river);
        let cut = ChannelCut {
            shape,
            depth: 2000.0,
            texel,
        };
        let (mut plain, mut beyond) = (0, 0);
        let places = (0..n * n).filter(|at| {
            let p = land.texel_dir(0, at % n, at / n);
            angle(p, river) < 2f64.to_radians() && dir_to_lonlat(p).0.abs() < 1.0
        });
        for at in places {
            let (column, y) = (at % n, at / n);
            let p = land.texel_dir(0, column, y);
            let (d, level, area) = nearest_curve(channels.lines(), p, main);
            // The texels that the cut of a stream reaches.
            let streams = channels
                .lines()
                .curves()
                .iter()
                .filter(|c| c.areas[1] < main);
            let stream = streams.into_iter().any(|c| cut.past_edge(c, p) <= texel);
            if stream {
                continue;
            }
            let h = 0.5 * shape.width_at(area);
            if d > h + 2.0 * texel && d < h + 0.5 * floodplain - texel {
                // The ground of the floodplain is at its level, and the
                // ground with no floodplain stays.
                assert!(before[at] > flat[at], "{column} {y}");
                assert!(
                    (f64::from(flat[at]) - level).abs() <= 2.0,
                    "{column} {y} {}",
                    flat[at]
                );
                assert_eq!(none[at], before[at], "{column} {y}");
                plain += 1;
            } else if d > h + 0.5 * floodplain + texel {
                assert_eq!(flat[at], before[at], "{column} {y}");
                beyond += 1;
            }
        }
        assert!(plain > 200 && beyond > 100, "{plain} {beyond}");
    }

    #[test]
    fn carve_on_the_coarse_copy_leaves_the_ground_past_the_floodplain() {
        let n = 2048;
        let mut map = valley_land(n);
        let channels = channel_map(&CoarseHeights::new(&map), 6371.0);
        map.set_channels(Some(Arc::new(channels)));
        let before = face_levels(&map);
        let shape = ChannelShape::default();
        carve_along(&mut map, -10.0, 10.0, shape, 1000.0);
        let after = face_levels(&map);
        let lines = map.channels().unwrap().lines().clone();
        let texel = FRAC_PI_2 / n as f64;
        let cut = ChannelCut {
            shape,
            depth: 1000.0,
            texel,
        };
        let (mut changed, mut kept) = (0, 0);
        for (at, (new, old)) in after.iter().zip(&before).enumerate() {
            let (x, y) = (at % n, at / n);
            let p = map.texel_dir(0, x, y);
            let in_brush =
                (-10..=10).any(|lon| angle(p, lonlat_to_dir(f64::from(lon), RIVER_LAT)) < 0.04);
            if new == old {
                kept += usize::from(in_brush && *old > meters_to_level(SEA_LEVEL));
                continue;
            }
            changed += 1;
            let bin = lines.texel_bin(0, x, y, n).unwrap();
            let past = lines
                .near(bin)
                .map(|curve| cut.past_edge(curve, p))
                .fold(f64::INFINITY, f64::min);
            assert!(past <= texel * 1.001, "{x} {y}: {past} {texel}");
        }
        assert!(changed > 500 && kept > changed, "{changed} {kept}");
    }

    #[test]
    fn after_a_carve_the_flow_follows_the_channels() {
        let n = 1024;
        let texel = FRAC_PI_2 / n as f64;
        let mut map = valley_land(n);
        let channels = channel_map(&CoarseHeights::new(&map), 6371.0);
        let river = lonlat_to_dir(0.0, RIVER_LAT);
        let shape = shape_at(channels.lines(), river, 4.0 * texel);
        map.set_channels(Some(Arc::new(channels)));
        carve_along(&mut map, -8.0, 8.0, shape, 2000.0);
        let lines = map.channels().unwrap().lines().clone();
        let cut = ChannelCut {
            shape,
            depth: 2000.0,
            texel,
        };
        let window = Window::centered(river, n, 1, 256);
        let heights = WindowHeights::new(&map, window);
        let flow = drain(&window, &heights.levels, &[], sea());
        let main = main_river(&lines, river);
        let (mut starts, mut steps) = (0, 0);
        for i in 0..window.cells * window.cells {
            let (cx, cy) = (i % window.cells, i / window.cells);
            let p = window.cell_dir(cx, cy);
            let (d, _, area) = nearest_curve(&lines, p, main);
            if d > 0.25 * shape.width_at(area) {
                continue;
            }
            if dir_to_lonlat(p).0.abs() > 6.0 {
                continue;
            }
            starts += 1;
            let mut at = i;
            loop {
                let p = window.cell_dir(at % window.cells, at / window.cells);
                if dir_to_lonlat(p).0 < -6.0 || heights.levels[at] < sea() {
                    break;
                }
                let past = lines
                    .curves()
                    .iter()
                    .map(|c| cut.past_edge(c, p))
                    .fold(f64::INFINITY, f64::min);
                assert!(past <= texel, "{starts} {steps}: {past} {texel}");
                let to = flow.receiver[at];
                if to == OUT || to == NONE {
                    break;
                }
                at = to as usize;
                steps += 1;
            }
        }
        assert!(starts > 50 && steps > 2000, "{starts} {steps}");
    }

    #[test]
    fn a_channel_across_a_face_edge_has_no_seam() {
        let n = 1024;
        let texel = FRAC_PI_2 / n as f64;
        let mut map = Heightmap::new(n, meters_to_level(-1000.0));
        for face in [0, 2] {
            let data: Vec<u16> = (0..n * n)
                .map(|i| equator_valley(map.texel_dir(face, i % n, i / n)))
                .collect();
            map.store_rect(face, face_rect(n), &data);
        }
        let channels = channel_map(&CoarseHeights::new(&map), 6371.0);
        let edge = lonlat_to_dir(45.0, 0.4);
        let shape = shape_at(channels.lines(), edge, 6.0 * texel);
        map.set_channels(Some(Arc::new(channels)));
        for step in 0..=20 {
            map.stamp(&Stamp {
                channel: shape,
                strength: 2000.0,
                ..carve(40.0 + 0.5 * f64::from(step), 0.4, 0.05)
            });
        }
        // The bed and the middle of the cut at each place along the river.
        // The bed of the river is far below the beds of the streams.
        let profile = |lon: f64| {
            let lats = (0..400).map(|i| -1.6 + 0.01 * f64::from(i));
            let levels: Vec<(f64, u16)> = lats
                .map(|lat| (lat, map.sample(lonlat_to_dir(lon, lat))))
                .collect();
            let bed = levels.iter().map(|&(_, level)| level).min().unwrap();
            let in_bed = levels.iter().filter(|&&(_, level)| level <= bed + 200);
            let (sum, count) = in_bed.fold((0.0, 0.0), |(sum, count), &(lat, _)| {
                (sum + lat, count + 1.0)
            });
            (f64::from(bed), sum / count)
        };
        let mut last = profile(43.0);
        for i in 1..=200 {
            let lon = 43.0 + 0.02 * f64::from(i);
            let next = profile(lon);
            assert!((next.0 - last.0).abs() <= 25.0, "{lon}: {last:?} {next:?}");
            assert!((next.1 - last.1).abs() <= 0.05, "{lon}: {last:?} {next:?}");
            last = next;
        }
    }

    #[test]
    fn carve_outside_the_window_uses_the_global_rivers() {
        let mut with = valley_with_window(48);
        let mut without = valley_with_window(48);
        without.set_window(None);
        // The first stamp is far from the window. The second stamp covers
        // the window and the ground around it.
        for stamp in [carve(-15.0, RIVER_LAT, 0.1), carve(0.0, RIVER_LAT, 0.3)] {
            let before = face_levels(&with);
            assert!(before == face_levels(&without));
            with.stamp(&stamp);
            without.stamp(&stamp);
            let (a, b) = (face_levels(&with), face_levels(&without));
            assert!(b != before);
            let window = with.window().expect("the map has a window").window();
            let mut inside = 0;
            for (i, (a, b)) in a.iter().zip(&b).enumerate() {
                let (u, v) = window.channel_of_texel((i % 256) as i64, (i / 256) as i64);
                if window.covers(u, v) {
                    inside += usize::from(a != b);
                } else {
                    assert_eq!(a, b, "texel {i}");
                }
            }
            assert_eq!(inside > 100, stamp.radius > 0.2, "{inside}");
            with.store_rect(0, face_rect(256), &before);
            without.store_rect(0, face_rect(256), &before);
        }
    }

    #[test]
    fn carve_without_channels_changes_nothing() {
        let mut map = valley();
        map.set_channels(None);
        let before = face_levels(&map);
        assert!(map.stamp(&carve(0.0, RIVER_LAT, 0.05)) > 0);
        assert!(face_levels(&map) == before);
    }

    fn ridge(lon: f64, lat: f64, radius: f64) -> Stamp {
        Stamp {
            mode: Mode::Ridge,
            ..carve(lon, lat, radius)
        }
    }

    #[test]
    fn ridge_raises_the_ground_by_more_at_a_larger_distance_from_a_river() {
        let mut map = valley();
        let before = face_levels(&map);
        let stamp = ridge(0.0, RIVER_LAT, 0.05);
        map.stamp(&stamp);
        let after = face_levels(&map);
        // The pairs are the ridge height and the change of each texel with
        // the full effect of the brush.
        let channels = map.channels().expect("the map has channels").clone();
        let falloff = Falloff::new(0, &stamp);
        let mut pairs = Vec::new();
        for (i, (new, old)) in after.iter().zip(&before).enumerate() {
            let (x, y) = (i % 256, i / 256);
            let amount = falloff.amount(map.flat[x], map.flat[y]);
            if amount.is_none() {
                assert_eq!(new, old);
            }
            if amount != Some(1.0) {
                continue;
            }
            let at = |x: usize| (x as f64 + 0.5) * channels.size() as f64 / 256.0 - 0.5;
            pairs.push((channels.ridge(0, at(x), at(y)), new - old));
        }
        pairs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!(pairs.is_sorted_by_key(|&(_, change)| change));
        // The stamp raises the ground by 1000 steps at most.
        let (near, far) = (pairs[0], pairs[pairs.len() - 1]);
        assert!(near.0 < 0.1 && near.1 < 100, "{near:?}");
        assert!(far.0 > 0.4 && far.1 > 400 && far.1 <= 1000, "{far:?}");
        let on_river = after[place(&map, 0.0, RIVER_LAT)] - before[place(&map, 0.0, RIVER_LAT)];
        assert!(on_river < 150, "{on_river}");
    }

    #[test]
    fn ridge_does_not_change_the_sea_floor() {
        let mut map = valley();
        let sea = meters_to_level(SEA_LEVEL);
        let before = face_levels(&map);
        // The brush covers the mouth of the river and the sea next to it.
        map.stamp(&ridge(-30.0, RIVER_LAT, 0.05));
        let after = face_levels(&map);
        assert!(after != before);
        for (new, old) in after.iter().zip(&before) {
            assert!(new >= old);
            assert!(*old > sea || new == old);
        }
    }

    #[test]
    fn ridge_reads_the_window_where_the_window_covers_the_ground() {
        let mut with = valley_with_window(48);
        let mut without = valley_with_window(48);
        without.set_window(None);
        // The first stamp is far from the window. The second stamp covers
        // the window and the ground around it.
        for stamp in [ridge(-15.0, RIVER_LAT, 0.1), ridge(0.0, RIVER_LAT, 0.3)] {
            let before = face_levels(&with);
            with.stamp(&stamp);
            without.stamp(&stamp);
            let (a, b) = (face_levels(&with), face_levels(&without));
            assert!(b != before);
            let window = with.window().expect("the map has a window").window();
            let mut inside = 0;
            for (i, (a, b)) in a.iter().zip(&b).enumerate() {
                let (u, v) = window.channel_of_texel((i % 256) as i64, (i / 256) as i64);
                if window.covers(u, v) {
                    inside += usize::from(a != b);
                } else {
                    assert_eq!(a, b, "texel {i}");
                }
            }
            assert_eq!(inside > 100, stamp.radius > 0.2, "{inside}");
            with.store_rect(0, face_rect(256), &before);
            without.store_rect(0, face_rect(256), &before);
        }
    }

    #[test]
    fn ridge_without_channels_raises_the_land_as_raise_does() {
        let mut map = valley();
        map.set_channels(None);
        let sea = meters_to_level(SEA_LEVEL);
        let before = face_levels(&map);
        let stamp = ridge(-30.0, RIVER_LAT, 0.05);
        map.stamp(&stamp);
        let after = face_levels(&map);
        let mut raised = valley();
        raised.stamp(&Stamp {
            mode: Mode::Raise,
            ..stamp
        });
        let raised = face_levels(&raised);
        assert!(after != before);
        for ((new, old), raised) in after.iter().zip(&before).zip(&raised) {
            assert_eq!(new, if *old > sea { raised } else { old });
        }
    }

    fn deposit(lon: f64, lat: f64, radius: f64) -> Stamp {
        Stamp {
            mode: Mode::Deposit,
            ..carve(lon, lat, radius)
        }
    }

    /// The radius of the pit of `basin`, in degrees.
    const PIT_RADIUS: f64 = 4.0;

    /// The slope of `valley` with no valley, and a round pit at longitude 0
    /// and latitude 0. The map has the channel map of its rivers.
    fn basin() -> Heightmap {
        let n = 256;
        let mut map = Heightmap::new(n, meters_to_level(-1000.0));
        let data: Vec<u16> = (0..n * n)
            .map(|i| {
                let (lon, lat) = dir_to_lonlat(map.texel_dir(0, i % n, i / n));
                let from_pit = lon.hypot(lat) / PIT_RADIUS;
                meters_to_level(if lon.abs() < 30.0 && lat.abs() < 20.0 {
                    let pit = 400.0 * (1.0 - from_pit * from_pit).max(0.0);
                    200.0 + 20.0 * (lon + 30.0) - pit
                } else {
                    -1000.0
                })
            })
            .collect();
        map.store_rect(0, face_rect(n), &data);
        let channels = channel_map(&CoarseHeights::new(&map), 6371.0);
        map.set_channels(Some(Arc::new(channels)));
        map
    }

    #[test]
    fn deposit_raises_a_pit_toward_its_rim() {
        let mut map = basin();
        let before = face_levels(&map);
        let center = place(&map, 0.0, 0.0);
        // Water leaves the pit at its west side.
        let rim = before[place(&map, -PIT_RADIUS, 0.0)];
        let level = |meters: f64| i32::from(meters_to_level(meters));
        assert!((i32::from(rim) - level(720.0)).abs() < level(20.0) - level(0.0));
        assert!(before[center] < rim - 1000);
        let stamp = Stamp {
            flow: 0.5,
            ..deposit(0.0, 0.0, 0.12)
        };
        map.stamp(&stamp);
        let once = face_levels(&map);
        let half = (i32::from(before[center]) + i32::from(rim)) / 2;
        assert!(
            (i32::from(once[center]) - half).abs() < 20,
            "{}",
            once[center]
        );
        for _ in 0..20 {
            map.stamp(&stamp);
        }
        let after = face_levels(&map);
        for (i, (new, old)) in after.iter().zip(&before).enumerate() {
            assert!(new >= old, "texel {i}");
            let (lon, lat) = dir_to_lonlat(map.texel_dir(0, i % 256, i / 256));
            if lon.hypot(lat) < PIT_RADIUS - 1.0 {
                // The floor of the pit is flat at the level of the rim.
                assert!(
                    *new <= rim && *new >= rim - 20,
                    "texel {i}: {new}, rim {rim}"
                );
            } else if lon.hypot(lat) > PIT_RADIUS + 1.0 {
                assert_eq!(new, old, "texel {i}");
            }
        }
    }

    #[test]
    fn deposit_raises_a_valley_floor_by_more_near_the_river() {
        let mut map = valley();
        let before = face_levels(&map);
        map.stamp(&deposit(0.0, RIVER_LAT, 0.05));
        let after = face_levels(&map);
        let river = place(&map, 0.0, RIVER_LAT);
        let rise = |i: usize| i32::from(after[i]) - i32::from(before[i]);
        // A step of 256 places goes one texel across the valley.
        for side in [river + 256, river - 256] {
            assert!(rise(river) > 100, "{}", rise(river));
            assert!(rise(side) > 0 && rise(side) < rise(river), "{}", rise(side));
            // The floor is flat from the river to the texel next to it. It
            // keeps the fall of one texel along the river.
            assert!(before[side] > before[river] + 100);
            assert!(after[side].abs_diff(after[river]) <= 40, "{}", after[side]);
        }
        for far in [river + 4 * 256, river - 4 * 256] {
            assert_eq!(rise(far), 0);
        }
        // The floor still goes down along the river.
        let along: Vec<u16> = (0..8).map(|k| after[river - k]).collect();
        assert!(along.is_sorted_by(|a, b| a > b), "{along:?}");
    }

    #[test]
    fn deposit_does_not_raise_ground_above_the_level_where_water_leaves() {
        let mut map = valley();
        let before = face_levels(&map);
        let stamp = deposit(0.0, RIVER_LAT, 0.05);
        for _ in 0..4 {
            map.stamp(&stamp);
        }
        let after = face_levels(&map);
        let channels = map.channels().expect("the map has channels").clone();
        let mut raised = 0;
        for (i, (new, old)) in after.iter().zip(&before).enumerate() {
            if new == old {
                continue;
            }
            raised += 1;
            let (x, y) = (i % 256, i / 256);
            let at = |x: usize| (x as f64 + 0.5) * channels.size() as f64 / 256.0 - 0.5;
            assert!(f64::from(*new) <= channels.fill_level(0, at(x), at(y)));
            // The banks are 3 texels from the texel, one at each side of the
            // river.
            assert!(
                *new < before[i - 3 * 256].min(before[i + 3 * 256]),
                "texel {i}"
            );
        }
        assert!(raised > 20, "{raised}");
    }

    #[test]
    fn deposit_does_not_lower_ground_and_does_not_change_the_sea_floor() {
        let sea = meters_to_level(SEA_LEVEL);
        // The brush covers the mouth of the river and the sea next to it.
        for (mut map, lat) in [(valley(), RIVER_LAT), (basin(), 0.0)] {
            let before = face_levels(&map);
            map.stamp(&deposit(-30.0, lat, 0.05));
            map.stamp(&deposit(0.0, lat, 0.2));
            let after = face_levels(&map);
            assert!(after != before);
            for (new, old) in after.iter().zip(&before) {
                assert!(new >= old);
                assert!(*old >= sea || new == old);
            }
        }
    }

    #[test]
    fn deposit_reads_the_window_where_the_window_covers_the_ground() {
        let mut with = valley_with_window(48);
        let mut without = valley_with_window(48);
        without.set_window(None);
        // The first stamp is far from the window. The second stamp covers
        // the window and the ground around it.
        for stamp in [deposit(-15.0, RIVER_LAT, 0.1), deposit(0.0, RIVER_LAT, 0.3)] {
            let before = face_levels(&with);
            with.stamp(&stamp);
            without.stamp(&stamp);
            let (a, b) = (face_levels(&with), face_levels(&without));
            assert!(b != before);
            let window = with.window().expect("the map has a window").window();
            let mut inside = 0;
            for (i, (a, b)) in a.iter().zip(&b).enumerate() {
                let (u, v) = window.channel_of_texel((i % 256) as i64, (i / 256) as i64);
                if window.covers(u, v) {
                    inside += usize::from(a != b);
                } else {
                    assert_eq!(a, b, "texel {i}");
                }
            }
            assert_eq!(inside > 10, stamp.radius > 0.2, "{inside}");
            with.store_rect(0, face_rect(256), &before);
            without.store_rect(0, face_rect(256), &before);
        }
    }

    #[test]
    fn deposit_without_channels_changes_nothing() {
        let mut map = basin();
        map.set_channels(None);
        let before = face_levels(&map);
        assert!(map.stamp(&deposit(0.0, 0.0, 0.12)) > 0);
        assert!(face_levels(&map) == before);
    }

    #[test]
    fn a_ceiling_stops_deposit_below_the_rim() {
        let mut map = basin();
        let center = place(&map, 0.0, 0.0);
        let ceiling = face_levels(&map)[center] + 300;
        let limits = Limits {
            ceiling,
            ..Limits::default()
        };
        map.stamp(&Stamp {
            limits,
            ..deposit(0.0, 0.0, 0.12)
        });
        assert_eq!(face_levels(&map)[center], ceiling);
    }

    #[test]
    fn undo_puts_back_a_fill() {
        let mut map = Heightmap::with_tile_size(64, 16, 100);
        map.set(3, 5, 6, 900);
        map.fill(300);
        assert_eq!(map.allocated_tiles(), 0);
        assert_eq!(map.get(3, 5, 6), 300);
        assert!(map.undo());
        assert_eq!(map.get(3, 5, 6), 900);
        assert_eq!(map.get(3, 6, 6), 100);
    }

    #[test]
    fn redo_puts_back_a_fill() {
        let mut map = Heightmap::with_tile_size(64, 16, 100);
        map.set(3, 5, 6, 900);
        map.fill(300);
        map.undo();
        map.take_reset();
        assert!(map.redo());
        assert!(map.take_reset());
        assert_eq!(map.allocated_tiles(), 0);
        assert_eq!(map.get(3, 5, 6), 300);
        assert!(map.undo());
        assert_eq!(map.get(3, 5, 6), 900);
    }

    /// All texels of a heightmap with 64 texels on a face side.
    fn texels(map: &Heightmap) -> Vec<u16> {
        let all = TexelRect {
            x0: 0,
            y0: 0,
            x1: 64,
            y1: 64,
        };
        let mut out = Vec::new();
        for face in 0..FACES {
            map.read_rect(face, all, &mut out);
        }
        out
    }

    /// One stroke of one stamp.
    fn stroke(map: &mut Heightmap, stamp: Stamp) -> bool {
        map.begin_stroke();
        map.stamp(&stamp);
        map.end_stroke()
    }

    #[test]
    fn an_uplift_stamp_gives_the_levels_of_a_raise_stamp() {
        let center = lonlat_to_dir(44.0, 34.0);
        let mut raised = Heightmap::new(64, 20_000);
        let mut lifted = Heightmap::new(64, 20_000);
        for flow in [1.0, 0.3] {
            let stamp = Stamp {
                flow,
                hardness: 0.3,
                ..raise(center, 0.2)
            };
            assert!(stroke(&mut raised, stamp));
            let stamp = Stamp {
                mode: Mode::Uplift,
                ..stamp
            };
            assert!(stroke(&mut lifted, stamp));
        }
        assert!(texels(&raised) == texels(&lifted));
    }

    #[test]
    fn undo_and_redo_work_for_each_mode() {
        // The place is near a cube corner, so a stroke changes three faces.
        let center = lonlat_to_dir(44.0, 34.0);
        let modes = [
            Mode::Raise,
            Mode::Lower,
            Mode::Smooth,
            Mode::Flatten,
            Mode::Weather,
            Mode::Uplift,
            Mode::Warp,
            Mode::Push,
            Mode::Noise,
        ];
        for mode in modes {
            let mut map = Heightmap::with_tile_size(64, 16, 20_000);
            // A hill gives the smooth mode a slope.
            assert!(stroke(&mut map, raise(center, 0.1)));
            let before = texels(&map);
            let stamp = Stamp {
                mode,
                level: 30_000,
                motion: arc_motion(lonlat_to_dir(40.0, 34.0), center),
                ..raise(center, 0.25)
            };
            assert!(stroke(&mut map, stamp), "{mode:?}");
            let after = texels(&map);
            assert_ne!(before, after, "{mode:?}");
            map.take_dirty();

            assert!(map.undo() && map.can_redo(), "{mode:?}");
            assert!(texels(&map) == before, "{mode:?}: undo");
            assert!(map.take_dirty().iter().flatten().count() >= 3, "{mode:?}");
            assert!(map.redo() && !map.can_redo(), "{mode:?}");
            assert!(texels(&map) == after, "{mode:?}: redo");
            assert!(map.take_dirty().iter().flatten().count() >= 3, "{mode:?}");
            // Two steps go back to the empty heightmap.
            assert!(map.undo() && map.undo() && !map.can_undo(), "{mode:?}");
            assert_eq!(map.allocated_tiles(), 0, "{mode:?}");
            assert!(map.redo() && map.redo() && !map.redo(), "{mode:?}");
            assert!(texels(&map) == after, "{mode:?}: two redo steps");
        }
    }

    #[test]
    fn a_new_stroke_removes_the_redo_steps() {
        let mut map = Heightmap::with_tile_size(64, 16, 0);
        let center = lonlat_to_dir(10.0, 5.0);
        stroke(&mut map, raise(center, 0.1));
        stroke(&mut map, raise(center, 0.1));
        map.undo();
        // A stroke that changes nothing keeps the redo step.
        map.begin_stroke();
        assert!(!map.end_stroke());
        assert!(map.can_redo());
        stroke(&mut map, raise(center, 0.05));
        assert!(!map.can_redo() && !map.redo());
        assert_eq!(
            map.undo_memory_bytes(),
            map.undo.iter().map(Saved::bytes).sum()
        );
    }

    #[test]
    fn the_undo_steps_stay_inside_the_memory_limit() {
        let tile = 16 * 16 * size_of::<u16>();
        let mut map = Heightmap::with_tile_size(64, 16, 0);
        assert_eq!(map.undo_memory_limit(), UNDO_MEMORY_LIMIT);
        let center = lonlat_to_dir(10.0, 5.0);
        // The first stroke gives memory to the tiles of the place.
        stroke(&mut map, raise(center, 0.2));
        let tiles = map.allocated_tiles();
        assert!(tiles >= 2);
        assert_eq!(map.undo_memory_bytes(), 0);
        // The limit holds three steps of this stroke, and not four.
        let limit = 3 * tiles * tile + tile;
        map.set_undo_memory_limit(limit);
        let strokes = 10;
        for i in 0..strokes {
            stroke(&mut map, raise(center, 0.2));
            let bytes = map.undo_memory_bytes();
            assert_eq!(bytes, (i + 1).min(3) * tiles * tile);
            assert!(bytes <= limit);
        }
        let top = texels(&map);

        // The newest steps are the ones that stay.
        let mut undone = 0;
        while map.undo() {
            undone += 1;
            assert!(map.undo_memory_bytes() <= limit);
        }
        assert_eq!(undone, 3);
        assert!(texels(&map) != top);
        let mut redone = 0;
        while map.redo() {
            redone += 1;
            assert!(map.undo_memory_bytes() <= limit);
        }
        assert_eq!(redone, 3);
        assert!(texels(&map) == top);

        // A smaller limit removes steps at once.
        map.set_undo_memory_limit(tiles * tile);
        assert_eq!(map.undo_memory_bytes(), tiles * tile);
        assert!(map.undo() && !map.undo());
    }

    #[test]
    fn a_step_that_is_larger_than_the_limit_is_not_kept() {
        let mut map = Heightmap::with_tile_size(64, 16, 0);
        let center = lonlat_to_dir(10.0, 5.0);
        stroke(&mut map, raise(center, 0.2));
        map.set_undo_memory_limit(16 * 16 * size_of::<u16>());
        // The undo step of the first stroke holds no tile. Its redo step
        // holds all tiles of the stroke.
        assert!(map.can_undo());
        assert!(map.undo());
        assert!(!map.can_redo());
        assert_eq!(map.undo_memory_bytes(), 0);
        stroke(&mut map, raise(center, 0.2));
        assert!(stroke(&mut map, raise(center, 0.2)));
        assert!(!map.can_undo());
        assert_eq!(map.undo_memory_bytes(), 0);
    }

    fn changed(map: &Heightmap) -> Vec<usize> {
        map.changed().collect()
    }

    #[test]
    fn a_stroke_marks_only_its_tiles_for_a_save() {
        let mut map = Heightmap::with_tile_size(64, 16, 100);
        assert!(!map.has_changes());
        map.begin_stroke();
        map.set(0, 3, 20, 7);
        map.set(0, 4, 20, 7);
        map.set(5, 63, 63, 7);
        map.end_stroke();
        // 4 tiles on a side: row 1 of face 0, and the last tile of face 5.
        assert_eq!(changed(&map), [4, 95]);
        assert_eq!(map.tile_key(4), (0, 0, 1));
        assert_eq!(map.tile_key(95), (5, 3, 3));
        assert!(!map.changed_all());
        map.clear_changes();
        assert!(!map.has_changes());

        // A readback of texels that are the same marks nothing.
        let rect = TexelRect {
            x0: 0,
            y0: 0,
            x1: 64,
            y1: 64,
        };
        let mut data = Vec::new();
        map.read_rect(0, rect, &mut data);
        map.store_rect(0, rect, &data);
        assert!(!map.has_changes());
        data[40 * 64 + 40] = 9;
        map.begin_stroke();
        map.store_rect(0, rect, &data);
        assert_eq!(changed(&map), [10]);
        map.clear_changes();

        // A canceled stroke puts the old texels back, so its tile is saved again.
        map.cancel_stroke();
        assert_eq!(changed(&map), [10]);
        assert_eq!(map.tile(10), None);
        map.clear_changes();

        // Undo takes the memory of a tile away, and redo gives it back.
        assert!(map.undo());
        assert_eq!(changed(&map), [4, 95]);
        assert_eq!(map.tile(95), None);
        map.clear_changes();
        assert!(map.redo());
        assert_eq!(changed(&map), [4, 95]);
        assert_eq!(map.tile(95).unwrap()[255], 7);
        assert_eq!(map.allocated().collect::<Vec<_>>(), [4, 95]);
    }

    #[test]
    fn a_stamp_marks_the_tiles_that_it_changes() {
        let mut map = Heightmap::new(1024, 1000);
        let center = face_dir(2, 0.0, 0.0);
        assert!(stroke(&mut map, raise(center, 0.01)));
        let marked = changed(&map);
        assert!(!marked.is_empty() && marked.len() <= 4, "{marked:?}");
        assert_eq!(marked, map.allocated().collect::<Vec<_>>());
    }

    #[test]
    fn a_new_base_level_marks_all_for_a_save() {
        let mut map = Heightmap::with_tile_size(64, 16, 100);
        map.set(1, 0, 0, 7);
        map.clear_changes();
        map.fill(100);
        assert_eq!(changed(&map), [16]);
        assert!(!map.changed_all());
        map.fill(300);
        assert!(map.changed_all());
        map.clear_changes();
        assert!(map.undo());
        assert!(map.changed_all());
    }

    #[test]
    fn a_loaded_tile_has_no_undo_step_and_no_mark() {
        let mut map = Heightmap::with_tile_size(40, 16, 100);
        map.take_reset();
        let data: Vec<u16> = (0..256).collect();
        map.load_tile(3, 2, 1, &data);
        assert!(!map.has_changes() && !map.can_undo());
        assert_eq!(map.tile(map.allocated().next().unwrap()), Some(&data[..]));
        assert_eq!(map.tile_size(), 16);
        assert_eq!(map.get(3, 33, 17), 17);
        // The texels of the tile go to the GPU copy.
        let rect = |x0, y0, x1, y1| TexelRect { x0, y0, x1, y1 };
        assert_eq!(map.take_dirty()[3], Some(rect(32, 16, 40, 32)));
    }

    #[test]
    fn a_heightmap_can_keep_no_undo_steps() {
        let mut map = Heightmap::with_tile_size(40, 16, 100);
        let data: Vec<u16> = (0..256).collect();
        let rect = TexelRect {
            x0: 0,
            y0: 0,
            x1: 16,
            y1: 16,
        };
        map.begin_stroke();
        map.store_rect(0, rect, &data);
        assert!(map.end_stroke() && map.can_undo());
        map.set_undo_steps(false);
        assert!(!map.can_undo());

        // A stroke still ends as a change, and a cancel still works.
        map.clear_changes();
        map.begin_stroke();
        map.store_rect(1, rect, &data);
        assert!(map.end_stroke());
        assert!(map.has_changes() && !map.can_undo() && map.undo_memory_bytes() == 0);
        map.begin_stroke();
        map.store_rect(2, rect, &data);
        map.cancel_stroke();
        assert_eq!(map.get(2, 5, 0), 100);
    }

    #[test]
    fn a_cleared_tile_and_a_loaded_base_have_no_undo_step_and_no_mark() {
        let mut map = Heightmap::with_tile_size(40, 16, 100);
        let data: Vec<u16> = (0..256).collect();
        map.load_tile(3, 2, 1, &data);
        map.take_reset();
        map.take_dirty();
        map.clear_tile(3, 2, 1);
        assert_eq!(map.get(3, 33, 17), 100);
        assert_eq!(map.allocated().count(), 0);
        assert!(map.take_dirty()[3].is_some());
        map.load_base(7);
        assert_eq!((map.base(), map.get(0, 0, 0)), (7, 7));
        assert!(map.take_reset());
        assert!(!map.has_changes() && !map.can_undo());
    }

    fn weather(center: V3, radius: f64, talus: f64) -> Stamp {
        Stamp {
            mode: Mode::Weather,
            talus,
            ..raise(center, radius)
        }
    }

    /// The levels of all faces.
    fn all_levels(map: &Heightmap) -> Vec<u16> {
        let mut levels = Vec::new();
        for face in 0..FACES {
            map.read_rect(face, face_rect(map.face_size()), &mut levels);
        }
        levels
    }

    fn sum(levels: &[u16]) -> u64 {
        levels.iter().map(|&level| u64::from(level)).sum()
    }

    #[test]
    fn weather_moves_ground_to_the_lower_neighbors() {
        let mut map = Heightmap::with_tile_size(64, 16, 1000);
        map.set(0, 32, 32, 9000);
        map.stamp(&weather(map.texel_dir(0, 32, 32), 0.2, 10.0));
        // The drop is 8000 levels, and 10 levels of it stay. Each pair moves
        // one eighth of the rest.
        let moved = (8000 - 10) / 8;
        assert_eq!(map.get(0, 32, 32), 9000 - 4 * moved);
        for (x, y) in [(31, 32), (33, 32), (32, 31), (32, 33)] {
            assert_eq!(map.get(0, x, y), 1000 + moved, "{x} {y}");
        }
        assert_eq!(map.get(0, 33, 33), 1000);
    }

    #[test]
    fn weather_keeps_the_sum_of_the_levels() {
        let mut map = Heightmap::with_tile_size(64, 16, 20_000);
        // Four tiles meet at this texel.
        map.stamp(&Stamp {
            hardness: 0.2,
            strength: 6000.0,
            ..raise(map.texel_dir(0, 32, 32), 0.2)
        });
        let before = all_levels(&map);
        for step in 0..5 {
            map.stamp(&Stamp {
                hardness: 0.3,
                flow: 0.7,
                ..weather(map.texel_dir(0, 30 + step, 33), 0.25, 20.0)
            });
        }
        let after = all_levels(&map);
        assert_eq!(sum(&after), sum(&before));
        // The stamps change texels on each side of the tile edge.
        let changed = |x: usize| (0..64).any(|y| after[y * 64 + x] != before[y * 64 + x]);
        assert!(changed(31) && changed(32));
    }

    #[test]
    fn weather_leaves_a_gentle_slope() {
        let n = 64;
        let mut map = Heightmap::with_tile_size(n, 16, 20_000);
        // Face 0 is a ramp of 30 levels for each texel step.
        let ramp: Vec<u16> = (0..n * n).map(|i| 20_000 + 30 * (i % n) as u16).collect();
        map.store_rect(0, face_rect(n), &ramp);
        let tiles = map.allocated_tiles();
        map.begin_stroke();
        for face in 0..2 {
            let stamp = weather(face_dir(face, 0.0, 0.0), 0.25, 40.0);
            assert!(map.stamp(&stamp) > 0);
        }
        assert!(!map.end_stroke());
        assert!(face_levels(&map) == ramp);
        assert_eq!(map.allocated_tiles(), tiles);
    }

    #[test]
    fn one_angle_gives_one_slope_at_each_face_size() {
        let world = 4000.0;
        for n in [64, 128, 8192] {
            let slope = talus_levels(15.0, n, world);
            assert_eq!(talus_levels(15.0, 2 * n, world) * 2.0, slope);
        }
        // A world with a slope of 20 degrees to the north on face 0. Returns
        // `true` if a weather stamp with the angle changes it.
        let changes = |n: usize, angle: f64| {
            let mut map = Heightmap::new(n, 0);
            let ramp: Vec<u16> = (0..n * n)
                .map(|i| {
                    let (_, lat) = dir_to_lonlat(map.texel_dir(0, i % n, i / n));
                    meters_to_level(20f64.to_radians().tan() * world * lat.to_radians())
                })
                .collect();
            map.store_rect(0, face_rect(n), &ramp);
            map.stamp(&Stamp {
                hardness: 0.5,
                ..weather(lonlat_to_dir(0.0, 0.0), 0.1, talus_levels(angle, n, world))
            });
            face_levels(&map) != ramp
        };
        for n in [64, 128] {
            assert!(changes(n, 15.0), "{n}");
            assert!(!changes(n, 25.0), "{n}");
        }
    }

    /// Face 2 is high and face 0 is low, with a spike on face 0 at the
    /// shared edge.
    #[test]
    fn weather_moves_no_ground_across_a_face_edge() {
        let n = 64;
        let mut map = Heightmap::with_tile_size(n, 16, 0);
        map.store_rect(2, face_rect(n), &vec![10_000; n * n]);
        map.set(0, n - 1, n / 2, 5000);
        let before = all_levels(&map);
        let stamp = weather(lonlat_to_dir(45.0, 0.0), 0.2, 1.0);
        let plan = map.stamp_plan(&stamp);
        assert!(plan.rects[0].is_some() && plan.rects[2].is_some());
        map.stamp(&stamp);
        let after = all_levels(&map);
        let face = |levels: &[u16], face: usize| levels[face * n * n..(face + 1) * n * n].to_vec();
        assert!(map.get(0, n as i64 - 1, n as i64 / 2) < 5000);
        assert_eq!(sum(&face(&after, 0)), sum(&face(&before, 0)));
        assert!(face(&after, 2) == face(&before, 2));
    }

    fn warp_stamp(center: V3, radius: f64) -> Stamp {
        Stamp {
            mode: Mode::Warp,
            hardness: 0.5,
            strength: 0.5 * radius,
            scale: 0.5 * radius,
            seed: 7,
            ..raise(center, radius)
        }
    }

    /// A level that is a smooth function of the direction.
    fn smooth_level(d: V3) -> f64 {
        let hill = 9000.0 * (3.0 * d[0] + 1.0).sin() * (2.0 * d[1]).cos();
        30000.0 + hill + 6000.0 * (4.0 * d[2] + d[0]).sin()
    }

    /// Ground with the levels of `level` on each face.
    fn ground(n: usize, level: impl Fn(usize, usize, usize, V3) -> u16) -> Heightmap {
        let mut map = Heightmap::new(n, 0);
        for face in 0..FACES {
            let levels: Vec<u16> = (0..n * n)
                .map(|i| level(face, i % n, i / n, map.texel_dir(face, i % n, i / n)))
                .collect();
            map.store_rect(face, face_rect(n), &levels);
        }
        map
    }

    /// Ground with no step at a face edge.
    fn smooth_ground(n: usize) -> Heightmap {
        ground(n, |_, _, _, d| smooth_level(d).round() as u16)
    }

    /// Ground with a level for each texel that has no relation to the levels
    /// of the texels around it.
    fn rough_ground(n: usize) -> Heightmap {
        ground(n, |face, x, y, _| {
            (hash(face as u32, x as u32, y as u32, 3) >> 16) as u16
        })
    }

    /// A face center, a face edge, a cube corner, and a pole.
    fn warp_places() -> [(&'static str, V3); 4] {
        [
            ("face center", lonlat_to_dir(1.3, 0.7)),
            ("face edge", lonlat_to_dir(45.0, 0.0)),
            ("cube corner", lonlat_to_dir(45.0, 35.264)),
            ("pole", lonlat_to_dir(0.0, 90.0)),
        ]
    }

    /// The texels of the rectangles of a stamp.
    fn plan_texels(plan: &StampPlan) -> Vec<(usize, usize, usize)> {
        let mut texels = Vec::new();
        for (face, rect) in plan.rects.iter().enumerate() {
            let Some(rect) = rect else { continue };
            for y in rect.y0..rect.y1 {
                texels.extend((rect.x0..rect.x1).map(|x| (face, x, y)));
            }
        }
        texels
    }

    fn difference(a: V3, b: V3) -> f64 {
        length(add(a, scale(b, -1.0)))
    }

    #[test]
    fn hash_and_noise_results_do_not_change() {
        assert_eq!(hash(0, 0, 0, 0), 0x1fce552);
        assert_eq!(hash(1, 2, 3, 4), 0x752b3909);
        assert_eq!(hash(-5i32 as u32, 7, -1i32 as u32, 99), 0xd66cffde);
        let noise = noise3([0.3, -1.6, 2.25], 7);
        let expected = [0.320083100684262, 0.03464712805474082, -0.02267717693059611];
        assert!(difference(noise, expected) < 1e-12, "{noise:?}");
        // At a lattice point, the noise is the vector of that point.
        let h = hash(-2i32 as u32, 5, -7i32 as u32, 7);
        let corner = [h, h >> 10, h >> 20].map(|k| (f64::from(k & 1023) - 511.5) / 511.5);
        assert!(difference(noise3([-2.0, 5.0, -7.0], 7), corner) < 1e-12);
        // The noise has no step at a lattice plane, also at zero.
        for k in 0..3 {
            for plane in [0.0, -3.0, 5.0] {
                let mut p = [0.4, -3.7, 12.1];
                p[k] = plane - 1e-9;
                let below = noise3(p, 7);
                p[k] = plane + 1e-9;
                assert!(difference(noise3(p, 7), below) < 1e-6, "{k} {plane}");
            }
        }
    }

    #[test]
    fn a_warped_level_is_the_ground_at_the_offset_place() {
        let n = 256;
        let step = FRAC_PI_2 / n as f64;
        for (place, center) in warp_places() {
            let mut map = smooth_ground(n);
            let stamp = warp_stamp(center, 0.2);
            let plan = map.stamp_plan(&stamp);
            map.stamp(&stamp);
            let mut moved = 0;
            for (face, x, y) in plan_texels(&plan) {
                let p = map.texel_dir(face, x, y);
                let offset = warp_offset(&plan.stamp, n, p);
                let expected = smooth_level(normalize(add(p, offset)));
                let level = f64::from(map.get(face, x as i64, y as i64));
                // The ground has whole levels, and it bends between the
                // texel centers. The largest error on this ground is 1.3.
                assert!(
                    (level - expected).abs() < 2.0,
                    "{place}: {level} at {face} {x} {y}, and not {expected}"
                );
                moved += usize::from(length(offset) > 2.0 * step);
            }
            assert!(moved > 1000, "{place}: {moved}");
        }
    }

    #[test]
    fn the_warp_offset_follows_the_strength_and_the_scale() {
        let n = 256;
        let map = Heightmap::new(n, 0);
        let center = lonlat_to_dir(44.0, 30.0);
        let stamp = Stamp {
            hardness: 1.0,
            strength: 0.02,
            ..warp_stamp(center, 0.2)
        };
        let plan = map.stamp_plan(&stamp);
        let twice = map.stamp_plan(&Stamp {
            strength: 0.04,
            ..stamp
        });
        let mut largest = 0.0f64;
        for (face, x, y) in plan_texels(&plan) {
            let p = map.texel_dir(face, x, y);
            let offset = warp_offset(&plan.stamp, n, p);
            assert!(dot(offset, p).abs() < 1e-12);
            largest = largest.max(length(offset));
            // The limit near the brush edge does not act here.
            if angle(p, center) < 0.1 {
                let double = warp_offset(&twice.stamp, n, p);
                assert!(difference(double, scale(offset, 2.0)) < 1e-12);
            }
        }
        assert!(largest <= 0.02 + 1e-12 && largest > 0.9 * 0.02, "{largest}");

        // Places on all parts of the sphere.
        let places = (0..20_000).map(|i| (0.37 * f64::from(i), 0.009 * f64::from(i) - 90.0));
        let at = |p: V3| noise3(p.map(|c| c / stamp.scale), stamp.seed);
        // With the full effect of the brush, the mean offset is 0.47 of the
        // strength.
        let along = |p: V3| length(add(at(p), scale(p, -dot(at(p), p)))).min(1.0);
        let offsets = places
            .clone()
            .map(|(lon, lat)| along(lonlat_to_dir(lon, lat)));
        let mean = offsets.sum::<f64>() / 20_000.0;
        assert!((0.42..0.52).contains(&mean), "{mean}");
        // The mean change of the noise between two places that are `apart`
        // radians from each other.
        let change = |apart: f64| {
            let pairs = places.clone().map(|(lon, lat)| {
                let (p, q) = (
                    lonlat_to_dir(lon, lat),
                    lonlat_to_dir(lon, lat + apart.to_degrees()),
                );
                difference(at(p), at(q))
            });
            pairs.sum::<f64>() / 20_000.0
        };
        let (near, far) = (change(stamp.scale / 8.0), change(4.0 * stamp.scale));
        assert!(near < 0.25 * far, "{near} {far}");
    }

    #[test]
    fn the_warp_plan_limits_the_strength_and_the_scale() {
        let map = Heightmap::new(256, 0);
        let stamp = Stamp {
            strength: 5.0,
            scale: 0.0,
            ..warp_stamp(lonlat_to_dir(10.0, 5.0), 0.1)
        };
        let plan = map.stamp_plan(&stamp).stamp;
        assert_eq!(plan.strength, 0.1);
        let texel_steps = plan.scale / (FRAC_PI_2 / 256.0);
        assert!((texel_steps - 2.0).abs() < 1e-6, "{texel_steps}");
        // The other modes keep the strength.
        let raise = Stamp {
            mode: Mode::Raise,
            ..stamp
        };
        assert_eq!(map.stamp_plan(&raise).stamp.strength, 5.0);
    }

    #[test]
    fn the_warp_offset_is_zero_at_the_brush_edge() {
        let n = 256;
        let margin = WARP_MARGIN * FRAC_PI_2 / n as f64;
        for (place, center) in warp_places() {
            let mut map = smooth_ground(n);
            let before = all_levels(&map);
            // The strength is more than the limit near the brush edge.
            let stamp = Stamp {
                strength: 0.2,
                hardness: 0.95,
                ..warp_stamp(center, 0.2)
            };
            let plan = map.stamp_plan(&stamp);
            map.stamp(&stamp);
            let after = all_levels(&map);
            let mut near_edge = 0;
            for (face, x, y) in plan_texels(&plan) {
                let p = map.texel_dir(face, x, y);
                let limit = (0.2 - angle(p, center) - margin).max(0.0);
                let offset = length(warp_offset(&plan.stamp, n, p));
                assert!(offset <= limit + 1e-9, "{place}: {offset} {limit}");
                near_edge += usize::from(limit < 0.02 && offset > 0.0);
                if limit == 0.0 {
                    let i = (face * n + y) * n + x;
                    assert_eq!(after[i], before[i], "{place}: {face} {x} {y}");
                }
            }
            assert!(near_edge > 100, "{place}: {near_edge}");
            assert!(after != before, "{place}");
        }
    }

    #[test]
    fn warp_leaves_flat_ground_flat() {
        let mut map = Heightmap::new(256, 30000);
        map.begin_stroke();
        for (_, center) in warp_places() {
            for radius in [0.06, 0.3] {
                let stamp = Stamp {
                    strength: radius,
                    ..warp_stamp(center, radius)
                };
                assert!(map.stamp(&stamp) > 0);
            }
        }
        assert!(!map.end_stroke());
        assert_eq!(map.allocated_tiles(), 0);
        assert!(!map.has_changes());
    }

    #[test]
    fn a_warped_level_is_between_the_levels_under_the_brush() {
        let n = 128;
        for (place, center) in warp_places() {
            for radius in [0.01, 0.06, 0.3] {
                for hardness in [0.0, 0.5, 0.95] {
                    let name = format!("{place}, radius {radius}, hardness {hardness}");
                    let mut map = rough_ground(n);
                    let before = all_levels(&map);
                    let stamp = Stamp {
                        strength: radius,
                        hardness,
                        ..warp_stamp(center, radius)
                    };
                    let plan = map.stamp_plan(&stamp);
                    // The texels under the brush.
                    let mut under = vec![false; before.len()];
                    for (face, x, y) in plan_texels(&plan) {
                        let falloff = Falloff::new(face, &plan.stamp);
                        let amount = falloff.amount(map.flat[x], map.flat[y]);
                        under[(face * n + y) * n + x] = amount.is_some();
                    }
                    let old = || before.iter().zip(&under).filter(|(_, under)| **under);
                    let lo = old().map(|(level, _)| *level).min();
                    let hi = old().map(|(level, _)| *level).max();
                    map.stamp(&stamp);
                    let after = all_levels(&map);
                    for (i, &level) in after.iter().enumerate() {
                        if under[i] {
                            assert!(lo <= Some(level) && Some(level) <= hi, "{name}");
                        } else {
                            assert_eq!(level, before[i], "{name}");
                        }
                    }
                    // A brush of 3 texel steps has no offset.
                    assert_eq!(after != before, radius > 0.05, "{name}");
                }
            }
        }
    }

    #[test]
    fn the_seed_sets_the_warp() {
        let warped = |seed: u32| {
            let mut map = smooth_ground(64);
            map.stamp(&Stamp {
                seed,
                ..warp_stamp(lonlat_to_dir(44.0, 34.0), 0.25)
            });
            all_levels(&map)
        };
        assert!(warped(7) == warped(7));
        assert!(warped(7) != warped(8));
    }

    /// Each face reads the levels from before the stamp, so the stamp on a
    /// copy of the ground gives the same levels.
    #[test]
    fn a_warp_stamp_reads_no_level_that_it_wrote() {
        let n = 64;
        let mut map = smooth_ground(n);
        let old = smooth_ground(n);
        let stamp = Stamp {
            strength: 0.3,
            ..warp_stamp(lonlat_to_dir(45.0, 35.264), 0.3)
        };
        let plan = map.stamp_plan(&stamp);
        map.stamp(&stamp);
        let mut faces = 0;
        for (face, rect) in plan.rects.into_iter().enumerate() {
            let Some(rect) = rect else { continue };
            let mut new = Vec::new();
            map.read_rect(face, rect, &mut new);
            assert!(new == old.offset_face(face, rect, &plan.stamp), "{face}");
            faces += 1;
        }
        assert_eq!(faces, 3);
    }

    /// Checks the levels along lines of texels that cross the +u edge of an
    /// even face at a right angle, `m` texels along the edge. The jump at
    /// the edge is not much larger than the jumps on each side.
    fn assert_no_seam(map: &Heightmap, face: usize, m: usize) {
        let (n, k) = (map.face_size(), 12);
        let next = 2 * ((face / 2 + 1) % 3);
        let near = (n - k..n).map(|x| (face, x, m));
        let far = (0..k).map(|i| (next, m, n - 1 - i));
        let levels: Vec<i32> = near
            .chain(far)
            .map(|(face, x, y)| i32::from(map.get(face, x as i64, y as i64)))
            .collect();
        let jumps: Vec<i32> = levels.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        let at_edge = jumps[k - 1];
        let inside = jumps.iter().enumerate().filter(|(i, _)| *i != k - 1);
        let inside = inside.map(|(_, jump)| *jump).max().unwrap();
        assert!(
            2 * at_edge <= 3 * inside + 4,
            "face {face}, line {m}: the jump at the edge is {at_edge}, and the largest jump \
             inside is {inside}: {levels:?}"
        );
    }

    #[test]
    fn warp_leaves_no_seam_at_a_face_edge() {
        let n = 256;
        let mut map = smooth_ground(n);
        let before = all_levels(&map);
        // Face 0 and face 2 meet at longitude 45.
        for i in 0..16 {
            map.stamp(&Stamp {
                seed: i,
                ..warp_stamp(lonlat_to_dir(41.0 + 0.5 * f64::from(i), 2.0), 0.15)
            });
        }
        let after = all_levels(&map);
        for m in (118..150).step_by(4) {
            // The texels at the edge, on each face.
            let changed = [(m + 1) * n - 1, (2 * n + n - 1) * n + m].map(|i| after[i] != before[i]);
            assert!(changed[0] && changed[1], "{m}");
            assert_no_seam(&map, 0, m);
        }
    }

    #[test]
    fn warp_leaves_no_seam_at_a_cube_corner() {
        let n = 256;
        let mut map = smooth_ground(n);
        // Faces 0, 2, and 4 meet at this corner.
        let corner = lonlat_to_dir(45.0, 35.264);
        for seed in 0..4 {
            map.stamp(&Stamp {
                seed,
                ..warp_stamp(corner, 0.2)
            });
        }
        for face in [0, 2, 4] {
            for m in [236, 243, 249] {
                assert_no_seam(&map, face, m);
            }
        }
    }

    /// A push stamp at `center` with a motion of a quarter of the radius. The
    /// motion goes east, or along +Y at a pole.
    fn push_stamp(center: V3, radius: f64) -> Stamp {
        let axis = if center[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        Stamp {
            mode: Mode::Push,
            hardness: 0.5,
            motion: scale(normalize(cross(axis, center)), 0.25 * radius),
            ..raise(center, radius)
        }
    }

    /// The stamps of a push stroke from `from` to `to`, a quarter of the
    /// radius apart. The first stamp has no motion.
    fn push_stroke(map: &mut Heightmap, from: V3, to: V3, stamp: Stamp) {
        let parts = (angle(from, to) / (0.25 * stamp.radius)).ceil() as usize;
        let at = |i: usize| {
            let t = i as f64 / parts as f64;
            normalize(add(scale(from, 1.0 - t), scale(to, t)))
        };
        map.begin_stroke();
        for i in 0..=parts {
            let motion = if i == 0 {
                [0.0; 3]
            } else {
                arc_motion(at(i - 1), at(i))
            };
            map.stamp(&Stamp {
                center: at(i),
                motion,
                ..stamp
            });
        }
        map.end_stroke();
    }

    /// Ground at level 20000 with a round hill at a place.
    fn hill_ground(n: usize, top: V3) -> Heightmap {
        ground(n, |_, _, _, d| {
            let k = angle(d, top) / 0.04;
            (20000.0 + 20000.0 * (-k * k).exp()).round() as u16
        })
    }

    /// The middle of the part of the ground above level 20000, as longitude
    /// and latitude.
    fn hill_middle(map: &Heightmap) -> (f64, f64) {
        let n = map.face_size();
        let mut sum = [0.0; 3];
        for (i, level) in all_levels(map).into_iter().enumerate() {
            let (face, x, y) = (i / (n * n), i % n, i / n % n);
            let w = f64::from(level.saturating_sub(20000));
            sum = add(sum, scale(map.texel_dir(face, x, y), w));
        }
        dir_to_lonlat(normalize(sum))
    }

    #[test]
    fn a_pushed_level_is_the_ground_behind_it() {
        let n = 256;
        let step = FRAC_PI_2 / n as f64;
        for (place, center) in warp_places() {
            let mut map = smooth_ground(n);
            let stamp = push_stamp(center, 0.2);
            let plan = map.stamp_plan(&stamp);
            map.stamp(&stamp);
            let mut moved = 0;
            for (face, x, y) in plan_texels(&plan) {
                let p = map.texel_dir(face, x, y);
                let offset = push_offset(&plan.stamp, n, p);
                // The offset points back along the motion.
                assert!(dot(offset, stamp.motion) <= 0.0, "{place}");
                let expected = smooth_level(normalize(add(p, offset)));
                let level = f64::from(map.get(face, x as i64, y as i64));
                assert!(
                    (level - expected).abs() < 2.0,
                    "{place}: {level} at {face} {x} {y}, and not {expected}"
                );
                moved += usize::from(length(offset) > 2.0 * step);
            }
            assert!(moved > 1000, "{place}: {moved}");
        }
    }

    #[test]
    fn the_push_offset_follows_the_motion_and_the_flow() {
        let n = 256;
        let center = lonlat_to_dir(10.0, 5.0);
        let stamp = push_stamp(center, 0.2);
        let len = length(stamp.motion);
        // The hardness is 0.5, so a texel near the center gets the full
        // effect.
        let near = lonlat_to_dir(11.0, 5.0);
        let at = |stamp: &Stamp| length(push_offset(stamp, n, near));
        assert!((at(&stamp) - len).abs() < 1e-3 * len);
        let half = Stamp { flow: 0.5, ..stamp };
        assert!((at(&half) - 0.5 * len).abs() < 1e-3 * len);
        let twice = Stamp {
            motion: scale(stamp.motion, 2.0),
            ..stamp
        };
        assert!((at(&twice) - 2.0 * len).abs() < 2e-3 * len);
        let still = Stamp {
            motion: [0.0; 3],
            ..stamp
        };
        assert_eq!(at(&still), 0.0);
    }

    #[test]
    fn a_push_stroke_moves_a_hill_with_the_pen() {
        let n = 256;
        let middle = |from: f64, to: f64, flow: f64| {
            let mut map = hill_ground(n, lonlat_to_dir(10.0, 5.0));
            let stamp = Stamp {
                flow,
                ..push_stamp(lonlat_to_dir(0.0, 5.0), 0.15)
            };
            push_stroke(
                &mut map,
                lonlat_to_dir(from, 5.0),
                lonlat_to_dir(to, 5.0),
                stamp,
            );
            hill_middle(&map)
        };
        let (east, lat) = middle(6.0, 16.0, 1.0);
        assert!(east > 14.0, "{east}");
        assert!((lat - 5.0).abs() < 0.3, "{lat}");
        // Less flow moves the hill less.
        let (slow, _) = middle(6.0, 16.0, 0.3);
        assert!(slow > 10.5 && slow < east - 1.0, "{slow}");
        // A stroke to the west moves the hill west.
        let (west, _) = middle(14.0, 4.0, 1.0);
        assert!(west < 6.0, "{west}");
    }

    #[test]
    fn the_push_offset_is_zero_at_the_brush_edge() {
        let n = 256;
        let margin = WARP_MARGIN * FRAC_PI_2 / n as f64;
        for (place, center) in warp_places() {
            let mut map = smooth_ground(n);
            let before = all_levels(&map);
            // The motion is more than the limit near the brush edge.
            let stamp = Stamp {
                motion: scale(push_stamp(center, 0.2).motion, 4.0),
                hardness: 0.95,
                ..push_stamp(center, 0.2)
            };
            let plan = map.stamp_plan(&stamp);
            map.stamp(&stamp);
            let after = all_levels(&map);
            let mut near_edge = 0;
            for (face, x, y) in plan_texels(&plan) {
                let p = map.texel_dir(face, x, y);
                let limit = (0.2 - angle(p, center) - margin).max(0.0);
                let offset = length(push_offset(&plan.stamp, n, p));
                assert!(offset <= limit + 1e-9, "{place}: {offset} {limit}");
                near_edge += usize::from(limit < 0.02 && offset > 0.0);
                if limit == 0.0 {
                    let i = (face * n + y) * n + x;
                    assert_eq!(after[i], before[i], "{place}: {face} {x} {y}");
                }
            }
            assert!(near_edge > 100, "{place}: {near_edge}");
            assert!(after != before, "{place}");
        }
    }

    #[test]
    fn push_leaves_flat_ground_flat() {
        let mut map = Heightmap::new(256, 30000);
        map.begin_stroke();
        for (_, center) in warp_places() {
            for radius in [0.06, 0.3] {
                assert!(map.stamp(&push_stamp(center, radius)) > 0);
            }
        }
        assert!(!map.end_stroke());
        assert_eq!(map.allocated_tiles(), 0);
        assert!(!map.has_changes());
    }

    #[test]
    fn a_pushed_level_is_between_the_levels_under_the_brush() {
        let n = 128;
        for (place, center) in warp_places() {
            let mut map = rough_ground(n);
            let before = all_levels(&map);
            let stamp = push_stamp(center, 0.2);
            let plan = map.stamp_plan(&stamp);
            let mut under = vec![false; before.len()];
            for (face, x, y) in plan_texels(&plan) {
                let falloff = Falloff::new(face, &plan.stamp);
                let amount = falloff.amount(map.flat[x], map.flat[y]);
                under[(face * n + y) * n + x] = amount.is_some();
            }
            let old = || before.iter().zip(&under).filter(|(_, under)| **under);
            let lo = old().map(|(level, _)| *level).min();
            let hi = old().map(|(level, _)| *level).max();
            map.stamp(&stamp);
            let after = all_levels(&map);
            for (i, &level) in after.iter().enumerate() {
                if under[i] {
                    assert!(lo <= Some(level) && Some(level) <= hi, "{place}");
                } else {
                    assert_eq!(level, before[i], "{place}");
                }
            }
            assert!(after != before, "{place}");
        }
    }

    #[test]
    fn push_leaves_no_seam_at_a_face_edge() {
        let n = 256;
        let mut map = smooth_ground(n);
        let before = all_levels(&map);
        // Face 0 and face 2 meet at longitude 45.
        let stamp = push_stamp(lonlat_to_dir(0.0, 2.0), 0.15);
        push_stroke(
            &mut map,
            lonlat_to_dir(38.0, 2.0),
            lonlat_to_dir(52.0, 2.0),
            stamp,
        );
        let after = all_levels(&map);
        for m in (118..150).step_by(4) {
            // The texels at the edge, on each face.
            let changed = [(m + 1) * n - 1, (2 * n + n - 1) * n + m].map(|i| after[i] != before[i]);
            assert!(changed[0] && changed[1], "{m}");
            assert_no_seam(&map, 0, m);
        }
    }

    #[test]
    fn push_leaves_no_seam_at_a_cube_corner() {
        let n = 256;
        let mut map = smooth_ground(n);
        // Faces 0, 2, and 4 meet at this corner.
        let stamp = push_stamp(lonlat_to_dir(0.0, 0.0), 0.2);
        push_stroke(
            &mut map,
            lonlat_to_dir(38.0, 30.0),
            lonlat_to_dir(52.0, 40.0),
            stamp,
        );
        for face in [0, 2, 4] {
            for m in [236, 243, 249] {
                assert_no_seam(&map, face, m);
            }
        }
    }

    #[test]
    fn one_undo_step_puts_back_a_push_stroke() {
        let n = 128;
        let mut map = hill_ground(n, lonlat_to_dir(10.0, 5.0));
        let before = all_levels(&map);
        let stamp = push_stamp(lonlat_to_dir(0.0, 5.0), 0.15);
        push_stroke(
            &mut map,
            lonlat_to_dir(6.0, 5.0),
            lonlat_to_dir(16.0, 5.0),
            stamp,
        );
        assert!(all_levels(&map) != before);
        assert!(map.undo());
        assert!(all_levels(&map) == before);
    }

    /// A noise stamp of 8000 levels with a scale of 0.1.
    fn noise_stamp(center: V3, radius: f64) -> Stamp {
        Stamp {
            mode: Mode::Noise,
            hardness: 0.5,
            strength: 8000.0,
            seed: 7,
            ..raise(center, radius)
        }
    }

    /// Ground at a level of `meters` above sea level.
    fn ground_at(n: usize, meters: f64) -> Heightmap {
        Heightmap::new(n, meters_to_level(SEA_LEVEL + meters))
    }

    #[test]
    fn a_noise_stamp_adds_the_noise_times_the_weight_and_the_strength() {
        let n = 256;
        for (place, center) in warp_places() {
            for ridged in [false, true] {
                let mut map = ground_at(n, 1000.0);
                let old = f64::from(map.get(0, 0, 0));
                let stamp = Stamp {
                    ridged,
                    ..noise_stamp(center, 0.2)
                };
                let plan = map.stamp_plan(&stamp);
                let octaves = noise_octaves(plan.stamp.scale, n);
                assert_eq!(octaves, 4);
                map.stamp(&stamp);
                let mut changed = 0;
                for (face, x, y) in plan_texels(&plan) {
                    let falloff = Falloff::new(face, &plan.stamp);
                    let amount = falloff.amount(map.flat[x], map.flat[y]).unwrap_or(0.0);
                    let p = map.texel_dir(face, x, y);
                    let value = fractal(p, &plan.stamp, octaves);
                    assert!((-1.0..=1.0).contains(&value));
                    let expected = (old + amount * 8000.0 * value).round();
                    let level = f64::from(map.get(face, x as i64, y as i64));
                    assert_eq!(level, expected, "{place}: {face} {x} {y}");
                    changed += usize::from(level != old);
                }
                assert!(changed > 1000, "{place}: {changed}");
            }
        }
    }

    #[test]
    fn noise_leaves_no_seam_at_a_face_edge() {
        let n = 256;
        // Face 0 and face 2 meet at longitude 45.
        let mut map = smooth_ground(n);
        stroke(&mut map, noise_stamp(lonlat_to_dir(45.0, 2.0), 0.2));
        for m in (110..150).step_by(4) {
            assert_no_seam(&map, 0, m);
        }
        // Faces 0, 2, and 4 meet at this corner.
        let mut map = smooth_ground(n);
        stroke(&mut map, noise_stamp(lonlat_to_dir(45.0, 35.264), 0.2));
        for face in [0, 2, 4] {
            for m in [236, 243, 249] {
                assert_no_seam(&map, face, m);
            }
        }
    }

    /// The mean change of the noise of `stamp` between places on all parts
    /// of the sphere and places `apart` radians to the east of them, in a
    /// band of latitudes.
    fn noise_change(stamp: &Stamp, octaves: u32, lat: (f64, f64), apart: f64) -> f64 {
        let count = 4000;
        let places = (0..count).map(|i| {
            let t = f64::from(i) / f64::from(count);
            (0.37 * f64::from(i), lat.0 + (lat.1 - lat.0) * t)
        });
        let sum: f64 = places
            .map(|(lon, lat)| {
                let p = lonlat_to_dir(lon, lat);
                let q = lonlat_to_dir(lon + apart.to_degrees() / lat.to_radians().cos(), lat);
                (fractal(p, stamp, octaves) - fractal(q, stamp, octaves)).abs()
            })
            .sum();
        sum / f64::from(count)
    }

    #[test]
    fn a_noise_feature_has_the_same_size_at_each_place() {
        let stamp = noise_stamp(lonlat_to_dir(0.0, 0.0), 0.2);
        for octaves in [1, 4] {
            let at = |lat| noise_change(&stamp, octaves, lat, 0.02);
            let (equator, middle, pole) = (at((-20.0, 20.0)), at((30.0, 50.0)), at((65.0, 85.0)));
            for other in [middle, pole] {
                assert!((other / equator - 1.0).abs() < 0.15, "{equator} {other}");
            }
        }
    }

    #[test]
    fn the_scale_sets_the_size_of_the_largest_noise_feature() {
        let stamp = noise_stamp(lonlat_to_dir(0.0, 0.0), 0.2);
        let wide = Stamp {
            scale: 0.2,
            ..stamp
        };
        let band = (-60.0, 60.0);
        // Features twice as large change as much over twice the distance.
        let narrow = noise_change(&stamp, 1, band, 0.02);
        let large = noise_change(&wide, 1, band, 0.04);
        assert!((large / narrow - 1.0).abs() < 0.15, "{narrow} {large}");
        assert!(noise_change(&wide, 1, band, 0.02) < 0.7 * narrow);
        // A smaller octave is at least 2 texel steps, and an octave is at most
        // the scale.
        let step = FRAC_PI_2 / 1024.0;
        assert_eq!(noise_octaves(4.0 * step, 1024), 2);
        assert_eq!(noise_octaves(4.0 * step - 1e-9, 1024), 1);
        assert_eq!(noise_octaves(step, 1024), 1);
        assert_eq!(noise_octaves(1.0, 1024), NOISE_OCTAVES);
    }

    #[test]
    fn the_ridged_noise_folds_the_soft_noise_at_zero() {
        let stamp = noise_stamp(lonlat_to_dir(0.0, 0.0), 0.2);
        let ridged = Stamp {
            ridged: true,
            ..stamp
        };
        let mut crest = 0.0f64;
        for i in 0..2000 {
            let p = lonlat_to_dir(0.37 * f64::from(i), 0.08 * f64::from(i) - 80.0);
            let soft = fractal(p, &stamp, 1);
            assert!((fractal(p, &ridged, 1) - (1.0 - 2.0 * soft.abs())).abs() < 1e-12);
            crest = crest.max(fractal(p, &ridged, 3));
            // The inverted ridged noise is upside down, and the inverted soft
            // noise does not change.
            let inverted = Stamp {
                invert: true,
                ..ridged
            };
            assert_eq!(fractal(p, &inverted, 3), -fractal(p, &ridged, 3));
            let soft = Stamp {
                invert: true,
                ..stamp
            };
            assert_eq!(fractal(p, &soft, 3), fractal(p, &stamp, 3));
        }
        assert!(crest > 0.9, "{crest}");
    }

    #[test]
    fn the_relief_setting_keeps_low_ground_calm() {
        let n = 128;
        let center = lonlat_to_dir(10.0, 5.0);
        // The largest change of a stamp on ground at a height.
        let change = |meters: f64, relief: f64| {
            let mut map = ground_at(n, meters);
            let before = all_levels(&map);
            map.stamp(&Stamp {
                relief,
                ..noise_stamp(center, 0.2)
            });
            let after = all_levels(&map);
            let pairs = before.iter().zip(&after);
            pairs.map(|(a, b)| a.abs_diff(*b)).max().unwrap()
        };
        let full = change(NOISE_RELIEF_M, 0.0);
        assert!(full > 3000, "{full}");
        // With no relief setting, the height does not matter.
        assert_eq!(change(0.0, 0.0), full);
        assert_eq!(change(-3000.0, 0.0), full);
        // With the full setting, the noise grows with the height above sea
        // level.
        assert_eq!(change(NOISE_RELIEF_M, 1.0), full);
        assert_eq!(change(0.0, 1.0), 0);
        assert_eq!(change(-3000.0, 1.0), 0);
        let half = change(0.5 * NOISE_RELIEF_M, 1.0);
        assert!(half.abs_diff(full / 2) < full / 20, "{half} {full}");
        let some = change(0.0, 0.25);
        assert!(some.abs_diff(3 * full / 4) < full / 20, "{some} {full}");
    }

    #[test]
    fn the_seed_sets_the_noise() {
        let noised = |seed: u32| {
            let mut map = smooth_ground(128);
            map.begin_stroke();
            for lon in [0.0, 3.0, 6.0] {
                map.stamp(&Stamp {
                    seed,
                    ..noise_stamp(lonlat_to_dir(lon, 5.0), 0.15)
                });
            }
            map.end_stroke();
            all_levels(&map)
        };
        assert!(noised(3) == noised(3));
        assert!(noised(3) != noised(4));
    }

    #[test]
    fn an_ocean_ridge_stamp_changes_nothing() {
        let mut map = smooth_ground(64);
        let before = all_levels(&map);
        map.begin_stroke();
        let stamp = Stamp {
            mode: Mode::OceanRidge,
            ..raise(lonlat_to_dir(10.0, 10.0), 0.2)
        };
        assert_eq!(map.stamp(&stamp), 0);
        assert!(!map.end_stroke());
        assert!(all_levels(&map) == before);
    }

    /// Lays a ridge with no crack near the stroke between two places, each
    /// in degrees of longitude and latitude.
    fn lay_ridge(map: &mut Heightmap, from: (f64, f64), to: (f64, f64)) {
        let spec = Ridge {
            radius: 0.1,
            crest: 50000,
            segment: 50.0,
            offset: 0.03,
            spread: 12.5 / 6371.0,
            magma: 0.0,
            world_km: 6371.0,
            seed: 7,
            limits: Limits::default(),
        };
        let mut line = RidgeLine::new(spec, map.face_size());
        map.begin_stroke();
        for i in 0..=20 {
            let k = f64::from(i) / 20.0;
            let (lon, lat) = (from.0 + (to.0 - from.0) * k, from.1 + (to.1 - from.1) * k);
            let before = line.len();
            line.push(lonlat_to_dir(lon, lat), 1.0);
            map.ridge(&line.plan(before));
        }
        map.end_stroke();
    }

    #[test]
    fn a_ridge_leaves_no_seam_at_a_face_edge() {
        let n = 256;
        let mut map = smooth_ground(n);
        let before = all_levels(&map);
        // Face 0 and face 2 meet at longitude 45.
        lay_ridge(&mut map, (37.0, -6.0), (53.0, 10.0));
        let after = all_levels(&map);
        for m in (118..150).step_by(4) {
            // The texels at the edge, on each face.
            let changed = [(m + 1) * n - 1, (2 * n + n - 1) * n + m].map(|i| after[i] != before[i]);
            assert!(changed[0] && changed[1], "{m}");
            assert_no_seam(&map, 0, m);
        }
    }

    #[test]
    fn a_ridge_leaves_no_seam_at_a_cube_corner() {
        let n = 256;
        let mut map = smooth_ground(n);
        // Faces 0, 2, and 4 meet at longitude 45 and latitude 35.264.
        lay_ridge(&mut map, (38.0, 29.0), (52.0, 41.0));
        for face in [0, 2, 4] {
            for m in [236, 243, 249] {
                assert_no_seam(&map, face, m);
            }
        }
    }

    #[test]
    fn a_range_stamp_changes_nothing() {
        let mut map = smooth_ground(64);
        let before = all_levels(&map);
        map.begin_stroke();
        let stamp = Stamp {
            mode: Mode::Range,
            ..raise(lonlat_to_dir(10.0, 10.0), 0.2)
        };
        assert_eq!(map.stamp(&stamp), 0);
        assert!(!map.end_stroke());
        assert!(all_levels(&map) == before);
    }

    /// The line of a range between two places, each in degrees of
    /// longitude and latitude.
    fn range_line(n: usize, from: (f64, f64), to: (f64, f64)) -> RangeLine {
        let spec = RangeSpec {
            radius: 0.1,
            height: 20000.0,
            spacing: 0.04,
            seed: 7,
            limits: Limits::default(),
        };
        let mut line = RangeLine::new(spec, n);
        for i in 0..=20 {
            let k = f64::from(i) / 20.0;
            let (lon, lat) = (from.0 + (to.0 - from.0) * k, from.1 + (to.1 - from.1) * k);
            line.push(lonlat_to_dir(lon, lat), 1.0);
        }
        line
    }

    /// Lays a range near the stroke between two places, one plan for each
    /// point.
    fn lay_range(map: &mut Heightmap, from: (f64, f64), to: (f64, f64)) {
        let spec = range_line(map.face_size(), from, to).spec();
        let mut line = RangeLine::new(spec, map.face_size());
        map.begin_stroke();
        for i in 0..=20 {
            let k = f64::from(i) / 20.0;
            let (lon, lat) = (from.0 + (to.0 - from.0) * k, from.1 + (to.1 - from.1) * k);
            let before = line.len();
            line.push(lonlat_to_dir(lon, lat), 1.0);
            map.range(&line.plan(before));
        }
        map.end_stroke();
    }

    #[test]
    fn a_range_leaves_no_seam_at_a_face_edge() {
        let n = 256;
        let mut map = smooth_ground(n);
        let before = all_levels(&map);
        // Face 0 and face 2 meet at longitude 45.
        lay_range(&mut map, (37.0, -6.0), (53.0, 10.0));
        let after = all_levels(&map);
        for m in (118..150).step_by(4) {
            // The texels at the edge, on each face.
            let changed = [(m + 1) * n - 1, (2 * n + n - 1) * n + m].map(|i| after[i] != before[i]);
            assert!(changed[0] && changed[1], "{m}");
            assert_no_seam(&map, 0, m);
        }
    }

    #[test]
    fn a_range_leaves_no_seam_at_a_cube_corner() {
        let n = 256;
        let mut map = smooth_ground(n);
        // Faces 0, 2, and 4 meet at longitude 45 and latitude 35.264.
        lay_range(&mut map, (38.0, 29.0), (52.0, 41.0));
        for face in [0, 2, 4] {
            for m in [236, 243, 249] {
                assert_no_seam(&map, face, m);
            }
        }
    }

    #[test]
    fn a_range_plan_twice_changes_nothing() {
        let n = 128;
        let mut map = smooth_ground(n);
        let line = range_line(n, (-10.0, 0.0), (10.0, 5.0));
        map.begin_stroke();
        let plan = line.plan(0);
        assert!(map.range(&plan) > 100);
        let once = all_levels(&map);
        assert_eq!(map.range(&plan), 0);
        assert!(all_levels(&map) == once);
        map.end_stroke();
    }

    /// Smooth ground with a different bump on each texel, so each mode moves
    /// each texel under the brush.
    fn bumpy_ground(n: usize) -> Heightmap {
        ground(n, |face, x, y, d| {
            let bump = hash(face as u32, x as u32, y as u32, 3) >> 20;
            (smooth_level(d) + f64::from(bump)).round() as u16
        })
    }

    /// The levels of bumpy ground before and after 3 stamps of a mode at a
    /// cube corner. `limits` gets the level at the brush center and gives
    /// the limits of the stamps.
    fn limited_run(
        mode: Mode,
        level: u16,
        limits: impl Fn(u16) -> Limits,
    ) -> (Limits, Vec<u16>, Vec<u16>) {
        let mut map = bumpy_ground(64);
        let center = lonlat_to_dir(44.0, 34.0);
        let limits = limits(map.sample(center));
        let stamp = match mode {
            Mode::Weather => weather(center, 0.25, 20.0),
            Mode::Warp => warp_stamp(center, 0.25),
            Mode::Push => push_stamp(center, 0.25),
            _ => raise(center, 0.25),
        };
        let stamp = Stamp {
            mode,
            level,
            limits,
            ..stamp
        };
        let before = all_levels(&map);
        for _ in 0..3 {
            map.stamp(&stamp);
        }
        (limits, before, all_levels(&map))
    }

    /// Asserts that each texel outside the limits kept its level, and that
    /// each other texel is still inside them.
    fn assert_inside(limits: Limits, before: &[u16], after: &[u16], name: &str) {
        for (new, old) in after.iter().zip(before) {
            if (limits.floor..=limits.ceiling).contains(old) {
                assert!(*new >= limits.floor, "{name}: {old} {new}");
                assert!(*new <= limits.ceiling, "{name}: {old} {new}");
            } else {
                assert_eq!(new, old, "{name}");
            }
        }
    }

    /// Asserts that the limits of a run of `limited_run` hold and that they
    /// change its result.
    fn assert_limits_bind(mode: Mode, level: u16, limits: impl Fn(u16) -> Limits) {
        let name = format!("{mode:?}");
        let (limits, before, after) = limited_run(mode, level, limits);
        assert_inside(limits, &before, &after, &name);
        assert_ne!(after, before, "{name}");
        let (_, _, free) = limited_run(mode, level, |_| Limits::default());
        assert_ne!(after, free, "{name}");
    }

    #[test]
    fn a_floor_stops_each_mode_that_lowers() {
        let modes = [
            Mode::Lower,
            Mode::Smooth,
            Mode::Flatten,
            Mode::Weather,
            Mode::Warp,
            Mode::Push,
            Mode::Noise,
        ];
        for mode in modes {
            // The flatten mode goes down to level 0.
            assert_limits_bind(mode, 0, |floor| Limits {
                floor,
                ..Limits::default()
            });
        }

        let limits = Limits {
            floor: meters_to_level(SEA_LEVEL) + 500,
            ..Limits::default()
        };
        let mut map = valley();
        let before = face_levels(&map);
        // The river is about 1300 steps above the sea at this place.
        for _ in 0..5 {
            map.stamp(&Stamp {
                limits,
                ..carve(-28.0, RIVER_LAT, 0.05)
            });
        }
        let after = face_levels(&map);
        assert_inside(limits, &before, &after, "Carve");
        let at = place(&map, -28.0, RIVER_LAT);
        assert!(before[at] > limits.floor + 500);
        assert_eq!(after[at], limits.floor);
    }

    #[test]
    fn a_ceiling_stops_each_mode_that_raises() {
        let modes = [
            Mode::Raise,
            Mode::Uplift,
            Mode::Ridge,
            Mode::Smooth,
            Mode::Flatten,
            Mode::Weather,
            Mode::Warp,
            Mode::Push,
            Mode::Noise,
        ];
        for mode in modes {
            // The flatten mode goes up to the highest level.
            assert_limits_bind(mode, 65535, |ceiling| Limits {
                ceiling,
                ..Limits::default()
            });
        }
    }

    #[test]
    fn ground_outside_the_limits_does_not_change() {
        let center = lonlat_to_dir(10.0, 5.0);
        let moved = |mode: Mode, limits: Limits| {
            let mut map = Heightmap::new(64, 20_000);
            map.stamp(&Stamp {
                mode,
                limits,
                ..raise(center, 0.1)
            });
            map.sample(center)
        };
        let floor = Limits {
            floor: 30_000,
            ..Limits::default()
        };
        assert_eq!(moved(Mode::Raise, floor), 20_000);
        assert_eq!(moved(Mode::Lower, floor), 20_000);
        let ceiling = Limits {
            ceiling: 10_000,
            ..Limits::default()
        };
        assert_eq!(moved(Mode::Lower, ceiling), 20_000);
        assert_eq!(moved(Mode::Raise, ceiling), 20_000);

        // A band around the level at the brush center leaves bumps above it
        // and below it.
        let modes = [
            Mode::Raise,
            Mode::Lower,
            Mode::Uplift,
            Mode::Smooth,
            Mode::Flatten,
            Mode::Weather,
            Mode::Warp,
            Mode::Push,
            Mode::Noise,
        ];
        for mode in modes {
            let band = |level: u16| Limits {
                floor: level - 1000,
                ceiling: level + 1000,
            };
            let name = format!("{mode:?}");
            let (limits, before, after) = limited_run(mode, 0, band);
            assert_inside(limits, &before, &after, &name);
            let outside = |l: &u16| !(limits.floor..=limits.ceiling).contains(l);
            assert!(before.iter().filter(|l| outside(l)).count() > 100, "{name}");
            assert_ne!(after, before, "{name}");
        }
    }

    #[test]
    fn weather_with_limits_keeps_the_sum_of_the_levels() {
        // The hill goes from level 20000 up to level 26000.
        let limits = Limits {
            floor: 21_000,
            ceiling: 24_000,
        };
        let run = |limits: Limits| {
            let mut map = Heightmap::with_tile_size(64, 16, 20_000);
            map.stamp(&Stamp {
                hardness: 0.2,
                strength: 6000.0,
                ..raise(map.texel_dir(0, 32, 32), 0.2)
            });
            let before = all_levels(&map);
            for step in 0..5 {
                map.stamp(&Stamp {
                    hardness: 0.3,
                    flow: 0.7,
                    limits,
                    ..weather(map.texel_dir(0, 30 + step, 33), 0.25, 20.0)
                });
            }
            (before, all_levels(&map))
        };
        let (before, after) = run(limits);
        assert_eq!(sum(&after), sum(&before));
        assert_inside(limits, &before, &after, "Weather");
        assert_ne!(after, before);
        assert_ne!(after, run(Limits::default()).1);
    }

    #[test]
    fn limits_at_the_extreme_levels_change_nothing() {
        let limits = Limits {
            floor: 0,
            ceiling: 65535,
        };
        assert_eq!(limits, Limits::default());
        let levels = [0.0, 1.0, 7.0, 20_000.0, 65_528.0, 65_534.0, 65_535.0];
        for old in levels {
            for new in levels {
                assert_eq!(limits.apply(old, new), new, "{old} {new}");
                // A weather pair moves one eighth of the drop at most.
                let (high, low) = (f64::max(old, new), f64::min(old, new));
                let most = ((high - low) * 0.125).floor();
                assert!(limits.pair_room(high, low) >= most, "{old} {new}");
            }
        }
    }
}
