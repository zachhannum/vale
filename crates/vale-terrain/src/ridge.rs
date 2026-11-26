//! The ocean ridge brush.
//!
//! The brush works on the whole line of a stroke. Each texel near the line
//! gets a place: the distance along the line and the distance across it. The
//! new level is a function of that place, of noise at the direction of the
//! texel, and of the level from before the stroke. Thus a texel takes the
//! ridge one time for each stroke.

use std::f64::consts::{FRAC_PI_2, TAU};
use std::ops::Deref;
use std::sync::Arc;

use crate::cube::{ELEV_MAX, ELEV_MIN, FACES};
use crate::heightmap::{Limits, hash, noise3};
use crate::line::{
    LinePlace, LinePlan, LinePoint, REACH, StrokeLine, TAPER, bump, noise1, smoothstep, unit,
};
use crate::math::{V3, add, scale};

/// The largest radius of an ocean ridge stroke, in radians. On a world of
/// the size of the Earth, the ridge is up to 3,060 km wide.
pub const MAX_RIDGE_RADIUS: f64 = 0.24;

/// The most drawn fracture zones of one ocean ridge line.
pub const MAX_RIDGE_GUIDES: usize = 16;

/// The settings of an ocean ridge stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ridge {
    /// The brush radius as an angle, in radians. The rise ends at this
    /// distance from the crest.
    pub radius: f64,
    /// The level of the crest.
    pub crest: u16,
    /// The mean length of a segment of the crest as an angle, in radians.
    pub segment: f64,
    /// The largest shift of a segment across the line as an angle, in
    /// radians.
    pub offset: f64,
    /// Half of the spreading rate, in radians per million years.
    pub spread: f64,
    /// The change of the magma part of the spreading from its value at the
    /// spreading rate. A higher part gives lower faults that are closer
    /// together.
    pub magma: f64,
    /// The radius of the world, in kilometers.
    pub world_km: f64,
    /// The seed of the pattern.
    pub seed: u32,
    pub limits: Limits,
    /// The height of the abyssal hills, from 0 to 2. At 1 it is the height
    /// of real sea floor at the spreading rate.
    pub hills: f64,
    /// Whether the abyssal hills grow to the texel size of the map, so they
    /// show on a coarse map. See `fit_of`.
    pub fit: bool,
    /// The size of the abyssal hills, from 1 to 10 times their real size.
    pub scale: f64,
}

/// The line of one ocean ridge stroke.
#[derive(Clone, Debug)]
pub struct RidgeLine {
    spec: Ridge,
    /// The 16-bit steps of one kilometer of relief. The tests set it to
    /// zero for the rise alone.
    relief: f64,
    line: StrokeLine,
    /// The drawn fracture zones, as unit directions in draw order.
    strokes: Vec<Vec<V3>>,
    guides: Arc<Guides>,
}

/// The part of the heightmap that changes after new points of a line, and
/// the segments that give the levels there.
#[derive(Clone, Debug)]
pub struct RidgePlan {
    /// The settings, inside their limits.
    pub ridge: Ridge,
    /// The 16-bit steps of one kilometer of relief.
    pub(crate) relief: f64,
    pub line: LinePlan,
    /// The drawn fracture zones.
    pub(crate) guides: Arc<Guides>,
}

impl Deref for RidgePlan {
    type Target = LinePlan;

    fn deref(&self) -> &LinePlan {
        &self.line
    }
}

impl RidgeLine {
    /// Starts a line with no points on a heightmap with `face_size` texels
    /// along one side of a face. The settings go inside their limits.
    pub fn new(spec: Ridge, face_size: usize) -> RidgeLine {
        let step = FRAC_PI_2 / face_size as f64;
        let radius = spec.radius.max(4.0 * step).min(MAX_RIDGE_RADIUS);
        let spec = Ridge {
            radius,
            segment: spec
                .segment
                .max(SEGMENT_BLENDS * blend_width(step, spec.world_km.max(1.0))),
            offset: spec.offset.clamp(0.0, 0.5 * radius),
            spread: spec.spread.max(1e-12),
            world_km: spec.world_km.max(1.0),
            ..spec
        };
        RidgeLine {
            spec,
            relief: LEVELS_PER_KM,
            line: StrokeLine::new(radius, face_size),
            strokes: Vec::new(),
            guides: Arc::default(),
        }
    }

    /// The settings, inside their limits.
    pub fn spec(&self) -> Ridge {
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
    pub fn plan(&self, from: usize) -> RidgePlan {
        self.wrap(self.line.plan(from))
    }

    /// As `StrokeLine::margin`.
    #[doc(hidden)]
    pub fn margin(&self, p: V3) -> f64 {
        self.line.margin(p)
    }

    #[cfg(test)]
    fn plan_of(&self, start: usize, stop: usize) -> RidgePlan {
        self.wrap(self.line.plan_of(start, stop))
    }

    fn wrap(&self, line: LinePlan) -> RidgePlan {
        RidgePlan {
            ridge: self.spec,
            relief: self.relief,
            line,
            guides: self.guides.clone(),
        }
    }

    /// Adds a drawn fracture zone: the unit directions of a stroke across
    /// the line, in draw order. Its crossing with the line becomes a
    /// transform fault, and the trace of that transform fault follows the
    /// stroke. The traces between two drawn fracture zones bend with them.
    /// Add the points of the line first.
    ///
    /// Returns `false` and changes nothing if the stroke does not cross the
    /// line inside the reach of the brush, or does not start or end near
    /// the line. A drawn fracture zone at the boundary of an older one
    /// takes its place. Past `MAX_RIDGE_GUIDES`, the oldest one goes.
    pub fn add_guide(&mut self, points: &[V3]) -> bool {
        let bare = self.bare(Arc::default());
        if self.drawn(&bare, points).is_none() {
            return false;
        }
        self.strokes.push(points.to_vec());
        if self.strokes.len() > MAX_RIDGE_GUIDES {
            self.strokes.remove(0);
        }
        self.guides = Arc::new(self.derive());
        true
    }

    /// Removes the last drawn fracture zone. Returns `false` if the line
    /// has none.
    pub fn pop_guide(&mut self) -> bool {
        if self.strokes.pop().is_none() {
            return false;
        }
        self.guides = Arc::new(self.derive());
        true
    }

    /// The strokes of the drawn fracture zones, in draw order.
    pub fn guide_strokes(&self) -> &[Vec<V3>] {
        &self.strokes
    }

    /// A plan with no segments and the guides `guides`, for the numbers of
    /// the settings.
    fn bare(&self, guides: Arc<Guides>) -> RidgePlan {
        RidgePlan {
            ridge: self.spec,
            relief: self.relief,
            line: LinePlan {
                face_size: self.line.face_size(),
                radius: self.line.radius(),
                rects: [None; FACES],
                segments: Vec::new(),
                tail: 0..0,
                end: 0,
                length: self.line.length(),
            },
            guides,
        }
    }

    /// The places of the points of a stroke on the line, as the bent
    /// distance along the line and across it. A point that the brush does
    /// not reach, or that is past an end of the line, is left out.
    fn kept(&self, bare: &RidgePlan, points: &[V3]) -> Vec<(f64, f64)> {
        let r = self.spec.radius;
        let mut kept = Vec::new();
        for &p in points {
            let (distance, u) = self.line.nearest(p);
            if distance >= REACH * r {
                continue;
            }
            let LinePlace::On(at) = self.line.plan_between(u - r..u + r).place(p) else {
                continue;
            };
            if at.past > 0.0 {
                continue;
            }
            kept.push(bare.bent_place(at, bare.noise(p)));
        }
        kept
    }

    /// The fracture zone of a stroke, before the snap to a boundary.
    fn drawn(&self, bare: &RidgePlan, points: &[V3]) -> Option<Drawn> {
        let r = self.spec.radius;
        let kept = self.kept(bare, points);
        if kept.len() < 2 {
            return None;
        }
        // The crossing: a place on the polyline, and the index of the first
        // point past it.
        let mut crossing = None;
        for (i, w) in kept.windows(2).enumerate() {
            let ((u0, a0), (u1, a1)) = (w[0], w[1]);
            if a0 * a1 <= 0.0 && a0 != a1 {
                let t = a0 / (a0 - a1);
                crossing = Some((u0 + t * (u1 - u0), i + 1));
                break;
            }
        }
        let n = kept.len();
        let crossing = crossing.or_else(|| {
            if kept[0].1.abs() < 0.1 * r {
                Some((kept[0].0, 0))
            } else if kept[n - 1].1.abs() < 0.1 * r {
                Some((kept[n - 1].0, n))
            } else {
                None
            }
        })?;
        let (u0, split) = crossing;
        if u0 <= 0.0 || u0 >= self.line.length() {
            return None;
        }
        let after: Vec<(f64, f64)> = kept[split..].to_vec();
        let before: Vec<(f64, f64)> = kept[..split].iter().rev().copied().collect();
        // The move along the line at the distances across it of each side.
        let mut sides: [Option<Vec<(f64, f64)>>; 2] = [None, None];
        for half in [after, before] {
            let Some(&(_, first)) = half.iter().find(|(_, a)| *a != 0.0) else {
                continue;
            };
            let side = usize::from(first < 0.0);
            let mut walk = vec![(0.0, 0.0)];
            for (u, a) in half {
                if (a < 0.0) == (side == 1) && a.abs() > walk[walk.len() - 1].0 {
                    walk.push((a.abs(), u - u0));
                }
            }
            if sides[side].is_none() {
                sides[side] = Some(walk);
            }
        }
        let reaches = |walk: &Option<Vec<(f64, f64)>>| {
            walk.as_ref()
                .is_some_and(|walk| walk[walk.len() - 1].0 >= 0.05 * r)
        };
        let h = REACH * r / (GUIDE_SAMPLES - 1) as f64;
        let mut m = [[0.0; GUIDE_SAMPLES]; 2];
        match (reaches(&sides[0]), reaches(&sides[1])) {
            (false, false) => return None,
            (left, right) => {
                for side in 0..2 {
                    if [left, right][side] {
                        m[side] = samples(sides[side].as_ref()?, h, r);
                    }
                }
                // A stroke on one side goes on straight across the line.
                if !left {
                    m[0] = m[1].map(|x| -x);
                } else if !right {
                    m[1] = m[0].map(|x| -x);
                }
            }
        }
        Some(Drawn { u0, m })
    }

    /// The guides of the strokes.
    fn derive(&self) -> Guides {
        let bare = self.bare(Arc::default());
        let sg = bare.segmenting();
        let mut list: Vec<Guide> = Vec::new();
        for (order, points) in self.strokes.iter().enumerate() {
            let Some(drawn) = self.drawn(&bare, points) else {
                continue;
            };
            // The guide snaps to the nearest boundary.
            let near = (drawn.u0 / sg.length).floor() as i32;
            let j = (near - 1..=near + 2)
                .min_by(|&a, &b| {
                    let da = (bare.boundary(&sg, a) - drawn.u0).abs();
                    let db = (bare.boundary(&sg, b) - drawn.u0).abs();
                    da.total_cmp(&db)
                })
                .unwrap_or(near);
            list.retain(|guide| guide.j != j);
            list.push(Guide {
                j,
                u0: bare.boundary(&sg, j),
                drawn: drawn.u0,
                m: drawn.m,
                order,
                part: 0.0,
            });
        }
        list.sort_by_key(|guide| guide.j);
        // The traces of two guides do not cross: they keep a quarter of
        // their distance at the crest. The guide drawn later moves.
        for i in 1..list.len() {
            let (head, tail) = list.split_at_mut(i);
            let (a, b) = (&mut head[i - 1], &mut tail[0]);
            let d = b.u0 - a.u0;
            for side in 0..2 {
                for k in 0..GUIDE_SAMPLES {
                    if b.u0 + b.m[side][k] - (a.u0 + a.m[side][k]) < 0.25 * d {
                        if b.order > a.order {
                            b.m[side][k] = a.m[side][k] - 0.75 * d;
                        } else {
                            a.m[side][k] = b.m[side][k] + 0.75 * d;
                        }
                    }
                }
            }
        }
        // The seeded transform faults near a guide are plain offsets.
        let mut excluded = Vec::new();
        let half = 0.5 * self.spec.segment;
        for guide in &list {
            let from = ((guide.u0 - half) / sg.length).floor() as i32 - 2;
            let to = ((guide.u0 + half) / sg.length).floor() as i32 + 2;
            for s in from..=to {
                let near = s == guide.j || (bare.boundary(&sg, s) - guide.u0).abs() < half;
                if near && bare.seeded_transform(&sg, s) {
                    excluded.push(s);
                }
            }
        }
        excluded.sort_unstable();
        excluded.dedup();
        excluded.truncate(GUIDE_EXCLUDED);
        // The group of a guide has a shift to the other side of the crest
        // from the group before it, so the offset at the guide is large.
        let step = REACH * self.spec.radius / (GUIDE_SAMPLES - 1) as f64;
        let mut guides = Guides {
            list,
            excluded,
            step,
        };
        for i in 0..guides.list.len() {
            let plan = self.bare(Arc::new(guides.clone()));
            let j = guides.list[i].j;
            let before = plan.group_part(plan.group(&sg, j - 1));
            let size = 0.5 + 0.5 * unit(hash(j as u32, 6, 0, self.spec.seed));
            guides.list[i].part = if before < 0.0 { size } else { -size };
        }
        guides
    }
}

/// The move along the line of one side of a drawn fracture zone at each
/// sample. `walk` holds the distance across the line and the move, with the
/// distance across the line growing. `h` is the step of the samples.
fn samples(walk: &[(f64, f64)], h: f64, r: f64) -> [f64; GUIDE_SAMPLES] {
    let (last_a, last_m) = walk[walk.len() - 1];
    // Past the stroke, the move goes on straight.
    let back = walk
        .iter()
        .rev()
        .find(|(a, _)| *a <= last_a - 0.15 * r)
        .unwrap_or(&walk[0]);
    let slope = if last_a > back.0 {
        ((last_m - back.1) / (last_a - back.0)).clamp(-GUIDE_SLOPE, GUIDE_SLOPE)
    } else {
        0.0
    };
    std::array::from_fn(|k| {
        let a = k as f64 * h;
        if k == 0 {
            0.0
        } else if a >= last_a {
            last_m + slope * (a - last_a)
        } else {
            let i = walk.partition_point(|(x, _)| *x <= a);
            let ((a0, m0), (a1, m1)) = (walk[i - 1], walk[i]);
            m0 + (m1 - m0) * (a - a0) / (a1 - a0)
        }
    })
}

impl RidgePlan {
    /// The noise at a unit direction.
    pub(crate) fn noise(&self, p: V3) -> RidgeNoise {
        let (r, seed) = (self.ridge.radius, self.ridge.seed);
        let at = |size: f64, salt: u32| noise3(p.map(|c| c / (size * r)), seed ^ salt);
        let (wide, fine) = (at(BEND_SIZE, SALT_BEND), at(BEND_FINE_SIZE, SALT_BEND_FINE));
        // The ragged seams grow with the hills.
        let km = self.ridge.world_km / self.fit();
        let seam = |size: f64| noise3(p.map(|c| c * km / size), seed ^ SALT_SEAM)[1];
        RidgeNoise {
            bend: add(wide, scale(fine, 0.5)),
            slow: at(SLOW_SIZE, SALT_SLOW),
            patch: patch_noise(p, self.ridge.world_km / self.fit().sqrt(), seed),
            seam: seam(SEAM_SIZE_KM) + 0.5 * seam(0.5 * SEAM_SIZE_KM),
        }
    }

    /// The fit factor of the hills.
    fn fit(&self) -> f64 {
        let spec = &self.ridge;
        fit_of(0.5 * self.step() * spec.world_km, spec.fit, spec.scale)
    }

    /// The crest segments of the settings.
    fn segmenting(&self) -> Segmenting {
        let spec = &self.ridge;
        let km = spec.world_km;
        let blend = blend_width(self.step(), km);
        // A crest segment is long enough for the blends at its two ends.
        let most = (spec.segment / (SEGMENT_BLENDS * blend)).floor().max(1.0);
        let full_rate = 2.0 * spec.spread * km;
        let count = (spec.segment * km / lerp_table(&SECOND_ORDER_LENGTH, full_rate))
            .round()
            .clamp(1.0, most);
        // The loop of `turn` has no more than `TURN_CELLS` cells.
        let oldest = TURN_REACH * spec.radius / spec.spread;
        Segmenting {
            length: spec.segment / count,
            group: count as i32,
            turn_cell: TURN_MYR.max(oldest / TURN_CELLS),
            blend,
        }
    }

    /// The place of boundary `j` along the line at the crest, before the
    /// bend of the spreading direction. Crest segment `j` starts at it.
    fn boundary(&self, sg: &Segmenting, j: i32) -> f64 {
        let jitter = 2.0 * unit(hash(j as u32, 0, 0, self.ridge.seed)) - 1.0;
        (f64::from(j) + JITTER * jitter) * sg.length
    }

    /// The boundary that starts seeded group `c` of crest segments. It is
    /// a transform fault. The other boundaries are offsets with no
    /// transform fault. A group has `sg.group` segments as a mean.
    fn seeded_start(&self, sg: &Segmenting, c: i32) -> i32 {
        let choices = (sg.group / 2 + 1) as u32;
        let h = hash(c as u32, 1, 0, self.ridge.seed);
        c.wrapping_mul(sg.group).wrapping_add((h % choices) as i32)
    }

    /// The seeded group of crest segment `k`.
    fn seeded_group(&self, sg: &Segmenting, k: i32) -> i32 {
        let c = k.div_euclid(sg.group);
        if k >= self.seeded_start(sg, c) {
            c
        } else {
            c - 1
        }
    }

    /// Boundary `j` starts a seeded group.
    fn seeded_transform(&self, sg: &Segmenting, j: i32) -> bool {
        j == self.seeded_start(sg, self.seeded_group(sg, j))
    }

    /// The boundary that starts group `c`. A guide starts group
    /// `GUIDE_GROUP` plus its index.
    fn group_start(&self, sg: &Segmenting, c: i32) -> i32 {
        match self.guides.of_group(c) {
            Some(guide) => guide.j,
            None => self.seeded_start(sg, c),
        }
    }

    /// The group of crest segment `k`. Without guides, it is the seeded
    /// group. A seeded group that starts near a guide joins the group
    /// before it, and a guide starts a group.
    fn group(&self, sg: &Segmenting, k: i32) -> i32 {
        let mut c = self.seeded_group(sg, k);
        let guides = &self.guides;
        if guides.list.is_empty() {
            return c;
        }
        for _ in 0..GUIDE_JOINS {
            if !guides.excluded.contains(&self.seeded_start(sg, c)) {
                break;
            }
            c -= 1;
        }
        let (mut start, mut id) = (self.seeded_start(sg, c), c);
        for (n, guide) in guides.list.iter().enumerate() {
            if guide.j <= k && guide.j > start {
                (start, id) = (guide.j, GUIDE_GROUP + n as i32);
            }
        }
        id
    }

    /// Boundary `j` is a transform fault.
    fn transform(&self, sg: &Segmenting, j: i32) -> bool {
        let guides = &self.guides;
        if guides.list.is_empty() {
            return self.seeded_transform(sg, j);
        }
        guides.list.iter().any(|guide| guide.j == j)
            || (self.seeded_transform(sg, j) && !guides.excluded.contains(&j))
    }

    /// The shift of crest segment `k` across the line. Most of it is the
    /// shift of its group.
    fn shift(&self, sg: &Segmenting, k: i32) -> f64 {
        let own = 2.0 * unit(hash(k as u32, 4, 0, self.ridge.seed)) - 1.0;
        self.group_shift(self.group(sg, k)) + self.ridge.offset * (1.0 - GROUP_SHIFT) * own
    }

    /// The shift of group `c` as a part of its largest shift, from -1 to 1.
    fn group_part(&self, c: i32) -> f64 {
        match self.guides.of_group(c) {
            Some(guide) => guide.part,
            None => 2.0 * unit(hash(c as u32, 3, 0, self.ridge.seed)) - 1.0,
        }
    }

    /// The part of the shift of a crest segment that its group `c` gives.
    fn group_shift(&self, c: i32) -> f64 {
        self.ridge.offset * GROUP_SHIFT * self.group_part(c)
    }

    /// The move along the line that the history of the spreading direction
    /// gives to the trace of a boundary, at `tau` million years of age from
    /// the line. `tau` is negative on the right. The tangent of the angle of
    /// the spreading direction is a noise of the age, and the move is its
    /// integral. The result is the move in radians, and its change with the
    /// distance across the line.
    fn turn(&self, sg: &Segmenting, tau: f64) -> (f64, f64) {
        let seed = self.ridge.seed ^ SALT_TURN;
        let tangent = |c: i32| TAN_TURN * (2.0 * unit(hash(c as u32, 5, 0, seed)) - 1.0);
        let cell_sum = |c: i32| 0.5 * (tangent(c) + tangent(c + 1));
        let x = tau / sg.turn_cell;
        let n = x.floor();
        let (s, n) = (x - n, n as i32);
        let mut sum = 0.0;
        for c in 0..n {
            sum += cell_sum(c);
        }
        for c in n..0 {
            sum -= cell_sum(c);
        }
        let (h0, h1) = (tangent(n), tangent(n + 1));
        sum += h0 * s + (h1 - h0) * s * s * s * (1.0 - 0.5 * s);
        let slope = h0 + (h1 - h0) * s * s * (3.0 - 2.0 * s);
        (self.ridge.spread * sg.turn_cell * sum, slope)
    }

    /// The move of boundary `j` along the line at `tau` million years of age
    /// from the line, from the migration of the boundary. A transform fault
    /// does not migrate. The result is the move in radians, and its change
    /// with the age.
    fn migration(&self, sg: &Segmenting, j: i32, tau: f64) -> (f64, f64) {
        let seed = self.ridge.seed ^ SALT_MIGRATE;
        let wander = WANDER_KM / self.ridge.world_km;
        // The move is a part of the shorter of the two segments at the
        // boundary, so two traces do not cross.
        let gap = |a: i32| self.boundary(sg, a) - self.boundary(sg, a - 1);
        let migrate = if self.transform(sg, j) {
            0.0
        } else {
            MIGRATE * gap(j).min(gap(j + 1))
        };
        let mut sum = (0.0, 0.0);
        for (size, cell, channel) in [(migrate, MIGRATE_MYR, 0), (wander, WANDER_MYR, 1)] {
            let (value, slope) = noise1(tau / cell, j as u32, channel, seed);
            let (zero, _) = noise1(0.0, j as u32, channel, seed);
            sum.0 += 0.5 * size * (value - zero);
            sum.1 += 0.5 * size * slope / cell;
        }
        if migrate > 0.0 {
            // On old floor, the trace has its own path of straight pieces
            // with sharp kinks between them.
            let t = (tau / KINK_OLD_MYR).clamp(0.0, 1.0);
            let (grow, change) = (t * t * (3.0 - 2.0 * t), 6.0 * t * (1.0 - t) / KINK_OLD_MYR);
            let size = 0.5 * KINK * sg.length;
            let (value, slope) = kink1(tau / KINK_MYR, j as u32, 3, seed);
            let (zero, _) = kink1(0.0, j as u32, 3, seed);
            sum.0 += size * grow * (value - zero);
            sum.1 += size * (change * (value - zero) + grow * slope / KINK_MYR);
        }
        sum
    }

