//! The fault block brush.
//!
//! The brush breaks the ground along the line of a stroke into long blocks
//! between parallel faults. Each block tilts, so it has a steep face at its
//! fault and a gentle back slope. The low part of each back slope is under a
//! flat basin floor. Each fault is in pieces. A piece ends beside the start
//! of the next piece, en echelon, so the blocks do not make one long ridge.
//!
//! A single fault stroke makes one escarpment along the line. The ground on
//! the left of the stroke goes up, and the ground on the right goes down.

use std::f64::consts::FRAC_PI_2;
use std::ops::{Deref, Range};

use crate::heightmap::{Limits, MAX_BRUSH_RADIUS, hash};
use crate::line::{LinePlace, LinePlan, LinePoint, REACH, StrokeLine, TAPER, smoothstep, unit};
use crate::math::V3;
use crate::ridge::TAPER_WIDTH;

/// The length of a piece of a fault, in fault spacings.
const PIECE: f64 = 6.0;

/// The largest shift of the end of a piece along the line, as a part of
/// the piece length.
const BREAK_JITTER: f64 = 0.3;

/// Half of the length over which a piece fades in or out, as a part of the
/// piece length.
const OVERLAP: f64 = 0.15;

/// The largest shift of a block across the line, in fault spacings.
const SHIFT: f64 = 0.2;

/// The shift across the line from one end of a piece to the other end, in
/// fault spacings. The pieces of all faults turn to one side.
const ECHELON: f64 = 0.3;

/// Half of the mean width of a block, in fault spacings.
const HALF: f64 = 0.33;

/// The largest change of the width of a block, as a part of the mean.
const WIDTH_JITTER: f64 = 0.3;

/// The place of the crest of a block, from -1 at the foot of its back
/// slope to 1 at its fault. The steep face is between the crest and the
/// fault.
const CREST: f64 = 0.6;

/// The smallest width of a steep face, in texel steps.
const MIN_FACE: f64 = 3.0;

/// The part of the height of a block that is under the basin floor.
const FILL: f64 = 0.3;

/// The part of the blocks that tilt to the side of the seed.
const DOMAIN: f64 = 0.75;

/// The part of the height that the basin floor has.
const BASE: f64 = 0.1;

/// The ground is flat out to `EDGE_FLAT` of the width. It ends at the
/// width.
const EDGE_FLAT: f64 = 0.5;

/// Half of the width of the escarpment of a single fault, as a part of the
/// width.
const SCARP: f64 = 0.06;

/// The ground of a single fault is flat out to this part of the width.
const SINGLE_FLAT: f64 = 0.2;

/// The smallest fault spacing, in texel steps.
const MIN_SPACING: f64 = 6.0;

/// The largest fault spacing, as a part of the brush radius.
const MAX_SPACING: f64 = 0.5;

const SALT_FAULT: u32 = 0x2f6b_c917;

/// The channels of the hash of a piece.
const BREAK: u32 = 0;
const SHIFT_CHANNEL: u32 = 1;
const HEIGHT: u32 = 2;
const SIDE: u32 = 3;
const PHASE: u32 = 4;
const DIP: u32 = 5;
const WIDTH: u32 = 6;

/// The settings of a fault block stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaultSpec {
    /// The brush radius as an angle, in radians. It is half of the width
    /// of the region.
    pub radius: f64,
    /// The height of the blocks at full flow, in 16-bit steps. A single
    /// fault has this height from its low side to its high side.
    pub height: f64,
    /// The mean distance between two faults as an angle, in radians.
    pub spacing: f64,
    /// The seed of the pattern.
    pub seed: u32,
    /// The stroke makes one fault along its line.
    pub single: bool,
    pub limits: Limits,
}

/// The line of one fault block stroke.
#[derive(Clone, Debug)]
pub struct FaultLine {
    spec: FaultSpec,
    line: StrokeLine,
}

/// The part of the heightmap that changes after new points of a line, and
/// the segments that give the levels there.
#[derive(Clone, Debug)]
pub struct FaultPlan {
    /// The settings, inside their limits.
    pub fault: FaultSpec,
    pub line: LinePlan,
}

impl Deref for FaultPlan {
    type Target = LinePlan;

    fn deref(&self) -> &LinePlan {
        &self.line
    }
}

