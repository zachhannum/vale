//! The abyssal hills brush.
//!
//! The brush paints the abyssal hills of the Ocean ridge flank on any sea
//! floor. A stroke has one frame: an origin and the direction of the fabric
//! there. The hills are a function of the place in that frame, so the
//! stamps of a stroke paint one field. Each stamp moves a texel toward its
//! level from before the stroke plus the hills, as the Paint mode moves it
//! toward its target.

use std::f64::consts::FRAC_PI_2;

use crate::channel::ChannelShape;
use crate::cube::FACES;
use crate::heightmap::{Heightmap, Limits, MAX_BRUSH_RADIUS, Mode, Stamp, TexelRect, brush_rect};
use crate::math::{V3, angle, cross, dot, length, normalize};
use crate::ridge::{Faulting, LEVELS_PER_KM, RidgePlan, fit_of, patch_noise, patch_of};
use crate::seamount::{FABRIC_COHERENCE, SeaFloor, sea_floor};

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

/// The texels that one stamp of an abyssal hills stroke changes.
#[derive(Clone, Debug)]
pub struct AbyssalPlan {
    /// The brush of the stamp: its center, radius, hardness, flow, and
    /// limits. Its mode is `Mode::AbyssalHills`.
    pub stamp: Stamp,
    /// The number of texels along one side of a face.
    pub face_size: usize,
    /// The rectangle of each face that changes. A stamp before the frame of
    /// the stroke has none.
    pub rects: [Option<TexelRect>; FACES],
    /// The origin of the frame and the direction of the fabric there, unit
    /// vectors.
    pub frame: [V3; 2],
    pub(crate) faulting: Faulting,
    pub(crate) world_km: f64,
}

/// The frame of one abyssal hills stroke.
#[derive(Clone, Debug)]
pub struct AbyssalStroke {
    spec: AbyssalHills,
    face_size: usize,
    floor: Option<SeaFloor>,
    first: Option<V3>,
    frame: Option<[V3; 2]>,
}

/// On floor with no fabric, a stamp sets the frame when the pen has moved
/// this part of the brush radius from the first stamp.
const MOTION: f64 = 0.1;

impl AbyssalStroke {
    /// Starts a stroke on a heightmap with `face_size` texels along one side
    /// of a face.
    pub fn new(spec: AbyssalHills, face_size: usize) -> AbyssalStroke {
        let spec = AbyssalHills {
            rate: spec.rate.max(1.0),
            hills: spec.hills.clamp(0.0, 2.0),
            world_km: spec.world_km.max(1.0),
            ..spec
        };
        AbyssalStroke {
            spec,
            face_size,
            floor: None,
            first: None,
            frame: None,
        }
    }

    /// The sea floor under the first stamp.
    pub fn floor(&self) -> Option<SeaFloor> {
        self.floor
    }

    /// The origin of the frame and the direction of the fabric there.
    pub fn frame(&self) -> Option<[V3; 2]> {
        self.frame
    }

