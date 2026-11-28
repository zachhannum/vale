//! The cube-sphere grid of the model, on the unit sphere.
//!
//! The layout is the layout of the cube map of `vale-terrain`, so cell
//! `face * n * n + y * n + x` is texel `(face, x, y)` of a cube map of face
//! size `n`. Face `2 * axis` looks along the positive axis, and face
//! `2 * axis + 1` along the negative axis. On each face, `x` runs along axis
//! `(axis + 1) % 3` and `y` along axis `(axis + 2) % 3`. The spacing is equal
//! in angle.

use std::f64::consts::FRAC_PI_4;

/// A point or a tangent vector in 3D Cartesian coordinates.
pub type Vector = [f64; 3];

pub const FACES: usize = 6;

/// One of the four edges of a cell. The order of the edges is −x, +x, −y,
/// +y on the face of the cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    /// The cell on the other side.
    pub neighbor: u32,
    /// The index of the same edge in the edges of the neighbor.
    pub back: u8,
    /// The arc length of the edge.
    pub length: f64,
    /// The arc distance between the two cell centers.
    pub distance: f64,
    /// The outward unit normal at the midpoint of the edge, tangent to the
    /// sphere.
    pub normal: Vector,
    /// The unit tangent at the midpoint of the edge, toward the neighbor.
    pub tangent: Vector,
    /// The midpoint of the edge in the gnomonic plane of the cell center,
    /// as an offset from the center.
    pub mid_offset: Vector,
}

/// The cells of a cube sphere with `n` by `n` cells on each face.
#[derive(Clone, Debug)]
pub struct Grid {
    n: usize,
    centers: Vec<Vector>,
    areas: Vec<f64>,
    edges: Vec<[Edge; 4]>,
    lsq: Vec<[Vector; 4]>,
}

