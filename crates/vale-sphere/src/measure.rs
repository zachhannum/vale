//! Lengths and areas on a sphere.
//!
//! The functions work on unit vectors, so the antimeridian and the poles
//! are not special.

use std::f64::consts::PI;

use crate::frame::LonLat;

pub(crate) type V3 = [f64; 3];

pub(crate) fn vec(p: LonLat) -> V3 {
    let (lon, lat) = (p[0].to_radians(), p[1].to_radians());
    [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()]
}

fn lonlat(v: V3) -> LonLat {
    let lat = v[2].clamp(-1.0, 1.0).asin().to_degrees();
    let lon = if v[0].abs() < 1e-15 && v[1].abs() < 1e-15 {
        0.0
    } else {
        v[1].atan2(v[0]).to_degrees()
    };
    [lon, lat]
}

pub(crate) fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// The angle between two points at the center of the sphere, in radians.
pub fn central_angle(a: LonLat, b: LonLat) -> f64 {
    let (a, b) = (vec(a), vec(b));
    norm(cross(a, b)).atan2(dot(a, b))
}

/// The points of a circle on the sphere around `center`, with an angular
/// radius in degrees, clockwise from north. The ring is open.
pub fn small_circle(center: LonLat, radius_deg: f64, points: usize) -> Vec<LonLat> {
    let c = vec(center);
    let (lon, lat) = (center[0].to_radians(), center[1].to_radians());
    let east = [-lon.sin(), lon.cos(), 0.0];
    let north = [-lat.sin() * lon.cos(), -lat.sin() * lon.sin(), lat.cos()];
    let (sin_r, cos_r) = radius_deg.to_radians().sin_cos();
    (0..points)
        .map(|i| {
            let (s, co) = (i as f64 / points as f64 * 2.0 * PI).sin_cos();
            lonlat(std::array::from_fn(|j| {
                c[j] * cos_r + (north[j] * co + east[j] * s) * sin_r
            }))
        })
        .collect()
}

/// The great-circle length of a path. A closed path includes the edge from
/// the last point back to the first.
pub fn path_length(points: &[LonLat], closed: bool, radius: f64) -> f64 {
    let mut sum: f64 = points.windows(2).map(|w| central_angle(w[0], w[1])).sum();
    if closed && points.len() > 2 {
        sum += central_angle(points[points.len() - 1], points[0]);
    }
    sum * radius
}

/// The signed area of a ring, in the square of the unit of `radius`. A
/// counterclockwise ring has a positive area.
pub fn signed_ring_area(ring: &[LonLat], radius: f64) -> f64 {
    let v: Vec<V3> = ring.iter().map(|&p| vec(p)).collect();
    if v.len() < 3 {
        return 0.0;
    }
    let a = v[0];
    let mut excess = 0.0;
    for w in v[1..].windows(2) {
        let (b, c) = (w[0], w[1]);
        let num = dot(a, cross(b, c));
        let den = 1.0 + dot(a, b) + dot(b, c) + dot(c, a);
        excess += 2.0 * num.atan2(den);
    }
    excess * radius * radius
}

/// The area of a ring, of either winding. Of the two areas that the ring
/// divides the sphere into, it is the smaller one.
pub fn ring_area(ring: &[LonLat], radius: f64) -> f64 {
    let sphere = 4.0 * PI * radius * radius;
    let a = signed_ring_area(ring, radius).abs() % sphere;
    a.min(sphere - a)
}

/// The area of a polygon: its first ring minus the other rings.
pub fn polygon_area(rings: &[Vec<LonLat>], radius: f64) -> f64 {
    let mut rings = rings.iter().map(|r| ring_area(r, radius));
    let outer = rings.next().unwrap_or(0.0);
    (outer - rings.sum::<f64>()).max(0.0)
}

/// The path with points added on each great-circle arc, so that two
/// neighbors are at most `max_step_deg` apart.
pub fn densify_great_circle(points: &[LonLat], max_step_deg: f64, closed: bool) -> Vec<LonLat> {
    let mut out = Vec::new();
    let n = points.len();
    if n == 0 {
        return out;
    }
    let edges = if closed && n > 2 { n } else { n - 1 };
    out.push(points[0]);
    for i in 0..edges {
        let (a, b) = (points[i], points[(i + 1) % n]);
        let angle = central_angle(a, b);
        let steps = (angle.to_degrees() / max_step_deg).ceil().max(1.0) as usize;
        let (va, vb) = (vec(a), vec(b));
        let s = angle.sin();
        for k in 1..steps {
            let t = k as f64 / steps as f64;
            let (wa, wb) = if s.abs() < 1e-12 {
                (1.0 - t, t)
            } else {
                (((1.0 - t) * angle).sin() / s, (t * angle).sin() / s)
            };
            let p = [
                wa * va[0] + wb * vb[0],
                wa * va[1] + wb * vb[1],
                wa * va[2] + wb * vb[2],
            ];
            out.push(lonlat(p));
        }
        out.push(b);
    }
    out
}

