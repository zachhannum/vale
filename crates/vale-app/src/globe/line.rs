//! The freehand line tool. A pen stroke on the sphere becomes a smooth line
//! feature in longitude and latitude.
//!
//! Lengths in points are on the screen. `scale` is the number of points in
//! one radian of the sphere, so a stroke has the same smoothness at each
//! zoom, and you zoom in for finer detail.

use vale_store::{Feature, Geometry, LonLat, Uuid};

use super::math::{V3, add, angle, cross, dir_to_lonlat, dot, lonlat_to_dir, normalize, scale};

/// The smoothing of a new line tool, from 0 to 1.
pub const DEFAULT_SMOOTHING: f64 = 0.35;
/// A stroke that starts this near the end of a line extends that line.
pub const JOIN: f64 = 12.0;
/// The spacing of the path before the filter.
const SPACING: f64 = 1.0;
/// The width of the filter at a smoothing of 1, as a standard deviation.
const SIGMA: f64 = 8.0;
/// The largest distance of a removed vertex from the stored line.
const TOLERANCE: f64 = 0.1;
/// A shorter stroke is a tap, and it makes no line.
const MIN_LENGTH: f64 = 3.0;
/// Input samples nearer than this to the last one add nothing.
const MIN_STEP: f64 = 0.25;
/// The largest step of longitude between two stored vertices, in degrees.
const MAX_DLON: f64 = 45.0;

/// The settings of the line tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineSettings {
    /// From 0, which keeps each wiggle of the pen, to 1.
    pub smoothing: f64,
}

impl Default for LineSettings {
    fn default() -> Self {
        LineSettings {
            smoothing: DEFAULT_SMOOTHING,
        }
    }
}

/// What a stroke does to the layer.
#[derive(Clone, Debug, PartialEq)]
pub enum LineEdit {
    /// A new feature.
    New(Geometry),
    /// The feature with this UUID gets a longer geometry.
    Extend { uuid: Uuid, geometry: Geometry },
}

/// The edit that a stroke makes in a line layer with these features. The
/// samples are on the unit sphere. A stroke that is too short gives `None`.
pub fn commit(features: &[Feature], samples: &[V3], scale: f64, strength: f64) -> Option<LineEdit> {
    let first = *samples.first()?;
    let join = nearest_end(features, first, JOIN / scale);
    let sigma = SIGMA * strength.clamp(0.0, 1.0) / scale;
    let mut samples = samples.to_vec();
    let mut prefix = Vec::new();
    if let Some(join) = &join {
        samples[0] = join.end;
        prefix = tail(&dirs(&join.parts), 3.0 * sigma);
    }
    let path = smooth(&samples, &prefix, scale, strength);
    if arc_length(&path) < MIN_LENGTH / scale {
        return None;
    }
    let mut new = to_parts(&path);
    let Some(join) = join else {
        return Some(LineEdit::New(Geometry::Lines(new)));
    };
    let mut parts = join.others;
    let mut chain = join.parts;
    let last = chain.last_mut().expect("a chain has a part");
    last.extend_from_slice(&new[0][1..]);
    chain.extend(new.drain(1..));
    parts.extend(chain);
    Some(LineEdit::Extend {
        uuid: join.uuid,
        geometry: Geometry::Lines(parts),
    })
}

/// The smooth path of a stroke. It starts at the first sample and ends at
/// the last one. `prefix` is the end of a line that the stroke extends, and
/// it ends at the first sample. The filter reads it, so the joint has no
/// kink, but the path does not include it.
pub fn smooth(samples: &[V3], prefix: &[V3], scale: f64, strength: f64) -> Vec<V3> {
    let mut kept: Vec<V3> = Vec::new();
    for &s in samples {
        if kept.last().is_none_or(|&k| angle(k, s) >= MIN_STEP / scale) {
            kept.push(s);
        }
    }
    let step = SPACING / scale;
    let mut points = resample(prefix, step);
    let k = points.len().saturating_sub(1);
    let curve = resample(&curve(&kept, scale), step);
    points.extend(curve.into_iter().skip(usize::from(!prefix.is_empty())));
    let sigma = SIGMA * strength.clamp(0.0, 1.0) / SPACING;
    let path = if sigma > 0.0 {
        filter(&points, k, sigma)
    } else {
        points[k..].to_vec()
    };
    simplify(&path, TOLERANCE / scale)
}

