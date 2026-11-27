//! The hotspot track brush.
//!
//! The line of a stroke is the track of a plate over a hotspot. The end of
//! the line is over the hotspot, and the age of each point of the line is its
//! distance from the end over the plate speed. After the pen goes up,
//! `HotspotLine::finish` places a chain of shield volcanoes along the line.
//! A texel near the line gets the swell of the hotspot, the moats of the
//! volcanoes, and the volcanoes, each by its age.

use std::cell::Cell;
use std::f64::consts::FRAC_PI_2;
use std::ops::Deref;
use std::sync::Arc;

use crate::cube::{ELEV_MIN, FACES};
use crate::erode::{ErodeBrush, ErodeStep, Erosion};
use crate::flow::Window;
use crate::heightmap::{Heightmap, Limits, hash};
use crate::line::{LinePlace, LinePlan, LinePoint, REACH, StrokeLine, bump, noise1, smoothstep};
use crate::margin::{circle_window, smooth_min};
use crate::math::{V3, add, angle, cross, dot, normalize, scale};
use crate::seamount::{
    BREAK, CAP, FLEX, GROWTH, HEIGHT, LAGOON, LEVELS_PER_M, NO_CAP, PEDESTAL_ALONG, PLAN_VOLCANOES,
    REEF, RIM, SHORE, Volcano, draw, gdh1_age, gdh1_depth, sea_floor, shield_volcano,
};

/// The brush radius of the app on a world of the size of the Earth, in
/// kilometers. The swell is this wide on each side of the line.
pub const HOTSPOT_RADIUS_KM: f64 = 500.0;

/// The default height of the largest volcano of the group at the hotspot in
/// meters, and the range of the setting.
pub const HOTSPOT_HEIGHT_M: f64 = 9000.0;
pub const HOTSPOT_HEIGHT_RANGE: (f64, f64) = (500.0, 12000.0);

/// The default plate speed in millimeters per year, and the range of the
/// setting.
pub const HOTSPOT_SPEED_MM: f64 = 90.0;
pub const HOTSPOT_SPEED_RANGE: (f64, f64) = (10.0, 200.0);

/// The height of a volcano over its base radius.
const HOTSPOT_RATIO: f64 = 0.1;

/// The places of the volcanoes are parts of the base radius of the largest
/// volcano at this ratio of height over base radius.
const PLACE_RATIO: f64 = 0.12;

/// The height of the swell over the hotspot, in meters (Crough 1983).
const SWELL_M: f64 = 1000.0;

/// The mean distance between two volcanoes, as a part of the base radius of
/// the largest volcano, and the smallest and the largest part of that
/// distance of one step.
const SPACING: f64 = 0.5;
const STEP: (f64, f64) = (0.6, 1.4);

/// Two lines of volcanoes side by side, as the Loa and the Kea trends: the
/// size of the noise along the line in mean distances, the noise above which
/// the line is double, and the distance of each line from the middle and the
/// height of its volcanoes. The distance is a part of the base radius of the
/// largest volcano.
const DOUBLE_SIZE: f64 = 8.0;
const DOUBLE_ABOVE: f64 = 0.0;
const DOUBLE_ACROSS: f64 = 0.4;
const DOUBLE_HEIGHT: f64 = 0.85;

/// The largest distance of a volcano of a single line from the line, as a
/// part of the base radius of the largest volcano.
const SINGLE_ACROSS: f64 = 0.08;

/// The supply of the hotspot changes along the line with this size in mean
/// distances, from this part of the most. The change starts at the end of
/// the group at the hotspot and grows to its full size over `FLUX_FADE`
/// mean distances.
const FLUX_SIZE: f64 = 6.0;
const FLUX_LOW: f64 = 0.7;
const FLUX_FADE: f64 = 3.0;

/// The supply of the hotspot pulses, so the volcanoes form in groups of
/// 1 to `GROUP_MOST`. The group over the hotspot has at least 2. Between two
/// groups is a gap of 1 to 2 mean distances with no volcano. The volcanoes
/// of a group have a common size, from `GROUP_LOW` of the most. The group
/// over the hotspot has the most.
const GROUP_MOST: f64 = 4.0;
const GROUP_GAP: (f64, f64) = (1.0, 2.0);
const GROUP_LOW: f64 = 0.85;

/// No volcano is taller than the setting times an envelope. The envelope is
/// 1 in the group at the hotspot. Past it, the envelope falls by `OLDER`
/// over `OLDER_SIZE` mean distances, and then by up to `OLDEST` more with a
/// size of `OLDEST_SIZE` mean distances.
const OLDER: f64 = 0.05;
const OLDER_SIZE: f64 = 8.0;
const OLDEST: f64 = 0.2;
const OLDEST_SIZE: f64 = 40.0;

/// The volcano over the hotspot grows to at most this part of the tallest
/// other volcano of its group.
const HOTSPOT_PART: f64 = 0.8;

/// The tallest volcano in meters.
const HEIGHT_MOST_M: f64 = 13000.0;

/// The part of the volcanoes off the line, their distance from it in base
/// radii of the largest volcano, and their height.
const OFF_LINE: f64 = 0.1;
const OFF_ACROSS: (f64, f64) = (0.6, 0.9);
const OFF_HEIGHT: f64 = 0.5;

/// A volcano over the hotspot has this part of its height. It grows to its
/// full height at this age in millions of years.
const GROW_FROM: f64 = 0.35;
const GROW_MYR: f64 = 0.5;

/// The deepest sum of the moats under the chain, as a part of the height of
/// the largest volcano.
const LINE_MOAT: f64 = 0.15;

/// The middle of the gap between two volcanoes is at this part of the reach
/// of their pedestals.
const PEDESTAL_MIDDLE: f64 = 0.5;

/// The rounding of the join of two islands, in meters, so lava fills the
/// saddle between them. A volcano with a wave-cut top does not round, and
/// a reef rounds less where it changes the height by up to `FREE_M`.
const JOIN_M: f64 = 800.0;
const FREE_M: f64 = 100.0;

/// The summit of an island goes down by this many meters in each million
/// years.
const ISLAND_EROSION_M: f64 = 600.0;

/// The waves cut the top of a volcano at this depth in meters when it goes
/// under the sea, with a rounding of `CAP_ROUND_M`.
const WAVE_BASE_M: f64 = 20.0;
const CAP_ROUND_M: f64 = 60.0;

/// The waves cut the platform of a sinking island from the time when its
/// summit is this many meters above the sea, so the platform has the
/// outline of the island then. The platform keeps this part of the relief
/// of the island, and it slopes down to its edge.
const PLATFORM_M: f64 = 400.0;
const PLATFORM_RELIEF: f64 = 0.15;

/// After it drowns, a volcano sinks by up to this many meters more, with
/// this time in millions of years, as the younger volcanoes load the plate.
const SINK_M: f64 = 600.0;
const SINK_MYR: f64 = 5.0;

/// The level of the top of a reef and of the floor of a lagoon, in meters.
const REEF_TOP_M: f64 = -1.0;
const LAGOON_M: f64 = -40.0;

/// The width of the band of a reef around the old shore, as a part of the
/// height of its volcano.
const REEF_BAND: f64 = 0.04;

/// The outer wall of a reef is this many times as steep as the flank under
/// it.
const REEF_WALL: f64 = 2.0;

/// A reef grows for this many millions of years after its volcano goes
/// under the sea. It fades over the last `REEF_FADE_MYR` of them.
const REEF_MYR: f64 = 30.0;
const REEF_FADE_MYR: f64 = 5.0;

const SALT_PLACE: u32 = 0x68_6f74;
const SALT_DOUBLE: u32 = 0x64_626c;
const SALT_FLUX: u32 = 0x66_6c78;
const SALT_SHAPE: u32 = 0x73_6870;
const SALT_GROUP: u32 = 0x67_7270;

/// The settings of a hotspot track stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hotspot {
    /// The brush radius as an angle, in radians. The swell ends at this
    /// distance from the line.
    pub radius: f64,
    /// The height of the largest volcano above the floor, in meters.
    pub height_m: f64,
    /// The plate speed, in millimeters per year.
    pub speed_mm: f64,
    /// Whether reefs grow at sea level on the sinking volcanoes.
    pub reefs: bool,
    /// The radius of the world, in kilometers.
    pub world_km: f64,
    /// The seed of the chain.
    pub seed: u32,
    pub limits: Limits,
}

/// The volcanoes of a line, and the numbers of its swell.
#[derive(Clone, Debug)]
pub(crate) struct Chain {
    /// The depth of the floor at the end of the line, in meters, and the age
    /// of floor of that depth, in millions of years.
    pub floor_depth: f64,
    pub floor_age: f64,
    /// The hotspot heats the plate to the depth of floor of this age.
    pub reset_age: f64,
    /// The plate speed, in radians per million years.
    pub speed: f64,
    pub volcanoes: Vec<Volcano>,
    /// The age of each volcano, in millions of years.
    pub ages: Vec<f64>,
    /// The height of each volcano before its moats and its erosion, in
    /// meters.
    pub heights_m: Vec<f64>,
    /// The age at which each volcano goes under the sea, or infinity.
    pub drowned: Vec<f64>,
}

/// The line of one hotspot track stroke.
#[derive(Clone, Debug)]
pub struct HotspotLine {
    spec: Hotspot,
    line: StrokeLine,
    chain: Option<Arc<Chain>>,
}

/// The part of the heightmap that changes after new points of a line, and
/// the volcanoes that give the levels there.
#[derive(Clone, Debug)]
pub struct HotspotPlan {
    /// The settings, inside their limits.
    pub hotspot: Hotspot,
    pub line: LinePlan,
    pub(crate) chain: Option<Arc<Chain>>,
    /// Each volcano that reaches a rectangle of the plan, in the order of
    /// the chain. There are `PLAN_VOLCANOES` at most.
    pub volcanoes: Vec<Volcano>,
}

impl Deref for HotspotPlan {
    type Target = LinePlan;

    fn deref(&self) -> &LinePlan {
        &self.line
    }
}

impl Chain {
    /// The height of the swell over the line at an age in millions of
    /// years, in meters. The heated plate sinks by the GDH1 curve, and the
    /// swell ends where it is as deep as the floor around it.
    pub fn swell_m(&self, age: f64) -> f64 {
        (self.floor_depth - gdh1_depth(self.reset_age + age.max(0.0))).max(0.0)
    }
}

