//! A scatter of points inside a polygon, fixed on the ground.
//!
//! The points come from a lattice of levels in a Lambert azimuthal equal-area
//! plane. Level `l` has square cells of size `s_l = S / 2^l`, where `S` is the
//! power of two that is equal to or more than `4 R`. The lattice is absolute:
//! it does not depend on the polygon, the window, or the spacing. Each cell
//! has one candidate in the middle half of the cell, at a position from a hash
//! of the seed, the level, and the cell. Level 0 keeps all its candidates.
//! Level `l` keeps the points of level `l - 1`, and adds each candidate that
//! is at least `s_l / 2` from all of them. Thus a smaller spacing only adds
//! points, and a point never moves.

use kurbo::{Point, Rect};

use crate::frame::{LonLat, wrap180};

/// The highest level.
const MAX_LEVEL: u32 = 50;

/// A Lambert azimuthal equal-area plane in meters.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScatterPlane {
    center: LonLat,
    radius: f64,
    sin_lat0: f64,
    cos_lat0: f64,
}

impl ScatterPlane {
    /// A plane with its center at `center`, on a sphere of `radius` meters.
    pub fn new(center: LonLat, radius: f64) -> Self {
        let (sin_lat0, cos_lat0) = center[1].to_radians().sin_cos();
        Self {
            center,
            radius,
            sin_lat0,
            cos_lat0,
        }
    }

    /// A plane for a polygon. The center is the mean direction of the ring
    /// vertices, rounded to whole degrees, so that a small edit does not move
    /// the lattice.
    pub fn for_rings(rings: &[Vec<LonLat>], radius: f64) -> Self {
        let mut sum = [0.0f64; 3];
        for p in rings.iter().flatten() {
            let (sin_lon, cos_lon) = p[0].to_radians().sin_cos();
            let (sin_lat, cos_lat) = p[1].to_radians().sin_cos();
            sum[0] += cos_lat * cos_lon;
            sum[1] += cos_lat * sin_lon;
            sum[2] += sin_lat;
        }
        let norm = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
        let center = if norm > 1e-9 {
            let lat = (sum[2] / norm).clamp(-1.0, 1.0).asin().to_degrees();
            let lon = sum[1].atan2(sum[0]).to_degrees();
            [wrap180(lon.round()), lat.round()]
        } else {
            [0.0, 0.0]
        };
        Self::new(center, radius)
    }

    /// The center as `[lon, lat]` in degrees.
    pub fn center(&self) -> LonLat {
        self.center
    }

    /// The sphere radius in meters.
    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// The plane point of a point on the sphere. `None` at the antipode of
    /// the center.
    pub fn forward(&self, p: LonLat) -> Option<Point> {
        let (sin_dlon, cos_dlon) = (p[0] - self.center[0]).to_radians().sin_cos();
        let (sin_lat, cos_lat) = p[1].to_radians().sin_cos();
        let cos_c = self.sin_lat0 * sin_lat + self.cos_lat0 * cos_lat * cos_dlon;
        if 1.0 + cos_c < 1e-12 {
            return None;
        }
        let k = self.radius * (2.0 / (1.0 + cos_c)).sqrt();
        Some(Point::new(
            k * cos_lat * sin_dlon,
            k * (self.cos_lat0 * sin_lat - self.sin_lat0 * cos_lat * cos_dlon),
        ))
    }

    /// The point on the sphere of a plane point.
    pub fn inverse(&self, p: Point) -> LonLat {
        let rho = p.x.hypot(p.y);
        if rho < 1e-9 {
            return self.center;
        }
        let c = 2.0 * (rho / (2.0 * self.radius)).min(1.0).asin();
        let (sin_c, cos_c) = c.sin_cos();
        let lat = (cos_c * self.sin_lat0 + p.y * sin_c * self.cos_lat0 / rho)
            .clamp(-1.0, 1.0)
            .asin();
        let dlon = (p.x * sin_c).atan2(rho * self.cos_lat0 * cos_c - p.y * self.sin_lat0 * sin_c);
        [
            wrap180(self.center[0] + dlon.to_degrees()),
            lat.to_degrees(),
        ]
    }

    /// The cell size `S` of level 0.
    fn top(&self) -> f64 {
        2f64.powi((4.0 * self.radius).log2().ceil() as i32)
    }
}

/// The parameters of a scatter.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScatterParams {
    /// The seed of the jitter.
    pub seed: u64,
    /// The wanted minimum distance between two points, in meters. The real
    /// minimum distance is between `spacing` and `2 * spacing`.
    pub spacing: f64,
    /// The minimum distance from a point to the polygon edge, in meters.
    pub inset: f64,
    /// The scale of a point is in `1 - size_jitter ..= 1`.
    pub size_jitter: f64,
    /// The rotation of a point is in `-rotation_jitter ..= rotation_jitter`
    /// degrees.
    pub rotation_jitter: f64,
    /// The maximum number of points.
    pub max_points: usize,
}