/// A curve through the midpoints of the samples, with each sample as the
/// control point of a quadratic part. A fast stroke has far samples, and the
/// curve takes away the corners between them.
fn curve(samples: &[V3], per_radian: f64) -> Vec<V3> {
    let Some(&first) = samples.first() else {
        return Vec::new();
    };
    let mut out = vec![first];
    let mut from = first;
    for i in 1..samples.len().saturating_sub(1) {
        let control = samples[i];
        let to = normalize(add(control, samples[i + 1]));
        let len = (angle(from, control) + angle(control, to)) * per_radian;
        let n = (len / (SPACING * 0.5)).ceil().max(1.0) as usize;
        for j in 1..=n {
            let t = j as f64 / n as f64;
            let (wa, wc, wb) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
            out.push(normalize(add(
                add(scale(from, wa), scale(control, wc)),
                scale(to, wb),
            )));
        }
        from = to;
    }
    if samples.len() > 1 {
        out.push(samples[samples.len() - 1]);
    }
    out
}

/// Points along the path at even steps of arc length. The ends stay.
fn resample(path: &[V3], step: f64) -> Vec<V3> {
    let Some(&first) = path.first() else {
        return Vec::new();
    };
    let mut out = vec![first];
    let mut carry = step;
    for w in path.windows(2) {
        let len = angle(w[0], w[1]);
        let mut at = 0.0;
        while len - at >= carry {
            at += carry;
            out.push(lerp(w[0], w[1], at / len));
            carry = step;
        }
        carry -= len - at;
    }
    let last = path[path.len() - 1];
    if angle(out[out.len() - 1], last) > step * 1e-3 {
        out.push(last);
    }
    out
}

/// The point at the part `t` of the short arc from `a` to `b`. The chord
/// is in the plane of the great circle, so the point is on the arc.
fn lerp(a: V3, b: V3, t: f64) -> V3 {
    normalize(add(scale(a, 1.0 - t), scale(b, t)))
}

/// A Gaussian filter over the points from index `k` to the end. The point
/// at `k` and the last point stay. Near each end, the window gets narrower
/// on both sides, so the filter does not pull an end in.
fn filter(points: &[V3], k: usize, sigma: f64) -> Vec<V3> {
    let reach = (3.0 * sigma).ceil() as usize;
    let weights: Vec<f64> = (0..=reach)
        .map(|j| (-((j * j) as f64) / (2.0 * sigma * sigma)).exp())
        .collect();
    let last = points.len() - 1;
    let mut out = Vec::with_capacity(points.len() - k);
    out.push(points[k]);
    for i in k + 1..points.len() {
        let w = reach.min(i).min(last - i);
        let mut sum = scale(points[i], weights[0]);
        for j in 1..=w {
            sum = add(sum, scale(add(points[i - j], points[i + j]), weights[j]));
        }
        out.push(normalize(sum));
    }
    out
}

/// The path with the vertices that are within `tolerance` of the simpler
/// line removed (Douglas and Peucker).
fn simplify(path: &[V3], tolerance: f64) -> Vec<V3> {
    if path.len() < 3 {
        return path.to_vec();
    }
    let mut keep = vec![false; path.len()];
    keep[0] = true;
    keep[path.len() - 1] = true;
    let mut spans = vec![(0, path.len() - 1)];
    while let Some((a, b)) = spans.pop() {
        let far = (a + 1..b)
            .map(|i| (i, distance_to_arc(path[i], path[a], path[b])))
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((i, d)) = far
            && d > tolerance
        {
            keep[i] = true;
            spans.push((a, i));
            spans.push((i, b));
        }
    }
    path.iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(p, _)| *p)
        .collect()
}

