//! The trench brush.
//!
//! The stroke is the axis of a trench, where one plate bends down under
//! another. The incoming plate follows the universal elastic profile of
//! Caldwell and others (1976): it goes down to the axis, and an outer rise
//! stands before it. Normal faults parallel to the axis break the outer
//! slope into horsts and grabens (Masson 1991). The faults start near the
//! outer rise, and their throw grows toward the axis. The wall on the side
//! of the overriding plate is steeper. An accretionary margin has a prism
//! of thrust ridges on that wall and a forearc basin behind it. An eroding
//! margin has a rough wall with slide scars.
//!
//! The change adds to the ground from before the stroke, so the hills and
//! the seamounts of the incoming plate go down with it. The brush is a line
//! brush, as the range brush is: the new level of a texel is a function of
//! its place on the line and of its level from before the stroke.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, SQRT_2};
use std::ops::Deref;

use crate::cube::{ELEV_MAX, ELEV_MIN};
use crate::heightmap::{Limits, hash};
use crate::line::{LinePlan, LinePoint, REACH, StrokeLine, TAPER, bump, noise1, smoothstep, unit};
use crate::math::V3;
use crate::seamount::{GRAVITY, MANTLE, POISSON, WATER, YOUNG, elastic_km};

/// The largest radius of a trench stroke, in radians. On a world of the
/// size of the Earth, the brush changes the ground up to 760 km from the
/// axis.
pub const MAX_TRENCH_RADIUS: f64 = 0.08;

/// The 16-bit steps of one meter.
const LEVELS_PER_M: f64 = 65535.0 / (ELEV_MAX - ELEV_MIN);

/// The wavelengths in kilometers and the parts of the depth of the two
/// octaves of the field of the depth along the axis.
const DEEPS: [(f64, f64); 2] = [(400.0, 0.25), (100.0, 0.12)];

/// The flexural parameter changes along the axis by this part, with this
/// wavelength in kilometers.
const BEND_VARY: f64 = 0.1;
const BEND_VARY_KM: f64 = 300.0;

/// Half of the width of the flat floor of the axis, in kilometers, past
/// half of a texel.
const FLOOR_KM: f64 = 1.0;

/// The mean distance between two bending faults, in kilometers and in
/// texels at the least.
const FAULT_KM: f64 = 6.0;
const FAULT_TEXELS: f64 = 3.0;

/// The largest shift of a fault from its place in the row, in fault
/// spacings.
const FAULT_JITTER: f64 = 0.25;

/// The faults move across the axis by up to this number of spacings, with
/// this wavelength along it in kilometers.
const FAULT_WANDER: f64 = 1.2;
const FAULT_WANDER_KM: f64 = 60.0;

/// The length in kilometers of the cells of the fade of a fault along the
/// axis.
const FAULT_LENGTH_KM: f64 = 40.0;

/// The throw of the faults near the axis: a part of the depth, and at most
/// this number of meters.
const THROW_PART: f64 = 0.12;
const THROW_MAX_M: f64 = 500.0;

/// The width of a scarp, in kilometers and at least one texel.
const SCARP_KM: f64 = 0.8;

/// The parts of the blocks between faults that are tilted, and that are
/// grabens. The rest are horsts.
const TILTED: f64 = 0.6;
const GRABENS: f64 = 0.2;

/// The width of the inner wall, as a part of the distance from the axis
/// to the zero of the outer wall: for an accretionary margin and for an
/// eroding margin.
const WALL: [f64; 2] = [0.6, 0.35];

/// The width of the inner wall changes along the axis by this part, with
/// this wavelength in kilometers.
const WALL_VARY: f64 = 0.15;
const WALL_VARY_KM: f64 = 200.0;

/// The mean distance between two thrust ridges of a prism, in kilometers
/// and in texels at the least.
const PRISM_KM: f64 = 12.0;
const PRISM_TEXELS: f64 = 4.0;

/// The mean length of a piece of a thrust ridge, in kilometers, and the
/// largest shift of the end of a piece, as a part of that length.
const PRISM_PIECE_KM: f64 = 75.0;
const PRISM_BREAK: f64 = 0.33;

/// Half of the length over which a piece fades in or out, as a part of the
/// piece length.
const PRISM_OVERLAP: f64 = 0.15;

/// The largest shift of a piece across the axis, in ridge spacings.
const PRISM_SHIFT: f64 = 0.2;

/// A ridge moves across the axis by up to this number of spacings, with
/// this wavelength along it in kilometers.
const PRISM_WANDER: f64 = 0.3;
const PRISM_WANDER_KM: f64 = 40.0;

/// The shift across the axis from one end of a piece to the other end, in
/// ridge spacings.
const PRISM_ECHELON: f64 = 0.5;

/// The width of the face of a ridge toward the axis and of its back, in
/// ridge spacings.
const PRISM_FRONT: f64 = 0.25;
const PRISM_BACK: f64 = 0.55;

/// The lowest and the highest ridge, in meters.
const PRISM_HEIGHT_M: (f64, f64) = (150.0, 400.0);

