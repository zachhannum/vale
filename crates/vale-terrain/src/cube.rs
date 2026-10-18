//! The cube map layout, with equal-angle spacing.
//!
//! Face `2 * axis` looks along the positive axis, and face `2 * axis + 1`
//! looks along the negative axis. On each face, `u` runs along axis
//! `(axis + 1) % 3` and `v` runs along axis `(axis + 2) % 3`.

use std::f64::consts::{FRAC_PI_4, TAU};

use crate::math::{V3, normalize};

pub const FACES: usize = 6;

/// The lowest and highest elevation that the 16-bit range holds, in meters.
pub const ELEV_MIN: f64 = -6000.0;
pub const ELEV_MAX: f64 = 6000.0;

pub fn meters_to_level(m: f64) -> u16 {
    (((m - ELEV_MIN) / (ELEV_MAX - ELEV_MIN)).clamp(0.0, 1.0) * 65535.0).round() as u16
}

pub fn level_to_meters(level: u16) -> f64 {
    ELEV_MIN + f64::from(level) / 65535.0 * (ELEV_MAX - ELEV_MIN)
}

/// The values that describe a face size to a person.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceSizeValues {
    /// The ground that one texel covers at the equator, in kilometers.
    pub km_per_pixel: f64,
    /// The width and the height of the equirectangular world map with the
    /// same detail, in pixels.
    pub map: (u32, u32),
    /// The memory of a layer with paint on each texel, before compression.
    pub bytes: u64,
}

/// The values of a face of `face_size` by `face_size` texels on a sphere with
/// a radius of `radius_km`.
pub fn face_size_values(face_size: u32, radius_km: f64) -> FaceSizeValues {
    let face = u64::from(face_size);
    FaceSizeValues {
        km_per_pixel: TAU * radius_km / (4.0 * f64::from(face_size)),
        map: (4 * face_size, 2 * face_size),
        bytes: FACES as u64 * face * face * size_of::<u16>() as u64,
    }
}

/// The face that a direction hits, and the flat coordinates on that face,
/// each from -1 to 1.
pub fn face_of(d: V3) -> (usize, f64, f64) {
    let abs = [d[0].abs(), d[1].abs(), d[2].abs()];
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

/// The direction of a point on a face. The inverse of `face_of`.
pub fn face_dir(face: usize, a: f64, b: f64) -> V3 {
    let axis = face / 2;
    let mut d = [0.0; 3];
    d[axis] = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
    d[(axis + 1) % 3] = a;
    d[(axis + 2) % 3] = b;
    normalize(d)
}

/// Flat face coordinate to equal-angle coordinate. Both run from -1 to 1.
pub fn warp(a: f64) -> f64 {
    a.atan() / FRAC_PI_4
}

/// Equal-angle coordinate to flat face coordinate.
pub fn unwarp(s: f64) -> f64 {
    (s * FRAC_PI_4).tan()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{angle, lonlat_to_dir};

    #[test]
    fn face_round_trip() {
        for &(lon, lat) in &[
            (0.0, 0.0),
            (100.0, 40.0),
            (-170.0, -85.0),
            (45.0, 35.3),
            (10.0, 89.0),
        ] {
            let d = lonlat_to_dir(lon, lat);
            let (face, a, b) = face_of(d);
            assert!(a.abs() <= 1.0 + 1e-12 && b.abs() <= 1.0 + 1e-12);
            assert!(angle(face_dir(face, a, b), d) < 1e-12);
        }
    }

    #[test]
    fn face_size_values_for_the_earth() {
        let small = face_size_values(4096, 6371.0);
        assert!((small.km_per_pixel - 2.443).abs() < 5e-4);
        assert_eq!(small.map, (16_384, 8192));
        assert_eq!(small.bytes, 201_326_592);

        let large = face_size_values(8192, 6371.0);
        assert!((large.km_per_pixel - 1.222).abs() < 5e-4);
        assert_eq!(large.map, (32_768, 16_384));
        assert_eq!(large.bytes, 805_306_368);

        let half = face_size_values(4096, 6371.0 / 2.0);
        assert_eq!(half.km_per_pixel, small.km_per_pixel / 2.0);
        assert_eq!((half.map, half.bytes), (small.map, small.bytes));
    }
}