/// A point of a scatter.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScatterPoint {
    pub lonlat: LonLat,
    pub scale: f64,
    pub rotation: f64,
}

/// A seed from bytes, for example from the UUID of a feature. FNV-1a.
pub fn seed_from(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The real minimum distance between two points for a spacing, in meters.
/// This is `s_L / 2` of the level that `scatter` uses when the cost guard
/// does not apply.
pub fn min_distance(plane: &ScatterPlane, spacing: f64) -> f64 {
    let top = plane.top();
    top / 2f64.powi(level_for(top, spacing) as i32 + 1)
}

/// The points of a scatter inside the polygon `rings`, with the even-odd
/// rule in the plane. The ring edges are straight lines in the plane.
///
/// `window` is a rectangle in the plane, in meters. `None` is the bounds of
/// the rings. The points inside a window are the same as the points of the
/// full polygon inside that window, so a pan does not move a point.
///
/// The order is by level, then by cell. If there are more than `max_points`
/// points, the first `max_points` points are kept.
///
/// The cost guard: if the window has more than `4 * max_points` cells at the
/// level of the spacing, the scatter uses the finest level that has fewer
/// cells. Then the density is lower, and the points do not overlap.
pub fn scatter(
    plane: &ScatterPlane,
    rings: &[Vec<LonLat>],
    params: &ScatterParams,
    window: Option<Rect>,
) -> Vec<ScatterPoint> {
    if !(params.spacing > 0.0 && params.spacing.is_finite()) || params.max_points == 0 {
        return Vec::new();
    }
    let mut plane_rings = Vec::with_capacity(rings.len());
    for ring in rings {
        let Some(ring) = ring
            .iter()
            .map(|&p| plane.forward(p))
            .collect::<Option<Vec<_>>>()
        else {
            return Vec::new();
        };
        if ring.len() >= 3 {
            plane_rings.push(ring);
        }
    }
    let Some(bounds) = plane_rings
        .iter()
        .flatten()
        .map(|&p| Rect::from_points(p, p))
        .reduce(|a, b| a.union(b))
    else {
        return Vec::new();
    };
    let inset = params.inset.max(0.0);
    let window = window.unwrap_or(bounds).intersect(bounds.inset(-inset));
    if !(window.width() > 0.0 && window.height() > 0.0) {
        return Vec::new();
    }

    let top = plane.top();
    let mut level = level_for(top, params.spacing);
    let budget = 4.0 * params.max_points as f64;
    while level > 0 && cell_count(window, top / 2f64.powi(level as i32)) > budget {
        level -= 1;
    }

    let points = lattice(params.seed, top, level, window);
    let edges = Edges::new(&plane_rings);
    let mut out: Vec<Cand> = points
        .into_iter()
        .filter(|c| window.contains(c.p) && edges.inside(c.p) && edges.clear(c.p, inset))
        .collect();
    out.sort_by_key(|c| (c.level, c.i, c.j));
    out.truncate(params.max_points);
    out.into_iter()
        .map(|c| {
            let h = hash(params.seed, c.level, c.i, c.j);
            ScatterPoint {
                lonlat: plane.inverse(c.p),
                scale: 1.0 - params.size_jitter * unit(mix(h ^ 3)),
                rotation: params.rotation_jitter * (2.0 * unit(mix(h ^ 4)) - 1.0),
            }
        })
        .collect()
}

/// The largest level `L` with `s_L / 2 >= spacing`, or 0.
fn level_for(top: f64, spacing: f64) -> u32 {
    let mut level = 0;
    while level < MAX_LEVEL && top / 2f64.powi(level as i32 + 2) >= spacing {
        level += 1;
    }
    level
}

fn cell_count(r: Rect, s: f64) -> f64 {
    ((r.x1 / s).floor() - (r.x0 / s).floor() + 1.0)
        * ((r.y1 / s).floor() - (r.y0 / s).floor() + 1.0)
}

#[derive(Copy, Clone)]
struct Cand {
    p: Point,
    level: u32,
    i: i64,
    j: i64,
}

/// The points of level `level` near `window`.
///
/// Two candidates of one level are always at least `s_l / 2` apart, because
/// each is in the middle half of its cell. So the acceptance of a candidate
/// at level `l` depends only on the points of level `l - 1` within `s_l / 2`.
/// Level `l` is computed over the window grown by `s_l`. A candidate there
/// needs the points of level `l - 1` within `s_l + s_l / 2 < s_{l-1}` of the
/// window, and level `l - 1` was computed over the window grown by
/// `s_{l-1}`. So the result inside the window does not depend on the window.
fn lattice(seed: u64, top: f64, level: u32, window: Rect) -> Vec<Cand> {
    let mut points: Vec<Cand> = Vec::new();
    for l in 0..=level {
        let s = top / 2f64.powi(l as i32);
        let region = window.inflate(s, s);
        let key = |p: Point| ((p.x / s).floor() as i64, (p.y / s).floor() as i64);
        let (i0, j0) = key(Point::new(region.x0, region.y0));
        let (i1, j1) = key(Point::new(region.x1, region.y1));
        // The points of the last level in cells, with one more cell on each
        // side. A point farther out is more than `s / 2` from the region.
        let (w, h) = ((i1 - i0 + 3) as usize, (j1 - j0 + 3) as usize);
        let cell = |(i, j): (i64, i64)| {
            let (gi, gj) = (i - i0 + 1, j - j0 + 1);
            (gi >= 0 && gj >= 0 && (gi as usize) < w && (gj as usize) < h)
                .then(|| gj as usize * w + gi as usize)
        };
        let mut start = vec![0u32; w * h + 1];
        for c in &points {
            if let Some(k) = cell(key(c.p)) {
                start[k + 1] += 1;
            }
        }
        for k in 0..w * h {
            start[k + 1] += start[k];
        }
        let mut fill = start.clone();
        let mut grid = vec![Point::ZERO; start[w * h] as usize];
        for c in &points {
            if let Some(k) = cell(key(c.p)) {
                grid[fill[k] as usize] = c.p;
                fill[k] += 1;
            }
        }
        let near = |p: Point, i: i64, j: i64| {
            (j - 1..=j + 1).any(|gj| {
                (i - 1..=i + 1).any(|gi| {
                    cell((gi, gj)).is_some_and(|k| {
                        grid[start[k] as usize..start[k + 1] as usize]
                            .iter()
                            .any(|q| (*q - p).hypot() < s / 2.0)
                    })
                })
            })
        };
        let mut added = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                let h = hash(seed, l, i, j);
                let p = Point::new(
                    (i as f64 + 0.25 + 0.5 * unit(mix(h ^ 1))) * s,
                    (j as f64 + 0.25 + 0.5 * unit(mix(h ^ 2))) * s,
                );
                if region.contains(p) && !near(p, i, j) {
                    added.push(Cand { p, level: l, i, j });
                }
            }
        }
        points.retain(|c| region.contains(c.p));
        points.extend(added);
    }
    points
}

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn hash(seed: u64, level: u32, i: i64, j: i64) -> u64 {
    let h = mix(seed);
    let h = mix(h ^ u64::from(level));
    let h = mix(h ^ i as u64);
    mix(h ^ j as u64)
}