    /// The trace of boundary `j` at `across`: the place along the line, and
    /// the change of that place with `across`. The trace starts at the
    /// boundary at the crest and follows the history of the spreading out
    /// to the flanks. `turn` is the result of `turn` at `across`.
    fn trace(&self, sg: &Segmenting, j: i32, across: f64, turn: (f64, f64)) -> Trace {
        let v = self.ridge.spread;
        let middle = 0.5 * (self.shift(sg, j.wrapping_sub(1)) + self.shift(sg, j));
        let off = across - middle;
        // The age is round at the crest, so the change has no step there.
        let tau = (off / v).hypot(MIGRATE_ROUND) - MIGRATE_ROUND;
        let (moved, change) = self.migration(sg, j, tau);
        let (warp, bend) = self.warp(sg, j, tau, off < 0.0);
        let to_tau = off / v / (tau + MIGRATE_ROUND);
        Trace {
            u: self.boundary(sg, j) + turn.0 + moved + warp,
            slope: turn.1 + (change + bend) * to_tau / v,
            age: tau,
        }
    }

    /// The move of the trace of boundary `j` along the line from a smooth
    /// field of the place along the line and the age, at `tau` million
    /// years of age on one side of the line. Traces near each other move
    /// together, so they do not cross. The move grows with the age, so
    /// young traces are straight. The result is the move in radians, and
    /// its change with the age.
    fn warp(&self, sg: &Segmenting, j: i32, tau: f64, right: bool) -> (f64, f64) {
        let seed = self.ridge.seed ^ SALT_TRACE_WARP;
        let km = self.ridge.world_km;
        let along = f64::from(j) * sg.length * km;
        let side = if right { 8.5 } else { 0.5 };
        let field = |tau: f64| {
            let t = (tau / TRACE_WARP_OLD_MYR).clamp(0.0, 1.0);
            let grow = t * t * (3.0 - 2.0 * t);
            let mut sum = 0.0;
            for (octave, [weight, along_km, myr]) in TRACE_WARP_OCTAVES.iter().enumerate() {
                let z = side + 3.0 * octave as f64;
                sum += weight * noise3([along / along_km, tau / myr, z], seed)[0];
            }
            TRACE_WARP_KM / km * grow * sum
        };
        let h = TRACE_WARP_STEP_MYR;
        (field(tau), (field(tau + h) - field(tau - h)) / (2.0 * h))
    }

    /// The depth of the ends of a crest segment below its middle, in
    /// kilometers. `part` is its length over the mean length of a crest
    /// segment, so a longer segment has deeper ends.
    fn segment_depth(&self, part: f64) -> f64 {
        let full_rate = 2.0 * self.ridge.spread * self.ridge.world_km;
        0.001 * lerp_table(&SEGMENT_DEPTH, full_rate) * part.clamp(0.5, 1.5)
    }

    /// The depth of the axial valley and the height of the axial high, in
    /// kilometers.
    fn axial(&self, f: &Faulting) -> (f64, f64) {
        let full_rate = 2.0 * self.ridge.spread * self.ridge.world_km;
        let faulted = (1.0 - f.magma) / (1.0 - magma_share(full_rate));
        let part = faulted.clamp(0.0, VALLEY_MOST).powf(VALLEY_POWER);
        let valley = lerp_table(&AXIAL_VALLEY, full_rate) * part;
        let high = MAGMA_HIGH * smoothstep(HIGH_MAGMA, 1.0, f.magma);
        (
            0.001 * valley,
            0.001 * lerp_table(&AXIAL_HIGH, full_rate).max(high),
        )
    }

    /// The distance along the line with the bend of the noise. The curve
    /// grows with the distance from the line.
    fn bent(&self, at: LinePoint, noise: RidgeNoise) -> f64 {
        let r = self.ridge.radius;
        let far = at.across.hypot(at.past) / (REACH * r);
        let curve = 0.5 * (noise.slow[1] - noise.slow[2]);
        at.u + r * (BEND_ALONG * noise.bend[0] + CURVE * curve * far * far)
    }

    /// The bent distance along the line and across it. Near the reach, the
    /// distance across has no bend.
    fn bent_place(&self, at: LinePoint, noise: RidgeNoise) -> (f64, f64) {
        let r = self.ridge.radius;
        let hold = 1.0 - smoothstep(r, REACH * r, at.across.hypot(at.past));
        let across = at.across + BEND_ACROSS * r * hold * noise.bend[1];
        // The seams between crest segments are ragged, so the hills of two
        // segments interfinger there.
        let seam = SEAM_KM * self.fit() / self.ridge.world_km * noise.seam;
        (self.bent(at, noise) + seam, across)
    }

    /// The faults of the settings.
    fn faulting(&self) -> Faulting {
        let spec = &self.ridge;
        let full_rate = 2.0 * spec.spread * spec.world_km;
        let half_texel = 0.5 * self.step() * spec.world_km;
        Faulting::new(
            full_rate,
            spec.magma,
            spec.hills,
            [half_texel, fit_of(half_texel, spec.fit, spec.scale)],
            self.relief,
        )
    }

    /// The abyssal hills of one side of the axis, in kilometers, at `along`
    /// kilometers along the group `group` and `across` kilometers from the
    /// crest. The hills are the tilted blocks of normal faults (Macdonald
    /// and others 1996). Each fault is a thin scarp along the fabric, and
    /// the block behind it tilts back to the level of the floor over a few
    /// kilometers. Most faults face the axis. Some faults are pairs that face
    /// each other across a narrow graben. The throw of a fault tapers to
    /// zero at its ends. The faults are in cells, `HILLS` in each cell, at
    /// seeded places, so they overlap en echelon and their spacing varies.
    /// A slow warp of the place bends each fault a little. `patch` is the
    /// throw scale and the part of the faults that show, from `patch_of`. A
    /// fault is zero outside its window, so the sum does not depend on the
    /// cells that the search starts from.
    pub(crate) fn hills(
        base: u32,
        f: &Faulting,
        group: i32,
        side: u32,
        [along, across]: [f64; 2],
        patch: [f64; 2],
    ) -> f64 {
        Self::hill_sum(base, f, [group, 0], side, [along, across], patch)
    }

    /// `hills`, with a search of `extra` more cells on each side.
    fn hill_sum(
        base: u32,
        f: &Faulting,
        [group, extra]: [i32; 2],
        side: u32,
        [along, across]: [f64; 2],
        [throw_scale, density]: [f64; 2],
    ) -> f64 {
        // On a coarse map, the hills are larger by the fit factor.
        let (along, across) = (along / f.fit, across / f.fit);
        let seed = base ^ SALT_HILL ^ (group as u32).wrapping_mul(0x9e37_79b9);
        let draw = |i: i32, j: i32, channel: u32| {
            unit(hash(i as u32, j as u32, side * 64 + channel, seed))
        };
        let (sp, s) = (f.spacing, f.hill_scale);
        // The warp: noise that changes slowly along the fabric and faster
        // across it, so neighbors bend differently.
        let bend = noise3(
            [
                along / (HILL_WARP[0] * s),
                across / (HILL_WARP[1] * s),
                f64::from(side),
            ],
            seed ^ SALT_WARP,
        );
        let across = across + HILL_WARP_KM * s * bend[0];
        let (cell_along, cell_across) = (HILL_CELL[0] * s, HILL_CELL[1] * sp);
        // A fault is longer by about a texel, and lower by as much, so it
        // keeps its volume and does not alias. Across the fabric, the
        // scarp is the mean over a box one texel wide.
        let blur = f.half_texel / f.fit;
        // Noise with features of less than two texels fades out.
        let fine = |size: f64| smoothstep(2.0 * blur, 4.0 * blur, size);
        let (fine_vary, fine_face) = (VARY[1] * fine(VARY_KM[1] * s), fine(FACE_KM));
        let (fine_rough, fine_gully) = (ROUGH_KM * fine(ROUGH_ALONG), GULLY * fine(GULLY_KM));
        let reach_along = 0.5 * (FAULT_LENGTH[1] * s).hypot(blur);
        let reach_across = 0.5 * GRABEN[1]
            + SCARP_KM
            + BACK_TILT[1] * s
            + blur
            + HILL_TILT * reach_along
            + STEP_KM * s
            + ROUGH_KM
            + FACE_ROUGH_KM
            + SCARP_KM * SHARP
            + 0.5 * GRABEN[1] * VARY[0]
            + BACK_TILT[1] * s * VARY[0];
        let cells = |x: f64, reach: f64, cell: f64| {
            ((x - reach) / cell).floor() as i32 - extra
                ..=((x + reach) / cell).floor() as i32 + extra
        };
        let ratio = FAULT_LENGTH[1] / FAULT_LENGTH[0];
        let mut sum = 0.0;
        for i in cells(along, reach_along, cell_along) {
            for j in cells(across, reach_across, cell_across) {
                for n in 0..HILLS {
                    let d = |channel: u32| draw(i, j, n * 16 + channel);
                    // In a calm patch, some faults fade out.
                    let shown = smoothstep(-SHOW_SOFT, SHOW_SOFT, density - d(8));
                    if shown <= 0.0 {
                        continue;
                    }
                    let middle = (f64::from(i) + d(0)) * cell_along;
                    let place = (f64::from(j) + d(1)) * cell_across;
                    // The length has a log-normal shape between its limits,
                    // and most faults are long.
                    let length = FAULT_LENGTH[0] * s * ratio.powf(d(2).max(d(3)));
                    let long = length.hypot(blur);
                    let q = 2.0 * (along - middle) / long;
                    if q.abs() >= 1.0 {
                        continue;
                    }
                    let tall = THROW_MIN + (1.0 - THROW_MIN) * (-(1.0 - d(4)).ln()).min(THROW_MAX);
                    let units = throw_scale * tall * (length / long);
                    let t = along - middle;
                    let fault = hash(i as u32, j as u32, side * 64 + n * 16 + 9, seed);
                    let links = Self::linked(base, fault, t / (LINK_KM * s) + d(10));
                    // Noise along the fault at a few scales: the throw, the
                    // back tilt, the width of a graben, the sharpness and the
                    // place of the scarp, and the spurs and the gullies of
                    // its face.
                    let wiggle = |x: f64, channel: u32| noise1(x, fault, channel, seed).0;
                    let vary = 1.0
                        + VARY[0] * wiggle(t / (VARY_KM[0] * s), 3)
                        + fine_vary * wiggle(t / (VARY_KM[1] * s), 4);
                    let units = units * (1.0 - q * q) * vary;
                    let back = mix(BACK_TILT[0], BACK_TILT[1], d(5)) * s;
                    let back = back * (1.0 + VARY[0] * wiggle(t / (VARY_KM[0] * s), 5));
                    let scarp = SCARP_KM * (1.0 + SHARP * fine_face * wiggle(t / FACE_KM, 6));
                    let tilt = HILL_TILT * (2.0 * d(6) - 1.0);
                    let (rough, _) = noise1(t / ROUGH_ALONG, fault, 2, seed);
                    let rough =
                        fine_rough * rough + FACE_ROUGH_KM * fine_face * wiggle(t / FACE_KM, 7);
                    let y = across - place - tilt * t - rough;
                    let gully = fine_gully * wiggle(t / GULLY_KM, 8);
                    let face = |y: f64| {
                        let on_face = bump((y + 0.5 * scarp) / (scarp + blur));
                        block(y, scarp, back, blur) * (1.0 + gully * on_face)
                    };
                    // A fault that faces the axis has its low side toward
                    // the crest. A graben is two faults that face each
                    // other.
                    let kind = d(7);
                    let half = 0.5 * mix(GRABEN[0], GRABEN[1], kind / GRABENS);
                    let half = half * (1.0 + VARY[0] * wiggle(t / (VARY_KM[0] * s), 9));
                    let mut shape = 0.0;
                    for (link, step) in links {
                        if link <= 0.0 {
                            continue;
                        }
                        let y = y - STEP_KM * s * step;
                        // The throw bends down to `THROW_CAP`, so no crest
                        // stands out.
                        let capped = THROW_CAP * (units * link / THROW_CAP).tanh();
                        shape += capped
                            * if kind < GRABENS {
                                face(y - half) + face(-y - half)
                            } else if kind < GRABENS + INWARD * (1.0 - GRABENS) {
                                face(y)
                            } else {
                                face(-y)
                            };
                    }
                    sum += f.hill_height * shown * shape;
                }
            }
        }
        // The blocks and the floors of the grabens are a little rough. The
        // lattice of the noise turns from the fabric, so it does not show.
        let (sin, cos) = GRAIN_TURN.sin_cos();
        let grain = noise3(
            [
                (along * cos - across * sin) / (GRAIN_KM * s),
                (along * sin + across * cos) / (GRAIN_KM * s),
                f64::from(side),
            ],
            seed ^ SALT_GRAIN,
        );
        sum * (1.0 + GRAIN * fine(GRAIN_KM * s) * grain[0])
    }

    /// The segments of a fault that grew from linked segments (Cowie and
    /// Scholz 1992; Macdonald and others 1996) at `z`, the place along the
    /// fault in segments: for the segment before, this one, and the one
    /// after, its throw factor and its place across the fabric, from minus
    /// one to one. `fault` is the hash of the fault. Each segment has its own
    /// throw, so the throw has several maxima, and a few segments are gaps.
    /// The ends of two segments overlap by `RELAY` of a segment, en echelon,
    /// and their throws taper there: a relay ramp.
    fn linked(base: u32, fault: u32, z: f64) -> [(f64, f64); 3] {
        let seed = base ^ SALT_LINK;
        let k = z.floor();
        let rest = z - k;
        let k = k as i32;
        std::array::from_fn(|n| {
            let m = k + n as i32 - 1;
            let r = rest + 1.0 - n as f64;
            let half = 0.5 * RELAY;
            let ramp = smoothstep(-half, half, r) * (1.0 - smoothstep(1.0 - half, 1.0 + half, r));
            let draw = |channel: u32| unit(hash(m as u32, fault, channel, seed));
            let u = draw(0);
            let throw = smoothstep(LINK_GAP, LINK_GAP + 0.3, u) * (0.5 + u);
            (throw * ramp, 2.0 * draw(1) - 1.0)
        })
    }

    /// The fine texture of the sea floor between and on the faults, in
    /// kilometers: sparse Gabor noise (Lagae and others 2009) with the
    /// anisotropic von Kármán spectrum of Goff and Jordan (1988), stretched
    /// along the fabric. Each cell has `GABOR_KERNELS` kernels at seeded
    /// places, each a wave in a round window, with a seeded direction and a
    /// seeded wavelength. The wavelengths are even in octaves, and the
    /// weight of a kernel is the root of the power of its octave. Waves of
    /// less than two texels fade out. `scale` is the throw scale of the
    /// patch.
    pub(crate) fn texture(
        base: u32,
        f: &Faulting,
        group: i32,
        side: u32,
        [along, across]: [f64; 2],
        scale: f64,
    ) -> f64 {
        // The texture grows by the root of the fit factor, so it stays soft.
        let grow = f.fit.sqrt();
        let (along, across) = (along / grow, across / grow);
        let seed = base ^ SALT_GABOR ^ (group as u32).wrapping_mul(0x9e37_79b9);
        let s = f.hill_scale;
        let cell = GABOR_CELL * s;
        // The place in the stretched plane, in cells.
        let (x, y) = (across / cell, along / (GABOR_ASPECT * cell));
        let (ix, iy) = (x.floor() as i32, y.floor() as i32);
        let texel = 2.0 * f.half_texel / grow;
        let corner = TAU * GABOR_WIDTH * s;
        let mut sum = 0.0;
        for i in ix - 1..=ix + 1 {
            for j in iy - 1..=iy + 1 {
                for n in 0..GABOR_KERNELS {
                    let d = |channel: u32| {
                        unit(hash(i as u32, j as u32, side * 64 + n * 8 + channel, seed))
                    };
                    let (dx, dy) = (x - f64::from(i) - d(0), y - f64::from(j) - d(1));
                    let r2 = dx * dx + dy * dy;
                    if r2 >= 1.0 {
                        continue;
                    }
                    // The wavelength, in kilometers in the stretched plane.
                    let octave = mix(GABOR_OCTAVES[0], GABOR_OCTAVES[1], d(2));
                    let wavelength = corner * (-octave).exp2();
                    let keep = smoothstep(2.0 * texel, 4.0 * texel, wavelength);
                    if keep <= 0.0 {
                        continue;
                    }
                    let q = octave.exp2();
                    let weight = q / (1.0 + q * q).powf(0.5 * (HURST + 1.0));
                    // Long waves are long along the fabric, and short waves
                    // have no direction.
                    let part = (octave - GABOR_OCTAVES[0]) / (GABOR_OCTAVES[1] - GABOR_OCTAVES[0]);
                    let stretch = mix(GABOR_STRETCH[0], GABOR_STRETCH[1], part);
                    let (sin, cos) = (TAU * d(3)).sin_cos();
                    let (ax, ay) = (dx * cell, dy * GABOR_ASPECT * cell);
                    let wave = (TAU * ((ax * cos + ay * sin / stretch) / wavelength + d(4))).cos();
                    let window = (1.0 - r2) * (1.0 - r2);
                    sum += keep * weight * window * wave;
                }
            }
        }
        sum * f.texture_height * scale
    }

    /// The staircase of the large faults of one wall of the axial valley,
    /// in parts of the sum of their throws. The segments of group `group`
    /// share the faults, so the walls have no break at an offset with no
    /// transform fault. `along` and `x` are the distance along the line and
    /// from the axis of the valley, and `zone` is half of the width of the
    /// floor and the width of the wall, all in kilometers. Each fault is in
    /// pieces along the line, as the faults of the blocks are.
    fn walls(
        &self,
        f: &Faulting,
        [group, side]: [i32; 2],
        along: f64,
        x: f64,
        zone: [f64; 2],
    ) -> f64 {
        let seed = self.ridge.seed ^ SALT_WALL;
        let key = (group as u32).wrapping_mul(2).wrapping_add(side as u32);
        let draw = |n: i32, j: i32, channel: u32| {
            unit(hash(
                n as u32,
                j as u32,
                key.wrapping_mul(8).wrapping_add(channel),
                seed,
            ))
        };
        let [floor, wall] = zone;
        let row = wall / f64::from(WALL_FAULTS);
        let mut sum = 0.0;
        for n in 0..WALL_FAULTS {
            let piece = mix(WALL_PIECE[0], WALL_PIECE[1], draw(n, -1, 0));
            let phase = draw(n, -1, 1);
            let cell = (along / piece + phase).floor() as i32;
            for j in cell - 1..=cell + 1 {
                let middle =
                    f64::from(j) + 0.5 - phase + PIECE_JITTER * (2.0 * draw(n, j, 2) - 1.0);
                let half = 0.5 * (1.0 + OVERLAP * draw(n, j, 3));
                let smooth = f.half_texel.min(MAX_SMOOTH * piece);
                let (start, end) = ((middle - half) * piece, (middle + half) * piece);
                if along <= start - smooth || along >= end + smooth {
                    continue;
                }
                let q = (2.0 * along - start - end) / (end - start);
                let taper = ellipse(q, 2.0 * smooth / (end - start));
                let throw = (0.5 + draw(n, j, 4)) / f64::from(WALL_FAULTS);
                let jitter = WALL_JITTER * (2.0 * draw(n, j, 5) - 1.0);
                let at = floor + row * (f64::from(n) + 0.5 + jitter);
                let scarp = (throw * row / DIP_TAN).max(1e-3);
                sum += throw * taper * block(x - at, scarp, MOUNT_KM, f.half_texel);
            }
        }
        sum
    }

    /// Lumps of lava from 0 to about 1 high, at `[along, across]`
    /// kilometers, in cells of `cell` kilometers. The segments of group
    /// `group` share them.
    fn lumps(&self, group: i32, [along, across]: [f64; 2], cell: f64) -> f64 {
        let seed = self.ridge.seed ^ SALT_LUMP;
        let (a, c) = (along / cell, across / cell);
        let (ia, ic) = (a.floor() as i32, c.floor() as i32);
        let mut sum = 0.0;
        for i in ia - 1..=ia + 1 {
            for j in ic - 1..=ic + 1 {
                let key = (group as u32).wrapping_mul(4);
                let draw =
                    |channel: u32| unit(hash(i as u32, j as u32, key.wrapping_add(channel), seed));
                let (pa, pc) = (f64::from(i) + draw(0), f64::from(j) + draw(1));
                let radius = mix(LUMP_SIZE, 1.0, draw(2));
                // Most lumps are low, and a few are high.
                let height = draw(3) * draw(3);
                sum += height * bump((a - pa).hypot(c - pc) / radius);
            }
        }
        sum
    }

