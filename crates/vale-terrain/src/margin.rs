//! The passive margin brush.
//!
//! The brush makes the edge of a continent that rifted from another
//! continent: a shallow shelf, a steeper slope, and a gentle rise at the
//! foot of the slope. Each stamp moves the sea floor under the brush toward
//! the margin profile. The profile is a function of the distance from the
//! nearest land, so the coast sets the side of the margin, and the land does
//! not change.
//!
//! A stroke reads a field of the distance to the land in a window around the
//! brush. The patterns along the coast come from the nearest point of the
//! coast, so a second stroke over the same place gives the same margin.

use std::f64::consts::FRAC_PI_2;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::cube::{ELEV_MAX, ELEV_MIN, FACES, face_of, warp};
use crate::heightmap::{Heightmap, Limits, TexelRect, brush_rect, falloff, noise3};
use crate::line::smoothstep;
use crate::math::{V3, add, cross, dot, length, normalize, scale};

/// The limits of the depth of the shelf break, in meters.
pub const SHELF_DEPTH_RANGE: (f64, f64) = (-1000.0, -20.0);

/// The limits of the width of the shelf, in kilometers.
pub const SHELF_WIDTH_RANGE: (f64, f64) = (5.0, 400.0);

/// The mean angle of the slope, in degrees (Pratson and Haxby 1996).
pub(crate) const SLOPE_DEG: f64 = 4.0;

/// The least and the largest angle of the slope, in degrees.
pub(crate) const SLOPE_RANGE_DEG: (f64, f64) = (1.0, 10.0);

/// The length of the features of the slope angle along the coast, in shelf
/// widths.
const ANGLE_SIZE: f64 = 2.0;

/// The part of the depth below the shelf break that the rise takes.
const RISE_PART: f64 = 0.4;

/// The width of the rise for each meter of its height, in kilometers. The
/// rise is a parabola, so its gradient is between 1:100 and 1:700 over its
/// middle.
const RISE_KM_PER_M: f64 = 0.25;

/// The part of the rise from where the margin fades into the floor from
/// before the stroke.
const RISE_FADE: f64 = 0.7;

/// The part of the deep sea of a window that is deeper than the floor of
/// the margin.
const FLOOR_PART: f64 = 0.15;

/// The least drop from the shelf break to the floor, in meters.
const LEAST_DROP: f64 = 200.0;

/// The rounding of the shelf break, in meters.
const BREAK_ROUND: f64 = 30.0;

/// The highest level of the margin, in meters, so the sea stays sea.
const TOP_M: f64 = -0.5;

/// The size of the features of the wander of the shelf break and its
/// largest distance, in shelf widths, for each of two octaves.
const WANDER: [(f64, f64); 2] = [(1.5, 0.22), (0.45, 0.08)];

/// The spacing of the gullies on the slope, in kilometers, and the least
/// spacing, in texels.
const GULLY_KM: f64 = 3.0;
const GULLY_TEXELS: f64 = 4.0;

/// The depth of a gully on a slope of `SLOPE_DEG`, in meters.
const GULLY_DEPTH: f64 = 220.0;

/// The spacing of the canyons along the coast, in kilometers, and the least
/// spacing, in gully spacings.
const CANYON_KM: f64 = 28.0;
const CANYON_GULLIES: f64 = 6.0;

/// The largest depth of a canyon on the slope, in meters.
const CANYON_DEPTH: f64 = 300.0;

/// The size of the features of the slide scars along the coast, in
/// kilometers, and the least size, in gully spacings.
const SCAR_KM: f64 = 70.0;
const SCAR_GULLIES: f64 = 12.0;

/// The largest depth of a slide scar, in meters.
const SCAR_DEPTH: f64 = 260.0;

/// The height of the debris below a scar, as a part of the depth of the
/// scar.
const DEBRIS_PART: f64 = 0.45;

/// The sizes of the features of the fine relief, in kilometers, and their
/// heights on the rise, in meters.
const RELIEF: [(f64, f64); 4] = [(40.0, 40.0), (16.0, 50.0), (7.0, 55.0), (3.5, 45.0)];

/// The number of cells along one side of a distance field, at most.
pub const FIELD_CELLS: usize = 1024;

/// The number of numbers of a cell of a distance field.
pub const FIELD_FLOATS: usize = 3;

/// The distance in cells from the nearest land cell where the field looks
/// for the coast.
const SEARCH: usize = 3;

/// The half width of the blur of the directions of a field, in cells.
const BLUR: usize = 3;

/// The distance from the first stamp of a window where the stamps can be,
/// in brush radii.
const PAINT: f64 = 4.0;

/// The largest half width of a window, in radians.
const MAX_HALF: f64 = 0.6;

const SALT_WANDER: u32 = 0x6d61_7267;
const SALT_ANGLE: u32 = 0x736c_6f70;
const SALT_GULLY: u32 = 0x6775_6c6c;
const SALT_PIECE: u32 = 0x7069_6563;
const SALT_CANYON: u32 = 0x6361_6e79;
const SALT_SCAR: u32 = 0x7363_6172;
const SALT_RELIEF: u32 = 0x7265_6c66;

/// The settings of a margin stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Margin {
    /// The depth of the shelf break, in meters.
    pub shelf_m: f64,
    /// The width of the shelf from the coast to the shelf break, in
    /// kilometers.
    pub shelf_km: f64,
    /// The radius of the world, in kilometers.
    pub world_km: f64,
    pub seed: u32,
    pub limits: Limits,
}

impl Margin {
    /// The settings inside their limits.
    fn clamped(self) -> Margin {
        Margin {
            shelf_m: self.shelf_m.clamp(SHELF_DEPTH_RANGE.0, SHELF_DEPTH_RANGE.1),
            shelf_km: self
                .shelf_km
                .clamp(SHELF_WIDTH_RANGE.0, SHELF_WIDTH_RANGE.1),
            world_km: self.world_km.max(1.0),
            ..self
        }
    }
}

