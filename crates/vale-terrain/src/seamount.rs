//! The seamounts brush.
//!
//! The volcanoes are the points of a seeded field on the sphere. A stamp
//! grows the volcanoes under the brush toward their full height. A texel
//! gets its level from before the stroke plus each volcano near it, times
//! the growth of that volcano. Thus a stamp over grown volcanoes changes
//! nothing, and a stamp adds no new volcanoes where the field has none.

use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use crate::cube::{ELEV_MAX, ELEV_MIN, FACES, face_dir, unwarp};
use crate::heightmap::{Heightmap, Limits, TexelRect, brush_rect, falloff, hash, noise3};
use crate::line::{smoothstep, unit};
use crate::math::{V3, angle, cross, dot, normalize};

/// The number of 32-bit numbers of one volcano in `SeamountPlan::floats`.
pub const VOLCANO_FLOATS: usize = 64;

/// The largest number of volcanoes in one plan.
pub const PLAN_VOLCANOES: usize = 256;

/// The settings of a seamounts stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seamounts {
    /// The number of seamounts of each height for each 10,000 km².
    pub density: f64,
    /// The characteristic height in meters. The number of seamounts above a
    /// height falls by a factor of e for each step of this height.
    pub height_m: f64,
    /// The radius of the world, in kilometers.
    pub world_km: f64,
    /// The seed of the field.
    pub seed: u32,
    pub limits: Limits,
}

/// The sea floor under a stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeaFloor {
    /// The age of the floor in millions of years.
    pub age_myr: f64,
    /// The direction of the abyssal hills, a unit vector square to the
    /// center of the window.
    pub fabric: V3,
    /// How strongly the slopes point in one direction, from 0 to 1. Below
    /// `FABRIC_COHERENCE`, `fabric` points north.
    pub coherence: f64,
}

/// One volcano of a plan, as 32-bit numbers. `SeamountPlan::floats` sends
/// them to the shader, which reads the same places.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volcano(pub [f32; VOLCANO_FLOATS]);

/// The texels that one stamp changes, and the volcanoes that give the
/// levels there.
#[derive(Clone, Debug)]
pub struct SeamountPlan {
    pub limits: Limits,
    /// The number of texels along one side of a face.
    pub face_size: usize,
    /// The rectangle of each face that changes.
    pub rects: [Option<TexelRect>; FACES],
    /// Each volcano that reaches a rectangle, with its growth. The volcanoes
    /// that the stamp grew come first.
    pub volcanoes: Vec<Volcano>,
    /// The number of volcanoes that the stamp grew.
    pub grown: usize,
}

/// The volcanoes of one stroke, and their growth.
#[derive(Clone, Debug)]
pub struct SeamountStroke {
    spec: Seamounts,
    face_size: usize,
    floor: Option<SeaFloor>,
    volcanoes: Vec<Volcano>,
    /// The index of each volcano in `volcanoes`, by its key in the field.
    keys: HashMap<u64, usize>,
}

// The places in `Volcano`.
const CENTER: usize = 0;
const AXIS: usize = 3;
pub(crate) const GROWTH: usize = 6;
pub(crate) const HEIGHT: usize = 7;
const RADIUS: usize = 8;
const SHIELD: usize = 9;
const TOP: usize = 10;
const CRATER_RADIUS: usize = 11;
const CRATER_DEPTH: usize = 12;
const CONES: usize = 13;
const CONE_OFFSET: usize = 14;
const CONE_HEIGHT: usize = 18;
const STRETCH: usize = 22;
const RIFTS: usize = 23;
const RIFT_ANGLE: usize = 24;
const RIFT_SIZE: usize = 28;
const OUTLINE: usize = 29;
const OUTLINE_PHASE: usize = 32;
const SLIDE: usize = 35;
const GULLIES: usize = 41;
const GULLY_SIZE: usize = 42;
const SEED: usize = 43;
pub(crate) const FLEX: usize = 44;
const FLEX_LENGTH: usize = 45;
const REACH: usize = 46;
const CONE_SIDE: usize = 48;
const CONE_TURN: usize = 52;
pub(crate) const CAP: usize = 56;
pub(crate) const RIM: usize = 57;
pub(crate) const LAGOON: usize = 58;
const EXTENT: usize = 59;
pub(crate) const SHORE: usize = 60;
pub(crate) const REEF: usize = 61;

/// The cap of a volcano with no cap.
pub(crate) const NO_CAP: f32 = 65535.0;

/// The 16-bit steps of one meter.
pub(crate) const LEVELS_PER_M: f64 = 65535.0 / (ELEV_MAX - ELEV_MIN);

/// The number of height bands of the field. A band is one characteristic
/// height. The last band holds e^-24 of the seamounts.
const BANDS: usize = 24;

/// The height of a small cone over its base radius, and the spread of that
/// ratio.
const CONE_RATIO: f64 = 0.2;
const CONE_RATIO_SPREAD: f64 = 0.05;

/// The height over the base radius of a large shield.
pub(crate) const SHIELD_RATIO: f64 = 0.09;

/// The heights in meters where a cone becomes a shield.
const SHIELD_FROM_M: f64 = 1500.0;
const SHIELD_TO_M: f64 = 3000.0;

/// The part of the small cones with a flat top, and the part with a summit
/// crater.
const FLAT_TOPS: f64 = 0.2;
const CRATERS: f64 = 0.15;

/// The part of the volcanoes that are a short ridge of 2 to 4 cones along
/// the fabric.
const RIDGES: f64 = 0.15;

/// The width of a rift zone, as an angle around the summit.
const RIFT_WIDTH: f64 = 0.3;

/// The foot of a flank curves into the floor from this part of the base
/// radius inside the base to the same part outside it.
const FOOT: f64 = 0.35;

/// The farthest reach of a debris apron, in base radii.
const APRON: f64 = 1.9;

/// The plate: Young's modulus in pascals, Poisson's ratio, the densities of
/// the mantle, the volcano, and the sea water in kg/m³, and gravity.
const YOUNG: f64 = 1.0e11;
const POISSON: f64 = 0.25;
const MANTLE: f64 = 3300.0;
const ROCK: f64 = 2800.0;
const WATER: f64 = 1030.0;
const GRAVITY: f64 = 9.81;

/// The elastic thickness in kilometers is this factor times the square root
/// of the age in millions of years (Watts 2001), inside the limits.
const ELASTIC_FACTOR: f64 = 2.7;
const ELASTIC_MIN_KM: f64 = 3.0;
const ELASTIC_MAX_KM: f64 = 40.0;

/// The moat of the volcano is deepest at its center, and it is at most this
/// part of the height.
const FLEX_MAX: f64 = 0.6;

/// The moats of a line of volcanoes add up to a moat that is narrower than
/// the moat of one volcano. The flexural length of a shield volcano of a
/// line is longer by this factor, so the moat and the arch of the line are
/// as far from the line as those of the Hawaiian chain (Watts and ten Brink
/// 1989).
const LINE_FLEX: f64 = 1.3;

/// Below this coherence, the slopes have no main direction.
pub const FABRIC_COHERENCE: f64 = 0.25;

const SALT_FIELD: u32 = 0x3c6e_f372;
const SALT_SHAPE: u32 = 0xa54f_f53a;

impl Volcano {
    pub(crate) fn get(&self, i: usize) -> f64 {
        f64::from(self.0[i])
    }

    fn v3(&self, i: usize) -> V3 {
        [self.get(i), self.get(i + 1), self.get(i + 2)]
    }

