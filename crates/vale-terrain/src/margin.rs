//! The passive margin brush.
//!
//! The brush makes the edge of a continent that rifted from another
//! continent: a shallow shelf, a steeper slope, and a gentle rise at the
//! foot of the slope. It is a line brush, as the Ocean ridge brush is. The
//! stroke is the shelf break, and each texel near the line gets a place on
//! it. The profile across the line is a function of that place, of the
//! level of the texel from before the stroke, and of a seed.

use std::f64::consts::FRAC_PI_2;
use std::ops::Deref;

use crate::cube::{ELEV_MAX, ELEV_MIN};
use crate::heightmap::{Heightmap, Limits, hash, noise3};
use crate::line::{LinePlan, LinePoint, REACH, StrokeLine, TAPER, bump, noise1, smoothstep, unit};
use crate::math::{V3, add, cross, normalize, scale};

/// The largest brush radius of a margin, in radians.
pub const MAX_MARGIN_RADIUS: f64 = 0.24;

/// The limits of the depth of the shelf break, in meters.
pub const SHELF_DEPTH_RANGE: (f64, f64) = (-1000.0, -20.0);

/// The mean angle of the slope, in degrees (Pratson and Haxby 1996).
pub(crate) const SLOPE_DEG: f64 = 4.0;

/// The least and the largest angle of the slope, in degrees.
pub(crate) const SLOPE_RANGE_DEG: (f64, f64) = (1.0, 10.0);

/// The length of a cell of the noise of the slope angle along the line, in
/// brush radii.
const ANGLE_CELL: f64 = 0.25;

/// The distance from the shelf break to the coast, in brush radii.
const COAST: f64 = 0.35;

/// The distances from the shelf break, in brush radii, where the land from
/// before the stroke starts to show and where it shows in full.
const LAND_BLEND: (f64, f64) = (0.28, 0.6);

/// The distance from the line, in brush radii, where the margin starts to
/// fade into the ground from before the stroke. It is gone at `REACH`.
const EDGE: f64 = 1.0;

/// The distance from the shelf break, in brush radii, where the rise ends at
/// the latest.
const RISE_END: f64 = 1.1;

/// The part of the depth below the shelf break that the rise takes, if the
/// brush is wide enough.
const RISE_PART: f64 = 0.55;

/// The least and the largest width of the rise for each meter of its
/// height, in kilometers. The rise is a parabola, so its gradient is
/// between 1:100 and 1:700 over its middle.
const RISE_WIDTH: (f64, f64) = (0.2, 0.42);

/// The distance from the line, in brush radii, where `MarginLine::read_floor`
/// reads the depth of the floor.
const FLOOR_REACH: f64 = 1.4;

/// The least drop from the shelf break to the floor, in meters.
const LEAST_DROP: f64 = 200.0;

/// The rounding of the shelf break, in meters.
const BREAK_ROUND: f64 = 30.0;

/// The size of the features of the wander of the shelf break and its
/// largest distance, in brush radii, for each of two octaves.
const WANDER: [(f64, f64); 2] = [(0.7, 0.18), (0.22, 0.06)];

/// The spacing of the gullies on the slope, in kilometers, and the least
/// spacing, in texels.
const GULLY_KM: f64 = 3.0;
const GULLY_TEXELS: f64 = 4.0;

/// The depth of a gully on a slope of `SLOPE_DEG`, in meters.
const GULLY_DEPTH: f64 = 110.0;

/// The length of a cell of the slide scars along the line, in kilometers,
/// and the least length, in gully spacings.
const SCAR_KM: f64 = 70.0;
const SCAR_GULLIES: f64 = 12.0;

/// The part of the cells with a slide scar.
const SCAR_CHANCE: f64 = 0.55;

/// The largest depth of a slide scar, in meters.
const SCAR_DEPTH: f64 = 260.0;

/// The height of the debris below a scar, as a part of the depth of the
/// scar.
const DEBRIS_PART: f64 = 0.45;