/// The angle from `p` to the nearest point of the short arc from `a` to `b`.
fn distance_to_arc(p: V3, a: V3, b: V3) -> f64 {
    let n = cross(a, b);
    let len = dot(n, n).sqrt();
    if len < 1e-15 {
        return angle(p, a);
    }
    let n = scale(n, 1.0 / len);
    // The point is beside the arc when it is on the inner side of both ends.
    if dot(cross(a, p), n) >= 0.0 && dot(cross(p, b), n) >= 0.0 {
        dot(p, n).abs().asin()
    } else {
        angle(p, a).min(angle(p, b))
    }
}

fn arc_length(path: &[V3]) -> f64 {
    path.windows(2).map(|w| angle(w[0], w[1])).sum()
}

/// The stored parts of a path. Longitude is from -180 to 180. Where the
/// path crosses the 180 degree meridian, one part ends at 180 and the next
/// part starts at -180, at the same latitude, or the other way. Two
/// vertices of a part are at most 45 degrees of longitude apart, so a
/// renderer in longitude and latitude draws each step the short way. Where
/// the path goes over a pole, the part has two vertices at the pole, with
/// the longitudes of each side.
pub fn to_parts(path: &[V3]) -> Vec<Vec<LonLat>> {
    let Some(&first) = path.first() else {
        return Vec::new();
    };
    let mut parts = vec![vec![lonlat(first)]];
    for w in path.windows(2) {
        segment(w[0], w[1], 0, &mut parts);
    }
    parts.retain(|p| p.len() > 1);
    parts
}

fn segment(a: V3, b: V3, depth: u32, parts: &mut Vec<Vec<LonLat>>) {
    let (la, lb) = (lonlat(a), lonlat(b));
    if wrap180(lb[0] - la[0]).abs() > MAX_DLON {
        // Away from a pole, a shorter step has a smaller step of
        // longitude. A step that stays large goes through the pole.
        let m = normalize(add(a, b));
        if depth < 24 && m[2].abs() < 1.0 - 1e-15 {
            segment(a, m, depth + 1, parts);
            segment(m, b, depth + 1, parts);
        } else {
            let lat = 90f64.copysign(a[2]);
            push(parts, [la[0], lat]);
            push(parts, [lb[0], lat]);
            push(parts, lb);
        }
        return;
    }
    if (a[1] >= 0.0) != (b[1] >= 0.0) {
        let t = a[1] / (a[1] - b[1]);
        let c = lerp(a, b, t);
        if c[0] < 0.0 {
            let lat = lonlat(c)[1];
            let side = 180f64.copysign(a[1] + 0.0);
            push(parts, [side, lat]);
            parts.push(vec![[-side, lat]]);
        }
    }
    push(parts, lb);
}

fn push(parts: &mut [Vec<LonLat>], p: LonLat) {
    let part = parts.last_mut().expect("a part");
    if part.last() != Some(&p) {
        part.push(p);
    }
}

/// Longitude and latitude. A point on the 180 degree meridian has the
/// longitude 180.
fn lonlat(d: V3) -> LonLat {
    let (lon, lat) = dir_to_lonlat(d);
    [if lon == -180.0 { 180.0 } else { lon }, lat]
}

fn wrap180(d: f64) -> f64 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

/// The line end nearest to a point, within `reach`.
struct Join {
    uuid: Uuid,
    /// The end, on the unit sphere.
    end: V3,
    /// The parts of the joined chain, in order toward the end.
    parts: Vec<Vec<LonLat>>,
    /// The other parts of the feature.
    others: Vec<Vec<LonLat>>,
}

fn nearest_end(features: &[Feature], p: V3, reach: f64) -> Option<Join> {
    let mut best: Option<(f64, Join)> = None;
    for f in features {
        let Geometry::Lines(parts) = &f.geometry else {
            continue;
        };
        for chain in chains(parts) {
            let ordered: Vec<Vec<LonLat>> = chain.iter().map(|&i| parts[i].clone()).collect();
            let start = lonlat_to_dir(ordered[0][0][0], ordered[0][0][1]);
            let end = ordered[ordered.len() - 1][ordered[ordered.len() - 1].len() - 1];
            let end = lonlat_to_dir(end[0], end[1]);
            for (at, reversed) in [(end, false), (start, true)] {
                let d = angle(p, at);
                if d > reach || best.as_ref().is_some_and(|b| b.0 <= d) {
                    continue;
                }
                let mut chain_parts = ordered.clone();
                if reversed {
                    chain_parts.reverse();
                    chain_parts.iter_mut().for_each(|p| p.reverse());
                }
                let others = (0..parts.len())
                    .filter(|i| !chain.contains(i))
                    .map(|i| parts[i].clone())
                    .collect();
                let join = Join {
                    uuid: f.uuid(),
                    end: at,
                    parts: chain_parts,
                    others,
                };
                best = Some((d, join));
            }
        }
    }
    best.map(|b| b.1)
}