/// A number in `0..1` from a hash.
fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// The ring edges in horizontal bands.
struct Edges {
    segs: Vec<(Point, Point)>,
    bands: Vec<Vec<u32>>,
    y0: f64,
    band: f64,
}

impl Edges {
    fn new(rings: &[Vec<Point>]) -> Self {
        let segs: Vec<(Point, Point)> = rings
            .iter()
            .flat_map(|r| {
                r.iter()
                    .zip(r.iter().cycle().skip(1))
                    .map(|(&a, &b)| (a, b))
            })
            .collect();
        let y0 = segs.iter().map(|s| s.0.y).fold(f64::INFINITY, f64::min);
        let y1 = segs.iter().map(|s| s.0.y).fold(f64::NEG_INFINITY, f64::max);
        let n = segs.len().clamp(1, 1024);
        let band = ((y1 - y0) / n as f64).max(1e-9);
        let mut bands = vec![Vec::new(); n];
        for (k, s) in segs.iter().enumerate() {
            let (a, b) = (s.0.y.min(s.1.y), s.0.y.max(s.1.y));
            let (ba, bb) = (Self::index(y0, band, n, a), Self::index(y0, band, n, b));
            for v in &mut bands[ba..=bb] {
                v.push(k as u32);
            }
        }
        Self {
            segs,
            bands,
            y0,
            band,
        }
    }

    fn index(y0: f64, band: f64, n: usize, y: f64) -> usize {
        (((y - y0) / band).floor().max(0.0) as usize).min(n - 1)
    }

    fn band_of(&self, y: f64) -> usize {
        Self::index(self.y0, self.band, self.bands.len(), y)
    }

    /// Even-odd point in polygon.
    fn inside(&self, p: Point) -> bool {
        let mut inside = false;
        for &k in &self.bands[self.band_of(p.y)] {
            let (a, b) = self.segs[k as usize];
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
        }
        inside
    }

    /// Whether `p` is at least `d` from every edge.
    fn clear(&self, p: Point, d: f64) -> bool {
        if d <= 0.0 {
            return true;
        }
        let (b0, b1) = (self.band_of(p.y - d), self.band_of(p.y + d));
        self.bands[b0..=b1].iter().flatten().all(|&k| {
            let (a, b) = self.segs[k as usize];
            let ab = b - a;
            let len2 = ab.hypot2();
            let t = if len2 > 0.0 {
                ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (p - (a + ab * t)).hypot() >= d
        })
    }
}