    /// The center, a unit vector.
    pub fn center(&self) -> V3 {
        self.v3(CENTER)
    }

    /// The growth, from 0 to 1.
    pub fn growth(&self) -> f64 {
        self.get(GROWTH)
    }

    /// The height of the edifice at full growth, in 16-bit steps.
    pub fn height(&self) -> f64 {
        self.get(HEIGHT)
    }

    /// The base radius as an angle, in radians.
    pub fn base_radius(&self) -> f64 {
        self.get(RADIUS)
    }

    /// The distance from the center past which the volcano changes
    /// nothing, as an angle in radians.
    pub fn reach(&self) -> f64 {
        self.get(REACH)
    }

    /// The number of rift zones.
    pub fn rifts(&self) -> usize {
        self.get(RIFTS) as usize
    }

    /// The number of cones of a ridge, 1 for a single volcano.
    pub fn cones(&self) -> usize {
        self.get(CONES) as usize
    }

    /// Whether the volcano has a flat top.
    pub fn flat_top(&self) -> bool {
        self.get(TOP) < 1.0
    }

    /// Whether the volcano has a summit crater.
    pub fn crater(&self) -> bool {
        self.get(CRATER_DEPTH) > 0.0
    }

    /// Whether the volcano is a shield.
    pub fn shield(&self) -> bool {
        self.get(SHIELD) >= 0.5
    }

    /// The change of the level at a unit direction at full growth, in 16-bit
    /// steps. The moat of the plate is negative.
    pub fn full_levels(&self, p: V3) -> f64 {
        self.edifice(p) - self.moat(p)
    }

    /// The edifice at a unit direction at full growth, in 16-bit steps.
    pub fn edifice(&self, p: V3) -> f64 {
        let c = self.center();
        let reach = self.reach();
        if dot(p, c) <= self.get(EXTENT).cos() {
            return 0.0;
        }
        let a = self.v3(AXIS);
        let b = cross(c, a);
        let (x, y) = (dot(p, a), dot(p, b));
        let d = (x * x + y * y).sqrt();
        let radius = self.get(RADIUS);
        let (u, v) = (x / radius, y / radius);
        let mut edifice: f64 = 0.0;
        for i in 0..self.cones() {
            let du = (u - self.get(CONE_OFFSET + i)) / self.get(STRETCH);
            let dv = v - self.get(CONE_SIDE + i);
            let r = (du * du + dv * dv).sqrt();
            let theta = dv.atan2(du) + self.get(CONE_TURN + i);
            edifice = edifice.max(self.get(CONE_HEIGHT + i) * self.profile(r, theta, i as u32));
        }
        edifice * self.get(HEIGHT) * (1.0 - smoothstep(0.85 * reach, reach, d))
    }

    /// The moat of the plate at a unit direction at full growth, in 16-bit
    /// steps. It is positive down.
    pub fn moat(&self, p: V3) -> f64 {
        let c = self.center();
        let reach = self.reach();
        if dot(p, c) <= reach.cos() {
            return 0.0;
        }
        let a = self.v3(AXIS);
        let b = cross(c, a);
        let (x, y) = (dot(p, a), dot(p, b));
        let d = (x * x + y * y).sqrt();
        let xi = d / self.get(FLEX_LENGTH);
        let moat = self.get(FLEX) * (-xi).exp() * (xi.cos() + xi.sin());
        moat * (1.0 - smoothstep(0.6 * reach, reach, d))
    }

    /// The level at full growth over the ground, as a part of the height,
    /// at a distance `r` from the center of a cone in base radii, at an angle
    /// `theta` from the axis. Each cone of a ridge has its own noise.
    fn profile(&self, r: f64, theta: f64, cone: u32) -> f64 {
        let mut outline = 1.0;
        for k in 0..3 {
            let phase = self.get(OUTLINE_PHASE + k);
            outline += self.get(OUTLINE + k) * ((k + 1) as f64 * theta - phase).cos();
        }
        let shield = self.get(SHIELD);
        let mut star = 0.0;
        for k in 0..self.rifts() {
            let off = wrap(theta - self.get(RIFT_ANGLE + k)) / RIFT_WIDTH;
            star += (-off * off).exp();
        }
        let star = 1.0 + self.get(RIFT_SIZE) * shield * star;
        let re = r / (outline * star);
        // The noise is long down the slope and short around the summit, so
        // its features are lava ridges and gullies.
        let around = self.get(GULLIES) / TAU;
        let seed = (self.0[SEED] as u32).wrapping_add(cone);
        let q = [theta.cos() * around, theta.sin() * around, re * 3.0];
        let n = noise3(q, seed);
        // The fine noise of the outline fades over the foot, so the foot
        // meets the floor in a smooth curve.
        let fade = 1.0 - smoothstep(1.0 - FOOT, 1.0 + FOOT, re);
        let re = re * (1.0 + 0.05 * n[0] * fade);
        let x = foot(1.0 - re);
        let mut f = x + (x.powf(1.5) - x) * shield;
        f *= 1.0 + self.get(GULLY_SIZE) * n[1] * 4.0 * re.min(1.0) * (1.0 - re).max(0.0);
        let top = self.get(TOP);
        f = f.min(top) / top;
        let crater_radius = self.get(CRATER_RADIUS);
        if crater_radius > 0.0 {
            f = f.min(1.0 - crater_radius);
            if re < crater_radius {
                let k = re / crater_radius;
                f -= self.get(CRATER_DEPTH) * (1.0 - k * k);
            }
        }
        let mut apron = 0.0;
        for s in 0..2 {
            let depth = self.get(SLIDE + 3 * s + 2);
            if depth <= 0.0 {
                continue;
            }
            let off = wrap(theta - self.get(SLIDE + 3 * s));
            let width = self.get(SLIDE + 3 * s + 1);
            let k = off / width;
            let scar = smoothstep(0.2, 0.5, re) * (1.0 - smoothstep(0.85, 1.05, re));
            f *= 1.0 - depth * (-k * k).exp() * scar;
            let k = off / (1.3 * width);
            let lobe = smoothstep(0.8, 1.0, re) * (1.0 - smoothstep(1.0, APRON, re));
            let hummocks = noise3([q[0] * 6.0, q[1] * 6.0, re * 8.0], seed ^ 1);
            apron += depth * 0.15 * (-k * k).exp() * lobe * (0.7 + 0.3 * hummocks[2]);
        }
        f.max(0.0) + apron
    }
}

impl SeamountPlan {
    /// The volcanoes as 32-bit numbers, `VOLCANO_FLOATS` for each.
    pub fn floats(&self) -> Vec<f32> {
        self.volcanoes.iter().flat_map(|v| v.0).collect()
    }

    /// The level of a texel at unit direction `p` with level `start` from
    /// before the stroke.
    pub fn level(&self, start: u16, p: V3) -> u16 {
        self.level_of(&self.volcanoes, start, p)
    }

    /// `level` with only the volcanoes in `near`.
    pub(crate) fn level_of(&self, near: &[Volcano], start: u16, p: V3) -> u16 {
        let sum: f64 = near.iter().map(|v| v.growth() * v.full_levels(p)).sum();
        let start = f64::from(start);
        let new = (start + sum).round().clamp(0.0, 65535.0);
        self.limits.apply(start, new) as u16
    }

    /// Whether the plan changes nothing.
    pub fn is_empty(&self) -> bool {
        self.rects.iter().all(Option::is_none)
    }
}

