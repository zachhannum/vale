//! The ocean ridge brush.
//!
//! The brush works on the whole line of a stroke. Each texel near the line
//! gets a place: the distance along the line and the distance across it. The
//! new level is a function of that place, of noise at the direction of the
//! texel, and of the level from before the stroke. Thus a texel takes the
//! ridge one time for each stroke.

use std::f64::consts::FRAC_PI_2;
use std::ops::Range;

use crate::cube::{FACES, face_dir, unwarp};
use crate::heightmap::{Limits, MAX_BRUSH_RADIUS, TexelRect, brush_rect, hash, noise3};
use crate::math::{V3, add, angle, cross, dot, length, normalize, scale};

/// The largest number of segments in one plan.
pub const RIDGE_SEGMENTS: usize = 128;

/// The distance between two points of the line that the app makes, as a part
/// of the brush radius.
pub const RIDGE_SPACING: f64 = 0.25;

/// The largest number of new segments in one plan. With the spacing of the
/// app, they are 4 brush radii of line, so the rectangles of a plan stay
/// small.
const NEW_SEGMENTS: usize = 16;

/// The ridge changes no ground farther from the line than this number of
/// brush radii.
const REACH: f64 = 1.5;

/// The cosine of the sharpest turn that has a miter. At a sharper turn, each
/// of the two segments ends with a round cap.
const SHARP_TURN: f64 = -0.5;

/// The settings of an ocean ridge stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ridge {
    /// The brush radius as an angle, in radians. The rise ends at this
    /// distance from the crest.
    pub radius: f64,
    /// The level of the crest.
    pub crest: u16,
    /// The mean length of a segment of the crest as an angle, in radians.
    pub segment: f64,
    /// The largest shift of a segment across the line as an angle, in
    /// radians.
    pub offset: f64,
    /// The height of the fabric, in 16-bit steps.
    pub roughness: f64,
    /// The depth of the valley along the crest, in 16-bit steps.
    pub valley: f64,
    /// The seed of the pattern.
    pub seed: u32,
    pub limits: Limits,
}

/// One piece of the line between two points, as the shader gets it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RidgeSegment {
    /// The normal of the great circle of the piece. It points to the left of
    /// the direction of the line.
    pub n: V3,
    /// The distance along the line at the start of the piece, in radians.
    pub u0: f64,
    /// The normal of the plane that divides this piece from the piece before
    /// it. It points along the line. At a cap, it is the tangent of the
    /// piece.
    pub m0: V3,
    /// The length of the piece, in radians.
    pub len: f64,
    /// As `m0`, for the plane at the end of the piece.
    pub m1: V3,
    /// The flow at the start and at the end of the piece.
    pub q0: f64,
    pub q1: f64,
    /// The piece has a round cap at its start.
    pub first: bool,
    /// The piece has a round cap at its end.
    pub last: bool,
}

impl RidgeSegment {
    /// The segment as 4 vectors of 4 numbers: `n` and the rest of `u0`,
    /// `m0` and `len`, `m1` and `q0`, then `q1`, the caps, the cell of `u0`,
    /// and a zero. The caps are 1 for `first` plus 2 for `last`.
    ///
    /// A 32-bit number cannot hold a long distance along the line to a part
    /// of a texel step. Thus `u0` goes as the nearest multiple of `segment`,
    /// the length of a crest segment, and the rest. The cell is the number
    /// of that multiple.
    pub fn to_f32(&self, segment: f64) -> [f32; 16] {
        let caps = f64::from(u8::from(self.first) + 2 * u8::from(self.last));
        let cell = (self.u0 / segment + 0.5).floor();
        let rest = self.u0 - cell * segment;
        let [n, m0, m1] = [self.n, self.m0, self.m1];
        [
            n[0], n[1], n[2], rest, m0[0], m0[1], m0[2], self.len, m1[0], m1[1], m1[2], self.q0,
            self.q1, caps, cell, 0.0,
        ]
        .map(|v| v as f32)
    }

    /// The two ends of the piece.
    fn ends(&self) -> (V3, V3) {
        (
            normalize(cross(self.m0, self.n)),
            normalize(cross(self.m1, self.n)),
        )
    }
}

/// The place of a direction on the line of a stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RidgePoint {
    /// The distance along the line, in radians.
    pub u: f64,
    /// The distance across the line, in radians. It is positive on the left.
    pub across: f64,
    /// The distance past the end of the line, in radians. Along the line, it
    /// is zero. On the outer side of a sharp turn, it grows with the distance
    /// from the corner.
    pub past: f64,
    /// The flow, from 0 to 1.
    pub flow: f64,
}

/// What a plan does with a texel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RidgePlace {
    /// The texel is not near the new part of the line. Its level stays.
    Far,
    /// The ridge does not reach the texel. It gets the level from before
    /// the stroke.
    Off,
    On(RidgePoint),
}

#[derive(Clone, Copy, Debug)]
struct LinePoint {
    dir: V3,
    flow: f64,
    /// The distance along the line, in radians.
    u: f64,
}

/// The line of one ocean ridge stroke.
#[derive(Clone, Debug)]
pub struct RidgeLine {
    spec: Ridge,
    face_size: usize,
    points: Vec<LinePoint>,
}

/// The part of the heightmap that changes after new points of a line, and
/// the segments that give the levels there.
#[derive(Clone, Debug)]
pub struct RidgePlan {
    /// The settings, inside their limits.
    pub ridge: Ridge,
    /// The number of texels along one side of a face.
    pub face_size: usize,
    /// The rectangle of each face that holds the texels that can change.
    pub rects: [Option<TexelRect>; FACES],
    /// The new segments, the segment before them, and the nearest other
    /// segments, in the order of the line. There are `RIDGE_SEGMENTS` at
    /// most.
    pub segments: Vec<RidgeSegment>,
    /// The new segments and the segment before them, as a range of
    /// `segments`. Only a texel closer to one of them than 1.5 brush radii
    /// changes.
    pub tail: Range<usize>,
    /// The number of points of the line that the plan covers. If it is less
    /// than the length of the line, make the next plan from it.
    pub end: usize,
}

impl RidgeLine {
    /// Starts a line with no points on a heightmap with `face_size` texels
    /// along one side of a face. The settings go inside their limits.
    pub fn new(spec: Ridge, face_size: usize) -> RidgeLine {
        let step = FRAC_PI_2 / face_size as f64;
        let radius = spec.radius.max(4.0 * step).min(MAX_BRUSH_RADIUS / REACH);
        let spec = Ridge {
            radius,
            segment: spec.segment.max(20.0 * crack_half_width(radius, step)),
            offset: spec.offset.clamp(0.0, 0.5 * radius),
            roughness: spec.roughness.max(0.0),
            valley: spec.valley.max(0.0),
            ..spec
        };
        RidgeLine {
            spec,
            face_size,
            points: Vec::new(),
        }
    }

    /// The settings, inside their limits.
    pub fn spec(&self) -> Ridge {
        self.spec
    }

    /// Adds a point with a flow from 0 to 1. A point closer to the last
    /// point than a quarter of a texel step is not added.
    pub fn push(&mut self, dir: V3, flow: f64) {
        let dir = normalize(dir);
        let u = match self.points.last() {
            Some(last) => {
                let len = angle(last.dir, dir);
                if len < 0.25 * FRAC_PI_2 / self.face_size as f64 {
                    return;
                }
                last.u + len
            }
            None => 0.0,
        };
        let flow = flow.clamp(0.0, 1.0);
        self.points.push(LinePoint { dir, flow, u });
    }

