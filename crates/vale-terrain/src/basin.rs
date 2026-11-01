//! The basin brush.
//!
//! The brush makes a watershed from two strokes. The first stroke is the
//! outline of the basin. The second stroke is the main river, from its source
//! at the first point to its outlet, where the line leaves the outline or
//! ends.
//!
//! The network of the rivers is on the coarse cells of one window. Each cell
//! of the area drains to a neighbor that is nearer to the main river inside
//! the area, so all the water of the network drains to the outlet.
//! Stream-power erosion with uplift runs to a steady state with the implicit
//! solver of Braun and Willett (2013), and the steady state gives the relief
//! of the basin above the main river. The main river stays at or below the
//! old ground. Finer levels then add rough ground at their scale and erode
//! it, so the gullies of all sizes come from the erosion. Last, the rivers
//! are cut into the texels as thin channels.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::f64::consts::{FRAC_PI_2, SQRT_2};

use crate::channel::{
    ChannelCurve, ChannelCut, ChannelLines, ChannelShape, METERS_PER_LEVEL, channel_size,
};
use crate::erode::Erosion;
use crate::flow::{
    CoarseHeights, Grid, NONE, OUT, WINDOW_CELLS, Window, WindowHeights, level_curves,
};
use crate::heightmap::{Heightmap, Limits, TexelRect, brush_rect, noise3};
use crate::line::StrokeLine;
use crate::math::{V3, add, angle, cross, dot, normalize, scale};
use crate::rain::Rain;

/// The smallest and the largest Density setting, in kilometers.
pub const BASIN_DENSITY_KM: (f64, f64) = (10.0, 500.0);

/// The brush radius is this number of cells or more, if the texels of the
/// heightmap are small enough.
const MIN_RADIUS_CELLS: f64 = 8.0;

/// The mean distance between two tributaries is this number of cells or
/// more.
const MIN_DENSITY_CELLS: f64 = 4.0;

/// The size of the bumps of the start surface, in tributary distances.
const WAVELENGTH: f64 = 1.6;

/// The height of the bumps of the start surface, as a part of the rise
/// across the area over one wavelength.
const BUMPS: f64 = 1.0;

/// The rise along the area from the outlet to the source in the start
/// surface, as a part of the rise across the area.
const ALONG: f64 = 0.3;

/// A tributary drains this area or more, in cells of even rain, for each
/// cell of the radius and each cell of the tributary distance.
const TRIBUTARY_AREA: f64 = 0.8;

/// The number of steps of the solver, and the time of one step.
const ITERATIONS: usize = 24;
const TIME_STEP: f64 = 0.02;

/// The exponent of the drained area in the stream-power law. The exponent
/// of the slope is 1.
const AREA_EXPONENT: f64 = 0.6;

/// The depth of the bed of the largest channel, in meters.
const DEPTH_M: f64 = 10.0;

/// Half of the diagonal of a cell, in cells, with a small margin.
const HALF_DIAGONAL: f64 = 0.75;

/// A cell drains to a neighbor that is nearer to the main river by this
/// part of a cell or more.
const CLOSER: f64 = 0.25;

/// The smallest drop from a cell to its receiver, in 16-bit steps.
const STEP: f64 = 2.0;

/// The smallest drop of the main river from a cell to the next cell, in
/// 16-bit steps.
const RIVER_STEP: f64 = 0.01;

/// The part of the rise of the main river from the outlet that the hills
/// next to it keep, so the hills near the source are higher.
const HEAD: f64 = 0.5;

/// The relief of the basin fades out over this part of the brush radius at
/// the side of the area.
const FADE: f64 = 0.4;

/// The part of the relief that rises evenly from the main river to the
/// side, so the divide is high enough near the outlet too.
const ACROSS: f64 = 0.25;

/// The cells next to the main river are this part of the height above it.
const BANK: f64 = 0.02;

/// The half-width of the mouth, as a part of the brush radius.
const MOUTH: f64 = 0.3;

/// The weight of the old ground in the start surface, for a rise of the
/// old ground of the Height setting.
const GROUND: f64 = 1.0;

/// The part of the old ground above the outlet that counts toward the
/// relief of the basin.
const KEEP: f64 = 0.7;

/// The change goes to zero over this part of the brush radius at the side
/// of the area.
const EDGE: f64 = 0.1;

/// The height of the rough ground of the first finer level, as a part of
/// the height of the basin, and the part that each next level keeps.
const ROUGH: f64 = 0.03;
const ROUGH_FALLOFF: f64 = 0.55;

/// The finest level has this number of cells along one side or fewer.
const FINE_CELLS: usize = 1024;

/// The number of erosion steps of each finer level, the erodibility, and the
/// largest cut of one step, as a part of the rough ground of the level.
const FINE_STEPS: usize = 4;
const ERODE: f64 = 1.0;
const CARVE: f64 = 3.0;

/// The part of the difference to the mean of its neighbors that a cell
/// moves by hillslope creep.
const CREEP: f64 = 0.2;

const SALT_BASIN: u32 = 0x5b2e_93c7;

/// The settings of a basin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BasinSpec {
    /// The height of the highest ground inside the divide above the main
    /// river, in 16-bit steps.
    pub height: f64,
    /// The mean distance between two tributaries along the main river, as an
    /// angle, in radians.
    pub density: f64,
    /// The seed of the network.
    pub seed: u32,
    pub rain: Rain,
    pub channel: ChannelShape,
    /// The radius of the world, in kilometers.
    pub world_km: f64,
    pub limits: Limits,
}

/// The outline and the main river of one basin.
#[derive(Clone, Debug)]
pub struct BasinLine {
    spec: BasinSpec,
    /// The outline, closed from the last point to the first.
    outline: Vec<V3>,
    /// The half-width of the basin, in radians.
    width: f64,
    line: StrokeLine,
}

impl BasinLine {
    /// Starts a main river with no points in an outline, on a heightmap
    /// with `face_size` texels along one side of a face. The settings go
    /// inside their limits.
    pub fn new(spec: BasinSpec, outline: &[V3], face_size: usize) -> BasinLine {
        let spec = BasinSpec {
            height: spec.height.clamp(0.0, 65535.0),
            ..spec
        };
        let outline: Vec<V3> = outline.iter().map(|&p| normalize(p)).collect();
        let width = outline_width(&outline);
        BasinLine {
            spec,
            outline,
            width,
            line: StrokeLine::new(width.max(1e-9), face_size),
        }
    }

    /// True if the outline holds an area with a half-width of four texels or
    /// more.
    pub fn has_outline(&self) -> bool {
        let texel = FRAC_PI_2 / self.line.face_size() as f64;
        self.outline.len() >= 3 && self.width >= 4.0 * texel
    }

    /// Adds a point to the main river. A point near the last point is not
    /// added.
    pub fn push(&mut self, dir: V3) {
        self.line.push(dir, 1.0);
    }

    /// The number of points of the main river.
    pub fn len(&self) -> usize {
        self.line.len()
    }

    pub fn is_empty(&self) -> bool {
        self.line.is_empty()
    }

    /// The length of the main river, in radians.
    pub fn length(&self) -> f64 {
        self.line.length()
    }

    /// The settings, inside their limits.
    pub fn spec(&self) -> BasinSpec {
        self.spec
    }

    /// The direction of each point of the main river.
    pub fn points(&self) -> Vec<V3> {
        self.line.points().collect()
    }

    /// The direction of each point of the outline.
    pub fn outline(&self) -> &[V3] {
        &self.outline
    }
}

/// The half-width of an outline: its area over its length, in radians.
fn outline_width(outline: &[V3]) -> f64 {
    if outline.len() < 3 {
        return 0.0;
    }
    let center = normalize(outline.iter().fold([0.0; 3], |sum, &p| add(sum, p)));
    // The area on the plane that touches the sphere at the center. The lines
    // of the outline are straight on that plane.
    let east = normalize(cross(
        if center[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        },
        center,
    ));
    let north = cross(center, east);
    let flat = |p: V3| {
        let depth = dot(p, center).max(1e-6);
        (dot(p, east) / depth, dot(p, north) / depth)
    };
    let count = outline.len();
    let (mut area, mut length) = (0.0, 0.0);
    for k in 0..count {
        let (a, b) = (outline[k], outline[(k + 1) % count]);
        let ((ax, ay), (bx, by)) = (flat(a), flat(b));
        area += 0.5 * (ax * by - bx * ay);
        length += angle(a, b);
    }
    if length > 0.0 {
        area.abs() / length
    } else {
        0.0
    }
}

/// The new texels of a basin: a face, a rectangle, and its texels row by
/// row. A texel outside the area keeps its level.
#[derive(Clone, Default)]
pub struct BasinCut {
    pub rects: Vec<(usize, TexelRect, Vec<u16>)>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) net: Option<BasinNet>,
}

/// The cells of a basin after the erosion.
#[derive(Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct BasinNet {
    pub(crate) window: Window,
    /// The cells of the area.
    pub(crate) inside: Vec<bool>,
    /// The level of each cell of the area.
    pub(crate) levels: Vec<f64>,
    pub(crate) receiver: Vec<u32>,
    /// The water that drains through each cell, in steradians of even rain.
    pub(crate) area: Vec<f32>,
    /// The cells of the main river, from the source to the outlet.
    pub(crate) chain: Vec<usize>,
    /// The smallest water of a tributary, in steradians of even rain.
    pub(crate) min_area: f64,
    /// The main river inside the outline.
    pub(crate) river: StrokeLine,
}

/// The receivers of the cells of the area, and the cells in an order where
/// each receiver is before the cells that it takes water from.
struct Flow {
    receiver: Vec<u32>,
    stack: Vec<usize>,
}

