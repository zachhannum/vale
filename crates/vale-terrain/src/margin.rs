//! The passive margin brush.
//!
//! The brush reshapes a rough shelf that the user paints into the edge of a
//! continent that rifted from another continent: a shallow shelf, a steeper
//! slope, and a gentle rise at the foot of the slope. Each stamp moves the
//! sea floor under the brush toward the margin profile. The profile is a
//! function of the distance from the painted shelf edge, the line where the
//! painted ground crosses the edge depth, a little below the shelf break.
//! The slope crosses that depth on the line, so the outline of the painted
//! shelf stays, and the land does not change.
//!
//! A stroke reads a field of the distance to the shelf edge in a window
//! around the brush. The patterns along the edge come from the nearest
//! point of the edge, so a second stroke over the same place gives the same
//! margin.

use std::cell::Cell;
use std::collections::VecDeque;
use std::f64::consts::FRAC_PI_2;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

use crate::cube::{ELEV_MAX, ELEV_MIN, FACES, face_of, warp};
use crate::erode::{ErodeBrush, ErodeStep, Erosion};
use crate::flow::Window;
use crate::heightmap::{Heightmap, Limits, TexelRect, brush_rect, falloff, noise3};
use crate::line::smoothstep;
use crate::math::{V3, add, cross, dot, length, normalize, scale};

mod canyons;

/// The limits of the depth of the shelf break, in meters.
pub const SHELF_DEPTH_RANGE: (f64, f64) = (-1000.0, -20.0);

/// The mean angle of the slope, in degrees (Pratson and Haxby 1996).
pub(crate) const SLOPE_DEG: f64 = 4.0;

/// The least and the largest angle of the slope, in degrees.
pub(crate) const SLOPE_RANGE_DEG: (f64, f64) = (1.0, 10.0);

/// The length of the features of the slope angle along the shelf edge, in
/// kilometers.
const ANGLE_KM: f64 = 200.0;

/// The length of the small features of the slope angle along the shelf
/// edge, in kilometers, and their part of the exponent of the angle.
const ANGLE_SMALL_KM: f64 = 50.0;
const ANGLE_SMALL: f64 = 0.8;

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

/// The depth of the shelf edge below the shelf break, as a part of the
/// drop from the break to the floor. The edge is in the middle of the
/// slope, and the slide scars are below it, so a second stroke finds the
/// same edge.
const EDGE_PART: f64 = 0.3;

/// The fine relief fades over this distance from the land, in kilometers.
const RELIEF_FADE_KM: f64 = 30.0;

/// The distance from the shelf edge where the field looks for the land, in
/// kilometers.
const SHELF_REACH_KM: f64 = 200.0;

/// The distance to the land or to the shelf edge of a cell with none in the
/// window, in kilometers.
const FAR_KM: f64 = 1.0e6;

/// The largest number of cells along one side of the erosion window of a
/// margin stroke. A gully is a few cells wide, so the cells must be small
/// for narrow gullies.
pub const MARGIN_CELLS: usize = 1024;

/// The room around the stamps of a margin stroke in its erosion window, as
/// a part of the width of the stamps.
const ERODE_ROOM: f64 = 0.15;

/// The size of the features of the slide scars along the coast, in
/// kilometers, and the least size, in texels.
const SCAR_KM: f64 = 70.0;
const SCAR_TEXELS: f64 = 48.0;

/// The largest depth of a slide scar, in meters.
const SCAR_DEPTH: f64 = 260.0;

/// The height of the debris below a scar, as a part of the depth of the
/// scar.
const DEBRIS_PART: f64 = 0.45;

/// The sizes of the features of the fine relief, in kilometers, and their
/// heights on the rise, in meters.
const RELIEF: [(f64, f64); 4] = [(40.0, 20.0), (16.0, 15.0), (7.0, 10.0), (3.5, 6.0)];

/// The number of cells along one side of a distance field, at most.
pub const FIELD_CELLS: usize = 384;

/// The number of numbers of a cell of a distance field.
pub const FIELD_FLOATS: usize = 4;

/// The distance in cells from the nearest land cell where the field looks
/// for the coast.
const SEARCH: usize = 1;

/// The half width of the blur of the directions of a field, in cells.
const BLUR: usize = 3;

/// The distance from the first stamp of a window where the stamps can be,
/// in brush radii.
const PAINT: f64 = 4.0;

/// The largest half width of a window, in radians.
const MAX_HALF: f64 = 0.6;

const SALT_ANGLE: u32 = 0x736c_6f70;
const SALT_SCAR: u32 = 0x7363_6172;
const SALT_RELIEF: u32 = 0x7265_6c66;

/// The settings of a margin stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Margin {
    /// The depth of the shelf break, in meters.
    pub shelf_m: f64,
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
            world_km: self.world_km.max(1.0),
            ..self
        }
    }
}

/// The distances to the land and to the shelf edge in a window: a square
/// grid of cells on the plane that touches the sphere at `center`. A cell
/// holds the distances in kilometers and the direction away from the shelf
/// edge on the plane.
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
    /// For each cell, row by row from the south: the distance to the land,
    /// the signed distance to the shelf edge in kilometers, positive on the
    /// deep side, and the east and north parts of the direction toward the
    /// deep side from the shelf edge. A cell with no land in the window has
    /// the distance to the land `FAR_KM`. A cell on the deep side with no
    /// shelf edge in the window has the distances `f32::MAX`.
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

/// The distance from the shelf edge where the margin ends, at most, in
/// kilometers, for a floor of `floor_m`.
fn reach_km(spec: &Margin, floor_m: f64) -> f64 {
    let drop = spec.shelf_m - floor_m;
    let height = RISE_PART * drop;
    let tan = SLOPE_RANGE_DEG.0.to_radians().tan();
    SHELF_REACH_KM + (drop - height) / (1000.0 * tan) + RISE_KM_PER_M * height
}

/// The edge depth, in meters. The shelf edge is where the painted ground
/// crosses it.
fn edge_m(spec: &Margin, floor_m: f64) -> f64 {
    spec.shelf_m - EDGE_PART * (spec.shelf_m - floor_m)
}

/// The depth of the deep floor in the levels of a window, in meters: the
/// depth that `FLOOR_PART` of the ground deeper than `LEAST_DROP` below the
/// shelf break is below.
fn floor_of(spec: &Margin, levels: &[f64]) -> f64 {
    let deep = spec.shelf_m - LEAST_DROP;
    let mut out: Vec<f64> = levels.iter().copied().filter(|&m| m < deep).collect();
    if out.is_empty() {
        return deep;
    }
    out.sort_by(f64::total_cmp);
    out[(FLOOR_PART * out.len() as f64) as usize]
}