/// The width of the forearc basin as a part of the width of the wall, and
/// its depth as a part of the depth of the axis. The width changes along
/// the axis by `BASIN_VARY`, with a wavelength of `BASIN_VARY_KM`.
const BASIN_WIDTH: f64 = 0.8;
const BASIN_DEPTH: f64 = 0.2;
const BASIN_VARY: f64 = 0.2;
const BASIN_VARY_KM: f64 = 150.0;

/// The height of the roughness of an eroding wall, as a part of the depth
/// of the axis.
const ROUGH_PART: f64 = 0.06;

/// The octaves of the roughness: the size along the axis and across it in
/// kilometers, and the weight. The roughness is longer down the wall, as
/// gullies are.
const ROUGH_OCTAVES: [[f64; 3]; 3] = [[12.0, 25.0, 1.0], [6.0, 12.0, 0.5], [3.0, 6.0, 0.25]];

/// The length of the cells along the axis that each have one slide scar,
/// in kilometers.
const SCAR_CELL_KM: f64 = 30.0;

/// The narrowest and the widest scar in kilometers, and the narrowest in
/// texels.
const SCAR_WIDTH_KM: (f64, f64) = (8.0, 20.0);
const SCAR_TEXELS: f64 = 4.0;

/// The depth of a scar as a part of the depth of the axis.
const SCAR_DEPTH: f64 = 0.1;

/// The place of the headwall at the middle of a scar, from the lowest to
/// the highest, as a part of the wall width.
const SCAR_HEAD: (f64, f64) = (0.65, 0.85);

/// The length of a scar down the wall, as a part of the wall width.
const SCAR_LENGTH: f64 = 0.5;

/// The width of a headwall, in kilometers and in texels at the least.
const SCAR_HEAD_KM: f64 = 1.0;
const SCAR_HEAD_TEXELS: f64 = 1.5;

const SALT_DEEP: u32 = 0x7472_6e63;
const SALT_FAULT: u32 = 0x666c_7473;
const SALT_PRISM: u32 = 0x7072_736d;
const SALT_ROUGH: u32 = 0x7267_6877;
const SALT_SCAR: u32 = 0x7363_7273;

/// The channels of the hashes and the noise.
const DEEP_WIDE: u32 = 0;
const DEEP_FINE: u32 = 1;
const BEND: u32 = 2;
const WALL_CHANNEL: u32 = 3;
const BASIN: u32 = 4;
const PLACE: u32 = 0;
const THROW: u32 = 1;
const KIND: u32 = 2;
const WANDER: u32 = 3;
const FADE: u32 = 4;
const PHASE: u32 = 0;
const BREAK: u32 = 1;
const SHIFT: u32 = 2;
const HEIGHT: u32 = 3;
const SIDE: u32 = 4;
const CENTER: u32 = 0;
const WIDTH: u32 = 1;
const HEAD: u32 = 2;
const DEPTH: u32 = 3;

/// The side of the overriding plate, from the direction of the stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrenchSide {
    Left,
    Right,
}

/// The kind of the margin of the overriding plate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrenchMargin {
    /// A prism of thrust ridges on the inner wall and a forearc basin
    /// behind it.
    Accretionary,
    /// A steep, rough inner wall with slide scars.
    Eroding,
}

/// The settings of a trench stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trench {
    /// The brush radius as an angle, in radians. The ground farther from
    /// the line than 1.5 radii does not change.
    pub radius: f64,
    /// The depth of the axis below the floor at full flow, in meters.
    pub depth_m: f64,
    /// The age of the incoming plate in millions of years. It sets the
    /// width of the bend.
    pub age_myr: f64,
    pub side: TrenchSide,
    pub margin: TrenchMargin,
    /// The radius of the world, in kilometers.
    pub world_km: f64,
    /// The seed of the pattern.
    pub seed: u32,
    pub limits: Limits,
}

/// The line of one trench stroke.
#[derive(Clone, Debug)]
pub struct TrenchLine {
    spec: Trench,
    line: StrokeLine,
}

/// The part of the heightmap that changes after new points of a line, and
/// the segments that give the levels there.
#[derive(Clone, Debug)]
pub struct TrenchPlan {
    /// The settings, inside their limits.
    pub trench: Trench,
    pub line: LinePlan,
}

impl Deref for TrenchPlan {
    type Target = LinePlan;

    fn deref(&self) -> &LinePlan {
        &self.line
    }
}

impl TrenchLine {
    /// Starts a line with no points on a heightmap with `face_size` texels
    /// along one side of a face. The settings go inside their limits.
    pub fn new(spec: Trench, face_size: usize) -> TrenchLine {
        let step = FRAC_PI_2 / face_size as f64;
        let radius = spec.radius.max(4.0 * step).min(MAX_TRENCH_RADIUS);
        let spec = Trench {
            radius,
            depth_m: spec.depth_m.clamp(0.0, ELEV_MAX - ELEV_MIN),
            age_myr: spec.age_myr.clamp(0.0, 200.0),
            world_km: spec.world_km.max(1.0),
            ..spec
        };
        TrenchLine {
            spec,
            line: StrokeLine::new(radius, face_size),
        }
    }

    /// The settings, inside their limits.
    pub fn spec(&self) -> Trench {
        self.spec
    }

    /// Adds a point with a flow from 0 to 1. A point closer to the last
    /// point than a quarter of a texel step is not added.
    pub fn push(&mut self, dir: V3, flow: f64) {
        self.line.push(dir, flow);
    }

