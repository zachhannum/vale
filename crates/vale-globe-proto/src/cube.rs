//! The heightmap: a 16-bit cube map with equal-angle spacing, and the brush.
//!
//! Face `2 * axis` looks along the positive axis, and face `2 * axis + 1`
//! looks along the negative axis. On each face, `u` runs along axis
//! `(axis + 1) % 3` and `v` runs along axis `(axis + 2) % 3`. The shader in
//! `globe.wgsl` uses the same layout.

use std::f64::consts::FRAC_PI_4;

use crate::math::{V3, cross, normalize};

pub const FACES: usize = 6;

/// The lowest and highest elevation that the 16-bit range holds, in meters.
pub const ELEV_MIN: f64 = -6000.0;
pub const ELEV_MAX: f64 = 6000.0;

/// The largest brush radius, in radians. The face test in `stamp` needs it.
pub const MAX_BRUSH_RADIUS: f64 = 0.3;

pub fn meters_to_level(m: f64) -> u16 {
    (((m - ELEV_MIN) / (ELEV_MAX - ELEV_MIN)).clamp(0.0, 1.0) * 65535.0).round() as u16
}

pub fn level_to_meters(level: u16) -> f64 {
    ELEV_MIN + f64::from(level) / 65535.0 * (ELEV_MAX - ELEV_MIN)
}

/// The face that a direction hits, and the flat coordinates on that face,
/// each from -1 to 1.
pub fn face_of(d: V3) -> (usize, f64, f64) {
    let abs = [d[0].abs(), d[1].abs(), d[2].abs()];
    let axis = if abs[0] >= abs[1] && abs[0] >= abs[2] {
        0
    } else if abs[1] >= abs[2] {
        1
    } else {
        2
    };
    let face = 2 * axis + usize::from(d[axis] < 0.0);
    (
        face,
        d[(axis + 1) % 3] / abs[axis],
        d[(axis + 2) % 3] / abs[axis],
    )
}

/// The direction of a point on a face. The inverse of `face_of`.
pub fn face_dir(face: usize, a: f64, b: f64) -> V3 {
    let axis = face / 2;
    let mut d = [0.0; 3];
    d[axis] = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
    d[(axis + 1) % 3] = a;
    d[(axis + 2) % 3] = b;
    normalize(d)
}

/// Flat face coordinate to equal-angle coordinate. Both run from -1 to 1.
pub fn warp(a: f64) -> f64 {
    a.atan() / FRAC_PI_4
}

/// Equal-angle coordinate to flat face coordinate.
pub fn unwarp(s: f64) -> f64 {
    (s * FRAC_PI_4).tan()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Raise,
    Lower,
    Smooth,
    Flatten,
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
    fn union(self, o: TexelRect) -> TexelRect {
        TexelRect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }
}

/// Changed texels of one face, ready for the GPU.
pub struct Upload {
    pub face: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u16>,
}

pub struct Heightmap {
    n: usize,
    faces: [Vec<u16>; FACES],
    /// The flat face coordinate at each texel center.
    flat: Vec<f64>,
    dirty: [Option<TexelRect>; FACES],
    /// Face copies from before the current stroke.
    stroke: Option<[Option<Vec<u16>>; FACES]>,
    undo: Vec<[Option<Vec<u16>>; FACES]>,
}

const UNDO_DEPTH: usize = 8;

impl Heightmap {
    /// Makes a heightmap with `n` by `n` texels on each face.
    pub fn new(n: usize, level: u16) -> Heightmap {
        let flat = (0..n)
            .map(|i| unwarp((i as f64 + 0.5) / n as f64 * 2.0 - 1.0))
            .collect();
        let mut map = Heightmap {
            n,
            faces: std::array::from_fn(|_| vec![level; n * n]),
            flat,
            dirty: [None; FACES],
            stroke: None,
            undo: Vec::new(),
        };
        map.mark_all_dirty();
        map
    }

    pub fn face_size(&self) -> usize {
        self.n
    }