impl FaultLine {
    /// Starts a line with no points on a heightmap with `face_size` texels
    /// along one side of a face. The settings go inside their limits.
    pub fn new(spec: FaultSpec, face_size: usize) -> FaultLine {
        let step = FRAC_PI_2 / face_size as f64;
        let radius = spec.radius.max(4.0 * step).min(MAX_BRUSH_RADIUS / REACH);
        let spacing = spec
            .spacing
            .min(MAX_SPACING * radius)
            .max(MIN_SPACING * step);
        let spec = FaultSpec {
            radius,
            height: spec.height.clamp(0.0, 65535.0),
            spacing,
            ..spec
        };
        FaultLine {
            spec,
            line: StrokeLine::new(radius, face_size),
        }
    }

    /// The settings, inside their limits.
    pub fn spec(&self) -> FaultSpec {
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

    /// The length of the line, in radians.
    pub fn length(&self) -> f64 {
        self.line.length()
    }

    /// The plan for the points from `from` on, as `StrokeLine::plan`.
    pub fn plan(&self, from: usize) -> FaultPlan {
        self.wrap(self.line.plan(from))
    }

    /// The plan for the distances `u` along the line, as
    /// `StrokeLine::plan_between`.
    pub fn plan_between(&self, u: Range<f64>) -> FaultPlan {
        self.wrap(self.line.plan_between(u))
    }

    /// As `StrokeLine::margin`.
    #[doc(hidden)]
    pub fn margin(&self, p: V3) -> f64 {
        self.line.margin(p)
    }

    fn wrap(&self, line: LinePlan) -> FaultPlan {
        FaultPlan {
            fault: self.spec,
            line,
        }
    }
}

impl FaultPlan {
    /// The length of a piece of a fault, in radians.
    pub(crate) fn piece(&self) -> f64 {
        PIECE * self.fault.spacing
    }

    /// The seed of the hashes of the pieces.
    pub(crate) fn seed(&self) -> u32 {
        self.fault.seed ^ SALT_FAULT
    }

    /// The side to which most blocks tilt: 1 or -1.
    pub(crate) fn side(&self) -> f64 {
        if self.hash(0, 0, SIDE) & 1 == 0 {
            1.0
        } else {
            -1.0
        }
    }

    /// The shift of a block across the line for each radian along it.
    pub(crate) fn tilt(&self) -> f64 {
        self.side() * ECHELON * self.fault.spacing / self.piece()
    }

    fn hash(&self, k: i32, j: i32, channel: u32) -> u32 {
        hash(k as u32, j as u32, channel, self.seed())
    }

    /// The width of the region at a place, in radians. The region is
    /// narrower near an end of the line.
    fn width(&self, at: LinePoint) -> f64 {
        let taper = smoothstep(0.0, TAPER * self.fault.radius, at.end);
        self.fault.radius * (TAPER_WIDTH + (1.0 - TAPER_WIDTH) * taper)
    }

    /// The height of a block at a place `q` across it, from -1 at the foot
    /// of its back slope to 1 at its fault, from 0 to 1. The crest is at
    /// `crest`. The height is 0 on the basin floor.
    #[doc(hidden)]
    pub fn profile(q: f64, crest: f64) -> f64 {
        let raw = if q <= -1.0 || q >= 1.0 {
            0.0
        } else if q <= crest {
            (q + 1.0) / (crest + 1.0)
        } else {
            1.0 - smoothstep(crest, 1.0, q)
        };
        (raw - FILL).max(0.0) / (1.0 - FILL)
    }

    /// The start of piece `j` of fault `k`, in radians along the line.
    fn start(&self, k: i32, j: i32) -> f64 {
        let phase = unit(self.hash(k, 0, PHASE));
        let jitter = BREAK_JITTER * (2.0 * unit(self.hash(k, j, BREAK)) - 1.0);
        (f64::from(j) + phase + jitter) * self.piece()
    }

