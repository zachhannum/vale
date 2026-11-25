//! The abyssal hills brush.
//!
//! The brush paints the abyssal hills of the Ocean ridge flank on any sea
//! floor. It is a line brush, as the Ocean ridge brush is: the pen draws a
//! line, and each texel near the line gets a place on it, the distance
//! along the line and the distance across it. The hills are a function of
//! that place, so the faults run along the stroke and bend with it. A plan
//! lays the hills on the ground from before the stroke, so a later plan can
//! lay the part near the new points again.

use std::f64::consts::FRAC_PI_2;
use std::ops::Deref;

use crate::heightmap::{Limits, MAX_BRUSH_RADIUS, falloff};
use crate::line::{LinePlan, LinePoint, REACH, StrokeLine, TAPER, smoothstep};
use crate::math::V3;
use crate::ridge::{
    Faulting, LEVELS_PER_KM, RidgePlan, TAPER_WIDTH, fit_of, patch_noise, patch_of,
};

/// The settings of an abyssal hills stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AbyssalHills {
    /// The full spreading rate of the floor, in millimeters per year. It
    /// sets the height, the width, the spacing, and the length of the
    /// hills, as on the flank of an ocean ridge.
    pub rate: f64,
    /// The height of the hills, from 0 to 2. At 1 it is the height of real
    /// sea floor at the spreading rate.
    pub hills: f64,
    /// The radius of the world, in kilometers.
    pub world_km: f64,
    /// The seed of the hills.
    pub seed: u32,
    pub limits: Limits,
    /// Whether the hills grow to the texel size of the map, as for
    /// `Ridge::fit`.
    pub fit: bool,
    /// The size of the hills, from 1 to 10 times their real size, as for
    /// `Ridge::scale`.
    pub scale: f64,
}

/// The texels that new points of an abyssal hills stroke change, and the
/// segments of the line that give the levels there.
#[derive(Clone, Debug)]
pub struct AbyssalPlan {
    /// The settings, inside their limits.
    pub spec: AbyssalHills,
    /// The brush radius, in radians: half of the width of the hills.
    pub radius: f64,
    /// The part of the radius with the full height.
    pub hardness: f64,
    pub line: LinePlan,
    pub(crate) faulting: Faulting,
}

impl Deref for AbyssalPlan {
    type Target = LinePlan;

    fn deref(&self) -> &LinePlan {
        &self.line
    }
}

/// The line of one abyssal hills stroke.
#[derive(Clone, Debug)]
pub struct AbyssalStroke {
    spec: AbyssalHills,
    face_size: usize,
    line: Option<StrokeLine>,
    hardness: f64,
    /// The number of points that the plans so far cover.
    from: usize,
}

