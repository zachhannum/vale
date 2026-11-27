//! The smooth display of a raster layer: a pyramid of coarser copies of the
//! map, and the blend of two of them at a fractional level.
//!
//! Level 0 is the map. Each texel of level `k` is the rounded mean of a 2 by
//! 2 block of level `k - 1`. A level halves the face size and rounds down, as
//! the mip levels of a texture do. `field.wgsl` reads the same values on the
//! GPU. The value at a place is the uniform cubic B-spline of the 4 by 4
//! texels around it.

use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_4, PI};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::cube::{FACES, face_of, unwarp, warp};
use crate::heightmap::{Heightmap, TexelRect};
use crate::math::V3;

/// The coarsest level of a face of `face_size` texels. Its face has 4 texels
/// or more along one side.
pub fn max_level(face_size: u32) -> u32 {
    match face_size {
        0 => 0,
        n => n.ilog2().saturating_sub(2),
    }
}

/// The number of texels along one side of a face at `level`.
pub fn level_size(face_size: u32, level: u32) -> u32 {
    face_size.checked_shr(level).unwrap_or(0).max(1)
}

/// The level that shows features of `feature_points` points or more, at
/// `points_per_radian` on the screen. A face spans a quarter turn, so one
/// texel is `PI / 2 / face_size` radians. The level is from 0 to
/// `max_level(face_size)`.
pub fn smoothing_level(feature_points: f64, points_per_radian: f64, face_size: u32) -> f64 {
    let texels = feature_points * f64::from(face_size) * 2.0 / (PI * points_per_radian);
    let level = texels.log2();
    if feature_points <= 0.0 || !level.is_finite() {
        return 0.0;
    }
    level.clamp(0.0, f64::from(max_level(face_size)))
}

/// The memory of the levels from 1 to `max_level(face_size)`, in bytes.
pub fn pyramid_bytes(face_size: u32) -> u64 {
    (1..=max_level(face_size))
        .map(|k| u64::from(level_size(face_size, k)).pow(2))
        .sum::<u64>()
        * (FACES * size_of::<u16>()) as u64
}

/// The texels of the next level of one face of `size` texels. A block that
/// goes past the face takes the mean of its texels on the face.
pub fn next_level(face: &[u16], size: usize) -> Vec<u16> {
    let half = (size / 2).max(1);
    let mut out = Vec::with_capacity(half * half);
    for y in 0..half {
        for x in 0..half {
            let (mut sum, mut count) = (0u32, 0u32);
            for (bx, by) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sy) = (2 * x + bx, 2 * y + by);
                if sx < size && sy < size {
                    sum += u32::from(face[sy * size + sx]);
                    count += 1;
                }
            }
            out.push(((sum + count / 2) / count) as u16);
        }
    }
    out
}

/// All levels of a map, from 0 to `max_level`, face by face.
pub struct Pyramid {
    face_size: u32,
    /// The texels of each level and face, row by row.
    levels: Vec<[Vec<u16>; FACES]>,
}

impl Pyramid {
    pub fn new(map: &Heightmap) -> Pyramid {
        let n = map.face_size();
        let full = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        let base = std::array::from_fn(|face| {
            let mut out = Vec::with_capacity(n * n);
            map.read_rect(face, full, &mut out);
            out
        });
        let face_size = n as u32;
        let mut levels = vec![base];
        for k in 1..=max_level(face_size) {
            let size = level_size(face_size, k - 1) as usize;
            let prev = levels.last().expect("level 0 is there");
            let next = std::array::from_fn(|face| next_level(&prev[face], size));
            levels.push(next);
        }
        Pyramid { face_size, levels }
    }

    pub fn face_size(&self) -> u32 {
        self.face_size
    }

    pub fn max_level(&self) -> u32 {
        (self.levels.len() - 1) as u32
    }

    /// The texels of one face at `level`, row by row.
    pub fn face(&self, level: u32, face: usize) -> &[u16] {
        &self.levels[level as usize][face]
    }

    /// The level of one texel. The texel is on the face.
    pub fn get(&self, level: u32, face: usize, x: usize, y: usize) -> u16 {
        let m = level_size(self.face_size, level) as usize;
        self.levels[level as usize][face][y * m + x]
    }
}

impl Pyramid {
    /// The value at a direction at one level, from 0 to 1, and its slope.
    /// See `Levels::spline`.
    pub fn spline(&self, level: u32, d: V3) -> [f64; 3] {
        Levels::spline(self, level, d)
    }

    /// The value that the display shows at a direction, from 0 to 1. See
    /// `Levels::field`.
    pub fn field(&self, d: V3, lod: f64) -> f64 {
        Levels::field(self, d, lod)
    }

    /// The value that the display shows at a direction, and its slope. See
    /// `Levels::field_slope`.
    pub fn field_slope(&self, d: V3, lod: f64) -> [f64; 3] {
        Levels::field_slope(self, d, lod)
    }

