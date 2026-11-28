//! The plateau brush.
//!
//! A stroke keeps its stamps. The growth of a point is 1 minus the product
//! of 1 minus the weight of each stamp there, so each stamp grows the
//! plateau under the brush. For each stamp, the CPU marks the cells of a
//! grid around the stamp that are in the plateau and finds the distance of
//! each cell to the edge. A cell is in the plateau if the fault cell that
//! holds it has grown, so the edge runs along straight faults. A texel gets
//! the height of the plateau from that distance, so the slopes are soft and
//! have no steps. A texel moves from its level from before the stroke
//! toward the top of the plateau, and it does not go down. Thus a second
//! stroke over the plateau changes nothing, and two strokes side by side
//! join.

use std::collections::HashMap;
use std::f64::consts::FRAC_PI_2;

use crate::cube::{ELEV_MIN, FACES};
use crate::heightmap::{Heightmap, Limits, TexelRect, brush_rect, hash, noise3};
use crate::line::{smoothstep, unit};
use crate::margin::smooth_min;
use crate::math::{V3, add, dot, length, normalize, scale};
use crate::seamount::{LEVELS_PER_M, gdh1_depth};

/// The default height of the top above the floor when the plateau forms,
/// in meters, and the range of the setting.
pub const PLATEAU_HEIGHT_M: f64 = 4000.0;
pub const PLATEAU_HEIGHT_RANGE: std::ops::RangeInclusive<f64> = 500.0..=8000.0;

/// The default age of the plateau in millions of years.
pub const PLATEAU_AGE_MYR: f64 = 120.0;

/// The number of cells along one side of the distance grid of a plan, at
/// most.
pub const PLATEAU_FIELD_CELLS: usize = 160;

/// The number of 32-bit numbers before the grid in `PlateauPlan::floats`,
/// and the largest number of a plan.
pub const PLATEAU_HEADER: usize = 24;
pub const PLATEAU_FLOATS: usize = PLATEAU_HEADER + PLATEAU_FIELD_CELLS * PLATEAU_FIELD_CELLS;

/// The settings of a plateau stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plateau {
    /// The height of the top above the floor when the plateau forms, in
    /// meters.
    pub height_m: f64,
    /// The age of the plateau in millions of years.
    pub age_myr: f64,
    pub world_km: f64,
    pub seed: u32,
    pub limits: Limits,
}

/// The top sinks by this part of the GDH1 curve from age 0, because the
/// thick crust is light and the plate under it is only partly hot.
const SINK: f64 = 0.5;

/// The top keeps at least this part of the height.
const KEEP: f64 = 0.2;

/// The floor is this percentile of the ground in a window of `FLOOR_REACH`
/// brush radii.
const FLOOR_PERCENTILE: f64 = 0.1;
const FLOOR_REACH: f64 = 6.0;

/// The top stays below a ceiling in meters, under sea level, and it rounds
/// into the ceiling over a height in meters. Thus a plateau never reaches
/// the sea surface.
const CEILING_M: f64 = -50.0;
const CEILING_ROUND_M: f64 = 300.0;

/// The warp of the growth: each octave moves a point by up to this part of
/// its wavelength. The first wavelength is 2 brush radii, and the last is
/// 3 texels. The warp moves a point by at most `WARP_REACH` brush radii.
const WARP: f64 = 0.08;
const WARP_TEXELS: f64 = 3.0;
const WARP_REACH: f64 = 0.56;

/// A cell of the grid is in the plateau if the growth there is above
/// `GROWN`, and the growth at the center of its fault cell is above
/// `CELL_GROWN`. A fault cell is a cell of a seeded Voronoi field. Its size
/// is a part of the brush radius, at least a number of texels and at most a
/// size in kilometers, and it is this many times as long along a seeded
/// axis of the world.
const GROWN: f64 = 0.15;
const CELL_GROWN: f64 = 0.6;
const CELL_PART: f64 = 0.45;
const CELL_TEXELS: f64 = 8.0;
const CELL_MAX_KM: f64 = 150.0;
const CELL_STRETCH: f64 = 1.6;

/// The distances are blurred this many times over 3 by 3 cells, so the
/// corners of the faults are round.
const BLURS: usize = 3;

/// The edge of the plateau is a foot and a long ramp, as the edges of the
/// Manihiki Plateau are. The foot rises to `FOOT` of the height over
/// `SCARP` times the height of the top, and it steps up in 3 fault blocks.
/// The ramp rises to the top with a length of `RAMP` times the height of
/// the top, and at most `RAMP_WARP` brush radii, so a small plateau still
/// reaches its top.
const SCARP: f64 = 8.0;
const FOOT: f64 = 0.35;
const RAMP: f64 = 24.0;
const RAMP_WARP: f64 = 0.25;
const FAULTS: [(f64, f64, f64); 3] = [(0.45, 0.0, 0.22), (0.35, 0.4, 0.58), (0.2, 0.8, 0.95)];
/// The faults move by up to this part of the foot along the edge, with
/// this wavelength in kilometers.
const FAULT_SHIFT: f64 = 0.25;
const FAULT_KM: f64 = 60.0;

