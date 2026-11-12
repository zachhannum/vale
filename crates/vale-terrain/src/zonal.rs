//! Values of a heightmap under a feature: the value between texels, and the
//! mean, the lowest, and the highest value under a polygon or a line.
//!
//! The values are levels. Points are longitude and latitude in degrees, and
//! the edges between points are great-circle arcs.

use std::f64::consts::FRAC_PI_2;

use crate::cube::{FACES, face_dir, face_of, unwarp, warp};
use crate::heightmap::{Heightmap, TexelRect};
use crate::math::{V3, add, angle, dot, lonlat_to_dir, normalize, scale};

/// The values of a heightmap under a feature.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zonal {
    /// The mean level, weighted by area for a polygon and by length for a
    /// line.
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    /// The area in steradians for a polygon, or the length in radians for a
    /// line.
    pub weight: f64,
    /// The texels that the values come from, on each face.
    pub rects: [Option<TexelRect>; FACES],
}

impl Heightmap {
    /// The level at a direction, from the four nearest texel centers.
    pub fn sample_bilinear(&self, d: V3) -> f64 {
        let (face, a, b) = face_of(d);
        let (fx, fy) = (self.texel_coord(a), self.texel_coord(b));
        bilinear(self, face, fx, fy, None)
    }

    /// The texel coordinate of a flat face coordinate, with texel centers at
    /// whole numbers.
    fn texel_coord(&self, a: f64) -> f64 {
        (warp(a) + 1.0) * 0.5 * self.face_size() as f64 - 0.5
    }

    /// The tiles that are not the same as in `old`, as the face and the
    /// texels of each tile. A write after `snapshot` copies the tile, so a
    /// tile that a snapshot shares has the same memory until a change.
    /// `None`: the size or the base level is different, so all texels can
    /// be different.
    pub fn changed_tiles(&self, old: &Heightmap) -> Option<Vec<(usize, TexelRect)>> {
        let (n, tile) = (self.face_size(), self.tile_size());
        if n != old.face_size() || tile != old.tile_size() || self.base() != old.base() {
            return None;
        }
        let per_side = n.div_ceil(tile);
        let mut changed = Vec::new();
        for index in 0..FACES * per_side * per_side {
            let same = match (self.tile(index), old.tile(index)) {
                (None, None) => true,
                (Some(a), Some(b)) => std::ptr::eq(a.as_ptr(), b.as_ptr()),
                _ => false,
            };
            if !same {
                let (face, column, row) = self.tile_key(index);
                changed.push((
                    face,
                    TexelRect {
                        x0: column * tile,
                        y0: row * tile,
                        x1: ((column + 1) * tile).min(n),
                        y1: ((row + 1) * tile).min(n),
                    },
                ));
            }
        }
        Some(changed)
    }
}

/// True if a rectangle of `rects` overlaps a rectangle of `changed` on the
/// same face.
pub fn rects_meet(rects: &[Option<TexelRect>; FACES], changed: &[(usize, TexelRect)]) -> bool {
    changed.iter().any(|&(face, c)| {
        rects[face].is_some_and(|r| r.x0 < c.x1 && c.x0 < r.x1 && r.y0 < c.y1 && c.y0 < r.y1)
    })
}

/// Adds a rectangle to the rectangle of a face.
fn grow(rects: &mut [Option<TexelRect>; FACES], face: usize, rect: TexelRect) {
    rects[face] = Some(match rects[face] {
        Some(r) => r.union(rect),
        None => rect,
    });
}

/// The bilinear level at texel coordinates on a face. A texel past the face
/// edge comes from the face that is there.
fn bilinear(
    map: &Heightmap,
    face: usize,
    fx: f64,
    fy: f64,
    mut rects: Option<&mut [Option<TexelRect>; FACES]>,
) -> f64 {
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let n = map.face_size() as i64;
    let mut read = |x: i64, y: i64| {
        if let Some(rects) = rects.as_deref_mut() {
            let (f, x, y) = if (0..n).contains(&x) && (0..n).contains(&y) {
                (face, x as usize, y as usize)
            } else {
                let flat =
                    |i: i64| unwarp(((i as f64 + 0.5) / n as f64 * 2.0 - 1.0).clamp(-1.99, 1.99));
                map.texel_of(face_dir(face, flat(x), flat(y)))
            };
            grow(
                rects,
                f,
                TexelRect {
                    x0: x,
                    y0: y,
                    x1: x + 1,
                    y1: y + 1,
                },
            );
        }
        f64::from(map.get(face, x, y))
    };
    let top = read(x0, y0) * (1.0 - tx) + read(x0 + 1, y0) * tx;
    let bottom = read(x0, y0 + 1) * (1.0 - tx) + read(x0 + 1, y0 + 1) * tx;
    top * (1.0 - ty) + bottom * ty
}