impl Grid {
    pub fn new(n: usize) -> Grid {
        assert!(n >= 2, "a face needs at least 2 by 2 cells");
        let len = FACES * n * n;
        let bound: Vec<f64> = (0..=n).map(|i| angle_to_flat(i as f64, n)).collect();
        let mid: Vec<f64> = (0..n).map(|i| angle_to_flat(i as f64 + 0.5, n)).collect();

        let mut centers = Vec::with_capacity(len);
        let mut areas = Vec::with_capacity(len);
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    centers.push(face_dir(face, mid[x], mid[y]));
                    let (a0, a1, b0, b1) = (bound[x], bound[x + 1], bound[y], bound[y + 1]);
                    let corner = |a: f64, b: f64| (a * b / (1.0 + a * a + b * b).sqrt()).atan();
                    areas.push(corner(a1, b1) - corner(a0, b1) - corner(a1, b0) + corner(a0, b0));
                }
            }
        }

        let mut grid = Grid {
            n,
            centers,
            areas,
            edges: Vec::with_capacity(len),
            lsq: Vec::with_capacity(len),
        };

        // The neighbor of each edge, from a point just outside its midpoint.
        let mut raw = Vec::with_capacity(len);
        for i in 0..len {
            let (face, x, y) = grid.face_xy(i);
            let c = grid.centers[i];
            let v = |a: f64, b: f64| face_dir(face, bound[a as usize], bound[b as usize]);
            let (x, y) = (x as f64, y as f64);
            let ends = [
                (v(x, y), v(x, y + 1.0)),
                (v(x + 1.0, y), v(x + 1.0, y + 1.0)),
                (v(x, y), v(x + 1.0, y)),
                (v(x, y + 1.0), v(x + 1.0, y + 1.0)),
            ];
            let cell: [(u32, Vector, Vector, f64); 4] = ends.map(|(p, q)| {
                let m = normalize(add(p, q));
                let mut normal = normalize(cross(p, q));
                if dot(normal, c) > 0.0 {
                    normal = scale(normal, -1.0);
                }
                let outside = normalize(add(m, scale(normal, 1e-7)));
                (grid.cell_of(outside) as u32, m, normal, dot(p, q).acos())
            });
            raw.push(cell);
        }

        for i in 0..len {
            let c = grid.centers[i];
            let edges = std::array::from_fn(|e| {
                let (j, m, normal, length) = raw[i][e];
                let back = raw[j as usize]
                    .iter()
                    .position(|r| r.0 as usize == i)
                    .expect("neighbors are mutual") as u8;
                let cj = grid.centers[j as usize];
                let chord = sub(cj, c);
                let tangent = normalize(sub(chord, scale(m, dot(chord, m))));
                Edge {
                    neighbor: j,
                    back,
                    length,
                    distance: dot(c, cj).clamp(-1.0, 1.0).acos(),
                    normal,
                    tangent,
                    mid_offset: gnomonic(c, m),
                }
            });
            grid.edges.push(edges);
        }

        for i in 0..len {
            let w = grid.lsq_weights(i);
            grid.lsq.push(w);
        }
        grid
    }

    /// The number of cells along the edge of a face.
    pub fn n(&self) -> usize {
        self.n
    }

    /// The number of cells.
    pub fn len(&self) -> usize {
        self.centers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.centers.is_empty()
    }

    pub fn index(&self, face: usize, x: usize, y: usize) -> usize {
        (face * self.n + y) * self.n + x
    }

    /// The face, the column, and the row of a cell.
    pub fn face_xy(&self, i: usize) -> (usize, usize, usize) {
        let n = self.n;
        (i / (n * n), i % n, i / n % n)
    }

    /// The unit vector of the center of a cell.
    pub fn center(&self, i: usize) -> Vector {
        self.centers[i]
    }

    /// The latitude and the longitude of the center of a cell, in radians.
    pub fn lat_lon(&self, i: usize) -> (f64, f64) {
        let [x, y, z] = self.centers[i];
        (z.clamp(-1.0, 1.0).asin(), y.atan2(x))
    }

    /// The solid angle of a cell, in steradians.
    pub fn area(&self, i: usize) -> f64 {
        self.areas[i]
    }

    pub fn edges(&self, i: usize) -> &[Edge; 4] {
        &self.edges[i]
    }

    /// The least-squares gradient weights of a cell, one for each edge
    /// neighbor: the gradient is `Σ w_k (q_k − q_i)`.
    pub fn lsq(&self, i: usize) -> &[Vector; 4] {
        &self.lsq[i]
    }

    /// The cell that holds a direction.
    pub fn cell_of(&self, d: Vector) -> usize {
        let (face, a, b) = face_of(d);
        let n = self.n;
        let index = |f: f64| {
            let s = f.atan() / FRAC_PI_4;
            (((s + 1.0) / 2.0 * n as f64).floor() as isize).clamp(0, n as isize - 1) as usize
        };
        self.index(face, index(a), index(b))
    }

    /// The gradient weights of a cell, with weights `1/r²` on the offsets of
    /// the neighbor centers in the gnomonic plane of the cell.
    fn lsq_weights(&self, i: usize) -> [Vector; 4] {
        let c = self.centers[i];
        let (e1, e2) = basis(c);
        let offsets = self.edges[i].map(|e| {
            let r = gnomonic(c, self.centers[e.neighbor as usize]);
            [dot(r, e1), dot(r, e2)]
        });
        let (mut m00, mut m01, mut m11) = (0.0, 0.0, 0.0);
        for [u, v] in offsets {
            let w = 1.0 / (u * u + v * v);
            m00 += w * u * u;
            m01 += w * u * v;
            m11 += w * v * v;
        }
        let det = m00 * m11 - m01 * m01;
        offsets.map(|[u, v]| {
            let w = 1.0 / (u * u + v * v);
            let a = w * (m11 * u - m01 * v) / det;
            let b = w * (m00 * v - m01 * u) / det;
            add(scale(e1, a), scale(e2, b))
        })
    }
}