/// A fractal noise moves the edge, so the outline is uneven at small
/// scales. The first octave has this wavelength in kilometers, and each
/// next octave has half the wavelength and `EDGE_GAIN` of the amplitude,
/// down to 3 texels. The sum of the octaves is at most `EDGE_REACH` brush
/// radii.
const EDGE_KM: f64 = 160.0;
const EDGE_GAIN: f64 = 0.5;
const EDGE_REACH: f64 = 0.2;

/// The relief of the top in meters, with the wavelength of each octave in
/// kilometers.
const RELIEF: [(f64, f64); 3] = [(300.0, 250.0), (120.0, 60.0), (40.0, 15.0)];

/// The salts of the seeded fields.
const SALT_WARP: u32 = 0x5a17_0000;
const SALT_FAULT: u32 = 0x5a17_1000;
const SALT_AXIS: u32 = 0x5a17_3000;
const SALT_RELIEF: u32 = 0x5a17_4000;
const SALT_EDGE: u32 = 0x5a17_6000;
const SALT_CELL: u32 = 0x5a17_7000;

/// The slots of the header of `PlateauPlan::floats`. The grid is on the
/// plane that touches the sphere at `CENTER`, and `CELL` is the width of a
/// cell on the plane at distance 1 from the center of the sphere. A cell
/// holds the signed distance to the edge in kilometers, positive in the
/// plateau, from `-FAR_KM` to `FAR_KM`, row by row from the south.
const FLOOR_LEVEL: usize = 0;
const TOP_LEVEL: usize = 1;
const TOP_M: usize = 2;
const WORLD_KM: usize = 3;
const SEED_LOW: usize = 4;
const SEED_HIGH: usize = 5;
const WARP_KM: usize = 6;
const TEXEL_KM: usize = 7;
const SIZE: usize = 8;
const AXIS: usize = 9;
const CENTER: usize = 12;
const EAST: usize = 15;
const NORTH: usize = 18;
const CELL: usize = 21;
const FAR_KM: usize = 22;

/// The height of the top above the floor at an age, for a height when the
/// plateau formed. The top sinks by the GDH1 curve.
pub fn plateau_top_m(height_m: f64, age_myr: f64) -> f64 {
    let sink = SINK * (gdh1_depth(age_myr.max(0.0)) - gdh1_depth(0.0));
    (height_m - sink).max(KEEP * height_m)
}

/// The stamps of a plateau stroke, and its floor.
#[derive(Clone, Debug)]
pub struct PlateauStroke {
    spec: Plateau,
    face_size: usize,
    /// The floor of the stroke in meters. The first stamp sets it.
    floor_m: Option<f64>,
    /// The brush radius of the first stamp, in radians. It sets the warp.
    warp: f64,
    stamps: Vec<[f64; 6]>,
}

/// The texels that a stamp of a plateau stroke changes, and what they need.
#[derive(Clone, Debug)]
pub struct PlateauPlan {
    pub limits: Limits,
    pub face_size: usize,
    pub rects: [Option<TexelRect>; FACES],
    floats: Vec<f32>,
}

/// The lengths of a stroke in kilometers that set the reach of a stamp.
struct Reach {
    top_km: f64,
    /// The distance from the edge where the height reaches the top, and
    /// past which the distances of the grid stop.
    far_km: f64,
}

impl PlateauStroke {
    /// Starts a stroke on a heightmap with `face_size` texels along one side
    /// of a face.
    pub fn new(spec: Plateau, face_size: usize) -> PlateauStroke {
        let spec = Plateau {
            height_m: spec.height_m.max(1.0),
            age_myr: spec.age_myr.max(0.0),
            world_km: spec.world_km.max(1.0),
            ..spec
        };
        PlateauStroke {
            spec,
            face_size,
            floor_m: None,
            warp: 0.0,
            stamps: Vec::new(),
        }
    }

    /// The floor of the stroke in meters. The first stamp sets it.
    pub fn floor_m(&self) -> Option<f64> {
        self.floor_m
    }

    fn reach(&self) -> Reach {
        let top_km = plateau_top_m(self.spec.height_m, self.spec.age_myr) / 1000.0;
        let ramp_km = (RAMP * top_km).min(RAMP_WARP * self.warp * self.spec.world_km);
        Reach {
            top_km,
            far_km: SCARP * top_km + 3.0 * ramp_km,
        }
    }