/// The sums of a zonal statistic.
struct Sums {
    sum: f64,
    weight: f64,
    min: f64,
    max: f64,
    count: usize,
    rects: [Option<TexelRect>; FACES],
}

impl Sums {
    fn new() -> Sums {
        Sums {
            sum: 0.0,
            weight: 0.0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            count: 0,
            rects: [None; FACES],
        }
    }

    fn add(&mut self, value: f64, weight: f64) {
        self.sum += value * weight;
        self.weight += weight;
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        self.count += 1;
    }

    fn finish(self) -> Option<Zonal> {
        if self.count == 0 {
            return None;
        }
        let mean = if self.weight > 0.0 {
            self.sum / self.weight
        } else {
            (self.min + self.max) * 0.5
        };
        Some(Zonal {
            mean,
            min: self.min,
            max: self.max,
            weight: self.weight,
            rects: self.rects,
        })
    }
}

/// The directions of a ring, with no repeated points and no closing point.
fn ring_dirs(ring: &[[f64; 2]]) -> Vec<V3> {
    let mut dirs: Vec<V3> = Vec::with_capacity(ring.len());
    for p in ring {
        let d = lonlat_to_dir(p[0], p[1]);
        if dirs.last().is_none_or(|&last| angle(last, d) > 1e-12) {
            dirs.push(d);
        }
    }
    while dirs.len() > 1 && angle(dirs[0], dirs[dirs.len() - 1]) <= 1e-12 {
        dirs.pop();
    }
    dirs
}

/// The part of a ring on the positive side of a plane through the center of
/// the sphere. A chord between two points projects to the great-circle arc
/// between them, so the cut of a chord is the cut of the arc. The points
/// are not unit vectors.
fn clip_ring(ring: &[V3], normal: V3) -> Vec<V3> {
    let mut out = Vec::with_capacity(ring.len() + 2);
    for i in 0..ring.len() {
        let (prev, cur) = (ring[(i + ring.len() - 1) % ring.len()], ring[i]);
        let (dp, dc) = (dot(normal, prev), dot(normal, cur));
        if (dp >= 0.0) != (dc >= 0.0) {
            let t = dp / (dp - dc);
            out.push(add(prev, scale(add(cur, scale(prev, -1.0)), t)));
        }
        if dc >= 0.0 {
            out.push(cur);
        }
    }
    out
}

/// The mean, the lowest, and the highest level of the texels whose centers
/// are inside a polygon. The rings are longitude and latitude in degrees,
/// and a point is inside if it is inside an odd number of rings, so holes
/// work. `None`: no texel center is inside.
pub fn polygon_zonal(map: &Heightmap, rings: &[Vec<[f64; 2]>]) -> Option<Zonal> {
    let rings: Vec<Vec<V3>> = rings
        .iter()
        .map(|ring| ring_dirs(ring))
        .filter(|ring| ring.len() >= 3)
        .collect();
    if rings.is_empty() {
        return None;
    }
    let n = map.face_size();
    let centers: Vec<f64> = (0..n)
        .map(|i| unwarp((i as f64 + 0.5) / n as f64 * 2.0 - 1.0))
        .collect();
    let mut sums = Sums::new();
    let mut edges = Vec::new();
    let mut levels = Vec::new();
    for face in 0..FACES {
        let axis = face / 2;
        let sign = if face % 2 == 0 { 1.0 } else { -1.0 };
        let (ua, va) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut planes = [[0.0; 3]; 4];
        for (k, plane) in planes.iter_mut().enumerate() {
            plane[axis] = sign;
            plane[if k < 2 { ua } else { va }] = if k % 2 == 0 { 1.0 } else { -1.0 };
        }
        edges.clear();
        for ring in &rings {
            let mut part = ring.clone();
            for &plane in &planes {
                if part.is_empty() {
                    break;
                }
                part = clip_ring(&part, plane);
            }
            let flat: Vec<[f64; 2]> = part
                .iter()
                .map(|p| {
                    let k = sign / p[axis];
                    [p[ua] * k, p[va] * k]
                })
                .collect();
            for i in 0..flat.len() {
                let (a, b) = (flat[i], flat[(i + 1) % flat.len()]);
                if a[1] != b[1] {
                    edges.push((a, b));
                }
            }
        }
        if edges.is_empty() {
            continue;
        }
        fill_face(map, face, &centers, &mut edges, &mut levels, &mut sums);
    }
    sums.finish()
}

