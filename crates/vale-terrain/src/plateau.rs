//! The plateau brush.
//!
//! A stroke keeps its stamps. The growth of a point is 1 minus the product
//! of 1 minus the weight of each stamp there, so each stamp grows the
//! plateau under the brush. The edge of the plateau is where the growth
//! crosses one half, after a seeded warp moves the point. A texel moves
//! from its level from before the stroke toward the top of the plateau, and
//! it does not go down. Thus a second stroke over the plateau changes
//! nothing, and two strokes side by side join.

use std::f64::consts::FRAC_PI_2;

use crate::cube::{ELEV_MIN, FACES};
use crate::heightmap::{Heightmap, Limits, TexelRect, brush_rect, hash, noise3};
use crate::line::{smoothstep, unit};
use crate::math::{V3, add, cross, dot, length, normalize, scale};
use crate::seamount::{LEVELS_PER_M, gdh1_depth};

/// The default height of the top above the floor when the plateau forms,
/// in meters, and the range of the setting.
pub const PLATEAU_HEIGHT_M: f64 = 4000.0;
pub const PLATEAU_HEIGHT_RANGE: std::ops::RangeInclusive<f64> = 500.0..=8000.0;

/// The default age of the plateau in millions of years.
pub const PLATEAU_AGE_MYR: f64 = 120.0;

/// The number of 32-bit numbers before the stamps in `PlateauPlan::floats`,
/// and the number for each stamp.
pub const PLATEAU_HEADER: usize = 16;
pub const PLATEAU_STAMP_FLOATS: usize = 8;

/// The largest number of stamps in one plan. The plan fits in a slot of
/// the volcano buffer of the seamounts brush.
pub const PLAN_STAMPS: usize = (crate::seamount::PLAN_VOLCANOES * crate::seamount::VOLCANO_FLOATS
    - PLATEAU_HEADER)
    / PLATEAU_STAMP_FLOATS;

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

/// The floor is this percentile of the ground in a window of 3 brush radii.
const FLOOR_PERCENTILE: f64 = 0.1;

/// The warp of the outline: each octave moves a point by up to this part of
/// its wavelength. The first wavelength is 2 brush radii, and the last is
/// 3 texels.
const WARP: f64 = 0.13;
const WARP_TEXELS: f64 = 3.0;
/// The warp moves a point by at most this part of the brush radius.
const WARP_REACH: f64 = 0.9;

/// The width of the scarp is this factor times the height of the top, as a
/// slope of about 9.5 degrees on average. The faults step down in 3 blocks.
const SCARP: f64 = 6.0;
const FAULTS: [(f64, f64, f64); 3] = [(0.45, 0.0, 0.22), (0.35, 0.4, 0.58), (0.2, 0.8, 0.95)];
/// The faults move by up to this part of the scarp along the edge, with
/// this wavelength in kilometers.
const FAULT_SHIFT: f64 = 0.25;
const FAULT_KM: f64 = 60.0;

/// The slide scars: a cell of this size in kilometers has a scar with this
/// chance. A scar is a scallop with a radius of a part of the cell, and it
/// cuts into the edge by up to its radius.
const SCAR_KM: f64 = 80.0;
const SCARS: f64 = 0.8;
const SCAR_RADIUS: (f64, f64) = (0.18, 0.3);

/// The rift basins: a cell of this size in kilometers has a straight rift
/// with this chance. A rift runs along a seeded axis of the world, turned by
/// up to an angle, and its half length is a part of the cell. Its half
/// width is in kilometers, and its floor is a part of the height of the top
/// below the top. A rift gets narrower and shallower toward its ends.
const RIFT_KM: f64 = 160.0;
const RIFTS: f64 = 0.7;
const RIFT_TURN: f64 = 0.45;
const RIFT_LENGTH: (f64, f64) = (0.4, 0.9);
const RIFT_HALF_KM: (f64, f64) = (8.0, 18.0);
const RIFT_DEPTH: f64 = 0.4;

