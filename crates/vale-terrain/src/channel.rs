//! The channels that the Carve mode cuts along the rivers.
//!
//! The flow gives the path and the drained area of each river. A channel is
//! a thin cut at the full resolution of the heightmap, along a smooth line
//! through that path. Its width grows with the square root of the drained
//! area. A large river on a low slope also gets a flat floodplain beside the
//! channel.
//!
//! The Glacial mode cuts a wide valley along the same rivers, with a flat
//! floor and steep sides.
//!
//! Each river node gives one curve of two straight pieces. A grid of bins
//! lists the curves near each bin, so a texel tests only a few curves.

use std::f64::consts::FRAC_PI_2;

use crate::cube::{ELEV_MAX, ELEV_MIN, FACES, warp};
use crate::flow::{RIVER_MIN_AREA, RIVER_OCTAVES, Window};
use crate::math::{V3, add, dot, length, scale};

/// The drained area where the channel has the width of the Channel width
/// setting, in square kilometers.
pub const REFERENCE_AREA_KM2: f64 = 1.0e6;

/// The default channel width, in kilometers.
pub const CHANNEL_WIDTH_KM: f64 = 0.6;

/// The smallest and the largest channel width, in kilometers.
pub const CHANNEL_WIDTHS_KM: (f64, f64) = (0.05, 10.0);

/// The default floodplain width, in kilometers. It is the full width of the
/// floodplain of a river that has a full floodplain.
pub const FLOODPLAIN_KM: f64 = 10.0;

/// The smallest and the largest floodplain width, in kilometers.
pub const FLOODPLAINS_KM: (f64, f64) = (0.0, 100.0);

/// A river gets a floodplain from the first drained area, and a full
/// floodplain from the second area, in square kilometers.
pub const PLAIN_AREAS_KM2: (f64, f64) = (1.0e5, 1.0e6);

/// A river has a full floodplain up to the first slope, and no floodplain
/// from the second slope, in meters for each meter.
pub const PLAIN_SLOPES: (f64, f64) = (2.0e-4, 1.0e-3);

/// The width of a glacial valley over the width of the channel of the same
/// river.
pub const VALLEY_WIDTH_FACTOR: f64 = 10.0;

/// The largest width of a glacial valley at the reference area, in
/// kilometers. The bins of the curves hold valleys up to this width.
pub const VALLEY_MAX_KM: f64 = 30.0;

/// The part of the half-width of a glacial valley that is flat floor.
pub const VALLEY_FLOOR_PART: f64 = 0.4;

/// The height of the side of a glacial valley at its edge, over the height of
/// the river level above the floor.
pub const VALLEY_WALL_RISE: f64 = 2.0;

/// The radius of the world of `ChannelShape::default`, in kilometers.
const DEFAULT_WORLD_KM: f64 = 6371.0;

/// The number of 32-bit numbers of one curve in `ChannelBuffers::curves`.
pub const CURVE_FLOATS: usize = 24;

/// The settings of the channels of one stamp, as angles on the sphere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelShape {
    /// The width of a channel that drains `reference`, in radians.
    pub width: f64,
    /// The drained area where the channel has `width`, in steradians.
    pub reference: f64,
    /// The full width of a full floodplain, in radians.
    pub floodplain: f64,
}

impl ChannelShape {
    /// The settings in kilometers, on a world with a radius of `world_km`.
    /// Each setting is clamped to its range.
    pub fn new(width_km: f64, floodplain_km: f64, world_km: f64) -> ChannelShape {
        let width = width_km.clamp(CHANNEL_WIDTHS_KM.0, CHANNEL_WIDTHS_KM.1);
        let floodplain = floodplain_km.clamp(FLOODPLAINS_KM.0, FLOODPLAINS_KM.1);
        ChannelShape {
            width: width / world_km,
            reference: REFERENCE_AREA_KM2 / (world_km * world_km),
            floodplain: floodplain / world_km,
        }
    }

    /// The width of a channel that drains `area` steradians, in radians.
    pub fn width_at(&self, area: f64) -> f64 {
        self.width * (area.max(0.0) / self.reference).sqrt()
    }
}

impl Default for ChannelShape {
    /// The default settings on a world the size of the Earth.
    fn default() -> ChannelShape {
        ChannelShape::new(CHANNEL_WIDTH_KM, FLOODPLAIN_KM, DEFAULT_WORLD_KM)
    }
}