impl HotspotLine {
    /// Starts a line with no points on a heightmap with `face_size` texels
    /// along one side of a face. The settings go inside their limits.
    pub fn new(spec: Hotspot, face_size: usize) -> HotspotLine {
        let step = FRAC_PI_2 / face_size as f64;
        let radius = spec
            .radius
            .clamp(4.0 * step, crate::ridge::MAX_RIDGE_RADIUS);
        let spec = Hotspot {
            radius,
            height_m: spec
                .height_m
                .clamp(HOTSPOT_HEIGHT_RANGE.0, HOTSPOT_HEIGHT_RANGE.1),
            speed_mm: spec
                .speed_mm
                .clamp(HOTSPOT_SPEED_RANGE.0, HOTSPOT_SPEED_RANGE.1),
            world_km: spec.world_km.max(1.0),
            ..spec
        };
        HotspotLine {
            spec,
            line: StrokeLine::new(radius, face_size),
            chain: None,
        }
    }

    /// The settings, inside their limits.
    pub fn spec(&self) -> Hotspot {
        self.spec
    }

    /// Adds a point with a flow from 0 to 1, as `StrokeLine::push`. A new
    /// point ends the chain of `finish`.
    pub fn push(&mut self, dir: V3, flow: f64) {
        let len = self.line.len();
        self.line.push(dir, flow);
        if self.line.len() != len {
            self.chain = None;
        }
    }

    /// The number of points.
    pub fn len(&self) -> usize {
        self.line.len()
    }

    pub fn is_empty(&self) -> bool {
        self.line.is_empty()
    }

    /// The length of the line, in radians.
    pub fn length(&self) -> f64 {
        self.line.length()
    }

    /// Whether the line has a chain from `finish`.
    pub fn is_finished(&self) -> bool {
        self.chain.is_some()
    }

    /// The volcanoes of the chain, in the order from the end of the line.
    pub fn volcanoes(&self) -> &[Volcano] {
        self.chain.as_ref().map_or(&[], |chain| &chain.volcanoes)
    }

    /// Places the chain of volcanoes on the line. `map` holds the ground
    /// from before the stroke. Call it after the last point, and again after
    /// new points. Before it, a plan changes nothing.
    pub fn finish(&mut self, map: &Heightmap) {
        let spec = self.spec;
        let line = &self.line;
        let n = line.face_size();
        let step = FRAC_PI_2 / n as f64;
        let km = spec.world_km;
        let length = line.length();
        let speed = spec.speed_mm / km;
        let r = spec.radius;
        let mut chain = Chain {
            floor_depth: gdh1_depth(0.0),
            floor_age: 0.0,
            reset_age: 0.0,
            speed,
            volcanoes: Vec::new(),
            ages: Vec::new(),
            heights_m: Vec::new(),
            drowned: Vec::new(),
        };
        if line.len() < 2 {
            self.chain = Some(Arc::new(chain));
            return;
        }
        let end = line.at(length);
        // The place of the hotspot goes into the seed, so each chain differs.
        let spec = Hotspot {
            seed: place_seed(spec.seed, end),
            ..spec
        };
        let floor = sea_floor(map, end, r);
        chain.floor_age = floor.age_myr;
        chain.floor_depth = gdh1_depth(floor.age_myr);
        chain.reset_age = gdh1_age(chain.floor_depth - SWELL_M);

        // The places of the volcanoes, from the end of the line back.
        let largest = spec.height_m / PLACE_RATIO / (km * 1000.0);
        let spacing = SPACING * largest;
        struct Site {
            s: f64,
            across: f64,
            raw: f64,
            env: f64,
            flow: f64,
            group: u32,
        }
        let mut sites: Vec<Site> = Vec::new();
        let mut s = 0.0;
        let mut k = 0u32;
        // The group of the volcano, its size, and the volcanoes left in it.
        let mut group = 0u32;
        let mut group_size = 1.0;
        let mut left = 0u32;
        // The start of the second group, where the change of the supply
        // starts.
        let mut second = f64::INFINITY;
        while s <= length && k < 4096 {
            // The group at the hotspot goes on until it has a volcano past
            // the hotspot.
            if left == 0 && (group == 0 || sites.last().is_some_and(|t| t.s > 0.0)) {
                let g = |i: u32| draw(hash(group, spec.seed, SALT_GROUP, 0), i);
                left = 1 + (g(0) * GROUP_MOST) as u32;
                if group == 0 {
                    left = left.max(2);
                }
                // The group over the hotspot is the largest.
                group_size = if group == 0 {
                    1.0
                } else {
                    GROUP_LOW + (1.0 - GROUP_LOW) * g(1)
                };
                if group > 0 {
                    s += spacing * (GROUP_GAP.0 + (GROUP_GAP.1 - GROUP_GAP.0) * g(2));
                    if s > length {
                        break;
                    }
                    second = second.min(s);
                }
                group += 1;
            }
            let h = hash(k, spec.seed, SALT_PLACE, 0);
            let d = |i: u32| draw(h, i);
            let double = noise1(s / (DOUBLE_SIZE * spacing), SALT_DOUBLE, 0, spec.seed).0;
            let double = double > DOUBLE_ABOVE;
            let off = d(3) < OFF_LINE;
            let side = if k.is_multiple_of(2) { 1.0 } else { -1.0 };
            let across = if off {
                side * (OFF_ACROSS.0 + (OFF_ACROSS.1 - OFF_ACROSS.0) * d(4)) * largest
            } else if double {
                side * DOUBLE_ACROSS * largest
            } else {
                (2.0 * d(0) - 1.0) * SINGLE_ACROSS * largest
            };
            let flux = noise1(s / (FLUX_SIZE * spacing), SALT_FLUX, 0, spec.seed).0;
            let flux = FLUX_LOW + (1.0 - FLUX_LOW) * (0.5 + 0.5 * flux);
            let fade = if second.is_finite() {
                smoothstep(second, second + FLUX_FADE * spacing, s)
            } else {
                0.0
            };
            let flux = 1.0 - (1.0 - flux) * fade;
            let env = if second.is_finite() {
                envelope((s - second) / spacing)
            } else {
                1.0
            };
            let flow = line.frame(length - s).2;
            let size = 0.65 + 0.35 * d(1).powf(0.6);
            let part = if off {
                OFF_HEIGHT
            } else if double {
                DOUBLE_HEIGHT
            } else {
                1.0
            };
            sites.push(Site {
                s,
                across,
                raw: size * flux * group_size * part,
                env,
                flow,
                group,
            });
            left = left.saturating_sub(1);
            if !off {
                let part = if double { 0.5 } else { 1.0 };
                s += spacing * (STEP.0 + (STEP.1 - STEP.0) * d(2)) * part;
            }
            k += 1;
        }
        // The tallest volcano of the group at the hotspot past the hotspot,
        // at its full height, has the height of the setting times its flow.
        // No volcano is taller than the setting times its envelope and its
        // flow. The volcanoes over the hotspot are still growing, so they
        // are smaller.
        let most = (sites.iter())
            .filter(|t| t.group == 1 && t.s > 0.0)
            .map(|t| t.raw)
            .fold(0.0, f64::max);
        let height_of = |t: &Site| {
            let part = if most > 0.0 { t.raw / most } else { 0.0 };
            let part = part.min(if t.s > 0.0 { 1.0 } else { HOTSPOT_PART });
            (spec.height_m * t.flow * t.env * part).min(HEIGHT_MOST_M)
        };
        for (k, site) in sites.iter().enumerate() {
            let (dir, tangent, _) = line.frame(length - site.s);
            let left = normalize(cross(dir, tangent));
            let a = site.across;
            let center = normalize(add(scale(dir, a.cos()), scale(left, a.sin())));
            let axis = normalize(add(tangent, scale(center, -dot(tangent, center))));
            let age = site.s / speed;
            let reach = REACH * r - a.abs() - 2.0 * step;
            let seed = hash(k as u32, spec.seed, SALT_SHAPE, 0);
            let load_age = (chain.floor_age - age).max(0.0);
            let height = height_of(site);
            let Some(mut v) = shield_volcano(
                seed,
                center,
                axis,
                height,
                HOTSPOT_RATIO,
                reach,
                age,
                load_age,
                km,
                n,
            ) else {
                continue;
            };
            v.0[GROWTH] = growth(age) as f32;
            chain.heights_m.push(v.height() / LEVELS_PER_M);
            chain.volcanoes.push(v);
            chain.ages.push(age);
        }

        // The pedestal of each volcano reaches past the middle of the gap to
        // the next volcano along its axis on each side, so the chain stands
        // on one ridge.
        let reaches: Vec<f64> = (chain.volcanoes.iter())
            .map(|v| {
                let mut gap = [f64::INFINITY; 2];
                for w in &chain.volcanoes {
                    let (u, across) = v.local(w.center());
                    if u.abs() > 0.25 && u.abs() > across.abs() {
                        let side = usize::from(u > 0.0);
                        gap[side] = gap[side].min(u.abs());
                    }
                }
                let gap = gap
                    .into_iter()
                    .filter(|g| g.is_finite())
                    .fold(0.0, f64::max);
                (0.5 * gap / PEDESTAL_MIDDLE).max(PEDESTAL_ALONG)
            })
            .collect();
        for (v, reach) in chain.volcanoes.iter_mut().zip(reaches) {
            v.set_pedestal_reach(reach);
        }

        // The moats add up under the chain. The volcanoes stand on the
        // floor over them.
        let moats = |vs: &[Volcano]| -> Vec<f64> {
            let at = |c: V3| vs.iter().map(|v| v.growth() * v.moat(c)).sum::<f64>();
            vs.iter().map(|v| at(v.center())).collect()
        };
        let most = moats(&chain.volcanoes).into_iter().fold(0.0, f64::max);
        let target = LINE_MOAT * spec.height_m * LEVELS_PER_M;
        if most > target {
            let k = target / most;
            for v in &mut chain.volcanoes {
                v.0[FLEX] = (v.get(FLEX) * k) as f32;
            }
        }
        let sums = moats(&chain.volcanoes);

        // The summit of an island goes down, and the waves cut its top flat
        // when it goes under the sea.
        let swell_full = chain.swell_m(GROW_MYR);
        for (i, v) in chain.volcanoes.iter_mut().enumerate() {
            let age = chain.ages[i];
            let moat = sums[i];
            v.0[HEIGHT] = (v.height() + moat) as f32;
            let c = v.center();
            let start_m = ELEV_MIN + map.level_at(c) / LEVELS_PER_M;
            let base_m = start_m + swell_full - moat / LEVELS_PER_M;
            // The flank breaks at the sea of the time of its growth. A
            // volcano that still grows breaks at the sea of now.
            let height_m = v.height() / LEVELS_PER_M;
            v.0[BREAK] = (-base_m / (height_m * v.growth())).clamp(0.05, 1.0) as f32;
            let edifice_m = v.edifice(c) / LEVELS_PER_M;
            let peak = base_m + edifice_m;
            let swell = |t: f64| chain.floor_depth - gdh1_depth(chain.reset_age + t);
            let sunk = |t: f64| swell(GROW_MYR).max(0.0) - swell(t).max(0.0);
            let above = |t: f64| peak - ISLAND_EROSION_M * (t - GROW_MYR).max(0.0) - sunk(t);
            let mut drowned = f64::INFINITY;
            if peak > 0.0 {
                let (mut low, mut high) = (GROW_MYR, GROW_MYR + 1000.0);
                if above(high) <= 0.0 {
                    for _ in 0..60 {
                        let mid = 0.5 * (low + high);
                        if above(mid) > 0.0 {
                            low = mid;
                        } else {
                            high = mid;
                        }
                    }
                    drowned = high;
                }
            }
            chain.drowned.push(drowned);
            let band = REEF_BAND * chain.heights_m[i] * LEVELS_PER_M;
            if age < drowned {
                if age > GROW_MYR && edifice_m > 0.0 {
                    let base = peak - sunk(age) - edifice_m;
                    let f = ((above(age) - base) / edifice_m).clamp(0.0, 1.0);
                    v.set_lower(1.0 - f);
                }
                if spec.reefs && peak > 0.0 {
                    v.0[RIM] = band as f32;
                    v.0[SHORE] = level_of_m(0.0) as f32;
                    v.0[REEF] = 1.0;
                }
            } else {
                let since = age - drowned;
                let sink = SINK_M * (1.0 - (-since / SINK_MYR).exp());
                let cap = level_of_m(-WAVE_BASE_M - (sunk(age) - sunk(drowned)) - sink);
                v.0[CAP] = cap as f32;
                // The top keeps the erosion of the time when the waves began
                // to cut it, and it sinks with the cap.
                if edifice_m > 0.0 {
                    let (mut low, mut high) = (GROW_MYR, drowned);
                    if above(low) > PLATFORM_M {
                        for _ in 0..60 {
                            let mid = 0.5 * (low + high);
                            if above(mid) > PLATFORM_M {
                                low = mid;
                            } else {
                                high = mid;
                            }
                        }
                    }
                    let eroded = ISLAND_EROSION_M * (low - GROW_MYR);
                    v.set_lower(((eroded + sink) / edifice_m).min(1.0));
                }
                if spec.reefs && since < REEF_MYR {
                    v.0[RIM] = band as f32;
                    v.0[SHORE] = cap as f32;
                    v.0[LAGOON] = level_of_m(LAGOON_M) as f32;
                    v.0[REEF] =
                        (1.0 - smoothstep(REEF_MYR - REEF_FADE_MYR, REEF_MYR, since)) as f32;
                }
            }
        }
        self.chain = Some(Arc::new(chain));
    }