/// The parts of a line in runs where each part starts at the end of the one
/// before. A line that crosses the 180 degree meridian is one run.
fn chains(parts: &[Vec<LonLat>]) -> Vec<Vec<usize>> {
    let dir = |p: LonLat| lonlat_to_dir(p[0], p[1]);
    let touches = |a: usize, b: usize| {
        let (Some(&end), Some(&start)) = (parts[a].last(), parts[b].first()) else {
            return false;
        };
        a != b && angle(dir(end), dir(start)) < 1e-9
    };
    let next = |a: usize| (0..parts.len()).find(|&b| touches(a, b));
    let has_prev = |b: usize| (0..parts.len()).any(|a| touches(a, b));
    let mut used = vec![false; parts.len()];
    let mut out = Vec::new();
    let starts = (0..parts.len()).filter(|&i| !has_prev(i));
    // A ring of parts has no start, so each part is a start after the others.
    for s in starts.chain(0..parts.len()).collect::<Vec<_>>() {
        if used[s] || parts[s].is_empty() {
            continue;
        }
        let mut chain = vec![s];
        used[s] = true;
        let mut at = s;
        while let Some(n) = next(at).filter(|&n| !used[n]) {
            chain.push(n);
            used[n] = true;
            at = n;
        }
        out.push(chain);
    }
    out
}

/// The points of the parts on the unit sphere, with the joints once.
fn dirs(parts: &[Vec<LonLat>]) -> Vec<V3> {
    let mut out: Vec<V3> = Vec::new();
    for p in parts.iter().flatten() {
        let d = lonlat_to_dir(p[0], p[1]);
        if out.last().is_none_or(|&l| angle(l, d) > 1e-12) {
            out.push(d);
        }
    }
    out
}