    /// Grows the plateau under a brush with its center at unit direction
    /// `center`, a radius in radians, a hardness, and a flow from 0 to 1.
    /// `map` holds the ground from before the stroke. Returns the plan of
    /// the texels that change.
    pub fn stamp(
        &mut self,
        map: &Heightmap,
        center: V3,
        radius: f64,
        hardness: f64,
        flow: f64,
    ) -> PlateauPlan {
        let n = self.face_size;
        let step = FRAC_PI_2 / n as f64;
        let radius = radius.max(2.0 * step);
        if self.floor_m.is_none() {
            self.floor_m = Some(plateau_floor(map, center, FLOOR_REACH * radius));
            self.warp = radius;
        }
        let mut plan = PlateauPlan {
            limits: self.spec.limits,
            face_size: n,
            rects: [None; FACES],
            floats: Vec::new(),
        };
        let flow = flow.clamp(0.0, 1.0);
        if flow <= 0.0 {
            return plan;
        }
        let center = normalize(center);
        self.stamps.push([
            center[0],
            center[1],
            center[2],
            radius,
            hardness.clamp(0.0, 0.999),
            flow,
        ]);
        let world = self.spec.world_km;
        let reach = self.reach();
        // The grid can grow within `grows` of the center of the stamp, and
        // the edge moves there. A texel within `far` of a moved edge can
        // change, and its distance needs the grid within `far` of it.
        let grows = radius + WARP_REACH * self.warp;
        let far = reach.far_km / world;
        let edge = (EDGE_REACH * self.warp).max(FAULT_SHIFT * SCARP * reach.top_km / world);
        // A fault cell that a stamp grows can reach this far from the
        // place where the growth changes.
        let cell_reach = 1.5 * CELL_STRETCH * self.fault_km() / world;
        let changes = grows + cell_reach + far + edge;
        plan.rects = std::array::from_fn(|face| brush_rect(n, face, center, changes));
        let half = changes + far;
        let cell = (2.0 * half / (PLATEAU_FIELD_CELLS - 1) as f64).max(step);
        let size = ((2.0 * half / cell).ceil() as usize + 1).min(PLATEAU_FIELD_CELLS);
        let near: Vec<usize> = (0..self.stamps.len())
            .filter(|&i| {
                let s = self.stamps[i];
                chord_angle([s[0], s[1], s[2]], center) < 1.5 * half + s[3] + WARP_REACH * self.warp
            })
            .collect();

        let top_m = reach.top_km * 1000.0;
        let floor_m = self.floor_m.unwrap_or(0.0);
        let seed = self.spec.seed;
        let axis = fault_axis(seed);
        let (east, north) = tangent_frame(center);
        let mut floats = vec![0.0f32; PLATEAU_HEADER];
        floats[FLOOR_LEVEL] = ((floor_m - ELEV_MIN) * LEVELS_PER_M) as f32;
        floats[TOP_LEVEL] = (top_m * LEVELS_PER_M) as f32;
        floats[TOP_M] = top_m as f32;
        floats[WORLD_KM] = world as f32;
        floats[SEED_LOW] = (seed & 0xffff) as f32;
        floats[SEED_HIGH] = (seed >> 16) as f32;
        floats[WARP_KM] = (self.warp * world) as f32;
        floats[TEXEL_KM] = (step * world) as f32;
        floats[SIZE] = size as f32;
        floats[CELL] = cell as f32;
        floats[FAR_KM] = reach.far_km as f32;
        for k in 0..3 {
            floats[AXIS + k] = axis[k] as f32;
            floats[CENTER + k] = center[k] as f32;
            floats[EAST + k] = east[k] as f32;
            floats[NORTH + k] = north[k] as f32;
        }
        let distances = self.distances(&near, center, east, north, cell, size, &reach);
        floats.extend(distances.iter().map(|&d| d as f32));
        plan.floats = floats;
        plan
    }

    /// The size of a fault cell in kilometers.
    fn fault_km(&self) -> f64 {
        let world = self.spec.world_km;
        // A cell is at least a number of texels, even on a coarse map.
        (CELL_PART * self.warp * world)
            .min(CELL_MAX_KM)
            .max(CELL_TEXELS * FRAC_PI_2 / self.face_size as f64 * world)
    }

    /// The growth at a unit direction, from the stamps in `near`.
    fn growth(&self, near: &[usize], q: V3) -> f64 {
        let mut rest = 1.0;
        for &i in near {
            let s = self.stamps[i];
            let k = chord_angle(q, [s[0], s[1], s[2]]) / s[3];
            if k >= 1.0 {
                continue;
            }
            let (hard, flow) = (s[4], s[5]);
            let w = if k <= hard {
                flow
            } else {
                let t = (k - hard) / (1.0 - hard);
                flow * (1.0 - t * t * (3.0 - 2.0 * t))
            };
            rest *= 1.0 - w;
        }
        1.0 - rest
    }

    /// The point after the warp of the growth.
    fn warped(&self, p: V3, cell_km: f64) -> V3 {
        let world = self.spec.world_km;
        let seed = self.spec.seed;
        let mut wavelength = 2.0 * self.warp * world;
        // The grid does not show the octaves that are shorter than its cells.
        let last = (WARP_TEXELS * FRAC_PI_2 / self.face_size as f64 * world).max(cell_km);
        let mut q = p;
        let mut k = 0;
        while wavelength >= last && k < 16 {
            let n = noise3(scale(p, world / wavelength), seed ^ (SALT_WARP + k));
            q = add(q, scale(n, WARP * wavelength / world));
            wavelength *= 0.5;
            k += 1;
        }
        normalize(q)
    }

