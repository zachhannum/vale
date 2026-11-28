//! The grid operators against analytic fields on the unit sphere, with
//! separate checks of the cells at the face edges and at the cube corners.

use isotherm_sim::grid::{cross, dot, normalize, scale, sub};
use isotherm_sim::{Grid, Ops, Vector};

/// Where a cell is on its face.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Place {
    Interior,
    Edge,
    Corner,
}

fn place(grid: &Grid, i: usize) -> Place {
    let (_, x, y) = grid.face_xy(i);
    let last = grid.n() - 1;
    let on_x = x == 0 || x == last;
    let on_y = y == 0 || y == last;
    match (on_x, on_y) {
        (true, true) => Place::Corner,
        (false, false) => Place::Interior,
        _ => Place::Edge,
    }
}

/// The errors of a scalar result against the exact values.
#[derive(Debug)]
struct Errors {
    /// The area-weighted RMS error over the RMS of the exact values.
    l2: f64,
    /// The largest error in each place, over the largest exact value.
    interior: f64,
    edge: f64,
    corner: f64,
}

fn errors(grid: &Grid, got: &[f64], exact: &[f64]) -> Errors {
    let scale = exact
        .iter()
        .fold(0.0_f64, |m, v| m.max(v.abs()))
        .max(1e-300);
    let (mut num, mut den) = (0.0, 0.0);
    let mut max = [0.0_f64; 3];
    for i in 0..grid.len() {
        let d = got[i] - exact[i];
        num += grid.area(i) * d * d;
        den += grid.area(i) * exact[i] * exact[i];
        let slot = place(grid, i) as usize;
        max[slot] = max[slot].max(d.abs() / scale);
    }
    Errors {
        l2: (num / den.max(1e-300)).sqrt(),
        interior: max[0],
        edge: max[1],
        corner: max[2],
    }
}

/// The largest absolute value in each place.
fn largest(grid: &Grid, got: &[f64]) -> [f64; 3] {
    let mut max = [0.0_f64; 3];
    for (i, v) in got.iter().enumerate() {
        let slot = place(grid, i) as usize;
        max[slot] = max[slot].max(v.abs());
    }
    max
}

fn field(grid: &Grid, f: impl Fn(Vector) -> f64) -> Vec<f64> {
    (0..grid.len()).map(|i| f(grid.center(i))).collect()
}

fn vectors(grid: &Grid, f: impl Fn(Vector) -> Vector) -> Vec<Vector> {
    (0..grid.len()).map(|i| f(grid.center(i))).collect()
}

fn total(grid: &Grid, values: &[f64]) -> f64 {
    values
        .iter()
        .enumerate()
        .map(|(i, v)| grid.area(i) * v)
        .sum()
}

fn y20(c: Vector) -> f64 {
    3.0 * c[2] * c[2] - 1.0
}

fn y32(c: Vector) -> f64 {
    (c[0] * c[0] - c[1] * c[1]) * c[2]
}

/// The rotation about an axis through two opposite cube corners.
fn rotation(c: Vector) -> Vector {
    cross(normalize([1.0, 1.0, 1.0]), c)
}

fn grad_z(c: Vector) -> Vector {
    sub([0.0, 0.0, 1.0], scale(c, c[2]))
}

fn gradient_error(n: usize) -> (f64, f64) {
    let grid = Grid::new(n);
    let mut ops = Ops::new(&grid);
    let mut g = vec![[0.0; 3]; grid.len()];
    ops.gradient(&field(&grid, |c| c[2]), &mut g);
    let (mut num, mut den, mut max) = (0.0, 0.0, 0.0_f64);
    for (i, g) in g.iter().enumerate() {
        let d = sub(*g, grad_z(grid.center(i)));
        num += grid.area(i) * dot(d, d);
        den += grid.area(i);
        max = max.max(dot(d, d).sqrt());
    }
    ((num / den).sqrt(), max)
}

