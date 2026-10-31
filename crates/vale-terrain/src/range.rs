//! The range brush.
//!
//! The brush raises a mountain range along the line of a stroke. The range
//! has a high core and lower sides. On it are ridges along the line, in
//! pieces. Each piece ends beside the start of the next piece, en echelon,
//! and the low ground between two pieces is a gap for the rivers. Erode
//! steps after the stroke cut the valleys.

use std::f64::consts::FRAC_PI_2;
use std::ops::{Deref, Range};

use crate::erode::{ErodeBrush, ErodeGrid, ErodeStep, Erosion};
use crate::heightmap::{Limits, MAX_BRUSH_RADIUS, hash};
use crate::line::{
    LinePlace, LinePlan, LinePoint, REACH, StrokeLine, TAPER, bump, smoothstep, unit,
};
use crate::math::V3;
use crate::ridge::TAPER_WIDTH;

/// The number of erode steps of one request of `erode_range`.
pub const RANGE_STEPS: usize = 4;

/// The rate of the erode steps of one request of `erode_range`, for an
/// erosion setting of 1 and a brush radius of 16 cells. A cell is under
/// the brushes of one or two requests, so the rate does not grow with the
/// length of line of a request.
pub const RANGE_ERODE_RATE: f64 = 1.0;

/// The length of a piece of a ridge, in ridge spacings.
const PIECE: f64 = 8.0;

/// The largest shift of the end of a piece along the line, as a part of
/// the piece length.
const BREAK_JITTER: f64 = 0.2;

/// Half of the length over which a piece fades in or out, as a part of the
/// piece length.
const OVERLAP: f64 = 0.15;

/// The largest shift of a piece across the line, in ridge spacings.
const SHIFT: f64 = 0.15;

/// The shift across the line from one end of a piece to the other end, in
/// ridge spacings. The pieces of all ridges turn to one side.
const ECHELON: f64 = 0.5;

/// Half of the width of a ridge, in ridge spacings.
const HALF: f64 = 0.6;

/// The core of the range is flat out to `CORE_FLAT` of the radius, and it
/// ends at `CORE`.
const CORE_FLAT: f64 = 0.15;
const CORE: f64 = 0.55;

/// The part of the height that the core gives. The foot gives the rest.
const CORE_SHARE: f64 = 0.55;

/// The part of the height that the ground between the ridges has.
const BASE: f64 = 0.2;

/// The smallest ridge spacing, in texel steps.
const MIN_SPACING: f64 = 6.0;

/// The largest ridge spacing, as a part of the brush radius.
const MAX_SPACING: f64 = 0.5;

const SALT_RANGE: u32 = 0x63a1_d84f;

/// The channels of the hash of a piece.
const BREAK: u32 = 0;
const SHIFT_CHANNEL: u32 = 1;
const HEIGHT: u32 = 2;
const SIDE: u32 = 3;
const PHASE: u32 = 4;

/// The settings of a range stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeSpec {
    /// The brush radius as an angle, in radians. It is half of the width
    /// of the range.
    pub radius: f64,
    /// The height of the range at full flow, in 16-bit steps.
    pub height: f64,
    /// The mean distance between two ridges as an angle, in radians.
    pub spacing: f64,
    /// The seed of the pattern.
    pub seed: u32,
    pub limits: Limits,
}

/// The line of one range stroke.
#[derive(Clone, Debug)]
pub struct RangeLine {
    spec: RangeSpec,
    line: StrokeLine,
}

/// The part of the heightmap that changes after new points of a line, and
/// the segments that give the levels there.
#[derive(Clone, Debug)]
pub struct RangePlan {
    /// The settings, inside their limits.
    pub range: RangeSpec,
    pub line: LinePlan,
}

impl Deref for RangePlan {
    type Target = LinePlan;

    fn deref(&self) -> &LinePlan {
        &self.line
    }
}

