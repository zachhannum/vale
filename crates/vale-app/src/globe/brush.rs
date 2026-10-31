//! The brush: settings, and the stamps that a stroke makes from pen samples.

use std::collections::VecDeque;
use std::f64::consts::FRAC_PI_2;
use std::time::Instant;

use vale_terrain::{
    ELEV_MAX, ELEV_MIN, GROUP_STAMPS, LINE_SPACING, Limits, MAX_BRUSH_RADIUS, Mode, RangeSpec,
    Ridge, Stamp, StampPlan, meters_to_level, talus_levels,
};

use super::math::{V3, add, angle, arc_motion, normalize, scale};

/// The distance between stamps, as a part of the brush radius.
pub const STAMP_SPACING: f64 = 0.12;
/// The flow of a mouse, which has no pressure.
pub const FIXED_FLOW: f64 = 0.5;
/// The multiplier from pen force to flow.
pub const PRESSURE_GAIN: f64 = 2.0;
/// The limits of the brush radius on screen, in points.
/// The limits of the flow setting.
pub const FLOW: std::ops::RangeInclusive<f64> = 0.01..=1.0;
/// The limits of the strength setting, in meters.
pub const STRENGTH_M: std::ops::RangeInclusive<f64> = 50.0..=6000.0;
/// The limits of the angle of the weather mode, in degrees. Painted land on
/// a world of the size of the Earth has slopes of 3 degrees or less.
pub const TALUS_DEG: std::ops::RangeInclusive<f64> = 0.02..=20.0;
/// The limits of the strength of the warp mode, in kilometers.
pub const WARP_KM: std::ops::RangeInclusive<f64> = 1.0..=500.0;
/// The limits of the scale of the warp mode, in kilometers.
pub const SCALE_KM: std::ops::RangeInclusive<f64> = 5.0..=2000.0;
/// The limits of the seed slider of the warp mode.
pub const SEED: std::ops::RangeInclusive<u32> = 0..=999;
/// The limits of the crest of the ocean ridge mode, in meters.
pub const RIDGE_CREST_M: std::ops::RangeInclusive<f64> = -6000.0..=6000.0;
/// The limits of the full spreading rate of the ocean ridge mode, in
/// millimeters per year.
pub const RIDGE_SPREAD_MM: std::ops::RangeInclusive<f64> = 10.0..=150.0;
/// The limits of the magma setting of the ocean ridge mode.
pub const RIDGE_MAGMA: std::ops::RangeInclusive<f64> = -0.25..=0.15;
/// The limits of the segment length of the ocean ridge mode, in kilometers.
pub const RIDGE_SEGMENT_KM: std::ops::RangeInclusive<f64> = 100.0..=1500.0;
/// The limits of the offset of the ocean ridge mode, in kilometers.
pub const RIDGE_OFFSET_KM: std::ops::RangeInclusive<f64> = 0.0..=500.0;
/// The limits of the ridge spacing of the range mode, in kilometers.
pub const RANGE_SPACING_KM: std::ops::RangeInclusive<f64> = 5.0..=300.0;
/// The part that the erosion setting of 1 adds to the rise of an uplift
/// stamp. With it, an uplift stroke gives a range of about the height of a
/// raise stroke at each erosion setting.
pub const UPLIFT_GAIN: f64 = 1.0;
/// The limits of the erosion setting of the uplift mode.
pub const EROSION: std::ops::RangeInclusive<f64> = 0.0..=1.0;
pub const SIZE_POINTS: std::ops::RangeInclusive<f32> = 4.0..=160.0;
/// The most passes that the stamps of one frame cost. A smooth stamp costs
/// one pass. A stamp of another mode costs one part in `GROUP_STAMPS` of a
/// pass, because the GPU applies a group of them in one pass.
pub const FRAME_PASSES: usize = 48;
/// The most texels that the stamps of one frame cover.
pub const FRAME_TEXELS: u64 = 40_000_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
    pub mode: Mode,
    /// The radius on screen, in points.
    pub size_points: f32,
    /// The locked radius on the sphere, in radians. `None`: the radius
    /// follows the zoom, and `size_points` sets it.
    pub lock: Option<f64>,
    pub hardness: f64,
    /// The part of the input flow that the brush applies, from 0 to 1.
    pub flow: f64,
    /// The height that one pass adds at full pressure, in meters, roughly.
    pub strength_m: f64,
    /// The target of the flatten mode. `None`: the level under the start of
    /// the stroke.
    pub flatten_level: Option<u16>,
    /// The steepest slope that the weather mode leaves, in degrees.
    pub talus_deg: f64,
    /// The strength of the erosion of the uplift mode, from 0 to 1. At 0,
    /// the mode gives the result of the raise mode.
    pub erosion: f64,
    /// The largest distance that one pass of the warp mode moves the ground,
    /// in kilometers.
    pub warp_km: f64,
    /// The size of the features of the noise of the warp mode, in
    /// kilometers.
    pub scale_km: f64,
    /// The elevation of the crest of the ocean ridge mode, in meters.
    pub ridge_crest_m: f64,
    /// The mean length of a segment of the crest, in kilometers.
    pub ridge_segment_km: f64,
    /// The largest shift of a segment across the line, in kilometers.
    pub ridge_offset_km: f64,
    /// The full spreading rate of the ocean ridge mode, in millimeters per
    /// year.
    pub ridge_spread_mm: f64,
    /// The change of the magma part of the spreading of the ocean ridge
    /// mode.
    pub ridge_magma: f64,
    /// The mean distance between two ridges of the range mode, in
    /// kilometers.
    pub range_spacing_km: f64,
    /// The brush does not move ground across the floor.
    pub floor_on: bool,
    /// The value of the floor, in the unit of the layer. For a height, it
    /// is an elevation in meters.
    pub floor_m: f64,
    /// The brush does not move ground across the ceiling.
    pub ceiling_on: bool,
    /// The value of the ceiling, in the unit of the layer.
    pub ceiling_m: f64,
    /// The values of the lowest and of the highest level of the layer that
    /// the brush paints. The globe sets them.
    pub value_range: (f64, f64),
}