impl SeamountStroke {
    /// Starts a stroke with no volcanoes on a heightmap with `face_size`
    /// texels along one side of a face.
    pub fn new(spec: Seamounts, face_size: usize) -> SeamountStroke {
        let spec = Seamounts {
            density: spec.density.max(0.0),
            height_m: spec.height_m.max(1.0),
            world_km: spec.world_km.max(1.0),
            ..spec
        };
        SeamountStroke {
            spec,
            face_size,
            floor: None,
            volcanoes: Vec::new(),
            keys: HashMap::new(),
        }
    }

    /// The sea floor of the stroke. The first stamp sets it.
    pub fn floor(&self) -> Option<SeaFloor> {
        self.floor
    }

    /// The volcanoes that the stroke grew.
    pub fn volcanoes(&self) -> &[Volcano] {
        &self.volcanoes
    }

    /// Grows the volcanoes under a brush with its center at unit direction
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
    ) -> SeamountPlan {
        let n = self.face_size;
        let step = FRAC_PI_2 / n as f64;
        let radius = radius.max(2.0 * step);
        let floor = *self
            .floor
            .get_or_insert_with(|| sea_floor(map, center, 3.0 * radius));
        let hardness = hardness.clamp(0.0, 0.999);
        let mut changed = Vec::new();
        for (key, place, height_m) in field(&self.spec, n, center, radius) {
            let w = falloff(angle(place, center) / radius, hardness) * flow.clamp(0.0, 1.0);
            if w <= 0.0 {
                continue;
            }
            let index = match self.keys.get(&key) {
                Some(&index) => index,
                None => {
                    let Some(v) = volcano(&self.spec, &floor, key, place, height_m, radius, n)
                    else {
                        continue;
                    };
                    self.volcanoes.push(v);
                    self.keys.insert(key, self.volcanoes.len() - 1);
                    self.volcanoes.len() - 1
                }
            };
            let v = &mut self.volcanoes[index];
            let g = v.growth();
            let grown = g + w * (1.0 - g);
            if (grown as f32) != v.0[GROWTH] {
                v.0[GROWTH] = grown as f32;
                changed.push(index);
            }
        }
        let mut plan = SeamountPlan {
            limits: self.spec.limits,
            face_size: n,
            rects: [None; FACES],
            volcanoes: Vec::new(),
            grown: 0,
        };
        if changed.is_empty() {
            return plan;
        }
        // The texels that a changed volcano reaches.
        let mut far: f64 = 0.0;
        for &i in &changed {
            let v = &self.volcanoes[i];
            far = far.max(angle(v.center(), center) + v.reach());
        }
        plan.rects = std::array::from_fn(|face| brush_rect(n, face, center, far));
        // Each volcano that reaches the rectangles. The changed ones come
        // first, then the largest.
        let mut near: Vec<usize> = (0..self.volcanoes.len())
            .filter(|&i| {
                let v = &self.volcanoes[i];
                angle(v.center(), center) < far + v.reach()
            })
            .collect();
        near.sort_by(|&a, &b| {
            let (va, vb) = (&self.volcanoes[a], &self.volcanoes[b]);
            let rank = |i: usize| !changed.contains(&i);
            rank(a)
                .cmp(&rank(b))
                .then(vb.height().total_cmp(&va.height()))
                .then(a.cmp(&b))
        });
        near.truncate(PLAN_VOLCANOES);
        plan.grown = near.iter().filter(|i| changed.contains(i)).count();
        near[..plan.grown].sort_unstable();
        near[plan.grown..].sort_unstable();
        plan.volcanoes = near.into_iter().map(|i| self.volcanoes[i]).collect();
        plan
    }
}

/// The age and the fabric of the sea floor in a window with a radius in
/// radians around a unit direction. The depth gives the age by the inverse
/// of the GDH1 curve (Stein and Stein 1992). The structure tensor of the
/// slopes gives the fabric: the hills run square to the main direction of
/// the slopes.
pub fn sea_floor(map: &Heightmap, center: V3, radius: f64) -> SeaFloor {
    let step = FRAC_PI_2 / map.face_size() as f64;
    let spacing = (2.0 * step).max(radius / 32.0);
    let half = (radius / spacing).ceil().min(32.0) as i64;
    let (east, north) = tangent_frame(center);
    let side = (2 * half + 1) as usize;
    let mut depths = Vec::with_capacity(side * side);
    let mut grid = vec![0.0; side * side];
    for j in -half..=half {
        for i in -half..=half {
            let (dx, dy) = (i as f64 * spacing, j as f64 * spacing);
            let p = normalize([
                center[0] + east[0] * dx + north[0] * dy,
                center[1] + east[1] * dx + north[1] * dy,
                center[2] + east[2] * dx + north[2] * dy,
            ]);
            let m = ELEV_MIN + map.level_at(p) / LEVELS_PER_M;
            grid[(j + half) as usize * side + (i + half) as usize] = m;
            depths.push(-m);
        }
    }
    depths.sort_by(f64::total_cmp);
    let age_myr = gdh1_age(depths[depths.len() / 2]);
    let (mut jxx, mut jxy, mut jyy) = (0.0, 0.0, 0.0);
    for y in 1..side - 1 {
        for x in 1..side - 1 {
            let gx = grid[y * side + x + 1] - grid[y * side + x - 1];
            let gy = grid[(y + 1) * side + x] - grid[(y - 1) * side + x];
            jxx += gx * gx;
            jxy += gx * gy;
            jyy += gy * gy;
        }
    }
    let trace = jxx + jyy;
    let coherence = if trace > 1e-9 {
        ((jxx - jyy).powi(2) + 4.0 * jxy * jxy).sqrt() / trace
    } else {
        0.0
    };
    let fabric = if coherence < FABRIC_COHERENCE {
        north
    } else {
        // The main direction of the slopes, turned by a right angle.
        let slope = 0.5 * (2.0 * jxy).atan2(jxx - jyy) + FRAC_PI_2;
        let (s, c) = slope.sin_cos();
        normalize([
            east[0] * c + north[0] * s,
            east[1] * c + north[1] * s,
            east[2] * c + north[2] * s,
        ])
    };
    SeaFloor {
        age_myr,
        fabric,
        coherence,
    }
}

/// The age in millions of years of sea floor at a depth in meters, by the
/// inverse of the GDH1 curve. Floor shallower than the curve at age 0 is
/// new, and floor deeper than its limit is 200 million years old.
pub fn gdh1_age(depth_m: f64) -> f64 {
    let young = 2600.0 + 365.0 * 20f64.sqrt();
    let age = if depth_m <= 2600.0 {
        0.0
    } else if depth_m < young {
        ((depth_m - 2600.0) / 365.0).powi(2)
    } else if depth_m < 5651.0 {
        -((5651.0 - depth_m) / 2473.0).ln() / 0.0278
    } else {
        200.0
    };
    age.min(200.0)
}

/// The depth of the sea floor at an age in millions of years, in meters, by
/// the GDH1 curve (Stein and Stein 1992).
pub(crate) fn gdh1_depth(age: f64) -> f64 {
    if age < 20.0 {
        2600.0 + 365.0 * age.sqrt()
    } else {
        5651.0 - 2473.0 * (-0.0278 * age).exp()
    }
}

/// The elastic thickness of the plate in kilometers at an age in millions
/// of years.
pub fn elastic_km(age_myr: f64) -> f64 {
    (ELASTIC_FACTOR * age_myr.max(0.0).sqrt()).clamp(ELASTIC_MIN_KM, ELASTIC_MAX_KM)
}