    /// The signed distances to the edge of the plateau in kilometers, on a
    /// grid of `size` cells along one side.
    #[allow(clippy::too_many_arguments)]
    fn distances(
        &self,
        near: &[usize],
        center: V3,
        east: V3,
        north: V3,
        cell: f64,
        size: usize,
        reach: &Reach,
    ) -> Vec<f64> {
        let world = self.spec.world_km;
        let cell_km = cell * world;
        let middle = (size - 1) as f64 * 0.5;
        let at = |i: usize, j: usize| {
            let (x, y) = ((i as f64 - middle) * cell, (j as f64 - middle) * cell);
            normalize(add(center, add(scale(east, x), scale(north, y))))
        };
        // The fault cells, and whether each has grown.
        let axis = fault_axis(self.spec.seed);
        let fault_km = self.fault_km();
        let seed = self.spec.seed ^ SALT_CELL;
        let mut grown: HashMap<[u32; 3], bool> = HashMap::new();
        let mut inside = vec![false; size * size];
        // The stamps in bins of the plane, so a point checks only the stamps
        // in its bin and the 8 bins around it.
        let widest = near.iter().map(|&i| self.stamps[i][3]).fold(0.0, f64::max);
        let bin = widest.max(cell);
        let bins = ((size as f64 * cell / bin).ceil() as usize + 2).max(1);
        let place = |q: V3| {
            let up = dot(q, center).max(1e-9);
            let x = dot(q, east) / up / bin + bins as f64 * 0.5;
            let y = dot(q, north) / up / bin + bins as f64 * 0.5;
            (
                (x.floor().max(0.0) as usize).min(bins - 1),
                (y.floor().max(0.0) as usize).min(bins - 1),
            )
        };
        let mut binned = vec![Vec::new(); bins * bins];
        for &k in near {
            let s = self.stamps[k];
            let (x, y) = place([s[0], s[1], s[2]]);
            binned[y * bins + x].push(k);
        }
        let mut local = Vec::new();
        let mut growth_at = |q: V3| {
            let (x, y) = place(q);
            local.clear();
            for by in y.saturating_sub(1)..(y + 2).min(bins) {
                for bx in x.saturating_sub(1)..(x + 2).min(bins) {
                    local.extend_from_slice(&binned[by * bins + bx]);
                }
            }
            // The stamps keep the order of the stroke.
            local.sort_unstable();
            self.growth(&local, q)
        };
        for j in 0..size {
            for i in 0..size {
                let p = at(i, j);
                if growth_at(self.warped(p, cell_km)) <= GROWN {
                    continue;
                }
                let q = scale(p, world / fault_km);
                let q = add(q, scale(axis, -dot(q, axis) * (1.0 - 1.0 / CELL_STRETCH)));
                let (site, place) = nearest_site(q, seed);
                let grew = match grown.get(&site) {
                    Some(&grew) => grew,
                    None => {
                        let at = add(place, scale(axis, dot(place, axis) * (CELL_STRETCH - 1.0)));
                        let grew = growth_at(self.warped(normalize(at), cell_km)) > CELL_GROWN;
                        grown.insert(site, grew);
                        grew
                    }
                };
                inside[j * size + i] = grew;
            }
        }
        // The distance of each cell to the nearest cell on the other side of
        // the edge, between the centers of the cells less half a cell.
        let to_outside = distance_transform(&inside.iter().map(|&x| !x).collect::<Vec<_>>(), size);
        let to_inside = distance_transform(&inside, size);
        let mut d: Vec<f64> = (0..size * size)
            .map(|k| {
                let cells = if inside[k] {
                    to_outside[k].sqrt() - 0.5
                } else {
                    0.5 - to_inside[k].sqrt()
                };
                (cells * cell_km).clamp(-reach.far_km, reach.far_km)
            })
            .collect();
        for _ in 0..BLURS {
            d = blur(&d, size);
        }
        d
    }
}

/// The nearest point of a seeded Voronoi field to a point, as the cell of
/// the lattice that holds it and its place.
fn nearest_site(q: V3, seed: u32) -> ([u32; 3], V3) {
    let (cell, _) = cell_of(q);
    let mut best = (f64::MAX, cell, q);
    for k in 0..27u32 {
        let c: [u32; 3] = std::array::from_fn(|i| {
            cell[i]
                .wrapping_add((k / 3u32.pow(i as u32)) % 3)
                .wrapping_sub(1)
        });
        let (point, _) = cell_point(c, seed);
        let at: V3 = std::array::from_fn(|i| f64::from(c[i] as i32) + point[i]);
        let to = add(q, scale(at, -1.0));
        let d = dot(to, to);
        if d < best.0 {
            best = (d, c, at);
        }
    }
    (best.1, best.2)
}

/// The squared distance in cells from each cell of a square grid to the
/// nearest cell that is `true`, by the method of Felzenszwalb and
/// Huttenlocher (2012). With no such cell, each distance is infinite.
fn distance_transform(features: &[bool], size: usize) -> Vec<f64> {
    let mut grid: Vec<f64> = features
        .iter()
        .map(|&f| if f { 0.0 } else { f64::INFINITY })
        .collect();
    let mut line = vec![0.0; size];
    let mut out = vec![0.0; size];
    for j in 0..size {
        line.copy_from_slice(&grid[j * size..(j + 1) * size]);
        transform_line(&line, &mut out);
        grid[j * size..(j + 1) * size].copy_from_slice(&out);
    }
    for i in 0..size {
        for j in 0..size {
            line[j] = grid[j * size + i];
        }
        transform_line(&line, &mut out);
        for j in 0..size {
            grid[j * size + i] = out[j];
        }
    }
    grid
}

/// The squared distance transform of one line.
fn transform_line(f: &[f64], out: &mut [f64]) {
    let n = f.len();
    let mut v = vec![0usize; n];
    let mut z = vec![0.0; n + 1];
    let mut k = 0;
    let first = f.iter().position(|x| x.is_finite());
    let Some(first) = first else {
        out.fill(f64::INFINITY);
        return;
    };
    v[0] = first;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in first + 1..n {
        if !f[q].is_finite() {
            continue;
        }
        loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q - p) as f64);
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }
            if s <= z[k] {
                v[0] = q;
                z[0] = f64::NEG_INFINITY;
                z[1] = f64::INFINITY;
                break;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f64::INFINITY;
            break;
        }
    }
    let mut k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let d = q as f64 - p as f64;
        *o = d * d + f[p];
    }
}