impl Default for BrushSettings {
    fn default() -> BrushSettings {
        BrushSettings {
            mode: Mode::Raise,
            size_points: 36.0,
            lock: None,
            hardness: 0.3,
            flow: 1.0,
            strength_m: 1500.0,
            flatten_level: None,
            talus_deg: 0.5,
            erosion: 0.5,
            warp_km: 50.0,
            scale_km: 200.0,
            ridge_crest_m: -700.0,
            ridge_segment_km: 400.0,
            ridge_offset_km: 100.0,
            ridge_spread_mm: 25.0,
            ridge_magma: 0.0,
            range_spacing_km: 30.0,
            floor_on: false,
            floor_m: 0.0,
            ceiling_on: false,
            ceiling_m: 1000.0,
            value_range: (ELEV_MIN, ELEV_MAX),
        }
    }
}

/// True if a brush mode needs heights in meters: it follows the rivers or
/// the slope of the ground. A raster layer of another unit does not have
/// the mode.
pub fn needs_height(mode: Mode) -> bool {
    use Mode::*;
    matches!(
        mode,
        Carve | Ridge | Deposit | Erode | Weather | Uplift | OceanRidge | Range
    )
}

/// The limits of a locked radius, in radians.
const LOCK_RADIUS: std::ops::RangeInclusive<f64> = 1e-4..=MAX_BRUSH_RADIUS;

impl BrushSettings {
    /// The floor and the ceiling in levels. A limit that is off has no
    /// effect.
    pub fn limits(&self) -> Limits {
        let off = Limits::default();
        Limits {
            floor: match self.floor_on {
                true => self.level(self.floor_m),
                false => off.floor,
            },
            ceiling: match self.ceiling_on {
                true => self.level(self.ceiling_m),
                false => off.ceiling,
            },
        }
    }

    /// The level of a value of the layer that the brush paints. A value out
    /// of the range gets the nearest end.
    pub fn level(&self, value: f64) -> u16 {
        let (min, max) = self.value_range;
        (((value - min) / (max - min)).clamp(0.0, 1.0) * 65535.0).round() as u16
    }

    /// The value of a level of the layer that the brush paints.
    pub fn value(&self, level: u16) -> f64 {
        let (min, max) = self.value_range;
        min + f64::from(level) / 65535.0 * (max - min)
    }

    /// The strength as a value of the layer that the brush paints. Each
    /// layer gets the same part of its range, and for a height the value is
    /// `strength_m`.
    pub fn strength_value(&self) -> f64 {
        let (min, max) = self.value_range;
        self.strength_m * (max - min) / (ELEV_MAX - ELEV_MIN)
    }

    /// The settings of an ocean ridge stroke with this brush radius, in
    /// radians. `world_km` is the radius of the world.
    pub fn ridge_spec(&self, radius: f64, world_km: f64, seed: u32) -> Ridge {
        Ridge {
            radius,
            crest: meters_to_level(self.ridge_crest_m),
            segment: self.ridge_segment_km / world_km,
            offset: self.ridge_offset_km / world_km,
            spread: 0.5 * self.ridge_spread_mm / world_km,
            magma: self.ridge_magma,
            world_km,
            seed,
            limits: self.limits(),
        }
    }

    /// The settings of a range stroke with this brush radius, in radians.
    /// `world_km` is the radius of the world.
    pub fn range_spec(&self, radius: f64, world_km: f64, seed: u32) -> RangeSpec {
        let height = meters_to_level(self.strength_m) - meters_to_level(0.0);
        RangeSpec {
            radius,
            height: f64::from(height),
            spacing: self.range_spacing_km / world_km,
            seed,
            limits: self.limits(),
        }
    }

    /// The age of the crust at the edge of an ocean ridge stroke, in
    /// millions of years. `radius_km` is the brush radius on the ground.
    pub fn ridge_edge_age(&self, radius_km: f64) -> f64 {
        // A full rate in mm/yr is a full rate in km/Myr.
        radius_km / (0.5 * self.ridge_spread_mm)
    }

    /// The brush radius as an angle on the sphere, in radians. `globe_radius`
    /// is the radius of the globe on screen, in points.
    pub fn radius(&self, globe_radius: f64) -> f64 {
        match self.lock {
            Some(angle) => angle.clamp(*LOCK_RADIUS.start(), *LOCK_RADIUS.end()),
            None => brush_radius(self.size_points, globe_radius),
        }
    }

