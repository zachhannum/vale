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
    /// A free rotation. The rows are the frame axes on the sphere.
    Rotation([[f64; 3]; 3]),
}

/// The unit vector of a point.
pub fn xyz(p: LonLat) -> [f64; 3] {
    let (l, f) = (p[0].to_radians(), p[1].to_radians());
    [f.cos() * l.cos(), f.cos() * l.sin(), f.sin()]
}

/// The point of a unit vector. At a pole the longitude is 0.
pub fn lonlat(v: [f64; 3]) -> LonLat {
    // At a pole the longitude is undefined. Rounding noise would give an
    // arbitrary one, which can fall on the cut.
    let lon = if v[0].hypot(v[1]) < 1e-12 {
        0.0
    } else {
        v[1].atan2(v[0]).to_degrees()
    };
    [lon, v[2].clamp(-1.0, 1.0).asin().to_degrees()]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The unit vectors east and north at a point that is not a pole.
fn east_north(p: LonLat) -> ([f64; 3], [f64; 3]) {
    let (l, f) = (p[0].to_radians(), p[1].to_radians());
    (
        [-l.sin(), l.cos(), 0.0],
        [-f.sin() * l.cos(), -f.sin() * l.sin(), f.cos()],
    )
}

/// The rotation of the PROJ `ob_tran` wrapper, in degrees.
fn ob_tran_rotate(p: LonLat, lon_0: f64, o_lat_p: f64, o_lon_p: f64) -> LonLat {
    let lam = (p[0] - lon_0).to_radians();
    let phi = p[1].to_radians();
    let (sp, cp) = o_lat_p.to_radians().sin_cos();
    let lon = (phi.cos() * lam.sin()).atan2(sp * phi.cos() * lam.cos() + cp * phi.sin());
    let lat = (sp * phi.sin() - cp * phi.cos() * lam.cos())
        .clamp(-1.0, 1.0)
        .asin();
    [wrap180(lon.to_degrees() + o_lon_p), lat.to_degrees()]
}

/// Wraps an angle into -180..180.
pub fn wrap180(d: f64) -> f64 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

impl Frame {
    /// The frame of an oblique aspect. The center becomes the frame origin, and
    /// the frame equator leaves the center `rotation` degrees counterclockwise from east.
    /// The center must not be a pole.
    pub fn oblique(center: LonLat, rotation: f64) -> Frame {
        let (e, n) = east_north(center);
        let (s, c) = rotation.to_radians().sin_cos();
        let east: [f64; 3] = std::array::from_fn(|i| c * e[i] + s * n[i]);
        let north: [f64; 3] = std::array::from_fn(|i| c * n[i] - s * e[i]);
        Frame::Rotation([xyz(center), east, north])
    }

    /// The frame of the `ob_tran` parameters `lon_0`, `o_lat_p`, and `o_lon_p`.
    pub fn from_ob_tran(lon_0: f64, o_lat_p: f64, o_lon_p: f64) -> Frame {
        // The columns are the images of the axes.
        let cols = [[0.0, 0.0], [90.0, 0.0], [0.0, 90.0]]
            .map(|p| xyz(ob_tran_rotate(p, lon_0, o_lat_p, o_lon_p)));
        Frame::Rotation(std::array::from_fn(|r| std::array::from_fn(|c| cols[c][r])))
    }

    /// The `ob_tran` parameters `lon_0`, `o_lat_p`, and `o_lon_p` of this frame.
    pub fn ob_tran(&self) -> [f64; 3] {
        let pole = self.from_frame([0.0, 90.0]);
        let origin = self.from_frame([0.0, 0.0]);
        // With the pole in place, the rotation is a shift, and the origin is the simplest `lon_0`.
        let (lon_0, o_lat_p) = if pole[1] > 90.0 - 1e-9 {
            (origin[0], 90.0)
        } else {
            (wrap180(pole[0] + 180.0), pole[1])
        };
        let o_lon_p = -ob_tran_rotate(origin, lon_0, o_lat_p, 0.0)[0];
        [lon_0, o_lat_p, wrap180(o_lon_p)]
    }

    /// The center and the rotation of an oblique frame. See [`Frame::oblique`].
    pub fn center_and_rotation(&self) -> (LonLat, f64) {
        let center = self.from_frame([0.0, 0.0]);
        let east = xyz(self.from_frame([90.0, 0.0]));
        let (e, n) = east_north(center);
        (center, dot(east, n).atan2(dot(east, e)).to_degrees())
    }

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
            Frame::Rotation(m) => {
                let v = xyz(p);
                lonlat(m.map(|row| dot(row, v)))
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
            Frame::Rotation(m) => {
                let v = xyz(p);
                lonlat(std::array::from_fn(|i| {
                    m[0][i] * v[0] + m[1][i] * v[1] + m[2][i] * v[2]
                }))
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
            Frame::oblique([20.0, 30.0], 40.0),
            Frame::from_ob_tran(10.0, 50.0, -30.0),
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

    #[test]
    fn an_oblique_frame_puts_the_center_at_the_origin() {
        let fr = Frame::oblique([20.0, 30.0], 40.0);
        let o = fr.to_frame([20.0, 30.0]);
        assert!(o[0].abs() < 1e-9 && o[1].abs() < 1e-9);
        let (c, r) = fr.center_and_rotation();
        assert!((c[0] - 20.0).abs() < 1e-9 && (c[1] - 30.0).abs() < 1e-9);
        assert!((r - 40.0).abs() < 1e-9);
        // With no rotation on the equator, the frame is a shift.
        let s = Frame::oblique([20.0, 0.0], 0.0);
        let q = s.to_frame([50.0, 10.0]);
        assert!((q[0] - 30.0).abs() < 1e-9 && (q[1] - 10.0).abs() < 1e-9);
    }

    #[test]
    fn the_ob_tran_parameters_make_the_same_frame() {
        for (c, r) in [
            ([20.0, 30.0], 40.0),
            ([-120.0, -50.0], -75.0),
            ([0.0, 0.0], 90.0),
        ] {
            let fr = Frame::oblique(c, r);
            let [lon_0, lat_p, lon_p] = fr.ob_tran();
            let back = Frame::from_ob_tran(lon_0, lat_p, lon_p);
            for p in grid() {
                let (a, b) = (fr.to_frame(p), back.to_frame(p));
                assert!(wrap180(a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
            }
        }
    }
}