    /// The number of points.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    fn normal(&self, j: usize) -> V3 {
        normalize(cross(self.points[j].dir, self.points[j + 1].dir))
    }

    /// The segment from point `j` to the point after it.
    fn segment(&self, j: usize) -> RidgeSegment {
        let (a, b) = (self.points[j], self.points[j + 1]);
        let n = self.normal(j);
        // The direction of the line at each end.
        let (t0, t1) = (cross(n, a.dir), cross(n, b.dir));
        let before = j.checked_sub(1).map(|i| cross(self.normal(i), a.dir));
        let after = (j + 2 < self.points.len()).then(|| cross(self.normal(j + 1), b.dir));
        let (m0, first) = match before {
            Some(t) if dot(t, t0) >= SHARP_TURN => (normalize(add(t, t0)), false),
            _ => (t0, true),
        };
        let (m1, last) = match after {
            Some(t) if dot(t1, t) >= SHARP_TURN => (normalize(add(t1, t)), false),
            _ => (t1, true),
        };
        RidgeSegment {
            n,
            u0: a.u,
            m0,
            len: b.u - a.u,
            m1,
            q0: a.flow,
            q1: b.flow,
            first,
            last,
        }
    }

    /// The distance from a direction to the segment from point `j`, in
    /// radians.
    fn distance(&self, j: usize, p: V3) -> f64 {
        let (a, b) = (self.points[j].dir, self.points[j + 1].dir);
        let n = self.normal(j);
        if dot(p, cross(n, a)) < 0.0 {
            angle(p, a)
        } else if dot(p, cross(n, b)) > 0.0 {
            angle(p, b)
        } else {
            dot(p, n).clamp(-1.0, 1.0).asin().abs()
        }
    }

    /// The plan for the points from `from` on. `from` is the number of
    /// points that the plans before this one covered: zero, or the `end` of
    /// the last plan. The segment before the new points is in the plan,
    /// because a new point changes the end of that segment. A plan covers 16
    /// new segments at most.
    ///
    /// The plan holds each segment that can reach a texel of its rectangles.
    /// If these are more than `RIDGE_SEGMENTS`, the plan holds the nearest
    /// ones. `GpuHeightmap::ridge` can then lose parts of the ridge of the
    /// segments that are not in the plan.
    pub fn plan(&self, from: usize) -> RidgePlan {
        let count = self.points.len().saturating_sub(1);
        let start = from.saturating_sub(2).min(count);
        self.plan_of(start, (start + NEW_SEGMENTS).min(count))
    }

    /// The difference between the two smallest distances from a unit
    /// direction to the segments of the line, in radians. With less than two
    /// segments near the direction, it is `f64::MAX`. Where it is near zero,
    /// the 32-bit numbers of the shader can give the place on the other
    /// segment. The tests of the shader use it.
    #[doc(hidden)]
    pub fn margin(&self, p: V3) -> f64 {
        let count = self.points.len().saturating_sub(1);
        self.plan_of(0, count).pick(p).1
    }

    /// The plan with the segments from `start` to `stop` as its new part.
    fn plan_of(&self, start: usize, stop: usize) -> RidgePlan {
        let count = self.points.len().saturating_sub(1);
        let n = self.face_size;
        let radius = self.spec.radius;
        let reach = REACH * radius;
        // Places on the new part of the line, with no more than the spacing
        // between two of them.
        let mut samples = Vec::new();
        for j in start..stop {
            let (a, b) = (self.points[j], self.points[j + 1]);
            let len = b.u - a.u;
            let steps = (len / (RIDGE_SPACING * radius)).ceil().max(1.0) as usize;
            let tangent = cross(self.normal(j), a.dir);
            samples.extend((0..steps).map(|i| {
                let (sin, cos) = (len * i as f64 / steps as f64).sin_cos();
                add(scale(a.dir, cos), scale(tangent, sin))
            }));
        }
        if stop > start {
            samples.push(self.points[stop].dir);
        }
        let rects = std::array::from_fn(|face| {
            let rects = samples.iter();
            let rects = rects.filter_map(|&s| brush_rect(n, face, s, 1.02 * reach));
            rects.reduce(TexelRect::union)
        });
        // A circle around each rectangle: its center and its radius. The
        // sides of a rectangle are great circles, so the corner that is
        // farthest from the center gives the radius.
        let circles: Vec<(V3, f64)> = rects
            .iter()
            .enumerate()
            .filter_map(|(face, rect): (usize, &Option<TexelRect>)| {
                let rect = (*rect)?;
                let flat = |x: usize| unwarp(x as f64 / n as f64 * 2.0 - 1.0);
                let corners = [
                    (rect.x0, rect.y0),
                    (rect.x1, rect.y0),
                    (rect.x0, rect.y1),
                    (rect.x1, rect.y1),
                ]
                .map(|(x, y)| face_dir(face, flat(x), flat(y)));
                let center = normalize(corners.into_iter().fold([0.0; 3], add));
                let radius = corners
                    .map(|c| angle(center, c))
                    .into_iter()
                    .fold(0.0, f64::max);
                Some((center, radius))
            })
            .collect();
        // The other segments that can reach a texel of a rectangle, the
        // nearest to the new part first.
        let mut others: Vec<(f64, usize)> = (0..start)
            .chain(stop..count)
            .filter_map(|j| {
                let mut circles = circles.iter();
                let reaches = circles.any(|&(c, r)| self.distance(j, c) < r + 1.01 * reach);
                let distances = samples.iter().map(|&s| self.distance(j, s));
                reaches.then(|| (distances.fold(f64::MAX, f64::min), j))
            })
            .collect();
        others.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        others.truncate(RIDGE_SEGMENTS.saturating_sub(stop - start));
        let mut indices: Vec<usize> = others.into_iter().map(|(_, j)| j).collect();
        indices.extend(start..stop);
        indices.sort_unstable();
        let tail = indices.iter().position(|&j| j == start).unwrap_or(0);
        RidgePlan {
            ridge: self.spec,
            face_size: n,
            rects,
            segments: indices.into_iter().map(|j| self.segment(j)).collect(),
            tail: tail..tail + (stop - start),
            end: if stop < count {
                stop + 1
            } else {
                self.points.len()
            },
        }
    }
}

impl RidgePlan {
    /// The texel step at the center of a face, in radians.
    fn step(&self) -> f64 {
        FRAC_PI_2 / self.face_size as f64
    }

    /// The place of a unit direction on the line. The segment with the
    /// smallest distance to the direction gives the place. With two equal
    /// distances, the first segment gives it.
    pub(crate) fn place(&self, p: V3) -> RidgePlace {
        self.pick(p).0
    }

    /// The place of a direction, and the result of `RidgeLine::margin`.
    fn pick(&self, p: V3) -> (RidgePlace, f64) {
        let reach = REACH * self.ridge.radius;
        let sin_reach = reach.sin();
        let mut near = false;
        let mut best = None;
        let mut best_distance = reach;
        let mut second_distance = f64::MAX;
        for (i, s) in self.segments.iter().enumerate() {
            let side = dot(p, s.n);
            if side.abs() >= sin_reach {
                continue;
            }
            let across = side.asin();
            // The distance to the nearest point of the segment.
            let (a, b) = s.ends();
            let to = |end: V3| 2.0 * (0.5 * length(add(p, scale(end, -1.0)))).min(1.0).asin();
            let nearest = if dot(p, cross(s.n, a)) < 0.0 {
                to(a)
            } else if dot(p, cross(s.n, b)) > 0.0 {
                to(b)
            } else {
                across.abs()
            };
            if nearest >= reach {
                continue;
            }
            near |= self.tail.contains(&i);
            let (da, db) = (dot(p, s.m0), -dot(p, s.m1));
            // The part of the segment before the direction, and the
            // distance to the line. At a miter, the distance is the one to
            // the great circle. At the reach, it becomes the distance to the
            // corner, so the outer side of a turn has an end.
            let (along, distance) = if s.first && da < 0.0 {
                (0.0, nearest)
            } else if s.last && db < 0.0 {
                (1.0, nearest)
            } else if da >= 0.0 && db >= 0.0 {
                let along = if da + db > 0.0 { da / (da + db) } else { 0.0 };
                (along, across.abs().max(nearest * nearest / reach))
            } else {
                continue;
            };
            if distance >= best_distance {
                second_distance = second_distance.min(distance);
                continue;
            }
            if best.is_some() {
                second_distance = best_distance;
            }
            {
                best_distance = distance;
                best = Some(RidgePoint {
                    u: s.u0 + along * s.len,
                    across,
                    past: (distance * distance - across * across).max(0.0).sqrt(),
                    flow: s.q0 + (s.q1 - s.q0) * along,
                });
            }
        }
        let place = match best {
            _ if !near => RidgePlace::Far,
            Some(point) => RidgePlace::On(point),
            None => RidgePlace::Off,
        };
        let two = second_distance < f64::MAX && best.is_some();
        (
            place,
            if two {
                second_distance - best_distance
            } else {
                f64::MAX
            },
        )
    }

    /// The noise at a unit direction.
    pub(crate) fn noise(&self, p: V3) -> RidgeNoise {
        let (r, seed) = (self.ridge.radius, self.ridge.seed);
        let at = |size: f64, salt: u32| noise3(p.map(|c| c / (size * r)), seed ^ salt);
        let (wide, fine) = (at(BEND_SIZE, SALT_BEND), at(BEND_FINE_SIZE, SALT_BEND_FINE));
        RidgeNoise {
            bend: add(wide, scale(fine, 0.5)),
            slow: at(SLOW_SIZE, SALT_SLOW),
            mid: at(MID_SIZE, SALT_MID),
        }
    }

    /// The move of boundary `i` along the line, as a part of the segment
    /// length.
    fn jitter(&self, i: i32) -> f64 {
        JITTER * (2.0 * unit(hash(i as u32, 0, 0, self.ridge.seed)) - 1.0)
    }

    /// The number of the boundary that starts a group of crest segments, for
    /// group `c`. A group has 4 segments as a mean.
    fn group_start(&self, c: i32) -> i32 {
        let h = hash(c as u32, 1, 0, self.ridge.seed);
        c.wrapping_mul(4).wrapping_add((h & 3) as i32)
    }

    /// The strength of the crack at boundary `i`, from 0 to 1. The boundary
    /// that starts a group has a strong crack.
    fn strength(&self, i: i32) -> f64 {
        let x = unit(hash(i as u32, 2, 0, self.ridge.seed));
        if i == self.group_start(i >> 2) {
            0.7 + 0.3 * x
        } else {
            0.1 + 0.4 * x * x
        }
    }

    /// The shift of crest segment `k` across the line.
    fn shift(&self, k: i32) -> f64 {
        let seed = self.ridge.seed;
        let c = k >> 2;
        let group = if k >= self.group_start(c) { c } else { c - 1 };
        let part = |i: i32, channel: u32| 2.0 * unit(hash(i as u32, channel, 0, seed)) - 1.0;
        self.ridge.offset * (0.75 * part(group, 3) + 0.25 * part(k, 4))
    }