/// The distance to the land in a window: a square grid of cells on the plane
/// that touches the sphere at `center`. A cell holds the distance to the
/// nearest land in kilometers and the direction away from that land on the
/// plane.
#[derive(Clone, Debug)]
pub struct MarginField {
    /// A number that no other field of this process has.
    pub id: u64,
    pub center: V3,
    pub east: V3,
    pub north: V3,
    /// The number of cells along one side.
    pub size: usize,
    /// The width of a cell on the plane, where the plane is at distance 1
    /// from the center of the sphere.
    pub cell: f64,
    /// The depth of the deep floor at the foot of the rise, in meters.
    pub floor_m: f64,
    /// For each cell, row by row from the south, the distance in kilometers
    /// and the east and north parts of the direction away from the land. A
    /// cell with no land in the window has the distance `f32::MAX`.
    pub cells: Vec<f32>,
    /// The distance from `center` where the stamps can be, in radians.
    paint: f64,
}

static FIELD_IDS: AtomicU64 = AtomicU64::new(1);

fn to_meters(level: f64) -> f64 {
    ELEV_MIN + level / 65535.0 * (ELEV_MAX - ELEV_MIN)
}

fn to_level(m: f64) -> f64 {
    (m - ELEV_MIN) / (ELEV_MAX - ELEV_MIN) * 65535.0
}

/// The distance from the coast where the margin ends, at most, in
/// kilometers, for a floor of `floor_m`.
fn reach_km(spec: &Margin, floor_m: f64) -> f64 {
    let drop = spec.shelf_m - floor_m;
    let height = RISE_PART * drop;
    let tan = SLOPE_RANGE_DEG.0.to_radians().tan();
    spec.shelf_km * (1.0 + WANDER[0].1 + WANDER[1].1)
        + (drop - height) / (1000.0 * tan)
        + RISE_KM_PER_M * height
}

impl MarginField {
    /// Reads the field around `center` from the ground of `map`, for stamps
    /// up to `paint` radians from `center`. The land is the ground above the
    /// sea level. With no `floor_m`, the field also reads the depth of the
    /// floor: the depth that `FLOOR_PART` of the sea farther from the land
    /// than the shelf and a slope of `SLOPE_DEG` is below. A margin from an
    /// earlier stroke is shallower, so it does not change that depth.
    pub fn read(
        map: &Heightmap,
        spec: &Margin,
        center: V3,
        paint: f64,
        floor_m: Option<f64>,
    ) -> MarginField {
        let spec = spec.clamped();
        let center = normalize(center);
        let east = cross([0.0, 0.0, 1.0], center);
        let east = if length(east) < 1e-12 {
            [0.0, 1.0, 0.0]
        } else {
            normalize(east)
        };
        let north = cross(center, east);
        // The window holds the stamps, and the land that a margin under them
        // can reach.
        let km = spec.world_km;
        let half = (paint + reach_km(&spec, floor_m.unwrap_or(ELEV_MIN)) / km).min(MAX_HALF);
        let half = half.tan();
        let texel = FRAC_PI_2 / map.face_size() as f64;
        let size = ((2.0 * half / texel).ceil() as usize).clamp(16, FIELD_CELLS);
        let cell = 2.0 * half / size as f64;
        let mut levels = Vec::with_capacity(size * size);
        for j in 0..size {
            let y = (j as f64 + 0.5) * cell - half;
            for i in 0..size {
                let x = (i as f64 + 0.5) * cell - half;
                let at = normalize(add(center, add(scale(east, x), scale(north, y))));
                levels.push(to_meters(f64::from(nearest_level(map, at))));
            }
        }
        let land: Vec<bool> = levels.iter().map(|&m| m > 0.0).collect();
        let (_, nearest) = distance_transform(&land, size);
        let plane = Plane {
            center,
            east,
            north,
            half,
            cell,
            size,
        };
        let segments = coast_segments(map, &plane);
        let mut cells = Vec::with_capacity(size * size * FIELD_FLOATS);
        let mut distances = Vec::with_capacity(size * size);
        for k in 0..size * size {
            if land[k] {
                distances.push(0.0);
                cells.extend([0.0, 0.0, 0.0]);
                continue;
            }
            if nearest[k] == usize::MAX {
                distances.push(f64::MAX);
                cells.extend([f32::MAX, 0.0, 0.0]);
                continue;
            }
            let (i, j) = ((k % size) as f64, (k / size) as f64);
            let (si, sj) = (nearest[k] % size, nearest[k] / size);
            // The nearest point of the coast segments near the nearest land
            // cell. With no segment there, the coast is half a cell from the
            // center of that cell.
            let mut best = (f64::MAX, (si as f64, sj as f64));
            for qj in sj.saturating_sub(SEARCH)..(sj + SEARCH + 1).min(size) {
                for qi in si.saturating_sub(SEARCH)..(si + SEARCH + 1).min(size) {
                    for &[a, b] in &segments[qj * size + qi] {
                        let q = nearest_on_segment((i, j), a, b);
                        let d = (i - q.0).hypot(j - q.1);
                        if d < best.0 {
                            best = (d, q);
                        }
                    }
                }
            }
            if best.0 == f64::MAX {
                let away = (i - si as f64).hypot(j - sj as f64);
                best = ((away - 0.5).max(0.0), (si as f64, sj as f64));
            }
            // The distance on the sphere, so two windows give the same
            // distance.
            let (here, there) = (plane.dir(i, j), plane.dir(best.1.0, best.1.1));
            let d = dot(here, there).clamp(-1.0, 1.0).acos() * km;
            let (dx, dy) = (i - best.1.0, j - best.1.1);
            let away = dx.hypot(dy).max(1e-9);
            distances.push(d);
            cells.extend([d as f32, (dx / away) as f32, (dy / away) as f32]);
        }
        smooth_directions(&mut cells, size);
        let deep = spec.shelf_m - LEAST_DROP;
        let floor_m = floor_m.unwrap_or_else(|| {
            let far = spec.shelf_km + 1500.0 / (1000.0 * SLOPE_DEG.to_radians().tan());
            let mut out: Vec<f64> = (0..size * size)
                .filter(|&k| distances[k] > far && levels[k] < deep)
                .map(|k| levels[k])
                .collect();
            if out.is_empty() {
                out = levels.iter().copied().filter(|&m| m < deep).collect();
            }
            if out.is_empty() {
                return deep;
            }
            out.sort_by(f64::total_cmp);
            out[(FLOOR_PART * out.len() as f64) as usize]
        });
        MarginField {
            id: FIELD_IDS.fetch_add(1, Ordering::Relaxed),
            center,
            east,
            north,
            size,
            cell,
            floor_m: floor_m.min(deep),
            cells,
            paint,
        }
    }