/// The flat face coordinate of an equal-angle position, with 0 at the low
/// edge of the face and `n` at the high edge.
fn angle_to_flat(i: f64, n: usize) -> f64 {
    ((i / n as f64 * 2.0 - 1.0) * FRAC_PI_4).tan()
}

/// The face that a direction hits, and its flat coordinates on that face.
pub fn face_of(d: Vector) -> (usize, f64, f64) {
    let abs = d.map(f64::abs);
    let axis = if abs[0] >= abs[1] && abs[0] >= abs[2] {
        0
    } else if abs[1] >= abs[2] {
        1
    } else {
        2
    };
    let face = 2 * axis + usize::from(d[axis] < 0.0);
    (
        face,
        d[(axis + 1) % 3] / abs[axis],
        d[(axis + 2) % 3] / abs[axis],
    )
}

/// The unit vector of a point with flat coordinates on a face.
pub fn face_dir(face: usize, a: f64, b: f64) -> Vector {
    let axis = face / 2;
    let mut d = [0.0; 3];
    d[axis] = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
    d[(axis + 1) % 3] = a;
    d[(axis + 2) % 3] = b;
    normalize(d)
}

/// The point `p` in the plane that touches the sphere at `c`, as an offset
/// from `c`.
fn gnomonic(c: Vector, p: Vector) -> Vector {
    sub(scale(p, 1.0 / dot(p, c)), c)
}

/// Two unit vectors that span the tangent plane at `c`.
fn basis(c: Vector) -> (Vector, Vector) {
    let helper = if c[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let e1 = normalize(cross(helper, c));
    (e1, cross(c, e1))
}

pub fn dot(a: Vector, b: Vector) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn add(a: Vector, b: Vector) -> Vector {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: Vector, b: Vector) -> Vector {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: Vector, s: f64) -> Vector {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn cross(a: Vector, b: Vector) -> Vector {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn normalize(a: Vector) -> Vector {
    scale(a, 1.0 / dot(a, a).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn the_areas_add_up_to_the_sphere() {
        for n in [2, 7, 32] {
            let grid = Grid::new(n);
            let total: f64 = (0..grid.len()).map(|i| grid.area(i)).sum();
            assert!((total - 4.0 * PI).abs() < 1e-12, "{n}: {total}");
        }
    }

    #[test]
    fn neighbors_are_mutual_and_adjacent() {
        let grid = Grid::new(16);
        let h = PI / 2.0 / 16.0;
        for i in 0..grid.len() {
            for e in grid.edges(i) {
                let j = e.neighbor as usize;
                assert_ne!(i, j);
                let back = grid.edges(j)[e.back as usize];
                assert_eq!(back.neighbor as usize, i);
                assert!((back.length - e.length).abs() < 1e-12);
                assert!(dot(back.normal, e.normal) < -1.0 + 1e-9);
                assert!(
                    e.distance > 0.5 * h && e.distance < 1.5 * h,
                    "{}",
                    e.distance
                );
            }
        }
    }

    #[test]
    fn cells_match_the_texels_of_the_cube_map() {
        let grid = Grid::new(8);
        // Face +X, column 0, row 0 is the cell at the lowest y and z.
        let c = grid.center(grid.index(0, 0, 0));
        assert!(c[0] > 0.0 && c[1] < 0.0 && c[2] < 0.0);
        // Face +Z holds the north pole.
        assert_eq!(grid.face_xy(grid.cell_of([0.0, 0.0, 1.0])).0, 4);
        for i in 0..grid.len() {
            assert_eq!(grid.cell_of(grid.center(i)), i);
        }
    }

    #[test]
    fn edge_normals_and_tangents_are_tangent_and_outward() {
        let grid = Grid::new(8);
        for i in 0..grid.len() {
            let c = grid.center(i);
            for e in grid.edges(i) {
                assert!(dot(e.normal, e.mid_offset) > 0.0);
                assert!(dot(e.tangent, e.normal) > 0.9);
                assert!(dot(e.mid_offset, c).abs() < 1e-12);
            }
        }
    }
}