#[test]
fn gradient_of_z_converges() {
    let (l2_32, max_32) = gradient_error(32);
    let (l2_64, max_64) = gradient_error(64);
    println!("gradient z: L2 {l2_32:.2e} {l2_64:.2e}, max {max_32:.2e} {max_64:.2e}");
    assert!(l2_32 < 3e-3, "{l2_32}");
    assert!(l2_32 / l2_64 > 2.5, "{l2_32} {l2_64}");
    assert!(max_32 < 6e-3 && max_64 < max_32, "{max_32} {max_64}");
}

#[test]
fn gradient_is_tangent() {
    let grid = Grid::new(16);
    let mut ops = Ops::new(&grid);
    let mut g = vec![[0.0; 3]; grid.len()];
    ops.gradient(&field(&grid, y32), &mut g);
    for (i, g) in g.iter().enumerate() {
        assert!(dot(*g, grid.center(i)).abs() < 1e-12);
    }
}

fn divergence_errors(n: usize) -> (Errors, [f64; 3]) {
    let grid = Grid::new(n);
    let mut ops = Ops::new(&grid);
    let mut out = vec![0.0; grid.len()];
    ops.divergence(&vectors(&grid, grad_z), &mut out);
    let grad = errors(&grid, &out, &field(&grid, |c| -2.0 * c[2]));
    ops.divergence(&vectors(&grid, rotation), &mut out);
    (grad, largest(&grid, &out))
}

#[test]
fn divergence_of_the_gradient_of_z_is_minus_two_z() {
    let (e32, _) = divergence_errors(32);
    let (e64, _) = divergence_errors(64);
    println!("div grad z: {e32:?} {e64:?}");
    assert!(e32.l2 < 3e-3 && e64.l2 < e32.l2, "{e32:?} {e64:?}");
    for (a, b) in [(e32.interior, e64.interior), (e32.edge, e64.edge)] {
        assert!(a < 1e-2 && b < a, "{e32:?} {e64:?}");
    }
    assert!(
        e32.corner < 1e-2 && e64.corner < e32.corner,
        "{e32:?} {e64:?}"
    );
}

#[test]
fn a_rotation_through_the_corners_has_no_divergence() {
    let (_, r32) = divergence_errors(32);
    let (_, r64) = divergence_errors(64);
    println!("div rotation: {r32:?} {r64:?}");
    let (m32, m64) = (max3(r32), max3(r64));
    assert!(m32 < 3e-3, "{r32:?}");
    assert!(m32 / m64 > 1.6, "{r32:?} {r64:?}");
    // The corner cells meet the axis of the rotation on two faces.
    assert!(r32[2] < 3e-3 && r64[2] < r32[2], "{r32:?} {r64:?}");
}

fn max3(v: [f64; 3]) -> f64 {
    v[0].max(v[1]).max(v[2])
}

/// The errors of the Laplacian of an eigenfunction, and the decay rate
/// `−Σ q ∇²q / Σ q²`.
fn laplacian_errors(n: usize, f: fn(Vector) -> f64, eigen: f64) -> (Errors, f64) {
    let grid = Grid::new(n);
    let mut ops = Ops::new(&grid);
    let q = field(&grid, f);
    let mut out = vec![0.0; grid.len()];
    ops.laplacian(&q, None, &mut out);
    let exact: Vec<f64> = q.iter().map(|v| eigen * v).collect();
    let qq: Vec<f64> = q.iter().map(|v| v * v).collect();
    let ql: Vec<f64> = q.iter().zip(&out).map(|(a, b)| a * b).collect();
    (
        errors(&grid, &out, &exact),
        -total(&grid, &ql) / total(&grid, &qq),
    )
}