    /// The number of points.
    pub fn len(&self) -> usize {
        self.line.len()
    }

    pub fn is_empty(&self) -> bool {
        self.line.is_empty()
    }

    /// The plan for the points from `from` on, as `StrokeLine::plan`.
    pub fn plan(&self, from: usize) -> TrenchPlan {
        TrenchPlan {
            trench: self.spec,
            line: self.line.plan(from),
        }
    }

    /// As `StrokeLine::margin`.
    #[doc(hidden)]
    pub fn margin(&self, p: V3) -> f64 {
        self.line.margin(p)
    }
}

/// The settings of the profile across the axis at one place along it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Section {
    /// The depth of the axis below the floor, in meters.
    pub depth: f64,
    /// The distance from the axis to the zero of the outer wall, in
    /// kilometers: π/4 times the flexural parameter.
    pub bend: f64,
    /// The flow times the taper times the window of the brush.
    pub amp: f64,
}

impl TrenchPlan {
    /// The flexural parameter of the incoming plate, in kilometers, from
    /// the elastic thickness at its age.
    pub(crate) fn alpha_km(&self) -> f64 {
        let te = elastic_km(self.trench.age_myr) * 1000.0;
        let rigidity = YOUNG * te.powi(3) / (12.0 * (1.0 - POISSON * POISSON));
        (4.0 * rigidity / ((MANTLE - WATER) * GRAVITY)).powf(0.25) / 1000.0
    }

    /// The width of a texel at the center of a face, in kilometers.
    pub(crate) fn texel_km(&self) -> f64 {
        FRAC_PI_2 / self.line.face_size as f64 * self.trench.world_km
    }

    /// 1 if the incoming plate is on the left of the line, and -1 if it is
    /// on the right.
    pub(crate) fn toward(&self) -> f64 {
        match self.trench.side {
            TrenchSide::Left => -1.0,
            TrenchSide::Right => 1.0,
        }
    }

    fn seed(&self, salt: u32) -> u32 {
        self.trench.seed ^ salt
    }

    /// The profile settings at a distance along the line in kilometers, for
    /// the weight `amp` of the brush.
    pub(crate) fn section(&self, u_km: f64, amp: f64) -> Section {
        let seed = self.seed(SALT_DEEP);
        let field = 1.0
            + DEEPS[0].1 * noise1(u_km / DEEPS[0].0, DEEP_WIDE, 0, seed).0
            + DEEPS[1].1 * noise1(u_km / DEEPS[1].0, DEEP_FINE, 0, seed).0;
        let vary = 1.0 + BEND_VARY * noise1(u_km / BEND_VARY_KM, BEND, 0, seed).0;
        Section {
            depth: self.trench.depth_m * field * amp,
            bend: FRAC_PI_4 * self.alpha_km() * vary,
            amp,
        }
    }

    /// The change of the level in meters at a distance `x` in kilometers
    /// from the axis toward the incoming plate, at a distance along the line
    /// in kilometers. A distance below zero is on the overriding plate.
    pub(crate) fn profile(&self, u_km: f64, x: f64, s: Section) -> f64 {
        let half = FLOOR_KM + 0.5 * self.texel_km();
        if x >= 0.0 {
            let xp = (x - half).max(0.0);
            s.depth * caldwell(xp / s.bend - 1.0) + self.faults(u_km, xp, s)
        } else {
            let yp = (-x - half).max(0.0);
            self.overriding(u_km, yp, s)
        }
    }

    /// The relief of the bending faults at a distance `xp` in kilometers
    /// from the edge of the axis floor, in meters.
    pub(crate) fn faults(&self, u_km: f64, xp: f64, s: Section) -> f64 {
        let texel = self.texel_km();
        let seed = self.seed(SALT_FAULT);
        let spacing = FAULT_KM.max(FAULT_TEXELS * texel);
        let scarp = SCARP_KM.max(texel);
        let most = (THROW_PART * s.depth).min(THROW_MAX_M * s.amp);
        let wander = FAULT_WANDER * spacing * noise1(u_km / FAULT_WANDER_KM, WANDER, 0, seed).0;
        let place = |j: i32| {
            let jitter = FAULT_JITTER * (2.0 * unit(hash(j as u32, PLACE, 0, seed)) - 1.0);
            spacing * (f64::from(j) + jitter) + wander
        };
        let k = ((xp - wander) / spacing).floor() as i32;
        let mut sum = 0.0;
        for j in k - 1..=k + 1 {
            let (a, b) = (place(j), place(j + 1));
            let window = smoothstep(a - 0.5 * scarp, a + 0.5 * scarp, xp)
                - smoothstep(b - 0.5 * scarp, b + 0.5 * scarp, xp);
            if window <= 0.0 {
                continue;
            }
            // The throw grows from the outer rise to the axis, and each
            // fault dies out along the axis.
            let middle = 0.5 * (a + b) / s.bend - 1.0;
            let grow = 1.0 - smoothstep(-1.0, 1.0, middle);
            let fade = noise1(u_km / FAULT_LENGTH_KM, j as u32, FADE, seed).0;
            let throw = most
                * grow
                * smoothstep(-0.3, 0.3, fade)
                * (0.6 + 0.4 * unit(hash(j as u32, THROW, 0, seed)));
            // A tilted block steps down toward the axis at its scarp.
            let kind = unit(hash(j as u32, KIND, 0, seed));
            let block = if kind < TILTED {
                let t = ((xp - a) / (b - a)).clamp(0.0, 1.0);
                throw * (0.5 - t)
            } else if kind < TILTED + GRABENS {
                -0.5 * throw
            } else {
                0.5 * throw
            };
            sum += window * block;
        }
        sum * smoothstep(0.0, spacing, xp)
    }

