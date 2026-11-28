//! The linear solve of the implicit step: `diag·x − c ∇²(w·x) = b` on the
//! unit sphere, by BiCGSTAB (van der Vorst 1992) with a Jacobi
//! preconditioner. The operator is not symmetric, because `w` varies and the
//! Laplacian has the correction for cells that are not in line.

use crate::grid::Grid;
use crate::ops::Stencil;

/// The largest number of iterations of one solve.
pub const MAX_ITERATIONS: usize = 200;
/// The residual, relative to `b`, where a solve stops.
pub const TOLERANCE: f64 = 1e-8;

/// The work buffers of the solver.
pub struct Solver {
    stencil: Stencil,
    precondition: Vec<f64>,
    r: Vec<f64>,
    r_hat: Vec<f64>,
    p: Vec<f64>,
    v: Vec<f64>,
    s: Vec<f64>,
    t: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
    weighted: Vec<f64>,
}

/// The f64 buffers of a solver for each cell, other than the stencil.
pub const F64_PER_CELL: usize = 10;

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

impl Solver {
    pub fn new(grid: &Grid) -> Solver {
        let n = grid.len();
        let zeros = || vec![0.0; n];
        Solver {
            stencil: Stencil::new(grid),
            precondition: zeros(),
            r: zeros(),
            r_hat: zeros(),
            p: zeros(),
            v: zeros(),
            s: zeros(),
            t: zeros(),
            y: zeros(),
            z: zeros(),
            weighted: zeros(),
        }
    }

    pub fn heap_bytes(&self) -> u64 {
        let f64s = [
            &self.precondition,
            &self.r,
            &self.r_hat,
            &self.p,
            &self.v,
            &self.s,
            &self.t,
            &self.y,
            &self.z,
            &self.weighted,
        ]
        .iter()
        .map(|v| v.capacity())
        .sum::<usize>();
        (f64s * 8) as u64 + self.stencil.heap_bytes()
    }

    /// The Laplacian of the grid.
    pub fn stencil(&self) -> &Stencil {
        &self.stencil
    }

    /// `out = diag·x − c ∇²(w·x)`.
    fn apply(
        stencil: &Stencil,
        diag: &[f64],
        c: f64,
        w: &[f64],
        x: &[f64],
        weighted: &mut [f64],
        out: &mut [f64],
    ) {
        for ((o, &wi), &xi) in weighted.iter_mut().zip(w).zip(x) {
            *o = wi * xi;
        }
        stencil.apply(weighted, out);
        for ((o, &d), &xi) in out.iter_mut().zip(diag).zip(x) {
            *o = d * xi - c * *o;
        }
    }

