//! The clip of features to a mask. A mask is a region of polygons in
//! longitude and latitude, and the polygons can overlap. A point is in the
//! region when a polygon of the region holds it.
//!
//! Each edge is the short arc of a great circle, as the globe draws it.

use vale_store::LonLat;

use super::combine::{Bounds, Feature, dir};
use super::line::to_parts_exact;
use super::math::{V3, angle, cross, dot, normalize};
use super::polygon::to_polygons_many;
use super::polygon::{SAME, bounds, crossing, dirs, overlap, overlay_open, signed_area};
use super::reshape::outlines;
use super::snap::{PathVertex, distance_to_arc, slerp};
use super::vector;
use super::vertex::lonlat;

/// A smaller area, in steradians, is no polygon.
const MIN_AREA: f64 = 1e-14;

/// A region, prepared for many clips.
pub(crate) struct Region {
    parts: Vec<Part>,
}

/// One polygon of a region.
struct Part {
    sides: Vec<Side>,
    /// A box around the area of the polygon.
    bounds: Bounds,
    rings: Vec<Ring>,
}

/// A closed ring: the last vertex is the first again.
struct Ring {
    vertices: Vec<LonLat>,
    dirs: Vec<V3>,
    /// A box around the edge from each vertex to the next.
    boxes: Vec<Bounds>,
}

impl Ring {
    fn new(ring: &[LonLat]) -> Self {
        let mut vertices = ring.to_vec();
        if vertices.len() > 1 && vertices.first() != vertices.last() {
            vertices.push(vertices[0]);
        }
        let dirs: Vec<V3> = vertices.iter().map(|&p| dir(p)).collect();
        let boxes = dirs.windows(2).map(|w| bounds(w[0], w[1])).collect();
        Ring {
            vertices,
            dirs,
            boxes,
        }
    }
}

/// The edges of a ring in blocks, with a box and a point on the left of
/// each block, for a quick test of the side of a point.
struct Side {
    ring: Vec<V3>,
    blocks: Vec<Block>,
}

struct Block {
    start: usize,
    bounds: Bounds,
    /// A point just on the left of the first edge of the block.
    left: V3,
}

/// The number of edges in a block.
const BLOCK: usize = 32;

impl Side {
    fn new(ring: Vec<V3>) -> Self {
        let n = ring.len();
        let edge = |i: usize| (ring[i], ring[(i + 1) % n]);
        let blocks = (0..n)
            .step_by(BLOCK)
            .map(|start| {
                let (a, b) = edge(start);
                let m = normalize([0, 1, 2].map(|k| a[k] + b[k]));
                let c = normalize(cross(a, b));
                Block {
                    start,
                    bounds: (start..(start + BLOCK).min(n))
                        .map(|i| bounds(edge(i).0, edge(i).1))
                        .reduce(join)
                        .expect("an edge"),
                    left: normalize([0, 1, 2].map(|k| m[k] + 1e-7 * c[k])),
                }
            })
            .collect();
        Side { ring, blocks }
    }

    /// The point is on the left of the ring: the arc from it to the nearest
    /// point on the left of a block crosses the ring an even number of times.
    fn left(&self, p: V3) -> bool {
        let n = self.ring.len();
        let Some(near) =
            (self.blocks.iter()).max_by(|a, b| dot(a.left, p).total_cmp(&dot(b.left, p)))
        else {
            return false;
        };
        let q = near.left;
        if dot(p, q) < 0.0 {
            return vector::contains(std::slice::from_ref(&self.ring), p);
        }
        let path = bounds(p, q);
        let mut crossings = 0;
        for block in self.blocks.iter().filter(|b| overlap(&b.bounds, &path)) {
            for i in block.start..(block.start + BLOCK).min(n) {
                let (a, b) = (self.ring[i], self.ring[(i + 1) % n]);
                if vector::arcs_cross(p, q, a, b) {
                    crossings += 1;
                }
            }
        }
        crossings % 2 == 0
    }
}