fn smoothstep(x: f64, from: f64, to: f64) -> f64 {
    let k = ((x - from) / (to - from)).clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// The part of a full floodplain that a river gets, from 0 to 1. `area` is
/// the drained area in steradians, and `slope` is in meters for each meter.
pub(crate) fn plain_factor(area: f64, slope: f64, world_km: f64) -> f64 {
    let km2 = area * world_km * world_km;
    let large = smoothstep(km2, PLAIN_AREAS_KM2.0, PLAIN_AREAS_KM2.1);
    large * (1.0 - smoothstep(slope, PLAIN_SLOPES.0, PLAIN_SLOPES.1))
}

/// The meters of one 16-bit step.
pub(crate) const METERS_PER_LEVEL: f64 = (ELEV_MAX - ELEV_MIN) / 65535.0;

/// The depth of a channel for a drained area, from 0.25 to 1. It grows over
/// `RIVER_OCTAVES` doublings from `RIVER_MIN_AREA`.
pub fn channel_size(area: f64) -> f64 {
    let octaves = (area.max(1e-30) / RIVER_MIN_AREA).log2() / RIVER_OCTAVES;
    0.25 + 0.75 * octaves.clamp(0.0, 1.0)
}

/// One river node as a curve of two straight pieces through 3 points. Each
/// value at a place on a piece is a blend of the values at its two ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelCurve {
    /// Unit vectors.
    pub points: [V3; 3],
    /// The level of the floodplain, in 16-bit steps. It goes down along the
    /// flow.
    pub levels: [f64; 3],
    /// The drained area, in steradians.
    pub areas: [f64; 3],
    /// The part of a full floodplain, from 0 to 1.
    pub plains: [f64; 3],
}

impl ChannelCurve {
    /// The distance from `p` to the nearer piece, and the values at the
    /// nearest place of that piece: the level, the area, and the plain part.
    /// The distance is the chord, which is the angle for a small angle.
    pub fn nearest(&self, p: V3) -> (f64, [f64; 3]) {
        let mut best = (f64::INFINITY, [0.0; 3]);
        for i in 0..2 {
            let (a, b) = (self.points[i], self.points[i + 1]);
            let e = add(b, scale(a, -1.0));
            let pa = add(p, scale(a, -1.0));
            let ee = dot(e, e);
            let t = if ee > 0.0 {
                (dot(pa, e) / ee).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = length(add(pa, scale(e, -t)));
            if d < best.0 {
                let mix = |v: &[f64; 3]| v[i] + (v[i + 1] - v[i]) * t;
                best = (d, [mix(&self.levels), mix(&self.areas), mix(&self.plains)]);
            }
        }
        best
    }
}

/// The values of one Carve stamp that the channels need.
#[derive(Clone, Copy, Debug)]
pub struct ChannelCut {
    pub shape: ChannelShape,
    /// The depth of the bed of the largest channel below the floodplain, in
    /// 16-bit steps.
    pub depth: f64,
    /// The angle of one texel step of the heightmap, in radians.
    pub texel: f64,
}

/// A place beside one curve.
#[derive(Clone, Copy, Debug)]
pub struct ChannelPlace {
    /// The distance to the curve, in radians.
    pub distance: f64,
    /// The floodplain level at the nearest place of the curve.
    pub level: f64,
    /// The depth of the channel, from 0.25 to 1.
    pub size: f64,
    /// The half-width of the bed, in radians.
    pub half: f64,
    /// The distance of the outer edge of the floodplain, in radians. With
    /// no floodplain, it is `half`.
    pub edge: f64,
}

/// The level of a texel at `old` after a move toward `target` by `most` at
/// most. A texel at or below the target does not change.
fn toward(old: f64, target: f64, most: f64) -> f64 {
    if old > target {
        old - (old - target).min(most.max(0.0))
    } else {
        old
    }
}

impl ChannelCut {
    /// The half-width of the bed for a drained area. It is half a texel at
    /// least.
    fn half_width(&self, area: f64) -> f64 {
        (0.5 * self.shape.width_at(area)).max(0.5 * self.texel)
    }

    pub fn place(&self, curve: &ChannelCurve, p: V3) -> ChannelPlace {
        let (distance, [level, area, plain]) = curve.nearest(p);
        let half = self.half_width(area);
        ChannelPlace {
            distance,
            level,
            size: channel_size(area),
            half,
            edge: half + 0.5 * plain * self.shape.floodplain,
        }
    }

