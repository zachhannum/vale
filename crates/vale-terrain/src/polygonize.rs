//! The outlines of the bands of a raster on the sphere.
//!
//! The texel centers of all six faces are the vertices of one mesh. A cell of
//! the mesh is a quad in a face, a quad across a face edge, or a triangle at
//! a cube corner. Marching squares on that mesh gives each limit as closed
//! rings. Two cells that share a mesh edge put a crossing on that edge at the
//! same point, so the rings have no gap at a face edge or a cube corner. The
//! rings of two limits do not cross, because the larger limit is inside the
//! smaller limit in each cell.
//!
//! The trace reads the whole face size. It filters and traces one face at a
//! time, so the memory holds the values of one face and of the texels at the
//! edges of the faces.

use std::collections::HashMap;
use std::f64::consts::FRAC_PI_2;
use std::ops::Range;

use crate::cube::{FACES, face_dir, unwarp};
use crate::heightmap::{Heightmap, TexelRect};
use crate::math::{V3, add, cross, dot, normalize, scale};

/// The rings of one band, each with the band on its left.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BandRings {
    pub rings: Vec<Vec<V3>>,
    /// The band covers the whole sphere. It then has no ring.
    pub whole: bool,
}

/// The parameters of `polygonize`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trace {
    /// The width of the Gaussian filter on the levels, in texels.
    pub sigma: f64,
    /// The largest distance of a removed vertex from the simpler ring, in
    /// texels.
    pub tolerance: f64,
}

/// The rings of each band of `map`. `limits` are the levels of the edges
/// between the bands, in ascending order, so band `i` goes from limit
/// `i - 1` to limit `i`. A texel at a limit is in the upper band.
pub fn polygonize(map: &Heightmap, limits: &[f64], trace: Trace) -> Vec<BandRings> {
    let n = map.face_size();
    let mesh = Mesh::new(map);
    let filter = Filter::new(trace.sigma, n);
    let threads = std::thread::available_parallelism().map_or(4, |t| t.get());
    let mut lines: Vec<Lines> = limits.iter().map(|_| Lines::default()).collect();
    let mut borders = Vec::with_capacity(FACES);
    let mut first = 0.0;
    for face in 0..FACES {
        let field = filter.face(map, face, threads);
        if face == 0 {
            first = field[0];
        }
        // The cells of the corners of the texel grid inside the face, in
        // bands of rows.
        let rows = 1..n;
        let step = rows.len().div_ceil(threads).max(1);
        let parts: Vec<Vec<Lines>> = std::thread::scope(|s| {
            let jobs: Vec<_> = (rows.step_by(step))
                .map(|j| {
                    let (mesh, field) = (&mesh, &field);
                    s.spawn(move || mesh.march_inside(face, field, j..(j + step).min(n), limits))
                })
                .collect();
            jobs.into_iter()
                .map(|j| j.join().expect("a trace"))
                .collect()
        });
        for part in parts {
            for (all, part) in lines.iter_mut().zip(part) {
                all.absorb(part);
            }
        }
        borders.push(Border::of(&field, n));
    }
    mesh.for_each_edge_cell(|ids| {
        let mut values = [0.0f32; 4];
        for (v, &id) in values.iter_mut().zip(ids) {
            *v = mesh.border_value(&borders, id);
        }
        mesh.march(&mut lines, ids, &values[..ids.len()], limits);
    });
    let tolerance = trace.tolerance * FRAC_PI_2 / n as f64;
    let rings: Vec<Vec<Vec<V3>>> = std::thread::scope(|s| {
        let jobs: Vec<_> = (lines.into_iter())
            .map(|l| s.spawn(move || l.rings(tolerance)))
            .collect();
        jobs.into_iter()
            .map(|j| j.join().expect("a ring"))
            .collect()
    });
    let band_of = |v: f32| limits.partition_point(|&t| t <= v as f64);
    let whole = band_of(first);
    (0..=limits.len())
        .map(|band| {
            let mut out = Vec::new();
            if band > 0 {
                out.extend(rings[band - 1].iter().cloned());
            }
            if band < limits.len() {
                out.extend(
                    rings[band]
                        .iter()
                        .map(|r| r.iter().rev().copied().collect()),
                );
            }
            BandRings {
                whole: out.is_empty() && band == whole,
                rings: out,
            }
        })
        .collect()
}

