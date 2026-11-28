//! Raster to vector conversion of the result fields and classes.

use std::f64::consts::{PI, TAU};

/// The area of a ring on a sphere of radius `radius_km`, in km². The ring
/// holds longitude and latitude pairs in degrees, with great circle edges.
/// The ring can be closed or open, and its direction does not matter.
pub fn ring_area_km2(ring: &[(f64, f64)], radius_km: f64) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    // The spherical excess is the sum of the signed areas of the triangles
    // that each edge makes with the north pole.
    let mut excess = 0.0;
    for (i, &(lon1, lat1)) in ring.iter().enumerate() {
        let (lon2, lat2) = ring[(i + 1) % ring.len()];
        let mut dlon = (lon2 - lon1).to_radians();
        dlon = (dlon + PI).rem_euclid(TAU) - PI;
        let t1 = (lat1.to_radians() / 2.0).tan();
        let t2 = (lat2.to_radians() / 2.0).tan();
        excess += 2.0 * ((dlon / 2.0).tan() * (t1 + t2)).atan2(1.0 + t1 * t2);
    }
    excess.abs() * radius_km * radius_km
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: f64 = 6371.0;

    #[test]
    fn octant_is_an_eighth_of_the_sphere() {
        let octant = [(0.0, 0.0), (90.0, 0.0), (0.0, 90.0)];
        let expected = 4.0 * PI * R * R / 8.0;
        assert!((ring_area_km2(&octant, R) - expected).abs() < 1e-6 * expected);
        let reversed = [(0.0, 90.0), (90.0, 0.0), (0.0, 0.0), (0.0, 90.0)];
        assert!((ring_area_km2(&reversed, R) - expected).abs() < 1e-6 * expected);
    }

    #[test]
    fn ring_across_the_antimeridian_is_small() {
        let ring = [(179.0, -1.0), (-179.0, -1.0), (-179.0, 1.0), (179.0, 1.0)];
        let side = 2.0_f64.to_radians() * R;
        let area = ring_area_km2(&ring, R);
        assert!((area - side * side).abs() < 1e-3 * side * side, "{area}");
    }
}
