//! The Polygon brush. A round brush moves along a pen stroke, and the area
//! that it covers becomes polygon area.
//!
//! The outline of the area is a ring around the smoothed stroke: the right
//! side, a round cap, the left side back, and a round cap. A round join
//! goes around the outer side of each turn, and the inner side goes
//! through the center of the brush. Thus each point within the brush
//! radius of the stroke has a nonzero winding number, and each other point
//! has a winding number of zero.
//!
//! Lengths in points are on the screen. `scale` is the number of points in
//! one radian of the sphere.

use std::f64::consts::{FRAC_PI_4, PI, TAU};
use std::ops::RangeInclusive;

use vale_store::{Geometry, LonLat};

use super::line::smooth;
use super::math::{Mat3, V3, add, angle, cross, dot, length, normalize, scale};
use super::polygon::{MIN_AREA, SAME, densify, dirs, overlay, signed_area, to_polygons_many};
use super::snap::{PathVertex, distance_to_arc, project};
use super::vector::arcs_cross;

/// The range of the brush radius, in points.
pub const SIZE: RangeInclusive<f32> = 1.0..=60.0;
/// The brush radius of a new Polygon brush, in points.
pub const DEFAULT_SIZE: f32 = 6.0;
/// The smallest pressure. A lighter touch paints at this fraction of the
/// radius.
pub const MIN_PRESSURE: f32 = 0.15;
/// The largest distance of the outline from the true edge of the brush.
const TOLERANCE: f64 = 0.25;
/// The largest brush radius, in radians.
const MAX_RADIUS: f64 = 0.3;
/// A free vertex nearer than this to the 180 degree meridian, in radians,
/// moves to this distance.
const MERIDIAN: f64 = 1e-8;
/// The pressure of a sample is the mean of this many samples on each side.
const PRESSURE_SPAN: usize = 2;

/// The area of a brush stroke.
pub struct Sweep {
    /// The simplified center line, in the order of the pen.
    pub center: Vec<V3>,
    /// The brush radius at each vertex of the center line, in radians.
    pub radii: Vec<f64>,
    /// The simple rings of the area, with the inside on the left.
    pub rings: Vec<Vec<PathVertex>>,
}

impl Sweep {
    /// The polygons of the area in longitude and latitude.
    pub fn polygons(&self) -> Vec<Vec<Vec<LonLat>>> {
        to_polygons_many(&self.rings)
    }

    /// The geometry of a new feature.
    pub fn geometry(&self) -> Option<Geometry> {
        let polygons = self.polygons();
        (!polygons.is_empty()).then_some(Geometry::Polygons(polygons))
    }
}

/// The area that a brush with a radius of `size` points covers along the
/// pen samples. `pressure` has one value for each sample, from 0 to 1. A
/// missing value is full pressure. `strength` is the smoothing, from 0 to 1.
pub fn sweep(
    samples: &[V3],
    pressure: &[f32],
    size: f32,
    scale: f64,
    strength: f64,
) -> Option<Sweep> {
    let first = *samples.first()?;
    let tolerance = TOLERANCE / scale;
    let raw_length: f64 = samples.windows(2).map(|w| angle(w[0], w[1])).sum();
    let center = if raw_length * scale < 1.0 {
        vec![first]
    } else {
        smooth(samples, &[], scale, strength)
    };
    let pressure = filtered(samples.len(), pressure);
    let radius = |p: f32| (f64::from(size) * f64::from(p) / scale).clamp(SAME, MAX_RADIUS);
    let radii: Vec<f64> = along(samples, &center)
        .into_iter()
        .map(|t| radius(at(&pressure, t)))
        .collect();
    let (center, radii) = simplify(&center, &radii, tolerance);
    let ring = outline(&center, &radii, tolerance);
    let reference = outside(&center, &radii)?;
    let ring_dirs: Vec<V3> = ring.iter().map(PathVertex::dir).collect();
    let min_area = MIN_AREA / (scale * scale);
    let rings: Vec<Vec<PathVertex>> = overlay(&[ring], |p| winding(&ring_dirs, p, reference) != 0)
        .into_iter()
        .filter(|r| signed_area(&dirs(r)).abs() > min_area)
        .map(|r| densify(&r).into_iter().map(off_meridian).collect())
        .collect();
    (!rings.is_empty()).then_some(Sweep {
        center,
        radii,
        rings,
    })
}