/// The mean of the 3 by 3 cells around each cell of a square grid. A cell at
/// a side takes the mean of the cells that it has.
fn blur(d: &[f64], size: usize) -> Vec<f64> {
    let mut out = vec![0.0; size * size];
    for j in 0..size {
        for i in 0..size {
            let (mut sum, mut count) = (0.0, 0.0);
            for y in j.saturating_sub(1)..(j + 2).min(size) {
                for x in i.saturating_sub(1)..(i + 2).min(size) {
                    sum += d[y * size + x];
                    count += 1.0;
                }
            }
            out[j * size + i] = sum / count;
        }
    }
    out
}

/// The floor under a window with a radius in radians around a unit
/// direction, in meters: a low percentile of the ground, so a plateau in
/// the window does not raise it.
pub fn plateau_floor(map: &Heightmap, center: V3, radius: f64) -> f64 {
    let step = FRAC_PI_2 / map.face_size() as f64;
    let spacing = (2.0 * step).max(radius / 32.0);
    let half = (radius / spacing).ceil().min(32.0) as i64;
    let (east, north) = tangent_frame(center);
    let mut depths = Vec::new();
    for j in -half..=half {
        for i in -half..=half {
            let (dx, dy) = (i as f64 * spacing, j as f64 * spacing);
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            let p = normalize(add(center, add(scale(east, dx), scale(north, dy))));
            depths.push(ELEV_MIN + map.level_at(p) / LEVELS_PER_M);
        }
    }
    depths.sort_by(f64::total_cmp);
    depths[((depths.len() - 1) as f64 * FLOOR_PERCENTILE).round() as usize]
}

fn tangent_frame(center: V3) -> (V3, V3) {
    let east = crate::math::cross([0.0, 0.0, 1.0], center);
    let east = if dot(east, east) < 1e-18 {
        [1.0, 0.0, 0.0]
    } else {
        normalize(east)
    };
    (east, crate::math::cross(center, east))
}

/// The axis along which the fault cells of a world are long.
fn fault_axis(seed: u32) -> V3 {
    let h = |k: u32| 2.0 * unit(hash(k, 0, 0, seed ^ SALT_AXIS)) - 1.0;
    let a = [h(1), h(2), h(3)];
    if length(a) < 1e-3 {
        [0.0, 0.0, 1.0]
    } else {
        normalize(a)
    }
}

/// The angle between two unit vectors.
fn chord_angle(a: V3, b: V3) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    2.0 * (0.5 * length(d)).min(1.0).asin()
}

/// The cell of a lattice with a cell size of 1 at a point, and the place of
/// the point in it.
fn cell_of(q: V3) -> ([u32; 3], V3) {
    let c = q.map(f64::floor);
    (
        c.map(|c| c as i32 as u32),
        [q[0] - c[0], q[1] - c[1], q[2] - c[2]],
    )
}

/// The point of a cell of a seeded lattice, inside the middle half of the
/// cell, and three numbers from 0 to 1 for its other parts.
fn cell_point(cell: [u32; 3], seed: u32) -> (V3, [f64; 3]) {
    let h = |k: u32| unit(hash(cell[0], cell[1], cell[2], seed.wrapping_add(k)));
    let point = [0.25 + 0.5 * h(0), 0.25 + 0.5 * h(1), 0.25 + 0.5 * h(2)];
    (point, [h(3), h(4), h(5)])
}

impl PlateauPlan {
    /// The plan as 32-bit numbers: a header and the grid.
    pub fn floats(&self) -> &[f32] {
        &self.floats
    }

    /// Whether the plan changes nothing.
    pub fn is_empty(&self) -> bool {
        self.rects.iter().all(Option::is_none)
    }

    fn get(&self, slot: usize) -> f64 {
        f64::from(self.floats[slot])
    }

    fn v3(&self, slot: usize) -> V3 {
        [self.get(slot), self.get(slot + 1), self.get(slot + 2)]
    }

    fn seed(&self) -> u32 {
        self.get(SEED_LOW) as u32 | ((self.get(SEED_HIGH) as u32) << 16)
    }

    /// The level of a texel at unit direction `p` with level `start` from
    /// before the stroke.
    pub fn level(&self, start: u16, p: V3) -> u16 {
        let f = self.part(p);
        if f <= 0.0 {
            return start;
        }
        // The profile rises from the floor, or from the ground where it is
        // below the floor, to the top. A texel takes the profile where it is
        // higher, so a stroke that comes again raises nothing.
        let start = f64::from(start);
        let base = start.min(self.get(FLOOR_LEVEL));
        let new = (base + f * (self.target(p) - base))
            .max(start)
            .round()
            .clamp(0.0, 65535.0);
        self.limits.apply(start, new) as u16
    }