    /// The value that the display of a height layer shows, and its slope.
    /// See `Levels::shown_slope`.
    pub fn shown_slope(
        &self,
        d: V3,
        land_lod: f64,
        sea_lod: f64,
        sea: f64,
        depth: f64,
    ) -> [f64; 3] {
        Levels::shown_slope(self, d, land_lod, sea_lod, sea, depth)
    }
}

impl Levels for Pyramid {
    fn face_size(&self) -> u32 {
        self.face_size
    }

    fn max_level(&self) -> u32 {
        (self.levels.len() - 1) as u32
    }

    fn get(&self, level: u32, face: usize, x: usize, y: usize) -> u16 {
        Pyramid::get(self, level, face, x, y)
    }
}

/// The levels of a pyramid, and the field that the display shows from them.
pub(crate) trait Levels {
    fn face_size(&self) -> u32;

    fn max_level(&self) -> u32;

    /// The level of one texel. The texel is on the face.
    fn get(&self, level: u32, face: usize, x: usize, y: usize) -> u16;

    fn load(&self, level: u32, face: usize, x: i64, y: i64) -> f64 {
        f64::from(self.get(level, face, x as usize, y as usize)) / 65535.0
    }

    /// The texel of the next face at a texel past a face edge. `x` or `y`
    /// is one or two texels past the face. The texel one step past the face
    /// is the texel of the next face that touches the edge there, and the
    /// texel two steps past is the texel after it, away from the edge.
    fn across(&self, level: u32, face: usize, x: i64, y: i64) -> f64 {
        let m = i64::from(level_size(self.face_size(), level));
        let flat_at = |i: i64| {
            let s = (i as f64 + 0.5) / m as f64 * 2.0 - 1.0;
            unwarp(s.clamp(-1.0, 1.0))
        };
        let axis = face / 2;
        let mut p = [0.0; 3];
        p[axis] = 1.0 - 2.0 * (face % 2) as f64;
        p[(axis + 1) % 3] = flat_at(x);
        p[(axis + 2) % 3] = flat_at(y);
        let (next, past) = if y < 0 || y >= m {
            ((axis + 2) % 3, y)
        } else {
            ((axis + 1) % 3, x)
        };
        // The number of steps past the face, after the first.
        let extra = if past < 0 { -past - 1 } else { past - m };
        let to = next * 2 + usize::from(p[next] < 0.0);
        let index = |i: usize| {
            let f = p[(next + i) % 3];
            let s = (f / p[next].abs()).atan() / FRAC_PI_4;
            let c = (((s * 0.5 + 0.5) * m as f64).floor() as i64).clamp(0, m - 1);
            // The axis of this face goes away from the edge on the next face.
            if (next + i) % 3 == axis {
                (c - extra * p[axis] as i64).clamp(0, m - 1)
            } else {
                c
            }
        };
        self.load(level, to, index(1), index(2))
    }

    /// The level of a texel, from 0 to 1. A texel one or two steps past the
    /// face comes from the next face.
    fn texel(&self, level: u32, face: usize, x: i64, y: i64) -> f64 {
        let last = i64::from(level_size(self.face_size(), level)) - 1;
        let (cx, cy) = (x.clamp(0, last), y.clamp(0, last));
        if x == cx && y == cy {
            return self.load(level, face, x, y);
        }
        if x != cx && y != cy {
            // Three faces meet at a cube corner, so no texel is there. A
            // texel `kx` and `ky` steps past the corner is the mean of the
            // texel `kx` and `ky` steps in from the corner and of that texel
            // turned a quarter turn each way, so each face finds the same
            // values near the corner.
            let kx = if x < 0 { -x } else { x - last };
            let ky = if y < 0 { -y } else { y - last };
            let inside = |i: i64, k: i64| (if i < 0 { k - 1 } else { last + 1 - k }).clamp(0, last);
            let past = |i: i64, k: i64| if i < 0 { -k } else { last + k };
            let sum = self.load(level, face, inside(x, kx), inside(y, ky))
                + self.across(level, face, inside(x, ky), past(y, kx))
                + self.across(level, face, past(x, ky), inside(y, kx));
            return sum / 3.0;
        }
        self.across(level, face, x, y)
    }