/// The vertex moved a little off the 180 degree meridian. A free vertex
/// on the meridian does not show on which side of it the ring goes.
fn off_meridian(v: PathVertex) -> PathVertex {
    let d = v.dir();
    if d[0] >= 0.0 || d[1].abs() >= MERIDIAN {
        return v;
    }
    let y = MERIDIAN.copysign(d[1]);
    PathVertex::Free(normalize([d[0], y, d[2]]))
}

/// The pressure of each sample, as the mean over a short span. The span
/// keeps the edge of the area from wobbling.
fn filtered(n: usize, pressure: &[f32]) -> Vec<f32> {
    let raw: Vec<f32> = (0..n)
        .map(|i| pressure.get(i).copied().unwrap_or(1.0).clamp(0.0, 1.0))
        .collect();
    (0..n)
        .map(|i| {
            let lo = i.saturating_sub(PRESSURE_SPAN);
            let hi = (i + PRESSURE_SPAN + 1).min(n);
            raw[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
        })
        .collect()
}

/// The place of each vertex of `path` along the samples, as a fraction of
/// the sample index. The smoothed path and the samples have about the same
/// length, so the fraction of the length is the same.
fn along(samples: &[V3], path: &[V3]) -> Vec<f64> {
    let cumulative = |pts: &[V3]| {
        let mut out = Vec::with_capacity(pts.len());
        let mut sum = 0.0;
        out.push(0.0);
        for w in pts.windows(2) {
            sum += angle(w[0], w[1]);
            out.push(sum);
        }
        out
    };
    let (s, p) = (cumulative(samples), cumulative(path));
    let (s_total, p_total) = (*s.last().unwrap_or(&0.0), *p.last().unwrap_or(&0.0));
    p.iter()
        .map(|&d| {
            if p_total <= 0.0 || s_total <= 0.0 {
                return 0.0;
            }
            let target = d / p_total * s_total;
            let j = s.partition_point(|&x| x <= target).clamp(1, s.len() - 1);
            let span = s[j] - s[j - 1];
            let f = if span > 0.0 {
                (target - s[j - 1]) / span
            } else {
                0.0
            };
            (j - 1) as f64 + f.clamp(0.0, 1.0)
        })
        .collect()
}

/// The value at a fractional index.
fn at(values: &[f32], t: f64) -> f32 {
    if values.is_empty() {
        return 1.0;
    }
    let i = (t.floor() as usize).min(values.len() - 1);
    let j = (i + 1).min(values.len() - 1);
    let f = (t - i as f64).clamp(0.0, 1.0) as f32;
    values[i] + (values[j] - values[i]) * f
}

/// The center line with fewer vertices, where the outline moves less than
/// `tolerance`. The error of a vertex is its distance from the new line
/// plus the change of its radius.
fn simplify(path: &[V3], radii: &[f64], tolerance: f64) -> (Vec<V3>, Vec<f64>) {
    let mut path_out: Vec<V3> = Vec::with_capacity(path.len());
    let mut radii_out: Vec<f64> = Vec::with_capacity(path.len());
    for (&p, &r) in path.iter().zip(radii) {
        if path_out.last().is_some_and(|&q| angle(q, p) < SAME) {
            continue;
        }
        path_out.push(p);
        radii_out.push(r);
    }
    let n = path_out.len();
    if n < 3 {
        return (path_out, radii_out);
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut spans = vec![(0, n - 1)];
    while let Some((a, b)) = spans.pop() {
        let (pa, pb) = (path_out[a], path_out[b]);
        let len = angle(pa, pb);
        let far = (a + 1..b)
            .map(|i| {
                let (_, t, d) = project(path_out[i], pa, pb);
                let f = if len > 0.0 { t / len } else { 0.0 };
                let r = radii_out[a] + (radii_out[b] - radii_out[a]) * f;
                (i, d + (radii_out[i] - r).abs())
            })
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((i, e)) = far
            && e > tolerance
        {
            keep[i] = true;
            spans.push((a, i));
            spans.push((i, b));
        }
    }
    let path = (0..n).filter(|&i| keep[i]).map(|i| path_out[i]).collect();
    let radii = (0..n).filter(|&i| keep[i]).map(|i| radii_out[i]).collect();
    (path, radii)
}

/// The point at the angle `r` from `c` in the direction `u`, which is
/// perpendicular to `c`.
fn offset(c: V3, u: V3, r: f64) -> V3 {
    normalize(add(scale(c, r.cos()), scale(u, r.sin())))
}

/// The largest angle between two vertices of an arc of radius `r`.
fn arc_step(r: f64, tolerance: f64) -> f64 {
    let step = if tolerance < r {
        2.0 * (1.0 - tolerance / r).acos()
    } else {
        FRAC_PI_4
    };
    step.min(FRAC_PI_4)
}

/// The points of an arc around `c` from the direction `u`, turned by `turn`
/// radians counterclockwise as seen from outside, without its ends.
fn arc(out: &mut Vec<V3>, c: V3, u: V3, r: f64, turn: f64, tolerance: f64) {
    let k = (turn.abs() / arc_step(r, tolerance)).ceil() as usize;
    for j in 1..k {
        let m = Mat3::axis_angle(c, turn * j as f64 / k as f64);
        out.push(offset(c, m.mul_vec(u), r));
    }
}

/// A direction perpendicular to `c`.
fn perpendicular(c: V3) -> V3 {
    let axis = if c[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    normalize(cross(axis, c))
}

/// The ring around the center line, counterclockwise as seen from outside.
fn outline(center: &[V3], radii: &[f64], tolerance: f64) -> Vec<PathVertex> {
    let n = center.len();
    if n == 1 {
        let (c, r) = (center[0], radii[0]);
        let u = perpendicular(c);
        let mut ring = vec![offset(c, u, r)];
        arc(&mut ring, c, u, r, TAU, tolerance);
        return ring.into_iter().map(PathVertex::Free).collect();
    }
    let normals: Vec<V3> = center
        .windows(2)
        .map(|w| normalize(cross(w[0], w[1])))
        .collect();
    let mut right = Vec::new();
    let mut left = Vec::new();
    for i in 0..n - 1 {
        let (c0, c1, nrm) = (center[i], center[i + 1], normals[i]);
        right.push(offset(c0, scale(nrm, -1.0), radii[i]));
        left.push(offset(c0, nrm, radii[i]));
        right.push(offset(c1, scale(nrm, -1.0), radii[i + 1]));
        left.push(offset(c1, nrm, radii[i + 1]));
        let Some(&next) = normals.get(i + 1) else {
            continue;
        };
        // The turn of the stroke at `c1`, counterclockwise as seen from
        // outside.
        let turn = dot(cross(nrm, next), c1).atan2(dot(nrm, next));
        if turn.abs() < SAME {
            continue;
        }
        let r = radii[i + 1];
        if turn > 0.0 {
            arc(&mut right, c1, scale(nrm, -1.0), r, turn, tolerance);
            left.push(c1);
        } else {
            right.push(c1);
            arc(&mut left, c1, nrm, r, turn, tolerance);
        }
    }
    let (first, last) = (normals[0], normals[n - 2]);
    let mut ring = right;
    arc(
        &mut ring,
        center[n - 1],
        scale(last, -1.0),
        radii[n - 1],
        PI,
        tolerance,
    );
    ring.extend(left.into_iter().rev());
    arc(&mut ring, center[0], first, radii[0], PI, tolerance);
    ring.into_iter().map(PathVertex::Free).collect()
}

/// A point that the brush does not reach, or `None` for a stroke that
/// covers the whole sphere.
fn outside(center: &[V3], radii: &[f64]) -> Option<V3> {
    let reach = radii.iter().copied().fold(0.0, f64::max) + 1e-6;
    let distance = |p: V3| {
        if center.len() == 1 {
            return angle(p, center[0]);
        }
        center
            .windows(2)
            .map(|w| distance_to_arc(p, w[0], w[1]))
            .fold(f64::INFINITY, f64::min)
    };
    let sum = center.iter().fold([0.0; 3], |s, &c| add(s, c));
    let antipode = (length(sum) > 1e-6).then(|| scale(normalize(sum), -1.0));
    let axes = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    antipode
        .into_iter()
        .chain(axes)
        .find(|&p| distance(p) > reach)
}

/// The winding number of the ring around `p`. The winding number around
/// `reference` is zero.
fn winding(ring: &[V3], p: V3, reference: V3) -> i32 {
    let sum = add(p, reference);
    let mid = if length(sum) > 1e-3 {
        normalize(sum)
    } else {
        perpendicular(p)
    };
    let n = ring.len();
    let mut w = 0;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let normal = cross(a, b);
        for (x, y) in [(p, mid), (mid, reference)] {
            if arcs_cross(x, y, a, b) {
                w += if dot(normal, x) > 0.0 { 1 } else { -1 };
            }
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::globe::combine::StrokeArea;
    use crate::globe::math::lonlat_to_dir;
    use crate::globe::polygon::tests::{assert_valid, inside};
    use crate::globe::snap::slerp;

    /// Points per radian: a sphere with a radius of 300 points on screen.
    const SCALE: f64 = 300.0;
    const SMOOTHING: f64 = 0.35;
    /// The default radius, in degrees.
    fn radius() -> f64 {
        (f64::from(DEFAULT_SIZE) / SCALE).to_degrees()
    }

    fn dir(p: LonLat) -> V3 {
        lonlat_to_dir(p[0], p[1])
    }

    /// Pen samples along great circles through the way, about 4 samples per
    /// point.
    fn samples(way: &[LonLat]) -> Vec<V3> {
        let mut out = vec![dir(way[0])];
        for w in way.windows(2) {
            let (a, b) = (dir(w[0]), dir(w[1]));
            let n = (angle(a, b) * SCALE * 4.0).ceil().max(1.0) as usize;
            out.extend((1..=n).map(|k| slerp(a, b, k as f64 / n as f64)));
        }
        out
    }

    fn paint(way: &[LonLat]) -> Vec<Vec<Vec<LonLat>>> {
        let s = samples(way);
        let sweep = sweep(&s, &[], DEFAULT_SIZE, SCALE, SMOOTHING).expect("an area");
        let p = sweep.polygons();
        assert_valid(&p);
        p
    }

    #[test]
    fn a_stroke_covers_each_point_within_the_radius() {
        let p = paint(&[[0.0, 0.0], [10.0, 0.0]]);
        let r = radius();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 1);
        for lon in [0.5, 5.0, 9.5] {
            assert!(inside(&p, [lon, 0.9 * r]), "{lon}");
            assert!(inside(&p, [lon, -0.9 * r]), "{lon}");
            assert!(!inside(&p, [lon, 1.1 * r]), "{lon}");
            assert!(!inside(&p, [lon, -1.1 * r]), "{lon}");
        }
    }

    #[test]
    fn the_ends_are_round() {
        let p = paint(&[[0.0, 0.0], [10.0, 0.0]]);
        let r = radius();
        assert!(inside(&p, [-0.9 * r, 0.0]));
        assert!(!inside(&p, [-1.1 * r, 0.0]));
        assert!(inside(&p, [10.0 + 0.9 * r, 0.0]));
        assert!(!inside(&p, [10.0 + 1.1 * r, 0.0]));
        // A point past the corner of a square end is outside a round end.
        assert!(!inside(&p, [-0.8 * r, 0.8 * r]));
    }

    #[test]
    fn a_tap_paints_a_round_dot() {
        let s = vec![dir([20.0, 10.0])];
        let sweep = sweep(&s, &[], DEFAULT_SIZE, SCALE, SMOOTHING).expect("a dot");
        let p = sweep.polygons();
        assert_valid(&p);
        let r = radius();
        for (dx, dy) in [(1.0, 0.0), (0.0, 1.0), (-0.7, 0.7), (-0.7, -0.7)] {
            let near = [
                20.0 + 0.9 * r * dx / 10f64.to_radians().cos(),
                10.0 + 0.9 * r * dy,
            ];
            let far = [
                20.0 + 1.1 * r * dx / 10f64.to_radians().cos(),
                10.0 + 1.1 * r * dy,
            ];
            assert!(inside(&p, near), "{near:?}");
            assert!(!inside(&p, far), "{far:?}");
        }
    }

    #[test]
    fn pressure_makes_the_radius_smaller() {
        let s = samples(&[[0.0, 0.0], [10.0, 0.0]]);
        let n = s.len();
        // Light at the start, full at the end.
        let pressure: Vec<f32> = (0..n)
            .map(|i| 0.25 + 0.75 * i as f32 / (n - 1) as f32)
            .collect();
        let p = sweep(&s, &pressure, DEFAULT_SIZE, SCALE, SMOOTHING)
            .expect("an area")
            .polygons();
        assert_valid(&p);
        let r = radius();
        assert!(!inside(&p, [0.5, 0.5 * r]));
        assert!(inside(&p, [0.5, 0.2 * r]));
        assert!(inside(&p, [9.5, 0.9 * r]));
        // The same stroke at half pressure.
        let half = vec![0.5; n];
        let p = sweep(&s, &half, DEFAULT_SIZE, SCALE, SMOOTHING)
            .expect("an area")
            .polygons();
        assert!(inside(&p, [5.0, 0.4 * r]));
        assert!(!inside(&p, [5.0, 0.6 * r]));
    }

    #[test]
    fn a_stroke_across_the_180_degree_meridian_has_a_polygon_on_each_side() {
        let p = paint(&[[175.0, 0.0], [185.0, 0.0]]);
        assert_eq!(p.len(), 2);
        assert!(inside(&p, [179.9, 0.0]));
        assert!(inside(&p, [-179.9, 0.0]));
        assert!(inside(&p, [178.0, 0.5 * radius()]));
    }

    #[test]
    fn a_stroke_over_a_pole_holds_the_pole() {
        let p = paint(&[[0.0, 80.0], [0.0, 90.0], [180.0, 80.0]]);
        assert!(inside(&p, [0.0, 89.9]));
        assert!(inside(&p, [180.0, 85.0]));
        assert!(!inside(&p, [90.0, 85.0]));
    }

    #[test]
    fn a_loop_around_a_pole_leaves_the_pole_out() {
        let way: Vec<LonLat> = (0..=36).map(|k| [k as f64 * 10.0 - 180.0, 85.0]).collect();
        let p = paint(&way);
        assert!(!inside(&p, [0.0, 90.0]));
        for lon in [-150.0, 0.0, 90.0] {
            assert!(inside(&p, [lon, 85.0]), "{lon}");
        }
    }

    #[test]
    fn a_closed_loop_keeps_its_middle_as_a_hole() {
        let c = [20.0, 0.0];
        let way: Vec<LonLat> = (0..=40)
            .map(|k| {
                let t = (k as f64 * 10.0).to_radians();
                [c[0] + 3.0 * t.cos(), c[1] + 3.0 * t.sin()]
            })
            .collect();
        let p = paint(&way);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 2, "an outer ring and a hole");
        assert!(!inside(&p, c));
        assert!(inside(&p, [23.0, 0.0]));
        assert!(inside(&p, [20.0, 3.0]));
    }

    #[test]
    fn a_tight_zigzag_has_no_holes() {
        let r = radius();
        let way: Vec<LonLat> = (0..30)
            .map(|k| [k as f64 * 0.5 * r, if k % 2 == 0 { 0.3 } else { -0.3 } * r])
            .collect();
        let s = samples(&way);
        let p = sweep(&s, &[], DEFAULT_SIZE, SCALE, 0.0)
            .expect("an area")
            .polygons();
        assert_valid(&p);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 1);
        assert!(inside(&p, [7.0 * r, 0.0]));
    }

    #[test]
    fn a_stroke_that_crosses_itself_is_valid() {
        // A figure of eight.
        let way: Vec<LonLat> = (0..=72)
            .map(|k| {
                let t = (k as f64 * 5.0).to_radians();
                [4.0 * t.sin(), 2.0 * (2.0 * t).sin()]
            })
            .collect();
        let p = paint(&way);
        assert!(inside(&p, [0.0, 0.0]));
        assert!(!inside(&p, [2.0, 0.0]));
        assert!(!inside(&p, [-2.0, 0.0]));
    }

    #[test]
    fn the_outline_of_a_long_stroke_has_few_vertices() {
        let s = samples(&[[0.0, 0.0], [10.0, 0.0]]);
        assert!(s.len() > 200);
        let sweep = sweep(&s, &[], DEFAULT_SIZE, SCALE, SMOOTHING).expect("an area");
        let vertices: usize = sweep.rings.iter().map(Vec::len).sum();
        // The caps, and one vertex at each degree of the two sides.
        assert!(vertices < 50, "{vertices}");
        assert_eq!(sweep.center.len(), 2);
    }

    #[test]
    fn each_vertex_of_the_outline_is_near_the_edge_of_the_brush() {
        let way: Vec<LonLat> = (0..=20)
            .map(|k| [k as f64, (k as f64 * 0.7).sin()])
            .collect();
        let s = samples(&way);
        let sweep = sweep(&s, &[], DEFAULT_SIZE, SCALE, SMOOTHING).expect("an area");
        let r = f64::from(DEFAULT_SIZE) / SCALE;
        let tolerance = 2.0 * TOLERANCE / SCALE;
        for v in sweep.rings.iter().flatten() {
            let d = sweep
                .center
                .windows(2)
                .map(|w| distance_to_arc(v.dir(), w[0], w[1]))
                .fold(f64::INFINITY, f64::min);
            assert!((d - r).abs() < tolerance, "{}", (d - r) * SCALE);
        }
    }

    fn square() -> Geometry {
        Geometry::Polygons(vec![vec![vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ]]])
    }

    fn area(way: &[LonLat]) -> StrokeArea {
        let s = samples(way);
        let sweep = sweep(&s, &[], DEFAULT_SIZE, SCALE, SMOOTHING).expect("an area");
        StrokeArea::of_sweep(&sweep, SCALE).expect("an area")
    }

    fn polygons(g: &Geometry) -> Vec<Vec<Vec<LonLat>>> {
        let Geometry::Polygons(p) = g else {
            panic!("not a polygon");
        };
        p.clone()
    }

    #[test]
    fn a_band_whose_edge_touches_a_polygon_reaches_it_and_joins_it() {
        // The center line stays east of the square, within the radius.
        let r = radius();
        let a = area(&[[10.0 + 0.5 * r, 5.0], [15.0, 5.0]]);
        assert_eq!(a.reaches(&square(), SCALE), Some(0));
        let p = polygons(&a.add(&square()).expect("a change"));
        assert_valid(&p);
        assert_eq!(p.len(), 1);
        assert!(inside(&p, [14.0, 5.0]));
        assert!(inside(&p, [5.0, 5.0]));
        // A band far away does not reach it.
        let far = area(&[[20.0, 5.0], [25.0, 5.0]]);
        assert_eq!(far.reaches(&square(), SCALE), None);
    }

    #[test]
    fn a_band_across_a_polygon_cuts_it_in_two() {
        let r = radius();
        let a = area(&[[5.0, -3.0], [5.0, 13.0]]);
        let p = polygons(&a.subtract(&square()).expect("a change").expect("a polygon"));
        assert_valid(&p);
        assert_eq!(p.len(), 2);
        assert!(!inside(&p, [5.0, 5.0]));
        assert!(!inside(&p, [5.0 + 0.9 * r, 5.0]));
        assert!(inside(&p, [5.0 + 1.1 * r, 5.0]));
        assert!(inside(&p, [2.0, 5.0]));
    }

    #[test]
    fn a_band_that_covers_a_polygon_deletes_it() {
        let small = Geometry::Polygons(vec![vec![vec![
            [0.0, 0.0],
            [0.2, 0.0],
            [0.2, 0.2],
            [0.0, 0.2],
            [0.0, 0.0],
        ]]]);
        let a = area(&[[-1.0, 0.1], [1.0, 0.1]]);
        assert_eq!(a.subtract(&small), Some(None));
    }
}