impl Region {
    pub(crate) fn new(polygons: &[Vec<Vec<LonLat>>]) -> Region {
        let parts = polygons
            .iter()
            .filter(|p| !p.is_empty())
            .map(|polygon| {
                let sides: Vec<Side> = (polygon.iter().enumerate())
                    .map(|(i, r)| Side::new(vector::ring(r, i > 0)))
                    .collect();
                let rings: Vec<Ring> = polygon.iter().map(|r| Ring::new(r)).collect();
                let bounds = area_bounds(rings.iter().flat_map(|r| &r.boxes), |p| {
                    sides.iter().all(|s| s.left(p))
                });
                Part {
                    sides,
                    bounds,
                    rings,
                }
            })
            .collect();
        Region { parts }
    }

    /// A polygon of the region holds the point.
    pub(crate) fn holds(&self, p: LonLat) -> bool {
        self.holds_dir(dir(p))
    }

    fn holds_dir(&self, p: V3) -> bool {
        self.parts
            .iter()
            .any(|part| in_box(&part.bounds, p) && part.sides.iter().all(|s| s.left(p)))
    }

    /// The edges of the region near a box, in runs of edges that follow each
    /// other. A ring with all of its edges near is one closed run.
    fn near(&self, b: &Bounds) -> (Vec<Vec<PathVertex>>, Vec<Vec<PathVertex>>) {
        let mut closed = Vec::new();
        let mut open = Vec::new();
        for part in self.parts.iter().filter(|p| overlap(&p.bounds, b)) {
            for ring in &part.rings {
                let n = ring.boxes.len();
                let near: Vec<bool> = ring.boxes.iter().map(|e| overlap(e, b)).collect();
                if n > 0 && near.iter().all(|&x| x) {
                    let v = &ring.vertices[..n];
                    closed.push(v.iter().map(|&p| PathVertex::Exact(p)).collect());
                    continue;
                }
                let mut run: Vec<PathVertex> = Vec::new();
                for (i, &is_near) in near.iter().enumerate() {
                    if is_near {
                        if run.is_empty() {
                            run.push(PathVertex::Exact(ring.vertices[i]));
                        }
                        run.push(PathVertex::Exact(ring.vertices[i + 1]));
                    } else if !run.is_empty() {
                        open.push(std::mem::take(&mut run));
                    }
                }
                if !run.is_empty() {
                    open.push(run);
                }
            }
        }
        (closed, open)
    }

    /// The edges of the region near a box.
    fn edges_near(&self, b: &Bounds) -> Vec<(V3, V3, Bounds)> {
        let mut out = Vec::new();
        for part in self.parts.iter().filter(|p| overlap(&p.bounds, b)) {
            for ring in &part.rings {
                for (i, e) in ring.boxes.iter().enumerate() {
                    let (a, c) = (ring.dirs[i], ring.dirs[i + 1]);
                    if overlap(e, b) && angle(a, c) >= SAME {
                        out.push((a, c, *e));
                    }
                }
            }
        }
        out
    }
}

/// The part of polygons inside the region.
pub(crate) fn clip_polygons(
    region: &Region,
    polygons: &[Vec<Vec<LonLat>>],
) -> Vec<Vec<Vec<LonLat>>> {
    let Some(&first) = polygons.iter().flatten().flatten().next() else {
        return Vec::new();
    };
    let feature = Feature::new(polygons);
    let rings: Vec<Ring> = polygons.iter().flatten().map(|r| Ring::new(r)).collect();
    let b = area_bounds(rings.iter().flat_map(|r| &r.boxes), |p| feature.holds(p));
    let (closed, open) = region.near(&b);
    if closed.is_empty() && open.is_empty() {
        // No edge of the region meets the box, so the region holds all of
        // the polygons or none.
        return if region.holds(first) {
            polygons.to_vec()
        } else {
            Vec::new()
        };
    }
    let mut paths: Vec<Vec<PathVertex>> = match outlines(polygons) {
        Some(o) => o.into_iter().map(|o| o.ring).collect(),
        None => (rings.iter())
            .map(|r| r.vertices.iter().map(|&p| PathVertex::Exact(p)).collect())
            .collect(),
    };
    paths.extend(closed);
    let kept: Vec<Vec<PathVertex>> =
        overlay_open(&paths, &open, |p| feature.holds(p) && region.holds_dir(p))
            .into_iter()
            .filter(|r| signed_area(&dirs(r)).abs() > MIN_AREA)
            .collect();
    to_polygons_many(&kept)
}