/// Two unit vectors square to `center` and to each other: east and north.
/// At a pole, east is the x axis.
fn tangent_frame(center: V3) -> (V3, V3) {
    let east = cross([0.0, 0.0, 1.0], center);
    let east = if dot(east, east) < 1e-18 {
        [1.0, 0.0, 0.0]
    } else {
        normalize(east)
    };
    (east, cross(center, east))
}

/// The height of a flank at `u` base radii inside the base, as a part of
/// the height: `u` itself, with a curve into the floor near the base.
fn foot(u: f64) -> f64 {
    if u >= FOOT {
        u
    } else if u > -FOOT {
        (u + FOOT) * (u + FOOT) / (4.0 * FOOT)
    } else {
        0.0
    }
}

/// An angle in the range from -pi to pi.
fn wrap(a: f64) -> f64 {
    (a + PI).rem_euclid(TAU) - PI
}

/// A number from 0 to 1 from a seed and an index.
pub(crate) fn draw(seed: u32, i: u32) -> f64 {
    unit(hash(seed, i, 0x5eed, SALT_SHAPE))
}

/// The points of the field within `radius` radians of `center` that can
/// show on a heightmap with `n` texels along a face side: the key, the
/// place, and the height in meters of each.
fn field(spec: &Seamounts, n: usize, center: V3, radius: f64) -> Vec<(u64, V3, f64)> {
    let step_m = FRAC_PI_2 / n as f64 * spec.world_km * 1000.0;
    let per_sr = spec.density / 1.0e4 * spec.world_km * spec.world_km;
    let size = spec.height_m;
    let mut points = Vec::new();
    for band in 0..BANDS {
        let low = (-(band as f64)).exp();
        let band_sr = per_sr * low * (1.0 - (-1.0f64).exp());
        if band_sr * 4.0 * PI < 1e-3 {
            break;
        }
        // A cone smaller than a texel does not show.
        let top_m = (band + 1) as f64 * size;
        if top_m / (CONE_RATIO - CONE_RATIO_SPREAD) < step_m {
            continue;
        }
        // About one point in each cell.
        let cells = (FRAC_PI_2 * 4.0 / 3.0 * band_sr).sqrt().ceil();
        let cells = cells.clamp(1.0, f64::from(1u32 << 22)) as usize;
        for face in 0..FACES {
            let Some(rect) = brush_rect(cells, face, center, radius) else {
                continue;
            };
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    let cell_hash = hash(x as u32, y as u32, (face * 64 + band) as u32, spec.seed);
                    let cell_hash = hash(cell_hash, SALT_FIELD, 0, 0);
                    let mean = band_sr * cell_solid_angle(cells, x, y);
                    let count = poisson(mean, unit(cell_hash));
                    for k in 0..count {
                        let h = hash(cell_hash, k, 1, 0);
                        let (s, t, e) = (unit(h), unit(hash(h, 2, 0, 0)), unit(hash(h, 3, 0, 0)));
                        let at =
                            |i: usize, f: f64| unwarp((i as f64 + f) / cells as f64 * 2.0 - 1.0);
                        let place = face_dir(face, at(x, s), at(y, t));
                        if angle(place, center) >= radius {
                            continue;
                        }
                        let height_m =
                            size * (band as f64 - (1.0 - e * (1.0 - (-1.0f64).exp())).ln());
                        let key = (band as u64) << 59
                            | (face as u64) << 56
                            | (x as u64) << 34
                            | (y as u64) << 12
                            | u64::from(k);
                        points.push((key, place, height_m));
                    }
                }
            }
        }
    }
    points
}

/// The solid angle of cell `(x, y)` of a face with `cells` cells along a
/// side, at its center.
fn cell_solid_angle(cells: usize, x: usize, y: usize) -> f64 {
    let size = 2.0 / cells as f64;
    let flat = |i: usize| unwarp((i as f64 + 0.5) * size - 1.0);
    let (fu, fv) = (flat(x), flat(y));
    let d = |f: f64| (1.0 + f * f) * FRAC_PI_4 * size;
    d(fu) * d(fv) / (1.0 + fu * fu + fv * fv).powf(1.5)
}

/// A count from a Poisson distribution with a mean, for a number `u` from
/// 0 to 1.
fn poisson(mean: f64, u: f64) -> u32 {
    let mut p = (-mean).exp();
    let mut sum = p;
    let mut k = 0;
    while u > sum && k < 31 {
        k += 1;
        p *= mean / f64::from(k);
        sum += p;
    }
    k
}