    /// The signed distance to the edge at a texel in kilometers, from the
    /// grid, between the 4 cells around it.
    fn distance_at(&self, p: V3) -> f64 {
        let far = self.get(FAR_KM);
        let size = self.get(SIZE) as usize;
        let cell = self.get(CELL);
        let up = dot(p, self.v3(CENTER));
        if up <= 0.0 || size < 2 {
            return -far;
        }
        let middle = (size - 1) as f64 * 0.5;
        let x = dot(p, self.v3(EAST)) / up / cell + middle;
        let y = dot(p, self.v3(NORTH)) / up / cell + middle;
        let top = (size - 1) as f64;
        if !(0.0..=top).contains(&x) || !(0.0..=top).contains(&y) {
            return -far;
        }
        let (i, j) = (
            (x.floor() as usize).min(size - 2),
            (y.floor() as usize).min(size - 2),
        );
        let (tx, ty) = (x - i as f64, y - j as f64);
        let at = |i: usize, j: usize| f64::from(self.floats[PLATEAU_HEADER + j * size + i]);
        let low = at(i, j) + (at(i + 1, j) - at(i, j)) * tx;
        let high = at(i, j + 1) + (at(i + 1, j + 1) - at(i, j + 1)) * tx;
        low + (high - low) * ty
    }

    /// The part of the height of the plateau at a texel, from 0 at the foot
    /// to 1 on the top.
    fn part(&self, p: V3) -> f64 {
        let world = self.get(WORLD_KM);
        let d = self.distance_at(p) + self.edge_noise(p);
        let top_km = self.get(TOP_M) / 1000.0;
        let u = d / (SCARP * top_km);
        if u <= -FAULT_SHIFT {
            return 0.0;
        }
        let n = noise3(scale(p, world / FAULT_KM), self.seed() ^ SALT_FAULT);
        let mut foot = 0.0;
        // The faults move together, and a small part of each move is its
        // own, so two faults never meet.
        for (k, (h, a, b)) in FAULTS.into_iter().enumerate() {
            let shift = FAULT_SHIFT * (0.8 * n[0] + 0.2 * n[k]);
            foot += h * smoothstep(a + shift, b + shift, u);
        }
        let ramp = (RAMP * top_km).min(RAMP_WARP * self.get(WARP_KM));
        let rise = 1.0 - (-d.max(0.0) / ramp).exp();
        FOOT * foot + (1.0 - FOOT) * rise * smoothstep(0.0, 1.0, u)
    }

    /// How far the fractal noise moves the edge out at a texel, in
    /// kilometers.
    fn edge_noise(&self, p: V3) -> f64 {
        let world = self.get(WORLD_KM);
        let last = WARP_TEXELS * self.get(TEXEL_KM);
        let mut wavelength = EDGE_KM;
        let mut amplitude = EDGE_REACH * (1.0 - EDGE_GAIN) * self.get(WARP_KM);
        let mut sum = 0.0;
        let mut k = 0;
        while wavelength >= last && k < 12 {
            let n = noise3(scale(p, world / wavelength), self.seed() ^ (SALT_EDGE + k));
            sum += amplitude * n[0];
            wavelength *= 0.5;
            amplitude *= EDGE_GAIN;
            k += 1;
        }
        sum
    }

    /// The level of the top of the plateau at a texel, with its low relief,
    /// below the ceiling.
    fn target(&self, p: V3) -> f64 {
        let world = self.get(WORLD_KM);
        let mut level = self.get(FLOOR_LEVEL) + self.get(TOP_LEVEL);
        for (k, (m, km)) in RELIEF.into_iter().enumerate() {
            let n = noise3(scale(p, world / km), self.seed() ^ (SALT_RELIEF + k as u32));
            level += LEVELS_PER_M * m * n[0];
        }
        let ceiling = (CEILING_M - ELEV_MIN) * LEVELS_PER_M;
        smooth_min(level, ceiling, CEILING_ROUND_M * LEVELS_PER_M)
    }
}