    /// The fracture zone along the trace of boundary `j`. `step` is the
    /// offset of the crest at the boundary in kilometers, and `age` is the
    /// age at the trace. The trough is deeper and wider at a larger offset,
    /// and each transform fault has its own strength, so a few troughs are
    /// deep and most are faint.
    fn fracture(&self, j: i32, step: f64, transform: bool, age: f64) -> Fracture {
        let seed = self.ridge.seed ^ SALT_FRACTURE;
        let draw = |channel: u32| unit(hash(j as u32, channel, 0, seed));
        let size = step / TROUGH_OFFSET_KM;
        let strength = mix(STRENGTH[0], STRENGTH[1], draw(0));
        let full_rate = 2.0 * self.ridge.spread * self.ridge.world_km;
        let kind = if transform {
            1.0
        } else {
            OFFSET_TROUGH * offset_trough_part(full_rate)
        };
        // The trace leaves the transform fault at this age.
        let out = 0.5 * step / (self.ridge.spread * self.ridge.world_km);
        let active = 1.0 + ACTIVE * (1.0 - smoothstep(out, out + ACTIVE_MYR, age));
        let keep = if transform {
            let (patches, _) = noise1(age / PATCH_MYR, j as u32, 1, seed);
            let patchy = PATCH * smoothstep(0.0, PATCH_OLD_MYR, age);
            1.0 - patchy * (1.0 - smoothstep(-0.5, 0.5, patches))
        } else {
            // The trough of a small offset breaks into pieces with gaps
            // between them on old floor.
            let (pieces, _) = noise1(age / GAP_MYR, j as u32, 1, seed);
            let broken = smoothstep(0.0, PATCH_OLD_MYR, age);
            1.0 - broken * (1.0 - smoothstep(-GAP_EDGE, GAP_EDGE, pieces - GAP_SHARE))
        };
        let filled = 1.0 - TROUGH_FILL * smoothstep(0.0, TROUGH_MYR, age);
        let least = TROUGH_LEAST * smoothstep(0.0, TROUGH_LEAST_KM, step);
        let full = (TROUGH_DEPTH * size.powf(TROUGH_POWER)).max(least) * strength * kind * active;
        let mut half = (TROUGH_KM * (size * strength * kind).powf(TROUGH_WIDENS))
            .clamp(TROUGH_KM_RANGE[0], TROUGH_KM_RANGE[1]);
        if !transform {
            // The trace of a small offset is a broad trough.
            half = half.max(OFFSET_HALF_KM);
        }
        let depth = full.min(TROUGH_MAX) * keep * filled;
        let ridge = if transform && draw(2) < TRANSVERSE_SHARE {
            TRANSVERSE * depth
        } else {
            0.0
        };
        Fracture {
            depth,
            half,
            ridge,
            side: if draw(3) < 0.5 { -1.0 } else { 1.0 },
        }
    }

    /// The level of a texel on the ridge. `start` is its level from before
    /// the stroke, and `noise` is the noise at its place.
    pub(crate) fn level(&self, start: u16, at: LinePoint, noise: RidgeNoise) -> u16 {
        self.level_in(start, at, noise, 0)
    }

    /// `level`, with a search of `extra` more boundaries on each side.
    fn level_in(&self, start: u16, at: LinePoint, noise: RidgeNoise, extra: i32) -> u16 {
        let spec = &self.ridge;
        let (r, t, km, v) = (spec.radius, self.step(), spec.world_km, spec.spread);
        let old = f64::from(start);
        let from_line = at.across.hypot(at.past);
        // The ridge has no height at an end of the line.
        let taper = smoothstep(0.0, TAPER * r, at.end);
        // Near an end, the ridge is narrow. `far` is the distance from the
        // line in brush radii of that narrow ridge.
        let lens = TAPER_WIDTH + (1.0 - TAPER_WIDTH) * taper;
        let far = from_line / (r * lens);
        // The noise bends the two distances.
        let (u, across) = self.bent_place(at, noise);
        // The age of the sea floor, in millions of years. The crest is round
        // over a few texels.
        let round = CREST_ROUND * t / v;
        let age = |d: f64| (d / v).hypot(round) - round;
        // The depth follows the age, and is zero at the crest and at the
        // brush radius.
        let (top, foot) = (depth(0.0), depth(age(r)));
        let lift = (f64::from(spec.crest) - old).max(0.0);
        let f = self.faulting();
        let mean = f.hill_mean * f.level;
        // The patch of the floor sets the throw of the hills and the part of
        // the faults that show.
        let patch = patch_of(RIDGE_PATCH * noise.patch);
        let local = patch[0] * patch[1];
        // Ground at the crest level gets no hills.
        // The test uses the throws at the real size, so the fit does not lower
        // the hills.
        let low = (lift / (HILL_TOP * f.hill_height / f.lift * f.level + 1.0)).min(1.0);
        // The blocks and the troughs fade toward the reach.
        let fade = 1.0 - smoothstep(HILL_FADE, REACH, far);
        let sg = self.segmenting();
        let (valley, high) = self.axial(&f);
        // The rise is flat over the floor of the axial valley, and it
        // follows the axis of the valley.
        let valley_part = if f.level > 0.0 {
            (valley / VALLEY_FLAT).min(1.0)
        } else {
            0.0
        };
        let flat_floor = valley_part * FLOOR_KM / km;
        // The fractal relief of volcanic hummocks and small faults between
        // the large faults. A slow ridge builds its crust of hummocky mounds
        // of pillow lava, and a fast ridge of smooth sheet flows (Smith and
        // Cann 1993), so the relief fades with the spreading rate, as the
        // trough of an offset does. Its power falls with the wavenumber as a power
        // law, as the relief of real sea floor does (Goff and Jordan 1988),
        // and the lumps are longer along the fabric. Octaves of less than
        // two texels fade out.
        let bumps = {
            let texel = 2.0 * f.half_texel;
            let (mut sum, mut total, mut weight) = (0.0, 0.0, 1.0);
            for o in 0..BUMP_OCTAVES {
                let size = BUMP_KM * f.fit * 0.5f64.powi(o as i32);
                let at = [u * km / (BUMP_ASPECT * size), across * km / size, 0.5];
                let value = noise3(at, spec.seed ^ SALT_BUMP.wrapping_add(o as u32))[0];
                sum += weight * smoothstep(2.0 * texel, 4.0 * texel, size) * value;
                total += weight;
                weight *= BUMP_GAIN;
            }
            let full_rate = 2.0 * v * km;
            let fast = smoothstep(OFFSET_RATE[0], OFFSET_RATE[1], full_rate);
            BUMPS * mix(1.0, BUMP_FAST, fast) * f.hill_height * sum / total
        };
        // The rise and the blocks of crest segment `k`, from the trace
        // `lower` at its start to the trace `upper` at its end. The age is
        // the distance from the crest of that segment.
        let magma_seed = spec.seed ^ SALT_MAGMA;
        let segment = |k: i32, lower: f64, upper: f64| {
            let crest = self.shift(&sg, k);
            let off = across - crest;
            let from_crest = off.hypot(at.past);
            // The ends of the segment are deeper than its middle. The rise
            // is lower by the mean height of the blocks, so the mean ground
            // follows the depth.
            let len = (upper - lower).max(SHORTEST_PART * sg.length);
            let place = ((u - lower) / len).clamp(0.0, 1.0);
            let ends = 0.5 * (1.0 + (std::f64::consts::TAU * place).cos());
            // The depth of each end changes with the age and from boundary
            // to boundary. The noise is of the age with a sign from the
            // middle of the two crests at the boundary, so it has no step at
            // the crest, and the two segments at a boundary share it.
            let tau = from_crest / v;
            let end = if place < 0.5 { k } else { k.wrapping_add(1) };
            let middle = 0.5 * (self.shift(&sg, end.wrapping_sub(1)) + self.shift(&sg, end));
            let (deeper, _) = noise1(
                (across - middle) / v / SEGMENT_MYR,
                end as u32,
                4,
                magma_seed,
            );
            let deeper = 1.0 - SEGMENT_CHANGE * (1.0 - deeper);
            // The crust thins toward the ends, so they are deep over the
            // last part of the segment, and the middle is a broad high.
            let sunk = self.segment_depth(len / sg.length) * f.level * ends * ends * deeper;
            // The ends of the segment, and the segment past the nearer end.
            let (near, next) = if place < 0.5 {
                (place, k.wrapping_sub(1))
            } else {
                (1.0 - place, k.wrapping_add(1))
            };
            let hook = (1.0 - near / HOOK_PART).max(0.0);
            // The axial valley and the rift mountains fade toward a
            // transform fault, where the nodal deep is.
            let transform = self.transform(&sg, if place < 0.5 { k } else { k.wrapping_add(1) });
            let fade_out = if transform {
                smoothstep(0.0, 1.0, hook)
            } else {
                0.0
            };
            // At an offset with no transform fault, the axis of the axial
            // valley bends from this crest to the middle of the two crests
            // over `JOG` of the segment, so the valley has no break there.
            let jog = if transform {
                0.0
            } else {
                0.5 * (1.0 - smoothstep(0.0, JOG, near))
            };
            let valley_axis = crest + (self.shift(&sg, next) - crest) * jog;
            let valley_off = across - valley_axis;
            let from_axis = valley_off.hypot(at.past) * km;
            // The floor of the valley is narrow at the middle of the segment
            // and wide at its ends.
            let floor_half = mix(FLOOR_HALF[0], FLOOR_HALF[1], ends);
            let wall = mix(WALL_WIDTH[0], WALL_WIDTH[1], ends);
            // The depth of the ends fades slower than the rise.
            // The throw of a fault grows from the middle of the floor of the
            // axial valley, and the rise is lower by the mean height of the
            // blocks there.
            let grow_end = floor_half + GROW * f.spacing + t * km;
            let grow = smoothstep(0.5 * floor_half, grow_end, from_axis.min(from_crest * km));
            let flat = grow * (mean * local + MAGMA_MEAN * MAGMA_RELIEF * f.level);
            // Near the axis, the rise follows the axial valley, so the
            // valley is at the top of the rise at an offset too.
            let near_axis =
                1.0 - valley_part * (1.0 - smoothstep(0.0, RISE_BEND_KM, from_crest * km));
            let from_rise = mix(from_axis / km, from_crest, near_axis);
            let from_rise = from_rise.max(flat_floor) / lens;
            let w = ((foot - depth(age(from_rise))) / (foot - top)).clamp(0.0, 1.0);
            // The foot of the rise has no slope.
            let w = w * w * (1.0 + FOOT) / (w + FOOT);
            // The depth also changes from place to place, so the lines of
            // equal depth wander, and the crest is deeper in places. The
            // crest and the foot keep their levels under the swell.
            let swell = w.powf((-SWELL * noise.bend[2]).exp());
            let sag = SAG * noise.slow[0].clamp(0.0, 1.0);
            let w = swell * (1.0 - sag);
            let rise = taper * (w * (lift - low * flat) - low * sunk * w * (2.0 - w)).max(0.0);
            // The magma of the segment changes with the age, so the hills
            // are higher in some bands and lower in others.
            // The segments between two transform faults share the bands of
            // the magma, so the bands go on past an offset with no transform
            // fault. The bands are of the age with a sign from the middle of
            // the group, and they bend along the line.
            let group = self.group(&sg, k);
            let center = self.group_shift(group);
            // The distance along the line from the first boundary of the
            // group. It stays small, so the shader keeps its digits.
            let origin = self.boundary(&sg, self.group_start(&sg, group));
            let group_along = (u - origin) * km;
            let (wave, _) = noise1(group_along / WAVE_KM, group as u32, 2, magma_seed);
            let band_at = |off: f64| {
                let tau = (off / v + WAVE_MYR * wave) / MAGMA_MYR;
                noise1_round(tau, group as u32, 0, magma_seed)
            };
            let (band, slope) = band_at(across - center);
            let rate = 1.0 + MAGMA_BAND * slope;
            let grown = rate.powf(-THROW_SPACING);
            let toward = if (self.shift(&sg, next) - crest) * off > 0.0 {
                -1.0
            } else {
                1.0
            };
            // The hills follow the distance from the crest. Both segments at
            // a boundary have the same distance there, so the hills go on
            // past a boundary with no offset.
            let hill_side = u32::from(off < 0.0);
            let hill_across = from_crest * km;
            // The hills read the distance along the whole line, so they go
            // on past the boundary of a group.
            let at = [u * km, hill_across];
            let hills = grow * grown * Self::hills(spec.seed, &f, 0, hill_side, at, patch);
            // Sediment ponds in the lows between the hills, up to twice its
            // thickness above the low of the hills: `HILL_LOW` of their mean
            // height. It fills `POND` of the depth of a low below that
            // level, and the hills above it keep their tops. The depth of
            // the age is the depth with the sediment, so the hills are lower
            // by about the rise of their mean.
            let held = f.hill_mean * local * grow * grown;
            let fill = (2.0 * SEDIMENT_RATE * tau - HILL_LOW * held).max(0.0);
            let blocks = pond(hills, fill) - (pond(held, fill) - held);
            // The fine texture, which sediment buries.
            let texture = Self::texture(spec.seed, &f, 0, hill_side, at, patch[0]);
            let buried = f.hill_height / (f.hill_height + fill).max(1e-9);
            // Sediment and mass wasting smooth the lumps of old floor (Goff
            // and Arbic 2010).
            let old = mix(1.0, BUMP_OLD, smoothstep(0.0, BUMP_MYR, tau));
            let blocks = blocks + grow * grown * (texture + bumps * patch[0] * old) * buried;
            // More magma gives a thicker crust, so the bands of the magma
            // are also small, smooth bands of the depth.
            let blocks = blocks + grow * MAGMA_RELIEF * 0.5 * (1.0 + band);
            // The axial valley is deeper at the ends of the segment. A broad
            // down-warp and the staircase of the large faults of its walls
            // make it, and the faults lift the rift mountains.
            let valley = valley * (1.0 + VALLEY_ALONG * (2.0 * ends - 1.0));
            let down = DOWN * valley * (1.0 - VALLEY_END * fade_out) * bump(from_axis / DOWN_KM);
            // At the inside corner, on the side of the transform fault, the
            // rift mountains are higher.
            let inside = if toward < 0.0 { INSIDE_CORNER } else { 0.0 };
            let valley_side = u32::from(valley_off < 0.0);
            let stairs = self.walls(
                &f,
                [group, valley_side as i32],
                group_along,
                from_axis,
                [floor_half, wall],
            );
            let stairs = STAIR * valley * (1.0 + (inside - 1.0) * fade_out) * stairs;
            // The floor is lumps of lava, with an axial volcanic ridge along
            // the middle of the segment. The ridge has gaps, and it is
            // highest at the middle of the segment.
            let on_floor = 1.0 - smoothstep(floor_half, floor_half + 0.5 * wall, from_axis);
            let cell = LUMP_KM.max(2.0 * LUMP_TEXELS * f.half_texel);
            let lumps = self.lumps(group, [group_along, valley_off * km], cell);
            let (gaps, _) = noise1(group_along / AVR_PIECE_KM, k as u32, 9, magma_seed);
            let ridge_top = smoothstep(AVR_END, 0.5, near) * smoothstep(-AVR_GAPS, AVR_GAPS, gaps);
            let axial_ridge = AVR_HEIGHT * ridge_top * bump(from_axis / AVR_KM);
            let floor = LUMP_HEIGHT * lumps * on_floor + axial_ridge;
            // Past the walls of the axial valley, the young crust is a broad
            // band above the depth of its age.
            let past_walls = smoothstep(floor_half, floor_half + wall, from_axis);
            let band_km = BAND_KM.min(BAND_RADIUS * r * km);
            let band = BAND * valley * past_walls * bump(from_axis / band_km);
            let relief = stairs - down + band + floor * (1.0 - fade_out);
            let axis = f.level * low * taper;
            (
                rise,
                blocks * f.level * low * taper * fade + relief * axis,
                high * bump(from_axis / HIGH_KM) * axis,
            )
        };
        // The traces of the boundaries near the texel. Crest segment `j`
        // has the part of a texel that is past trace `j` and before trace
        // `j + 1`, with a blend over `sg.blend` on each side of a trace.
        // Each part is exactly zero past its blend, so the sum does not
        // depend on the boundaries that the search starts from.
        // With guides, the turn is that of the guides at each boundary, and
        // the search has one more boundary on each side.
        let guides = &self.guides;
        let (seeded, i, wide) = if guides.list.is_empty() {
            let turn = self.turn(&sg, across / v);
            (Some(turn), ((u - turn.0) / sg.length).floor() as i32, 0)
        } else {
            let base = guides.base(u, across);
            (None, (base / sg.length).floor() as i32, 1)
        };
        let (mut rise, mut hills, mut deep, mut above) = (0.0, 0.0, 0.0, 0.0);
        let mut ridges = 0.0;
        let mut lower: Option<(f64, f64)> = None;
        for j in i - 1 - wide - extra..=i + 2 + wide + extra {
            let turn = seeded.unwrap_or_else(|| guides.turn(self.boundary(&sg, j), across));
            let trace = self.trace(&sg, j, across, turn);
            // The distance from the trace, square to it.
            let off = (u - trace.u) / trace.slope.hypot(1.0);
            let past = smoothstep(-sg.blend, sg.blend, off);
            if let Some((below, before)) = lower {
                let weight = before - past;
                if weight > 0.0 {
                    let (a, b, c) = segment(j - 1, below, trace.u);
                    rise += weight * a;
                    hills += weight * b;
                    above += weight * c;
                }
            }
            lower = Some((trace.u, past));
            // A fracture zone along the trace: a trough with steep walls,
            // and at some, a transverse ridge on one side.
            let (before, after) = (self.shift(&sg, j - 1), self.shift(&sg, j));
            let step = (after - before).abs() * km;
            let transform = self.transform(&sg, j);
            let fz = self.fracture(j, step, transform, trace.age);
            let width = 0.5 * sg.blend + fz.half / km;
            deep += fz.depth * (1.0 - smoothstep(TROUGH_FLOOR, 1.0, (off / width).abs()));
            let ridge_at = off - fz.side * TRANSVERSE_AT * width;
            ridges += fz.ridge * bump(ridge_at / (TRANSVERSE_WIDTH * width));
            // A deep at each end of the crest at a transform fault. It is
            // long along the transform fault, and its middle is inside it.
            if transform {
                let nodal = (NODAL_DEPTH * step).min(NODAL_MAX);
                let size = sg.blend + NODAL_KM / km;
                let long = sg.blend + NODAL_ALONG_KM / km;
                for (crest, other) in [(before, after), (after, before)] {
                    let middle = crest + (other - crest).signum() * NODAL_IN * long;
                    let q = (off / size).hypot((across - middle) / long);
                    deep += nodal * bump(q);
                }
            }
        }
        let axis = f.level * low * taper;
        let deep = deep * axis * fade;
        let hills = hills + ridges * axis * fade;
        // The highest blocks bend down below the Crest setting. The rift
        // mountains can go above it by `ABOVE_CREST` of the depth of the
        // axial valley, and the axial high goes above it.
        let room = ABOVE_CREST * valley * axis;
        let hills = soft_cap(hills, (lift - rise).max(0.0) + room);
        let up = (rise + hills).min(lift + room) - deep + above;
        let change = at.flow * up;
        let new = (old + (change + 0.5).floor()).clamp(0.0, 65535.0);
        spec.limits.apply(old, new) as u16
    }
}

/// The numbers of `RidgePlan::level` that are the same for each texel of a
/// plan. The shader gets them.
#[cfg(feature = "gpu")]
pub(crate) struct RidgeShape {
    /// `Segmenting::length` and `Segmenting::group`.
    pub length: f64,
    pub group: i32,
    pub turn_cell: f64,
    pub blend: f64,
    /// The roundness of the age at the crest, in millions of years.
    pub round: f64,
    /// The depth at the crest and at the brush radius, in meters.
    pub top: f64,
    pub foot: f64,
    /// The mean height of the hills, in 16-bit steps.
    pub mean: f64,
    pub magma: f64,
    pub spacing: f64,
    pub hill_height: f64,
    pub hill_scale: f64,
    pub hill_mean: f64,
    pub half_texel: f64,
    pub level: f64,
    /// The depth of the axial valley and the height of the axial high, in
    /// kilometers.
    pub valley: f64,
    pub high: f64,
    pub valley_part: f64,
    pub flat_floor: f64,
    pub band_km: f64,
    /// The depth of the ends of a crest segment of the mean length, in
    /// kilometers.
    pub segment_depth: f64,
    pub fit: f64,
    pub lift: f64,
    pub texture_height: f64,
}

#[cfg(feature = "gpu")]
impl RidgePlan {
    /// The numbers of `level` that are the same for each texel. The steps
    /// are those of `level_in`.
    pub(crate) fn shape(&self) -> RidgeShape {
        let spec = &self.ridge;
        let (r, t, km, v) = (spec.radius, self.step(), spec.world_km, spec.spread);
        let round = CREST_ROUND * t / v;
        let age = |d: f64| (d / v).hypot(round) - round;
        let f = self.faulting();
        let sg = self.segmenting();
        let (valley, high) = self.axial(&f);
        let valley_part = if f.level > 0.0 {
            (valley / VALLEY_FLAT).min(1.0)
        } else {
            0.0
        };
        RidgeShape {
            length: sg.length,
            group: sg.group,
            turn_cell: sg.turn_cell,
            blend: sg.blend,
            round,
            top: depth(0.0),
            foot: depth(age(r)),
            mean: f.hill_mean * f.level,
            magma: f.magma,
            spacing: f.spacing,
            hill_height: f.hill_height,
            hill_scale: f.hill_scale,
            hill_mean: f.hill_mean,
            half_texel: f.half_texel,
            level: f.level,
            valley,
            high,
            valley_part,
            flat_floor: valley_part * FLOOR_KM / km,
            band_km: BAND_KM.min(BAND_RADIUS * r * km),
            segment_depth: self.segment_depth(1.0),
            fit: f.fit,
            lift: f.lift,
            texture_height: f.texture_height,
        }
    }
}

/// The faults and the abyssal hills of a stroke.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Faulting {
    /// The magma part of the spreading.
    pub(crate) magma: f64,
    /// The mean distance between two faults, in kilometers.
    pub(crate) spacing: f64,
    /// Half of the texel step, in kilometers.
    pub(crate) half_texel: f64,
    /// The 16-bit steps of one kilometer of height.
    pub(crate) level: f64,
    /// The throw unit of a fault, in kilometers.
    pub(crate) hill_height: f64,
    /// The scale of the faults, in `FAULT_LENGTH`, `HILL_CELL`, and
    /// `BACK_TILT` units.
    pub(crate) hill_scale: f64,
    /// The mean height of the hills where all faults show at full throw,
    /// in kilometers.
    pub(crate) hill_mean: f64,
    /// The height unit of the fine texture, in kilometers.
    pub(crate) texture_height: f64,
    /// The fit factor of `fit_of`, and the scale of the height of the
    /// faults of `fit_height`. The texture does not grow higher.
    pub(crate) fit: f64,
    pub(crate) lift: f64,
}