/// The volcano of a point of the field, or `None` if it does not show. The
/// volcano and its moat stay within `reach` radians of its center.
fn volcano(
    spec: &Seamounts,
    floor: &SeaFloor,
    key: u64,
    center: V3,
    height_m: f64,
    reach: f64,
    n: usize,
) -> Option<Volcano> {
    let seed = hash(key as u32, (key >> 32) as u32, spec.seed, SALT_SHAPE);
    let r = |i: u32| draw(seed, i);
    let mut v = [0f32; VOLCANO_FLOATS];
    let shield = smoothstep(SHIELD_FROM_M, SHIELD_TO_M, height_m);
    let cone_ratio = CONE_RATIO + CONE_RATIO_SPREAD * (2.0 * r(0) - 1.0);
    let ratio = cone_ratio + (SHIELD_RATIO - cone_ratio) * shield;
    let km = spec.world_km;
    let mut radius = height_m / ratio / (km * 1000.0);
    let mut height_m = height_m;
    // The fabric at the center.
    let axis = {
        let f = floor.fabric;
        let along = sub(f, scale3(center, dot(f, center)));
        if dot(along, along) < 1e-12 {
            tangent_frame(center).1
        } else {
            normalize(along)
        }
    };
    let cones = if r(1) < RIDGES {
        2 + (r(2) * 3.0) as usize
    } else {
        1
    };
    let stretch = if cones > 1 {
        1.2 + 0.3 * r(3)
    } else {
        1.0 + 0.15 * r(3)
    };
    // The cones of a ridge are apart by uneven steps, off one line, and on
    // a gentle bend. Each cone has its own turn, so no two have the same
    // shape. The end cones are often smaller.
    let mut offsets = [0.0; 4];
    let mut sides = [0.0; 4];
    let mut turns = [0.0; 4];
    let mut heights = [0.0; 4];
    let bend = 0.8 * (r(4) - 0.5);
    for i in 0..cones {
        let k = i as u32;
        if i > 0 {
            offsets[i] = offsets[i - 1] + (0.6 + 0.45 * r(10 + k)) * stretch;
        }
        if cones > 1 {
            sides[i] = 0.6 * (r(44 + k) - 0.5);
        }
        turns[i] = if i == 0 { 0.0 } else { r(48 + k) * TAU };
        let end = i == 0 || i == cones - 1;
        heights[i] = if i == cones / 2 {
            1.0
        } else if end {
            0.5 + 0.4 * r(14 + k)
        } else {
            0.6 + 0.35 * r(14 + k)
        };
    }
    let middle = 0.5 * (offsets[0] + offsets[cones - 1]);
    let half = (0.5 * (offsets[cones - 1] - offsets[0])).max(1e-9);
    for i in 0..cones {
        offsets[i] -= middle;
        if cones > 1 {
            let t = offsets[i] / half;
            sides[i] += bend * half * (t * t - 0.5);
        }
    }
    // A ridge runs near the fabric, not exactly along it.
    let axis = if cones > 1 {
        let (s, c) = (0.7 * (r(52) - 0.5)).sin_cos();
        let across = cross(center, axis);
        normalize([
            axis[0] * c + across[0] * s,
            axis[1] * c + across[1] * s,
            axis[2] * c + across[2] * s,
        ])
    } else {
        axis
    };
    let rifts = if shield > 0.0 {
        2 + (r(5) * 3.0) as usize
    } else {
        0
    };
    let rift_size = 0.6 + 0.4 * r(6);
    let rift_start = r(7) * TAU;
    let outline = [
        0.06 + 0.06 * r(20),
        0.05 + 0.05 * r(21),
        0.03 + 0.04 * r(22),
    ];
    // The farthest place of the volcano, in base radii.
    let along = (0..cones).fold(0.0f64, |m, i| m.max(offsets[i].hypot(sides[i])));
    let slides = shield >= 0.5;
    let foot = if slides { APRON } else { 1.05 + FOOT };
    let outline_max = 1.0 + outline.iter().sum::<f64>() + 0.05;
    let extent = (along + stretch * foot * outline_max * (1.0 + rift_size * shield)) * radius;
    let fit = 0.85 * reach / extent;
    if fit < 1.0 {
        radius *= fit;
        height_m *= fit;
    }
    let step = FRAC_PI_2 / n as f64;
    if radius < step || height_m * LEVELS_PER_M < 1.0 {
        return None;
    }
    let flat = shield < 0.5 && r(8) < FLAT_TOPS;
    let crater = shield < 0.5 && !flat && r(8) < FLAT_TOPS + CRATERS;
    let radius_m = radius * km * 1000.0;
    let fill = 1.0 / 3.0 + (0.229 - 1.0 / 3.0) * shield;
    let volume: f64 =
        heights[..cones].iter().sum::<f64>() * PI * radius_m * radius_m * stretch * height_m * fill;
    let (moat_m, flex_m) = flexure(floor.age_myr, MANTLE - WATER, volume, radius_m, height_m);
    let put = |v: &mut [f32; VOLCANO_FLOATS], i: usize, x: f64| v[i] = x as f32;
    for k in 0..3 {
        put(&mut v, CENTER + k, center[k]);
        put(&mut v, AXIS + k, axis[k]);
    }
    put(&mut v, HEIGHT, height_m * LEVELS_PER_M);
    put(&mut v, RADIUS, radius);
    put(&mut v, SHIELD, shield);
    put(&mut v, TOP, if flat { 0.6 + 0.25 * r(9) } else { 1.0 });
    put(
        &mut v,
        CRATER_RADIUS,
        if crater { 0.12 + 0.1 * r(9) } else { 0.0 },
    );
    put(
        &mut v,
        CRATER_DEPTH,
        if crater { 0.08 + 0.08 * r(23) } else { 0.0 },
    );
    put(&mut v, CONES, cones as f64);
    for i in 0..4 {
        put(&mut v, CONE_OFFSET + i, offsets[i]);
        put(&mut v, CONE_HEIGHT + i, heights[i]);
        put(&mut v, CONE_SIDE + i, sides[i]);
        put(&mut v, CONE_TURN + i, turns[i]);
    }
    put(&mut v, STRETCH, stretch);
    put(&mut v, RIFTS, rifts as f64);
    for i in 0..rifts {
        let even = rift_start + TAU * i as f64 / rifts as f64;
        put(
            &mut v,
            RIFT_ANGLE + i,
            wrap(even + 0.6 * (r(30 + i as u32) - 0.5)),
        );
    }
    put(&mut v, RIFT_SIZE, rift_size);
    for (k, &size) in outline.iter().enumerate() {
        put(&mut v, OUTLINE + k, size);
        put(&mut v, OUTLINE_PHASE + k, r(24 + k as u32) * TAU);
    }
    if slides {
        let count = 1 + (r(35) * 2.0) as usize;
        for s in 0..count {
            put(&mut v, SLIDE + 3 * s, wrap(r(36 + s as u32) * TAU));
            put(&mut v, SLIDE + 3 * s + 1, 0.25 + 0.2 * r(38 + s as u32));
            put(&mut v, SLIDE + 3 * s + 2, 0.25 + 0.25 * r(40 + s as u32));
        }
    }
    put(&mut v, GULLIES, 24.0 + 16.0 * r(42) + 16.0 * shield);
    put(&mut v, GULLY_SIZE, 0.07 + 0.06 * r(43));
    // A whole number below 2^24 is exact as a 32-bit float.
    put(&mut v, SEED, f64::from(hash(seed, 7, 7, 7) & 0xff_ffff));
    put(&mut v, FLEX, moat_m * LEVELS_PER_M);
    put(&mut v, FLEX_LENGTH, 1.6 * flex_m / (km * 1000.0));
    put(&mut v, REACH, reach);
    put(&mut v, EXTENT, reach);
    v[CAP] = NO_CAP;
    Some(Volcano(v))
}

/// The moat of a thin elastic plate under the load of an edifice, at its
/// center, and the flexural length, each in meters. The plate has an age in
/// millions of years. Under the plate is mantle, and over it is matter that
/// is lighter than the mantle by `contrast` in kg/m³. The edifice has a
/// volume in m³, a base radius, and a height.
fn flexure(age_myr: f64, contrast: f64, volume: f64, radius_m: f64, height_m: f64) -> (f64, f64) {
    let te = elastic_km(age_myr) * 1000.0;
    let rigidity = YOUNG * te.powi(3) / (12.0 * (1.0 - POISSON * POISSON));
    let flex_m = (rigidity / (contrast * GRAVITY)).powf(0.25);
    let load = (ROCK - WATER) * GRAVITY * volume;
    let spread = 1.0 + 0.25 * (radius_m / flex_m).powi(2);
    let moat_m = (load * flex_m * flex_m / (8.0 * rigidity) / spread).min(FLEX_MAX * height_m);
    (moat_m, flex_m)
}