/// The parts of lines inside the region, in the order and the direction of
/// each line.
pub(crate) fn clip_lines(region: &Region, lines: &[Vec<LonLat>]) -> Vec<Vec<LonLat>> {
    let mut out = Vec::new();
    for line in lines.iter().filter(|l| l.len() >= 2) {
        clip_line(region, line, &mut out);
    }
    out
}

fn clip_line(region: &Region, line: &[LonLat], out: &mut Vec<Vec<LonLat>>) {
    let points: Vec<V3> = line.iter().map(|&p| dir(p)).collect();
    let boxes: Vec<Bounds> = points.windows(2).map(|w| bounds(w[0], w[1])).collect();
    let all = boxes.iter().copied().reduce(join).expect("an edge");
    let edges = region.edges_near(&all);
    if edges.is_empty() {
        if region.holds(line[0]) {
            out.push(line.to_vec());
        }
        return;
    }
    // The places along each edge of the line where it meets the region, by
    // their angle from its start.
    let mut hits: Vec<Vec<f64>> = vec![Vec::new(); boxes.len()];
    let mut order: Vec<(f64, usize, bool)> = (boxes.iter().enumerate())
        .map(|(i, b)| (b.0[0], i, true))
        .chain(edges.iter().enumerate().map(|(j, e)| (e.2.0[0], j, false)))
        .collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let hi = |&(_, i, own): &(f64, usize, bool)| {
        if own { boxes[i].1[0] } else { edges[i].2.1[0] }
    };
    for (k, x) in order.iter().enumerate() {
        for y in &order[k + 1..] {
            if y.0 > hi(x) {
                break;
            }
            let (s, e) = match (x.2, y.2) {
                (true, false) => (x.1, y.1),
                (false, true) => (y.1, x.1),
                _ => continue,
            };
            if !overlap(&boxes[s], &edges[e].2) {
                continue;
            }
            let (a, b) = (points[s], points[s + 1]);
            let (c, d) = (edges[e].0, edges[e].1);
            match crossing(a, b, c, d) {
                Some(p) => hits[s].push(angle(a, p)),
                None => {
                    let on = |p: V3, (u, v): (V3, V3)| distance_to_arc(p, u, v) < SAME;
                    for p in [a, b, c, d] {
                        if on(p, (a, b)) && on(p, (c, d)) {
                            hits[s].push(angle(a, p));
                        }
                    }
                }
            }
        }
    }
    let mut piece: Vec<LonLat> = Vec::new();
    let mut inside = false;
    let mut test = true;
    for (s, mut ts) in hits.into_iter().enumerate() {
        let (a, b) = (points[s], points[s + 1]);
        let len = angle(a, b);
        ts.sort_by(f64::total_cmp);
        // A meeting at the start of the edge needs a test after it.
        if ts.first().is_some_and(|&t| t < SAME) {
            test = true;
        }
        let at_end = ts.last().is_some_and(|&t| t > len - SAME);
        let mut stops: Vec<(V3, LonLat)> = vec![(a, line[s])];
        for t in ts.into_iter().filter(|&t| t >= SAME && t <= len - SAME) {
            let p = slerp(a, b, t / len);
            if angle(stops[stops.len() - 1].0, p) >= SAME {
                stops.push((p, on_side(lonlat(p), line[s], line[s + 1])));
            }
        }
        stops.push((b, line[s + 1]));
        let last = stops.len() - 2;
        for (k, w) in stops.windows(2).enumerate() {
            let split = k < last || at_end;
            if test || split {
                let mid = normalize([0, 1, 2].map(|i| w[0].0[i] + w[1].0[i]));
                inside = region.holds_dir(mid);
            }
            test = split;
            if inside {
                if piece.is_empty() {
                    piece.push(w[0].1);
                }
                piece.push(w[1].1);
            } else if !piece.is_empty() {
                finish(&mut piece, out);
            }
        }
    }
    finish(&mut piece, out);
}