/// What a distance field needs from the heightmap: the levels at the cell
/// centers and the pieces of the coast. `MarginInputs::build` makes the
/// field from it, on any thread.
#[derive(Clone, Debug)]
pub struct MarginInputs {
    spec: Margin,
    plane: Plane,
    paint: f64,
    floor_m: Option<f64>,
    levels: Vec<f64>,
    pieces: Pieces,
}

impl MarginField {
    /// Reads the field around `center` from the ground of `map`, for stamps
    /// up to `paint` radians from `center`. See `MarginInputs::read` and
    /// `MarginInputs::build`.
    pub fn read(
        map: &Heightmap,
        spec: &Margin,
        center: V3,
        paint: f64,
        floor_m: Option<f64>,
    ) -> MarginField {
        MarginInputs::read(map, spec, center, paint, floor_m).build()
    }
}

impl MarginInputs {
    /// Reads what the field around `center` needs from the ground of
    /// `map`, for stamps up to `paint` radians from `center`, with a depth
    /// of the floor or none.
    pub fn read(
        map: &Heightmap,
        spec: &Margin,
        center: V3,
        paint: f64,
        floor_m: Option<f64>,
    ) -> MarginInputs {
        let spec = spec.clamped();
        let center = normalize(center);
        let east = cross([0.0, 0.0, 1.0], center);
        let east = if length(east) < 1e-12 {
            [0.0, 1.0, 0.0]
        } else {
            normalize(east)
        };
        let north = cross(center, east);
        // The window holds the stamps, and the shelf edge and the land that
        // a margin under them can reach.
        let km = spec.world_km;
        let half = (paint + reach_km(&spec, floor_m.unwrap_or(ELEV_MIN)) / km).min(MAX_HALF);
        let half = half.tan();
        let texel = FRAC_PI_2 / map.face_size() as f64;
        let size = ((2.0 * half / texel).ceil() as usize).clamp(16, FIELD_CELLS);
        let cell = 2.0 * half / size as f64;
        let plane = Plane {
            center,
            east,
            north,
            half,
            cell,
            size,
        };
        let levels = par_map(size * size, |k| {
            let at = plane.dir((k % size) as f64, (k / size) as f64);
            to_meters(f64::from(nearest_level(map, at)))
        });
        let land: Vec<bool> = levels.iter().map(|&m| m > 0.0).collect();
        let pieces = coast_segments(map, &plane, &land);
        MarginInputs {
            spec,
            plane,
            paint,
            floor_m,
            levels,
            pieces,
        }
    }

    /// The distance field. The land is the ground above the sea level, and
    /// the shelf edge is the line where the ground crosses the edge depth. With no depth of the floor, the field also reads it
    /// with `floor_of`.
    pub fn build(self) -> MarginField {
        let MarginInputs {
            spec,
            plane,
            paint,
            floor_m,
            levels,
            pieces,
        } = self;
        let Plane {
            center,
            east,
            north,
            cell,
            size,
            ..
        } = plane;
        let km = spec.world_km;
        let deep = spec.shelf_m - LEAST_DROP;
        let floor_m = floor_m
            .unwrap_or_else(|| floor_of(&spec, &levels))
            .min(deep);
        let land: Vec<bool> = levels.iter().map(|&m| m > 0.0).collect();
        // The coast, and the shelf edge: the line where the painted ground
        // crosses the edge depth.
        let exact = reach_km(&spec, floor_m) / (cell * km);
        let coast = nearest_points(&land, &pieces, size, exact);
        let depth = edge_m(&spec, floor_m);
        let shallow: Vec<bool> = levels.iter().map(|&m| m >= depth).collect();
        let deep_side: Vec<bool> = shallow.iter().map(|&l| !l).collect();
        let edge = level_segments(&levels, size, depth);
        let outside = nearest_points(&shallow, &edge, size, exact);
        let inside = nearest_points(&deep_side, &edge, size, exact);
        let distance = |k: usize, q: (f64, f64)| {
            let here = plane.dir((k % size) as f64, (k / size) as f64);
            dot(here, plane.dir(q.0, q.1)).clamp(-1.0, 1.0).acos() * km
        };
        let cells = par_map(size * size, |k| {
            let d = match coast[k] {
                _ if land[k] => 0.0,
                Some(q) => distance(k, q),
                None => FAR_KM,
            };
            let (i, j) = ((k % size) as f64, (k / size) as f64);
            // The signed distance to the shelf edge, and the direction
            // toward the deep side from it.
            let (s, away) = match (shallow[k], outside[k], inside[k]) {
                (false, Some(q), _) => (distance(k, q), (i - q.0, j - q.1)),
                (true, _, Some(q)) => (-distance(k, q), (q.0 - i, q.1 - j)),
                (true, _, None) => (-FAR_KM, (0.0, 0.0)),
                (false, None, _) => (f64::MAX, (0.0, 0.0)),
            };
            let size = away.0.hypot(away.1).max(1e-9);
            if s == f64::MAX {
                [f32::MAX, f32::MAX, 0.0, 0.0]
            } else {
                [
                    d as f32,
                    s as f32,
                    (away.0 / size) as f32,
                    (away.1 / size) as f32,
                ]
            }
        });
        let mut cells: Vec<f32> = cells.into_iter().flatten().collect();
        smooth_directions(&mut cells, size);
        MarginField {
            id: FIELD_IDS.fetch_add(1, Ordering::Relaxed),
            center,
            east,
            north,
            size,
            cell,
            floor_m,
            cells,
            paint,
        }
    }

    /// The same inputs with another depth of the floor.
    fn with_floor(self, floor_m: Option<f64>) -> MarginInputs {
        MarginInputs { floor_m, ..self }
    }
}

impl MarginField {
    /// Whether a stamp at `center` with a radius in radians is inside the
    /// part of the window for stamps.
    pub fn covers(&self, center: V3, radius: f64) -> bool {
        let a = dot(normalize(center), self.center).clamp(-1.0, 1.0).acos();
        a + radius <= self.paint
    }