/// The relief of the top in meters at a height of 2,500 m, with the
/// wavelength of each octave in kilometers.
const RELIEF: [(f64, f64); 3] = [(120.0, 200.0), (45.0, 40.0), (18.0, 10.0)];
const RELIEF_HEIGHT_M: f64 = 2500.0;

/// The volcanoes on the top: a cell of this size in kilometers has a
/// volcano with this chance. Its radius is a part of the cell, and its
/// height is a part of the height of the top.
const VOLCANO_KM: f64 = 150.0;
const VOLCANOES: f64 = 0.5;
const VOLCANO_RADIUS: (f64, f64) = (0.08, 0.22);
const VOLCANO_HEIGHT: (f64, f64) = (0.3, 0.9);

/// The salts of the seeded fields.
const SALT_WARP: u32 = 0x5a17_0000;
const SALT_FAULT: u32 = 0x5a17_1000;
const SALT_SCAR: u32 = 0x5a17_2000;
const SALT_RIFT: u32 = 0x5a17_3000;
const SALT_RELIEF: u32 = 0x5a17_4000;
const SALT_VOLCANO: u32 = 0x5a17_5000;

/// The slots of the header of `PlateauPlan::floats`.
const FLOOR_LEVEL: usize = 0;
const TOP_LEVEL: usize = 1;
const TOP_M: usize = 2;
const WORLD_KM: usize = 3;
const SEED_LOW: usize = 4;
const SEED_HIGH: usize = 5;
const WARP_KM: usize = 6;
const TEXEL_KM: usize = 7;
const COUNT: usize = 8;
const RIFT_AXIS: usize = 9;

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
            self.floor_m = Some(plateau_floor(map, center, 3.0 * radius));
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
        // A texel that this stamp changes is at most `far` from its center.
        // Its warped point is at most `warp` from it, and a stamp reaches
        // that point if it is less than its radius away.
        let warp = WARP_REACH * self.warp;
        let far = radius + warp;
        plan.rects = std::array::from_fn(|face| brush_rect(n, face, center, far));
        let mut near: Vec<(f64, usize)> = self
            .stamps
            .iter()
            .enumerate()
            .map(|(i, s)| (crate::math::angle([s[0], s[1], s[2]], center), i))
            .filter(|&(a, i)| a < far + warp + self.stamps[i][3])
            .collect();
        if near.len() > PLAN_STAMPS {
            near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            near.truncate(PLAN_STAMPS);
        }
        // The stamps keep the order of the stroke, so the CPU and the GPU
        // multiply them in the same order.
        let mut near: Vec<usize> = near.into_iter().map(|(_, i)| i).collect();
        near.sort_unstable();

        let top_m = plateau_top_m(self.spec.height_m, self.spec.age_myr);
        let floor_m = self.floor_m.unwrap_or(0.0);
        let world = self.spec.world_km;
        let seed = self.spec.seed;
        let axis = rift_axis(seed);
        let mut floats = vec![0.0f32; PLATEAU_HEADER];
        floats[FLOOR_LEVEL] = ((floor_m - ELEV_MIN) * LEVELS_PER_M) as f32;
        floats[TOP_LEVEL] = (top_m * LEVELS_PER_M) as f32;
        floats[TOP_M] = top_m as f32;
        floats[WORLD_KM] = world as f32;
        floats[SEED_LOW] = (seed & 0xffff) as f32;
        floats[SEED_HIGH] = (seed >> 16) as f32;
        floats[WARP_KM] = (self.warp * world) as f32;
        floats[TEXEL_KM] = (step * world) as f32;
        floats[COUNT] = near.len() as f32;
        for k in 0..3 {
            floats[RIFT_AXIS + k] = axis[k] as f32;
        }
        for i in near {
            let s = self.stamps[i];
            floats.extend(s.map(|v| v as f32));
            floats.extend([0.0, 0.0]);
        }
        plan.floats = floats;
        plan
    }
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

/// The axis along which the rifts of a world are long.
fn rift_axis(seed: u32) -> V3 {
    let h = |k: u32| 2.0 * unit(hash(k, 0, 0, seed ^ SALT_RIFT)) - 1.0;
    let a = [h(1), h(2), h(3)];
    if length(a) < 1e-3 {
        [0.0, 0.0, 1.0]
    } else {
        normalize(a)
    }
}