    /// Whether a stamp at `center` with a radius in radians is inside the
    /// part of the window for stamps.
    pub fn covers(&self, center: V3, radius: f64) -> bool {
        let a = dot(normalize(center), self.center).clamp(-1.0, 1.0).acos();
        a + radius <= self.paint
    }

    /// The distance to the land in kilometers and the direction away from it
    /// at a unit direction, from the 4 cells around it. A direction outside
    /// the window, or near a cell with no land in the window, gives `None`.
    pub(crate) fn sample(&self, p: V3) -> Option<(f64, V3)> {
        let z = dot(p, self.center);
        if z <= 0.0 {
            return None;
        }
        let half = 0.5 * self.size as f64 * self.cell;
        let fx = (dot(p, self.east) / z + half) / self.cell - 0.5;
        let fy = (dot(p, self.north) / z + half) / self.cell - 0.5;
        let last = (self.size - 1) as f64;
        if !(0.0..last).contains(&fx) || !(0.0..last).contains(&fy) {
            return None;
        }
        let (i, j) = (fx.floor() as usize, fy.floor() as usize);
        let (tx, ty) = (fx - i as f64, fy - j as f64);
        let mut out = [0.0; 3];
        for (k, w) in [
            (j * self.size + i, (1.0 - tx) * (1.0 - ty)),
            (j * self.size + i + 1, tx * (1.0 - ty)),
            ((j + 1) * self.size + i, (1.0 - tx) * ty),
            ((j + 1) * self.size + i + 1, tx * ty),
        ] {
            let c = &self.cells[k * FIELD_FLOATS..(k + 1) * FIELD_FLOATS];
            if c[0] == f32::MAX {
                return None;
            }
            for (o, v) in out.iter_mut().zip(c) {
                *o += w * f64::from(*v);
            }
        }
        let away = add(scale(self.east, out[1]), scale(self.north, out[2]));
        Some((out[0], away))
    }
}

/// The level of the texel nearest to a unit direction. A stroke does not
/// change which texels are land, so the land cells of a field do not depend
/// on the depth of the sea.
fn nearest_level(map: &Heightmap, d: V3) -> u16 {
    let n = map.face_size();
    let (face, a, b) = face_of(d);
    let at = |v: f64| (((warp(v) + 1.0) * 0.5 * n as f64).floor() as i64).clamp(0, n as i64 - 1);
    map.get(face, at(a), at(b))
}

/// A square grid of cells on the plane that touches the sphere at
/// `center`.
struct Plane {
    center: V3,
    east: V3,
    north: V3,
    /// The half width of the grid and the width of a cell, on the plane.
    half: f64,
    cell: f64,
    size: usize,
}

impl Plane {
    /// The unit direction at a place in cells, where the center of cell
    /// `(i, j)` is at `(i, j)`.
    fn dir(&self, i: f64, j: f64) -> V3 {
        let x = (i + 0.5) * self.cell - self.half;
        let y = (j + 0.5) * self.cell - self.half;
        normalize(add(
            self.center,
            add(scale(self.east, x), scale(self.north, y)),
        ))
    }

    /// The place in cells of a unit direction, or `None` behind the plane.
    fn place(&self, d: V3) -> Option<(f64, f64)> {
        let z = dot(d, self.center);
        if z <= 0.0 {
            return None;
        }
        let at = |axis: V3| (dot(d, axis) / z + self.half) / self.cell - 0.5;
        Some((at(self.east), at(self.north)))
    }
}

/// The pieces of the coast near each cell of a plane, in cells. A piece
/// joins the middles of the sides between a land texel and a sea texel in a
/// square of four texels of `map`. Thus the coast depends only on which
/// texels are land, and not on the cells or on the depth of the sea. A piece
/// is in the list of the cell that holds its middle.
fn coast_segments(map: &Heightmap, plane: &Plane) -> Vec<Vec<[(f64, f64); 2]>> {
    let size = plane.size;
    let mut out = vec![Vec::new(); size * size];
    let n = map.face_size();
    let reach = (plane.half * std::f64::consts::SQRT_2).atan();
    for face in 0..FACES {
        let Some(rect) = brush_rect(n, face, plane.center, reach) else {
            continue;
        };
        for y in rect.y0..rect.y1.min(n - 1) {
            for x in rect.x0..rect.x1.min(n - 1) {
                let corner = |dx: usize, dy: usize| {
                    let (cx, cy) = (x + dx, y + dy);
                    let level = map.get(face, cx as i64, cy as i64);
                    (map.texel_dir(face, cx, cy), to_meters(f64::from(level)))
                };
                let ring = [corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)];
                if ring.iter().all(|c| c.1 > 0.0) || ring.iter().all(|c| c.1 <= 0.0) {
                    continue;
                }
                let mut points = Vec::new();
                for e in 0..4 {
                    let ((p0, v0), (p1, v1)) = (ring[e], ring[(e + 1) % 4]);
                    if (v0 > 0.0) != (v1 > 0.0) {
                        points.extend(plane.place(normalize(add(p0, p1))));
                    }
                }
                for pair in points.as_chunks::<2>().0 {
                    let (a, b) = (pair[0], pair[1]);
                    let (mx, my) = ((0.5 * (a.0 + b.0)).round(), (0.5 * (a.1 + b.1)).round());
                    let last = (size - 1) as f64;
                    if (0.0..=last).contains(&mx) && (0.0..=last).contains(&my) {
                        out[my as usize * size + mx as usize].push([a, b]);
                    }
                }
            }
        }
    }
    out
}

