//! The line of a stroke of a line brush.
//!
//! Each texel near the line gets a place: the distance along the line and
//! the distance across it. A line brush gives the new level of a texel as a
//! function of that place and of the level from before the stroke.

use std::f64::consts::FRAC_PI_2;
use std::ops::Range;

use crate::cube::{FACES, face_dir, unwarp};
use crate::heightmap::{TexelRect, brush_rect, hash};
use crate::math::{V3, add, angle, cross, dot, length, normalize, scale};

/// The largest number of segments in one plan.
pub const LINE_SEGMENTS: usize = 128;

/// The distance between two points of the line that the app makes, as a part
/// of the brush radius.
pub const LINE_SPACING: f64 = 0.25;

/// The largest number of segments in one plan past the first segment that
/// the new points change. With the spacing of the app, they are 4 brush
/// radii of line, so the rectangles of a plan stay small.
const NEW_SEGMENTS: usize = 16;

/// A line brush changes no ground farther from the line than this number of
/// brush radii.
pub(crate) const REACH: f64 = 1.5;

/// The cosine of the sharpest turn that has a miter. At a sharper turn, each
/// of the two segments ends with a round cap.
const SHARP_TURN: f64 = -0.5;

/// One piece of the line between two points, as the shader gets it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineSegment {
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

