//! The finite-volume operators of the grid, on the unit sphere. On a sphere
//! of radius `R`, divide the gradient, the divergence, and the advection by
//! `R`, and the Laplacian by `R²`.
//!
//! Neighbor cells across a face edge are not centered on the edge, so a
//! value at an edge is the mean of the two linear reconstructions from the
//! cells on its sides, not the mean of the two cell values.

use crate::grid::{Grid, Vector, add, dot, scale, sub};

/// The operators of one grid, with scratch space.
pub struct Ops<'g> {
    grid: &'g Grid,
    gradient: Vec<Vector>,
    components: [Vec<Vector>; 3],
    limiter: Vec<f64>,
}

impl<'g> Ops<'g> {
    pub fn new(grid: &'g Grid) -> Ops<'g> {
        let n = grid.len();
        Ops {
            grid,
            gradient: vec![[0.0; 3]; n],
            components: std::array::from_fn(|_| vec![[0.0; 3]; n]),
            limiter: vec![1.0; n],
        }
    }

    pub fn grid(&self) -> &'g Grid {
        self.grid
    }

    /// The gradient of `q` at each cell, as a tangent vector.
    pub fn gradient(&mut self, q: &[f64], out: &mut [Vector]) {
        gradient(self.grid, q, out);
    }

    /// The divergence of the tangent field `u`.
    pub fn divergence(&mut self, u: &[Vector], out: &mut [f64]) {
        let grid = self.grid;
        let mut component = vec![0.0; grid.len()];
        for (c, grads) in self.components.iter_mut().enumerate() {
            for (value, v) in component.iter_mut().zip(u) {
                *value = v[c];
            }
            gradient(grid, &component, grads);
        }
        for (i, out) in out.iter_mut().enumerate() {
            let mut sum = 0.0;
            for e in grid.edges(i) {
                let u_e = self.edge_vector(u, i, e.neighbor as usize, e.back as usize, e);
                sum += e.length * dot(u_e, e.normal);
            }
            *out = sum / grid.area(i);
        }
    }

    /// `∇·(k ∇q)`, with `k = 1` if it is `None`. The flux through an edge
    /// is the mean gradient, with its part along the line between the two
    /// centers replaced by the difference of the two values.
    pub fn laplacian(&mut self, q: &[f64], k: Option<&[f64]>, out: &mut [f64]) {
        let grid = self.grid;
        gradient(grid, q, &mut self.gradient);
        let g = &self.gradient;
        for (i, out) in out.iter_mut().enumerate() {
            let mut sum = 0.0;
            for e in grid.edges(i) {
                let j = e.neighbor as usize;
                let mean = scale(add(g[i], g[j]), 0.5);
                let along = (q[j] - q[i]) / e.distance - dot(mean, e.tangent);
                let g_e = add(mean, scale(e.tangent, along));
                let k_e = k.map_or(1.0, |k| 0.5 * (k[i] + k[j]));
                sum += e.length * k_e * dot(g_e, e.normal);
            }
            *out = sum / grid.area(i);
        }
    }

    /// `−∇·(u q)`, with second-order upwind values at the edges. With
    /// `limit`, the Barth–Jespersen limiter keeps each edge value within the
    /// range of the cell and its neighbors.
    pub fn advection(&mut self, q: &[f64], u: &[Vector], limit: bool, out: &mut [f64]) {
        let grid = self.grid;
        let n = grid.len();
        let mut component = vec![0.0; n];
        for (c, grads) in self.components.iter_mut().enumerate() {
            for (value, v) in component.iter_mut().zip(u) {
                *value = v[c];
            }
            gradient(grid, &component, grads);
        }
        gradient(grid, q, &mut self.gradient);
        if limit {
            for i in 0..n {
                self.limiter[i] = barth_jespersen(grid, q, &self.gradient, i);
            }
        } else {
            self.limiter.fill(1.0);
        }
        for (i, out) in out.iter_mut().enumerate() {
            let mut sum = 0.0;
            for e in grid.edges(i) {
                let j = e.neighbor as usize;
                let speed = dot(self.edge_vector(u, i, j, e.back as usize, e), e.normal);
                let value = if speed >= 0.0 {
                    q[i] + self.limiter[i] * dot(self.gradient[i], e.mid_offset)
                } else {
                    let back = &grid.edges(j)[e.back as usize];
                    q[j] + self.limiter[j] * dot(self.gradient[j], back.mid_offset)
                };
                sum += e.length * speed * value;
            }
            *out = -sum / grid.area(i);
        }
    }

    /// The vector field `u` at the midpoint of an edge, from the
    /// reconstructions of its components in the two cells.
    fn edge_vector(
        &self,
        u: &[Vector],
        i: usize,
        j: usize,
        back: usize,
        e: &crate::Edge,
    ) -> Vector {
        let r_j = self.grid.edges(j)[back].mid_offset;
        let c = &self.components;
        let at = |cell: usize, r: Vector| -> Vector {
            std::array::from_fn(|k| u[cell][k] + dot(c[k][cell], r))
        };
        scale(add(at(i, e.mid_offset), at(j, r_j)), 0.5)
    }
}

/// The least-squares gradient of `q`.
fn gradient(grid: &Grid, q: &[f64], out: &mut [Vector]) {
    for (i, out) in out.iter_mut().enumerate() {
        let mut g = [0.0; 3];
        for (e, w) in grid.edges(i).iter().zip(grid.lsq(i)) {
            g = add(g, scale(*w, q[e.neighbor as usize] - q[i]));
        }
        *out = g;
    }
}

/// The Barth–Jespersen factor of a cell, from 0 for a constant value to 1
/// for the full gradient.
fn barth_jespersen(grid: &Grid, q: &[f64], g: &[Vector], i: usize) -> f64 {
    let edges = grid.edges(i);
    let (mut lo, mut hi) = (q[i], q[i]);
    for e in edges {
        lo = lo.min(q[e.neighbor as usize]);
        hi = hi.max(q[e.neighbor as usize]);
    }
    let mut phi: f64 = 1.0;
    for e in edges {
        let delta = dot(g[i], e.mid_offset);
        if delta > 1e-300 {
            phi = phi.min((hi - q[i]) / delta);
        } else if delta < -1e-300 {
            phi = phi.min((lo - q[i]) / delta);
        }
    }
    phi.max(0.0)
}

/// The tangent part of a vector at the unit vector `c`.
pub fn tangent_part(v: Vector, c: Vector) -> Vector {
    sub(v, scale(c, dot(v, c)))
}