/// Smooths the directions away from the land of the sea cells of a field,
/// with two box blurs of `BLUR` cells on each side. The coast of the texels
/// is a staircase, so the direction to its nearest point jumps, and the
/// patterns along the coast would break into beads.
fn smooth_directions(cells: &mut [f32], size: usize) {
    let sea = |c: &[f32]| c[0] > 0.0 && c[0] < f32::MAX;
    for _ in 0..2 {
        for axis in 0..2 {
            let before = cells.to_vec();
            for j in 0..size {
                for i in 0..size {
                    let k = j * size + i;
                    if !sea(&before[k * FIELD_FLOATS..]) {
                        continue;
                    }
                    let (mut x, mut y) = (0.0f64, 0.0f64);
                    for o in -(BLUR as i64)..=BLUR as i64 {
                        let (oi, oj) = if axis == 0 {
                            (i as i64 + o, j as i64)
                        } else {
                            (i as i64, j as i64 + o)
                        };
                        if oi < 0 || oj < 0 || oi >= size as i64 || oj >= size as i64 {
                            continue;
                        }
                        let q = (oj as usize * size + oi as usize) * FIELD_FLOATS;
                        if sea(&before[q..]) {
                            x += f64::from(before[q + 1]);
                            y += f64::from(before[q + 2]);
                        }
                    }
                    let length = x.hypot(y);
                    if length > 1e-9 {
                        cells[k * FIELD_FLOATS + 1] = (x / length) as f32;
                        cells[k * FIELD_FLOATS + 2] = (y / length) as f32;
                    }
                }
            }
        }
    }
}

/// The point of the segment from `a` to `b` that is nearest to `p`.
fn nearest_on_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (ux, uy) = (b.0 - a.0, b.1 - a.1);
    let length = ux * ux + uy * uy;
    let t = if length > 0.0 {
        (((p.0 - a.0) * ux + (p.1 - a.1) * uy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a.0 + ux * t, a.1 + uy * t)
}

/// The squared distance in cells from each cell to the nearest cell of
/// `sites`, and the index of that cell, on a square grid of `size` cells.
/// With no site, the distance is `f64::MAX` and the index is `usize::MAX`.
/// The steps are those of Felzenszwalb and Huttenlocher (2012).
fn distance_transform(sites: &[bool], size: usize) -> (Vec<f64>, Vec<usize>) {
    let inf = f64::MAX;
    let mut f = vec![inf; size];
    let mut out = vec![(inf, 0usize); size];
    // The columns first: the nearest site in the same column.
    let mut column = vec![(inf, usize::MAX); size * size];
    for i in 0..size {
        for (j, v) in f.iter_mut().enumerate() {
            *v = if sites[j * size + i] { 0.0 } else { inf };
        }
        lower_envelope(&f, &mut out);
        for (j, &(d, at)) in out.iter().enumerate() {
            if d < inf {
                column[j * size + i] = (d, at * size + i);
            }
        }
    }
    // Then the rows, from the columns.
    let mut squared = vec![inf; size * size];
    let mut nearest = vec![usize::MAX; size * size];
    for j in 0..size {
        for (i, v) in f.iter_mut().enumerate() {
            *v = column[j * size + i].0;
        }
        lower_envelope(&f, &mut out);
        for (i, &(d, at)) in out.iter().enumerate() {
            if d < inf {
                squared[j * size + i] = d;
                nearest[j * size + i] = column[j * size + at].1;
            }
        }
    }
    (squared, nearest)
}

/// The lower envelope of the parabolas `(q - p)² + f[p]` for the `p` with a
/// finite `f[p]`: for each `q`, the least value and the `p` that gives it.
fn lower_envelope(f: &[f64], out: &mut [(f64, usize)]) {
    let n = f.len();
    let used: Vec<usize> = (0..n).filter(|&p| f[p] < f64::MAX).collect();
    if used.is_empty() {
        out.fill((f64::MAX, 0));
        return;
    }
    let mut v = vec![used[0]];
    let mut z = vec![f64::NEG_INFINITY, f64::INFINITY];
    let cross_at = |q: usize, p: usize| {
        let (qf, pf) = (q as f64, p as f64);
        ((f[q] + qf * qf) - (f[p] + pf * pf)) / (2.0 * (qf - pf))
    };
    for &q in &used[1..] {
        let mut s = cross_at(q, v[v.len() - 1]);
        while s <= z[v.len() - 1] {
            v.pop();
            z.pop();
            if v.is_empty() {
                break;
            }
            s = cross_at(q, v[v.len() - 1]);
        }
        if v.is_empty() {
            v.push(q);
            z = vec![f64::NEG_INFINITY, f64::INFINITY];
            continue;
        }
        v.push(q);
        let last = z.len() - 1;
        z[last] = s;
        z.push(f64::INFINITY);
    }
    let mut k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let d = q as f64 - p as f64;
        *o = (d * d + f[p], p);
    }
}

/// One margin stroke.
#[derive(Clone, Debug)]
pub struct MarginStroke {
    spec: Margin,
    face_size: usize,
    field: Option<Arc<MarginField>>,
}

/// One stamp of a margin stroke: the texels that it can change and what it
/// needs to give their levels.
#[derive(Clone, Debug)]
pub struct MarginPlan {
    /// The settings, inside their limits.
    pub spec: Margin,
    pub field: Arc<MarginField>,
    pub center: V3,
    /// The brush radius, in radians.
    pub radius: f64,
    pub hardness: f64,
    pub flow: f64,
    pub face_size: usize,
    pub rects: [Option<TexelRect>; FACES],
}

impl MarginStroke {
    /// Starts a stroke on a heightmap with `face_size` texels along one side
    /// of a face.
    pub fn new(spec: Margin, face_size: usize) -> MarginStroke {
        MarginStroke {
            spec: spec.clamped(),
            face_size,
            field: None,
        }
    }

    /// The settings, inside their limits.
    pub fn spec(&self) -> Margin {
        self.spec
    }

    /// Adds a stamp at unit direction `center`, with a radius in radians, a
    /// hardness, and a flow from 0 to 1, and returns its plan. The first
    /// stamp reads the distance field and the depth of the floor from
    /// `map`. A stamp outside the window of the field reads a new field with
    /// the same floor. Give `map` the ground from before the stroke, or the
    /// ground of the stroke so far: a stroke does not change the land.
    pub fn stamp(
        &mut self,
        map: &Heightmap,
        center: V3,
        radius: f64,
        hardness: f64,
        flow: f64,
    ) -> MarginPlan {
        let center = normalize(center);
        let step = FRAC_PI_2 / self.face_size as f64;
        let radius = radius.max(2.0 * step);
        let covered = self
            .field
            .as_ref()
            .is_some_and(|field| field.covers(center, radius));
        if !covered {
            let floor = self.field.as_ref().map(|field| field.floor_m);
            let field = MarginField::read(map, &self.spec, center, PAINT * radius, floor);
            self.field = Some(Arc::new(field));
        }
        let field = self.field.clone().expect("the stroke has a field");
        let n = self.face_size;
        MarginPlan {
            spec: self.spec,
            field,
            center,
            radius,
            hardness: hardness.clamp(0.0, 0.999),
            flow: flow.clamp(0.0, 1.0),
            face_size: n,
            rects: std::array::from_fn(|face| brush_rect(n, face, center, radius)),
        }
    }
}