    /// The plan of a stamp with its center at unit direction `center`, a
    /// radius in radians, a hardness, and a flow from 0 to 1. `map` holds
    /// the ground from before the stroke.
    ///
    /// The first stamp reads the fabric of the floor with `sea_floor`. With
    /// a clear fabric, the first stamp sets the frame. On floor with no
    /// fabric, the hills run in the direction of the stroke: the first stamp
    /// that is `MOTION` radii from the first stamp sets the frame, and the
    /// stamps before it change nothing.
    pub fn stamp(
        &mut self,
        map: &Heightmap,
        center: V3,
        radius: f64,
        hardness: f64,
        flow: f64,
    ) -> AbyssalPlan {
        let n = self.face_size;
        let step = FRAC_PI_2 / n as f64;
        let center = normalize(center);
        let radius = radius.clamp(2.0 * step, MAX_BRUSH_RADIUS);
        if self.frame.is_none() {
            let floor = *self
                .floor
                .get_or_insert_with(|| sea_floor(map, center, 3.0 * radius));
            if floor.coherence >= FABRIC_COHERENCE {
                self.frame = Some([center, along_at(center, floor.fabric)]);
            } else {
                let first = *self.first.get_or_insert(center);
                if angle(first, center) >= MOTION * radius {
                    let motion = [
                        center[0] - first[0],
                        center[1] - first[1],
                        center[2] - first[2],
                    ];
                    self.frame = Some([center, along_at(center, motion)]);
                }
            }
        }
        let km = self.spec.world_km;
        let half_texel = 0.5 * step * km;
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
        let rects = match self.frame {
            Some(_) => std::array::from_fn(|face| brush_rect(n, face, center, radius)),
            None => [None; FACES],
        };
        AbyssalPlan {
            stamp: Stamp {
                center,
                radius,
                hardness: hardness.clamp(0.0, 1.0),
                flow: flow.clamp(0.0, 1.0),
                mode: Mode::AbyssalHills,
                level: 0,
                strength: self.spec.hills,
                lower: 0.0,
                talus: 0.0,
                scale: 0.0,
                seed: self.spec.seed,
                motion: [0.0; 3],
                ridged: false,
                invert: false,
                relief: 0.0,
                grain: 0.0,
                channel: ChannelShape::default(),
                limits: self.spec.limits,
            },
            face_size: n,
            rects,
            frame: self
                .frame
                .unwrap_or([center, along_at(center, [0.0, 0.0, 1.0])]),
            faulting,
            world_km: km,
        }
    }
}

impl AbyssalPlan {
    /// Whether the plan changes nothing.
    pub fn is_empty(&self) -> bool {
        self.rects.iter().all(Option::is_none)
    }

    /// The place of a unit direction in the frame: the distances along the
    /// fabric and across it, in kilometers on the sphere.
    pub(crate) fn place(&self, p: V3) -> [f64; 2] {
        let [origin, along] = self.frame;
        let across = cross(origin, along);
        let (x, y, z) = (dot(p, along), dot(p, across), dot(p, origin));
        [
            x.atan2(z) * self.world_km,
            y.atan2(x.hypot(z)) * self.world_km,
        ]
    }

    /// The height of the hills at a unit direction above their local mean,
    /// in 16-bit steps.
    pub fn hills(&self, p: V3) -> f64 {
        let f = &self.faulting;
        let seed = self.stamp.seed;
        let at = self.place(p);
        let patch = patch_of(patch_noise(p, self.world_km / f.fit.sqrt(), seed));
        let height = RidgePlan::hills(seed, f, 0, 0, at, patch)
            + RidgePlan::texture(seed, f, 0, 0, at, patch[0])
            - f.hill_mean * patch[0] * patch[1];
        height * f.level
    }

    /// The level of a texel at unit direction `p`, with level `start` from
    /// before the stroke, level `old` now, and the effect `amount` of the
    /// brush there. The texel moves toward `start` plus the hills by the
    /// effect, and it does not go past that level.
    pub fn level(&self, start: u16, old: u16, amount: f64, p: V3) -> u16 {
        let target = f64::from(start) + self.hills(p);
        let old = f64::from(old);
        let new = (old + (target - old) * amount.min(1.0))
            .round()
            .clamp(0.0, 65535.0);
        self.stamp.limits.apply(old, new) as u16
    }
}