/// The length of a cell of the canyons along the line, in kilometers, and
/// the least length, in gully spacings.
const CANYON_KM: f64 = 28.0;
const CANYON_GULLIES: f64 = 6.0;

/// The part of the cells with a canyon.
const CANYON_CHANCE: f64 = 0.65;

/// The largest depth of a canyon on the slope, in meters.
const CANYON_DEPTH: f64 = 300.0;

/// The sizes of the features of the fine relief, in kilometers, and their
/// heights on the rise, in meters.
const RELIEF: [(f64, f64); 4] = [(40.0, 40.0), (16.0, 50.0), (7.0, 50.0), (3.5, 40.0)];

const SALT_CANYON: u32 = 0x6361_6e79;
const SALT_RELIEF: u32 = 0x7265_6c66;
const SALT_WANDER: u32 = 0x6d61_7267;
const SALT_ANGLE: u32 = 0x736c_6f70;
const SALT_GULLY: u32 = 0x6775_6c6c;
const SALT_SCAR: u32 = 0x7363_6172;

/// The settings of a margin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Margin {
    /// The brush radius, in radians.
    pub radius: f64,
    /// The depth of the shelf break, in meters.
    pub shelf_m: f64,
    /// Whether the land is on the left side of the line, in the direction
    /// of the stroke.
    pub land_left: bool,
    /// The depth of the deep floor at the foot of the rise, in meters. It
    /// is at least `LEAST_DROP` below the shelf break. `MarginLine::read_floor`
    /// reads it from the ground.
    pub floor_m: f64,
    /// The radius of the world, in kilometers.
    pub world_km: f64,
    pub seed: u32,
    pub limits: Limits,
}

/// The line of a margin.
#[derive(Clone, Debug)]
pub struct MarginLine {
    spec: Margin,
    line: StrokeLine,
    points: Vec<V3>,
}

/// The texels that new points of a margin change, and the segments of the
/// line that give the levels there.
#[derive(Clone, Debug)]
pub struct MarginPlan {
    /// The settings, inside their limits.
    pub spec: Margin,
    /// The width of a texel at the center of a face, in kilometers.
    pub texel_km: f64,
    pub line: LinePlan,
}

impl Deref for MarginPlan {
    type Target = LinePlan;

    fn deref(&self) -> &LinePlan {
        &self.line
    }
}

impl MarginLine {
    /// Starts a line with no points on a heightmap with `face_size` texels
    /// along one side of a face. The settings go inside their limits.
    pub fn new(spec: Margin, face_size: usize) -> MarginLine {
        let step = FRAC_PI_2 / face_size as f64;
        let radius = spec.radius.max(4.0 * step).min(MAX_MARGIN_RADIUS);
        let shelf_m = spec.shelf_m.clamp(SHELF_DEPTH_RANGE.0, SHELF_DEPTH_RANGE.1);
        let spec = Margin {
            radius,
            shelf_m,
            floor_m: spec.floor_m.min(shelf_m - LEAST_DROP),
            world_km: spec.world_km.max(1.0),
            ..spec
        };
        MarginLine {
            spec,
            line: StrokeLine::new(radius, face_size),
            points: Vec::new(),
        }
    }

    /// Sets the depth of the deep floor from the ground of `map`: the mean
    /// depth at `FLOOR_REACH` brush radii from the points on the side of the
    /// sea. Call it after the last point and before the first plan, with the
    /// ground from before the stroke. With fewer than two points, the depth
    /// stays.
    pub fn read_floor(&mut self, map: &Heightmap) {
        let n = self.points.len();
        if n < 2 {
            return;
        }
        let (sin, cos) = (FLOOR_REACH * self.spec.radius).sin_cos();
        let sea = if self.spec.land_left { -1.0 } else { 1.0 };
        let sum: f64 = (0..n)
            .map(|i| {
                let (a, b) = (
                    self.points[i.saturating_sub(1)],
                    self.points[(i + 1).min(n - 1)],
                );
                let left = normalize(cross(a, b));
                let at = normalize(add(scale(self.points[i], cos), scale(left, sea * sin)));
                to_meters(map.level_between(at, true))
            })
            .sum();
        self.spec.floor_m = (sum / n as f64).min(self.spec.shelf_m - LEAST_DROP);
    }