    /// The distance to the land and the signed distance to the shelf edge
    /// in kilometers, and the direction toward the deep side from the shelf
    /// edge, at a unit direction, from the 4 cells around it. A direction
    /// outside the window, or near a cell with no shelf edge in the window,
    /// gives `None`.
    pub(crate) fn sample(&self, p: V3) -> Option<(f64, f64, V3)> {
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
        let mut out = [0.0; FIELD_FLOATS];
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
        let away = add(scale(self.east, out[2]), scale(self.north, out[3]));
        Some((out[0], out[1], away))
    }
}

/// The level of the texel nearest to a unit direction, from before the
/// stroke. Thus the fields of a stroke do not depend on its earlier stamps.
fn nearest_level(map: &Heightmap, d: V3) -> u16 {
    let n = map.face_size();
    let (face, a, b) = face_of(d);
    let at = |v: f64| (((warp(v) + 1.0) * 0.5 * n as f64).floor() as usize).min(n - 1);
    map.level_before(face, at(a), at(b))
}

/// A square grid of cells on the plane that touches the sphere at
/// `center`.
#[derive(Clone, Debug)]
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

/// The pieces of a line, in cells, in the list of the cell that holds the
/// middle of each piece.
type Pieces = Vec<Vec<[(f64, f64); 2]>>;

/// Adds a piece to the list of the cell that holds its middle.
fn add_piece(out: &mut Pieces, size: usize, a: (f64, f64), b: (f64, f64)) {
    let (mx, my) = ((0.5 * (a.0 + b.0)).round(), (0.5 * (a.1 + b.1)).round());
    let last = (size - 1) as f64;
    if (0.0..=last).contains(&mx) && (0.0..=last).contains(&my) {
        out[my as usize * size + mx as usize].push([a, b]);
    }
}

/// The pieces of the coast near the coast cells of a plane, in cells. A
/// piece joins the middles of the sides between a land texel and a sea
/// texel in a square of four texels of `map`. Thus the coast depends only on
/// which texels are land, and not on the cells or on the depth of the sea.
/// A coast cell is a cell with land and sea among it and the cells around
/// it.
fn coast_segments(map: &Heightmap, plane: &Plane, land: &[bool]) -> Pieces {
    let size = plane.size;
    let n = map.face_size();
    let coast_cells: Vec<usize> = (0..size * size)
        .filter(|&k| {
            let (i, j) = (k % size, k / size);
            (i.saturating_sub(1)..(i + 2).min(size))
                .flat_map(|x| (j.saturating_sub(1)..(j + 2).min(size)).map(move |y| y * size + x))
                .any(|q| land[q] != land[k])
        })
        .collect();
    // The pieces of the squares of texels near each coast cell. A square
    // near two coast cells gives its pieces two times, and that changes no
    // distance.
    let found = par_map(coast_cells.len(), |c| {
        let k = coast_cells[c];
        let middle = plane.dir((k % size) as f64, (k / size) as f64);
        let mut pieces = Vec::new();
        for face in 0..FACES {
            let Some(rect) = brush_rect(n, face, middle, 0.75 * plane.cell) else {
                continue;
            };
            for y in rect.y0..rect.y1.min(n - 1) {
                for x in rect.x0..rect.x1.min(n - 1) {
                    let is_land = |dx: usize, dy: usize| {
                        let level = map.get(face, (x + dx) as i64, (y + dy) as i64);
                        to_meters(f64::from(level)) > 0.0
                    };
                    let land = [is_land(0, 0), is_land(1, 0), is_land(1, 1), is_land(0, 1)];
                    if land.iter().all(|&l| l) || land.iter().all(|&l| !l) {
                        continue;
                    }
                    let corners = [(0, 0), (1, 0), (1, 1), (0, 1)]
                        .map(|(dx, dy)| map.texel_dir(face, x + dx, y + dy));
                    let mut points = Vec::new();
                    for e in 0..4 {
                        if land[e] != land[(e + 1) % 4] {
                            let middle = normalize(add(corners[e], corners[(e + 1) % 4]));
                            points.extend(plane.place(middle));
                        }
                    }
                    for pair in points.as_chunks::<2>().0 {
                        pieces.push((pair[0], pair[1]));
                    }
                }
            }
        }
        pieces
    });
    let mut out = vec![Vec::new(); size * size];
    for (a, b) in found.into_iter().flatten() {
        add_piece(&mut out, size, a, b);
    }
    out
}

/// The pieces of the line where `values` crosses `level`, in cells, from
/// the values at the cell centers.
fn level_segments(values: &[f64], size: usize, level: f64) -> Pieces {
    let mut out = vec![Vec::new(); size * size];
    for j in 0..size - 1 {
        for i in 0..size - 1 {
            let corner = |di: usize, dj: usize| {
                let (x, y) = (i + di, j + dj);
                ((x as f64, y as f64), values[y * size + x] - level)
            };
            let ring = [corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)];
            let mut points = Vec::new();
            for e in 0..4 {
                let ((p0, v0), (p1, v1)) = (ring[e], ring[(e + 1) % 4]);
                if (v0 >= 0.0) != (v1 >= 0.0) {
                    let t = v0 / (v0 - v1);
                    points.push((p0.0 + (p1.0 - p0.0) * t, p0.1 + (p1.1 - p0.1) * t));
                }
            }
            for pair in points.as_chunks::<2>().0 {
                add_piece(&mut out, size, pair[0], pair[1]);
            }
        }
    }
    out
}

/// The nearest point of `pieces` to each cell that is not a site, in cells,
/// near the nearest site. Farther than `exact` cells from the nearest site,
/// or with no piece near, the nearest point is half a cell from the center
/// of the nearest site, toward the cell. With no site, it is `None`.
fn nearest_points(
    sites: &[bool],
    pieces: &Pieces,
    size: usize,
    exact: f64,
) -> Vec<Option<(f64, f64)>> {
    let (_, nearest) = distance_transform(sites, size);
    par_map(size * size, |k| {
        if sites[k] || nearest[k] == usize::MAX {
            return None;
        }
        let (i, j) = ((k % size) as f64, (k / size) as f64);
        let (si, sj) = (nearest[k] % size, nearest[k] / size);
        let (dx, dy) = (i - si as f64, j - sj as f64);
        let away = dx.hypot(dy);
        let edge = (si as f64 + 0.5 * dx / away, sj as f64 + 0.5 * dy / away);
        if away > exact {
            return Some(edge);
        }
        let mut best = (f64::MAX, edge);
        for qj in sj.saturating_sub(SEARCH)..(sj + SEARCH + 1).min(size) {
            for qi in si.saturating_sub(SEARCH)..(si + SEARCH + 1).min(size) {
                for &[a, b] in &pieces[qj * size + qi] {
                    let q = nearest_on_segment((i, j), a, b);
                    let d = (i - q.0).hypot(j - q.1);
                    if d < best.0 {
                        best = (d, q);
                    }
                }
            }
        }
        Some(best.1)
    })
}

