//! The scarp brush.
//!
//! The brush drops the ground on the right side of the stroke, as a normal
//! fault does. It is a line brush, as the abyssal hills brush is: each texel
//! near the line gets a place on it, and the drop is a function of that
//! place. A seeded field bends the trace of the fault and changes the drop
//! along the line. With more steps than one, the drop is on parallel faults.
//! Slide scars cut amphitheaters into the face, each with an apron of debris
//! below it. A plan lays the scarp on the ground from before the stroke, so a
//! second pass over a place in one stroke does not drop it again.

use std::f64::consts::FRAC_PI_2;
use std::ops::{Deref, RangeInclusive};

use crate::heightmap::{Limits, MAX_BRUSH_RADIUS, falloff, hash};
use crate::hotspot::place_seed;
use crate::line::{LinePlan, LinePoint, REACH, StrokeLine, bump, noise1, smoothstep, unit};
use crate::math::V3;
use crate::seamount::LEVELS_PER_M;

/// The default drop of a scarp, in meters.
pub const SCARP_DROP_M: f64 = 2000.0;

/// The drop of a scarp, in meters.
pub const SCARP_DROP_RANGE: RangeInclusive<f64> = 50.0..=6000.0;

/// The default width of a scarp across the fault, in kilometers.
pub const SCARP_WIDTH_KM: f64 = 20.0;

/// The width of a scarp across the fault, in kilometers.
pub const SCARP_WIDTH_RANGE: RangeInclusive<f64> = 1.0..=300.0;

/// The number of parallel faults of a scarp.
pub const SCARP_STEPS: RangeInclusive<u32> = 1..=5;

/// The bend of the trace, as a part of the brush radius at most.
const BEND: f64 = 0.15;
/// The change of the drop along the line, as a part of the drop at most.
const DROP_CHANGE: f64 = 0.4;
/// The change of the share of the drop of one step, as a part of an even
/// share at most.
const STEP_CHANGE: f64 = 0.3;
/// The narrowest scarp, in texel steps.
const MIN_WIDTH_TEXELS: f64 = 4.0;
/// The radius of the line of a stroke, as a part of the brush radius. The
/// line then reaches 2 brush radii, so the dropped ground goes back up over
/// the brush radius past the brush.
pub(crate) const LINE_RADIUS: f64 = 2.0 / REACH;

/// The salts of the hashes of the field.
const SALT_BEND: u32 = 0x5ca7_0001;
const SALT_DROP: u32 = 0x5ca7_0002;
const SALT_STEP: u32 = 0x5ca7_0003;
const SALT_SCAR: u32 = 0x5ca7_0004;

/// The settings of a scarp stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scarp {
    /// The drop of the ground on the right side of the stroke, in meters.
    pub drop_m: f64,
    /// The width of the scarp across the fault, in kilometers. It is the
    /// brush radius at most.
    pub width_km: f64,
    /// The number of parallel faults that step down, in `SCARP_STEPS`.
    pub steps: u32,
    /// Whether slide scars cut the face of the scarp.
    pub scars: bool,
    /// The radius of the world, in kilometers.
    pub world_km: f64,
    /// The seed of the field. The first point of the stroke also goes into
    /// the seed of a stroke.
    pub seed: u32,
    pub limits: Limits,
}

/// The texels that new points of a scarp stroke change, and the segments
/// of the line that give the levels there.
#[derive(Clone, Debug)]
pub struct ScarpPlan {
    /// The settings, inside their limits.
    pub spec: Scarp,
    /// The brush radius, in radians.
    pub radius: f64,
    /// The part of the radius with the full drop.
    pub hardness: f64,
    /// The width of the scarp, in radians, from 4 texel steps to the brush
    /// radius.
    pub width: f64,
    /// The seed of the stroke.
    pub seed: u32,
    pub line: LinePlan,
}

impl Deref for ScarpPlan {
    type Target = LinePlan;

    fn deref(&self) -> &LinePlan {
        &self.line
    }
}

/// The line of one scarp stroke.
#[derive(Clone, Debug)]
pub struct ScarpStroke {
    spec: Scarp,
    face_size: usize,
    line: Option<StrokeLine>,
    hardness: f64,
    seed: u32,
    /// The number of points that the plans so far cover.
    from: usize,
}