    /// The settings, inside their limits.
    pub fn spec(&self) -> Margin {
        self.spec
    }

    /// Adds a point with a flow from 0 to 1. A point closer to the last
    /// point than a quarter of a texel step is not added.
    pub fn push(&mut self, dir: V3, flow: f64) {
        let before = self.line.len();
        self.line.push(dir, flow);
        if self.line.len() > before {
            self.points.push(dir);
        }
    }

    /// The number of points.
    pub fn len(&self) -> usize {
        self.line.len()
    }

    pub fn is_empty(&self) -> bool {
        self.line.is_empty()
    }

    /// The plan for the points from `from` on, as `StrokeLine::plan`.
    pub fn plan(&self, from: usize) -> MarginPlan {
        let line = self.line.plan(from);
        MarginPlan {
            spec: self.spec,
            texel_km: FRAC_PI_2 / line.face_size as f64 * self.spec.world_km,
            line,
        }
    }

    /// As `StrokeLine::margin`.
    #[doc(hidden)]
    pub fn margin(&self, p: V3) -> f64 {
        self.line.margin(p)
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

fn to_meters(level: f64) -> f64 {
    ELEV_MIN + level / 65535.0 * (ELEV_MAX - ELEV_MIN)
}

fn to_level(m: f64) -> f64 {
    (m - ELEV_MIN) / (ELEV_MAX - ELEV_MIN) * 65535.0
}

impl MarginPlan {
    /// Whether the plan changes nothing.
    pub fn is_empty(&self) -> bool {
        self.line.rects.iter().all(Option::is_none)
    }

    /// The distance that the shelf break moves to the land at a unit
    /// direction, in kilometers.
    pub(crate) fn wander(&self, p: V3) -> f64 {
        let r = self.spec.radius;
        let km = self.spec.world_km;
        WANDER
            .iter()
            .enumerate()
            .map(|(k, &(size, amount))| {
                let n = noise3(scale(p, 1.0 / (size * r)), self.spec.seed ^ SALT_WANDER);
                n[k] * amount * r * km
            })
            .sum()
    }

    /// The slope angle at a distance along the line in kilometers, as a
    /// tangent.
    pub(crate) fn slope(&self, along: f64) -> f64 {
        let cell = ANGLE_CELL * self.spec.radius * self.spec.world_km;
        let (n, _) = noise1(along / cell, 0, 0, self.spec.seed ^ SALT_ANGLE);
        let deg = (SLOPE_DEG * 4f64.powf(n)).clamp(SLOPE_RANGE_DEG.0, SLOPE_RANGE_DEG.1);
        deg.to_radians().tan()
    }

    /// The spacing of the gullies, in kilometers.
    fn gully_km(&self) -> f64 {
        GULLY_KM.max(GULLY_TEXELS * self.texel_km)
    }

    /// The depth of the gullies under a place, from 0 to 1. `along` is the
    /// distance along the line and `down` the distance down the slope, in
    /// kilometers.
    pub(crate) fn gullies(&self, along: f64, down: f64) -> f64 {
        let spacing = self.gully_km();
        let seed = self.spec.seed ^ SALT_GULLY;
        let (bend, _) = noise1(down / (4.0 * spacing), 1, 0, seed);
        let (n, _) = noise1(along / spacing + 0.6 * bend, 0, 0, seed);
        let v = 1.0 - n.abs();
        // Each gully deepens and fades down the slope, so the gullies are
        // pieces with gaps.
        let piece = noise3([along / spacing, down / (2.5 * spacing), 0.0], seed)[0];
        v * v * v * smoothstep(-0.3, 0.5, piece)
    }

    /// The depth of the canyons, in meters. The arguments are those of
    /// `scars`. A canyon starts a little inside the shelf break, cuts the
    /// slope, and goes on across the rise as a channel that gets shallower.
    pub(crate) fn canyons(&self, along: f64, down: f64, slope: f64, rise: f64) -> f64 {
        let spacing = self.gully_km();
        let cell = CANYON_KM.max(CANYON_GULLIES * spacing);
        let seed = self.spec.seed ^ SALT_CANYON;
        let (bend, _) = noise1(down / (2.0 * cell), 0, 0, seed);
        let along = along + 0.3 * cell * bend;
        let k = (along / cell).floor();
        let mut depth = 0.0f64;
        for c in [k - 1.0, k, k + 1.0] {
            let c32 = c as i32 as u32;
            if unit(hash(c32, 0, 0, seed)) >= CANYON_CHANCE {
                continue;
            }
            let center = (c + 0.25 + 0.5 * unit(hash(c32, 1, 0, seed))) * cell;
            let half = (0.05 + 0.05 * unit(hash(c32, 2, 0, seed))) * cell + self.texel_km;
            let size = 0.4 + 0.6 * unit(hash(c32, 3, 0, seed));
            let head = -0.1 * cell * unit(hash(c32, 4, 0, seed));
            let side = ((along - center) / half).abs();
            if side >= 1.0 {
                continue;
            }
            let start = smoothstep(head, head + 0.3 * slope + self.texel_km, down);
            let channel = (-(down - slope).max(0.0) / (0.12 * rise + self.texel_km)).exp();
            let v = 1.0 - side;
            depth = depth.max(CANYON_DEPTH * size * v * v.sqrt() * start * channel);
        }
        depth
    }

    /// The fine relief at a unit direction, in meters, at a height of 1 on
    /// the rise.
    pub(crate) fn relief(&self, p: V3) -> f64 {
        let km = self.spec.world_km;
        let least = 2.0 * self.texel_km;
        RELIEF
            .iter()
            .enumerate()
            .map(|(k, &(size, height))| {
                let fade = smoothstep(least, 2.0 * least, size);
                let seed = self.spec.seed ^ SALT_RELIEF.wrapping_add(k as u32);
                noise3(scale(p, km / size), seed)[0] * height * fade
            })
            .sum()
    }

    /// The change of the slide scars and their debris, in meters. `along`
    /// is the distance along the line and `down` the distance below the
    /// shelf break, in kilometers. `slope` and `rise` are the widths of the
    /// slope and the rise, in kilometers.
    pub(crate) fn scars(&self, along: f64, down: f64, slope: f64, rise: f64) -> f64 {
        let cell = SCAR_KM.max(SCAR_GULLIES * self.gully_km());
        let seed = self.spec.seed ^ SALT_SCAR;
        let k = (along / cell).floor();
        let mut change = 0.0;
        for c in [k - 1.0, k, k + 1.0] {
            let c32 = c as i32 as u32;
            if unit(hash(c32, 0, 0, seed)) >= SCAR_CHANCE {
                continue;
            }
            let center = (c + 0.2 + 0.6 * unit(hash(c32, 1, 0, seed))) * cell;
            let half = cell * (0.06 + 0.14 * unit(hash(c32, 2, 0, seed)));
            let depth = SCAR_DEPTH * (0.3 + 0.7 * unit(hash(c32, 3, 0, seed)));
            let head = slope * (0.05 + 0.35 * unit(hash(c32, 4, 0, seed)));
            let side = (along - center) / half;
            // The scar: a steep headwall, and a floor that runs out at the
            // foot of the slope.
            let cut = smoothstep(head, head + 0.08 * slope + 0.5 * self.texel_km, down)
                * (1.0 - smoothstep(0.75 * slope, slope + 0.2 * rise, down));
            change -= depth * bump(side) * cut;
            // The debris: a lobe on the rise below the scar.
            let length = 0.15 * rise + 2.0 * half;
            let mid = slope + 0.5 * length;
            let lobe = bump((down - mid) / (0.5 * length + self.texel_km));
            change += DEBRIS_PART * depth * bump(side / 1.3) * lobe;
        }
        change
    }

    /// The level of the margin at a place on the line, in meters, with the
    /// level `old` from before the stroke, in meters, at the unit direction
    /// `p`.
    pub(crate) fn profile(&self, at: LinePoint, p: V3, old: f64) -> f64 {
        self.profile_of(at, p, old, true)
    }

    /// The level of `profile`, with the gullies, the scars, the canyons,
    /// and the fine relief if `detail`.
    fn profile_of(&self, at: LinePoint, p: V3, old: f64, detail: bool) -> f64 {
        let spec = &self.spec;
        let km = spec.world_km;
        let r = spec.radius * km;
        let side = if spec.land_left { 1.0 } else { -1.0 };
        let along = at.u * km;
        // The distance from the shelf break to the land.
        let x = side * at.across * km - self.wander(p);
        let b = spec.shelf_m;
        let tan = self.slope(along);
        // The shelf rises from the break to the coast.
        let shelf = b - b / (COAST * r) * x;
        let upper = smooth_min(shelf, b + 1000.0 * tan * x, BREAK_ROUND);
        // The floor below the margin, and the rise above it. The rise is as
        // high as the brush gives room for.
        let drop = b - spec.floor_m;
        let per_m = 1.0 / (1000.0 * tan);
        let room = RISE_END * r - drop * per_m;
        let height = (RISE_PART * drop)
            .min(room / (RISE_WIDTH.0 - per_m))
            .max(0.0);
        let slope = (drop - height) * per_m;
        let rise = (RISE_END * r - slope).clamp(RISE_WIDTH.0 * height, RISE_WIDTH.1 * height);
        let down = -x;
        let t = ((down - slope) / rise.max(1e-9)).clamp(0.0, 1.0);
        // The rise lies on the floor from before the stroke, where that floor
        // is deeper than the floor of the line, so it joins the floor.
        let apron = old.min(spec.floor_m) + height * (1.0 - t) * (1.0 - t);
        let mut level = smooth_max(upper, apron, 0.25 * height + 1.0);
        // The land from before the stroke comes back past the coast.
        let land = smoothstep(LAND_BLEND.0 * r, LAND_BLEND.1 * r, x);
        if !detail {
            return level + (old - level) * land;
        }
        // The details fade out before the end of the rise, so the floor past
        // it keeps its level.
        let fade = 1.0 - smoothstep(0.6, 1.0, t);
        let base = level;
        // The gullies cut the slope, from below the break to the upper rise.
        let on_slope = smoothstep(0.0, 0.15 * slope, down)
            * (1.0 - smoothstep(0.7 * slope, slope + 0.15 * rise, down));
        let deep = GULLY_DEPTH * (tan / SLOPE_DEG.to_radians().tan()).sqrt();
        level -= deep * on_slope * self.gullies(along, down);
        level += self.scars(along, down, slope, rise);
        level -= self.canyons(along, down, slope, rise);
        // The fine relief is low on the shelf and high on the rise.
        let rough = 0.3 + 0.7 * smoothstep(0.0, slope, down);
        level += rough * self.relief(p);
        level = base + (level - base) * fade;
        level + (old - level) * land
    }

    /// The new level of a texel with the level `start` from before the
    /// stroke, at a place on the line and its unit direction `p`.
    pub(crate) fn level(&self, start: u16, at: LinePoint, p: V3) -> u16 {
        let r = self.spec.radius;
        let taper = smoothstep(0.0, TAPER * r, at.end);
        let edge = 1.0 - smoothstep(EDGE * r, REACH * r, at.across.hypot(at.past));
        let weight = at.flow * taper * edge;
        let old = f64::from(start);
        if weight <= 0.0 {
            return start;
        }
        let target = to_level(self.profile(at, p, to_meters(old)));
        let change = (target - old) * weight;
        let new = (old + (change + 0.5).floor()).clamp(0.0, 65535.0);
        self.spec.limits.apply(old, new) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::{FACES, meters_to_level};
    use crate::math::{angle, lonlat_to_dir};

    const KM: f64 = 6371.0;

    fn spec(radius: f64) -> Margin {
        Margin {
            radius,
            shelf_m: -130.0,
            land_left: true,
            floor_m: -5000.0,
            world_km: KM,
            seed: 7,
            limits: Limits::default(),
        }
    }

    fn plan_of(spec: Margin) -> MarginPlan {
        let mut line = MarginLine::new(spec, 4096);
        for k in 0..=40 {
            line.push(lonlat_to_dir(0.0, -10.0 + k as f64 * 0.5), 1.0);
        }
        line.plan(0)
    }

    fn at(u: f64, across: f64) -> LinePoint {
        LinePoint {
            u,
            across,
            past: 0.0,
            flow: 1.0,
            end: 1.0,
        }
    }

    /// The profile across the line at a distance along it, in meters, at
    /// the place `x` km from the line toward the land, on a floor of
    /// `old` meters, with no wander.
    fn across(plan: &MarginPlan, u: f64, x: f64, old: f64) -> f64 {
        // A direction on the line where the wander is near zero would be
        // hard to find, so the test takes the wander out of `x`.
        let p = lonlat_to_dir(0.0, u.to_degrees());
        let w = plan.wander(p);
        plan.profile_of(at(u, (x + w) / KM), p, old, false)
    }

    #[test]
    fn the_shelf_goes_down_gently_to_the_shelf_break() {
        let plan = plan_of(spec(0.03));
        let r = 0.03 * KM;
        for u in [0.0, 0.05, 0.1] {
            let at_break = across(&plan, u, 0.0, 0.0);
            assert!((at_break + 130.0).abs() < 25.0, "{at_break}");
            let mut last = at_break;
            for k in 1..=10 {
                let x = k as f64 * 0.04 * r;
                let h = across(&plan, u, x, 0.0);
                let grade = (h - last) / (0.04 * r * 1000.0);
                assert!(h >= last - 1.0 && grade < 0.003, "{x} {h} {last}");
                last = h;
            }
            // The coast is near 0.6 brush radii.
            let coast = across(&plan, u, COAST * r, 0.0);
            assert!(coast.abs() < 30.0, "{coast}");
        }
    }

    #[test]
    fn the_shelf_depth_sets_the_depth_of_the_break() {
        let plan = plan_of(Margin {
            shelf_m: -400.0,
            ..spec(0.03)
        });
        let h = across(&plan, 0.02, 0.0, -5000.0);
        assert!((h + 400.0).abs() < 25.0, "{h}");
        let plan = plan_of(Margin {
            shelf_m: -5.0,
            ..spec(0.03)
        });
        assert_eq!(plan.spec.shelf_m, SHELF_DEPTH_RANGE.1);
    }

    /// The steepest grade of the slope below the break, in degrees, with no
    /// gullies or scars.
    fn steepest(plan: &MarginPlan, u: f64) -> f64 {
        let along = u * KM;
        plan.slope(along).atan().to_degrees()
    }

    #[test]
    fn the_slope_falls_at_about_4_degrees_with_variation_from_1_to_10() {
        let plan = plan_of(spec(0.03));
        let angles: Vec<f64> = (0..2000)
            .map(|k| steepest(&plan, k as f64 * 1e-3))
            .collect();
        let lo = angles.iter().copied().fold(f64::MAX, f64::min);
        let hi = angles.iter().copied().fold(0.0, f64::max);
        let mut sorted = angles.clone();
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() / 2];
        assert!(lo >= 1.0 && hi <= 10.0, "{lo} {hi}");
        assert!(lo < 1.8 && hi > 8.0, "{lo} {hi}");
        assert!((3.0..5.5).contains(&median), "{median}");
        // The profile has that slope below the break.
        for u in [0.01, 0.2, 0.4] {
            let deg = steepest(&plan, u);
            let tan = deg.to_radians().tan();
            let (x0, x1) = (-2.0, -6.0);
            let drop = across(&plan, u, x0, -5000.0) - across(&plan, u, x1, -5000.0);
            let measured = (drop / ((x0 - x1) * 1000.0)).atan().to_degrees();
            assert!(
                (measured - deg).abs() < 0.05 * deg,
                "{u}: {measured} {deg} {tan}"
            );
        }
    }

    #[test]
    fn the_rise_has_a_gentle_gradient_and_meets_the_floor_with_no_step() {
        let plan = plan_of(spec(0.06));
        let r = 0.06 * KM;
        for u in [0.0, 0.1, 0.3] {
            let profile: Vec<(f64, f64)> = (0..=300)
                .map(|k| {
                    let x = -(k as f64) * 1.5 * r / 300.0;
                    (x, across(&plan, u, x, -5000.0))
                })
                .collect();
            // The foot of the slope is where the grade falls below 1:40.
            let grade = |k: usize| {
                (profile[k].1 - profile[k + 1].1) / ((profile[k].0 - profile[k + 1].0) * 1000.0)
            };
            let foot = (5..299).find(|&k| grade(k) < 1.0 / 40.0).expect("a foot");
            let end = (foot..299)
                .find(|&k| profile[k].1 < -5000.0 + 1.0)
                .expect("the rise ends");
            let width = end - foot;
            assert!(width > 20, "{width}");
            // The middle of the rise is between 1:100 and 1:700.
            for k in foot + width / 6..foot + width * 2 / 3 {
                let g = grade(k);
                assert!((1.0 / 700.0..1.0 / 100.0).contains(&g), "{u} {k} {g}");
            }
            // No step at the floor.
            for &(_, h) in &profile[end..] {
                assert!((h + 5000.0).abs() < 2.0, "{h}");
            }
        }
    }

    #[test]
    fn the_margin_joins_the_land_and_the_deep_floor() {
        let plan = plan_of(spec(0.03));
        let r = 0.03 * KM;
        // The land from before the stroke shows past the coast.
        let land = across(&plan, 0.1, 1.05 * r, 300.0);
        assert!((land - 300.0).abs() < 1.0, "{land}");
        // The floor from before the stroke shows past the rise.
        let floor = across(&plan, 0.1, -1.45 * r, -5600.0);
        assert!((floor + 5600.0).abs() < 2.0, "{floor}");
    }

    #[test]
    fn the_side_selects_the_side_of_the_land() {
        let left = plan_of(spec(0.03));
        let right = plan_of(Margin {
            land_left: false,
            ..spec(0.03)
        });
        let p = lonlat_to_dir(0.0, 0.0);
        let d = 0.5 * 0.03;
        let a = left.profile_of(at(0.0, d), p, 0.0, true);
        let b = right.profile_of(at(0.0, -d), p, 0.0, true);
        assert!(a > -130.0 && a == b, "{a} {b}");
        assert!(left.profile_of(at(0.0, -d), p, -5000.0, true) < -1000.0);
    }

    #[test]
    fn the_shelf_break_wanders_around_the_stroke() {
        let plan = plan_of(spec(0.03));
        let r = 0.03 * KM;
        let w: Vec<f64> = (0..400)
            .map(|k| plan.wander(lonlat_to_dir(0.0, k as f64 * 0.05)))
            .collect();
        let lo = w.iter().copied().fold(f64::MAX, f64::min);
        let hi = w.iter().copied().fold(f64::MIN, f64::max);
        assert!(lo < -0.08 * r && hi > 0.08 * r, "{lo} {hi}");
        assert!(lo > -0.25 * r && hi < 0.25 * r, "{lo} {hi}");
    }

    #[test]
    fn the_slope_has_gullies_and_slide_scars_with_debris() {
        let plan = plan_of(spec(0.03));
        let spacing = plan.gully_km();
        // Along the middle of the slope, the gullies come about once in each
        // spacing, with gaps between their pieces.
        let samples: Vec<f64> = (0..4000)
            .map(|k| plan.gullies(k as f64 * 0.25 * spacing / 4.0, 10.0))
            .collect();
        let crossings = samples
            .windows(2)
            .filter(|w| (w[0] - 0.5) * (w[1] - 0.5) < 0.0)
            .count();
        let gullies = crossings / 2;
        let expected = 4000 / 16;
        assert!(
            gullies > expected / 6 && gullies < expected * 2,
            "{gullies}"
        );
        // The scars cut the slope, and debris lies below them.
        let (slope, rise) = (40.0, 200.0);
        let mut cut = 0.0f64;
        let mut debris = 0.0f64;
        for k in 0..20_000 {
            let along = k as f64 * 0.5;
            cut = cut.min(plan.scars(along, 0.6 * slope, slope, rise));
            debris = debris.max(plan.scars(along, slope + 0.1 * rise, slope, rise));
        }
        assert!(
            cut < -0.3 * SCAR_DEPTH && debris > 0.1 * SCAR_DEPTH,
            "{cut} {debris}"
        );
        // No scar reaches the shelf or the far rise.
        for k in 0..20_000 {
            let along = k as f64 * 0.5;
            assert_eq!(plan.scars(along, -5.0, slope, rise), 0.0);
            assert_eq!(plan.scars(along, slope + rise, slope, rise), 0.0);
        }
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

    fn painted(seed: u32) -> (Heightmap, Vec<u16>) {
        let n = 256;
        let mut map = Heightmap::new(n, 0);
        for face in 0..6 {
            for j in 0..n {
                for i in 0..n {
                    let d = map.texel_dir(face, i, j);
                    // Land to the west of the meridian, deep floor to the east.
                    let east = d[1];
                    let m = if east < 0.0 { 200.0 } else { -4500.0 };
                    map.set(face, i, j, meters_to_level(m));
                }
            }
        }
        let before = levels(&map);
        let mut line = MarginLine::new(
            Margin {
                seed,
                world_km: 6371.0,
                ..spec(0.12)
            },
            n,
        );
        for k in 0..=24 {
            line.push(lonlat_to_dir(0.0, -12.0 + k as f64), 1.0);
        }
        line.read_floor(&map);
        assert!((line.spec().floor_m + 4500.0).abs() < 1.0);
        map.begin_stroke();
        let mut from = 0;
        while from < line.len() {
            let plan = line.plan(from);
            map.margin(&plan);
            from = plan.end;
        }
        map.end_stroke();
        (map, before)
    }

    #[test]
    fn the_ground_far_from_the_line_does_not_change() {
        let (map, before) = painted(3);
        let n = map.face_size();
        let after = levels(&map);
        let first = lonlat_to_dir(0.0, -12.0);
        let last = lonlat_to_dir(0.0, 12.0);
        let mut changed = 0;
        for face in 0..FACES {
            for j in 0..n {
                for i in 0..n {
                    let k = (face * n + j) * n + i;
                    if after[k] == before[k] {
                        continue;
                    }
                    changed += 1;
                    let d = map.texel_dir(face, i, j);
                    // The distance to the line, a meridian arc.
                    let lat = d[2].asin();
                    let near = if lat.abs() <= 12f64.to_radians() {
                        d[1].abs().asin()
                    } else {
                        angle(d, first).min(angle(d, last))
                    };
                    assert!(near < 1.5 * 0.12 + 0.01, "{near}");
                }
            }
        }
        assert!(changed > 1000, "{changed}");
    }

    #[test]
    fn two_equal_strokes_with_the_same_seed_give_equal_results() {
        let (a, _) = painted(3);
        let (b, _) = painted(3);
        let (c, _) = painted(4);
        assert_eq!(levels(&a), levels(&b));
        assert_ne!(levels(&a), levels(&c));
    }

    #[test]
    fn one_undo_step_puts_back_the_ground_from_before_a_margin() {
        let (mut map, before) = painted(3);
        assert!(map.can_undo());
        map.undo();
        assert_eq!(levels(&map), before);
        assert!(!map.can_undo());
    }
}