    /// The bed level of a curve at its nearest place to `p`.
    pub fn bed(&self, curve: &ChannelCurve, p: V3) -> f64 {
        let at = self.place(curve, p);
        at.level - self.depth * at.size
    }

    /// The level of a texel at `old` after the cut of the channel of one
    /// curve. The bed is flat out to the half-width. Past it, the target goes
    /// up to the floodplain level over one texel, and the change goes down
    /// to 0 over the same texel. `rate` is the largest change of a stamp at
    /// the texel, before the depth of the channel scales it.
    pub fn channel(&self, at: &ChannelPlace, old: f64, rate: f64) -> f64 {
        let bed = at.level - self.depth * at.size;
        let weight = self.channel_weight(at);
        toward(
            old,
            at.level - (at.level - bed) * weight,
            rate * at.size * weight,
        )
    }

    /// The weight of the channel of a curve at a place, from 0 to 1. It is
    /// 1 out to the half-width, and it goes down to 0 over one texel.
    pub fn channel_weight(&self, at: &ChannelPlace) -> f64 {
        1.0 - ((at.distance - at.half) / self.texel).clamp(0.0, 1.0)
    }

    /// The weight of the floodplain of a curve at a place, from 0 to 1. It
    /// is 1 out to the edge of the floodplain, and it goes down to 0 over
    /// one more texel. A curve with no floodplain has no weight.
    pub fn plain_weight(&self, at: &ChannelPlace) -> f64 {
        if at.edge <= at.half {
            return 0.0;
        }
        ((at.edge + self.texel - at.distance) / self.texel).clamp(0.0, 1.0)
    }

    /// The level of a texel at `old` after the cut of the floodplain of one
    /// curve.
    pub fn plain(&self, at: &ChannelPlace, old: f64, rate: f64) -> f64 {
        toward(old, at.level, rate * at.size * self.plain_weight(at))
    }

    /// How far a place is past the outer edge of the floodplain of a curve,
    /// in radians. The cut changes nothing past one texel.
    pub fn past_edge(&self, curve: &ChannelCurve, p: V3) -> f64 {
        let at = self.place(curve, p);
        at.distance - at.edge
    }
}

/// The values of one Glacial stamp that the valleys need.
#[derive(Clone, Copy, Debug)]
pub struct GlacialCut {
    pub shape: ChannelShape,
    /// The depth of the floor below the river level for the largest rivers,
    /// in 16-bit steps.
    pub strength: f64,
    /// The depth of the floor below sea level at the coast, in 16-bit steps.
    pub depth: f64,
    /// The level of the sea.
    pub sea: f64,
    /// The angle of one texel step of the heightmap, in radians.
    pub texel: f64,
}

/// The largest half-width of a glacial valley for a drained area, in
/// radians. It is the same on each size of world.
fn largest_valley(area: f64) -> f64 {
    0.5 * VALLEY_MAX_KM * (area.max(0.0) / REFERENCE_AREA_KM2).sqrt()
}

impl GlacialCut {
    /// The half-width of the valley for a drained area. It is
    /// `largest_valley` at most and half a texel at least.
    pub fn half_width(&self, area: f64) -> f64 {
        let half = 0.5 * VALLEY_WIDTH_FACTOR * self.shape.width_at(area);
        half.min(largest_valley(area)).max(0.5 * self.texel)
    }

    /// The level of the valley floor at a river level and a channel size.
    /// The floor is `strength` times the size below the river level. A floor
    /// below sea level goes to `depth` below sea level at most, where the
    /// river level is sea level.
    pub fn floor(&self, level: f64, size: f64) -> f64 {
        let drop = self.strength * size;
        let land = level - drop;
        if drop <= 0.0 {
            return level;
        }
        if land >= self.sea {
            return land;
        }
        self.sea - self.depth * ((self.sea - land) / drop).min(1.0)
    }