impl Faulting {
    /// The faults at a full spreading rate in millimeters per year, with a
    /// change of the magma part, a scale of the height of the hills from 0
    /// to 2, half of the texel step in kilometers and the fit factor, and
    /// the 16-bit steps of one kilometer.
    pub(crate) fn new(
        full_rate: f64,
        magma: f64,
        hills: f64,
        [half_texel, fit]: [f64; 2],
        level: f64,
    ) -> Faulting {
        let m = magma_share(full_rate) + magma;
        let spacing = lerp_table(&FAULT_SPACING, m);
        let hill_scale = (spacing / HILL_SPACING).clamp(1.0, HILL_SCALE_MAX);
        let lift = fit_height(fit);
        let rms = hills.clamp(0.0, 2.0) * hill_rms(full_rate) * lift;
        let hill_height = rms * (1.0 - GABOR_SHARE * GABOR_SHARE).sqrt() / HILL_RMS_UNIT;
        Faulting {
            magma: m,
            spacing,
            half_texel,
            level,
            hill_height,
            hill_scale,
            hill_mean: hill_height * HILL_MEAN_UNIT,
            texture_height: hill_height / lift * fit.sqrt().min(lift) * GABOR_PER_THROW,
            fit,
            lift,
        }
    }
}

/// The fracture zone along the trace of a boundary.
struct Fracture {
    /// The depth of the trough, in kilometers.
    depth: f64,
    /// Half of the width of the trough past half of the blend, in
    /// kilometers.
    half: f64,
    /// The height of the transverse ridge, in kilometers.
    ridge: f64,
    /// The side of the transverse ridge along the line: -1 or 1.
    side: f64,
}

/// The crest segments of a stroke.
struct Segmenting {
    /// The mean length of a crest segment, in radians.
    length: f64,
    /// The mean number of crest segments between two transform faults.
    group: i32,
    /// The age of a cell of the noise of the spreading direction, in
    /// millions of years.
    turn_cell: f64,
    /// Half of the width of the blend of two crest segments at their
    /// boundary, in radians.
    blend: f64,
}

/// The drawn fracture zones of a line, which set the traces of the
/// boundaries.
#[derive(Clone, Debug, Default)]
pub(crate) struct Guides {
    /// The guides in the order of their boundaries.
    pub(crate) list: Vec<Guide>,
    /// The seeded transform faults that are plain offsets, in order.
    pub(crate) excluded: Vec<i32>,
    /// The distance across the line between two samples, in radians.
    pub(crate) step: f64,
}

/// One drawn fracture zone.
#[derive(Clone, Debug)]
pub(crate) struct Guide {
    /// The boundary of the transform fault.
    pub(crate) j: i32,
    /// The place of boundary `j` at the crest, in radians.
    pub(crate) u0: f64,
    /// The place where the stroke crosses the line, before the snap. The
    /// tests read it.
    #[allow(dead_code)]
    drawn: f64,
    /// The move of the trace along the line from `u0` at `k` samples
    /// across the line, on the left and on the right.
    pub(crate) m: [[f64; GUIDE_SAMPLES]; 2],
    /// The place of the stroke in draw order.
    order: usize,
    /// The result of `RidgePlan::group_part` for the group of the guide.
    pub(crate) part: f64,
}

/// A fracture zone of a stroke, before the snap: the place where it
/// crosses the line, and the moves of `Guide::m` from that place.
struct Drawn {
    u0: f64,
    m: [[f64; GUIDE_SAMPLES]; 2],
}

impl Guides {
    /// The guide of group `c`, if a guide starts it.
    fn of_group(&self, c: i32) -> Option<&Guide> {
        if self.list.is_empty() || c < GUIDE_GROUP {
            return None;
        }
        self.list.get((c - GUIDE_GROUP) as usize)
    }

    /// The move of a trace that starts at `b` at the crest, at `across`, and
    /// its change with `across`. Between two guides, it is a mix of their
    /// moves.
    fn turn(&self, b: f64, across: f64) -> (f64, f64) {
        let list = &self.list;
        let n = list.partition_point(|guide| guide.u0 <= b);
        if n == 0 {
            return list[0].at(across, self.step);
        }
        if n == list.len() {
            return list[n - 1].at(across, self.step);
        }
        let (a, c) = (&list[n - 1], &list[n]);
        let t = (b - a.u0) / (c.u0 - a.u0);
        let (ma, sa) = a.at(across, self.step);
        let (mc, sc) = c.at(across, self.step);
        (ma + t * (mc - ma), sa + t * (sc - sa))
    }

    /// The place at the crest of the trace through `u` at `across`: the
    /// inverse of `b + turn(b, across).0`.
    fn base(&self, u: f64, across: f64) -> f64 {
        let list = &self.list;
        let place = |guide: &Guide| guide.u0 + guide.at(across, self.step).0;
        let n = list.partition_point(|guide| place(guide) <= u);
        if n == 0 {
            return u - list[0].at(across, self.step).0;
        }
        if n == list.len() {
            return u - list[n - 1].at(across, self.step).0;
        }
        let (a, c) = (&list[n - 1], &list[n]);
        let (pa, pc) = (place(a), place(c));
        a.u0 + (u - pa) / (pc - pa) * (c.u0 - a.u0)
    }
}

impl Guide {
    /// The move of the trace at `across`, and its change with `across`.
    /// `step` is the distance between two samples. A cubic Hermite curve
    /// goes through the samples.
    fn at(&self, across: f64, step: f64) -> (f64, f64) {
        let m = &self.m[usize::from(across < 0.0)];
        let last = (GUIDE_SAMPLES - 1) as f64;
        let x = (across.abs() / step).min(last);
        let k = (x.floor() as usize).min(GUIDE_SAMPLES - 2);
        let t = x - k as f64;
        let tangent = |i: usize| {
            if i == 0 {
                m[1] - m[0]
            } else if i == GUIDE_SAMPLES - 1 {
                m[i] - m[i - 1]
            } else {
                0.5 * (m[i + 1] - m[i - 1])
            }
        };
        let (p0, p1, d0, d1) = (m[k], m[k + 1], tangent(k), tangent(k + 1));
        let (t2, t3) = (t * t, t * t * t);
        let value = (2.0 * t3 - 3.0 * t2 + 1.0) * p0
            + (t3 - 2.0 * t2 + t) * d0
            + (3.0 * t2 - 2.0 * t3) * p1
            + (t3 - t2) * d1;
        if x >= last {
            return (value, 0.0);
        }
        let slope = ((6.0 * t2 - 6.0 * t) * (p0 - p1)
            + (3.0 * t2 - 4.0 * t + 1.0) * d0
            + (3.0 * t2 - 2.0 * t) * d1)
            / step;
        (value, if across < 0.0 { -slope } else { slope })
    }
}

/// The trace of a boundary at a distance across the line.
struct Trace {
    /// The place along the line, in radians.
    u: f64,
    /// The change of `u` with the distance across the line.
    slope: f64,
    /// The age of the sea floor at the trace, in millions of years.
    age: f64,
}

/// The noise at the place of a texel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct RidgeNoise {
    /// Three numbers from -1.5 to 1.5: noise with cells of 0.9 of the brush
    /// radius, plus half of noise with cells of 0.37 of the brush radius.
    pub bend: V3,
    /// Three numbers from -1 to 1. They change over about two brush radii.
    pub slow: V3,
    /// A number from -1.5 to 1.5 that changes over about `PATCH_KM`.
    pub patch: f64,
    /// A number from -1.5 to 1.5: noise in cells of `SEAM_SIZE_KM` times
    /// the fit factor, plus half of noise in cells of half of that.
    pub seam: f64,
}

/// The samples of each side of a guide, from the crest to the reach.
pub(crate) const GUIDE_SAMPLES: usize = 32;
/// The group of the first guide. A guide starts group `GUIDE_GROUP` plus its
/// index.
pub(crate) const GUIDE_GROUP: i32 = 1 << 28;
/// The most seeded transform faults that guides make plain offsets.
pub(crate) const GUIDE_EXCLUDED: usize = 64;
/// The most seeded groups that a group can join.
const GUIDE_JOINS: usize = 8;
/// The largest change of the move of a guide with the distance across the
/// line, past the end of its stroke.
const GUIDE_SLOPE: f64 = 1.5;

/// The sizes of the cells of the noise, as parts of the brush radius.
const BEND_SIZE: f64 = 0.9;
const BEND_FINE_SIZE: f64 = 0.37;
const SLOW_SIZE: f64 = 1.7;

/// The largest bend of the distance along the line and across it is 1.5
/// times this part of the brush radius.
const BEND_ALONG: f64 = 0.08;
const BEND_ACROSS: f64 = 0.08;

/// At the reach, the slow noise moves the distance along the line by up to
/// this part of the brush radius.
const CURVE: f64 = 0.3;

/// The seed of each noise is the seed of the pattern with these bits
/// changed.
const SALT_BEND: u32 = 0x51ed_270b;
const SALT_BEND_FINE: u32 = 0x2f6b_8a35;
const SALT_SLOW: u32 = 0x7a3c_91d7;
const SALT_TURN: u32 = 0x6c8e_9cf5;
const SALT_MIGRATE: u32 = 0x3d4f_a1c7;
const SALT_MAGMA: u32 = 0x9b05_688c;
const SALT_WALL: u32 = 0x4c95_e1a3;
const SALT_LUMP: u32 = 0x2d8e_7f19;
const SALT_FRACTURE: u32 = 0x5f3a_c2e1;
const SALT_HILL: u32 = 0x7e4b_15d3;
const SALT_PATCH: u32 = 0x1f83_d9ab;
const SALT_WARP: u32 = 0x5be0_cd19;
const SALT_GABOR: u32 = 0x6a09_e667;
const SALT_LINK: u32 = 0x3c6e_f372;
const SALT_GRAIN: u32 = 0xa54f_f53a;
const SALT_TRACE_WARP: u32 = 0x8e2b_4d61;
const SALT_BUMP: u32 = 0x1b87_3593;
const SALT_SEAM: u32 = 0x4cf5_ad43;

/// The largest move of a boundary, as a part of the mean length of a crest
/// segment.
const JITTER: f64 = 0.3;

/// The mean length of a crest segment between two offsets with no
/// transform fault, in kilometers, at a full spreading rate in millimeters
/// per year (Macdonald and others 1991; Sempere and others 1993). The
/// Segment setting is the mean distance between two transform faults.
const SECOND_ORDER_LENGTH: [(f64, f64); 3] = [(20.0, 50.0), (50.0, 80.0), (90.0, 140.0)];

/// A crest segment is at least this number of blend widths long.
const SEGMENT_BLENDS: f64 = 5.0;

/// The width of the valley of a transform fault, in kilometers. Two crest
/// segments blend over it and 1.5 texels on each side of their boundary.
const VALLEY_KM: f64 = 4.0;

/// The part of the shift of a crest segment that is the shift of its group.
/// The rest is the offset of the segment from its group.
const GROUP_SHIFT: f64 = 0.8;

/// The tangent of the largest angle of the spreading direction to its mean,
/// 8 degrees.
const TAN_TURN: f64 = 0.14;

/// The age of a cell of the noise of the spreading direction, in millions
/// of years, and the largest number of cells out to `TURN_REACH` brush
/// radii.
const TURN_MYR: f64 = 8.0;
const TURN_CELLS: f64 = 16.0;
const TURN_REACH: f64 = 1.8;

/// The largest move of an offset with no transform fault from its place at
/// the crest, as a part of the shorter crest segment at the offset, and the
/// age of a cell of its noise in millions of years. The offset migrates
/// along the crest, so its trace is oblique to the crest and makes a V or a
/// zigzag on the flanks (Hey 1977; Macdonald and others 1988; Tucholke and
/// others 1997).
const MIGRATE: f64 = 0.35;
const MIGRATE_MYR: f64 = 3.0;

/// The largest small move of each trace, in kilometers, and the age of a
/// cell of its noise in millions of years.
const WANDER_KM: f64 = 3.0;
const WANDER_MYR: f64 = 0.8;

/// The warp of the traces: the largest move in kilometers, and for each
/// octave of the field, its weight, its cell along the line in kilometers,
/// and its cell of age in millions of years. The warp grows over
/// `TRACE_WARP_OLD_MYR`. Its change with the age is a difference over
/// `TRACE_WARP_STEP_MYR` on each side.
const TRACE_WARP_KM: f64 = 16.0;
const TRACE_WARP_OCTAVES: [[f64; 3]; 2] = [[1.0, 200.0, 10.0], [0.45, 90.0, 4.0]];
const TRACE_WARP_OLD_MYR: f64 = 10.0;
const TRACE_WARP_STEP_MYR: f64 = 0.05;

/// The age of a trace is round over this number of millions of years at
/// the crest.
const MIGRATE_ROUND: f64 = 0.5;

/// The depth of the ends of a crest segment of the mean length below its
/// middle at a full spreading rate in millimeters per year, in meters (Lin
/// and others 1990; Shaw and Lin 1993).
const SEGMENT_DEPTH: [(f64, f64); 3] = [(20.0, 900.0), (50.0, 400.0), (100.0, 100.0)];

/// The shortest crest segment for its depth and its magma, as a part of the
/// mean length.
const SHORTEST_PART: f64 = 0.2;

/// The age of a cell of the noise of the magma, in millions of years, and
/// the largest change of the spacing of the faults, as a part of the
/// spacing. The throw changes with the spacing to this power (Behn and Ito
/// 2008).
const MAGMA_MYR: f64 = 2.0;
const MAGMA_BAND: f64 = 0.26;
const THROW_SPACING: f64 = 0.65;

/// The change of the depth with the magma, from the least to the most, in
/// kilometers of throw. The bands of the magma show mostly in the spacing
/// and the throw of the faults (Olive and others 2015).
const MAGMA_RELIEF: f64 = 0.12;

/// The mean height of the bands of the magma, as a part of
/// `MAGMA_RELIEF`.
const MAGMA_MEAN: f64 = 0.5;

/// The bands of the magma move by up to this age, in millions of years,
/// with a noise along the segment that has cells of `WAVE_KM` kilometers.
const WAVE_MYR: f64 = 0.8;
const WAVE_KM: f64 = 40.0;

/// The depth of the ends of a segment changes with the age, with cells of
/// this number of millions of years, by up to `SEGMENT_CHANGE` of the
/// depth.
const SEGMENT_MYR: f64 = 6.0;
const SEGMENT_CHANGE: f64 = 0.35;

/// The axial valley fades toward a transform fault over this part of a
/// crest segment at each end.
const HOOK_PART: f64 = 0.15;

/// The depth of the trough of a fracture zone, in kilometers, is
/// `TROUGH_DEPTH` times the offset in `TROUGH_OFFSET_KM` to the power
/// `TROUGH_POWER`, times a strength of the transform fault from
/// `STRENGTH[0]` to `STRENGTH[1]`. It is no deeper than `TROUGH_MAX`. A
/// trough along an offset with no transform fault is `OFFSET_TROUGH` times
/// as deep.
const TROUGH_DEPTH: f64 = 1.3;
const TROUGH_OFFSET_KM: f64 = 100.0;
const TROUGH_POWER: f64 = 1.3;
const STRENGTH: [f64; 2] = [0.5, 1.6];
const TROUGH_MAX: f64 = 1.5;
const OFFSET_TROUGH: f64 = 0.8;

/// Half of the width of a trough past half of the blend, in kilometers, is
/// `TROUGH_KM` times the offset in `TROUGH_OFFSET_KM` times the strength,
/// to the power `TROUGH_WIDENS`, from `TROUGH_KM_RANGE[0]` to
/// `TROUGH_KM_RANGE[1]`. The floor of a trough is `TROUGH_FLOOR` of its
/// width.
const TROUGH_KM: f64 = 5.0;
const TROUGH_WIDENS: f64 = 0.8;
const TROUGH_KM_RANGE: [f64; 2] = [1.0, 10.0];
const TROUGH_FLOOR: f64 = 0.3;

/// Along the transform fault, and for `ACTIVE_MYR` past it, the trough is
/// up to `ACTIVE` deeper.
const ACTIVE: f64 = 0.4;
const ACTIVE_MYR: f64 = 3.0;

/// With the age, the trough breaks into patches `PATCH_MYR` long. Over
/// `PATCH_OLD_MYR`, the patches grow to `PATCH` of the depth.
const PATCH_MYR: f64 = 4.0;
const PATCH_OLD_MYR: f64 = 10.0;
const PATCH: f64 = 0.3;

/// The trough along an offset with no transform fault breaks into pieces
/// over `PATCH_OLD_MYR`. A piece or a gap is about `GAP_MYR` long. A gap is
/// where a noise from -1 to 1 is below `GAP_SHARE`, and its ends are round
/// over `GAP_EDGE` of the noise.
const GAP_MYR: f64 = 5.0;
const GAP_SHARE: f64 = -0.1;
const GAP_EDGE: f64 = 0.08;

/// The path of the trace of an offset with no transform fault is straight
/// pieces between kinks `KINK_MYR` million years apart. Each kink moves the
/// trace by up to `KINK` of the mean length of a crest segment. The kinks
/// grow over `KINK_OLD_MYR`.
const KINK: f64 = 0.12;
const KINK_MYR: f64 = 4.0;
const KINK_OLD_MYR: f64 = 6.0;

/// A trough is at least `TROUGH_LEAST` kilometers deep times the strength,
/// before the other factors of its depth. An offset of `TROUGH_LEAST_KM`
/// kilometers or more reaches that depth. A trough along an offset with no
/// transform fault is at least `OFFSET_HALF_KM` kilometers wide on each
/// side.
const TROUGH_LEAST: f64 = 0.3;
const TROUGH_LEAST_KM: f64 = 10.0;
const OFFSET_HALF_KM: f64 = 6.0;

/// The depth of the trough along an offset with no transform fault goes
/// from its full depth at a full spreading rate of `OFFSET_RATE[0]` mm/yr to
/// `OFFSET_FAST` of it at `OFFSET_RATE[1]` mm/yr. On a slow ridge the trace
/// of such an offset is a broad trough (Tucholke and others 1997). On a fast
/// ridge the offset is an overlapping spreading center, and its trace has
/// little relief (Carbotte and Macdonald 1992).
const OFFSET_RATE: [f64; 2] = [40.0, 90.0];
const OFFSET_FAST: f64 = 0.15;

/// The part of the depth of the trough of an offset with no transform fault
/// at a full spreading rate in mm/yr.
fn offset_trough_part(full_rate: f64) -> f64 {
    mix(
        1.0,
        OFFSET_FAST,
        smoothstep(OFFSET_RATE[0], OFFSET_RATE[1], full_rate),
    )
}

/// The part of the depth of a trough that sediment fills, over this number
/// of millions of years.
const TROUGH_FILL: f64 = 0.15;
const TROUGH_MYR: f64 = 20.0;

/// `TRANSVERSE_SHARE` of the transform faults have a transverse ridge on
/// one side of the trough, `TRANSVERSE` times as high as the trough is deep.
/// Its middle is `TRANSVERSE_AT` widths of the trough from the trace, and
/// its half width is `TRANSVERSE_WIDTH` widths of the trough.
const TRANSVERSE_SHARE: f64 = 0.5;
const TRANSVERSE: f64 = 0.6;
const TRANSVERSE_AT: f64 = 1.4;
const TRANSVERSE_WIDTH: f64 = 0.9;

/// The depth of a deep at an end of a crest at a transform fault for each
/// kilometer of offset, and its largest depth, in kilometers. Past the
/// blend, the deep is `NODAL_ALONG_KM` long along the transform fault and
/// `NODAL_KM` wide. Its middle is `NODAL_IN` of its length inside the
/// transform fault.
const NODAL_DEPTH: f64 = 0.005;
const NODAL_MAX: f64 = 0.6;
const NODAL_KM: f64 = 8.0;
const NODAL_ALONG_KM: f64 = 30.0;
const NODAL_IN: f64 = 0.3;

/// The depth of the axial valley at a full spreading rate in millimeters
/// per year, in meters, at the magma part of that rate (Chen and Morgan
/// 1990; Ito and Behn 2008). Less magma gives a deeper valley: the depth
/// is in proportion to the part of the spreading that the faults take, to
/// the power `VALLEY_POWER`, up to `VALLEY_MOST` times the depth of the
/// table.
const AXIAL_VALLEY: [(f64, f64); 4] = [(20.0, 1500.0), (35.0, 1000.0), (50.0, 450.0), (70.0, 0.0)];
const VALLEY_POWER: f64 = 1.0;
const VALLEY_MOST: f64 = 1.5;

/// The height of the axial high at a full spreading rate in millimeters
/// per year, in meters. Near a magma part of 1, a high of up to
/// `MAGMA_HIGH` meters takes the place of the valley.
const AXIAL_HIGH: [(f64, f64); 3] = [(60.0, 0.0), (90.0, 250.0), (130.0, 350.0)];
const MAGMA_HIGH: f64 = 300.0;
const HIGH_MAGMA: f64 = 0.85;

/// The rise is flat over `FLOOR_KM` kilometers on each side of the crest
/// when the axial valley is `VALLEY_FLAT` kilometers deep or deeper. The
/// axial high is `HIGH_KM` kilometers wide on each side of the axis.
const FLOOR_KM: f64 = 8.0;
const VALLEY_FLAT: f64 = 0.5;
const HIGH_KM: f64 = 5.0;

/// Half of the width of the floor of the axial valley, and the width of
/// each wall, in kilometers, at the middle and at the ends of a segment.
/// The depth of the valley is `VALLEY_ALONG` less than the mean at the
/// middle, and that much more at the ends (Macdonald 1982).
const FLOOR_HALF: [f64; 2] = [3.0, 5.0];
const WALL_WIDTH: [f64; 2] = [6.0, 9.0];
const VALLEY_ALONG: f64 = 0.15;

/// The axial valley is a down-warp of `DOWN` of its depth that is
/// `DOWN_KM` kilometers wide on each side of the axis, and a staircase of
/// `WALL_FAULTS` large faults in each wall that face the axis (Behn and Ito
/// 2008). The throws of the faults of a wall add up to `STAIR` of the depth
/// as a mean. Each fault lifts its footwall, and the footwall tilts back
/// and gets lower over `MOUNT_KM` kilometers away from the axis: the rift
/// mountains.
const DOWN: f64 = 0.85;
const DOWN_KM: f64 = 25.0;

/// Past the walls of the axial valley, the young crust is a band that is
/// `BAND` times the depth of the axial valley high, and `BAND_KM` kilometers
/// wide on each side of the axis, or `BAND_RADIUS` of the brush radius if
/// that is less.
const BAND: f64 = 0.25;
const BAND_KM: f64 = 120.0;
const BAND_RADIUS: f64 = 0.25;
const WALL_FAULTS: i32 = 4;
const STAIR: f64 = 0.65;
const MOUNT_KM: f64 = 24.0;

/// A wall fault is up to `WALL_JITTER` of the space between two wall
/// faults from its place, and its pieces are from `WALL_PIECE[0]` to
/// `WALL_PIECE[1]` kilometers long.
const WALL_JITTER: f64 = 0.3;
const WALL_PIECE: [f64; 2] = [15.0, 40.0];

