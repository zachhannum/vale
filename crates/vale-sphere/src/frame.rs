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
    /// The rotation of the PROJ `ob_tran` wrapper with a new pole. The new
    /// pole is at longitude `lon_0 + 180` and at latitude `lat_p`. The
    /// longitude `lon_p` turns the frame around that pole.
    Oblique { lon_0: f64, lat_p: f64, lon_p: f64 },
}

/// The pole parameters of an oblique aspect that has its center on the
/// center line, with a given azimuth. The azimuth is clockwise from north, so
/// 90 is east. The center is at frame longitude 0 and latitude 0.
/// Returns `(lon_0, lat_p, lon_p)`.
pub fn oblique_pole(lon_c: f64, lat_c: f64, azimuth: f64) -> (f64, f64, f64) {
    let (lc, pc, a) = (lon_c.to_radians(), lat_c.to_radians(), azimuth.to_radians());
    // The new pole is 90 degrees to the left of the azimuth.
    let theta = a - std::f64::consts::FRAC_PI_2;
    let sin_p = (pc.cos() * theta.cos()).clamp(-1.0, 1.0);
    let lon_pole = lc + (theta.sin() * pc.cos()).atan2(-pc.sin() * sin_p);
    let lat_p = sin_p.asin();
    let lon_0 = lon_pole - std::f64::consts::PI;
    let (sp, cp) = (lat_p.sin(), lat_p.cos());
    let l = lc - lon_0;
    let center_lon = (pc.cos() * l.sin()).atan2(sp * pc.cos() * l.cos() + cp * pc.sin());
    (
        wrap180(lon_0.to_degrees()),
        lat_p.to_degrees(),
        wrap180(-center_lon.to_degrees()),
    )
}

/// The center and the azimuth of the center line of an oblique aspect.
/// This undoes `oblique_pole`. Returns `(lon_c, lat_c, azimuth)`.
pub fn oblique_center(lon_0: f64, lat_p: f64, lon_p: f64) -> (f64, f64, f64) {
    let frame = Frame::Oblique {
        lon_0,
        lat_p,
        lon_p,
    };
    let c = frame.from_frame([0.0, 0.0]);
    let ahead = frame.from_frame([0.01, 0.0]);
    let (pc, pa) = (c[1].to_radians(), ahead[1].to_radians());
    let dl = (ahead[0] - c[0]).to_radians();
    let azimuth = (dl.sin() * pa.cos()).atan2(pc.cos() * pa.sin() - pc.sin() * pa.cos() * dl.cos());
    (wrap180(c[0]), c[1], wrap180(azimuth.to_degrees()))
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
            Frame::Oblique {
                lon_0,
                lat_p,
                lon_p,
            } => {
                let (sphip, cphip) = lat_p.to_radians().sin_cos();
                let (l, f) = ((p[0] - lon_0).to_radians(), p[1].to_radians());
                let (cl, sf, cf) = (l.cos(), f.sin(), f.cos());
                let lon = (cf * l.sin()).atan2(sphip * cf * cl + cphip * sf);
                let lat = (sphip * sf - cphip * cf * cl).clamp(-1.0, 1.0).asin();
                [wrap180(lon.to_degrees() + lon_p), lat.to_degrees()]
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
            Frame::Oblique {
                lon_0,
                lat_p,
                lon_p,
            } => {
                let (sphip, cphip) = lat_p.to_radians().sin_cos();
                let (l, f) = ((p[0] - lon_p).to_radians(), p[1].to_radians());
                let (cl, sf, cf) = (l.cos(), f.sin(), f.cos());
                let lat = (sphip * sf + cphip * cf * cl).clamp(-1.0, 1.0).asin();
                let lon = (cf * l.sin()).atan2(sphip * cf * cl - cphip * sf);
                [wrap180(lon.to_degrees() + lon_0), lat.to_degrees()]
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
