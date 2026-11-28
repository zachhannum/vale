//! A zonal temperature field for the tests of the Explore workspace.

use isotherm_fields::{Means, Output};
use vale_terrain::{face_dir, unwarp};

pub const FACE: usize = 16;

/// The temperature of a zonal field at a latitude, warmer by one degree
/// for each month.
pub fn zonal(lat: f64, month: usize) -> f64 {
    30.0 - 60.0 * lat.sin().powi(2) + month as f64
}

/// Means of twelve months of the zonal field.
pub fn zonal_means() -> Means {
    let n = FACE;
    let lats: Vec<f64> = (0..6 * n * n)
        .map(|i| {
            let (face, y, x) = (i / (n * n), i / n % n, i % n);
            let s = |k: usize| unwarp((k as f64 + 0.5) / n as f64 * 2.0 - 1.0);
            face_dir(face, s(x), s(y))[2].asin()
        })
        .collect();
    let mut means = Means::new(n, &[Output::Temperature]);
    for month in 0..12 {
        let values: Vec<f64> = lats.iter().map(|&lat| zonal(lat, month)).collect();
        means.add(month, &[&values]);
    }
    means.close();
    means
}