/// At an offset with no transform fault, the axis of the axial valley bends
/// to the next crest over this part of the segment.
const JOG: f64 = 0.25;

/// Out to this distance from the crest, in kilometers, the rise follows
/// the axis of the axial valley more than the crest.
const RISE_BEND_KM: f64 = 70.0;

/// Near a transform fault, over `HOOK_PART` of the segment, the valley
/// loses `VALLEY_END` of its depth, and the nodal deep takes its place.
/// There, the rift mountains are `INSIDE_CORNER` times as high on the side
/// of the transform fault (Tucholke and Lin 1994), and they fade on the
/// other side.
const VALLEY_END: f64 = 0.5;
const INSIDE_CORNER: f64 = 1.25;

/// The rift mountains go above the Crest setting by no more than this part
/// of the depth of the axial valley.
const ABOVE_CREST: f64 = 0.2;

/// The lumps of lava on the floor of the axial valley are in cells of
/// `LUMP_KM` kilometers, or of `LUMP_TEXELS` texels if that is larger. A
/// lump is up to `LUMP_HEIGHT` kilometers high, and its radius is from
/// `LUMP_SIZE` of a cell to one cell.
const LUMP_KM: f64 = 4.0;
const LUMP_TEXELS: f64 = 1.2;
const LUMP_HEIGHT: f64 = 0.25;
const LUMP_SIZE: f64 = 0.4;

/// The axial volcanic ridge is up to `AVR_HEIGHT` kilometers high and
/// `AVR_KM` kilometers wide on each side of the axis. It is absent over
/// `AVR_END` of the segment at each end. Its gaps come from a noise along
/// the line with cells of `AVR_PIECE_KM` kilometers, and `AVR_GAPS` sets
/// how much of the noise is a gap.
const AVR_HEIGHT: f64 = 0.25;
const AVR_KM: f64 = 2.0;
const AVR_END: f64 = 0.12;
const AVR_PIECE_KM: f64 = 18.0;
const AVR_GAPS: f64 = 0.3;

/// The sediment on the sea floor, in kilometers for each million years of
/// age (Goff and Arbic 2010). It ponds in the lows between the fault
/// blocks up to twice its thickness, and fills `POND` of the depth of a
/// low below that level, with a round edge of `SEDIMENT_SOFT` of the
/// level. All are in kilometers of throw.
const SEDIMENT_RATE: f64 = 0.012;
const SEDIMENT_SOFT: f64 = 0.3;
const POND: f64 = 0.3;
const HILL_LOW: f64 = 1.0;

/// The rise is a power of its curve, from 0 at the foot to 1 at the
/// crest. The power is `e` to minus `SWELL` times a noise with features of
/// about one brush radius, so the rise swells where the noise is above
/// zero and sinks where it is below.
const SWELL: f64 = 1.4;

/// The largest sag of the whole rise, as a part of the lift. The sag
/// follows a noise with features of about two brush radii, where the noise
/// is above zero.
const SAG: f64 = 0.4;

/// The width of the rise at an end of the line, as a part of its full
/// width.
pub(crate) const TAPER_WIDTH: f64 = 0.2;

/// The top of the rise is round over about this number of texels.
const CREST_ROUND: f64 = 2.0;

/// The foot of the rise is round below about this part of the height.
const FOOT: f64 = 0.25;

/// The depth of the sea floor below the crest at an age in millions of
/// years, in meters (Stein and Stein 1992).
fn depth(age: f64) -> f64 {
    if age < 20.0 {
        2600.0 + 365.0 * age.sqrt()
    } else {
        5651.0 - 2473.0 * (-0.0278 * age).exp()
    }
}

/// The fit factor of the abyssal hills at half of the texel step in
/// kilometers: how much larger the hills are than on real sea floor. It is
/// the Scale setting, from 1 to 10. With `fit`, a map with texels larger
/// than `FIT_TEXEL_KM` gets hills at least as large as its texels over
/// that size, so their scarps are a few texels wide.
pub(crate) fn fit_of(half_texel: f64, fit: bool, scale: f64) -> f64 {
    let scale = scale.clamp(1.0, 10.0);
    if fit {
        scale.max(2.0 * half_texel / FIT_TEXEL_KM)
    } else {
        scale
    }
}

/// The scale of the height of the faults at a fit factor. Up to
/// `FIT_HEIGHT_FULL`, it is the factor, so the slopes stay. Past it, it
/// grows with the root of the factor, so large blocks are lower than
/// slabs, up to `FIT_HEIGHT_MAX`, so the relief stays plausible.
pub(crate) fn fit_height(fit: f64) -> f64 {
    let full = fit.min(FIT_HEIGHT_FULL);
    (full * (fit / full).sqrt()).min(FIT_HEIGHT_MAX)
}

/// The patch noise at a unit direction on a world with a radius in
/// kilometers: a number from -1.5 to 1.5 that changes over about
/// `PATCH_KM`.
pub(crate) fn patch_noise(p: V3, km: f64, seed: u32) -> f64 {
    let patch = |size: f64| noise3(p.map(|c| c * km / size), seed ^ SALT_PATCH);
    patch(PATCH_KM)[0] + 0.5 * patch(PATCH_FINE * PATCH_KM)[1]
}

/// The throw scale and the part of the faults that show, at the patch
/// noise of `RidgeNoise`.
pub(crate) fn patch_of(noise: f64) -> [f64; 2] {
    let w = smoothstep(-PATCH_EDGE, PATCH_EDGE, noise);
    [
        mix(PATCH_THROW[0], PATCH_THROW[1], w),
        mix(PATCH_DENSITY[0], PATCH_DENSITY[1], w),
    ]
}

/// The RMS height of the abyssal hills at a full spreading rate in
/// millimeters per year, in kilometers.
fn hill_rms(full_rate: f64) -> f64 {
    HILL_RMS / full_rate.max(HILL_SLOWEST).sqrt()
}

/// The magma part of the spreading at a full spreading rate in millimeters
/// per year.
fn magma_share(full_rate: f64) -> f64 {
    lerp_table(&MAGMA_SHARE, full_rate)
}

/// Points of a function, and the function between them. Past the first and
/// the last point, the function is flat.
fn lerp_table(table: &[(f64, f64)], x: f64) -> f64 {
    let (first, last) = (table[0], table[table.len() - 1]);
    if x <= first.0 {
        return first.1;
    }
    for pair in table.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    last.1
}

/// The magma part of the spreading at a full spreading rate in millimeters
/// per year (Behn and Ito 2008).
const MAGMA_SHARE: [(f64, f64); 3] = [(20.0, 0.75), (50.0, 0.85), (100.0, 0.95)];

/// The distance between faults at a magma part, in kilometers (Buck,
/// Lavier, and Poliakov 2005; Behn and Ito 2008).
const FAULT_SPACING: [(f64, f64); 5] = [(0.5, 8.0), (0.6, 5.0), (0.7, 4.0), (0.8, 3.0), (0.9, 2.0)];

/// The tangent of the dip of a fault, about 50 degrees.
const DIP_TAN: f64 = 1.2;

/// The 16-bit steps of one kilometer of height.
pub(crate) const LEVELS_PER_KM: f64 = 1000.0 * 65535.0 / (ELEV_MAX - ELEV_MIN);

/// The texel size up to which the hills have their real size, in
/// kilometers, and the scales of their height of `fit_height`.
const FIT_TEXEL_KM: f64 = 0.5;
const FIT_HEIGHT_FULL: f64 = 2.0;
const FIT_HEIGHT_MAX: f64 = 3.0;

/// The RMS height of all abyssal hills times the square root of the full
/// spreading rate, in kilometers, for rates in millimeters per year from
/// `HILL_SLOWEST`. It gives about 220 m at 20 mm/yr and 95 m at 110 mm/yr
/// (Malinverno 1991; Goff 1991; Goff and others 1997).
const HILL_RMS: f64 = 1.0;
const HILL_SLOWEST: f64 = 10.0;

/// The shortest and the longest fault of the hills, in kilometers. The
/// lengths have a log-normal shape between the limits, and most are long.
const FAULT_LENGTH: [f64; 2] = [15.0, 60.0];

/// Larger fault spacings give longer faults and wider blocks: the lengths
/// scale by the spacing over `HILL_SPACING` kilometers, from 1 to
/// `HILL_SCALE_MAX`.
const HILL_SPACING: f64 = 2.0;
const HILL_SCALE_MAX: f64 = 2.0;

/// The cells of the faults: along the fabric in kilometers, and across it
/// in fault spacings, with `HILLS` faults in each cell.
const HILL_CELL: [f64; 2] = [25.0, 3.0];
const HILLS: u32 = 2;

/// The largest turn of a fault from the fabric, as a tangent.
const HILL_TILT: f64 = 0.05;

/// The width of a scarp, and the shortest and the longest back tilt of a
/// block, in kilometers.
const SCARP_KM: f64 = 0.5;
const BACK_TILT: [f64; 2] = [3.0, 8.0];

/// The part of the faults that are pairs across a graben, and the
/// narrowest and the widest graben, in kilometers.
const GRABENS: f64 = 0.2;
const GRABEN: [f64; 2] = [0.3, 2.0];

/// The throw of a fault in height units: a smallest throw plus a part
/// from an exponential distribution, up to `THROW_MAX`. With the noise
/// along the fault, the throw bends down to at most `THROW_CAP`.
const THROW_MIN: f64 = 0.3;
const THROW_MAX: f64 = 2.0;
const THROW_CAP: f64 = 1.1;

/// The patches of the floor: a noise with features of about `PATCH_KM`
/// kilometers, plus half of a noise with features of `PATCH_FINE` of that.
/// From the low end of the noise to the high end of `PATCH_EDGE`, the
/// throw scale goes over `PATCH_THROW` and the part of the faults that show
/// over `PATCH_DENSITY`. A fault fades out over `SHOW_SOFT` of that part.
const PATCH_KM: f64 = 80.0;
const PATCH_FINE: f64 = 0.43;
const PATCH_EDGE: f64 = 0.6;
const PATCH_THROW: [f64; 2] = [0.5, 1.3];
const PATCH_DENSITY: [f64; 2] = [0.4, 0.85];
const SHOW_SOFT: f64 = 0.15;

/// The fractal relief has `BUMP_OCTAVES` octaves of noise from cells of
/// `BUMP_KM` across the fabric and `BUMP_ASPECT` times that along it, times
/// the fit factor. Each octave has `BUMP_GAIN` of the height of the octave
/// before it, and the sum is `BUMPS` of the height of a hill.
const BUMP_OCTAVES: usize = 4;
const BUMP_KM: f64 = 6.0;
const BUMP_ASPECT: f64 = 1.5;
const BUMP_GAIN: f64 = 0.9;
const BUMPS: f64 = 3.0;

/// The part of the fractal relief that a fast ridge keeps.
const BUMP_FAST: f64 = 0.25;

/// The fractal relief fades to `BUMP_OLD` of its height over `BUMP_MYR`
/// million years of age.
const BUMP_OLD: f64 = 0.4;
const BUMP_MYR: f64 = 25.0;

/// The cells of the noise of the ragged seams, in kilometers, times the fit
/// factor.
const SEAM_SIZE_KM: f64 = 24.0;

/// The largest move of a texel along the line by the ragged seams, in
/// kilometers, times the fit factor.
const SEAM_KM: f64 = 4.0;

/// On a ridge, the age and the segments give the floor its order, so the
/// patches have `RIDGE_PATCH` of the contrast that they have under the
/// Abyssal hills mode, and no area far from the axis stands out.
const RIDGE_PATCH: f64 = 0.35;

/// The warp of the faults: its feature size along and across the fabric,
/// and its largest move across the fabric, in kilometers.
const HILL_WARP: [f64; 2] = [40.0, 10.0];
const HILL_WARP_KM: f64 = 1.5;

/// The segments of a fault: their length in kilometers, the part of them
/// below which a segment is a gap, and the overlap of two segments, as a
/// part of a segment.
const LINK_KM: f64 = 10.0;
const LINK_GAP: f64 = 0.1;
const RELAY: f64 = 0.3;

/// The largest step of a fault trace between segments, in kilometers, and
/// the roughness of the trace: its largest move across the fabric and its
/// feature size along it, in kilometers.
const STEP_KM: f64 = 0.6;
const ROUGH_KM: f64 = 0.15;
const ROUGH_ALONG: f64 = 2.5;

/// The noise along a fault: the largest change of the throw, the back
/// tilt, and the width of a graben at two feature sizes in kilometers; the
/// largest change of the width of the scarp; the largest move of the scarp
/// and the largest change of the height of its face, with their feature
/// sizes in kilometers. The face changes make spurs and gullies, as on a
/// scarp with talus.
const VARY: [f64; 2] = [0.35, 0.2];
const VARY_KM: [f64; 2] = [5.0, 1.5];
const SHARP: f64 = 0.6;
const FACE_ROUGH_KM: f64 = 0.08;
const FACE_KM: f64 = 0.9;
const GULLY: f64 = 0.3;
const GULLY_KM: f64 = 1.0;

/// The roughness of the blocks: its largest change of their height, its
/// feature size in kilometers, and the turn of its lattice from the
/// fabric in radians.
const GRAIN: f64 = 0.15;
const GRAIN_KM: f64 = 0.8;
const GRAIN_TURN: f64 = 0.6;

/// The fine texture: its part of the RMS height of the hills, its cell
/// across the fabric and its stretch along it, the kernels in each cell,
/// and its characteristic width, all in kilometers. The wavelengths go
/// over `GABOR_OCTAVES` octaves of the corner wavenumber, with a Hurst
/// number of `HURST` (Goff and Jordan 1988; Goff 1991).
const GABOR_SHARE: f64 = 1.0 / 3.0;
const GABOR_CELL: f64 = 4.0;
const GABOR_ASPECT: f64 = 1.5;

/// The stretch of a wave of the texture along the fabric, at the longest
/// and at the shortest wavelength.
const GABOR_STRETCH: [f64; 2] = [3.0, 1.0];
const GABOR_KERNELS: u32 = 3;
const GABOR_WIDTH: f64 = 2.5;
const GABOR_OCTAVES: [f64; 2] = [-1.0, 4.0];
const HURST: f64 = 0.9;

/// The RMS of the texture in its height units. See the test
/// `the_texture_has_its_rms_height`.
const GABOR_RMS_UNIT: f64 = 0.336;

/// The RMS height and the mean height of the hills, in height units. See
/// the test `the_hills_have_their_rms_and_mean_height`.
const HILL_RMS_UNIT: f64 = 0.479;
/// The texture unit over the throw unit, so the texture has its part of
/// the RMS height.
const GABOR_PER_THROW: f64 =
    GABOR_SHARE * HILL_RMS_UNIT / (GABOR_RMS_UNIT * 0.942_809_041_582_063_4);

const HILL_MEAN_UNIT: f64 = 0.770;

/// The part of the single faults that face the axis.
const INWARD: f64 = 0.8;

/// The largest move of the middle of a piece of a wall fault along the
/// line, and the largest overlap of two pieces, in piece lengths.
const PIECE_JITTER: f64 = 0.15;
const OVERLAP: f64 = 0.3;

/// The largest smoothing of a wall fault along a piece, in piece lengths.
const MAX_SMOOTH: f64 = 0.3;

/// The highest hills, in throw units. Ground this close below the Crest
/// setting gets lower hills.
const HILL_TOP: f64 = 4.0;

/// The throw of a fault grows over this number of fault spacings from the
/// axis.
const GROW: f64 = 1.5;

/// The blocks start to fade at this number of brush radii from the line.
const HILL_FADE: f64 = 0.8;

/// A fault block of throw 1 at `y` fault spacings from its scarp, away from
/// the axis for a fault that faces the axis. The scarp is `scarp` wide, and
/// the block tilts back over `w` behind it. The scarp and the back are
/// smoothstep curves, so the foot, the top, and the end of the block are
/// round. The block is the mean over a box of half width `b`, so it is zero
/// at `-scarp - b` and before, and at `w + b` and after.
fn block(y: f64, scarp: f64, w: f64, b: f64) -> f64 {
    if y + b <= -scarp || y - b >= w {
        return 0.0;
    }
    // The integral of the block from `-scarp` to `y`.
    let area = |y: f64| {
        if y <= -scarp {
            0.0
        } else if y < 0.0 {
            let t = (y + scarp) / scarp;
            scarp * t * t * t * (1.0 - 0.5 * t)
        } else if y < w {
            let t = y / w;
            0.5 * scarp + w * t * (1.0 - t * t * (1.0 - 0.5 * t))
        } else {
            0.5 * (scarp + w)
        }
    };
    (area(y + b) - area(y - b)) / (2.0 * b)
}

/// The throw along a piece at `q`, from -1 at its start to 1 at its end:
/// `sqrt(1 - q * q)`, as the mean over a box of half width `b`. It is zero at
/// `1 + b` and more from the middle.
fn ellipse(q: f64, b: f64) -> f64 {
    let area = |x: f64| {
        let x = x.clamp(-1.0, 1.0);
        0.5 * (x * (1.0 - x * x).sqrt() + x.asin())
    };
    (area(q + b) - area(q - b)) / (2.0 * b)
}

/// `soft_cap` changes no number below this part of the cap.
const CAP_KNEE: f64 = 0.85;

/// `value` if it is below `CAP_KNEE` of `cap`. Above that, a number that
/// bends toward `cap` and is never above it.
fn soft_cap(value: f64, cap: f64) -> f64 {
    let knee = CAP_KNEE * cap;
    if value <= knee {
        return value;
    }
    let room = cap - knee;
    knee + room * ((value - knee) / room).tanh()
}

/// The width in radians over which two crest segments blend, on a
/// heightmap with texel steps of `step` radians.
fn blend_width(step: f64, world_km: f64) -> f64 {
    1.5 * step + VALLEY_KM / world_km
}

/// As `noise1`, with straight lines between the values, so the noise has a
/// kink at each whole number.
fn kink1(x: f64, a: u32, b: u32, seed: u32) -> (f64, f64) {
    let n = x.floor();
    let (s, n) = (x - n, n as i32);
    let value = |c: i32| 2.0 * unit(hash(c as u32, a, b, seed)) - 1.0;
    let (h0, h1) = (value(n), value(n.wrapping_add(1)));
    (h0 + (h1 - h0) * s, h1 - h0)
}

/// As `noise1`, with a cubic curve through four values. The curve has no
/// flat part at each whole number, so a change goes over the full cell.
fn noise1_round(x: f64, a: u32, b: u32, seed: u32) -> (f64, f64) {
    let n = x.floor();
    let (s, n) = (x - n, n as i32);
    let value = |c: i32| 2.0 * unit(hash(c as u32, a, b, seed)) - 1.0;
    let [h0, h1, h2, h3] = [-1, 0, 1, 2].map(|i| value(n.wrapping_add(i)));
    // Catmull-Rom: the slope at each value is half of the change over the
    // two cells next to it.
    let (m1, m2) = (0.5 * (h2 - h0), 0.5 * (h3 - h1));
    let (s2, s3) = (s * s, s * s * s);
    let value = (2.0 * s3 - 3.0 * s2 + 1.0) * h1
        + (s3 - 2.0 * s2 + s) * m1
        + (-2.0 * s3 + 3.0 * s2) * h2
        + (s3 - s2) * m2;
    let slope = (6.0 * s2 - 6.0 * s) * (h1 - h2)
        + (3.0 * s2 - 4.0 * s + 1.0) * m1
        + (3.0 * s2 - 2.0 * s) * m2;
    (value.clamp(-1.0, 1.0), slope)
}

/// The larger of `a` and `b`, with a round join over `k`. With `k` zero, it
/// is the larger number.
fn smooth_max(a: f64, b: f64, k: f64) -> f64 {
    if k <= 0.0 {
        return a.max(b);
    }
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.max(b) + 0.25 * k * h * h
}

/// Ground at `height` with sediment that ponds up to `level`. A low below
/// the level fills by `POND` of its depth, and ground above it does not
/// change.
fn pond(height: f64, level: f64) -> f64 {
    let full = smooth_max(height, level, SEDIMENT_SOFT * level);
    height + POND * (full - height)
}