impl ScarpStroke {
    /// Starts a stroke on a heightmap with `face_size` texels along one side
    /// of a face.
    pub fn new(spec: Scarp, face_size: usize) -> ScarpStroke {
        let clamp = |x: f64, range: RangeInclusive<f64>| x.clamp(*range.start(), *range.end());
        let spec = Scarp {
            drop_m: clamp(spec.drop_m, SCARP_DROP_RANGE),
            width_km: clamp(spec.width_km, SCARP_WIDTH_RANGE),
            steps: spec.steps.clamp(*SCARP_STEPS.start(), *SCARP_STEPS.end()),
            world_km: spec.world_km.max(1.0),
            ..spec
        };
        ScarpStroke {
            spec,
            face_size,
            line: None,
            hardness: 0.0,
            seed: spec.seed,
            from: 0,
        }
    }

    /// The number of points of the line.
    pub fn len(&self) -> usize {
        self.line.as_ref().map_or(0, StrokeLine::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Adds a pen sample at unit direction `center`, with a radius in
    /// radians, a hardness, and a flow from 0 to 1, and returns the plan of
    /// the texels that change. The first sample sets the radius and the
    /// hardness of the stroke. The flow sets the drop along the line. The
    /// scarp shows once the line has two points.
    pub fn stamp(&mut self, center: V3, radius: f64, hardness: f64, flow: f64) -> ScarpPlan {
        self.push(center, radius, hardness, flow);
        self.next_plan()
    }

    /// Adds pen samples at once, as `stamp` adds one, and returns the plans
    /// of the texels that change. A plan covers 16 new points at most.
    pub fn stamps(
        &mut self,
        centers: &[V3],
        radius: f64,
        hardness: f64,
        flow: f64,
    ) -> Vec<ScarpPlan> {
        for &center in centers {
            self.push(center, radius, hardness, flow);
        }
        let mut plans = vec![self.next_plan()];
        while self.from < self.len() {
            plans.push(self.next_plan());
        }
        plans
    }

    /// Adds a point to the line. The first call makes the line and the seed
    /// of the stroke.
    fn push(&mut self, center: V3, radius: f64, hardness: f64, flow: f64) {
        let n = self.face_size;
        let step = FRAC_PI_2 / n as f64;
        if self.line.is_none() {
            let radius = radius.max(4.0 * step).min(MAX_BRUSH_RADIUS / 2.0);
            self.hardness = hardness.clamp(0.0, 0.999);
            self.line = Some(StrokeLine::new(radius * LINE_RADIUS, n));
            self.seed = place_seed(self.spec.seed, crate::math::normalize(center));
        }
        let line = self.line.as_mut().expect("the line is there");
        line.push(center, flow.clamp(0.0, 1.0));
    }

    /// The plan of the points from the end of the plans before it.
    fn next_plan(&mut self) -> ScarpPlan {
        let line = self.line.as_ref().expect("a stamp makes the line");
        let plan = line.plan(self.from);
        self.from = plan.end;
        let radius = line.radius() / LINE_RADIUS;
        let step = FRAC_PI_2 / self.face_size as f64;
        let width = (self.spec.width_km / self.spec.world_km)
            .max(MIN_WIDTH_TEXELS * step)
            .min(radius);
        ScarpPlan {
            spec: self.spec,
            radius,
            hardness: self.hardness,
            width,
            seed: self.seed,
            line: plan,
        }
    }
}

impl ScarpPlan {
    /// Whether the plan changes nothing.
    pub fn is_empty(&self) -> bool {
        self.line.rects.iter().all(Option::is_none)
    }

    /// The drop in 16-bit steps.
    pub(crate) fn drop_levels(&self) -> f64 {
        self.spec.drop_m * LEVELS_PER_M
    }

    /// The number of slide scars along one brush radius of line, or zero
    /// with no scars. The scars are about 3 widths apart.
    pub(crate) fn scar_count(&self) -> u32 {
        if !self.spec.scars {
            return 0;
        }
        (self.radius / (3.0 * self.width)).round().max(1.0) as u32
    }

    /// The bend of the trace to the left, in radians, and the drop, in
    /// 16-bit steps, at the distance `ur` along the line in brush radii.
    pub(crate) fn field(&self, ur: f64) -> (f64, f64) {
        let seed = self.seed;
        let bend = 0.3 * noise1(0.5 * ur, 0, SALT_BEND, seed).0
            + 0.4 * noise1(2.0 * ur, 1, SALT_BEND, seed).0
            + 0.3 * noise1(4.0 * ur, 2, SALT_BEND, seed).0;
        let change =
            0.6 * noise1(ur, 0, SALT_DROP, seed).0 + 0.4 * noise1(4.0 * ur, 1, SALT_DROP, seed).0;
        (
            BEND * self.radius * bend,
            self.drop_levels() * (1.0 + DROP_CHANGE * change),
        )
    }

    /// The part of the drop at the distance `s` to the right of the trace,
    /// from 0 to 1, at the distance `ur` along the line in brush radii. Each
    /// step is a fault with a seeded share of the drop.
    pub(crate) fn profile(&self, ur: f64, s: f64) -> f64 {
        let n = self.spec.steps;
        let c = 0.5 * self.width;
        let w = self.width / f64::from(2 * n - 1);
        let mut shares = [0.0; 5];
        let mut total = 0.0;
        for (i, share) in (0..n).zip(&mut shares) {
            *share = 1.0 + STEP_CHANGE * noise1(0.5 * ur, i, SALT_STEP, self.seed).0;
            total += *share;
        }
        let mut part = 0.0;
        for (i, share) in (0..n).zip(shares) {
            let center = -c + 0.5 * w + 2.0 * f64::from(i) * w;
            part += share * smoothstep(center - 0.5 * w, center + 0.5 * w, s);
        }
        part / total
    }

    /// The cut of the slide scars at a place, less their aprons, in 16-bit
    /// steps. `ur` is the distance along the line in brush radii, `s` the
    /// distance to the right of the trace, `drop` the drop there, and
    /// `profile` the part of the drop there. A scar lowers the face down to
    /// the lower ground and bites into the higher ground. Its apron lies on
    /// the lower ground below it.
    pub(crate) fn scars(&self, ur: f64, s: f64, drop: f64, profile: f64) -> f64 {
        let m = self.scar_count();
        if m == 0 {
            return 0.0;
        }
        let (width, c) = (self.width, 0.5 * self.width);
        let v = ur * f64::from(m);
        let j = v.floor() as i32;
        // The width of the apron.
        let apron = (0.6 * width).min(0.5 * (self.radius - c));
        let mut total = 0.0;
        for k in j - 1..=j + 1 {
            let h = |i: u32| unit(hash(k as u32, SALT_SCAR, i, self.seed));
            if h(0) >= 0.5 {
                continue;
            }
            // The distance along the line from the center of the scar, in
            // scar spacings, and the half length of the scar.
            let dv = v - (f64::from(k) + 0.25 + 0.5 * h(1));
            let half = 0.25 + 0.15 * h(2);
            let depth = (0.25 + 0.25 * h(3)) * drop;
            let q = dv / half;
            let head = -c + 0.15 * width - 0.5 * width * (1.0 - q * q);
            let cut = bump(q) * smoothstep(head, head + 0.12 * width, s) * (1.0 - profile);
            let r = (q / 1.2).hypot((s - c - 0.5 * apron) / (0.5 * apron));
            total += depth * (cut - 0.45 * bump(r));
        }
        total
    }

    /// The drop of the ground at a place on the line, in 16-bit steps. Past
    /// the ends of the line, the drop tapers with the falloff of the brush.
    /// The dropped ground stays down to the edge of the brush, and it goes
    /// back up to the old ground over the next brush radius.
    pub(crate) fn change(&self, at: LinePoint) -> f64 {
        let r = self.radius;
        if at.past >= r {
            return 0.0;
        }
        let side = 1.0 - smoothstep(r, 2.0 * r, at.across.abs());
        let weight = at.flow * falloff(at.past / r, self.hardness) * side;
        if weight <= 0.0 {
            return 0.0;
        }
        let ur = at.u / r;
        let (bend, drop) = self.field(ur);
        let s = bend - at.across;
        let profile = self.profile(ur, s);
        weight * (drop * profile + self.scars(ur, s, drop, profile))
    }

    /// The new level of a texel with the level `start` from before the
    /// stroke, at a place on the line.
    pub(crate) fn level(&self, start: u16, at: LinePoint) -> u16 {
        let change = self.change(at);
        if change == 0.0 {
            return start;
        }
        let old = f64::from(start);
        let new = (old + (0.5 - change).floor()).clamp(0.0, 65535.0);
        self.spec.limits.apply(old, new) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::{FACES, meters_to_level};
    use crate::heightmap::Heightmap;
    use crate::math::{angle, lonlat_to_dir};

    const WORLD_KM: f64 = 1000.0;
    const RADIUS: f64 = 0.05;

    fn spec() -> Scarp {
        Scarp {
            drop_m: 1000.0,
            width_km: 20.0,
            steps: 1,
            scars: false,
            world_km: WORLD_KM,
            seed: 3,
            limits: Limits::default(),
        }
    }

    fn flat(n: usize) -> Heightmap {
        Heightmap::new(n, meters_to_level(-1000.0))
    }

    /// Paints a stroke through `points` in one undo step.
    fn paint(map: &mut Heightmap, spec: Scarp, points: &[V3], radius: f64, flow: f64) {
        let mut stroke = ScarpStroke::new(spec, map.face_size());
        map.begin_stroke();
        for &p in points {
            let plan = stroke.stamp(p, radius, 0.5, flow);
            map.scarp(&plan);
        }
        map.end_stroke();
    }

    /// Points from `a` to `b` in degrees of longitude and latitude.
    fn points(a: (f64, f64), b: (f64, f64), count: usize) -> Vec<V3> {
        (0..count)
            .map(|i| {
                let t = i as f64 / (count - 1) as f64;
                lonlat_to_dir(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
            })
            .collect()
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

    /// The plan of a stroke of two points along the equator.
    fn plan(spec: Scarp) -> ScarpPlan {
        let mut stroke = ScarpStroke::new(spec, 1024);
        stroke.stamp(lonlat_to_dir(0.0, 0.0), RADIUS, 0.5, 1.0);
        stroke.stamp(lonlat_to_dir(1.0, 0.0), RADIUS, 0.5, 1.0)
    }

    /// The place at the distance `ur` along the line in brush radii and at
    /// the distance `s` to the right of the trace, inside the line.
    fn at(plan: &ScarpPlan, ur: f64, s: f64) -> LinePoint {
        LinePoint {
            u: ur * plan.radius,
            across: plan.field(ur).0 - s,
            past: 0.0,
            flow: 1.0,
            end: 1.0,
        }
    }

    #[test]
    fn the_drop_and_the_width_set_the_scarp() {
        let one = plan(spec());
        let two = plan(Scarp {
            drop_m: 2000.0,
            ..spec()
        });
        let wide = plan(Scarp {
            width_km: 40.0,
            ..spec()
        });
        let c = 0.5 * one.width;
        assert!((one.width - 0.02).abs() < 1e-12, "{}", one.width);
        assert!((wide.width - 0.04).abs() < 1e-12, "{}", wide.width);
        let mut mean = 0.0;
        for i in 0..200 {
            let ur = 0.1 * f64::from(i);
            let drop = one.field(ur).1;
            // The lower side, inside the full part of the brush.
            let low = one.change(at(&one, ur, c + 0.2 * (RADIUS - c)));
            assert!((low - drop).abs() < 1e-6, "{low} for a drop of {drop}");
            let low_two = two.change(at(&two, ur, c + 0.2 * (RADIUS - c)));
            assert!((low_two - 2.0 * low).abs() < 1e-6, "{low_two} and {low}");
            mean += low / 200.0;
            // The higher side does not change, and the face is the width.
            assert!(one.change(at(&one, ur, -c)) < 1e-6);
            assert!((one.change(at(&one, ur, 0.0)) - 0.5 * drop).abs() < 1e-6);
            assert!((one.change(at(&one, ur, c)) - drop).abs() < 1e-6);
            let half = wide.change(at(&wide, ur, c));
            assert!(half > 0.6 * drop && half < 0.9 * drop, "{half} of {drop}");
            assert!(wide.change(at(&wide, ur, -2.0 * c)) < 1e-6);
        }
        let full = 1000.0 * LEVELS_PER_M;
        assert!((mean - full).abs() < 0.2 * full, "a mean drop of {mean}");
        // The width is the brush radius at most.
        let widest = plan(Scarp {
            width_km: 300.0,
            ..spec()
        });
        assert_eq!(widest.width, RADIUS);
    }

    #[test]
    fn the_steps_make_parallel_faults_that_step_down() {
        let plan = plan(Scarp { steps: 3, ..spec() });
        let c = 0.5 * plan.width;
        let w = plan.width / 5.0;
        for i in 0..50 {
            let ur = 0.37 * f64::from(i);
            let drop = plan.field(ur).1;
            let change = |s: f64| plan.change(at(&plan, ur, s));
            // The drop grows across the scarp.
            let mut last = 0.0;
            for k in 0..=100 {
                let now = change(-c + plan.width * f64::from(k) / 100.0);
                assert!(now >= last - 1e-9, "{now} after {last}");
                last = now;
            }
            // Between two faults, the ground is flat, part of the way down.
            let mut level = 0.0;
            for gap in 0..2 {
                let center = -c + 1.5 * w + 2.0 * f64::from(gap) * w;
                let here = change(center);
                assert!((change(center - 0.4 * w) - here).abs() < 1e-6);
                assert!((change(center + 0.4 * w) - here).abs() < 1e-6);
                assert!(here > level + 0.15 * drop, "{here} after {level}");
                assert!(here < drop - 0.15 * drop, "{here} of {drop}");
                level = here;
            }
        }
    }

    #[test]
    fn the_scarp_does_not_depend_on_the_stamp_spacing() {
        let mut a = flat(512);
        let mut b = flat(512);
        let base = a.base();
        paint(
            &mut a,
            spec(),
            &points((0.0, 0.0), (12.0, 0.0), 13),
            0.05,
            1.0,
        );
        paint(
            &mut b,
            spec(),
            &points((0.0, 0.0), (12.0, 0.0), 49),
            0.05,
            1.0,
        );
        let (a, b) = (levels(&a), levels(&b));
        let worst = a.iter().zip(&b).map(|(x, y)| x.abs_diff(*y)).max();
        assert!(worst <= Some(1), "a difference of {worst:?}");
        let low = a.iter().filter(|&&l| l < base - 2000).count();
        assert!(low > 1000, "{low} texels dropped");
    }

    #[test]
    fn the_ends_taper_with_the_falloff_of_the_brush() {
        let plan = plan(Scarp {
            width_km: 10.0,
            ..spec()
        });
        let c = 0.5 * plan.width;
        let ur = plan.length / plan.radius;
        let (bend, drop) = plan.field(ur);
        let mut last = f64::MAX;
        for k in 0..=60 {
            let past = RADIUS * f64::from(k) / 50.0;
            let s = c + 0.1 * (RADIUS - c);
            let place = LinePoint {
                past,
                end: 0.0,
                ..at(&plan, ur, s)
            };
            let e = past / RADIUS;
            let want = if e < 1.0 { drop * falloff(e, 0.5) } else { 0.0 };
            let now = plan.change(place);
            assert!((now - want).abs() < 1e-6, "{now} for {want} at {past}");
            assert!(now <= last, "{now} after {last}");
            last = now;
            assert!(bend.abs() <= BEND * RADIUS);
        }
        assert_eq!(last, 0.0);
    }

    #[test]
    fn the_dropped_ground_goes_back_up_more_gently_than_the_face() {
        let plan = plan(Scarp {
            width_km: 20.0,
            ..spec()
        });
        let ur = 2.0;
        let bend = plan.field(ur).0;
        let change = |across: f64| {
            let place = LinePoint {
                across,
                ..at(&plan, ur, 0.0)
            };
            plan.change(place)
        };
        let step = 0.002 * RADIUS;
        let slope = |from: f64, to: f64| {
            let mut most = 0.0f64;
            let mut a = from;
            while a < to {
                most = most.max((change(a) - change(a - step)).abs() / step);
                a += step;
            }
            most
        };
        let drop = plan.field(ur).1;
        // The ground stays down to the edge of the brush, at any hardness.
        assert!((change(-RADIUS) - drop).abs() < 1e-6 * drop);
        assert_eq!(change(-2.0 * RADIUS), 0.0);
        let c = 0.5 * plan.width;
        let face = slope(bend - c, bend + c);
        let back = slope(-2.0 * RADIUS, -RADIUS);
        assert!(
            back < 0.5 * face,
            "{back} on the far side and {face} on the face"
        );
    }

    #[test]
    fn the_seeded_field_bends_the_trace_and_changes_the_drop() {
        let plan = plan(spec());
        let other = plan_seeded(4);
        let fields: Vec<(f64, f64)> = (0..400).map(|i| plan.field(0.05 * f64::from(i))).collect();
        let bends = fields.iter().map(|f| f.0);
        let drops = fields.iter().map(|f| f.1);
        let (low, high) = bends.fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
        assert!(high - low > 0.1 * RADIUS, "a bend from {low} to {high}");
        assert!(high <= BEND * RADIUS && low >= -BEND * RADIUS);
        let (low, high) = drops.fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
        assert!(high > 1.15 * low, "a drop from {low} to {high}");
        // The trace turns less than 45 degrees from the stroke.
        for pair in fields.windows(2) {
            assert!((pair[1].0 - pair[0].0).abs() < 0.05 * RADIUS);
        }
        // Another seed gives another field.
        assert_ne!(plan.field(3.3), other.field(3.3));
    }

    fn plan_seeded(seed: u32) -> ScarpPlan {
        plan(Scarp { seed, ..spec() })
    }

    #[test]
    fn slide_scars_cut_amphitheaters_with_an_apron_below() {
        let on = plan(Scarp {
            scars: true,
            ..spec()
        });
        let off = plan(spec());
        let (width, c) = (on.width, 0.5 * on.width);
        let apron = (0.6 * width).min(0.5 * (RADIUS - c));
        assert!(on.scar_count() >= 1);
        let (mut best, mut best_ur) = (0.0, 0.0);
        let mut plain = 0;
        for i in 0..2000 {
            let ur = 0.01 * f64::from(i);
            let s = -0.5 * c;
            let cut = on.change(at(&on, ur, s)) - off.change(at(&off, ur, s));
            assert!(cut >= -1e-9, "a scar raises the face by {cut}");
            plain += usize::from(cut == 0.0);
            if cut > best {
                (best, best_ur) = (cut, ur);
            }
        }
        let drop = on.field(best_ur).1;
        assert!(best > 0.1 * drop, "a cut of {best} for a drop of {drop}");
        assert!(plain > 100, "{plain} places with no scar");
        // The scar bites into the higher ground behind the face.
        let behind = on.change(at(&on, best_ur, -c - 0.1 * width));
        assert!(behind > 0.05 * drop, "{behind} behind the face");
        // The apron lies on the lower ground below the scar.
        let s = c + 0.5 * apron;
        let raise = off.change(at(&off, best_ur, s)) - on.change(at(&on, best_ur, s));
        assert!(raise > 0.05 * drop, "an apron of {raise}");
    }

    #[test]
    fn a_second_pass_in_one_stroke_does_not_drop_again() {
        let spec = Scarp { steps: 2, ..spec() };
        let mut once = flat(512);
        let mut twice = flat(512);
        let base = f64::from(once.base());
        let there = points((0.0, 0.0), (8.0, 0.0), 17);
        let mut back = there.clone();
        back.extend(there.iter().rev().skip(1));
        paint(&mut once, spec, &there, RADIUS, 1.0);
        paint(&mut twice, spec, &back, RADIUS, 1.0);
        let most = (1.0 + DROP_CHANGE) * spec.drop_m * LEVELS_PER_M + 1.0;
        let lowest = levels(&twice).into_iter().min().unwrap_or(0);
        assert!(base - f64::from(lowest) <= most, "{lowest} below {base}");
        // Away from the turn, the first pass gives each place.
        let n = once.face_size();
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    let p = once.texel_dir(face, x, y);
                    if angle(p, lonlat_to_dir(4.0, 0.0)) > 0.06 {
                        continue;
                    }
                    let (a, b) = (
                        once.get(face, x as i64, y as i64),
                        twice.get(face, x as i64, y as i64),
                    );
                    assert_eq!(a, b, "a second drop");
                }
            }
        }
    }

    #[test]
    fn the_change_reaches_past_the_brush_by_one_radius_at_most() {
        let spec = Scarp {
            steps: 5,
            scars: true,
            width_km: 300.0,
            ..spec()
        };
        let mut map = flat(512);
        let base = map.base();
        let line = points((0.0, 0.0), (10.0, 3.0), 11);
        paint(&mut map, spec, &line, RADIUS, 1.0);
        let fine = points((0.0, 0.0), (10.0, 3.0), 600);
        let n = map.face_size();
        let mut changed = 0;
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    if map.get(face, x as i64, y as i64) == base {
                        continue;
                    }
                    changed += 1;
                    let p = map.texel_dir(face, x, y);
                    let near = fine.iter().map(|&c| angle(c, p)).fold(f64::MAX, f64::min);
                    assert!(near < 2.0 * RADIUS, "a change at {near}");
                }
            }
        }
        assert!(changed > 1500, "{changed} texels changed");
    }

    #[test]
    fn equal_strokes_give_equal_levels_and_one_undo_puts_back_the_ground() {
        let spec = Scarp {
            steps: 3,
            scars: true,
            ..spec()
        };
        let mut a = flat(512);
        let mut b = flat(512);
        let before = levels(&a);
        let line = points((0.0, 0.0), (3.0, 1.0), 7);
        paint(&mut a, spec, &line, RADIUS, 0.8);
        paint(&mut b, spec, &line, RADIUS, 0.8);
        assert_eq!(levels(&a), levels(&b));
        assert_ne!(levels(&a), before);
        assert!(a.undo());
        assert_eq!(levels(&a), before);
    }
}