    /// The level of a texel at `old` after the cut of the valley of one
    /// curve. The target is the floor out to `VALLEY_FLOOR_PART` of the
    /// half-width. Past it, the target goes up with the square of the
    /// distance, to `VALLEY_WALL_RISE` times the height of the river level
    /// above the floor at the half-width. A stamp lowers the texel by
    /// `amount` times that height at most, and the change goes down to 0
    /// over one texel past the half-width.
    pub fn valley(&self, curve: &ChannelCurve, p: V3, old: f64, amount: f64) -> f64 {
        let (distance, [level, area, _]) = curve.nearest(p);
        let half = self.half_width(area);
        let floor = self.floor(level, channel_size(area));
        let s = distance / half;
        let target = if s <= VALLEY_FLOOR_PART {
            floor
        } else {
            let t = ((s - VALLEY_FLOOR_PART) / (1.0 - VALLEY_FLOOR_PART)).min(1.0);
            floor + VALLEY_WALL_RISE * (level - floor) * t * t
        };
        let weight = 1.0 - ((distance - half) / self.texel).clamp(0.0, 1.0);
        toward(old, target, amount * (level - floor) * weight)
    }
}

/// The bins of the curves.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Bins {
    /// One bin for each flow cell of each face. A face has `side` bins along
    /// one side.
    Faces { side: usize },
    /// One bin for 4 by 4 cells of a window.
    Window { window: Window },
}

/// The number of cells along one side of a bin of a window.
pub const WINDOW_BIN_CELLS: usize = 4;

/// The curves of the rivers, and the list of the curves near each bin. The
/// lists are in one array. Bin `b` has the items from `starts[b]` to
/// `starts[b + 1]`.
#[derive(Clone, Debug)]
pub struct ChannelLines {
    curves: Vec<ChannelCurve>,
    starts: Vec<u32>,
    items: Vec<u32>,
    bins: Bins,
}

/// The curves and the bins in flat arrays for the GPU.
pub struct ChannelBuffers {
    /// `CURVE_FLOATS` numbers for each curve: the 3 points, then the 3
    /// levels, the 3 areas, and the 3 plain parts. Each group of 3 numbers
    /// has a 0 after it. With no curves, the list has one curve of zeros.
    pub curves: Vec<f32>,
    /// The number of bin starts, then the first item of each bin and the end
    /// of the items, and then the curve of each item.
    pub index: Vec<u32>,
}

/// The largest reach of the cut of a curve or of its glacial valley past its
/// line, in radians, at the largest settings. It does not hold the texel
/// ramps.
fn reach(curve: &ChannelCurve, world_km: f64) -> f64 {
    let largest = ChannelShape::new(CHANNEL_WIDTHS_KM.1, FLOODPLAINS_KM.1, world_km);
    (0..3)
        .map(|i| {
            let width = 0.5 * largest.width_at(curve.areas[i]);
            let channel = width + 0.5 * curve.plains[i] * largest.floodplain;
            channel.max(largest_valley(curve.areas[i]))
        })
        .fold(0.0, f64::max)
}

/// A bin along one axis covers at least this part of the angle that the
/// same bin covers at the center of a face.
const SMALLEST_STEP: f64 = 0.62;

impl ChannelLines {
    /// No curves.
    pub fn empty() -> ChannelLines {
        ChannelLines {
            curves: Vec::new(),
            starts: vec![0],
            items: Vec::new(),
            bins: Bins::Faces { side: 0 },
        }
    }

    /// The curves in bins of one flow cell on each face. A face has `side`
    /// flow cells along one side. `world_km` is the radius of the world.
    pub fn faces(curves: Vec<ChannelCurve>, side: usize, world_km: f64) -> ChannelLines {
        if curves.is_empty() {
            return ChannelLines::empty();
        }
        let cell = FRAC_PI_2 / side as f64;
        let mut pairs = Vec::new();
        for (k, curve) in curves.iter().enumerate() {
            // The texel ramps are 2.5 texels at most, and a texel is not
            // larger than a cell. One more cell holds the texel centers.
            let cells = reach(curve, world_km) / (cell * SMALLEST_STEP) + 3.5;
            for face in 0..FACES {
                let axis = face / 2;
                let sign = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
                let project = |d: V3| {
                    let depth = d[axis] * sign;
                    let at = |a: f64| (warp(a / depth) + 1.0) * 0.5 * side as f64;
                    (depth > 0.2).then(|| (at(d[(axis + 1) % 3]), at(d[(axis + 2) % 3])))
                };
                let [Some(a), Some(b), Some(c)] = curve.points.map(project) else {
                    continue;
                };
                let range = |p: [f64; 3]| {
                    let low = (p[0].min(p[1]).min(p[2]) - cells).floor().max(0.0);
                    let high = (p[0].max(p[1]).max(p[2]) + cells).floor();
                    low as usize..(high.min(side as f64 - 1.0) + 1.0).max(low) as usize
                };
                for y in range([a.1, b.1, c.1]) {
                    for x in range([a.0, b.0, c.0]) {
                        pairs.push(((face * side + y) * side + x, k as u32));
                    }
                }
            }
        }
        ChannelLines::from_pairs(curves, pairs, FACES * side * side, Bins::Faces { side })
    }