impl Heightmap {
    /// Grows the plateau of a plan on the ground from before the stroke.
    /// Call this in a stroke. Returns the number of texels that changed.
    pub fn plateau(&mut self, plan: &PlateauPlan) -> usize {
        assert_eq!(
            plan.face_size,
            self.face_size(),
            "the face size of the plan"
        );
        let mut changed = 0;
        for (face, rect) in plan.rects.into_iter().enumerate() {
            let Some(rect) = rect else { continue };
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    let p = self.texel_dir(face, x, y);
                    let new = plan.level(self.level_before(face, x, y), p);
                    if new != self.texel(face, x, y) {
                        self.set(face, x, y, new);
                        changed += 1;
                    }
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::meters_to_level;
    use crate::math::{angle, lonlat_to_dir};

    const N: usize = 1024;
    const FLOOR_M: f64 = -5000.0;
    const RADIUS: f64 = 0.08;

    fn spec() -> Plateau {
        Plateau {
            height_m: 3000.0,
            age_myr: 0.0,
            world_km: 1000.0,
            seed: 7,
            limits: Limits::default(),
        }
    }

    fn flat() -> Heightmap {
        Heightmap::with_tile_size(N, 64, meters_to_level(FLOOR_M))
    }

    fn meters(level: u16) -> f64 {
        ELEV_MIN + f64::from(level) / LEVELS_PER_M
    }

    fn line(from: (f64, f64), to: (f64, f64), count: usize) -> Vec<V3> {
        (0..count)
            .map(|i| {
                let t = i as f64 / (count - 1).max(1) as f64;
                lonlat_to_dir(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
            })
            .collect()
    }

    fn paint(map: &mut Heightmap, spec: Plateau, centers: &[V3], radius: f64) -> PlateauStroke {
        let mut stroke = PlateauStroke::new(spec, map.face_size());
        map.begin_stroke();
        for &c in centers {
            let plan = stroke.stamp(map, c, radius, 0.5, 0.7);
            map.plateau(&plan);
        }
        map.end_stroke();
        stroke
    }

    /// The texels near a place, with their direction and their level.
    fn texels(map: &Heightmap, center: V3, within: f64) -> Vec<(V3, u16)> {
        let mut out = Vec::new();
        for face in 0..FACES {
            for y in 0..N {
                for x in 0..N {
                    let p = map.texel_dir(face, x, y);
                    if angle(p, center) < within {
                        out.push((p, map.get(face, x as i64, y as i64)));
                    }
                }
            }
        }
        out
    }

    fn same(a: &Heightmap, b: &Heightmap) -> bool {
        (0..FACES).all(|f| {
            (0..N).all(|y| {
                (0..N).all(|x| a.get(f, x as i64, y as i64) == b.get(f, x as i64, y as i64))
            })
        })
    }

    /// A stroke that paints a wide plateau around (10, 5).
    fn wide() -> Vec<V3> {
        let mut centers = line((4.0, 2.0), (16.0, 2.0), 25);
        centers.extend(line((16.0, 6.0), (4.0, 6.0), 25));
        centers.extend(line((4.0, 10.0), (16.0, 10.0), 25));
        centers
    }

    fn percentile(mut v: Vec<f64>, p: f64) -> f64 {
        v.sort_by(f64::total_cmp);
        v[((v.len() - 1) as f64 * p).round() as usize]
    }

    #[test]
    fn the_height_sets_the_top_above_the_floor_and_the_top_has_low_relief() {
        let mut map = flat();
        paint(&mut map, spec(), &wide(), RADIUS);
        let top: Vec<f64> = texels(&map, lonlat_to_dir(10.0, 6.0), 0.08)
            .into_iter()
            .map(|(_, l)| meters(l) - FLOOR_M)
            .collect();
        let median = percentile(top.clone(), 0.5);
        assert!((median - 3000.0).abs() < 150.0, "the top at {median} m");
        // Most of the top is within a few hundred meters of its middle.
        let level = top.iter().filter(|&&m| (m - median).abs() < 300.0).count();
        let part = level as f64 / top.len() as f64;
        assert!(part > 0.7, "{part} of the top is level");
    }

    /// The distance from the center to the foot of the scarp at an angle,
    /// in radians, and the width of the scarp from 10 to 90 percent of the
    /// height.
    fn edge(map: &Heightmap, center: V3, theta: f64) -> (f64, f64) {
        let (east, north) = tangent_frame(center);
        let dir = add(scale(east, theta.cos()), scale(north, theta.sin()));
        let step = 0.1 * FRAC_PI_2 / N as f64;
        let height = |d: f64| {
            let p = normalize(add(center, scale(dir, d)));
            ELEV_MIN + map.level_at(p) / LEVELS_PER_M - FLOOR_M
        };
        let mut d = 0.3;
        while height(d) < 300.0 {
            d -= step;
        }
        let low = d;
        while height(d) < 2700.0 && d > 0.0 {
            d -= step;
        }
        (low, low - d)
    }

    #[test]
    fn the_outline_is_uneven() {
        let mut map = flat();
        let center = lonlat_to_dir(10.0, 5.0);
        let radius = 0.15;
        paint(&mut map, spec(), &[center], radius);
        let edges: Vec<(f64, f64)> = (0..180)
            .map(|i| edge(&map, center, i as f64 * std::f64::consts::TAU / 180.0))
            .collect();
        let reach: Vec<f64> = edges.iter().map(|e| e.0 / radius).collect();
        let mean = reach.iter().sum::<f64>() / reach.len() as f64;
        let spread =
            (reach.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / reach.len() as f64).sqrt();
        assert!(spread > 0.03, "the outline is a circle: {spread}");
        // The outline is uneven at small scales too: neighbors at 2 degrees
        // differ.
        let rough = reach.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / reach.len() as f64;
        assert!(rough > 0.004, "the outline is smooth: {rough}");
    }

    #[test]
    fn the_slopes_are_soft_and_continuous() {
        let mut map = flat();
        paint(&mut map, spec(), &wide(), RADIUS);
        // A texel is 1.5 km wide, and no slope between two texels is steeper
        // than 15 degrees.
        let texel_m = FRAC_PI_2 / N as f64 * spec().world_km * 1000.0;
        let steepest = texel_m * 15f64.to_radians().tan();
        let mut changed = 0;
        for face in 0..FACES {
            for y in 0..N - 1 {
                for x in 0..N - 1 {
                    let here = meters(map.get(face, x as i64, y as i64));
                    for (dx, dy) in [(1, 0), (0, 1)] {
                        let there = meters(map.get(face, (x + dx) as i64, (y + dy) as i64));
                        assert!(
                            (there - here).abs() < steepest,
                            "a step of {} m at face {face} x {x} y {y} from {here} m, {:?}",
                            there - here,
                            crate::math::dir_to_lonlat(map.texel_dir(face, x, y))
                        );
                    }
                    changed += usize::from(here > FLOOR_M + 1.0);
                }
            }
        }
        assert!(changed > 10_000, "{changed} texels changed");
    }

    #[test]
    fn the_plateau_sinks_with_age() {
        let top = |age_myr: f64| {
            let mut map = flat();
            paint(&mut map, Plateau { age_myr, ..spec() }, &wide(), RADIUS);
            let top: Vec<f64> = texels(&map, lonlat_to_dir(10.0, 6.0), 0.05)
                .into_iter()
                .map(|(_, l)| meters(l))
                .collect();
            percentile(top, 0.5)
        };
        let (young, old) = (top(0.0), top(120.0));
        let sink = 0.5 * (gdh1_depth(120.0) - gdh1_depth(0.0));
        assert!((young - old - sink).abs() < 100.0, "{young} m and {old} m");
    }

    #[test]
    fn the_plateau_has_no_moat_and_stays_near_the_brush() {
        let mut map = flat();
        let before = map.snapshot();
        let centers = line((8.0, 4.0), (12.0, 6.0), 20);
        paint(&mut map, spec(), &centers, RADIUS);
        let mut changed = 0;
        for face in 0..FACES {
            for y in 0..N {
                for x in 0..N {
                    let (old, new) = (
                        before.get(face, x as i64, y as i64),
                        map.get(face, x as i64, y as i64),
                    );
                    assert!(new >= old, "a moat");
                    if new != old {
                        changed += 1;
                        let p = map.texel_dir(face, x, y);
                        let near = centers
                            .iter()
                            .map(|&c| angle(p, c))
                            .fold(f64::MAX, f64::min);
                        assert!(near < 2.0 * RADIUS, "a change at {near}");
                    }
                }
            }
        }
        assert!(changed > 1000, "{changed} texels changed");
    }

    #[test]
    fn a_second_stroke_does_not_raise_the_plateau_and_a_stroke_next_to_it_joins() {
        let mut map = flat();
        let first = line((6.0, 5.0), (10.0, 5.0), 20);
        paint(&mut map, spec(), &first, RADIUS);
        // The same stroke again changes nothing.
        let once = map.snapshot();
        paint(&mut map, spec(), &first, RADIUS);
        assert!(same(&map, &once), "raised again");
        // A stroke over a part of the plateau and past its end raises no
        // texel above the top and its relief.
        paint(&mut map, spec(), &line((8.0, 5.0), (14.0, 5.0), 25), RADIUS);
        let highest = texels(&map, lonlat_to_dir(10.0, 5.0), 0.2)
            .into_iter()
            .map(|(_, l)| meters(l))
            .fold(f64::MIN, f64::max);
        let relief: f64 = RELIEF.iter().map(|r| r.0).sum();
        assert!(highest < FLOOR_M + 3000.0 + relief, "a top at {highest} m");
        // Along the strokes, the top has no seam where they meet.
        let along = line((7.0, 5.0), (13.0, 5.0), 400);
        let heights: Vec<f64> = along
            .iter()
            .map(|&p| map.level_at(p) / LEVELS_PER_M)
            .collect();
        let jump = heights
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f64::max);
        assert!(jump < 80.0, "a step of {jump} m");
        assert!(meters(map.level_at(lonlat_to_dir(13.0, 5.0)) as u16) > FLOOR_M + 2000.0);
    }

    #[test]
    fn the_plateau_stays_below_sea_level() {
        let mut map = flat();
        let tall = Plateau {
            height_m: 8000.0,
            ..spec()
        };
        // Strokes over each other, at the largest height.
        for _ in 0..3 {
            paint(&mut map, tall, &wide(), RADIUS);
        }
        let highest = texels(&map, lonlat_to_dir(10.0, 6.0), 0.2)
            .into_iter()
            .map(|(_, l)| meters(l))
            .fold(f64::MIN, f64::max);
        assert!(highest < 0.0, "a top at {highest} m");
        assert!(highest > -1000.0, "a top at {highest} m");
    }

    #[test]
    fn equal_strokes_give_equal_results_and_one_undo_puts_back_the_ground() {
        let centers = line((8.0, 4.0), (12.0, 6.0), 20);
        let mut a = flat();
        let mut b = flat();
        let before = a.snapshot();
        paint(&mut a, spec(), &centers, RADIUS);
        paint(&mut b, spec(), &centers, RADIUS);
        assert!(same(&a, &b));
        assert!(!same(&a, &before));
        assert!(a.undo());
        assert!(same(&a, &before));
    }
}

#[cfg(test)]
mod timing {
    use super::*;
    use crate::cube::meters_to_level;
    use crate::math::lonlat_to_dir;

    #[test]
    #[ignore = "manual"]
    fn manual_time_of_a_plan() {
        let map = Heightmap::new(10752, meters_to_level(-5000.0));
        let spec = Plateau {
            height_m: 4000.0,
            age_myr: 120.0,
            world_km: 6371.0,
            seed: 7,
            limits: Limits::default(),
        };
        let mut stroke = PlateauStroke::new(spec, 10752);
        for i in 0..40 {
            let c = lonlat_to_dir(f64::from(i) * 0.4, 0.0);
            let t = std::time::Instant::now();
            let _ = stroke.stamp(&map, c, 100.0 / 6371.0, 0.5, 0.8);
            if i % 10 == 9 {
                eprintln!("plan {i}: {:?}", t.elapsed());
            }
        }
    }
}
