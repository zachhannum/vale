//! The smooth display of a raster layer: a pyramid of coarser copies of the
//! map, and the blend of two of them at a fractional level.
//!
//! Level 0 is the map. Each texel of level `k` is the rounded mean of a 2 by
//! 2 block of level `k - 1`. A level halves the face size and rounds down, as
//! the mip levels of a texture do. `field.wgsl` reads the same values on the
//! GPU.

use std::f64::consts::{FRAC_PI_4, PI};

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

    fn load(&self, level: u32, face: usize, x: i64, y: i64) -> f64 {
        f64::from(self.get(level, face, x as usize, y as usize)) / 65535.0
    }

    /// The texel of the next face that touches a texel on a face edge. `x`
    /// or `y` is one texel past the face.
    fn across(&self, level: u32, face: usize, x: i64, y: i64) -> f64 {
        let m = i64::from(level_size(self.face_size, level));
        let flat_at = |i: i64| {
            let s = (i as f64 + 0.5) / m as f64 * 2.0 - 1.0;
            unwarp(s.clamp(-1.0, 1.0))
        };
        let axis = face / 2;
        let mut p = [0.0; 3];
        p[axis] = 1.0 - 2.0 * (face % 2) as f64;
        p[(axis + 1) % 3] = flat_at(x);
        p[(axis + 2) % 3] = flat_at(y);
        let next = if y < 0 || y >= m {
            (axis + 2) % 3
        } else {
            (axis + 1) % 3
        };
        let to = next * 2 + usize::from(p[next] < 0.0);
        let index = |f: f64| {
            let s = (f / p[next].abs()).atan() / FRAC_PI_4;
            (((s * 0.5 + 0.5) * m as f64).floor() as i64).clamp(0, m - 1)
        };
        let (cx, cy) = (index(p[(next + 1) % 3]), index(p[(next + 2) % 3]));
        self.load(level, to, cx, cy)
    }

    /// The level of a texel, from 0 to 1. A texel one step past the face
    /// comes from the next face, and a texel past a cube corner is the mean
    /// of the three corner texels.
    fn texel(&self, level: u32, face: usize, x: i64, y: i64) -> f64 {
        let last = i64::from(level_size(self.face_size, level)) - 1;
        let (cx, cy) = (x.clamp(0, last), y.clamp(0, last));
        if x == cx && y == cy {
            return self.load(level, face, x, y);
        }
        if x != cx && y != cy {
            let sum = self.load(level, face, cx, cy)
                + self.across(level, face, x, cy)
                + self.across(level, face, cx, y);
            return sum / 3.0;
        }
        self.across(level, face, x, y)
    }

    /// The bilinear value at a direction at one level, from 0 to 1.
    pub fn bilinear(&self, level: u32, d: V3) -> f64 {
        let m = f64::from(level_size(self.face_size, level));
        let (face, a, b) = face_of(d);
        let f = [
            (warp(a) * 0.5 + 0.5) * m - 0.5,
            (warp(b) * 0.5 + 0.5) * m - 0.5,
        ];
        let (x, y) = (f[0].floor() as i64, f[1].floor() as i64);
        let (tx, ty) = (f[0] - f[0].floor(), f[1] - f[1].floor());
        let h00 = self.texel(level, face, x, y);
        let h10 = self.texel(level, face, x + 1, y);
        let h01 = self.texel(level, face, x, y + 1);
        let h11 = self.texel(level, face, x + 1, y + 1);
        let mix = |a: f64, b: f64, t: f64| a + (b - a) * t;
        mix(mix(h00, h10, tx), mix(h01, h11, tx), ty)
    }

    /// The value that the display shows at a direction, from 0 to 1. A `lod`
    /// of 0 or less is the bilinear map. Between two whole levels, the value
    /// is the blend of the bilinear values of both. A `lod` past the
    /// coarsest level is that level.
    pub fn field(&self, d: V3, lod: f64) -> f64 {
        if lod.is_nan() || lod <= 0.0 {
            return self.bilinear(0, d);
        }
        let top = self.max_level();
        let lod = lod.min(f64::from(top));
        let k = lod.floor() as u32;
        if k >= top {
            return self.bilinear(top, d);
        }
        let t = lod - f64::from(k);
        let a = self.bilinear(k, d);
        a + (self.bilinear(k + 1, d) - a) * t
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

    #[test]
    fn lod_zero_is_the_bilinear_map() {
        let map = waves(16);
        let p = Pyramid::new(&map);
        let level = |face, x, y| f64::from(map.get(face, x, y)) / 65535.0;
        for face in 0..FACES {
            for &(x, y) in &[(3, 4), (7, 7), (12, 1)] {
                let center = p.field(map.texel_dir(face, x, y), 0.0);
                assert!((center - level(face, x as i64, y as i64)).abs() < 1e-9);
                // A third of the way to the next texel in x and in y.
                let flat = |i: usize| unwarp(((i as f64 + 0.5 + 1.0 / 3.0) / 16.0) * 2.0 - 1.0);
                let d = face_dir(face, flat(x), flat(y));
                let (x, y) = (x as i64, y as i64);
                let top = (2.0 * level(face, x, y) + level(face, x + 1, y)) / 3.0;
                let bottom = (2.0 * level(face, x, y + 1) + level(face, x + 1, y + 1)) / 3.0;
                let expected = (2.0 * top + bottom) / 3.0;
                assert!((p.field(d, 0.0) - expected).abs() < 1e-9);
                assert!((p.field(d, -2.0) - expected).abs() < 1e-9);
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
        assert!((p.field(d, 0.5) - (p.bilinear(0, d) + p.bilinear(1, d)) / 2.0).abs() < 1e-12);
        assert_eq!(p.field(d, 9.0), p.bilinear(3, d));
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
}