/// The values of the texels at the four edges of one face, after the
/// filter: the first row, the last row, the first column, and the last
/// column.
struct Border([Vec<f32>; 4]);

impl Border {
    fn of(field: &[f32], n: usize) -> Border {
        Border([
            field[..n].to_vec(),
            field[(n - 1) * n..].to_vec(),
            (0..n).map(|y| field[y * n]).collect(),
            (0..n).map(|y| field[y * n + n - 1]).collect(),
        ])
    }
}

/// The Gaussian filter on the levels.
struct Filter {
    kernel: Vec<f32>,
    reach: usize,
}

/// The number of rows that one job of the filter makes.
const FILTER_ROWS: usize = 256;

impl Filter {
    /// A filter of width `sigma` texels for a face size of `n`. A width of
    /// 0 keeps the levels.
    fn new(sigma: f64, n: usize) -> Filter {
        let reach = ((3.0 * sigma).ceil() as usize).min((n / 2).saturating_sub(1));
        let raw: Vec<f64> = (0..=2 * reach)
            .map(|i| {
                let x = i as f64 - reach as f64;
                match reach {
                    0 => 1.0,
                    _ => (-x * x / (2.0 * sigma * sigma)).exp(),
                }
            })
            .collect();
        let sum: f64 = raw.iter().sum();
        let kernel = raw.iter().map(|w| (w / sum) as f32).collect();
        Filter { kernel, reach }
    }

    /// The filtered levels of one face, row by row. The filter reads past
    /// each face edge.
    fn face(&self, map: &Heightmap, face: usize, threads: usize) -> Vec<f32> {
        let n = map.face_size();
        let mut out = vec![0.0f32; n * n];
        let mut jobs: Vec<Vec<(usize, &mut [f32])>> = (0..threads).map(|_| Vec::new()).collect();
        for (k, chunk) in out.chunks_mut(FILTER_ROWS * n).enumerate() {
            jobs[k % threads].push((k * FILTER_ROWS, chunk));
        }
        std::thread::scope(|s| {
            for job in jobs {
                s.spawn(move || {
                    for (y0, chunk) in job {
                        self.rows(map, face, y0, chunk);
                    }
                });
            }
        });
        out
    }

    /// The filtered levels of the rows from `y0`, as many as `out` holds.
    fn rows(&self, map: &Heightmap, face: usize, y0: usize, out: &mut [f32]) {
        let n = map.face_size();
        let r = self.reach;
        let count = out.len() / n;
        let wide = n + 2 * r;
        let mut line = vec![0.0f32; wide];
        let mut texels = Vec::with_capacity(n);
        // The rows of the margin first get the filter along the row.
        let mut across = vec![0.0f32; (count + 2 * r) * n];
        for (k, row) in across.chunks_mut(n).enumerate() {
            let fy = (y0 + k) as i64 - r as i64;
            if (0..n as i64).contains(&fy) {
                texels.clear();
                let y = fy as usize;
                let rect = TexelRect {
                    x0: 0,
                    y0: y,
                    x1: n,
                    y1: y + 1,
                };
                map.read_rect(face, rect, &mut texels);
                for (l, &t) in line[r..r + n].iter_mut().zip(&texels) {
                    *l = f32::from(t);
                }
                for i in 0..r {
                    line[i] = f32::from(map.get(face, i as i64 - r as i64, fy));
                    line[r + n + i] = f32::from(map.get(face, (n + i) as i64, fy));
                }
            } else {
                for (x, l) in line.iter_mut().enumerate() {
                    *l = f32::from(map.get(face, x as i64 - r as i64, fy));
                }
            }
            if r == 0 {
                row.copy_from_slice(&line);
                continue;
            }
            for (x, o) in row.iter_mut().enumerate() {
                let window = &line[x..x + self.kernel.len()];
                *o = window.iter().zip(&self.kernel).map(|(v, w)| v * w).sum();
            }
        }
        for (y, row) in out.chunks_mut(n).enumerate() {
            row.fill(0.0);
            for (i, w) in self.kernel.iter().enumerate() {
                let source = &across[(y + i) * n..(y + i + 1) * n];
                for (o, v) in row.iter_mut().zip(source) {
                    *o += v * w;
                }
            }
        }
    }
}