    pub fn face(&self, face: usize) -> &[u16] {
        &self.faces[face]
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

    pub fn fill(&mut self, level: u16) {
        self.begin_stroke();
        for face in 0..FACES {
            self.save_face(face);
            self.faces[face].fill(level);
        }
        self.end_stroke();
        self.mark_all_dirty();
    }

    /// The texel index for an equal-angle coordinate.
    fn index(&self, s: f64) -> usize {
        (((s + 1.0) * 0.5 * self.n as f64).floor().max(0.0) as usize).min(self.n - 1)
    }

    /// The level at a direction, from the nearest texel.
    pub fn sample(&self, d: V3) -> u16 {
        let (face, a, b) = face_of(d);
        self.faces[face][self.index(warp(b)) * self.n + self.index(warp(a))]
    }

    /// The direction of a texel center.
    pub fn texel_dir(&self, face: usize, x: usize, y: usize) -> V3 {
        face_dir(face, self.flat[x], self.flat[y])
    }

    pub fn begin_stroke(&mut self) {
        self.stroke = Some(std::array::from_fn(|_| None));
    }

    /// Ends the stroke. Returns `true` if the stroke changed the heightmap.
    pub fn end_stroke(&mut self) -> bool {
        if let Some(saved) = self.stroke.take()
            && saved.iter().any(Option::is_some)
        {
            if self.undo.len() == UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.undo.push(saved);
            return true;
        }
        false
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Puts back the faces from before the last stroke.
    pub fn undo(&mut self) -> bool {
        let Some(saved) = self.undo.pop() else {
            return false;
        };
        for (face, old) in saved.into_iter().enumerate() {
            if let Some(old) = old {
                self.faces[face] = old;
                self.dirty[face] = Some(TexelRect {
                    x0: 0,
                    y0: 0,
                    x1: self.n,
                    y1: self.n,
                });
            }
        }
        true
    }

    fn save_face(&mut self, face: usize) {
        if let Some(saved) = &mut self.stroke
            && saved[face].is_none()
        {
            saved[face] = Some(self.faces[face].clone());
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
                let (s, c) = (i as f64 / SAMPLES as f64 * std::f64::consts::TAU).sin_cos();
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

    /// Applies one stamp. Returns the number of texels that it visited.
    pub fn stamp(&mut self, stamp: &Stamp) -> usize {
        let radius = stamp.radius.clamp(1e-6, MAX_BRUSH_RADIUS);
        let mut visited = 0;
        for face in 0..FACES {
            if let Some(rect) = self.stamp_rect(face, stamp.center, radius) {
                self.save_face(face);
                visited += self.stamp_face(face, rect, stamp, radius);
                self.dirty[face] = Some(match self.dirty[face] {
                    Some(d) => d.union(rect),
                    None => rect,
                });
            }
        }
        visited
    }

    fn stamp_face(&mut self, face: usize, rect: TexelRect, stamp: &Stamp, radius: f64) -> usize {
        let n = self.n;
        let axis = face / 2;
        let sign = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
        let c = stamp.center;
        let (cn, cu, cv) = (c[axis] * sign, c[(axis + 1) % 3], c[(axis + 2) % 3]);
        let cos_radius = radius.cos();
        let hard = stamp.hardness.clamp(0.0, 0.999);
        // The smooth mode reads neighbors, so it needs the values from before.
        let reach = ((radius / (std::f64::consts::FRAC_PI_2 / n as f64)) * 0.25).max(1.0) as usize;
        let (bx, by) = (rect.x0.saturating_sub(reach), rect.y0.saturating_sub(reach));
        let (bw, bh) = ((rect.x1 + reach).min(n) - bx, (rect.y1 + reach).min(n) - by);
        let mut before = Vec::new();
        if stamp.mode == Mode::Smooth {
            before.reserve(bw * bh);
            for y in by..by + bh {
                before.extend_from_slice(&self.faces[face][y * n + bx..y * n + bx + bw]);
            }
        }
        let old_at = |x: usize, y: usize| f64::from(before[(y - by) * bw + (x - bx)]);
        let data = &mut self.faces[face];
        for y in rect.y0..rect.y1 {
            let fv = self.flat[y];
            for x in rect.x0..rect.x1 {
                let fu = self.flat[x];
                let cos_angle = (cn + fu * cu + fv * cv) / (1.0 + fu * fu + fv * fv).sqrt();
                if cos_angle <= cos_radius {
                    continue;
                }
                let t = cos_angle.min(1.0).acos() / radius;
                let weight = if t <= hard {
                    1.0
                } else {
                    let k = (t - hard) / (1.0 - hard);
                    1.0 - k * k * (3.0 - 2.0 * k)
                };
                let amount = weight * stamp.flow;
                let i = y * n + x;
                let old = f64::from(data[i]);
                let new = match stamp.mode {
                    Mode::Raise => old + amount * stamp.strength,
                    Mode::Lower => old - amount * stamp.strength,
                    Mode::Flatten => old + (f64::from(stamp.level) - old) * amount.min(1.0),
                    Mode::Smooth => {
                        let (xa, xb) = (x.saturating_sub(reach), (x + reach).min(n - 1));
                        let (ya, yb) = (y.saturating_sub(reach), (y + reach).min(n - 1));
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
                data[i] = new.round().clamp(0.0, 65535.0) as u16;
            }
        }
        (rect.x1 - rect.x0) * (rect.y1 - rect.y0)
    }

    /// Takes the changed texels of each face.
    pub fn take_uploads(&mut self) -> Vec<Upload> {
        let n = self.n;
        let mut out = Vec::new();
        for face in 0..FACES {
            let Some(r) = self.dirty[face].take() else {
                continue;
            };
            let (w, h) = (r.x1 - r.x0, r.y1 - r.y0);
            let mut data = Vec::with_capacity(w * h);
            for y in r.y0..r.y1 {
                data.extend_from_slice(&self.faces[face][y * n + r.x0..y * n + r.x1]);
            }
            out.push(Upload {
                face: face as u32,
                x: r.x0 as u32,
                y: r.y0 as u32,
                width: w as u32,
                height: h as u32,
                data,
            });
        }
        out
    }
}

/// The solid angle of one texel, in steradians.
pub fn texel_solid_angle(map: &Heightmap, x: usize, y: usize) -> f64 {
    // d(flat)/d(index) at the texel center, then the flat-face area factor.
    let n = map.n as f64;
    let (fu, fv) = (map.flat[x], map.flat[y]);
    let step = |f: f64| (1.0 + f * f) * FRAC_PI_4 * 2.0 / n;
    step(fu) * step(fv) / (1.0 + fu * fu + fv * fv).powf(1.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{angle, lonlat_to_dir};

    #[test]
    fn face_round_trip() {
        for &(lon, lat) in &[
            (0.0, 0.0),
            (100.0, 40.0),
            (-170.0, -85.0),
            (45.0, 35.3),
            (10.0, 89.0),
        ] {
            let d = lonlat_to_dir(lon, lat);
            let (face, a, b) = face_of(d);
            assert!(a.abs() <= 1.0 + 1e-12 && b.abs() <= 1.0 + 1e-12);
            assert!(angle(face_dir(face, a, b), d) < 1e-12);
        }
    }

    #[test]
    fn solid_angles_sum_to_the_sphere() {
        let map = Heightmap::new(64, 0);
        let mut sum = 0.0;
        for y in 0..64 {
            for x in 0..64 {
                sum += texel_solid_angle(&map, x, y);
            }
        }
        let sphere = 4.0 * std::f64::consts::PI;
        assert!((sum * 6.0 - sphere).abs() / sphere < 1e-3, "{}", sum * 6.0);
    }

    /// The ground area that one stamp changes, in steradians.
    fn painted_area(center: V3, radius: f64) -> f64 {
        let n = 256;
        let mut map = Heightmap::new(n, 0);
        map.stamp(&Stamp {
            center,
            radius,
            hardness: 1.0,
            flow: 1.0,
            mode: Mode::Raise,
            level: 0,
            strength: 1000.0,
        });
        let mut area = 0.0;
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    if map.face(face)[y * n + x] > 0 {
                        area += texel_solid_angle(&map, x, y);
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
        let cap = std::f64::consts::TAU * (1.0 - radius.cos());
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
    fn undo_puts_back_the_stroke() {
        let mut map = Heightmap::new(64, 100);
        map.begin_stroke();
        map.stamp(&Stamp {
            center: lonlat_to_dir(45.0, 0.0),
            radius: 0.2,
            hardness: 0.5,
            flow: 1.0,
            mode: Mode::Raise,
            level: 0,
            strength: 500.0,
        });
        map.end_stroke();
        assert!(map.sample(lonlat_to_dir(45.0, 0.0)) > 100);
        assert!(map.undo());
        for face in 0..FACES {
            assert!(map.face(face).iter().all(|&v| v == 100));
        }
    }
}