    /// The brush radius on screen, in points.
    pub fn points(&self, globe_radius: f64) -> f32 {
        match self.lock {
            Some(_) => (self.radius(globe_radius) * globe_radius) as f32,
            None => self.size_points,
        }
    }

    /// Locks the radius on the sphere, or lets it follow the zoom again. The
    /// brush keeps its size at this zoom.
    pub fn set_lock(&mut self, lock: bool, globe_radius: f64) {
        if lock == self.lock.is_some() {
            return;
        }
        if !lock {
            let points = self.points(globe_radius);
            self.size_points = points.clamp(*SIZE_POINTS.start(), *SIZE_POINTS.end());
        }
        self.lock = lock.then(|| self.radius(globe_radius));
    }

    /// The brush radius on the ground, in kilometers. `world_km` is the
    /// radius of the world.
    pub fn radius_km(&self, globe_radius: f64, world_km: f64) -> f64 {
        self.radius(globe_radius) * world_km
    }

    /// The limits of `radius_km` at this zoom.
    pub fn radius_km_range(
        &self,
        globe_radius: f64,
        world_km: f64,
    ) -> std::ops::RangeInclusive<f64> {
        let (min, max) = match self.lock {
            Some(_) => (*LOCK_RADIUS.start(), *LOCK_RADIUS.end()),
            None => (
                brush_radius(*SIZE_POINTS.start(), globe_radius),
                brush_radius(*SIZE_POINTS.end(), globe_radius),
            ),
        };
        min * world_km..=max * world_km
    }

    /// Sets the brush radius on the ground, in kilometers.
    pub fn set_radius_km(&mut self, km: f64, globe_radius: f64, world_km: f64) {
        let angle = km / world_km;
        match &mut self.lock {
            Some(lock) => *lock = angle.clamp(*LOCK_RADIUS.start(), *LOCK_RADIUS.end()),
            None => {
                let points = (angle * globe_radius) as f32;
                self.size_points = points.clamp(*SIZE_POINTS.start(), *SIZE_POINTS.end());
            }
        }
    }
}

/// The radius of a brush that follows the zoom, as an angle on the sphere in
/// radians. `globe_radius` is the radius of the globe on screen, in points.
pub fn brush_radius(size_points: f32, globe_radius: f64) -> f64 {
    (f64::from(size_points) / globe_radius).clamp(1e-4, MAX_BRUSH_RADIUS)
}

/// The flow of a pen with this force.
pub fn pen_flow(force: f32) -> f64 {
    (f64::from(force) * PRESSURE_GAIN).clamp(0.0, 1.0)
}

/// The point at the part `t` of the short arc from `a` to `b`.
pub fn slerp(a: V3, b: V3, t: f64) -> V3 {
    let w = angle(a, b);
    if w < 1e-9 {
        return a;
    }
    let s = w.sin();
    normalize(add(
        scale(a, ((1.0 - t) * w).sin() / s),
        scale(b, (t * w).sin() / s),
    ))
}

/// One input sample of a stroke.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// The place under the pen, or `None` off the globe.
    pub dir: Option<V3>,
    /// From 0 to 1.
    pub flow: f64,
    /// The brush radius in radians.
    pub radius: f64,
}

/// The most straight parts of one curve of a stroke.
const CURVE_PARTS: f64 = 64.0;

/// A place on the path of a stroke, and the flow there.
type Point = (V3, f64);

/// The state of one stroke between samples.
///
/// The path of the stroke is a curve, so a fast stroke has no corners. The
/// curve goes from the middle of one pair of samples to the middle of the
/// next pair, and the sample between them bends it. The path is half a
/// sample behind the pen. `finish` draws the rest.
#[derive(Default)]
pub struct Stroke {
    /// The last sample.
    last: Option<Point>,
    /// The end of the path.
    end: Option<Point>,
    /// The distance from the end of the path to the next stamp, in radians.
    carry: f64,
    /// The brush radius of the last sample, in radians.
    radius: f64,
    level: Option<u16>,
    /// The center of the last stamp of the line.
    last_center: Option<V3>,
}

/// The values that each stamp of one sample gets.
struct Tip<'a> {
    brush: &'a BrushSettings,
    radius: f64,
    spacing: f64,
    level: u16,
}

impl Tip<'_> {
    /// A stamp at `center`. `motion` is the move from the stamp before it.
    fn stamp(&self, center: V3, flow: f64, motion: V3) -> Stamp {
        let input = flow;
        let flow = flow * self.brush.flow;
        let span = (ELEV_MAX - ELEV_MIN) / 65535.0;
        Stamp {
            center,
            radius: self.radius,
            hardness: self.brush.hardness,
            flow: match self.brush.mode {
                Mode::Raise
                | Mode::Lower
                | Mode::Carve
                | Mode::Ridge
                | Mode::Erode
                | Mode::Weather
                | Mode::Uplift
                | Mode::Warp
                | Mode::Push => flow,
                // The flow of a mouse gives the full height of the ridge.
                Mode::OceanRidge | Mode::Range => (input / FIXED_FLOW).min(1.0) * self.brush.flow,
                // These three modes move toward a target, so each stamp does less.
                Mode::Smooth | Mode::Flatten | Mode::Deposit => flow * 0.3,
            },
            mode: self.brush.mode,
            level: self.level,
            strength: self.brush.strength_m * STAMP_SPACING / span
                * match self.brush.mode {
                    Mode::Uplift => 1.0 + UPLIFT_GAIN * self.brush.erosion.clamp(0.0, 1.0),
                    _ => 1.0,
                },
            // `set_world` sets the slope, the warp strength, the scale, and the
            // seed, which follow the world.
            talus: 0.0,
            scale: 0.0,
            seed: 0,
            motion,
            limits: self.brush.limits(),
        }
    }
}