/// The mesh of the texel centers of all faces.
struct Mesh<'a> {
    map: &'a Heightmap,
    n: usize,
}

impl<'a> Mesh<'a> {
    fn new(map: &'a Heightmap) -> Mesh<'a> {
        Mesh {
            map,
            n: map.face_size(),
        }
    }

    fn id(&self, face: usize, x: usize, y: usize) -> u32 {
        ((face * self.n + y) * self.n + x) as u32
    }

    fn split(&self, id: u32) -> (usize, usize, usize) {
        let (id, n) = (id as usize, self.n);
        (id / (n * n), id % n, id / n % n)
    }

    fn dir(&self, id: u32) -> V3 {
        let (face, x, y) = self.split(id);
        self.map.texel_dir(face, x, y)
    }

    /// The filtered value of a texel at the edge of its face.
    fn border_value(&self, borders: &[Border], id: u32) -> f32 {
        let (face, x, y) = self.split(id);
        let b = &borders[face].0;
        let last = self.n - 1;
        match (x, y) {
            (_, 0) => b[0][x],
            (_, y) if y == last => b[1][x],
            (0, _) => b[2][y],
            _ => b[3][y],
        }
    }

    /// The texel at a place that can be past the face edge on one axis.
    fn past_edge(&self, face: usize, x: i64, y: i64) -> u32 {
        let n = self.n as i64;
        if (0..n).contains(&x) && (0..n).contains(&y) {
            return self.id(face, x as usize, y as usize);
        }
        let flat = |i: i64| unwarp((i as f64 + 0.5) / self.n as f64 * 2.0 - 1.0);
        let (f, x, y) = self.map.texel_of(face_dir(face, flat(x), flat(y)));
        self.id(f, x, y)
    }

    /// The segments of the cells of the corners `(i, j)` of the texel grid
    /// of a face, for `i` from 1 to `n - 1` and `j` in `rows`. All texels of
    /// these cells are in the face.
    fn march_inside(
        &self,
        face: usize,
        field: &[f32],
        rows: Range<usize>,
        limits: &[f64],
    ) -> Vec<Lines> {
        let n = self.n;
        let mut lines: Vec<Lines> = limits.iter().map(|_| Lines::default()).collect();
        // A face of a negative axis looks at its grid from behind.
        let flip = face % 2 == 1;
        for j in rows {
            for i in 1..n {
                let mut corners = [(i - 1, j - 1), (i, j - 1), (i, j), (i - 1, j)];
                if flip {
                    corners.reverse();
                }
                let values = corners.map(|(x, y)| field[y * n + x]);
                let ids = corners.map(|(x, y)| self.id(face, x, y));
                self.march(&mut lines, &ids, &values, limits);
            }
        }
        lines
    }

    /// Adds the segments of one cell to the lines of each limit that the
    /// values of the cell go over.
    fn march(&self, lines: &mut [Lines], ids: &[u32], values: &[f32], limits: &[f64]) {
        let lo = values.iter().copied().fold(f32::INFINITY, f32::min) as f64;
        let hi = values.iter().copied().fold(f32::NEG_INFINITY, f32::max) as f64;
        let first = limits.partition_point(|&t| t <= lo);
        let last = limits.partition_point(|&t| t <= hi);
        if first == last {
            return;
        }
        let mut dirs = [[0.0; 3]; 4];
        for (d, &id) in dirs.iter_mut().zip(ids) {
            *d = self.dir(id);
        }
        for t in first..last {
            lines[t].march(ids, values, &dirs[..ids.len()], limits[t]);
        }
    }

    /// Calls `f` with the texels of each cell of a corner of the texel grid
    /// on a face edge, counterclockwise seen from outside the sphere. A cell
    /// on a face edge has four texels, and a cell at a cube corner has
    /// three.
    fn for_each_edge_cell(&self, mut f: impl FnMut(&[u32])) {
        let n = self.n;
        let edge = (0..=n).flat_map(|j| {
            let inner = (1..n).filter(move |_| j == 0 || j == n);
            [0, n].into_iter().chain(inner).map(move |i| (i, j))
        });
        let corners: Vec<(usize, usize)> = edge.collect();
        for face in 0..FACES {
            for &(i, j) in &corners {
                let (i, j) = (i as i64, j as i64);
                let n = n as i64;
                let around = [(i - 1, j - 1), (i, j - 1), (i, j), (i - 1, j)];
                let mut ids: Vec<u32> = Vec::with_capacity(4);
                for (x, y) in around {
                    let out_x = !(0..n).contains(&x);
                    let out_y = !(0..n).contains(&y);
                    if out_x && out_y {
                        continue;
                    }
                    ids.push(self.past_edge(face, x, y));
                }
                // Each cell of a face edge belongs to the face with the least
                // index.
                let owner = ids.iter().map(|&id| self.split(id).0);
                if owner.min() != Some(face) {
                    continue;
                }
                if self.clockwise(&ids) {
                    ids.reverse();
                }
                f(&ids);
            }
        }
    }

    fn clockwise(&self, ids: &[u32]) -> bool {
        let dirs: Vec<V3> = ids.iter().map(|&id| self.dir(id)).collect();
        let mut normal = [0.0; 3];
        let mut center = [0.0; 3];
        for (k, &d) in dirs.iter().enumerate() {
            normal = add(normal, cross(d, dirs[(k + 1) % dirs.len()]));
            center = add(center, d);
        }
        dot(normal, center) < 0.0
    }
}

/// The segments of one limit, keyed by the mesh edge of each crossing.
#[derive(Default)]
struct Lines {
    crossings: HashMap<u64, u32>,
    points: Vec<V3>,
    next: Vec<u32>,
}

type Corner = (u32, f32, V3);

/// A crossing is never closer than this to a texel center, as a share of
/// the mesh edge. Two cells that meet at a texel at the limit then still
/// have two crossings.
const MIN_SHARE: f64 = 1e-3;

impl Lines {
    /// Adds the crossings and the segments of `other`. A crossing on the
    /// same mesh edge is the same crossing.
    fn absorb(&mut self, other: Lines) {
        let mut index = vec![0u32; other.points.len()];
        // In the order of `other`, so each run gives the same rings.
        let mut keys: Vec<(u32, u64)> = other.crossings.into_iter().map(|(k, i)| (i, k)).collect();
        keys.sort_unstable();
        for (i, key) in keys {
            index[i as usize] = *self.crossings.entry(key).or_insert_with(|| {
                self.points.push(other.points[i as usize]);
                self.next.push(u32::MAX);
                (self.points.len() - 1) as u32
            });
        }
        for (i, &to) in other.next.iter().enumerate() {
            if to != u32::MAX {
                self.next[index[i] as usize] = index[to as usize];
            }
        }
    }