/// A shield volcano of one cone with rift zones and slides, or `None` if it
/// does not show on a heightmap with `n` texels along a face side. `axis`
/// is a unit vector square to `center`, `height_m` is the height, and
/// `load_age` is the age of the plate under it in millions of years, in
/// a world with a radius of `km` kilometers. Volcanic rock fills its moat.
/// It reaches `reach` radians from its center. A volcano that does not fit
/// in that reach is smaller.
#[allow(clippy::too_many_arguments)]
pub(crate) fn shield_volcano(
    seed: u32,
    center: V3,
    axis: V3,
    height_m: f64,
    reach: f64,
    load_age: f64,
    km: f64,
    n: usize,
) -> Option<Volcano> {
    let r = |i: u32| draw(seed, i);
    let mut v = [0f32; VOLCANO_FLOATS];
    let mut radius = height_m / SHIELD_RATIO / (km * 1000.0);
    let mut height_m = height_m;
    let stretch = 1.0 + 0.15 * r(3);
    let rifts = 2 + (r(5) * 3.0) as usize;
    let rift_size = 0.6 + 0.4 * r(6);
    let rift_start = r(7) * TAU;
    let outline = [
        0.06 + 0.06 * r(20),
        0.05 + 0.05 * r(21),
        0.03 + 0.04 * r(22),
    ];
    let outline_max = 1.0 + outline.iter().sum::<f64>() + 0.05;
    let mut extent = stretch * APRON * outline_max * (1.0 + rift_size) * radius;
    if extent > 0.85 * reach {
        let fit = 0.85 * reach / extent;
        radius *= fit;
        height_m *= fit;
        extent *= fit;
    }
    let step = FRAC_PI_2 / n as f64;
    if !(radius >= step && height_m * LEVELS_PER_M >= 1.0) {
        return None;
    }
    let radius_m = radius * km * 1000.0;
    let volume = PI * radius_m * radius_m * stretch * height_m * 0.229;
    let (moat_m, flex_m) = flexure(load_age, MANTLE - ROCK, volume, radius_m, height_m);
    let put = |v: &mut [f32; VOLCANO_FLOATS], i: usize, x: f64| v[i] = x as f32;
    for k in 0..3 {
        put(&mut v, CENTER + k, center[k]);
        put(&mut v, AXIS + k, axis[k]);
    }
    put(&mut v, GROWTH, 1.0);
    put(&mut v, HEIGHT, height_m * LEVELS_PER_M);
    put(&mut v, RADIUS, radius);
    put(&mut v, SHIELD, 1.0);
    put(&mut v, TOP, 1.0);
    put(&mut v, CONES, 1.0);
    put(&mut v, CONE_HEIGHT, 1.0);
    put(&mut v, STRETCH, stretch);
    put(&mut v, RIFTS, rifts as f64);
    for i in 0..rifts {
        let even = rift_start + TAU * i as f64 / rifts as f64;
        put(
            &mut v,
            RIFT_ANGLE + i,
            wrap(even + 0.6 * (r(30 + i as u32) - 0.5)),
        );
    }
    put(&mut v, RIFT_SIZE, rift_size);
    for (k, &size) in outline.iter().enumerate() {
        put(&mut v, OUTLINE + k, size);
        put(&mut v, OUTLINE_PHASE + k, r(24 + k as u32) * TAU);
    }
    let count = 1 + (r(35) * 2.0) as usize;
    for s in 0..count {
        put(&mut v, SLIDE + 3 * s, wrap(r(36 + s as u32) * TAU));
        put(&mut v, SLIDE + 3 * s + 1, 0.25 + 0.2 * r(38 + s as u32));
        put(&mut v, SLIDE + 3 * s + 2, 0.25 + 0.25 * r(40 + s as u32));
    }
    put(&mut v, GULLIES, 40.0 + 16.0 * r(42));
    put(&mut v, GULLY_SIZE, 0.07 + 0.06 * r(43));
    put(&mut v, SEED, f64::from(hash(seed, 7, 7, 7) & 0xff_ffff));
    put(&mut v, FLEX, moat_m * LEVELS_PER_M);
    put(
        &mut v,
        FLEX_LENGTH,
        LINE_FLEX * 1.6 * flex_m / (km * 1000.0),
    );
    put(&mut v, REACH, reach);
    put(&mut v, EXTENT, extent);
    v[CAP] = NO_CAP;
    Some(Volcano(v))
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::meters_to_level;
    use crate::math::lonlat_to_dir;

    const WORLD_KM: f64 = 1000.0;
    const N: usize = 1024;

    fn spec() -> Seamounts {
        Seamounts {
            density: 50.0,
            height_m: 400.0,
            world_km: WORLD_KM,
            seed: 7,
            limits: Limits::default(),
        }
    }

    fn floor(age_myr: f64) -> SeaFloor {
        SeaFloor {
            age_myr,
            fabric: [0.0, 0.0, 1.0],
            coherence: 1.0,
        }
    }

    /// The volcano of each point of the field near a place.
    fn volcanoes(spec: &Seamounts, reach: f64) -> Vec<Volcano> {
        let center = lonlat_to_dir(10.0, 5.0);
        let field = field(spec, N, center, 0.3);
        let floor = floor(60.0);
        field
            .into_iter()
            .filter_map(|(key, p, h)| volcano(spec, &floor, key, p, h, reach, N))
            .collect()
    }

    fn meters(levels: f64) -> f64 {
        levels / LEVELS_PER_M
    }

    fn radius_m(v: &Volcano) -> f64 {
        v.base_radius() * WORLD_KM * 1000.0
    }

    /// The level at full growth at a place on the tangent plane of the
    /// center, at a distance in base radii and an angle from the axis.
    fn at(v: &Volcano, r: f64, theta: f64) -> f64 {
        let c = v.center();
        let a = v.v3(AXIS);
        let b = cross(c, a);
        let d = r * v.base_radius();
        let (s, k) = theta.sin_cos();
        let p = normalize([
            c[0] + (a[0] * k + b[0] * s) * d,
            c[1] + (a[1] * k + b[1] * s) * d,
            c[2] + (a[2] * k + b[2] * s) * d,
        ]);
        v.full_levels(p)
    }

    /// The edifice alone, with no moat.
    fn edifice(v: &Volcano, r: f64, theta: f64) -> f64 {
        let mut v = *v;
        v.0[FLEX] = 0.0;
        at(&v, r, theta)
    }

    /// The distance from the center in base radii where the edifice drops
    /// below a part of its height, at an angle.
    fn outline(v: &Volcano, theta: f64, part: f64) -> f64 {
        let h = v.height();
        let mut r = 0.0;
        while r < 4.0 && edifice(v, r, theta) > part * h {
            r += 0.01;
        }
        r
    }

    /// Whether two heightmaps have the same levels.
    fn same(a: &Heightmap, b: &Heightmap) -> bool {
        let n = a.face_size();
        (0..FACES).all(|f| {
            (0..n).all(|y| {
                (0..n).all(|x| a.get(f, x as i64, y as i64) == b.get(f, x as i64, y as i64))
            })
        })
    }

    #[test]
    fn the_number_above_a_height_falls_exponentially() {
        let spec = spec();
        let center = lonlat_to_dir(30.0, 20.0);
        let radius = 0.6;
        let heights: Vec<f64> = field(&spec, 4096, center, radius)
            .into_iter()
            .map(|(_, _, h)| h)
            .collect();
        // The area of the cap in 10,000 km².
        let area = TAU * (1.0 - radius.cos()) * WORLD_KM * WORLD_KM / 1.0e4;
        let above = |h: f64| heights.iter().filter(|&&x| x >= h).count() as f64 / area;
        // Each cone of 100 m or more shows on this map.
        for k in 1..6 {
            let h = 200.0 * k as f64;
            let expected = spec.density * (-h / spec.height_m).exp();
            let got = above(h);
            assert!(
                (got / expected - 1.0).abs() < 0.15,
                "{got} above {h} m for each 10,000 km², not {expected}"
            );
        }
    }

    #[test]
    fn a_small_seamount_is_a_cone() {
        let spec = spec();
        let all = volcanoes(&spec, 0.1);
        let small: Vec<_> = all.iter().filter(|v| meters(v.height()) < 1000.0).collect();
        assert!(small.len() > 100, "{} small cones", small.len());
        for v in &small {
            let ratio = meters(v.height()) / radius_m(v);
            assert!((0.15..=0.25).contains(&ratio), "a ratio of {ratio}");
            assert_eq!(v.rifts(), 0);
        }
        let flat = small.iter().filter(|v| v.flat_top()).count() as f64;
        let crater = small.iter().filter(|v| v.crater()).count() as f64;
        let count = small.len() as f64;
        assert!((0.1..0.3).contains(&(flat / count)), "{flat} flat tops");
        assert!((0.07..0.25).contains(&(crater / count)), "{crater} craters");
        // A flat top is flat, and a crater is lower than its rim.
        let v = small
            .iter()
            .find(|v| v.flat_top() && v.cones() == 1)
            .unwrap();
        let h = v.height();
        assert!((edifice(v, 0.0, 0.0) - edifice(v, 0.1, 1.0)).abs() < 0.02 * h);
        let v = small.iter().find(|v| v.crater() && v.cones() == 1).unwrap();
        let rim = (1..50)
            .map(|i| edifice(v, i as f64 * 0.01, 0.0))
            .fold(0.0, f64::max);
        assert!(edifice(v, 0.0, 0.0) < rim - 0.05 * h);
    }

    #[test]
    fn the_foot_of_a_flank_curves_into_the_floor() {
        // The slope falls with no step from the flank to the floor.
        let step = 0.001;
        let slope = |u: f64| (foot(u + step) - foot(u - step)) / (2.0 * step);
        let mut last = slope(1.0);
        let mut u = 1.0;
        while u > -1.0 {
            let s = slope(u);
            assert!((s - last).abs() < 0.01, "a step of the slope at {u}");
            last = s;
            u -= step;
        }
        assert_eq!(foot(-FOOT), 0.0);
        assert!((slope(0.0) - 0.5).abs() < 0.01);
    }

    #[test]
    fn a_large_seamount_is_a_shield_with_rifts_and_slides() {
        let spec = Seamounts {
            height_m: 2500.0,
            density: 5.0,
            ..spec()
        };
        let all = volcanoes(&spec, 0.2);
        let shields: Vec<_> = all.iter().filter(|v| meters(v.height()) > 3000.0).collect();
        assert!(shields.len() > 5, "{} shields", shields.len());
        for v in shields.into_iter().filter(|v| v.cones() == 1) {
            assert!(v.shield());
            assert!((2..=4).contains(&v.rifts()), "{} rifts", v.rifts());
            let ratio = meters(v.height()) / radius_m(v);
            assert!(ratio < 0.13, "a ratio of {ratio}");
            // The base reaches farther along each rift.
            let mut round = *v;
            round.0[RIFT_SIZE] = 0.0;
            for k in 0..v.rifts() {
                let rift = v.get(RIFT_ANGLE + k);
                let (a, b) = (outline(v, rift, 0.05), outline(&round, rift, 0.05));
                assert!(a > 1.25 * b, "{a} along a rift, {b} with no rift");
            }
            // A slide cuts the flank and leaves an apron past the base.
            let slide = v.get(SLIDE);
            let mut bare = *v;
            bare.0[SLIDE + 2] = 0.0;
            bare.0[SLIDE + 5] = 0.0;
            assert!(edifice(v, 0.6, slide) < 0.9 * edifice(&bare, 0.6, slide));
            let past = 1.1 * outline(&bare, slide, 0.0);
            assert!(edifice(v, past, slide) > 0.005 * v.height());
        }
    }

    #[test]
    fn no_seamount_is_a_circle() {
        let all = volcanoes(&spec(), 0.1);
        for v in all.iter().filter(|v| v.cones() == 1) {
            let radii: Vec<f64> = (0..72)
                .map(|i| outline(v, i as f64 / 72.0 * TAU, 0.1))
                .collect();
            let (low, high) = radii
                .iter()
                .fold((f64::MAX, 0.0f64), |(l, h), &r| (l.min(r), h.max(r)));
            assert!(high > 1.1 * low, "radii from {low} to {high}");
            // Ridges and gullies run down the flank: the level around the
            // middle of the flank goes up and down many times.
            let ring: Vec<f64> = (0..360)
                .map(|i| edifice(v, 0.75 * low, i as f64 / 360.0 * TAU))
                .collect();
            let turns = (0..360)
                .filter(|&i| {
                    let (a, b, c) = (ring[i], ring[(i + 1) % 360], ring[(i + 2) % 360]);
                    (b - a) * (c - b) < 0.0
                })
                .count();
            assert!(turns >= 8, "{turns} turns around the flank");
        }
    }

    #[test]
    fn close_seamounts_merge_into_a_ridge_along_the_fabric() {
        let all = volcanoes(&spec(), 0.1);
        let ridges: Vec<_> = all.iter().filter(|v| v.cones() > 1).collect();
        assert!(ridges.len() > 20, "{} ridges", ridges.len());
        for v in ridges {
            let h = v.height();
            // The ridge is longer along the axis than across it: the second
            // moments of the edifice above a tenth of its height.
            let (mut w, mut mu, mut mv) = (0.0, 0.0, 0.0);
            let (mut uu, mut vv) = (0.0, 0.0);
            for j in -60..=60 {
                for i in -60..=60 {
                    let (x, y) = (f64::from(i) * 0.05, f64::from(j) * 0.05);
                    if edifice(v, x.hypot(y), y.atan2(x)) > 0.1 * h {
                        w += 1.0;
                        mu += x;
                        mv += y;
                        uu += x * x;
                        vv += y * y;
                    }
                }
            }
            let along = uu / w - (mu / w).powi(2);
            let across = vv / w - (mv / w).powi(2);
            assert!(along > 1.5 * across, "{along} along, {across} across");
            // The ground between two cones stays high.
            let mu = 0.5 * (v.get(CONE_OFFSET) + v.get(CONE_OFFSET + 1));
            let mv = 0.5 * (v.get(CONE_SIDE) + v.get(CONE_SIDE + 1));
            let saddle = edifice(v, mu.hypot(mv), mv.atan2(mu));
            let low = v.get(CONE_HEIGHT).min(v.get(CONE_HEIGHT + 1));
            assert!(saddle > 0.2 * low * h, "a saddle at {saddle} of {h}");
        }
    }

    #[test]
    fn the_cones_of_a_ridge_are_uneven() {
        let all = volcanoes(&spec(), 0.1);
        let ridges: Vec<_> = all.iter().filter(|v| v.cones() > 2).collect();
        assert!(
            ridges.len() > 10,
            "{} ridges of 3 cones or more",
            ridges.len()
        );
        let mut off_line = 0;
        let mut turns = Vec::new();
        for v in &ridges {
            let k = v.cones();
            // The distance of the middle cones from the line between the end
            // cones, in base radii.
            let (a, b) = ((0, k - 1), (CONE_OFFSET, CONE_SIDE));
            let p = |i: usize| [v.get(b.0 + i), v.get(b.1 + i)];
            let (p0, p1) = (p(a.0), p(a.1));
            let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
            let far = (1..k - 1)
                .map(|i| ((p(i)[0] - p0[0]) * dy - (p(i)[1] - p0[1]) * dx).abs() / dx.hypot(dy))
                .fold(0.0, f64::max);
            if far > 0.05 {
                off_line += 1;
            }
            // The steps between the cones are uneven.
            let steps: Vec<f64> = (1..k).map(|i| p(i)[0] - p(i - 1)[0]).collect();
            assert!(steps.iter().all(|&s| s > 0.0));
            turns.extend((1..k).map(|i| v.get(CONE_TURN + i)));
        }
        assert!(
            off_line * 10 > ridges.len() * 8,
            "{off_line} ridges off one line"
        );
        // Each cone after the first has its own turn, so its own shape.
        let distinct = turns
            .windows(2)
            .filter(|w| (w[0] - w[1]).abs() > 0.1)
            .count();
        assert!(distinct * 10 > turns.len() * 8);
        // The ridges do not all run along the fabric.
        let fabric = [0.0, 0.0, 1.0];
        let tilted = ridges
            .iter()
            .filter(|v| dot(v.v3(AXIS), fabric).abs() < 0.995)
            .count();
        assert!(tilted * 2 > ridges.len(), "{tilted} ridges off the fabric");
    }

    #[test]
    fn a_large_seamount_bends_the_plate() {
        let spec = Seamounts {
            height_m: 2500.0,
            density: 5.0,
            ..spec()
        };
        let all = volcanoes(&spec, 0.25);
        let v = all
            .iter()
            .filter(|v| v.cones() == 1)
            .max_by(|a, b| a.height().total_cmp(&b.height()))
            .unwrap();
        // A moat around the edifice, and a low arch outside it.
        let ring = |r: f64| {
            (0..36)
                .map(|i| at(v, r, i as f64 / 36.0 * TAU))
                .sum::<f64>()
                / 36.0
        };
        let length = v.get(FLEX_LENGTH) / v.base_radius();
        let moat = ring(1.6f64.max(0.4 * length + 1.0));
        let arch = ring(PI * length);
        assert!(moat < -1.0, "a moat at {moat}");
        assert!(arch > 0.0, "an arch at {arch}");
        assert!(arch < -0.2 * moat);
        // An older plate is stiffer, so its moat is wider and shallower.
        let older = volcano(&spec, &floor(150.0), 1, v.center(), 6000.0, 0.25, N).unwrap();
        let young = volcano(&spec, &floor(5.0), 1, v.center(), 6000.0, 0.25, N).unwrap();
        assert!(older.get(FLEX_LENGTH) > 2.0 * young.get(FLEX_LENGTH));
        assert!(older.get(FLEX) < young.get(FLEX));
        assert!(elastic_km(100.0) > 25.0 && elastic_km(100.0) < 30.0);
    }

    #[test]
    fn the_age_comes_from_the_depth() {
        for age in [0.5, 4.0, 15.0, 30.0, 80.0, 150.0] {
            let depth = if age < 20.0 {
                2600.0 + 365.0 * f64::sqrt(age)
            } else {
                5651.0 - 2473.0 * (-0.0278 * age).exp()
            };
            assert!((gdh1_age(depth) - age).abs() < 1e-6 * age.max(1.0));
        }
        assert_eq!(gdh1_age(1000.0), 0.0);
        assert_eq!(gdh1_age(6000.0), 200.0);
    }

    /// A heightmap with abyssal hills at a depth, along a direction at an
    /// angle from north toward east, near `center`.
    fn hills(depth_m: f64, azimuth: f64, center: V3) -> Heightmap {
        let n = 256;
        let mut map = Heightmap::new(n, meters_to_level(-depth_m));
        let (east, north) = tangent_frame(center);
        let (s, c) = azimuth.sin_cos();
        // Square to the hills.
        let across = [
            east[0] * c - north[0] * s,
            east[1] * c - north[1] * s,
            east[2] * c - north[2] * s,
        ];
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    let p = map.texel_dir(face, x, y);
                    if angle(p, center) > 0.3 {
                        continue;
                    }
                    let wave = (dot(p, across) * 60.0).sin() * 150.0;
                    map.set(face, x, y, meters_to_level(-depth_m + wave));
                }
            }
        }
        map
    }

    #[test]
    fn the_fabric_comes_from_the_slopes() {
        let center = lonlat_to_dir(20.0, 10.0);
        let (east, north) = tangent_frame(center);
        for azimuth in [0.0, 0.5, 1.2, 2.5] {
            let map = hills(4500.0, azimuth, center);
            let floor = sea_floor(&map, center, 0.15);
            assert!(floor.coherence > 0.8, "{}", floor.coherence);
            let (s, c) = azimuth.sin_cos();
            let along = [
                east[0] * s + north[0] * c,
                east[1] * s + north[1] * c,
                east[2] * s + north[2] * c,
            ];
            assert!(dot(floor.fabric, along).abs() > 0.99);
            assert!((floor.age_myr - gdh1_age(4500.0)).abs() < 5.0);
        }
        // Flat ground has no main direction, so the fabric points north.
        let flat = Heightmap::new(256, meters_to_level(-3000.0));
        let floor = sea_floor(&flat, center, 0.1);
        assert!(dot(floor.fabric, north) > 0.999);
    }

    fn paint(map: &mut Heightmap, spec: Seamounts, centers: &[V3], radius: f64) -> SeamountStroke {
        let mut stroke = SeamountStroke::new(spec, map.face_size());
        map.begin_stroke();
        for &c in centers {
            let plan = stroke.stamp(map, c, radius, 0.5, 0.6);
            map.seamounts(&plan);
        }
        map.end_stroke();
        stroke
    }

    fn line(from: (f64, f64), to: (f64, f64), count: usize) -> Vec<V3> {
        (0..count)
            .map(|i| {
                let t = i as f64 / (count - 1) as f64;
                lonlat_to_dir(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
            })
            .collect()
    }

    /// A heightmap on a slope, as on a ridge flank.
    fn slope() -> Heightmap {
        let mut map = Heightmap::with_tile_size(N, 64, meters_to_level(-4000.0));
        for face in [0, 2, 4] {
            for y in 0..N {
                for x in 0..N {
                    let p = map.texel_dir(face, x, y);
                    if angle(p, lonlat_to_dir(10.0, 5.0)) < 0.25 {
                        let m = -4000.0 + 3000.0 * (p[1] - 0.17);
                        map.set(face, x, y, meters_to_level(m));
                    }
                }
            }
        }
        map
    }

    #[test]
    fn a_stroke_grows_seamounts_on_the_ground_and_stays_near_the_brush() {
        let spec = Seamounts {
            height_m: 800.0,
            ..spec()
        };
        let mut map = slope();
        let before = map.snapshot();
        let centers = line((8.0, 4.0), (12.0, 6.0), 40);
        let radius = 0.02;
        let stroke = paint(&mut map, spec, &centers, radius);
        assert!(stroke.volcanoes().len() > 5);
        let (mut changed, mut highest) = (0, 0.0f64);
        for face in 0..FACES {
            for y in 0..N {
                for x in 0..N {
                    let (old, new) = (
                        before.get(face, x as i64, y as i64),
                        map.get(face, x as i64, y as i64),
                    );
                    if old == new {
                        continue;
                    }
                    changed += 1;
                    highest = highest.max(f64::from(new) - f64::from(old));
                    // The moat and the arch reach past the brush by 1 brush
                    // radius at most.
                    let p = map.texel_dir(face, x, y);
                    let near = centers
                        .iter()
                        .map(|&c| angle(p, c))
                        .fold(f64::MAX, f64::min);
                    assert!(near < 2.0 * radius, "a change at {near}");
                    // A seamount rises from the ground without a step.
                    if near > 1.9 * radius {
                        assert!((f64::from(new) - f64::from(old)).abs() <= 2.0);
                    }
                }
            }
        }
        assert!(changed > 1000, "{changed} texels changed");
        assert!(meters(highest) > 300.0, "{} m at most", meters(highest));
    }

    #[test]
    fn equal_strokes_give_equal_results_and_a_second_stroke_adds_no_seamounts() {
        let centers = line((8.0, 4.0), (12.0, 6.0), 30);
        let mut a = slope();
        let mut b = slope();
        let first = paint(&mut a, spec(), &centers, 0.02);
        paint(&mut b, spec(), &centers, 0.02);
        assert!(same(&a, &b));
        let again = paint(&mut a, spec(), &centers, 0.02);
        let keys = |s: &SeamountStroke| {
            let mut k: Vec<_> = s.keys.keys().copied().collect();
            k.sort_unstable();
            k
        };
        assert_eq!(keys(&first), keys(&again));
        assert!(!same(&a, &b), "the second stroke grows them");
    }

    #[test]
    fn one_undo_puts_back_the_ground() {
        let mut map = slope();
        let before = map.snapshot();
        paint(&mut map, spec(), &line((8.0, 4.0), (12.0, 6.0), 30), 0.02);
        assert!(!same(&map, &before));
        assert!(map.undo());
        assert!(same(&map, &before));
    }
}
