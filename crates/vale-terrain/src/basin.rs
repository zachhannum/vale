//! The basin brush.
//!
//! The brush makes a watershed from one stroke. The line of the stroke is the
//! main river, from its source at the first point to its outlet at the last
//! point. The area of the basin is the ground within the brush radius of the
//! line, with a round end at the source and a flat end at the outlet.
//!
//! The work is on the cells of one window. Each cell of the area drains to a
//! neighbor in the area that is nearer to the main river, so all the water
//! drains to the outlet, and no stream runs beside the main river.
//! Stream-power erosion with uplift then runs to a steady state with the
//! implicit solver of Braun and Willett (2013). The tributaries and the
//! concave profiles come from the erosion. A wall on the rim of the area is
//! the divide. Last, the rivers are cut into the texels as thin channels.

use std::f64::consts::{FRAC_PI_2, SQRT_2};

use crate::channel::{ChannelCurve, ChannelCut, ChannelShape, METERS_PER_LEVEL, channel_size};
use crate::erode::Erosion;
use crate::flow::{
    CoarseHeights, Grid, NONE, OUT, WINDOW_CELLS, Window, WindowHeights, level_curves,
};
use crate::heightmap::{Heightmap, Limits, MAX_BRUSH_RADIUS, TexelRect, brush_rect, noise3};
use crate::line::{LINE_SPACING, StrokeLine};
use crate::math::{V3, add, angle, dot, normalize, scale};
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

/// The half-width of the valley of a tributary, in tributary distances.
const VALLEY: f64 = 1.0;

/// The width of a valley grows with the water of its stream to this power.
const VALLEY_GROWTH: f64 = 0.3;

/// A stream with this part of the water of a tributary or more makes a
/// valley.
const SMALL_STREAMS: f64 = 0.25;

/// The ground goes down to the old ground over this part of the brush radius
/// at the side of the area.
const EDGE: f64 = 0.25;

const SALT_BASIN: u32 = 0x5b2e_93c7;

/// The settings of a basin stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BasinSpec {
    /// The brush radius as an angle, in radians. It is half of the width of
    /// the basin.
    pub radius: f64,
    /// The height of the highest ground inside the divide above the outlet,
    /// in 16-bit steps.
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

/// The line of one basin stroke.
#[derive(Clone, Debug)]
pub struct BasinLine {
    spec: BasinSpec,
    line: StrokeLine,
}

impl BasinLine {
    /// Starts a line with no points on a heightmap with `face_size` texels
    /// along one side of a face. The settings go inside their limits.
    pub fn new(spec: BasinSpec, face_size: usize) -> BasinLine {
        let step = FRAC_PI_2 / face_size as f64;
        let radius = spec.radius.clamp(8.0 * step, MAX_BRUSH_RADIUS);
        let spec = BasinSpec {
            radius,
            height: spec.height.clamp(0.0, 65535.0),
            ..spec
        };
        BasinLine {
            spec,
            line: StrokeLine::new(radius, face_size),
        }
    }

