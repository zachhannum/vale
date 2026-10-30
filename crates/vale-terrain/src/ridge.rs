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
    /// The depth of the valley along the crest, in 16-bit steps. Only the
    /// shader uses it. `RidgePlan::level` takes the axial valley from the
    /// spreading rate and the magma.
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

    /// The crest segments of the settings.
    fn segmenting(&self) -> Segmenting {
        let spec = &self.ridge;
        let km = spec.world_km;
        let blend = 1.5 * self.step() + VALLEY_KM / km;
        // A crest segment is long enough for the blends at its two ends.
        let most = (spec.segment / (SEGMENT_BLENDS * blend)).floor().max(1.0);
        let count = (spec.segment * km / SECOND_ORDER_KM)
            .round()
            .clamp(1.0, most);
        // The loop of `turn` has no more than `TURN_CELLS` cells.
        let oldest = TURN_REACH * spec.radius / spec.spread;
        Segmenting {
            length: spec.segment / count,
            group: count as i32,
            turn_cell: TURN_MYR.max(oldest / TURN_CELLS),
            blend,
        }
    }

    /// The place of boundary `j` along the line at the crest, before the
    /// bend of the spreading direction. Crest segment `j` starts at it.
    fn boundary(&self, sg: &Segmenting, j: i32) -> f64 {
        let jitter = 2.0 * unit(hash(j as u32, 0, 0, self.ridge.seed)) - 1.0;
        (f64::from(j) + JITTER * jitter) * sg.length
    }

    /// The boundary that starts group `c` of crest segments. It is a
    /// transform fault. The other boundaries are offsets with no transform
    /// fault. A group has `sg.group` segments as a mean.
    fn group_start(&self, sg: &Segmenting, c: i32) -> i32 {
        let choices = (sg.group / 2 + 1) as u32;
        let h = hash(c as u32, 1, 0, self.ridge.seed);
        c.wrapping_mul(sg.group).wrapping_add((h % choices) as i32)
    }

    /// The group of crest segment `k`.
    fn group(&self, sg: &Segmenting, k: i32) -> i32 {
        let c = k.div_euclid(sg.group);
        if k >= self.group_start(sg, c) {
            c
        } else {
            c - 1
        }
    }

    /// Boundary `j` is a transform fault.
    fn transform(&self, sg: &Segmenting, j: i32) -> bool {
        j == self.group_start(sg, self.group(sg, j))
    }

    /// The shift of crest segment `k` across the line. Most of it is the
    /// shift of its group.
    fn shift(&self, sg: &Segmenting, k: i32) -> f64 {
        let own = 2.0 * unit(hash(k as u32, 4, 0, self.ridge.seed)) - 1.0;
        self.group_shift(self.group(sg, k)) + self.ridge.offset * (1.0 - GROUP_SHIFT) * own
    }

    /// The part of the shift of a crest segment that its group `c` gives.
    fn group_shift(&self, c: i32) -> f64 {
        let part = 2.0 * unit(hash(c as u32, 3, 0, self.ridge.seed)) - 1.0;
        self.ridge.offset * GROUP_SHIFT * part
    }

    /// The move along the line that the history of the spreading direction
    /// gives to the trace of a boundary, at `tau` million years of age from
    /// the line. `tau` is negative on the right. The tangent of the angle of
    /// the spreading direction is a noise of the age, and the move is its
    /// integral. The result is the move in radians, and its change with the
    /// distance across the line.
    fn turn(&self, sg: &Segmenting, tau: f64) -> (f64, f64) {
        let seed = self.ridge.seed ^ SALT_TURN;
        let tangent = |c: i32| TAN_TURN * (2.0 * unit(hash(c as u32, 5, 0, seed)) - 1.0);
        let cell_sum = |c: i32| 0.5 * (tangent(c) + tangent(c + 1));
        let x = tau / sg.turn_cell;
        let n = x.floor();
        let (s, n) = (x - n, n as i32);
        let mut sum = 0.0;
        for c in 0..n {
            sum += cell_sum(c);
        }
        for c in n..0 {
            sum -= cell_sum(c);
        }
        let (h0, h1) = (tangent(n), tangent(n + 1));
        sum += h0 * s + (h1 - h0) * s * s * s * (1.0 - 0.5 * s);
        let slope = h0 + (h1 - h0) * s * s * (3.0 - 2.0 * s);
        (self.ridge.spread * sg.turn_cell * sum, slope)
    }

    /// The move of boundary `j` along the line at `tau` million years of age
    /// from the line, from the migration of the boundary. A transform fault
    /// does not migrate. The result is the move in radians, and its change
    /// with the age.
    fn migration(&self, sg: &Segmenting, j: i32, tau: f64) -> (f64, f64) {
        let seed = self.ridge.seed ^ SALT_MIGRATE;
        let wander = WANDER_KM / self.ridge.world_km;
        let migrate = if self.transform(sg, j) {
            0.0
        } else {
            MIGRATE * sg.length
        };
        let mut sum = (0.0, 0.0);
        for (size, cell, channel) in [(migrate, MIGRATE_MYR, 0), (wander, WANDER_MYR, 1)] {
            let (value, slope) = noise1(tau / cell, j as u32, channel, seed);
            let (zero, _) = noise1(0.0, j as u32, channel, seed);
            sum.0 += 0.5 * size * (value - zero);
            sum.1 += 0.5 * size * slope / cell;
        }
        sum
    }

    /// The trace of boundary `j` at `across`: the place along the line, and
    /// the change of that place with `across`. The trace starts at the
    /// boundary at the crest and follows the history of the spreading out
    /// to the flanks. `turn` is the result of `turn` at `across`.
    fn trace(&self, sg: &Segmenting, j: i32, across: f64, turn: (f64, f64)) -> Trace {
        let v = self.ridge.spread;
        let middle = 0.5 * (self.shift(sg, j.wrapping_sub(1)) + self.shift(sg, j));
        let off = across - middle;
        // The age is round at the crest, so the change has no step there.
        let tau = (off / v).hypot(MIGRATE_ROUND) - MIGRATE_ROUND;
        let (moved, change) = self.migration(sg, j, tau);
        let to_tau = off / v / (tau + MIGRATE_ROUND);
        Trace {
            u: self.boundary(sg, j) + turn.0 + moved,
            slope: turn.1 + change * to_tau / v,
            age: tau,
        }
    }

    /// The depth of the ends of a crest segment below its middle, in
    /// kilometers. `len` is its length in kilometers.
    fn segment_depth(&self, len: f64) -> f64 {
        let full_rate = 2.0 * self.ridge.spread * self.ridge.world_km;
        let part = (len / SECOND_ORDER_KM).clamp(0.5, 1.5);
        0.001 * lerp_table(&SEGMENT_DEPTH, full_rate) * part
    }

    /// The depth of the axial valley and the height of the axial high, in
    /// kilometers.
    fn axial(&self, f: &Faulting) -> (f64, f64) {
        let full_rate = 2.0 * self.ridge.spread * self.ridge.world_km;
        let faulted = (1.0 - f.magma) / (1.0 - magma_share(full_rate));
        let part = faulted.clamp(0.0, VALLEY_MOST).powf(VALLEY_POWER);
        let valley = lerp_table(&AXIAL_VALLEY, full_rate) * part;
        let high = MAGMA_HIGH * smoothstep(HIGH_MAGMA, 1.0, f.magma);
        (
            0.001 * valley,
            0.001 * lerp_table(&AXIAL_HIGH, full_rate).max(high),
        )
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
            magma: m,
            spacing,
            throw,
            box_half: (0.5 * self.step() * spec.world_km / spacing).min(MAX_BOX),
            half_texel: 0.5 * self.step() * spec.world_km,
            level: spec.roughness / ROUGHNESS_KM,
        }
    }

    /// The height of the fault blocks on one side of the axis, in kilometers
    /// of throw. Each crest segment `k` has its own faults. `along` is the
    /// distance from the start of the segment in kilometers, `len` is the
    /// length of the segment in kilometers, and `x` is the distance from the
    /// axis in fault spacings. The faults are in rows along the line, one
    /// row for each fault spacing. Each fault is one piece of a row. Its
    /// height is exactly zero outside its window, so the sum does not depend
    /// on the cells that the search starts from.
    fn faults(&self, f: &Faulting, k: i32, side: u32, [along, len]: [f64; 2], x: f64) -> f64 {
        self.fault_sum(f, [k, 0], side, [along, len], x)
    }

    /// `faults`, with a search of `extra` more cells on each side.
    fn fault_sum(
        &self,
        f: &Faulting,
        [k, extra]: [i32; 2],
        side: u32,
        [along, len]: [f64; 2],
        x: f64,
    ) -> f64 {
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
        let (spacing, heave) = (
            lerp_table(&FAULT_SPACING, f.magma),
            lerp_table(&FAULT_HEAVE, f.magma),
        );
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
                // The middle of a segment has more magma than its ends, so
                // its faults are lower and closer together. At the ends,
                // some rows have no piece.
                let place = middle * piece / len;
                let magma = f.magma + SEGMENT_MAGMA * (std::f64::consts::TAU * (place - 0.5)).cos();
                let grown = lerp_table(&FAULT_HEAVE, magma) / heave;
                let keep = (spacing / lerp_table(&FAULT_SPACING, magma)).min(1.0);
                let kept = smoothstep(-KEEP_SOFT, KEEP_SOFT, keep - draw(i, j, 8));
                let q = (2.0 * along - start - end) / (end - start);
                let bow = (1.0 - q * q).max(0.0);
                let taper = ellipse(q, 2.0 * smooth / (end - start));
                // Each piece is a fault with its own throw and place. Most
                // faults face the axis.
                let height = throw(draw(i, j, 5)) * grown;
                let inward = draw(i, j, 2) < INWARD;
                let at = f64::from(i)
                    + 0.5
                    + FAULT_JITTER * (2.0 * draw(i, j, 1) - 1.0)
                    + STEP_OVER * (2.0 * draw(i, j, 6) - 1.0)
                    + BOW * (2.0 * draw(i, j, 7) - 1.0) * bow;
                let scarp = (height / (DIP_TAN * f.spacing)).clamp(1e-6, SCARP_MAX);
                let y = if inward { x - at } else { at - x };
                let width = (BLOCK_WIDTH * (height / f.throw).sqrt()).clamp(WIDTH_MIN, WIDTH_MAX);
                sum += height * taper * kept * block(y, scarp, width, b);
            }
        }
        sum
    }

    /// The level of a texel on the ridge. `start` is its level from before
    /// the stroke, and `noise` is the noise at its place.
    pub(crate) fn level(&self, start: u16, at: RidgePoint, noise: RidgeNoise) -> u16 {
        self.level_in(start, at, noise, 0)
    }

    /// `level`, with a search of `extra` more boundaries on each side.
    fn level_in(&self, start: u16, at: RidgePoint, noise: RidgeNoise, extra: i32) -> u16 {
        let spec = &self.ridge;
        let (r, t, km, v) = (spec.radius, self.step(), spec.world_km, spec.spread);
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
        // The age of the sea floor, in millions of years. The crest is round
        // over a few texels.
        let round = CREST_ROUND * t / v;
        let age = |d: f64| (d / v).hypot(round) - round;
        // The depth follows the age, and is zero at the crest and at the
        // brush radius.
        let (top, foot) = (depth(0.0), depth(age(r)));
        let lift = (f64::from(spec.crest) - old).max(0.0);
        let f = self.faulting();
        let mean = BLOCK_MEAN * f.throw * f.level;
        // Ground at the crest level gets no faults.
        let low = (lift / (HILL_TOP * mean + 1.0)).min(1.0);
        // The blocks and the troughs fade toward the reach.
        let fade = 1.0 - smoothstep(HILL_FADE, REACH, far);
        let sg = self.segmenting();
        let (valley, high) = self.axial(&f);
        // The rise is flat over the floor of the axial valley.
        let flat_floor = if f.level > 0.0 {
            (valley / VALLEY_FLAT).min(1.0) * FLOOR_KM / km
        } else {
            0.0
        };
        // The rise and the blocks of crest segment `k`, from the trace
        // `lower` at its start to the trace `upper` at its end. The age is
        // the distance from the crest of that segment.
        let magma_seed = spec.seed ^ SALT_MAGMA;
        let segment = |k: i32, lower: f64, upper: f64| {
            let crest = self.shift(&sg, k);
            let off = across - crest;
            let from_crest = off.hypot(at.past);
            let from_rise = from_crest.max(flat_floor) / lens;
            let w = ((foot - depth(age(from_rise))) / (foot - top)).clamp(0.0, 1.0);
            // The foot of the rise has no slope.
            let w = w * w * (1.0 + FOOT) / (w + FOOT);
            // The ends of the segment are deeper than its middle. The rise
            // is lower by the mean height of the blocks, so the mean ground
            // follows the depth.
            let len = (upper - lower).max(SHORTEST_PART * sg.length);
            let place = ((u - lower) / len).clamp(0.0, 1.0);
            let ends = 0.5 * (1.0 + (std::f64::consts::TAU * place).cos());
            // The depth of each end changes with the age and from boundary
            // to boundary. The noise is of the age with a sign from the
            // middle of the two crests at the boundary, so it has no step at
            // the crest, and the two segments at a boundary share it.
            let side = u32::from(off < 0.0);
            let tau = from_crest / v;
            let end = if place < 0.5 { k } else { k.wrapping_add(1) };
            let middle = 0.5 * (self.shift(&sg, end.wrapping_sub(1)) + self.shift(&sg, end));
            let (deeper, _) = noise1(
                (across - middle) / v / SEGMENT_MYR,
                end as u32,
                4,
                magma_seed,
            );
            let deeper = 1.0 - SEGMENT_CHANGE * (1.0 - deeper);
            let sunk = self.segment_depth(len * km) * f.level * ends * deeper;
            // The depth of the ends fades slower than the rise.
            // The throw of a fault grows from the middle of the floor of the
            // axial valley, and the rise is lower by the mean height of the
            // blocks there.
            let grow_end = flat_floor + GROW * f.spacing / km + t;
            let grow = smoothstep(0.5 * flat_floor, grow_end, from_crest);
            let flat = grow * (mean + MAGMA_MEAN * MAGMA_RELIEF * f.level);
            let rise = taper * (w * (lift - low * flat) - low * sunk * w * (2.0 - w)).max(0.0);
            // The magma of the segment changes with the age, so the faults
            // are closer together in some bands and farther apart in others.
            // The throw grows with the spacing.
            // The segments between two transform faults share the bands of
            // the magma, so the bands go on past an offset with no transform
            // fault. The bands are of the age with a sign from the middle of
            // the group, and they bend along the line.
            let group = self.group(&sg, k);
            let center = self.group_shift(group);
            let (wave, _) = noise1(u * km / WAVE_KM, group as u32, 2, magma_seed);
            let band_at = |off: f64| {
                let tau = (off / v + WAVE_MYR * wave) / MAGMA_MYR;
                noise1_round(tau, group as u32, 0, magma_seed)
            };
            let (band, slope) = band_at(across - center);
            let (zero, _) = band_at(crest - center);
            let away = if off < 0.0 { -1.0 } else { 1.0 };
            let rows = away * MAGMA_BAND * MAGMA_MYR * v * km / f.spacing;
            let rate = 1.0 + MAGMA_BAND * slope;
            let grown = rate.powf(-THROW_SPACING);
            // Near the ends of the segment, the faults hook toward the
            // crest of the next segment.
            let (near, next) = if place < 0.5 {
                (place, k.wrapping_sub(1))
            } else {
                (1.0 - place, k.wrapping_add(1))
            };
            let hook = (1.0 - near / HOOK_PART).max(0.0);
            // The axial valley and the rift mountains fade toward a
            // transform fault, where the nodal deep is.
            let transform = self.transform(&sg, if place < 0.5 { k } else { k.wrapping_add(1) });
            let fade_out = if transform {
                smoothstep(0.0, 1.0, hook)
            } else {
                0.0
            };
            let toward = if (self.shift(&sg, next) - crest) * off > 0.0 {
                -1.0
            } else {
                1.0
            };
            let x = from_crest * km / f.spacing
                + rows * (band - zero)
                + HOOK_ROWS * hook * hook * toward;
            let along = [(u - lower) * km, len * km];
            let faults = grow * grown * self.faults(&f, k, side, along, x);
            // Sediment fills the lows between the blocks up to twice its
            // thickness above the low of the hills: `HILL_LOW` of the mean
            // height of the blocks. The depth of the age is the depth with
            // the sediment, so the blocks are lower by about the rise of
            // their mean.
            let held = BLOCK_MEAN * f.throw * grow * grown;
            let fill = (2.0 * SEDIMENT_RATE * tau - HILL_LOW * held).max(0.0);
            let ponded = smooth_max(faults, fill, SEDIMENT_SOFT * fill);
            let gain = smooth_max(held, fill, held) - held;
            let blocks = ponded - gain;
            // More magma gives a thicker crust, so the bands of the magma
            // are also bands of the depth.
            // The bands are narrower at their tops than at their feet.
            let top = 0.5 * (1.0 + band);
            let blocks = blocks + grow * MAGMA_RELIEF * top * top;
            // The axial valley, its rift mountains, and the axial high.
            let (floor, rim) = axial_valley(from_crest * km);
            let down = valley * (1.0 - RIM) * (1.0 - VALLEY_END * fade_out) * floor;
            // At the inside corner, on the side of the transform fault, the
            // rift mountains are higher.
            let inside = if toward < 0.0 { INSIDE_CORNER } else { 0.0 };
            // The rift mountains change along the segment, and each side
            // has its own.
            let (peaks, _) = noise1(u * km / PEAKS_KM, k as u32, side + 6, magma_seed);
            let rim = rim * (1.0 + PEAKS * peaks);
            let up = valley * RIM * (1.0 + (inside - 1.0) * fade_out) * rim
                + high * bump(from_crest * km / HIGH_KM);
            let axis = f.level * low * taper;
            (
                rise,
                blocks * f.level * low * taper * fade,
                [down * axis, up * axis],
            )
        };
        // The traces of the boundaries near the texel. Crest segment `j`
        // has the part of a texel that is past trace `j` and before trace
        // `j + 1`, with a blend over `sg.blend` on each side of a trace.
        // Each part is exactly zero past its blend, so the sum does not
        // depend on the boundaries that the search starts from.
        let turn = self.turn(&sg, across / v);
        let i = ((u - turn.0) / sg.length).floor() as i32;
        let (mut rise, mut hills, mut deep) = (0.0, 0.0, 0.0);
        let (mut down, mut above) = (0.0, 0.0);
        let mut lower: Option<(f64, f64)> = None;
        for j in i - 1 - extra..=i + 2 + extra {
            let trace = self.trace(&sg, j, across, turn);
            // The distance from the trace, square to it.
            let off = (u - trace.u) / trace.slope.hypot(1.0);
            let past = smoothstep(-sg.blend, sg.blend, off);
            if let Some((below, before)) = lower {
                let weight = before - past;
                if weight > 0.0 {
                    let (a, b, [c, d]) = segment(j - 1, below, trace.u);
                    rise += weight * a;
                    hills += weight * b;
                    down += weight * c;
                    above += weight * d;
                }
            }
            lower = Some((trace.u, past));
            // A trough along the trace, from the offset of the crest. It
            // gets shallower with the age, and a transform fault has a deeper
            // one.
            let (before, after) = (self.shift(&sg, j - 1), self.shift(&sg, j));
            let step = (after - before).abs() * km;
            let transform = self.transform(&sg, j);
            let kind = if transform { 1.0 } else { OFFSET_TROUGH };
            let trough = kind * (TROUGH_DEPTH * step).min(TROUGH_MAX);
            let width = sg.blend + TROUGH_KM / km;
            let filled = 1.0 - TROUGH_FILL * smoothstep(0.0, TROUGH_MYR, trace.age);
            deep += trough * filled * bump(off / width);
            // A deep at each end of the crest at a transform fault. It is
            // long along the transform fault, and its middle is inside it.
            if transform {
                let nodal = (NODAL_DEPTH * step).min(NODAL_MAX);
                let size = sg.blend + NODAL_KM / km;
                let long = sg.blend + NODAL_ALONG_KM / km;
                for (crest, other) in [(before, after), (after, before)] {
                    let middle = crest + (other - crest).signum() * NODAL_IN * long;
                    let q = (off / size).hypot((across - middle) / long);
                    deep += nodal * bump(q);
                }
            }
        }
        let deep = deep * f.level * low * taper * fade;
        // The highest blocks bend down below the Crest setting.
        let hills = soft_cap(hills, (lift - rise).max(0.0));
        // The rift mountains and the axial high go above the Crest setting.
        let up = (rise + hills).min(lift) - deep - down + above;
        let change = at.flow * up;
        let new = (old + (change + 0.5).floor()).clamp(0.0, 65535.0);
        spec.limits.apply(old, new) as u16
    }
}