impl AbyssalStroke {
    /// Starts a stroke on a heightmap with `face_size` texels along one side
    /// of a face.
    pub fn new(spec: AbyssalHills, face_size: usize) -> AbyssalStroke {
        let spec = AbyssalHills {
            rate: spec.rate.max(1.0),
            hills: spec.hills.clamp(0.0, 2.0),
            world_km: spec.world_km.max(1.0),
            scale: spec.scale.clamp(1.0, 10.0),
            ..spec
        };
        AbyssalStroke {
            spec,
            face_size,
            line: None,
            hardness: 0.0,
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
    /// hardness of the stroke. The flow sets the height of the hills along
    /// the line. The hills show once the line has two points.
    pub fn stamp(&mut self, center: V3, radius: f64, hardness: f64, flow: f64) -> AbyssalPlan {
        self.line_of(radius, hardness)
            .push(center, flow.clamp(0.0, 1.0));
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
    ) -> Vec<AbyssalPlan> {
        let line = self.line_of(radius, hardness);
        for &center in centers {
            line.push(center, flow.clamp(0.0, 1.0));
        }
        let mut plans = vec![self.next_plan()];
        while self.from < self.len() {
            plans.push(self.next_plan());
        }
        plans
    }

    /// The line of the stroke. The first call makes it.
    fn line_of(&mut self, radius: f64, hardness: f64) -> &mut StrokeLine {
        let n = self.face_size;
        let step = FRAC_PI_2 / n as f64;
        if self.line.is_none() {
            let radius = radius.max(4.0 * step).min(MAX_BRUSH_RADIUS / REACH);
            self.hardness = hardness.clamp(0.0, 0.999);
            self.line = Some(StrokeLine::new(radius, n));
        }
        self.line.as_mut().expect("the line is there")
    }

    /// The plan of the points from the end of the plans before it.
    fn next_plan(&mut self) -> AbyssalPlan {
        let line = self.line.as_ref().expect("a stamp makes the line");
        let plan = line.plan(self.from);
        self.from = plan.end;
        let km = self.spec.world_km;
        let half_texel = 0.5 * FRAC_PI_2 / self.face_size as f64 * km;
        let faulting = Faulting::new(
            self.spec.rate,
            0.0,
            self.spec.hills,
            [
                half_texel,
                fit_of(half_texel, self.spec.fit, self.spec.scale),
            ],
            LEVELS_PER_KM,
        );
        AbyssalPlan {
            spec: self.spec,
            radius: line.radius(),
            hardness: self.hardness,
            line: plan,
            faulting,
        }
    }
}

impl AbyssalPlan {
    /// Whether the plan changes nothing.
    pub fn is_empty(&self) -> bool {
        self.line.rects.iter().all(Option::is_none)
    }

    /// The height of the hills above their local mean, in 16-bit steps, at
    /// a place on the line and its unit direction `p`.
    pub(crate) fn hills(&self, at: LinePoint, p: V3) -> f64 {
        let f = &self.faulting;
        let (seed, km) = (self.spec.seed, self.spec.world_km);
        let place = [at.u * km, at.across * km];
        let patch = patch_of(patch_noise(p, km / f.fit.sqrt(), seed));
        let height = RidgePlan::hills(seed, f, 0, 0, place, patch)
            + RidgePlan::texture(seed, f, 0, 0, place, patch[0])
            - f.hill_mean * patch[0] * patch[1];
        height * f.level
    }

    /// The weight of the hills at a place on the line: the flow, times the
    /// falloff of the brush across the line, times the taper near an end.
    /// Near an end, the hills are also narrower.
    pub(crate) fn weight(&self, at: LinePoint) -> f64 {
        let r = self.radius;
        let taper = smoothstep(0.0, TAPER * r, at.end);
        let width = r * (TAPER_WIDTH + (1.0 - TAPER_WIDTH) * taper);
        let d = at.across.hypot(at.past) / width;
        if d >= 1.0 {
            return 0.0;
        }
        at.flow * taper * falloff(d, self.hardness)
    }

    /// The new level of a texel with the level `start` from before the
    /// stroke, at a place on the line and its unit direction `p`.
    pub(crate) fn level(&self, start: u16, at: LinePoint, p: V3) -> u16 {
        let weight = self.weight(at);
        if weight <= 0.0 {
            return start;
        }
        let change = self.hills(at, p) * weight;
        let old = f64::from(start);
        let new = (old + (change + 0.5).floor()).clamp(0.0, 65535.0);
        self.spec.limits.apply(old, new) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::{FACES, meters_to_level};
    use crate::heightmap::Heightmap;
    use crate::math::{angle, dot, lonlat_to_dir, normalize};
    use crate::seamount::sea_floor;

    const WORLD_KM: f64 = 1000.0;

    fn spec() -> AbyssalHills {
        AbyssalHills {
            rate: 110.0,
            hills: 1.0,
            world_km: WORLD_KM,
            seed: 3,
            limits: Limits::default(),
            fit: false,
            scale: 1.0,
        }
    }

    fn flat(n: usize) -> Heightmap {
        Heightmap::new(n, meters_to_level(-4000.0))
    }

    /// Paints a stroke through `points` in one undo step.
    fn paint(map: &mut Heightmap, spec: AbyssalHills, points: &[V3], radius: f64, flow: f64) {
        let mut stroke = AbyssalStroke::new(spec, map.face_size());
        map.begin_stroke();
        for &p in points {
            let plan = stroke.stamp(p, radius, 0.5, flow);
            map.abyssal(&plan);
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

    #[test]
    fn the_hills_appear_under_the_brush_and_not_past_it() {
        let (n, radius) = (512, 0.05);
        let mut map = flat(n);
        let base = map.base();
        let line = points((0.0, 0.0), (12.0, 0.0), 13);
        paint(&mut map, spec(), &line, radius, 1.0);
        let fine = points((0.0, 0.0), (12.0, 0.0), 600);
        let mut changed = 0;
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    let level = map.get(face, x as i64, y as i64);
                    let p = map.texel_dir(face, x, y);
                    let near = fine.iter().map(|&c| angle(c, p)).fold(f64::MAX, f64::min);
                    if near >= radius {
                        assert_eq!(level, base, "a texel past the brush");
                    }
                    changed += usize::from(level != base);
                }
            }
        }
        assert!(changed > 1500, "{changed} texels changed");
    }

    /// The points of an arc of radius 0.15 radians around the place of
    /// longitude and latitude 0, at angles in degrees.
    fn arc(angles: impl Iterator<Item = f64>) -> Vec<V3> {
        angles
            .map(|t| {
                let t = t.to_radians();
                normalize([1.0, 0.15 * t.cos(), 0.15 * t.sin()])
            })
            .collect()
    }

    #[test]
    fn the_faults_follow_a_curved_stroke() {
        let mut map = flat(1024);
        let line = arc((0..=36).map(|i| f64::from(i) * 5.0));
        paint(
            &mut map,
            AbyssalHills {
                fit: true,
                ..spec()
            },
            &line,
            0.04,
            1.0,
        );
        for t in [30.0_f64, 60.0, 90.0, 120.0, 150.0] {
            let p = arc([t].into_iter())[0];
            let tangent = normalize([0.0, -t.to_radians().sin(), t.to_radians().cos()]);
            let floor = sea_floor(&map, p, 0.02);
            let along = dot(floor.fabric, tangent).abs();
            assert!(
                floor.coherence > 0.3,
                "at {t}: a coherence of {}",
                floor.coherence
            );
            assert!(along > 0.9, "at {t}: the fabric is {along} along the line");
        }
    }

    /// The largest change of the level between two texels side by side on
    /// a great circle from `a` to `b`, in steps of a tenth of a texel.
    fn largest_step(map: &Heightmap, a: V3, b: V3) -> u16 {
        let steps = (angle(a, b) / (0.1 * FRAC_PI_2 / map.face_size() as f64)) as usize;
        let mut last = map.sample(a);
        let mut worst = 0;
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let p = normalize([
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]);
            let level = map.sample(p);
            worst = worst.max(level.abs_diff(last));
            last = level;
        }
        worst
    }

    #[test]
    fn a_sharp_bend_has_no_seam() {
        let mut map = flat(1024);
        // East along the equator, then north at longitude 5.
        let mut line = points((0.0, 0.0), (5.0, 0.0), 11);
        line.extend(points((5.0, 0.5), (5.0, 5.0), 10));
        paint(
            &mut map,
            AbyssalHills {
                fit: true,
                ..spec()
            },
            &line,
            0.04,
            1.0,
        );
        // Across the straight part, and through the corner from the inside
        // to the outside.
        let straight = largest_step(&map, lonlat_to_dir(2.5, -2.0), lonlat_to_dir(2.5, 2.0));
        let corner = largest_step(&map, lonlat_to_dir(3.5, 1.5), lonlat_to_dir(6.5, -1.5));
        assert!(straight > 0, "no hills");
        assert!(
            corner <= 2 * straight + 2,
            "a step of {corner} at the corner, {straight} on the straight part"
        );
    }

    #[test]
    fn the_mean_level_stays_near_the_start() {
        let mut map = flat(512);
        let base = f64::from(map.base());
        paint(
            &mut map,
            spec(),
            &points((0.0, 0.0), (16.0, 0.0), 17),
            0.05,
            1.0,
        );
        let (mut sum, mut squares, mut count) = (0.0, 0.0, 0.0);
        for face in 0..FACES {
            for y in 0..512 {
                for x in 0..512 {
                    let p = map.texel_dir(face, x, y);
                    // The middle of the stroke, where the hills are full.
                    if angle(p, lonlat_to_dir(8.0, 0.0)) > 0.045 {
                        continue;
                    }
                    let d = f64::from(map.get(face, x as i64, y as i64)) - base;
                    (sum, squares, count) = (sum + d, squares + d * d, count + 1.0);
                }
            }
        }
        let mean = sum / count;
        let rms = (squares / count - mean * mean).sqrt();
        assert!(rms > 50.0, "an RMS of {rms} levels");
        assert!(
            mean.abs() < 0.3 * rms,
            "a mean of {mean} levels, an RMS of {rms}"
        );
    }

    #[test]
    fn equal_strokes_give_equal_levels_and_one_undo_puts_back_the_ground() {
        let mut a = flat(512);
        let mut b = flat(512);
        let before = levels(&a);
        let line = points((0.0, 0.0), (3.0, 1.0), 7);
        paint(&mut a, spec(), &line, 0.05, 0.8);
        paint(&mut b, spec(), &line, 0.05, 0.8);
        assert_eq!(levels(&a), levels(&b));
        assert_ne!(levels(&a), before);
        assert!(a.undo());
        assert_eq!(levels(&a), before);
    }

    #[test]
    fn the_hills_setting_scales_the_hills() {
        let plan = |hills: f64| {
            let mut stroke = AbyssalStroke::new(AbyssalHills { hills, ..spec() }, 512);
            stroke.stamp(lonlat_to_dir(0.0, 0.0), 0.05, 0.5, 1.0);
            stroke.stamp(lonlat_to_dir(1.0, 0.0), 0.05, 0.5, 1.0)
        };
        let (none, one, two) = (plan(0.0), plan(1.0), plan(2.0));
        for i in 0..50 {
            let at = LinePoint {
                u: 0.0003 * f64::from(i),
                across: 0.01,
                past: 0.0,
                flow: 1.0,
                end: 1.0,
            };
            let p = lonlat_to_dir(0.02 * f64::from(i), 0.5);
            assert_eq!(none.hills(at, p), 0.0);
            assert!((two.hills(at, p) - 2.0 * one.hills(at, p)).abs() < 1e-9);
        }
    }
}