/// Whether the arc from `p` lies inside the arc from `a` to `b`, which has
/// the pole `n`.
fn on_arc(p: V3, a: V3, b: V3, n: V3) -> bool {
    dot(cross(a, p), n) >= 0.0 && dot(cross(p, b), n) >= 0.0
}

pub(crate) fn arcs_cross(a: V3, b: V3, c: V3, d: V3) -> bool {
    let (n1, n2) = (cross(a, b), cross(c, d));
    let t = cross(n1, n2);
    if norm(t) < 1e-12 {
        return false;
    }
    [t, [-t[0], -t[1], -t[2]]]
        .into_iter()
        .any(|p| on_arc(p, a, b, n1) && on_arc(p, c, d, n2))
}

/// Whether two edges of a closed ring cross. Neighbor edges do not count.
pub fn ring_crosses_itself(ring: &[LonLat]) -> bool {
    let v: Vec<V3> = ring.iter().map(|&p| vec(p)).collect();
    let n = v.len();
    if n < 4 {
        return false;
    }
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if arcs_cross(v[i], v[(i + 1) % n], v[j], v[(j + 1) % n]) {
                return true;
            }
        }
    }
    false
}

/// The first moment of the smaller side of a ring, on the unit sphere. Its
/// direction is the center of the area of that side.
fn ring_moment(ring: &[LonLat]) -> V3 {
    let v: Vec<V3> = ring.iter().map(|&p| vec(p)).collect();
    let mut m = [0.0; 3];
    if v.len() < 3 {
        return m;
    }
    // The moment of the left side is half the sum of the edge poles, each
    // scaled by the angle of its edge.
    for (i, &a) in v.iter().enumerate() {
        let b = v[(i + 1) % v.len()];
        let pole = cross(a, b);
        let s = norm(pole);
        if s < 1e-15 {
            continue;
        }
        let k = 0.5 * s.atan2(dot(a, b)) / s;
        (0..3).for_each(|j| m[j] += k * pole[j]);
    }
    // The two sides have opposite moments.
    if signed_ring_area(ring, 1.0).rem_euclid(4.0 * PI) > 2.0 * PI {
        m = m.map(|x| -x);
    }
    m
}

fn center_of(sum: V3) -> Option<LonLat> {
    let n = norm(sum);
    (n > 1e-12).then(|| lonlat(sum.map(|x| x / n)))
}

/// The center of the area of polygons, each a list of rings with the outer
/// ring first. `None` if the area is empty.
pub fn area_center(polygons: &[Vec<Vec<LonLat>>]) -> Option<LonLat> {
    let mut sum = [0.0; 3];
    for rings in polygons {
        for (i, ring) in rings.iter().enumerate() {
            let sign = if i == 0 { 1.0 } else { -1.0 };
            let m = ring_moment(ring);
            (0..3).for_each(|j| sum[j] += sign * m[j]);
        }
    }
    center_of(sum)
}

/// The center of lines, with each great-circle arc weighted by its length.
/// `None` if the lines have no length.
pub fn line_center(lines: &[Vec<LonLat>]) -> Option<LonLat> {
    let mut sum = [0.0; 3];
    for line in lines {
        for w in line.windows(2) {
            let (a, b) = (vec(w[0]), vec(w[1]));
            let mid = [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
            if norm(mid) < 1e-12 {
                continue;
            }
            // The moment of an arc of angle t is its unit midpoint times
            // 2 sin(t / 2), which is a + b times tan(t / 2).
            let k = norm(cross(a, b)) / (1.0 + dot(a, b));
            (0..3).for_each(|j| sum[j] += k * mid[j]);
        }
    }
    center_of(sum)
}

/// The mean direction of points. `None` if there are no points or they cancel.
pub fn point_center(points: &[LonLat]) -> Option<LonLat> {
    let mut sum = [0.0; 3];
    for &p in points {
        let v = vec(p);
        (0..3).for_each(|j| sum[j] += v[j]);
    }
    center_of(sum)
}