    /// The crossing of `limit` on the mesh edge between two corners, each
    /// with its texel, its value, and its direction.
    fn crossing(&mut self, a: Corner, b: Corner, limit: f64) -> u32 {
        let key = (u64::from(a.0.min(b.0)) << 32) | u64::from(a.0.max(b.0));
        *self.crossings.entry(key).or_insert_with(|| {
            // The same point from both cells: interpolate from the lower id.
            let ((_, va, da), (_, vb, db)) = match a.0 < b.0 {
                true => (a, b),
                false => (b, a),
            };
            let share =
                ((limit - va as f64) / (vb as f64 - va as f64)).clamp(MIN_SHARE, 1.0 - MIN_SHARE);
            self.points
                .push(normalize(add(scale(da, 1.0 - share), scale(db, share))));
            self.next.push(u32::MAX);
            (self.points.len() - 1) as u32
        })
    }

    /// Adds the segments of a cell whose values go over `limit`. Each segment
    /// has the values at the limit or higher on its left.
    fn march(&mut self, ids: &[u32], values: &[f32], dirs: &[V3], limit: f64) {
        let k = ids.len();
        let up = |i: usize| values[i] as f64 >= limit;
        // The exits go from a high corner to a low corner, the entries from a
        // low corner to a high corner, counterclockwise.
        let mut exits = [0u32; 2];
        let mut entries = [0u32; 2];
        let (mut ne, mut nn) = (0, 0);
        let mut order = [(false, 0u32); 4];
        for i in 0..k {
            let j = (i + 1) % k;
            if up(i) == up(j) {
                continue;
            }
            let corner = |i: usize| (ids[i], values[i], dirs[i]);
            let c = self.crossing(corner(i), corner(j), limit);
            order[ne + nn] = (up(i), c);
            if up(i) {
                exits[ne] = c;
                ne += 1;
            } else {
                entries[nn] = c;
                nn += 1;
            }
        }
        match ne {
            1 => self.next[exits[0] as usize] = entries[0],
            2 => {
                // A saddle: the mean of the corners decides if the high
                // corners join through the middle of the cell.
                let mean = values.iter().map(|&v| v as f64).sum::<f64>() / k as f64;
                let joined = mean >= limit;
                for e in 0..4 {
                    let (is_exit, c) = order[e];
                    if !is_exit {
                        continue;
                    }
                    // Joined: the next entry counterclockwise. Apart: the
                    // entry before.
                    let to = match joined {
                        true => order[(e + 1) % 4].1,
                        false => order[(e + 3) % 4].1,
                    };
                    self.next[c as usize] = to;
                }
            }
            _ => {}
        }
    }