    /// The curves in bins of `WINDOW_BIN_CELLS` by `WINDOW_BIN_CELLS` cells
    /// of a window.
    pub fn window(curves: Vec<ChannelCurve>, window: Window, world_km: f64) -> ChannelLines {
        let side = window.cells.div_ceil(WINDOW_BIN_CELLS);
        let cell = FRAC_PI_2 * window.cell as f64 / window.face_size as f64;
        let mut pairs = Vec::new();
        for (k, curve) in curves.iter().enumerate() {
            let cells = reach(curve, world_km) / (cell * SMALLEST_STEP) + 3.5;
            let [Some(a), Some(b), Some(c)] = curve.points.map(|p| window.place(p)) else {
                continue;
            };
            let range = |p: [f64; 3]| {
                let bin = |c: f64| (c / WINDOW_BIN_CELLS as f64).floor();
                let low = bin(p[0].min(p[1]).min(p[2]) - cells).max(0.0);
                let high = bin(p[0].max(p[1]).max(p[2]) + cells).min(side as f64 - 1.0);
                low as usize..(high + 1.0).max(low) as usize
            };
            for y in range([a.1, b.1, c.1]) {
                for x in range([a.0, b.0, c.0]) {
                    pairs.push((y * side + x, k as u32));
                }
            }
        }
        ChannelLines::from_pairs(curves, pairs, side * side, Bins::Window { window })
    }

    fn from_pairs(
        curves: Vec<ChannelCurve>,
        mut pairs: Vec<(usize, u32)>,
        count: usize,
        bins: Bins,
    ) -> ChannelLines {
        pairs.sort_unstable();
        pairs.dedup();
        let mut starts = vec![0u32; count + 1];
        for &(bin, _) in &pairs {
            starts[bin + 1] += 1;
        }
        for b in 0..count {
            starts[b + 1] += starts[b];
        }
        ChannelLines {
            curves,
            starts,
            items: pairs.into_iter().map(|(_, k)| k).collect(),
            bins,
        }
    }

    pub fn curves(&self) -> &[ChannelCurve] {
        &self.curves
    }

    pub fn has_curves(&self) -> bool {
        !self.curves.is_empty()
    }

    /// The bin of texel `x`, `y` of a face with `face_size` texels along one
    /// side, in lines of `faces`. Lines of a window have no face bins.
    pub fn texel_bin(&self, face: usize, x: usize, y: usize, face_size: usize) -> Option<usize> {
        match self.bins {
            Bins::Faces { side } if side > 0 => {
                Some((face * side + y * side / face_size) * side + x * side / face_size)
            }
            _ => None,
        }
    }

    /// The bin of a place of a window in cells, in lines of `window`. A
    /// place outside the window has no bin.
    pub fn window_bin(&self, u: f64, v: f64) -> Option<usize> {
        let Bins::Window { window } = self.bins else {
            return None;
        };
        let inside = |c: f64| c >= 0.0 && c < window.cells as f64;
        if !inside(u) || !inside(v) {
            return None;
        }
        let side = window.cells.div_ceil(WINDOW_BIN_CELLS);
        let bin = |c: f64| c as usize / WINDOW_BIN_CELLS;
        Some(bin(v) * side + bin(u))
    }