    /// The height of block `j` of fault `k` at a place on the line, from 0
    /// to 1.
    fn block(&self, k: i32, j: i32, u: f64, across: f64) -> f64 {
        let (spacing, piece) = (self.fault.spacing, self.piece());
        let (b0, b1) = (self.start(k, j), self.start(k, j + 1));
        let o = OVERLAP * piece;
        let along = smoothstep(b0 - o, b0 + o, u) * (1.0 - smoothstep(b1 - o, b1 + o, u));
        if along <= 0.0 {
            return 0.0;
        }
        let middle = 0.5 * (b0 + b1);
        // The throw of a fault is largest at the middle of its piece.
        let m = ((u - middle) / (0.5 * (b1 - b0))).clamp(-1.0, 1.0);
        let slip = 0.6 + 0.4 * (1.0 - m * m);
        let shift = SHIFT * (2.0 * unit(self.hash(k, j, SHIFT_CHANNEL)) - 1.0);
        let center = spacing * (f64::from(k) + shift) + self.tilt() * (u - middle);
        let dip = if unit(self.hash(k, j, DIP)) < DOMAIN {
            self.side()
        } else {
            -self.side()
        };
        let width = 1.0 + WIDTH_JITTER * (2.0 * unit(self.hash(k, j, WIDTH)) - 1.0);
        let half = HALF * width * spacing;
        let q = dip * (across - center) / half;
        // A face of a few texel steps keeps the levels of the GPU near those
        // of the CPU.
        let face = MIN_FACE * FRAC_PI_2 / self.line.face_size as f64;
        let crest = CREST.min(1.0 - face / half);
        let height = 0.6 + 0.4 * unit(self.hash(k, j, HEIGHT));
        height * along * slip * FaultPlan::profile(q, crest)
    }

    /// The blocks at a place on the line, from 0 on the basin floor to 1 on
    /// the highest crests.
    #[doc(hidden)]
    pub fn block_part(&self, u: f64, across: f64) -> f64 {
        let k0 = (across / self.fault.spacing).round() as i32;
        let mut part: f64 = 0.0;
        for k in k0 - 2..=k0 + 2 {
            let phase = unit(self.hash(k, 0, PHASE));
            let j0 = (u / self.piece() - phase).floor() as i32;
            for j in j0 - 1..=j0 + 1 {
                part = part.max(self.block(k, j, u, across));
            }
        }
        part
    }

    /// The change of a single fault at a place, from -0.5 on the low side
    /// to 0.5 on the high side, before the taper.
    fn single_part(&self, at: LinePoint) -> f64 {
        let w = self.width(at);
        let d = at.across.hypot(at.past) / w;
        let fade = 1.0 - smoothstep(SINGLE_FLAT, 1.0, d);
        let step = smoothstep(-SCARP, SCARP, at.across / w) - 0.5;
        step * fade
    }

    /// The new level of a texel with the level `start` from before the
    /// stroke, at a place on the line.
    pub(crate) fn level(&self, start: u16, at: LinePoint) -> u16 {
        let spec = &self.fault;
        let taper = smoothstep(0.0, TAPER * spec.radius, at.end);
        let part = if spec.single {
            self.single_part(at)
        } else {
            let d = at.across.hypot(at.past) / self.width(at);
            let envelope = 1.0 - smoothstep(EDGE_FLAT, 1.0, d);
            if envelope <= 0.0 {
                return start;
            }
            envelope * (BASE + (1.0 - BASE) * self.block_part(at.u, at.across))
        };
        let change = spec.height * at.flow * taper * part;
        let old = f64::from(start);
        let new = (old + (change + 0.5).floor()).clamp(0.0, 65535.0);
        spec.limits.apply(old, new) as u16
    }