    /// Solves for `x`, from `x = 0`. Returns the number of iterations.
    pub fn solve(
        &mut self,
        grid: &Grid,
        diag: &[f64],
        c: f64,
        w: &[f64],
        b: &[f64],
        x: &mut [f64],
    ) -> usize {
        let n = grid.len();
        for i in 0..n {
            self.precondition[i] = 1.0 / (diag[i] - c * w[i] * self.stencil.diagonal(i));
        }
        x.fill(0.0);
        self.r.copy_from_slice(b);
        self.r_hat.copy_from_slice(b);
        self.p.fill(0.0);
        self.v.fill(0.0);
        let limit = TOLERANCE * dot(b, b).sqrt();
        if limit == 0.0 {
            return 0;
        }
        let (mut rho, mut alpha, mut omega) = (1.0, 1.0, 1.0);
        for iteration in 1..=MAX_ITERATIONS {
            let rho_new = dot(&self.r_hat, &self.r);
            let beta = rho_new / rho * (alpha / omega);
            rho = rho_new;
            for i in 0..n {
                self.p[i] = self.r[i] + beta * (self.p[i] - omega * self.v[i]);
                self.y[i] = self.precondition[i] * self.p[i];
            }
            Self::apply(
                &self.stencil,
                diag,
                c,
                w,
                &self.y,
                &mut self.weighted,
                &mut self.v,
            );
            alpha = rho / dot(&self.r_hat, &self.v);
            for i in 0..n {
                self.s[i] = self.r[i] - alpha * self.v[i];
            }
            if dot(&self.s, &self.s).sqrt() < limit {
                for (x, y) in x.iter_mut().zip(&self.y) {
                    *x += alpha * y;
                }
                return iteration;
            }
            for i in 0..n {
                self.z[i] = self.precondition[i] * self.s[i];
            }
            Self::apply(
                &self.stencil,
                diag,
                c,
                w,
                &self.z,
                &mut self.weighted,
                &mut self.t,
            );
            omega = dot(&self.t, &self.s) / dot(&self.t, &self.t);
            for (i, x) in x.iter_mut().enumerate() {
                *x += alpha * self.y[i] + omega * self.z[i];
                self.r[i] = self.s[i] - omega * self.t[i];
            }
            if dot(&self.r, &self.r).sqrt() < limit {
                return iteration;
            }
        }
        MAX_ITERATIONS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Vector;
    use crate::ops;

    /// The real spherical harmonic `Y₃²`, up to a constant, at a unit vector.
    fn y32([x, y, z]: Vector) -> f64 {
        z * (x * x - y * y)
    }

    #[test]
    fn an_implicit_step_damps_a_harmonic_at_its_rate() {
        // (q₁ − q₀)/dt = ∇²q₁, and ∇²Y₃² = −12 Y₃², so q₁ = q₀ / (1 + 12 dt).
        let grid = Grid::new(32);
        let mut solver = Solver::new(&grid);
        let n = grid.len();
        let dt = 0.05;
        let q0: Vec<f64> = (0..n).map(|i| y32(grid.center(i))).collect();
        let diag = vec![1.0 / dt; n];
        let w = vec![1.0; n];
        let mut g = vec![[0.0; 3]; n];
        let mut b = vec![0.0; n];
        ops::laplacian(&grid, &q0, None, &mut g, &mut b);
        let mut dq = vec![0.0; n];
        let iterations = solver.solve(&grid, &diag, 1.0, &w, &b, &mut dq);
        assert!(iterations < MAX_ITERATIONS);
        let factor = 1.0 / (1.0 + 12.0 * dt);
        let (mut num, mut den) = (0.0, 0.0);
        for i in 0..n {
            let q1 = q0[i] + dq[i];
            num += q1 * q0[i] * grid.area(i);
            den += q0[i] * q0[i] * grid.area(i);
        }
        let measured = num / den;
        assert!(
            (measured / factor - 1.0).abs() < 0.01,
            "{measured} {factor}"
        );
    }

    #[test]
    fn the_stencil_is_the_laplacian() {
        let grid = Grid::new(16);
        let n = grid.len();
        let q: Vec<f64> = (0..n)
            .map(|i| {
                let [x, y, z] = grid.center(i);
                (3.0 * x).sin() + y * z + z * z * z
            })
            .collect();
        let (mut a, mut b, mut g) = (vec![0.0; n], vec![0.0; n], vec![[0.0; 3]; n]);
        ops::laplacian(&grid, &q, None, &mut g, &mut a);
        Stencil::new(&grid).apply(&q, &mut b);
        for (a, b) in a.iter().zip(&b) {
            assert!((a - b).abs() < 1e-9 * a.abs().max(1.0), "{a} {b}");
        }
    }

    #[test]
    fn the_solve_meets_its_tolerance() {
        let grid = Grid::new(16);
        let mut solver = Solver::new(&grid);
        let n = grid.len();
        let diag: Vec<f64> = (0..n).map(|i| 1.0 + grid.center(i)[0].abs()).collect();
        let w: Vec<f64> = (0..n).map(|i| 2.0 + grid.center(i)[2]).collect();
        let b: Vec<f64> = (0..n).map(|i| grid.center(i)[1] + 0.3).collect();
        let mut x = vec![0.0; n];
        solver.solve(&grid, &diag, 0.5, &w, &b, &mut x);
        let mut ax = vec![0.0; n];
        let mut weighted = vec![0.0; n];
        Solver::apply(&solver.stencil, &diag, 0.5, &w, &x, &mut weighted, &mut ax);
        let err: f64 = ax
            .iter()
            .zip(&b)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!(err < 1e-7 * dot(&b, &b).sqrt(), "{err}");
    }
}