    /// The closed rings, with vertices that are less than `tolerance`
    /// radians from the simpler ring removed.
    fn rings(self, tolerance: f64) -> Vec<Vec<V3>> {
        let mut seen = vec![false; self.points.len()];
        let mut out = Vec::new();
        for start in 0..self.points.len() {
            if seen[start] {
                continue;
            }
            let mut ring = Vec::new();
            let mut at = start;
            while !seen[at] {
                seen[at] = true;
                ring.push(self.points[at]);
                at = self.next[at] as usize;
                if at == u32::MAX as usize {
                    break;
                }
            }
            debug_assert_eq!(at, start, "each crossing has a next crossing");
            let ring = simplify_ring(&ring, tolerance);
            if ring.len() >= 3 {
                out.push(ring);
            }
        }
        out
    }
}

/// The distance in radians from `p` to the great-circle arc from `a` to `b`.
fn arc_distance(p: V3, a: V3, b: V3) -> f64 {
    let normal = cross(a, b);
    let len = dot(normal, normal).sqrt();
    if len < 1e-15 {
        let d = add(p, scale(a, -1.0));
        return dot(d, d).sqrt();
    }
    let normal = scale(normal, 1.0 / len);
    let off = dot(p, normal);
    // The foot of `p` on the great circle must be between `a` and `b`.
    let foot = add(p, scale(normal, -off));
    if dot(cross(a, foot), normal) >= 0.0 && dot(cross(foot, b), normal) >= 0.0 {
        off.abs().asin()
    } else {
        let da = add(p, scale(a, -1.0));
        let db = add(p, scale(b, -1.0));
        dot(da, da).sqrt().min(dot(db, db).sqrt())
    }
}

/// A closed ring with fewer vertices, by Douglas and Peucker. The ring is
/// split at its first vertex and at the vertex farthest from it.
fn simplify_ring(ring: &[V3], tolerance: f64) -> Vec<V3> {
    if ring.len() < 4 || tolerance <= 0.0 {
        return ring.to_vec();
    }
    let far = (1..ring.len())
        .max_by(|&a, &b| {
            let d = |i: usize| -dot(ring[0], ring[i]);
            d(a).total_cmp(&d(b))
        })
        .unwrap_or(1);
    let mut keep = vec![false; ring.len()];
    keep[0] = true;
    keep[far] = true;
    let mut closed: Vec<V3> = ring.to_vec();
    closed.push(ring[0]);
    let mut stack = vec![(0, far), (far, ring.len())];
    while let Some((a, b)) = stack.pop() {
        let mut best = (0.0, 0);
        for i in a + 1..b {
            let d = arc_distance(closed[i], closed[a], closed[b]);
            if d > best.0 {
                best = (d, i);
            }
        }
        if best.0 > tolerance {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    (0..ring.len())
        .filter(|&i| keep[i])
        .map(|i| ring[i])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{angle, lonlat_to_dir};

    /// The signed area of a ring on the unit sphere, positive for a ring
    /// that goes counterclockwise around its inside.
    fn area(ring: &[V3]) -> f64 {
        let k = ring.len();
        let mut sum = 0.0;
        for i in 0..k {
            let (a, b) = (ring[i], ring[(i + 1) % k]);
            // The signed area of the triangle from the pole of the ring.
            let c = normalize(ring.iter().fold([0.0; 3], |s, &p| add(s, p)));
            let num = dot(c, cross(a, b));
            let den = 1.0 + dot(a, b) + dot(b, c) + dot(c, a);
            sum += 2.0 * num.atan2(den);
        }
        sum
    }

    /// A map of face size `n` with a level from `f` at each texel center.
    fn map_of(n: usize, f: impl Fn(V3) -> u16) -> Heightmap {
        let mut map = Heightmap::new(n, 0);
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    let d = map.texel_dir(face, x, y);
                    map.set(face, x, y, f(d));
                }
            }
        }
        map
    }