/// Sets the parts of a stamp that follow the world: the slope limit, and
/// in the warp mode the offset and the size of the noise as angles.
/// `world_km` is the radius of the world.
pub fn set_world(
    stamp: &mut Stamp,
    brush: &BrushSettings,
    face_size: usize,
    world_km: f64,
    seed: u32,
) {
    stamp.talus = talus_levels(brush.talus_deg, face_size, world_km * 1000.0);
    stamp.scale = brush.scale_km / world_km;
    stamp.seed = seed;
    if stamp.mode == Mode::Warp {
        stamp.strength = brush.warp_km * STAMP_SPACING / world_km;
    }
}

/// The distance between the stamps of a mode, as a part of the brush radius.
/// The stamps of the ocean ridge mode and of the range mode are the points
/// of the line of the stroke.
fn stamp_spacing(mode: Mode) -> f64 {
    match mode {
        Mode::OceanRidge | Mode::Range => LINE_SPACING,
        _ => STAMP_SPACING,
    }
}

/// The place at `t` on the curve from `a` to `b` that `control` bends.
fn curve(a: Point, control: Point, b: Point, t: f64) -> Point {
    let (wa, wc, wb) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
    let dir = add(add(scale(a.0, wa), scale(control.0, wc)), scale(b.0, wb));
    (normalize(dir), a.1 * wa + control.1 * wc + b.1 * wb)
}

impl Stroke {
    /// Adds a stamp at `center` to `out`.
    fn put(&mut self, tip: &Tip, center: V3, flow: f64, out: &mut Vec<Stamp>) {
        let last = self.last_center.replace(center);
        let motion = last.map_or([0.0; 3], |last| arc_motion(last, center));
        out.push(tip.stamp(center, flow, motion));
    }

    fn tip<'a>(&self, brush: &'a BrushSettings, face_size: usize, level: u16) -> Tip<'a> {
        let texel = FRAC_PI_2 / face_size as f64;
        Tip {
            brush,
            radius: self.radius,
            spacing: (self.radius * stamp_spacing(brush.mode)).max(texel * 0.5),
            level,
        }
    }

    /// Adds the stamps of a straight part of the path, from its end to `to`.
    fn line_to(&mut self, tip: &Tip, to: Point, out: &mut Vec<Stamp>) {
        let Some(from) = self.end.replace(to) else {
            return;
        };
        let len = angle(from.0, to.0);
        let mut at = 0.0;
        while len - at >= self.carry {
            at += self.carry;
            let t = at / len;
            self.put(
                tip,
                slerp(from.0, to.0, t),
                from.1 + (to.1 - from.1) * t,
                out,
            );
            self.carry = tip.spacing;
        }
        self.carry -= len - at;
    }

    /// Adds the stamps of one sample to `out`. `level_at` gives the level of
    /// the heightmap at a place.
    pub fn add_sample(
        &mut self,
        brush: &BrushSettings,
        face_size: usize,
        sample: Sample,
        level_at: impl FnOnce(V3) -> u16,
        out: &mut Vec<Stamp>,
    ) {
        let Some(dir) = sample.dir else {
            self.finish(brush, face_size, out);
            (self.last, self.end, self.last_center) = (None, None, None);
            return;
        };
        let level = match (self.level, brush.flatten_level) {
            (Some(level), _) | (None, Some(level)) => level,
            (None, None) => level_at(dir),
        };
        self.level = Some(level);
        self.radius = sample.radius;
        let tip = self.tip(brush, face_size, level);
        let point = (dir, sample.flow);
        match (self.last.replace(point), self.end) {
            (Some(last), Some(end)) => {
                let middle = (normalize(add(last.0, dir)), (last.1 + sample.flow) * 0.5);
                let len = angle(end.0, last.0) + angle(last.0, middle.0);
                let parts = (len / tip.spacing).ceil().clamp(1.0, CURVE_PARTS);
                for part in 1..=parts as u32 {
                    let to = curve(end, last, middle, f64::from(part) / parts);
                    self.line_to(&tip, to, out);
                }
            }
            _ => {
                self.put(&tip, dir, sample.flow, out);
                self.carry = tip.spacing;
                self.end = Some(point);
            }
        }
    }

    /// Adds the stamps from the end of the path to the last sample. Call it
    /// when the pen goes up.
    pub fn finish(&mut self, brush: &BrushSettings, face_size: usize, out: &mut Vec<Stamp>) {
        if let (Some(last), Some(level)) = (self.last, self.level) {
            let tip = self.tip(brush, face_size, level);
            self.line_to(&tip, last, out);
            // The line of a ridge or a range ends at the last sample.
            let line = matches!(brush.mode, Mode::OceanRidge | Mode::Range);
            if line && self.end.is_some() {
                self.put(&tip, last.0, last.1, out);
            }
        }
    }
}

