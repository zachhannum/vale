//! The brush: settings, and the stamps that a stroke makes from pen samples.

use std::collections::VecDeque;
use std::f64::consts::FRAC_PI_2;
use std::time::Instant;

use vale_terrain::{ELEV_MAX, ELEV_MIN, GROUP_STAMPS, MAX_BRUSH_RADIUS, Mode, Stamp, StampPlan};

use super::math::{V3, add, angle, normalize, scale};

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
        }
    }
}

/// The limits of a locked radius, in radians.
const LOCK_RADIUS: std::ops::RangeInclusive<f64> = 1e-4..=MAX_BRUSH_RADIUS;

impl BrushSettings {
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
}

/// The values that each stamp of one sample gets.
struct Tip<'a> {
    brush: &'a BrushSettings,
    radius: f64,
    spacing: f64,
    level: u16,
}

impl Tip<'_> {
    fn stamp(&self, center: V3, flow: f64) -> Stamp {
        let flow = flow * self.brush.flow;
        let span = (ELEV_MAX - ELEV_MIN) / 65535.0;
        Stamp {
            center,
            radius: self.radius,
            hardness: self.brush.hardness,
            flow: match self.brush.mode {
                Mode::Raise | Mode::Lower | Mode::Carve => flow,
                // These two modes move toward a target, so each stamp does less.
                Mode::Smooth | Mode::Flatten => flow * 0.3,
            },
            mode: self.brush.mode,
            level: self.level,
            strength: self.brush.strength_m * STAMP_SPACING / span,
        }
    }
}

/// The place at `t` on the curve from `a` to `b` that `control` bends.
fn curve(a: Point, control: Point, b: Point, t: f64) -> Point {
    let (wa, wc, wb) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
    let dir = add(add(scale(a.0, wa), scale(control.0, wc)), scale(b.0, wb));
    (normalize(dir), a.1 * wa + control.1 * wc + b.1 * wb)
}

impl Stroke {
    fn tip<'a>(&self, brush: &'a BrushSettings, face_size: usize, level: u16) -> Tip<'a> {
        let texel = FRAC_PI_2 / face_size as f64;
        Tip {
            brush,
            radius: self.radius,
            spacing: (self.radius * STAMP_SPACING).max(texel * 0.5),
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
            out.push(tip.stamp(slerp(from.0, to.0, t), from.1 + (to.1 - from.1) * t));
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
            (self.last, self.end) = (None, None);
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
                out.push(tip.stamp(dir, sample.flow));
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
        Mode::Smooth => GROUP_STAMPS,
        Mode::Raise | Mode::Lower | Mode::Flatten | Mode::Carve => 1,
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

    use vale_terrain::Heightmap;

    use super::*;
    use crate::globe::math::lonlat_to_dir;

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

    fn plan_of(map: &Heightmap, radius: f64, mode: Mode) -> StampPlan {
        map.stamp_plan(&Stamp {
            center: lonlat_to_dir(0.0, 0.0),
            radius,
            hardness: 0.3,
            flow: 1.0,
            mode,
            level: 0,
            strength: 1.0,
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