    const SHARP: Trace = Trace {
        sigma: 0.0,
        tolerance: 0.0,
    };

    #[test]
    fn a_cap_gives_one_ring_on_each_side() {
        let pole = lonlat_to_dir(20.0, 10.0);
        let map = map_of(32, |d| if angle(d, pole) < 0.5 { 1000 } else { 0 });
        let bands = polygonize(&map, &[500.0], SHARP);
        assert_eq!(bands.len(), 2);
        assert_eq!(bands[0].rings.len(), 1);
        assert_eq!(bands[1].rings.len(), 1);
        // The upper band is the cap, on the left of its ring.
        let cap = area(&bands[1].rings[0]);
        let expected = std::f64::consts::TAU * (1.0 - 0.5f64.cos());
        assert!((cap - expected).abs() < 0.05 * expected, "{cap} {expected}");
        assert!(area(&bands[0].rings[0]) < 0.0);
    }

    #[test]
    fn a_ring_over_a_cube_corner_and_three_face_edges_is_closed() {
        // A cube corner is at the direction (1, 1, 1).
        let corner = normalize([1.0, 1.0, 1.0]);
        let map = map_of(16, |d| if angle(d, corner) < 0.4 { 1000 } else { 0 });
        let bands = polygonize(&map, &[500.0], SHARP);
        assert_eq!(bands[1].rings.len(), 1);
        let ring = &bands[1].rings[0];
        for p in ring {
            assert!((angle(*p, corner) - 0.4).abs() < 0.15);
        }
        // No step of the ring is longer than a cell, so there is no gap.
        let step = FRAC_PI_2 / 16.0;
        for (i, &p) in ring.iter().enumerate() {
            assert!(angle(p, ring[(i + 1) % ring.len()]) < 2.0 * step);
        }
    }

    #[test]
    fn an_island_in_a_lake_on_an_island_gives_nested_rings() {
        let c = lonlat_to_dir(45.0, 45.0);
        let map = map_of(32, |d| match angle(d, c) {
            a if a < 0.15 => 1000,
            a if a < 0.3 => 0,
            a if a < 0.6 => 1000,
            _ => 0,
        });
        let bands = polygonize(&map, &[500.0], SHARP);
        // The coast, the shore of the lake, and the coast of the small
        // island are rings of both bands.
        assert_eq!(bands[0].rings.len(), 3);
        assert_eq!(bands[1].rings.len(), 3);
        let land: f64 = bands[1].rings.iter().map(|r| area(r)).sum();
        let ring = |a: f64| std::f64::consts::TAU * (1.0 - a.cos());
        let expected = ring(0.6) - ring(0.3) + ring(0.15);
        assert!(
            (land - expected).abs() < 0.1 * expected,
            "{land} {expected}"
        );
    }

    #[test]
    fn a_band_with_no_ring_is_whole_or_empty() {
        let map = map_of(8, |_| 700);
        let bands = polygonize(&map, &[500.0, 900.0], SHARP);
        let whole: Vec<bool> = bands.iter().map(|b| b.whole).collect();
        assert_eq!(whole, [false, true, false]);
    }

    #[test]
    fn a_texel_at_a_limit_is_in_the_upper_band() {
        let c = lonlat_to_dir(0.0, 0.0);
        let map = map_of(16, |d| if angle(d, c) < 0.3 { 500 } else { 0 });
        let bands = polygonize(&map, &[500.0], SHARP);
        assert_eq!(bands[1].rings.len(), 1);
    }