    /// The width of the inner wall in kilometers.
    pub(crate) fn wall_km(&self, u_km: f64, s: Section) -> f64 {
        let part = match self.trench.margin {
            TrenchMargin::Accretionary => WALL[0],
            TrenchMargin::Eroding => WALL[1],
        };
        let vary = noise1(u_km / WALL_VARY_KM, WALL_CHANNEL, 0, self.seed(SALT_DEEP)).0;
        part * s.bend * (1.0 + WALL_VARY * vary)
    }

    /// The change on the overriding plate at a distance `yp` in kilometers
    /// from the edge of the axis floor, in meters.
    pub(crate) fn overriding(&self, u_km: f64, yp: f64, s: Section) -> f64 {
        let wall = self.wall_km(u_km, s);
        let mut change = -s.depth * (1.0 - smoothstep(0.0, wall, yp));
        match self.trench.margin {
            TrenchMargin::Accretionary => {
                let vary = noise1(u_km / BASIN_VARY_KM, BASIN, 0, self.seed(SALT_DEEP)).0;
                let width = BASIN_WIDTH * wall * (1.0 + BASIN_VARY * vary);
                let basin = smoothstep(0.95 * wall, wall + 0.3 * width, yp)
                    * (1.0 - smoothstep(wall + 0.7 * width, wall + width, yp));
                let toe = smoothstep(0.0, 0.15 * wall, yp);
                let top = 1.0 - smoothstep(0.9 * wall, 1.1 * wall, yp);
                change += self.prism(u_km, yp) * s.amp * toe * top - BASIN_DEPTH * s.depth * basin;
            }
            TrenchMargin::Eroding => {
                let t = (yp / wall).clamp(0.0, 1.0);
                let rough = self.roughness(u_km, yp);
                change += ROUGH_PART * s.depth * rough * 4.0 * t * (1.0 - t);
                change -= self.scars(u_km, yp, wall, s);
            }
        }
        change
    }

    /// The thrust ridges of an accretionary prism, in meters, at a distance
    /// `yp` in kilometers from the edge of the axis floor.
    pub(crate) fn prism(&self, u_km: f64, yp: f64) -> f64 {
        let seed = self.seed(SALT_PRISM);
        let spacing = PRISM_KM.max(PRISM_TEXELS * self.texel_km());
        let piece = PRISM_PIECE_KM;
        let overlap = PRISM_OVERLAP * piece;
        let side = if hash(0, 0, SIDE, seed) & 1 == 0 {
            1.0
        } else {
            -1.0
        };
        let tilt = side * PRISM_ECHELON * spacing / piece;
        let h = |k: i32, j: i32, c: u32| hash(k as u32, j as u32, c, seed);
        let start = |k: i32, j: i32, phase: f64| {
            let jitter = PRISM_BREAK * (2.0 * unit(h(k, j, BREAK)) - 1.0);
            (f64::from(j) + phase + jitter) * piece
        };
        let k0 = (yp / spacing).round() as i32;
        let mut best: f64 = 0.0;
        for k in k0 - 2..=k0 + 2 {
            let phase = unit(h(k, 0, PHASE));
            let wander = PRISM_WANDER * noise1(u_km / PRISM_WANDER_KM, k as u32, WANDER, seed).0;
            let j0 = (u_km / piece - phase).floor() as i32;
            for j in j0 - 1..=j0 + 1 {
                let (b0, b1) = (start(k, j, phase), start(k, j + 1, phase));
                let along = smoothstep(b0 - overlap, b0 + overlap, u_km)
                    * (1.0 - smoothstep(b1 - overlap, b1 + overlap, u_km));
                if along <= 0.0 {
                    continue;
                }
                let shift = PRISM_SHIFT * (2.0 * unit(h(k, j, SHIFT)) - 1.0);
                let center =
                    spacing * (f64::from(k) + shift + wander) + tilt * (u_km - 0.5 * (b0 + b1));
                let d = yp - center;
                let width = (if d < 0.0 { PRISM_FRONT } else { PRISM_BACK }) * spacing;
                let height = PRISM_HEIGHT_M.0
                    + (PRISM_HEIGHT_M.1 - PRISM_HEIGHT_M.0) * unit(h(k, j, HEIGHT));
                best = best.max(height * along * bump(d.abs() / width));
            }
        }
        best
    }

    /// The roughness of an eroding wall, from about -1 to 1.
    pub(crate) fn roughness(&self, u_km: f64, yp: f64) -> f64 {
        let seed = self.seed(SALT_ROUGH);
        let (mut sum, mut total) = (0.0, 0.0);
        for (i, [along, across, weight]) in ROUGH_OCTAVES.into_iter().enumerate() {
            sum += weight * noise2(u_km / along, yp / across, i as u32, seed);
            total += weight;
        }
        sum / total
    }