    /// The level of a texel on the ridge. `start` is its level from before
    /// the stroke, and `noise` is the noise at its place.
    pub(crate) fn level(&self, start: u16, at: RidgePoint, noise: RidgeNoise) -> u16 {
        let spec = &self.ridge;
        let (r, t, seed) = (spec.radius, self.step(), spec.seed);
        let old = f64::from(start);
        let from_line = at.across.hypot(at.past);
        // The noise bends the two distances. Near the reach, the distance
        // across has no bend, so the ridge ends there.
        let hold = 1.0 - smoothstep(r, REACH * r, from_line);
        let u = at.u + BEND_ALONG * r * noise.bend[0];
        let across = at.across + BEND_ACROSS * r * hold * noise.bend[1];
        // The crest has a boundary near each multiple of the segment length.
        // `s` is the distance from the nearest boundary, `k` is the number
        // of the crest segment, and `along` is the distance from the start
        // of that segment.
        let cell = (u / spec.segment + 0.5).floor();
        let i = cell as i32;
        let jitter = |i: i32| self.jitter(i);
        let s = u - (cell + jitter(i)) * spec.segment;
        let (k, along, len) = if s < 0.0 {
            let len = spec.segment * (1.0 + jitter(i) - jitter(i.wrapping_sub(1)));
            (i.wrapping_sub(1), s + len, len)
        } else {
            let len = spec.segment * (1.0 + jitter(i.wrapping_add(1)) - jitter(i));
            (i, s, len)
        };
        // The strength of the crack at each end of the crest segment, and
        // the part of it that the place gets: all at that end, and nothing
        // from the middle of the segment on.
        let part = (along / len).clamp(0.0, 1.0);
        let end = |x: f64, i: i32| {
            let near = (1.0 - 2.0 * x).max(0.0);
            let strength = self.strength(i);
            near * near * strength * strength
        };
        let ends = end(part, k) + end(1.0 - part, k.wrapping_add(1));
        let strength = self.strength(i);
        let half = crack_half_width(r, t) * (1.0 + 0.5 * strength);
        // The crest steps sideways at each boundary. Near the crest, the
        // step is as narrow as a crack. On the outer flank, it is wide, so
        // the outline of the rise has a bend there and no cut. The zone of a
        // step is shorter than 0.9 of the segment length on each side, so
        // the steps of 3 boundaries can reach a place.
        let zone = crack_half_width(r, t) + STEP_SPREAD * (from_line - spec.offset).max(0.0);
        let zone = zone.min(0.9 * spec.segment);
        let mut shift = self.shift(i.wrapping_sub(2));
        for near in -1..=1 {
            let j = i.wrapping_add(near);
            let from = s + (jitter(i) - f64::from(near) - jitter(j)) * spec.segment;
            let step = self.shift(j) - self.shift(j.wrapping_sub(1));
            shift += step * smoothstep(-zone, zone, from);
        }
        let crack_mask = 1.0 - smoothstep(0.0, half, s.abs());
        // The distance from the crest of the segment.
        let off = across - shift;
        let from_crest = off.hypot(at.past);
        // The height of the crest, and the width of the rise on each side.
        // The first number is for the height, and the other two are for the
        // width on the left and on the right.
        let slow = noise.slow.map(|v| smoothstep(-0.6, 0.6, v));
        let mid = noise.mid.map(|v| smoothstep(-0.6, 0.6, v));
        let less: [f64; 3] = std::array::from_fn(|c| 0.5 * slow[c] + 0.5 * mid[c]);
        let height = 1.0 - CREST_DROP * less[0];
        let side = if from_crest > 0.0 {
            off / from_crest
        } else {
            0.0
        };
        let narrow = 0.5 * (less[1] + less[2]) + 0.5 * (less[1] - less[2]) * side;
        let width = r * (1.0 - NARROW * narrow);
        // A valley has a flat top of the rise across its width, so its rims
        // are at the level of the crest.
        let valley_width = (0.05 * r * (1.0 + 0.4 * noise.bend[2])).max(2.0 * t);
        let top = if spec.valley > 0.0 { valley_width } else { 0.0 };
        // A square root has a sharp point at zero. `round` makes it round,
        // and the ratio keeps the end of the rise at the width.
        let e = 2.0 * t;
        let round = |d: f64| d * d / ((d * d + e * e).sqrt() + e);
        let drop = round((from_crest - top).max(0.0)) / round(width - top);
        let w = (1.0 - drop.sqrt()).max(0.0);
        // The foot of the rise has no sharp edge.
        let w = w * w * (1.0 + FOOT) / (w + FOOT);
        let lift = (f64::from(spec.crest) - old).max(0.0);
        // Ground at the crest level gets no fabric and no valley.
        let rough = (1.0 + FINGER_HEIGHT) * spec.roughness;
        let low = (lift / (rough + spec.valley + 1.0)).min(1.0);
        let rise = lift * height * w;
        let deep = (0.6 + 0.4 * ends) * (0.75 + noise.bend[2] / 6.0);
        let valley = spec.valley * low * deep * (1.0 - smoothstep(0.0, valley_width, from_crest));
        // The fabric: ridged noise in cells that are long along the line.
        // Each octave has cells of two times the size of the octave before.
        let finest = fabric_step(r, t);
        // The largest octave comes first, and the noise of an octave moves
        // the next one across the line.
        let (mut sum, mut total, mut moved) = (0.0, 0.0, 0.0);
        for (octave, (weight, stretch)) in FABRIC_OCTAVES.into_iter().enumerate().rev() {
            let size = finest * f64::from(1 << octave);
            if octave > 0 && size > FABRIC_LARGEST * r {
                continue;
            }
            let channel = (k as u32).wrapping_mul(4).wrapping_add(octave as u32);
            let n = value2(
                along / (stretch * size),
                off / size + moved,
                channel,
                seed ^ SALT_FABRIC,
            );
            moved = FABRIC_MOVE * (2.0 * n - 1.0);
            let ridged = 1.0 - (2.0 * n - 1.0).abs();
            sum += weight * ridged * ridged;
            total += weight;
        }
        let detail = (sum / total - FABRIC_MEAN) * (1.0 - crack_mask);
        // The fingers: one more octave with large cells. Its pattern does
        // not break at a crack.
        let size = FINGER_SIZE * r;
        let fingers = if size >= 2.0 * finest {
            let n = value2(
                u / (FINGER_STRETCH * size),
                off / size,
                0,
                seed ^ SALT_FINGER,
            );
            let ridged = 1.0 - (2.0 * n - 1.0).abs();
            FINGER_HEIGHT * (ridged * ridged - FABRIC_MEAN)
        } else {
            0.0
        };
        // The fabric is zero at the edge of the rise and in a round end of
        // the line.
        let x = (from_crest / width).min(1.0);
        let cap = 1.0 - smoothstep(0.0, 0.5 * width, at.past);
        let fabric = (detail * (1.0 - x * x) + fingers * (1.0 - x * x * x)) * cap;
        let hills = spec.roughness * low * fabric;
        // A strong crack is deep and goes far. A weak crack ends on the
        // flank.
        let end = r * (0.7 + 0.8 * strength);
        let depth = (0.01 + 0.32 * strength * strength) * lift * (0.35 + 0.65 * w);
        let crack = depth * crack_mask * (1.0 - smoothstep(0.4 * end, end, from_line));
        let q = at.flow;
        let change = (q * (rise + hills - valley)).max(0.0) - q * crack;
        let new = (old + (change + 0.5).floor()).clamp(0.0, 65535.0);
        spec.limits.apply(old, new) as u16
    }
}

/// The noise at the place of a texel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct RidgeNoise {
    /// Three numbers from -1.5 to 1.5: noise with cells of 0.9 of the brush
    /// radius, plus half of noise with cells of 0.37 of the brush radius.
    pub bend: V3,
    /// Three numbers from -1 to 1. They change over about two brush radii.
    pub slow: V3,
    /// Three numbers from -1 to 1. They change over about half of a brush
    /// radius.
    pub mid: V3,
}

/// The sizes of the cells of the noise, as parts of the brush radius.
const BEND_SIZE: f64 = 0.9;
const BEND_FINE_SIZE: f64 = 0.37;
const SLOW_SIZE: f64 = 1.7;
const MID_SIZE: f64 = 0.55;

/// The largest bend of the distance along the line and across it is 1.5
/// times this part of the brush radius.
const BEND_ALONG: f64 = 0.08;
const BEND_ACROSS: f64 = 0.08;

/// The seed of each noise is the seed of the pattern with these bits
/// changed.
const SALT_BEND: u32 = 0x51ed_270b;
const SALT_BEND_FINE: u32 = 0x2f6b_8a35;
const SALT_SLOW: u32 = 0x7a3c_91d7;
const SALT_MID: u32 = 0x4cf5_ad43;
const SALT_FABRIC: u32 = 0x1b87_3593;
const SALT_FINGER: u32 = 0x6d2b_79f5;

/// The largest move of a boundary, as a part of the segment length.
const JITTER: f64 = 0.35;

/// The crest is lower than the Crest setting by up to this part of the rise.
const CREST_DROP: f64 = 0.25;

/// A flank is narrower than the brush radius by up to this part.
const NARROW: f64 = 0.45;

/// The zone of a step of the crest gets wider by this part of the distance
/// from the line past the Offset setting.
const STEP_SPREAD: f64 = 0.6;

/// The cell of the fingers across the line, as a part of the brush radius,
/// and the number of times that the cell is longer along the line.
const FINGER_SIZE: f64 = 0.22;
const FINGER_STRETCH: f64 = 5.0;

