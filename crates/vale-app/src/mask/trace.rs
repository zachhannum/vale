//! The outline of a value range in a grid, by marching squares.

use std::collections::HashMap;

use kurbo::Point;

/// The rings around the samples of `grid` with a value in `lo..hi`.
///
/// `grid` has `width` × `height` samples, row by row. The sample of pixel
/// (i, j) is at (i + 0.5, j + 0.5) in grid units. NaN is outside the range,
/// and so is each place outside the grid, so a ring closes at the edge of
/// the grid. The capture writes NaN where no ground is.
///
/// A ring crosses the edge between two samples at the point where the linear
/// value meets the limit. It crosses the edge to a NaN sample, or to a
/// place outside the grid, halfway. In a saddle cell, the mean of the four
/// corners decides if the inside joins through the middle. An edge between
/// a sample under `lo` and a sample from `hi` up has no crossing.
///
/// The inside is on the left of each ring on a page with y down: an outer
/// ring has a negative shoelace area, and a hole has a positive one. The
/// rings do not cross, so the even-odd rule and the nonzero rule give the
/// same region. The last point of a ring does not repeat the first.
pub fn trace(grid: &[f32], width: usize, height: usize, lo: f32, hi: f32) -> Vec<Vec<Point>> {
    assert_eq!(grid.len(), width * height, "the grid size");
    let field = Field {
        grid,
        width,
        height,
        lo,
        hi,
    };
    // The samples with a ring of outside samples around them.
    let (w, h) = (width + 2, height + 2);
    // The crossing that leaves each crossing, by the edge ids.
    let mut next: HashMap<usize, usize> = HashMap::new();
    let mut points: HashMap<usize, Point> = HashMap::new();
    let mut starts = Vec::new();
    for j in 0..h - 1 {
        for i in 0..w - 1 {
            let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            let inside = corners.map(|(x, y)| field.inside(x, y));
            if inside.iter().all(|b| *b) || !inside.iter().any(|b| *b) {
                continue;
            }
            // Edge k runs from corner k to corner k + 1, clockwise.
            let edges = [
                2 * (j * w + i),
                2 * (j * w + i + 1) + 1,
                2 * ((j + 1) * w + i),
                2 * (j * w + i) + 1,
            ];
            let saddle =
                inside == [true, false, true, false] || inside == [false, true, false, true];
            let joined = saddle && field.inside_value(corners.map(|(x, y)| field.value(x, y)));
            let exit = |k: usize| inside[k] && !inside[(k + 1) % 4];
            for k in (0..4).filter(|&k| !inside[k] && inside[(k + 1) % 4]) {
                let to = if joined {
                    (1..4).map(|d| (k + 4 - d) % 4).find(|&m| exit(m))
                } else {
                    (1..4).map(|d| (k + d) % 4).find(|&m| exit(m))
                };
                let Some(m) = to else { continue };
                for e in [k, m] {
                    let (a, b) = (corners[e], corners[(e + 1) % 4]);
                    points
                        .entry(edges[e])
                        .or_insert_with(|| field.crossing(a, b));
                }
                next.insert(edges[k], edges[m]);
                starts.push(edges[k]);
            }
        }
    }
    let mut rings = Vec::new();
    for start in starts {
        let Some(mut at) = next.remove(&start) else {
            continue;
        };
        let mut ring = vec![points[&start]];
        while at != start {
            ring.push(points[&at]);
            match next.remove(&at) {
                Some(n) => at = n,
                None => break,
            }
        }
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }
    rings
}

struct Field<'a> {
    grid: &'a [f32],
    width: usize,
    height: usize,
    lo: f32,
    hi: f32,
}