impl RangeLine {
    /// Starts a line with no points on a heightmap with `face_size` texels
    /// along one side of a face. The settings go inside their limits.
    pub fn new(spec: RangeSpec, face_size: usize) -> RangeLine {
        let step = FRAC_PI_2 / face_size as f64;
        let radius = spec.radius.max(4.0 * step).min(MAX_BRUSH_RADIUS / REACH);
        let spacing = spec
            .spacing
            .min(MAX_SPACING * radius)
            .max(MIN_SPACING * step);
        let spec = RangeSpec {
            radius,
            height: spec.height.clamp(0.0, 65535.0),
            spacing,
            ..spec
        };
        RangeLine {
            spec,
            line: StrokeLine::new(radius, face_size),
        }
    }

    /// The settings, inside their limits.
    pub fn spec(&self) -> RangeSpec {
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
    pub fn plan(&self, from: usize) -> RangePlan {
        self.wrap(self.line.plan(from))
    }

    /// The plan for the distances `u` along the line, as
    /// `StrokeLine::plan_between`.
    pub fn plan_between(&self, u: Range<f64>) -> RangePlan {
        self.wrap(self.line.plan_between(u))
    }

    /// As `StrokeLine::margin`.
    #[doc(hidden)]
    pub fn margin(&self, p: V3) -> f64 {
        self.line.margin(p)
    }

    /// The brushes of the erode steps of the distances `u` along the line:
    /// one at each point from one brush radius before `u` to its end.
    pub fn brushes(&self, u: Range<f64>) -> Vec<ErodeBrush> {
        let r = self.spec.radius;
        let points = self.line.points_in(u.start - r..=u.end);
        let brushes = points.into_iter().map(|(center, _)| ErodeBrush {
            center,
            radius: r,
            hardness: 0.5,
            flow: 1.0,
        });
        brushes.collect()
    }

    fn wrap(&self, line: LinePlan) -> RangePlan {
        RangePlan {
            range: self.spec,
            line,
        }
    }
}

impl RangePlan {
    /// The length of a piece of a ridge, in radians.
    pub(crate) fn piece(&self) -> f64 {
        PIECE * self.range.spacing
    }

    /// The seed of the hashes of the pieces.
    pub(crate) fn seed(&self) -> u32 {
        self.range.seed ^ SALT_RANGE
    }

    /// The shift of a crest across the line for each radian along it.
    pub(crate) fn tilt(&self) -> f64 {
        let side = if self.hash(0, 0, SIDE) & 1 == 0 {
            1.0
        } else {
            -1.0
        };
        side * ECHELON * self.range.spacing / self.piece()
    }

    fn hash(&self, k: i32, j: i32, channel: u32) -> u32 {
        hash(k as u32, j as u32, channel, self.seed())
    }

    /// The distance from the line as a part of the width of the range at
    /// that place. The range is narrower near an end of the line.
    pub(crate) fn distance(&self, at: LinePoint) -> f64 {
        let r = self.range.radius;
        let taper = smoothstep(0.0, TAPER * r, at.end);
        at.across.hypot(at.past) / (r * (TAPER_WIDTH + (1.0 - TAPER_WIDTH) * taper))
    }

    /// The part of the height at a distance `d` from the line, as a part of
    /// the width. It is 1 at the line and 0 at 1 and past it.
    #[doc(hidden)]
    pub fn envelope(d: f64) -> f64 {
        let core = 1.0 - smoothstep(CORE_FLAT, CORE, d);
        let foot = 1.0 - smoothstep(0.0, 1.0, d);
        CORE_SHARE * core + (1.0 - CORE_SHARE) * foot
    }

    /// The start of piece `j` of ridge `k`, in radians along the line.
    fn start(&self, k: i32, j: i32) -> f64 {
        let phase = unit(self.hash(k, 0, PHASE));
        let jitter = BREAK_JITTER * (2.0 * unit(self.hash(k, j, BREAK)) - 1.0);
        (f64::from(j) + phase + jitter) * self.piece()
    }