/// The height of the fingers, as a part of the Roughness setting.
const FINGER_HEIGHT: f64 = 1.5;

/// The octaves of the fabric, from the finest one. Each has a weight, and
/// the number of times that its cell is longer along the line than across
/// it.
const FABRIC_OCTAVES: [(f64, f64); 3] = [(0.8, 8.0), (1.0, 5.0), (0.8, 3.0)];

/// The noise of an octave of the fabric moves the next octave by up to this
/// number of cells.
const FABRIC_MOVE: f64 = 0.5;

/// The foot of the rise is round below about this part of the height.
const FOOT: f64 = 0.06;

/// The largest cell of the fabric, as a part of the brush radius. The finest
/// octave has no such limit.
const FABRIC_LARGEST: f64 = 0.15;

/// The mean of the ridged noise, about.
const FABRIC_MEAN: f64 = 0.45;

/// Half of the width of the weakest crack before the bend of the noise, in
/// radians.
fn crack_half_width(radius: f64, step: f64) -> f64 {
    (0.02 * radius).max(3.0 * step)
}

/// The size of the finest cell of the fabric across the line before the bend
/// of the noise, in radians.
fn fabric_step(radius: f64, step: f64) -> f64 {
    (0.02 * radius).max(7.0 * step)
}

/// A number from 0 to 1 from the low 24 bits of a hash. It is exact as a
/// 32-bit number.
fn unit(h: u32) -> f64 {
    f64::from(h & 0xff_ffff) / f64::from(1u32 << 24)
}