    /// The value at a direction at one level, from 0 to 1, and its change
    /// across one texel of that level along the x and the y axis of the
    /// face. The value is the uniform cubic B-spline of the 4 by 4 texels
    /// around the place, and the slope is the derivative of the same spline.
    fn spline(&self, level: u32, d: V3) -> [f64; 3] {
        let m = f64::from(level_size(self.face_size(), level));
        let (face, a, b) = face_of(d);
        let f = [
            (warp(a) * 0.5 + 0.5) * m - 0.5,
            (warp(b) * 0.5 + 0.5) * m - 0.5,
        ];
        let (x, y) = (f[0].floor() as i64 - 1, f[1].floor() as i64 - 1);
        let (tx, ty) = (f[0] - f[0].floor(), f[1] - f[1].floor());
        let (wx, sx) = (spline_weights(tx), spline_slopes(tx));
        let (wy, sy) = (spline_weights(ty), spline_slopes(ty));
        let rows: [[f64; 4]; 4] = std::array::from_fn(|r| {
            std::array::from_fn(|c| self.texel(level, face, x + c as i64, y + r as i64))
        });
        // The sum is of the differences from one texel, as in `field.wgsl`.
        let base = rows[1][1];
        let mut h = [base, 0.0, 0.0];
        for (r, row) in rows.iter().enumerate() {
            let dot = |w: [f64; 4]| (0..4).map(|c| w[c] * (row[c] - base)).sum::<f64>();
            let v = dot(wx);
            h[0] += wy[r] * v;
            h[1] += wy[r] * dot(sx);
            h[2] += sy[r] * v;
        }
        h
    }

    /// `spline` at a level, with the slope across one texel of level 0.
    fn level_spline(&self, level: u32, d: V3) -> [f64; 3] {
        let [h, sx, sy] = self.spline(level, d);
        let scale = f64::from(1u32 << level);
        [h, sx / scale, sy / scale]
    }

    /// The value that the display shows at a direction, from 0 to 1. A `lod`
    /// of 0 or less is the spline of the map. Between two whole levels, the
    /// value is the blend of the splines of both. A `lod` past the coarsest
    /// level is that level.
    fn field(&self, d: V3, lod: f64) -> f64 {
        self.field_slope(d, lod)[0]
    }

    /// `field`, and its slope across one texel of level 0, as in
    /// `field.wgsl`.
    fn field_slope(&self, d: V3, lod: f64) -> [f64; 3] {
        if lod.is_nan() || lod <= 0.0 {
            return self.spline(0, d);
        }
        let top = self.max_level();
        let lod = lod.min(f64::from(top));
        let k = lod.floor() as u32;
        if k >= top {
            return self.level_spline(top, d);
        }
        let t = lod - f64::from(k);
        let a = self.level_spline(k, d);
        let b = self.level_spline(k + 1, d);
        std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
    }

    /// The value that the display of a height layer shows at a direction,
    /// and its slope, as `shown_field` in `field.wgsl`. Above sea level, it
    /// is the field at `land_lod`. Below sea level, it blends to the field at
    /// `sea_lod`. See `combine_sea`. A `depth` below 0 means no sea, and then
    /// the value is the field at `land_lod`, as it is with equal levels.
    fn shown_slope(&self, d: V3, land_lod: f64, sea_lod: f64, sea: f64, depth: f64) -> [f64; 3] {
        let land = self.field_slope(d, land_lod);
        if depth < 0.0 || land_lod == sea_lod {
            return land;
        }
        combine_sea(land, || self.field_slope(d, sea_lod), sea, depth)
    }
}

/// The smallest step of a value, from 0 to 1.
const VALUE_STEP: f64 = 1.0 / 65535.0;

/// The blend of the land field `land` and the sea field `sea_field` below
/// sea level `sea`, with each value and its slope as `field_slope` gives
/// them. At or above sea level, the result is `land`. Below it, the result
/// goes from `land` at the coast to `sea_field` at `depth` below sea level,
/// along a smoothstep of the depth of `land`. The sea field is held at least
/// one value step below sea level, so the result stays below sea level. The
/// value and the slope are continuous at the coast. `sea_field` runs only
/// below sea level. All values are from 0 to 1.
pub fn combine_sea(
    land: [f64; 3],
    sea_field: impl FnOnce() -> [f64; 3],
    sea: f64,
    depth: f64,
) -> [f64; 3] {
    let below = land[0] - sea;
    if below.is_nan() || below >= 0.0 {
        return land;
    }
    // The blend `a` and its derivative `da` by the land value.
    let (a, da) = if depth > 0.0 {
        let t = (-below / depth).min(1.0);
        (t * t * (3.0 - 2.0 * t), -6.0 * t * (1.0 - t) / depth)
    } else {
        (1.0, 0.0)
    };
    if a.is_nan() || a <= 0.0 {
        return land;
    }
    let c = sea_field();
    let deep = c[0] - sea;
    let clamped = deep < -VALUE_STEP;
    let m = if clamped { deep } else { -VALUE_STEP };
    let value = sea + (1.0 - a) * below + a * m;
    let slope = |i: usize| {
        let sea_slope = if clamped { c[i] } else { 0.0 };
        (1.0 - a) * land[i] + a * sea_slope + da * (m - below) * land[i]
    };
    [value, slope(1), slope(2)]
}

/// The weights of the uniform cubic B-spline at `t` from 0 to 1, for the
/// texels at -1, 0, 1, and 2.
fn spline_weights(t: f64) -> [f64; 4] {
    let s = 1.0 - t;
    let (t2, t3) = (t * t, t * t * t);
    [
        s * s * s / 6.0,
        (3.0 * t3 - 6.0 * t2 + 4.0) / 6.0,
        (3.0 * (t + t2 - t3) + 1.0) / 6.0,
        t3 / 6.0,
    ]
}

