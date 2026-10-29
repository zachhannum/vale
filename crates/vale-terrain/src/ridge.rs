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

/// The largest number of segments in one plan past the first segment that
/// the new points change. With the spacing of the app, they are 4 brush
/// radii of line, so the rectangles of a plan stay small.
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
    /// The height of the hills, in 16-bit steps.
    pub roughness: f64,
    /// The depth of the valley along the crest, in 16-bit steps.
    pub valley: f64,
    /// Half of the spreading rate, in radians per million years.
    pub spread: f64,
    /// The change of the magma part of the spreading from its value at the
    /// spreading rate. A higher part gives lower faults that are closer
    /// together.
    pub magma: f64,
    /// The radius of the world, in kilometers.
    pub world_km: f64,
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
    /// The distance along the line to its nearer end, in radians.
    pub end: f64,
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
    /// The new segments and the segments before them that the new points
    /// change, as a range of `segments`. Only a texel closer to one of them
    /// than 1.5 brush radii changes.
    pub tail: Range<usize>,
    /// The number of points of the line that the plan covers. If it is less
    /// than the length of the line, make the next plan from it.
    pub end: usize,
    /// The length of the line, in radians.
    pub length: f64,
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
            spread: spec.spread.max(1e-12),
            world_km: spec.world_km.max(1.0),
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
    /// because a new point changes the end of that segment. The ridge is
    /// lower near the end of the line, so the plan also holds the segments
    /// of the last brush radius before the new points, up to 32 of them. A
    /// plan covers 16 segments past them at most.
    ///
    /// The plan holds each segment that can reach a texel of its rectangles.
    /// If these are more than `RIDGE_SEGMENTS`, the plan holds the nearest
    /// ones. `GpuHeightmap::ridge` can then lose parts of the ridge of the
    /// segments that are not in the plan.
    pub fn plan(&self, from: usize) -> RidgePlan {
        let count = self.points.len().saturating_sub(1);
        let mut start = from.saturating_sub(2).min(count);
        if let Some(old) = from.checked_sub(1).and_then(|i| self.points.get(i)) {
            let limit = old.u - TAPER * self.spec.radius;
            while start > 0 && from - start < 2 * NEW_SEGMENTS && self.points[start].u > limit {
                start -= 1;
            }
        }
        let stop = (start + NEW_SEGMENTS).max(from).min(count);
        self.plan_of(start, stop)
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
            length: self.points.last().map_or(0.0, |point| point.u),
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
                let u = s.u0 + along * s.len;
                best = Some(RidgePoint {
                    u,
                    end: u.min(self.length - u).max(0.0),
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

    /// The group of crest segment `k`.
    fn group(&self, k: i32) -> i32 {
        let c = k >> 2;
        if k >= self.group_start(c) { c } else { c - 1 }
    }

    /// The shift of crest segment `k` across the line.
    fn shift(&self, k: i32) -> f64 {
        let seed = self.ridge.seed;
        let group = self.group(k);
        let part = |i: i32, channel: u32| 2.0 * unit(hash(i as u32, channel, 0, seed)) - 1.0;
        self.ridge.offset * (0.75 * part(group, 3) + 0.25 * part(k, 4))
    }

    /// The place of the crest across the line, at `s` from boundary `i`
    /// along the line and at `from_line` from the line. The crest steps
    /// sideways at each boundary. Near the line, the step is narrow. On the
    /// outer flank, it is wide. The zone of a step is shorter than 0.9 of
    /// the segment length on each side, so the steps of 3 boundaries can
    /// reach a place.
    fn crest(&self, i: i32, s: f64, from_line: f64) -> f64 {
        let spec = &self.ridge;
        let zone = crack_half_width(spec.radius, self.step())
            + STEP_SPREAD * (from_line - spec.offset).max(0.0);
        let zone = zone.min(0.9 * spec.segment);
        let mut shift = self.shift(i.wrapping_sub(2));
        for near in -1..=1 {
            let j = i.wrapping_add(near);
            let from = s + (self.jitter(i) - f64::from(near) - self.jitter(j)) * spec.segment;
            let step = self.shift(j) - self.shift(j.wrapping_sub(1));
            shift += step * smoothstep(-zone, zone, from);
        }
        shift
    }

    /// The distance along the line with the bend of the noise. The curve
    /// grows with the distance from the line.
    fn bent(&self, at: RidgePoint, noise: RidgeNoise) -> f64 {
        let r = self.ridge.radius;
        let far = at.across.hypot(at.past) / (REACH * r);
        let curve = 0.5 * (noise.slow[1] - noise.slow[2]);
        at.u + r * (BEND_ALONG * noise.bend[0] + CURVE * curve * far * far)
    }

    /// The faults of the settings.
    fn faulting(&self) -> Faulting {
        let spec = &self.ridge;
        let full_rate = 2.0 * spec.spread * spec.world_km;
        let m = magma_share(full_rate) + spec.magma;
        let spacing = lerp_table(&FAULT_SPACING, m);
        let throw = KAPPA * lerp_table(&FAULT_HEAVE, m) * DIP_TAN;
        Faulting {
            spacing,
            throw,
            box_half: (0.5 * self.step() * spec.world_km / spacing).min(MAX_BOX),
            half_texel: 0.5 * self.step() * spec.world_km,
            level: spec.roughness / ROUGHNESS_KM,
        }
    }

    /// The height of the fault blocks on one side of the axis, in kilometers
    /// of throw. Each crest segment `k` has its own faults. `along` is the
    /// distance from the start of the segment in kilometers, and `x` is the
    /// distance from the axis in fault spacings. The faults are in rows
    /// along the line, one row for each fault spacing. Each fault is one
    /// piece of a row. Its height is exactly zero outside its window, so the
    /// sum does not depend on the cells that the search starts from.
    fn faults(&self, f: &Faulting, k: i32, side: u32, along: f64, x: f64) -> f64 {
        self.fault_sum(f, [k, 0], side, along, x)
    }

    /// `faults`, with a search of `extra` more cells on each side.
    fn fault_sum(&self, f: &Faulting, [k, extra]: [i32; 2], side: u32, along: f64, x: f64) -> f64 {
        let seed = self.ridge.seed ^ SALT_FAULT;
        let b = f.box_half;
        let draw = |i: i32, j: i32, channel: u32| {
            let key = (k as u32)
                .wrapping_mul(32)
                .wrapping_add(side * 16 + channel);
            unit(hash(i as u32, j as u32, key, seed))
        };
        let reach = WIDTH_MAX.max(SCARP_MAX) + b;
        let first = ((x - PLACE_SPREAD - reach).floor() as i32 - extra).max(0);
        let last = (x + PLACE_SPREAD - 1.0 + reach).floor() as i32 + extra;
        let mut sum = 0.0;
        // A throw: a smallest throw plus a part from an exponential
        // distribution.
        let throw =
            |u: f64| f.throw * (THROW_MIN + (1.0 - THROW_MIN) * (-(1.0 - u).ln()).min(THROW_MAX));
        for i in first..=last {
            // Row `i` of faults has pieces of one length, with its own
            // phase.
            let piece = (throw(draw(i, -1, 0)) / GAMMA).clamp(PIECE_MIN, PIECE_MAX);
            let phase = draw(i, -1, 1);
            let cell = (along / piece + phase).floor() as i32;
            for j in cell - 1 - extra..=cell + 1 + extra {
                let middle =
                    f64::from(j) + 0.5 - phase + PIECE_JITTER * (2.0 * draw(i, j, 3) - 1.0);
                let half = 0.5 * (1.0 + OVERLAP * draw(i, j, 4));
                // The throw is the mean over a box one texel long.
                let smooth = f.half_texel.min(MAX_SMOOTH * piece);
                let (start, end) = ((middle - half) * piece, (middle + half) * piece);
                if along <= start - smooth || along >= end + smooth {
                    continue;
                }
                let q = (2.0 * along - start - end) / (end - start);
                let bow = (1.0 - q * q).max(0.0);
                let taper = ellipse(q, 2.0 * smooth / (end - start));
                // Each piece is a fault with its own throw and place. Most
                // faults face the axis.
                let height = throw(draw(i, j, 5)) * taper;
                let inward = draw(i, j, 2) < INWARD;
                let at = f64::from(i)
                    + 0.5
                    + FAULT_JITTER * (2.0 * draw(i, j, 1) - 1.0)
                    + STEP_OVER * (2.0 * draw(i, j, 6) - 1.0)
                    + BOW * (2.0 * draw(i, j, 7) - 1.0) * bow;
                let scarp = (height / (DIP_TAN * f.spacing)).clamp(1e-6, SCARP_MAX);
                let y = if inward { x - at } else { at - x };
                let width = (BLOCK_WIDTH * (height / f.throw).sqrt()).clamp(WIDTH_MIN, WIDTH_MAX);
                sum += height * block(y, scarp, width, b);
            }
        }
        sum
    }

    /// The level of a texel on the ridge. `start` is its level from before
    /// the stroke, and `noise` is the noise at its place.
    pub(crate) fn level(&self, start: u16, at: RidgePoint, noise: RidgeNoise) -> u16 {
        let spec = &self.ridge;
        let (r, t, km) = (spec.radius, self.step(), spec.world_km);
        let old = f64::from(start);
        let from_line = at.across.hypot(at.past);
        // The ridge has no height at an end of the line.
        let taper = smoothstep(0.0, TAPER * r, at.end);
        // Near an end, the ridge is narrow. `far` is the distance from the
        // line in brush radii of that narrow ridge.
        let lens = TAPER_WIDTH + (1.0 - TAPER_WIDTH) * taper;
        let far = from_line / (r * lens);
        // The noise bends the two distances. Near the reach, the distance
        // across has no bend.
        let hold = 1.0 - smoothstep(r, REACH * r, from_line);
        let u = self.bent(at, noise);
        let across = at.across + BEND_ACROSS * r * hold * noise.bend[1];
        // The crest has a boundary near each multiple of the segment length.
        // `s` is the distance from the nearest boundary `i`, the start of
        // crest segment `i`.
        let cell = (u / spec.segment + 0.5).floor();
        let i = cell as i32;
        let jitter = |i: i32| self.jitter(i);
        let s = u - (cell + jitter(i)) * spec.segment;
        let off = across - self.crest(i, s, from_line);
        let from_crest = off.hypot(at.past);
        // The age of the sea floor, in millions of years. The crest is round
        // over a few texels.
        let round = CREST_ROUND * t / spec.spread;
        let age = |d: f64| (d / spec.spread).hypot(round) - round;
        // The depth follows the age, and is zero at the crest and at the
        // brush radius.
        let (top, foot) = (depth(0.0), depth(age(r)));
        let w = ((foot - depth(age(from_crest / lens))) / (foot - top)).clamp(0.0, 1.0);
        // The foot of the rise has no slope.
        let w = w * w * (1.0 + FOOT) / (w + FOOT);
        let lift = (f64::from(spec.crest) - old).max(0.0);
        // The fault blocks.
        let f = self.faulting();
        let mean = BLOCK_MEAN * f.throw * f.level;
        // Ground at the crest level gets no faults.
        let low = (lift / (HILL_TOP * mean + 1.0)).min(1.0);
        // The rise is lower by the mean height of the blocks, so the mean
        // ground follows the depth.
        let rise = (lift - low * mean) * taper * w;
        // The blocks of crest segment `k` at `along` kilometers from its
        // start. They are at the distance from the crest of that segment,
        // and the throw of a fault grows near that crest.
        let segment_blocks = |k: i32, along: f64| {
            let off = across - self.shift(k);
            let from_crest = off.hypot(at.past);
            let x = from_crest * km / f.spacing;
            let grow = smoothstep(0.0, GROW * f.spacing / km + t, from_crest);
            grow * self.faults(&f, k, u32::from(off < 0.0), along, x)
        };
        // Near boundary `i`, the blocks of the two crest segments blend.
        let zone = BLEND.min(0.1 * spec.segment * km);
        let next = smoothstep(-zone, zone, s * km);
        let mut blocks = 0.0;
        if next > 0.0 {
            blocks += next * segment_blocks(i, s * km);
        }
        if next < 1.0 {
            let before = spec.segment * (1.0 + jitter(i) - jitter(i.wrapping_sub(1)));
            blocks += (1.0 - next) * segment_blocks(i.wrapping_sub(1), (s + before) * km);
        }
        // The blocks fade toward the reach.
        let fade = 1.0 - smoothstep(HILL_FADE, REACH, far);
        let hills = blocks * f.level * low * taper * fade;
        // The highest blocks bend down below the Crest setting.
        let hills = soft_cap(hills, (lift - rise).max(0.0));
        let up = (rise + hills).min(lift);
        let change = at.flow * up;
        let new = (old + (change + 0.5).floor()).clamp(0.0, 65535.0);
        spec.limits.apply(old, new) as u16
    }
}

/// The faults of a stroke.
struct Faulting {
    /// The mean distance between two faults, in kilometers.
    spacing: f64,
    /// The mean throw of a fault, in kilometers.
    throw: f64,
    /// Half of the texel step, in fault spacings.
    box_half: f64,
    /// Half of the texel step, in kilometers.
    half_texel: f64,
    /// The 16-bit steps of one kilometer of throw.
    level: f64,
}

/// The noise at the place of a texel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct RidgeNoise {
    /// Three numbers from -1.5 to 1.5: noise with cells of 0.9 of the brush
    /// radius, plus half of noise with cells of 0.37 of the brush radius.
    pub bend: V3,
    /// Three numbers from -1 to 1. They change over about two brush radii.
    pub slow: V3,
}

/// The sizes of the cells of the noise, as parts of the brush radius.
const BEND_SIZE: f64 = 0.9;
const BEND_FINE_SIZE: f64 = 0.37;
const SLOW_SIZE: f64 = 1.7;

/// The largest bend of the distance along the line and across it is 1.5
/// times this part of the brush radius.
const BEND_ALONG: f64 = 0.08;
const BEND_ACROSS: f64 = 0.08;

/// At the reach, the slow noise moves the distance along the line by up to
/// this part of the brush radius.
const CURVE: f64 = 0.3;

/// The seed of each noise is the seed of the pattern with these bits
/// changed.
const SALT_BEND: u32 = 0x51ed_270b;
const SALT_BEND_FINE: u32 = 0x2f6b_8a35;
const SALT_SLOW: u32 = 0x7a3c_91d7;
const SALT_FAULT: u32 = 0x1b87_3593;

/// The largest move of a boundary, as a part of the segment length.
const JITTER: f64 = 0.35;

/// The ridge gets its full height and width at this number of brush radii
/// from an end of the line.
pub(crate) const TAPER: f64 = 1.2;

/// The width of the rise at an end of the line, as a part of its full
/// width.
const TAPER_WIDTH: f64 = 0.2;

/// The top of the rise is round over about this number of texels.
const CREST_ROUND: f64 = 2.0;

/// The foot of the rise is round below about this part of the height.
const FOOT: f64 = 0.25;

/// The zone of a step of the crest gets wider by this part of the distance
/// from the line past the Offset setting.
const STEP_SPREAD: f64 = 0.6;

/// The depth of the sea floor below the crest at an age in millions of
/// years, in meters (Stein and Stein 1992).
fn depth(age: f64) -> f64 {
    if age < 20.0 {
        2600.0 + 365.0 * age.sqrt()
    } else {
        5651.0 - 2473.0 * (-0.0278 * age).exp()
    }
}

/// The magma part of the spreading at a full spreading rate in millimeters
/// per year.
fn magma_share(full_rate: f64) -> f64 {
    lerp_table(&MAGMA_SHARE, full_rate)
}

/// Points of a function, and the function between them. Past the first and
/// the last point, the function is flat.
fn lerp_table(table: &[(f64, f64)], x: f64) -> f64 {
    let (first, last) = (table[0], table[table.len() - 1]);
    if x <= first.0 {
        return first.1;
    }
    for pair in table.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    last.1
}

/// The magma part of the spreading at a full spreading rate in millimeters
/// per year (Behn and Ito 2008).
const MAGMA_SHARE: [(f64, f64); 3] = [(20.0, 0.75), (50.0, 0.85), (100.0, 0.95)];

/// The distance between faults and the heave of a fault at a magma part, in
/// kilometers (Buck, Lavier, and Poliakov 2005; Behn and Ito 2008).
const FAULT_SPACING: [(f64, f64); 5] = [(0.5, 8.0), (0.6, 5.0), (0.7, 4.0), (0.8, 3.0), (0.9, 2.0)];
const FAULT_HEAVE: [(f64, f64); 5] = [(0.5, 5.0), (0.6, 2.5), (0.7, 1.2), (0.8, 0.7), (0.9, 0.3)];

/// The tangent of the dip of a fault, about 50 degrees.
const DIP_TAN: f64 = 1.2;

/// The throw of a fault divided by its heave and the tangent of its dip.
const KAPPA: f64 = 0.42;

/// The Roughness setting that gives the blocks their height in the model, in
/// kilometers. Other settings scale the height.
const ROUGHNESS_KM: f64 = 0.3;

/// The smallest throw of a fault as a part of the mean throw, and the
/// largest exponential part, in mean throws.
const THROW_MIN: f64 = 0.6;
const THROW_MAX: f64 = 3.0;

/// The largest move of a fault across the line from the middle of its
/// cell, in fault spacings.
const FAULT_JITTER: f64 = 0.4;

/// The part of the faults that face the axis.
const INWARD: f64 = 0.8;

/// The throw of a fault divided by its length.
const GAMMA: f64 = 0.02;

/// The shortest and the longest piece of a fault, in kilometers.
const PIECE_MIN: f64 = 10.0;
const PIECE_MAX: f64 = 30.0;

/// The largest move of the middle of a piece along the line, and the
/// largest overlap of two pieces, in piece lengths.
const PIECE_JITTER: f64 = 0.15;
const OVERLAP: f64 = 0.3;

/// The largest step of a piece across the line from its fault, and the
/// largest bow of a piece, in fault spacings.
const STEP_OVER: f64 = 0.3;
const BOW: f64 = 0.2;

/// The largest distance of a scarp from the middle of its cell, in fault
/// spacings.
const PLACE_SPREAD: f64 = 0.5 + FAULT_JITTER + STEP_OVER + BOW;

/// The faults of two crest segments blend over this distance on each side
/// of their boundary, in kilometers.
const BLEND: f64 = 10.0;

/// The width of a block behind its scarp, in fault spacings, is
/// `BLOCK_WIDTH` times the square root of its throw in mean throws, from
/// `WIDTH_MIN` to `WIDTH_MAX`. A scarp is no wider than `SCARP_MAX` fault
/// spacings.
const BLOCK_WIDTH: f64 = 2.5;
const WIDTH_MIN: f64 = 1.0;
const WIDTH_MAX: f64 = 5.0;
const SCARP_MAX: f64 = 0.4;

/// The largest half width of the box that smooths the blocks, in fault
/// spacings, and along a piece, in piece lengths.
const MAX_BOX: f64 = 2.0;
const MAX_SMOOTH: f64 = 0.3;

/// The mean height of the blocks, in mean throws.
const BLOCK_MEAN: f64 = 1.08;

/// The highest block, in mean heights of the blocks. Ground this close
/// below the Crest setting gets lower blocks.
const HILL_TOP: f64 = 4.0;

/// The throw of a fault grows over this number of fault spacings from the
/// axis.
const GROW: f64 = 1.5;

/// The blocks start to fade at this number of brush radii from the line.
const HILL_FADE: f64 = 0.8;

/// Behind its scarp, a block is `(1 - x)` to this power at `x` of its width.
const BACK: f64 = 1.0;

/// A fault block of throw 1 at `y` fault spacings from its scarp, away from
/// the axis for a fault that faces the axis. The scarp is `scarp` wide, and
/// the block tilts back over `w` behind it. The block is the mean over a
/// box of half width `b`, so it is zero at `-scarp - b` and before, and at
/// `w + b` and after.
fn block(y: f64, scarp: f64, w: f64, b: f64) -> f64 {
    if y + b <= -scarp || y - b >= w {
        return 0.0;
    }
    let area = |y: f64| {
        if y <= -scarp {
            0.0
        } else if y < 0.0 {
            (y + scarp) * (y + scarp) / (2.0 * scarp)
        } else if y < w {
            0.5 * scarp + w / (BACK + 1.0) * (1.0 - (1.0 - y / w).powf(BACK + 1.0))
        } else {
            0.5 * scarp + w / (BACK + 1.0)
        }
    };
    (area(y + b) - area(y - b)) / (2.0 * b)
}

/// The throw along a piece at `q`, from -1 at its start to 1 at its end:
/// `sqrt(1 - q * q)`, as the mean over a box of half width `b`. It is zero at
/// `1 + b` and more from the middle.
fn ellipse(q: f64, b: f64) -> f64 {
    let area = |x: f64| {
        let x = x.clamp(-1.0, 1.0);
        0.5 * (x * (1.0 - x * x).sqrt() + x.asin())
    };
    (area(q + b) - area(q - b)) / (2.0 * b)
}

/// `soft_cap` changes no number below this part of the cap.
const CAP_KNEE: f64 = 0.85;

/// `value` if it is below `CAP_KNEE` of `cap`. Above that, a number that
/// bends toward `cap` and is never above it.
fn soft_cap(value: f64, cap: f64) -> f64 {
    let knee = CAP_KNEE * cap;
    if value <= knee {
        return value;
    }
    let room = cap - knee;
    knee + room * ((value - knee) / room).tanh()
}

/// Half of the width of the narrowest step of the crest before the bend of
/// the noise, in radians.
fn crack_half_width(radius: f64, step: f64) -> f64 {
    (0.02 * radius).max(3.0 * step)
}

/// A number from 0 to 1 from the low 24 bits of a hash. It is exact as a
/// 32-bit number.
fn unit(h: u32) -> f64 {
    f64::from(h & 0xff_ffff) / f64::from(1u32 << 24)
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
            spread: 12.5 / 6371.0,
            magma: 0.0,
            world_km: 6371.0,
            seed: 7,
            limits: Limits::default(),
        }
    }

    fn mix(a: f64, b: f64, t: f64) -> f64 {
        a + (b - a) * t
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
            end: f64::MAX,
        }
    }

    /// Noise with no bend.
    fn calm() -> RidgeNoise {
        RidgeNoise {
            bend: [0.0; 3],
            slow: [-1.0; 3],
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
    fn the_rise_follows_the_depth_of_the_age_and_ends_at_the_radius() {
        let plan = shape(spec(), FINE);
        let r = plan.ridge.radius;
        let v = plan.ridge.spread;
        // The age at the radius is 51 million years, so the depth has the
        // two parts of the curve.
        assert!(r / v > 50.0 && r / v < 52.0);
        let round = CREST_ROUND * plan.step() / v;
        let age = |d: f64| (d / v).hypot(round) - round;
        for k in 1..12 {
            let u = middle(&plan, k);
            // With the calm noise, the crest has the Crest setting.
            let top = level(&plan, u, 0.0);
            assert_eq!(top, f64::from(CREST), "{k}");
            for side in [-1.0, 1.0] {
                let width = width(&plan, calm(), u, side);
                assert!(width > 0.97 * r && width <= r, "{k}: {width}");
                let mut last = top;
                for i in 1..=200 {
                    let d = r * f64::from(i) / 200.0;
                    let new = level(&plan, u, side * d);
                    let height = (new - f64::from(START)) / LIFT;
                    let foot = depth(age(r));
                    let w = (foot - depth(age(d))) / (foot - depth(0.0));
                    let want = w * w * (1.0 + FOOT) / (w + FOOT);
                    assert!((height - want).abs() < 1e-4, "{k} {i}: {height} {want}");
                    assert!(new <= last, "{k} {i}");
                    last = new;
                }
            }
            for across in [r, 1.2 * r, -r, -1.49 * r] {
                assert_eq!(level(&plan, u, across), f64::from(START), "{across}");
            }
        }
        // The depth of Stein and Stein, in meters.
        assert_eq!(depth(0.0), 2600.0);
        assert!((depth(4.0) - 3330.0).abs() < 1e-9);
        assert!((depth(19.999) - depth(20.0)).abs() < 10.0);
        assert!((depth(100.0) - 5497.6).abs() < 0.1);
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

    /// Noise with a bend.
    fn wild() -> RidgeNoise {
        RidgeNoise {
            bend: [1.5, -1.5, 1.5],
            slow: [1.0, -1.0, 0.3],
        }
    }

    #[test]
    fn the_ridge_raises_the_ground_up_to_the_crest() {
        let rough = Ridge {
            offset: 0.03,
            roughness: 2000.0,
            ..spec()
        };
        let plan = shape(rough, FINE);
        let smooth = shape(
            Ridge {
                roughness: 0.0,
                ..rough
            },
            FINE,
        );
        let r = plan.ridge.radius;
        let (mut past_foot, mut hills) = (0, 0);
        for noise in [calm(), wild(), RidgeNoise::default()] {
            for j in 0..60 {
                let u = f64::from(j) * 0.0537;
                for i in 0..=300 {
                    let across = r * (f64::from(i) / 100.0 - 1.5);
                    // Ground at the crest level or above it does not change.
                    for start in [CREST, CREST + 1, CREST + 5000] {
                        let new = plan.level(start, at(u, across), noise);
                        assert_eq!(new, start, "{u} {across}");
                    }
                    // The ground does not go down, and it does not go above
                    // the crest.
                    let new = plan.level(START, at(u, across), noise);
                    assert!((START..=CREST).contains(&new), "{u} {across}");
                    // Past the foot of the rise, the blocks only raise the
                    // ground.
                    let base = smooth.level(START, at(u, across), noise);
                    if base == START && new > START {
                        past_foot += 1;
                    }
                    hills += usize::from(new != base);
                    // Ground a small distance below the crest gets a small
                    // part of the blocks.
                    let near = plan.level(CREST - 10, at(u, across), noise);
                    assert!((CREST - 10..=CREST).contains(&near), "{u} {across}");
                }
            }
        }
        assert!(hills > 30000, "{hills}");
        assert!(past_foot > 1000, "{past_foot}");
    }

    #[test]
    fn a_block_is_zero_outside_its_window_and_has_no_step() {
        for (scarp, w, b) in [(0.2, 1.0, 0.7), (0.01, 0.3, 0.05), (0.4, 6.0, 2.0)] {
            assert_eq!(block(-scarp - b - 1e-12, scarp, w, b), 0.0);
            assert_eq!(block(-scarp - b - 0.3, scarp, w, b), 0.0);
            assert_eq!(block(w + b + 1e-12, scarp, w, b), 0.0);
            assert_eq!(block(w + b + 5.0, scarp, w, b), 0.0);
            let mut last = 0.0;
            for i in 0..=4000 {
                let y = -scarp - b + (w + scarp + 2.0 * b) * f64::from(i) / 4000.0;
                let new = block(y, scarp, w, b);
                assert!((0.0..=1.0).contains(&new), "{y}");
                assert!((new - last).abs() < 0.01, "{y}: {last} {new}");
                last = new;
            }
        }
    }

    #[test]
    fn the_sum_of_the_faults_has_each_fault_in_its_window() {
        let plan = shape(
            Ridge {
                roughness: 2000.0,
                ..spec()
            },
            FINE,
        );
        let f = plan.faulting();
        // The full rate of 25 mm per year gives a magma part of 0.767.
        assert!((f.spacing - 3.33).abs() < 0.01, "{}", f.spacing);
        assert!(f.box_half > 0.1 && f.box_half < 0.5, "{}", f.box_half);
        let mut positive = 0;
        for k in 0..4 {
            for a in 0..300 {
                let along = f64::from(a) * 2.13 - 300.0;
                for xi in 0..300 {
                    let x = f64::from(xi) * 0.071;
                    let sum = plan.faults(&f, k, 0, along, x);
                    assert!(sum >= 0.0);
                    positive += usize::from(sum > 0.0);
                    // A search that starts from more cells gives the same
                    // sum.
                    let more = plan.fault_sum(&f, [k, 3], 0, along, x);
                    assert_eq!(sum, more, "{k} {along} {x}");
                }
            }
        }
        assert!(positive > 300000, "{positive}");
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
        // The top of the rise is round, so some places have the highest
        // level. The crest is in the middle of them.
        let crest_at = |u: f64| {
            let places: Vec<f64> = (-500..=500).map(|i| f64::from(i) * step).collect();
            let levels: Vec<f64> = places.iter().map(|a| level(&plan, u, *a)).collect();
            let highest = levels.iter().copied().fold(f64::MIN, f64::max);
            let top = places.iter().zip(&levels).filter(|(_, l)| **l == highest);
            let top: Vec<f64> = top.map(|(a, _)| *a).collect();
            top.iter().sum::<f64>() / top.len() as f64
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
        // The step of the crest at the start of a group is larger as a
        // mean.
        let (mut strong, mut weak) = (Vec::new(), Vec::new());
        for i in 1..100 {
            let step = (shifts[i] - shifts[i - 1]).abs();
            assert!(step > 0.0);
            let i = i as i32;
            if i == plan.group_start(i >> 2) {
                strong.push(step);
            } else {
                weak.push(step);
            }
        }
        let mean = |steps: &[f64]| steps.iter().sum::<f64>() / steps.len() as f64;
        assert!(mean(&strong) > 2.0 * mean(&weak));
        // At the crest, the step is narrow. At the
        // edge of the rise, a small part of the step is in that distance, so
        // the outline of the rise has no cut.
        let near = 2.0 * crack_half_width(r, plan.step());
        let mut large = 0;
        for i in 1..100 {
            let step = shifts[i] - shifts[i - 1];
            if step.abs() < 0.3 * stairs.offset {
                continue;
            }
            large += 1;
            let u = boundary(&plan, i as i32);
            // The crest of the level, then the crest of the formula.
            let crest = crest_at(u + near) - crest_at(u - near);
            assert!((crest - step).abs() <= 0.02 * step.abs() + 2.0 * r / 500.0);
            let i = i as i32;
            let edge = plan.crest(i, near, r) - plan.crest(i, -near, r);
            assert!(edge.abs() < 0.4 * step.abs(), "{i}: {edge} {step}");
            assert!(edge * step > 0.0, "{i}: {edge} {step}");
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
    fn the_ridge_gets_lower_and_narrower_to_an_end_of_the_line() {
        let full = Ridge {
            roughness: 1500.0,
            valley: 1000.0,
            ..spec()
        };
        for plan in [shape(spec(), FINE), shape(full, FINE)] {
            let r = plan.ridge.radius;
            let ground = f64::from(START);
            for k in 1..12 {
                let u = middle(&plan, k);
                let near = |end: f64, across: f64, noise: RidgeNoise| {
                    let point = RidgePoint {
                        end,
                        ..at(u, across)
                    };
                    f64::from(plan.level(START, point, noise))
                };
                // At the end, nothing changes, also in a crack.
                for across in [0.0, 0.02 * r, -0.3 * r, 1.2 * r] {
                    assert_eq!(near(0.0, across, calm()), ground);
                    assert_eq!(near(0.0, across, wild()), ground);
                    let crack = RidgePoint {
                        end: 0.0,
                        ..at(boundary(&plan, k), across)
                    };
                    assert_eq!(f64::from(plan.level(START, crack, calm())), ground);
                }
                // The ridge has its full shape at 1.2 brush radii from the
                // end.
                for across in [0.0, 0.3 * r, -0.8 * r] {
                    let whole = near(f64::MAX, across, calm());
                    assert_eq!(near(TAPER * r, across, calm()), whole);
                }
                if plan.ridge.roughness > 0.0 {
                    continue;
                }
                // At half of that distance, the crest has half of its
                // height, and the rise is 0.6 times as wide.
                let half = near(0.5 * TAPER * r, 0.0, calm());
                assert!((half - (ground + 0.5 * LIFT)).abs() <= 1.0, "{k}: {half}");
                assert!(near(0.5 * TAPER * r, 0.57 * r, calm()) > ground);
                assert_eq!(near(0.5 * TAPER * r, 0.6 * r, calm()), ground);
            }
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
        assert_eq!(point.end, point.u);
        let point = on(&plan, lonlat_to_dir(25.0, 3.0));
        assert!((point.end - 5f64.to_radians()).abs() < 1e-4, "{point:?}");
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
        // The new part of the plan starts 1.2 brush radii before the old
        // end of the line.
        assert!(last.segments.len() >= 5 && last.tail.len() == 4);
        assert_eq!(last.place(lonlat_to_dir(8.0, 3.0)), RidgePlace::Far);
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
            // No texel is above the crest.
            assert!(after.iter().all(|l| *l <= CREST), "{name}");
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
            lay(&mut map, Ridge { limits, ..full() }, &dirs);
            levels(&map)
        };
        let free = laid(Limits::default());
        assert!(free.iter().all(|l| *l >= START) && free.iter().any(|l| *l > 22000));
        // The ridge only raises the ground, so a floor changes nothing.
        let floor = laid(Limits {
            floor: START + 5000,
            ceiling: 65535,
        });
        assert!(floor == free);
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
