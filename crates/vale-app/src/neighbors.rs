//! Colors for polygons such that two neighbors never have the same color.
//! Two polygons are neighbors if they share a border. A border is the part
//! of the boundary of one polygon that is near the boundary of the other.
//! Polygons that touch only at a corner are not neighbors.

use std::collections::HashMap;

use vale_store::{Feature, Geometry, LonLat};

/// The tolerance of a border, as a part of the median perimeter of the
/// features. A square with sides of 1 has a tolerance of 0.01.
const TOLERANCE: f64 = 1.0 / 400.0;
/// The shortest border, in tolerances. Two polygons that touch at a corner
/// share about 2 tolerances of boundary.
const MIN_BORDER: f64 = 4.0;

type V3 = [f64; 3];

/// A position as a point on the unit sphere.
fn unit(p: LonLat) -> V3 {
    let (lon, lat) = (p[0].to_radians(), p[1].to_radians());
    [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}

fn lerp(a: V3, b: V3, t: f64) -> V3 {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// The distance from `p` to the segment from `a` to `b`.
fn segment_distance(p: V3, a: V3, b: V3) -> f64 {
    let ab = sub(b, a);
    let n = dot(ab, ab);
    let t = if n > 0.0 {
        (dot(sub(p, a), ab) / n).clamp(0.0, 1.0)
    } else {
        0.0
    };
    len(sub(p, lerp(a, b, t)))
}

/// The segments of the boundary of each feature, on the unit sphere. A
/// feature that is not a polygon has no segments.
fn boundaries(features: &[Feature]) -> Vec<Vec<(V3, V3)>> {
    features
        .iter()
        .map(|f| match &f.geometry {
            Geometry::Polygons(polygons) => polygons
                .iter()
                .flatten()
                .flat_map(|ring| {
                    let points: Vec<V3> = ring.iter().map(|p| unit(*p)).collect();
                    let n = points.len();
                    (0..n)
                        .map(move |i| (points[i], points[(i + 1) % n]))
                        .filter(|(a, b)| a != b)
                        .collect::<Vec<_>>()
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

/// The area of each feature on the unit sphere. The area of a ring is the
/// area of the flat polygon through its points, and a hole subtracts its
/// area. Good for polygons that are small next to the sphere.
pub fn areas(features: &[Feature]) -> Vec<f64> {
    features
        .iter()
        .map(|f| match &f.geometry {
            Geometry::Polygons(polygons) => polygons
                .iter()
                .map(|rings| {
                    rings
                        .iter()
                        .enumerate()
                        .map(|(i, ring)| {
                            let area = ring_area(ring);
                            if i == 0 { area } else { -area }
                        })
                        .sum::<f64>()
                        .max(0.0)
                })
                .sum(),
            _ => 0.0,
        })
        .collect()
}

fn ring_area(ring: &[LonLat]) -> f64 {
    let points: Vec<V3> = ring.iter().map(|p| unit(*p)).collect();
    let n = points.len();
    let mut sum = [0.0; 3];
    for i in 0..n {
        let c = cross(points[i], points[(i + 1) % n]);
        sum = [sum[0] + c[0], sum[1] + c[1], sum[2] + c[2]];
    }
    len(sum) / 2.0
}

/// The neighbors of each feature, as indices of features, in order.
pub fn neighbors(features: &[Feature]) -> Vec<Vec<usize>> {
    let segments = boundaries(features);
    let count = segments.len();
    let mut perimeters: Vec<f64> = segments
        .iter()
        .map(|s| s.iter().map(|(a, b)| len(sub(*b, *a))).sum::<f64>())
        .filter(|p| *p > 0.0)
        .collect();
    let mut out = vec![Vec::new(); count];
    if perimeters.len() < 2 {
        return out;
    }
    perimeters.sort_by(f64::total_cmp);
    let tol = (perimeters[perimeters.len() / 2] * TOLERANCE).max(1e-12);

    // A grid of cells of 2 tolerances. A segment is in each cell that a
    // point on it is in, at steps of 1 tolerance, so a point within one
    // tolerance of the segment is in one of the 27 cells around such a cell.
    let cell = 2.0 * tol;
    let key = |p: V3| {
        (
            (p[0] / cell).floor() as i64,
            (p[1] / cell).floor() as i64,
            (p[2] / cell).floor() as i64,
        )
    };
    let mut grid: HashMap<(i64, i64, i64), Vec<(u32, u32)>> = HashMap::new();
    for (f, list) in segments.iter().enumerate() {
        for (s, (a, b)) in list.iter().enumerate() {
            let steps = (len(sub(*b, *a)) / tol).ceil().max(1.0) as usize;
            let mut last = None;
            for i in 0..=steps {
                let k = key(lerp(*a, *b, i as f64 / steps as f64));
                if last != Some(k) {
                    let entry = grid.entry(k).or_default();
                    if entry.last() != Some(&(f as u32, s as u32)) {
                        entry.push((f as u32, s as u32));
                    }
                    last = Some(k);
                }
            }
        }
    }

    // The length of the boundary of each feature that is near another feature.
    let mut shared: HashMap<(usize, usize), f64> = HashMap::new();
    let mut near: Vec<usize> = Vec::new();
    for (f, list) in segments.iter().enumerate() {
        for (a, b) in list {
            let length = len(sub(*b, *a));
            let steps = (length / tol).ceil().max(1.0) as usize;
            let weight = length / steps as f64;
            for i in 0..steps {
                let p = lerp(*a, *b, (i as f64 + 0.5) / steps as f64);
                let (x, y, z) = key(p);
                near.clear();
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            let Some(entry) = grid.get(&(x + dx, y + dy, z + dz)) else {
                                continue;
                            };
                            for &(g, s) in entry {
                                let g = g as usize;
                                if g == f || near.contains(&g) {
                                    continue;
                                }
                                let (c, d) = segments[g][s as usize];
                                if segment_distance(p, c, d) <= tol {
                                    near.push(g);
                                }
                            }
                        }
                    }
                }
                for &g in &near {
                    *shared.entry((f, g)).or_default() += weight;
                }
            }
        }
    }
    for (&(f, g), &length) in &shared {
        if length >= MIN_BORDER * tol {
            out[f].push(g);
            out[g].push(f);
        }
    }
    for list in &mut out {
        list.sort_unstable();
        list.dedup();
    }
    out
}

/// A color for each feature, from 0, such that no two neighbors have the
/// same color. It uses at least `colors` colors if the features allow it,
/// and more only if the features need them. Each feature takes the free
/// color with the least area so far, so the colors cover about the same
/// area. Equal input gives equal colors.
pub fn color(neighbors: &[Vec<usize>], areas: &[f64], colors: usize) -> Vec<usize> {
    let count = neighbors.len();
    let mut out: Vec<Option<usize>> = vec![None; count];
    let mut totals: Vec<f64> = vec![0.0; colors.max(1)];
    // DSatur: the next feature is the one with the most colors among its
    // neighbors, then the one with the most neighbors, then the largest.
    let mut seen: Vec<Vec<usize>> = vec![Vec::new(); count];
    for _ in 0..count {
        let Some(next) = (0..count).filter(|&i| out[i].is_none()).max_by(|&a, &b| {
            seen[a]
                .len()
                .cmp(&seen[b].len())
                .then(neighbors[a].len().cmp(&neighbors[b].len()))
                .then(areas[a].total_cmp(&areas[b]))
                .then(b.cmp(&a))
        }) else {
            break;
        };
        let free = (0..totals.len())
            .filter(|c| !seen[next].contains(c))
            .min_by(|&a, &b| totals[a].total_cmp(&totals[b]).then(a.cmp(&b)));
        let c = free.unwrap_or_else(|| {
            totals.push(0.0);
            totals.len() - 1
        });
        totals[c] += areas[next];
        out[next] = Some(c);
        for &n in &neighbors[next] {
            if !seen[n].contains(&c) {
                seen[n].push(c);
            }
        }
    }
    out.into_iter().map(Option::unwrap_or_default).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square with its lower left corner at (`x`, `y`), in degrees.
    fn square(x: f64, y: f64, size: f64) -> Feature {
        let ring = vec![[x, y], [x + size, y], [x + size, y + size], [x, y + size]];
        Feature::new(Geometry::Polygons(vec![vec![ring]]), Default::default())
    }

    /// A grid of `n` by `n` squares of 1 degree, row by row.
    fn grid(n: usize) -> Vec<Feature> {
        (0..n * n)
            .map(|i| square((i % n) as f64, (i / n) as f64, 1.0))
            .collect()
    }

    #[test]
    fn squares_that_share_an_edge_are_neighbors_and_corners_are_not() {
        let found = neighbors(&grid(3));
        // The middle square has 4 neighbors, not 8.
        assert_eq!(found[4], vec![1, 3, 5, 7]);
        assert_eq!(found[0], vec![1, 3]);
    }

    #[test]
    fn a_small_gap_or_overlap_is_still_a_border() {
        let gap = [square(0.0, 0.0, 1.0), square(1.005, 0.0, 1.0)];
        assert_eq!(neighbors(&gap)[0], vec![1]);
        let overlap = [square(0.0, 0.0, 1.0), square(0.995, 0.3, 1.0)];
        assert_eq!(neighbors(&overlap)[0], vec![1]);
        let apart = [square(0.0, 0.0, 1.0), square(1.2, 0.0, 1.0)];
        assert!(neighbors(&apart)[0].is_empty());
    }

    #[test]
    fn a_border_on_the_antimeridian_counts() {
        let west = square(179.0, 0.0, 1.0);
        let east = square(-180.0, 0.0, 1.0);
        let found = neighbors(&[west, east]);
        assert_eq!(found[0], vec![1]);
    }

    #[test]
    fn neighbors_never_share_a_color() {
        let features = grid(6);
        let found = neighbors(&features);
        let colors = color(&found, &areas(&features), 4);
        assert!(colors.iter().all(|c| *c < 4), "{colors:?}");
        for (i, list) in found.iter().enumerate() {
            for &j in list {
                assert_ne!(colors[i], colors[j], "{i} {j}");
            }
        }
    }

    #[test]
    fn colors_cover_about_the_same_area() {
        // Ten squares apart from each other: no neighbors.
        let features: Vec<Feature> = (0..10).map(|i| square(i as f64 * 3.0, 0.0, 1.0)).collect();
        let colors = color(&neighbors(&features), &areas(&features), 5);
        let mut counts = [0; 5];
        for c in colors {
            counts[c] += 1;
        }
        assert_eq!(counts, [2; 5]);
    }

    #[test]
    fn a_dense_graph_takes_more_colors() {
        // Five features that are all neighbors of each other.
        let all: Vec<Vec<usize>> = (0..5)
            .map(|i| (0..5).filter(|j| *j != i).collect())
            .collect();
        let mut colors = color(&all, &[1.0; 5], 3);
        colors.sort_unstable();
        assert_eq!(colors, [0, 1, 2, 3, 4]);
    }
}