/// Adds the texels of one face whose centers are inside the edges, by rows.
fn fill_face(
    map: &Heightmap,
    face: usize,
    centers: &[f64],
    edges: &mut [([f64; 2], [f64; 2])],
    levels: &mut Vec<u16>,
    sums: &mut Sums,
) {
    let n = centers.len();
    // The first row and the row after the last that an edge can cross.
    let row = |v: f64| (warp(v) + 1.0) * 0.5 * n as f64 - 0.5;
    let span = |e: &([f64; 2], [f64; 2])| {
        let (lo, hi) = (e.0[1].min(e.1[1]), e.0[1].max(e.1[1]));
        let start = row(lo).floor().max(0.0) as usize;
        let end = ((row(hi).ceil() + 1.0).max(0.0) as usize).min(n);
        (start, end)
    };
    edges.sort_by_key(|e| span(e).0);
    let mut next = 0;
    let mut active: Vec<(usize, usize)> = Vec::new();
    let mut crossings: Vec<f64> = Vec::new();
    let first = span(&edges[0]).0;
    for y in first..n {
        while next < edges.len() && span(&edges[next]).0 <= y {
            active.push((next, span(&edges[next]).1));
            next += 1;
        }
        active.retain(|&(_, end)| end > y);
        if active.is_empty() {
            if next == edges.len() {
                break;
            }
            continue;
        }
        let vc = centers[y];
        crossings.clear();
        for &(i, _) in &active {
            let (a, b) = edges[i];
            if (a[1] <= vc) != (b[1] <= vc) {
                crossings.push(a[0] + (vc - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
            }
        }
        crossings.sort_by(f64::total_cmp);
        for pair in crossings.as_chunks::<2>().0 {
            let (x0, x1) = (first_after(centers, pair[0]), first_after(centers, pair[1]));
            if x0 >= x1 {
                continue;
            }
            let rect = TexelRect {
                x0,
                y0: y,
                x1,
                y1: y + 1,
            };
            levels.clear();
            map.read_rect(face, rect, levels);
            for (x, &level) in (x0..x1).zip(levels.iter()) {
                sums.add(f64::from(level), map.texel_solid_angle(x, y));
            }
            grow(&mut sums.rects, face, rect);
        }
    }
}

/// The first texel whose center is past the flat coordinate `u`.
fn first_after(centers: &[f64], u: f64) -> usize {
    let n = centers.len();
    let guess = ((warp(u.clamp(-1.0, 1.0)) + 1.0) * 0.5 * n as f64 - 0.5).floor() + 1.0;
    let mut x = (guess.max(0.0) as usize).min(n);
    while x > 0 && centers[x - 1] > u {
        x -= 1;
    }
    while x < n && centers[x] <= u {
        x += 1;
    }
    x
}

/// The mean, the lowest, and the highest level along a line, from bilinear
/// samples about one texel apart. The mean is weighted by length. The points
/// are longitude and latitude in degrees. `None`: the line has no points.
pub fn line_zonal(map: &Heightmap, line: &[[f64; 2]]) -> Option<Zonal> {
    let dirs: Vec<V3> = line.iter().map(|p| lonlat_to_dir(p[0], p[1])).collect();
    let first = *dirs.first()?;
    let step = FRAC_PI_2 / map.face_size() as f64;
    let mut sums = Sums::new();
    let sample = |sums: &mut Sums, d: V3, weight: f64| {
        let (face, a, b) = face_of(d);
        let (fx, fy) = (map.texel_coord(a), map.texel_coord(b));
        let value = bilinear(map, face, fx, fy, Some(&mut sums.rects));
        sums.add(value, weight);
    };
    if dirs.len() == 1 || dirs.windows(2).all(|w| angle(w[0], w[1]) == 0.0) {
        sample(&mut sums, first, 0.0);
        return sums.finish();
    }
    for w in dirs.windows(2) {
        let (p, q) = (w[0], w[1]);
        let theta = angle(p, q);
        if theta == 0.0 {
            continue;
        }
        let k = (theta / step).ceil().max(1.0) as usize;
        let piece = theta / k as f64;
        // Trapezoids: each end sample of an arc has half the weight.
        for i in 0..=k {
            let t = i as f64 / k as f64;
            let d = normalize(add(
                scale(p, ((1.0 - t) * theta).sin()),
                scale(q, (t * theta).sin()),
            ));
            let weight = if i == 0 || i == k { piece * 0.5 } else { piece };
            sample(&mut sums, d, weight);
        }
    }
    sums.finish()
}