    /// The center of piece `j` of ridge `k` across the line at `u`, and
    /// the effect of the piece along the line there, from 0 to 1.
    fn crest(&self, k: i32, j: i32, u: f64) -> (f64, f64) {
        let (spacing, piece) = (self.range.spacing, self.piece());
        let (b0, b1) = (self.start(k, j), self.start(k, j + 1));
        let o = OVERLAP * piece;
        let along = smoothstep(b0 - o, b0 + o, u) * (1.0 - smoothstep(b1 - o, b1 + o, u));
        let tilt = self.tilt();
        let shift = SHIFT * (2.0 * unit(self.hash(k, j, SHIFT_CHANNEL)) - 1.0);
        let center = spacing * (f64::from(k) + shift) + tilt * (u - 0.5 * (b0 + b1));
        (center, along)
    }

    /// The pieces near a place: ridge `k` and piece `j`. Each other piece
    /// has no effect there.
    fn pieces(&self, u: f64, across: f64) -> impl Iterator<Item = (i32, i32)> + '_ {
        let k0 = (across / self.range.spacing).round() as i32;
        (k0 - 2..=k0 + 2).flat_map(move |k| {
            let phase = unit(self.hash(k, 0, PHASE));
            let j0 = (u / self.piece() - phase).floor() as i32;
            (j0 - 1..=j0 + 1).map(move |j| (k, j))
        })
    }

    /// The ridges at a place on the line, from 0 between the ridges to 1 on
    /// the highest crests.
    #[doc(hidden)]
    pub fn ridge_part(&self, u: f64, across: f64) -> f64 {
        let half = HALF * self.range.spacing;
        let parts = self.pieces(u, across).map(|(k, j)| {
            let (center, along) = self.crest(k, j, u);
            let height = 0.75 + 0.25 * unit(self.hash(k, j, HEIGHT));
            height * along * bump((across - center).abs() / half)
        });
        parts.fold(0.0, f64::max)
    }

    /// The new level of a texel with the level `start` from before the
    /// stroke, at a place on the line.
    pub(crate) fn level(&self, start: u16, at: LinePoint) -> u16 {
        let spec = &self.range;
        let d = self.distance(at);
        let envelope = RangePlan::envelope(d);
        if envelope <= 0.0 {
            return start;
        }
        let taper = smoothstep(0.0, TAPER * spec.radius, at.end);
        let ridges = BASE + (1.0 - BASE) * self.ridge_part(at.u, at.across);
        let change = spec.height * at.flow * taper * envelope * ridges;
        let old = f64::from(start);
        let new = (old + (change + 0.5).floor()).clamp(0.0, 65535.0);
        spec.limits.apply(old, new) as u16
    }

    /// The level of the range at a unit direction, for a cell with the
    /// level `start` from before the stroke. Only a place on the line with
    /// a distance along it in `u` has a level.
    pub fn settled_level(&self, dir: V3, start: u16, u: Range<f64>) -> Option<u16> {
        match self.place(dir) {
            LinePlace::On(at) if u.contains(&at.u) => Some(self.level(start, at)),
            _ => None,
        }
    }
}

