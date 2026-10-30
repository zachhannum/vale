//! The ocean ridge brush.
//!
//! The brush works on the whole line of a stroke. Each texel near the line
//! gets a place: the distance along the line and the distance across it. The
//! new level is a function of that place, of noise at the direction of the
//! texel, and of the level from before the stroke. Thus a texel takes the
//! ridge one time for each stroke.

use std::f64::consts::FRAC_PI_2;
use std::ops::Range;

use crate::cube::{ELEV_MAX, ELEV_MIN, FACES, face_dir, unwarp};
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
    /// The 16-bit steps of one kilometer of relief. The tests set it to
    /// zero for the rise alone.
    relief: f64,
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
    /// The 16-bit steps of one kilometer of relief.
    pub(crate) relief: f64,
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
            spread: spec.spread.max(1e-12),
            world_km: spec.world_km.max(1.0),
            ..spec
        };
        RidgeLine {
            spec,
            relief: LEVELS_PER_KM,
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
            relief: self.relief,
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
            level: self.relief,
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

    /// The staircase of the large faults of one wall of the axial valley,
    /// in parts of the sum of their throws. The segments of group `group`
    /// share the faults, so the walls have no break at an offset with no
    /// transform fault. `along` and `x` are the distance along the line and
    /// from the axis of the valley, and `zone` is half of the width of the
    /// floor and the width of the wall, all in kilometers. Each fault is in
    /// pieces along the line, as the faults of the blocks are.
    fn walls(
        &self,
        f: &Faulting,
        [group, side]: [i32; 2],
        along: f64,
        x: f64,
        zone: [f64; 2],
    ) -> f64 {
        let seed = self.ridge.seed ^ SALT_WALL;
        let key = (group as u32).wrapping_mul(2).wrapping_add(side as u32);
        let draw = |n: i32, j: i32, channel: u32| {
            unit(hash(
                n as u32,
                j as u32,
                key.wrapping_mul(8).wrapping_add(channel),
                seed,
            ))
        };
        let [floor, wall] = zone;
        let row = wall / f64::from(WALL_FAULTS);
        let mut sum = 0.0;
        for n in 0..WALL_FAULTS {
            let piece = mix(WALL_PIECE[0], WALL_PIECE[1], draw(n, -1, 0));
            let phase = draw(n, -1, 1);
            let cell = (along / piece + phase).floor() as i32;
            for j in cell - 1..=cell + 1 {
                let middle =
                    f64::from(j) + 0.5 - phase + PIECE_JITTER * (2.0 * draw(n, j, 2) - 1.0);
                let half = 0.5 * (1.0 + OVERLAP * draw(n, j, 3));
                let smooth = f.half_texel.min(MAX_SMOOTH * piece);
                let (start, end) = ((middle - half) * piece, (middle + half) * piece);
                if along <= start - smooth || along >= end + smooth {
                    continue;
                }
                let q = (2.0 * along - start - end) / (end - start);
                let taper = ellipse(q, 2.0 * smooth / (end - start));
                let throw = (0.5 + draw(n, j, 4)) / f64::from(WALL_FAULTS);
                let jitter = WALL_JITTER * (2.0 * draw(n, j, 5) - 1.0);
                let at = floor + row * (f64::from(n) + 0.5 + jitter);
                let scarp = (throw * row / DIP_TAN).max(1e-3);
                sum += throw * taper * block(x - at, scarp, MOUNT_KM, f.half_texel);
            }
        }
        sum
    }

    /// Lumps of lava from 0 to about 1 high, at `[along, across]`
    /// kilometers, in cells of `cell` kilometers. The segments of group
    /// `group` share them.
    fn lumps(&self, group: i32, [along, across]: [f64; 2], cell: f64) -> f64 {
        let seed = self.ridge.seed ^ SALT_LUMP;
        let (a, c) = (along / cell, across / cell);
        let (ia, ic) = (a.floor() as i32, c.floor() as i32);
        let mut sum = 0.0;
        for i in ia - 1..=ia + 1 {
            for j in ic - 1..=ic + 1 {
                let key = (group as u32).wrapping_mul(4);
                let draw =
                    |channel: u32| unit(hash(i as u32, j as u32, key.wrapping_add(channel), seed));
                let (pa, pc) = (f64::from(i) + draw(0), f64::from(j) + draw(1));
                let radius = mix(LUMP_SIZE, 1.0, draw(2));
                // Most lumps are low, and a few are high.
                let height = draw(3) * draw(3);
                sum += height * bump((a - pa).hypot(c - pc) / radius);
            }
        }
        sum
    }

    /// The fracture zone along the trace of boundary `j`. `step` is the
    /// offset of the crest at the boundary in kilometers, and `age` is the
    /// age at the trace. The trough is deeper and wider at a larger offset,
    /// and each transform fault has its own strength, so a few troughs are
    /// deep and most are faint.
    fn fracture(&self, j: i32, step: f64, transform: bool, age: f64) -> Fracture {
        let seed = self.ridge.seed ^ SALT_FRACTURE;
        let draw = |channel: u32| unit(hash(j as u32, channel, 0, seed));
        let size = step / TROUGH_OFFSET_KM;
        let strength = mix(STRENGTH[0], STRENGTH[1], draw(0));
        let kind = if transform { 1.0 } else { OFFSET_TROUGH };
        // The trace leaves the transform fault at this age.
        let out = 0.5 * step / (self.ridge.spread * self.ridge.world_km);
        let active = 1.0 + ACTIVE * (1.0 - smoothstep(out, out + ACTIVE_MYR, age));
        let (patches, _) = noise1(age / PATCH_MYR, j as u32, 1, seed);
        let patchy = PATCH * smoothstep(0.0, PATCH_OLD_MYR, age);
        let keep = 1.0 - patchy * (1.0 - smoothstep(-0.5, 0.5, patches));
        let filled = 1.0 - TROUGH_FILL * smoothstep(0.0, TROUGH_MYR, age);
        let full = TROUGH_DEPTH * size.powf(TROUGH_POWER) * strength * kind * active;
        let depth = full.min(TROUGH_MAX) * keep * filled;
        let half = (TROUGH_KM * (size * strength * kind).powf(TROUGH_WIDENS))
            .clamp(TROUGH_KM_RANGE[0], TROUGH_KM_RANGE[1]);
        let ridge = if transform && draw(2) < TRANSVERSE_SHARE {
            TRANSVERSE * depth
        } else {
            0.0
        };
        Fracture {
            depth,
            half,
            ridge,
            side: if draw(3) < 0.5 { -1.0 } else { 1.0 },
        }
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
        // The rise is flat over the floor of the axial valley, and it
        // follows the axis of the valley.
        let valley_part = if f.level > 0.0 {
            (valley / VALLEY_FLAT).min(1.0)
        } else {
            0.0
        };
        let flat_floor = valley_part * FLOOR_KM / km;
        // The rise and the blocks of crest segment `k`, from the trace
        // `lower` at its start to the trace `upper` at its end. The age is
        // the distance from the crest of that segment.
        let magma_seed = spec.seed ^ SALT_MAGMA;
        let segment = |k: i32, lower: f64, upper: f64| {
            let crest = self.shift(&sg, k);
            let off = across - crest;
            let from_crest = off.hypot(at.past);
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
            // At an offset with no transform fault, the axis of the axial
            // valley bends from this crest to the middle of the two crests
            // over `JOG` of the segment, so the valley has no break there.
            let jog = if transform {
                0.0
            } else {
                0.5 * (1.0 - smoothstep(0.0, JOG, near))
            };
            let valley_axis = crest + (self.shift(&sg, next) - crest) * jog;
            let valley_off = across - valley_axis;
            let from_axis = valley_off.hypot(at.past) * km;
            // The floor of the valley is narrow at the middle of the segment
            // and wide at its ends.
            let floor_half = mix(FLOOR_HALF[0], FLOOR_HALF[1], ends);
            let wall = mix(WALL_WIDTH[0], WALL_WIDTH[1], ends);
            // The depth of the ends fades slower than the rise.
            // The throw of a fault grows from the middle of the floor of the
            // axial valley, and the rise is lower by the mean height of the
            // blocks there.
            let grow_end = floor_half + GROW * f.spacing + t * km;
            let grow = smoothstep(0.5 * floor_half, grow_end, from_axis.min(from_crest * km));
            let flat = grow * (mean + MAGMA_MEAN * MAGMA_RELIEF * f.level);
            // Near the axis, the rise follows the axial valley, so the
            // valley is at the top of the rise at an offset too.
            let near_axis =
                1.0 - valley_part * (1.0 - smoothstep(0.0, RISE_BEND_KM, from_crest * km));
            let from_rise = mix(from_axis / km, from_crest, near_axis);
            let from_rise = from_rise.max(flat_floor) / lens;
            let w = ((foot - depth(age(from_rise))) / (foot - top)).clamp(0.0, 1.0);
            // The foot of the rise has no slope.
            let w = w * w * (1.0 + FOOT) / (w + FOOT);
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
            // The distance along the line from the first boundary of the
            // group. It stays small, so the shader keeps its digits.
            let origin = self.boundary(&sg, self.group_start(&sg, group));
            let group_along = (u - origin) * km;
            let (wave, _) = noise1(group_along / WAVE_KM, group as u32, 2, magma_seed);
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
            // Sediment ponds in the lows between the blocks, up to twice its
            // thickness above the low of the hills: `HILL_LOW` of the mean
            // height of the blocks. It fills `POND` of the depth of a low
            // below that level, and the hills above it keep their tops. The
            // depth of the age is the depth with the sediment, so the blocks
            // are lower by about the rise of their mean.
            let held = BLOCK_MEAN * f.throw * grow * grown;
            let fill = (2.0 * SEDIMENT_RATE * tau - HILL_LOW * held).max(0.0);
            let blocks = pond(faults, fill) - (pond(held, fill) - held);
            // More magma gives a thicker crust, so the bands of the magma
            // are also small, smooth bands of the depth.
            let blocks = blocks + grow * MAGMA_RELIEF * 0.5 * (1.0 + band);
            // The axial valley is deeper at the ends of the segment. A broad
            // down-warp and the staircase of the large faults of its walls
            // make it, and the faults lift the rift mountains.
            let valley = valley * (1.0 + VALLEY_ALONG * (2.0 * ends - 1.0));
            let down = DOWN * valley * (1.0 - VALLEY_END * fade_out) * bump(from_axis / DOWN_KM);
            // At the inside corner, on the side of the transform fault, the
            // rift mountains are higher.
            let inside = if toward < 0.0 { INSIDE_CORNER } else { 0.0 };
            let valley_side = u32::from(valley_off < 0.0);
            let stairs = self.walls(
                &f,
                [group, valley_side as i32],
                group_along,
                from_axis,
                [floor_half, wall],
            );
            let stairs = STAIR * valley * (1.0 + (inside - 1.0) * fade_out) * stairs;
            // The floor is lumps of lava, with an axial volcanic ridge along
            // the middle of the segment. The ridge has gaps, and it is
            // highest at the middle of the segment.
            let on_floor = 1.0 - smoothstep(floor_half, floor_half + 0.5 * wall, from_axis);
            let cell = LUMP_KM.max(2.0 * LUMP_TEXELS * f.half_texel);
            let lumps = self.lumps(group, [group_along, valley_off * km], cell);
            let (gaps, _) = noise1(group_along / AVR_PIECE_KM, k as u32, 9, magma_seed);
            let ridge_top = smoothstep(AVR_END, 0.5, near) * smoothstep(-AVR_GAPS, AVR_GAPS, gaps);
            let axial_ridge = AVR_HEIGHT * ridge_top * bump(from_axis / AVR_KM);
            let floor = LUMP_HEIGHT * lumps * on_floor + axial_ridge;
            // Past the walls of the axial valley, the young crust is a broad
            // band above the depth of its age.
            let past_walls = smoothstep(floor_half, floor_half + wall, from_axis);
            let band_km = BAND_KM.min(BAND_RADIUS * r * km);
            let band = BAND * valley * past_walls * bump(from_axis / band_km);
            let relief = stairs - down + band + floor * (1.0 - fade_out);
            let axis = f.level * low * taper;
            (
                rise,
                blocks * f.level * low * taper * fade + relief * axis,
                high * bump(from_axis / HIGH_KM) * axis,
            )
        };
        // The traces of the boundaries near the texel. Crest segment `j`
        // has the part of a texel that is past trace `j` and before trace
        // `j + 1`, with a blend over `sg.blend` on each side of a trace.
        // Each part is exactly zero past its blend, so the sum does not
        // depend on the boundaries that the search starts from.
        let turn = self.turn(&sg, across / v);
        let i = ((u - turn.0) / sg.length).floor() as i32;
        let (mut rise, mut hills, mut deep, mut above) = (0.0, 0.0, 0.0, 0.0);
        let mut ridges = 0.0;
        let mut lower: Option<(f64, f64)> = None;
        for j in i - 1 - extra..=i + 2 + extra {
            let trace = self.trace(&sg, j, across, turn);
            // The distance from the trace, square to it.
            let off = (u - trace.u) / trace.slope.hypot(1.0);
            let past = smoothstep(-sg.blend, sg.blend, off);
            if let Some((below, before)) = lower {
                let weight = before - past;
                if weight > 0.0 {
                    let (a, b, c) = segment(j - 1, below, trace.u);
                    rise += weight * a;
                    hills += weight * b;
                    above += weight * c;
                }
            }
            lower = Some((trace.u, past));
            // A fracture zone along the trace: a trough with steep walls,
            // and at some, a transverse ridge on one side.
            let (before, after) = (self.shift(&sg, j - 1), self.shift(&sg, j));
            let step = (after - before).abs() * km;
            let transform = self.transform(&sg, j);
            let fz = self.fracture(j, step, transform, trace.age);
            let width = 0.5 * sg.blend + fz.half / km;
            deep += fz.depth * (1.0 - smoothstep(TROUGH_FLOOR, 1.0, (off / width).abs()));
            let ridge_at = off - fz.side * TRANSVERSE_AT * width;
            ridges += fz.ridge * bump(ridge_at / (TRANSVERSE_WIDTH * width));
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
        let axis = f.level * low * taper;
        let deep = deep * axis * fade;
        let hills = hills + ridges * axis * fade;
        // The highest blocks bend down below the Crest setting. The rift
        // mountains can go above it by `ABOVE_CREST` of the depth of the
        // axial valley, and the axial high goes above it.
        let room = ABOVE_CREST * valley * axis;
        let hills = soft_cap(hills, (lift - rise).max(0.0) + room);
        let up = (rise + hills).min(lift + room) - deep + above;
        let change = at.flow * up;
        let new = (old + (change + 0.5).floor()).clamp(0.0, 65535.0);
        spec.limits.apply(old, new) as u16
    }
}

/// The numbers of `RidgePlan::level` that are the same for each texel of a
/// plan. The shader gets them.
#[cfg(feature = "gpu")]
pub(crate) struct RidgeShape {
    /// `Segmenting::length` and `Segmenting::group`.
    pub length: f64,
    pub group: i32,
    pub turn_cell: f64,
    pub blend: f64,
    /// The roundness of the age at the crest, in millions of years.
    pub round: f64,
    /// The depth at the crest and at the brush radius, in meters.
    pub top: f64,
    pub foot: f64,
    /// The mean height of the blocks, in 16-bit steps.
    pub mean: f64,
    pub magma: f64,
    pub spacing: f64,
    pub throw: f64,
    /// The heave of a fault at the magma part, in kilometers.
    pub heave: f64,
    pub box_half: f64,
    pub half_texel: f64,
    pub level: f64,
    /// The depth of the axial valley and the height of the axial high, in
    /// kilometers.
    pub valley: f64,
    pub high: f64,
    pub valley_part: f64,
    pub flat_floor: f64,
    pub band_km: f64,
    /// The depth of the ends of a crest segment of `SECOND_ORDER_KM`, in
    /// kilometers.
    pub segment_depth: f64,
}

#[cfg(feature = "gpu")]
impl RidgePlan {
    /// The numbers of `level` that are the same for each texel. The steps
    /// are those of `level_in`.
    pub(crate) fn shape(&self) -> RidgeShape {
        let spec = &self.ridge;
        let (r, t, km, v) = (spec.radius, self.step(), spec.world_km, spec.spread);
        let round = CREST_ROUND * t / v;
        let age = |d: f64| (d / v).hypot(round) - round;
        let f = self.faulting();
        let sg = self.segmenting();
        let (valley, high) = self.axial(&f);
        let valley_part = if f.level > 0.0 {
            (valley / VALLEY_FLAT).min(1.0)
        } else {
            0.0
        };
        RidgeShape {
            length: sg.length,
            group: sg.group,
            turn_cell: sg.turn_cell,
            blend: sg.blend,
            round,
            top: depth(0.0),
            foot: depth(age(r)),
            mean: BLOCK_MEAN * f.throw * f.level,
            magma: f.magma,
            spacing: f.spacing,
            throw: f.throw,
            heave: lerp_table(&FAULT_HEAVE, f.magma),
            box_half: f.box_half,
            half_texel: f.half_texel,
            level: f.level,
            valley,
            high,
            valley_part,
            flat_floor: valley_part * FLOOR_KM / km,
            band_km: BAND_KM.min(BAND_RADIUS * r * km),
            segment_depth: self.segment_depth(SECOND_ORDER_KM),
        }
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

/// The fracture zone along the trace of a boundary.
struct Fracture {
    /// The depth of the trough, in kilometers.
    depth: f64,
    /// Half of the width of the trough past half of the blend, in
    /// kilometers.
    half: f64,
    /// The height of the transverse ridge, in kilometers.
    ridge: f64,
    /// The side of the transverse ridge along the line: -1 or 1.
    side: f64,
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
const SALT_WALL: u32 = 0x4c95_e1a3;
const SALT_LUMP: u32 = 0x2d8e_7f19;
const SALT_FRACTURE: u32 = 0x5f3a_c2e1;

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
const MAGMA_BAND: f64 = 0.26;
const THROW_SPACING: f64 = 0.65;

/// The change of the depth with the magma, from the least to the most, in
/// kilometers of throw. The bands of the magma show mostly in the spacing
/// and the throw of the faults (Olive and others 2015).
const MAGMA_RELIEF: f64 = 0.12;

/// The mean height of the bands of the magma, as a part of
/// `MAGMA_RELIEF`.
const MAGMA_MEAN: f64 = 0.5;

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

/// The depth of the trough of a fracture zone, in kilometers, is
/// `TROUGH_DEPTH` times the offset in `TROUGH_OFFSET_KM` to the power
/// `TROUGH_POWER`, times a strength of the transform fault from
/// `STRENGTH[0]` to `STRENGTH[1]`. It is no deeper than `TROUGH_MAX`. A
/// trough along an offset with no transform fault is `OFFSET_TROUGH` times
/// as deep.
const TROUGH_DEPTH: f64 = 1.3;
const TROUGH_OFFSET_KM: f64 = 100.0;
const TROUGH_POWER: f64 = 1.3;
const STRENGTH: [f64; 2] = [0.5, 1.6];
const TROUGH_MAX: f64 = 1.5;
const OFFSET_TROUGH: f64 = 0.8;

/// Half of the width of a trough past half of the blend, in kilometers, is
/// `TROUGH_KM` times the offset in `TROUGH_OFFSET_KM` times the strength,
/// to the power `TROUGH_WIDENS`, from `TROUGH_KM_RANGE[0]` to
/// `TROUGH_KM_RANGE[1]`. The floor of a trough is `TROUGH_FLOOR` of its
/// width.
const TROUGH_KM: f64 = 10.0;
const TROUGH_WIDENS: f64 = 0.8;
const TROUGH_KM_RANGE: [f64; 2] = [2.0, 20.0];
const TROUGH_FLOOR: f64 = 0.3;

/// Along the transform fault, and for `ACTIVE_MYR` past it, the trough is
/// up to `ACTIVE` deeper.
const ACTIVE: f64 = 0.4;
const ACTIVE_MYR: f64 = 3.0;

/// With the age, the trough breaks into patches `PATCH_MYR` long. Over
/// `PATCH_OLD_MYR`, the patches grow to `PATCH` of the depth.
const PATCH_MYR: f64 = 4.0;
const PATCH_OLD_MYR: f64 = 10.0;
const PATCH: f64 = 0.5;

/// The part of the depth of a trough that sediment fills, over this number
/// of millions of years.
const TROUGH_FILL: f64 = 0.4;
const TROUGH_MYR: f64 = 20.0;

/// `TRANSVERSE_SHARE` of the transform faults have a transverse ridge on
/// one side of the trough, `TRANSVERSE` times as high as the trough is deep.
/// Its middle is `TRANSVERSE_AT` widths of the trough from the trace, and
/// its half width is `TRANSVERSE_WIDTH` widths of the trough.
const TRANSVERSE_SHARE: f64 = 0.5;
const TRANSVERSE: f64 = 0.6;
const TRANSVERSE_AT: f64 = 1.4;
const TRANSVERSE_WIDTH: f64 = 0.9;

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

/// The rise is flat over `FLOOR_KM` kilometers on each side of the crest
/// when the axial valley is `VALLEY_FLAT` kilometers deep or deeper. The
/// axial high is `HIGH_KM` kilometers wide on each side of the axis.
const FLOOR_KM: f64 = 8.0;
const VALLEY_FLAT: f64 = 0.5;
const HIGH_KM: f64 = 5.0;

/// Half of the width of the floor of the axial valley, and the width of
/// each wall, in kilometers, at the middle and at the ends of a segment.
/// The depth of the valley is `VALLEY_ALONG` less than the mean at the
/// middle, and that much more at the ends (Macdonald 1982).
const FLOOR_HALF: [f64; 2] = [5.0, 8.0];
const WALL_WIDTH: [f64; 2] = [10.0, 14.0];
const VALLEY_ALONG: f64 = 0.15;

/// The axial valley is a down-warp of `DOWN` of its depth that is
/// `DOWN_KM` kilometers wide on each side of the axis, and a staircase of
/// `WALL_FAULTS` large faults in each wall that face the axis (Behn and Ito
/// 2008). The throws of the faults of a wall add up to `STAIR` of the depth
/// as a mean. Each fault lifts its footwall, and the footwall tilts back
/// and gets lower over `MOUNT_KM` kilometers away from the axis: the rift
/// mountains.
const DOWN: f64 = 0.85;
const DOWN_KM: f64 = 35.0;

/// Past the walls of the axial valley, the young crust is a band that is
/// `BAND` times the depth of the axial valley high, and `BAND_KM` kilometers
/// wide on each side of the axis, or `BAND_RADIUS` of the brush radius if
/// that is less.
const BAND: f64 = 0.25;
const BAND_KM: f64 = 120.0;
const BAND_RADIUS: f64 = 0.25;
const WALL_FAULTS: i32 = 4;
const STAIR: f64 = 0.65;
const MOUNT_KM: f64 = 24.0;

/// A wall fault is up to `WALL_JITTER` of the space between two wall
/// faults from its place, and its pieces are from `WALL_PIECE[0]` to
/// `WALL_PIECE[1]` kilometers long.
const WALL_JITTER: f64 = 0.3;
const WALL_PIECE: [f64; 2] = [15.0, 40.0];

/// At an offset with no transform fault, the axis of the axial valley bends
/// to the next crest over this part of the segment.
const JOG: f64 = 0.25;

/// Out to this distance from the crest, in kilometers, the rise follows
/// the axis of the axial valley more than the crest.
const RISE_BEND_KM: f64 = 70.0;

/// Near a transform fault, over `HOOK_PART` of the segment, the valley
/// loses `VALLEY_END` of its depth, and the nodal deep takes its place.
/// There, the rift mountains are `INSIDE_CORNER` times as high on the side
/// of the transform fault (Tucholke and Lin 1994), and they fade on the
/// other side.
const VALLEY_END: f64 = 0.5;
const INSIDE_CORNER: f64 = 1.25;

/// The rift mountains go above the Crest setting by no more than this part
/// of the depth of the axial valley.
const ABOVE_CREST: f64 = 0.2;

/// The lumps of lava on the floor of the axial valley are in cells of
/// `LUMP_KM` kilometers, or of `LUMP_TEXELS` texels if that is larger. A
/// lump is up to `LUMP_HEIGHT` kilometers high, and its radius is from
/// `LUMP_SIZE` of a cell to one cell.
const LUMP_KM: f64 = 4.0;
const LUMP_TEXELS: f64 = 1.2;
const LUMP_HEIGHT: f64 = 0.25;
const LUMP_SIZE: f64 = 0.4;

/// The axial volcanic ridge is up to `AVR_HEIGHT` kilometers high and
/// `AVR_KM` kilometers wide on each side of the axis. It is absent over
/// `AVR_END` of the segment at each end. Its gaps come from a noise along
/// the line with cells of `AVR_PIECE_KM` kilometers, and `AVR_GAPS` sets
/// how much of the noise is a gap.
const AVR_HEIGHT: f64 = 0.25;
const AVR_KM: f64 = 2.0;
const AVR_END: f64 = 0.12;
const AVR_PIECE_KM: f64 = 18.0;
const AVR_GAPS: f64 = 0.3;

/// The sediment on the sea floor, in kilometers for each million years of
/// age (Goff and Arbic 2010). It ponds in the lows between the fault
/// blocks up to twice its thickness, and fills `POND` of the depth of a
/// low below that level, with a round edge of `SEDIMENT_SOFT` of the
/// level. All are in kilometers of throw.
const SEDIMENT_RATE: f64 = 0.005;
const SEDIMENT_SOFT: f64 = 0.3;
const POND: f64 = 0.3;
const HILL_LOW: f64 = 1.0;

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

/// The 16-bit steps of one kilometer of height.
const LEVELS_PER_KM: f64 = 1000.0 * 65535.0 / (ELEV_MAX - ELEV_MIN);

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
const HILL_TOP: f64 = 3.6;

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

/// Ground at `height` with sediment that ponds up to `level`. A low below
/// the level fills by `POND` of its depth, and ground above it does not
/// change.
fn pond(height: f64, level: f64) -> f64 {
    let full = smooth_max(height, level, SEDIMENT_SOFT * level);
    height + POND * (full - height)
}

/// `a` to `b`, at `t` from 0 to 1.
fn mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
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
            spread: 12.5 / 6371.0,
            magma: 0.0,
            world_km: 6371.0,
            seed: 7,
            limits: Limits::default(),
        }
    }

    /// A plan with no segments and no relief, for the formula of the rise.
    fn shape(spec: Ridge, n: usize) -> RidgePlan {
        let mut line = RidgeLine::new(spec, n);
        line.relief = 0.0;
        line.plan(0)
    }

    /// A plan with no segments, for the formula.
    fn rough(spec: Ridge, n: usize) -> RidgePlan {
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
        let rough_spec = Ridge {
            offset: 0.03,
            ..spec()
        };
        let plan = rough(rough_spec, FINE);
        let smooth = shape(rough_spec, FINE);
        let straight = rough(
            Ridge {
                offset: 0.0,
                ..rough_spec
            },
            FINE,
        );
        let r = plan.ridge.radius;
        // A trough, two deeps, and the axial valley can be at one place. The
        // rift mountains go above the crest.
        let f = plan.faulting();
        let (valley, high) = plan.axial(&f);
        let floor = DOWN * (1.0 + VALLEY_ALONG) * valley;
        let deepest = (TROUGH_MAX + 2.0 * NODAL_MAX + floor) * f.level;
        // The level is a whole number, so it can be one step higher.
        let highest = f64::from(CREST) + (ABOVE_CREST * valley + high) * f.level + 1.0;
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
                    let valley_floor = f64::from(START) - floor * f.level;
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
            spread: 0.5 * rate / 6371.0,
            ..spec()
        };
        rough(spec, FINE)
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

        // At a slow rate, the floor is below the crest, and the rift
        // mountains are about the depth of the valley above the floor.
        let plan = at_rate(20.0);
        let f = plan.faulting();
        let valley = depths[0].0 * f.level;
        let crest = f64::from(CREST);
        let floor = mean_at(&plan, 0.0);
        assert!(floor < crest - 0.8 * valley, "{floor} {valley}");
        let profile: Vec<f64> = (0..=40).map(|km| mean_at(&plan, f64::from(km))).collect();
        let top = (0..=40)
            .max_by(|a, b| profile[*a].total_cmp(&profile[*b]))
            .unwrap();
        assert!((15..=35).contains(&top), "{top}");
        let relief = (profile[top] - floor) / valley;
        assert!((0.8..=1.3).contains(&relief), "{relief}");

        // The walls are staircases of scarps that face the axis. Away
        // from the axis, a scarp goes up and the ground behind it tilts
        // down a little.
        let fine = Faulting {
            half_texel: 1e-3,
            ..plan.faulting()
        };
        let zone = [FLOOR_HALF[0], WALL_WIDTH[0]];
        let (mut scarps, mut down) = (0, 0.0f64);
        for i in 0..200 {
            let along = f64::from(i) * 3.7;
            let mut rising = false;
            let mut last = plan.walls(&fine, [3, i % 2], along, 0.0, zone);
            for j in 1..=1500 {
                let x = f64::from(j) * 0.02;
                let next = plan.walls(&fine, [3, i % 2], along, x, zone);
                let slope = (next - last) / 0.02;
                if slope > 0.2 && !rising {
                    scarps += 1;
                }
                rising = slope > 0.2;
                down = down.min(slope);
                last = next;
            }
        }
        let scarps = f64::from(scarps) / 200.0;
        assert!((2.5..=4.5).contains(&scarps), "{scarps}");
        assert!(down > -0.1, "{down}");

        // The floor is rough: lumps and an axial volcanic ridge with gaps.
        let along: Vec<f64> = (0..2000)
            .map(|i| level(&plan, 0.3 + f64::from(i) * 0.5 / 6371.0, 0.0))
            .collect();
        let change = along.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / 1999.0;
        assert!(change > 0.007 * valley, "{change}");

        // The valley is deeper and wider at the ends of a segment than at
        // its middle, at an offset with no transform fault too.
        let sg = plan.segmenting();
        let (mut ends, mut middles) = (0.0, 0.0);
        for k in 10..60 {
            if plan.transform(&sg, k) {
                continue;
            }
            let (from, to) = (boundary(&plan, k - 1), boundary(&plan, k));
            for km in [0.0, 6.0] {
                let across = km / plan.ridge.world_km;
                ends += level(&plan, mix(from, to, 0.95), across)
                    + level(&plan, mix(from, to, 0.05), across);
                middles += 2.0 * level(&plan, mix(from, to, 0.5), across);
            }
        }
        assert!(ends < middles, "{ends} {middles}");

        // At a fast rate, the axis is a high above the crest, about 10
        // kilometers wide.
        let plan = at_rate(100.0);
        let high = depths[3].1 * plan.faulting().level;
        let axis = mean_at(&plan, 0.0);
        assert!(axis > crest + 0.8 * high, "{axis} {high}");
        assert!(mean_at(&plan, 5.0) < crest + 0.1 * high);
    }

    #[test]
    fn more_magma_gives_closer_and_smaller_faults() {
        // Behn & Ito (2008): the spacing and the heave of the faults fall
        // as the magma part of the spreading grows.
        let faulting = |magma: f64| shape(Ridge { magma, ..spec() }, FINE).faulting();
        let (low, mid, high) = (faulting(-0.2), faulting(0.0), faulting(0.1));
        assert!(low.magma < mid.magma && mid.magma < high.magma);
        assert!(low.spacing > mid.spacing && mid.spacing > high.spacing);
        assert!(low.throw > mid.throw && mid.throw > high.throw);
        // A faster ridge has more magma, so its faults are closer.
        let fast = Ridge {
            spread: 4.0 * spec().spread,
            ..spec()
        };
        assert!(shape(fast, FINE).faulting().spacing < mid.spacing);
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
        // The sediment fills the lows of old sea floor, and the old sea
        // floor stays rough.
        let (young, old) = (rough(0.15 * r), rough(0.75 * r));
        assert!(old < young && old > 0.8 * young, "{young} {old}");
        // A low fills by a part of its depth, so it keeps a part of its
        // relief. The ground above the level of the sediment keeps all of
        // its relief.
        let fill = 0.4;
        assert!((pond(0.0, fill) - POND * fill).abs() < 1e-12);
        let low = pond(0.2, fill) - pond(0.0, fill);
        assert!((low - 0.2 * (1.0 - POND)).abs() < 1e-12, "{low}");
        assert_eq!(pond(1.0, fill), 1.0);
        assert_eq!(pond(1.5, fill) - pond(1.0, fill), 0.5);
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
        let plan = rough(spec(), FINE);
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
    fn a_larger_offset_leaves_a_deeper_wider_fracture_zone() {
        let plan = shape(spec(), FINE);
        // Past the transform fault and before the patches, so the depth
        // depends only on the offset and the strength.
        let mut depths = Vec::new();
        for j in 0..60 {
            let small = plan.fracture(j, 20.0, true, 0.0);
            let large = plan.fracture(j, 100.0, true, 0.0);
            assert!(large.depth > 4.0 * small.depth, "{j}");
            assert!(large.half > 2.0 * small.half, "{j}");
            assert!(large.depth <= TROUGH_MAX);
            // An offset with no transform fault leaves a fainter trough.
            assert!(plan.fracture(j, 20.0, false, 0.0).depth < small.depth);
            depths.push(plan.fracture(j, 60.0, true, 0.0).depth);
        }
        // Each transform fault has its own strength, so some troughs are
        // deep and others are faint.
        let most = depths.iter().copied().fold(0.0, f64::max);
        let least = depths.iter().copied().fold(f64::MAX, f64::min);
        assert!(most > 2.0 * least, "{most} {least}");
        // Some of the transform faults have a transverse ridge.
        let ridges = (0..60)
            .filter(|j| plan.fracture(*j, 100.0, true, 0.0).ridge > 0.0)
            .count();
        assert!(ridges > 10 && ridges < 50, "{ridges}");
    }

    #[test]
    fn a_fracture_zone_is_deeper_near_the_transform_fault_and_patchy_when_old() {
        let plan = shape(spec(), FINE);
        let v = plan.ridge.spread * plan.ridge.world_km;
        // The trace leaves a transform fault of 100 km after this age.
        let out = 50.0 / v;
        let mut changes = 0;
        for j in 0..20 {
            let at = |age: f64| plan.fracture(j, 100.0, true, age).depth;
            assert!(at(0.5 * out) > at(out + ACTIVE_MYR), "{j}");
            // Along an old fracture zone, the depth goes up and down.
            let old: Vec<f64> = (0..40).map(|i| at(30.0 + f64::from(i))).collect();
            let most = old.iter().copied().fold(0.0, f64::max);
            let least = old.iter().copied().fold(f64::MAX, f64::min);
            changes += usize::from(most > 1.5 * least);
        }
        assert!(changes > 10, "{changes}");
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
            ..spec()
        };
        let plan = rough(full, FINE);
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
        for plan in [shape(spec(), FINE), rough(spec(), FINE)] {
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
                if plan.relief > 0.0 {
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
            ..spec()
        };
        let plan = rough(full, FINE);
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
            let rim = ABOVE_CREST * plan.axial(&f).0 * f.level;
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
        let plan = rough(full(), 128);
        let (valley, _) = plan.axial(&plan.faulting());
        let deepest = (TROUGH_MAX + 2.0 * NODAL_MAX + valley) * LEVELS_PER_KM;
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