/// The faults of a stroke.
struct Faulting {
    /// The magma part of the spreading.
    magma: f64,
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

/// The crest segments of a stroke.
struct Segmenting {
    /// The mean length of a crest segment, in radians.
    length: f64,
    /// The mean number of crest segments between two transform faults.
    group: i32,
    /// The age of a cell of the noise of the spreading direction, in
    /// millions of years.
    turn_cell: f64,
    /// Half of the width of the blend of two crest segments at their
    /// boundary, in radians.
    blend: f64,
}

/// The trace of a boundary at a distance across the line.
struct Trace {
    /// The place along the line, in radians.
    u: f64,
    /// The change of `u` with the distance across the line.
    slope: f64,
    /// The age of the sea floor at the trace, in millions of years.
    age: f64,
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
const SALT_TURN: u32 = 0x6c8e_9cf5;
const SALT_MIGRATE: u32 = 0x3d4f_a1c7;
const SALT_MAGMA: u32 = 0x9b05_688c;

/// The largest move of a boundary, as a part of the mean length of a crest
/// segment.
const JITTER: f64 = 0.3;

/// The mean length of a crest segment between two offsets with no
/// transform fault, in kilometers (Macdonald and others 1991). The Segment
/// setting is the mean distance between two transform faults.
const SECOND_ORDER_KM: f64 = 130.0;

/// A crest segment is at least this number of blend widths long.
const SEGMENT_BLENDS: f64 = 10.0;

/// The width of the valley of a transform fault, in kilometers. Two crest
/// segments blend over it and 1.5 texels on each side of their boundary.
const VALLEY_KM: f64 = 4.0;

/// The part of the shift of a crest segment that is the shift of its group.
/// The rest is the offset of the segment from its group.
const GROUP_SHIFT: f64 = 0.8;

/// The tangent of the largest angle of the spreading direction to its mean,
/// 8 degrees.
const TAN_TURN: f64 = 0.14;

/// The age of a cell of the noise of the spreading direction, in millions
/// of years, and the largest number of cells out to `TURN_REACH` brush
/// radii.
const TURN_MYR: f64 = 8.0;
const TURN_CELLS: f64 = 16.0;
const TURN_REACH: f64 = 1.8;

/// The largest move of an offset with no transform fault from its place at
/// the crest, as a part of the mean length of a crest segment, and the age
/// of a cell of its noise in millions of years. The offset migrates along
/// the crest, so its trace is oblique to the crest (Hey 1977).
const MIGRATE: f64 = 0.1;
const MIGRATE_MYR: f64 = 4.0;

/// The largest small move of each trace, in kilometers, and the age of a
/// cell of its noise in millions of years.
const WANDER_KM: f64 = 3.0;
const WANDER_MYR: f64 = 0.8;

/// The age of a trace is round over this number of millions of years at
/// the crest.
const MIGRATE_ROUND: f64 = 0.5;

/// The depth of the ends of a crest segment below its middle at a full
/// spreading rate in millimeters per year, in meters (Lin and others 1990;
/// Shaw and Lin 1993).
const SEGMENT_DEPTH: [(f64, f64); 3] = [(20.0, 600.0), (50.0, 300.0), (100.0, 100.0)];

/// The shortest crest segment for its depth and its magma, as a part of the
/// mean length.
const SHORTEST_PART: f64 = 0.2;

/// The magma part of the spreading at the middle of a crest segment is
/// higher than its mean by this number, and at the ends it is lower.
const SEGMENT_MAGMA: f64 = 0.03;

/// A piece of a fault at the end of a segment fades in over this part of
/// the chance that it is there.
const KEEP_SOFT: f64 = 0.1;

/// The age of a cell of the noise of the magma, in millions of years, and
/// the largest change of the spacing of the faults, as a part of the
/// spacing. The throw changes with the spacing to this power (Behn and Ito
/// 2008).
const MAGMA_MYR: f64 = 2.0;
const MAGMA_BAND: f64 = 0.12;
const THROW_SPACING: f64 = 1.0;

/// The change of the depth with the magma, from the least to the most, in
/// kilometers.
const MAGMA_RELIEF: f64 = 1.0;

/// The mean height of the bands of the magma, as a part of
/// `MAGMA_RELIEF`.
const MAGMA_MEAN: f64 = 0.33;

/// The bands of the magma move by up to this age, in millions of years,
/// with a noise along the segment that has cells of `WAVE_KM` kilometers.
const WAVE_MYR: f64 = 0.8;
const WAVE_KM: f64 = 40.0;

/// The depth of the ends of a segment changes with the age, with cells of
/// this number of millions of years, by up to `SEGMENT_CHANGE` of the
/// depth.
const SEGMENT_MYR: f64 = 6.0;
const SEGMENT_CHANGE: f64 = 0.35;

/// The faults hook over this part of a crest segment at each end, by up to
/// `HOOK_ROWS` fault spacings.
const HOOK_PART: f64 = 0.15;
const HOOK_ROWS: f64 = 1.0;

/// The depth of the trough along a transform fault for each kilometer of
/// offset, and its largest depth, in kilometers. A trough along an offset
/// with no transform fault is `OFFSET_TROUGH` times as deep.
const TROUGH_DEPTH: f64 = 0.012;
const TROUGH_MAX: f64 = 1.2;
const OFFSET_TROUGH: f64 = 0.6;

/// Half of the width of a trough past the blend, in kilometers.
const TROUGH_KM: f64 = 6.0;

/// The part of the depth of a trough that sediment fills, over this number
/// of millions of years.
const TROUGH_FILL: f64 = 0.4;
const TROUGH_MYR: f64 = 20.0;

/// The depth of a deep at an end of a crest at a transform fault for each
/// kilometer of offset, and its largest depth, in kilometers. Past the
/// blend, the deep is `NODAL_ALONG_KM` long along the transform fault and
/// `NODAL_KM` wide. Its middle is `NODAL_IN` of its length inside the
/// transform fault.
const NODAL_DEPTH: f64 = 0.005;
const NODAL_MAX: f64 = 0.6;
const NODAL_KM: f64 = 12.0;
const NODAL_ALONG_KM: f64 = 30.0;
const NODAL_IN: f64 = 0.3;

/// The depth of the axial valley at a full spreading rate in millimeters
/// per year, in meters, at the magma part of that rate (Chen and Morgan
/// 1990; Ito and Behn 2008). Less magma gives a deeper valley: the depth
/// is in proportion to the part of the spreading that the faults take, to
/// the power `VALLEY_POWER`, up to `VALLEY_MOST` times the depth of the
/// table.
const AXIAL_VALLEY: [(f64, f64); 4] = [(20.0, 1500.0), (35.0, 1000.0), (50.0, 450.0), (70.0, 0.0)];
const VALLEY_POWER: f64 = 1.0;
const VALLEY_MOST: f64 = 1.5;

/// The height of the axial high at a full spreading rate in millimeters
/// per year, in meters. Near a magma part of 1, a high of up to
/// `MAGMA_HIGH` meters takes the place of the valley.
const AXIAL_HIGH: [(f64, f64); 3] = [(60.0, 0.0), (90.0, 250.0), (130.0, 350.0)];
const MAGMA_HIGH: f64 = 300.0;
const HIGH_MAGMA: f64 = 0.85;

/// Half of the width of the floor of the axial valley, and the distance from
/// the axis to the crests of the rift mountains, in kilometers. The depth of
/// the valley is from its floor to the crests of the rift mountains. The
/// crests are `RIM` of that depth above the depth of the age, and the
/// rift mountains are `RIM_KM` wide on each side of their crests. The
/// axial high is `HIGH_KM` wide on each side of the axis.
const FLOOR_KM: f64 = 8.0;
const WALL_KM: f64 = 16.0;
const RIM: f64 = 0.5;
const RIM_KM: f64 = 18.0;
const HIGH_KM: f64 = 5.0;

/// Near a transform fault, over `HOOK_PART` of the segment, the valley
/// loses this part of its depth, and the nodal deep takes its place. There,
/// the rift mountains are `INSIDE_CORNER` times as high on the side of the
/// transform fault (Tucholke and Lin 1994), and they fade on the other side.
const VALLEY_END: f64 = 0.5;

/// The rise is flat over the floor of an axial valley this deep or deeper,
/// in kilometers.
const VALLEY_FLAT: f64 = 0.5;
const INSIDE_CORNER: f64 = 1.4;

/// The height of the rift mountains changes by up to this part, with a
/// noise along the line that has cells of `PEAKS_KM` kilometers.
const PEAKS: f64 = 0.35;
const PEAKS_KM: f64 = 25.0;

/// The sediment on the sea floor, in kilometers for each million years of
/// age (Goff and Arbic 2010). It fills the lows between the fault blocks
/// up to twice its thickness, with a round edge of up to `SEDIMENT_SOFT`
/// kilometers. Both are in kilometers of throw, so the Roughness setting
/// scales them with the blocks.
const SEDIMENT_RATE: f64 = 0.005;
const SEDIMENT_SOFT: f64 = 1.5;
const HILL_LOW: f64 = 0.5;

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

/// A small width in radians. The Segment setting is at least 20 times this
/// width.
fn crack_half_width(radius: f64, step: f64) -> f64 {
    (0.02 * radius).max(3.0 * step)
}

/// A number from 0 to 1 from the low 24 bits of a hash. It is exact as a
/// 32-bit number.
fn unit(h: u32) -> f64 {
    f64::from(h & 0xff_ffff) / f64::from(1u32 << 24)
}

/// Noise of one number with cells of size 1, from -1 to 1, and its change.
/// The value at each whole number is from a hash of that number and of `a`
/// and `b`, and a smoothstep joins two values.
fn noise1(x: f64, a: u32, b: u32, seed: u32) -> (f64, f64) {
    let n = x.floor();
    let (s, n) = (x - n, n as i32);
    let value = |c: i32| 2.0 * unit(hash(c as u32, a, b, seed)) - 1.0;
    let (h0, h1) = (value(n), value(n.wrapping_add(1)));
    (
        h0 + (h1 - h0) * s * s * (3.0 - 2.0 * s),
        (h1 - h0) * 6.0 * s * (1.0 - s),
    )
}

/// As `noise1`, with a cubic curve through four values. The curve has no
/// flat part at each whole number, so a change goes over the full cell.
fn noise1_round(x: f64, a: u32, b: u32, seed: u32) -> (f64, f64) {
    let n = x.floor();
    let (s, n) = (x - n, n as i32);
    let value = |c: i32| 2.0 * unit(hash(c as u32, a, b, seed)) - 1.0;
    let [h0, h1, h2, h3] = [-1, 0, 1, 2].map(|i| value(n.wrapping_add(i)));
    // Catmull-Rom: the slope at each value is half of the change over the
    // two cells next to it.
    let (m1, m2) = (0.5 * (h2 - h0), 0.5 * (h3 - h1));
    let (s2, s3) = (s * s, s * s * s);
    let value = (2.0 * s3 - 3.0 * s2 + 1.0) * h1
        + (s3 - 2.0 * s2 + s) * m1
        + (-2.0 * s3 + 3.0 * s2) * h2
        + (s3 - s2) * m2;
    let slope = (6.0 * s2 - 6.0 * s) * (h1 - h2)
        + (3.0 * s2 - 4.0 * s + 1.0) * m1
        + (3.0 * s2 - 2.0 * s) * m2;
    (value.clamp(-1.0, 1.0), slope)
}

/// The larger of `a` and `b`, with a round join over `k`. With `k` zero, it
/// is the larger number.
fn smooth_max(a: f64, b: f64, k: f64) -> f64 {
    if k <= 0.0 {
        return a.max(b);
    }
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.max(b) + 0.25 * k * h * h
}

/// The shape of the axial valley at `x` kilometers from the axis: 1 on its
/// floor and 0 past its walls, and the rift mountains: 0 on the floor, 1 at
/// their crests at the top of the walls, and 0 far from them.
fn axial_valley(x: f64) -> (f64, f64) {
    let wall = smoothstep(FLOOR_KM, WALL_KM, x);
    let rim = if x < WALL_KM {
        wall
    } else {
        bump((x - WALL_KM) / RIM_KM)
    };
    (1.0 - wall, rim)
}

/// A bump of height 1 at zero that is exactly zero at 1 and past it.
fn bump(q: f64) -> f64 {
    let x = (1.0 - q * q).max(0.0);
    x * x
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

    /// The place of boundary `i` along the line at the crest, with no bend.
    fn boundary(plan: &RidgePlan, i: i32) -> f64 {
        plan.boundary(&plan.segmenting(), i)
    }

    /// The place of the trace of boundary `j` along the line at `across`.
    fn trace_at(plan: &RidgePlan, j: i32, across: f64) -> f64 {
        let sg = plan.segmenting();
        let turn = plan.turn(&sg, across / plan.ridge.spread);
        plan.trace(&sg, j, across, turn).u
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
        let straight = shape(
            Ridge {
                offset: 0.0,
                ..rough
            },
            FINE,
        );
        let r = plan.ridge.radius;
        // A trough, two deeps, and the axial valley can be at one place. The
        // rift mountains go above the crest.
        let f = plan.faulting();
        let (valley, high) = plan.axial(&f);
        let deepest = (TROUGH_MAX + 2.0 * NODAL_MAX + (1.0 - RIM) * valley) * f.level;
        let highest =
            f64::from(CREST) + ((1.0 + PEAKS) * INSIDE_CORNER * RIM * valley + high) * f.level;
        let (mut past_foot, mut hills, mut lower, mut count) = (0, 0, 0, 0);
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
                    // The ground goes above the crest by no more than the
                    // rift mountains. Only a trough, a deep, or the axial
                    // valley takes it down, by no more than its depth.
                    let new = plan.level(START, at(u, across), noise);
                    assert!(f64::from(new) <= highest, "{u} {across}");
                    assert!(f64::from(new) >= f64::from(START) - deepest, "{u} {across}");
                    lower += usize::from(new < START);
                    count += 1;
                    // With no offset, there is no trough and no deep.
                    let flat = f64::from(straight.level(START, at(u, across), noise));
                    let valley_floor = f64::from(START) - valley * f.level;
                    assert!((valley_floor..=highest).contains(&flat), "{u} {across}");
                    // Past the foot of the rise, the blocks only raise the
                    // ground.
                    let base = smooth.level(START, at(u, across), noise);
                    assert!(base >= START, "{u} {across}");
                    if base == START && new > START {
                        past_foot += 1;
                    }
                    hills += usize::from(new != base);
                    // Ground a small distance below the crest gets a small
                    // part of the blocks, the troughs, and the rift
                    // mountains.
                    let near = plan.level(CREST - 10, at(u, across), noise);
                    assert!((CREST - 20..=CREST + 5).contains(&near), "{u} {across}");
                }
            }
        }
        assert!(hills > 30000, "{hills}");
        assert!(past_foot > 1000, "{past_foot}");
        assert!(lower > 0 && lower < count / 20, "{lower} {count}");
    }

    /// The mean level over many places along the line at `km` kilometers
    /// from the line, with the calm noise.
    fn mean_at(plan: &RidgePlan, km: f64) -> f64 {
        let across = km / plan.ridge.world_km;
        let levels = (0..400).map(|i| level(plan, 0.3 + f64::from(i) * 0.0013, across));
        levels.sum::<f64>() / 400.0
    }

    /// A ridge with no offset at a full spreading rate in millimeters per
    /// year.
    fn at_rate(rate: f64) -> RidgePlan {
        let spec = Ridge {
            roughness: 2000.0,
            spread: 0.5 * rate / 6371.0,
            ..spec()
        };
        shape(spec, FINE)
    }

    #[test]
    fn the_axial_valley_follows_the_spreading_rate() {
        // The valley is deep at a slow rate, shallow at an intermediate
        // rate, and gone at a fast rate, where a high takes its place.
        let depths = [20.0, 40.0, 60.0, 100.0].map(|rate| {
            let plan = at_rate(rate);
            plan.axial(&plan.faulting())
        });
        assert!((1.0..=2.0).contains(&depths[0].0), "{depths:?}");
        assert!(
            depths[1].0 < 0.6 * depths[0].0 && depths[1].0 > 0.2,
            "{depths:?}"
        );
        assert!(depths[2].0 < 0.3, "{depths:?}");
        assert_eq!(depths[3].0, 0.0);
        assert!((0.2..=0.4).contains(&depths[3].1), "{depths:?}");
        assert_eq!(depths[0].1, 0.0);
        // Less magma gives a deeper valley.
        let plan = shape(
            Ridge {
                magma: -0.1,
                ..at_rate(20.0).ridge
            },
            FINE,
        );
        assert!(plan.axial(&plan.faulting()).0 > depths[0].0);

        // At a slow rate, the floor is below the crest and the rift
        // mountains about 30 kilometers apart are above it.
        let plan = at_rate(20.0);
        let f = plan.faulting();
        let valley = depths[0].0 * f.level;
        let crest = f64::from(CREST);
        let floor = mean_at(&plan, 0.0);
        assert!(floor < crest - 0.4 * valley, "{floor} {valley}");
        let profile: Vec<f64> = (0..=40).map(|km| mean_at(&plan, f64::from(km))).collect();
        let top = (0..=40)
            .max_by(|a, b| profile[*a].total_cmp(&profile[*b]))
            .unwrap();
        assert!((12..=20).contains(&top), "{top}");
        assert!(profile[top] > floor + 0.7 * valley, "{profile:?}");
        // The floor is flat over about 10 kilometers.
        assert!(
            (mean_at(&plan, 5.0) - floor).abs() < 0.1 * valley,
            "{} {floor} {valley}",
            mean_at(&plan, 5.0)
        );

        // At a fast rate, the axis is a high above the crest, about 10
        // kilometers wide.
        let plan = at_rate(100.0);
        let high = depths[3].1 * plan.faulting().level;
        let axis = mean_at(&plan, 0.0);
        assert!(axis > crest + 0.8 * high, "{axis} {high}");
        assert!(mean_at(&plan, 5.0) < crest + 0.1 * high);
    }

    #[test]
    fn sediment_makes_old_sea_floor_smoother() {
        // A slow rate gives old sea floor inside the brush radius.
        let plan = at_rate(12.0);
        let r = plan.ridge.radius;
        // The mean change of the level over one kilometer across the line,
        // near `across`.
        let rough = |across: f64| {
            let step = 1.0 / plan.ridge.world_km;
            let mut sum = 0.0;
            for i in 0..100 {
                for k in 0..20 {
                    let (u, a) = (0.3 + f64::from(i) * 0.0021, across + f64::from(k) * step);
                    sum += (level(&plan, u, a + step) - level(&plan, u, a)).abs();
                }
            }
            sum / 2000.0
        };
        // The sediment fills the lows of old sea floor. Without it, the
        // change at 0.75 brush radii is within 5 percent of the change at
        // 0.15 brush radii.
        let (young, old) = (rough(0.15 * r), rough(0.75 * r));
        assert!(old < 0.8 * young, "{young} {old}");
        // Sediment does not reach past the ridge.
        for across in [REACH * r * 1.001, 2.0 * r] {
            assert_eq!(level(&plan, 0.4, across), f64::from(START));
        }
        // The round join is never below the larger number, and it is the
        // larger number far from the other.
        for (a, b) in [(0.0, 0.3), (0.3, 0.0), (0.2, 0.25), (1.0, 0.1)] {
            assert!(smooth_max(a, b, 0.2) >= a.max(b));
        }
        assert_eq!(smooth_max(1.0, 0.1, 0.2), 1.0);
        assert_eq!(smooth_max(0.4, 0.3, 0.0), 0.4);
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
                    let sum = plan.faults(&f, k, 0, [along, 130.0], x);
                    assert!(sum >= 0.0);
                    positive += usize::from(sum > 0.0);
                    // A search that starts from more cells gives the same
                    // sum.
                    let more = plan.fault_sum(&f, [k, 3], 0, [along, 130.0], x);
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
        let sg = plan.segmenting();
        let (r, length) = (plan.ridge.radius, sg.length);
        // The Segment setting of 637 km gives 5 crest segments of 127 km
        // between two transform faults as a mean.
        assert_eq!(sg.group, 5);
        assert!((length * plan.ridge.world_km - 127.4).abs() < 0.1);
        // The mean distance between two boundaries is the segment length,
        // and the distances are not equal.
        let gaps: Vec<f64> = (0..400)
            .map(|i| boundary(&plan, i + 1) - boundary(&plan, i))
            .collect();
        let mean = gaps.iter().sum::<f64>() / 400.0;
        assert!((mean / length - 1.0).abs() < 0.01, "{mean}");
        let low = gaps.iter().copied().fold(f64::MAX, f64::min);
        let high = gaps.iter().copied().fold(f64::MIN, f64::max);
        assert!(low > 0.39 * length && low < 0.6 * length, "{low}");
        assert!(high < 1.61 * length && high > 1.4 * length, "{high}");
        // The transform faults are between 2 and 8 segments apart, and the
        // Segment setting is the mean distance between them.
        let transforms: Vec<i32> = (0..400).filter(|&j| plan.transform(&sg, j)).collect();
        let apart: Vec<i32> = transforms.windows(2).map(|t| t[1] - t[0]).collect();
        assert!(apart.iter().all(|a| (2..=8).contains(a)), "{apart:?}");
        let mean = f64::from(transforms[transforms.len() - 1] - transforms[0])
            / (transforms.len() - 1) as f64;
        assert!((mean - 5.0).abs() < 0.3, "{mean}");
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
        let shifts: Vec<f64> = (0..100).map(|k| plan.shift(&sg, k)).collect();
        for (k, &shift) in shifts.iter().enumerate() {
            let k = k as i32;
            let (from, to) = (trace_at(&plan, k, shift), trace_at(&plan, k + 1, shift));
            for part in [0.3, 0.5, 0.7] {
                let crest = crest_at(mix(from, to, part));
                assert!((crest - shift).abs() <= step, "{k}: {crest} {shift}");
            }
            assert!(shift.abs() <= stairs.offset, "{shift}");
        }
        let highest = shifts.iter().copied().fold(f64::MIN, f64::max);
        let lowest = shifts.iter().copied().fold(f64::MAX, f64::min);
        assert!(highest > 0.6 * stairs.offset && lowest < -0.6 * stairs.offset);
        // The step of the crest at a transform fault is larger as a mean.
        let (mut strong, mut weak) = (Vec::new(), Vec::new());
        for i in 1..100 {
            let step = (shifts[i] - shifts[i - 1]).abs();
            assert!(step > 0.0);
            if plan.transform(&sg, i as i32) {
                strong.push(step);
            } else {
                weak.push(step);
            }
        }
        let mean = |steps: &[f64]| steps.iter().sum::<f64>() / steps.len() as f64;
        assert!(mean(&strong) > 2.0 * mean(&weak));
        // At the crest, the step is as narrow as the blend.
        let mut large = 0;
        for i in 1..100 {
            let step = shifts[i] - shifts[i - 1];
            if step.abs() < 0.3 * stairs.offset {
                continue;
            }
            large += 1;
            let i = i as i32;
            let (before, after) = (
                trace_at(&plan, i, shifts[i as usize - 1]),
                trace_at(&plan, i, shifts[i as usize]),
            );
            let near = 2.0 * sg.blend;
            let crest = crest_at(before.max(after) + near) - crest_at(before.min(after) - near);
            assert!(
                (crest - step).abs() <= 2.0 * r / 500.0,
                "{i}: {crest} {step}"
            );
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
    fn a_transform_fault_leaves_a_step_of_the_depth_that_fades_with_the_age() {
        let plan = shape(
            Ridge {
                offset: 0.04,
                ..spec()
            },
            FINE,
        );
        let sg = plan.segmenting();
        let (r, v) = (plan.ridge.radius, plan.ridge.spread);
        let round = CREST_ROUND * plan.step() / v;
        let age = |d: f64| (d / v).hypot(round) - round;
        let foot = depth(age(r));
        // The height of the rise at `d` from the crest.
        let rise = |d: f64| {
            let w = ((foot - depth(age(d))) / (foot - depth(0.0))).clamp(0.0, 1.0);
            LIFT * w * w * (1.0 + FOOT) / (w + FOOT)
        };
        let mut count = 0;
        for j in 1..200 {
            let (before, after) = (plan.shift(&sg, j - 1), plan.shift(&sg, j));
            if !plan.transform(&sg, j) || (after - before).abs() < 0.02 {
                continue;
            }
            count += 1;
            let mut steps = Vec::new();
            for side in [-1.0, 1.0] {
                for d in [0.1, 0.3, 0.6] {
                    // A place past the two crests on one side.
                    let across = if side > 0.0 {
                        before.max(after) + d * r
                    } else {
                        before.min(after) - d * r
                    };
                    let at_trace = trace_at(&plan, j, across);
                    let older = level(&plan, at_trace - 1.5 * sg.blend, across);
                    let younger = level(&plan, at_trace + 1.5 * sg.blend, across);
                    let start = f64::from(START);
                    assert!((older - start - rise((across - before).abs())).abs() <= 1.0);
                    assert!((younger - start - rise((across - after).abs())).abs() <= 1.0);
                    let step = older - younger;
                    let want = rise((across - before).abs()) - rise((across - after).abs());
                    assert!((step - want).abs() <= 2.0, "{j}: {step} {want}");
                    steps.push(step.abs());
                }
                // The step fades with the age.
                let n = steps.len();
                assert!(steps[n - 3] > steps[n - 2] && steps[n - 2] > steps[n - 1]);
                assert!(steps[n - 3] > 1000.0, "{steps:?}");
            }
        }
        assert!(count > 5, "{count}");
    }

    #[test]
    fn the_traces_bend_with_the_spreading_direction() {
        let plan = shape(
            Ridge {
                offset: 0.04,
                ..spec()
            },
            FINE,
        );
        let sg = plan.segmenting();
        let (r, km) = (plan.ridge.radius, plan.ridge.world_km);
        let wander = WANDER_KM / km;
        let places: Vec<f64> = (-150..=150).map(|i| r * f64::from(i) / 100.0).collect();
        // The move of a trace from its boundary at the crest.
        let moves = |j: i32| -> Vec<f64> {
            let start = boundary(&plan, j);
            places
                .iter()
                .map(|&a| trace_at(&plan, j, a) - start)
                .collect()
        };
        let transforms: Vec<i32> = (0..60).filter(|&j| plan.transform(&sg, j)).collect();
        let first = moves(transforms[0]);
        // The trace of a transform fault is not straight.
        let (a, b) = (first[0], first[first.len() - 1]);
        let bend = first
            .iter()
            .enumerate()
            .map(|(i, m)| (m - mix(a, b, i as f64 / 300.0)).abs())
            .fold(0.0, f64::max);
        assert!(bend * km > 10.0, "{}", bend * km);
        // The traces of all transform faults bend together.
        for &j in &transforms[1..] {
            for (m, n) in moves(j).iter().zip(&first) {
                assert!((m - n).abs() <= 2.0 * wander + 1e-12, "{j}");
            }
        }
        // The angle of the spreading direction is no more than 8 degrees.
        for &a in &places {
            let (_, slope) = plan.turn(&sg, a / plan.ridge.spread);
            assert!(slope.abs() <= TAN_TURN);
        }
        // An offset with no transform fault migrates, so its trace moves
        // from the transform faults by up to a part of the segment length,
        // and some of it is oblique.
        let mut oblique = false;
        for j in 0..60 {
            if plan.transform(&sg, j) {
                continue;
            }
            for (i, (m, n)) in moves(j).iter().zip(&first).enumerate() {
                assert!((m - n).abs() <= MIGRATE * sg.length + 2.0 * wander + 1e-12);
                let turn = plan.turn(&sg, places[i] / plan.ridge.spread);
                let trace = plan.trace(&sg, j, places[i], turn);
                oblique |= trace.slope.abs() > 0.4;
            }
        }
        assert!(oblique);
    }

    #[test]
    fn the_turn_is_the_sum_of_its_slope() {
        let plan = shape(spec(), FINE);
        let sg = plan.segmenting();
        let v = plan.ridge.spread;
        assert_eq!(plan.turn(&sg, 0.0).0, 0.0);
        let h = 1e-4;
        for i in -400..400 {
            let tau = f64::from(i) * 0.37;
            let (_, slope) = plan.turn(&sg, tau);
            let change = (plan.turn(&sg, tau + h).0 - plan.turn(&sg, tau - h).0) / (2.0 * h);
            assert!((change - v * slope).abs() < 1e-6 * v, "{tau}");
        }
    }

    #[test]
    fn the_level_does_not_depend_on_the_boundaries_that_the_search_starts_from() {
        let full = Ridge {
            offset: 0.05,
            roughness: 2000.0,
            ..spec()
        };
        let plan = shape(full, FINE);
        let r = plan.ridge.radius;
        let mut changed = 0;
        for j in 0..400 {
            let u = 0.3 + f64::from(j) * 0.00731;
            for i in 0..=120 {
                let across = r * (f64::from(i) / 40.0 - 1.5);
                let p = lonlat_to_dir(u.to_degrees(), across.to_degrees());
                let noise = plan.noise(p);
                let new = plan.level_in(START, at(u, across), noise, 0);
                assert_eq!(
                    new,
                    plan.level_in(START, at(u, across), noise, 3),
                    "{u} {across}"
                );
                changed += usize::from(new != START);
                // Nothing changes at the reach.
                if i == 0 || i == 120 {
                    assert_eq!(new, START);
                }
            }
        }
        assert!(changed > 30000, "{changed}");
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
        // Along the line, over the traces of the boundaries and over the
        // middle of segments. At a trace, the step of the depth spreads over
        // the blend.
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
            // No texel is above the crest by more than the rift mountains.
            let plan = line.plan(0);
            let f = plan.faulting();
            let rim = (1.0 + PEAKS) * INSIDE_CORNER * RIM * plan.axial(&f).0 * f.level;
            let highest = f64::from(CREST) + rim;
            assert!(after.iter().all(|l| f64::from(*l) <= highest), "{name}");
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
        let laid_with = |spec: Ridge| {
            let mut map = Heightmap::new(128, START);
            lay(&mut map, spec, &dirs);
            levels(&map)
        };
        let laid = |limits: Limits| laid_with(Ridge { limits, ..full() });
        let free = laid(Limits::default());
        assert!(free.iter().any(|l| *l > 22000));
        // Only the troughs, the deeps, and the axial valley take the ground
        // down, by no more than their depth.
        let plan = shape(full(), 128);
        let (valley, _) = plan.axial(&plan.faulting());
        let deepest = (TROUGH_MAX + 2.0 * NODAL_MAX + valley) * full().roughness / ROUGHNESS_KM;
        assert!(
            free.iter()
                .all(|l| f64::from(*l) >= f64::from(START) - deepest)
        );
        // The troughs and the deeps of the offsets take some ground lower
        // than a ridge with no offset does.
        let straight = laid_with(Ridge {
            offset: 0.0,
            ..full()
        });
        assert!(free.iter().zip(&straight).any(|(a, b)| a + 200 < *b));
        // A floor above the ground keeps the ground of the troughs and the
        // deeps.
        let floor = laid(Limits {
            floor: START + 5000,
            ceiling: 65535,
        });
        assert!(floor.iter().zip(&free).all(|(a, b)| *a == START.max(*b)));
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
