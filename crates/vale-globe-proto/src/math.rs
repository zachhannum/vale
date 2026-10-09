//! Vectors and rotations on the unit sphere.

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

/// A point on the great circle from `a` to `b`. Both are unit vectors.
pub fn slerp(a: V3, b: V3, t: f64) -> V3 {
    let w = angle(a, b);
    if w < 1e-9 {
        return a;
    }
    let s = w.sin();
    normalize(add(
        scale(a, ((1.0 - t) * w).sin() / s),
        scale(b, (t * w).sin() / s),
    ))
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

/// A rotation matrix, stored as rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3(pub [V3; 3]);

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    pub fn mul_vec(&self, v: V3) -> V3 {
        [dot(self.0[0], v), dot(self.0[1], v), dot(self.0[2], v)]
    }

    /// Multiplies by the transpose, which is the inverse for a rotation.
    pub fn inv_mul_vec(&self, v: V3) -> V3 {
        let m = &self.0;
        [
            m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2],
            m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2],
            m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2],
        ]
    }

    pub fn mul(&self, o: &Mat3) -> Mat3 {
        let col = |j: usize| [o.0[0][j], o.0[1][j], o.0[2][j]];
        let (c0, c1, c2) = (col(0), col(1), col(2));
        let row = |r: V3| [dot(r, c0), dot(r, c1), dot(r, c2)];
        Mat3([row(self.0[0]), row(self.0[1]), row(self.0[2])])
    }

    /// A rotation by `angle` radians about a unit `axis`.
    pub fn axis_angle(axis: V3, angle: f64) -> Mat3 {
        let (s, c) = angle.sin_cos();
        let t = 1.0 - c;
        let [x, y, z] = axis;
        Mat3([
            [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
            [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
            [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
        ])
    }

    /// The shortest rotation that takes unit vector `a` to unit vector `b`.
    pub fn between(a: V3, b: V3) -> Mat3 {
        let axis = cross(a, b);
        let l = length(axis);
        if l < 1e-12 {
            return Mat3::IDENTITY;
        }
        Mat3::axis_angle(scale(axis, 1.0 / l), l.atan2(dot(a, b)))
    }

    /// Removes the drift that many small rotations add.
    pub fn orthonormalized(&self) -> Mat3 {
        let x = normalize(self.0[0]);
        let z = normalize(cross(x, self.0[1]));
        let y = cross(z, x);
        Mat3([x, y, z])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn between_takes_a_to_b() {
        let a = lonlat_to_dir(10.0, 20.0);
        let b = lonlat_to_dir(-120.0, -50.0);
        let r = Mat3::between(a, b);
        assert!(angle(r.mul_vec(a), b) < 1e-9);
        let back = r.inv_mul_vec(b);
        assert!(angle(back, a) < 1e-9);
    }

    #[test]
    fn lonlat_round_trip() {
        let (lon, lat) = dir_to_lonlat(lonlat_to_dir(33.0, -71.0));
        assert!((lon - 33.0).abs() < 1e-9 && (lat + 71.0).abs() < 1e-9);
    }
}
