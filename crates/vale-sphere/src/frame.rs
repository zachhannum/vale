//! Frame rotation.

/// A point as `[lon, lat]` in degrees.
pub type LonLat = [f64; 2];

/// A rotation of the sphere in which the edge of a projection is a straight line.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Frame {
    /// Frame longitude is `lon - lon0`. Frame latitude is the latitude.
    Shift { lon0: f64 },
    /// The center of the projection becomes the north pole of the frame.
    Polar { lon0: f64, lat0: f64 },
}

/// Wraps an angle into -180..180.
pub fn wrap180(d: f64) -> f64 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

impl Frame {
    /// Moves a point of the sphere into the frame.
    pub fn to_frame(&self, p: LonLat) -> LonLat {
        match *self {
            Frame::Shift { lon0 } => [wrap180(p[0] - lon0), p[1]],
            Frame::Polar { lon0, lat0 } => {
                let (l, f) = ((p[0] - lon0).to_radians(), p[1].to_radians());
                let b = (90.0 - lat0).to_radians();
                let (x1, y1, z1) = (f.cos() * l.cos(), f.cos() * l.sin(), f.sin());
                let x2 = x1 * b.cos() - z1 * b.sin();
                let z2 = x1 * b.sin() + z1 * b.cos();
                // At the frame pole the longitude is undefined. Rounding noise in
                // x2 and y1 would give an arbitrary one, which can fall on the cut.
                let lon = if x2.hypot(y1) < 1e-12 {
                    0.0
                } else {
                    y1.atan2(x2).to_degrees()
                };
                [lon, z2.clamp(-1.0, 1.0).asin().to_degrees()]
            }
        }
    }

    /// Moves a frame point back to the sphere.
    pub fn from_frame(&self, p: LonLat) -> LonLat {
        match *self {
            Frame::Shift { lon0 } => [lon0 + p[0], p[1]],
            Frame::Polar { lon0, lat0 } => {
                let (l, f) = (p[0].to_radians(), p[1].to_radians());
                let b = (90.0 - lat0).to_radians();
                let (x2, y2, z2) = (f.cos() * l.cos(), f.cos() * l.sin(), f.sin());
                let x1 = x2 * b.cos() + z2 * b.sin();
                let z1 = -x2 * b.sin() + z2 * b.cos();
                [
                    lon0 + y2.atan2(x1).to_degrees(),
                    z1.clamp(-1.0, 1.0).asin().to_degrees(),
                ]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Vec<LonLat> {
        let mut v = Vec::new();
        for i in 0..20 {
            for j in 0..10 {
                v.push([-180.0 + i as f64 * 18.0 + 3.0, -85.0 + j as f64 * 19.0]);
            }
        }
        v
    }

    #[test]
    fn round_trip() {
        for fr in [
            Frame::Shift { lon0: 150.0 },
            Frame::Polar {
                lon0: 20.0,
                lat0: 30.0,
            },
        ] {
            for p in grid() {
                let q = fr.from_frame(fr.to_frame(p));
                assert!(wrap180(q[0] - p[0]).abs() < 1e-9, "{p:?} {q:?}");
                assert!((q[1] - p[1]).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn polar_latitudes() {
        let fr = Frame::Polar {
            lon0: 20.0,
            lat0: 30.0,
        };
        assert!((fr.to_frame([20.0, 30.0])[1] - 90.0).abs() < 1e-9);
        assert!((fr.to_frame([-160.0, -30.0])[1] + 90.0).abs() < 1e-9);
        assert!(fr.to_frame([110.0, 0.0])[1].abs() < 1e-9);
    }
}