fn finish(piece: &mut Vec<LonLat>, out: &mut Vec<Vec<LonLat>>) {
    let p = std::mem::take(piece);
    if p.len() >= 2 {
        out.push(p);
    }
}

/// A point on an edge, on the side of the 180 degree meridian of its ends.
fn on_side(p: LonLat, a: LonLat, b: LonLat) -> LonLat {
    if p[0].abs() > 179.0 && a[0].signum() == b[0].signum() && p[0].signum() != a[0].signum() {
        [-p[0], p[1]]
    } else {
        p
    }
}

/// The outlines of polygons, as lines. The edges along the 180 degree
/// meridian and along a pole are no part of an outline.
pub(crate) fn polygon_outlines(polygons: &[Vec<Vec<LonLat>>]) -> Vec<Vec<LonLat>> {
    match outlines(polygons) {
        Some(o) => o
            .into_iter()
            .flat_map(|o| {
                let mut ring = o.ring;
                if let Some(&first) = ring.first() {
                    ring.push(first);
                }
                to_parts_exact(&ring)
            })
            .collect(),
        None => polygons
            .iter()
            .flatten()
            .map(|r| Ring::new(r).vertices)
            .filter(|r| r.len() >= 2)
            .collect(),
    }
}

/// A box around an area: around the boxes of its edges, and out to each
/// pole of an axis that the area holds.
fn area_bounds<'a>(edges: impl Iterator<Item = &'a Bounds>, holds: impl Fn(V3) -> bool) -> Bounds {
    let mut b = edges.copied().reduce(join).unwrap_or(([0.0; 3], [0.0; 3]));
    for k in 0..3 {
        let mut pole = [0.0; 3];
        pole[k] = 1.0;
        if holds(pole) {
            b.1[k] = 1.0;
        }
        pole[k] = -1.0;
        if holds(pole) {
            b.0[k] = -1.0;
        }
    }
    b
}

fn join(a: Bounds, b: Bounds) -> Bounds {
    (
        [0, 1, 2].map(|k| a.0[k].min(b.0[k])),
        [0, 1, 2].map(|k| a.1[k].max(b.1[k])),
    )
}

fn in_box(b: &Bounds, p: V3) -> bool {
    (0..3).all(|k| b.0[k] <= p[k] && p[k] <= b.1[k])
}

#[cfg(test)]
mod tests {
    use super::super::polygon::tests::assert_valid;
    use super::*;

    /// A box in longitude and latitude, with a vertex at each degree, going
    /// counterclockwise.
    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Vec<LonLat>> {
        let steps = |a: f64, b: f64| {
            let n = (b - a).abs().ceil() as usize;
            (0..n).map(move |k| a + (b - a) * k as f64 / n as f64)
        };
        let mut ring: Vec<LonLat> = Vec::new();
        ring.extend(steps(x0, x1).map(|x| [x, y0]));
        ring.extend(steps(y0, y1).map(|y| [x1, y]));
        ring.extend(steps(x1, x0).map(|x| [x, y1]));
        ring.extend(steps(y1, y0).map(|y| [x0, y]));
        ring.push(ring[0]);
        vec![ring]
    }

    fn area(polygons: &[Vec<Vec<LonLat>>]) -> f64 {
        (polygons.iter())
            .flat_map(|p| p.iter().enumerate())
            .map(|(i, r)| signed_area(&vector::ring(r, i > 0)))
            .sum()
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-3 * b.abs()
    }