impl Field<'_> {
    /// The value of sample (x, y) of the padded grid. NaN on the pad.
    fn value(&self, x: usize, y: usize) -> f32 {
        if x == 0 || y == 0 || x > self.width || y > self.height {
            return f32::NAN;
        }
        self.grid[(y - 1) * self.width + x - 1]
    }

    fn inside(&self, x: usize, y: usize) -> bool {
        let v = self.value(x, y);
        v >= self.lo && v < self.hi
    }

    /// The mean of `v` is in the range.
    fn inside_value(&self, v: [f32; 4]) -> bool {
        let mean = v.iter().map(|v| f64::from(*v)).sum::<f64>() / 4.0;
        mean >= f64::from(self.lo) && mean < f64::from(self.hi)
    }

    /// The crossing on the edge between sample `a` and sample `b`, in grid
    /// units. One of them is inside.
    fn crossing(&self, a: (usize, usize), b: (usize, usize)) -> Point {
        let (p, q) = if self.inside(a.0, a.1) {
            (a, b)
        } else {
            (b, a)
        };
        let (vp, vq) = (
            f64::from(self.value(p.0, p.1)),
            f64::from(self.value(q.0, q.1)),
        );
        let limit = if vq < f64::from(self.lo) {
            f64::from(self.lo)
        } else {
            f64::from(self.hi)
        };
        let t = if vp.is_finite() && vq.is_finite() {
            ((limit - vp) / (vq - vp)).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let at = |s: (usize, usize)| Point::new(s.0 as f64 - 0.5, s.1 as f64 - 0.5);
        at(p).lerp(at(q), t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(ring: &[Point]) -> f64 {
        let n = ring.len();
        (0..n)
            .map(|i| {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                a.x * b.y - b.x * a.y
            })
            .sum::<f64>()
            / 2.0
    }

    /// The value of the grid at a point on the line between two samples.
    fn on_edge(grid: &[f32], width: usize, p: Point) -> Option<f64> {
        let (x, y) = (p.x - 0.5, p.y - 0.5);
        let sample = |i: f64, j: f64| {
            let (i, j) = (i as usize, j as usize);
            f64::from(grid[j * width + i])
        };
        if (x - x.round()).abs() < 1e-9 {
            let (j0, t) = (y.floor(), y - y.floor());
            Some(sample(x.round(), j0) * (1.0 - t) + sample(x.round(), j0 + 1.0) * t)
        } else if (y - y.round()).abs() < 1e-9 {
            let (i0, t) = (x.floor(), x - x.floor());
            Some(sample(i0, y.round()) * (1.0 - t) + sample(i0 + 1.0, y.round()) * t)
        } else {
            None
        }
    }

    #[test]
    fn a_circle_field_gives_one_ring_on_the_iso_line() {
        let (w, h) = (20, 20);
        let grid: Vec<f32> = (0..w * h)
            .map(|k| {
                let (x, y) = ((k % w) as f32 + 0.5, (k / w) as f32 + 0.5);
                ((x - 10.0).powi(2) + (y - 10.0).powi(2)).sqrt()
            })
            .collect();
        let rings = trace(&grid, w, h, f32::NEG_INFINITY, 5.0);
        assert_eq!(rings.len(), 1);
        let ring = &rings[0];
        assert!(area(ring) < 0.0, "an outer ring has a negative area");
        let r = std::f64::consts::PI * 25.0;
        assert!((area(ring).abs() - r).abs() < r * 0.05);
        for p in ring {
            let v = on_edge(&grid, w, *p).expect("a crossing is on an edge");
            assert!((v - 5.0).abs() < 1e-6, "{v} at {p:?}");
        }
    }

    #[test]
    fn a_hole_has_the_other_orientation() {
        let (w, h) = (20, 20);
        let grid: Vec<f32> = (0..w * h)
            .map(|k| {
                let (x, y) = ((k % w) as f32 + 0.5, (k / w) as f32 + 0.5);
                ((x - 10.0).powi(2) + (y - 10.0).powi(2)).sqrt()
            })
            .collect();
        let mut rings = trace(&grid, w, h, 3.0, f32::INFINITY);
        assert_eq!(rings.len(), 2);
        rings.sort_by(|a, b| area(a).abs().total_cmp(&area(b).abs()));
        assert!(area(&rings[0]) > 0.0, "the hole");
        assert!(area(&rings[1]) < 0.0, "the outer ring");
    }

    #[test]
    fn a_ring_closes_at_the_edge_of_the_grid() {
        let grid = [1.0; 6];
        let rings = trace(&grid, 3, 2, 0.0, f32::INFINITY);
        assert_eq!(rings.len(), 1);
        let ring = &rings[0];
        // The corners of the grid are cut by half a pixel.
        assert!((area(ring) + 5.5).abs() < 1e-9, "{}", area(ring));
        for p in ring {
            let on_border = p.x.abs() < 1e-9
                || (p.x - 3.0).abs() < 1e-9
                || p.y.abs() < 1e-9
                || (p.y - 2.0).abs() < 1e-9;
            assert!(on_border, "{p:?}");
        }
    }

    #[test]
    fn nan_is_outside() {
        let grid = [1.0, f32::NAN, 1.0];
        let rings = trace(&grid, 3, 1, 0.0, 2.0);
        assert_eq!(rings.len(), 2);
    }

    #[test]
    fn an_empty_range_gives_no_ring() {
        let grid = [1.0, 2.0, 3.0, 4.0];
        assert!(trace(&grid, 2, 2, 10.0, 20.0).is_empty());
        assert!(trace(&[], 0, 0, 0.0, 1.0).is_empty());
    }

    #[test]
    fn a_full_range_gives_the_grid_outline() {
        let grid = [1.0, 2.0, 3.0, 4.0];
        let rings = trace(&grid, 2, 2, f32::NEG_INFINITY, f32::INFINITY);
        assert_eq!(rings.len(), 1);
        assert!((area(&rings[0]) + 3.5).abs() < 1e-9);
    }

    #[test]
    fn the_mean_decides_a_saddle() {
        // Two samples inside on one diagonal.
        let joined = [1.0, 0.4, 0.4, 1.0];
        assert_eq!(trace(&joined, 2, 2, 0.5, f32::INFINITY).len(), 1);
        let apart = [1.0, 0.0, 0.0, 1.0];
        assert_eq!(trace(&apart, 2, 2, 0.6, f32::INFINITY).len(), 2);
    }

    #[test]
    fn a_band_crosses_at_both_limits() {
        let grid: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let rings = trace(&grid, 10, 1, 2.5, 6.5);
        assert_eq!(rings.len(), 1);
        let xs: Vec<f64> = rings[0].iter().map(|p| p.x).collect();
        let min = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let max = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!((min - 3.0).abs() < 1e-9, "{min}");
        assert!((max - 7.0).abs() < 1e-9, "{max}");
    }
}