/// The number of texels in the rectangles of a plan.
pub fn plan_texels(plan: &StampPlan) -> u64 {
    let rects = plan.rects.iter().flatten();
    rects.map(|r| ((r.x1 - r.x0) * (r.y1 - r.y0)) as u64).sum()
}

/// The cost of a stamp, where `GROUP_STAMPS` is the cost of one pass.
fn stamp_cost(plan: &StampPlan) -> usize {
    match plan.stamp.mode {
        Mode::Smooth | Mode::Weather | Mode::Warp | Mode::Push => GROUP_STAMPS,
        Mode::Raise
        | Mode::Lower
        | Mode::Flatten
        | Mode::Carve
        | Mode::Ridge
        | Mode::Deposit
        | Mode::Erode
        | Mode::Uplift
        | Mode::OceanRidge
        | Mode::Range => 1,
    }
}

/// The stamps that wait for the GPU, each with the time of its pen sample.
#[derive(Default)]
pub struct Backlog {
    stamps: VecDeque<(StampPlan, Instant)>,
}

impl Backlog {
    pub fn push(&mut self, plan: StampPlan, time: Instant) {
        self.stamps.push_back((plan, time));
    }

    pub fn len(&self) -> usize {
        self.stamps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stamps.is_empty()
    }

    pub fn clear(&mut self) {
        self.stamps.clear();
    }