    /// The distance from a point to the nearest edge of rings, in radians.
    fn to_edges(p: V3, rings: &[Vec<LonLat>]) -> f64 {
        (rings.iter())
            .flat_map(|r| r.windows(2))
            .map(|w| distance_to_arc(p, dir(w[0]), dir(w[1])))
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn a_polygon_half_over_the_mask_keeps_the_half_inside() {
        let mask = square(5.0, -5.0, 15.0, 15.0);
        let region = Region::new(std::slice::from_ref(&mask));
        let clipped = clip_polygons(&region, &[square(0.0, 0.0, 10.0, 10.0)]);
        assert_valid(&clipped);
        let want = area(&[square(5.0, 0.0, 10.0, 10.0)]);
        assert!(near(area(&clipped), want), "{} {want}", area(&clipped));
        for &p in clipped.iter().flatten().flatten() {
            assert!(region.holds(p) || to_edges(dir(p), &mask) < 1e-9, "{p:?}");
        }
    }

    #[test]
    fn a_polygon_inside_the_mask_stays_whole() {
        let region = Region::new(&[square(5.0, -5.0, 15.0, 15.0)]);
        let polygon = [square(6.0, 1.0, 9.0, 4.0)];
        let clipped = clip_polygons(&region, &polygon);
        assert!(near(area(&clipped), area(&polygon)));
        let edge = [square(5.5, -4.5, 14.5, 14.5)];
        assert!(near(area(&clip_polygons(&region, &edge)), area(&edge)));
    }

    #[test]
    fn a_polygon_outside_the_mask_is_gone() {
        let region = Region::new(&[square(5.0, -5.0, 15.0, 15.0)]);
        assert!(clip_polygons(&region, &[square(20.0, 0.0, 30.0, 10.0)]).is_empty());
        // The polygon holds the mask.
        let around = [square(-20.0, -20.0, 40.0, 40.0)];
        let clipped = clip_polygons(&region, &around);
        assert!(near(area(&clipped), area(&[square(5.0, -5.0, 15.0, 15.0)])));
    }

    #[test]
    fn overlapping_mask_polygons_clip_to_their_union() {
        let region = Region::new(&[square(0.0, -5.0, 10.0, 15.0), square(5.0, -5.0, 15.0, 15.0)]);
        let clipped = clip_polygons(&region, &[square(-5.0, 0.0, 20.0, 10.0)]);
        assert_valid(&clipped);
        let want = area(&[square(0.0, 0.0, 15.0, 10.0)]);
        assert!(near(area(&clipped), want), "{} {want}", area(&clipped));
    }

    #[test]
    fn a_polygon_across_180_clips_to_a_mask_across_180() {
        let mask = [
            square(175.0, -5.0, 180.0, 15.0),
            square(-180.0, -5.0, -165.0, 15.0),
        ];
        let region = Region::new(&mask);
        let polygon = [
            square(170.0, 0.0, 180.0, 10.0),
            square(-180.0, 0.0, -170.0, 10.0),
        ];
        let clipped = clip_polygons(&region, &polygon);
        assert_valid(&clipped);
        let want = area(&[square(0.0, 0.0, 15.0, 10.0)]);
        assert!(near(area(&clipped), want), "{} {want}", area(&clipped));
        let lines = clip_lines(&region, &[vec![[170.0, 5.0], [180.0, 5.0]]]);
        assert_eq!(lines.len(), 1);
        assert!((lines[0][0][0] - 175.0).abs() < 1e-6, "{lines:?}");
        assert_eq!(lines[0][1], [180.0, 5.0]);
    }

    #[test]
    fn a_mask_around_a_pole_clips_a_polygon_near_the_pole() {
        let mut cap: Vec<LonLat> = (0..=36)
            .map(|k| [-180.0 + 10.0 * f64::from(k), 80.0])
            .collect();
        cap.extend([[180.0, 90.0], [-180.0, 90.0], [-180.0, 80.0]]);
        let region = Region::new(&[vec![cap]]);
        assert!(region.holds([0.0, 89.0]));
        let clipped = clip_polygons(&region, &[square(0.0, 70.0, 10.0, 85.0)]);
        assert_valid(&clipped);
        let want = area(&[square(0.0, 80.0, 10.0, 85.0)]);
        assert!(
            (area(&clipped) - want).abs() < 0.05 * want,
            "{} {want}",
            area(&clipped)
        );
    }

    #[test]
    fn a_line_across_the_mask_keeps_the_piece_inside() {
        let region = Region::new(&[square(5.0, -5.0, 15.0, 15.0)]);
        let line: Vec<LonLat> = (-10..=30).map(|x| [f64::from(x) + 0.5, 5.0]).collect();
        let clipped = clip_lines(&region, &[line]);
        assert_eq!(clipped.len(), 1, "{clipped:?}");
        let piece = &clipped[0];
        let (a, b) = (piece[0], piece[piece.len() - 1]);
        assert!(
            (a[0] - 5.0).abs() < 1e-9 && (a[1] - 5.0).abs() < 0.01,
            "{a:?}"
        );
        assert!(
            (b[0] - 15.0).abs() < 1e-9 && (b[1] - 5.0).abs() < 0.01,
            "{b:?}"
        );
        assert!(piece.windows(2).all(|w| w[0][0] < w[1][0]));
        assert_eq!(piece.len(), 12);
    }

    #[test]
    fn a_line_inside_or_outside_the_mask_stays_or_goes() {
        let region = Region::new(&[square(5.0, -5.0, 15.0, 15.0)]);
        let inside = vec![[6.0, 0.0], [8.0, 2.0], [7.0, 3.0]];
        assert_eq!(
            clip_lines(&region, std::slice::from_ref(&inside)),
            vec![inside]
        );
        assert!(clip_lines(&region, &[vec![[20.0, 0.0], [30.0, 0.0]]]).is_empty());
    }

    #[test]
    fn the_outline_of_a_polygon_half_over_the_mask_stays_on_the_outline() {
        let region = Region::new(&[square(5.0, -5.0, 15.0, 15.0)]);
        let polygon = square(0.0, 0.0, 10.0, 10.0);
        let outlines = polygon_outlines(std::slice::from_ref(&polygon));
        let clipped = clip_lines(&region, &outlines);
        assert!(!clipped.is_empty());
        let mut length = 0.0;
        for piece in &clipped {
            for w in piece.windows(2) {
                let (a, b) = (dir(w[0]), dir(w[1]));
                let mid = normalize([0, 1, 2].map(|k| a[k] + b[k]));
                for p in [a, b, mid] {
                    assert!(to_edges(p, &polygon) < 1e-9, "{w:?}");
                }
                length += angle(a, b);
            }
        }
        // The top, the right side, and the bottom of the half inside.
        let want = (5.0 + 10.0 + 5.0f64).to_radians();
        assert!((length - want).abs() < 0.01 * want, "{length} {want}");
    }

    #[test]
    #[ignore = "manual"]
    fn manual_clip_time() {
        let n = 20_000;
        let ring: Vec<LonLat> = (0..=n)
            .map(|k| {
                let t = std::f64::consts::TAU * f64::from(k) / f64::from(n);
                [30.0 * t.cos(), 30.0 * t.sin()]
            })
            .collect();
        let start = std::time::Instant::now();
        let region = Region::new(&[vec![ring]]);
        let polygons: Vec<Vec<Vec<LonLat>>> = (0..50)
            .map(|k| {
                let t = std::f64::consts::TAU * f64::from(k) / 50.0;
                let (x, y) = (30.0 * t.cos(), 30.0 * t.sin());
                square(x - 2.0, y - 2.0, x + 2.0, y + 2.0)
            })
            .collect();
        let mut kept = 0;
        for p in &polygons {
            kept += clip_polygons(&region, std::slice::from_ref(p)).len();
        }
        let outlines: Vec<Vec<LonLat>> = polygons
            .iter()
            .flat_map(|p| polygon_outlines(std::slice::from_ref(p)))
            .collect();
        let lines = clip_lines(&region, &outlines);
        println!(
            "{kept} polygons, {} lines in {:?}",
            lines.len(),
            start.elapsed()
        );
        assert_eq!(kept, 50);
    }
}
