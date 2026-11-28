//! A zonal temperature field and ice cover for the tests of the Explore workspace.

// Each test file uses a part of the helpers.
#![allow(dead_code)]

use isotherm_fields::{Means, Output};
use vale_terrain::{face_dir, unwarp};

pub const FACE: usize = 16;

/// The temperature of a zonal field at a latitude, warmer by one degree
/// for each month.
pub fn zonal(lat: f64, month: usize) -> f64 {
    30.0 - 60.0 * lat.sin().powi(2) + month as f64
}

/// The latitude of each cell, in radians.
fn lats() -> Vec<f64> {
    let n = FACE;
    (0..6 * n * n)
        .map(|i| {
            let (face, y, x) = (i / (n * n), i / n % n, i % n);
            let s = |k: usize| unwarp((k as f64 + 0.5) / n as f64 * 2.0 - 1.0);
            face_dir(face, s(x), s(y))[2].asin()
        })
        .collect()
}

/// The ice cover of the zonal field at a latitude: none at the equator,
/// full at the poles.
pub fn zonal_ice(lat: f64) -> f64 {
    lat.sin().powi(2)
}

/// Means of twelve months of the zonal field.
pub fn zonal_means() -> Means {
    let lats = lats();
    let mut means = Means::new(FACE, &[Output::Temperature]);
    for month in 0..12 {
        let values: Vec<f64> = lats.iter().map(|&lat| zonal(lat, month)).collect();
        means.add(month, &[&values]);
    }
    means.close();
    means
}

/// Means of twelve months of the zonal field and its ice cover.
pub fn zonal_means_with_ice() -> Means {
    let lats = lats();
    let ice: Vec<f64> = lats.iter().map(|&lat| zonal_ice(lat)).collect();
    let mut means = Means::new(FACE, &[Output::Temperature, Output::IceCover]);
    for month in 0..12 {
        let values: Vec<f64> = lats.iter().map(|&lat| zonal(lat, month)).collect();
        means.add(month, &[&values, &ice]);
    }
    means.close();
    means
}