/// Makes the basin of an outline and a main river on the ground of `map`. A
/// main river with less than two points in the outline makes no basin.
pub fn basin(map: &Heightmap, line: &BasinLine) -> BasinCut {
    if line.len() < 2 || !line.has_outline() {
        return BasinCut::default();
    }
    let spec = line.spec;
    let n = map.face_size();
    // The half-width of the basin.
    let radius = line.width;
    let window = basin_window(&line.outline, radius, n);
    let side = window.cells;
    let count = side * side;
    let cell = FRAC_PI_2 * window.cell as f64 / n as f64;
    let outline = OutlineGrid::new(&line.outline, &window);
    let Some(river) = outline.clip(&line.line, &window) else {
        return BasinCut::default();
    };
    let chain = chain_cells(&river, &window);
    if chain.len() < 2 {
        return BasinCut::default();
    }
    let old = WindowHeights::new(map, window);
    let dirs: Vec<V3> = (0..count)
        .map(|i| window.cell_dir(i % side, i / side))
        .collect();
    let places: Vec<(f64, f64)> = dirs.iter().map(|&p| river.nearest(p)).collect();
    // The distance of each cell to the outline. It is less than zero outside
    // the outline. A cell of the area is inside the outline with all its
    // texels, so its whole cell changes.
    let edge: Vec<f64> = outline.distances().iter().map(|&e| e * cell).collect();
    let mut inside: Vec<bool> = edge.iter().map(|&e| e >= HALF_DIAGONAL * cell).collect();
    let mut on_chain = vec![false; count];
    for &c in &chain {
        inside[c] = true;
        on_chain[c] = true;
    }
    // The distance to the main river inside the area. A part of the area
    // that the main river does not reach is not in the area.
    let reach = river_distances(&window, &chain, &inside, cell);
    for i in 0..count {
        inside[i] &= reach[i].is_finite();
    }
    let length = river.length().max(cell);
    let rim_of = |inside: &[bool]| -> Vec<bool> {
        (0..count)
            .map(|i| {
                inside[i]
                    && !on_chain[i]
                    && window
                        .neighbors(i)
                        .iter()
                        .any(|&j| j == NONE || !inside[j as usize])
            })
            .collect()
    };
    let mut rim = rim_of(&inside);
    // From 0 at the main river to 1 at the outline.
    let across = |i: usize| {
        if on_chain[i] {
            0.0
        } else {
            reach[i] / (reach[i] + edge[i].max(0.0)).max(cell)
        }
    };

    // The water of each cell, in cells of even rain.
    let rain = if spec.rain.is_even() {
        vec![1.0; count]
    } else {
        let mut erosion = Erosion::window_alone(&old);
        erosion.set_rain(spec.rain);
        erosion.cell_rain()
    };
    let solid: Vec<f64> = (0..count).map(|i| window.solid_angle(i)).collect();
    let mean_solid = solid.iter().sum::<f64>() / count as f64;
    let water: Vec<f64> = (0..count)
        .map(|i| solid[i] / mean_solid * f64::from(rain[i]))
        .collect();

    // The start surface rises across the area and along it, with bumps of
    // the size of the tributary distance. Its valleys start the network.
    let density = spec.density.clamp(
        MIN_DENSITY_CELLS * cell,
        radius.max(MIN_DENSITY_CELLS * cell),
    );
    let wavelength = WAVELENGTH * density;
    let seed = spec.seed ^ SALT_BASIN;
    let old_outlet = f64::from(old.levels[*chain.last().expect("the chain has cells")]);
    let relief = spec.height.max(1.0);
    let mut h: Vec<f64> = (0..count)
        .map(|i| {
            // Smaller octaves bend the streams.
            let octave = |k: u32| {
                let size = f64::from(1u32 << k);
                noise3(scale(dirs[i], size / wavelength), seed.wrapping_add(k))[0] / size
            };
            let bump = octave(0) + octave(1) + octave(2);
            // The old ground steers the streams, so they follow its valleys.
            let ground = (f64::from(old.levels[i]) - old_outlet) / relief;
            across(i)
                + ALONG * (1.0 - places[i].1 / length)
                + BUMPS * wavelength / radius * bump
                + GROUND * ground
        })
        .collect();

    // A cell drains to a neighbor that is nearer to the main river by a part
    // of a cell. A cell with no such neighbor is not in the area.
    let order_of = |inside: &[bool]| {
        let mut order: Vec<usize> = (0..count).filter(|&i| inside[i] && !on_chain[i]).collect();
        order.sort_by(|&a, &b| reach[a].total_cmp(&reach[b]));
        order
    };
    let mut order = order_of(&inside);
    let mut flow = loop {
        let can_take = |i: usize, j: usize| {
            on_chain[j] || (inside[j] && !rim[j] && reach[j] < reach[i] - CLOSER * cell)
        };
        let flow = route(&window, &chain, &order, can_take, &h);
        if flow.stack.len() == chain.len() + order.len() {
            break flow;
        }
        let mut reached = vec![false; count];
        for &i in &flow.stack {
            reached[i] = true;
        }
        inside = reached;
        rim = rim_of(&inside);
        order = order_of(&inside);
    };
    let can_take = |i: usize, j: usize| {
        on_chain[j] || (inside[j] && !rim[j] && reach[j] < reach[i] - CLOSER * cell)
    };
    let length_to = |i: usize, r: usize| angle(dirs[i], dirs[r]) / cell;

    // Braun and Willett (2013): an implicit step of the stream-power law
    // with uplift, in the order of the stack. With an uplift of 1 and an
    // erodibility of 1, the steady state of a cell is the level of its
    // receiver plus its length over its water to the power of the area
    // exponent.
    let mut area = vec![0.0; count];
    for step in 0..=ITERATIONS {
        if step > 0 {
            let old_receivers = std::mem::take(&mut flow.receiver);
            flow = route(&window, &chain, &order, can_take, &h);
            if flow.receiver == old_receivers {
                break;
            }
        }
        area.iter_mut().for_each(|a| *a = 0.0);
        for &i in &flow.stack {
            area[i] = water[i];
        }
        for &i in flow.stack.iter().rev() {
            let r = flow.receiver[i];
            if r != OUT {
                area[r as usize] += area[i];
            }
        }
        for &i in &flow.stack {
            let r = flow.receiver[i];
            if r == OUT {
                h[i] = 0.0;
                continue;
            }
            let r = r as usize;
            let f = TIME_STEP * area[i].powf(AREA_EXPONENT) / length_to(i, r);
            h[i] = (h[i] + TIME_STEP + f * h[r]) / (1.0 + f);
        }
    }
    // The steady state of the last receivers, and the cell of the main river
    // where the water of each cell joins it.
    let mut join = vec![usize::MAX; count];
    for &i in &flow.stack {
        let r = flow.receiver[i];
        if r == OUT {
            h[i] = 0.0;
        } else {
            h[i] = h[r as usize] + length_to(i, r as usize) / area[i].powf(AREA_EXPONENT);
        }
        join[i] = if on_chain[i] { i } else { join[r as usize] };
    }

    // Past the outlet, the relief opens in a gap along the main river, the
    // mouth of the basin.
    let gap = |p: V3| {
        let (past, side) = river.past_end(p);
        let mouth = MOUTH * radius;
        1.0 - (1.0 - smoothstep(side / mouth)) * smoothstep(past / (0.25 * mouth).max(2.0 * cell))
    };
    // The relief fades out near the outline.
    let fade = |e: f64| smoothstep(e / (FADE * radius));
    // The inner basin is inside the divide, out of the mouth. Its water goes
    // to the main river. The water of the rest of the area can leave the
    // area.
    let in_basin = |p: V3, e: f64| {
        let (past, side) = river.past_end(p);
        e > FADE * radius && !(past > 0.0 && side < MOUTH * radius)
    };
    let old_levels: Vec<f64> = old.levels.iter().map(|&l| f64::from(l)).collect();

    // The main river follows a concave profile, and it stays at or below the
    // lowest old ground upstream of it. Its bed is the depth of the channel
    // below its level.
    let depth = DEPTH_M / METERS_PER_LEVEL;
    let lift = |c: usize| depth * channel_size(area[c] * mean_solid);
    let lowest = |levels: &[f64], window: &Window, c: usize| {
        let near = window.neighbors(c);
        let around = near
            .iter()
            .filter(|&&j| j != NONE)
            .map(|&j| levels[j as usize]);
        around.fold(levels[c], f64::min)
    };
    let mut low = f64::MAX;
    let floor: Vec<f64> = chain
        .iter()
        .map(|&c| {
            low = low.min(lowest(&old_levels, &window, c));
            low
        })
        .collect();
    let (first, last) = (floor[0], floor[floor.len() - 1]);
    let source = h[chain[0]].max(f64::MIN_POSITIVE);
    let mut river_levels: Vec<f64> = chain
        .iter()
        .zip(&floor)
        .map(|(&c, &f)| f.min(last + (first - last) * h[c] / source) + lift(c))
        .collect();
    for k in (0..chain.len() - 1).rev() {
        river_levels[k] = river_levels[k].max(river_levels[k + 1] + RIVER_STEP);
    }
    let mut chain_index = vec![usize::MAX; count];
    for (k, &c) in chain.iter().enumerate() {
        chain_index[c] = k;
    }

    // The relief rises from the main river to the divides, and the hills
    // near the source are higher. It goes onto the old ground.
    let above = |i: usize| match join[i] {
        usize::MAX => 0.0,
        j => (h[i] - (1.0 - HEAD) * h[j]).max(0.0),
    };
    let top = (0..count)
        .filter(|&i| inside[i] && !rim[i])
        .map(above)
        .fold(f64::MIN_POSITIVE, f64::max);
    let mut levels: Vec<f64> = (0..count)
        .map(|i| {
            if !inside[i] {
                return old_levels[i];
            }
            if on_chain[i] {
                return river_levels[chain_index[i]];
            }
            let shape = (1.0 - ACROSS) * above(i) / top + ACROSS * across(i);
            let target = spec.height * shape;
            // Where the old ground is already high above the main river, the
            // basin adds less.
            let base = match join[i] {
                usize::MAX => old_levels[i],
                j => river_levels[chain_index[j]],
            };
            let already = (old_levels[i] - base).clamp(0.0, target);
            old_levels[i] + (target - KEEP * already) * fade(edge[i]) * gap(dirs[i])
        })
        .collect();
    // Each cell is above its receiver, so the old ground has no hollows in
    // the basin. A stream cuts a gap through a barrier below it, and what is
    // left fills.
    for &i in flow.stack.iter().rev() {
        let r = flow.receiver[i];
        if r != OUT && !on_chain[r as usize] {
            levels[r as usize] = levels[r as usize].min(levels[i] - STEP);
        }
    }
    let inner: Vec<bool> = (0..count)
        .map(|i| on_chain[i] || (inside[i] && in_basin(dirs[i], edge[i])))
        .collect();
    // Each cell next to the main river is above the river cells next to it.
    // Thus the main river stays on the line.
    let bank = STEP + BANK * spec.height;
    for i in (0..count).filter(|&i| inner[i] && !on_chain[i]) {
        let near = window.neighbors(i);
        let river = near
            .iter()
            .filter(|&&j| j != NONE && on_chain[j as usize])
            .map(|&j| levels[j as usize] + bank)
            .fold(f64::MIN, f64::max);
        levels[i] = levels[i].max(river);
    }
    for &i in &flow.stack {
        let r = flow.receiver[i];
        if r != OUT && !on_chain[i] {
            levels[i] = levels[i].max(levels[r as usize] + STEP);
        }
    }
    let min_area = TRIBUTARY_AREA * (density / cell) * (radius / cell) * mean_solid;

    // Finer levels add rough ground at their scale and erode it.
    let mut furthest = f64::MIN;
    let lifts = chain
        .iter()
        .map(|&c| {
            furthest = furthest.max(places[c].1);
            (furthest, lift(c))
        })
        .collect();
    let mut grid = Level {
        window,
        inside: inside.clone(),
        chain: chain.clone(),
        distance: places.iter().map(|p| p.0).collect(),
        along: places.iter().map(|p| p.1).collect(),
        edge: edge.clone(),
        rain: rain.iter().map(|&r| f64::from(r)).collect(),
        old: old_levels,
        levels: levels.clone(),
        lifts,
    };
    let mut rough = ROUGH * spec.height;
    let levels = grid.levels.clone();
    while grid.window.cell > 1 && 2 * grid.window.cells <= FINE_CELLS {
        let rough_at = |p: V3, e: f64, size: f64| rough * fractal(p, size, seed) * fade(e) * gap(p);
        grid = grid.finer(map, &river, rough_at);
        grid.erode(rough);
        rough *= ROUGH_FALLOFF;
    }

    // The channels of the main river and the tributaries, on the finest
    // level.
    let fine = grid.flow();
    let fine_cell = FRAC_PI_2 * grid.window.cell as f64 / n as f64;
    let fine_solid = mean_solid * (fine_cell / cell).powi(2);
    let mut curve_area: Vec<f32> = fine.area.iter().map(|&a| (a * fine_solid) as f32).collect();
    // The main river follows the drawn line, not the cells. Its level and
    // its water come from the cells of the main river at the same place
    // along the line.
    let profile = grid.profile();
    let water: Vec<(f64, f64)> = profile
        .iter()
        .zip(&grid.chain)
        .map(|(&(u, _), &c)| (u, (fine.area[c] * fine_solid).max(2.0 * min_area)))
        .collect();
    for &c in &grid.chain {
        curve_area[c] = 0.0;
    }
    let mut curves = level_curves(
        &grid.window,
        &fine.receiver,
        &curve_area,
        min_area,
        &grid.levels,
    );
    let points: Vec<V3> = river.points().collect();
    let mut drawn = vec![(points[0], 0.0)];
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let steps = (angle(a, b) / fine_cell).ceil().max(1.0) as usize;
        let start = drawn.last().map_or(0.0, |d| d.1);
        for k in 1..=steps {
            let t = k as f64 / steps as f64;
            let p = normalize(add(a, scale(add(b, scale(a, -1.0)), t)));
            drawn.push((p, start + angle(a, b) * t));
        }
    }
    for pair in drawn.windows(2) {
        let ((a, ua), (b, ub)) = (pair[0], pair[1]);
        let um = 0.5 * (ua + ub);
        let at = |u: f64| [profile_at(&profile, u), profile_at(&water, u)];
        let [[la, wa], [lm, wm], [lb, wb]] = [at(ua), at(um), at(ub)];
        curves.push(ChannelCurve {
            points: [a, normalize(add(a, b)), b],
            levels: [la, lm, lb],
            areas: [wa, wm, wb],
            plains: [0.0; 3],
        });
    }
    let lines = ChannelLines::window(curves, grid.window, spec.world_km);
    let cut = ChannelCut {
        shape: spec.channel,
        depth,
        texel: FRAC_PI_2 / n as f64,
    };

    // The texels take the change of the finest level, and rough ground below
    // its cell size.
    let fine_side = grid.window.cells;
    let change: Vec<f64> = (0..fine_side * fine_side)
        .map(|i| {
            if grid.inside[i] {
                grid.levels[i] - grid.old[i]
            } else {
                0.0
            }
        })
        .collect();
    let texel_size = FRAC_PI_2 / n as f64;
    let edge_width = EDGE * radius;
    let texel = |p: V3, old: f64| -> Option<f64> {
        let (u, v) = grid.window.place(p)?;
        let inside_window = |c: f64| c >= 0.0 && c < fine_side as f64;
        if !inside_window(u) || !inside_window(v) {
            return None;
        }
        // Near the outline, the distance is exact.
        let mut e = bilinear(&grid.edge, fine_side, u, v);
        if e.abs() < 2.0 * cell {
            let (uc, vc) = window.place(p)?;
            if let Some(near) = outline.near(uc, vc) {
                e = near.copysign(e).min(window_border(side, uc, vc)) * cell;
            }
        }
        if e <= 0.0 {
            return None;
        }
        let d = bilinear(&grid.distance, fine_side, u, v);
        let mut new = old + bilinear(&change, fine_side, u, v);
        if fine_cell > 1.5 * texel_size {
            // The detail fades out at the main river.
            let calm = smoothstep(d / (3.0 * fine_cell));
            new += 0.5 * rough * fractal(p, fine_cell, seed) * fade(e) * gap(p) * calm;
        }
        if let Some(bin) = lines.window_bin(u, v) {
            let surface = new;
            for curve in lines.near(bin) {
                let at = cut.place(curve, p);
                let weight = cut.channel_weight(&at);
                if weight > 0.0 {
                    let bed = at.level - depth * at.size;
                    new = new.min(surface + weight * (bed - surface));
                }
            }
        }
        // The change goes to zero at the outline.
        Some(old + (new - old) * smoothstep(e / edge_width))
    };

    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut rects = Vec::new();
    for (face, rect) in basin_rects(&window, &inside, n) {
        let mut data = Vec::with_capacity((rect.x1 - rect.x0) * (rect.y1 - rect.y0));
        map.read_rect(face, rect, &mut data);
        let width = rect.x1 - rect.x0;
        let rows = (rect.y1 - rect.y0).div_ceil(threads).max(1);
        std::thread::scope(|scope| {
            for (part, chunk) in data.chunks_mut(rows * width).enumerate() {
                let texel = &texel;
                scope.spawn(move || {
                    for (k, level) in chunk.iter_mut().enumerate() {
                        let (x, y) = (rect.x0 + k % width, rect.y0 + part * rows + k / width);
                        let old = f64::from(*level);
                        if let Some(new) = texel(map.texel_dir(face, x, y), old) {
                            let new = spec.limits.apply(old, new);
                            *level = new.round().clamp(0.0, 65535.0) as u16;
                        }
                    }
                });
            }
        });
        rects.push((face, rect, data));
    }
    let mut net_area: Vec<f32> = area.iter().map(|&a| (a * mean_solid) as f32).collect();
    for &c in &chain {
        net_area[c] = net_area[c].max(min_area as f32);
    }
    BasinCut {
        rects,
        net: Some(BasinNet {
            window,
            inside,
            levels,
            receiver: flow.receiver,
            area: net_area,
            chain,
            min_area,
            river,
        }),
    }
}