    /// The depth of the slide scars of an eroding wall in meters, at a
    /// distance `yp` in kilometers from the edge of the axis floor. The
    /// wall is `wall` kilometers wide.
    pub(crate) fn scars(&self, u_km: f64, yp: f64, wall: f64, s: Section) -> f64 {
        let seed = self.seed(SALT_SCAR);
        let texel = self.texel_km();
        let head_width = SCAR_HEAD_KM.max(SCAR_HEAD_TEXELS * texel);
        let length = SCAR_LENGTH * wall;
        let c0 = (u_km / SCAR_CELL_KM).floor() as i32;
        let mut deepest: f64 = 0.0;
        for c in c0 - 1..=c0 + 1 {
            let r = |channel: u32| unit(hash(c as u32, channel, 0, seed));
            let center = (f64::from(c) + 0.2 + 0.6 * r(CENTER)) * SCAR_CELL_KM;
            let width = (SCAR_WIDTH_KM.0 + (SCAR_WIDTH_KM.1 - SCAR_WIDTH_KM.0) * r(WIDTH))
                .max(SCAR_TEXELS * texel);
            let q = (u_km - center) / (0.5 * width);
            let lateral = bump(q);
            if lateral <= 0.0 {
                continue;
            }
            // The headwall curves down the wall at the sides of the scar.
            let head = wall * (SCAR_HEAD.0 + (SCAR_HEAD.1 - SCAR_HEAD.0) * r(HEAD))
                - 0.35 * length * q * q;
            let below = 1.0 - smoothstep(head - head_width, head, yp);
            let down = smoothstep(head - length, head - 0.3 * length, yp);
            let depth = SCAR_DEPTH * s.depth * (0.7 + 0.6 * r(DEPTH));
            deepest = deepest.max(depth * lateral * below * down);
        }
        deepest
    }

    /// The change of the level in meters at a place on the line.
    pub(crate) fn change_m(&self, at: LinePoint) -> f64 {
        let r = self.trench.radius;
        let km = self.trench.world_km;
        let window = 1.0 - smoothstep(r, REACH * r, at.across.hypot(at.past));
        let taper = smoothstep(0.0, TAPER * r, at.end);
        let amp = at.flow * taper * window;
        if amp <= 0.0 {
            return 0.0;
        }
        let u_km = at.u * km;
        let section = self.section(u_km, amp);
        self.profile(u_km, self.toward() * at.across * km, section)
    }

    /// The new level of a texel with the level `start` from before the
    /// stroke, at a place on the line.
    pub(crate) fn level(&self, start: u16, at: LinePoint) -> u16 {
        let change = self.change_m(at) * LEVELS_PER_M;
        let old = f64::from(start);
        let new = (old + (change + 0.5).floor()).clamp(0.0, 65535.0);
        self.trench.limits.apply(old, new) as u16
    }
}

/// The universal elastic profile of a plate at a trench (Caldwell and
/// others 1976), as a part of the depth of the axis. `x` is the distance
/// from the zero of the profile over the distance from the axis to that
/// zero, so the axis is at -1 with a value of -1, and the outer rise is at
/// 1 with a value of e^(-π/2).
pub(crate) fn caldwell(x: f64) -> f64 {
    SQRT_2 * (-FRAC_PI_4 * (x + 1.0)).exp() * (FRAC_PI_4 * x).sin()
}