/// The place of a texel for the profile.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Place {
    /// The distance to the land, in kilometers.
    pub d: f64,
    /// The unit direction of the texel, and of the nearest point of the
    /// coast.
    pub p: V3,
    pub coast: V3,
}

/// The shape of the profile at one place, with no details.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shape {
    /// The level, in meters.
    pub level: f64,
    /// The distance below the shelf break, and the widths of the slope and
    /// the rise, in kilometers.
    pub down: f64,
    pub slope: f64,
    pub rise: f64,
    /// The place on the rise, from 0 at its top to 1 at its foot.
    pub t: f64,
}

impl MarginPlan {
    /// Whether the plan changes nothing.
    pub fn is_empty(&self) -> bool {
        self.rects.iter().all(Option::is_none)
    }

    /// The width of a texel at the center of a face, in kilometers.
    pub fn texel_km(&self) -> f64 {
        FRAC_PI_2 / self.face_size as f64 * self.spec.world_km
    }

    /// The place of a unit direction, or `None` outside the field.
    pub(crate) fn place(&self, p: V3) -> Option<Place> {
        let (d, away) = self.field.sample(p)?;
        let tangent = add(away, scale(p, -dot(away, p)));
        let size = length(tangent);
        let coast = if size < 1e-6 {
            p
        } else {
            let a = d / self.spec.world_km;
            normalize(add(scale(p, a.cos()), scale(tangent, -a.sin() / size)))
        };
        Some(Place { d, p, coast })
    }

    fn noise(&self, q: V3, size_km: f64, salt: u32) -> V3 {
        noise3(
            scale(q, self.spec.world_km / size_km),
            self.spec.seed ^ salt,
        )
    }

    /// The distance that the shelf break moves away from the coast, in
    /// kilometers.
    pub(crate) fn wander(&self, p: V3) -> f64 {
        let w = self.spec.shelf_km;
        WANDER
            .iter()
            .enumerate()
            .map(|(k, &(size, amount))| self.noise(p, size * w, SALT_WANDER)[k] * amount * w)
            .sum()
    }

    /// The slope angle at a point of the coast, as a tangent.
    pub(crate) fn slope(&self, coast: V3) -> f64 {
        let n = self.noise(coast, ANGLE_SIZE * self.spec.shelf_km, SALT_ANGLE)[0];
        let n = (1.4 * n).clamp(-1.0, 1.0);
        let deg = (SLOPE_DEG * 4f64.powf(n)).clamp(SLOPE_RANGE_DEG.0, SLOPE_RANGE_DEG.1);
        deg.to_radians().tan()
    }

    /// The spacing of the gullies, in kilometers.
    fn gully_km(&self) -> f64 {
        GULLY_KM.max(GULLY_TEXELS * self.texel_km())
    }

    /// The depth of the gullies at a place, from 0 to 1.
    pub(crate) fn gullies(&self, at: Place) -> f64 {
        let spacing = self.gully_km();
        let n = self.noise(at.coast, spacing, SALT_GULLY)[0];
        let v = 1.0 - n.abs();
        // Each gully deepens and fades down the slope, so the gullies are
        // pieces with gaps.
        let piece = self.noise(at.p, 2.5 * spacing, SALT_PIECE)[0];
        v * v * v * smoothstep(-0.3, 0.5, piece)
    }

    /// The depth of the canyons in meters, at a place `down` kilometers
    /// below the shelf break, with a slope and a rise of these widths in
    /// kilometers. A canyon starts a little inside the shelf break, cuts the
    /// slope, and goes on across the rise as a channel that gets shallower.
    pub(crate) fn canyons(&self, at: Place, down: f64, slope: f64, rise: f64) -> f64 {
        let texel = self.texel_km();
        let size = CANYON_KM.max(CANYON_GULLIES * self.gully_km());
        let n = self.noise(at.coast, size, SALT_CANYON);
        let v = ((0.15 - n[0].abs()) / 0.15).clamp(0.0, 1.0);
        let here = smoothstep(-0.2, 0.3, n[1]);
        let deep = 0.4 + 0.3 * (n[2] + 1.0);
        let head = -0.15 * size;
        let start = smoothstep(head, head + 0.3 * slope + texel, down);
        let channel = (-(down - slope).max(0.0) / (0.12 * rise + texel)).exp();
        CANYON_DEPTH * deep * here * v * v.sqrt() * start * channel
    }

    /// The change of the slide scars and their debris, in meters. The
    /// arguments are those of `canyons`.
    pub(crate) fn scars(&self, at: Place, down: f64, slope: f64, rise: f64) -> f64 {
        let texel = self.texel_km();
        let size = SCAR_KM.max(SCAR_GULLIES * self.gully_km());
        let n = self.noise(at.coast, size, SALT_SCAR);
        let here = smoothstep(0.25, 0.6, n[0]);
        let depth = SCAR_DEPTH * (0.3 + 0.35 * (n[1] + 1.0));
        let head = slope * (0.05 + 0.175 * (n[2] + 1.0));
        // The scar: a steep headwall, and a floor that runs out at the foot
        // of the slope.
        let cut = smoothstep(head, head + 0.08 * slope + 0.5 * texel, down)
            * (1.0 - smoothstep(0.75 * slope, slope + 0.05 * rise + texel, down));
        // The debris: a lobe on the rise below the scar.
        let length = 0.15 * rise + 0.3 * size;
        let q = (down - slope - 0.5 * length) / (0.5 * length + texel);
        let x = (1.0 - q * q).max(0.0);
        here * depth * (DEBRIS_PART * x * x - cut)
    }

