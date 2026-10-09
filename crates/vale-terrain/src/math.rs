//! Vectors on the unit sphere.

pub type V3 = [f64; 3];

pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn length(a: V3) -> f64 {
    dot(a, a).sqrt()
}

pub fn normalize(a: V3) -> V3 {
    let l = length(a);
    if l > 0.0 { scale(a, 1.0 / l) } else { a }
}

/// The angle between two unit vectors, in radians.
pub fn angle(a: V3, b: V3) -> f64 {
    length(cross(a, b)).atan2(dot(a, b))
}

/// Longitude and latitude in degrees to a unit vector. The north pole is +Z.
pub fn lonlat_to_dir(lon: f64, lat: f64) -> V3 {
    let (lon, lat) = (lon.to_radians(), lat.to_radians());
    [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()]
}

/// A unit vector to longitude and latitude in degrees.
pub fn dir_to_lonlat(d: V3) -> (f64, f64) {
    (
        d[1].atan2(d[0]).to_degrees(),
        d[2].clamp(-1.0, 1.0).asin().to_degrees(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lonlat_round_trip() {
        let (lon, lat) = dir_to_lonlat(lonlat_to_dir(33.0, -71.0));
        assert!((lon - 33.0).abs() < 1e-9 && (lat + 71.0).abs() < 1e-9);
    }
}
