//! Lengths and areas on a sphere.
//!
//! The functions work on unit vectors, so the antimeridian and the poles
//! are not special.

use std::f64::consts::PI;

use crate::frame::LonLat;

type V3 = [f64; 3];

fn vec(p: LonLat) -> V3 {
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

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// The angle between two points at the center of the sphere, in radians.
pub fn central_angle(a: LonLat, b: LonLat) -> f64 {
    let (a, b) = (vec(a), vec(b));
    norm(cross(a, b)).atan2(dot(a, b))
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

fn arcs_cross(a: V3, b: V3, c: V3, d: V3) -> bool {
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