    /// The plan for the points from `from` on, as `StrokeLine::plan`.
    pub fn plan(&self, from: usize) -> HotspotPlan {
        self.wrap(self.line.plan(from), PLAN_VOLCANOES)
    }

    /// The plan of the whole line, with each volcano.
    pub(crate) fn plan_all(&self) -> HotspotPlan {
        self.wrap(self.line.plan_between(0.0..self.line.length()), usize::MAX)
    }

    /// As `StrokeLine::margin`.
    #[doc(hidden)]
    pub fn margin(&self, p: V3) -> f64 {
        self.line.margin(p)
    }

    fn wrap(&self, mut line: LinePlan, most: usize) -> HotspotPlan {
        let Some(chain) = &self.chain else {
            line.rects = [None; FACES];
            return HotspotPlan {
                hotspot: self.spec,
                line,
                chain: None,
                volcanoes: Vec::new(),
            };
        };
        let circles = line.circles();
        let mut near: Vec<(f64, usize)> = (chain.volcanoes.iter().enumerate())
            .filter_map(|(i, v)| {
                let d = circles.iter().map(|&(c, r)| angle(c, v.center()) - r);
                let d = d.fold(f64::INFINITY, f64::min);
                (d < v.reach()).then_some((d, i))
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        near.truncate(most);
        let mut near: Vec<usize> = near.into_iter().map(|(_, i)| i).collect();
        near.sort_unstable();
        HotspotPlan {
            hotspot: self.spec,
            line,
            chain: Some(chain.clone()),
            volcanoes: near.into_iter().map(|i| chain.volcanoes[i]).collect(),
        }
    }

    /// The age of the place at a distance along the line, in millions of
    /// years.
    #[cfg(test)]
    fn age(&self, u: f64) -> f64 {
        (self.line.length() - u).max(0.0) * self.spec.world_km / self.spec.speed_mm
    }
}

impl HotspotPlan {
    /// The volcanoes as 32-bit numbers, `VOLCANO_FLOATS` for each.
    pub fn floats(&self) -> Vec<f32> {
        self.volcanoes.iter().flat_map(|v| v.0).collect()
    }

    /// Whether the line of the plan has a chain.
    pub fn is_finished(&self) -> bool {
        self.chain.is_some()
    }

    /// The depth of the floor at the end of the line in meters, the reset
    /// age, and the plate speed in radians per million years.
    #[cfg(feature = "gpu")]
    pub(crate) fn swell_numbers(&self) -> (f64, f64, f64) {
        self.chain.as_ref().map_or((0.0, 0.0, 1.0), |chain| {
            (chain.floor_depth, chain.reset_age, chain.speed)
        })
    }

    /// The level of a texel at unit direction `p` with level `start` from
    /// before the stroke and place `at` on the line, with only the
    /// volcanoes in `near`.
    pub(crate) fn level_of(&self, near: &[Volcano], start: u16, at: LinePoint, p: V3) -> u16 {
        let Some(chain) = &self.chain else {
            return start;
        };
        let age = (self.line.length - at.u).max(0.0) / chain.speed;
        let distance = (at.across * at.across + at.past * at.past).sqrt() / self.hotspot.radius;
        let swell = chain.swell_m(age) * LEVELS_PER_M * bump(distance);
        let moat: f64 = near.iter().map(|v| v.growth() * v.moat(p)).sum();
        let old = f64::from(start);
        let base = old + swell - moat;
        let mut top = (0.0, 1.0);
        for v in near {
            let e = v.growth() * v.edifice(p);
            if e > 0.0 {
                top = join(top, shaped(v, base, e));
            }
        }
        let top = top.0;
        let new = (base + top + 0.5).floor().clamp(0.0, 65535.0);
        self.hotspot.limits.apply(old, new) as u16
    }
}

/// The number of erode steps of a hotspot track, and their rate at an
/// erosion of 1.
pub const HOTSPOT_ERODE_STEPS: usize = 24;
pub const HOTSPOT_ERODE_RATE: f64 = 6.0;

/// The radius of the erode brush of an island, in base radii.
const ISLAND_BRUSH: f64 = 1.3;

/// An eroded cell keeps a part of its drop that grows with the square of
/// its age, and all of it from this age in millions of years. From there to
/// twice this age, the drop grows by up to `OLD_GAIN` more, so the old
/// islands have deep valleys.
const KEEP_MYR: f64 = 3.0;
const OLD_GAIN: f64 = 1.0;

/// The rain shadow makes the drop on the windward side of an island larger
/// by this part times the shadow, and on the lee side smaller by as much.
const WINDWARD: f64 = 1.0;

impl HotspotLine {
    /// One erode brush over each volcano that is an island.
    fn island_brushes(&self) -> Vec<ErodeBrush> {
        let Some(chain) = &self.chain else {
            return Vec::new();
        };
        let islands = (0..chain.volcanoes.len()).filter(|&i| chain.ages[i] < chain.drowned[i]);
        islands
            .map(|i| {
                let v = &chain.volcanoes[i];
                ErodeBrush {
                    center: v.center(),
                    radius: ISLAND_BRUSH * v.base_radius(),
                    hardness: 0.6,
                    flow: 1.0,
                }
            })
            .collect()
    }
}

/// The window of the cells of the erosion of a finished hotspot track: the
/// islands of the chain and room around them for the flow, as
/// `margin_window`. With no islands, it is the window around the end of the
/// line.
pub fn hotspot_window(line: &HotspotLine, face_size: usize) -> Window {
    let mut circles: Vec<(V3, f64)> = (line.island_brushes().iter())
        .map(|b| (b.center, b.radius))
        .collect();
    if circles.is_empty() && !line.is_empty() {
        circles.push((line.line.at(line.length()), line.spec.radius));
    }
    if circles.is_empty() {
        circles.push(([1.0, 0.0, 0.0], line.spec.radius));
    }
    circle_window(&circles, face_size)
}

/// Erodes the islands of a finished hotspot track with stream power, as the
/// Erode brush erodes land. `erosion` holds the cells of the ground from
/// before the stroke. The cells under the islands get the levels of the
/// line, and then `HOTSPOT_ERODE_STEPS` steps erode them. The sea is the
/// base level: the sea floor does not change, and the land goes down to the
/// sea and no lower. A cell keeps a part of its drop that grows with its
/// age, so the old islands have deep valleys and the young ones have few.
/// With a rain shadow, the drop is larger on the side of each island that
/// faces the wind of the rain of `erosion`, and smaller on its lee side.
/// `amount` from 0 to 1 sets the rate. Returns the drops: the levels of the
/// line minus the eroded levels. Lay each plan of the line before the step.
pub fn erode_hotspot(erosion: &mut Erosion, line: &HotspotLine, amount: f64) -> ErodeStep {
    let brushes = line.island_brushes();
    if brushes.is_empty() {
        return erosion.drops_since(erosion.levels(), &brushes);
    }
    let limits = line.spec.limits;
    let sea = level_of_m(0.0).ceil() as u16;
    erosion.set_limits(Limits {
        floor: limits.floor.max(sea),
        ..limits
    });
    let plan = line.plan_all();
    let speed = plan.chain.as_ref().map_or(1.0, |chain| chain.speed);
    let ages: Vec<Cell<f64>> = erosion.levels().iter().map(|_| Cell::new(0.0)).collect();
    let sides: Vec<Cell<f64>> = erosion.levels().iter().map(|_| Cell::new(1.0)).collect();
    let rain = erosion.rain();
    let windward = if rain.is_even() {
        0.0
    } else {
        WINDWARD * rain.shadow.min(1.0)
    };
    erosion.lift_cells(&brushes, |i, dir, now| match plan.line.place(dir) {
        LinePlace::On(at) => {
            ages[i].set((plan.line.length - at.u).max(0.0) / speed);
            // The side of the nearest island that the cell is on.
            let near = (brushes.iter()).min_by(|a, b| {
                (angle(a.center, dir) / a.radius).total_cmp(&(angle(b.center, dir) / b.radius))
            });
            if let Some(near) = near.filter(|_| windward > 0.0) {
                let out = add(dir, scale(near.center, -dot(dir, near.center)));
                let length = dot(out, out).sqrt();
                if length > 0.0 {
                    let up = dot(scale(out, 1.0 / length), rain.upwind(near.center));
                    sides[i].set(1.0 + windward * up);
                }
            }
            Some(plan.level_of(&plan.volcanoes, now, at, dir))
        }
        _ => None,
    });
    let lifted = erosion.levels().to_vec();
    let radius = brushes.iter().map(|b| b.radius).fold(0.0, f64::max);
    let rate = amount * HOTSPOT_ERODE_RATE * (radius / erosion.cell_angle() / 16.0).sqrt();
    erosion.steps(&brushes, rate, HOTSPOT_ERODE_STEPS);
    let kept: Vec<u16> = (lifted
        .iter()
        .zip(erosion.levels())
        .zip(ages.iter().zip(&sides)))
    .map(|((&before, &now), (age, side))| {
        let age = age.get();
        let gain = 1.0 + OLD_GAIN * smoothstep(KEEP_MYR, 2.0 * KEEP_MYR, age);
        let part = (age / KEEP_MYR).min(1.0).powi(2) * gain * side.get();
        let drop = f64::from(before.saturating_sub(now)) * part;
        // The windward drop is larger than the drop of the steps, and
        // it stops at the sea.
        let room = f64::from(before.saturating_sub(sea));
        before - drop.min(room).round() as u16
    })
    .collect();
    erosion.drops_since_to(&lifted, &kept, &brushes)
}

/// The envelope of the heights of the volcanoes `x` mean distances past the
/// group at the hotspot.
fn envelope(x: f64) -> f64 {
    let x = x.max(0.0);
    1.0 - OLDER * (x / OLDER_SIZE).min(1.0)
        - OLDEST * (1.0 - (-(x - OLDER_SIZE).max(0.0) / OLDEST_SIZE).exp())
}

/// The seed of a chain with its hotspot at `end`. The place is in steps of
/// about 1 m on a world of the size of the Earth.
fn place_seed(seed: u32, end: V3) -> u32 {
    let q = |x: f64| (x * f64::from(1u32 << 23)).round() as i64 as u32;
    hash(q(end[0]), q(end[1]), q(end[2]), seed)
}

/// The growth of a volcano at an age in millions of years.
fn growth(age: f64) -> f64 {
    GROW_FROM + (1.0 - GROW_FROM) * smoothstep(0.0, GROW_MYR, age)
}

/// The 16-bit level of an elevation in meters, with no rounding.
fn level_of_m(m: f64) -> f64 {
    (m - ELEV_MIN) * LEVELS_PER_M
}

/// The height of a volcano with the edifice `e` over the level `base`, with
/// its wave-cut top and its reef, and the part of the rounding of `join`
/// that it takes: 0 with a wave-cut top, else 1 where the reef does not
/// change it, and 0 where the reef changes it by `FREE_M` or more.
fn shaped(v: &Volcano, base: f64, e: f64) -> (f64, f64) {
    let z = base + e;
    let cap = v.get(CAP);
    let mut s = if cap < f64::from(NO_CAP) {
        // The platform is at the cap where the top of the volcano is, and it
        // slopes down to its edge.
        let k = PLATFORM_RELIEF;
        let edge = cap - k / (1.0 - k) * (PLATFORM_M + WAVE_BASE_M) * LEVELS_PER_M;
        smooth_min(z, edge + k * (z - edge), CAP_ROUND_M * LEVELS_PER_M)
    } else {
        z
    };
    let band = v.get(RIM);
    if band > 0.0 {
        // The reef is flat at its top from `band` below the old shore up,
        // and its outer wall goes down from there.
        let shore = v.get(SHORE);
        let top = level_of_m(REEF_TOP_M);
        let reef = (top - REEF_WALL * (shore - band - z)).min(top);
        let outer = s.max(reef);
        let lagoon = v.get(LAGOON);
        let inner = if lagoon > 0.0 {
            smoothstep(0.0, 0.15, (z - shore) / band)
        } else {
            0.0
        };
        let reefed = outer + (lagoon - outer) * inner;
        s += (reefed - s) * v.get(REEF);
    }
    let free = if cap < f64::from(NO_CAP) {
        0.0
    } else {
        (1.0 - (s - z).abs() / (FREE_M * LEVELS_PER_M)).max(0.0)
    };
    ((s - base).max(0.0), free)
}

/// The join of the heights of two volcanoes, each with the part of the
/// rounding that it takes: their maximum, rounded by `JOIN_M` where both
/// are high. Thus two shields merge, and two flat tops do not get a bump.
fn join((a, fa): (f64, f64), (b, fb): (f64, f64)) -> (f64, f64) {
    let free = fa.min(fb);
    let k = (JOIN_M * LEVELS_PER_M * free).min(a).min(b);
    if k <= 0.0 {
        return (a.max(b), free);
    }
    let h = (k - (a - b).abs()).max(0.0) / k;
    (a.max(b) + 0.25 * k * h * h, free)
}

impl Heightmap {
    /// Lays the hotspot track of a plan on the ground from before the
    /// stroke, as `ridge` lays an ocean ridge. Call this in a stroke.
    /// Returns the number of texels that changed.
    pub fn hotspot(&mut self, plan: &HotspotPlan) -> usize {
        let n = self.face_size();
        assert_eq!(plan.face_size, n, "the face size of the plan");
        const BLOCK: usize = 32;
        let mut changed = 0;
        let mut near = Vec::new();
        for (face, rect) in plan.rects.into_iter().enumerate() {
            let Some(rect) = rect else { continue };
            for by in (rect.y0..rect.y1).step_by(BLOCK) {
                for bx in (rect.x0..rect.x1).step_by(BLOCK) {
                    let (x1, y1) = ((bx + BLOCK).min(rect.x1), (by + BLOCK).min(rect.y1));
                    // The volcanoes that reach the block.
                    let middle = self.texel_dir(face, (bx + x1) / 2, (by + y1) / 2);
                    let corners = [(bx, by), (x1 - 1, y1 - 1), (bx, y1 - 1), (x1 - 1, by)];
                    let far = corners
                        .map(|(x, y)| angle(middle, self.texel_dir(face, x, y)))
                        .into_iter()
                        .fold(0.0, f64::max)
                        + 2.0 * FRAC_PI_2 / n as f64;
                    near.clear();
                    near.extend(
                        (plan.volcanoes.iter())
                            .filter(|v| angle(v.center(), middle) < v.reach() + far),
                    );
                    for y in by..y1 {
                        for x in bx..x1 {
                            let p = self.texel_dir(face, x, y);
                            let place = match plan.line.place(p) {
                                LinePlace::Far => continue,
                                LinePlace::Off => None,
                                LinePlace::On(place) => Some(place),
                            };
                            let start = self.level_before(face, x, y);
                            let new = place.map_or(start, |at| plan.level_of(&near, start, at, p));
                            if new != self.get(face, x as i64, y as i64) {
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
    use crate::flow::WindowHeights;
    use crate::line::LINE_SPACING;
    use crate::math::lonlat_to_dir;
    use crate::rain::Rain;
    use crate::seamount::{CALDERA_MYR, SLIDE_MYR};
    use std::f64::consts::TAU;

    const KM: f64 = 6371.0;
    const N: usize = 2048;
    const FLOOR_M: f64 = -5448.0;

    fn spec() -> Hotspot {
        Hotspot {
            radius: HOTSPOT_RADIUS_KM / KM,
            height_m: HOTSPOT_HEIGHT_M,
            speed_mm: HOTSPOT_SPEED_MM,
            reefs: false,
            world_km: KM,
            seed: 7,
            limits: Limits::default(),
        }
    }

    /// The seeds of the tests that measure the volcanoes of a kind. One
    /// chain has few of a kind, so these tests look at many chains.
    const SEEDS: std::ops::Range<u32> = 0..8;

    fn seeded(seed: u32) -> Hotspot {
        Hotspot { seed, ..spec() }
    }

    /// A line along the equator from longitude `from` to `to` in degrees,
    /// with the point spacing of the app and a flow.
    fn line(spec: Hotspot, n: usize, from: f64, to: f64, flow: f64) -> HotspotLine {
        let mut line = HotspotLine::new(spec, n);
        let step = (LINE_SPACING * line.spec().radius).to_degrees();
        let count = ((to - from).abs() / step).ceil() as usize;
        for i in 0..=count {
            let lon = from + (to - from) * i as f64 / count as f64;
            line.push(lonlat_to_dir(lon, 0.0), flow);
        }
        line
    }

    /// Lays a finished line on flat floor. Returns the map, with the ground
    /// from before the stroke in `before`.
    fn lay(line: &mut HotspotLine, n: usize, floor_m: f64) -> (Heightmap, Heightmap) {
        let before = Heightmap::new(n, meters_to_level(floor_m));
        let mut map = Heightmap::new(n, meters_to_level(floor_m));
        line.finish(&map);
        map.begin_stroke();
        let mut from = 0;
        while from < line.len() {
            let plan = line.plan(from);
            map.hotspot(&plan);
            from = plan.end;
        }
        map.end_stroke();
        (map, before)
    }

    /// Lays a finished line on flat floor with its erosion at `amount`.
    fn lay_eroded(line: &mut HotspotLine, n: usize, amount: f64) -> Heightmap {
        lay_eroded_in(line, n, amount, Rain::default())
    }

    /// `lay_eroded` with a rain.
    fn lay_eroded_in(line: &mut HotspotLine, n: usize, amount: f64, rain: Rain) -> Heightmap {
        let mut map = Heightmap::new(n, meters_to_level(FLOOR_M));
        line.finish(&map);
        let window = hotspot_window(line, n);
        let mut erosion = Erosion::window_alone(&WindowHeights::new(&map, window));
        erosion.set_branching(line.spec().seed, 0.5);
        erosion.set_rain(rain);
        let step = erode_hotspot(&mut erosion, line, amount);
        map.begin_stroke();
        let mut from = 0;
        while from < line.len() {
            let plan = line.plan(from);
            map.hotspot(&plan);
            from = plan.end;
        }
        map.erode(&step);
        map.end_stroke();
        map
    }

    fn meters(level: f64) -> f64 {
        ELEV_MIN + level / LEVELS_PER_M
    }

    /// The elevation at a longitude and a latitude, in meters.
    fn at(map: &Heightmap, lon: f64, lat: f64) -> f64 {
        meters(map.level_at(lonlat_to_dir(lon, lat)))
    }

    fn degrees(km: f64) -> f64 {
        (km / KM).to_degrees()
    }

    #[test]
    fn age_is_distance_from_the_end_over_plate_speed() {
        let spec = spec();
        let (from, to) = (-6.0, 6.0);
        let mut line = line(spec, N, from, to, 1.0);
        lay(&mut line, N, FLOOR_M);
        let length = line.length();
        // 90 mm/yr is 90 km in a million years.
        assert!((line.age(length - 90.0 / KM) - 1.0).abs() < 1e-9);
        assert_eq!(line.age(length), 0.0);
        let chain = line.chain.clone().unwrap();
        for (v, &age) in chain.volcanoes.iter().zip(&chain.ages) {
            let (_, u) = line.line.nearest(v.center());
            assert!((age - line.age(u)).abs() < 0.02, "{age} {}", line.age(u));
        }
        assert_eq!(chain.ages[0], 0.0);
        assert!(angle(chain.volcanoes[0].center(), lonlat_to_dir(to, 0.0)) < 0.1 * spec.radius);
        // With no volcanoes, the floor rises by the swell of its age.
        let mut bare = self::line(spec, N, from, to, 0.0);
        let (map, before) = lay(&mut bare, N, FLOOR_M);
        let lat = degrees(0.5 * HOTSPOT_RADIUS_KM);
        for lon in [-3.0, 0.0, 3.0, 6.0] {
            let age = (to - lon).to_radians() * KM / spec.speed_mm;
            let rise = at(&map, lon, lat) - at(&before, lon, lat);
            let swell = chain.swell_m(age) * bump(0.5);
            assert!((rise - swell).abs() < 1.0, "{lon}: {rise} {swell}");
        }
    }

    #[test]
    fn volcanoes_have_seeded_spacing_and_double_lines() {
        let spec = spec();
        let mut long = line(spec, N, -20.0, 20.0, 1.0);
        let (_, _) = lay(&mut long, N, FLOOR_M);
        let chain = long.chain.clone().unwrap();
        let largest = spec.height_m / PLACE_RATIO / 1000.0;
        let length = long.length();
        let mut along = Vec::new();
        let mut across = Vec::new();
        for v in &chain.volcanoes {
            let (d, u) = long.line.nearest(v.center());
            let side = dot(v.center(), lonlat_to_dir(0.0, 90.0)).signum();
            along.push((length - u) * KM);
            across.push(d * KM * side);
        }
        // In a group of a single line, the volcanoes are about half a base
        // radius apart. Between two groups is a longer gap.
        let spacing = SPACING * largest;
        let singles: Vec<f64> = (1..along.len())
            .filter(|&i| across[i].abs() < 0.1 * largest && across[i - 1].abs() < 0.1 * largest)
            .map(|i| along[i] - along[i - 1])
            .collect();
        let steps: Vec<f64> = singles
            .iter()
            .copied()
            .filter(|&d| d <= STEP.1 * spacing)
            .collect();
        let mean = steps.iter().sum::<f64>() / steps.len() as f64;
        let gaps = singles
            .iter()
            .filter(|&&d| d > (STEP.0 + GROUP_GAP.0) * spacing)
            .count();
        eprintln!(
            "{} volcanoes, mean step {mean:.1} km, {gaps} gaps",
            along.len()
        );
        assert!((mean - spacing).abs() < 0.2 * spacing);
        assert!(gaps > 3);
        // Some parts have two lines, one on each side.
        let left = across.iter().filter(|&&a| a > 0.25 * largest).count();
        let right = across.iter().filter(|&&a| a < -0.25 * largest).count();
        let doubles = left + right;
        eprintln!("{left} left, {right} right");
        assert!(left > 3 && right > 3);
        assert!(doubles > along.len() / 6 && doubles < 2 * along.len() / 3);
        // The seed sets the chain.
        let mut other = line(Hotspot { seed: 8, ..spec }, N, -20.0, 20.0, 1.0);
        lay(&mut other, N, FLOOR_M);
        let other = other.chain.clone().unwrap();
        assert_ne!(other.volcanoes[1].center(), chain.volcanoes[1].center());
    }

    #[test]
    fn chains_in_other_places_differ_with_the_same_seed() {
        let spec = spec();
        let chain = |from: f64| {
            let mut line = line(spec, N, from, from + 40.0, 1.0);
            lay(&mut line, N, FLOOR_M);
            line.chain.clone().unwrap()
        };
        let (a, b) = (chain(-20.0), chain(-60.0));
        // The same stroke in the same place gives the same chain.
        assert_eq!(a.heights_m, chain(-20.0).heights_m);
        // The same stroke in another place gives another chain.
        assert_ne!(a.heights_m, b.heights_m);
    }

    #[test]
    fn volcanoes_are_shields_and_the_largest_has_the_setting() {
        for seed in SEEDS {
            let spec = Hotspot {
                height_m: 6000.0,
                ..seeded(seed)
            };
            let mut line = line(spec, N, -10.0, 10.0, 1.0);
            lay(&mut line, N, FLOOR_M);
            let chain = line.chain.clone().unwrap();
            let spacing = SPACING * spec.height_m / PLACE_RATIO / 1000.0;
            let distance = |i: usize| chain.ages[i] * spec.speed_mm / spacing;
            // The tallest volcano at its full height is in the group at the
            // hotspot, past the hotspot.
            let tallest = (0..chain.volcanoes.len())
                .max_by(|&a, &b| chain.heights_m[a].total_cmp(&chain.heights_m[b]))
                .unwrap();
            let h = chain.heights_m[tallest];
            assert!((h - 6000.0).abs() < 1.0, "{seed}: {h}");
            assert!(chain.ages[tallest] > 0.0, "{seed}");
            assert!(distance(tallest) < GROUP_MOST * STEP.1, "{seed}");
            for (i, (v, &h)) in chain.volcanoes.iter().zip(&chain.heights_m).enumerate() {
                assert!(v.shield() && v.cones() == 1 && v.rifts() >= 2);
                let radius_m = v.base_radius() * KM * 1000.0;
                assert!((h / radius_m - HOTSPOT_RATIO).abs() < 1e-3);
                // The volcanoes over the hotspot still grow, and they are
                // smaller.
                if chain.ages[i] == 0.0 {
                    assert!(h < HOTSPOT_PART * 6000.0 + 1.0, "{seed}: {h}");
                }
                // The old volcanoes are under the envelope.
                let past = distance(i) - GROUP_MOST * STEP.1 - GROUP_GAP.1;
                assert!(h < 6000.0 * envelope(past) + 1.0, "{seed}: {h}");
            }
            // Lower volcanoes vary in height.
            let lowest = chain.heights_m.iter().copied().fold(f64::MAX, f64::min);
            assert!(lowest < 0.7 * h, "{seed}: {lowest}");
        }
    }

    #[test]
    fn the_swell_is_1_km_high_1000_km_wide_and_falls_with_age() {
        // With no flow, the line has no volcanoes.
        let mut line = line(spec(), N, -10.0, 10.0, 0.0);
        let (map, before) = lay(&mut line, N, FLOOR_M);
        assert!(line.volcanoes().is_empty());
        let rise = |lon: f64, km: f64| at(&map, lon, degrees(km)) - at(&before, lon, degrees(km));
        let young = rise(10.0, 0.0);
        assert!((young - SWELL_M).abs() < 2.0, "{young}");
        // It ends 500 km from the line on each side.
        assert!(rise(9.0, 470.0) > 0.0 && rise(9.0, -470.0) > 0.0);
        assert!(rise(9.0, 510.0).abs() < 0.5 && rise(9.0, -510.0).abs() < 0.5);
        // It ends past the end of the line in a round cap.
        assert!(rise(10.0 + degrees(250.0), 0.0) > 0.4 * young);
        assert!(rise(10.0 + degrees(510.0), 0.0).abs() < 0.5);
        // It falls with age.
        let mut last = young;
        for lon in [8.0, 4.0, 0.0, -4.0, -8.0] {
            let r = rise(lon, 0.0);
            assert!(r < last, "{lon}: {r}");
            last = r;
        }
    }

    #[test]
    fn volcanoes_and_floor_sink_by_gdh1() {
        let mut line = line(spec(), N, -10.0, 10.0, 0.0);
        let (map, _) = lay(&mut line, N, FLOOR_M);
        let chain = line.chain.clone().unwrap();
        assert!((chain.reset_age - 25.9).abs() < 0.2, "{}", chain.reset_age);
        // Between two ages, the floor sinks as the GDH1 curve.
        let age = |lon: f64| (10.0 - lon).to_radians() * KM / 90.0;
        for (a, b) in [(8.0, 4.0), (4.0, -8.0)] {
            let sunk = at(&map, a, 0.0) - at(&map, b, 0.0);
            let gdh1 = gdh1_depth(chain.reset_age + age(b)) - gdh1_depth(chain.reset_age + age(a));
            assert!((sunk - gdh1).abs() < 1.0, "{sunk} {gdh1}");
        }
        // The flat tops of the guyots sink in the same way, and by the load
        // of the younger volcanoes.
        let mut pairs = 0;
        for seed in SEEDS {
            let spec = Hotspot {
                speed_mm: 30.0,
                ..seeded(seed)
            };
            let mut line = self::line(spec, N, -12.0, 12.0, 1.0);
            lay(&mut line, N, FLOOR_M);
            let chain = line.chain.clone().unwrap();
            let guyots: Vec<usize> = (0..chain.volcanoes.len())
                .filter(|&i| chain.ages[i] > chain.drowned[i])
                .collect();
            for w in guyots.windows(2) {
                let (i, j) = (w[0], w[1]);
                let cap = |i: usize| meters(chain.volcanoes[i].get(CAP));
                let sunk = cap(i) - cap(j);
                let drop = |i: usize| {
                    let since = chain.ages[i] - chain.drowned[i];
                    let load = SINK_M * (1.0 - (-since / SINK_MYR).exp());
                    chain.swell_m(chain.drowned[i]) - chain.swell_m(chain.ages[i]) + load
                };
                assert!((sunk - (drop(j) - drop(i))).abs() < 0.5, "{seed}: {sunk}");
                pairs += 1;
            }
        }
        eprintln!("{pairs} pairs of guyots");
        assert!(pairs > 8 * SEEDS.len(), "{pairs}");
    }

    /// A chain of large volcanoes on a slow plate, so it has guyots.
    fn slow(seed: u32, reefs: bool, n: usize) -> (Heightmap, HotspotLine) {
        let spec = Hotspot {
            speed_mm: 30.0,
            reefs,
            ..seeded(seed)
        };
        let mut line = line(spec, n, -12.0, 12.0, 1.0);
        let (map, _) = lay(&mut line, n, FLOOR_M);
        (map, line)
    }

    /// The tangent frame at a unit direction: east and north.
    fn frame(c: V3) -> (V3, V3) {
        let east = normalize(cross([0.0, 0.0, 1.0], c));
        (east, cross(c, east))
    }

    #[test]
    fn old_islands_have_deeper_valleys_than_young_ones() {
        let n = 2 * N;
        let mut line = line(spec(), n, -6.0, 6.0, 1.0);
        let eroded = lay_eroded(&mut line, n, 0.5);
        let (plain, _) = lay(&mut line, n, FLOOR_M);
        let chain = line.chain.clone().unwrap();
        // The depth of the valleys of an island: the 90th percentile of the
        // drops of its land.
        let valleys = |i: usize| {
            let v = &chain.volcanoes[i];
            let c = v.center();
            let (east, north) = frame(c);
            let r = 0.6 * v.base_radius();
            let mut drops = Vec::new();
            for j in -20..=20 {
                for k in -20..=20 {
                    let (x, y) = (r * f64::from(j) / 20.0, r * f64::from(k) / 20.0);
                    let p = normalize(add(add(c, scale(east, x)), scale(north, y)));
                    let (a, b) = (meters(plain.level_at(p)), meters(eroded.level_at(p)));
                    if a > 0.0 {
                        drops.push(a - b);
                    }
                }
            }
            drops.sort_by(f64::total_cmp);
            drops.get(drops.len() * 9 / 10).copied()
        };
        let islands = (0..chain.volcanoes.len()).filter(|&i| chain.ages[i] < chain.drowned[i]);
        let (mut young, mut old) = (Vec::new(), Vec::new());
        for i in islands {
            // An old island can be under the sea now.
            let Some(depth) = valleys(i) else { continue };
            eprintln!("age {:.1}: valleys {depth:.0} m", chain.ages[i]);
            if chain.ages[i] < 1.0 {
                young.push(depth);
            } else if chain.ages[i] > 4.0 {
                old.push(depth);
            }
        }
        let mean = |d: &[f64]| d.iter().sum::<f64>() / d.len() as f64;
        assert!(!young.is_empty() && !old.is_empty());
        assert!(mean(&old) > 150.0, "{old:?}");
        assert!(mean(&old) > 3.0 * mean(&young), "{young:?} {old:?}");
        // The sea floor does not erode.
        let p = lonlat_to_dir(0.0, degrees(300.0));
        assert_eq!(eroded.level_at(p), plain.level_at(p));
    }

    #[test]
    fn erosion_grades_the_islands_to_the_sea() {
        let n = 2 * N;
        let mut line = line(spec(), n, -6.0, 6.0, 1.0);
        let eroded = lay_eroded(&mut line, n, 1.0);
        let (plain, _) = lay(&mut line, n, FLOOR_M);
        let sea = meters_to_level(0.0);
        let (mut land, mut lowered, mut drowned) = (0, 0, 0);
        // The texels of face 0 within 8 degrees of the middle of the line.
        let half = (n as f64 * 8.0 / 90.0) as i64;
        let middle = n as i64 / 2;
        for y in middle - half..middle + half {
            for x in middle - half..middle + half {
                let (a, b) = (plain.get(0, x, y), eroded.get(0, x, y));
                if a > sea {
                    land += 1;
                    lowered += usize::from(b < a);
                    drowned += usize::from(b < sea);
                } else {
                    // The sea floor does not erode.
                    assert_eq!(a, b, "{x} {y}");
                }
            }
        }
        // The land goes down to the sea and no lower, so no island is a
        // ring around a drowned basin.
        assert!(lowered > land / 4, "{lowered} of {land}");
        assert_eq!(drowned, 0);
    }

    #[test]
    fn wave_cut_tops_make_deeper_guyots_on_older_volcanoes() {
        // The ages and the tops of the guyots of all chains.
        let mut tops = Vec::new();
        for seed in SEEDS {
            wave_cut_tops(seed, &mut tops);
        }
        tops.sort_by(|a: &(f64, f64), b| a.0.total_cmp(&b.0));
        let tops: Vec<f64> = tops.into_iter().map(|t| t.1).collect();
        // The tops of the older guyots are deeper.
        let count = tops.len();
        eprintln!("{count} guyots");
        assert!(count > 3 * SEEDS.len(), "{count}");
        let mean = |t: &[f64]| t.iter().sum::<f64>() / t.len() as f64;
        let (young, old) = tops.split_at(count / 2);
        assert!(mean(old) < mean(young) - 50.0);
        let fifth = count / 5;
        assert!(mean(&tops[count - fifth..]) < mean(&tops[..fifth]) - 100.0);
    }

    /// Checks the wave-cut tops of the guyots of a chain, and adds their
    /// ages and tops to `tops`.
    fn wave_cut_tops(seed: u32, tops: &mut Vec<(f64, f64)>) {
        let (map, line) = slow(seed, false, N);
        let chain = line.chain.clone().unwrap();
        for i in 0..chain.volcanoes.len() {
            let v = &chain.volcanoes[i];
            let (age, drowned) = (chain.ages[i], chain.drowned[i]);
            // A volcano that went under the sea soon after it grew has a
            // small top.
            if age <= drowned || drowned < 5.0 {
                continue;
            }
            // The levels near the center, and the part of them within 100 m
            // of the cap. The platform slopes down to its edge. An island
            // that barely rose above the sea leaves a platform of about 0.1
            // base radii.
            let c = v.center();
            let (east, north) = frame(c);
            let r = 0.05 * v.base_radius();
            let mut levels = Vec::new();
            for j in -4..=4 {
                for k in -4..=4 {
                    let (x, y) = (r * f64::from(j) / 4.0, r * f64::from(k) / 4.0);
                    let p = normalize(add(add(c, scale(east, x)), scale(north, y)));
                    levels.push(meters(map.level_at(p)));
                }
            }
            // The top of a larger neighbor can cover a part of the top.
            levels.sort_by(f64::total_cmp);
            let top = levels[levels.len() / 2];
            let cap = meters(v.get(CAP));
            let flat = levels.iter().filter(|&&m| m > cap - 100.0).count();
            eprintln!("age {age:.1}: top {top:.0} m, cap {cap:.0} m, flat {flat}");
            assert!(top < -WAVE_BASE_M + 5.0, "{seed}: {top}");
            assert!(flat > levels.len() * 3 / 4, "{seed}: {flat}");
            tops.push((age, top));
        }
    }

    #[test]
    fn reefs_make_an_atoll_rim_at_sea_level() {
        let atolls: usize = SEEDS.map(atolls).sum();
        eprintln!("{atolls} atolls");
        assert!(atolls >= 2 * SEEDS.len(), "{atolls}");
    }

    /// Checks the atolls of a chain with reefs, and returns their count.
    fn atolls(seed: u32) -> usize {
        // The rim of a reef is narrow, so the texels are smaller.
        let (map, line) = slow(seed, true, 2 * N);
        let (bare, _) = slow(seed, false, N);
        let chain = line.chain.clone().unwrap();
        let atolls: Vec<usize> = (0..chain.volcanoes.len())
            .filter(|&i| {
                let since = chain.ages[i] - chain.drowned[i];
                // No island is near, and the reef of no other volcano
                // crosses the platform.
                let alone = (0..chain.volcanoes.len()).filter(|&j| j != i).all(|j| {
                    let (a, b) = (&chain.volcanoes[i], &chain.volcanoes[j]);
                    let apart = angle(a.center(), b.center());
                    let far = a.base_radius() + 1.2 * b.base_radius();
                    let reef = 0.25 * a.base_radius();
                    (chain.ages[j] > chain.drowned[j] && apart > reef) || apart > far
                });
                since > 2.0 && since < REEF_MYR - REEF_FADE_MYR && chain.drowned[i] > 5.0 && alone
            })
            .collect();
        for &i in &atolls {
            atoll(&map, &bare, &chain.volcanoes[i]);
        }
        atolls.len()
    }

    /// Checks the lagoon and the rim of an atoll, and that it is a guyot on
    /// the map without reefs.
    fn atoll(map: &Heightmap, bare: &Heightmap, v: &Volcano) {
        let c = v.center();
        // The lagoon: the middle level near the center. A landslide scar
        // can cut into the platform.
        let (east, north) = frame(c);
        let r = 0.06 * v.base_radius();
        let mut levels: Vec<f64> = (0..64)
            .map(|k| {
                let (x, y) = (f64::from(k % 8) / 3.5 - 1.0, f64::from(k / 8) / 3.5 - 1.0);
                let p = normalize(add(add(c, scale(east, r * x)), scale(north, r * y)));
                meters(map.level_at(p))
            })
            .collect();
        levels.sort_by(f64::total_cmp);
        let lagoon = levels[levels.len() / 2];
        // The rim: the highest texel on rays from the center.
        let n = map.face_size();
        let texel = |p: V3| {
            let (face, a, b) = crate::cube::face_of(p);
            let at = |a: f64| ((crate::cube::warp(a) + 1.0) * 0.5 * n as f64).floor() as i64;
            meters(f64::from(map.get(face, at(a), at(b))))
        };
        let mut rims = Vec::new();
        for k in 0..16 {
            let a = f64::from(k) / 16.0 * std::f64::consts::TAU;
            let dir = add(scale(east, a.cos()), scale(north, a.sin()));
            let mut best = f64::MIN;
            for j in 1..400 {
                let d = v.base_radius() * f64::from(j) / 400.0;
                best = best.max(texel(normalize(add(c, scale(dir, d)))));
            }
            rims.push(best);
        }
        eprintln!("lagoon {lagoon:.0} m, rims {rims:.0?}");
        assert!((lagoon - LAGOON_M).abs() < 10.0, "{lagoon}");
        // A ray along the chain can end on the lagoon of a joined neighbor.
        let at_rim = rims
            .iter()
            .filter(|&&m| (m - REEF_TOP_M).abs() < 3.0)
            .count();
        assert!(at_rim >= 14, "{rims:?}");
        // Without reefs, it is a guyot.
        assert!(meters(bare.level_at(c)) < -WAVE_BASE_M + 1.0);
    }

    #[test]
    fn the_chain_makes_a_moat_and_an_arch() {
        let spec = spec();
        let mut line = line(spec, N, -10.0, 10.0, 1.0);
        let (map, before) = lay(&mut line, N, FLOOR_M);
        let chain = line.chain.clone().unwrap();
        // The change minus the swell, on a line across the chain from the
        // foot of the volcanoes out.
        let lon = 0.0;
        let age = (10.0f64 - lon).to_radians() * KM / spec.speed_mm;
        let profile: Vec<(f64, f64)> = (20..140)
            .map(|k| {
                let km = 5.0 * f64::from(k);
                let p = lonlat_to_dir(lon, degrees(km));
                let swell = chain.swell_m(age) * bump(km / HOTSPOT_RADIUS_KM);
                let d = meters(map.level_at(p)) - meters(before.level_at(p)) - swell;
                (d, km)
            })
            .collect();
        let lowest = |a: (f64, f64), b: (f64, f64)| if b.0 < a.0 { b } else { a };
        let moat = profile.iter().copied().fold((0.0, 0.0), lowest);
        let highest = |a: (f64, f64), b: (f64, f64)| if b.0 > a.0 { b } else { a };
        let past = profile.iter().copied().filter(|p| p.1 > moat.1);
        let arch = past.fold((0.0, 0.0), highest);
        eprintln!(
            "moat {:.0} m at {} km, arch {:.0} m at {} km",
            moat.0, moat.1, arch.0, arch.1
        );
        assert!(moat.0 < -300.0);
        assert!(arch.0 > 50.0 && arch.1 > moat.1 + 100.0);
        // The moat stays above the bottom of the range.
        let mut low = f64::MAX;
        for i in -100..=100 {
            for j in -60..=60 {
                low = low.min(at(&map, 0.1 * f64::from(i), 0.1 * f64::from(j)));
            }
        }
        eprintln!("lowest {low:.0} m");
        assert!(low > ELEV_MIN + 150.0, "{low}");
    }

    #[test]
    fn the_flanks_have_no_rays() {
        let mut islands = Vec::new();
        for seed in SEEDS {
            let mut line = line(seeded(seed), N, -10.0, 10.0, 1.0);
            lay(&mut line, N, FLOOR_M);
            let unscarred =
                (line.volcanoes().iter()).filter(|v| v.get(BREAK) < 0.7 && v.slides() == 0);
            islands.extend(unscarred.copied());
        }
        // The edifice on rings around the summit of each island, above its
        // break. Rays down the slope are waves of many cycles around a ring.
        // The walls of a landslide scar are not rays.
        let count = 720;
        let (mut rays, mut rings) = (0.0, 0);
        for v in &islands {
            let c = v.center();
            let (east, north) = frame(c);
            for i in 0..6 {
                let r = (0.15 + 0.05 * f64::from(i)) * v.base_radius();
                let ring: Vec<f64> = (0..count)
                    .map(|k| {
                        let (s, co) = (TAU * f64::from(k) / f64::from(count)).sin_cos();
                        let p = add(c, add(scale(east, r * co), scale(north, r * s)));
                        v.edifice(normalize(p)) / v.height()
                    })
                    .collect();
                // The amplitude of the waves of 20 to 120 cycles.
                let power: f64 = (20..=120)
                    .map(|m| {
                        let (mut a, mut b) = (0.0, 0.0);
                        for (k, e) in ring.iter().enumerate() {
                            let turn = TAU * f64::from(m) * k as f64 / f64::from(count);
                            a += e * turn.cos();
                            b += e * turn.sin();
                        }
                        (a * a + b * b) * 2.0 / f64::from(count * count)
                    })
                    .sum();
                rays += power.sqrt();
                rings += 1;
            }
        }
        assert!(rings >= 6 * SEEDS.len(), "{rings}");
        let rays = rays / f64::from(rings as u32);
        eprintln!("rays {rays:.4} of the height");
        assert!(rays < 0.0025, "{rays}");
    }

    #[test]
    fn islands_are_broad_and_stand_on_a_ridge() {
        let (mut islands, mut channels) = (0, 0);
        for seed in SEEDS {
            let (i, c) = broad_islands(seed);
            islands += i;
            channels += c;
        }
        eprintln!("{islands} islands, {channels} channels");
        assert!(islands > 3 * SEEDS.len(), "{islands}");
        assert!(channels > SEEDS.len(), "{channels}");
    }

    /// Checks the channels and the slopes of the islands of a chain.
    /// Returns the count of the islands and of the channels.
    fn broad_islands(seed: u32) -> (usize, usize) {
        let mut line = line(seeded(seed), N, -10.0, 10.0, 1.0);
        let (map, _) = lay(&mut line, N, FLOOR_M);
        let chain = line.chain.clone().unwrap();
        // The crest of the chain: the highest ground across the line.
        let crest = |lon: f64| {
            (-30..=30)
                .map(|j| at(&map, lon, degrees(2.0 * f64::from(j))))
                .fold(f64::MIN, f64::max)
        };
        let lon = |v: &Volcano| v.center()[1].atan2(v.center()[0]).to_degrees();
        let mut islands: Vec<f64> = (chain.volcanoes.iter())
            .filter(|v| meters(map.level_at(v.center())) > 0.0)
            .map(lon)
            .collect();
        islands.sort_by(f64::total_cmp);
        // The channels between islands less than 350 km apart are shallow:
        // the floor beside them is deeper than 4,000 m.
        let mut deepest = 0.0f64;
        let mut channels = 0;
        for w in islands.windows(2).filter(|w| w[1] - w[0] < degrees(350.0)) {
            channels += 1;
            let steps = ((w[1] - w[0]) / 0.02).ceil() as usize;
            let low = (0..=steps)
                .map(|k| crest(w[0] + (w[1] - w[0]) * k as f64 / steps as f64))
                .fold(f64::MAX, f64::min);
            deepest = deepest.min(low);
        }
        eprintln!("deepest channel {deepest:.0} m");
        assert!(deepest > -2500.0, "{seed}: {deepest}");
        // Across the largest young island, the flank on land is gentle,
        // and the flank under the sea is steep. Along the chain, the ridge
        // is gentle. A ray toward a near volcano, as in a double line, runs
        // onto its flank, so it does not count.
        let largest = (0..chain.volcanoes.len())
            .filter(|&i| chain.ages[i] < 1.0)
            .max_by(|&a, &b| {
                let size = |i: usize| chain.volcanoes[i].height() * chain.volcanoes[i].growth();
                size(a).total_cmp(&size(b))
            })
            .unwrap();
        let v = &chain.volcanoes[largest];
        let c = v.center();
        let (east, north) = frame(c);
        let (mut land, mut sea) = (Vec::new(), Vec::new());
        let mut rays = 0;
        for turn in (2..=6).chain(10..=14) {
            let (s, co) = (TAU * f64::from(turn) / 16.0).sin_cos();
            let dir = add(scale(east, co), scale(north, s));
            let blocked = chain.volcanoes.iter().any(|w| {
                let d = add(w.center(), scale(c, -1.0));
                let apart = angle(w.center(), c);
                apart > 0.0
                    && apart < 1.5 * v.base_radius()
                    && dot(d, dir) > 40f64.to_radians().cos() * crate::math::length(d)
            });
            if blocked {
                continue;
            }
            let height = |km: f64| meters(map.level_at(normalize(add(c, scale(dir, km / KM)))));
            // The flank of the island ends where the ray first reaches
            // 3,000 m below the sea. A ray that does not reach it within a
            // base radius runs along the ridge of the chain.
            let (mut on_land, mut flank) = (Vec::new(), Vec::new());
            let radius_km = v.base_radius() * KM;
            for k in 0..250 {
                let km = f64::from(k);
                let (h, slope) = (height(km), (height(km) - height(km + 1.0)).abs() / 1000.0);
                if h > 200.0 {
                    on_land.push(slope);
                } else if h < -3000.0 {
                    if km < radius_km {
                        land.append(&mut on_land);
                        sea.append(&mut flank);
                        rays += 1;
                    }
                    break;
                } else if h < -500.0 {
                    flank.push(slope);
                }
            }
        }
        let mean = |d: &[f64]| d.iter().sum::<f64>() / d.len() as f64;
        let (land, sea) = (mean(&land), mean(&sea));
        eprintln!("slopes on {rays} rays: land {land:.3}, sea {sea:.3}");
        assert!(rays >= 4, "{seed}: {rays}");
        assert!(land < 0.1 && sea > 2.0 * land, "{seed}: {land} {sea}");
        (islands.len(), channels)
    }

    #[test]
    fn ground_past_1_5_radii_does_not_change() {
        let spec = spec();
        let mut line = line(spec, N, -10.0, 10.0, 1.0);
        let (map, before) = lay(&mut line, N, FLOOR_M);
        let reach = REACH * line.spec().radius;
        let (mut near, mut far) = (0, 0);
        for face in 0..FACES {
            for y in (0..N).step_by(3) {
                for x in (0..N).step_by(3) {
                    let (xi, yi) = (x as i64, y as i64);
                    if map.get(face, xi, yi) == before.get(face, xi, yi) {
                        continue;
                    }
                    let (d, _) = line.line.nearest(map.texel_dir(face, x, y));
                    if d > reach {
                        far += 1;
                    } else {
                        near += 1;
                    }
                }
            }
        }
        assert!(near > 1000);
        assert_eq!(far, 0);
    }

    #[test]
    fn equal_strokes_give_equal_results() {
        let spec = spec();
        let mut a = line(spec, N, -6.0, 6.0, 0.8);
        let mut b = line(spec, N, -6.0, 6.0, 0.8);
        let a = lay_eroded(&mut a, N, 0.5);
        let b = lay_eroded(&mut b, N, 0.5);
        for face in 0..FACES {
            for y in (0..N).step_by(2) {
                for x in (0..N).step_by(2) {
                    let (x, y) = (x as i64, y as i64);
                    assert_eq!(a.get(face, x, y), b.get(face, x, y));
                }
            }
        }
    }

    /// The distance from the center of a volcano to its coast at the sea of
    /// its growth on each of `count` rays, in base radii.
    fn coast(v: &Volcano, count: usize) -> Vec<f64> {
        let c = v.center();
        let (east, north) = frame(c);
        (0..count)
            .map(|k| {
                let (s, co) = (TAU * k as f64 / count as f64).sin_cos();
                let dir = add(scale(east, co), scale(north, s));
                (1..400)
                    .map(|j| f64::from(j) / 200.0)
                    .filter(|&d| {
                        let p = normalize(add(c, scale(dir, d * v.base_radius())));
                        v.edifice(p) / v.height() > v.get(BREAK)
                    })
                    .fold(0.0, f64::max)
            })
            .collect()
    }

    #[test]
    fn rift_zones_give_the_islands_arms_and_points() {
        let mut islands = Vec::new();
        for seed in SEEDS {
            let mut line = line(seeded(seed), N, -10.0, 10.0, 1.0);
            lay(&mut line, N, FLOOR_M);
            let unscarred =
                (line.volcanoes().iter()).filter(|v| v.get(BREAK) < 0.7 && v.slides() == 0);
            islands.extend(unscarred.copied());
        }
        assert!(islands.len() >= SEEDS.len(), "{}", islands.len());
        for v in &islands {
            assert!((2..=3).contains(&v.rifts()), "{}", v.rifts());
            // The coast reaches far out along the rift zones and stays near
            // the summit between them.
            let coast = coast(v, 72);
            let far = coast.iter().copied().fold(0.0, f64::max);
            let near = coast.iter().copied().fold(f64::MAX, f64::min);
            eprintln!("coast from {near:.2} to {far:.2} base radii");
            assert!(far > 1.8 * near, "{near} {far}");
        }
    }

    #[test]
    fn volcanoes_keep_their_own_summits() {
        let pairs: usize = SEEDS.map(own_summits).sum();
        eprintln!("{pairs} pairs");
        assert!(pairs >= SEEDS.len(), "{pairs}");
    }

    /// Checks that each two joined volcanoes of a chain have a saddle
    /// between their summits. Returns the count of the pairs.
    fn own_summits(seed: u32) -> usize {
        let mut line = line(seeded(seed), N, -10.0, 10.0, 1.0);
        let (map, _) = lay(&mut line, N, FLOOR_M);
        let chain = line.chain.clone().unwrap();
        let height = |p: V3| meters(map.level_at(p));
        // The summit near the center of each volcano on land.
        let summit = |v: &Volcano| {
            let c = v.center();
            let (east, north) = frame(c);
            let r = 0.1 * v.base_radius();
            (0..121)
                .map(|k| {
                    let (x, y) = (f64::from(k % 11) / 5.0 - 1.0, f64::from(k / 11) / 5.0 - 1.0);
                    height(normalize(add(
                        add(c, scale(east, r * x)),
                        scale(north, r * y),
                    )))
                })
                .fold(f64::MIN, f64::max)
        };
        // Two neighbors on one island, each with a summit above 1,000 m.
        // A volcano nearer than 0.6 base radii stands on the flank of the
        // other, as Kilauea stands on Mauna Loa.
        let vs = &chain.volcanoes;
        let mut pairs = 0;
        for (i, a) in vs.iter().enumerate() {
            for b in &vs[i + 1..] {
                let apart = angle(a.center(), b.center());
                let between = vs.iter().any(|w| {
                    angle(w.center(), a.center()) < apart && angle(w.center(), b.center()) < apart
                });
                if between || apart < 0.6 * a.base_radius().max(b.base_radius()) {
                    continue;
                }
                let (sa, sb) = (summit(a), summit(b));
                if sa < 1000.0 || sb < 1000.0 {
                    continue;
                }
                // The saddle: the lowest ground on the way between them.
                let saddle = (0..=100)
                    .map(|k| {
                        let t = f64::from(k) / 100.0;
                        height(normalize(add(
                            scale(a.center(), 1.0 - t),
                            scale(b.center(), t),
                        )))
                    })
                    .fold(f64::MAX, f64::min);
                if saddle <= 0.0 {
                    continue;
                }
                eprintln!("summits {sa:.0} m and {sb:.0} m, saddle {saddle:.0} m");
                assert!(sa.min(sb) - saddle > 300.0, "{seed}: {sa} {sb} {saddle}");
                pairs += 1;
            }
        }
        pairs
    }

    #[test]
    fn landslides_cut_scars_into_old_volcanoes() {
        let mut line = line(spec(), N, -10.0, 10.0, 1.0);
        lay(&mut line, N, FLOOR_M);
        let chain = line.chain.clone().unwrap();
        let (mut young, mut old) = (0, 0);
        for (i, (v, &age)) in chain.volcanoes.iter().zip(&chain.ages).enumerate() {
            if age < CALDERA_MYR {
                // A young volcano has a caldera and no scars.
                assert_eq!(v.slides(), 0);
                assert!(v.crater());
                young += 1;
            } else if age > SLIDE_MYR && age < chain.drowned[i] && v.get(BREAK) < 0.7 {
                assert!((1..=3).contains(&v.slides()), "{}", v.slides());
                assert!(!v.crater());
                // Each scar cuts deep into the volcano out from its head.
                let (c, axis) = (v.center(), [3, 4, 5].map(|i| f64::from(v.0[i])));
                let side = cross(c, axis);
                let bare = v.without_slides();
                for s in 0..v.slides() {
                    let (angle, head) = v.slide(s);
                    let dir = add(scale(axis, angle.cos()), scale(side, angle.sin()));
                    let cut = (0..40)
                        .map(|k| {
                            let d = (head + 0.01 * f64::from(k)) * v.base_radius();
                            let p = normalize(add(c, scale(dir, d)));
                            (bare.edifice(p) - v.edifice(p)) / v.height()
                        })
                        .fold(0.0, f64::max);
                    // An island that erosion lowered near the sea has
                    // less to lose.
                    let rise = 1.0 - v.get(BREAK) - v.lower();
                    assert!(cut > 0.05f64.min(0.5 * rise), "{cut} {rise}");
                }
                old += 1;
            }
        }
        assert!(young > 0 && old > 0, "{young} {old}");
    }

    #[test]
    fn windward_valleys_are_deeper_than_leeward() {
        let n = 2 * N;
        let mut line = line(spec(), n, -6.0, 6.0, 1.0);
        let (plain, _) = lay(&mut line, n, FLOOR_M);
        // The wind comes from the north.
        let rain = Rain {
            wind: 0.0,
            shadow: 0.6,
        };
        let eroded = lay_eroded_in(&mut line, n, 0.5, rain);
        let chain = line.chain.clone().unwrap();
        let (mut north_drop, mut south_drop) = (0.0, 0.0);
        for (v, &age) in chain.volcanoes.iter().zip(&chain.ages) {
            if age < 2.0 {
                continue;
            }
            let c = v.center();
            let (east, north) = frame(c);
            let r = 0.8 * v.base_radius();
            for j in -30..=30 {
                for k in -30..=30 {
                    let (x, y) = (r * f64::from(j) / 30.0, r * f64::from(k) / 30.0);
                    let p = normalize(add(add(c, scale(east, x)), scale(north, y)));
                    let (a, b) = (meters(plain.level_at(p)), meters(eroded.level_at(p)));
                    if a > 0.0 && k > 3 {
                        north_drop += a - b;
                    } else if a > 0.0 && k < -3 {
                        south_drop += a - b;
                    }
                }
            }
        }
        eprintln!("drops: north {north_drop:.0}, south {south_drop:.0}");
        assert!(north_drop > 1.5 * south_drop, "{north_drop} {south_drop}");
    }

    #[test]
    fn land_shrinks_with_age() {
        // The land and the mean of the highest ground of each span of 2
        // million years from the end of the line, over all chains. The
        // supply of one chain changes, so one chain can have more land in an
        // older span.
        let mut land = [0usize; 4];
        let mut top = [0.0; 4];
        for seed in SEEDS {
            let (l, t) = land_by_age(seed);
            for span in 0..4 {
                land[span] += l[span];
                top[span] += t[span] / SEEDS.len() as f64;
            }
        }
        eprintln!("land {land:?}, tops {top:.0?}");
        assert!(land[0] > 2 * land[1] && land[1] > land[3], "{land:?}");
        assert!(top[0] > top[1] && top[1] > top[3], "{top:?}");
    }

    #[test]
    fn every_chain_makes_land_at_the_hotspot() {
        // At the default settings on a floor at -5,448 m, the group at the
        // hotspot is an island.
        let n = N / 4;
        for seed in SEEDS {
            let mut line = line(seeded(seed), n, -10.0, 10.0, 1.0);
            let (map, _) = lay(&mut line, n, FLOOR_M);
            let young = degrees(2.0 * spec().speed_mm);
            let top = (0..=100)
                .flat_map(|i| (-20..=20).map(move |j| (i, j)))
                .map(|(i, j)| {
                    let lon = 10.0 - young * f64::from(i) / 100.0;
                    at(&map, lon, 0.05 * f64::from(j))
                })
                .fold(f64::MIN, f64::max);
            eprintln!("{seed}: top {top:.0} m");
            assert!(top > 1000.0, "{seed}: {top}");
        }
    }

    /// The land and the highest ground of each span of 2 million years of a
    /// chain.
    fn land_by_age(seed: u32) -> ([usize; 4], [f64; 4]) {
        let mut line = line(seeded(seed), N, -10.0, 10.0, 1.0);
        let (map, _) = lay(&mut line, N, FLOOR_M);
        let chain = line.chain.clone().unwrap();
        let mut land = [0usize; 4];
        let mut top = [f64::MIN; 4];
        for i in 0..=800 {
            let lon = -10.0 + 0.025 * f64::from(i);
            let age = (10.0 - lon).to_radians() * KM / spec().speed_mm;
            let span = (age / 2.0) as usize;
            if span >= 4 {
                continue;
            }
            for j in -60..=60 {
                let h = at(&map, lon, 0.025 * f64::from(j));
                if h > 0.0 {
                    land[span] += 1;
                }
                top[span] = top[span].max(h);
            }
        }
        eprintln!("{seed}: land {land:?}, tops {top:.0?}");
        assert!(!chain.volcanoes.is_empty());
        // The young span has the most land.
        assert!(land[1..].iter().all(|&l| l < land[0]), "{seed}: {land:?}");
        (land, top)
    }
}