/// The end of a path, at least `len` long when the path is that long.
fn tail(path: &[V3], len: f64) -> Vec<V3> {
    let mut start = path.len().saturating_sub(1);
    let mut at = 0.0;
    while start > 0 && at < len {
        at += angle(path[start - 1], path[start]);
        start -= 1;
    }
    path[start..].to_vec()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// Points per radian: a sphere with a radius of 300 points on screen.
    const SCALE: f64 = 300.0;

    fn turns(path: &[V3]) -> Vec<f64> {
        path.windows(3)
            .map(|w| {
                let (n1, n2) = (cross(w[0], w[1]), cross(w[1], w[2]));
                let c = dot(n1, n2) / (dot(n1, n1) * dot(n2, n2)).sqrt();
                c.clamp(-1.0, 1.0).acos().to_degrees()
            })
            .collect()
    }

    /// A point at `(x, y)` points from the center of the sphere's front.
    fn at(x: f64, y: f64) -> V3 {
        normalize([1.0, x / SCALE, y / SCALE])
    }

    /// The distance of each point from the line y = 0, in points.
    fn offset(p: V3) -> f64 {
        p[2] / p[0] * SCALE
    }

    #[test]
    fn a_fast_curve_has_no_corners() {
        // Twelve samples on a circle of 60 points: a fast pen, with a
        // turn of 30 degrees at each sample.
        let samples: Vec<V3> = (0..=9)
            .map(|i| {
                let a = f64::from(i) * 30f64.to_radians();
                at(60.0 * a.cos(), 60.0 * a.sin())
            })
            .collect();
        assert!(turns(&samples).iter().all(|&t| t > 29.0));
        let path = smooth(&samples, &[], SCALE, DEFAULT_SMOOTHING);
        let worst = turns(&path).into_iter().fold(0.0, f64::max);
        assert!(worst < 10.0, "a corner of {worst} degrees");
    }

    #[test]
    fn a_slow_stroke_keeps_its_detail_and_loses_its_jitter() {
        // A slow pen gives a sample at each point. A wiggle of 6 points
        // with a wavelength of 30 points is detail. A jitter of half a
        // point at each sample is the hand.
        let n = 120;
        let wave = |x: f64| 6.0 * (x * std::f64::consts::TAU / 30.0).sin();
        let jitter = |i: usize| if i % 2 == 0 { 0.5 } else { -0.5 };
        let samples: Vec<V3> = (0..=n)
            .map(|i| {
                let x = i as f64;
                at(x, wave(x) + jitter(i))
            })
            .collect();
        let path = smooth(&samples, &[], SCALE, DEFAULT_SMOOTHING);
        let dense = resample(&path, SPACING / SCALE);
        // Away from the ends, the wave keeps most of its height.
        let inner = &dense[30..dense.len() - 30];
        let peak = inner.iter().map(|&p| offset(p)).fold(0.0, f64::max);
        assert!(peak > 0.8 * 6.0, "the wave is {peak} high");
        // The jitter is gone: the path is near the wave.
        let error = inner
            .iter()
            .map(|&p| (offset(p) - wave(p[1] / p[0] * SCALE) * peak / 6.0).abs())
            .fold(0.0, f64::max);
        assert!(error < 0.2, "the path is {error} points from the wave");
    }

    #[test]
    fn no_smoothing_keeps_the_samples_and_the_ends_stay() {
        let samples = [at(0.0, 0.0), at(10.0, 4.0), at(20.0, 0.0)];
        let path = smooth(&samples, &[], SCALE, 0.0);
        assert_eq!(path[0], samples[0]);
        assert!(angle(path[path.len() - 1], samples[2]) < 1e-12);
        let path = smooth(&samples, &[], SCALE, 1.0);
        assert!(angle(path[path.len() - 1], samples[2]) < 1e-12);
    }

    fn stroke(from: LonLat, to: LonLat, n: usize) -> Vec<V3> {
        let (a, b) = (lonlat_to_dir(from[0], from[1]), lonlat_to_dir(to[0], to[1]));
        (0..=n).map(|i| lerp(a, b, i as f64 / n as f64)).collect()
    }

    fn lines(edit: &LineEdit) -> &Vec<Vec<LonLat>> {
        let (LineEdit::New(Geometry::Lines(parts))
        | LineEdit::Extend {
            geometry: Geometry::Lines(parts),
            ..
        }) = edit
        else {
            panic!("not a line");
        };
        parts
    }

    /// Each step of a part is short in longitude, or it is at a pole.
    fn steps_are_short(parts: &[Vec<LonLat>]) -> bool {
        parts.iter().flat_map(|p| p.windows(2)).all(|w| {
            (w[1][0] - w[0][0]).abs() <= MAX_DLON || (w[0][1].abs() == 90.0 && w[0][1] == w[1][1])
        })
    }

    #[test]
    fn a_line_across_180_degrees_has_two_parts_that_meet_there() {
        let edit = commit(&[], &stroke([170.0, 10.0], [-170.0, 10.0], 40), SCALE, 0.35).unwrap();
        let parts = lines(&edit);
        assert_eq!(parts.len(), 2);
        let (end, start) = (parts[0][parts[0].len() - 1], parts[1][0]);
        assert_eq!((end[0], start[0]), (180.0, -180.0));
        assert_eq!(end[1], start[1]);
        assert!(end[1] > 10.0, "a great circle bends to the pole");
        assert!(parts[0].iter().all(|p| p[0] >= 170.0 - 1e-9));
        assert!(parts[1].iter().all(|p| p[0] <= -170.0 + 1e-9));
        assert!(steps_are_short(parts));
        // The other way.
        let edit = commit(&[], &stroke([-170.0, 0.0], [170.0, 0.0], 40), SCALE, 0.35).unwrap();
        let parts = lines(&edit);
        assert_eq!(parts[0][parts[0].len() - 1][0], -180.0);
        assert_eq!(parts[1][0][0], 180.0);
    }

    #[test]
    fn a_line_over_a_pole_has_short_steps() {
        let edit = commit(&[], &stroke([20.0, 80.0], [-160.0, 80.0], 40), SCALE, 0.35).unwrap();
        let parts = lines(&edit);
        assert!(steps_are_short(parts));
        // The path goes over the pole and comes down the other side.
        let all: Vec<LonLat> = parts.iter().flatten().copied().collect();
        assert!(all.iter().any(|p| p[1] > 89.9));
        assert!((all[0][0] - 20.0).abs() < 1e-6);
        assert!((all[all.len() - 1][0] + 160.0).abs() < 1e-6);
    }

    #[test]
    fn a_step_through_the_pole_has_two_pole_vertices() {
        let path = [lonlat_to_dir(30.0, 89.0), lonlat_to_dir(-150.0, 89.0)];
        let parts = to_parts(&path);
        assert_eq!(parts.len(), 1);
        let pole = |lon: f64| {
            parts[0]
                .iter()
                .any(|p| p[1] == 90.0 && (p[0] - lon).abs() < 1e-9)
        };
        assert!(pole(30.0) && pole(-150.0), "{parts:?}");
        assert!(steps_are_short(&parts));
    }

    fn feature(parts: Vec<Vec<LonLat>>) -> Feature {
        Feature::new(Geometry::Lines(parts), BTreeMap::new())
    }

    #[test]
    fn a_stroke_that_starts_at_a_line_end_extends_that_line() {
        let near = 6.0 / SCALE;
        let old = feature(vec![vec![[0.0, 0.0], [10.0, 0.0]]]);
        let other = feature(vec![vec![[0.0, 30.0], [10.0, 30.0]]]);
        let features = [other, old.clone()];
        // From near the end at (10, 0) to the east.
        let from = [10.0 + near.to_degrees(), 0.0];
        let edit = commit(&features, &stroke(from, [20.0, 2.0], 20), SCALE, 0.35).unwrap();
        let LineEdit::Extend { uuid, .. } = &edit else {
            panic!("a new line");
        };
        assert_eq!(*uuid, old.uuid());
        let parts = lines(&edit);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0][..2], [[0.0, 0.0], [10.0, 0.0]]);
        assert!((parts[0][parts[0].len() - 1][0] - 20.0).abs() < 1e-6);

        // From near the start at (0, 0) to the west: the line turns round.
        let from = [-near.to_degrees(), 0.0];
        let edit = commit(&features, &stroke(from, [-10.0, 0.0], 20), SCALE, 0.35).unwrap();
        let parts = lines(&edit);
        assert_eq!(parts[0][..2], [[10.0, 0.0], [0.0, 0.0]]);

        // From far away: a new line.
        let from = [10.0 + (3.0 * near).to_degrees(), 0.0];
        let edit = commit(&features, &stroke(from, [20.0, 0.0], 20), SCALE, 0.35).unwrap();
        assert!(matches!(edit, LineEdit::New(_)));
    }

    #[test]
    fn a_line_across_180_degrees_is_one_chain() {
        let crossing = vec![
            vec![[170.0, 0.0], [180.0, 0.0]],
            vec![[-180.0, 0.0], [-170.0, 0.0]],
        ];
        assert_eq!(chains(&crossing), vec![vec![0, 1]]);
        let old = feature(crossing);
        let edit = commit(
            &[old.clone()],
            &stroke([-170.0, 0.0], [-160.0, 0.0], 20),
            SCALE,
            0.35,
        )
        .unwrap();
        let parts = lines(&edit);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], vec![[170.0, 0.0], [180.0, 0.0]]);
        assert!((parts[1][parts[1].len() - 1][0] + 160.0).abs() < 1e-6);
    }

    #[test]
    fn a_tap_makes_no_line() {
        let p = lonlat_to_dir(0.0, 0.0);
        assert_eq!(commit(&[], &[p, p], SCALE, 0.35), None);
        assert_eq!(commit(&[], &[], SCALE, 0.35), None);
    }
}