    /// Takes the stamps of one frame, oldest first: at most `FRAME_PASSES`
    /// passes and `FRAME_TEXELS` texels, and at least one stamp.
    pub fn take_frame(&mut self) -> Vec<(StampPlan, Instant)> {
        let mut out = Vec::new();
        let (mut cost, mut texels) = (0, 0);
        while let Some((plan, _)) = self.stamps.front() {
            let (more_cost, more) = (stamp_cost(plan), plan_texels(plan));
            let full =
                cost + more_cost > FRAME_PASSES * GROUP_STAMPS || texels + more > FRAME_TEXELS;
            if full && !out.is_empty() {
                break;
            }
            cost += more_cost;
            texels += more;
            out.extend(self.stamps.pop_front());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use vale_terrain::{Heightmap, meters_to_level};

    use super::*;
    use crate::globe::math::{length, lonlat_to_dir};

    fn sample(lon: f64, radius: f64) -> Sample {
        Sample {
            dir: Some(lonlat_to_dir(lon, 0.0)),
            flow: 1.0,
            radius,
        }
    }

    #[test]
    fn the_spacing_and_the_carry_set_the_stamp_count() {
        let brush = BrushSettings::default();
        let mut stroke = Stroke::default();
        let mut out = Vec::new();
        // The spacing is 0.012 radians. The arc is 0.1 radians in two parts.
        let radius = 0.1;
        let arc = 0.1_f64.to_degrees();
        stroke.add_sample(&brush, 1024, sample(0.0, radius), |_| 7, &mut out);
        assert_eq!(out.len(), 1);
        stroke.add_sample(&brush, 1024, sample(arc * 0.45, radius), |_| 9, &mut out);
        // The path ends at the middle of the two samples, at 0.0225 radians.
        assert_eq!(out.len(), 2);
        stroke.add_sample(&brush, 1024, sample(arc, radius), |_| 9, &mut out);
        // The path ends at 0.0725 radians.
        assert_eq!(out.len(), 7);
        stroke.finish(&brush, 1024, &mut out);
        // The stamps are at each 0.012 radians of the whole arc: 8 steps.
        assert_eq!(out.len(), 9);
        for (i, stamp) in out.iter().enumerate() {
            let at = angle(lonlat_to_dir(0.0, 0.0), stamp.center);
            assert!((at - 0.012 * i as f64).abs() < 1e-6, "stamp {i} at {at}");
            // The level comes from the start of the stroke.
            assert_eq!(stamp.level, 7);
        }
    }

    #[test]
    fn each_stamp_has_the_motion_from_the_stamp_before_it() {
        let brush = BrushSettings {
            mode: Mode::Push,
            ..BrushSettings::default()
        };
        let mut stroke = Stroke::default();
        let mut out = Vec::new();
        let arc = 0.1_f64.to_degrees();
        stroke.add_sample(&brush, 1024, sample(0.0, 0.1), |_| 7, &mut out);
        stroke.add_sample(&brush, 1024, sample(arc, 0.1), |_| 7, &mut out);
        stroke.finish(&brush, 1024, &mut out);
        assert!(out.len() > 5);
        assert_eq!(out[0].motion, [0.0; 3]);
        for pair in out.windows(2) {
            let motion = pair[1].motion;
            assert!((length(motion) - angle(pair[0].center, pair[1].center)).abs() < 1e-12);
            // The stroke goes east along the equator.
            assert!(motion[1] > 0.0 && motion[2].abs() < 1e-12, "{motion:?}");
            assert_eq!(pair[1].flow, 1.0);
        }

        // A sample off the globe breaks the line, so the next stamp has no
        // motion.
        let count = out.len();
        let off = Sample {
            dir: None,
            ..sample(0.0, 0.1)
        };
        stroke.add_sample(&brush, 1024, off, |_| 7, &mut out);
        stroke.add_sample(&brush, 1024, sample(2.0 * arc, 0.1), |_| 7, &mut out);
        assert_eq!(out.len(), count + 1);
        assert_eq!(out[count].motion, [0.0; 3]);
    }

    #[test]
    fn the_stamp_has_the_limits_of_the_brush_in_levels() {
        let stamp_of = |brush: &BrushSettings| {
            let mut out = Vec::new();
            Stroke::default().add_sample(brush, 1024, sample(0.0, 0.1), |_| 7, &mut out);
            out[0]
        };
        // Both limits are off at the start.
        let mut brush = BrushSettings::default();
        assert!(!brush.floor_on && !brush.ceiling_on);
        assert_eq!((brush.floor_m, brush.ceiling_m), (0.0, 1000.0));
        assert_eq!(brush.limits(), Limits::default());
        assert_eq!(stamp_of(&brush).limits, Limits::default());

        brush.floor_on = true;
        let floor = meters_to_level(0.0);
        assert_eq!(floor, 32768);
        assert_eq!(
            stamp_of(&brush).limits,
            Limits {
                floor,
                ceiling: 65535
            }
        );

        brush.ceiling_on = true;
        brush.ceiling_m = 3000.0;
        let ceiling = meters_to_level(3000.0);
        assert_eq!(ceiling, 49151);
        for mode in [
            Mode::Lower,
            Mode::Smooth,
            Mode::Weather,
            Mode::Uplift,
            Mode::Warp,
            Mode::Push,
        ] {
            brush.mode = mode;
            assert_eq!(stamp_of(&brush).limits, Limits { floor, ceiling });
        }

        // The floor can be above the ceiling.
        brush.floor_m = 4000.0;
        let limits = stamp_of(&brush).limits;
        assert_eq!(limits.floor, meters_to_level(4000.0));
        assert_eq!(limits.ceiling, ceiling);
    }

    #[test]
    fn the_spacing_is_at_least_half_a_texel() {
        let brush = BrushSettings::default();
        let mut stroke = Stroke::default();
        let mut out = Vec::new();
        let texel = FRAC_PI_2 / 256.0;
        stroke.add_sample(&brush, 256, sample(0.0, 1e-4), |_| 0, &mut out);
        let arc = (texel * 4.2).to_degrees();
        stroke.add_sample(&brush, 256, sample(arc, 1e-4), |_| 0, &mut out);
        stroke.finish(&brush, 256, &mut out);
        // The arc holds eight steps of half a texel.
        assert_eq!(out.len(), 9);
    }

    #[test]
    fn the_flow_changes_from_sample_to_sample() {
        let brush = BrushSettings {
            mode: Mode::Smooth,
            ..BrushSettings::default()
        };
        let mut stroke = Stroke::default();
        let mut out = Vec::new();
        let first = Sample {
            flow: 0.0,
            ..sample(0.0, 0.1)
        };
        stroke.add_sample(&brush, 1024, first, |_| 0, &mut out);
        let arc = 0.03_f64.to_degrees();
        stroke.add_sample(&brush, 1024, sample(arc, 0.1), |_| 0, &mut out);
        stroke.finish(&brush, 1024, &mut out);
        assert_eq!(out.len(), 3);
        // The stamps are at 0.4 and 0.8 of the arc. The smooth mode uses 0.3
        // of the flow.
        assert!((out[1].flow - 0.12).abs() < 1e-6, "{}", out[1].flow);
        assert!((out[2].flow - 0.24).abs() < 1e-6, "{}", out[2].flow);
    }

    #[test]
    fn the_flow_of_a_range_stamp_follows_the_pressure() {
        let brush = BrushSettings {
            mode: Mode::Range,
            flow: 0.8,
            ..BrushSettings::default()
        };
        let flows = |pressure: [f64; 2]| {
            let mut stroke = Stroke::default();
            let mut out = Vec::new();
            let arc = 0.03_f64.to_degrees();
            for (lon, flow) in [(0.0, pressure[0]), (arc, pressure[1])] {
                let at = Sample {
                    flow,
                    ..sample(lon, 0.1)
                };
                stroke.add_sample(&brush, 1024, at, |_| 0, &mut out);
            }
            stroke.finish(&brush, 1024, &mut out);
            out
        };
        // The flow of a mouse gives the full height.
        let mouse = flows([FIXED_FLOW; 2]);
        assert!(mouse.iter().all(|s| (s.flow - 0.8).abs() < 1e-9));
        // A light pen gives a lower range, and a firm pen the full height.
        let pen = flows([0.1, 0.4]);
        assert!((pen[0].flow - 0.8 * 0.2).abs() < 1e-9, "{}", pen[0].flow);
        assert!(pen.windows(2).all(|w| w[1].flow > w[0].flow));
        let last = pen.last().unwrap();
        assert!((last.flow - 0.8 * 0.8).abs() < 1e-9, "{}", last.flow);
        assert!(flows([1.0; 2]).iter().all(|s| (s.flow - 0.8).abs() < 1e-9));
        // The stamps are the points of the line, and the last one is at the
        // last sample.
        let step = angle(pen[0].center, pen[1].center);
        assert!((step - 0.1 * LINE_SPACING).abs() < 1e-9, "{step}");
        let end = lonlat_to_dir(0.03_f64.to_degrees(), 0.0);
        assert!(angle(last.center, end) < 1e-9);
    }

    #[test]
    fn a_sample_off_the_globe_breaks_the_line() {
        let brush = BrushSettings::default();
        let mut stroke = Stroke::default();
        let mut out = Vec::new();
        stroke.add_sample(&brush, 1024, sample(0.0, 0.1), |_| 0, &mut out);
        let off = Sample {
            dir: None,
            ..sample(0.0, 0.1)
        };
        stroke.add_sample(&brush, 1024, off, |_| 0, &mut out);
        stroke.add_sample(&brush, 1024, sample(40.0, 0.1), |_| 0, &mut out);
        assert_eq!(out.len(), 2);
    }

    /// The largest angle between two steps of the path of the stamps, in
    /// radians.
    fn sharpest_turn(stamps: &[Stamp]) -> f64 {
        let turns = stamps.windows(3).map(|w| {
            let a = add(w[1].center, scale(w[0].center, -1.0));
            let b = add(w[2].center, scale(w[1].center, -1.0));
            angle(a, b)
        });
        turns.fold(0.0, f64::max)
    }

    #[test]
    fn a_fast_curve_with_a_small_brush_has_no_corners() {
        let brush = BrushSettings::default();
        let mut stroke = Stroke::default();
        let mut out = Vec::new();
        // A circle of 0.1 radians from 12 samples: each sample turns the pen
        // 30 degrees. The stamps are 0.00024 radians apart.
        let radius = 0.002;
        for i in 0..=12 {
            let turn = f64::from(i) * std::f64::consts::TAU / 12.0;
            let (lon, lat) = (0.1 * turn.cos(), 0.1 * turn.sin());
            let at = Sample {
                dir: Some(lonlat_to_dir(lon.to_degrees(), lat.to_degrees())),
                flow: 1.0,
                radius,
            };
            stroke.add_sample(&brush, 4096, at, |_| 0, &mut out);
        }
        stroke.finish(&brush, 4096, &mut out);
        assert!(out.len() > 2000, "{}", out.len());
        let turn = sharpest_turn(&out).to_degrees();
        assert!(turn < 2.0, "the sharpest turn is {turn} degrees");
    }

    #[test]
    fn the_radius_stops_at_the_limit() {
        assert_eq!(brush_radius(160.0, 360.0), MAX_BRUSH_RADIUS);
        assert!((brush_radius(36.0, 360.0) - 0.1).abs() < 1e-9);
        assert_eq!(brush_radius(0.0, 360.0), 1e-4);
        assert_eq!(pen_flow(0.25), 0.5);
        assert_eq!(pen_flow(3.0), 1.0);
    }

    #[test]
    fn the_radius_in_kilometers_uses_the_world_radius() {
        let mut brush = BrushSettings::default();
        // 36 points on a globe of 360 points are 0.1 radians.
        assert!((brush.radius_km(360.0, 6371.0) - 637.1).abs() < 1e-9);
        assert!((brush.radius_km(360.0, 1000.0) - 100.0).abs() < 1e-9);
        brush.set_radius_km(50.0, 360.0, 1000.0);
        assert!((brush.size_points - 18.0).abs() < 1e-4);
        assert!((brush.radius_km(360.0, 1000.0) - 50.0).abs() < 1e-3);
        let range = brush.radius_km_range(360.0, 1000.0);
        assert!((range.start() - 4.0 / 0.36).abs() < 1e-9);
        assert_eq!(*range.end(), MAX_BRUSH_RADIUS * 1000.0);
        brush.set_radius_km(1.0, 360.0, 1000.0);
        assert_eq!(brush.size_points, 4.0);

        brush.set_lock(true, 360.0);
        brush.set_radius_km(20.0, 360.0, 1000.0);
        assert_eq!(brush.lock, Some(0.02));
        assert_eq!(brush.radius_km(720.0, 500.0), 10.0);
        assert_eq!(brush.radius_km_range(720.0, 500.0), 0.05..=150.0);
    }

    #[test]
    fn the_size_lock_keeps_the_ground_size_at_each_zoom() {
        let mut brush = BrushSettings::default();
        // Without the lock, the size on screen stays, and the ground size
        // follows the zoom.
        assert!((brush.radius(360.0) - 0.1).abs() < 1e-9);
        assert!((brush.radius(720.0) - 0.05).abs() < 1e-9);
        assert_eq!(brush.points(720.0), 36.0);

        brush.set_lock(true, 360.0);
        for globe_radius in [180.0, 360.0, 720.0, 14400.0] {
            assert!((brush.radius(globe_radius) - 0.1).abs() < 1e-9);
            assert!((brush.radius_km(globe_radius, 6371.0) - 637.1).abs() < 1e-6);
        }
        assert!((brush.points(720.0) - 72.0).abs() < 1e-4);

        // The brush keeps its size on screen when the lock goes off.
        brush.set_lock(false, 720.0);
        assert_eq!(brush.lock, None);
        assert!((brush.size_points - 72.0).abs() < 1e-4);
        assert!((brush.radius(720.0) - 0.1).abs() < 1e-6);
    }

    fn plan(map: &Heightmap, radius: f64) -> StampPlan {
        plan_of(map, radius, Mode::Raise)
    }

    #[test]
    fn a_warp_stamp_takes_its_angles_from_the_world_radius() {
        let brush = BrushSettings {
            mode: Mode::Warp,
            warp_km: 100.0,
            scale_km: 250.0,
            ..BrushSettings::default()
        };
        let mut stamp = plan_of(&Heightmap::new(256, 0), 0.1, Mode::Warp).stamp;
        set_world(&mut stamp, &brush, 256, 1000.0, 9);
        assert!((stamp.strength - 0.1 * STAMP_SPACING).abs() < 1e-12);
        assert!((stamp.scale - 0.25).abs() < 1e-12);
        assert_eq!(stamp.seed, 9);

        // Another mode keeps its strength.
        let mut raise = plan_of(&Heightmap::new(256, 0), 0.1, Mode::Raise).stamp;
        set_world(&mut raise, &brush, 256, 1000.0, 9);
        assert_eq!(raise.strength, 1.0);
    }

    fn plan_of(map: &Heightmap, radius: f64, mode: Mode) -> StampPlan {
        map.stamp_plan(&Stamp {
            center: lonlat_to_dir(0.0, 0.0),
            radius,
            hardness: 0.3,
            flow: 1.0,
            mode,
            level: 0,
            strength: 1.0,
            talus: 0.0,
            scale: 0.0,
            seed: 0,
            motion: [0.0; 3],
            ridged: false,
            relief: 0.0,
            limits: Limits::default(),
        })
    }

    #[test]
    fn smooth_stamps_cost_one_pass_each() {
        let map = Heightmap::new(256, 0);
        let start = Instant::now();
        let mut backlog = Backlog::default();
        for i in 0..FRAME_PASSES + 10 {
            let plan = plan_of(&map, 0.01, Mode::Smooth);
            backlog.push(plan, start + Duration::from_millis(i as u64));
        }
        assert_eq!(backlog.take_frame().len(), FRAME_PASSES);
        let second = backlog.take_frame();
        assert_eq!(second.len(), 10);
        assert_eq!(
            second[0].1,
            start + Duration::from_millis(FRAME_PASSES as u64)
        );

        // Half of the passes go to smooth stamps, and the other half to 32
        // times as many stamps of another mode.
        for mode in [Mode::Smooth, Mode::Flatten] {
            for _ in 0..2000 {
                backlog.push(plan_of(&map, 0.01, mode), start);
            }
            let frame = backlog.take_frame();
            let count = match mode {
                Mode::Smooth => FRAME_PASSES,
                _ => FRAME_PASSES * GROUP_STAMPS,
            };
            assert_eq!(frame.len(), count);
            backlog.clear();
        }
        for i in 0..2000 {
            let mode = if i < 24 { Mode::Smooth } else { Mode::Lower };
            backlog.push(plan_of(&map, 0.01, mode), start);
        }
        assert_eq!(backlog.take_frame().len(), 24 + 24 * GROUP_STAMPS);
    }

    #[test]
    fn the_budget_carries_the_rest_and_keeps_the_sample_times() {
        let map = Heightmap::new(256, 0);
        let start = Instant::now();
        let mut backlog = Backlog::default();
        // One frame takes 1536 small raise stamps.
        let most = FRAME_PASSES * GROUP_STAMPS;
        assert_eq!(most, 1536);
        for i in 0..most + 10 {
            backlog.push(plan(&map, 0.01), start + Duration::from_millis(i as u64));
        }
        let first = backlog.take_frame();
        assert_eq!(first.len(), most);
        assert_eq!(backlog.len(), 10);
        let second = backlog.take_frame();
        assert_eq!(second.len(), 10);
        assert_eq!(second[0].1, start + Duration::from_millis(most as u64));
        assert!(backlog.take_frame().is_empty());
    }

    #[test]
    fn the_budget_counts_texels_and_sends_one_stamp_at_least() {
        // One stamp of the largest brush covers about 9.8 million texels here.
        let map = Heightmap::new(8192, 0);
        let large = plan(&map, MAX_BRUSH_RADIUS);
        let texels = plan_texels(&large);
        assert!(texels > FRAME_TEXELS / 5 && texels < FRAME_TEXELS / 3);
        let mut backlog = Backlog::default();
        for _ in 0..8 {
            backlog.push(large, Instant::now());
        }
        let frame = backlog.take_frame();
        assert_eq!(frame.len() as u64, FRAME_TEXELS / texels);
        assert_eq!(backlog.len(), 8 - frame.len());

        // A stamp over the whole budget still goes out, alone.
        let map = Heightmap::new(32768, 0);
        let huge = plan(&map, MAX_BRUSH_RADIUS);
        assert!(plan_texels(&huge) > FRAME_TEXELS);
        let mut backlog = Backlog::default();
        backlog.push(huge, Instant::now());
        backlog.push(huge, Instant::now());
        assert_eq!(backlog.take_frame().len(), 1);
        assert_eq!(backlog.len(), 1);
    }
}