/// Value noise of two numbers with cells of size 1, from -1 to 1. The
/// value at each lattice point is from a hash, and a smoothstep joins the
/// values.
pub(crate) fn noise2(x: f64, y: f64, octave: u32, seed: u32) -> f64 {
    let (i, j) = (x.floor(), y.floor());
    let (sx, sy) = (x - i, y - j);
    let (i, j) = (i as i32, j as i32);
    let value = |a: i32, b: i32| 2.0 * unit(hash(a as u32, b as u32, octave, seed)) - 1.0;
    let (tx, ty) = (sx * sx * (3.0 - 2.0 * sx), sy * sy * (3.0 - 2.0 * sy));
    let low = value(i, j) + (value(i.wrapping_add(1), j) - value(i, j)) * tx;
    let high = value(i, j.wrapping_add(1))
        + (value(i.wrapping_add(1), j.wrapping_add(1)) - value(i, j.wrapping_add(1))) * tx;
    low + (high - low) * ty
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::{FACES, meters_to_level};
    use crate::heightmap::Heightmap;
    use crate::line::LINE_SPACING;
    use crate::math::lonlat_to_dir;

    const EARTH_KM: f64 = 6371.0;

    fn spec(margin: TrenchMargin) -> Trench {
        Trench {
            radius: 0.05,
            depth_m: 2500.0,
            age_myr: 120.0,
            side: TrenchSide::Left,
            margin,
            world_km: EARTH_KM,
            seed: 11,
            limits: Limits::default(),
        }
    }

    /// A line east along the equator from `from` to `to` degrees of
    /// longitude, with the point spacing of the app, at full flow.
    fn straight(spec: Trench, n: usize, from: f64, to: f64) -> TrenchLine {
        let mut line = TrenchLine::new(spec, n);
        let step = (LINE_SPACING * line.spec().radius).to_degrees();
        let count = ((to - from) / step).ceil() as usize;
        for i in 0..=count {
            let lon = from + (to - from) * i as f64 / count as f64;
            line.push(lonlat_to_dir(lon, 0.0), 1.0);
        }
        line
    }

    /// The plan of a long straight line on an Earth with texels near
    /// 920 m, for the profile alone.
    fn plan(margin: TrenchMargin) -> TrenchPlan {
        straight(spec(margin), 10752, -20.0, 20.0).plan(0)
    }

    /// A place far from the ends, `u_km` along the line and `x` kilometers
    /// from the axis toward the incoming plate.
    fn at(plan: &TrenchPlan, u_km: f64, x: f64) -> LinePoint {
        let km = plan.trench.world_km;
        LinePoint {
            u: u_km / km,
            across: x / km * plan.toward(),
            past: 0.0,
            flow: 1.0,
            end: 1.0,
        }
    }

    /// Lays a line in one stroke.
    fn lay(map: &mut Heightmap, line: &TrenchLine) {
        map.begin_stroke();
        let mut from = 0;
        while from < line.len() {
            let plan = line.plan(from);
            map.trench(&plan);
            from = plan.end;
        }
        map.end_stroke();
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

    #[test]
    fn the_caldwell_profile_has_its_axis_and_its_outer_rise() {
        assert!((caldwell(-1.0) + 1.0).abs() < 1e-12);
        assert!(caldwell(0.0).abs() < 1e-12);
        let rise = (-FRAC_PI_2).exp();
        assert!((caldwell(1.0) - rise).abs() < 1e-12);
        for x in [0.5, 0.9, 1.1, 1.5, 2.5] {
            assert!(caldwell(x) < caldwell(1.0), "at {x}");
        }
    }

    #[test]
    fn the_incoming_plate_follows_the_caldwell_profile() {
        let plan = plan(TrenchMargin::Accretionary);
        let half = FLOOR_KM + 0.5 * plan.texel_km();
        for u_km in [500.0, 900.0, 1700.0] {
            let s = plan.section(u_km, 1.0);
            for i in 0..60 {
                let xp = 4.0 * f64::from(i);
                let change = plan.change_m(at(&plan, u_km, xp + half));
                let faults = plan.faults(u_km, xp, s);
                let expected = s.depth * caldwell(xp / s.bend - 1.0);
                assert!(
                    (change - faults - expected).abs() < 1e-6,
                    "at {u_km} km, {xp} km: {change} m, {faults} m of faults, {expected} m"
                );
            }
        }
        // The age sets the width: the flexural parameter of plate of 100
        // million years is near 75 km.
        let old = TrenchPlan {
            trench: Trench {
                age_myr: 100.0,
                ..plan.trench
            },
            ..plan.clone()
        };
        assert!((old.alpha_km() - 75.0).abs() < 3.0, "{}", old.alpha_km());
        let young = TrenchPlan {
            trench: Trench {
                age_myr: 10.0,
                ..plan.trench
            },
            ..plan
        };
        assert!(young.alpha_km() < 0.5 * old.alpha_km());
    }

    #[test]
    fn on_old_floor_the_outer_rise_is_a_few_hundred_meters_high_and_100_to_200_km_out() {
        let n = 2048;
        let floor = meters_to_level(-3000.0);
        let mut map = Heightmap::new(n, floor);
        let line = straight(spec(TrenchMargin::Accretionary), n, -20.0, 20.0);
        lay(&mut map, &line);
        // The mean change across the line, from many places along it.
        let texel_km = FRAC_PI_2 / n as f64 * EARTH_KM;
        let mut best = (f64::MIN, 0.0);
        for i in 0..60 {
            let x = 30.0 + texel_km * f64::from(i);
            let mut sum = 0.0;
            for k in 0..200 {
                let lon = -12.0 + 24.0 * f64::from(k) / 200.0;
                let level = map.sample(lonlat_to_dir(lon, -(x / EARTH_KM).to_degrees()));
                sum += (f64::from(level) - f64::from(floor)) / LEVELS_PER_M;
            }
            let mean = sum / 200.0;
            if mean > best.0 {
                best = (mean, x);
            }
        }
        let (height, x) = best;
        assert!(height > 200.0 && height < 900.0, "a rise of {height} m");
        assert!(x > 100.0 && x < 200.0, "the rise at {x} km");
    }

    /// The RMS of the fault relief over the line in a band of the profile
    /// coordinate of `caldwell`.
    fn fault_rms(plan: &TrenchPlan, band: (f64, f64)) -> (f64, f64) {
        let (mut sum, mut count, mut most) = (0.0, 0.0, 0.0f64);
        for k in 0..400 {
            let u_km = 300.0 + 7.3 * f64::from(k);
            let s = plan.section(u_km, 1.0);
            for i in 0..80 {
                let x = band.0 + (band.1 - band.0) * f64::from(i) / 80.0;
                let f = plan.faults(u_km, (x + 1.0) * s.bend, s);
                (sum, count, most) = (sum + f * f, count + 1.0, most.max(f.abs()));
            }
        }
        ((sum / count).sqrt(), most)
    }

    #[test]
    fn the_faults_start_near_the_outer_rise_and_grow_toward_the_axis() {
        let plan = plan(TrenchMargin::Accretionary);
        let (beyond, _) = fault_rms(&plan, (1.2, 2.5));
        let (outer, _) = fault_rms(&plan, (0.2, 0.7));
        let (inner, most) = fault_rms(&plan, (-0.8, -0.3));
        assert!(beyond < 1e-9, "{beyond} m past the outer rise");
        assert!(outer > 5.0, "{outer} m on the outer slope");
        assert!(
            inner > 2.0 * outer,
            "{inner} m near the axis, {outer} m out"
        );
        assert!(most <= THROW_MAX_M, "a throw of {most} m");
    }

    #[test]
    fn the_faults_break_the_slope_into_blocks_along_the_axis() {
        // Along a line across the slope, the relief steps at the faults;
        // along the axis, a fault keeps its place for a while and dies out.
        let plan = plan(TrenchMargin::Accretionary);
        let s = plan.section(1000.0, 1.0);
        let x = 0.3 * s.bend;
        let across: Vec<f64> = (0..400)
            .map(|i| plan.faults(1000.0, x + 0.1 * f64::from(i), s))
            .collect();
        let steps = across
            .windows(2)
            .filter(|w| (w[1] - w[0]).abs() > 10.0)
            .count();
        assert!(steps > 5, "{steps} steep steps across the slope");
        let along: Vec<f64> = (0..400)
            .map(|i| plan.faults(1000.0 + 0.5 * f64::from(i), x, s))
            .collect();
        let jumps = along
            .windows(2)
            .filter(|w| (w[1] - w[0]).abs() > 10.0)
            .count();
        assert!(jumps < steps / 2, "{jumps} steep steps along the axis");
        let (low, high) = along
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(high - low > 20.0, "the faults do not change along the axis");
    }

    #[test]
    fn a_seamount_on_the_incoming_plate_goes_down_with_it() {
        let n = 1024;
        let floor = meters_to_level(-3000.0);
        let line = straight(spec(TrenchMargin::Accretionary), n, -10.0, 10.0);
        let mut flat = Heightmap::new(n, floor);
        let mut mount = Heightmap::new(n, floor);
        // A cone 1,500 m high and 40 km in radius, 60 km from the axis.
        let center = lonlat_to_dir(0.0, -(60.0 / EARTH_KM).to_degrees());
        let mut cone = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let d = crate::math::angle(mount.texel_dir(0, x, y), center) * EARTH_KM;
                if d < 40.0 {
                    let height = 1500.0 * (1.0 - d / 40.0) * LEVELS_PER_M;
                    let level = floor + height as u16;
                    mount.set(0, x, y, level);
                    cone.push((x, y, level - floor));
                }
            }
        }
        assert!(cone.len() > 20);
        lay(&mut flat, &line);
        lay(&mut mount, &line);
        for (x, y, height) in cone {
            let (a, b) = (
                flat.get(0, x as i64, y as i64),
                mount.get(0, x as i64, y as i64),
            );
            assert_eq!(b - a, height, "at {x}, {y}");
            assert!(a != floor, "no change at {x}, {y}");
        }
    }

    /// The largest slope of the mean profile on each side of the axis, in
    /// meters per kilometer: the incoming plate and the overriding plate.
    fn wall_slopes(plan: &TrenchPlan) -> (f64, f64) {
        let mean = |x: f64| {
            let sum: f64 = (0..200)
                .map(|k| plan.change_m(at(plan, 300.0 + 11.0 * f64::from(k), x)))
                .sum();
            sum / 200.0
        };
        let steepest = |sign: f64| {
            (0..150)
                .map(|i| {
                    let x = sign * f64::from(i);
                    (mean(x + sign) - mean(x)).abs()
                })
                .fold(0.0, f64::max)
        };
        (steepest(1.0), steepest(-1.0))
    }

    #[test]
    fn the_inner_wall_is_steeper_than_the_outer_wall() {
        for margin in [TrenchMargin::Accretionary, TrenchMargin::Eroding] {
            let (outer, inner) = wall_slopes(&plan(margin));
            assert!(
                inner > 1.3 * outer,
                "{margin:?}: {inner} m/km, {outer} m/km out"
            );
        }
    }

    #[test]
    fn an_accretionary_margin_has_thrust_ridges_and_a_forearc_basin() {
        let plan = plan(TrenchMargin::Accretionary);
        let u_km = 1200.0;
        let s = plan.section(u_km, 1.0);
        let wall = plan.wall_km(u_km, s);
        let half = FLOOR_KM + 0.5 * plan.texel_km();
        // Ridges on the wall: local highs of the prism across the axis.
        let prism: Vec<f64> = (0..200)
            .map(|i| plan.prism(u_km, wall * f64::from(i) / 200.0))
            .collect();
        let crests = prism
            .windows(3)
            .filter(|w| w[1] > w[0] && w[1] >= w[2] && w[1] > 100.0)
            .count();
        assert!(crests >= 2, "{crests} thrust ridges");
        // The basin behind the wall is lower than the top of the wall.
        let change = |y: f64| plan.change_m(at(&plan, u_km, -(y + half)));
        let basin = change(wall + 0.5 * BASIN_WIDTH * wall);
        let top = change(0.95 * wall);
        assert!(basin < -0.12 * s.depth, "a basin {basin} m deep");
        assert!(top > basin + 0.1 * s.depth, "the top at {top} m");
        // The eroding margin has no basin.
        let eroding = TrenchPlan {
            trench: Trench {
                margin: TrenchMargin::Eroding,
                ..plan.trench
            },
            ..plan.clone()
        };
        let wall = eroding.wall_km(u_km, s);
        let past = eroding.change_m(at(&eroding, u_km, -(1.4 * wall + half)));
        assert!(past.abs() < 0.01 * s.depth, "{past} m past an eroding wall");
    }

    /// The RMS of the second difference of the change along the axis, with
    /// steps of 1.5 km, on the inner wall.
    fn wall_roughness(plan: &TrenchPlan) -> f64 {
        let (mut sum, mut count) = (0.0, 0.0);
        for i in 0..10 {
            let s = plan.section(1000.0, 1.0);
            let y = plan.wall_km(1000.0, s) * (0.1 + 0.08 * f64::from(i));
            for k in 0..600 {
                let u = 700.0 + f64::from(k);
                let f = |u: f64| plan.change_m(at(plan, u, -y));
                let d = f(u + 1.5) - 2.0 * f(u) + f(u - 1.5);
                (sum, count) = (sum + d * d, count + 1.0);
            }
        }
        (sum / count).sqrt()
    }

    #[test]
    fn an_eroding_wall_is_steeper_and_rougher_with_slide_scars() {
        let (accretionary, eroding) = (
            plan(TrenchMargin::Accretionary),
            plan(TrenchMargin::Eroding),
        );
        let (_, inner_a) = wall_slopes(&accretionary);
        let (_, inner_e) = wall_slopes(&eroding);
        assert!(inner_e > 1.3 * inner_a, "{inner_e} m/km, {inner_a} m/km");
        let (rough_a, rough_e) = (wall_roughness(&accretionary), wall_roughness(&eroding));
        assert!(rough_e > 1.5 * rough_a, "{rough_e} m, {rough_a} m");
        // Scars cut the upper wall in some places along the axis.
        let s = eroding.section(1000.0, 1.0);
        let wall = eroding.wall_km(1000.0, s);
        let cut = (0..400)
            .map(|k| eroding.scars(500.0 + f64::from(k), 0.55 * wall, wall, s))
            .filter(|&d| d > 0.04 * s.depth)
            .count();
        assert!(cut > 40 && cut < 360, "{cut} km of 400 with scars");
    }

    #[test]
    fn the_axis_depth_changes_along_the_line() {
        let plan = plan(TrenchMargin::Accretionary);
        let depths: Vec<f64> = (0..300)
            .map(|k| -plan.change_m(at(&plan, 200.0 + 10.0 * f64::from(k), 0.0)))
            .collect();
        let (low, high) = depths
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(low > 0.0);
        assert!(high - low > 0.3 * 2500.0, "from {low} m to {high} m");
    }

    #[test]
    fn the_trench_is_shallower_toward_each_end_and_stays_inside_its_reach() {
        let n = 1024;
        let floor = meters_to_level(-3000.0);
        let mut map = Heightmap::new(n, floor);
        let line = straight(spec(TrenchMargin::Eroding), n, -10.0, 10.0);
        lay(&mut map, &line);
        let r = line.spec().radius;
        let axis = |lon: f64| floor - map.sample(lonlat_to_dir(lon, 0.0));
        let middle = axis(0.0);
        let near_end = axis(10.0 - 0.3 * r.to_degrees());
        assert!(near_end < middle / 3, "{near_end} near the end, {middle}");
        assert_eq!(axis(10.0 + 0.2 * r.to_degrees()), 0);
        let reach = REACH * r;
        let mut changed = 0;
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    let level = map.get(face, x as i64, y as i64);
                    let p = map.texel_dir(face, x, y);
                    let (lon, lat) = crate::math::dir_to_lonlat(p);
                    let to_line = if lon.abs() <= 10.0 {
                        lat.to_radians().abs()
                    } else {
                        crate::math::angle(p, lonlat_to_dir(10f64.copysign(lon), 0.0))
                    };
                    if to_line > reach {
                        assert_eq!(level, floor, "a texel past the reach");
                    }
                    changed += usize::from(level != floor);
                }
            }
        }
        assert!(changed > 10_000, "{changed} texels changed");
    }

    #[test]
    fn equal_strokes_give_equal_levels() {
        let n = 512;
        let floor = meters_to_level(-3000.0);
        let line = straight(spec(TrenchMargin::Accretionary), n, -5.0, 8.0);
        let (mut a, mut b) = (Heightmap::new(n, floor), Heightmap::new(n, floor));
        lay(&mut a, &line);
        lay(&mut b, &line);
        assert_eq!(levels(&a), levels(&b));
        let other = straight(
            Trench {
                seed: 12,
                ..spec(TrenchMargin::Accretionary)
            },
            n,
            -5.0,
            8.0,
        );
        let mut c = Heightmap::new(n, floor);
        lay(&mut c, &other);
        assert_ne!(levels(&a), levels(&c));
    }
}