/// The unit vector square to `center` nearest to `v`. A `v` along
/// `center` gives north, or the x axis at a pole.
fn along_at(center: V3, v: V3) -> V3 {
    let k = dot(v, center);
    let t = [
        v[0] - center[0] * k,
        v[1] - center[1] * k,
        v[2] - center[2] * k,
    ];
    if length(t) > 1e-12 {
        return normalize(t);
    }
    let east = cross([0.0, 0.0, 1.0], center);
    if length(east) < 1e-12 {
        return [1.0, 0.0, 0.0];
    }
    normalize(cross(center, normalize(east)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::meters_to_level;
    use crate::math::lonlat_to_dir;
    use crate::ridge::{Ridge, RidgeLine};

    const N: usize = 512;
    const WORLD_KM: f64 = 1000.0;
    const RADIUS: f64 = 0.05;

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

    fn flat() -> Heightmap {
        Heightmap::new(N, meters_to_level(-4000.0))
    }

    /// The stamps of a stroke east along the equator.
    fn east(count: usize) -> Vec<V3> {
        (0..count)
            .map(|i| lonlat_to_dir(i as f64 * 0.5, 0.0))
            .collect()
    }

    /// Paints a stroke in one undo step. Returns the plans.
    fn paint(
        map: &mut Heightmap,
        spec: AbyssalHills,
        centers: &[V3],
        flow: f64,
    ) -> Vec<AbyssalPlan> {
        let start = map.snapshot();
        let mut stroke = AbyssalStroke::new(spec, N);
        map.begin_stroke();
        let plans: Vec<AbyssalPlan> = centers
            .iter()
            .map(|&c| {
                let plan = stroke.stamp(&start, c, RADIUS, 0.5, flow);
                map.abyssal(&plan);
                plan
            })
            .collect();
        map.end_stroke();
        plans
    }

    fn levels(map: &Heightmap) -> Vec<u16> {
        let mut out = Vec::new();
        for face in 0..FACES {
            for y in 0..N {
                for x in 0..N {
                    out.push(map.get(face, x as i64, y as i64));
                }
            }
        }
        out
    }

    #[test]
    fn the_hills_appear_under_the_brush_and_not_past_it() {
        let mut map = flat();
        let base = map.base();
        let centers = east(6);
        let plans = paint(&mut map, spec(), &centers, 1.0);
        // On flat floor, the first stamp waits for the direction of the
        // stroke.
        assert!(plans[0].is_empty());
        assert!(!plans[1].is_empty());
        let mut changed = 0;
        for face in 0..FACES {
            for y in 0..N {
                for x in 0..N {
                    let level = map.get(face, x as i64, y as i64);
                    let p = map.texel_dir(face, x, y);
                    let near = centers
                        .iter()
                        .map(|&c| angle(c, p))
                        .fold(f64::MAX, f64::min);
                    if near >= RADIUS {
                        assert_eq!(level, base, "a texel past the brush");
                    }
                    changed += usize::from(level != base);
                }
            }
        }
        assert!(changed > 800, "{changed} texels changed");
    }

    #[test]
    fn a_texel_moves_toward_its_target_and_never_past_it() {
        let mut map = flat();
        let start = map.snapshot();
        let mut stroke = AbyssalStroke::new(spec(), N);
        map.begin_stroke();
        stroke.stamp(&start, lonlat_to_dir(0.0, 0.0), RADIUS, 0.3, 0.6);
        let plan = stroke.stamp(&start, lonlat_to_dir(0.5, 0.0), RADIUS, 0.3, 0.6);
        assert!(!plan.is_empty());
        let rects: Vec<(usize, TexelRect)> = (0..FACES)
            .filter_map(|face| Some((face, plan.rects[face]?)))
            .collect();
        let read = |map: &Heightmap| {
            let mut out = Vec::new();
            for &(face, r) in &rects {
                for y in r.y0..r.y1 {
                    for x in r.x0..r.x1 {
                        out.push(map.get(face, x as i64, y as i64));
                    }
                }
            }
            out
        };
        let mut targets = Vec::new();
        for &(face, r) in &rects {
            for y in r.y0..r.y1 {
                for x in r.x0..r.x1 {
                    let p = map.texel_dir(face, x, y);
                    targets.push(f64::from(start.get(face, x as i64, y as i64)) + plan.hills(p));
                }
            }
        }
        let mut last = read(&map);
        for _ in 0..4 {
            map.abyssal(&plan);
            let now = read(&map);
            for ((&old, &new), &target) in last.iter().zip(&now).zip(&targets) {
                let (old, new) = (f64::from(old), f64::from(new));
                assert!((new - target).abs() <= (old - target).abs() + 0.5);
                assert!((new - target) * (old - target) >= -0.5);
            }
            last = now;
        }
        map.end_stroke();
    }

    #[test]
    fn the_mean_level_stays_near_the_start() {
        let mut map = flat();
        let base = f64::from(map.base());
        paint(&mut map, spec(), &east(12), 1.0);
        let (mut sum, mut squares, mut count) = (0.0, 0.0, 0.0);
        for face in 0..FACES {
            for y in 0..N {
                for x in 0..N {
                    let p = map.texel_dir(face, x, y);
                    // The middle of the stroke, where the hills are full.
                    if angle(p, lonlat_to_dir(2.75, 0.0)) > 0.04 {
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
    fn the_fabric_follows_the_floor_or_the_stroke() {
        // On flat floor, the hills run along the stroke.
        let map = flat();
        let mut stroke = AbyssalStroke::new(spec(), N);
        for c in east(3) {
            stroke.stamp(&map, c, RADIUS, 0.5, 1.0);
        }
        let [origin, along] = stroke.frame().unwrap();
        assert!(angle(origin, lonlat_to_dir(0.5, 0.0)) < 1e-9);
        assert!(dot(along, [0.0, 1.0, 0.0]) > 0.99, "{along:?}");
        // On the flank of a ridge along a meridian, the hills run north,
        // even for a stroke east.
        let mut ridge_map = flat();
        let ridge = Ridge {
            radius: 0.2,
            crest: meters_to_level(-2600.0),
            segment: 0.4,
            offset: 0.0,
            spread: 55.0 / WORLD_KM,
            magma: 0.0,
            world_km: WORLD_KM,
            seed: 1,
            limits: Limits::default(),
            hills: 0.0,
            fit: false,
            scale: 1.0,
        };
        let mut line = RidgeLine::new(ridge, N);
        for i in 0..=16 {
            line.push(lonlat_to_dir(5.0, -30.0 + 60.0 * f64::from(i) / 16.0), 1.0);
        }
        ridge_map.begin_stroke();
        let mut from = 0;
        while from < line.len() {
            let plan = line.plan(from);
            ridge_map.ridge(&plan);
            from = plan.end;
        }
        ridge_map.end_stroke();
        let mut stroke = AbyssalStroke::new(spec(), N);
        let plan = stroke.stamp(&ridge_map, lonlat_to_dir(0.0, 0.0), RADIUS, 0.5, 1.0);
        assert!(!plan.is_empty());
        assert!(stroke.floor().unwrap().coherence >= FABRIC_COHERENCE);
        let [_, along] = stroke.frame().unwrap();
        assert!(dot(along, [0.0, 0.0, 1.0]).abs() > 0.95, "{along:?}");
    }

    #[test]
    fn equal_strokes_give_equal_levels_and_one_undo_puts_back_the_ground() {
        let mut a = flat();
        let mut b = flat();
        let before = levels(&a);
        paint(&mut a, spec(), &east(5), 0.8);
        paint(&mut b, spec(), &east(5), 0.8);
        assert_eq!(levels(&a), levels(&b));
        assert_ne!(levels(&a), before);
        assert!(a.undo());
        assert_eq!(levels(&a), before);
    }

    #[test]
    fn the_hills_setting_scales_the_hills() {
        let plan = |hills: f64| {
            let map = flat();
            let mut stroke = AbyssalStroke::new(AbyssalHills { hills, ..spec() }, N);
            stroke.stamp(&map, lonlat_to_dir(0.0, 0.0), RADIUS, 0.5, 1.0);
            stroke.stamp(&map, lonlat_to_dir(0.5, 0.0), RADIUS, 0.5, 1.0)
        };
        let (none, one, two) = (plan(0.0), plan(1.0), plan(2.0));
        for i in 0..50 {
            let p = lonlat_to_dir(0.5 + 0.02 * f64::from(i), 0.3);
            assert_eq!(none.hills(p), 0.0);
            assert!((two.hills(p) - 2.0 * one.hills(p)).abs() < 1e-9);
        }
    }
}