impl LineSegment {
    /// The segment as 4 vectors of 4 numbers: `n` and the rest of `u0`,
    /// `m0` and `len`, `m1` and `q0`, then `q1`, the caps, the cell of `u0`,
    /// and a zero. The caps are 1 for `first` plus 2 for `last`.
    ///
    /// A 32-bit number cannot hold a long distance along the line to a part
    /// of a texel step. Thus `u0` goes as the nearest multiple of `cell`,
    /// a length of the pattern of the brush, and the rest. The cell of `u0`
    /// is the number of that multiple.
    pub fn to_f32(&self, cell: f64) -> [f32; 16] {
        let caps = f64::from(u8::from(self.first) + 2 * u8::from(self.last));
        let whole = (self.u0 / cell + 0.5).floor();
        let rest = self.u0 - whole * cell;
        let [n, m0, m1] = [self.n, self.m0, self.m1];
        [
            n[0], n[1], n[2], rest, m0[0], m0[1], m0[2], self.len, m1[0], m1[1], m1[2], self.q0,
            self.q1, caps, whole, 0.0,
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
pub(crate) struct LinePoint {
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
pub(crate) enum LinePlace {
    /// The texel is not near the new part of the line. Its level stays.
    Far,
    /// The brush does not reach the texel. It gets the level from before
    /// the stroke.
    Off,
    On(LinePoint),
}

#[derive(Clone, Copy, Debug)]
struct Vertex {
    dir: V3,
    flow: f64,
    /// The distance along the line, in radians.
    u: f64,
}

/// The line of one stroke of a line brush.
#[derive(Clone, Debug)]
pub struct StrokeLine {
    /// The brush radius as an angle, in radians.
    radius: f64,
    face_size: usize,
    points: Vec<Vertex>,
}

/// The part of the heightmap that changes after new points of a line, and
/// the segments that give the levels there.
#[derive(Clone, Debug)]
pub struct LinePlan {
    /// The number of texels along one side of a face.
    pub face_size: usize,
    /// The brush radius as an angle, in radians.
    pub radius: f64,
    /// The rectangle of each face that holds the texels that can change.
    pub rects: [Option<TexelRect>; FACES],
    /// The new segments, the segment before them, and the nearest other
    /// segments, in the order of the line. There are `LINE_SEGMENTS` at
    /// most.
    pub segments: Vec<LineSegment>,
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

impl StrokeLine {
    /// Starts a line with no points on a heightmap with `face_size` texels
    /// along one side of a face. `radius` is the brush radius as an angle,
    /// in radians.
    pub fn new(radius: f64, face_size: usize) -> StrokeLine {
        StrokeLine {
            radius,
            face_size,
            points: Vec::new(),
        }
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
        self.points.push(Vertex { dir, flow, u });
    }

    /// The number of points.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The length of the line, in radians.
    pub fn length(&self) -> f64 {
        self.points.last().map_or(0.0, |point| point.u)
    }

    /// The direction and the flow of each point with a distance along the
    /// line in `u`.
    pub(crate) fn points_in(&self, u: std::ops::RangeInclusive<f64>) -> Vec<(V3, f64)> {
        let points = self.points.iter().filter(|point| u.contains(&point.u));
        points.map(|point| (point.dir, point.flow)).collect()
    }

    fn normal(&self, j: usize) -> V3 {
        normalize(cross(self.points[j].dir, self.points[j + 1].dir))
    }

    /// The segment from point `j` to the point after it.
    fn segment(&self, j: usize) -> LineSegment {
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
        LineSegment {
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
    pub(crate) fn distance(&self, j: usize, p: V3) -> f64 {
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

    /// The direction of each point.
    pub(crate) fn points(&self) -> impl Iterator<Item = V3> + '_ {
        self.points.iter().map(|point| point.dir)
    }

    /// The distance from a direction to the line, and the distance along the
    /// line of the nearest place, in radians. The line has a round end at
    /// each end.
    pub(crate) fn nearest(&self, p: V3) -> (f64, f64) {
        let Some(last) = self.points.last() else {
            return (f64::INFINITY, 0.0);
        };
        if self.points.len() == 1 {
            return (angle(p, last.dir), 0.0);
        }
        let mut best = (f64::INFINITY, 0);
        for j in 0..self.points.len() - 1 {
            let d = self.distance(j, p);
            if d < best.0 {
                best = (d, j);
            }
        }
        let j = best.1;
        let (a, b) = (self.points[j], self.points[j + 1]);
        let along = dot(p, normalize(cross(self.normal(j), a.dir)))
            .clamp(-1.0, 1.0)
            .asin();
        (best.0, a.u + along.clamp(0.0, b.u - a.u))
    }

    /// The distance of a direction past the plane through the last point,
    /// square to the last segment, and its distance from the great circle of
    /// the last segment, in radians. The first value is less than zero before
    /// the plane.
    pub(crate) fn past_end(&self, p: V3) -> (f64, f64) {
        let count = self.points.len();
        if count < 2 {
            return (f64::MIN, 0.0);
        }
        let n = self.normal(count - 2);
        let tangent = normalize(cross(n, self.points[count - 1].dir));
        let past = dot(p, tangent).clamp(-1.0, 1.0).asin();
        (past, dot(p, n).clamp(-1.0, 1.0).asin().abs())
    }

    /// The plan for the points from `from` on. `from` is the number of
    /// points that the plans before this one covered: zero, or the `end` of
    /// the last plan. The segment before the new points is in the plan,
    /// because a new point changes the end of that segment. A line brush is
    /// lower near the end of the line, so the plan also holds the segments
    /// of the last brush radius before the new points, up to 32 of them. A
    /// plan covers 16 segments past them at most.
    ///
    /// The plan holds each segment that can reach a texel of its rectangles.
    /// If these are more than `LINE_SEGMENTS`, the plan holds the nearest
    /// ones. The GPU pass can then lose the parts of the line of the segments
    /// that are not in the plan.
    pub fn plan(&self, from: usize) -> LinePlan {
        let count = self.points.len().saturating_sub(1);
        let mut start = from.saturating_sub(2).min(count);
        if let Some(old) = from.checked_sub(1).and_then(|i| self.points.get(i)) {
            let limit = old.u - TAPER * self.radius;
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

    /// The plan with the segments that cover the distances `u` along the
    /// line as its new part.
    pub fn plan_between(&self, u: Range<f64>) -> LinePlan {
        let count = self.points.len().saturating_sub(1);
        let start = self.points.partition_point(|p| p.u <= u.start);
        let start = start.saturating_sub(1).min(count);
        let stop = self
            .points
            .partition_point(|p| p.u < u.end)
            .clamp(start, count);
        self.plan_of(start, stop)
    }

    /// The plan with the segments from `start` to `stop` as its new part.
    pub(crate) fn plan_of(&self, start: usize, stop: usize) -> LinePlan {
        let count = self.points.len().saturating_sub(1);
        let n = self.face_size;
        let radius = self.radius;
        let reach = REACH * radius;
        // Places on the new part of the line, with no more than the spacing
        // between two of them.
        let mut samples = Vec::new();
        for j in start..stop {
            let (a, b) = (self.points[j], self.points[j + 1]);
            let len = b.u - a.u;
            let steps = (len / (LINE_SPACING * radius)).ceil().max(1.0) as usize;
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
        others.truncate(LINE_SEGMENTS.saturating_sub(stop - start));
        let mut indices: Vec<usize> = others.into_iter().map(|(_, j)| j).collect();
        indices.extend(start..stop);
        indices.sort_unstable();
        let tail = indices.iter().position(|&j| j == start).unwrap_or(0);
        LinePlan {
            face_size: n,
            radius,
            rects,
            segments: indices.into_iter().map(|j| self.segment(j)).collect(),
            tail: tail..tail + (stop - start),
            end: if stop < count {
                stop + 1
            } else {
                self.points.len()
            },
            length: self.length(),
        }
    }
}

impl LinePlan {
    /// The texel step at the center of a face, in radians.
    pub(crate) fn step(&self) -> f64 {
        FRAC_PI_2 / self.face_size as f64
    }

    /// The place of a unit direction on the line. The segment with the
    /// smallest distance to the direction gives the place. With two equal
    /// distances, the first segment gives it.
    pub(crate) fn place(&self, p: V3) -> LinePlace {
        self.pick(p).0
    }

    /// The place of a direction, and the result of `StrokeLine::margin`.
    fn pick(&self, p: V3) -> (LinePlace, f64) {
        let reach = REACH * self.radius;
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
                best = Some(LinePoint {
                    u,
                    end: u.min(self.length - u).max(0.0),
                    across,
                    past: (distance * distance - across * across).max(0.0).sqrt(),
                    flow: s.q0 + (s.q1 - s.q0) * along,
                });
            }
        }
        let place = match best {
            _ if !near => LinePlace::Far,
            Some(point) => LinePlace::On(point),
            None => LinePlace::Off,
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
}

/// A line brush gets its full height and width at this number of brush
/// radii from an end of the line.
pub(crate) const TAPER: f64 = 1.2;

/// A number from 0 to 1 from the low 24 bits of a hash. It is exact as a
/// 32-bit number.
pub(crate) fn unit(h: u32) -> f64 {
    f64::from(h & 0xff_ffff) / f64::from(1u32 << 24)
}

/// Noise of one number with cells of size 1, from -1 to 1, and its change.
/// The value at each whole number is from a hash of that number and of `a`
/// and `b`, and a smoothstep joins two values.
pub(crate) fn noise1(x: f64, a: u32, b: u32, seed: u32) -> (f64, f64) {
    let n = x.floor();
    let (s, n) = (x - n, n as i32);
    let value = |c: i32| 2.0 * unit(hash(c as u32, a, b, seed)) - 1.0;
    let (h0, h1) = (value(n), value(n.wrapping_add(1)));
    (
        h0 + (h1 - h0) * s * s * (3.0 - 2.0 * s),
        (h1 - h0) * 6.0 * s * (1.0 - s),
    )
}

/// A bump of height 1 at zero that is exactly zero at 1 and past it.
pub(crate) fn bump(q: f64) -> f64 {
    let x = (1.0 - q * q).max(0.0);
    x * x
}

pub(crate) fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