/// One request of the erode steps of a range. The cells under the line of
/// the distances `u` take the levels of the range, and then
/// `RANGE_STEPS` erode steps lower them. `amount` is the erosion setting.
/// The result has the sum of the drops of the steps.
///
/// Call it for each part of the line that the plans cover, in the order of
/// the line, with cells that hold the ground from before the stroke.
pub fn erode_range(
    erosion: &mut Erosion,
    line: &RangeLine,
    u: Range<f64>,
    amount: f64,
) -> ErodeStep {
    let spec = line.spec();
    let plan = line.plan_between(u.clone());
    let brushes = line.brushes(u.clone());
    erosion.set_limits(spec.limits);
    erosion.lift(&brushes, |dir, start| {
        plan.settled_level(dir, start, u.clone())
    });
    let cell = match erosion.grid() {
        ErodeGrid::Global { size } => FRAC_PI_2 / size as f64,
        ErodeGrid::Window(window) => FRAC_PI_2 * window.cell as f64 / window.face_size as f64,
    };
    let cells = spec.radius / cell;
    // A slope with no valley erodes less with smaller cells, so the rate
    // grows with the number of cells in the radius.
    let rate = amount * RANGE_ERODE_RATE * (cells / 16.0).sqrt();
    erosion.steps(&brushes, rate, RANGE_STEPS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::meters_to_level;
    use crate::flow::{Window, WindowHeights};
    use crate::heightmap::Heightmap;
    use crate::line::LINE_SPACING;
    use crate::math::lonlat_to_dir;
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;

    const START: u16 = 10000;

    fn spec(radius: f64, spacing: f64) -> RangeSpec {
        RangeSpec {
            radius,
            height: 20000.0,
            spacing,
            seed: 7,
            limits: Limits::default(),
        }
    }

    /// A line along the equator from `from` to `to` degrees of longitude,
    /// with the point spacing of the app.
    fn straight(spec: RangeSpec, n: usize, from: f64, to: f64, flow: f64) -> RangeLine {
        let mut line = RangeLine::new(spec, n);
        for point in straight_points(line.spec().radius, from, to) {
            line.push(point, flow);
        }
        line
    }

    /// The points of a line along the equator with the point spacing of the
    /// app for a brush radius.
    fn straight_points(radius: f64, from: f64, to: f64) -> Vec<V3> {
        let step = (LINE_SPACING * radius).to_degrees();
        let count = ((to - from) / step).ceil() as usize;
        let lon = |i: usize| from + (to - from) * i as f64 / count as f64;
        (0..=count).map(|i| lonlat_to_dir(lon(i), 0.0)).collect()
    }

    /// A plan with no line, for the pattern of the ridges.
    fn pattern(spacing: f64, seed: u32) -> RangePlan {
        let spec = RangeSpec {
            seed,
            ..spec(0.4, spacing)
        };
        RangeLine::new(spec, 4096).plan(0)
    }

    /// Lays the whole line in one plan on flat ground. Returns the map and
    /// the plan.
    fn laid(line: &RangeLine, n: usize) -> (Heightmap, RangePlan) {
        let mut map = Heightmap::new(n, START);
        let plan = line.plan_between(0.0..line.length());
        map.begin_stroke();
        map.range(&plan);
        map.end_stroke();
        (map, plan)
    }

    /// The change of each texel of face 0 with a place on the line.
    fn changes(map: &Heightmap, plan: &RangePlan) -> Vec<(LinePoint, f64)> {
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

    fn mean(values: impl Iterator<Item = f64>) -> f64 {
        let (sum, count) = values.fold((0.0, 0), |(s, c), v| (s + v, c + 1));
        assert!(count > 0);
        sum / f64::from(count)
    }

    #[test]
    fn a_range_has_a_high_core_and_lower_sides() {
        let n = 256;
        let line = straight(spec(0.12, 0.04), n, -20.0, 20.0, 1.0);
        let (map, plan) = laid(&line, n);
        let changes = changes(&map, &plan);
        let middle = |at: &LinePoint| at.end > TAPER * plan.range.radius;
        let band = |lo: f64, hi: f64| {
            let near = changes.iter().filter(|(at, _)| middle(at));
            let near = near.filter(|(at, _)| (lo..hi).contains(&plan.distance(*at)));
            mean(near.map(|&(_, change)| change))
        };
        let (core, side) = (band(0.0, 0.1), band(0.55, 0.65));
        assert!(core > 2.0 * side && side > 0.0, "{core} {side}");
        for &(at, change) in &changes {
            if plan.distance(at) >= 1.02 {
                assert_eq!(change, 0.0, "{at:?}");
            }
        }
    }

    /// The parts of the ridges on a grid of `rows` rows across the line and
    /// `columns` columns along it, with steps `du` and `da`. Row 0 is at
    /// `across0`.
    fn grid(
        plan: &RangePlan,
        (columns, du): (usize, f64),
        (rows, da, across0): (usize, f64, f64),
    ) -> Vec<Vec<f64>> {
        let row = |i: usize| {
            let across = across0 + i as f64 * da;
            (0..columns)
                .map(|j| plan.ridge_part(j as f64 * du, across))
                .collect()
        };
        (0..rows).map(row).collect()
    }

    /// The mean length of the runs of true values.
    fn mean_run(runs: impl Iterator<Item = Vec<bool>>) -> f64 {
        let (mut sum, mut count) = (0usize, 0usize);
        for run in runs {
            let mut length = 0;
            for high in run.into_iter().chain([false]) {
                if high {
                    length += 1;
                } else if length > 0 {
                    (sum, count) = (sum + length, count + 1);
                    length = 0;
                }
            }
        }
        sum as f64 / count.max(1) as f64
    }

    #[test]
    fn the_ridges_of_a_range_are_long_along_the_stroke() {
        let spacing = 0.04;
        let plan = pattern(spacing, 7);
        let (du, da) = (spacing / 8.0, spacing / 8.0);
        let parts = grid(&plan, (1200, du), (160, da, -10.0 * spacing));
        let along = mean_run(
            parts
                .iter()
                .map(|row| row.iter().map(|&r| r > 0.6).collect()),
        );
        let across =
            mean_run((0..parts[0].len()).map(|j| parts.iter().map(|row| row[j] > 0.6).collect()));
        let (along, across) = (along * du, across * da);
        assert!(along >= 4.0 * across, "{along} {across}");
    }

    /// The mean distance between two crests on lines across the stroke.
    fn crest_distance(plan: &RangePlan) -> f64 {
        let spacing = plan.range.spacing;
        let da = spacing / 32.0;
        let mut distances = Vec::new();
        for i in 0..200 {
            let u = i as f64 * 0.37 * spacing;
            let parts: Vec<f64> = (0..1280)
                .map(|j| plan.ridge_part(u, (j as f64 - 640.0) * da))
                .collect();
            let peaks: Vec<usize> = (1..parts.len() - 1)
                .filter(|&j| parts[j] > 0.3 && parts[j] > parts[j - 1] && parts[j] >= parts[j + 1])
                .collect();
            distances.extend(peaks.windows(2).map(|w| (w[1] - w[0]) as f64 * da));
        }
        mean(distances.into_iter())
    }

    #[test]
    fn the_spacing_sets_the_mean_distance_between_ridges() {
        for spacing in [0.03, 0.06] {
            let distance = crest_distance(&pattern(spacing, 7));
            assert!(
                (distance / spacing - 1.0).abs() < 0.25,
                "{spacing} {distance}"
            );
        }
    }

    #[test]
    fn another_seed_gives_another_pattern() {
        let spacing = 0.04;
        let size = ((400, spacing / 4.0), (40, spacing / 4.0, -5.0 * spacing));
        let a = grid(&pattern(spacing, 7), size.0, size.1);
        assert!(a == grid(&pattern(spacing, 7), size.0, size.1));
        let b = grid(&pattern(spacing, 8), size.0, size.1);
        let pairs = a.iter().flatten().zip(b.iter().flatten());
        let total = pairs.clone().count();
        let changed = pairs.filter(|(a, b)| (*a - *b).abs() > 0.05).count();
        assert!(changed * 10 > total * 3, "{changed} of {total}");
    }

    /// A crest piece: the first and the last column, and the mean row at
    /// each of them.
    #[derive(Debug)]
    struct Piece {
        first: (usize, f64),
        last: (usize, f64),
    }

    /// The pieces of crest of the grid: the 4-connected parts above
    /// `high`. A part at a side of the grid is not a piece.
    fn pieces(parts: &[Vec<f64>], high: f64) -> Vec<Piece> {
        let (rows, columns) = (parts.len(), parts[0].len());
        let mut seen = vec![vec![false; columns]; rows];
        let mut pieces = Vec::new();
        for i in 0..rows {
            for j in 0..columns {
                if seen[i][j] || parts[i][j] <= high {
                    continue;
                }
                seen[i][j] = true;
                let mut stack = vec![(i, j)];
                let mut cells = Vec::new();
                while let Some((i, j)) = stack.pop() {
                    cells.push((i, j));
                    let near = [
                        (i.wrapping_sub(1), j),
                        (i + 1, j),
                        (i, j.wrapping_sub(1)),
                        (i, j + 1),
                    ];
                    for (a, b) in near {
                        if a < rows && b < columns && !seen[a][b] && parts[a][b] > high {
                            seen[a][b] = true;
                            stack.push((a, b));
                        }
                    }
                }
                let side = cells
                    .iter()
                    .any(|&(i, j)| i == 0 || j == 0 || i == rows - 1 || j == columns - 1);
                if side {
                    continue;
                }
                let first = cells.iter().map(|c| c.1).min().unwrap();
                let last = cells.iter().map(|c| c.1).max().unwrap();
                let row_at = |column: usize| {
                    mean(cells.iter().filter(|c| c.1 == column).map(|c| c.0 as f64))
                };
                pieces.push(Piece {
                    first: (first, row_at(first)),
                    last: (last, row_at(last)),
                });
            }
        }
        pieces
    }

    #[test]
    fn the_ridges_of_a_range_end_and_start_beside_each_other() {
        let spacing = 0.04;
        let plan = pattern(spacing, 7);
        let piece = PIECE * spacing;
        let (du, da) = (piece / 32.0, spacing / 16.0);
        let parts = grid(&plan, (32 * 30, du), (16 * 12, da, -6.0 * spacing));
        let pieces = pieces(&parts, 0.7);
        assert!(pieces.len() > 50, "{}", pieces.len());
        let lengths = pieces
            .iter()
            .map(|p| (p.last.0 - p.first.0) as f64 * du / piece);
        let fitting = lengths.filter(|l| (0.4..=2.5).contains(l)).count();
        assert_eq!(fitting, pieces.len());
        let ends: Vec<(usize, (usize, f64))> = pieces
            .iter()
            .enumerate()
            .flat_map(|(i, p)| [(i, p.first), (i, p.last)])
            .collect();
        let beside = ends.iter().filter(|&&(i, (column, row))| {
            ends.iter().any(|&(other, (c, r))| {
                let along = (c as f64 - column as f64).abs() * du;
                let across = (r - row).abs() * da;
                other != i && along <= 0.5 * piece && (0.25..=1.2).contains(&(across / spacing))
            })
        });
        let beside = beside.count();
        assert!(beside * 10 >= ends.len() * 8, "{beside} of {}", ends.len());
    }

    #[test]
    fn the_radius_sets_the_width_and_the_strength_and_flow_set_the_height() {
        let n = 256;
        let step = FRAC_PI_2 / n as f64;
        // The largest distance of a changed texel from the line, and the
        // largest change.
        let run = |radius: f64, height: f64, flow: f64| {
            let spec = RangeSpec {
                height,
                ..spec(radius, 0.04)
            };
            let line = straight(spec, n, -25.0, 25.0, flow);
            let (map, plan) = laid(&line, n);
            let mut width: f64 = 0.0;
            let mut top: f64 = 0.0;
            for (at, change) in changes(&map, &plan) {
                let distance = at.across.hypot(at.past);
                if change != 0.0 {
                    assert!(distance <= radius + step, "{radius} {distance}");
                    width = width.max(distance);
                }
                top = top.max(change);
            }
            assert!(width >= 0.9 * radius, "{radius} {width}");
            (width, top)
        };
        let (narrow, high) = run(0.08, 20000.0, 1.0);
        let (wide, _) = run(0.16, 20000.0, 1.0);
        assert!((wide / narrow - 2.0).abs() < 0.15, "{narrow} {wide}");
        let (_, half_flow) = run(0.08, 20000.0, 0.5);
        let (_, half_height) = run(0.08, 10000.0, 1.0);
        for half in [half_flow, half_height] {
            assert!((half / high - 0.5).abs() < 0.01, "{high} {half}");
        }
    }

    /// The flow of the texels of a square of face 0: the receiver of each
    /// texel and the number of texels that drain through it. The water
    /// leaves at the sides and at the sea. A pit fills to its rim.
    fn texel_flow(map: &Heightmap, first: usize, side: usize, sea: u16) -> (Vec<usize>, Vec<f64>) {
        let level = |i: usize| map.get(0, (first + i % side) as i64, (first + i / side) as i64);
        let count = side * side;
        let mut receiver = vec![usize::MAX; count];
        let mut done = vec![false; count];
        let mut heap = BinaryHeap::new();
        for (i, done) in done.iter_mut().enumerate() {
            let (x, y) = (i % side, i / side);
            let edge = x == 0 || y == 0 || x == side - 1 || y == side - 1;
            if edge || level(i) <= sea {
                *done = true;
                heap.push(Reverse((level(i), i)));
            }
        }
        let mut order = Vec::with_capacity(count);
        while let Some(Reverse((fill, i))) = heap.pop() {
            order.push(i);
            let (x, y) = ((i % side) as i64, (i / side) as i64);
            for (dx, dy) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let (a, b) = (x + dx, y + dy);
                if a < 0 || b < 0 || a >= side as i64 || b >= side as i64 {
                    continue;
                }
                let j = b as usize * side + a as usize;
                if done[j] {
                    continue;
                }
                done[j] = true;
                receiver[j] = i;
                heap.push(Reverse((level(j).max(fill), j)));
            }
        }
        let mut area = vec![1.0; count];
        for &i in order.iter().rev() {
            if receiver[i] != usize::MAX {
                area[receiver[i]] += area[i];
            }
        }
        (receiver, area)
    }

    /// The rivers of a range on a square of face 0 from texel `first` with
    /// `side` texels along each side:
    /// the area-weighted part of the river texels in the middle of the
    /// range that flow along it, the rivers that leave the range, and the
    /// number of crossings of a crest line with the number of them where
    /// the ridges are low.
    fn rivers(
        map: &Heightmap,
        plan: &RangePlan,
        (first, side): (usize, usize),
        sea: u16,
    ) -> (f64, usize, usize, usize) {
        let r = plan.range.radius;
        let (receiver, area) = texel_flow(map, first, side, sea);
        let place =
            |i: usize| match plan.place(map.texel_dir(0, first + i % side, first + i / side)) {
                LinePlace::On(at) => Some(at),
                _ => None,
            };
        let inside = |i: usize| place(i).is_some_and(|at| plan.distance(at) < 1.0);
        let river = |i: usize| area[i] >= 40.0;

        // Along: the direction of each river texel to the texel 3 steps
        // down.
        let (mut along, mut total) = (0.0, 0.0);
        for i in (0..side * side).filter(|&i| river(i)) {
            let Some(at) = place(i) else { continue };
            if plan.distance(at) >= 0.8 || at.end < TAPER * r {
                continue;
            }
            let mut j = i;
            for _ in 0..3 {
                if receiver[j] != usize::MAX {
                    j = receiver[j];
                }
            }
            let dx = (j % side) as f64 - (i % side) as f64;
            let dy = (j / side) as f64 - (i / side) as f64;
            if dx == 0.0 && dy == 0.0 {
                continue;
            }
            total += area[i];
            if dx.abs() / dx.hypot(dy) > 0.7 {
                along += area[i];
            }
        }
        let along = along / total;

        // Out: the rivers that leave the range, more than 4 texels apart.
        let mut exits: Vec<usize> = Vec::new();
        for i in (0..side * side).filter(|&i| river(i) && inside(i)) {
            let to = receiver[i];
            if to == usize::MAX || inside(to) {
                continue;
            }
            let apart = |e: &usize| {
                let dx = (e % side) as f64 - (i % side) as f64;
                let dy = (e / side) as f64 - (i / side) as f64;
                dx.hypot(dy) > 4.0
            };
            if exits.iter().all(apart) {
                exits.push(i);
            }
        }

        // Gaps: where a river crosses the crest line of a piece, the
        // ridges from before the erosion are low.
        let (mut crossings, mut low) = (0, 0);
        for i in (0..side * side).filter(|&i| river(i)) {
            let to = receiver[i];
            let (Some(a), Some(b)) = (place(i), (to != usize::MAX).then(|| place(to)).flatten())
            else {
                continue;
            };
            let middle = |at: LinePoint| plan.distance(at) < 0.9 && at.end >= TAPER * r;
            if !middle(a) || !middle(b) {
                continue;
            }
            let (u, across) = (0.5 * (a.u + b.u), 0.5 * (a.across + b.across));
            let crosses = plan.pieces(u, across).any(|(k, j)| {
                let (ca, wa) = plan.crest(k, j, a.u);
                let (cb, wb) = plan.crest(k, j, b.u);
                (a.across - ca).signum() != (b.across - cb).signum() && wa.max(wb) > 0.05
            });
            if crosses {
                crossings += 1;
                low += usize::from(plan.ridge_part(u, across) < 0.5);
            }
        }
        (along, exits.len(), crossings, low)
    }

    /// Lays a range with its erode requests as the app does, on land that
    /// slopes down to a sea south of the range, with the largest erosion.
    /// Returns the rivers after the erosion.
    fn river_run(seed: u32) -> (f64, usize, usize, usize) {
        let n = 256;
        let sea = meters_to_level(0.0);
        let mut map = Heightmap::new(n, sea);
        for y in 0..n {
            for x in 0..n {
                let lat = map.texel_dir(0, x, y)[2].asin().to_degrees();
                let meters = if lat > -12.0 {
                    40.0 * (lat + 12.0)
                } else {
                    -100.0
                };
                map.set(0, x, y, meters_to_level(meters));
            }
        }
        let spec = RangeSpec {
            radius: 0.12,
            height: f64::from(meters_to_level(2500.0) - sea),
            spacing: 0.04,
            seed,
            limits: Limits::default(),
        };
        let window = Window::centered(lonlat_to_dir(0.0, -3.0), n, 1, 224);
        let mut erosion = Erosion::window_alone(&WindowHeights::new(&map, window));
        let mut line = RangeLine::new(spec, n);
        let r = line.spec().radius;
        let points = straight_points(r, -32.0, 32.0);
        let (mut from, mut lifted) = (0, 0.0);
        let mut steps = Vec::new();
        map.begin_stroke();
        for (i, &point) in points.iter().enumerate() {
            line.push(point, 1.0);
            let plan = line.plan(from);
            map.range(&plan);
            from = plan.end;
            let ended = i + 1 == points.len();
            let settled = if ended {
                line.length()
            } else {
                line.length() - TAPER * r
            };
            if settled - lifted >= r || (ended && settled > lifted) {
                steps.push(erode_range(&mut erosion, &line, lifted..settled, 1.0));
                lifted = settled;
            }
        }
        let plan = line.plan_between(0.0..line.length());
        for step in &steps {
            map.erode(step);
        }
        map.end_stroke();
        rivers(&map, &plan, (16, n - 32), sea)
    }

    #[test]
    fn the_rivers_of_a_range_run_along_it_and_leave_through_gaps() {
        let (mut along, mut crossings, mut low) = (0.0, 0, 0);
        for seed in [7, 8, 9] {
            let (seed_along, exits, seed_crossings, seed_low) = river_run(seed);
            assert!(exits >= 2, "seed {seed}: {exits}");
            along += seed_along / 3.0;
            crossings += seed_crossings;
            low += seed_low;
        }
        assert!(along >= 0.75, "{along}");
        assert!(
            crossings > 0 && low * 10 >= crossings * 7,
            "{low} of {crossings}"
        );
    }
}