/// The window of the cells of a basin. The cell is the smallest that holds
/// the outline in the window, with `MIN_RADIUS_CELLS` cells in the
/// half-width of the basin or more, and it is not larger than a cell of the
/// coarse copy. A larger area is cut at the sides of the window.
fn basin_window(outline: &[V3], radius: f64, n: usize) -> Window {
    let center = normalize(outline.iter().fold([0.0; 3], |sum, &p| add(sum, p)));
    let global = n / CoarseHeights::size_for(n);
    let mut cell = 1;
    loop {
        let cells = WINDOW_CELLS.min(n / cell);
        let mut window = Window::centered(center, n, cell, cells);
        // The window ends less than half a face past the face edge.
        let step = cell as i64;
        let past = (n as i64 / 2 - 1) / step * step;
        let last = n as i64 + past - (cells * cell) as i64;
        window.x0 = window.x0.clamp(-past, last);
        window.y0 = window.y0.clamp(-past, last);
        let angle = FRAC_PI_2 * cell as f64 / n as f64;
        let margin = 2.0;
        let room = |c: f64| c >= margin && c <= cells as f64 - margin;
        let fits = outline
            .iter()
            .all(|&p| window.place(p).is_some_and(|(u, v)| room(u) && room(v)));
        let coarser = 2 * cell <= global && radius / (2.0 * angle) >= MIN_RADIUS_CELLS;
        if fits || !coarser {
            return window;
        }
        cell *= 2;
    }
}