    /// The curves near a bin.
    pub fn near(&self, bin: usize) -> impl Iterator<Item = &ChannelCurve> + '_ {
        let (from, to) = (self.starts[bin] as usize, self.starts[bin + 1] as usize);
        self.items[from..to]
            .iter()
            .map(|&k| &self.curves[k as usize])
    }

    /// The new level of a texel at `old` in `bin` after a cut. `p` is the
    /// direction of the texel. Each channel lowers the texel toward its
    /// target by `rate` times its depth at most, and the lowest result wins.
    /// The nearest curve whose floodplain reaches the texel gives the
    /// floodplain level, so the floodplain is flat across the river.
    pub fn lowered(&self, bin: usize, p: V3, old: f64, rate: f64, cut: &ChannelCut) -> f64 {
        let mut low = old;
        let mut plain: Option<ChannelPlace> = None;
        for curve in self.near(bin) {
            let at = cut.place(curve, p);
            low = low.min(cut.channel(&at, old, rate));
            let nearer = plain.is_none_or(|q| at.distance < q.distance);
            if nearer && cut.plain_weight(&at) > 0.0 {
                plain = Some(at);
            }
        }
        match plain {
            Some(at) => low.min(cut.plain(&at, old, rate)),
            None => low,
        }
    }

    /// The new level of a texel at `old` in `bin` after a Glacial stamp with
    /// an effect of `amount`. `p` is the direction of the texel. The lowest
    /// result of the valleys wins.
    pub fn glacial(&self, bin: usize, p: V3, old: f64, amount: f64, cut: &GlacialCut) -> f64 {
        let each = self
            .near(bin)
            .map(|curve| cut.valley(curve, p, old, amount));
        each.fold(old, f64::min)
    }

    /// The drop of the erode brush at a texel in `bin` from the channels of
    /// the curves. The level of a curve of the erode brush is its drop. The
    /// largest drop of a channel times its weight wins. `cut` gives the
    /// width and the texel, and its depth has no effect.
    pub fn channel_drop(&self, bin: usize, p: V3, cut: &ChannelCut) -> f64 {
        let each = self.near(bin).map(|curve| {
            let (distance, [level, area, _]) = curve.nearest(p);
            let half = cut.half_width(area);
            let rise = ((distance - half) / cut.texel).clamp(0.0, 1.0);
            (1.0 - rise) * level
        });
        each.fold(0.0, f64::max)
    }

    /// The least distance of a place past the outer edge of the floodplain
    /// of each curve, in radians. Each curve counts, not only the curves of
    /// one bin.
    pub fn past_edge(&self, p: V3, cut: &ChannelCut) -> f64 {
        let each = self.curves.iter().map(|curve| cut.past_edge(curve, p));
        each.fold(f64::INFINITY, f64::min)
    }

    /// The curves and the bins in flat arrays for the GPU.
    pub fn to_gpu(&self) -> ChannelBuffers {
        let mut curves = Vec::with_capacity(self.curves.len().max(1) * CURVE_FLOATS);
        for curve in &self.curves {
            let groups = [curve.levels, curve.areas, curve.plains];
            for values in curve.points.into_iter().chain(groups) {
                curves.extend(values.map(|v| v as f32));
                curves.push(0.0);
            }
        }
        if curves.is_empty() {
            curves.resize(CURVE_FLOATS, 0.0);
        }
        let mut index = Vec::with_capacity(1 + self.starts.len() + self.items.len());
        index.push(self.starts.len() as u32);
        index.extend(&self.starts);
        index.extend(&self.items);
        ChannelBuffers { curves, index }
    }

    /// The number of bins along one side of a face or of the window. Lines
    /// with no curves can have 0.
    pub fn bin_side(&self) -> usize {
        match self.bins {
            Bins::Faces { side } => side,
            Bins::Window { window } => window.cells.div_ceil(WINDOW_BIN_CELLS),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{angle, lonlat_to_dir};

    const EARTH_KM: f64 = 6371.0;

    fn km2(area_km2: f64) -> f64 {
        area_km2 / (EARTH_KM * EARTH_KM)
    }

    #[test]
    fn the_width_is_the_setting_at_the_reference_area() {
        for (width, world) in [(0.6, EARTH_KM), (2.0, 1000.0), (0.05, 20000.0)] {
            let shape = ChannelShape::new(width, 10.0, world);
            let reference = REFERENCE_AREA_KM2 / (world * world);
            let km = shape.width_at(reference) * world;
            assert!((km - width).abs() < 1e-9, "{km} {width}");
        }
        let clamped = ChannelShape::new(50.0, 500.0, EARTH_KM);
        assert!((clamped.width * EARTH_KM - CHANNEL_WIDTHS_KM.1).abs() < 1e-9);
        assert!((clamped.floodplain * EARTH_KM - FLOODPLAINS_KM.1).abs() < 1e-9);
    }

    #[test]
    fn the_width_grows_with_the_square_root_of_the_area() {
        let shape = ChannelShape::default();
        let base = shape.width_at(km2(1.0e4));
        for factor in [4.0, 16.0, 100.0, 300.0] {
            let ratio = shape.width_at(km2(1.0e4 * factor)) / base;
            assert!((ratio - f64::sqrt(factor)).abs() < 1e-9, "{ratio}");
        }
        // The lower Mississippi drains about 3 million square kilometers.
        let km = shape.width_at(km2(3.0e6)) * EARTH_KM;
        assert!(km > 0.8 && km < 1.3, "{km}");
    }

    #[test]
    fn a_large_flat_river_has_a_floodplain_and_a_steep_one_has_none() {
        let large = km2(2.0e6);
        assert!((plain_factor(large, 1.0e-4, EARTH_KM) - 1.0).abs() < 1e-9);
        assert_eq!(plain_factor(large, 2.0e-3, EARTH_KM), 0.0);
        assert_eq!(plain_factor(km2(5.0e4), 1.0e-4, EARTH_KM), 0.0);
        let half = plain_factor(km2(5.5e5), 1.0e-4, EARTH_KM);
        assert!(half > 0.3 && half < 0.7, "{half}");

        // The cut of a curve with a floodplain lowers the ground beside the
        // bed to the floodplain level, out to half the floodplain width.
        let cut = ChannelCut {
            shape: ChannelShape::default(),
            depth: 500.0,
            texel: 1.0e-4,
        };
        let curve = |plain: f64| ChannelCurve {
            points: [-1.0, 0.0, 1.0].map(|lon| lonlat_to_dir(lon * 0.05, 0.0)),
            levels: [40000.0; 3],
            areas: [large; 3],
            plains: [plain; 3],
        };
        let plain_km = 0.5 * FLOODPLAIN_KM;
        let beside = lonlat_to_dir(0.0, (0.8 * plain_km / EARTH_KM).to_degrees());
        let (flat, steep) = (curve(1.0), curve(0.0));
        let old = 40300.0;
        let lowered = |curve: &ChannelCurve| {
            let at = cut.place(curve, beside);
            cut.channel(&at, old, 1000.0)
                .min(cut.plain(&at, old, 1000.0))
        };
        assert_eq!(lowered(&flat), 40000.0);
        assert_eq!(lowered(&steep), old);
    }

    #[test]
    fn the_bed_goes_down_along_each_river() {
        let cut = ChannelCut {
            shape: ChannelShape::default(),
            depth: 400.0,
            texel: 1.0e-4,
        };
        // A river that gains area and goes down to the sea.
        let curve = ChannelCurve {
            points: [0.0, 0.5, 1.0].map(|lon| lonlat_to_dir(lon, 0.0)),
            levels: [33000.0, 32900.0, 32768.0],
            areas: [6.0e-4, 2.0e-3, 4.0e-2],
            plains: [0.0; 3],
        };
        let mut last = f64::INFINITY;
        for i in 0..=50 {
            let p = lonlat_to_dir(f64::from(i) / 50.0, 0.0);
            let bed = cut.bed(&curve, p);
            assert_eq!(cut.channel(&cut.place(&curve, p), 4.0e4, 1.0e6), bed);
            assert!(bed < last, "{i} {bed} {last}");
            last = bed;
        }
        let (d, _) = curve.nearest(lonlat_to_dir(0.25, 0.01));
        assert!((d - 0.01f64.to_radians()).abs() < 1e-6, "{d}");
        assert!(angle(curve.points[0], curve.points[2]) > 0.0);
    }

    fn glacial_cut(depth: f64) -> GlacialCut {
        GlacialCut {
            shape: ChannelShape::default(),
            strength: 1000.0,
            depth,
            sea: 32768.0,
            texel: 1.0e-5,
        }
    }

    /// A straight river along the equator at a level and a drained area.
    fn straight(level: f64, area: f64) -> ChannelCurve {
        ChannelCurve {
            points: [-1.0, 0.0, 1.0].map(|lon| lonlat_to_dir(lon, 0.0)),
            levels: [level; 3],
            areas: [area; 3],
            plains: [0.0; 3],
        }
    }

    /// The level after 100 full Glacial stamps at an angle from the river,
    /// for ground at `old`.
    fn valley_at(cut: &GlacialCut, curve: &ChannelCurve, radians: f64, old: f64) -> f64 {
        let p = lonlat_to_dir(0.0, radians.to_degrees());
        (0..100).fold(old, |level, _| cut.valley(curve, p, level, 1.0))
    }

    #[test]
    fn glacial_valley_of_a_larger_river_is_wider() {
        let cut = glacial_cut(0.0);
        let (small, large) = (km2(1.0e4), km2(1.0e6));
        let ratio = cut.half_width(large) / cut.half_width(small);
        assert!((ratio - 10.0).abs() < 1e-9, "{ratio}");
        // Half of the default channel width times the factor.
        let km = cut.half_width(large) * EARTH_KM;
        assert!((km - 0.5 * VALLEY_WIDTH_FACTOR * CHANNEL_WIDTH_KM).abs() < 1e-9);
        // A place beside the large river is in its valley, and the same
        // place beside the small river is not.
        let beside = 0.5 * cut.half_width(large);
        let old = 50000.0;
        assert!(valley_at(&cut, &straight(40000.0, large), beside, old) < old);
        assert_eq!(valley_at(&cut, &straight(40000.0, small), beside, old), old);
        // A wider channel gives a wider valley, up to the largest width.
        let wide = GlacialCut {
            shape: ChannelShape::new(CHANNEL_WIDTHS_KM.1, 0.0, EARTH_KM),
            ..cut
        };
        let km = wide.half_width(large) * EARTH_KM;
        assert!((km - 0.5 * VALLEY_MAX_KM).abs() < 1e-9, "{km}");
    }

    #[test]
    fn glacial_profile_is_flat_then_steep() {
        let cut = glacial_cut(0.0);
        let curve = straight(40000.0, km2(1.0e6));
        let half = cut.half_width(km2(1.0e6));
        let old = 60000.0;
        let drop = 1000.0 * channel_size(km2(1.0e6));
        let floor = cut.floor(40000.0, channel_size(km2(1.0e6)));
        assert_eq!(floor, 40000.0 - drop);
        let profile: Vec<f64> = (0..=20)
            .map(|i| valley_at(&cut, &curve, half * f64::from(i) / 20.0, old))
            .collect();
        for (i, level) in profile.iter().enumerate() {
            if i <= 7 {
                assert_eq!(*level, floor, "{i}");
            }
        }
        // Each step of the side rises more than the step before it.
        for i in 9..20 {
            let (a, b) = (profile[i] - profile[i - 1], profile[i + 1] - profile[i]);
            assert!(b > a && a > 0.0, "{i} {profile:?}");
        }
        assert!((profile[20] - (floor + VALLEY_WALL_RISE * drop)).abs() < 1.0);
        // Past the half-width and one texel, the ground stays.
        assert_eq!(valley_at(&cut, &curve, half + 2.0 * cut.texel, old), old);
        // Ground below the target stays, and one stamp lowers the ground by
        // the height of the river above the floor at most.
        assert_eq!(valley_at(&cut, &curve, 0.0, 38000.0), 38000.0);
        assert_eq!(cut.valley(&curve, curve.points[1], old, 1.0), old - drop);
    }

    #[test]
    fn glacial_floor_goes_below_sea_level_by_the_depth_at_the_coast() {
        let sea = 32768.0;
        for depth in [0.0, 300.0] {
            let cut = glacial_cut(depth);
            // Far above the sea, the floor is the strength below the river.
            assert_eq!(cut.floor(sea + 5000.0, 1.0), sea + 4000.0);
            // At the coast, the floor is the depth below the sea.
            assert_eq!(cut.floor(sea, 1.0), sea - depth);
            assert_eq!(cut.floor(sea, 0.25), sea - depth);
            // Between them, it goes down to the sea and then to the depth.
            let mid = cut.floor(sea + 500.0, 1.0);
            assert_eq!(mid, sea - 0.5 * depth);
            assert!(mid <= sea);
        }
    }

    #[test]
    fn a_bin_lists_each_curve_that_can_reach_it() {
        let side = 32;
        let curve = ChannelCurve {
            points: [10.0, 11.0, 12.0].map(|lon| lonlat_to_dir(lon, 1.0)),
            levels: [40000.0; 3],
            areas: [km2(1.0e6); 3],
            plains: [1.0; 3],
        };
        let lines = ChannelLines::faces(vec![curve], side, EARTH_KM);
        let cut = ChannelCut {
            shape: ChannelShape::new(CHANNEL_WIDTHS_KM.1, FLOODPLAINS_KM.1, EARTH_KM),
            depth: 100.0,
            texel: FRAC_PI_2 / (4 * side) as f64,
        };
        let n = 4 * side;
        for y in 0..n {
            for x in 0..n {
                let flat = |i: usize| crate::cube::unwarp((i as f64 + 0.5) / n as f64 * 2.0 - 1.0);
                let p = crate::math::normalize(crate::cube::face_dir(0, flat(x), flat(y)));
                let bin = lines.texel_bin(0, x, y, n).unwrap();
                if cut.past_edge(&curve, p) < 1.5 * cut.texel {
                    assert_eq!(lines.near(bin).count(), 1, "{x} {y}");
                }
            }
        }
    }
}