/// `a` to `b`, at `t` from 0 to 1.
fn mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::FACES;
    use crate::heightmap::{Heightmap, TexelRect};
    use crate::line::{LINE_SEGMENTS, LINE_SPACING, LinePlace};
    use crate::math::lonlat_to_dir;
    use crate::math::{angle, cross, length, normalize};

    const START: u16 = 10000;
    const CREST: u16 = 30000;
    const LIFT: f64 = 20000.0;
    /// The face size of the tests of the formula. The texel step is small,
    /// so the round parts of the formula are small.
    const FINE: usize = 4096;

    fn spec() -> Ridge {
        Ridge {
            radius: 0.1,
            crest: CREST,
            segment: 0.1,
            offset: 0.0,
            spread: 12.5 / 6371.0,
            magma: 0.0,
            world_km: 6371.0,
            seed: 7,
            limits: Limits::default(),
            hills: 1.0,
            fit: false,
            scale: 1.0,
        }
    }

    /// A plan with no segments and no relief, for the formula of the rise.
    fn shape(spec: Ridge, n: usize) -> RidgePlan {
        let mut line = RidgeLine::new(spec, n);
        line.relief = 0.0;
        line.plan(0)
    }

    /// A plan with no segments, for the formula.
    fn rough(spec: Ridge, n: usize) -> RidgePlan {
        RidgeLine::new(spec, n).plan(0)
    }

    fn at(u: f64, across: f64) -> LinePoint {
        LinePoint {
            u,
            across,
            past: 0.0,
            flow: 1.0,
            end: f64::MAX,
        }
    }

    /// Noise with no bend.
    fn calm() -> RidgeNoise {
        RidgeNoise {
            bend: [0.0; 3],
            slow: [-1.0; 3],
            patch: 0.0,
            seam: 0.0,
        }
    }

    /// The level at a place with the calm noise.
    fn level(plan: &RidgePlan, u: f64, across: f64) -> f64 {
        f64::from(plan.level(START, at(u, across), calm()))
    }

    /// The level at a place with the noise of that place. The line is the
    /// equator, from longitude 0 to the east.
    fn real(plan: &RidgePlan, u: f64, across: f64) -> f64 {
        let p = lonlat_to_dir(u.to_degrees(), across.to_degrees());
        f64::from(plan.level(START, at(u, across), plan.noise(p)))
    }

    /// The place of boundary `i` along the line at the crest, with no bend.
    fn boundary(plan: &RidgePlan, i: i32) -> f64 {
        plan.boundary(&plan.segmenting(), i)
    }

    /// The place of the trace of boundary `j` along the line at `across`.
    fn trace_at(plan: &RidgePlan, j: i32, across: f64) -> f64 {
        let sg = plan.segmenting();
        let turn = plan.turn(&sg, across / plan.ridge.spread);
        plan.trace(&sg, j, across, turn).u
    }

    /// The middle of crest segment `k`.
    fn middle(plan: &RidgePlan, k: i32) -> f64 {
        0.5 * (boundary(plan, k) + boundary(plan, k + 1))
    }

    /// The width of the rise on one side of the crest: the first distance
    /// with no rise.
    fn width(plan: &RidgePlan, noise: RidgeNoise, u: f64, side: f64) -> f64 {
        let r = plan.ridge.radius;
        let mut places = (1..=3000).map(|i| r * f64::from(i) / 2000.0);
        let end = places.find(|a| plan.level(START, at(u, side * a), noise) == START);
        end.unwrap()
    }

    #[test]
    fn the_rise_follows_the_depth_of_the_age_and_ends_at_the_radius() {
        let plan = shape(spec(), FINE);
        let r = plan.ridge.radius;
        let v = plan.ridge.spread;
        // The age at the radius is 51 million years, so the depth has the
        // two parts of the curve.
        assert!(r / v > 50.0 && r / v < 52.0);
        let round = CREST_ROUND * plan.step() / v;
        let age = |d: f64| (d / v).hypot(round) - round;
        for k in 1..12 {
            let u = middle(&plan, k);
            // With the calm noise, the crest has the Crest setting.
            let top = level(&plan, u, 0.0);
            assert_eq!(top, f64::from(CREST), "{k}");
            for side in [-1.0, 1.0] {
                let width = width(&plan, calm(), u, side);
                assert!(width > 0.97 * r && width <= r, "{k}: {width}");
                let mut last = top;
                for i in 1..=200 {
                    let d = r * f64::from(i) / 200.0;
                    let new = level(&plan, u, side * d);
                    let height = (new - f64::from(START)) / LIFT;
                    let foot = depth(age(r));
                    let w = (foot - depth(age(d))) / (foot - depth(0.0));
                    let want = w * w * (1.0 + FOOT) / (w + FOOT);
                    assert!((height - want).abs() < 1e-4, "{k} {i}: {height} {want}");
                    assert!(new <= last, "{k} {i}");
                    last = new;
                }
            }
            for across in [r, 1.2 * r, -r, -1.49 * r] {
                assert_eq!(level(&plan, u, across), f64::from(START), "{across}");
            }
        }
        // The depth of Stein and Stein, in meters.
        assert_eq!(depth(0.0), 2600.0);
        assert!((depth(4.0) - 3330.0).abs() < 1e-9);
        assert!((depth(19.999) - depth(20.0)).abs() < 10.0);
        assert!((depth(100.0) - 5497.6).abs() < 0.1);
    }

    #[test]
    fn the_noise_bends_the_crest() {
        // A fast ridge has long crest segments.
        let fast = Ridge {
            spread: 45.0 / 6371.0,
            ..spec()
        };
        let plan = shape(fast, FINE);
        let r = plan.ridge.radius;
        let step = r / 1000.0;
        // The place of the crest in the middle half of a crest segment.
        let mut most: f64 = 0.0;
        for k in 1..30 {
            let (from, to) = (boundary(&plan, k), boundary(&plan, k + 1));
            let crests: Vec<f64> = (0..=20)
                .map(|i| {
                    let u = mix(from, to, 0.25 + 0.5 * f64::from(i) / 20.0);
                    let places = (-150..=150).map(|j| f64::from(j) * step);
                    let highest =
                        places.max_by(|a, b| real(&plan, u, *a).total_cmp(&real(&plan, u, *b)));
                    highest.unwrap()
                })
                .collect();
            let low = crests.iter().copied().fold(f64::MAX, f64::min);
            let high = crests.iter().copied().fold(f64::MIN, f64::max);
            // The bend is 0.12 of the radius at most.
            assert!(low >= -0.121 * r && high <= 0.121 * r, "{k}: {low} {high}");
            most = most.max(high - low);
        }
        assert!(most > 0.02 * r, "{most}");
    }

    #[test]
    fn the_flow_scales_the_rise() {
        let plan = shape(spec(), FINE);
        let u = middle(&plan, 1);
        for across in [0.0, 0.01, 0.03, 0.08] {
            let rise = |flow: f64| {
                let point = LinePoint {
                    flow,
                    ..at(u, across)
                };
                f64::from(plan.level(START, point, calm())) - f64::from(START)
            };
            assert!(rise(1.0) > 0.0);
            assert!((rise(0.5) - 0.5 * rise(1.0)).abs() <= 1.0, "{across}");
            assert_eq!(rise(0.0), 0.0);
        }
    }

    /// Noise with a bend.
    fn wild() -> RidgeNoise {
        RidgeNoise {
            bend: [1.5, -1.5, 1.5],
            slow: [1.0, -1.0, 0.3],
            patch: 1.5,
            seam: 1.5,
        }
    }

    #[test]
    fn the_ridge_raises_the_ground_up_to_the_crest() {
        let rough_spec = Ridge {
            offset: 0.03,
            ..spec()
        };
        let plan = rough(rough_spec, FINE);
        let smooth = shape(rough_spec, FINE);
        let straight = rough(
            Ridge {
                offset: 0.0,
                ..rough_spec
            },
            FINE,
        );
        let r = plan.ridge.radius;
        // A trough, two deeps, and the axial valley can be at one place. The
        // rift mountains go above the crest.
        let f = plan.faulting();
        let (valley, high) = plan.axial(&f);
        let floor = DOWN * (1.0 + VALLEY_ALONG) * valley;
        let deepest = (TROUGH_MAX + 2.0 * NODAL_MAX + floor) * f.level;
        // The level is a whole number, so it can be one step higher.
        let highest = f64::from(CREST) + (ABOVE_CREST * valley + high) * f.level + 1.0;
        let (mut past_foot, mut hills, mut lower, mut count) = (0, 0, 0, 0);
        for noise in [calm(), wild(), RidgeNoise::default()] {
            for j in 0..60 {
                let u = f64::from(j) * 0.0537;
                for i in 0..=300 {
                    let across = r * (f64::from(i) / 100.0 - 1.5);
                    // Ground at the crest level or above it does not change.
                    for start in [CREST, CREST + 1, CREST + 5000] {
                        let new = plan.level(start, at(u, across), noise);
                        assert_eq!(new, start, "{u} {across}");
                    }
                    // The ground goes above the crest by no more than the
                    // rift mountains. Only a trough, a deep, or the axial
                    // valley takes it down, by no more than its depth.
                    let new = plan.level(START, at(u, across), noise);
                    assert!(f64::from(new) <= highest, "{u} {across}");
                    assert!(f64::from(new) >= f64::from(START) - deepest, "{u} {across}");
                    lower += usize::from(new < START);
                    count += 1;
                    // With no offset, there is no trough and no deep.
                    let flat = f64::from(straight.level(START, at(u, across), noise));
                    let valley_floor = f64::from(START) - floor * f.level;
                    assert!((valley_floor..=highest).contains(&flat), "{u} {across}");
                    // Past the foot of the rise, the blocks only raise the
                    // ground.
                    let base = smooth.level(START, at(u, across), noise);
                    assert!(base >= START, "{u} {across}");
                    if base == START && new > START {
                        past_foot += 1;
                    }
                    hills += usize::from(new != base);
                    // Ground a small distance below the crest gets a small
                    // part of the blocks, the troughs, and the rift
                    // mountains.
                    let near = plan.level(CREST - 10, at(u, across), noise);
                    assert!(
                        (CREST - 30..=CREST + 5).contains(&near),
                        "{u} {across} {near}"
                    );
                }
            }
        }
        assert!(hills > 30000, "{hills}");
        assert!(past_foot > 1000, "{past_foot}");
        assert!(lower > 0 && lower < count / 20, "{lower} {count}");
    }

    /// The mean level over many places along the line at `km` kilometers
    /// from the line, with the calm noise.
    fn mean_at(plan: &RidgePlan, km: f64) -> f64 {
        let across = km / plan.ridge.world_km;
        let levels = (0..400).map(|i| level(plan, 0.3 + f64::from(i) * 0.0013, across));
        levels.sum::<f64>() / 400.0
    }

    /// A ridge with no offset at a full spreading rate in millimeters per
    /// year.
    fn at_rate(rate: f64) -> RidgePlan {
        let spec = Ridge {
            spread: 0.5 * rate / 6371.0,
            ..spec()
        };
        rough(spec, FINE)
    }

    #[test]
    fn the_axial_valley_follows_the_spreading_rate() {
        // The valley is deep at a slow rate, shallow at an intermediate
        // rate, and gone at a fast rate, where a high takes its place.
        let depths = [20.0, 40.0, 60.0, 100.0].map(|rate| {
            let plan = at_rate(rate);
            plan.axial(&plan.faulting())
        });
        assert!((1.0..=2.0).contains(&depths[0].0), "{depths:?}");
        assert!(
            depths[1].0 < 0.6 * depths[0].0 && depths[1].0 > 0.2,
            "{depths:?}"
        );
        assert!(depths[2].0 < 0.3, "{depths:?}");
        assert_eq!(depths[3].0, 0.0);
        assert!((0.2..=0.4).contains(&depths[3].1), "{depths:?}");
        assert_eq!(depths[0].1, 0.0);
        // Less magma gives a deeper valley.
        let plan = shape(
            Ridge {
                magma: -0.1,
                ..at_rate(20.0).ridge
            },
            FINE,
        );
        assert!(plan.axial(&plan.faulting()).0 > depths[0].0);

        // At a slow rate, the floor is below the crest, and the rift
        // mountains are about the depth of the valley above the floor.
        let plan = at_rate(20.0);
        let f = plan.faulting();
        let valley = depths[0].0 * f.level;
        let crest = f64::from(CREST);
        let floor = mean_at(&plan, 0.0);
        assert!(floor < crest - 0.8 * valley, "{floor} {valley}");
        let profile: Vec<f64> = (0..=40).map(|km| mean_at(&plan, f64::from(km))).collect();
        let top = (0..=40)
            .max_by(|a, b| profile[*a].total_cmp(&profile[*b]))
            .unwrap();
        assert!((15..=35).contains(&top), "{top}");
        let relief = (profile[top] - floor) / valley;
        assert!((0.8..=1.3).contains(&relief), "{relief}");

        // The walls are staircases of scarps that face the axis. Away
        // from the axis, a scarp goes up and the ground behind it tilts
        // down a little.
        let fine = Faulting {
            half_texel: 1e-3,
            ..plan.faulting()
        };
        let zone = [FLOOR_HALF[0], WALL_WIDTH[0]];
        let (mut scarps, mut down) = (0, 0.0f64);
        for i in 0..200 {
            let along = f64::from(i) * 3.7;
            let mut rising = false;
            let mut last = plan.walls(&fine, [3, i % 2], along, 0.0, zone);
            for j in 1..=1500 {
                let x = f64::from(j) * 0.02;
                let next = plan.walls(&fine, [3, i % 2], along, x, zone);
                let slope = (next - last) / 0.02;
                if slope > 0.2 && !rising {
                    scarps += 1;
                }
                rising = slope > 0.2;
                down = down.min(slope);
                last = next;
            }
        }
        let scarps = f64::from(scarps) / 200.0;
        assert!((2.5..=4.5).contains(&scarps), "{scarps}");
        assert!(down > -0.1, "{down}");

        // The floor is rough: lumps and an axial volcanic ridge with gaps.
        let along: Vec<f64> = (0..2000)
            .map(|i| level(&plan, 0.3 + f64::from(i) * 0.5 / 6371.0, 0.0))
            .collect();
        let change = along.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / 1999.0;
        assert!(change > 0.007 * valley, "{change}");

        // The valley is deeper and wider at the ends of a segment than at
        // its middle, at an offset with no transform fault too.
        let sg = plan.segmenting();
        let (mut ends, mut middles) = (0.0, 0.0);
        for k in 10..60 {
            if plan.transform(&sg, k) {
                continue;
            }
            let (from, to) = (boundary(&plan, k - 1), boundary(&plan, k));
            for km in [0.0, 6.0] {
                let across = km / plan.ridge.world_km;
                ends += level(&plan, mix(from, to, 0.95), across)
                    + level(&plan, mix(from, to, 0.05), across);
                middles += 2.0 * level(&plan, mix(from, to, 0.5), across);
            }
        }
        assert!(ends < middles, "{ends} {middles}");

        // At a fast rate, the axis is a high above the crest, about 10
        // kilometers wide.
        let plan = at_rate(100.0);
        let high = depths[3].1 * plan.faulting().level;
        let axis = mean_at(&plan, 0.0);
        assert!(axis > crest + 0.8 * high, "{axis} {high}");
        assert!(mean_at(&plan, 5.0) < crest + 0.1 * high);
    }

    #[test]
    fn more_magma_gives_closer_and_smaller_faults() {
        // Behn & Ito (2008): the spacing and the heave of the faults fall
        // as the magma part of the spreading grows.
        let faulting = |magma: f64| shape(Ridge { magma, ..spec() }, FINE).faulting();
        let (low, mid, high) = (faulting(-0.2), faulting(0.0), faulting(0.1));
        assert!(low.magma < mid.magma && mid.magma < high.magma);
        assert!(low.spacing > mid.spacing && mid.spacing > high.spacing);
        // A faster ridge has more magma, so its faults are closer.
        let fast = Ridge {
            spread: 4.0 * spec().spread,
            ..spec()
        };
        assert!(shape(fast, FINE).faulting().spacing < mid.spacing);
    }

    #[test]
    fn sediment_makes_old_sea_floor_smoother() {
        // A slow rate gives old sea floor inside the brush radius.
        let plan = at_rate(12.0);
        let r = plan.ridge.radius;
        // The mean change of the level over one kilometer across the line,
        // near `across`.
        let rough = |across: f64| {
            let step = 1.0 / plan.ridge.world_km;
            let mut sum = 0.0;
            for i in 0..100 {
                for k in 0..20 {
                    let (u, a) = (0.3 + f64::from(i) * 0.0021, across + f64::from(k) * step);
                    sum += (level(&plan, u, a + step) - level(&plan, u, a)).abs();
                }
            }
            sum / 2000.0
        };
        // The sediment fills the lows of old sea floor, and the old sea
        // floor stays rough.
        let (young, old) = (rough(0.15 * r), rough(0.75 * r));
        assert!(old < young && old > 0.5 * young, "{young} {old}");
        // A low fills by a part of its depth, so it keeps a part of its
        // relief. The ground above the level of the sediment keeps all of
        // its relief.
        let fill = 0.4;
        assert!((pond(0.0, fill) - POND * fill).abs() < 1e-12);
        let low = pond(0.2, fill) - pond(0.0, fill);
        assert!((low - 0.2 * (1.0 - POND)).abs() < 1e-12, "{low}");
        assert_eq!(pond(1.0, fill), 1.0);
        assert_eq!(pond(1.5, fill) - pond(1.0, fill), 0.5);
        // Sediment does not reach past the ridge.
        for across in [REACH * r * 1.001, 2.0 * r] {
            assert_eq!(level(&plan, 0.4, across), f64::from(START));
        }
        // The round join is never below the larger number, and it is the
        // larger number far from the other.
        for (a, b) in [(0.0, 0.3), (0.3, 0.0), (0.2, 0.25), (1.0, 0.1)] {
            assert!(smooth_max(a, b, 0.2) >= a.max(b));
        }
        assert_eq!(smooth_max(1.0, 0.1, 0.2), 1.0);
        assert_eq!(smooth_max(0.4, 0.3, 0.0), 0.4);
    }

    #[test]
    fn a_block_is_zero_outside_its_window_and_has_no_step() {
        for (scarp, w, b) in [(0.2, 1.0, 0.7), (0.01, 0.3, 0.05), (0.4, 6.0, 2.0)] {
            assert_eq!(block(-scarp - b - 1e-12, scarp, w, b), 0.0);
            assert_eq!(block(-scarp - b - 0.3, scarp, w, b), 0.0);
            assert_eq!(block(w + b + 1e-12, scarp, w, b), 0.0);
            assert_eq!(block(w + b + 5.0, scarp, w, b), 0.0);
            let mut last = 0.0;
            for i in 0..=4000 {
                let y = -scarp - b + (w + scarp + 2.0 * b) * f64::from(i) / 4000.0;
                let new = block(y, scarp, w, b);
                assert!((0.0..=1.0).contains(&new), "{y}");
                assert!((new - last).abs() < 0.01, "{y}: {last} {new}");
                last = new;
            }
        }
    }

    #[test]
    fn an_old_trace_of_a_small_offset_breaks_and_kinks() {
        let plan = shape(spec(), FINE);
        let sg = plan.segmenting();
        let (mut gaps, mut pieces, mut kinks) = (0, 0, 0);
        for j in 0..40 {
            if plan.transform(&sg, j) {
                continue;
            }
            // Young troughs are whole, and old troughs have gaps.
            assert!(plan.fracture(j, 20.0, false, 0.0).depth > 0.0);
            for i in 0..60 {
                let depth = plan.fracture(j, 20.0, false, 20.0 + f64::from(i)).depth;
                gaps += usize::from(depth < 1e-6);
                pieces += usize::from(depth > 0.2);
            }
            // The path has a step of its slope at a kink.
            let slope = |tau: f64| plan.migration(&sg, j, tau).1 - plan.warp(&sg, j, tau, false).1;
            let at = 30.0 * KINK_MYR;
            kinks += usize::from((slope(at + 0.01) - slope(at - 0.01)).abs() > 1e-6);
        }
        assert!(gaps > 100 && pieces > 100, "{gaps} {pieces}");
        assert!(kinks > 10, "{kinks}");
        // Two traces next to each other do not cross.
        for tau in [0.0, 5.0, 20.0, 80.0] {
            for j in 0..40 {
                let at = |j: i32| plan.boundary(&sg, j) + plan.migration(&sg, j, tau).0;
                assert!(at(j + 1) > at(j), "{j} at {tau} Myr");
            }
        }
    }

    #[test]
    fn the_swell_moves_the_flank_and_keeps_the_crest_and_the_foot() {
        let plan = shape(spec(), FINE);
        let r = plan.ridge.radius;
        let u = middle(&plan, 3);
        let with = |swell: f64| RidgeNoise {
            bend: [0.0, 0.0, swell],
            ..calm()
        };
        let at_swell =
            |swell: f64, across: f64| f64::from(plan.level(START, at(u, across), with(swell)));
        let lift = f64::from(CREST - START);
        assert_eq!(at_swell(1.0, 0.0), f64::from(CREST));
        assert_eq!(at_swell(-1.0, 0.0), f64::from(CREST));
        assert_eq!(at_swell(1.0, r), f64::from(START));
        assert_eq!(at_swell(-1.0, r), f64::from(START));
        let (high, low) = (at_swell(1.0, 0.5 * r), at_swell(-1.0, 0.5 * r));
        assert!(high - low > 0.2 * lift, "{high} {low}");
        // The sag lowers the whole rise.
        let sagged = RidgeNoise {
            slow: [1.0, -1.0, -1.0],
            ..calm()
        };
        let crest = f64::from(plan.level(START, at(u, 0.0), sagged));
        assert!(
            (crest - f64::from(CREST) + SAG * lift).abs() < 2.0,
            "{crest}"
        );
    }

    #[test]
    fn a_ridge_is_up_to_3000_km_wide_on_an_earth_size_world() {
        let km = 6371.0;
        let wide = Ridge {
            radius: 1500.0 / km,
            ..spec()
        };
        let line = RidgeLine::new(wide, 1024);
        assert!((line.spec().radius * km - 1500.0).abs() < 1e-9);
        let wider = Ridge {
            radius: 1.0,
            ..spec()
        };
        assert_eq!(RidgeLine::new(wider, 1024).spec().radius, MAX_RIDGE_RADIUS);
        // A stroke of the largest radius at a cube corner reaches each of
        // the three faces at the corner.
        let mut line = RidgeLine::new(wider, 1024);
        let corner = normalize([1.0, 1.0, 1.0]);
        line.push(corner, 1.0);
        line.push(normalize([1.0, 1.0, 0.9]), 1.0);
        let plan = line.plan(0);
        let faces = plan.rects.iter().filter(|r| r.is_some()).count();
        assert!(faces >= 3, "{faces}");
    }

    #[test]
    fn a_short_segment_setting_holds_on_a_wide_brush() {
        let km = 6371.0;
        let spec = Ridge {
            radius: 1000.0 / km,
            segment: 100.0 / km,
            ..spec()
        };
        let line = RidgeLine::new(spec, 4096);
        assert!((line.spec().segment * km - 100.0).abs() < 1e-9);
        // A slow ridge has crest segments of about 50 km between the
        // transform faults.
        let wide = RidgeLine::new(
            Ridge {
                segment: 400.0 / km,
                ..spec
            },
            4096,
        );
        let sg = wide.plan(0).segmenting();
        assert_eq!(sg.group, 7);
    }

    #[test]
    fn a_block_is_round_at_its_foot_its_top_and_its_end() {
        let (scarp, w, b) = (0.2, 1.0, 1e-6);
        let slope = |y: f64| (block(y + 1e-5, scarp, w, b) - block(y - 1e-5, scarp, w, b)) / 2e-5;
        for y in [-scarp, 0.0, w] {
            assert!(slope(y).abs() < 0.02, "{y}: {}", slope(y));
        }
        assert!(slope(-0.5 * scarp) > 5.0);
        assert!(slope(0.5 * w) < -1.0);
        assert!((block(0.0, scarp, w, b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_sum_of_the_hills_has_each_hill_in_its_window() {
        let plan = rough(spec(), FINE);
        let f = plan.faulting();
        // The full rate of 25 mm per year gives a magma part of 0.767.
        assert!((f.spacing - 3.33).abs() < 0.01, "{}", f.spacing);
        let mut positive = 0;
        for group in 0..3 {
            for a in 0..200 {
                let along = f64::from(a) * 1.13 - 100.0;
                for c in 0..200 {
                    let across = f64::from(c) * 0.37;
                    let patch = [1.0, f64::from(c % 7) / 6.0];
                    let sum =
                        RidgePlan::hills(plan.ridge.seed, &f, group, 1, [along, across], patch);
                    assert!(sum >= 0.0);
                    positive += usize::from(sum > 0.0);
                    // A search that starts from more cells gives the same
                    // sum.
                    let more = RidgePlan::hill_sum(
                        plan.ridge.seed,
                        &f,
                        [group, 2],
                        1,
                        [along, across],
                        patch,
                    );
                    assert_eq!(sum, more, "{group} {along} {across}");
                }
            }
        }
        assert!(positive > 60000, "{positive}");
    }

    /// The plan of a fast ridge on a small world, so a texel is small
    /// beside a fault.
    fn fast() -> RidgePlan {
        rough(
            Ridge {
                spread: 55.0 / 1000.0,
                world_km: 1000.0,
                ..spec()
            },
            FINE,
        )
    }

    /// The patch of the floor at `[along, across]` kilometers on the world
    /// of `fast`.
    fn patch_at(plan: &RidgePlan, [along, across]: [f64; 2]) -> [f64; 2] {
        let p = lonlat_to_dir(
            (along / 1000.0).to_degrees(),
            (across / 1000.0).to_degrees(),
        );
        patch_of(plan.noise(p).patch)
    }

    /// The mean and the RMS of `height` over a large part of the world of
    /// `fast`.
    fn moments(mut height: impl FnMut([f64; 2]) -> f64) -> (f64, f64) {
        let (mut sum, mut squares, mut n) = (0.0, 0.0, 0.0);
        for a in 0..300 {
            for c in 0..300 {
                let h = height([f64::from(a) * 6.71, f64::from(c) * 3.07]);
                (sum, squares, n) = (sum + h, squares + h * h, n + 1.0);
            }
        }
        let mean = sum / n;
        (mean, (squares / n - mean * mean).sqrt())
    }

    #[test]
    fn the_texture_has_its_rms_height() {
        let plan = fast();
        let f = plan.faulting();
        let (mean, rms) = moments(|at| {
            RidgePlan::texture(plan.ridge.seed, &f, 0, 0, at, patch_at(&plan, at)[0])
                / f.texture_height
        });
        assert!(mean.abs() < 0.02, "a mean of {mean}");
        assert!((rms - GABOR_RMS_UNIT).abs() < 0.01, "an RMS of {rms}");
    }

    #[test]
    fn the_hills_have_their_rms_and_mean_height() {
        let plan = fast();
        let f = plan.faulting();
        // Where half of the faults show at full throw, the mean is half of
        // the mean of all faults.
        let (mean, _) = moments(|at| {
            RidgePlan::hills(plan.ridge.seed, &f, 0, 0, at, [1.0, 0.5]) / f.hill_height
        });
        let (_, rms) = moments(|at| {
            RidgePlan::hills(plan.ridge.seed, &f, 0, 0, at, patch_at(&plan, at)) / f.hill_height
        });
        assert!(
            (2.0 * mean - HILL_MEAN_UNIT).abs() < 0.01,
            "a mean of {}, an RMS of {rms}",
            2.0 * mean
        );
        assert!((rms - HILL_RMS_UNIT).abs() < 0.01, "an RMS of {rms}");
    }

    #[test]
    fn the_hills_setting_scales_the_hills() {
        let height = |hills: f64| {
            shape(Ridge { hills, ..spec() }, FINE)
                .faulting()
                .hill_height
        };
        assert_eq!(height(0.0), 0.0);
        assert!((height(2.0) - 2.0 * height(1.0)).abs() < 1e-12);
        assert_eq!(height(3.0), height(2.0));
    }

    #[test]
    fn the_scale_grows_the_hills_and_the_fit_sets_a_least_scale() {
        // The fit factor is the scale, and with the fit, at least the texel
        // over `FIT_TEXEL_KM`.
        assert_eq!(fit_of(0.1, false, 3.0), 3.0);
        assert_eq!(fit_of(0.1, true, 3.0), 3.0);
        assert_eq!(fit_of(1.0, true, 1.0), 4.0);
        assert_eq!(fit_of(1.0, true, 8.0), 8.0);
        assert_eq!(fit_of(1.0, false, 20.0), 10.0);
        assert_eq!(fit_of(0.1, true, 0.5), 1.0);
        // Twice the scale gives the same faults at twice the distances,
        // with the throws of `fit_height`. The blur is the same, so the
        // texel is also twice as large.
        let small = Faulting::new(110.0, 0.0, 1.0, [0.2, 1.0], LEVELS_PER_KM);
        let large = Faulting::new(110.0, 0.0, 1.0, [0.4, 2.0], LEVELS_PER_KM);
        assert!((large.lift - fit_height(2.0)).abs() < 1e-12 && large.lift > 1.5);
        let mut shown = 0;
        for a in 0..200 {
            for c in 0..50 {
                let at = [f64::from(a) * 0.75, f64::from(c) * 0.5];
                let one = RidgePlan::hills(9, &small, 0, 0, at, [1.0, 0.7]);
                let two = RidgePlan::hills(9, &large, 0, 0, at.map(|x| 2.0 * x), [1.0, 0.7]);
                assert!((two - one * large.lift).abs() < 1e-9, "{at:?}: {one} {two}");
                shown += usize::from(one > 0.0);
            }
        }
        assert!(shown > 1000, "{shown}");
        // The throws grow with the factor, then more slowly, up to a cap.
        assert_eq!(fit_height(1.0), 1.0);
        assert!(fit_height(4.0) < 4.0 && fit_height(4.0) > fit_height(2.0));
        assert_eq!(fit_height(10.0), FIT_HEIGHT_MAX);
    }

    #[test]
    fn abyssal_hills_are_lower_on_fast_ridges() {
        // Malinverno (1991); Goff (1991).
        for (rate, rms) in [(20.0, 0.224), (110.0, 0.095)] {
            assert!((hill_rms(rate) - rms).abs() < 0.005, "at {rate} mm/yr");
        }
    }

    #[test]
    fn each_segment_of_the_crest_has_its_own_shift() {
        let stairs = Ridge {
            offset: 0.04,
            segment: 0.0432,
            ..spec()
        };
        let plan = shape(stairs, FINE);
        let sg = plan.segmenting();
        let (r, length) = (plan.ridge.radius, sg.length);
        // The Segment setting of 275 km gives 5 crest segments of 55 km
        // between two transform faults as a mean.
        assert_eq!(sg.group, 5);
        assert!((length * plan.ridge.world_km - 55.0).abs() < 0.1);
        // The mean distance between two boundaries is the segment length,
        // and the distances are not equal.
        let gaps: Vec<f64> = (0..400)
            .map(|i| boundary(&plan, i + 1) - boundary(&plan, i))
            .collect();
        let mean = gaps.iter().sum::<f64>() / 400.0;
        assert!((mean / length - 1.0).abs() < 0.01, "{mean}");
        let low = gaps.iter().copied().fold(f64::MAX, f64::min);
        let high = gaps.iter().copied().fold(f64::MIN, f64::max);
        assert!(low > 0.39 * length && low < 0.6 * length, "{low}");
        assert!(high < 1.61 * length && high > 1.4 * length, "{high}");
        // The transform faults are between 2 and 8 segments apart, and the
        // Segment setting is the mean distance between them.
        let transforms: Vec<i32> = (0..400).filter(|&j| plan.transform(&sg, j)).collect();
        let apart: Vec<i32> = transforms.windows(2).map(|t| t[1] - t[0]).collect();
        assert!(apart.iter().all(|a| (2..=8).contains(a)), "{apart:?}");
        let mean = f64::from(transforms[transforms.len() - 1] - transforms[0])
            / (transforms.len() - 1) as f64;
        assert!((mean - 5.0).abs() < 0.3, "{mean}");
        // The place of the crest is the same along a segment, and it is no
        // farther from the line than the offset.
        let step = r / 500.0;
        // The top of the rise is round, so some places have the highest
        // level. The crest is in the middle of them.
        let crest_at = |u: f64| {
            let places: Vec<f64> = (-500..=500).map(|i| f64::from(i) * step).collect();
            let levels: Vec<f64> = places.iter().map(|a| level(&plan, u, *a)).collect();
            let highest = levels.iter().copied().fold(f64::MIN, f64::max);
            let top = places.iter().zip(&levels).filter(|(_, l)| **l == highest);
            let top: Vec<f64> = top.map(|(a, _)| *a).collect();
            top.iter().sum::<f64>() / top.len() as f64
        };
        let shifts: Vec<f64> = (0..100).map(|k| plan.shift(&sg, k)).collect();
        for (k, &shift) in shifts.iter().enumerate() {
            let k = k as i32;
            let (from, to) = (trace_at(&plan, k, shift), trace_at(&plan, k + 1, shift));
            for part in [0.3, 0.5, 0.7] {
                let crest = crest_at(mix(from, to, part));
                assert!((crest - shift).abs() <= step, "{k}: {crest} {shift}");
            }
            assert!(shift.abs() <= stairs.offset, "{shift}");
        }
        let highest = shifts.iter().copied().fold(f64::MIN, f64::max);
        let lowest = shifts.iter().copied().fold(f64::MAX, f64::min);
        assert!(highest > 0.6 * stairs.offset && lowest < -0.6 * stairs.offset);
        // The step of the crest at a transform fault is larger as a mean.
        let (mut strong, mut weak) = (Vec::new(), Vec::new());
        for i in 1..100 {
            let step = (shifts[i] - shifts[i - 1]).abs();
            assert!(step > 0.0);
            if plan.transform(&sg, i as i32) {
                strong.push(step);
            } else {
                weak.push(step);
            }
        }
        let mean = |steps: &[f64]| steps.iter().sum::<f64>() / steps.len() as f64;
        assert!(mean(&strong) > 2.0 * mean(&weak));
        // At the crest, the step is as narrow as the blend.
        let mut large = 0;
        for i in 1..100 {
            let step = shifts[i] - shifts[i - 1];
            if step.abs() < 0.3 * stairs.offset {
                continue;
            }
            large += 1;
            let i = i as i32;
            let (before, after) = (
                trace_at(&plan, i, shifts[i as usize - 1]),
                trace_at(&plan, i, shifts[i as usize]),
            );
            let near = 2.0 * sg.blend;
            let crest = crest_at(before.max(after) + near) - crest_at(before.min(after) - near);
            assert!(
                (crest - step).abs() <= 2.0 * r / 500.0,
                "{i}: {crest} {step}"
            );
        }
        assert!(large > 10);
        // An offset above half of the radius has the effect of that half.
        let wide = RidgeLine::new(
            Ridge {
                offset: 1.0,
                ..spec()
            },
            FINE,
        );
        assert_eq!(wide.spec().offset, 0.5 * r);
    }

    #[test]
    fn a_transform_fault_leaves_a_step_of_the_depth_that_fades_with_the_age() {
        let plan = shape(
            Ridge {
                offset: 0.04,
                ..spec()
            },
            FINE,
        );
        let sg = plan.segmenting();
        let (r, v) = (plan.ridge.radius, plan.ridge.spread);
        let round = CREST_ROUND * plan.step() / v;
        let age = |d: f64| (d / v).hypot(round) - round;
        let foot = depth(age(r));
        // The height of the rise at `d` from the crest.
        let rise = |d: f64| {
            let w = ((foot - depth(age(d))) / (foot - depth(0.0))).clamp(0.0, 1.0);
            LIFT * w * w * (1.0 + FOOT) / (w + FOOT)
        };
        let mut count = 0;
        for j in 1..200 {
            let (before, after) = (plan.shift(&sg, j - 1), plan.shift(&sg, j));
            if !plan.transform(&sg, j) || (after - before).abs() < 0.02 {
                continue;
            }
            count += 1;
            let mut steps = Vec::new();
            for side in [-1.0, 1.0] {
                for d in [0.1, 0.3, 0.6] {
                    // A place past the two crests on one side.
                    let across = if side > 0.0 {
                        before.max(after) + d * r
                    } else {
                        before.min(after) - d * r
                    };
                    let at_trace = trace_at(&plan, j, across);
                    let older = level(&plan, at_trace - 1.5 * sg.blend, across);
                    let younger = level(&plan, at_trace + 1.5 * sg.blend, across);
                    let start = f64::from(START);
                    assert!((older - start - rise((across - before).abs())).abs() <= 1.0);
                    assert!((younger - start - rise((across - after).abs())).abs() <= 1.0);
                    let step = older - younger;
                    let want = rise((across - before).abs()) - rise((across - after).abs());
                    assert!((step - want).abs() <= 2.0, "{j}: {step} {want}");
                    steps.push(step.abs());
                }
                // The step fades with the age.
                let n = steps.len();
                assert!(steps[n - 3] > steps[n - 2] && steps[n - 2] > steps[n - 1]);
                assert!(steps[n - 3] > 1000.0, "{steps:?}");
            }
        }
        assert!(count > 5, "{count}");
    }

    #[test]
    fn a_larger_offset_leaves_a_deeper_wider_fracture_zone() {
        let plan = shape(spec(), FINE);
        // Past the transform fault and before the patches, so the depth
        // depends only on the offset and the strength.
        let mut depths = Vec::new();
        for j in 0..60 {
            let small = plan.fracture(j, 20.0, true, 0.0);
            let large = plan.fracture(j, 100.0, true, 0.0);
            assert!(large.depth > 2.0 * small.depth, "{j}");
            assert!(large.half > 2.0 * small.half, "{j}");
            assert!(large.depth <= TROUGH_MAX);
            // An offset with no transform fault leaves a fainter trough.
            assert!(plan.fracture(j, 20.0, false, 0.0).depth < small.depth);
            depths.push(plan.fracture(j, 60.0, true, 0.0).depth);
        }
        // Each transform fault has its own strength, so some troughs are
        // deep and others are faint.
        let most = depths.iter().copied().fold(0.0, f64::max);
        let least = depths.iter().copied().fold(f64::MAX, f64::min);
        assert!(most > 2.0 * least, "{most} {least}");
        // Some of the transform faults have a transverse ridge.
        let ridges = (0..60)
            .filter(|j| plan.fracture(*j, 100.0, true, 0.0).ridge > 0.0)
            .count();
        assert!(ridges > 10 && ridges < 50, "{ridges}");
    }

    #[test]
    fn a_fracture_zone_is_deeper_near_the_transform_fault_and_patchy_when_old() {
        let plan = shape(spec(), FINE);
        let v = plan.ridge.spread * plan.ridge.world_km;
        // The trace leaves a transform fault of 100 km after this age.
        let out = 50.0 / v;
        let mut changes = 0;
        for j in 0..20 {
            let at = |age: f64| plan.fracture(j, 100.0, true, age).depth;
            assert!(at(0.5 * out) > at(out + ACTIVE_MYR), "{j}");
            // Along an old fracture zone, the depth goes up and down.
            let old: Vec<f64> = (0..40).map(|i| at(30.0 + f64::from(i))).collect();
            let most = old.iter().copied().fold(0.0, f64::max);
            let least = old.iter().copied().fold(f64::MAX, f64::min);
            changes += usize::from(most > 1.3 * least);
        }
        assert!(changes > 10, "{changes}");
    }

    #[test]
    fn the_traces_bend_with_the_spreading_direction() {
        let plan = shape(
            Ridge {
                offset: 0.04,
                ..spec()
            },
            FINE,
        );
        let sg = plan.segmenting();
        let (r, km) = (plan.ridge.radius, plan.ridge.world_km);
        let wander = WANDER_KM / km;
        let warp = 1.45 * TRACE_WARP_KM / km;
        let places: Vec<f64> = (-150..=150).map(|i| r * f64::from(i) / 100.0).collect();
        // The move of a trace from its boundary at the crest.
        let moves = |j: i32| -> Vec<f64> {
            let start = boundary(&plan, j);
            places
                .iter()
                .map(|&a| trace_at(&plan, j, a) - start)
                .collect()
        };
        let transforms: Vec<i32> = (0..60).filter(|&j| plan.transform(&sg, j)).collect();
        let first = moves(transforms[0]);
        // The trace of a transform fault is not straight.
        let (a, b) = (first[0], first[first.len() - 1]);
        let bend = first
            .iter()
            .enumerate()
            .map(|(i, m)| (m - mix(a, b, i as f64 / 300.0)).abs())
            .fold(0.0, f64::max);
        assert!(bend * km > 10.0, "{}", bend * km);
        // The traces of all transform faults bend together.
        for &j in &transforms[1..] {
            for (m, n) in moves(j).iter().zip(&first) {
                assert!((m - n).abs() <= 2.0 * (wander + warp) + 1e-12, "{j}");
            }
        }
        // The angle of the spreading direction is no more than 8 degrees.
        for &a in &places {
            let (_, slope) = plan.turn(&sg, a / plan.ridge.spread);
            assert!(slope.abs() <= TAN_TURN);
        }
        // An offset with no transform fault migrates, so its trace moves
        // from the transform faults by up to a part of the segment length,
        // and some of it is oblique.
        let mut oblique = false;
        for j in 0..60 {
            if plan.transform(&sg, j) {
                continue;
            }
            for (i, (m, n)) in moves(j).iter().zip(&first).enumerate() {
                let most = (MIGRATE + KINK) * sg.length + 2.0 * (wander + warp);
                assert!((m - n).abs() <= most + 1e-12);
                let turn = plan.turn(&sg, places[i] / plan.ridge.spread);
                let trace = plan.trace(&sg, j, places[i], turn);
                oblique |= trace.slope.abs() > 0.4;
            }
        }
        assert!(oblique);
    }

    #[test]
    fn the_turn_is_the_sum_of_its_slope() {
        let plan = shape(spec(), FINE);
        let sg = plan.segmenting();
        let v = plan.ridge.spread;
        assert_eq!(plan.turn(&sg, 0.0).0, 0.0);
        let h = 1e-4;
        for i in -400..400 {
            let tau = f64::from(i) * 0.37;
            let (_, slope) = plan.turn(&sg, tau);
            let change = (plan.turn(&sg, tau + h).0 - plan.turn(&sg, tau - h).0) / (2.0 * h);
            assert!((change - v * slope).abs() < 1e-6 * v, "{tau}");
        }
    }

    #[test]
    fn the_level_does_not_depend_on_the_boundaries_that_the_search_starts_from() {
        let full = Ridge {
            offset: 0.05,
            ..spec()
        };
        let plan = rough(full, FINE);
        let r = plan.ridge.radius;
        let mut changed = 0;
        for j in 0..400 {
            let u = 0.3 + f64::from(j) * 0.00731;
            for i in 0..=120 {
                let across = r * (f64::from(i) / 40.0 - 1.5);
                let p = lonlat_to_dir(u.to_degrees(), across.to_degrees());
                let noise = plan.noise(p);
                let new = plan.level_in(START, at(u, across), noise, 0);
                assert_eq!(
                    new,
                    plan.level_in(START, at(u, across), noise, 3),
                    "{u} {across}"
                );
                changed += usize::from(new != START);
                // Nothing changes at the reach.
                if i == 0 || i == 120 {
                    assert_eq!(new, START);
                }
            }
        }
        assert!(changed > 30000, "{changed}");
    }

    #[test]
    fn the_ridge_gets_lower_and_narrower_to_an_end_of_the_line() {
        for plan in [shape(spec(), FINE), rough(spec(), FINE)] {
            let r = plan.ridge.radius;
            let ground = f64::from(START);
            for k in 1..12 {
                let u = middle(&plan, k);
                let near = |end: f64, across: f64, noise: RidgeNoise| {
                    let point = LinePoint {
                        end,
                        ..at(u, across)
                    };
                    f64::from(plan.level(START, point, noise))
                };
                // At the end, nothing changes, also in a crack.
                for across in [0.0, 0.02 * r, -0.3 * r, 1.2 * r] {
                    assert_eq!(near(0.0, across, calm()), ground);
                    assert_eq!(near(0.0, across, wild()), ground);
                    let crack = LinePoint {
                        end: 0.0,
                        ..at(boundary(&plan, k), across)
                    };
                    assert_eq!(f64::from(plan.level(START, crack, calm())), ground);
                }
                // The ridge has its full shape at 1.2 brush radii from the
                // end.
                for across in [0.0, 0.3 * r, -0.8 * r] {
                    let whole = near(f64::MAX, across, calm());
                    assert_eq!(near(TAPER * r, across, calm()), whole);
                }
                if plan.relief > 0.0 {
                    continue;
                }
                // At half of that distance, the crest has half of its
                // height, and the rise is 0.6 times as wide.
                let half = near(0.5 * TAPER * r, 0.0, calm());
                assert!((half - (ground + 0.5 * LIFT)).abs() <= 1.0, "{k}: {half}");
                assert!(near(0.5 * TAPER * r, 0.57 * r, calm()) > ground);
                assert_eq!(near(0.5 * TAPER * r, 0.6 * r, calm()), ground);
            }
        }
    }

    #[test]
    fn the_level_has_no_step_along_the_line_and_across_it() {
        let full = Ridge {
            offset: 0.04,
            ..spec()
        };
        let plan = rough(full, FINE);
        let (r, fine) = (plan.ridge.radius, plan.step() / 64.0);
        // Along the line, over the traces of the boundaries and over the
        // middle of segments. At a trace, the step of the depth spreads over
        // the blend.
        for across in [-1.3, -0.6, -0.04, 0.0, 0.02, 0.5, 1.1] {
            let mut last = real(&plan, 0.3, across * r);
            for i in 1..40000 {
                let new = real(&plan, 0.3 + f64::from(i) * fine, across * r);
                assert!((new - last).abs() < 500.0, "{across} {i}: {last} {new}");
                last = new;
            }
        }
        // Across the line, to the reach.
        for j in 0..40 {
            let u = 0.3 + f64::from(j) * 0.0137;
            let count = (1.5 * r / fine) as i32;
            let mut last = real(&plan, u, -1.5 * r);
            assert_eq!(last, f64::from(START));
            for i in -count..=count {
                let new = real(&plan, u, f64::from(i) * fine);
                assert!((new - last).abs() < 400.0, "{u} {i}: {last} {new}");
                last = new;
            }
            assert!((last - f64::from(START)).abs() <= 1.0, "{u}: {last}");
        }
    }

    #[test]
    fn a_segment_goes_to_the_shader_as_16_numbers() {
        let mut line = RidgeLine::new(spec(), 128);
        for lon in [0.0, 3.0, 6.0] {
            line.push(lonlat_to_dir(lon, 0.0), 0.5);
        }
        let plan = line.plan(0);
        assert_eq!(
            (plan.segments.len(), plan.tail.clone(), plan.end),
            (2, 0..2, 3)
        );
        let (first, second) = (plan.segments[0], plan.segments[1]);
        assert!(first.first && !first.last && !second.first && second.last);
        let len = 3f64.to_radians() as f32;
        // The second segment starts at 3 degrees, and cell 1 is at 0.1.
        let numbers = second.to_f32(0.1);
        assert!((numbers[2] - 1.0).abs() < 1e-6 && (numbers[3] - (len - 0.1)).abs() < 1e-6);
        assert!((numbers[7] - len).abs() < 1e-6);
        assert_eq!(numbers[11..], [0.5, 0.5, 2.0, 1.0, 0.0]);
        assert_eq!(first.to_f32(0.1)[13..], [1.0, 0.0, 0.0]);
        // The plane at the shared point is the same for the two segments.
        assert!(length(add(first.m1, scale(second.m0, -1.0))) < 1e-12);
    }

    fn on(plan: &RidgePlan, p: V3) -> LinePoint {
        match plan.place(p) {
            LinePlace::On(point) => point,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_place_has_the_distances_along_and_across_the_line() {
        let mut line = RidgeLine::new(spec(), 128);
        for i in 0..=10 {
            line.push(lonlat_to_dir(3.0 * f64::from(i), 0.0), f64::from(i) / 10.0);
        }
        // A point with no distance from the last point is not added.
        line.push(lonlat_to_dir(30.0, 0.0), 1.0);
        assert_eq!(line.len(), 11);
        let plan = line.plan(0);
        let point = on(&plan, lonlat_to_dir(10.0, 3.0));
        assert!((point.u - 10f64.to_radians()).abs() < 1e-4, "{point:?}");
        assert!((point.across - 3f64.to_radians()).abs() < 1e-12);
        assert!((point.flow - 1.0 / 3.0).abs() < 1e-3);
        assert_eq!(point.past, 0.0);
        assert_eq!(point.end, point.u);
        let point = on(&plan, lonlat_to_dir(25.0, 3.0));
        assert!((point.end - 5f64.to_radians()).abs() < 1e-4, "{point:?}");
        // The left of the line is positive.
        assert!(on(&plan, lonlat_to_dir(20.0, -2.0)).across < 0.0);
        // Past an end, the place is the end.
        for (lon, lat, u) in [(-2.0, 1.0, 0.0), (33.0, -1.0, 30f64.to_radians())] {
            let p = lonlat_to_dir(lon, lat);
            let point = on(&plan, p);
            let end = lonlat_to_dir(if u == 0.0 { 0.0 } else { 30.0 }, 0.0);
            assert!((point.u - u).abs() < 1e-12);
            assert!((point.across - lat.to_radians()).abs() < 1e-12);
            assert!((point.across.hypot(point.past) - angle(p, end)).abs() < 1e-9);
        }
        assert_eq!(plan.place(lonlat_to_dir(10.0, 9.0)), LinePlace::Far);
        assert_eq!(plan.place(lonlat_to_dir(-9.0, 0.0)), LinePlace::Far);
        // The plan of the last point changes no texel at the start.
        let last = line.plan(10);
        // The new part of the plan starts 1.2 brush radii before the old
        // end of the line.
        assert!(last.segments.len() >= 5 && last.tail.len() == 4);
        assert_eq!(last.place(lonlat_to_dir(8.0, 3.0)), LinePlace::Far);
        assert_eq!(
            on(&last, lonlat_to_dir(29.0, 3.0)),
            on(&plan, lonlat_to_dir(29.0, 3.0))
        );
    }

    #[test]
    fn a_place_has_no_step_at_a_turn_and_at_an_end() {
        // The line turns by 60 degrees at the middle point.
        let corner = lonlat_to_dir(0.0, 0.0);
        let mut line = RidgeLine::new(spec(), 128);
        let dirs = path(&[(-9.0, 0.0), (0.0, 0.0), (4.5, 7.8)], 0.05);
        for &dir in &dirs {
            line.push(dir, 1.0);
        }
        let plan = line.plan(0);
        for center in [corner, dirs[0], dirs[dirs.len() - 1]] {
            let e1 = normalize(cross(center, [0.0, 0.0, 1.0]));
            let e2 = cross(center, e1);
            let around = |i: usize| {
                let (sin, cos) = (i as f64 / 720.0 * std::f64::consts::TAU).sin_cos();
                let side = add(scale(e1, cos), scale(e2, sin));
                on(&plan, normalize(add(center, scale(side, 0.05))))
            };
            for i in 0..720 {
                let (a, b) = (around(i), around(i + 1));
                let distance = |p: LinePoint| p.across.hypot(p.past);
                assert!((a.u - b.u).abs() < 3e-3, "{i}: {a:?} {b:?}");
                assert!((a.across - b.across).abs() < 1e-3, "{i}: {a:?} {b:?}");
                assert!((distance(a) - distance(b)).abs() < 1e-3, "{i}: {a:?} {b:?}");
            }
        }
    }

    /// Points along the great circles between places, each in degrees of
    /// longitude and latitude, with `spacing` radians between two points.
    fn path(places: &[(f64, f64)], spacing: f64) -> Vec<V3> {
        let mut dirs = vec![lonlat_to_dir(places[0].0, places[0].1)];
        for w in places.windows(2) {
            let (a, b) = (lonlat_to_dir(w[0].0, w[0].1), lonlat_to_dir(w[1].0, w[1].1));
            let steps = (angle(a, b) / spacing).ceil() as usize;
            let tangent = cross(normalize(cross(a, b)), a);
            dirs.extend((1..=steps).map(|i| {
                let (sin, cos) = (angle(a, b) * i as f64 / steps as f64).sin_cos();
                add(scale(a, cos), scale(tangent, sin))
            }));
        }
        dirs
    }

    /// Ground with slopes below the crest level.
    fn ground(n: usize) -> Heightmap {
        let mut map = Heightmap::new(n, 0);
        let face = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        for f in 0..FACES {
            let levels: Vec<u16> = (0..n * n)
                .map(|i| {
                    let d = map.texel_dir(f, i % n, i / n);
                    let level = 3000.0 * (9.0 * d[0] + 4.0 * d[1]).sin() * (7.0 * d[2]).cos();
                    (f64::from(START) + level) as u16
                })
                .collect();
            map.store_rect(f, face, &levels);
        }
        map
    }

    fn levels(map: &Heightmap) -> Vec<u16> {
        let n = map.face_size();
        let mut out = Vec::new();
        for face in 0..FACES {
            let rect = TexelRect {
                x0: 0,
                y0: 0,
                x1: n,
                y1: n,
            };
            map.read_rect(face, rect, &mut out);
        }
        out
    }

    /// Starts a stroke and lays a ridge point by point, as the app does.
    /// The flow changes along the line.
    fn lay(map: &mut Heightmap, spec: Ridge, dirs: &[V3]) -> RidgeLine {
        let mut line = RidgeLine::new(spec, map.face_size());
        map.begin_stroke();
        for (i, &dir) in dirs.iter().enumerate() {
            let from = line.len();
            line.push(dir, 0.6 + 0.4 * (i as f64 * 0.3).sin());
            map.ridge(&line.plan(from));
        }
        line
    }

    /// The levels after a stroke, from the place of each texel on the full
    /// line.
    fn expected(start: &Heightmap, line: &RidgeLine) -> Vec<u16> {
        let n = start.face_size();
        let plan = line.plan_of(0, line.len() - 1);
        let mut out = levels(start);
        for (i, level) in out.iter_mut().enumerate() {
            let p = start.texel_dir(i / (n * n), i % n, i / n % n);
            if let LinePlace::On(point) = plan.place(p) {
                *level = plan.level(*level, point, plan.noise(p));
            }
        }
        out
    }

    fn full() -> Ridge {
        Ridge {
            offset: 0.04,
            segment: 0.08,
            ..spec()
        }
    }

    /// Strokes that cross their own line, turn back, and go over a face
    /// edge, a cube corner, and a pole.
    fn strokes() -> Vec<(&'static str, Vec<V3>)> {
        let spacing = LINE_SPACING * spec().radius;
        let stroke = |places: &[(f64, f64)]| path(places, spacing);
        vec![
            (
                "cross",
                stroke(&[(-10.0, -5.0), (10.0, 5.0), (10.0, -5.0), (-10.0, 5.0)]),
            ),
            ("turn", stroke(&[(-10.0, 0.0), (10.0, 0.0), (-10.0, 3.0)])),
            ("back", stroke(&[(-10.0, 20.0), (5.0, 20.0), (-10.0, 20.0)])),
            (
                "curve",
                stroke(&[(100.0, -9.0), (108.0, -4.0), (112.0, 4.0), (110.0, 14.0)]),
            ),
            ("face edge", stroke(&[(36.0, -6.0), (54.0, 10.0)])),
            ("cube corner", stroke(&[(36.0, 28.0), (54.0, 42.0)])),
            ("pole", stroke(&[(20.0, 80.0), (200.0, 80.0)])),
        ]
    }

    #[test]
    fn a_stroke_in_steps_gives_the_levels_of_the_full_line() {
        let n = 128;
        let start = ground(n);
        for (name, dirs) in strokes() {
            let mut map = ground(n);
            let line = lay(&mut map, full(), &dirs);
            assert!(line.len() > 10 && line.len() <= LINE_SEGMENTS, "{name}");
            let after = levels(&map);
            let want = expected(&start, &line);
            let before = levels(&start);
            let wrong: Vec<_> = (0..after.len())
                .filter(|&i| after[i] != want[i])
                .map(|i| (i / (n * n), i % n, i / n % n, before[i], after[i], want[i]))
                .collect();
            assert!(
                wrong.is_empty(),
                "{name}: {} {:?}",
                wrong.len(),
                &wrong[..wrong.len().min(12)]
            );
            assert!(after != levels(&start), "{name}");
            // No texel is above the crest by more than the rift mountains.
            let plan = line.plan(0);
            let f = plan.faulting();
            let rim = ABOVE_CREST * plan.axial(&f).0 * f.level;
            let highest = f64::from(CREST) + rim;
            assert!(after.iter().all(|l| f64::from(*l) <= highest), "{name}");
            // A second pass over the line changes nothing.
            assert_eq!(map.ridge(&line.plan(0)), 0, "{name}");
            for from in [1, line.len() / 2, line.len()] {
                assert_eq!(map.ridge(&line.plan(from)), 0, "{name}");
            }
        }
    }

    #[test]
    fn a_long_line_goes_in_more_than_one_plan() {
        let n = 128;
        let dirs = path(
            &[(-60.0, -20.0), (0.0, 20.0), (60.0, -20.0), (120.0, 20.0)],
            0.01,
        );
        let mut in_steps = ground(n);
        let line = lay(&mut in_steps, full(), &dirs);
        assert!(line.len() > 2 * LINE_SEGMENTS);
        let mut in_plans = ground(n);
        in_plans.begin_stroke();
        let (mut from, mut plans) = (0, 0);
        while from < line.len() {
            let plan = line.plan(from);
            assert!(plan.segments.len() <= LINE_SEGMENTS && plan.end > from);
            in_plans.ridge(&plan);
            from = plan.end;
            plans += 1;
        }
        assert!(plans >= 3);
        assert!(levels(&in_steps) == levels(&in_plans));
    }

    #[test]
    fn the_ground_far_from_the_line_does_not_change() {
        let n = 128;
        let wide = Ridge {
            offset: 1.0,
            ..full()
        };
        for (name, dirs) in strokes() {
            let (start, mut map) = (ground(n), ground(n));
            let line = lay(&mut map, wide, &dirs);
            let reach = REACH * line.spec().radius;
            let (before, after) = (levels(&start), levels(&map));
            let mut farthest: f64 = 0.0;
            for i in (0..after.len()).filter(|&i| after[i] != before[i]) {
                let p = map.texel_dir(i / (n * n), i % n, i / n % n);
                let distances = (0..line.len() - 1).map(|j| line.line.distance(j, p));
                farthest = farthest.max(distances.fold(f64::MAX, f64::min));
            }
            assert!(farthest < reach, "{name}: {farthest}");
            assert!(farthest > 0.6 * reach, "{name}: {farthest}");
        }
    }

    #[test]
    fn the_seed_sets_the_pattern() {
        let dirs = path(&[(-10.0, -5.0), (10.0, 5.0)], 0.025);
        let laid = |seed: u32| {
            let mut map = ground(128);
            lay(&mut map, Ridge { seed, ..full() }, &dirs);
            levels(&map)
        };
        assert!(laid(7) == laid(7));
        assert!(laid(7) != laid(8));
    }

    #[test]
    fn the_floor_and_the_ceiling_apply_to_a_ridge() {
        let dirs = path(&[(-10.0, -5.0), (10.0, 5.0)], 0.025);
        let laid_with = |spec: Ridge| {
            let mut map = Heightmap::new(128, START);
            lay(&mut map, spec, &dirs);
            levels(&map)
        };
        let laid = |limits: Limits| laid_with(Ridge { limits, ..full() });
        let free = laid(Limits::default());
        assert!(free.iter().any(|l| *l > 22000));
        // Only the troughs, the deeps, and the axial valley take the ground
        // down, by no more than their depth.
        let plan = rough(full(), 128);
        let (valley, _) = plan.axial(&plan.faulting());
        let deepest = (TROUGH_MAX + 2.0 * NODAL_MAX + valley) * LEVELS_PER_KM;
        assert!(
            free.iter()
                .all(|l| f64::from(*l) >= f64::from(START) - deepest)
        );
        // The troughs and the deeps of the offsets take some ground lower
        // than a ridge with no offset does.
        let straight = laid_with(Ridge {
            offset: 0.0,
            ..full()
        });
        assert!(free.iter().zip(&straight).any(|(a, b)| a + 200 < *b));
        // A floor above the ground keeps all the ground.
        let above = laid(Limits {
            floor: START + 5000,
            ceiling: 65535,
        });
        assert!(above.iter().all(|l| *l == START));
        // A floor below the ground stops the troughs and the deeps at it.
        let low = START - 200;
        let floor = laid(Limits {
            floor: low,
            ceiling: 65535,
        });
        assert!(free.iter().any(|l| *l < low));
        assert!(floor.iter().zip(&free).all(|(a, b)| *a == low.max(*b)));
        let ceiling = laid(Limits {
            floor: 0,
            ceiling: 20000,
        });
        assert_eq!(ceiling.iter().max(), Some(&20000));
        assert!(ceiling.iter().zip(&free).all(|(a, b)| a == b || *b > 20000));
    }

    #[test]
    fn one_undo_step_puts_back_the_ground_from_before_a_ridge() {
        let dirs = path(&[(30.0, -5.0), (60.0, 5.0)], 0.025);
        let mut map = ground(128);
        let before = levels(&map);
        lay(&mut map, full(), &dirs);
        assert!(map.end_stroke());
        let after = levels(&map);
        assert!(after != before);
        assert!(map.undo());
        assert!(levels(&map) == before);
        assert!(!map.can_undo());
        assert!(map.redo());
        assert!(levels(&map) == after);
        // A cancelled stroke also puts back the ground.
        let mut map = ground(128);
        lay(&mut map, full(), &dirs);
        map.cancel_stroke();
        assert!(levels(&map) == before);
    }

    /// A line along the equator from longitude 0 to 60, with the settings
    /// of `full`.
    fn equator() -> RidgeLine {
        let mut line = RidgeLine::new(full(), FINE);
        for dir in path(&[(0.0, 0.0), (60.0, 0.0)], LINE_SPACING * spec().radius) {
            line.push(dir, 1.0);
        }
        line
    }

    /// A stroke through places in degrees of longitude and latitude.
    fn guide_stroke(places: &[(f64, f64)]) -> Vec<V3> {
        path(places, 0.05 * spec().radius)
    }

    /// The place of the trace of boundary `j` along the line at `across`,
    /// with the guides of the plan.
    fn guided_trace(plan: &RidgePlan, j: i32, across: f64) -> f64 {
        let sg = plan.segmenting();
        let turn = plan.guides.turn(plan.boundary(&sg, j), across);
        plan.trace(&sg, j, across, turn).u
    }

    /// The most that the wander and the warp move a trace of a transform
    /// fault.
    fn trace_noise(plan: &RidgePlan) -> f64 {
        (WANDER_KM + 1.45 * TRACE_WARP_KM) / plan.ridge.world_km
    }

    #[test]
    fn the_trace_of_a_drawn_fracture_zone_follows_the_stroke() {
        let mut line = equator();
        let stroke = guide_stroke(&[(30.0, -7.0), (34.0, 7.0)]);
        assert!(line.add_guide(&stroke));
        let plan = line.plan(0);
        let r = plan.ridge.radius;
        let sg = plan.segmenting();
        assert_eq!(plan.guides.list.len(), 1);
        let guide = &plan.guides.list[0];
        // The snap moves the trace by less than a boundary step.
        let snap = guide.u0 - guide.drawn;
        assert!(snap.abs() <= sg.length, "{snap}");
        let kept = line.kept(&line.bare(Arc::default()), &stroke);
        let mut count = 0;
        for (u, across) in kept {
            if across.abs() < 0.05 * r || across.abs() > 1.3 * r {
                continue;
            }
            count += 1;
            let trace = guided_trace(&plan, guide.j, across);
            let miss = (trace - (u + snap)).abs();
            assert!(miss <= trace_noise(&plan) + 2e-4, "{across}: {miss}");
        }
        assert!(count > 20, "{count}");
    }

    #[test]
    fn a_fracture_zone_drawn_on_one_side_goes_on_straight_across_the_line() {
        let mut line = equator();
        assert!(line.add_guide(&guide_stroke(&[(30.0, 0.2), (33.0, 7.0)])));
        let guides = line.plan(0).guides;
        let m = &guides.list[0].m;
        assert_eq!(m[1], m[0].map(|x| -x));
        assert!(m[0][GUIDE_SAMPLES - 1] > 0.02, "{m:?}");
    }

    #[test]
    fn a_stroke_far_from_the_line_is_not_a_fracture_zone() {
        let mut line = equator();
        assert!(!line.add_guide(&guide_stroke(&[(30.0, 15.0), (32.0, 25.0)])));
        // A stroke near the line that does not cross it, and does not end
        // near it.
        assert!(!line.add_guide(&guide_stroke(&[(30.0, 3.0), (34.0, 5.0)])));
        assert!(line.guide_strokes().is_empty());
        assert!(line.plan(0).guides.list.is_empty());
        assert!(!line.pop_guide());
    }

    #[test]
    fn a_fracture_zone_at_the_boundary_of_another_takes_its_place() {
        let (first, second) = (
            guide_stroke(&[(30.0, -7.0), (34.0, 7.0)]),
            guide_stroke(&[(34.0, -7.0), (30.0, 7.0)]),
        );
        let mut line = equator();
        assert!(line.add_guide(&first));
        assert!(line.add_guide(&second));
        assert_eq!(line.guide_strokes().len(), 2);
        let mut alone = equator();
        assert!(alone.add_guide(&second));
        let (both, alone) = (line.plan(0).guides, alone.plan(0).guides);
        assert_eq!(both.list.len(), 1);
        assert_eq!(both.list[0].j, alone.list[0].j);
        assert_eq!(both.list[0].m, alone.list[0].m);
    }

    #[test]
    fn converging_fracture_zones_keep_the_traces_in_order() {
        let mut line = equator();
        assert!(line.add_guide(&guide_stroke(&[(29.0, -7.0), (33.0, 7.0)])));
        assert!(line.add_guide(&guide_stroke(&[(35.0, -7.0), (31.0, 7.0)])));
        let plan = line.plan(0);
        let (r, sg) = (plan.ridge.radius, plan.segmenting());
        let guides = &plan.guides;
        let (a, b) = (guides.list[0].j, guides.list[1].j);
        assert!(b > a + 2, "{a} {b}");
        for i in -140..=140 {
            let across = r * f64::from(i) / 100.0;
            let place = |j: i32| {
                let start = plan.boundary(&sg, j);
                start + guides.turn(start, across).0
            };
            for j in a - 3..b + 3 {
                assert!(place(j + 1) > place(j), "{across} {j}");
            }
            assert!(guided_trace(&plan, b, across) > guided_trace(&plan, a, across));
        }
        // The level does not depend on the boundaries that the search starts
        // from.
        let mut changed = 0;
        for j in 0..60 {
            let u = 0.5 + f64::from(j) * 0.0037;
            for i in 0..=60 {
                let across = r * (f64::from(i) / 20.0 - 1.5);
                let p = lonlat_to_dir(u.to_degrees(), across.to_degrees());
                let noise = plan.noise(p);
                let new = plan.level_in(START, at(u, across), noise, 0);
                assert_eq!(new, plan.level_in(START, at(u, across), noise, 3));
                changed += usize::from(new != START);
            }
        }
        assert!(changed > 2000, "{changed}");
    }

    #[test]
    fn removing_the_last_fracture_zone_gives_back_the_levels_without_it() {
        let mut line = equator();
        let levels = |plan: &RidgePlan| -> Vec<u16> {
            let r = plan.ridge.radius;
            let mut out = Vec::new();
            for j in 0..80 {
                let u = 0.5 + f64::from(j) * 0.0029;
                for i in 0..=30 {
                    out.push(real(plan, u, r * (f64::from(i) / 10.0 - 1.5)) as u16);
                }
            }
            out
        };
        let before = levels(&line.plan(0));
        assert!(line.add_guide(&guide_stroke(&[(30.0, -7.0), (34.0, 7.0)])));
        assert_ne!(levels(&line.plan(0)), before);
        assert!(line.pop_guide());
        assert!(line.guide_strokes().is_empty());
        assert_eq!(levels(&line.plan(0)), before);
    }

    #[test]
    fn a_drawn_fracture_zone_is_a_transform_fault_with_a_trough() {
        let mut line = equator();
        assert!(line.add_guide(&guide_stroke(&[(30.0, -7.0), (34.0, 7.0)])));
        let plan = line.plan(0);
        let (r, km, sg) = (plan.ridge.radius, plan.ridge.world_km, plan.segmenting());
        let j = plan.guides.list[0].j;
        assert!(plan.transform(&sg, j));
        // The offset at the transform fault is large.
        let step = (plan.shift(&sg, j) - plan.shift(&sg, j - 1)).abs();
        assert!(step >= 0.4 * plan.ridge.offset * GROUP_SHIFT, "{step}");
        let off = 2.0 * sg.blend + 15.0 / km;
        for across in [-1.0, -0.8, -0.6, 0.6, 0.8, 1.0].map(|a| a * r) {
            let u = guided_trace(&plan, j, across);
            let low = level(&plan, u, across);
            let sides = level(&plan, u - off, across).min(level(&plan, u + off, across));
            assert!(low + 300.0 < sides, "{across}: {low} {sides}");
        }
    }
}
