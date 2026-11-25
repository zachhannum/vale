//! The cube map layout, with equal-angle spacing.
//!
//! Face `2 * axis` looks along the positive axis, and face `2 * axis + 1`
//! looks along the negative axis. On each face, `u` runs along axis
//! `(axis + 1) % 3` and `v` runs along axis `(axis + 2) % 3`.

use std::f64::consts::{FRAC_PI_4, TAU};

use crate::math::{V3, normalize};

pub const FACES: usize = 6;

/// The lowest and highest elevation of the default range, in meters.
pub const DEFAULT_ELEV_MIN: f64 = -6000.0;
pub const DEFAULT_ELEV_MAX: f64 = 6000.0;

/// The highest value of the lowest elevation of a range, in meters. A range
/// holds sea level with some room on each side.
pub const ELEV_MIN_HIGHEST: f64 = -100.0;
/// The lowest value of the highest elevation of a range, in meters.
pub const ELEV_MAX_LOWEST: f64 = 100.0;
/// The largest distance of an end of a range from sea level, in meters.
pub const ELEV_LIMIT: f64 = 30000.0;

/// The elevation range of a heightmap: level 0 is at `min` and level 65,535
/// is at `max`, in meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Elevation {
    pub min: f64,
    pub max: f64,
}

impl Default for Elevation {
    fn default() -> Elevation {
        Elevation {
            min: DEFAULT_ELEV_MIN,
            max: DEFAULT_ELEV_MAX,
        }
    }
}

impl Elevation {
    /// A range from `min` to `max`, or `None` if it breaks the rule of
    /// `is_valid`.
    pub fn new(min: f64, max: f64) -> Option<Elevation> {
        let range = Elevation { min, max };
        range.is_valid().then_some(range)
    }

    /// Whether the range holds sea level: `min` is at most
    /// `ELEV_MIN_HIGHEST`, `max` is at least `ELEV_MAX_LOWEST`, and each end
    /// is at most `ELEV_LIMIT` from 0 m.
    pub fn is_valid(self) -> bool {
        (-ELEV_LIMIT..=ELEV_MIN_HIGHEST).contains(&self.min)
            && (ELEV_MAX_LOWEST..=ELEV_LIMIT).contains(&self.max)
    }

    /// The level of an elevation in meters. An elevation out of the range
    /// gets the nearest end.
    pub fn level(self, m: f64) -> u16 {
        (((m - self.min) / (self.max - self.min)).clamp(0.0, 1.0) * 65535.0).round() as u16
    }

    /// The elevation of a level, in meters.
    pub fn meters(self, level: u16) -> f64 {
        self.min + f64::from(level) / 65535.0 * (self.max - self.min)
    }

    /// The meters of one 16-bit step.
    pub fn meters_per_level(self) -> f64 {
        (self.max - self.min) / 65535.0
    }

    /// The level of sea level.
    pub fn sea_level(self) -> u16 {
        self.level(0.0)
    }
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

    #[test]
    fn a_range_holds_sea_level_inside_the_limits() {
        assert!(Elevation::default().is_valid());
        assert!(Elevation::new(-100.0, 100.0).is_some());
        assert!(Elevation::new(-30000.0, 30000.0).is_some());
        assert!(Elevation::new(-99.0, 100.0).is_none());
        assert!(Elevation::new(-100.0, 99.0).is_none());
        assert!(Elevation::new(-30001.0, 100.0).is_none());
        assert!(Elevation::new(-100.0, 30001.0).is_none());
        assert!(Elevation::new(f64::NAN, 100.0).is_none());
    }

    #[test]
    fn a_narrow_range_has_sea_level_at_0_m() {
        let range = Elevation::new(-3000.0, 3000.0).unwrap();
        let half = 0.5 * range.meters_per_level();
        assert!(range.meters(range.sea_level()).abs() <= half);
        assert!((range.meters(range.level(1000.0)) - 1000.0).abs() <= half);
        assert_eq!(range.level(-5000.0), 0);
        assert_eq!(range.level(5000.0), u16::MAX);
    }
}
