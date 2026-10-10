//! The tile store and the brush.
//!
//! Each face splits into square tiles. A tile takes memory from the first
//! change of one of its texels. Each other tile reads as the base level.

use std::collections::{HashMap, VecDeque};
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, TAU};
use std::sync::Arc;

use crate::bands::{Bands, SEA_LEVEL};
use crate::cube::{FACES, face_dir, face_of, meters_to_level, unwarp, warp};
use crate::flow::{ChannelMap, ChannelWindow};
use crate::math::{V3, cross, normalize};

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
    /// Lowers the ground along the rivers of the channel map. Land does not
    /// go below sea level.
    Carve,
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
    /// The largest change of the raise and lower modes, in 16-bit steps.
    pub strength: f64,
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

type Tile = Box<[u16]>;

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
    /// The rivers that the carve mode follows.
    channels: Option<Arc<ChannelMap>>,
    /// The rivers of a part of the sphere at a small scale. The carve mode
    /// follows them where the window has them.
    window: Option<Arc<ChannelWindow>>,
    /// The band limits and the color ramp of the preview.
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
            channels: None,
            window: None,
            bands: Bands::default(),
        };
        map.mark_all_dirty();
        map
    }

    pub fn face_size(&self) -> usize {
        self.n
    }

    /// The level of each texel in a tile that has no memory.
    pub fn base(&self) -> u16 {
        self.base
    }

    /// Sets the rivers that the carve mode follows. With `None`, the carve
    /// mode changes nothing.
    pub fn set_channels(&mut self, channels: Option<Arc<ChannelMap>>) {
        self.channels = channels;
    }

    pub fn channels(&self) -> Option<&Arc<ChannelMap>> {
        self.channels.as_ref()
    }

    /// Sets the window of small rivers. The carve mode follows it on the
    /// ground that `Window::covers`, and the channel map on the other ground.
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

    /// Takes the rectangle of changed texels of each face.
    pub fn take_dirty(&mut self) -> [Option<TexelRect>; FACES] {
        std::mem::take(&mut self.dirty)
    }

    /// Returns `true` one time after the base level or the full set of tiles
    /// changed. A new heightmap, `fill`, and the undo or the redo of a `fill`
    /// do this. A reader then needs the base level and each tile again, so the call also
    /// clears the changed rectangles.
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
            }
        }
        self.stroke = None;
        if saved.base != level || !saved.tiles.is_empty() {
            self.base = level;
            self.push_undo(saved);
            self.mark_all_dirty();
            self.reset = true;
        }
    }

    /// The texel index for an equal-angle coordinate.
    fn index(&self, s: f64) -> usize {
        (((s + 1.0) * 0.5 * self.n as f64).floor().max(0.0) as usize).min(self.n - 1)
    }

    fn texel(&self, face: usize, x: usize, y: usize) -> u16 {
        match &self.tiles[self.tile_index(face, x, y)] {
            Some(tile) => tile[self.offset(x, y)],
            None => self.base,
        }
    }

    /// The level at a direction, from the nearest texel.
    pub fn sample(&self, d: V3) -> u16 {
        let (face, a, b) = face_of(d);
        self.texel(face, self.index(warp(a)), self.index(warp(b)))
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
        save_tile(&mut self.stroke, index, tile);
        tile.get_or_insert_with(|| vec![base; len].into())[offset] = level;
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
                save_tile(&mut self.stroke, index, tile);
                let tile = tile.get_or_insert_with(|| vec![base; t * t].into());
                for (new, to) in rows {
                    tile[to].copy_from_slice(new);
                }
            }
        }
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

    /// Adds an undo step. The redo steps are no longer valid.
    fn push_undo(&mut self, saved: Saved) {
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
        }
        for (&index, tile) in &mut step.tiles {
            std::mem::swap(&mut self.tiles[index], tile);
            let (face, rect) = self.tile_rect(index);
            self.mark_dirty(face, rect);
        }
    }

    /// The texels of one face that a brush circle can touch.
    fn stamp_rect(&self, face: usize, center: V3, radius: f64) -> Option<TexelRect> {
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
        let lo = |a: f64| self.index(warp(a.clamp(-1.0, 1.0))).saturating_sub(1);
        let hi = |a: f64| (self.index(warp(a.clamp(-1.0, 1.0))) + 2).min(self.n);
        Some(TexelRect {
            x0: lo(a0),
            y0: lo(b0),
            x1: hi(a1),
            y1: hi(b1),
        })
    }

    /// The texels that a stamp can touch.
    pub fn stamp_plan(&self, stamp: &Stamp) -> StampPlan {
        let radius = stamp.radius.clamp(1e-6, MAX_BRUSH_RADIUS);
        StampPlan {
            stamp: Stamp { radius, ..*stamp },
            rects: std::array::from_fn(|face| self.stamp_rect(face, stamp.center, radius)),
            reach: ((radius / (FRAC_PI_2 / self.n as f64)) * 0.25).max(1.0) as usize,
        }
    }

    /// Applies one stamp. Returns the number of texels that it visited.
    pub fn stamp(&mut self, stamp: &Stamp) -> usize {
        let plan = self.stamp_plan(stamp);
        let mut visited = 0;
        for (face, rect) in plan.rects.into_iter().enumerate() {
            if let Some(rect) = rect {
                visited += self.stamp_face(face, rect, &plan.stamp, plan.reach);
                self.mark_dirty(face, rect);
            }
        }
        visited
    }

    fn stamp_face(&mut self, face: usize, rect: TexelRect, stamp: &Stamp, reach: usize) -> usize {
        let (t, base) = (self.tile, self.base);
        let radius = stamp.radius;
        let axis = face / 2;
        let sign = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
        let c = stamp.center;
        let (cn, cu, cv) = (c[axis] * sign, c[(axis + 1) % 3], c[(axis + 2) % 3]);
        let cos_radius = radius.cos();
        let hard = stamp.hardness.clamp(0.0, 0.999);
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
        for ty in rect.y0 / t..=(rect.y1 - 1) / t {
            for tx in rect.x0 / t..=(rect.x1 - 1) / t {
                let index = (face * self.per_side + ty) * self.per_side + tx;
                let tile = &mut self.tiles[index];
                let mut saved = false;
                for y in rect.y0.max(ty * t)..rect.y1.min((ty + 1) * t) {
                    let fv = self.flat[y];
                    for x in rect.x0.max(tx * t)..rect.x1.min((tx + 1) * t) {
                        let fu = self.flat[x];
                        let cos_angle = (cn + fu * cu + fv * cv) / (1.0 + fu * fu + fv * fv).sqrt();
                        if cos_angle <= cos_radius {
                            continue;
                        }
                        let k = cos_angle.min(1.0).acos() / radius;
                        let weight = if k <= hard {
                            1.0
                        } else {
                            let k = (k - hard) / (1.0 - hard);
                            1.0 - k * k * (3.0 - 2.0 * k)
                        };
                        let amount = weight * stamp.flow;
                        let offset = (y - ty * t) * t + (x - tx * t);
                        let old_level = tile.as_ref().map_or(base, |tile| tile[offset]);
                        let old = f64::from(old_level);
                        let new = match stamp.mode {
                            Mode::Raise => old + amount * stamp.strength,
                            Mode::Lower => old - amount * stamp.strength,
                            Mode::Flatten => old + (f64::from(stamp.level) - old) * amount.min(1.0),
                            Mode::Carve => {
                                // The place of the texel in the window. On
                                // the face of the window, the place is exact.
                                let in_window = window.as_ref().and_then(|channels| {
                                    let window = channels.window();
                                    let (u, v) = if window.face == face {
                                        window.channel_of_texel(x as i64, y as i64)
                                    } else {
                                        window.channel_of_dir(face_dir(face, fu, fv))?
                                    };
                                    channels.lookup(u, v)
                                });
                                let in_map = || {
                                    // The place of the texel in the channel map.
                                    let at = |x: usize| (x as f64 + 0.5) * channel_size / n - 0.5;
                                    let (distance, flow) =
                                        channels.as_ref()?.sample(face, at(x), at(y));
                                    (flow > 0).then_some((distance, f64::from(flow)))
                                };
                                let Some((distance, flow)) = in_window.or_else(in_map) else {
                                    continue;
                                };
                                // A large river has a wide and deep valley.
                                let size = flow / 255.0;
                                let k = (distance / (1.5 + 4.5 * size)).clamp(0.0, 1.0);
                                let profile = 1.0 - k * k * (3.0 - 2.0 * k);
                                let depth =
                                    amount * stamp.strength * profile * (0.25 + 0.75 * size);
                                (old - depth).round().max(old.min(sea))
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
                        let new = new.round().clamp(0.0, 65535.0) as u16;
                        if new == old_level {
                            continue;
                        }
                        if !saved {
                            save_tile(&mut self.stroke, index, tile);
                            saved = true;
                        }
                        tile.get_or_insert_with(|| vec![base; t * t].into())[offset] = new;
                    }
                }
            }
        }
        (rect.x1 - rect.x0) * (rect.y1 - rect.y0)
    }
}

/// Keeps a copy of a tile for undo, at the first change in a stroke.
fn save_tile(stroke: &mut Option<Saved>, index: usize, tile: &Option<Tile>) {
    if let Some(saved) = stroke {
        saved.tiles.entry(index).or_insert_with(|| tile.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::{
        CoarseHeights, FlowMap, Window, WindowHeights, channel_map, window_channels,
    };
    use crate::math::{angle, dir_to_lonlat, lonlat_to_dir};

    fn raise(center: V3, radius: f64) -> Stamp {
        Stamp {
            center,
            radius,
            hardness: 1.0,
            flow: 1.0,
            mode: Mode::Raise,
            level: 0,
            strength: 1000.0,
        }
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
                        let rect = map.stamp_rect(face, center, radius);
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
        let n = 256;
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
        let channels = channel_map(&CoarseHeights::new(&map));
        assert!(channels.has_rivers());
        map.set_channels(Some(Arc::new(channels)));
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
        // The river is about 1300 steps above the sea at this place.
        for _ in 0..5 {
            map.stamp(&carve(-28.0, RIVER_LAT, 0.05));
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
        map.set_channels(Some(Arc::new(flow.channels())));
        let window = Window::centered(lonlat_to_dir(0.0, RIVER_LAT), 256, 1, cells);
        let channels = window_channels(&WindowHeights::new(&map, window), &flow);
        assert!(channels.has_rivers());
        map.set_window(Some(Arc::new(channels)));
        map
    }

    /// The number of texels across the river at longitude 0 that a carve
    /// stroke along the river lowers by more than half of the largest change.
    fn valley_width(map: &mut Heightmap) -> usize {
        let before = face_levels(map);
        for step in -2..=2 {
            map.stamp(&carve(f64::from(step), RIVER_LAT, 0.1));
        }
        let after = face_levels(map);
        let column = place(map, 0.0, RIVER_LAT) % 256;
        let lowered: Vec<u16> = (0..256)
            .map(|y| before[y * 256 + column] - after[y * 256 + column])
            .collect();
        let most = *lowered.iter().max().unwrap();
        assert!(most > 500, "{most}");
        lowered.iter().filter(|&&change| change > most / 2).count()
    }

    #[test]
    fn carve_in_a_window_cuts_a_narrow_valley() {
        let mut map = valley_with_window(96);
        let narrow = valley_width(&mut map);
        let mut map = valley_with_window(96);
        map.set_window(None);
        let wide = valley_width(&mut map);
        assert!(narrow >= 1 && 3 * narrow <= wide, "{narrow} {wide}");
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
    fn undo_and_redo_work_for_each_mode() {
        // The place is near a cube corner, so a stroke changes three faces.
        let center = lonlat_to_dir(44.0, 34.0);
        for mode in [Mode::Raise, Mode::Lower, Mode::Smooth, Mode::Flatten] {
            let mut map = Heightmap::with_tile_size(64, 16, 20_000);
            // A hill gives the smooth mode a slope.
            assert!(stroke(&mut map, raise(center, 0.1)));
            let before = texels(&map);
            let stamp = Stamp {
                mode,
                level: 30_000,
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
}