/// The derivatives of `spline_weights` by `t`.
fn spline_slopes(t: f64) -> [f64; 4] {
    let s = 1.0 - t;
    let t2 = t * t;
    [
        -s * s * 0.5,
        (3.0 * t2 - 4.0 * t) * 0.5,
        (1.0 + 2.0 * t - 3.0 * t2) * 0.5,
        t2 * 0.5,
    ]
}

/// The levels of `Pyramid` of a reduced copy of a heightmap, made only for
/// the blocks of texels that a read reaches. The texels are the same as in
/// the full pyramid of the copy.
pub struct LocalPyramid {
    map: Heightmap,
    /// The reduction of `map` to the copy, a power of two.
    factor: usize,
    face_size: u32,
    blocks: Mutex<HashMap<BlockKey, Arc<[u16]>>>,
    filled: AtomicU64,
}

/// A block of a `LocalPyramid`: the level, the face, and the column and the
/// row of the block.
type BlockKey = (u32, usize, usize, usize);

/// The largest number of blocks in the cache of a `LocalPyramid`.
const MAX_BLOCKS: usize = 256;

impl LocalPyramid {
    /// The levels of a copy of `map` with a face of `face_size` texels, as
    /// the GPU has it. `map` is a snapshot, so a later change to the
    /// heightmap does not change it. `None`: the copy is not a reduction of
    /// `map` by a power of two.
    pub fn new(map: Heightmap, face_size: u32) -> Option<LocalPyramid> {
        let factor = map
            .face_size()
            .div_ceil(face_size as usize)
            .max(1)
            .next_power_of_two();
        if map.face_size().div_ceil(factor) != face_size as usize {
            return None;
        }
        Some(LocalPyramid {
            map,
            factor,
            face_size,
            blocks: Mutex::new(HashMap::new()),
            filled: AtomicU64::new(0),
        })
    }

    /// The value that the display shows at a direction, from 0 to 1. See
    /// `Levels::field`.
    pub fn field(&self, d: V3, lod: f64) -> f64 {
        Levels::field(self, d, lod)
    }

    /// The value that the display of a height layer shows, and its slope.
    /// See `Levels::shown_slope`.
    pub fn shown_slope(
        &self,
        d: V3,
        land_lod: f64,
        sea_lod: f64,
        sea: f64,
        depth: f64,
    ) -> [f64; 3] {
        Levels::shown_slope(self, d, land_lod, sea_lod, sea, depth)
    }

    pub fn face_size(&self) -> u32 {
        self.face_size
    }

    /// The number of blocks that the pyramid made.
    pub fn filled(&self) -> u64 {
        self.filled.load(Ordering::Relaxed)
    }

    /// The number of texels along one side of a block at `level`. A block
    /// covers 64 texels of the copy or more.
    fn block(level: u32) -> usize {
        (64usize >> level.min(6)).max(2)
    }

    /// The texels of one block at `level`, row by row: each texel of the
    /// level from the column `bx * side` and the row `by * side`, up to the
    /// end of the face.
    fn fill(&self, level: u32, face: usize, bx: usize, by: usize) -> Arc<[u16]> {
        self.filled.fetch_add(1, Ordering::Relaxed);
        let side = Self::block(level);
        let m = level_size(self.face_size, level) as usize;
        let (x0, y0) = (bx * side, by * side);
        let (x1, y1) = ((x0 + side).min(m), (y0 + side).min(m));
        let rect = TexelRect {
            x0: x0 << level,
            y0: y0 << level,
            x1: x1 << level,
            y1: y1 << level,
        };
        let mut data = Vec::new();
        self.map.read_reduced(face, rect, self.factor, &mut data);
        let (mut w, mut h) = (rect.x1 - rect.x0, rect.y1 - rect.y0);
        for _ in 0..level {
            let mut next = Vec::with_capacity(w * h / 4);
            for y in (0..h).step_by(2) {
                for x in (0..w).step_by(2) {
                    let sum = u32::from(data[y * w + x])
                        + u32::from(data[y * w + x + 1])
                        + u32::from(data[(y + 1) * w + x])
                        + u32::from(data[(y + 1) * w + x + 1]);
                    next.push(((sum + 2) / 4) as u16);
                }
            }
            (data, w, h) = (next, w / 2, h / 2);
        }
        data.into()
    }
}

impl Levels for LocalPyramid {
    fn face_size(&self) -> u32 {
        self.face_size
    }

    fn max_level(&self) -> u32 {
        max_level(self.face_size)
    }