    /// The fine relief at a unit direction, in meters, at a height of 1 on
    /// the rise.
    pub(crate) fn relief(&self, p: V3) -> f64 {
        let least = 2.0 * self.texel_km();
        RELIEF
            .iter()
            .enumerate()
            .map(|(k, &(size, height))| {
                let fade = smoothstep(least, 2.0 * least, size);
                let salt = SALT_RELIEF.wrapping_add(k as u32);
                self.noise(p, size, salt)[0] * height * fade
            })
            .sum()
    }

    /// The shape of the profile at a distance `x` from the coast in
    /// kilometers, with a slope angle with the tangent `tan`.
    pub(crate) fn shape(&self, x: f64, tan: f64) -> Shape {
        let w = self.spec.shelf_km;
        let b = self.spec.shelf_m;
        let floor = self.field.floor_m;
        // The shelf goes down from the coast to the break, and the slope
        // falls from the break.
        let down = x - w;
        let upper = smooth_min(b * x / w, b - 1000.0 * tan * down, BREAK_ROUND);
        // The rise is a parabola from the foot of the slope to the floor.
        let drop = b - floor;
        let height = RISE_PART * drop;
        let slope = (drop - height) / (1000.0 * tan);
        let rise = RISE_KM_PER_M * height;
        let t = ((down - slope) / rise).clamp(0.0, 1.0);
        let apron = floor + height * (1.0 - t) * (1.0 - t);
        Shape {
            level: smooth_max(upper, apron, 0.25 * height + 1.0),
            down,
            slope,
            rise,
            t,
        }
    }

    /// The level of the margin at a place, in meters, with the level `old`
    /// from before the stroke, in meters.
    pub(crate) fn profile(&self, at: Place, old: f64) -> f64 {
        let w = self.spec.shelf_km;
        // The distance from the coast with the wander of the shelf break.
        // Near the coast the wander fades, so the coast does not move.
        let x = at.d - self.wander(at.p) * smoothstep(0.0, 0.5 * w, at.d);
        let tan = self.slope(at.coast);
        let Shape {
            level: base,
            down,
            slope,
            rise,
            t,
        } = self.shape(x, tan);
        let on_slope = smoothstep(0.0, 0.15 * slope, down)
            * (1.0 - smoothstep(0.7 * slope, slope + 0.15 * rise, down));
        let deep = GULLY_DEPTH * (tan / SLOPE_DEG.to_radians().tan()).sqrt();
        let mut level = base - deep * on_slope * self.gullies(at);
        level += self.scars(at, down, slope, rise);
        level -= self.canyons(at, down, slope, rise);
        // The fine relief is low on the shelf and high on the rise, and it
        // fades near the coast.
        let rough = (0.3 + 0.7 * smoothstep(0.0, slope, down)) * smoothstep(0.0, 0.3 * w, at.d);
        level += rough * self.relief(at.p);
        // The margin fades into the floor from before the stroke at the end
        // of the rise.
        let fade = smoothstep(RISE_FADE, 1.0, t);
        (level + (old - level) * fade).min(TOP_M.max(old))
    }

    /// The new level of a texel with the level `start` from before the
    /// stroke and the level `now`, at the unit direction `p`. The land does
    /// not change.
    pub(crate) fn level(&self, start: u16, now: u16, p: V3) -> u16 {
        let old = f64::from(start);
        if to_meters(old) > 0.0 {
            return now;
        }
        let chord = length(add(p, scale(self.center, -1.0)));
        let k = 2.0 * (0.5 * chord).min(1.0).asin() / self.radius;
        if k >= 1.0 {
            return now;
        }
        let weight = falloff(k, self.hardness) * self.flow;
        if weight <= 0.0 {
            return now;
        }
        let Some(at) = self.place(p) else {
            return now;
        };
        let target = to_level(self.profile(at, to_meters(old)));
        let cur = f64::from(now);
        let new = (cur + ((target - cur) * weight + 0.5).floor()).clamp(0.0, 65535.0);
        self.spec.limits.apply(cur, new) as u16
    }
}

/// The smooth minimum of two numbers, with a rounding of `k`.
pub(crate) fn smooth_min(a: f64, b: f64, k: f64) -> f64 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}