    #[test]
    fn the_bands_share_their_rings() {
        let c = lonlat_to_dir(-30.0, 20.0);
        let map = map_of(24, |d| (3000.0 * (1.0 - angle(d, c) / 3.2)) as u16);
        let trace = Trace {
            sigma: 1.0,
            tolerance: 0.2,
        };
        let bands = polygonize(&map, &[1000.0, 2000.0], trace);
        // The hole of band 1 is the outer ring of band 2, in reverse.
        let upper = &bands[2].rings[0];
        let hole: Vec<V3> = bands[1]
            .rings
            .last()
            .unwrap()
            .iter()
            .rev()
            .copied()
            .collect();
        assert_eq!(&hole, upper);
    }

    #[test]
    fn the_rings_of_a_busy_field_close_over_all_edges_and_corners() {
        let wave = |d: V3| {
            let s = (5.0 * d[0]).sin() + (7.0 * d[1]).cos() + (6.0 * d[2] + 1.0).sin();
            ((s + 3.0) * 1000.0) as u16
        };
        let map = map_of(16, wave);
        let limits = [1500.0, 2500.0, 3000.0, 3500.0, 4500.0];
        let bands = polygonize(&map, &limits, SHARP);
        let step = FRAC_PI_2 / 16.0;
        let mut count = 0;
        for band in &bands {
            for ring in &band.rings {
                count += 1;
                for (i, &p) in ring.iter().enumerate() {
                    assert!(angle(p, ring[(i + 1) % ring.len()]) < 2.0 * step);
                }
            }
        }
        assert!(count > 10);
    }

    #[test]
    #[ignore]
    fn manual_time_of_a_busy_world_of_2048_pixel_faces() {
        let wave = |d: V3| {
            let s = (40.0 * d[0]).sin() + (50.0 * d[1]).cos() + (45.0 * d[2] + 1.0).sin();
            ((s + 3.0) * 1000.0) as u16
        };
        let map = map_of(2048, wave);
        let limits: Vec<f64> = (1..12).map(|i| i as f64 * 500.0).collect();
        let trace = Trace {
            sigma: 1.2,
            tolerance: 0.1,
        };
        let start = std::time::Instant::now();
        let bands = polygonize(&map, &limits, trace);
        let points: usize = bands.iter().flat_map(|b| &b.rings).map(|r| r.len()).sum();
        eprintln!("{:?}, {points} points", start.elapsed());
    }

    #[test]
    fn a_snapshot_shares_its_tiles_until_a_change() {
        let mut map = map_of(8, |d| (d[0] * 100.0 + 200.0) as u16);
        let snapshot = map.snapshot();
        assert!(map.same_tiles(&snapshot));
        map.set(0, 1, 1, 7);
        assert!(!map.same_tiles(&snapshot));
    }

    #[test]
    #[ignore]
    fn manual_time_of_a_painted_world_of_8192_pixel_faces() {
        // Continents with coasts that have detail at each scale.
        let level = |d: V3| {
            let mut s = 0.0;
            for k in 0..9 {
                let f = 3.0 * 2f64.powi(k);
                let a = 1.0 / 2f64.powi(k);
                s += a
                    * ((f * d[0] + k as f64).sin() * (f * d[1] + 2.0 * k as f64).cos()
                        + (f * d[2] + 0.5 * k as f64).sin());
            }
            (32768.0 + s * 6000.0).clamp(0.0, 65535.0) as u16
        };
        let n = 8192;
        let mut map = Heightmap::new(n, 0);
        let start = std::time::Instant::now();
        for face in 0..FACES {
            let levels: Vec<u16> = (0..n * n)
                .map(|i| level(map.texel_dir(face, i % n, i / n)))
                .collect();
            map.store_face(face, &levels);
        }
        eprintln!("paint {:?}", start.elapsed());
        let limits: Vec<f64> = (1..12).map(|i| i as f64 * 5461.0).collect();
        let trace = Trace {
            sigma: 1.2,
            tolerance: 0.1,
        };
        let start = std::time::Instant::now();
        let bands = polygonize(&map, &limits, trace);
        let points: usize = bands.iter().flat_map(|b| &b.rings).map(|r| r.len()).sum();
        eprintln!("trace {:?}, {points} points", start.elapsed());
    }

    #[test]
    fn simplify_keeps_a_corner_and_drops_points_on_a_line() {
        let ring: Vec<V3> = [
            [0.0, 0.0],
            [5.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
        ]
        .map(|p| lonlat_to_dir(p[0], p[1]))
        .to_vec();
        let out = simplify_ring(&ring, 1e-3);
        assert_eq!(out.len(), 4);
    }
}
