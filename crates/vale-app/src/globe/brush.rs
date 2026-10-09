//! The brush: settings, and the stamps that a stroke makes from pen samples.

use std::collections::VecDeque;
use std::f64::consts::FRAC_PI_2;
use std::time::Instant;

use vale_terrain::{ELEV_MAX, ELEV_MIN, MAX_BRUSH_RADIUS, Mode, Stamp, StampPlan};

use super::math::{V3, add, angle, normalize, scale};

/// The distance between stamps, as a part of the brush radius.
pub const STAMP_SPACING: f64 = 0.12;
/// The flow of a mouse, which has no pressure.
pub const FIXED_FLOW: f64 = 0.5;
/// The multiplier from pen force to flow.
pub const PRESSURE_GAIN: f64 = 2.0;
/// The limits of the brush radius on screen, in points.
pub const SIZE_POINTS: std::ops::RangeInclusive<f32> = 4.0..=160.0;
/// The most stamps that one frame sends to the GPU.
pub const FRAME_STAMPS: usize = 48;
/// The most texels that the stamps of one frame cover.
pub const FRAME_TEXELS: u64 = 40_000_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
    pub mode: Mode,
    /// The radius on screen, in points.
    pub size_points: f32,
    pub hardness: f64,
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
            hardness: 0.3,
            strength_m: 1500.0,
            flatten_level: None,
        }
    }
}

/// The brush radius as an angle on the sphere, in radians. `globe_radius` is
/// the radius of the globe on screen, in points.
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

/// The state of one stroke between samples.
#[derive(Default)]
pub struct Stroke {
    last: Option<(V3, f64)>,
    /// The distance from the last sample to the next stamp, in radians.
    carry: f64,
    level: Option<u16>,
}

impl Stroke {
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
            self.last = None;
            return;
        };
        let Sample { flow, radius, .. } = sample;
        let texel = FRAC_PI_2 / face_size as f64;
        let spacing = (radius * STAMP_SPACING).max(texel * 0.5);
        let level = match (self.level, brush.flatten_level) {
            (Some(level), _) | (None, Some(level)) => level,
            (None, None) => level_at(dir),
        };
        self.level = Some(level);
        let span = (ELEV_MAX - ELEV_MIN) / 65535.0;
        let stamp_at = |center: V3, flow: f64| Stamp {
            center,
            radius,
            hardness: brush.hardness,
            flow: match brush.mode {
                Mode::Raise | Mode::Lower => flow,
                // These two modes move toward a target, so each stamp does less.
                Mode::Smooth | Mode::Flatten => flow * 0.3,
            },
            mode: brush.mode,
            level,
            strength: brush.strength_m * STAMP_SPACING / span,
        };
        match self.last {
            None => {
                out.push(stamp_at(dir, flow));
                self.carry = spacing;
            }
            Some((prev, prev_flow)) => {
                let len = angle(prev, dir);
                let mut at = 0.0;
                while len - at >= self.carry {
                    at += self.carry;
                    let t = at / len;
                    out.push(stamp_at(
                        slerp(prev, dir, t),
                        prev_flow + (flow - prev_flow) * t,
                    ));
                    self.carry = spacing;
                }
                self.carry -= len - at;
            }
        }
        self.last = Some((dir, flow));
    }
}

/// The number of texels in the rectangles of a plan.
pub fn plan_texels(plan: &StampPlan) -> u64 {
    let rects = plan.rects.iter().flatten();
    rects.map(|r| ((r.x1 - r.x0) * (r.y1 - r.y0)) as u64).sum()
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

    /// Takes the stamps of one frame, oldest first: at most `FRAME_STAMPS`
    /// stamps and `FRAME_TEXELS` texels, and at least one stamp.
    pub fn take_frame(&mut self) -> Vec<(StampPlan, Instant)> {
        let mut out = Vec::new();
        let mut texels = 0;
        while let Some((plan, _)) = self.stamps.front() {
            let more = plan_texels(plan);
            let full = out.len() == FRAME_STAMPS || texels + more > FRAME_TEXELS;
            if full && !out.is_empty() {
                break;
            }
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
        // 0.045 radians hold three steps, and 0.009 radians carry over.
        assert_eq!(out.len(), 4);
        stroke.add_sample(&brush, 1024, sample(arc, radius), |_| 9, &mut out);
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

    #[test]
    fn the_radius_stops_at_the_limit() {
        assert_eq!(brush_radius(160.0, 360.0), MAX_BRUSH_RADIUS);
        assert!((brush_radius(36.0, 360.0) - 0.1).abs() < 1e-9);
        assert_eq!(brush_radius(0.0, 360.0), 1e-4);
        assert_eq!(pen_flow(0.25), 0.5);
        assert_eq!(pen_flow(3.0), 1.0);
    }

    fn plan(map: &Heightmap, radius: f64) -> StampPlan {
        map.stamp_plan(&Stamp {
            center: lonlat_to_dir(0.0, 0.0),
            radius,
            hardness: 0.3,
            flow: 1.0,
            mode: Mode::Raise,
            level: 0,
            strength: 1.0,
        })
    }

    #[test]
    fn the_budget_carries_the_rest_and_keeps_the_sample_times() {
        let map = Heightmap::new(256, 0);
        let start = Instant::now();
        let mut backlog = Backlog::default();
        for i in 0..FRAME_STAMPS + 10 {
            backlog.push(plan(&map, 0.01), start + Duration::from_millis(i as u64));
        }
        let first = backlog.take_frame();
        assert_eq!(first.len(), FRAME_STAMPS);
        assert_eq!(backlog.len(), 10);
        let second = backlog.take_frame();
        assert_eq!(second.len(), 10);
        assert_eq!(
            second[0].1,
            start + Duration::from_millis(FRAME_STAMPS as u64)
        );
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