    /// The level of the blocks at a unit direction, for a cell with the
    /// level `start` from before the stroke. Only a place on the line with
    /// a distance along it in `u` has a level.
    pub fn settled_level(&self, dir: V3, start: u16, u: Range<f64>) -> Option<u16> {
        match self.place(dir) {
            LinePlace::On(at) if u.contains(&at.u) => Some(self.level(start, at)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heightmap::Heightmap;
    use crate::line::LINE_SPACING;
    use crate::math::lonlat_to_dir;

    const START: u16 = 30000;

    fn spec(radius: f64, spacing: f64) -> FaultSpec {
        FaultSpec {
            radius,
            height: 20000.0,
            spacing,
            seed: 7,
            single: false,
            limits: Limits::default(),
        }
    }

    /// A line along the equator from `from` to `to` degrees of longitude,
    /// with the point spacing of the app.
    fn straight(spec: FaultSpec, n: usize, from: f64, to: f64, flow: f64) -> FaultLine {
        let mut line = FaultLine::new(spec, n);
        let step = (LINE_SPACING * line.spec().radius).to_degrees();
        let count = ((to - from).abs() / step).ceil() as usize;
        for i in 0..=count {
            let lon = from + (to - from) * i as f64 / count as f64;
            line.push(lonlat_to_dir(lon, 0.0), flow);
        }
        line
    }

    /// A plan with no line, for the pattern of the blocks.
    fn pattern(spacing: f64, seed: u32) -> FaultPlan {
        let spec = FaultSpec {
            seed,
            ..spec(0.4, spacing)
        };
        FaultLine::new(spec, 4096).plan(0)
    }

    /// Lays the whole line in one plan on flat ground.
    fn laid(line: &FaultLine, n: usize) -> (Heightmap, FaultPlan) {
        let mut map = Heightmap::new(n, START);
        let plan = line.plan_between(0.0..line.length());
        map.begin_stroke();
        map.fault(&plan);
        map.end_stroke();
        (map, plan)
    }

    /// The change of each texel of face 0 with a place on the line. A
    /// texel with no place does not change.
    fn changes(map: &Heightmap, plan: &FaultPlan) -> Vec<(LinePoint, f64)> {
        let n = map.face_size();
        let mut out = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let level = map.get(0, x as i64, y as i64);
                let change = f64::from(level) - f64::from(START);
                match plan.place(map.texel_dir(0, x, y)) {
                    LinePlace::On(at) => out.push((at, change)),
                    _ => assert_eq!(change, 0.0, "{x} {y}"),
                }
            }
        }
        out
    }

    /// The block parts on a line across the stroke at `u`, with a step of
    /// `da`, from `-count * da` to `count * da`.
    fn section(plan: &FaultPlan, u: f64, da: f64, count: i32) -> Vec<f64> {
        (-count..=count)
            .map(|i| plan.block_part(u, f64::from(i) * da))
            .collect()
    }

    /// The places of the steep faces on a section: the runs of steps that
    /// drop or rise faster than `steep` for each step.
    fn faces(parts: &[f64], steep: f64) -> Vec<(usize, f64)> {
        let mut out = Vec::new();
        let mut i = 1;
        while i < parts.len() {
            let d = parts[i] - parts[i - 1];
            if d.abs() > steep {
                out.push((i, d.signum()));
                while i < parts.len() && (parts[i] - parts[i - 1]).abs() > 0.3 * steep {
                    i += 1;
                }
            }
            i += 1;
        }
        out
    }

    #[test]
    fn a_block_has_one_steep_face_and_one_gentle_back_slope() {
        let n = 2000;
        let slope = |i: usize| {
            let q0 = -1.0 + 2.0 * i as f64 / n as f64;
            let q1 = -1.0 + 2.0 * (i + 1) as f64 / n as f64;
            (FaultPlan::profile(q1, CREST) - FaultPlan::profile(q0, CREST)) / (q1 - q0)
        };
        let slopes: Vec<f64> = (0..n).map(slope).collect();
        let up = slopes.iter().cloned().fold(0.0, f64::max);
        let down = slopes.iter().cloned().fold(0.0, f64::min);
        assert!(-down > 4.0 * up, "{up} {down}");
        // The rise is before the crest and the drop is after it.
        let top = (0..n).max_by(|&a, &b| {
            let (pa, pb) = (
                FaultPlan::profile(-1.0 + 2.0 * a as f64 / n as f64, CREST),
                { FaultPlan::profile(-1.0 + 2.0 * b as f64 / n as f64, CREST) },
            );
            pa.total_cmp(&pb)
        });
        let top = top.unwrap();
        assert!(slopes[..top].iter().all(|&s| s >= 0.0));
        assert!(slopes[top + 1..].iter().all(|&s| s <= 0.0));
        assert_eq!(FaultPlan::profile(-0.9, CREST), 0.0, "the basin floor");
    }

    #[test]
    fn the_blocks_are_long_along_the_stroke() {
        let spacing = 0.04;
        let plan = pattern(spacing, 7);
        let (du, da) = (spacing / 8.0, spacing / 8.0);
        let high = |u: f64, a: f64| plan.block_part(u, a) > 0.4;
        let mut along = (0usize, 0usize);
        for row in -80..80 {
            let mut run = 0;
            for j in 0..=1200 {
                if j < 1200 && high(j as f64 * du, row as f64 * da) {
                    run += 1;
                } else if run > 0 {
                    along = (along.0 + run, along.1 + 1);
                    run = 0;
                }
            }
        }
        let mut across = (0usize, 0usize);
        for j in 0..400 {
            let mut run = 0;
            for row in -80..=80 {
                if row < 80 && high(j as f64 * 3.0 * du, row as f64 * da) {
                    run += 1;
                } else if run > 0 {
                    across = (across.0 + run, across.1 + 1);
                    run = 0;
                }
            }
        }
        let along = along.0 as f64 / along.1 as f64 * du;
        let across = across.0 as f64 / across.1 as f64 * da;
        assert!(along >= 4.0 * across, "{along} {across}");
    }

    #[test]
    fn each_block_has_its_steep_face_on_one_side() {
        let spacing = 0.04;
        let plan = pattern(spacing, 7);
        let da = spacing / 128.0;
        let mut checked = 0;
        for i in 0..40 {
            let u = i as f64 * 0.83 * spacing;
            let parts = section(&plan, u, da, 1280);
            // A face drops more than 6 percent of the height in a step of
            // 1/128 of the spacing. A back slope does not.
            for (start, sign) in faces(&parts, 0.06) {
                // The block of the face is on the high side of the face.
                // There, the ground falls gently away from the face.
                let crest = if sign < 0.0 { start - 1 } else { start };
                let away = |s: i64| (crest as i64 + s * if sign < 0.0 { -1 } else { 1 }) as usize;
                let next = away(8);
                if next >= parts.len() || parts[crest] < 0.3 {
                    continue;
                }
                let gentle = (parts[crest] - parts[next]) / 8.0;
                assert!(gentle < 0.035, "{u} {crest} {gentle}");
                checked += 1;
            }
        }
        assert!(checked > 100, "{checked}");
    }

    /// The mean distance between two steep faces on lines across the
    /// stroke, in radians.
    fn fault_distance(plan: &FaultPlan) -> f64 {
        let spacing = plan.fault.spacing;
        let da = spacing / 128.0;
        let (mut sum, mut count) = (0.0, 0);
        for i in 0..200 {
            let u = i as f64 * 0.37 * spacing;
            let parts = section(plan, u, da, 2560);
            let faces = faces(&parts, 0.06);
            for pair in faces.windows(2) {
                sum += (pair[1].0 - pair[0].0) as f64 * da;
                count += 1;
            }
        }
        sum / f64::from(count)
    }

    #[test]
    fn the_spacing_sets_the_distance_between_the_faults() {
        for spacing in [0.02, 0.04] {
            let distance = fault_distance(&pattern(spacing, 7));
            assert!(
                (0.75 * spacing..1.5 * spacing).contains(&distance),
                "{spacing} {distance}"
            );
        }
    }

    #[test]
    fn another_seed_gives_another_pattern() {
        let (a, b) = (pattern(0.04, 7), pattern(0.04, 8));
        let mut differ = 0;
        for i in 0..400 {
            let (u, across) = (i as f64 * 0.013, (i % 20) as f64 * 0.01 - 0.1);
            if (a.block_part(u, across) - b.block_part(u, across)).abs() > 0.1 {
                differ += 1;
            }
        }
        assert!(differ > 100, "{differ}");
    }

    /// The place across the line of the crest of block `j` of fault `k`
    /// at `u`, and its height.
    fn crest(plan: &FaultPlan, k: i32, j: i32, u: f64) -> (f64, f64) {
        let spacing = plan.fault.spacing;
        let places = (-256..=256).map(|i| spacing * (f64::from(k) + f64::from(i) / 128.0));
        let heights = places.map(|a| (a, plan.block(k, j, u, a)));
        heights.fold(
            (0.0, 0.0),
            |best, (a, h)| if h > best.1 { (a, h) } else { best },
        )
    }

    #[test]
    fn a_fault_ends_and_the_next_one_starts_beside_it() {
        let spacing = 0.04;
        let plan = pattern(spacing, 7);
        let o = OVERLAP * plan.piece();
        let (mut offsets, mut ends) = (Vec::new(), 0);
        for k in -3..3 {
            for j in 0..12 {
                let b1 = plan.start(k, j + 1);
                // The block ends: past the overlap, it has no height.
                let (_, after) = crest(&plan, k, j, b1 + o);
                assert_eq!(after, 0.0, "{k} {j}");
                ends += 1;
                // The next block starts beside the end of this block.
                let (a0, h0) = crest(&plan, k, j, b1 - 2.0 * o);
                let (a1, h1) = crest(&plan, k, j + 1, b1 + 2.0 * o);
                assert!(h0 > 0.2 && h1 > 0.2, "{k} {j} {h0} {h1}");
                offsets.push((a1 - a0).abs() / spacing);
            }
        }
        assert_eq!(ends, 72);
        let mean = offsets.iter().sum::<f64>() / offsets.len() as f64;
        let beside = offsets.iter().filter(|&&d| d > 0.1).count();
        assert!(mean > 0.2, "{mean}");
        assert!(3 * beside >= 2 * offsets.len(), "{beside} {offsets:?}");
        assert!(offsets.iter().all(|&d| d < 1.5), "{offsets:?}");
    }

    #[test]
    fn the_blocks_stay_inside_the_brush_radius() {
        let n = 256;
        let line = straight(spec(0.12, 0.03), n, -20.0, 20.0, 1.0);
        let (map, plan) = laid(&line, n);
        let changes = changes(&map, &plan);
        let mut inside = 0;
        for &(at, change) in &changes {
            let d = at.across.hypot(at.past) / plan.fault.radius;
            if d >= 1.0 {
                assert_eq!(change, 0.0, "{at:?}");
            } else if change > 0.0 {
                inside += 1;
            }
            assert!(change >= 0.0, "{at:?}");
        }
        assert!(inside > 1000, "{inside}");
    }

    #[test]
    fn the_flow_sets_the_height_of_the_blocks() {
        let n = 256;
        let full = straight(spec(0.12, 0.03), n, -20.0, 20.0, 1.0);
        let half = straight(spec(0.12, 0.03), n, -20.0, 20.0, 0.5);
        let (full_map, plan) = laid(&full, n);
        let (half_map, _) = laid(&half, n);
        let high = |map: &Heightmap| {
            let changes = changes(map, &plan);
            changes.iter().map(|&(_, c)| c).fold(0.0, f64::max)
        };
        let (a, b) = (high(&full_map), high(&half_map));
        assert!(a > 10000.0, "{a}");
        assert!((b / a - 0.5).abs() < 0.01, "{a} {b}");
    }

    /// The mean change on each side of a single fault, past the
    /// escarpment, in the middle of the line.
    fn sides(line: &FaultLine, n: usize) -> (f64, f64) {
        let (map, plan) = laid(line, n);
        let changes = changes(&map, &plan);
        let r = plan.fault.radius;
        let middle = changes.iter().filter(|(at, _)| at.end > TAPER * r);
        let (mut left, mut right) = ((0.0, 0), (0.0, 0));
        for &(at, change) in middle {
            let x = at.across / r;
            if (0.1..0.2).contains(&x) {
                left = (left.0 + change, left.1 + 1);
            } else if (-0.2..-0.1).contains(&x) {
                right = (right.0 + change, right.1 + 1);
            }
        }
        (left.0 / f64::from(left.1), right.0 / f64::from(right.1))
    }

    #[test]
    fn a_single_fault_makes_one_escarpment_with_the_high_side_on_the_left() {
        let n = 256;
        let single = FaultSpec {
            single: true,
            ..spec(0.12, 0.03)
        };
        let east = straight(single, n, -20.0, 20.0, 1.0);
        let (left, right) = sides(&east, n);
        // The left of a stroke to the east is the north.
        assert!(left > 9000.0 && right < -9000.0, "{left} {right}");
        let west = straight(single, n, 20.0, -20.0, 1.0);
        let (left, right) = sides(&west, n);
        assert!(left > 9000.0 && right < -9000.0, "{left} {right}");
        // A texel on one side of the line is high after one stroke and low
        // after the other.
        let (map_east, _) = laid(&east, n);
        let (map_west, _) = laid(&west, n);
        let (x, y) = ((n / 2) as i64, (n / 2 - 4) as i64);
        let (a, b) = (map_east.get(0, x, y), map_west.get(0, x, y));
        assert!(
            a.abs_diff(START) > 8000 && b.abs_diff(START) > 8000,
            "{a} {b}"
        );
        assert_ne!(a > START, b > START);
    }

    #[test]
    fn a_single_fault_has_a_steep_escarpment() {
        let n = 512;
        let single = FaultSpec {
            single: true,
            ..spec(0.12, 0.03)
        };
        let line = straight(single, n, -20.0, 20.0, 1.0);
        let (map, _) = laid(&line, n);
        let x = (n / 2) as i64;
        let column: Vec<f64> = (0..n as i64).map(|y| f64::from(map.get(0, x, y))).collect();
        let steps: Vec<f64> = column.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        let largest = steps.iter().cloned().fold(0.0, f64::max);
        let steep = steps.iter().filter(|&&s| s > 0.25 * largest).count();
        assert!(largest > 1000.0, "{largest}");
        assert!(steep <= 8, "{steep}");
    }
}