fn mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Noise from 0 to 1 at a point of a plane. Each lattice point has a number
/// from `hash`, and the noise changes smoothly between the 4 lattice points
/// around the point.
fn value2(x: f64, y: f64, channel: u32, seed: u32) -> f64 {
    let (cx, cy) = (x.floor(), y.floor());
    let (ix, iy) = (cx as i32 as u32, cy as i32 as u32);
    let ease = |t: f64| t * t * (3.0 - 2.0 * t);
    let (tx, ty) = (ease(x - cx), ease(y - cy));
    let at = |dx: u32, dy: u32| {
        unit(hash(
            ix.wrapping_add(dx),
            iy.wrapping_add(dy),
            channel,
            seed,
        ))
    };
    mix(mix(at(0, 0), at(1, 0), tx), mix(at(0, 1), at(1, 1), tx), ty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heightmap::Heightmap;
    use crate::math::lonlat_to_dir;

    const START: u16 = 10000;
    const CREST: u16 = 30000;
    const LIFT: f64 = 20000.0;
    /// The face size of the tests of the formula. The texel step is small,
    /// so the round parts of the formula are small.
    const FINE: usize = 4096;

    fn spec() -> Ridge {
        Ridge {
            radius: 0.1,
            crest: CREST,
            segment: 0.1,
            offset: 0.0,
            roughness: 0.0,
            valley: 0.0,
            seed: 7,
            limits: Limits::default(),
        }
    }

    /// A plan with no segments, for the formula.
    fn shape(spec: Ridge, n: usize) -> RidgePlan {
        RidgeLine::new(spec, n).plan(0)
    }

    fn at(u: f64, across: f64) -> RidgePoint {
        RidgePoint {
            u,
            across,
            past: 0.0,
            flow: 1.0,
        }
    }

    /// Noise with no bend. It gives the widest rise and the highest crest.
    fn calm() -> RidgeNoise {
        RidgeNoise {
            bend: [0.0; 3],
            slow: [-1.0; 3],
            mid: [-1.0; 3],
        }
    }

    /// Noise with no bend. It gives the narrowest rise and the lowest crest.
    fn narrow() -> RidgeNoise {
        RidgeNoise {
            bend: [0.0; 3],
            slow: [1.0; 3],
            mid: [1.0; 3],
        }
    }

    /// The level at a place with the calm noise.
    fn level(plan: &RidgePlan, u: f64, across: f64) -> f64 {
        f64::from(plan.level(START, at(u, across), calm()))
    }

    /// The level at a place with the noise of that place. The line is the
    /// equator, from longitude 0 to the east.
    fn real(plan: &RidgePlan, u: f64, across: f64) -> f64 {
        let p = lonlat_to_dir(u.to_degrees(), across.to_degrees());
        f64::from(plan.level(START, at(u, across), plan.noise(p)))
    }

    /// The place of boundary `i` along the line, with no bend.
    fn boundary(plan: &RidgePlan, i: i32) -> f64 {
        (f64::from(i) + plan.jitter(i)) * plan.ridge.segment
    }

    /// The middle of crest segment `k`.
    fn middle(plan: &RidgePlan, k: i32) -> f64 {
        0.5 * (boundary(plan, k) + boundary(plan, k + 1))
    }

    /// The width of the rise on one side of the crest: the first distance
    /// with no rise.
    fn width(plan: &RidgePlan, noise: RidgeNoise, u: f64, side: f64) -> f64 {
        let r = plan.ridge.radius;
        let mut places = (1..=3000).map(|i| r * f64::from(i) / 2000.0);
        let end = places.find(|a| plan.level(START, at(u, side * a), noise) == START);
        end.unwrap()
    }

    #[test]
    fn the_rise_follows_the_square_root_and_ends_at_its_width() {
        let plan = shape(spec(), FINE);
        let r = plan.ridge.radius;
        for k in 1..12 {
            let u = middle(&plan, k);
            // With the calm noise, the crest has the Crest setting.
            let top = level(&plan, u, 0.0);
            assert_eq!(top, f64::from(CREST), "{k}");
            for side in [-1.0, 1.0] {
                let width = width(&plan, calm(), u, side);
                assert!(width > 0.99 * r && width <= r, "{k}: {width}");
                let mut last = top;
                for i in 1..=200 {
                    let across = side * width * f64::from(i) / 200.0;
                    let new = level(&plan, u, across);
                    let drop = (top - new) / (top - f64::from(START));
                    let root = (f64::from(i) / 200.0).sqrt();
                    assert!((drop - root).abs() < 0.06, "{k} {i}: {drop}");
                    assert!(new <= last, "{k} {i}");
                    last = new;
                }
                assert!(level(&plan, u, side * 0.97 * width) > f64::from(START));
            }
            for across in [r, 1.2 * r, -r, -1.49 * r] {
                assert_eq!(level(&plan, u, across), f64::from(START), "{across}");
            }
            // With the other noise, a flank has 0.55 of the radius or more,
            // and the crest is lower by a quarter of the rise at most.
            let low = f64::from(plan.level(START, at(u, 0.0), narrow()));
            let most = f64::from(CREST) - 0.25 * LIFT;
            assert!((low - most).abs() <= 1.0, "{k}: {low}");
            for side in [-1.0, 1.0] {
                let width = width(&plan, narrow(), u, side);
                assert!(width > 0.54 * r && width < 0.65 * r, "{k}: {width}");
            }
        }
    }

    #[test]
    fn the_noise_changes_the_height_and_the_width_along_the_line() {
        let plan = shape(spec(), FINE);
        let r = plan.ridge.radius;
        let (mut tops, mut left, mut right) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..400 {
            let u = f64::from(i) * 0.01;
            let across = (-300..=300).map(|j| r * f64::from(j) / 300.0);
            let levels: Vec<f64> = across.map(|a| real(&plan, u, a)).collect();
            let top = levels.iter().copied().fold(f64::MIN, f64::max);
            assert!(top <= f64::from(CREST), "{u}: {top}");
            let crest = levels.iter().position(|l| *l == top).unwrap();
            let risen = |l: &&f64| **l > f64::from(START);
            tops.push(top);
            left.push(levels[crest..].iter().take_while(risen).count() as f64 / 300.0);
            right.push(levels[..crest].iter().rev().take_while(risen).count() as f64 / 300.0);
        }
        let range = |values: &[f64]| {
            let low = values.iter().copied().fold(f64::MAX, f64::min);
            let high = values.iter().copied().fold(f64::MIN, f64::max);
            (low, high)
        };
        // The crest is lower in places. In a strong crack, it is lower by
        // 0.58 of the rise at most.
        let (low, high) = range(&tops);
        assert!(high > f64::from(CREST) - 0.1 * LIFT, "{high}");
        assert!(low < f64::from(CREST) - 0.2 * LIFT, "{low}");
        assert!(low > f64::from(CREST) - 0.6 * LIFT, "{low}");
        // The two flanks have other widths.
        for widths in [&left, &right] {
            let (low, high) = range(widths);
            assert!(low > 0.3 && low < 0.75, "{low}");
            assert!(high > 0.85 && high <= 1.1, "{high}");
        }
        let apart = left
            .iter()
            .zip(&right)
            .filter(|(a, b)| (*a - *b).abs() > 0.1);
        assert!(apart.count() > 100);
    }

    #[test]
    fn the_noise_bends_the_crest() {
        let plan = shape(spec(), FINE);
        let r = plan.ridge.radius;
        let step = r / 1000.0;
        // The place of the crest in the middle half of a crest segment.
        let mut most: f64 = 0.0;
        for k in 1..30 {
            let (from, to) = (boundary(&plan, k), boundary(&plan, k + 1));
            let crests: Vec<f64> = (0..=20)
                .map(|i| {
                    let u = mix(from, to, 0.25 + 0.5 * f64::from(i) / 20.0);
                    let places = (-150..=150).map(|j| f64::from(j) * step);
                    let highest =
                        places.max_by(|a, b| real(&plan, u, *a).total_cmp(&real(&plan, u, *b)));
                    highest.unwrap()
                })
                .collect();
            let low = crests.iter().copied().fold(f64::MAX, f64::min);
            let high = crests.iter().copied().fold(f64::MIN, f64::max);
            // The bend is 0.12 of the radius at most.
            assert!(low >= -0.121 * r && high <= 0.121 * r, "{k}: {low} {high}");
            most = most.max(high - low);
        }
        assert!(most > 0.02 * r, "{most}");
    }

    #[test]
    fn the_flow_scales_the_rise() {
        let plan = shape(spec(), FINE);
        let u = middle(&plan, 1);
        for across in [0.0, 0.01, 0.03, 0.08] {
            let rise = |flow: f64| {
                let point = RidgePoint {
                    flow,
                    ..at(u, across)
                };
                f64::from(plan.level(START, point, calm())) - f64::from(START)
            };
            assert!(rise(1.0) > 0.0);
            assert!((rise(0.5) - 0.5 * rise(1.0)).abs() <= 1.0, "{across}");
            assert_eq!(rise(0.0), 0.0);
        }
    }

    /// Noise with a bend and with other numbers for the height and the
    /// widths.
    fn wild() -> RidgeNoise {
        RidgeNoise {
            bend: [1.5, -1.5, 1.5],
            slow: [1.0, -1.0, 0.3],
            mid: [-0.5, 1.0, 0.2],
        }
    }

    #[test]
    fn the_ridge_gives_the_higher_of_the_ground_and_the_cross_section() {
        let rough = Ridge {
            offset: 0.03,
            roughness: 2000.0,
            valley: 3000.0,
            ..spec()
        };
        let plan = shape(rough, FINE);
        let r = plan.ridge.radius;
        let half = 1.5 * crack_half_width(r, plan.step());
        let top = f64::from(CREST) + 0.55 * (1.0 + FINGER_HEIGHT) * rough.roughness;
        let mut away = 0;
        for noise in [calm(), narrow(), wild(), RidgeNoise::default()] {
            for j in 0..60 {
                let u = f64::from(j) * 0.0537;
                // The distance from the nearest boundary, with the bend.
                let bent = u + BEND_ALONG * r * noise.bend[0];
                let i = (bent / rough.segment + 0.5).floor() as i32;
                let from_crack = (bent - boundary(&plan, i)).abs();
                for i in 0..=300 {
                    let across = r * (f64::from(i) / 100.0 - 1.5);
                    // Ground at the crest level or above it does not change,
                    // also in a crack.
                    for start in [CREST, CREST + 1, CREST + 5000] {
                        let new = plan.level(start, at(u, across), noise);
                        assert_eq!(new, start, "{u} {across}");
                    }
                    let new = f64::from(plan.level(START, at(u, across), noise));
                    assert!(new <= top, "{u} {across}");
                    // Away from a crack, the ground does not go down.
                    if from_crack > half {
                        away += 1;
                        assert!(new >= f64::from(START), "{u} {across}");
                    }
                    // Ground a small distance below the crest does not go
                    // above the crest by the full height of the fabric.
                    let near = plan.level(CREST - 10, at(u, across), noise);
                    assert!((CREST - 14..=CREST + 10).contains(&near), "{u} {across}");
                }
            }
        }
        assert!(away > 40000, "{away}");
    }

    #[test]
    fn each_segment_of_the_crest_has_its_own_shift() {
        let stairs = Ridge {
            offset: 0.04,
            ..spec()
        };
        let plan = shape(stairs, FINE);
        let (r, segment) = (plan.ridge.radius, stairs.segment);
        // The mean distance between two boundaries is the segment length,
        // and the distances are not equal.
        let gaps: Vec<f64> = (0..400)
            .map(|i| boundary(&plan, i + 1) - boundary(&plan, i))
            .collect();
        let mean = gaps.iter().sum::<f64>() / 400.0;
        assert!((mean / segment - 1.0).abs() < 0.01, "{mean}");
        let low = gaps.iter().copied().fold(f64::MAX, f64::min);
        let high = gaps.iter().copied().fold(f64::MIN, f64::max);
        assert!(low > 0.29 * segment && low < 0.6 * segment, "{low}");
        assert!(high < 1.71 * segment && high > 1.4 * segment, "{high}");
        // The place of the crest is the same along a segment, and it is no
        // farther from the line than the offset.
        let step = r / 500.0;
        let crest_at = |u: f64| {
            let places = (-500..=500).map(|i| f64::from(i) * step);
            let highest = places.max_by(|a, b| level(&plan, u, *a).total_cmp(&level(&plan, u, *b)));
            highest.unwrap()
        };
        let shifts: Vec<f64> = (0..100).map(|k| plan.shift(k)).collect();
        for (k, &shift) in shifts.iter().enumerate() {
            let k = k as i32;
            let (from, to) = (boundary(&plan, k), boundary(&plan, k + 1));
            for part in [0.3, 0.5, 0.7] {
                let crest = crest_at(mix(from, to, part));
                assert!((crest - shift).abs() <= step, "{k}: {crest} {shift}");
            }
            assert!(shift.abs() <= stairs.offset, "{shift}");
        }
        let highest = shifts.iter().copied().fold(f64::MIN, f64::max);
        let lowest = shifts.iter().copied().fold(f64::MAX, f64::min);
        assert!(highest > 0.6 * stairs.offset && lowest < -0.6 * stairs.offset);
        // The step of the crest at a strong crack is larger as a mean.
        let (mut strong, mut weak) = (Vec::new(), Vec::new());
        for i in 1..100 {
            let step = (shifts[i] - shifts[i - 1]).abs();
            assert!(step > 0.0);
            if plan.strength(i as i32) >= 0.7 {
                strong.push(step);
            } else {
                weak.push(step);
            }
        }
        let mean = |steps: &[f64]| steps.iter().sum::<f64>() / steps.len() as f64;
        assert!(mean(&strong) > 2.0 * mean(&weak));
        // At the crest, the step is as narrow as the crack. At the edge of
        // the rise, a small part of the step is in that distance, so the
        // outline of the rise has no cut.
        let near = 2.0 * crack_half_width(r, plan.step());
        let mut large = 0;
        for i in 1..100 {
            let step = shifts[i] - shifts[i - 1];
            if step.abs() < 0.3 * stairs.offset {
                continue;
            }
            large += 1;
            let u = boundary(&plan, i as i32);
            let crest = crest_at(u + near) - crest_at(u - near);
            assert!((crest - step).abs() <= 0.02 * step.abs() + 2.0 * r / 500.0);
            let edge = width(&plan, calm(), u + near, 1.0) - width(&plan, calm(), u - near, 1.0);
            assert!(edge.abs() < 0.4 * step.abs(), "{i}: {edge} {step}");
        }
        assert!(large > 10);
        // An offset above half of the radius has the effect of that half.
        let wide = RidgeLine::new(
            Ridge {
                offset: 1.0,
                ..spec()
            },
            FINE,
        );
        assert_eq!(wide.spec().offset, 0.5 * r);
    }

    #[test]
    fn a_few_cracks_are_strong_and_most_are_weak() {
        for seed in [7, 8, 9] {
            let plan = shape(Ridge { seed, ..spec() }, FINE);
            let strengths: Vec<f64> = (-200..200).map(|i| plan.strength(i)).collect();
            assert!(strengths.iter().all(|s| (0.1..=1.0).contains(s)));
            let strong = strengths.iter().filter(|s| **s >= 0.7).count();
            assert_eq!(strong, 100);
            let faint = strengths.iter().filter(|s| **s < 0.25).count();
            assert!(faint > 150, "{faint}");
            // There are 7 weak cracks at most between two strong ones, and
            // the numbers are not equal.
            let places = (0..400).filter(|i| strengths[*i] >= 0.7);
            let places: Vec<usize> = places.collect();
            let gaps: Vec<usize> = places.windows(2).map(|w| w[1] - w[0]).collect();
            assert!(gaps.iter().all(|gap| (1..=7).contains(gap)));
            for gap in 1..=7 {
                assert!(gaps.contains(&gap), "{seed}: {gap}");
            }
        }
    }

    #[test]
    fn a_crack_is_at_each_boundary_and_a_strong_one_continues_past_the_rise() {
        let plan = shape(spec(), FINE);
        let r = plan.ridge.radius;
        let (mut strong, mut weak) = (0, 0);
        for i in 1..60 {
            let u = boundary(&plan, i);
            let strength = plan.strength(i);
            // The crest is lower in the crack than on the two sides.
            let side = 2.0 * crack_half_width(r, plan.step()) * (1.0 + 0.5 * strength);
            let center = level(&plan, u, 0.0);
            let sides = 0.5 * (level(&plan, u - side, 0.0) + level(&plan, u + side, 0.0));
            assert!(center < sides - 0.009 * LIFT, "{i}: {center} {sides}");
            for side in [-1.0, 1.0] {
                if strength >= 0.7 {
                    assert!(level(&plan, u, side * r) < f64::from(START) - 100.0);
                    assert!(level(&plan, u, side * 1.2 * r) < f64::from(START));
                    assert!(level(&plan, u, side * 1.499 * r) >= f64::from(START) - 1.0);
                    strong += 1;
                } else {
                    // A weak crack ends at 1.1 times the radius or before.
                    assert_eq!(level(&plan, u, side * 1.1 * r), f64::from(START));
                    weak += 1;
                }
            }
        }
        assert!(strong > 10 && weak > 40, "{strong} {weak}");
    }

    /// The widths of the cracks along the line at a distance across it, in
    /// texel steps. A crack that starts before the line is not in the list.
    fn crack_widths(plan: &RidgePlan, across: f64, len: f64, noise: bool) -> Vec<f64> {
        let step = plan.step();
        let fine = step / 32.0;
        let mut widths = Vec::new();
        let mut run = 0;
        let mut first = true;
        for i in 0..(len / fine) as usize {
            let u = i as f64 * fine;
            let new = if noise {
                real(plan, u, across)
            } else {
                level(plan, u, across)
            };
            if new < f64::from(START) {
                run += 1;
            } else if run > 0 {
                if !first || i > run {
                    widths.push(f64::from(run as u32) * fine / step);
                }
                first = false;
                run = 0;
            }
        }
        widths
    }

    #[test]
    fn a_crack_is_4_texels_wide_at_a_small_radius() {
        let step = FRAC_PI_2 / FINE as f64;
        let small = Ridge {
            radius: 20.0 * step,
            segment: 0.0,
            ..spec()
        };
        let plan = shape(small, FINE);
        // The segments are 60 texels long or more.
        assert_eq!(plan.ridge.segment, 60.0 * step);
        // With no bend, a strong crack is 8 to 9 texels wide. The outer
        // part of each side is less than half a level deep, and more of a
        // crack that ends near this distance.
        let widths = crack_widths(&plan, 1.05 * plan.ridge.radius, 4000.0 * step, false);
        assert!(widths.len() > 8, "{widths:?}");
        assert!(widths.iter().all(|w| (4.9..=9.0).contains(w)), "{widths:?}");
        // The bend of the noise makes a crack narrower in places.
        for across in [-1.2, -1.05, 1.05, 1.2] {
            let across = across * plan.ridge.radius;
            let widths = crack_widths(&plan, across, 4000.0 * step, true);
            assert!(widths.len() > 8, "{widths:?}");
            assert!(widths.iter().all(|w| *w >= 4.0), "{widths:?}");
        }
    }

    /// The change that the fabric makes at places across the line.
    fn hills(
        rough: &RidgePlan,
        smooth: &RidgePlan,
        noise: Option<RidgeNoise>,
        u: f64,
        places: &[f64],
    ) -> Vec<f64> {
        let hills = places.iter();
        hills
            .map(|&across| match noise {
                Some(noise) => {
                    let level = |plan: &RidgePlan| plan.level(START, at(u, across), noise);
                    f64::from(level(rough)) - f64::from(level(smooth))
                }
                None => real(rough, u, across) - real(smooth, u, across),
            })
            .collect()
    }

    #[test]
    fn the_fabric_runs_along_a_segment_and_breaks_at_a_crack() {
        let rough = Ridge {
            roughness: 2000.0,
            ..spec()
        };
        let (rough, smooth) = (shape(rough, FINE), shape(spec(), FINE));
        let r = rough.ridge.radius;
        let cell = fabric_step(r, rough.step());
        let inside: Vec<f64> = (-180..=180).map(|i| r * f64::from(i) / 200.0).collect();
        let outside: Vec<f64> = (200..=300).map(|i| r * f64::from(i) / 200.0).collect();
        let mean = |a: &[f64], b: &[f64]| {
            let sum: f64 = a.iter().zip(b).map(|(a, b)| (a - b).abs()).sum();
            sum / a.len() as f64
        };
        let mut last = Vec::new();
        for k in 1..20 {
            let u = middle(&rough, k);
            let pattern = hills(&rough, &smooth, Some(calm()), u, &inside);
            let highest = pattern.iter().copied().fold(f64::MIN, f64::max);
            let lowest = pattern.iter().copied().fold(f64::MAX, f64::min);
            assert!(highest - lowest > 600.0, "{k}: {lowest} {highest}");
            // The detail has the height of the Roughness setting, and the
            // fingers have 1.5 times that height.
            assert!(highest <= 2750.0 && lowest >= -2250.0, "{k}");
            assert!(lowest < -200.0 && highest > 200.0, "{k}");
            // The fabric changes less along the line than across it.
            let along = hills(&rough, &smooth, Some(calm()), u + cell, &inside);
            let moved: Vec<f64> = inside.iter().map(|a| a + cell).collect();
            let across = hills(&rough, &smooth, Some(calm()), u, &moved);
            let (along, across) = (mean(&pattern, &along), mean(&pattern, &across));
            assert!(
                along < 0.7 * across && across > 100.0,
                "{k}: {along} {across}"
            );
            // Each segment has its own pattern, and the fabric ends at the
            // edge of the rise.
            assert!(mean(&pattern, &last) > 100.0 || last.is_empty(), "{k}");
            for hill in hills(&rough, &smooth, Some(calm()), u, &outside) {
                assert_eq!(hill, 0.0);
            }
            for side in [-1.0, 1.0] {
                let edge = width(&smooth, calm(), u, side);
                let near: Vec<f64> = (95..=100)
                    .map(|i| side * edge * f64::from(i) / 100.0)
                    .collect();
                let hills = hills(&rough, &smooth, Some(calm()), u, &near);
                assert!(hills.iter().all(|h| h.abs() <= 300.0), "{k}: {hills:?}");
            }
            // A round end of the line has no fabric.
            let end = |plan: &RidgePlan| {
                let point = RidgePoint {
                    past: 0.6 * r,
                    ..at(u, 0.0)
                };
                plan.level(START, point, calm())
            };
            assert!(end(&smooth) > START + 1000);
            assert_eq!(end(&rough), end(&smooth));
            last = pattern;
        }
        // The fingers go through a crack.
        let crack = hills(&rough, &smooth, Some(calm()), boundary(&rough, 3), &inside);
        assert!(crack.iter().any(|h| h.abs() > 200.0));
        // At a small radius, the fabric has no fingers. Then a crack has no
        // fabric.
        let step = FRAC_PI_2 / FINE as f64;
        let small = Ridge {
            radius: 50.0 * step,
            segment: 0.0,
            roughness: 2000.0,
            ..spec()
        };
        let (rough, smooth) = (
            shape(small, FINE),
            shape(
                Ridge {
                    roughness: 0.0,
                    ..small
                },
                FINE,
            ),
        );
        let inside: Vec<f64> = (-45..=45).map(|i| f64::from(i) * step).collect();
        for i in 1..20 {
            let hills = hills(&rough, &smooth, Some(calm()), boundary(&rough, i), &inside);
            assert!(hills.iter().all(|h| h.abs() <= 2.0), "{i}");
            let hills = hills_at(&rough, &smooth, middle(&rough, i), &inside);
            assert!(hills.iter().any(|h| h.abs() > 200.0), "{i}");
        }
    }

    /// `hills` with the calm noise.
    fn hills_at(rough: &RidgePlan, smooth: &RidgePlan, u: f64, places: &[f64]) -> Vec<f64> {
        hills(rough, smooth, Some(calm()), u, places)
    }

    /// The distances between two bottoms of the fabric across the line, in
    /// texel steps, for a radius of `radius` texel steps. A turn of less
    /// than a tenth of the roughness does not count.
    fn fabric_widths(radius: f64) -> Vec<f64> {
        let step = FRAC_PI_2 / FINE as f64;
        let rough = Ridge {
            radius: radius * step,
            segment: 0.0,
            roughness: 4000.0,
            ..spec()
        };
        let (rough, smooth) = (
            shape(rough, FINE),
            shape(
                Ridge {
                    roughness: 0.0,
                    ..rough
                },
                FINE,
            ),
        );
        let fine = step / 32.0;
        let count = (0.8 * radius * 32.0) as i32;
        let places: Vec<f64> = (-count..=count).map(|i| f64::from(i) * fine).collect();
        let mut distances = Vec::new();
        for k in 1..12 {
            let pattern = hills(&rough, &smooth, None, middle(&rough, k), &places);
            // The places where the pattern turns from up to down or back.
            let mut turns = Vec::new();
            let (mut end, mut up) = (0, None);
            for (i, &hill) in pattern.iter().enumerate() {
                let change = hill - pattern[end];
                match up {
                    None if change.abs() > 400.0 => up = Some(change > 0.0),
                    None => continue,
                    Some(up) if (change > 0.0) == up => {}
                    Some(now) if change.abs() > 400.0 => {
                        if !now {
                            turns.push(end);
                        }
                        up = Some(!now);
                    }
                    Some(_) => continue,
                }
                end = i;
            }
            distances.extend(turns.windows(2).map(|w| (w[1] - w[0]) as f64 * fine / step));
        }
        distances
    }

    #[test]
    fn a_fabric_ridge_is_4_texels_wide_or_more() {
        // At a small radius, the fabric has one octave. At a large radius,
        // it has three.
        for radius in [40.0, 300.0] {
            let distances = fabric_widths(radius);
            assert!(distances.len() > 20, "{radius}: {}", distances.len());
            let least = distances.iter().copied().fold(f64::MAX, f64::min);
            assert!(least >= 4.0, "{radius}: {least}");
        }
    }

    #[test]
    fn the_level_has_no_step_along_the_line_and_across_it() {
        let full = Ridge {
            offset: 0.04,
            roughness: 1500.0,
            valley: 1000.0,
            ..spec()
        };
        let plan = shape(full, FINE);
        let (r, fine) = (plan.ridge.radius, plan.step() / 64.0);
        // Along the line, over cracks and over the middle of segments.
        for across in [-1.3, -0.6, -0.04, 0.0, 0.02, 0.5, 1.1] {
            let mut last = real(&plan, 0.3, across * r);
            for i in 1..40000 {
                let new = real(&plan, 0.3 + f64::from(i) * fine, across * r);
                assert!((new - last).abs() < 400.0, "{across} {i}: {last} {new}");
                last = new;
            }
        }
        // Across the line, to the reach.
        for j in 0..40 {
            let u = 0.3 + f64::from(j) * 0.0137;
            let count = (1.5 * r / fine) as i32;
            let mut last = real(&plan, u, -1.5 * r);
            assert_eq!(last, f64::from(START));
            for i in -count..=count {
                let new = real(&plan, u, f64::from(i) * fine);
                assert!((new - last).abs() < 400.0, "{u} {i}: {last} {new}");
                last = new;
            }
            assert!((last - f64::from(START)).abs() <= 1.0, "{u}: {last}");
        }
    }

    #[test]
    fn the_valley_lowers_the_crest() {
        let r = spec().radius;
        let places: Vec<f64> = (-200..=200).map(|i| r * f64::from(i) / 200.0).collect();
        let highest = |plan: &RidgePlan, u: f64| {
            let levels = places.iter().map(|&across| level(plan, u, across));
            levels.fold(f64::MIN, f64::max)
        };
        let none = shape(spec(), FINE);
        let shallow = shape(
            Ridge {
                valley: 3000.0,
                ..spec()
            },
            FINE,
        );
        let deep = shape(
            Ridge {
                valley: 8000.0,
                ..spec()
            },
            FINE,
        );
        // With the calm noise, the valley is 0.05 of the radius wide on each
        // side, and it has 0.45 of its depth in the middle of a segment.
        let width = 0.05 * r;
        for k in 1..12 {
            let u = middle(&none, k);
            let crest = level(&none, u, 0.0);
            assert_eq!(highest(&none, u), crest);
            for (plan, depth) in [(&shallow, 3000.0), (&deep, 8000.0)] {
                // The rims are at the level of the crest, and the floor is
                // below them.
                assert!((level(plan, u, 0.0) - (crest - 0.45 * depth)).abs() <= 1.0);
                assert_eq!(highest(plan, u), crest);
                for side in [-1.0, 1.0] {
                    assert_eq!(level(plan, u, side * width), crest);
                    let slope = level(plan, u, side * 0.5 * width);
                    assert!((slope - (crest - 0.225 * depth)).abs() <= 1.0);
                    assert!(level(plan, u, side * 2.0 * width) < crest);
                    // The rise still ends at its width.
                    let end = self::width(&none, calm(), u, side);
                    assert!(level(plan, u, side * 0.97 * end) > f64::from(START));
                    assert_eq!(level(plan, u, side * 1.01 * end), f64::from(START));
                }
            }
            // With half of the flow, the valley has half of the depth.
            let half = RidgePoint {
                flow: 0.5,
                ..at(u, 0.0)
            };
            let rim = f64::from(START) + 0.5 * (crest - f64::from(START));
            let floor = f64::from(deep.level(START, half, calm()));
            assert!((floor - (rim - 0.5 * 0.45 * 8000.0)).abs() <= 1.0);
        }
        // The valley is deeper in places, and its depth is the Valley
        // setting at most.
        let depths: Vec<f64> = (0..2000)
            .map(|i| {
                let u = f64::from(i) * 0.002;
                let across = (-40..=40).map(|j| r * f64::from(j) / 400.0);
                let levels: Vec<f64> = across.clone().map(|a| real(&deep, u, a)).collect();
                let flat = across.map(|a| real(&none, u, a));
                let depths = flat.zip(&levels).map(|(a, b)| a - b);
                depths.fold(f64::MIN, f64::max)
            })
            .collect();
        let most = depths.iter().copied().fold(f64::MIN, f64::max);
        let least = depths.iter().copied().fold(f64::MAX, f64::min);
        assert!(most <= 8001.0 && most > 5000.0, "{most}");
        assert!(least < 3500.0, "{least}");
    }

    #[test]
    fn a_segment_goes_to_the_shader_as_16_numbers() {
        let mut line = RidgeLine::new(spec(), 128);
        for lon in [0.0, 3.0, 6.0] {
            line.push(lonlat_to_dir(lon, 0.0), 0.5);
        }
        let plan = line.plan(0);
        assert_eq!(
            (plan.segments.len(), plan.tail.clone(), plan.end),
            (2, 0..2, 3)
        );
        let (first, second) = (plan.segments[0], plan.segments[1]);
        assert!(first.first && !first.last && !second.first && second.last);
        let len = 3f64.to_radians() as f32;
        // The second segment starts at 3 degrees, and cell 1 is at 0.1.
        let numbers = second.to_f32(0.1);
        assert!((numbers[2] - 1.0).abs() < 1e-6 && (numbers[3] - (len - 0.1)).abs() < 1e-6);
        assert!((numbers[7] - len).abs() < 1e-6);
        assert_eq!(numbers[11..], [0.5, 0.5, 2.0, 1.0, 0.0]);
        assert_eq!(first.to_f32(0.1)[13..], [1.0, 0.0, 0.0]);
        // The plane at the shared point is the same for the two segments.
        assert!(length(add(first.m1, scale(second.m0, -1.0))) < 1e-12);
    }

    fn on(plan: &RidgePlan, p: V3) -> RidgePoint {
        match plan.place(p) {
            RidgePlace::On(point) => point,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_place_has_the_distances_along_and_across_the_line() {
        let mut line = RidgeLine::new(spec(), 128);
        for i in 0..=10 {
            line.push(lonlat_to_dir(3.0 * f64::from(i), 0.0), f64::from(i) / 10.0);
        }
        // A point with no distance from the last point is not added.
        line.push(lonlat_to_dir(30.0, 0.0), 1.0);
        assert_eq!(line.len(), 11);
        let plan = line.plan(0);
        let point = on(&plan, lonlat_to_dir(10.0, 3.0));
        assert!((point.u - 10f64.to_radians()).abs() < 1e-4, "{point:?}");
        assert!((point.across - 3f64.to_radians()).abs() < 1e-12);
        assert!((point.flow - 1.0 / 3.0).abs() < 1e-3);
        assert_eq!(point.past, 0.0);
        // The left of the line is positive.
        assert!(on(&plan, lonlat_to_dir(20.0, -2.0)).across < 0.0);
        // Past an end, the place is the end.
        for (lon, lat, u) in [(-2.0, 1.0, 0.0), (33.0, -1.0, 30f64.to_radians())] {
            let p = lonlat_to_dir(lon, lat);
            let point = on(&plan, p);
            let end = lonlat_to_dir(if u == 0.0 { 0.0 } else { 30.0 }, 0.0);
            assert!((point.u - u).abs() < 1e-12);
            assert!((point.across - lat.to_radians()).abs() < 1e-12);
            assert!((point.across.hypot(point.past) - angle(p, end)).abs() < 1e-9);
        }
        assert_eq!(plan.place(lonlat_to_dir(10.0, 9.0)), RidgePlace::Far);
        assert_eq!(plan.place(lonlat_to_dir(-9.0, 0.0)), RidgePlace::Far);
        // The plan of the last point changes no texel at the start.
        let last = line.plan(10);
        assert!(last.segments.len() >= 5 && last.tail.len() == 2);
        assert_eq!(last.place(lonlat_to_dir(10.0, 3.0)), RidgePlace::Far);
        assert_eq!(
            on(&last, lonlat_to_dir(29.0, 3.0)),
            on(&plan, lonlat_to_dir(29.0, 3.0))
        );
    }

    #[test]
    fn a_place_has_no_step_at_a_turn_and_at_an_end() {
        // The line turns by 60 degrees at the middle point.
        let corner = lonlat_to_dir(0.0, 0.0);
        let mut line = RidgeLine::new(spec(), 128);
        let dirs = path(&[(-9.0, 0.0), (0.0, 0.0), (4.5, 7.8)], 0.05);
        for &dir in &dirs {
            line.push(dir, 1.0);
        }
        let plan = line.plan(0);
        for center in [corner, dirs[0], dirs[dirs.len() - 1]] {
            let e1 = normalize(cross(center, [0.0, 0.0, 1.0]));
            let e2 = cross(center, e1);
            let around = |i: usize| {
                let (sin, cos) = (i as f64 / 720.0 * std::f64::consts::TAU).sin_cos();
                let side = add(scale(e1, cos), scale(e2, sin));
                on(&plan, normalize(add(center, scale(side, 0.05))))
            };
            for i in 0..720 {
                let (a, b) = (around(i), around(i + 1));
                let distance = |p: RidgePoint| p.across.hypot(p.past);
                assert!((a.u - b.u).abs() < 3e-3, "{i}: {a:?} {b:?}");
                assert!((a.across - b.across).abs() < 1e-3, "{i}: {a:?} {b:?}");
                assert!((distance(a) - distance(b)).abs() < 1e-3, "{i}: {a:?} {b:?}");
            }
        }
    }

    /// Points along the great circles between places, each in degrees of
    /// longitude and latitude, with `spacing` radians between two points.
    fn path(places: &[(f64, f64)], spacing: f64) -> Vec<V3> {
        let mut dirs = vec![lonlat_to_dir(places[0].0, places[0].1)];
        for w in places.windows(2) {
            let (a, b) = (lonlat_to_dir(w[0].0, w[0].1), lonlat_to_dir(w[1].0, w[1].1));
            let steps = (angle(a, b) / spacing).ceil() as usize;
            let tangent = cross(normalize(cross(a, b)), a);
            dirs.extend((1..=steps).map(|i| {
                let (sin, cos) = (angle(a, b) * i as f64 / steps as f64).sin_cos();
                add(scale(a, cos), scale(tangent, sin))
            }));
        }
        dirs
    }

    /// Ground with slopes below the crest level.
    fn ground(n: usize) -> Heightmap {
        let mut map = Heightmap::new(n, 0);
        let face = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        for f in 0..FACES {
            let levels: Vec<u16> = (0..n * n)
                .map(|i| {
                    let d = map.texel_dir(f, i % n, i / n);
                    let level = 3000.0 * (9.0 * d[0] + 4.0 * d[1]).sin() * (7.0 * d[2]).cos();
                    (f64::from(START) + level) as u16
                })
                .collect();
            map.store_rect(f, face, &levels);
        }
        map
    }

    fn levels(map: &Heightmap) -> Vec<u16> {
        let n = map.face_size();
        let mut out = Vec::new();
        for face in 0..FACES {
            let rect = TexelRect {
                x0: 0,
                y0: 0,
                x1: n,
                y1: n,
            };
            map.read_rect(face, rect, &mut out);
        }
        out
    }

    /// Starts a stroke and lays a ridge point by point, as the app does.
    /// The flow changes along the line.
    fn lay(map: &mut Heightmap, spec: Ridge, dirs: &[V3]) -> RidgeLine {
        let mut line = RidgeLine::new(spec, map.face_size());
        map.begin_stroke();
        for (i, &dir) in dirs.iter().enumerate() {
            let from = line.len();
            line.push(dir, 0.6 + 0.4 * (i as f64 * 0.3).sin());
            map.ridge(&line.plan(from));
        }
        line
    }

    /// The levels after a stroke, from the place of each texel on the full
    /// line.
    fn expected(start: &Heightmap, line: &RidgeLine) -> Vec<u16> {
        let n = start.face_size();
        let plan = line.plan_of(0, line.len() - 1);
        let mut out = levels(start);
        for (i, level) in out.iter_mut().enumerate() {
            let p = start.texel_dir(i / (n * n), i % n, i / n % n);
            if let RidgePlace::On(point) = plan.place(p) {
                *level = plan.level(*level, point, plan.noise(p));
            }
        }
        out
    }

    fn full() -> Ridge {
        Ridge {
            offset: 0.04,
            roughness: 1500.0,
            valley: 1000.0,
            segment: 0.08,
            ..spec()
        }
    }

    /// Strokes that cross their own line, turn back, and go over a face
    /// edge, a cube corner, and a pole.
    fn strokes() -> Vec<(&'static str, Vec<V3>)> {
        let spacing = RIDGE_SPACING * spec().radius;
        let stroke = |places: &[(f64, f64)]| path(places, spacing);
        vec![
            (
                "cross",
                stroke(&[(-10.0, -5.0), (10.0, 5.0), (10.0, -5.0), (-10.0, 5.0)]),
            ),
            ("turn", stroke(&[(-10.0, 0.0), (10.0, 0.0), (-10.0, 3.0)])),
            ("back", stroke(&[(-10.0, 20.0), (5.0, 20.0), (-10.0, 20.0)])),
            (
                "curve",
                stroke(&[(100.0, -9.0), (108.0, -4.0), (112.0, 4.0), (110.0, 14.0)]),
            ),
            ("face edge", stroke(&[(36.0, -6.0), (54.0, 10.0)])),
            ("cube corner", stroke(&[(36.0, 28.0), (54.0, 42.0)])),
            ("pole", stroke(&[(20.0, 80.0), (200.0, 80.0)])),
        ]
    }

    #[test]
    fn a_stroke_in_steps_gives_the_levels_of_the_full_line() {
        let n = 128;
        let start = ground(n);
        for (name, dirs) in strokes() {
            let mut map = ground(n);
            let line = lay(&mut map, full(), &dirs);
            assert!(line.len() > 10 && line.len() <= RIDGE_SEGMENTS, "{name}");
            let after = levels(&map);
            let want = expected(&start, &line);
            let before = levels(&start);
            let wrong: Vec<_> = (0..after.len())
                .filter(|&i| after[i] != want[i])
                .map(|i| (i / (n * n), i % n, i / n % n, before[i], after[i], want[i]))
                .collect();
            assert!(
                wrong.is_empty(),
                "{name}: {} {:?}",
                wrong.len(),
                &wrong[..wrong.len().min(12)]
            );
            assert!(after != levels(&start), "{name}");
            // No texel is above the crest by more than the fabric.
            let top = f64::from(CREST) + 1.4 * full().roughness;
            assert!(after.iter().all(|l| f64::from(*l) <= top), "{name}");
            // A second pass over the line changes nothing.
            assert_eq!(map.ridge(&line.plan(0)), 0, "{name}");
            for from in [1, line.len() / 2, line.len()] {
                assert_eq!(map.ridge(&line.plan(from)), 0, "{name}");
            }
        }
    }

    #[test]
    fn a_long_line_goes_in_more_than_one_plan() {
        let n = 128;
        let dirs = path(
            &[(-60.0, -20.0), (0.0, 20.0), (60.0, -20.0), (120.0, 20.0)],
            0.01,
        );
        let mut in_steps = ground(n);
        let line = lay(&mut in_steps, full(), &dirs);
        assert!(line.len() > 2 * RIDGE_SEGMENTS);
        let mut in_plans = ground(n);
        in_plans.begin_stroke();
        let (mut from, mut plans) = (0, 0);
        while from < line.len() {
            let plan = line.plan(from);
            assert!(plan.segments.len() <= RIDGE_SEGMENTS && plan.end > from);
            in_plans.ridge(&plan);
            from = plan.end;
            plans += 1;
        }
        assert!(plans >= 3);
        assert!(levels(&in_steps) == levels(&in_plans));
    }

    #[test]
    fn the_ground_far_from_the_line_does_not_change() {
        let n = 128;
        let wide = Ridge {
            offset: 1.0,
            ..full()
        };
        for (name, dirs) in strokes() {
            let (start, mut map) = (ground(n), ground(n));
            let line = lay(&mut map, wide, &dirs);
            let reach = REACH * line.spec().radius;
            let (before, after) = (levels(&start), levels(&map));
            let mut farthest: f64 = 0.0;
            for i in (0..after.len()).filter(|&i| after[i] != before[i]) {
                let p = map.texel_dir(i / (n * n), i % n, i / n % n);
                let distances = (0..line.len() - 1).map(|j| line.distance(j, p));
                farthest = farthest.max(distances.fold(f64::MAX, f64::min));
            }
            assert!(farthest < reach, "{name}: {farthest}");
            assert!(farthest > 0.6 * reach, "{name}: {farthest}");
        }
    }

    #[test]
    fn the_seed_sets_the_pattern() {
        let dirs = path(&[(-10.0, -5.0), (10.0, 5.0)], 0.025);
        let laid = |seed: u32| {
            let mut map = ground(128);
            lay(&mut map, Ridge { seed, ..full() }, &dirs);
            levels(&map)
        };
        assert!(laid(7) == laid(7));
        assert!(laid(7) != laid(8));
    }

    #[test]
    fn the_floor_and_the_ceiling_apply_to_a_ridge() {
        let dirs = path(&[(-10.0, -5.0), (10.0, 5.0)], 0.025);
        let laid = |limits: Limits| {
            let mut map = Heightmap::new(128, START);
            // With this seed, the line has a strong crack.
            let spec = Ridge {
                limits,
                seed: 6,
                ..full()
            };
            lay(&mut map, spec, &dirs);
            levels(&map)
        };
        let free = laid(Limits::default());
        assert!(free.iter().any(|l| *l < START - 1000) && free.iter().any(|l| *l > 22000));
        let floor = laid(Limits {
            floor: START,
            ceiling: 65535,
        });
        assert!(floor.iter().all(|l| *l >= START));
        assert!(floor.iter().zip(&free).all(|(a, b)| a == b || *b < START));
        let ceiling = laid(Limits {
            floor: 0,
            ceiling: 20000,
        });
        assert_eq!(ceiling.iter().max(), Some(&20000));
        assert!(ceiling.iter().zip(&free).all(|(a, b)| a == b || *b > 20000));
    }

    #[test]
    fn one_undo_step_puts_back_the_ground_from_before_a_ridge() {
        let dirs = path(&[(30.0, -5.0), (60.0, 5.0)], 0.025);
        let mut map = ground(128);
        let before = levels(&map);
        lay(&mut map, full(), &dirs);
        assert!(map.end_stroke());
        let after = levels(&map);
        assert!(after != before);
        assert!(map.undo());
        assert!(levels(&map) == before);
        assert!(!map.can_undo());
        assert!(map.redo());
        assert!(levels(&map) == after);
        // A cancelled stroke also puts back the ground.
        let mut map = ground(128);
        lay(&mut map, full(), &dirs);
        map.cancel_stroke();
        assert!(levels(&map) == before);
    }
}