/// The angle between two unit vectors, as the shader finds it.
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

fn distance(a: V3, b: V3) -> f64 {
    length([a[0] - b[0], a[1] - b[1], a[2] - b[2]])
}

impl PlateauPlan {
    /// The plan as 32-bit numbers: a header and the stamps.
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

    /// The number of stamps.
    pub fn stamps(&self) -> usize {
        self.floats.len().saturating_sub(PLATEAU_HEADER) / PLATEAU_STAMP_FLOATS
    }

    fn stamp(&self, i: usize) -> [f64; 6] {
        let at = PLATEAU_HEADER + i * PLATEAU_STAMP_FLOATS;
        std::array::from_fn(|k| f64::from(self.floats[at + k]))
    }

    fn seed(&self) -> u32 {
        self.get(SEED_LOW) as u32 | ((self.get(SEED_HIGH) as u32) << 16)
    }

    /// Whether stamp `i` can reach a texel within `far` radians of `p`.
    pub(crate) fn reaches(&self, i: usize, p: V3, far: f64) -> bool {
        let s = self.stamp(i);
        let warp = WARP_REACH * self.get(WARP_KM) / self.get(WORLD_KM);
        chord_angle([s[0], s[1], s[2]], p) < s[3] + warp + far
    }

    /// The level of a texel at unit direction `p` with level `start` from
    /// before the stroke.
    pub fn level(&self, start: u16, p: V3) -> u16 {
        let all: Vec<usize> = (0..self.stamps()).collect();
        self.level_of(&all, start, p)
    }

    /// `level` with only the stamps in `near`.
    pub(crate) fn level_of(&self, near: &[usize], start: u16, p: V3) -> u16 {
        let f = self.part(near, p);
        if f <= 0.0 {
            return start;
        }
        let start = f64::from(start);
        let target = self.target(p);
        let new = (start + f * (target - start).max(0.0))
            .round()
            .clamp(0.0, 65535.0);
        self.limits.apply(start, new) as u16
    }