/// The cells of the main river, from the source to the outlet. Two cells
/// next to each other in the list are neighbors, and no cell is a neighbor
/// of a later cell other than the next one. Thus a loop of the line is cut
/// out.
fn chain_cells(line: &StrokeLine, window: &Window) -> Vec<usize> {
    let side = window.cells;
    let points: Vec<V3> = line.points().collect();
    let cell = FRAC_PI_2 * window.cell as f64 / window.face_size as f64;
    let mut cells: Vec<usize> = Vec::new();
    let mut add_cell = |p: V3| {
        if let Some((x, y)) = window.cell_at(p) {
            let i = y * side + x;
            if cells.last() != Some(&i) {
                cells.push(i);
            }
        }
    };
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let steps = (angle(a, b) / (0.25 * cell)).ceil().max(1.0) as usize;
        for s in 0..steps {
            let t = s as f64 / steps as f64;
            add_cell(normalize(add(a, scale(add(b, scale(a, -1.0)), t))));
        }
    }
    add_cell(*points.last().expect("the line has points"));
    let touch = |a: usize, b: usize| {
        let (ax, ay, bx, by) = (a % side, a / side, b % side, b / side);
        ax.abs_diff(bx) <= 1 && ay.abs_diff(by) <= 1
    };
    let mut chain = vec![cells[0]];
    let mut i = 0;
    while i + 1 < cells.len() {
        let far = (i + 1..cells.len())
            .rev()
            .find(|&k| touch(cells[i], cells[k]));
        let k = far.unwrap_or(i + 1);
        if cells[k] != cells[i] {
            chain.push(cells[k]);
        }
        i = k;
    }
    chain
}