fn check_laplacian(f: fn(Vector) -> f64, eigen: f64, name: &str) {
    let (e32, rate32) = laplacian_errors(32, f, eigen);
    let (e64, rate64) = laplacian_errors(64, f, eigen);
    println!("laplacian {name}: rate {rate32:.4} {rate64:.4}, {e32:?} {e64:?}");
    assert!((rate32 / -eigen - 1.0).abs() < 5e-3, "{rate32}");
    assert!((rate64 / -eigen - 1.0).abs() < (rate32 / -eigen - 1.0).abs());
    assert!(e32.l2 < 2.5e-2 && e64.l2 < e32.l2, "{e32:?} {e64:?}");
    // The largest errors are at the face edges, where the stencil is skewed.
    // They stay bounded, but they do not get smaller with a finer grid.
    for e in [&e32, &e64] {
        assert!(e.interior < 1.5e-2, "{e:?}");
        assert!(e.edge < 6.5e-2 && e.corner < 6.5e-2, "{e:?}");
    }
}

#[test]
fn laplacian_of_y20_is_minus_six_y20() {
    check_laplacian(y20, -6.0, "Y20");
}

#[test]
fn laplacian_of_y32_is_minus_twelve_y32() {
    check_laplacian(y32, -12.0, "Y32");
}

#[test]
fn laplacian_conserves_with_a_varying_diffusivity() {
    let grid = Grid::new(24);
    let mut ops = Ops::new(&grid);
    let q = field(&grid, |c| (3.0 * c[0]).sin() + c[1] * c[2]);
    let k = field(&grid, |c| 1.0 + 0.5 * c[2]);
    let mut out = vec![0.0; grid.len()];
    ops.laplacian(&q, Some(&k), &mut out);
    let scale: f64 = (0..grid.len()).map(|i| grid.area(i) * out[i].abs()).sum();
    assert!(total(&grid, &out).abs() < 1e-12 * scale.max(1.0));
}

fn advection_errors(n: usize, limit: bool) -> Errors {
    let grid = Grid::new(n);
    let mut ops = Ops::new(&grid);
    let u = vectors(&grid, rotation);
    let mut out = vec![0.0; grid.len()];
    ops.advection(&field(&grid, |c| c[2]), &u, limit, &mut out);
    // −∇·(u z) = −u·∇z for a field with no divergence.
    let exact = field(&grid, |c| -rotation(c)[2]);
    errors(&grid, &out, &exact)
}

#[test]
fn advection_of_z_by_a_rotation_converges() {
    let e32 = advection_errors(32, false);
    let e64 = advection_errors(64, false);
    println!("advection: {e32:?} {e64:?}");
    assert!(
        e32.interior < 5e-3 && e64.interior < e32.interior,
        "{e32:?} {e64:?}"
    );
    assert!(e32.edge < 1e-2 && e64.edge < e32.edge, "{e32:?} {e64:?}");
    assert!(
        e32.corner < 1e-2 && e64.corner < e32.corner,
        "{e32:?} {e64:?}"
    );
}

#[test]
fn the_limiter_keeps_a_smooth_field_accurate() {
    let e = advection_errors(32, true);
    println!("advection with the limiter: {e:?}");
    assert!(e.l2 < 5e-2, "{e:?}");
}

#[test]
fn advection_conserves_and_the_limiter_adds_no_extremes() {
    let grid = Grid::new(24);
    let mut ops = Ops::new(&grid);
    let u = vectors(&grid, |c| {
        let r = rotation(c);
        [r[0] + 0.3 * c[1], r[1], r[2]]
    });
    // A step: 1 in a cap around the north pole, 0 elsewhere.
    let q = field(&grid, |c| f64::from(c[2] > 0.5));
    let mut out = vec![0.0; grid.len()];
    for limit in [false, true] {
        ops.advection(&q, &u, limit, &mut out);
        let scale: f64 = (0..grid.len()).map(|i| grid.area(i) * out[i].abs()).sum();
        assert!(total(&grid, &out).abs() < 1e-12 * scale.max(1.0));
    }
    // One small explicit step of the rotation stays within [0, 1] with the
    // limiter, up to the small discrete divergence of the rotation.
    let dt = 0.2 * std::f64::consts::PI / 2.0 / 24.0;
    ops.advection(&q, &vectors(&grid, rotation), true, &mut out);
    for (q, d) in q.iter().zip(&out) {
        let next = q + dt * d;
        assert!((-1e-4..=1.0 + 1e-4).contains(&next), "{next}");
    }
}