/// Smooths the directions toward the sea of the cells of a field, with two
/// box blurs of `BLUR` cells on each side. The direction to the nearest
/// point of a line jumps where two parts of the line are at the same
/// distance, and the patterns along the coast would break there.
fn smooth_directions(cells: &mut [f32], size: usize) {
    let used = |c: &[f32]| c[1] < f32::MAX && (c[2] != 0.0 || c[3] != 0.0);
    for _ in 0..2 {
        for axis in 0..2 {
            let before = cells.to_vec();
            let blurred = par_map(size * size, |k| {
                let (i, j) = (k % size, k / size);
                if !used(&before[k * FIELD_FLOATS..]) {
                    return None;
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
                    if used(&before[q..]) {
                        x += f64::from(before[q + 2]);
                        y += f64::from(before[q + 3]);
                    }
                }
                let length = x.hypot(y);
                (length > 1e-9).then(|| [(x / length) as f32, (y / length) as f32])
            });
            for (k, v) in blurred.into_iter().enumerate() {
                if let Some([x, y]) = v {
                    cells[k * FIELD_FLOATS + 2] = x;
                    cells[k * FIELD_FLOATS + 3] = y;
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
    let envelope = |f: Vec<f64>| {
        let mut out = vec![(inf, 0usize); size];
        lower_envelope(&f, &mut out);
        out
    };
    // The columns first: the nearest site in the same column.
    let columns = par_map(size, |i| {
        envelope(
            (0..size)
                .map(|j| if sites[j * size + i] { 0.0 } else { inf })
                .collect(),
        )
    });
    // Then the rows, from the columns.
    let rows = par_map(size, |j| {
        envelope((0..size).map(|i| columns[i][j].0).collect())
    });
    let mut squared = vec![inf; size * size];
    let mut nearest = vec![usize::MAX; size * size];
    for (j, row) in rows.iter().enumerate() {
        for (i, &(d, at)) in row.iter().enumerate() {
            if d < inf {
                squared[j * size + i] = d;
                nearest[j * size + i] = columns[at][j].1 * size + at;
            }
        }
    }
    (squared, nearest)
}

/// `f` of each number below `count`, in order, on the threads of the
/// device.
fn par_map<T: Send>(count: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = count.div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let f = &f;
        let parts: Vec<_> = (0..count)
            .step_by(chunk)
            .map(|start| {
                scope.spawn(move || {
                    (start..(start + chunk).min(count))
                        .map(f)
                        .collect::<Vec<T>>()
                })
            })
            .collect();
        parts
            .into_iter()
            .flat_map(|part| part.join().expect("a worker thread of the field"))
            .collect()
    })
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
#[derive(Debug)]
pub struct MarginStroke {
    spec: Margin,
    face_size: usize,
    /// The fields of the stroke, in the order of their windows. A field that
    /// the worker thread still builds is `None`.
    fields: Vec<Option<Arc<MarginField>>>,
    /// The center and the part for stamps of the last window.
    last: Option<(V3, f64)>,
    /// The stamps that wait for their field, in order, with the number of
    /// the field.
    waiting: VecDeque<(Stamp, usize)>,
    worker: Option<Worker>,
}

/// The thread that builds the fields of a stroke in order. Each field gets
/// the depth of the floor of the field before it.
#[derive(Debug)]
struct Worker {
    jobs: mpsc::Sender<MarginInputs>,
    done: mpsc::Receiver<Arc<MarginField>>,
}

/// One stamp of a stroke before it has its field.
#[derive(Clone, Copy, Debug)]
struct Stamp {
    center: V3,
    radius: f64,
    hardness: f64,
    flow: f64,
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
            fields: Vec::new(),
            last: None,
            waiting: VecDeque::new(),
            worker: None,
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
    /// ground of the stroke so far: a stroke does not change the land. Use
    /// `stamp` or `stamp_later` in one stroke, not both.
    pub fn stamp(
        &mut self,
        map: &Heightmap,
        center: V3,
        radius: f64,
        hardness: f64,
        flow: f64,
    ) -> MarginPlan {
        let stamp = self.stamp_of(center, radius, hardness, flow);
        let window = self.window(map, stamp, true);
        let field = self.fields[window].clone().expect("the field is there");
        self.plan(stamp, field)
    }

    /// Adds a stamp as `stamp` does, but a new field builds on a worker
    /// thread, and the stamp waits for it. Returns the plans of the stamps
    /// that have their fields, in order.
    pub fn stamp_later(
        &mut self,
        map: &Heightmap,
        center: V3,
        radius: f64,
        hardness: f64,
        flow: f64,
    ) -> Vec<MarginPlan> {
        let stamp = self.stamp_of(center, radius, hardness, flow);
        let window = self.window(map, stamp, false);
        self.waiting.push_back((stamp, window));
        self.poll()
    }

    /// The plans of the waiting stamps that have their fields now, in
    /// order.
    pub fn poll(&mut self) -> Vec<MarginPlan> {
        if let Some(worker) = &self.worker {
            while let Ok(field) = worker.done.try_recv() {
                let slot = self.fields.iter_mut().find(|f| f.is_none());
                *slot.expect("a field that waits") = Some(field);
            }
        }
        self.ready()
    }

    /// Waits for the fields of all the waiting stamps, and returns their
    /// plans in order.
    pub fn finish(&mut self) -> Vec<MarginPlan> {
        if let Some(worker) = &self.worker {
            while let Some(slot) = self.fields.iter_mut().find(|f| f.is_none()) {
                let field = worker.done.recv().expect("the worker of the stroke");
                *slot = Some(field);
            }
        }
        self.ready()
    }

    /// Whether a stamp waits for its field.
    pub fn is_waiting(&self) -> bool {
        !self.waiting.is_empty()
    }

    fn ready(&mut self) -> Vec<MarginPlan> {
        let mut out = Vec::new();
        while let Some(&(stamp, window)) = self.waiting.front() {
            let Some(field) = self.fields[window].clone() else {
                break;
            };
            self.waiting.pop_front();
            out.push(self.plan(stamp, field));
        }
        out
    }

    fn stamp_of(&self, center: V3, radius: f64, hardness: f64, flow: f64) -> Stamp {
        let step = FRAC_PI_2 / self.face_size as f64;
        Stamp {
            center: normalize(center),
            radius: radius.max(2.0 * step),
            hardness: hardness.clamp(0.0, 0.999),
            flow: flow.clamp(0.0, 1.0),
        }
    }

    /// The number of the field of a stamp. The last window holds the stamp,
    /// or the stroke reads a new window around it. With `now`, the field of
    /// a new window builds at once.
    fn window(&mut self, map: &Heightmap, stamp: Stamp, now: bool) -> usize {
        if let Some((center, paint)) = self.last {
            let a = dot(stamp.center, center).clamp(-1.0, 1.0).acos();
            if a + stamp.radius <= paint {
                return self.fields.len() - 1;
            }
        }
        let paint = PAINT * stamp.radius;
        let inputs = MarginInputs::read(map, &self.spec, stamp.center, paint, None);
        self.last = Some((stamp.center, paint));
        if now {
            let floor = self
                .fields
                .last()
                .map(|field| field.as_ref().expect("the fields are there").floor_m);
            let field = inputs.with_floor(floor).build();
            self.fields.push(Some(Arc::new(field)));
        } else {
            let worker = self.worker.get_or_insert_with(|| {
                let (jobs, inbox) = mpsc::channel::<MarginInputs>();
                let (outbox, done) = mpsc::channel();
                std::thread::spawn(move || {
                    let mut floor = None;
                    for inputs in inbox {
                        let field = inputs.with_floor(floor).build();
                        floor = Some(field.floor_m);
                        if outbox.send(Arc::new(field)).is_err() {
                            break;
                        }
                    }
                });
                Worker { jobs, done }
            });
            worker.jobs.send(inputs).expect("the worker of the stroke");
            self.fields.push(None);
        }
        self.fields.len() - 1
    }

    fn plan(&self, stamp: Stamp, field: Arc<MarginField>) -> MarginPlan {
        let n = self.face_size;
        MarginPlan {
            spec: self.spec,
            field,
            center: stamp.center,
            radius: stamp.radius,
            hardness: stamp.hardness,
            flow: stamp.flow,
            face_size: n,
            rects: std::array::from_fn(|face| brush_rect(n, face, stamp.center, stamp.radius)),
        }
    }
}

/// The place of a texel for the profile.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Place {
    /// The distance to the land, and the signed distance to the shelf edge,
    /// in kilometers.
    pub d: f64,
    pub s: f64,
    /// The unit direction of the texel, and of the nearest point of the
    /// shelf edge.
    pub p: V3,
    pub edge: V3,
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
        let (d, s, away) = self.field.sample(p)?;
        // The nearest point of the shelf edge.
        let tangent = add(away, scale(p, -dot(away, p)));
        let size = length(tangent);
        let edge = if size < 1e-6 {
            p
        } else {
            let a = s / self.spec.world_km;
            normalize(add(scale(p, a.cos()), scale(tangent, -a.sin() / size)))
        };
        Some(Place { d, s, p, edge })
    }

    fn noise(&self, q: V3, size_km: f64, salt: u32) -> V3 {
        noise3(
            scale(q, self.spec.world_km / size_km),
            self.spec.seed ^ salt,
        )
    }

    /// The slope angle at a point of the shelf edge, as a tangent.
    pub(crate) fn slope(&self, edge: V3) -> f64 {
        let large = self.noise(edge, ANGLE_KM, SALT_ANGLE)[0];
        let small = self.noise(edge, ANGLE_SMALL_KM, SALT_ANGLE)[1];
        let n = (1.4 * large).clamp(-1.0, 1.0) + ANGLE_SMALL * small;
        let deg = (SLOPE_DEG * 4f64.powf(n)).clamp(SLOPE_RANGE_DEG.0, SLOPE_RANGE_DEG.1);
        deg.to_radians().tan()
    }

    /// The change of the slide scars and their debris, in meters, at a
    /// place `down` kilometers below the shelf break, with a slope and a rise
    /// of these widths in kilometers.
    pub(crate) fn scars(&self, at: Place, down: f64, slope: f64, rise: f64) -> f64 {
        let texel = self.texel_km();
        let size = SCAR_KM.max(SCAR_TEXELS * texel);
        let n = self.noise(at.edge, size, SALT_SCAR);
        let here = smoothstep(0.25, 0.6, n[0]);
        let depth = SCAR_DEPTH * (0.3 + 0.35 * (n[1] + 1.0));
        let head = slope * (0.55 + 0.1 * (n[2] + 1.0));
        // The scar: a steep headwall, and a floor that runs out at the foot
        // of the slope.
        let cut = smoothstep(head, head + 0.08 * slope + 0.5 * texel, down)
            * (1.0 - smoothstep(0.85 * slope, slope + 0.05 * rise + texel, down));
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

    /// The shape of the profile at a distance `d` from the land and a
    /// signed distance `x` from the shelf edge in kilometers, with a slope
    /// angle with the tangent `tan`.
    pub(crate) fn shape(&self, d: f64, x: f64, tan: f64) -> Shape {
        let b = self.spec.shelf_m;
        let floor = self.field.floor_m;
        // The shelf break is on the shelf side of the edge, so the slope
        // crosses the edge depth on the edge. The shelf goes down from
        // the coast to the break, and the slope falls from the break.
        let drop = b - floor;
        let down = x + EDGE_PART * drop / (1000.0 * tan);
        let part = if down >= 0.0 { 1.0 } else { d / (d - down) };
        let upper = smooth_min(b * part, b - 1000.0 * tan * down, BREAK_ROUND);
        // The rise is a parabola from the foot of the slope to the floor.
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
    /// from before the stroke, in meters, with the fine relief if `relief`.
    pub(crate) fn profile(&self, at: Place, old: f64, relief: bool) -> f64 {
        let Shape {
            level: base,
            down,
            slope,
            rise,
            t,
        } = self.shape(at.d, at.s, self.slope(at.edge));
        let mut level = base;
        level += self.scars(at, down, slope, rise);
        // The fine relief is low on the shelf and high on the rise, and it
        // fades near the coast.
        let rough =
            (0.3 + 0.7 * smoothstep(0.0, slope, down)) * smoothstep(0.0, RELIEF_FADE_KM, at.d);

        if relief {
            level += rough * self.relief(at.p);
        }
        // The margin fades into the floor from before the stroke at the end
        // of the rise.
        let fade = smoothstep(RISE_FADE, 1.0, t);
        (level + (old - level) * fade).min(TOP_M.max(old))
    }

    /// The new level of a texel with the level `start` from before the
    /// stroke and the level `now`, at the unit direction `p`. The land does
    /// not change.
    pub(crate) fn level(&self, start: u16, now: u16, p: V3) -> u16 {
        self.level_with(start, now, p, true)
    }

    /// The level of `level`, with the fine relief if `relief`.
    fn level_with(&self, start: u16, now: u16, p: V3, relief: bool) -> u16 {
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
        let target = to_level(self.profile(at, to_meters(old), relief));
        let cur = f64::from(now);
        let new = (cur + ((target - cur) * weight + 0.5).floor()).clamp(0.0, 65535.0);
        self.spec.limits.apply(cur, new) as u16
    }
}

/// The window of the cells of the erosion of a margin stroke: the stamps
/// of the plans and room around them for the flow, with at most
/// `MARGIN_CELLS` cells along one side. The cells are as small as that
/// gives.
pub fn margin_window(plans: &[MarginPlan], face_size: usize) -> Window {
    let n = face_size;
    let sum = plans
        .iter()
        .fold([0.0; 3], |sum, plan| add(sum, plan.center));
    let center = normalize(sum);
    let reach = plans
        .iter()
        .map(|plan| dot(plan.center, center).clamp(-1.0, 1.0).acos() + plan.radius)
        .fold(0.0, f64::max);
    let texels = 2.0 * reach * n as f64 / FRAC_PI_2 * (1.0 + ERODE_ROOM);
    let mut cell = 1;
    while (texels / cell as f64).ceil() as usize > MARGIN_CELLS && 2 * cell <= n / 8 {
        cell *= 2;
    }
    let cells = MARGIN_CELLS
        .min((texels / cell as f64).ceil() as usize + 2)
        .min(n / cell);
    let mut window = Window::centered(center, n, cell, cells);
    // The window ends less than half a face past the face edge.
    let step = cell as i64;
    let past = (n as i64 / 2 - 1) / step * step;
    let last = n as i64 + past - (cells * cell) as i64;
    window.x0 = window.x0.clamp(-past, last);
    window.y0 = window.y0.clamp(-past, last);
    window
}

/// Cuts the canyons of the margin of a stroke, as `canyons::carve` gives
/// them. `erosion` holds the cells of the ground from before the stroke.
/// The cells under the stamps of the plans get the margin with no fine
/// relief, stamp by stamp, and then the canyons cut it. Thus the drops hold
/// the canyons, and the fine relief of the stamps stays. The land does not
/// change. `amount` from 0 to 1 sets the rate. Returns the drops: the
/// margin of the cells minus the cut margin.
pub fn erode_margin(erosion: &mut Erosion, plans: &[MarginPlan], amount: f64) -> ErodeStep {
    let brushes: Vec<ErodeBrush> = plans
        .iter()
        .map(|plan| ErodeBrush {
            center: plan.center,
            radius: plan.radius,
            hardness: plan.hardness,
            flow: plan.flow,
        })
        .collect();
    let Some(first) = plans.first() else {
        return erosion.drops_since(erosion.levels(), &brushes);
    };
    let start = erosion.levels().to_vec();
    let dirs: Vec<Cell<Option<V3>>> = start.iter().map(|_| Cell::new(None)).collect();
    for (plan, brush) in plans.iter().zip(&brushes) {
        erosion.lift_cells(std::slice::from_ref(brush), |i, dir, now| {
            dirs[i].set(Some(dir));
            Some(plan.level_with(start[i], now, dir, false))
        });
    }
    let dirs: Vec<Option<V3>> = dirs.into_iter().map(Cell::into_inner).collect();
    let weights: Vec<f32> = dirs
        .iter()
        .map(|dir| {
            let Some(dir) = *dir else {
                return 0.0;
            };
            brushes
                .iter()
                .map(|b| {
                    let chord = length(add(dir, scale(b.center, -1.0)));
                    let k = 2.0 * (0.5 * chord).min(1.0).asin() / b.radius;
                    if k >= 1.0 {
                        0.0
                    } else {
                        falloff(k, b.hardness) * b.flow
                    }
                })
                .fold(0.0, f64::max) as f32
        })
        .collect();
    let margin = erosion.levels().to_vec();
    let mut heights: Vec<f32> = margin
        .iter()
        .map(|&l| to_meters(f64::from(l)) as f32)
        .collect();
    let spec = first.spec;
    let grid = canyons::Grid {
        side: erosion.side(),
        cell_km: erosion.cell_angle() * spec.world_km,
        dirs: &dirs,
        weights: &weights,
        world_km: spec.world_km,
        seed: spec.seed,
        shelf_m: spec.shelf_m,
        branching: erosion.branching(),
    };
    canyons::carve(&grid, &mut heights, amount);
    let kept: Vec<u16> = margin
        .iter()
        .zip(&heights)
        .map(|(&before, &h)| {
            let new = to_level(f64::from(h)).round().clamp(0.0, f64::from(before));
            spec.limits.apply(f64::from(before), new) as u16
        })
        .collect();
    erosion.drops_since_to(&margin, &kept, &brushes)
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
    use crate::flow::WindowHeights;
    use crate::math::{angle, lonlat_to_dir};

    const KM: f64 = 6371.0;

    fn spec() -> Margin {
        Margin {
            shelf_m: -130.0,
            world_km: KM,
            seed: 7,
            limits: Limits::default(),
        }
    }

    /// Ground on face 0 with the level in meters that `f` gives for a
    /// longitude and a latitude in degrees, and a floor at 4,500 m on the
    /// other faces.
    fn painted(n: usize, f: impl Fn(f64, f64) -> f64) -> Heightmap {
        let mut map = Heightmap::new(n, meters_to_level(-4500.0));
        for y in 0..n {
            for x in 0..n {
                let d = map.texel_dir(0, x, y);
                let lon = d[1].atan2(d[0]).to_degrees();
                let lat = d[2].asin().to_degrees();
                map.set(0, x, y, meters_to_level(f(lon, lat)));
            }
        }
        map
    }

    /// The ground of a rough painted shelf: land at 100 m west of
    /// longitude `coast` and between the latitudes -25 and 25, a flat shelf
    /// at 150 m for 1 degree east of the land, and a floor at 4,500 m.
    fn rough(coast: f64, lon: f64, lat: f64) -> f64 {
        match lon - coast {
            _ if lat.abs() >= 25.0 => -4500.0,
            x if x < 0.0 => 100.0,
            x if x < 1.0 => -150.0,
            _ => -4500.0,
        }
    }

    fn coast(n: usize, coast: f64) -> Heightmap {
        painted(n, |lon, lat| rough(coast, lon, lat))
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

    /// The signed distance from the shelf edge to the shelf break of
    /// `plan`, in kilometers, for a slope angle with the tangent `tan`.
    fn break_x(plan: &MarginPlan, tan: f64) -> f64 {
        let b = plan.spec.shelf_m;
        -(b - edge_m(&plan.spec, plan.field.floor_m)) / (1000.0 * tan)
    }

    #[test]
    fn the_field_gives_the_distances_to_the_land_and_to_the_shelf_edge() {
        let map = coast(512, 0.0);
        let plan = plan_of(&map);
        for lon in [0.5, 1.0, 3.0, 6.0] {
            let (d, s, away) = plan.field.sample(lonlat_to_dir(lon, 0.0)).unwrap();
            let expected = lon.to_radians() * KM;
            assert!((d - expected).abs() < 25.0, "{lon}: {d} {expected}");
            // The shelf edge is 1 degree from the coast.
            let expected = (lon - 1.0).to_radians() * KM;
            assert!((s - expected).abs() < 25.0, "{lon}: {s} {expected}");
            // Away from the shelf is east.
            assert!(dot(away, [0.0, 1.0, 0.0]) > 0.95, "{away:?}");
        }
        // The nearest point of the shelf edge is on the shelf edge.
        let at = plan.place(lonlat_to_dir(4.0, 2.0)).unwrap();
        let edge_lon = at.edge[1].atan2(at.edge[0]).to_degrees();
        assert!((edge_lon - 1.0).abs() < 0.3, "{edge_lon}");
        assert!((plan.field.floor_m + 4500.0).abs() < 1.0);
    }

    #[test]
    fn the_slope_crosses_the_edge_depth_on_the_shelf_edge() {
        let map = coast(256, 0.0);
        let plan = plan_of(&map);
        let depth = edge_m(&plan.spec, plan.field.floor_m);
        for deg in [1.5, 4.0, 9.0] {
            let tan = f64::to_radians(deg).tan();
            let h = plan.shape(110.0, 0.0, tan).level;
            assert!((h - depth).abs() < 0.05 * depth.abs(), "{deg}: {h}");
        }
    }

    #[test]
    fn the_shelf_goes_down_gently_to_the_shelf_break() {
        let map = coast(256, 0.0);
        let plan = plan_of(&map);
        let tan = SLOPE_DEG.to_radians().tan();
        // The land is 100 km from the break.
        let x0 = break_x(&plan, tan) - 100.0;
        let at_break = plan.shape(100.0, x0 + 100.0, tan).level;
        assert!((at_break + 130.0).abs() < 10.0, "{at_break}");
        assert!(plan.shape(0.0, x0, tan).level.abs() < 1.0);
        let mut last = 0.0;
        for k in 1..=20 {
            let x = k as f64 * 5.0;
            let h = plan.shape(x, x0 + x, tan).level;
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
        let tan = SLOPE_DEG.to_radians().tan();
        let h = plan.shape(100.0, break_x(&plan, tan), tan).level;
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
            let x0 = break_x(&plan, tan);
            let level = |x| plan.shape(100.0 + x, x0 + x, tan).level;
            let (a, b) = (level(2.0), level(4.0));
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
            let x0 = break_x(&plan, tan) - 100.0;
            let at = |x: f64| plan.shape(x, x0 + x, tan);
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
    fn the_painted_shelf_sets_the_side_and_the_land_does_not_change() {
        for (land_east, sign) in [(false, 1.0), (true, -1.0)] {
            let n = 512;
            // With the land east of the meridian, the shelf is west of it.
            let mut map = painted(n, |lon, lat| rough(0.0, sign * lon, lat));
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

    /// The longitude in degrees where the ground along a latitude first
    /// goes below the edge depth east of the coast.
    fn edge_at(map: &Heightmap, lat: f64) -> f64 {
        let depth = edge_m(&spec(), -4500.0);
        (0..2000)
            .map(|k| k as f64 * 0.002)
            .find(|&lon| meters(map, lon, lat) < depth)
            .unwrap_or(f64::MAX)
    }

    #[test]
    fn the_shelf_keeps_the_outline_that_the_user_painted() {
        // A rough shelf with a deep inlet and a wide spur.
        let n = 1024;
        let mut map = painted(n, |lon, lat| {
            let inlet = lat.abs() < 0.5 && lon > 0.4;
            let spur = (3.0..4.0).contains(&lat) && (0.0..1.8).contains(&lon);
            match rough(0.0, lon, lat) {
                m if m < 0.0 && inlet => -4500.0,
                m if m < 0.0 && spur => -150.0,
                m => m,
            }
        });
        let rows = [-3.0, 0.0, 3.5];
        let before = rows.map(|lat| edge_at(&map, lat));
        assert!(
            (before[0] - 1.0).abs() < 0.1
                && (before[1] - 0.4).abs() < 0.1
                && (before[2] - 1.8).abs() < 0.1,
            "{before:?}"
        );
        paint(&mut map, spec(), (1.0, -6.0), (1.0, 6.0), 0.05);
        let after = rows.map(|lat| edge_at(&map, lat));
        for (a, b) in after.iter().zip(&before) {
            let km = (a - b).abs().to_radians() * KM;
            assert!(km < 15.0, "{before:?} {after:?}");
        }
        // The shelf goes down to the break, and the deep side falls to the
        // floor.
        let shelf = meters(&map, 0.3, -3.0);
        let foot = meters(&map, 3.0, -3.0);
        assert!(shelf > -130.0 && shelf < -20.0, "{shelf}");
        assert!(foot < -3000.0, "{foot}");
    }

    /// The time to read a field on a large face.
    #[test]
    #[ignore = "manual"]
    fn manual_time_of_a_field() {
        let n = 8192;
        let mut map = Heightmap::new(n, meters_to_level(-4500.0));
        let half = 1200;
        for y in n / 2 - half..n / 2 + half {
            for x in n / 2 - half..n / 2 + half {
                let d = map.texel_dir(0, x, y);
                let lon = d[1].atan2(d[0]).to_degrees() + 2.0 * (d[2] * 40.0).sin();
                if lon < 0.0 {
                    map.set(0, x, y, meters_to_level(100.0));
                }
            }
        }
        let t = std::time::Instant::now();
        let field = MarginField::read(&map, &spec(), lonlat_to_dir(1.0, 0.0), 0.04, None);
        eprintln!("{:?} for {} cells", t.elapsed(), field.size);
    }

    #[test]
    fn the_slope_has_slide_scars_with_debris() {
        let map = coast(4096, 0.0);
        let plan = plan_of(&map);
        let (slope, rise) = (30.0, 400.0);
        let (mut cut, mut debris) = (0.0f64, 0.0f64);
        for k in 0..20_000 {
            let edge = lonlat_to_dir(0.0, -10.0 + k as f64 * 0.001);
            let at = Place {
                d: 115.0,
                s: 15.0,
                p: edge,
                edge,
            };

            cut = cut.min(plan.scars(at, 0.84 * slope, slope, rise));
            debris = debris.max(plan.scars(at, slope + 0.1 * rise, slope, rise));
            // No scar reaches the shelf.
            assert_eq!(plan.scars(at, -5.0, slope, rise), 0.0);
        }
        assert!(
            cut < -0.3 * SCAR_DEPTH && debris > 0.1 * SCAR_DEPTH,
            "{cut} {debris}"
        );
    }

    /// A margin stroke along the coast of `coast(n, 0.0)` with its erosion,
    /// and the drops of the erosion in meters at places in degrees.
    fn eroded(n: usize) -> (Heightmap, Heightmap, impl Fn(f64, f64) -> f64) {
        let before = coast(n, 0.0);
        let mut stroke = MarginStroke::new(spec(), n);
        let plans: Vec<MarginPlan> = (0..=40)
            .flat_map(|i| [0.8, 1.6, 2.4].map(|lon| (lon, -3.0 + 0.15 * f64::from(i))))
            .map(|(lon, lat)| stroke.stamp(&before, lonlat_to_dir(lon, lat), 0.012, 0.7, 1.0))
            .collect();
        let window = margin_window(&plans, n);
        let mut erosion = Erosion::window_alone(&WindowHeights::new(&before, window));
        erosion.set_branching(7, 0.5);
        let step = erode_margin(&mut erosion, &plans, 0.5);
        let paint = |erode: bool| {
            let mut map = coast(n, 0.0);
            map.begin_stroke();
            for plan in &plans {
                map.margin(plan);
            }
            if erode {
                map.erode(&step);
            }
            map.end_stroke();
            map
        };
        let (plain, map) = (paint(false), paint(true));
        let drop = move |lon: f64, lat: f64| meters(&plain, lon, lat) - meters(&map, lon, lat);
        let map = paint(true);
        (before, map, drop)
    }

    #[test]
    fn the_erosion_cuts_channels_in_the_slope_and_keeps_the_land() {
        let (before, map, drop) = eroded(2048);
        let (old, new) = (levels(&before), levels(&map));
        for (a, b) in new.iter().zip(&old) {
            if to_meters(f64::from(*b)) > 0.0 {
                assert_eq!(a, b, "the land changed");
            }
        }
        // The channels of the slope: the places along the coast where the
        // drop is deeper than 10 m and than at the places next to them.
        let drops: Vec<f64> = (0..1000)
            .map(|k| drop(1.0, -2.0 + k as f64 * 0.004))
            .collect();
        let deepest = drops.iter().copied().fold(0.0, f64::max);
        let channels = drops
            .windows(3)
            .filter(|w| w[1] > 10.0 && w[1] >= w[0] && w[1] > w[2])
            .count();
        assert!(channels >= 5 && deepest > 50.0, "{channels} {deepest}");
        // The shelf keeps most of its shape.
        let shelf = (0..400)
            .map(|k| drop(0.25, -2.0 + k as f64 * 0.01))
            .fold(0.0, f64::max);
        assert!(shelf < 0.5 * deepest, "{shelf} {deepest}");
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
        // A second stroke reads the shelf edge from the margin of the first
        // stroke. The edge moves by less than a field cell, and more strokes
        // do not move it farther.
        let mut map = coast(1024, 0.0);
        let rows = [-3.6, -1.0, 0.0, 2.0, 4.0];
        paint(&mut map, spec(), (2.0, -8.0), (2.0, 8.0), 0.05);
        let once = rows.map(|lat| edge_at(&map, lat));
        for _ in 0..4 {
            paint(&mut map, spec(), (2.0, -8.0), (2.0, 8.0), 0.05);
        }
        let later = rows.map(|lat| edge_at(&map, lat));
        for (a, b) in once.iter().zip(&later) {
            let km = (a - b).abs().to_radians() * KM;
            assert!(km < 3.0, "{once:?} {later:?}");
        }
    }

    #[test]
    fn a_stroke_with_fields_on_a_worker_gives_the_same_margin() {
        let mut now = coast(256, 0.0);
        let mut later = coast(256, 0.0);
        let mut stroke = MarginStroke::new(spec(), 256);
        later.begin_stroke();
        let mut plans = 0;
        for i in 0..=40 {
            let p = lonlat_to_dir(2.0, -20.0 + i as f64);
            for plan in stroke.stamp_later(&later, p, 0.03, 0.6, 1.0) {
                later.margin(&plan);
                plans += 1;
            }
        }
        for plan in stroke.finish() {
            later.margin(&plan);
            plans += 1;
        }
        later.end_stroke();
        assert_eq!(plans, 41);
        assert!(!stroke.is_waiting());
        paint_line(&mut now, spec(), 0.03);
        assert_eq!(levels(&now), levels(&later));
    }

    /// The stroke of `a_stroke_with_fields_on_a_worker_gives_the_same_margin`
    /// with `stamp`.
    fn paint_line(map: &mut Heightmap, spec: Margin, radius: f64) {
        let mut stroke = MarginStroke::new(spec, map.face_size());
        map.begin_stroke();
        for i in 0..=40 {
            let p = lonlat_to_dir(2.0, -20.0 + i as f64);
            let plan = stroke.stamp(map, p, radius, 0.6, 1.0);
            map.margin(&plan);
        }
        map.end_stroke();
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