/// The flow on the cells of the area. The cells of the main river have
/// fixed receivers. `order` holds the other cells of the area, nearest to the
/// main river first. Each of them drains to its steepest neighbor that has a
/// receiver and that `can_take(cell, neighbor)` allows. A cell with no such
/// neighbor has no receiver.
fn route(
    window: &Window,
    chain: &[usize],
    order: &[usize],
    can_take: impl Fn(usize, usize) -> bool,
    h: &[f64],
) -> Flow {
    let mut receiver = vec![NONE; h.len()];
    let mut stack: Vec<usize> = chain.iter().rev().copied().collect();
    for (k, &c) in chain.iter().enumerate() {
        receiver[c] = chain.get(k + 1).map_or(OUT, |&next| next as u32);
    }
    for &i in order {
        let mut best = (f64::MIN, NONE);
        for (k, &j) in window.neighbors(i).iter().enumerate() {
            if j == NONE || receiver[j as usize] == NONE || !can_take(i, j as usize) {
                continue;
            }
            let run = if k < 4 { 1.0 } else { SQRT_2 };
            let score = (h[i] - h[j as usize]) / run + 1e-9 * window.scatter(i, k);
            if score > best.0 {
                best = (score, j);
            }
        }
        if best.1 != NONE {
            receiver[i] = best.1;
            stack.push(i);
        }
    }
    Flow { receiver, stack }
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A fractal noise of a direction, from about -1 to 1, with features of
/// `size` radians and two smaller octaves.
fn fractal(p: V3, size: f64, seed: u32) -> f64 {
    (0..3)
        .map(|k| {
            let scale_k = f64::from(1u32 << k);
            let salt = seed.wrapping_add(0x9e37 * (k + 7));
            noise3(scale(p, scale_k / size), salt)[0] / scale_k
        })
        .sum::<f64>()
        / 1.75
}

/// The cells of one level of a basin.
struct Level {
    window: Window,
    inside: Vec<bool>,
    /// The cells of the main river, from the source to the outlet.
    chain: Vec<usize>,
    /// The distance of each cell to the main river, and the distance along
    /// the main river of the nearest place, in radians.
    distance: Vec<f64>,
    along: Vec<f64>,
    /// The distance of each cell to the outline, in radians. It is less than
    /// zero outside the outline.
    edge: Vec<f64>,
    rain: Vec<f64>,
    /// The level of each cell before the basin.
    old: Vec<f64>,
    levels: Vec<f64>,
    /// The height of the level of the main river above its bed at each
    /// distance along the main river.
    lifts: Vec<(f64, f64)>,
}

/// The flow on the cells of a grid. A receiver can be a cell outside the
/// area.
struct GridFlow {
    receiver: Vec<u32>,
    /// The cells of the area, each receiver before its donors.
    stack: Vec<usize>,
    /// The level of each cell after the flood fills the pits.
    filled: Vec<f64>,
    /// The water of each cell, in cells of even rain.
    area: Vec<f64>,
}

impl Level {
    /// The grid with cells of half the size. It takes the change of this
    /// grid, and `rough` gives the rough ground of a direction, a distance to
    /// the outline, and a size.
    fn finer(
        &self,
        map: &Heightmap,
        river: &StrokeLine,
        rough: impl Fn(V3, f64, f64) -> f64,
    ) -> Level {
        let window = Window {
            cell: self.window.cell / 2,
            cells: 2 * self.window.cells,
            ..self.window
        };
        let (side, coarse) = (window.cells, self.window.cells);
        let up = |field: &[f64]| -> Vec<f64> {
            (0..side * side)
                .map(|i| {
                    let (u, v) = (
                        ((i % side) as f64 + 0.5) / 2.0,
                        ((i / side) as f64 + 0.5) / 2.0,
                    );
                    bilinear(field, coarse, u, v)
                })
                .collect()
        };
        let change: Vec<f64> = (0..coarse * coarse)
            .map(|i| {
                if self.inside[i] {
                    self.levels[i] - self.old[i]
                } else {
                    0.0
                }
            })
            .collect();
        let cell = FRAC_PI_2 * window.cell as f64 / window.face_size as f64;
        let distance = up(&self.distance);
        let along = up(&self.along);
        let edge = up(&self.edge);
        let chain = chain_cells(river, &window);
        let mut inside: Vec<bool> = edge.iter().map(|&e| e >= HALF_DIAGONAL * cell).collect();
        for &c in &chain {
            inside[c] = true;
        }
        let old: Vec<f64> = WindowHeights::new(map, window)
            .levels
            .iter()
            .map(|&l| f64::from(l))
            .collect();
        let change = up(&change);
        let levels = (0..side * side)
            .map(|i| {
                if inside[i] {
                    let p = window.cell_dir(i % side, i / side);
                    old[i] + change[i] + rough(p, edge[i], 2.0 * cell)
                } else {
                    old[i]
                }
            })
            .collect();
        // The main river keeps the profile of the coarser level, and it
        // stays at or below the lowest old ground upstream of it.
        let profile = self.profile();
        let mut levels: Vec<f64> = levels;
        let mut low = f64::MAX;
        for &c in &chain {
            let near = window.neighbors(c);
            let around = near
                .iter()
                .filter(|&&j| j != NONE)
                .map(|&j| old[j as usize]);
            low = low.min(around.fold(old[c], f64::min));
            let lift = profile_at(&self.lifts, along[c]);
            levels[c] = profile_at(&profile, along[c]).min(low + lift);
        }
        for k in (0..chain.len().saturating_sub(1)).rev() {
            levels[chain[k]] = levels[chain[k]].max(levels[chain[k + 1]] + RIVER_STEP);
        }
        Level {
            window,
            inside,
            chain,
            distance,
            along,
            edge,
            rain: up(&self.rain),
            old,
            levels,
            lifts: self.lifts.clone(),
        }
    }

    /// The flow of the water. The main river has fixed receivers, and the
    /// water of the other cells goes to the main river or out of the area,
    /// after a flood fills the pits.
    fn flow(&self) -> GridFlow {
        let side = self.window.cells;
        let count = side * side;
        let mut receiver = vec![NONE; count];
        let mut done = vec![false; count];
        let mut queued = vec![false; count];
        let mut filled = self.levels.clone();
        let mut heap = BinaryHeap::new();
        let mut pushes = 0u32;
        let mut stack: Vec<usize> = self.chain.iter().rev().copied().collect();
        for (k, &c) in self.chain.iter().enumerate() {
            receiver[c] = self.chain.get(k + 1).map_or(OUT, |&next| next as u32);
        }
        // The cells outside the area next to it, and the main river, are
        // where the flood starts.
        for i in 0..count {
            let edge = !self.inside[i]
                && self
                    .window
                    .neighbors(i)
                    .iter()
                    .any(|&j| j != NONE && self.inside[j as usize]);
            if edge || receiver[i] != NONE {
                done[i] = true;
                queued[i] = true;
                heap.push(Low(self.levels[i], pushes, i));
                pushes += 1;
            }
        }
        while let Some(Low(level, _, i)) = heap.pop() {
            if !done[i] {
                let mut best = (f64::MIN, NONE);
                for (k, &j) in self.window.neighbors(i).iter().enumerate() {
                    if j == NONE || !done[j as usize] {
                        continue;
                    }
                    let run = if k < 4 { 1.0 } else { SQRT_2 };
                    // The scatter turns a stream a little to one side, so
                    // the streams do not follow the grid.
                    let slope = (level - filled[j as usize]) / run;
                    let score = slope * (0.7 + 0.6 * self.window.scatter(i, k))
                        + 1e-9 * self.window.scatter(i, k);
                    if score > best.0 {
                        best = (score, j);
                    }
                }
                receiver[i] = best.1;
                done[i] = true;
                stack.push(i);
            }
            for &j in &self.window.neighbors(i) {
                if j != NONE && self.inside[j as usize] && !queued[j as usize] {
                    let j = j as usize;
                    queued[j] = true;
                    filled[j] = self.levels[j].max(level);
                    heap.push(Low(filled[j], pushes, j));
                    pushes += 1;
                }
            }
        }
        let mut area = vec![0.0; count];
        let solid = |i: usize| self.window.solid_angle(i);
        let mean = solid(side / 2 * side + side / 2);
        for &i in &stack {
            area[i] = solid(i) / mean * self.rain[i];
        }
        for &i in stack.iter().rev() {
            let r = receiver[i];
            if r < OUT && self.inside[r as usize] {
                area[r as usize] += area[i];
            }
        }
        GridFlow {
            receiver,
            stack,
            filled,
            area,
        }
    }

    /// The level of the main river at each distance along the line, in the
    /// order of the line.
    fn profile(&self) -> Vec<(f64, f64)> {
        let mut furthest = f64::MIN;
        self.chain
            .iter()
            .map(|&c| {
                furthest = furthest.max(self.along[c]);
                (furthest, self.levels[c])
            })
            .collect()
    }

    /// Erodes the cells with a few implicit steps of the stream-power law.
    /// The steps cut a cell by `rough` times `CARVE` at most, so the level
    /// adds valleys at its own scale. The main river keeps its levels.
    fn erode(&mut self, rough: f64) {
        let side = self.window.cells;
        let cell = FRAC_PI_2 * self.window.cell as f64 / self.window.face_size as f64;
        let mut on_chain = vec![false; side * side];
        for &c in &self.chain {
            on_chain[c] = true;
        }
        for _ in 0..FINE_STEPS {
            let flow = self.flow();
            for &i in &flow.stack {
                if on_chain[i] {
                    continue;
                }
                let r = flow.receiver[i];
                if r >= OUT {
                    continue;
                }
                let r = r as usize;
                let run = angle(
                    self.window.cell_dir(i % side, i / side),
                    self.window.cell_dir(r % side, r / side),
                ) / cell;
                let f = ERODE * flow.area[i].powf(AREA_EXPONENT) / run;
                // A pit fills to one step of rough ground below its rim, so
                // a hollow stays shallow.
                let from = self.levels[i].max(flow.filled[i] - rough);
                let drop = ((from - self.levels[r]) * f / (1.0 + f)).clamp(0.0, CARVE * rough);
                self.levels[i] = from - drop;
            }
            // The main river goes down at each step.
            for k in (0..self.chain.len().saturating_sub(1)).rev() {
                let (c, next) = (self.chain[k], self.chain[k + 1]);
                self.levels[c] = self.levels[c].max(self.levels[next] + RIVER_STEP);
            }
        }
        // A little hillslope creep rounds the crests.
        let old = self.levels.clone();
        for i in (0..side * side).filter(|&i| self.inside[i] && !on_chain[i]) {
            let near = self.window.neighbors(i);
            let (sum, count) = near
                .iter()
                .filter(|&&j| j != NONE)
                .fold((0.0, 0.0), |(s, c), &j| (s + old[j as usize], c + 1.0));
            self.levels[i] += CREEP * (sum / count - old[i]);
        }
    }
}

/// The level of a profile of the main river at a distance along the line.
fn profile_at(profile: &[(f64, f64)], u: f64) -> f64 {
    match profile.partition_point(|point| point.0 < u) {
        0 => profile[0].1,
        k if k == profile.len() => profile[k - 1].1,
        k => {
            let (a, b) = (profile[k - 1], profile[k]);
            let t = if b.0 > a.0 {
                (u - a.0) / (b.0 - a.0)
            } else {
                1.0
            };
            a.1 + (b.1 - a.1) * t
        }
    }
}

/// A cell in the heap of a flood: a level, the number of cells that went
/// in before it, and the cell. The heap gives the lowest level first, and of
/// two cells at one level, the cell that went in first.
#[derive(PartialEq)]
struct Low(f64, u32, usize);

impl Eq for Low {}

impl Ord for Low {
    fn cmp(&self, other: &Low) -> Ordering {
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}

impl PartialOrd for Low {
    fn partial_cmp(&self, other: &Low) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The outline of a basin on the cells of a window: its lines in cells, and
/// for each cell the lines near it.
struct OutlineGrid {
    side: usize,
    lines: Vec<[f64; 4]>,
    bins: Vec<Vec<u32>>,
}

/// The lines of a bin of `OutlineGrid` are the lines that come nearer to the
/// cell than this number of cells.
const OUTLINE_REACH: f64 = 2.0;

impl OutlineGrid {
    fn new(outline: &[V3], window: &Window) -> OutlineGrid {
        let side = window.cells;
        let cell = FRAC_PI_2 * window.cell as f64 / window.face_size as f64;
        // The lines are short, so they are straight in the cells of the
        // window.
        let mut places = Vec::new();
        for k in 0..outline.len() {
            let (a, b) = (outline[k], outline[(k + 1) % outline.len()]);
            let steps = (angle(a, b) / (0.5 * cell)).ceil().max(1.0) as usize;
            for s in 0..steps {
                let t = s as f64 / steps as f64;
                if let Some(place) =
                    window.place(normalize(add(a, scale(add(b, scale(a, -1.0)), t))))
                {
                    places.push(place);
                }
            }
        }
        let lines: Vec<[f64; 4]> = (0..places.len())
            .map(|k| {
                let (a, b) = (places[k], places[(k + 1) % places.len()]);
                [a.0, a.1, b.0, b.1]
            })
            .collect();
        let mut bins = vec![Vec::new(); side * side];
        let last = side as f64 - 1.0;
        for (k, l) in lines.iter().enumerate() {
            let span = |a: f64, b: f64| {
                let from = (a.min(b) - OUTLINE_REACH).floor().clamp(0.0, last) as usize;
                let to = (a.max(b) + OUTLINE_REACH).floor().clamp(0.0, last) as usize;
                from..=to
            };
            for y in span(l[1], l[3]) {
                for x in span(l[0], l[2]) {
                    bins[y * side + x].push(k as u32);
                }
            }
        }
        OutlineGrid { side, lines, bins }
    }

    /// The distance from a place to a line, in cells.
    fn distance_to(&self, k: u32, u: f64, v: f64) -> f64 {
        let [ax, ay, bx, by] = self.lines[k as usize];
        let (dx, dy) = (bx - ax, by - ay);
        let len = dx * dx + dy * dy;
        let t = if len > 0.0 {
            (((u - ax) * dx + (v - ay) * dy) / len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (u - ax - t * dx).hypot(v - ay - t * dy)
    }

    /// The nearest line to a place and its distance, in cells, if a line is
    /// nearer than `OUTLINE_REACH`.
    fn nearest(&self, u: f64, v: f64) -> Option<(f64, u32)> {
        let inside = |c: f64| c >= 0.0 && c < self.side as f64;
        if !inside(u) || !inside(v) {
            return None;
        }
        let bin = &self.bins[v as usize * self.side + u as usize];
        let best = bin
            .iter()
            .map(|&k| (self.distance_to(k, u, v), k))
            .min_by(|a, b| a.0.total_cmp(&b.0))?;
        (best.0 <= OUTLINE_REACH).then_some(best)
    }

    /// The distance from a place to the outline, in cells, if it is nearer
    /// than `OUTLINE_REACH`.
    fn near(&self, u: f64, v: f64) -> Option<f64> {
        self.nearest(u, v).map(|n| n.0)
    }

    /// True if a place is inside the outline.
    fn contains(&self, u: f64, v: f64) -> bool {
        let mut inside = false;
        for l in &self.lines {
            let (ay, by) = (l[1], l[3]);
            if (ay <= v) != (by <= v) {
                let x = l[0] + (v - ay) / (by - ay) * (l[2] - l[0]);
                if x > u {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// The distance of each cell to the outline and to the sides of the
    /// window, in cells. It is less than zero outside the outline.
    fn distances(&self) -> Vec<f64> {
        let side = self.side;
        let count = side * side;
        // The cells inside the outline, row by row.
        let mut inside = vec![false; count];
        let mut crossings = Vec::new();
        for y in 0..side {
            let v = y as f64 + 0.5;
            crossings.clear();
            for l in &self.lines {
                let (ay, by) = (l[1], l[3]);
                if (ay <= v) != (by <= v) {
                    crossings.push(l[0] + (v - ay) / (by - ay) * (l[2] - l[0]));
                }
            }
            crossings.sort_by(f64::total_cmp);
            for [a, b] in crossings.as_chunks::<2>().0 {
                let from = (a - 0.5).ceil().max(0.0) as usize;
                let to = ((b - 0.5).ceil().max(0.0) as usize).min(side);
                for x in from..to {
                    inside[y * side + x] = true;
                }
            }
        }
        // The cells near the outline have the exact distance and the nearest
        // line. The other cells take the nearest line of a neighbor.
        let center = |i: usize| ((i % side) as f64 + 0.5, (i / side) as f64 + 0.5);
        let mut distance = vec![f64::INFINITY; count];
        let mut nearest = vec![u32::MAX; count];
        let mut heap = BinaryHeap::new();
        let mut pushes = 0u32;
        for i in 0..count {
            let (u, v) = center(i);
            if let Some((d, k)) = self.nearest(u, v) {
                distance[i] = d;
                nearest[i] = k;
                heap.push(Low(d, pushes, i));
                pushes += 1;
            }
        }
        while let Some(Low(d, _, i)) = heap.pop() {
            if d > distance[i] {
                continue;
            }
            let (x, y) = (i % side, i / side);
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if nx < 0 || ny < 0 || nx >= side as i64 || ny >= side as i64 {
                    continue;
                }
                let j = ny as usize * side + nx as usize;
                let (u, v) = center(j);
                let dj = self.distance_to(nearest[i], u, v);
                if dj < distance[j] {
                    distance[j] = dj;
                    nearest[j] = nearest[i];
                    heap.push(Low(dj, pushes, j));
                    pushes += 1;
                }
            }
        }
        (0..count)
            .map(|i| {
                let (u, v) = center(i);
                let d = if inside[i] { distance[i] } else { -distance[i] };
                d.min(window_border(side, u, v))
            })
            .collect()
    }

    /// The part of a main river inside the outline, from its first point
    /// inside. The line ends where it leaves the outline.
    fn clip(&self, line: &StrokeLine, window: &Window) -> Option<StrokeLine> {
        let contains = |p: V3| {
            window
                .place(p)
                .is_some_and(|(u, v)| self.contains(u, v) && window_border(self.side, u, v) > 0.0)
        };
        let mut river = StrokeLine::new(line.radius(), line.face_size());
        let mut last = None;
        for p in line.points() {
            if contains(p) {
                river.push(p, 1.0);
                last = Some(p);
            } else if let Some(mut a) = last {
                // The place where the line leaves the outline.
                let mut b = p;
                for _ in 0..30 {
                    let m = normalize(add(a, b));
                    if contains(m) {
                        a = m;
                    } else {
                        b = m;
                    }
                }
                river.push(a, 1.0);
                break;
            }
        }
        (river.len() >= 2).then_some(river)
    }
}

/// The distance from a place in a window to the sides of the window, less
/// one cell, in cells.
fn window_border(side: usize, u: f64, v: f64) -> f64 {
    let side = side as f64;
    u.min(side - u).min(v).min(side - v) - 1.0
}

/// The distance of each cell of the area to the main river along the cells
/// of the area, in radians. It is infinite for a cell that the main river
/// does not reach.
fn river_distances(window: &Window, chain: &[usize], inside: &[bool], cell: f64) -> Vec<f64> {
    let mut distance = vec![f64::INFINITY; inside.len()];
    let mut heap = BinaryHeap::new();
    let mut pushes = 0u32;
    for &c in chain {
        distance[c] = 0.0;
        heap.push(Low(0.0, pushes, c));
        pushes += 1;
    }
    while let Some(Low(d, _, i)) = heap.pop() {
        if d > distance[i] {
            continue;
        }
        for (k, &j) in window.neighbors(i).iter().enumerate() {
            if j == NONE || !inside[j as usize] {
                continue;
            }
            let run = if k < 4 { 1.0 } else { SQRT_2 };
            let dj = d + run * cell;
            if dj < distance[j as usize] {
                distance[j as usize] = dj;
                heap.push(Low(dj, pushes, j as usize));
                pushes += 1;
            }
        }
    }
    distance
}

/// A field on a square of `side` cells at a place in cells, with bilinear
/// blending between the cell centers.
fn bilinear(field: &[f64], side: usize, u: f64, v: f64) -> f64 {
    let last = side as i64 - 1;
    let (x, y) = (u - 0.5, v - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (x - x0, y - y0);
    let at = |dx: i64, dy: i64| {
        let cx = (x0 as i64 + dx).clamp(0, last) as usize;
        let cy = (y0 as i64 + dy).clamp(0, last) as usize;
        field[cy * side + cx]
    };
    let top = at(0, 0) * (1.0 - tx) + at(1, 0) * tx;
    let bottom = at(0, 1) * (1.0 - tx) + at(1, 1) * tx;
    top * (1.0 - ty) + bottom * ty
}

/// The rectangle of each face that the cells of an area touch.
fn basin_rects(window: &Window, inside: &[bool], n: usize) -> Vec<(usize, TexelRect)> {
    let side = window.cells;
    let cell = FRAC_PI_2 * window.cell as f64 / n as f64;
    let reach = HALF_DIAGONAL * cell + 2.0 * FRAC_PI_2 / n as f64;
    let mut rects = Vec::new();
    for face in 0..crate::cube::FACES {
        let mut all: Option<TexelRect> = None;
        for i in (0..inside.len()).filter(|&i| inside[i]) {
            let p = window.cell_dir(i % side, i / side);
            if let Some(rect) = brush_rect(n, face, p, reach) {
                all = Some(all.map_or(rect, |r| r.union(rect)));
            }
        }
        if let Some(rect) = all {
            rects.push((face, rect));
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::meters_to_level;
    use crate::math::{dir_to_lonlat, lonlat_to_dir};

    const N: usize = 256;

    fn level(m: f64) -> u16 {
        meters_to_level(m)
    }

    fn spec(density_deg: f64, seed: u32) -> BasinSpec {
        BasinSpec {
            height: f64::from(level(1500.0) - level(0.0)),
            density: density_deg.to_radians(),
            seed,
            rain: Rain::default(),
            channel: ChannelShape::default(),
            world_km: 6371.0,
            limits: Limits::default(),
        }
    }

    /// Land at 300 m.
    fn ground() -> Heightmap {
        Heightmap::new(N, level(300.0))
    }

    fn ground_of(n: usize) -> Heightmap {
        Heightmap::new(n, level(300.0))
    }

    /// The half-width of the outline of the tests, in degrees.
    const WIDTH: f64 = 8.0;

    /// The latitude of the middle of the basin at a longitude.
    fn middle(lon: f64, wave: f64) -> f64 {
        wave * (lon.to_radians() * 12.0).sin()
    }

    /// The outline around a line: `WIDTH` degrees to each side of the line
    /// and past the source, with a round end there. The outline ends at the
    /// longitude `to`.
    fn outline_of(from: f64, to: f64, wave: f64) -> Vec<V3> {
        let steps = 200;
        let mut outline = Vec::new();
        for k in 0..=steps {
            let lon = to + (from - to) * k as f64 / steps as f64;
            outline.push(lonlat_to_dir(lon, middle(lon, wave) + WIDTH));
        }
        let center = middle(from, wave);
        for k in 1..50 {
            let a = (90.0 + 180.0 * k as f64 / 50.0).to_radians();
            outline.push(lonlat_to_dir(
                from + WIDTH * a.cos(),
                center + WIDTH * a.sin(),
            ));
        }
        for k in 0..=steps {
            let lon = from + (to - from) * k as f64 / steps as f64;
            outline.push(lonlat_to_dir(lon, middle(lon, wave) - WIDTH));
        }
        outline
    }

    /// True if a place is outside the outline of `outline_of` by `margin`
    /// degrees or more.
    fn outside(lon: f64, lat: f64, from: f64, to: f64, wave: f64, margin: f64) -> bool {
        if lon > to + margin {
            return true;
        }
        if lon < from {
            let center = middle(from, wave);
            return (lon - from).hypot(lat - center) > WIDTH + margin;
        }
        (lat - middle(lon, wave)).abs() > WIDTH + margin
    }

    /// A basin along the equator. The main river goes from `from` to one
    /// degree past `to` degrees of longitude, and it leaves the outline at
    /// `to`.
    fn line(spec: BasinSpec, from: f64, to: f64, wave: f64) -> BasinLine {
        line_of(N, spec, from, to, wave)
    }

    fn line_of(n: usize, spec: BasinSpec, from: f64, to: f64, wave: f64) -> BasinLine {
        let mut line = BasinLine::new(spec, &outline_of(from, to, wave), n);
        let steps = 40;
        for k in 0..=steps {
            let lon = from + (to + 1.0 - from) * k as f64 / steps as f64;
            line.push(lonlat_to_dir(lon, middle(lon, wave)));
        }
        line
    }

    fn apply(map: &Heightmap, cut: &BasinCut) -> Heightmap {
        let mut map = map.snapshot();
        map.basin(cut);
        map
    }

    /// The tributaries: cells off the main river with the water of a
    /// tributary, that drain into the main river.
    fn tributaries(net: &BasinNet) -> usize {
        let on: std::collections::HashSet<usize> = net.chain.iter().copied().collect();
        (0..net.inside.len())
            .filter(|&i| net.inside[i] && !on.contains(&i))
            .filter(|&i| f64::from(net.area[i]) >= net.min_area)
            .filter(|&i| on.contains(&(net.receiver[i] as usize)))
            .count()
    }

    fn spacing(spec: BasinSpec) -> f64 {
        let line = line(spec, -25.0, 25.0, 0.0);
        let cut = basin(&ground(), &line);
        let net = cut.net.unwrap();
        line.length() / tributaries(&net).max(1) as f64
    }

    /// The cell angle of a window, in radians.
    fn cell_angle(window: &Window) -> f64 {
        FRAC_PI_2 * window.cell as f64 / window.face_size as f64
    }

    fn touch(side: usize, a: usize, b: usize) -> bool {
        (a % side).abs_diff(b % side) <= 1 && (a / side).abs_diff(b / side) <= 1
    }

    #[test]
    fn all_the_water_of_the_basin_leaves_at_the_outlet() {
        for (n, seed) in [(N, 7), (2048, 7), (2048, 20)] {
            let line = line_of(n, spec(2.0, seed), -20.0, 20.0, 2.0);
            let net = basin(&ground_of(n), &line).net.unwrap();
            let outlet = *net.chain.last().unwrap();
            // Each cell of the area drains to the outlet.
            for i in (0..net.inside.len()).filter(|&i| net.inside[i]) {
                let mut at = i;
                while net.receiver[at] != OUT {
                    let to = net.receiver[at] as usize;
                    assert!(net.inside[to], "cell {at} drains out of the basin");
                    at = to;
                }
                assert_eq!(at, outlet, "face size {n}, cell {i}");
            }
            if n > N {
                assert!(net.window.cell > 1, "{:?}", net.window);
            }
        }
    }

    #[test]
    fn the_main_river_follows_the_drawn_line() {
        for (n, wave) in [(N, 0.0), (N, 3.0), (2048, 3.0)] {
            let line = line_of(n, spec(2.0, 7), -20.0, 20.0, wave);
            let net = basin(&ground_of(n), &line).net.unwrap();
            let side = net.window.cells;
            let cell = cell_angle(&net.window);
            for (k, &c) in net.chain.iter().enumerate() {
                let (d, _) = net.river.nearest(net.window.cell_dir(c % side, c / side));
                assert!(
                    d <= cell,
                    "face size {n}, wave {wave}: cell {k} is {} cells off",
                    d / cell
                );
            }
            // The main river runs from the source to the outline.
            let ends = [line.points()[0], lonlat_to_dir(20.0, middle(20.0, wave))];
            let first = net.chain[0];
            let last = *net.chain.last().unwrap();
            let dir = |c: usize| net.window.cell_dir(c % side, c / side);
            assert!(angle(dir(first), ends[0]) <= cell);
            assert!(angle(dir(last), ends[1]) <= cell);
        }
    }

    #[test]
    fn tributaries_join_the_main_river_at_the_density_distance() {
        let mean = |degrees: f64| {
            let seeds = 1..5;
            let sum: f64 = seeds.clone().map(|seed| spacing(spec(degrees, seed))).sum();
            sum / seeds.len() as f64
        };
        let (near, far) = (mean(1.5), mean(3.0));
        for (degrees, spacing) in [(1.5, near), (3.0, far)] {
            let d = f64::to_radians(degrees);
            assert!(
                (0.5 * d..=2.0 * d).contains(&spacing),
                "density {degrees}: the tributaries are {} degrees apart",
                spacing.to_degrees()
            );
        }
        assert!(far > 1.4 * near, "{near} and {far}");
    }

    /// Ground that rises by 1000 m from 25 degrees east to 25 degrees west.
    fn ramp() -> Heightmap {
        let mut map = Heightmap::new(N, level(300.0));
        for face in 0..crate::cube::FACES {
            for y in 0..N {
                for x in 0..N {
                    let (lon, _) = dir_to_lonlat(map.texel_dir(face, x, y));
                    let m = 300.0 + 1000.0 * ((25.0 - lon) / 50.0).clamp(0.0, 1.0);
                    map.set(face, x, y, level(m));
                }
            }
        }
        map
    }

    #[test]
    fn the_main_river_is_concave_and_goes_down_at_each_step() {
        let map = ramp();
        let line = line(spec(2.0, 7), -20.0, 20.0, 2.0);
        let cut = basin(&map, &line);
        let net = cut.net.clone().unwrap();
        let levels: Vec<f64> = net.chain.iter().map(|&c| net.levels[c]).collect();
        assert!(levels.windows(2).all(|w| w[1] < w[0]), "{levels:?}");
        let count = levels.len();
        let (top, low) = (levels[0], levels[count - 1]);
        for (k, &level) in levels.iter().enumerate().skip(1).take(count - 2) {
            let straight = top + (low - top) * k as f64 / (count - 1) as f64;
            assert!(
                level < straight,
                "node {k} is above the straight line: {levels:?}"
            );
        }
        let third = count / 3;
        let first = (levels[0] - levels[third]) / third as f64;
        let last = (levels[count - 1 - third] - levels[count - 1]) / third as f64;
        assert!(first > 2.0 * last, "the slopes are {first} and {last}");
        // On the texels, the bed goes down from the source to the outlet.
        let after = apply(&map, &cut);
        let side = net.window.cells;
        let bed: Vec<u16> = net
            .chain
            .iter()
            .map(|&c| {
                let (face, x, y) = after.texel_of(net.window.cell_dir(c % side, c / side));
                let near = |a: usize| a.saturating_sub(2)..=(a + 2).min(N - 1);
                let texels = near(y).flat_map(|y| near(x).map(move |x| (x, y)));
                texels
                    .map(|(x, y)| after.get(face, x as i64, y as i64))
                    .min()
                    .unwrap()
            })
            .collect();
        assert!(bed.windows(2).all(|w| w[1] <= w[0]), "{bed:?}");
    }

    #[test]
    fn the_main_river_is_not_above_the_old_ground() {
        // Hills, and a ridge across the basin.
        let mut map = ground();
        for face in 0..crate::cube::FACES {
            for y in 0..N {
                for x in 0..N {
                    let p = map.texel_dir(face, x, y);
                    let (lon, lat) = dir_to_lonlat(p);
                    let hills = 400.0 * fractal(p, 0.08, 3);
                    let ridge = 900.0 * (-((lon + 0.6 * lat - 2.0) / 2.5).powi(2)).exp();
                    map.set(face, x, y, level(500.0 + hills + ridge));
                }
            }
        }
        let line = line(spec(2.0, 7), -20.0, 20.0, 2.0);
        let cut = basin(&map, &line);
        let after = apply(&map, &cut);
        let net = cut.net.unwrap();
        let side = net.window.cells;
        // The bed of the channel is at or below the lowest old ground near
        // it and upstream of it, with a margin for the depth of the channel.
        let margin = DEPTH_M / METERS_PER_LEVEL;
        let mut low = f64::MAX;
        for &c in &net.chain {
            let (face, x, y) = after.texel_of(net.window.cell_dir(c % side, c / side));
            let near = |a: usize| a.saturating_sub(1)..=(a + 1).min(N - 1);
            let texels = || near(y).flat_map(|y| near(x).map(move |x| (x, y)));
            let lowest = |map: &Heightmap| {
                texels()
                    .map(|(x, y)| f64::from(map.get(face, x as i64, y as i64)))
                    .fold(f64::MAX, f64::min)
            };
            low = low.min(lowest(&map));
            let bed = lowest(&after);
            // At the outline, the change goes to zero, so the bed meets the
            // old ground there.
            let (lon, _) = dir_to_lonlat(net.window.cell_dir(c % side, c / side));
            if lon < 19.0 {
                assert!(
                    bed <= low + margin,
                    "the bed is {bed}, the old ground {low}"
                );
            }
        }
    }

    #[test]
    fn the_basin_follows_the_outline() {
        // The outline goes 12 degrees to the north of the main river and 5
        // degrees to the south.
        let mut outline = Vec::new();
        for k in 0..=100 {
            outline.push(lonlat_to_dir(20.0 - 40.0 * f64::from(k) / 100.0, 12.0));
        }
        for k in 0..=100 {
            outline.push(lonlat_to_dir(-20.0 + 40.0 * f64::from(k) / 100.0, -5.0));
        }
        let mut line = BasinLine::new(spec(2.0, 7), &outline, N);
        for k in 0..=40 {
            line.push(lonlat_to_dir(-15.0 + 36.0 * f64::from(k) / 40.0, 0.0));
        }
        let map = ground();
        let cut = basin(&map, &line);
        let (mut north, mut south) = (f64::MIN, f64::MAX);
        for (face, rect, data) in &cut.rects {
            let width = rect.x1 - rect.x0;
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    if data[(y - rect.y0) * width + x - rect.x0]
                        != map.get(*face, x as i64, y as i64)
                    {
                        let (_, lat) = dir_to_lonlat(map.texel_dir(*face, x, y));
                        north = north.max(lat);
                        south = south.min(lat);
                    }
                }
            }
        }
        assert!(
            north > 10.0 && north < 12.1,
            "the basin ends at {north} north"
        );
        assert!(
            south < -3.0 && south > -5.1,
            "the basin ends at {south} south"
        );
    }

    #[test]
    fn the_rain_changes_the_density_of_the_tributaries() {
        // Low ground with a range to the west, so the west wind brings
        // little rain.
        let mut map = Heightmap::new(N, level(20.0));
        for face in 0..crate::cube::FACES {
            for y in 0..N {
                for x in 0..N {
                    let (lon, lat) = dir_to_lonlat(map.texel_dir(face, x, y));
                    if (-28.0..-18.0).contains(&lon) && lat.abs() < 30.0 {
                        map.set(face, x, y, level(3000.0));
                    }
                }
            }
        }
        let count = |shadow: f64| {
            let rain = Rain {
                wind: 270.0,
                shadow,
            };
            let line = line(
                BasinSpec {
                    rain,
                    ..spec(1.5, 7)
                },
                -5.0,
                20.0,
                0.0,
            );
            tributaries(&basin(&map, &line).net.unwrap())
        };
        let (wet, dry) = (count(0.0), count(1.0));
        assert!(dry < wet, "{dry} and {wet} tributaries");
    }

    #[test]
    fn another_seed_gives_another_network() {
        let map = ground();
        let cut = |seed: u32| basin(&map, &line(spec(2.0, seed), -20.0, 20.0, 0.0));
        let (a, b, c) = (cut(7), cut(7), cut(8));
        assert!(a.rects == b.rects);
        let (a, c) = (a.net.unwrap(), c.net.unwrap());
        let inside = (0..a.inside.len()).filter(|&i| a.inside[i] && c.inside[i]);
        let (all, other) = inside.fold((0, 0), |(all, other), i| {
            (all + 1, other + usize::from(a.receiver[i] != c.receiver[i]))
        });
        assert!(10 * other > all, "{other} of {all} cells");
    }

    #[test]
    fn the_ground_outside_the_basin_does_not_change() {
        for ceiling in [None, Some(level(800.0))] {
            let map = ground();
            let mut spec = spec(2.0, 7);
            if let Some(ceiling) = ceiling {
                spec.limits.ceiling = ceiling;
            }
            let line = line(spec, -20.0, 20.0, 2.0);
            let cut = basin(&map, &line);
            let mut changed = 0;
            for (face, rect, data) in &cut.rects {
                let width = rect.x1 - rect.x0;
                for y in rect.y0..rect.y1 {
                    for x in rect.x0..rect.x1 {
                        let new = data[(y - rect.y0) * width + x - rect.x0];
                        let old = map.get(*face, x as i64, y as i64);
                        let (lon, lat) = dir_to_lonlat(map.texel_dir(*face, x, y));
                        if outside(lon, lat, -20.0, 20.0, 2.0, 0.1) {
                            assert_eq!(new, old, "face {face}, texel {x}, {y}");
                        }
                        if let Some(ceiling) = ceiling {
                            assert!(new <= ceiling, "face {face}, texel {x}, {y}");
                        }
                        changed += usize::from(new != old);
                    }
                }
            }
            assert!(changed > 1000, "{changed} texels changed");
        }
    }

    #[test]
    fn a_basin_across_a_face_edge_has_no_seam() {
        let map = ground();
        // Face 0 and face 2 meet at longitude 45.
        let line = line(spec(2.0, 7), 30.0, 60.0, 0.0);
        let cut = basin(&map, &line);
        let faces: Vec<usize> = cut.rects.iter().map(|r| r.0).collect();
        assert!(faces.contains(&0) && faces.contains(&2), "{faces:?}");
        let after = apply(&map, &cut);
        // A channel near the edge gives large jumps in a few rows, so the
        // test takes the median jump over the rows of the basin. The +u edge
        // of face 0 meets the -v edge of face 2.
        let level =
            |face: usize, x: usize, y: usize| i64::from(after.get(face, x as i64, y as i64));
        let (mut edge, mut inside) = (Vec::new(), Vec::new());
        for m in 104..=152 {
            let near = (N - 8..N).map(|x| level(0, x, m));
            let row: Vec<i64> = near.chain((0..8).map(|i| level(2, m, N - 1 - i))).collect();
            for (k, pair) in row.windows(2).enumerate() {
                let jump = (pair[1] - pair[0]).abs();
                if k == 7 {
                    edge.push(jump)
                } else {
                    inside.push(jump)
                }
            }
        }
        let median = |mut jumps: Vec<i64>| {
            jumps.sort_unstable();
            jumps[jumps.len() / 2]
        };
        let (edge, inside) = (median(edge), median(inside));
        assert!(
            edge <= 2 * inside + 4,
            "the median jump at the edge is {edge}, and inside it is {inside}"
        );
    }

    #[test]
    fn the_cells_of_the_line_are_a_chain() {
        let mut line = BasinLine::new(spec(2.0, 7), &outline_of(-20.0, 20.0, 0.0), N);
        // A line with a loop.
        for k in 0..=60 {
            let t = k as f64 / 60.0;
            let a = t * std::f64::consts::TAU * 1.2;
            line.push(lonlat_to_dir(
                -10.0 + 20.0 * t + 3.0 * a.sin(),
                3.0 - 3.0 * a.cos(),
            ));
        }
        let window = basin_window(&line.outline, line.width, N);
        let chain = chain_cells(&line.line, &window);
        let side = window.cells;
        for (k, &c) in chain.iter().enumerate() {
            if let Some(&next) = chain.get(k + 1) {
                assert!(touch(side, c, next) && c != next, "cell {k}");
            }
            for &later in chain.iter().skip(k + 2) {
                assert!(!touch(side, c, later), "cell {k} touches a later cell");
            }
        }
    }

    #[test]
    fn a_line_of_one_point_makes_no_basin() {
        let mut line = BasinLine::new(spec(2.0, 7), &outline_of(-20.0, 20.0, 0.0), N);
        line.push(lonlat_to_dir(0.0, 0.0));
        assert!(basin(&ground(), &line).rects.is_empty());
        // A main river outside the outline makes no basin, and nor does an
        // outline with no area.
        let mut line = BasinLine::new(spec(2.0, 7), &outline_of(-20.0, 20.0, 0.0), N);
        line.push(lonlat_to_dir(30.0, 0.0));
        line.push(lonlat_to_dir(40.0, 0.0));
        assert!(basin(&ground(), &line).rects.is_empty());
        let flat = [
            lonlat_to_dir(0.0, 0.0),
            lonlat_to_dir(10.0, 0.0),
            lonlat_to_dir(5.0, 0.0),
        ];
        let line = BasinLine::new(spec(2.0, 7), &flat, N);
        assert!(!line.has_outline());
    }

    /// Writes a hillshade of a basin to `target/basin-<n>.pgm`, on a grid of
    /// longitude and latitude. `N`, `SEED`, `FROM`, `TO`, and `Z` set the
    /// face size, the seed, the ends of the line, and the relief.
    #[test]
    #[ignore = "manual"]
    fn manual_basin_hillshade() {
        let var = |k: &str, d: f64| {
            std::env::var(k)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(d)
        };
        let n = var("N", 2048.0) as usize;
        let (from, to, z) = (var("FROM", -15.0), var("TO", 15.0), var("Z", 20.0));
        let mut map = Heightmap::new(n, level(300.0));
        if var("TERRAIN", 0.0) > 0.0 {
            // Rough hills and a ridge across the basin.
            for y in 0..n {
                for x in 0..n {
                    let p = map.texel_dir(0, x, y);
                    let (lon, lat) = crate::math::dir_to_lonlat(p);
                    let hills = 400.0 * fractal(p, 0.08, 3) + 300.0 * fractal(p, 0.02, 4);
                    let ridge = 900.0 * (-((lon + 0.6 * lat - 2.0) / 2.5).powi(2)).exp();
                    map.set(0, x, y, level(500.0 + hills + ridge));
                }
            }
        }
        // With `MEANDER`, the line has bends of that many degrees, about 3
        // degrees apart, on a larger wave.
        let meander = var("MEANDER", 0.0);
        let outline = outline_of(from, to, 2.0);
        let mut line = BasinLine::new(spec(2.0, var("SEED", 7.0) as u32), &outline, n);
        for k in 0..=2000 {
            let lon = from + (to + 1.0 - from) * f64::from(k) / 2000.0;
            let lat =
                2.0 * (lon.to_radians() * 12.0).sin() + meander * (lon.to_radians() * 120.0).sin();
            line.push(lonlat_to_dir(lon, lat));
        }
        let after = if var("BEFORE", 0.0) > 0.0 {
            map.snapshot()
        } else {
            apply(&map, &basin(&map, &line))
        };
        // `VIEW` and `SPAN` set the first longitude and the width of the
        // picture, in degrees.
        let step = 90.0 / n as f64 / var("ZOOM", 1.0);
        let (west, span) = (var("VIEW", from - 10.0), var("SPAN", to - from + 20.0));
        let tall = 24.0 * span / (to - from + 20.0);
        let (w, h) = ((span / step) as usize, (tall / step) as usize);
        let at = |x: usize, y: usize| {
            let (lon, lat) = (
                west + x as f64 * step,
                var("LAT", 0.0) + 0.5 * tall - y as f64 * step,
            );
            f64::from(after.sample(lonlat_to_dir(lon, lat))) * METERS_PER_LEVEL
        };
        let texel_m = step.to_radians() * 6_371_000.0;
        let mut out = format!("P5 {w} {h} 255\n").into_bytes();
        for y in 0..h {
            for x in 0..w {
                let dx = (at(x + 1, y) - at(x.saturating_sub(1), y)) / (2.0 * texel_m);
                let dy = (at(x, y + 1) - at(x, y.saturating_sub(1))) / (2.0 * texel_m);
                let (nx, ny) = (-dx * z, -dy * z);
                let len = (nx * nx + ny * ny + 1.0).sqrt();
                let light = ((-nx - ny + 1.0) / (len * 3f64.sqrt())).clamp(0.0, 1.0);
                // With `HEIGHT`, the picture also shows the height, from
                // black at 0 m to white at that many meters.
                let top = var("HEIGHT", 0.0);
                let change = var("CHANGE", 0.0);
                if change > 0.0 {
                    let lon = west + x as f64 * step;
                    let lat = var("LAT", 0.0) + 0.5 * tall - y as f64 * step;
                    let was = f64::from(map.sample(lonlat_to_dir(lon, lat))) * METERS_PER_LEVEL;
                    let d = (at(x, y) - was) / change;
                    out.push(((0.5 + 0.5 * d).clamp(0.0, 1.0) * 255.0) as u8);
                    continue;
                }
                let light = if top > 0.0 {
                    let m = at(x, y) + crate::cube::ELEV_MIN;
                    0.75 * (m / top).clamp(0.0, 1.0) + 0.25 * light
                } else {
                    light
                };
                out.push((light * 255.0) as u8);
            }
        }
        std::fs::write(format!("../../target/basin-{n}.pgm"), out).unwrap();
    }
}