/// The smooth maximum of two numbers, with a rounding of `k`.
pub(crate) fn smooth_max(a: f64, b: f64, k: f64) -> f64 {
    -smooth_min(-a, -b, k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::meters_to_level;
    use crate::math::{angle, lonlat_to_dir};

    const KM: f64 = 6371.0;

    fn spec() -> Margin {
        Margin {
            shelf_m: -130.0,
            shelf_km: 100.0,
            world_km: KM,
            seed: 7,
            limits: Limits::default(),
        }
    }

    /// Land at 100 m west of longitude `coast` on face 0 and between the
    /// latitudes -25 and 25, and a floor at 4,500 m elsewhere.
    fn coast(n: usize, coast: f64) -> Heightmap {
        let mut map = Heightmap::new(n, meters_to_level(-4500.0));
        for y in 0..n {
            for x in 0..n {
                let d = map.texel_dir(0, x, y);
                let lon = d[1].atan2(d[0]).to_degrees();
                let lat = d[2].asin().to_degrees();
                if lon < coast && lat.abs() < 25.0 {
                    map.set(0, x, y, meters_to_level(100.0));
                }
            }
        }
        map
    }

    fn levels(map: &Heightmap) -> Vec<u16> {
        let n = map.face_size();
        let mut out = Vec::new();
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    out.push(map.get(face, x as i64, y as i64));
                }
            }
        }
        out
    }

    /// A stroke of stamps from `a` to `b` in degrees of longitude and
    /// latitude, in one undo step.
    fn paint(map: &mut Heightmap, spec: Margin, a: (f64, f64), b: (f64, f64), radius: f64) {
        let mut stroke = MarginStroke::new(spec, map.face_size());
        map.begin_stroke();
        let count = 24;
        for i in 0..=count {
            let t = i as f64 / count as f64;
            let p = lonlat_to_dir(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            let plan = stroke.stamp(map, p, radius, 0.6, 1.0);
            map.margin(&plan);
        }
        map.end_stroke();
    }

    fn plan_of(map: &Heightmap) -> MarginPlan {
        MarginStroke::new(spec(), map.face_size()).stamp(
            map,
            lonlat_to_dir(3.0, 0.0),
            0.05,
            0.5,
            1.0,
        )
    }

    #[test]
    fn the_distance_transform_finds_the_nearest_site() {
        let size = 23;
        let sites: Vec<bool> = (0..size * size)
            .map(|k| k % 37 == 5 || k == 300 || (k % size < 7 && k / size > 4))
            .collect();
        let (squared, nearest) = distance_transform(&sites, size);
        for k in 0..size * size {
            let (i, j) = ((k % size) as f64, (k / size) as f64);
            let best = (0..size * size)
                .filter(|&s| sites[s])
                .map(|s| {
                    let (si, sj) = ((s % size) as f64, (s / size) as f64);
                    (i - si).powi(2) + (j - sj).powi(2)
                })
                .fold(f64::MAX, f64::min);
            assert_eq!(squared[k], best, "{k}");
            let s = nearest[k];
            let (si, sj) = ((s % size) as f64, (s / size) as f64);
            assert_eq!((i - si).powi(2) + (j - sj).powi(2), best);
        }
        let (squared, nearest) = distance_transform(&[false; 16], 4);
        assert!(squared.iter().all(|&d| d == f64::MAX));
        assert!(nearest.iter().all(|&s| s == usize::MAX));
    }

    #[test]
    fn the_field_gives_the_distance_to_the_land() {
        let map = coast(512, 0.0);
        let plan = plan_of(&map);
        for lon in [1.0, 3.0, 6.0] {
            let (d, away) = plan.field.sample(lonlat_to_dir(lon, 0.0)).unwrap();
            let expected = lon.to_radians() * KM;
            assert!((d - expected).abs() < 25.0, "{lon}: {d} {expected}");
            // Away from the land is east.
            assert!(dot(away, [0.0, 1.0, 0.0]) > 0.95, "{away:?}");
        }
        // The nearest point of the coast is on the coast.
        let at = plan.place(lonlat_to_dir(4.0, 2.0)).unwrap();
        let coast_lon = at.coast[1].atan2(at.coast[0]).to_degrees();
        assert!(coast_lon.abs() < 0.3, "{coast_lon}");
        assert!((plan.field.floor_m + 4500.0).abs() < 1.0);
    }

    #[test]
    fn the_shelf_goes_down_gently_to_the_shelf_break() {
        let map = coast(256, 0.0);
        let plan = plan_of(&map);
        let tan = SLOPE_DEG.to_radians().tan();
        let at_break = plan.shape(100.0, tan).level;
        assert!((at_break + 130.0).abs() < 10.0, "{at_break}");
        assert!(plan.shape(0.0, tan).level.abs() < 1.0);
        let mut last = 0.0;
        for k in 1..=20 {
            let h = plan.shape(k as f64 * 5.0, tan).level;
            let grade = (last - h) / 5000.0;
            assert!(h < last && grade < 0.003, "{k} {h}");
            last = h;
        }
    }

    #[test]
    fn the_shelf_depth_sets_the_depth_of_the_break() {
        let map = coast(256, 0.0);
        let mut plan = plan_of(&map);
        plan.spec.shelf_m = -400.0;
        let h = plan.shape(100.0, SLOPE_DEG.to_radians().tan()).level;
        assert!((h + 400.0).abs() < 10.0, "{h}");
        let stroke = MarginStroke::new(
            Margin {
                shelf_m: -5.0,
                ..spec()
            },
            256,
        );
        assert_eq!(stroke.spec().shelf_m, SHELF_DEPTH_RANGE.1);
    }

    #[test]
    fn the_slope_falls_at_about_4_degrees_with_variation_from_1_to_10() {
        let map = coast(256, 0.0);
        let plan = plan_of(&map);
        let angles: Vec<f64> = (0..4000)
            .map(|k| {
                let coast = lonlat_to_dir(0.0, -40.0 + k as f64 * 0.02);
                plan.slope(coast).atan().to_degrees()
            })
            .collect();
        let lo = angles.iter().copied().fold(f64::MAX, f64::min);
        let hi = angles.iter().copied().fold(0.0, f64::max);
        let mut sorted = angles.clone();
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() / 2];
        assert!(lo >= 1.0 && hi <= 10.0, "{lo} {hi}");
        assert!(lo < 1.5 && hi > 8.0, "{lo} {hi}");
        assert!((3.0..5.5).contains(&median), "{median}");
        // The profile has that slope below the break.
        for deg in [1.5, 4.0, 9.0] {
            let tan = f64::to_radians(deg).tan();
            let (a, b) = (plan.shape(102.0, tan).level, plan.shape(104.0, tan).level);
            let measured = ((a - b) / 2000.0).atan().to_degrees();
            assert!((measured - deg).abs() < 0.05 * deg, "{deg}: {measured}");
        }
    }

    #[test]
    fn the_rise_has_a_gentle_gradient_and_meets_the_floor_with_no_step() {
        let map = coast(256, 0.0);
        let plan = plan_of(&map);
        for deg in [2.0, 4.0, 8.0] {
            let tan = f64::to_radians(deg).tan();
            let at = |x: f64| plan.shape(x, tan);
            let s = at(200.0);
            let foot = 100.0 + s.slope;
            for k in 0..=20 {
                let t = 1.0 / 6.0 + k as f64 / 20.0 * (2.0 / 3.0 - 1.0 / 6.0);
                let x = foot + t * s.rise;
                let grade = (at(x).level - at(x + 1.0).level) / 1000.0;
                assert!(
                    (1.0 / 700.0..1.0 / 100.0).contains(&grade),
                    "{deg} {t} {grade}"
                );
            }
            // No step at the floor.
            let end = foot + s.rise;
            for k in 0..20 {
                let h = at(end - 5.0 + k as f64).level;
                assert!((h + 4500.0).abs() < 10.0, "{h}");
            }
        }
    }

    /// The level in meters at a place.
    fn meters(map: &Heightmap, lon: f64, lat: f64) -> f64 {
        to_meters(map.level_between(lonlat_to_dir(lon, lat), true))
    }

    #[test]
    fn the_coast_sets_the_side_and_the_land_does_not_change() {
        for (land_east, sign) in [(false, 1.0), (true, -1.0)] {
            let n = 512;
            let mut map = coast(n, 0.0);
            if land_east {
                // The land is east of the meridian.
                for y in 0..n {
                    for x in 0..n {
                        let d = map.texel_dir(0, x, y);
                        let lon = d[1].atan2(d[0]).to_degrees();
                        let lat = d[2].asin().to_degrees();
                        let m = if lon > 0.0 && lat.abs() < 25.0 {
                            100.0
                        } else {
                            -4500.0
                        };
                        map.set(0, x, y, meters_to_level(m));
                    }
                }
            }
            let before = levels(&map);
            paint(
                &mut map,
                spec(),
                (sign * 2.0, -8.0),
                (sign * 2.0, 8.0),
                0.05,
            );
            let after = levels(&map);
            for (a, b) in after.iter().zip(&before) {
                if to_meters(f64::from(*b)) > 0.0 {
                    assert_eq!(a, b, "the land changed");
                } else {
                    assert!(to_meters(f64::from(*a)) <= 0.0, "the sea became land");
                }
            }
            // The shelf is on the sea side of the coast, wherever the land is.
            let shelf = meters(&map, sign * 0.4, 0.0);
            let deep = meters(&map, sign * 4.0, 0.0);
            assert!(shelf > -100.0 && shelf < 0.0, "{land_east}: {shelf}");
            assert!(deep < -1500.0, "{land_east}: {deep}");
        }
    }

    #[test]
    fn the_shelf_break_wanders() {
        let map = coast(256, 0.0);
        let plan = plan_of(&map);
        let w: Vec<f64> = (0..1000)
            .map(|k| plan.wander(lonlat_to_dir(2.0, -40.0 + k as f64 * 0.08)))
            .collect();
        let lo = w.iter().copied().fold(f64::MAX, f64::min);
        let hi = w.iter().copied().fold(f64::MIN, f64::max);
        assert!(lo < -8.0 && hi > 8.0, "{lo} {hi}");
        assert!(lo > -30.0 && hi < 30.0, "{lo} {hi}");
    }

    #[test]
    fn the_slope_has_gullies_canyons_and_slide_scars_with_debris() {
        let map = coast(4096, 0.0);
        let plan = plan_of(&map);
        let (slope, rise) = (30.0, 400.0);
        let mut gullies = 0;
        let mut was = false;
        let (mut canyon, mut cut, mut debris) = (0.0f64, 0.0f64, 0.0f64);
        for k in 0..20_000 {
            let coast = lonlat_to_dir(0.0, -10.0 + k as f64 * 0.001);
            let at = Place {
                d: 115.0,
                p: coast,
                coast,
            };
            let deep = plan.gullies(at) > 0.4;
            gullies += usize::from(deep && !was);
            was = deep;
            canyon = canyon.max(plan.canyons(at, 15.0, slope, rise));
            cut = cut.min(plan.scars(at, 0.6 * slope, slope, rise));
            debris = debris.max(plan.scars(at, slope + 0.1 * rise, slope, rise));
            // No scar reaches the shelf.
            assert_eq!(plan.scars(at, -5.0, slope, rise), 0.0);
        }
        // 2,224 km of coast with a gully at each spacing, in pieces.
        let spacings = (2224.0 / plan.gully_km()) as usize;
        assert!(
            gullies > spacings / 6 && gullies < 2 * spacings,
            "{gullies}"
        );
        assert!(canyon > 0.5 * CANYON_DEPTH, "{canyon}");
        assert!(
            cut < -0.3 * SCAR_DEPTH && debris > 0.1 * SCAR_DEPTH,
            "{cut} {debris}"
        );
    }

    #[test]
    fn the_ground_outside_the_brush_does_not_change() {
        let mut map = coast(256, 0.0);
        let before = levels(&map);
        let radius = 0.05;
        paint(&mut map, spec(), (2.0, -8.0), (2.0, 8.0), radius);
        let after = levels(&map);
        let n = map.face_size();
        let centers: Vec<V3> = (0..=240)
            .map(|i| lonlat_to_dir(2.0, -8.0 + i as f64 / 15.0))
            .collect();
        let mut changed = 0;
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    let k = (face * n + y) * n + x;
                    if after[k] == before[k] {
                        continue;
                    }
                    changed += 1;
                    let p = map.texel_dir(face, x, y);
                    let near = centers
                        .iter()
                        .map(|&c| angle(c, p))
                        .fold(f64::MAX, f64::min);
                    assert!(near < radius + 0.01, "{near}");
                }
            }
        }
        assert!(changed > 100, "{changed}");
    }

    #[test]
    fn a_second_stroke_keeps_the_margin() {
        // Under the hard part of the brush, the first stroke gives the
        // margin, and a second stroke changes it little.
        let mut map = coast(256, 0.0);
        paint(&mut map, spec(), (2.0, -8.0), (2.0, 8.0), 0.05);
        let once = levels(&map);
        paint(&mut map, spec(), (2.0, -8.0), (2.0, 8.0), 0.05);
        let twice = levels(&map);
        let n = map.face_size();
        let mut most = 0.0f64;
        for y in 0..n {
            for x in 0..n {
                let p = map.texel_dir(0, x, y);
                let lon = p[1].atan2(p[0]).to_degrees();
                let lat = p[2].asin().to_degrees();
                if (lon - 2.0).abs() > 1.2 || lat.abs() > 6.0 {
                    continue;
                }
                let k = y * n + x;
                let change = to_meters(f64::from(once[k])) - to_meters(f64::from(twice[k]));
                most = most.max(change.abs());
            }
        }
        assert!(most < 5.0, "{most}");
    }

    #[test]
    fn two_equal_strokes_with_the_same_seed_give_equal_results() {
        let run = |seed| {
            let mut map = coast(256, 0.0);
            paint(
                &mut map,
                Margin { seed, ..spec() },
                (2.0, -8.0),
                (2.0, 8.0),
                0.05,
            );
            levels(&map)
        };
        assert_eq!(run(3), run(3));
        assert_ne!(run(3), run(4));
    }

    #[test]
    fn one_undo_step_puts_back_the_ground_from_before_a_margin() {
        let mut map = coast(256, 0.0);
        let before = levels(&map);
        paint(&mut map, spec(), (2.0, -8.0), (2.0, 8.0), 0.05);
        assert_ne!(levels(&map), before);
        map.undo();
        assert_eq!(levels(&map), before);
        assert!(!map.can_undo());
    }
}