    fn get(&self, level: u32, face: usize, x: usize, y: usize) -> u16 {
        let side = Self::block(level);
        let key = (level, face, x / side, y / side);
        let mut blocks = self.blocks.lock().expect("no panic holds the lock");
        let block = match blocks.get(&key) {
            Some(block) => block.clone(),
            None => {
                let block = self.fill(level, face, key.2, key.3);
                if blocks.len() >= MAX_BLOCKS {
                    blocks.clear();
                }
                blocks.insert(key, block.clone());
                block
            }
        };
        drop(blocks);
        let m = level_size(self.face_size, level) as usize;
        let width = (key.2 * side + side).min(m) - key.2 * side;
        block[(y % side) * width + x % side]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::face_dir;
    use crate::math::{add, cross, dot, normalize, scale};

    /// A map of smooth waves over the sphere.
    fn waves(n: usize) -> Heightmap {
        let mut map = Heightmap::new(n, 0);
        let full = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        let axes = [[0.3, 0.8, 0.5], [-0.7, 0.2, 0.6], [0.5, -0.4, 0.75]];
        for face in 0..FACES {
            let mut data = Vec::with_capacity(n * n);
            for y in 0..n {
                for x in 0..n {
                    let d = map.texel_dir(face, x, y);
                    let w: f64 = axes
                        .iter()
                        .enumerate()
                        .map(|(i, a)| (dot(d, *a) * (9.0 + 4.0 * i as f64)).sin())
                        .sum();
                    data.push((32768.0 + 9000.0 * w) as u16);
                }
            }
            map.store_rect(face, full, &data);
        }
        map
    }

    #[test]
    fn the_levels_stop_at_four_texels() {
        assert_eq!(max_level(0), 0);
        assert_eq!(max_level(4), 0);
        assert_eq!(max_level(8), 1);
        assert_eq!(max_level(2048), 9);
        assert_eq!(max_level(300), 6);
        assert_eq!(level_size(2048, 9), 4);
        assert_eq!(level_size(300, 3), 37);
        assert_eq!(level_size(1, 5), 1);
        assert_eq!(pyramid_bytes(8), 6 * 16 * 2);
        assert_eq!(pyramid_bytes(4), 0);
    }

    #[test]
    fn the_smoothing_level_follows_the_feature_size() {
        let n = 1024;
        // One texel spans PI / 2 / n radians.
        let texel = PI / 2.0 / f64::from(n);
        let ppr = 1.0 / texel;
        assert!(smoothing_level(1.0, ppr, n).abs() < 1e-12);
        assert!((smoothing_level(8.0, ppr, n) - 3.0).abs() < 1e-12);
        assert!((smoothing_level(6.0, ppr, n) - 6f64.log2()).abs() < 1e-12);
        assert_eq!(smoothing_level(0.5, ppr, n), 0.0);
        assert_eq!(smoothing_level(1e9, ppr, n), 8.0);
        assert_eq!(smoothing_level(0.0, ppr, n), 0.0);
        assert_eq!(smoothing_level(-3.0, ppr, n), 0.0);
        assert_eq!(smoothing_level(f64::NAN, ppr, n), 0.0);
        assert_eq!(smoothing_level(4.0, 0.0, n), 0.0);
        assert_eq!(smoothing_level(4.0, ppr, 0), 0.0);
    }

    #[test]
    fn a_level_texel_is_the_rounded_mean_of_its_block() {
        let map = waves(32);
        let p = Pyramid::new(&map);
        assert_eq!(p.max_level(), 3);
        for k in 1..=3 {
            let m = level_size(32, k) as usize;
            for face in 0..FACES {
                for y in 0..m {
                    for x in 0..m {
                        let sum: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)]
                            .iter()
                            .map(|&(bx, by)| u32::from(p.get(k - 1, face, 2 * x + bx, 2 * y + by)))
                            .sum();
                        assert_eq!(u32::from(p.get(k, face, x, y)), (sum + 2) / 4);
                    }
                }
            }
        }
        // An odd face drops its last row and column, and a face of one texel
        // keeps it.
        assert_eq!(next_level(&[1, 2, 3, 4, 5, 6, 7, 8, 9], 3), vec![3]);
        assert_eq!(next_level(&[1, 2, 3, 6], 2), vec![3]);
        assert_eq!(next_level(&[7], 1), vec![7]);
    }

    /// The spline of 4 by 4 texels of level 0 of `map` on a face, with the
    /// texel `(x, y)` at the second column and row, at `(tx, ty)` from it.
    fn spline_at(map: &Heightmap, face: usize, x: usize, y: usize, tx: f64, ty: f64) -> f64 {
        let (wx, wy) = (spline_weights(tx), spline_weights(ty));
        let mut sum = 0.0;
        for (r, wy) in wy.iter().enumerate() {
            for (c, wx) in wx.iter().enumerate() {
                let level = map.get(face, (x + c) as i64 - 1, (y + r) as i64 - 1);
                sum += wx * wy * f64::from(level) / 65535.0;
            }
        }
        sum
    }

    #[test]
    fn lod_zero_is_the_spline_of_the_map() {
        let map = waves(16);
        let p = Pyramid::new(&map);
        let flat = |i: usize, t: f64| unwarp(((i as f64 + 0.5 + t) / 16.0) * 2.0 - 1.0);
        for face in 0..FACES {
            for &(x, y) in &[(3, 4), (7, 7), (12, 1)] {
                let center = p.field(map.texel_dir(face, x, y), 0.0);
                assert!((center - spline_at(&map, face, x, y, 0.0, 0.0)).abs() < 1e-9);
                // A third of the way to the next texel in x and in y.
                let d = face_dir(face, flat(x, 1.0 / 3.0), flat(y, 1.0 / 3.0));
                let expected = spline_at(&map, face, x, y, 1.0 / 3.0, 1.0 / 3.0);
                assert!((p.field(d, 0.0) - expected).abs() < 1e-9);
                assert!((p.field(d, -2.0) - expected).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn the_spline_keeps_a_ramp_and_its_coast() {
        // A ramp along x on face 0 keeps each value, so a coast on smooth
        // ground stays where the texels put it.
        let n = 16;
        let mut map = Heightmap::new(n, 0);
        let full = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        let ramp = |x: f64| 20000.0 + 1500.0 * x;
        let data: Vec<u16> = (0..n * n).map(|i| ramp((i % n) as f64) as u16).collect();
        map.store_rect(0, full, &data);
        let p = Pyramid::new(&map);
        let flat = |x: f64| unwarp(((x + 0.5) / n as f64) * 2.0 - 1.0);
        for x in [2.0, 3.3, 7.5, 11.9] {
            for y in [2.0, 6.7, 12.2] {
                let [h, sx, sy] = p.field_slope(face_dir(0, flat(x), flat(y)), 0.0);
                assert!((h - ramp(x) / 65535.0).abs() < 1e-9, "{x} {y}");
                assert!((sx - 1500.0 / 65535.0).abs() < 1e-9);
                assert!(sy.abs() < 1e-9);
            }
        }
    }

    #[test]
    fn the_slope_is_the_derivative_of_the_value() {
        let map = waves(32);
        let p = Pyramid::new(&map);
        let m = 32.0;
        let flat = |x: f64| unwarp(((x + 0.5) / m) * 2.0 - 1.0);
        let e = 1e-4;
        for lod in [0.0, 0.6, 2.0] {
            for &(x, y) in &[(5.2, 9.7), (16.0, 16.5), (24.9, 3.1)] {
                let at = |x: f64, y: f64| p.field(face_dir(2, flat(x), flat(y)), lod);
                let [_, sx, sy] = p.field_slope(face_dir(2, flat(x), flat(y)), lod);
                let dx = (at(x + e, y) - at(x - e, y)) / (2.0 * e);
                let dy = (at(x, y + e) - at(x, y - e)) / (2.0 * e);
                assert!((sx - dx).abs() < 1e-6, "{lod} {x} {y}: {sx} {dx}");
                assert!((sy - dy).abs() < 1e-6, "{lod} {x} {y}: {sy} {dy}");
            }
        }
    }

    #[test]
    fn the_field_is_continuous_at_whole_levels() {
        let map = waves(32);
        let p = Pyramid::new(&map);
        let d = normalize([0.4, 0.31, -0.77]);
        for k in 0..=3 {
            let k = f64::from(k);
            let at = p.field(d, k);
            assert!((p.field(d, k + 1e-6) - at).abs() < 1e-5);
            assert!((p.field(d, k - 1e-6) - at).abs() < 1e-5);
        }
        assert!((p.field(d, 0.5) - (p.spline(0, d)[0] + p.spline(1, d)[0]) / 2.0).abs() < 1e-12);
        assert_eq!(p.field(d, 9.0), p.spline(3, d)[0]);
    }

    /// The largest step of the field along a short arc through `at`, in
    /// steps of `step` radians, and the largest step that a field with no
    /// seam can make.
    fn steps(p: &Pyramid, at: V3, along: V3, lod: f64, step: f64) -> (f64, f64) {
        let side = normalize(cross(at, along));
        let tangent = cross(side, at);
        let n = f64::from(p.face_size());
        // The steepest texel step, at the base level.
        let mut steep = 0f64;
        for face in 0..FACES {
            let m = n as usize;
            let f = p.face(0, face);
            for y in 0..m {
                for x in 1..m {
                    steep = steep.max(f[y * m + x].abs_diff(f[y * m + x - 1]).into());
                    steep = steep.max(f[x * m + y].abs_diff(f[(x - 1) * m + y]).into());
                }
            }
        }
        // A step of the arc is at most this many texels on an equal-angle face.
        let texels = step / (PI / 2.0 / n) * 2.0;
        let limit = 2.0 * steep / 65535.0 * texels;
        let count = (4.0 * PI / 2.0 / n / step) as i64;
        let point = |i: i64| {
            let t = i as f64 * step;
            normalize(add(scale(at, t.cos()), scale(tangent, t.sin())))
        };
        let mut worst = 0f64;
        let mut last = p.field(point(-count), lod);
        for i in 1 - count..=count {
            let value = p.field(point(i), lod);
            worst = worst.max((value - last).abs());
            last = value;
        }
        (worst, limit)
    }

    #[test]
    fn the_field_has_no_seam_at_face_edges_and_cube_corners() {
        let map = waves(32);
        let p = Pyramid::new(&map);
        let mut places = Vec::new();
        // The middle of each of the 12 edges, crossed at a right angle and
        // at a slant.
        for axis in 0..3 {
            for (sa, sb) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
                let mut d = [0.0; 3];
                d[(axis + 1) % 3] = sa;
                d[(axis + 2) % 3] = sb;
                let mut across = [0.0; 3];
                across[(axis + 1) % 3] = sa;
                across[(axis + 2) % 3] = -sb;
                places.push((normalize(d), normalize(across)));
                let mut slant = across;
                slant[axis] = 0.6;
                places.push((normalize(d), normalize(slant)));
            }
        }
        // The 8 cube corners, along two arcs each.
        for i in 0..8 {
            let s = |bit: usize| if i & bit == 0 { 1.0 } else { -1.0 };
            let d = normalize([s(1), s(2), s(4)]);
            places.push((d, normalize([s(1), -s(2), 0.3])));
            places.push((d, normalize([0.2, s(2), -s(4)])));
        }
        for (at, along) in places {
            for lod in [0.0, 0.4, 1.0, 1.7, 3.0] {
                let (worst, limit) = steps(&p, at, along, lod, 1e-4);
                assert!(
                    worst <= limit,
                    "a step of {worst} at {at:?} along {along:?}, lod {lod}, limit {limit}"
                );
            }
        }
    }

    #[test]
    fn a_local_pyramid_has_the_levels_of_the_copy() {
        for (n, size) in [(40, 40), (80, 40), (64, 16)] {
            let map = waves(n);
            let factor = n / size;
            let mut copy = Heightmap::new(size, map.base());
            let rect = TexelRect {
                x0: 0,
                y0: 0,
                x1: size,
                y1: size,
            };
            for face in 0..FACES {
                let mut data = Vec::new();
                map.read_reduced(face, rect, factor, &mut data);
                copy.store_rect(face, rect, &data);
            }
            let full = Pyramid::new(&copy);
            let local = LocalPyramid::new(map.snapshot(), size as u32).unwrap();
            let mut dirs = Vec::new();
            // Random directions, and directions near face edges and corners.
            let mut seed = 7u64;
            let mut next = || {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
            };
            for _ in 0..300 {
                dirs.push(normalize([next(), next(), next()]));
            }
            for _ in 0..100 {
                let e = 1e-3 * next();
                dirs.push(normalize([1.0, 1.0 + e, next()]));
                dirs.push(normalize([-1.0, 1.0 + e, -1.0 - e * 0.5]));
            }
            for lod in [0.0, 0.4, 1.7, 3.0, 9.0] {
                for &d in &dirs {
                    assert_eq!(local.field(d, lod), full.field(d, lod), "{n} {lod} {d:?}");
                }
            }
            assert!(local.filled() > 0);
        }
    }

    /// A random number from -1 to 1 for each call.
    fn random(seed: u64) -> impl FnMut() -> f64 {
        let mut seed = seed;
        move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        }
    }

    /// A map of rough ground: random texels around the middle value.
    fn rough(n: usize, seed: u64) -> Heightmap {
        let mut map = Heightmap::new(n, 0);
        let full = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        let mut next = random(seed);
        for face in 0..FACES {
            let data: Vec<u16> = (0..n * n)
                .map(|_| (32768.0 + 6000.0 * next()) as u16)
                .collect();
            map.store_rect(face, full, &data);
        }
        map
    }

    const SEA: f64 = 0.5;
    const DEPTH: f64 = 0.04;

    #[test]
    fn equal_levels_show_the_land_field() {
        let p = Pyramid::new(&waves(32));
        let mut next = random(3);
        for _ in 0..500 {
            let d = normalize([next(), next(), next()]);
            let lod = 2.0 * next() + 1.5;
            let field = p.field_slope(d, lod);
            assert_eq!(p.shown_slope(d, lod, lod, SEA, DEPTH), field);
            // A layer with no sea keeps the land field.
            assert_eq!(p.shown_slope(d, lod, lod + 1.3, SEA, -1.0), field);
        }
    }

    #[test]
    fn land_shows_the_land_field() {
        let p = Pyramid::new(&waves(32));
        let mut next = random(5);
        let mut count = 0;
        for _ in 0..1000 {
            let d = normalize([next(), next(), next()]);
            let field = p.field_slope(d, 0.5);
            if field[0] >= SEA {
                assert_eq!(p.shown_slope(d, 0.5, 3.0, SEA, DEPTH), field);
                count += 1;
            }
        }
        assert!(count > 100);
    }

    #[test]
    fn the_shown_field_keeps_the_side_of_sea_level() {
        let mut next = random(11);
        let mut sea_points = 0;
        for (i, map) in [waves(32), rough(16, 1), rough(32, 2)].iter().enumerate() {
            let p = Pyramid::new(map);
            for sea in [0.45, 0.5, 0.53] {
                for _ in 0..400 {
                    let d = normalize([next(), next(), next()]);
                    let land_lod = 1.5 + 1.5 * next();
                    let sea_lod = 1.5 + 1.5 * next();
                    let depth = 0.03 + 0.03 * next();
                    let field = p.field(d, land_lod);
                    let shown = p.shown_slope(d, land_lod, sea_lod, sea, depth)[0];
                    if field >= sea {
                        assert!(shown >= sea, "{i} {sea} {d:?}");
                    } else {
                        assert!(shown < sea, "{i} {sea} {d:?}: {field} {shown}");
                        sea_points += 1;
                    }
                }
            }
        }
        assert!(sea_points > 500);
    }

    #[test]
    fn the_shown_field_and_its_slope_are_continuous_at_the_coast() {
        for map in [waves(32), rough(32, 4)] {
            let p = Pyramid::new(&map);
            let m = 32.0;
            let flat = |x: f64| unwarp(((x + 0.5) / m) * 2.0 - 1.0);
            let dir = |x: f64, y: f64| face_dir(2, flat(x), flat(y));
            let (land_lod, sea_lod) = (0.5, 2.5);
            let mut coasts = 0;
            for y in [4.3, 11.7, 20.2, 27.9] {
                let below = |x: f64| p.field(dir(x, y), land_lod) < SEA;
                let mut x = 1.0;
                while x < 30.0 {
                    let (mut a, mut b) = (x, x + 0.05);
                    x = b;
                    if below(a) == below(b) {
                        continue;
                    }
                    for _ in 0..60 {
                        let mid = 0.5 * (a + b);
                        if below(mid) == below(a) {
                            a = mid;
                        } else {
                            b = mid;
                        }
                    }
                    let ga = p.shown_slope(dir(a, y), land_lod, sea_lod, SEA, DEPTH);
                    let gb = p.shown_slope(dir(b, y), land_lod, sea_lod, SEA, DEPTH);
                    for i in 0..3 {
                        assert!((ga[i] - gb[i]).abs() < 1e-9, "{y} {a}: {ga:?} {gb:?}");
                    }
                    coasts += 1;
                }
            }
            assert!(coasts > 4);
        }
    }

    #[test]
    fn the_shown_slope_is_the_derivative_of_the_value() {
        let p = Pyramid::new(&waves(32));
        let m = 32.0;
        let flat = |x: f64| unwarp(((x + 0.5) / m) * 2.0 - 1.0);
        let at =
            |x: f64, y: f64| p.shown_slope(face_dir(2, flat(x), flat(y)), 0.5, 2.5, SEA, DEPTH);
        let e = 1e-5;
        let mut next = random(9);
        let mut blended = 0;
        for _ in 0..400 {
            let (x, y) = (16.0 + 13.0 * next(), 16.0 + 13.0 * next());
            let [h, sx, sy] = at(x, y);
            let field = p.field(face_dir(2, flat(x), flat(y)), 0.5);
            if field < SEA && field > SEA - DEPTH && h < SEA - 2.0 * VALUE_STEP {
                blended += 1;
            }
            let dx = (at(x + e, y)[0] - at(x - e, y)[0]) / (2.0 * e);
            let dy = (at(x, y + e)[0] - at(x, y - e)[0]) / (2.0 * e);
            // The clamp of the sea field one step below sea level has a kink.
            let sea = p.field(face_dir(2, flat(x), flat(y)), 2.5);
            if (sea - (SEA - VALUE_STEP)).abs() < 1e-6 {
                continue;
            }
            assert!((sx - dx).abs() < 1e-5, "{x} {y}: {sx} {dx}");
            assert!((sy - dy).abs() < 1e-5, "{x} {y}: {sy} {dy}");
        }
        assert!(blended > 20);
    }

    #[test]
    fn the_deep_sea_shows_the_sea_field() {
        let p = Pyramid::new(&rough(32, 6));
        let mut next = random(13);
        let mut count = 0;
        for _ in 0..3000 {
            let d = normalize([next(), next(), next()]);
            let land = p.field(d, 0.0);
            if land >= SEA - 0.01 {
                continue;
            }
            count += 1;
            let sea = p.field_slope(d, 3.0);
            let shown = p.shown_slope(d, 0.0, 3.0, SEA, 0.01);
            if sea[0] - SEA < -VALUE_STEP {
                for i in 0..3 {
                    assert!(
                        (shown[i] - sea[i]).abs() < 1e-12,
                        "{d:?}: {shown:?} {sea:?}"
                    );
                }
            } else {
                assert!((shown[0] - (SEA - VALUE_STEP)).abs() < 1e-12);
                assert_eq!([shown[1], shown[2]], [0.0, 0.0]);
            }
        }
        assert!(count > 100);
    }
}