    /// Adds a point. A point near the last point is not added.
    pub fn push(&mut self, dir: V3) {
        self.line.push(dir, 1.0);
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

    /// The settings, inside their limits.
    pub fn spec(&self) -> BasinSpec {
        self.spec
    }

    /// The direction of each point.
    pub fn points(&self) -> Vec<V3> {
        self.line.points().collect()
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
    /// The end of the mouth, past the side of the area.
    pub(crate) mouth: V3,
}

/// The receivers of the cells of the area, and the cells in an order where
/// each receiver is before the cells that it takes water from.
struct Flow {
    receiver: Vec<u32>,
    stack: Vec<usize>,
}

/// Makes the basin of a line on the ground of `map`. A line of less than two
/// points makes no basin.
pub fn basin(map: &Heightmap, line: &BasinLine) -> BasinCut {
    if line.len() < 2 {
        return BasinCut::default();
    }
    let spec = line.spec;
    let n = map.face_size();
    let radius = spec.radius;
    let window = basin_window(&line.line, radius, n);
    let side = window.cells;
    let count = side * side;
    let cell = FRAC_PI_2 * window.cell as f64 / n as f64;
    let old = WindowHeights::new(map, window);
    let dirs: Vec<V3> = (0..count)
        .map(|i| window.cell_dir(i % side, i / side))
        .collect();
    let places: Vec<(f64, f64)> = dirs.iter().map(|&p| line.line.nearest(p)).collect();
    let chain = chain_cells(&line.line, &window);
    // A cell of the area is inside the radius with all its texels, so its
    // whole cell changes.
    let length = line.length().max(cell);
    let outside = |(d, _): (f64, f64)| d - radius;
    let margin = HALF_DIAGONAL * cell;
    let mut inside: Vec<bool> = places.iter().map(|&p| outside(p) + margin <= 0.0).collect();
    let mut on_chain = vec![false; count];
    for &c in &chain {
        inside[c] = true;
        on_chain[c] = true;
    }
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
    let mut h: Vec<f64> = (0..count)
        .map(|i| {
            let (d, u) = places[i];
            // Smaller octaves bend the streams.
            let octave = |k: u32| {
                let size = f64::from(1u32 << k);
                noise3(scale(dirs[i], size / wavelength), seed.wrapping_add(k))[0] / size
            };
            let bump = octave(0) + octave(1) + octave(2);
            let across = if on_chain[i] { 0.0 } else { d / radius };
            across + ALONG * (1.0 - u / length) + BUMPS * wavelength / radius * bump
        })
        .collect();

    // A cell drains to a neighbor that is nearer to the main river by a part
    // of a cell. A cell with no such neighbor is not in the area.
    let order_of = |inside: &[bool]| {
        let mut order: Vec<usize> = (0..count).filter(|&i| inside[i] && !on_chain[i]).collect();
        order.sort_by(|&a, &b| places[a].0.total_cmp(&places[b].0));
        order
    };
    let mut order = order_of(&inside);
    let mut flow = loop {
        let can_take = |i: usize, j: usize| {
            on_chain[j] || (inside[j] && !rim[j] && places[j].0 < places[i].0 - CLOSER * cell)
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
        on_chain[j] || (inside[j] && !rim[j] && places[j].0 < places[i].0 - CLOSER * cell)
    };
    let length_to = |i: usize, r: usize| angle(dirs[i], dirs[r]) / cell;
    let outlet = *chain.last().expect("the chain has cells");

    // Braun and Willett (2013): an implicit step of the stream-power law
    // with uplift, in the order of the stack. With an uplift of 1 and an
    // erodibility of 1, the steady state of a cell is the level of its
    // receiver plus its length over the square root of its water.
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
    // The steady state of the last receivers.
    for &i in &flow.stack {
        h[i] = match flow.receiver[i] {
            OUT => 0.0,
            r => h[r as usize] + length_to(i, r as usize) / area[i].powf(AREA_EXPONENT),
        };
    }

    // The levels. The outlet is at the old ground plus the depth of the
    // channel there, so the bed of the channel meets the old ground.
    let area_sr: Vec<f64> = area.iter().map(|a| a * mean_solid).collect();
    let depth = DEPTH_M / METERS_PER_LEVEL;
    let base = f64::from(old.levels[outlet]) + depth * channel_size(area_sr[outlet]);
    let top = (0..count)
        .filter(|&i| inside[i] && !rim[i])
        .map(|i| h[i])
        .fold(f64::MIN_POSITIVE, f64::max);
    let mut levels: Vec<f64> = (0..count)
        .map(|i| {
            if inside[i] {
                base + h[i] / top * spec.height
            } else {
                f64::from(old.levels[i])
            }
        })
        .collect();
    // Each cell is above its receiver, and each cell next to the main river
    // is above the river cells next to it. Thus the main river stays on the
    // line.
    let floor = |levels: &mut Vec<f64>| {
        for &i in &flow.stack {
            if let r @ 0..OUT = flow.receiver[i] {
                levels[i] = levels[i].max(levels[r as usize] + STEP);
            }
        }
    };
    floor(&mut levels);
    for i in (0..count).filter(|&i| inside[i] && !on_chain[i] && !rim[i]) {
        let near = window.neighbors(i);
        let river = near
            .iter()
            .filter(|&&j| j != NONE && on_chain[j as usize])
            .map(|&j| levels[j as usize] + STEP)
            .fold(f64::MIN, f64::max);
        levels[i] = levels[i].max(river);
    }
    floor(&mut levels);
    // The channels: the tributaries and the whole main river.
    let min_area = TRIBUTARY_AREA * (density / cell) * (radius / cell) * mean_solid;
    let mut curve_area: Vec<f32> = area_sr.iter().map(|&a| a as f32).collect();
    for &c in &chain {
        curve_area[c] = curve_area[c].max((min_area / SMALL_STREAMS) as f32);
    }
    // Each cell of the main river drains more than each tributary that joins
    // the main river below it, so the curve of the main river does not bend
    // toward a tributary.
    let mut largest = vec![0.0f32; count];
    for i in (0..count).filter(|&i| inside[i] && !on_chain[i]) {
        if let r @ 0..OUT = flow.receiver[i] {
            largest[r as usize] = largest[r as usize].max(curve_area[i]);
        }
    }
    for pair in chain.windows(2) {
        curve_area[pair[0]] = curve_area[pair[0]].max(1.001 * largest[pair[1]]);
    }
    // The streams that shape the valleys. The smallest ones make no
    // channel.
    let valley_area = min_area * SMALL_STREAMS;
    let mut curves = level_curves(&window, &flow.receiver, &curve_area, valley_area, &levels);
    // The mouth: the main river goes on past the outlet to the side of the
    // area, and down to the old ground there. It is the only gap in the
    // divide.
    let points = line.points();
    let (end, before) = (points[points.len() - 1], points[points.len() - 2]);
    let ahead = normalize(add(end, scale(before, -1.0)));
    let past = |k: f64| normalize(add(end, scale(ahead, k * radius)));
    let mouth = past(1.0 + EDGE);
    let water = f64::from(curve_area[outlet]);
    let outlet_level = levels[outlet];
    let mouth_level =
        (f64::from(map.sample(mouth)) + depth * channel_size(water)).min(outlet_level - STEP);
    curves.push(ChannelCurve {
        points: [end, past(0.5), mouth],
        levels: [
            outlet_level,
            0.5 * (outlet_level + mouth_level),
            mouth_level,
        ],
        areas: [water; 3],
        plains: [0.0; 3],
    });
    let cut = ChannelCut {
        shape: spec.channel,
        depth,
        texel: FRAC_PI_2 / n as f64,
    };
    // The half-width of the valley of a stream, in radians.
    let valley = |area: f64| {
        let width = VALLEY * density * (area / min_area).max(0.0).powf(VALLEY_GROWTH);
        width.clamp(cell, radius)
    };
    // The part of the full depth of a valley, from 0 at the smallest stream
    // to 1 at two times the water of a tributary.
    let depth_part = |area: f64| {
        let octaves = (area / valley_area).max(1.0).log2() / (2.0 / SMALL_STREAMS).log2();
        smoothstep(octaves)
    };
    let reach = |curve: &ChannelCurve| {
        let area = curve.areas.iter().fold(0.0, |a: f64, &b| a.max(b));
        valley(area) / cell + 2.0
    };
    let bins = CurveBins::new(&curves, &window, reach);

    // The ridges between the valleys: the levels of the cells, smoothed over
    // the tributary distance.
    let ridges = smooth_inside(&levels, &inside, side, (VALLEY * density / cell) as usize);

    // How far each cell is outside the area, in radians.
    let edge: Vec<f64> = places.iter().map(|&p| outside(p)).collect();
    let blend = |field: &[f64], u: f64, v: f64| {
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
    };
    let band = EDGE * radius;

    // Each curve with the cosine of the largest angle from its middle point
    // to a place that it shapes.
    let bounds: Vec<f64> = curves
        .iter()
        .map(|curve| {
            let [a, b, c] = curve.points;
            let length = angle(b, a).max(angle(b, c));
            (length + reach(curve) * cell).min(FRAC_PI_2).cos()
        })
        .collect();
    let texel = |p: V3, old: f64| -> Option<f64> {
        let (u, v) = window.place(p)?;
        let inside_window = |c: f64| c >= 0.0 && c < side as f64;
        if !inside_window(u) || !inside_window(v) {
            return None;
        }
        let near = blend(&edge, u, v);
        let out = if near.abs() < 2.0 * cell {
            outside(line.line.nearest(p))
        } else {
            near
        };
        if out > 0.0 {
            return None;
        }
        // Each stream gives a valley that rises from its floor to the
        // ridges, and the lowest valley wins. Where two valleys meet, they
        // make a ridge.
        let ridge = blend(&ridges, u, v);
        let (mut ground, mut channel) = (ridge, f64::INFINITY);
        for k in bins.near(u, v) {
            let curve = &curves[k];
            if dot(p, curve.points[1]) < bounds[k] {
                continue;
            }
            let (distance, [floor, area, _]) = curve.nearest(p);
            // A small stream cuts less, so its valley fades out at its head.
            let top = ridge.max(floor);
            let shallow = top - (top - floor) * depth_part(area);
            let t = (distance / valley(area)).min(1.0);
            let rise = 1.0 - (1.0 - t) * (1.0 - t);
            ground = ground.min(shallow + (top - shallow) * rise);
            if area >= min_area {
                let at = cut.place(curve, p);
                let weight = cut.channel_weight(&at);
                if weight > 0.0 {
                    let bed = shallow - depth * at.size;
                    channel = channel.min(shallow + weight * (bed - shallow));
                }
            }
        }
        // The ground goes down to the old ground at the side.
        let side_part = smoothstep((out + band) / band);
        let new = ground.min(channel);
        Some(new + (old - new) * side_part)
    };

    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut rects = Vec::new();
    for (face, rect) in basin_rects(&line.line, radius, n) {
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
    BasinCut {
        rects,
        net: Some(BasinNet {
            window,
            inside,
            levels,
            receiver: flow.receiver,
            area: curve_area,
            chain,
            min_area,
            mouth,
        }),
    }
}

/// The window of the cells of a basin. The cell is the smallest that holds
/// the area in the window, with `MIN_RADIUS_CELLS` cells in the radius or
/// more, and it is not larger than a cell of the coarse copy. A larger area
/// is cut at the sides of the window.
fn basin_window(line: &StrokeLine, radius: f64, n: usize) -> Window {
    let points: Vec<V3> = line.points().collect();
    let center = normalize(points.iter().fold([0.0; 3], |sum, &p| add(sum, p)));
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
        let margin = radius / angle + 2.0;
        let room = |c: f64| c >= margin && c <= cells as f64 - margin;
        let fits = points
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

/// A field smoothed over the cells of the area, with 3 passes of a box of
/// `reach` cells on each side. A cell outside the area takes the smoothed
/// field of the area near it, and a cell far from the area keeps its value.
fn smooth_inside(field: &[f64], inside: &[bool], side: usize, reach: usize) -> Vec<f64> {
    let mut sum: Vec<f64> = (0..field.len())
        .map(|i| if inside[i] { field[i] } else { 0.0 })
        .collect();
    let mut weight: Vec<f64> = inside.iter().map(|&i| f64::from(u8::from(i))).collect();
    for _ in 0..3 {
        for values in [&mut sum, &mut weight] {
            box_pass(values, side, reach, true);
            box_pass(values, side, reach, false);
        }
    }
    (0..field.len())
        .map(|i| {
            if weight[i] > 1e-6 {
                sum[i] / weight[i]
            } else {
                field[i]
            }
        })
        .collect()
}

/// One pass of a box filter along the rows or along the columns.
fn box_pass(values: &mut [f64], side: usize, reach: usize, rows: bool) {
    let at = |line: usize, k: usize| {
        if rows {
            line * side + k
        } else {
            k * side + line
        }
    };
    let mut copy = vec![0.0; side + 1];
    for line in 0..side {
        // The running sum of the line.
        for k in 0..side {
            copy[k + 1] = copy[k] + values[at(line, k)];
        }
        let count = (2 * reach + 1) as f64;
        for k in 0..side {
            let (a, b) = (k.saturating_sub(reach), (k + reach + 1).min(side));
            values[at(line, k)] = (copy[b] - copy[a]) / count;
        }
    }
}

/// The curves near each square of 4 by 4 cells of a window.
struct CurveBins {
    side: usize,
    bins: Vec<Vec<usize>>,
}

impl CurveBins {
    /// `reach` gives the largest distance from a curve to a place that it
    /// shapes, in cells.
    fn new(
        curves: &[ChannelCurve],
        window: &Window,
        reach: impl Fn(&ChannelCurve) -> f64,
    ) -> CurveBins {
        let side = window.cells.div_ceil(BIN_CELLS);
        let mut bins = vec![Vec::new(); side * side];
        for (k, curve) in curves.iter().enumerate() {
            let places: Vec<(f64, f64)> = curve
                .points
                .iter()
                .filter_map(|&p| window.place(p))
                .collect();
            if places.len() < 3 {
                continue;
            }
            let reach = reach(curve);
            let range = |pick: fn(&(f64, f64)) -> f64| {
                let low = places.iter().map(pick).fold(f64::MAX, f64::min) - reach;
                let high = places.iter().map(pick).fold(f64::MIN, f64::max) + reach;
                let bin =
                    |c: f64| (c / BIN_CELLS as f64).floor().clamp(0.0, side as f64 - 1.0) as usize;
                bin(low)..=bin(high)
            };
            for y in range(|p| p.1) {
                for x in range(|p| p.0) {
                    bins[y * side + x].push(k);
                }
            }
        }
        CurveBins { side, bins }
    }

    /// The curves near a place of the window in cells.
    fn near(&self, u: f64, v: f64) -> impl Iterator<Item = usize> + '_ {
        let bin = |c: f64| ((c / BIN_CELLS as f64) as usize).min(self.side - 1);
        self.bins[bin(v) * self.side + bin(u)].iter().copied()
    }
}

/// The number of cells along one side of a bin of `CurveBins`.
const BIN_CELLS: usize = 4;

/// The rectangle of each face that the area of a line can touch.
fn basin_rects(line: &StrokeLine, radius: f64, n: usize) -> Vec<(usize, TexelRect)> {
    let points: Vec<V3> = line.points().collect();
    let mut samples = vec![points[0]];
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let steps = (angle(a, b) / (LINE_SPACING * radius)).ceil().max(1.0) as usize;
        for s in 1..=steps {
            let t = s as f64 / steps as f64;
            samples.push(normalize(add(a, scale(add(b, scale(a, -1.0)), t))));
        }
    }
    let mut rects = Vec::new();
    for face in 0..crate::cube::FACES {
        let mut all: Option<TexelRect> = None;
        for &s in &samples {
            if let Some(rect) = brush_rect(n, face, s, radius * 1.02) {
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
    use crate::flow::{drain, sea};
    use crate::math::{dir_to_lonlat, lonlat_to_dir};

    const N: usize = 256;

    fn level(m: f64) -> u16 {
        meters_to_level(m)
    }

    fn spec(density_deg: f64, seed: u32) -> BasinSpec {
        BasinSpec {
            radius: 8f64.to_radians(),
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

    /// A line along the equator, from `from` to `to` degrees of longitude.
    fn line(spec: BasinSpec, from: f64, to: f64, wave: f64) -> BasinLine {
        line_of(N, spec, from, to, wave)
    }

    fn line_of(n: usize, spec: BasinSpec, from: f64, to: f64, wave: f64) -> BasinLine {
        let mut line = BasinLine::new(spec, n);
        let steps = 40;
        for k in 0..=steps {
            let lon = from + (to - from) * k as f64 / steps as f64;
            let lat = wave * (lon.to_radians() * 12.0).sin();
            line.push(lonlat_to_dir(lon, lat));
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

    /// The receivers of the flow on the cells of the ground after a basin.
    fn flow_after(map: &Heightmap, net: &BasinNet) -> Vec<u32> {
        let heights = WindowHeights::new(map, net.window);
        drain(&net.window, &heights.levels, &[], sea()).receiver
    }

    #[test]
    fn all_the_water_of_the_basin_leaves_at_the_outlet() {
        for (n, seed) in [(N, 7), (2048, 7), (2048, 20)] {
            all_the_water_leaves_at_the_outlet(n, seed);
        }
    }

    fn all_the_water_leaves_at_the_outlet(n: usize, seed: u32) {
        let map = ground_of(n);
        let line = line_of(n, spec(2.0, seed), -20.0, 20.0, 2.0);
        let cut = basin(&map, &line);
        let net = cut.net.clone().unwrap();
        let outlet = *net.chain.last().unwrap();
        // On the cells of the basin, each cell drains to the outlet, and the
        // levels go down at each step.
        for i in (0..net.inside.len()).filter(|&i| net.inside[i]) {
            let mut at = i;
            while net.receiver[at] != OUT {
                let to = net.receiver[at] as usize;
                assert!(net.inside[to], "cell {at} drains out of the basin");
                assert!(net.levels[to] < net.levels[at], "cell {at} drains up");
                at = to;
            }
            assert_eq!(at, outlet);
        }
        // On the new ground, the water of each cell inside the divide leaves
        // the basin at the outlet.
        let after = apply(&map, &cut);
        let receiver = flow_after(&after, &net);
        let side = net.window.cells;
        // The divide is the crest at the inner side of the band where the
        // ground goes down to the old ground.
        let cell = cell_angle(&net.window);
        let divide = EDGE * line.spec.radius + 2.0 * cell;
        let within = |i: usize| {
            let (d, _) = line.line.nearest(net.window.cell_dir(i % side, i / side));
            d - line.spec.radius < -divide
        };
        let inner = (0..net.inside.len()).filter(|&i| net.inside[i] && within(i));
        for i in inner {
            let mut at = i;
            let mut last = i;
            while at < OUT as usize && net.inside[at] {
                last = at;
                at = receiver[at] as usize;
            }
            // The mouth is the band at the end of the line.
            let dir = |i: usize| net.window.cell_dir(i % side, i / side);
            let from_outlet = angle(dir(last), net.mouth);
            assert!(
                from_outlet <= divide,
                "the water of cell {i} leaves the basin {} cells from the mouth",
                from_outlet / cell
            );
        }
        if n > N {
            assert!(net.window.cell > 1, "{:?}", net.window);
        }
    }

    #[test]
    fn the_main_river_follows_the_drawn_line() {
        for (n, wave) in [(N, 0.0), (N, 3.0), (2048, 3.0)] {
            let map = ground_of(n);
            let line = line_of(n, spec(2.0, 7), -20.0, 20.0, wave);
            let cut = basin(&map, &line);
            let net = cut.net.clone().unwrap();
            let after = apply(&map, &cut);
            let receiver = flow_after(&after, &net);
            let side = net.window.cells;
            let cell = cell_angle(&net.window);
            let outlet = *net.chain.last().unwrap();
            let mut at = net.chain[0];
            let mut steps = 0;
            let end = *line.points().last().unwrap();
            let mouth = EDGE * line.spec.radius + cell;
            loop {
                // The river is within one cell of the line if the center of
                // its cell is within one and a half cells. The check ends at
                // the band of the mouth.
                let p = net.window.cell_dir(at % side, at / side);
                if at == outlet || angle(p, end) < mouth {
                    break;
                }
                let (d, _) = line.line.nearest(p);
                assert!(
                    d <= 1.5 * cell,
                    "face size {n}, wave {wave}: step {steps} is {} cells from the line",
                    d / cell
                );
                at = receiver[at] as usize;
                assert!(
                    at < OUT as usize && net.inside[at],
                    "wave {wave}: the river leaves at step {steps} of {}, cell {} {}, outlet {} {}",
                    net.chain.len(),
                    at % side,
                    at / side,
                    outlet % side,
                    outlet / side
                );
                steps += 1;
            }
            assert!(
                steps + 1 >= net.chain.len() / 2,
                "wave {wave}: {steps} steps"
            );
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

    #[test]
    fn the_main_river_is_concave_and_goes_down_at_each_step() {
        let map = ground();
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
                        let (d, _) = line.line.nearest(map.texel_dir(*face, x, y));
                        if d > line.spec.radius {
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
        let mut line = BasinLine::new(spec(2.0, 7), N);
        // A line with a loop.
        for k in 0..=60 {
            let t = k as f64 / 60.0;
            let a = t * std::f64::consts::TAU * 1.2;
            line.push(lonlat_to_dir(
                -10.0 + 20.0 * t + 3.0 * a.sin(),
                3.0 - 3.0 * a.cos(),
            ));
        }
        let window = basin_window(&line.line, line.spec.radius, N);
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
        let mut line = BasinLine::new(spec(2.0, 7), N);
        line.push(lonlat_to_dir(0.0, 0.0));
        assert!(basin(&ground(), &line).rects.is_empty());
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
        let map = Heightmap::new(n, level(300.0));
        let line = line_of(n, spec(2.0, var("SEED", 7.0) as u32), from, to, 2.0);
        let after = apply(&map, &basin(&map, &line));
        let step = 90.0 / n as f64;
        let (w, h) = (((to - from + 20.0) / step) as usize, (24.0 / step) as usize);
        let at = |x: usize, y: usize| {
            let (lon, lat) = (from - 10.0 + x as f64 * step, 12.0 - y as f64 * step);
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
                out.push((light * 255.0) as u8);
            }
        }
        std::fs::write(format!("../../target/basin-{n}.pgm"), out).unwrap();
    }
}