    /// The point after the warp of the outline.
    fn warped(&self, p: V3) -> V3 {
        let world = self.get(WORLD_KM);
        let seed = self.seed();
        let mut wavelength = 2.0 * self.get(WARP_KM);
        let last = WARP_TEXELS * self.get(TEXEL_KM);
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

    /// The part of the height of the plateau at a texel, from 0 at the foot
    /// of the scarp to 1 on the top.
    fn part(&self, near: &[usize], p: V3) -> f64 {
        let q = self.warped(p);
        // The product of 1 minus the weight of each stamp, and its gradient.
        let mut rest = 1.0;
        let mut slope = [0.0; 3];
        for &i in near {
            let s = self.stamp(i);
            let c = [s[0], s[1], s[2]];
            let (radius, hard, flow) = (s[3], s[4], s[5]);
            let k = chord_angle(q, c) / radius;
            if k >= 1.0 {
                continue;
            }
            let (w, dw) = if k <= hard {
                (flow, 0.0)
            } else {
                let t = (k - hard) / (1.0 - hard);
                (
                    flow * (1.0 - t * t * (3.0 - 2.0 * t)),
                    -flow * 6.0 * t * (1.0 - t) / ((1.0 - hard) * radius),
                )
            };
            // The direction away from the center of the stamp.
            let away = add(scale(q, dot(q, c)), scale(c, -1.0));
            let size = length(away);
            let away = if size > 1e-12 {
                scale(away, 1.0 / size)
            } else {
                [0.0; 3]
            };
            slope = add(scale(slope, 1.0 - w), scale(away, -rest * dw));
            rest *= 1.0 - w;
        }
        let growth = 1.0 - rest;
        if growth <= 0.0 {
            return 0.0;
        }
        let gradient = length(slope).max(1e-9);
        let world = self.get(WORLD_KM);
        let d = (growth - 0.5) / gradient * world - self.scar(p);
        let width = SCARP * self.get(TOP_M) / 1000.0;
        let u = d / width;
        if u <= -FAULT_SHIFT {
            return 0.0;
        }
        let n = noise3(scale(p, world / FAULT_KM), self.seed() ^ SALT_FAULT);
        let mut f = 0.0;
        for (k, (h, a, b)) in FAULTS.into_iter().enumerate() {
            let shift = FAULT_SHIFT * n[k];
            f += h * smoothstep(a + shift, b + shift, u);
        }
        f
    }

    /// How far the slide scars cut into the edge at a texel, in kilometers.
    fn scar(&self, p: V3) -> f64 {
        let (cell, at) = cell_of(scale(p, self.get(WORLD_KM) / SCAR_KM));
        let (point, more) = cell_point(cell, self.seed() ^ SALT_SCAR);
        if more[0] >= SCARS {
            return 0.0;
        }
        let r = SCAR_RADIUS.0 + (SCAR_RADIUS.1 - SCAR_RADIUS.0) * more[1];
        let k = distance(at, point) / r;
        if k >= 1.0 {
            return 0.0;
        }
        r * SCAR_KM * (1.0 - k * k)
    }

    /// The depth of the rifts at a texel, from 0 to 1.
    fn rift(&self, p: V3) -> f64 {
        let axis = [
            self.get(RIFT_AXIS),
            self.get(RIFT_AXIS + 1),
            self.get(RIFT_AXIS + 2),
        ];
        let radius = self.get(WORLD_KM) / RIFT_KM;
        let q = scale(p, radius);
        let (cell, _) = cell_of(q);
        let seed = self.seed() ^ SALT_RIFT;
        let mut deepest: f64 = 0.0;
        for k in 0..27u32 {
            let c = [0, 1, 2].map(|i| {
                cell[i]
                    .wrapping_add((k / 3u32.pow(i as u32)) % 3)
                    .wrapping_sub(1)
            });
            let (point, more) = cell_point(c, seed);
            if more[0] >= RIFTS {
                continue;
            }
            // The middle of the rift is on the sphere, inside its cell.
            let at = [0, 1, 2].map(|i| f64::from(c[i] as i32) + point[i]);
            let up = normalize(at);
            let middle = scale(up, radius);
            if cell_of(middle).0 != c {
                continue;
            }
            let along = add(axis, scale(up, -dot(axis, up)));
            if length(along) < 1e-6 {
                continue;
            }
            let along = normalize(along);
            let turn = RIFT_TURN * (2.0 * more[1] - 1.0);
            let (sin, cos) = turn.sin_cos();
            let along = add(scale(along, cos), scale(cross(up, along), sin));
            let half = RIFT_LENGTH.0 + (RIFT_LENGTH.1 - RIFT_LENGTH.0) * more[2];
            let to = add(q, scale(middle, -1.0));
            let s = dot(to, along);
            let off = length(add(to, scale(along, -s.clamp(-half, half))));
            let end = 1.0 - smoothstep(0.5 * half, half, s.abs());
            let h = unit(hash(c[0], c[1], c[2], seed.wrapping_add(6)));
            let width = (RIFT_HALF_KM.0 + (RIFT_HALF_KM.1 - RIFT_HALF_KM.0) * h) / RIFT_KM;
            let width = width * (0.4 + 0.6 * end);
            deepest = deepest.max(end * (1.0 - smoothstep(0.35 * width, width, off)));
        }
        deepest
    }

    /// The level of the top of the plateau at a texel, with its relief, its
    /// rifts, and its volcanoes.
    fn target(&self, p: V3) -> f64 {
        let world = self.get(WORLD_KM);
        let seed = self.seed();
        let top = self.get(TOP_LEVEL);
        let mut level = self.get(FLOOR_LEVEL) + top;
        let relief = self.get(TOP_M) / RELIEF_HEIGHT_M * LEVELS_PER_M;
        for (k, (m, km)) in RELIEF.into_iter().enumerate() {
            let n = noise3(scale(p, world / km), seed ^ (SALT_RELIEF + k as u32));
            level += relief * m * n[0];
        }
        let rift = self.rift(p);
        level -= RIFT_DEPTH * top * rift;
        // A volcano and its foot stay inside its cell.
        let (cell, at) = cell_of(scale(p, world / VOLCANO_KM));
        let (point, more) = cell_point(cell, seed ^ SALT_VOLCANO);
        if more[0] < VOLCANOES {
            let r = VOLCANO_RADIUS.0 + (VOLCANO_RADIUS.1 - VOLCANO_RADIUS.0) * more[1];
            let h = VOLCANO_HEIGHT.0 + (VOLCANO_HEIGHT.1 - VOLCANO_HEIGHT.0) * more[2];
            let x = 1.0 - distance(at, point) / r;
            if x > 0.0 {
                level += h * top * x * x;
            }
        }
        level
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
        const BLOCK: usize = 32;
        let n = self.face_size();
        let mut changed = 0;
        let mut near = Vec::new();
        for (face, rect) in plan.rects.into_iter().enumerate() {
            let Some(rect) = rect else { continue };
            for by in (rect.y0..rect.y1).step_by(BLOCK) {
                for bx in (rect.x0..rect.x1).step_by(BLOCK) {
                    let (x1, y1) = ((bx + BLOCK).min(rect.x1), (by + BLOCK).min(rect.y1));
                    let middle = self.texel_dir(face, (bx + x1) / 2, (by + y1) / 2);
                    let corners = [(bx, by), (x1 - 1, y1 - 1), (bx, y1 - 1), (x1 - 1, by)];
                    let far = corners
                        .map(|(x, y)| chord_angle(middle, self.texel_dir(face, x, y)))
                        .into_iter()
                        .fold(0.0, f64::max)
                        + 2.0 * FRAC_PI_2 / n as f64;
                    near.clear();
                    near.extend((0..plan.stamps()).filter(|&i| plan.reaches(i, middle, far)));
                    if near.is_empty() {
                        continue;
                    }
                    for y in by..y1 {
                        for x in bx..x1 {
                            let p = self.texel_dir(face, x, y);
                            let new = plan.level_of(&near, self.level_before(face, x, y), p);
                            if new != self.texel(face, x, y) {
                                self.set(face, x, y, new);
                                changed += 1;
                            }
                        }
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

    #[test]
    fn the_top_has_rift_basins_and_a_few_volcanoes() {
        let mut map = flat();
        paint(&mut map, spec(), &wide(), RADIUS);
        // The texels of the plateau, past the foot of the scarp.
        let top: Vec<f64> = texels(&map, lonlat_to_dir(10.0, 6.0), 0.15)
            .into_iter()
            .map(|(_, l)| meters(l) - FLOOR_M)
            .filter(|&m| m > 1000.0)
            .collect();
        let count = top.len() as f64;
        let rifts = top.iter().filter(|&&m| m > 1200.0 && m < 2300.0).count() as f64;
        let volcanoes = top.iter().filter(|&&m| m > 3400.0).count() as f64;
        assert!(
            rifts / count > 0.01 && rifts / count < 0.25,
            "rifts {}",
            rifts / count
        );
        assert!(
            volcanoes > 0.0 && volcanoes / count < 0.1,
            "volcanoes {}",
            volcanoes / count
        );
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
    fn the_outline_is_uneven_and_the_edges_are_steep_scarps() {
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
        // The brush falls off over half its radius, but a scarp rises over
        // about 18 km, 0.018 radians.
        let width = percentile(edges.iter().map(|e| e.1).collect(), 0.5);
        assert!(width < 0.03, "the scarp is {width} radians wide");
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
        paint(&mut map, spec(), &line((6.0, 5.0), (10.0, 5.0), 20), RADIUS);
        let first = map.snapshot();
        // Over a part of the plateau and past its end.
        paint(&mut map, spec(), &line((8.0, 5.0), (14.0, 5.0), 25), RADIUS);
        for (p, level) in texels(&first, lonlat_to_dir(8.0, 5.0), 0.03) {
            let now = map.level_at(p);
            assert!((now - f64::from(level)).abs() <= 1.0, "raised again");
        }
        // Along the stroke, the top has no seam where the strokes meet.
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
