//! The rivers of a heightmap.
//!
//! A coarse copy of the heightmap gives the flow of water from each land cell
//! to the sea. The cells that drain a large area are rivers. The channel map
//! holds the distance from each of its texels to the nearest river, and the
//! size of that river.
//!
//! A window is a square of small cells on a part of the sphere. It gives the
//! same things at the scale of a near view.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_4};

use crate::bands::SEA_LEVEL;
use crate::cube::{FACES, face_dir, face_of, meters_to_level, unwarp, warp};
use crate::heightmap::{Heightmap, TexelRect};
use crate::math::{V3, add, normalize, scale};

/// The largest number of flow cells along one side of a face.
pub const FLOW_SIZE: usize = 512;

/// The smallest area that a river cell drains, in steradians.
pub const RIVER_MIN_AREA: f64 = 5.0e-4;

/// The flow byte of the channel map goes from 1 to 255 over this number of
/// doublings of the drained area.
pub const RIVER_OCTAVES: f64 = 12.0;

/// The distance byte of the channel map for a distance of one channel texel.
pub const CHANNEL_SCALE: f64 = 32.0;

/// The largest distance that the channel map holds, in channel texels.
const CHANNEL_REACH: f64 = 255.0 / CHANNEL_SCALE;

/// The number of smoothing passes over the river nodes.
const SMOOTH_PASSES: usize = 2;

/// The number of cells that a river cell of a window drains at least. A
/// river cell of the full map drains about this number of cells when the map
/// has `FLOW_SIZE` cells along one side of a face.
pub const RIVER_MIN_CELLS: f64 = 62.0;

/// The lowest number of cells that a river cell must drain. At this number,
/// each land cell is a river cell.
pub const RIVER_MIN_CELLS_LOWEST: f64 = 1.0;

/// The limits of the valley width factor of the carve mode.
pub const VALLEY_MIN: f64 = 0.4;
pub const VALLEY_MAX: f64 = 1.25;

/// The widest valley of the carve mode, from the river to the side, in
/// channel texels. It is less than `CHANNEL_REACH`.
pub const VALLEY_WIDEST: f64 = 7.5;

/// The number of cells along one side of the window that `GpuHeightmap`
/// holds.
pub const WINDOW_CELLS: usize = 512;

/// The channel texels at each side of a window where the distance to a river
/// is not sure. A river outside the window can be nearer.
pub const WINDOW_MARGIN: f64 = 8.0;

const NONE: u32 = u32::MAX;

/// The receiver of a cell whose water leaves the window.
const OUT: u32 = u32::MAX - 1;

/// The steps to the 8 neighbors of a cell. The first 4 share a side with it.
const STEPS: [(i64, i64); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (-1, 1),
    (1, -1),
    (-1, -1),
];

fn sea() -> u16 {
    meters_to_level(SEA_LEVEL)
}

/// The equal-angle coordinate at the center of cell `i` of `m`. The cell can
/// be past the face edge.
fn center(m: usize, i: i64) -> f64 {
    (i as f64 + 0.5) / m as f64 * 2.0 - 1.0
}

fn cell_dir(m: usize, face: usize, x: usize, y: usize) -> V3 {
    let flat = |i: usize| unwarp(center(m, i as i64));
    face_dir(face, flat(x), flat(y))
}

/// The cell that holds a direction.
fn cell_at(m: usize, d: V3) -> (usize, usize, usize) {
    let (face, a, b) = face_of(d);
    let index = |a: f64| (((warp(a) + 1.0) * 0.5 * m as f64).floor().max(0.0) as usize).min(m - 1);
    (face, index(a), index(b))
}

/// The cell at a place that can be past the face edge. The cell grid
/// continues past the edge, as the texel grid does in `Heightmap::get`.
fn cell_past_edge(m: usize, face: usize, x: i64, y: i64) -> (usize, usize, usize) {
    let flat = |i: i64| unwarp(center(m, i).clamp(-1.99, 1.99));
    cell_at(m, face_dir(face, flat(x), flat(y)))
}

fn split(m: usize, i: usize) -> (usize, usize, usize) {
    (i / (m * m), i % m, i / m % m)
}

/// The 8 neighbors of a cell, in the order of `STEPS`. Next to a cube corner,
/// one cell can be in the list two times.
fn neighbors(m: usize, i: usize) -> [u32; 8] {
    let (face, x, y) = split(m, i);
    let inside = x > 0 && y > 0 && x + 1 < m && y + 1 < m;
    STEPS.map(|(dx, dy)| {
        if inside {
            return (i as i64 + dx + dy * m as i64) as u32;
        }
        let (x, y) = (x as i64 + dx, y as i64 + dy);
        let (face, x, y) = if x >= 0 && y >= 0 && x < m as i64 && y < m as i64 {
            (face, x as usize, y as usize)
        } else {
            cell_past_edge(m, face, x, y)
        };
        ((face * m + y) * m + x) as u32
    })
}

fn mix(mut h: u32) -> u32 {
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

/// A fixed number from 0 to 1 for neighbor `k` of cell `i`.
fn scatter(i: usize, k: usize) -> f64 {
    let h = mix((i as u32).wrapping_mul(8).wrapping_add(k as u32));
    f64::from(h) / f64::from(u32::MAX)
}

/// The solid angle of each cell of one face, in steradians, row by row.
fn solid_angles(m: usize) -> Vec<f64> {
    let flat: Vec<f64> = (0..m).map(|i| unwarp(center(m, i as i64))).collect();
    let step = |f: f64| (1.0 + f * f) * FRAC_PI_4 * 2.0 / m as f64;
    let mut out = Vec::with_capacity(m * m);
    for &fv in &flat {
        for &fu in &flat {
            let r = 1.0 + fu * fu + fv * fv;
            out.push(step(fu) * step(fv) / (r * r.sqrt()));
        }
    }
    out
}

/// A copy of the heightmap with large cells. Each cell holds the mean level
/// of some of its texels.
#[derive(Clone)]
pub struct CoarseHeights {
    m: usize,
    /// The cells of all faces, face by face, in row order.
    levels: Vec<u16>,
}

impl CoarseHeights {
    pub fn new(map: &Heightmap) -> CoarseHeights {
        let m = CoarseHeights::size_for(map.face_size());
        let mut heights = CoarseHeights {
            m,
            levels: vec![map.base(); FACES * m * m],
        };
        for (face, rect) in map.allocated_rects() {
            heights.update(map, face, rect);
        }
        heights
    }

    /// Makes `m` by `m` cells on each face. `level` gives the level at the
    /// center of each cell.
    pub fn from_fn(m: usize, level: impl Fn(V3) -> u16) -> CoarseHeights {
        assert!(m > 0, "the size must not be zero");
        let cells = (0..FACES * m * m).map(|i| {
            let (face, x, y) = split(m, i);
            level(cell_dir(m, face, x, y))
        });
        CoarseHeights {
            m,
            levels: cells.collect(),
        }
    }

    /// The number of cells along one side of a face.
    pub fn size(&self) -> usize {
        self.m
    }

    /// The size for a heightmap with `face_size` texels along one side of a
    /// face.
    pub fn size_for(face_size: usize) -> usize {
        face_size / face_size.div_ceil(FLOW_SIZE)
    }

    pub fn level(&self, face: usize, x: usize, y: usize) -> u16 {
        self.levels[(face * self.m + y) * self.m + x]
    }

    /// Reads again each cell that has a texel in `rect`. A cell reads 16 of
    /// its texels at most.
    pub fn update(&mut self, map: &Heightmap, face: usize, rect: TexelRect) {
        if rect.x1 <= rect.x0 || rect.y1 <= rect.y0 {
            return;
        }
        let (m, n) = (self.m, map.face_size());
        // The texels of cell `c` along one axis that the mean reads.
        let lattice = |c: usize| {
            let (from, to) = (c * n / m, (c + 1) * n / m);
            let count = (to - from).min(4);
            (0..count).map(move |i| (from + (2 * i + 1) * (to - from) / (2 * count)) as i64)
        };
        let cells = |t0: usize, t1: usize| t0 * m / n..=((t1 - 1) * m / n).min(m - 1);
        for cy in cells(rect.y0, rect.y1) {
            for cx in cells(rect.x0, rect.x1) {
                let (mut sum, mut count) = (0u32, 0u32);
                for y in lattice(cy) {
                    for x in lattice(cx) {
                        sum += u32::from(map.get(face, x, y));
                        count += 1;
                    }
                }
                self.levels[(face * m + cy) * m + cx] = ((sum + count / 2) / count) as u16;
            }
        }
    }
}

/// The cells that the water flows on.
trait Grid {
    /// The 8 neighbors of a cell, in the order of `STEPS`. A neighbor past
    /// the end of the grid is `NONE`.
    fn neighbors(&self, i: usize) -> [u32; 8];
    /// A fixed number from 0 to 1 for neighbor `k` of cell `i`.
    fn scatter(&self, i: usize, k: usize) -> f64;
    /// The solid angle of a cell, in steradians.
    fn solid_angle(&self, i: usize) -> f64;
}

/// The cells of the six faces.
struct CubeGrid {
    m: usize,
    solid: Vec<f64>,
}

impl Grid for CubeGrid {
    fn neighbors(&self, i: usize) -> [u32; 8] {
        neighbors(self.m, i)
    }

    fn scatter(&self, i: usize, k: usize) -> f64 {
        scatter(i, k)
    }

    fn solid_angle(&self, i: usize) -> f64 {
        self.solid[i % (self.m * self.m)]
    }
}

/// The receiver of each land cell, and the solid angle that drains through
/// it.
///
/// The flood fills each pit to the level of its rim, from the ways out to
/// the land inside. Then each cell drains to a steep neighbor that is lower
/// in the filled land. A way out is an ocean cell or the end of the grid.
/// `inflow` gives water from outside the grid: a cell and a solid angle.
fn drain(grid: &impl Grid, levels: &[u16], inflow: &[(usize, f64)]) -> (Vec<u32>, Vec<f32>) {
    let count = levels.len();
    let sea = sea();
    let is_land = |i: usize| levels[i] >= sea;
    let mut receiver = vec![NONE; count];
    let lands = levels.iter().filter(|&&level| level >= sea).count();

    // The heap gives the lowest cell first. Of two cells at one level, it
    // gives first the cell that came in first. Thus a flat drains to the
    // nearest way out.
    let mut heap = BinaryHeap::new();
    let mut pushes = 0u32;
    let mut filled = levels.to_vec();
    for i in (0..count).filter(|&i| is_land(i)) {
        let near = grid.neighbors(i);
        let out = near.iter().find(|&&j| j == NONE || !is_land(j as usize));
        if let Some(&out) = out {
            receiver[i] = if out == NONE { OUT } else { out };
            heap.push(Reverse((levels[i], pushes, i as u32)));
            pushes += 1;
        }
    }
    // The land cells in the order that they leave the heap.
    let mut order: Vec<u32> = Vec::with_capacity(lands);
    let mut done = vec![false; count];
    while let Some(Reverse((level, _, i))) = heap.pop() {
        let i = i as usize;
        let mut best = 0.0;
        for (k, &j) in grid.neighbors(i).iter().enumerate() {
            let past_end = j == NONE;
            let j = j as usize;
            if !past_end && is_land(j) && !done[j] {
                // A cell that is not in the heap goes in. This cell takes
                // its water if the cell finds no lower neighbor.
                if receiver[j] == NONE {
                    receiver[j] = i as u32;
                    filled[j] = levels[j].max(level);
                    heap.push(Reverse((filled[j], pushes, j as u32)));
                    pushes += 1;
                }
                continue;
            }
            // The scatter lets a cell on an even slope or on a flat turn
            // to one side, so that the streams join. Without it they run
            // side by side. The ground past the end of the grid counts as
            // a flat.
            let below = if past_end { level } else { filled[j] };
            let drop = f64::from(level) - f64::from(below);
            // On a flat, each lower neighbor counts the same.
            let slope = 1.0 + if k < 4 { drop } else { drop * FRAC_1_SQRT_2 };
            let score = slope * (0.75 + 0.5 * grid.scatter(i, k));
            if score > best {
                best = score;
                receiver[i] = if past_end { OUT } else { j as u32 };
            }
        }
        done[i] = true;
        order.push(i as u32);
    }

    // Each receiver left the heap before its cells, so the reverse order
    // goes downstream.
    let mut area = vec![0.0f64; count];
    for &i in &order {
        area[i as usize] = grid.solid_angle(i as usize);
    }
    for &(i, solid) in inflow {
        if done[i] {
            area[i] += solid;
        }
    }
    for &i in order.iter().rev() {
        let to = receiver[i as usize];
        if to != OUT && is_land(to as usize) {
            area[to as usize] += area[i as usize];
        }
    }
    (receiver, area.iter().map(|&area| area as f32).collect())
}

/// The path of the water from each land cell to the sea. A cell below sea
/// level is ocean, and it has no flow.
pub struct FlowMap {
    m: usize,
    /// The cell that takes the water of each land cell.
    receiver: Vec<u32>,
    /// The solid angle that drains through each land cell, in steradians.
    area: Vec<f32>,
}

impl FlowMap {
    /// A world with no ocean or no land has no flow.
    pub fn new(heights: &CoarseHeights) -> FlowMap {
        let grid = CubeGrid {
            m: heights.m,
            solid: solid_angles(heights.m),
        };
        let (receiver, area) = drain(&grid, &heights.levels, &[]);
        FlowMap {
            m: heights.m,
            receiver,
            area,
        }
    }

    fn index(&self, face: usize, x: usize, y: usize) -> usize {
        (face * self.m + y) * self.m + x
    }

    /// The cell that takes the water of a land cell. The receiver of a cell
    /// at the coast is an ocean cell.
    pub fn receiver(&self, face: usize, x: usize, y: usize) -> Option<(usize, usize, usize)> {
        let to = self.receiver[self.index(face, x, y)];
        (to != NONE).then(|| split(self.m, to as usize))
    }

    /// The solid angle that drains through a cell, in steradians. The area
    /// of an ocean cell is 0.
    pub fn area(&self, face: usize, x: usize, y: usize) -> f64 {
        f64::from(self.area[self.index(face, x, y)])
    }

    pub fn is_river(&self, face: usize, x: usize, y: usize) -> bool {
        self.is_river_with(face, x, y, RIVER_MIN_CELLS)
    }

    /// The same as `is_river`, for rivers that drain `min_cells` cells or
    /// more. `RIVER_MIN_CELLS` cells have an area of `RIVER_MIN_AREA`.
    pub fn is_river_with(&self, face: usize, x: usize, y: usize, min_cells: f64) -> bool {
        Threshold::global(min_cells).holds(self.area(face, x, y))
    }

    /// The river cells as nodes, and the first ocean cell after each river.
    #[cfg(test)]
    fn rivers(&self) -> Rivers {
        self.rivers_with(RIVER_MIN_CELLS)
    }

    fn rivers_with(&self, min_cells: f64) -> Rivers {
        let dir = |i: usize| {
            let (face, x, y) = split(self.m, i);
            cell_dir(self.m, face, x, y)
        };
        let threshold = Threshold::global(min_cells);
        Rivers::new(&self.receiver, &self.area, threshold, dir, dir)
    }

    /// Draws each river into a channel map with 2 texels for each cell along
    /// one axis.
    pub fn channels(&self) -> ChannelMap {
        self.channels_with(RIVER_MIN_CELLS)
    }

    /// The same as `channels`, for rivers that drain `min_cells` cells or
    /// more.
    pub fn channels_with(&self, min_cells: f64) -> ChannelMap {
        let threshold = Threshold::global(min_cells);
        let mut map = ChannelMap::empty(2 * self.m);
        map.min_cells = threshold.min_cells;
        let mut rivers = self.rivers_with(min_cells);
        rivers.smooth();
        for (k, &cell) in rivers.cells.iter().enumerate() {
            let flow = threshold.flow_byte(f64::from(self.area[cell as usize]));
            map.draw(rivers.pos[k], rivers.pos[rivers.down[k] as usize], flow);
        }
        map
    }
}

/// The area that a cell must drain to be a river cell.
#[derive(Clone, Copy)]
struct Threshold {
    min_cells: f64,
    /// The area of `min_cells` cells, in steradians.
    area: f64,
}

impl Threshold {
    /// `area` gives the area of a number of cells. `min_cells` goes up to
    /// `RIVER_MIN_CELLS_LOWEST` if it is less.
    fn new(min_cells: f64, area: impl Fn(f64) -> f64) -> Threshold {
        let min_cells = min_cells.max(RIVER_MIN_CELLS_LOWEST);
        Threshold {
            min_cells,
            area: area(min_cells),
        }
    }

    /// The threshold of the full map, where `RIVER_MIN_CELLS` cells have an
    /// area of `RIVER_MIN_AREA`.
    fn global(min_cells: f64) -> Threshold {
        Threshold::new(min_cells, |cells| {
            RIVER_MIN_AREA * (cells / RIVER_MIN_CELLS)
        })
    }

    /// Whether a cell that drains `area` is a river cell. The cells of a
    /// grid do not have one area. At the lowest number, each cell that
    /// drains some land is a river cell, also a cell that is smaller than
    /// the others.
    fn holds(&self, area: f64) -> bool {
        if self.min_cells <= RIVER_MIN_CELLS_LOWEST {
            area > 0.0
        } else {
            area >= self.area
        }
    }

    fn flow_byte(&self, area: f64) -> u8 {
        size_byte(area / self.area)
    }
}

/// The nodes of all rivers. The first nodes are the river cells. The nodes
/// after them are the ocean cells where the rivers end.
#[derive(Default)]
struct Rivers {
    /// The cell of each river node.
    cells: Vec<u32>,
    /// The place of each node, a unit vector.
    pos: Vec<V3>,
    /// The node that takes the water of each river node.
    down: Vec<u32>,
    /// The upstream node with the largest area, for each river node.
    donor: Vec<u32>,
}

impl Rivers {
    /// Makes a node for each cell that drains enough for `threshold`. `place`
    /// gives the place of a cell. `outside` gives the place where the water
    /// of a cell goes past the end of the grid.
    fn new(
        receiver: &[u32],
        area: &[f32],
        threshold: Threshold,
        place: impl Fn(usize) -> V3,
        outside: impl Fn(usize) -> V3,
    ) -> Rivers {
        let mut rivers = Rivers::default();
        // The node of each river cell.
        let mut node = vec![NONE; area.len()];
        for i in (0..area.len()).filter(|&i| threshold.holds(f64::from(area[i]))) {
            node[i] = rivers.cells.len() as u32;
            rivers.cells.push(i as u32);
            rivers.pos.push(place(i));
        }
        let count = rivers.cells.len();
        rivers.down = vec![NONE; count];
        rivers.donor = vec![NONE; count];
        for k in 0..count {
            let cell = rivers.cells[k] as usize;
            let to = receiver[cell];
            if to == OUT || node[to as usize] == NONE {
                // The river ends here, in the ocean or at the end of the
                // grid. The end gets a node that has no cell.
                rivers.down[k] = rivers.pos.len() as u32;
                rivers.pos.push(if to == OUT {
                    outside(cell)
                } else {
                    place(to as usize)
                });
                continue;
            }
            let down = node[to as usize] as usize;
            rivers.down[k] = down as u32;
            let area = |k: usize| area[rivers.cells[k] as usize];
            let donor = rivers.donor[down];
            if donor == NONE || area(k) > area(donor as usize) {
                rivers.donor[down] = k as u32;
            }
        }
        rivers
    }

    /// Moves each node toward the nodes before and after it, on the sphere.
    /// The first node and the last node of a river stay in place.
    fn smooth(&mut self) {
        self.smooth_with(normalize);
    }

    /// The same as `smooth`, for nodes on a plane.
    fn smooth_flat(&mut self) {
        self.smooth_with(|sum| scale(sum, 0.25));
    }

    /// `fit` makes a node from the sum of 4 nodes.
    fn smooth_with(&mut self, fit: impl Fn(V3) -> V3) {
        for _ in 0..SMOOTH_PASSES {
            let old = self.pos.clone();
            for k in 0..self.cells.len() {
                if self.donor[k] == NONE {
                    continue;
                }
                let ends = add(old[self.down[k] as usize], old[self.donor[k] as usize]);
                self.pos[k] = fit(add(scale(old[k], 2.0), ends));
            }
        }
    }
}

#[cfg(test)]
fn flow_byte(area: f64) -> u8 {
    size_byte(area / RIVER_MIN_AREA)
}

/// The flow byte of a river that drains `ratio` times the smallest area of a
/// river.
fn size_byte(ratio: f64) -> u8 {
    (255.0 * ratio.log2() / RIVER_OCTAVES)
        .round()
        .clamp(1.0, 255.0) as u8
}

/// The distance from a river of a size to the last texels that it writes, in
/// channel texels. The carve mode reads the 4 texels around a place in the
/// valley, so the reach is more than the widest valley of the river.
fn reach(flow: u8) -> f64 {
    let widest = (1.5 + 4.5 * f64::from(flow) / 255.0) * VALLEY_MAX;
    (widest + 1.5).min(CHANNEL_REACH)
}

/// Draws a straight part of a river from `a` to `b` into a square of channel
/// texels with `size` texels along one side. The part writes the texels in
/// its reach. A texel keeps the nearest river that writes it.
fn draw_line(data: &mut [u8], size: usize, a: (f64, f64), b: (f64, f64), flow: u8) {
    let ((ax, ay), (bx, by)) = (a, b);
    let last = (size - 1) as f64;
    let reach = reach(flow);
    let from = |a: f64, b: f64| (a.min(b) - reach).ceil().max(0.0);
    let to = |a: f64, b: f64| (a.max(b) + reach).floor().min(last);
    let (x0, x1, y0, y1) = (from(ax, bx), to(ax, bx), from(ay, by), to(ay, by));
    if x0 > x1 || y0 > y1 {
        return;
    }
    let (dx, dy) = (bx - ax, by - ay);
    let length_sq = dx * dx + dy * dy;
    for y in y0 as usize..=y1 as usize {
        for x in x0 as usize..=x1 as usize {
            let (px, py) = (x as f64 - ax, y as f64 - ay);
            let t = if length_sq > 0.0 {
                ((px * dx + py * dy) / length_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (qx, qy) = (px - t * dx, py - t * dy);
            let distance_sq = qx * qx + qy * qy;
            if distance_sq > reach * reach {
                continue;
            }
            let byte = (distance_sq.sqrt() * CHANNEL_SCALE).round();
            if byte >= 255.0 {
                continue;
            }
            let byte = byte as u8;
            let at = (y * size + x) * 2;
            let old = &mut data[at..at + 2];
            if byte < old[0] || (byte == old[0] && flow > old[1]) {
                old.copy_from_slice(&[byte, flow]);
            }
        }
    }
}

/// The blend of 4 values around a place. `tx` and `ty` are the place from
/// the first value to the others, from 0 to 1. `stamp.wgsl` has the same
/// steps.
fn blend(values: [f64; 4], tx: f64, ty: f64) -> f64 {
    let [a, b, c, d] = values;
    let (top, bottom) = (a * (1.0 - tx) + b * tx, c * (1.0 - tx) + d * tx);
    top * (1.0 - ty) + bottom * ty
}

/// The rivers as a texture. Each texel has two bytes. The first byte is the
/// distance to the nearest river in channel texels, times `CHANNEL_SCALE`.
/// The second byte is the size of that river from 1 to 255, or 0 if no river
/// is near. A texel with no river near holds 255 and 0. A large river is
/// near at a larger distance than a small river.
pub struct ChannelMap {
    size: usize,
    min_cells: f64,
    data: Vec<u8>,
}

impl ChannelMap {
    /// A map with no rivers, with `size` by `size` texels on each face.
    pub fn empty(size: usize) -> ChannelMap {
        assert!(size > 0, "the size must not be zero");
        ChannelMap {
            size,
            min_cells: RIVER_MIN_CELLS,
            data: [255, 0].repeat(FACES * size * size),
        }
    }

    /// The size for a heightmap with `face_size` texels along one side of a
    /// face.
    pub fn size_for(face_size: usize) -> usize {
        2 * CoarseHeights::size_for(face_size)
    }

    /// The number of texels along one side of a face.
    pub fn size(&self) -> usize {
        self.size
    }

    /// The number of cells that a river cell of this map drains at least. A
    /// river that drains this number has a size byte of 1.
    pub fn min_cells(&self) -> f64 {
        self.min_cells
    }

    /// The two bytes of each texel, face by face, in row order.
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn texel(&self, face: usize, x: usize, y: usize) -> [u8; 2] {
        let at = ((face * self.size + y) * self.size + x) * 2;
        [self.data[at], self.data[at + 1]]
    }

    /// The distance to the nearest river in channel texels, and the size of
    /// the river. `u` and `v` are in channel texels, and a texel center is at
    /// a whole number. The distance is a blend of the 4 texels around the
    /// place. The size is the largest of the 4. `stamp.wgsl` has the same
    /// steps.
    pub fn sample(&self, face: usize, u: f64, v: f64) -> (f64, u8) {
        let last = (self.size - 1) as f64;
        let (x0, y0) = (u.floor(), v.floor());
        let (tx, ty) = (u - x0, v - y0);
        let at = |x: f64, y: f64| {
            self.texel(
                face,
                x.clamp(0.0, last) as usize,
                y.clamp(0.0, last) as usize,
            )
        };
        let (a, b) = (at(x0, y0), at(x0 + 1.0, y0));
        let (c, d) = (at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
        let distances = [a, b, c, d].map(|texel| f64::from(texel[0]));
        let distance = blend(distances, tx, ty) / CHANNEL_SCALE;
        (distance, a[1].max(b[1]).max(c[1]).max(d[1]))
    }

    /// The same as `sample`, at a direction.
    pub fn at(&self, d: V3) -> (f64, u8) {
        let (face, a, b) = face_of(d);
        let texel = |a: f64| (warp(a) + 1.0) * 0.5 * self.size as f64 - 0.5;
        self.sample(face, texel(a), texel(b))
    }

    pub fn has_rivers(&self) -> bool {
        self.data
            .as_chunks::<2>()
            .0
            .iter()
            .any(|texel| texel[1] > 0)
    }

    /// Draws one part of a river from `a` to `b` on each face that it is
    /// near. On a face, the part is a straight line in the texel grid, and
    /// the grid continues past the face edges.
    fn draw(&mut self, a: V3, b: V3, flow: u8) {
        let k = self.size as f64;
        // The largest flat coordinate of an end of a part that can reach a
        // texel of the face. A part is less than 4 texels long.
        let past = 2.0 * (CHANNEL_REACH + 4.0) / k;
        let limit = unwarp((1.0 + past).min(1.9));
        for face in 0..FACES {
            let axis = face / 2;
            let sign = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
            let project = |d: V3| {
                let depth = d[axis] * sign;
                let (u, v) = (d[(axis + 1) % 3], d[(axis + 2) % 3]);
                if depth <= 0.0 || u.abs() > limit * depth || v.abs() > limit * depth {
                    return None;
                }
                let texel = |a: f64| (warp(a / depth) + 1.0) * 0.5 * k - 0.5;
                Some((texel(u), texel(v)))
            };
            let (Some((ax, ay)), Some((bx, by))) = (project(a), project(b)) else {
                continue;
            };
            let texels = self.size * self.size * 2;
            let data = &mut self.data[face * texels..(face + 1) * texels];
            draw_line(data, self.size, (ax, ay), (bx, by), flow);
        }
    }
}

/// The channel map of the rivers on `heights`.
pub fn channel_map(heights: &CoarseHeights) -> ChannelMap {
    FlowMap::new(heights).channels()
}

/// A square of cells on the texel grid of one face. The grid continues past
/// the face edges, as it does in `Heightmap::get`, so the window can cover a
/// part of the next face. A cell is at a fixed place on the sphere: two
/// windows on one face with one cell size share the cells where they overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// The face that gives the texel grid.
    pub face: usize,
    /// The first texel of the window on that face. Each number is a multiple
    /// of `cell`. It can be less than 0, and the window can end past the face
    /// edge, by less than half a face.
    pub x0: i64,
    pub y0: i64,
    /// The number of texels along one side of a cell, a power of two.
    pub cell: usize,
    /// The number of cells along one side of the window.
    pub cells: usize,
    /// The number of texels along one side of a face of the heightmap.
    pub face_size: usize,
}

impl Window {
    /// The window around a direction, on the face of that direction.
    pub fn centered(d: V3, face_size: usize, cell: usize, cells: usize) -> Window {
        assert!(cell.is_power_of_two() && cells > 0, "the window sizes");
        let (face, a, b) = face_of(d);
        let half = (cells / 2) as i64;
        let first = |a: f64| {
            let texel = (warp(a) + 1.0) * 0.5 * face_size as f64;
            ((texel / cell as f64).floor() as i64 - half) * cell as i64
        };
        Window {
            face,
            x0: first(a),
            y0: first(b),
            cell,
            cells,
            face_size,
        }
    }

    /// The equal-angle coordinate of a place along one axis. `p` is in cells
    /// from the first cell, and `first` is `x0` or `y0`.
    fn angle_at(&self, first: i64, p: f64) -> f64 {
        (first as f64 + p * self.cell as f64) / self.face_size as f64 * 2.0 - 1.0
    }

    /// The direction of a place in the window. `u` and `v` are in cells, and
    /// the center of the first cell is at 0.5.
    pub fn dir(&self, u: f64, v: f64) -> V3 {
        let flat = |first: i64, p: f64| unwarp(self.angle_at(first, p).clamp(-1.99, 1.99));
        face_dir(self.face, flat(self.x0, u), flat(self.y0, v))
    }

    /// The direction of the center of a cell.
    pub fn cell_dir(&self, x: usize, y: usize) -> V3 {
        self.dir(x as f64 + 0.5, y as f64 + 0.5)
    }

    /// The place of a direction in cells. The inverse of `dir`. The place can
    /// be outside the window. A direction too far from the face has no place.
    pub fn place(&self, d: V3) -> Option<(f64, f64)> {
        let axis = self.face / 2;
        let sign = if self.face.is_multiple_of(2) {
            1.0
        } else {
            -1.0
        };
        let depth = d[axis] * sign;
        if depth <= 0.2 {
            return None;
        }
        let cells = |a: f64, first: i64| {
            let texel = (warp(a / depth) + 1.0) * 0.5 * self.face_size as f64;
            (texel - first as f64) / self.cell as f64
        };
        Some((
            cells(d[(axis + 1) % 3], self.x0),
            cells(d[(axis + 2) % 3], self.y0),
        ))
    }

    /// The cell that holds a direction.
    pub fn cell_at(&self, d: V3) -> Option<(usize, usize)> {
        let (u, v) = self.place(d)?;
        let inside = |p: f64| p >= 0.0 && p < self.cells as f64;
        (inside(u) && inside(v)).then_some((u as usize, v as usize))
    }

    /// The solid angle of a cell, in steradians.
    pub fn cell_solid_angle(&self, x: usize, y: usize) -> f64 {
        let flat = |first: i64, i: usize| unwarp(self.angle_at(first, i as f64 + 0.5));
        let (fu, fv) = (flat(self.x0, x), flat(self.y0, y));
        let step =
            |f: f64| (1.0 + f * f) * FRAC_PI_4 * 2.0 * self.cell as f64 / self.face_size as f64;
        let r = 1.0 + fu * fu + fv * fv;
        step(fu) * step(fv) / (r * r.sqrt())
    }

    /// The number of channel texels along one side of the window.
    pub fn channels(&self) -> usize {
        2 * self.cells
    }

    /// The place of a heightmap texel of the window face in channel texels.
    /// A texel center of the channels is at a whole number. The result is
    /// exact.
    pub fn channel_of_texel(&self, x: i64, y: i64) -> (f64, f64) {
        let at = |x: i64, first: i64| ((x - first) as f64 + 0.5) * 2.0 / self.cell as f64 - 0.5;
        (at(x, self.x0), at(y, self.y0))
    }

    /// The place of a direction in channel texels.
    pub fn channel_of_dir(&self, d: V3) -> Option<(f64, f64)> {
        let (u, v) = self.place(d)?;
        Some((2.0 * u - 0.5, 2.0 * v - 0.5))
    }

    /// Whether a place in channel texels is far enough from the sides of the
    /// window for a sure distance to the nearest river.
    pub fn covers(&self, u: f64, v: f64) -> bool {
        let last = (self.channels() - 1) as f64 - WINDOW_MARGIN;
        u >= WINDOW_MARGIN && v >= WINDOW_MARGIN && u <= last && v <= last
    }
}

impl Grid for Window {
    fn neighbors(&self, i: usize) -> [u32; 8] {
        let n = self.cells as i64;
        let (x, y) = ((i % self.cells) as i64, (i / self.cells) as i64);
        STEPS.map(|(dx, dy)| {
            let (x, y) = (x + dx, y + dy);
            if x < 0 || y < 0 || x >= n || y >= n {
                return NONE;
            }
            (y * n + x) as u32
        })
    }

    /// The key is the place of the cell on the face, so a cell has one
    /// pattern in each window that holds it.
    fn scatter(&self, i: usize, k: usize) -> f64 {
        let cell = self.cell as i64;
        let x = self.x0.div_euclid(cell) + (i % self.cells) as i64;
        let y = self.y0.div_euclid(cell) + (i / self.cells) as i64;
        let grid = (self.face as u32) << 8 | self.cell.trailing_zeros();
        let key = mix(mix(mix(grid) ^ x as u32) ^ y as u32);
        scatter(key as usize, k)
    }

    fn solid_angle(&self, i: usize) -> f64 {
        self.cell_solid_angle(i % self.cells, i / self.cells)
    }
}

/// The levels of the cells of a window.
#[derive(Clone)]
pub struct WindowHeights {
    window: Window,
    /// The cells in row order.
    levels: Vec<u16>,
}

impl WindowHeights {
    /// Reads the heightmap. A cell of more than one texel holds the mean of
    /// 4 of its texels.
    pub fn new(map: &Heightmap, window: Window) -> WindowHeights {
        assert_eq!(map.face_size(), window.face_size, "the face size");
        let cell = window.cell as i64;
        let face = window.face;
        let mut levels = Vec::with_capacity(window.cells * window.cells);
        for cy in 0..window.cells as i64 {
            for cx in 0..window.cells as i64 {
                let (x, y) = (window.x0 + cx * cell, window.y0 + cy * cell);
                levels.push(if cell == 1 {
                    map.get(face, x, y)
                } else {
                    let (near, far) = (cell / 4, cell * 3 / 4);
                    let at = |dx: i64, dy: i64| u32::from(map.get(face, x + dx, y + dy));
                    let sum = at(near, near) + at(far, near) + at(near, far) + at(far, far);
                    ((sum + 2) / 4) as u16
                });
            }
        }
        WindowHeights { window, levels }
    }

    /// `level` gives the level at the center of each cell.
    pub fn from_fn(window: Window, level: impl Fn(V3) -> u16) -> WindowHeights {
        let cells = 0..window.cells * window.cells;
        let levels = cells.map(|i| level(window.cell_dir(i % window.cells, i / window.cells)));
        WindowHeights {
            window,
            levels: levels.collect(),
        }
    }

    pub fn window(&self) -> Window {
        self.window
    }

    pub fn level(&self, x: usize, y: usize) -> u16 {
        self.levels[y * self.window.cells + x]
    }
}

/// The path of the water in a window. The water of a land cell at a side of
/// the window can leave the window.
struct WindowFlow {
    window: Window,
    receiver: Vec<u32>,
    area: Vec<f32>,
    threshold: Threshold,
}

impl WindowFlow {
    /// `global` gives the rivers that come into the window from outside.
    #[cfg(test)]
    fn new(heights: &WindowHeights, global: Option<&FlowMap>) -> WindowFlow {
        WindowFlow::with(heights, global, RIVER_MIN_CELLS)
    }

    /// The same as `new`, for rivers that drain `min_cells` cells or more.
    fn with(heights: &WindowHeights, global: Option<&FlowMap>, min_cells: f64) -> WindowFlow {
        let window = heights.window;
        let outside = Threshold::global(min_cells);
        let mut inflow = Vec::new();
        if let Some(global) = global {
            let m = global.m;
            // The window cell that holds the center of a cell of `global`.
            let inside = |i: usize| {
                let (face, x, y) = split(m, i);
                let (x, y) = window.cell_at(cell_dir(m, face, x, y))?;
                Some(y * window.cells + x)
            };
            // A river that comes in keeps the area that it drains outside.
            for (i, &area) in global.area.iter().enumerate() {
                let to = global.receiver[i];
                if !outside.holds(f64::from(area)) || to == NONE || inside(i).is_some() {
                    continue;
                }
                if let Some(cell) = inside(to as usize) {
                    inflow.push((cell, f64::from(area)));
                }
            }
        }
        let (receiver, area) = drain(&window, &heights.levels, &inflow);
        let count = heights.levels.len();
        let solid: f64 = (0..count).map(|i| window.solid_angle(i)).sum();
        WindowFlow {
            window,
            receiver,
            area,
            threshold: Threshold::new(min_cells, |cells| cells * solid / count as f64),
        }
    }

    /// The river cells as nodes on the plane of the window, in cells.
    fn rivers(&self) -> Rivers {
        let cells = self.window.cells;
        let place = |i: usize| [(i % cells) as f64 + 0.5, (i / cells) as f64 + 0.5, 0.0];
        // The place one cell past each side that the cell is at.
        let outside = |i: usize| {
            let past = |c: usize| match c {
                0 => -1.0,
                c if c == cells - 1 => 1.0,
                _ => 0.0,
            };
            let [x, y, _] = place(i);
            [x + past(i % cells), y + past(i / cells), 0.0]
        };
        Rivers::new(&self.receiver, &self.area, self.threshold, place, outside)
    }

    fn channels(&self) -> ChannelWindow {
        let size = self.window.channels();
        let mut data = [255, 0].repeat(size * size);
        let mut rivers = self.rivers();
        rivers.smooth_flat();
        // A cell is 2 channel texels wide.
        let texel = |p: V3| (2.0 * p[0] - 0.5, 2.0 * p[1] - 0.5);
        for (k, &cell) in rivers.cells.iter().enumerate() {
            let flow = self
                .threshold
                .flow_byte(f64::from(self.area[cell as usize]));
            let (a, b) = (rivers.pos[k], rivers.pos[rivers.down[k] as usize]);
            draw_line(&mut data, size, texel(a), texel(b), flow);
        }
        ChannelWindow {
            window: self.window,
            min_cells: self.threshold.min_cells,
            data,
        }
    }
}

/// The rivers of a window as a texture, with 2 texels for each cell along
/// one axis. A texel has the two bytes of a `ChannelMap` texel. The distance
/// is in channel texels of the window.
pub struct ChannelWindow {
    window: Window,
    min_cells: f64,
    data: Vec<u8>,
}

impl ChannelWindow {
    pub fn window(&self) -> Window {
        self.window
    }

    /// The number of texels along one side.
    pub fn size(&self) -> usize {
        self.window.channels()
    }

    /// The number of cells that a river cell of this window drains at least.
    pub fn min_cells(&self) -> f64 {
        self.min_cells
    }

    /// The two bytes of each texel, in row order.
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn texel(&self, x: usize, y: usize) -> [u8; 2] {
        let at = (y * self.size() + x) * 2;
        [self.data[at], self.data[at + 1]]
    }

    /// The distance to the nearest river in channel texels, and the size of
    /// the river from 0 to 255. `u` and `v` are in channel texels, and a
    /// texel center is at a whole number. Each result is a blend of the 4
    /// texels around the place, so a small change of the place gives a small
    /// change of the result. `stamp.wgsl` has the same steps.
    pub fn sample(&self, u: f64, v: f64) -> (f64, f64) {
        let last = (self.size() - 1) as f64;
        let (x0, y0) = (u.floor(), v.floor());
        let (tx, ty) = (u - x0, v - y0);
        let at =
            |x: f64, y: f64| self.texel(x.clamp(0.0, last) as usize, y.clamp(0.0, last) as usize);
        let texels = [
            at(x0, y0),
            at(x0 + 1.0, y0),
            at(x0, y0 + 1.0),
            at(x0 + 1.0, y0 + 1.0),
        ];
        let byte = |i: usize| blend(texels.map(|texel| f64::from(texel[i])), tx, ty);
        (byte(0) / CHANNEL_SCALE, byte(1))
    }

    /// The same as `sample`, at a place that `Window::covers`.
    pub fn lookup(&self, u: f64, v: f64) -> Option<(f64, f64)> {
        self.window.covers(u, v).then(|| self.sample(u, v))
    }

    /// The same as `lookup`, at a direction.
    pub fn at(&self, d: V3) -> Option<(f64, f64)> {
        let (u, v) = self.window.channel_of_dir(d)?;
        self.lookup(u, v)
    }

    pub fn has_rivers(&self) -> bool {
        let texels = self.data.as_chunks::<2>().0;
        texels.iter().any(|texel| texel[1] > 0)
    }
}

/// The channels of the rivers in a window. A river of `global` that comes
/// into the window keeps its size.
pub fn window_channels(heights: &WindowHeights, global: &FlowMap) -> ChannelWindow {
    window_channels_with(heights, global, RIVER_MIN_CELLS)
}

/// The same as `window_channels`, for rivers that drain `min_cells` cells or
/// more, in the window and in `global`.
pub fn window_channels_with(
    heights: &WindowHeights,
    global: &FlowMap,
    min_cells: f64,
) -> ChannelWindow {
    WindowFlow::with(heights, Some(global), min_cells).channels()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::level_to_meters;
    use crate::math::{angle, dir_to_lonlat, lonlat_to_dir};

    const OCEAN: f64 = -1000.0;

    /// Heights from a function of the cell. The function gives meters.
    fn from_cells(m: usize, meters: impl Fn(usize, usize, usize) -> f64) -> CoarseHeights {
        CoarseHeights::from_fn(m, |d| {
            let (face, x, y) = cell_at(m, d);
            meters_to_level(meters(face, x, y))
        })
    }

    fn is_ocean(heights: &CoarseHeights, cell: (usize, usize, usize)) -> bool {
        heights.level(cell.0, cell.1, cell.2) < sea()
    }

    fn cells(m: usize) -> impl Iterator<Item = (usize, usize, usize)> {
        (0..FACES * m * m).map(move |i| split(m, i))
    }

    /// The cells from `start` down to the first ocean cell.
    fn path(flow: &FlowMap, start: (usize, usize, usize)) -> Vec<(usize, usize, usize)> {
        let mut path = vec![start];
        while let Some(next) = {
            let (face, x, y) = path[path.len() - 1];
            flow.receiver(face, x, y)
        } {
            path.push(next);
            assert!(path.len() <= flow.area.len(), "the receivers make a loop");
        }
        path
    }

    /// Land with hills and pits on about a third of the sphere.
    fn lumpy(d: V3) -> u16 {
        let wave = |k: f64, p: f64| (k * d[0] + p).sin() * (k * d[1] - p).cos() * (k * d[2]).cos();
        let meters =
            -250.0 + 1800.0 * wave(3.0, 0.4) + 900.0 * wave(7.0, 1.3) + 400.0 * wave(17.0, 2.1);
        meters_to_level(meters)
    }

    /// A strip of land on face 0, 7 cells wide. The middle row is a valley
    /// that goes down to the cell with the lowest `x`. `middle` gives the
    /// middle row for each `x`.
    fn valley(m: usize, middle: impl Fn(usize) -> f64) -> CoarseHeights {
        from_cells(m, |face, x, y| {
            let across = (y as f64 - middle(x)).abs();
            if face != 0 || !(8..m - 8).contains(&x) || across > 3.4 {
                return OCEAN;
            }
            100.0 + 20.0 * x as f64 + 300.0 * across
        })
    }

    #[test]
    fn coarse_follows_a_changed_rect() {
        assert_eq!(CoarseHeights::size_for(8192), 512);
        assert_eq!(CoarseHeights::size_for(1024), 512);
        assert_eq!(CoarseHeights::size_for(256), 256);
        assert_eq!(ChannelMap::size_for(8192), 1024);

        let mut map = Heightmap::new(2048, 100);
        let mut heights = CoarseHeights::new(&map);
        assert_eq!(heights.size(), 512);
        assert!(heights.levels.iter().all(|&level| level == 100));

        // The rectangle covers cells 100 to 119 and 200 to 219 on face 3.
        let rect = TexelRect {
            x0: 400,
            y0: 800,
            x1: 480,
            y1: 880,
        };
        map.store_rect(3, rect, &vec![9000; 80 * 80]);
        heights.update(&map, 3, rect);
        for (face, x, y) in cells(512) {
            let inside = face == 3 && (100..120).contains(&x) && (200..220).contains(&y);
            let expected = if inside { 9000 } else { 100 };
            assert_eq!(heights.level(face, x, y), expected, "{face} {x} {y}");
        }
        assert!(CoarseHeights::new(&map).levels == heights.levels);

        // A cell with a part of its texels changed holds a mean.
        map.set(3, 401, 801, 100);
        heights.update(&map, 3, rect);
        let mean = (9000 * 15 + 100 + 8) / 16;
        assert_eq!(u32::from(heights.level(3, 100, 200)), mean);
    }

    #[test]
    fn neighbours_cross_each_face_edge() {
        let m = 32;
        let step = std::f64::consts::FRAC_PI_2 / m as f64;
        for i in 0..FACES * m * m {
            let (face, x, y) = split(m, i);
            let near = neighbors(m, i);
            let here = cell_dir(m, face, x, y);
            for &j in &near {
                let (f, a, b) = split(m, j as usize);
                assert_ne!(j as usize, i);
                assert!(angle(here, cell_dir(m, f, a, b)) < 2.0 * step, "{i} {j}");
            }
            let near_corner = (x < 2 || x >= m - 2) && (y < 2 || y >= m - 2);
            if near_corner {
                continue;
            }
            for &j in &near {
                assert!(neighbors(m, j as usize).contains(&(i as u32)), "{i} {j}");
            }
            let on_edge = x == 0 || y == 0 || x == m - 1 || y == m - 1;
            let past = near.iter().filter(|&&j| j as usize / (m * m) != face);
            assert_eq!(past.count(), if on_edge { 3 } else { 0 });
        }
    }

    #[test]
    fn a_cone_drains_outward() {
        let m = 48;
        // The cone covers parts of 3 faces.
        let peak = lonlat_to_dir(38.0, 28.0);
        let heights = CoarseHeights::from_fn(m, |d| {
            let from_peak = angle(d, peak);
            meters_to_level(if from_peak < 0.6 {
                3100.0 - 5000.0 * from_peak
            } else {
                OCEAN
            })
        });
        let flow = FlowMap::new(&heights);
        let mut faces = [false; FACES];
        for (face, x, y) in cells(m) {
            let Some((f, a, b)) = flow.receiver(face, x, y) else {
                assert!(is_ocean(&heights, (face, x, y)));
                continue;
            };
            faces[face] = true;
            let here = angle(cell_dir(m, face, x, y), peak);
            assert!(angle(cell_dir(m, f, a, b), peak) > here, "{face} {x} {y}");
        }
        assert_eq!(faces.iter().filter(|&&land| land).count(), 3);
    }

    #[test]
    fn the_drained_area_sums_to_the_land_area() {
        let m = 48;
        let heights = CoarseHeights::from_fn(m, lumpy);
        let flow = FlowMap::new(&heights);
        let solid = solid_angles(m);
        let (mut land, mut drained) = (0.0, 0.0);
        for (face, x, y) in cells(m) {
            if is_ocean(&heights, (face, x, y)) {
                assert_eq!(flow.area(face, x, y), 0.0);
                continue;
            }
            land += solid[y * m + x];
            let to = flow
                .receiver(face, x, y)
                .expect("a land cell has a receiver");
            if is_ocean(&heights, to) {
                drained += flow.area(face, x, y);
            }
        }
        let sphere = 4.0 * std::f64::consts::PI;
        assert!(land > 0.2 * sphere && land < 0.5 * sphere, "{land}");
        assert!((drained - land).abs() / land < 1e-3, "{drained} {land}");
    }

    #[test]
    fn a_basin_drains_to_the_sea() {
        let m = 48;
        // An island with a pit in the middle, 2000 m below the rim.
        let middle = lonlat_to_dir(-20.0, 10.0);
        let heights = CoarseHeights::from_fn(m, |d| {
            let r = angle(d, middle);
            meters_to_level(if r < 0.5 {
                2500.0 - 8000.0 * (r - 0.25).abs()
            } else {
                OCEAN
            })
        });
        let pit = cell_at(m, middle);
        assert!(level_to_meters(heights.level(pit.0, pit.1, pit.2)) < 800.0);
        let flow = FlowMap::new(&heights);
        for cell in cells(m).filter(|&cell| !is_ocean(&heights, cell)) {
            let path = path(&flow, cell);
            assert!(is_ocean(&heights, path[path.len() - 1]), "{cell:?}");
        }
        // The water of the land inside the rim leaves at one place.
        let most = cells(m).map(|(face, x, y)| flow.area(face, x, y));
        assert!(most.fold(0.0, f64::max) > 0.15);
        for heights in [CoarseHeights::from_fn(m, lumpy), valley(m, |_| 24.0)] {
            let flow = FlowMap::new(&heights);
            for cell in cells(m).filter(|&cell| !is_ocean(&heights, cell)) {
                let path = path(&flow, cell);
                assert!(is_ocean(&heights, path[path.len() - 1]), "{cell:?}");
            }
        }
    }

    #[test]
    fn a_world_with_no_sea_has_no_rivers() {
        let m = 32;
        let hills = |d: V3| meters_to_level(2000.0 + 1500.0 * (5.0 * d[0]).sin() * d[2]);
        for heights in [
            CoarseHeights::from_fn(m, hills),
            CoarseHeights::from_fn(m, |_| meters_to_level(OCEAN)),
        ] {
            let flow = FlowMap::new(&heights);
            for (face, x, y) in cells(m) {
                assert!(!flow.is_river(face, x, y));
                assert_eq!(flow.receiver(face, x, y), None);
            }
            let channels = channel_map(&heights);
            assert_eq!(channels.size(), 2 * m);
            assert!(!channels.has_rivers());
            assert!(channels.bytes() == ChannelMap::empty(2 * m).bytes());
        }
    }

    #[test]
    fn a_river_crosses_a_face_edge() {
        let m = 128;
        // A valley along the equator from face 2 down to face 0. The faces
        // meet at longitude 45.
        let heights = CoarseHeights::from_fn(m, |d| {
            let (lon, lat) = dir_to_lonlat(d);
            let across = (lat - 0.4).abs();
            meters_to_level(if lon > 10.0 && lon < 80.0 && across < 3.0 {
                100.0 + 20.0 * (lon - 10.0) + 300.0 * across
            } else {
                OCEAN
            })
        });
        let flow = FlowMap::new(&heights);
        let start = cell_at(m, lonlat_to_dir(75.0, 0.4));
        let path = path(&flow, start);
        assert_eq!(start.0, 2);
        assert_eq!(path[path.len() - 1].0, 0);
        assert!(is_ocean(&heights, path[path.len() - 1]));
        let changes = path.windows(2).filter(|pair| pair[0].0 != pair[1].0);
        assert_eq!(changes.count(), 1);
        let land = &path[..path.len() - 1];
        assert!(land.iter().all(|&(face, x, y)| flow.is_river(face, x, y)));

        // The texel `(k - 1, j)` of face 0 touches the texel `(j, k - 1)` of
        // face 2.
        let channels = flow.channels();
        let k = channels.size();
        let mut on_river = 0;
        for j in k / 2 - 12..k / 2 + 12 {
            let (a, b) = (channels.texel(0, k - 1, j), channels.texel(2, j, k - 1));
            // Past the reach of a small river, a texel holds a large river
            // that is not so near.
            if a[0].min(b[0]) < 64 {
                assert!(a[0].abs_diff(b[0]) <= 32, "row {j}: {a:?} {b:?}");
            }
            if a[0] < 32 && b[0] < 32 {
                assert!(a[1] > 0 && b[1] > 0);
                on_river += 1;
            }
        }
        assert!(on_river > 0);
        let mid = channels.texel(0, k - 1, k / 2);
        assert!(mid[0] < 32 && channels.texel(2, k / 2, k - 1)[0] < 32);
    }

    /// The only river of a straight valley is its middle row. The side rows
    /// are too short.
    fn straight(m: usize) -> (CoarseHeights, FlowMap) {
        let heights = valley(m, |_| (m / 2) as f64);
        let flow = FlowMap::new(&heights);
        for (face, x, y) in cells(m) {
            let on_river = face == 0 && y == m / 2 && (8..m - 9).contains(&x);
            assert_eq!(flow.is_river(face, x, y), on_river, "{face} {x} {y}");
        }
        (heights, flow)
    }

    #[test]
    fn the_distance_grows_away_from_a_river() {
        let m = 128;
        let channels = straight(m).1.channels();
        assert!(channels.has_rivers());
        // The center of cell row `m / 2` is between texel rows `m` and
        // `m + 1`.
        for x in [40, 128, 200] {
            for up in [false, true] {
                for j in 0..12 {
                    let y = if up { m + 1 + j } else { m - j };
                    let [distance, flow] = channels.texel(0, x, y);
                    let from_river = j as f64 + 0.5;
                    // The river at this place writes the texels in its reach.
                    let reach = reach(channels.texel(0, x, m)[1]);
                    if from_river < reach - 0.5 {
                        let expected = from_river * CHANNEL_SCALE;
                        assert!((f64::from(distance) - expected).abs() <= 2.0, "{x} {j}");
                        assert!(flow > 0);
                    } else if from_river > reach + 0.5 {
                        assert_eq!([distance, flow], [255, 0], "{x} {j}");
                    }
                }
            }
        }
        // `sample` blends the distance bytes.
        let (near, flow) = channels.sample(0, 128.0, m as f64 + 0.5);
        assert!((near - 0.5).abs() < 0.1 && flow > 0, "{near}");
        let (far, _) = channels.sample(0, 128.0, m as f64 + 3.5);
        assert!((far - 3.0).abs() < 0.1, "{far}");
        let (on_edge, _) = channels.sample(0, -3.0, -3.0);
        assert_eq!(
            on_edge,
            f64::from(channels.texel(0, 0, 0)[0]) / CHANNEL_SCALE
        );
        let d = cell_dir(2 * m, 0, 128, m + 2);
        assert_eq!(
            channels.at(d).0,
            f64::from(channels.texel(0, 128, m + 2)[0]) / 32.0
        );
    }

    #[test]
    fn flow_byte_grows_downstream() {
        let m = 128;
        let (_, flow) = straight(m);
        let channels = flow.channels();
        let bytes: Vec<u8> = (20..2 * m - 20)
            .map(|x| channels.texel(0, x, m)[1])
            .collect();
        assert!(bytes.iter().all(|&byte| byte > 0));
        // The water goes to the low `x` end.
        assert!(bytes.windows(2).all(|pair| pair[0] >= pair[1]), "{bytes:?}");
        assert!(bytes[0] > bytes[bytes.len() - 1] + 40, "{bytes:?}");
        // The byte follows the area of the cell above the texel.
        let area = flow.area(0, 30, m / 2);
        assert_eq!(flow_byte(RIVER_MIN_AREA), 1);
        assert_eq!(flow_byte(RIVER_MIN_AREA * 64.0), 128);
        assert_eq!(flow_byte(1.0e3), 255);
        assert!(channels.texel(0, 60, m)[1].abs_diff(flow_byte(area)) <= 3);
    }

    #[test]
    fn smoothing_keeps_the_outlet_and_the_source() {
        let m = 128;
        // The valley has a step to the next row at each third cell.
        let heights = valley(m, |x| 40.0 + (x as f64 / 3.0).floor());
        let flow = FlowMap::new(&heights);
        let mut rivers = flow.rivers();
        let before = rivers.pos.clone();
        rivers.smooth();
        let count = rivers.cells.len();
        assert!(count > 50);
        let step = std::f64::consts::FRAC_PI_2 / m as f64;
        let (mut sources, mut outlets, mut moved) = (0, 0, 0);
        for (k, &old) in before.iter().enumerate() {
            let shift = angle(rivers.pos[k], old);
            assert!(shift < step, "node {k} moved {shift}");
            if k >= count {
                // The ocean cell at the end of a river.
                assert_eq!(rivers.pos[k], old);
                let cell = cell_at(m, rivers.pos[k]);
                assert!(is_ocean(&heights, cell));
                outlets += 1;
            } else if rivers.donor[k] == NONE {
                assert_eq!(rivers.pos[k], old);
                let cell = split(m, rivers.cells[k] as usize);
                assert_eq!(rivers.pos[k], cell_dir(m, cell.0, cell.1, cell.2));
                sources += 1;
            } else if shift > 0.05 * step {
                moved += 1;
            }
        }
        assert!(sources >= 1 && outlets >= 1);
        assert!(moved > count / 4, "{moved} of {count}");
    }

    /// One large slope down to a sea, with low hills on it.
    fn slope_meters(d: V3) -> f64 {
        let low = lonlat_to_dir(160.0, -20.0);
        let along = d[0] * low[0] + d[1] * low[1] + d[2] * low[2];
        -300.0
            + 1300.0 * (1.0 - along)
            + 60.0 * (40.0 * d[0]).sin() * (40.0 * d[1]).cos()
            + 40.0 * (55.0 * d[2] + 20.0 * d[0]).sin()
            + 10.0 * (300.0 * d[1]).sin() * (300.0 * d[2]).cos()
    }

    fn slope(d: V3) -> u16 {
        meters_to_level(slope_meters(d))
    }

    /// A valley along the equator that goes down to the west, from face 2 to
    /// face 0.
    fn equator_valley(d: V3) -> u16 {
        let (lon, lat) = dir_to_lonlat(d);
        let across = (lat - 0.4).abs();
        meters_to_level(if lon > 10.0 && lon < 80.0 && across < 3.0 {
            100.0 + 20.0 * (lon - 10.0) + 300.0 * across
        } else {
            OCEAN
        })
    }

    impl WindowFlow {
        fn rivers_count(&self) -> usize {
            let rivers = self.area.iter();
            let rivers = rivers.filter(|&&a| self.threshold.holds(f64::from(a)));
            rivers.count()
        }
    }

    #[test]
    fn window_place_is_the_inverse_of_the_cell_direction() {
        let past_edges = Window {
            face: 3,
            x0: -40,
            y0: 200,
            cell: 4,
            cells: 32,
            face_size: 256,
        };
        let centered = Window::centered(lonlat_to_dir(44.0, 30.0), 1024, 2, 48);
        assert_eq!((centered.face, centered.x0 % 2, centered.y0 % 2), (0, 0, 0));
        assert_eq!(centered.cell_at(lonlat_to_dir(44.0, 30.0)), Some((24, 24)));
        for window in [past_edges, centered] {
            for y in 0..window.cells {
                for x in 0..window.cells {
                    let d = window.cell_dir(x, y);
                    let (u, v) = window.place(d).expect("the cell has a place");
                    assert!((u - x as f64 - 0.5).abs() < 1e-9, "{x} {y}: {u}");
                    assert!((v - y as f64 - 0.5).abs() < 1e-9, "{x} {y}: {v}");
                    assert_eq!(window.cell_at(d), Some((x, y)));
                }
            }
            let behind = scale(window.cell_dir(0, 0), -1.0);
            assert_eq!(window.place(behind), None);
            // A texel of the heightmap, 5 and 7 texels from the first one.
            let cell = window.cell as f64;
            let d = window.dir(5.5 / cell, 7.5 / cell);
            let (u, v) = window.channel_of_dir(d).expect("the texel has a place");
            let exact = window.channel_of_texel(window.x0 + 5, window.y0 + 7);
            assert!((u - exact.0).abs() < 1e-9 && (v - exact.1).abs() < 1e-9);
        }

        // A window on one full face has the cells of that face.
        let m = 32;
        let face = Window {
            face: 4,
            x0: 0,
            y0: 0,
            cell: 8,
            cells: m,
            face_size: 256,
        };
        let solid = solid_angles(m);
        for (i, &solid) in solid.iter().enumerate() {
            let (x, y) = (i % m, i / m);
            assert!(angle(face.cell_dir(x, y), cell_dir(m, 4, x, y)) < 1e-12);
            assert!((face.cell_solid_angle(x, y) - solid).abs() < 1e-12);
        }
        assert!(face.covers(8.0, 55.0) && !face.covers(7.9, 55.0));
        assert!(!face.covers(8.0, 55.1));
    }

    #[test]
    fn a_window_has_more_rivers_than_the_global_map() {
        let m = 256;
        let global = FlowMap::new(&CoarseHeights::from_fn(m, slope));
        // A cell of the window is a quarter of a cell of the global map.
        let window = Window::centered(lonlat_to_dir(10.0, 5.0), 1024, 1, 128);
        let on_ground = cells(m).filter(|&(face, x, y)| {
            global.is_river(face, x, y) && window.cell_at(cell_dir(m, face, x, y)).is_some()
        });
        let coarse = on_ground.count();
        let flow = WindowFlow::new(&WindowHeights::from_fn(window, slope), Some(&global));
        let fine = flow.rivers_count();
        assert!(coarse > 10, "{coarse}");
        assert!(fine > 5 * coarse, "{fine} {coarse}");
    }

    #[test]
    fn a_window_with_no_sea_drains_to_its_border() {
        let window = Window::centered(lonlat_to_dir(10.0, 5.0), 1024, 2, 64);
        let land = |d: V3| meters_to_level(slope_meters(d) + 2000.0);
        let heights = WindowHeights::from_fn(window, land);
        assert!(heights.levels.iter().all(|&level| level >= sea()));
        let flow = WindowFlow::new(&heights, None);
        let cells = window.cells;
        for start in 0..cells * cells {
            let (mut i, mut steps) = (start, 0);
            while flow.receiver[i] != OUT {
                i = flow.receiver[i] as usize;
                steps += 1;
                assert!(steps <= cells * cells, "the receivers make a loop");
            }
            let (x, y) = (i % cells, i / cells);
            assert!(x == 0 || y == 0 || x == cells - 1 || y == cells - 1);
        }
        assert!(flow.rivers_count() > 50);
        assert!(flow.channels().has_rivers());
    }

    #[test]
    fn a_river_keeps_its_size_when_it_enters_a_window() {
        let global = FlowMap::new(&CoarseHeights::from_fn(64, equator_valley));
        // The window is 2 cells of the global map wide, on the low part of
        // the valley.
        let window = Window::centered(lonlat_to_dir(20.0, 0.4), 8192, 2, 128);
        let heights = WindowHeights::from_fn(window, equator_valley);
        let alone = WindowFlow::new(&heights, None).channels();
        let fed = window_channels(&heights, &global);
        let size = fed.size();
        let texels = |map: &ChannelWindow| {
            let all = (0..size * size).map(|i| map.texel(i % size, i / size));
            all.collect::<Vec<_>>()
        };
        let largest = |map: &ChannelWindow| texels(map).iter().map(|t| t[1]).max().unwrap();
        assert!(largest(&alone) < 220, "{}", largest(&alone));
        assert_eq!(largest(&fed), 255);
        // The large river goes on to the side of the window.
        let on_river = texels(&fed);
        let on_river = on_river.iter().filter(|t| t[0] < 32 && t[1] >= 250);
        assert!(on_river.count() > 100);
    }

    #[test]
    fn a_cell_has_the_same_streams_in_two_windows() {
        let first = Window::centered(lonlat_to_dir(10.0, 5.0), 1024, 2, 64);
        let second = Window {
            x0: first.x0 + 40,
            y0: first.y0 + 24,
            ..first
        };
        let flows = [first, second].map(|window| {
            let flow = WindowFlow::new(&WindowHeights::from_fn(window, slope), None);
            (window, flow)
        });
        // The receiver of a cell, as a place on the face in cells.
        let receiver = |(window, flow): &(Window, WindowFlow), x: i64, y: i64| {
            let (x0, y0) = (window.x0 / 2, window.y0 / 2);
            let to = flow.receiver[((y - y0) * 64 + x - x0) as usize] as i64;
            (to % 64 + x0, to / 64 + y0)
        };
        // The cells that are 10 cells or more from the sides of each window.
        let (mut same, mut all) = (0, 0);
        for y in second.y0 / 2 + 10..first.y0 / 2 + 54 {
            for x in second.x0 / 2 + 10..first.x0 / 2 + 54 {
                all += 1;
                same += usize::from(receiver(&flows[0], x, y) == receiver(&flows[1], x, y));
            }
        }
        assert!(all > 500);
        eprintln!("the same receiver: {same} of {all}");
        assert!(same * 100 >= all * 90, "{same} of {all}");
    }

    #[test]
    fn a_window_past_a_face_edge_follows_the_land_of_the_next_face() {
        let n = 256;
        let mut map = Heightmap::new(n, meters_to_level(OCEAN));
        for face in [0, 2] {
            let levels: Vec<u16> = (0..n * n)
                .map(|i| equator_valley(map.texel_dir(face, i % n, i / n)))
                .collect();
            let all = TexelRect {
                x0: 0,
                y0: 0,
                x1: n,
                y1: n,
            };
            map.store_rect(face, all, &levels);
        }
        // The faces meet at longitude 45.
        let window = Window::centered(lonlat_to_dir(44.0, 0.4), n, 1, 64);
        assert_eq!(window.face, 0);
        assert!(window.x0 + 64 > n as i64 + 20);
        let heights = WindowHeights::new(&map, window);
        for y in 0..64 {
            for x in 0..64 {
                let d = window.cell_dir(x, y);
                assert_eq!(heights.level(x, y), map.sample(d), "{x} {y}");
            }
        }
        let level_at = |lon: f64, lat: f64| {
            let (x, y) = window
                .cell_at(lonlat_to_dir(lon, lat))
                .expect("in the window");
            assert!(x as i64 + window.x0 >= n as i64);
            heights.level(x, y)
        };
        assert!(level_at(48.0, 0.4) >= sea() && level_at(48.0, 6.0) < sea());

        // The river is in each column of texels, on both sides of the edge.
        let channels = WindowFlow::new(&heights, None).channels();
        let edge = 2 * (n - window.x0 as usize);
        assert!(edge > 40 && edge < 100);
        for x in 8..104 {
            let nearest = (0..128).map(|y| channels.texel(x, y)[0]).min().unwrap();
            assert!(nearest < 32, "column {x}: {nearest}");
        }
    }

    #[test]
    fn the_default_threshold_gives_the_same_channels() {
        let m = 48;
        let flow = FlowMap::new(&CoarseHeights::from_fn(m, lumpy));
        let channels = flow.channels();
        assert!(channels.has_rivers());
        assert_eq!(channels.min_cells(), RIVER_MIN_CELLS);
        assert!(channels.bytes() == flow.channels_with(RIVER_MIN_CELLS).bytes());
        assert!(channels.bytes() != flow.channels_with(4.0 * RIVER_MIN_CELLS).bytes());
        for (face, x, y) in cells(m) {
            let river = flow.is_river(face, x, y);
            assert_eq!(river, flow.is_river_with(face, x, y, RIVER_MIN_CELLS));
            assert_eq!(river, flow.area(face, x, y) >= RIVER_MIN_AREA);
        }

        let window = Window::centered(lonlat_to_dir(10.0, 5.0), 1024, 2, 64);
        let heights = WindowHeights::from_fn(window, slope);
        let global = FlowMap::new(&CoarseHeights::from_fn(m, slope));
        let channels = window_channels(&heights, &global);
        assert!(channels.has_rivers());
        assert_eq!(channels.min_cells(), RIVER_MIN_CELLS);
        let same = window_channels_with(&heights, &global, RIVER_MIN_CELLS);
        assert!(channels.bytes() == same.bytes());
        let other = window_channels_with(&heights, &global, 8.0);
        assert_eq!(other.min_cells(), 8.0);
        assert!(channels.bytes() != other.bytes());
    }

    /// The land of a window with no sea, and its flow at a threshold.
    fn high_window(min_cells: f64) -> (WindowHeights, WindowFlow) {
        let window = Window::centered(lonlat_to_dir(10.0, 5.0), 1024, 2, 64);
        let land = |d: V3| meters_to_level(slope_meters(d) + 2000.0);
        let heights = WindowHeights::from_fn(window, land);
        let flow = WindowFlow::with(&heights, None, min_cells);
        (heights, flow)
    }

    #[test]
    fn a_lower_threshold_takes_the_rivers_higher_up() {
        let (heights, few) = high_window(RIVER_MIN_CELLS);
        let (_, many) = high_window(8.0);
        let rivers = |flow: &WindowFlow| -> Vec<bool> {
            let areas = flow.area.iter();
            areas.map(|&a| flow.threshold.holds(f64::from(a))).collect()
        };
        let (few, many) = (rivers(&few), rivers(&many));
        assert!(few.iter().zip(&many).all(|(few, many)| !few || *many));
        let count = |rivers: &[bool]| rivers.iter().filter(|&&river| river).count();
        assert!(
            count(&many) > 3 * count(&few),
            "{} {}",
            count(&many),
            count(&few)
        );
        // The highest river cell.
        let top = |rivers: &[bool]| {
            let levels = heights.levels.iter().zip(rivers);
            levels
                .filter(|(_, river)| **river)
                .map(|(level, _)| *level)
                .max()
        };
        assert!(top(&many) > top(&few));

        let m = 128;
        let global = FlowMap::new(&CoarseHeights::from_fn(m, lumpy));
        let (mut few, mut many) = (0, 0);
        for (face, x, y) in cells(m) {
            let (at_248, at_62) = (
                global.is_river_with(face, x, y, 4.0 * RIVER_MIN_CELLS),
                global.is_river(face, x, y),
            );
            assert!(!at_248 || at_62);
            few += usize::from(at_248);
            many += usize::from(at_62);
        }
        assert!(few > 0 && many > 2 * few, "{many} {few}");
    }

    #[test]
    fn at_the_lowest_threshold_each_land_cell_is_a_river() {
        // A texel is 0.71 channel texels or less from the center of its
        // cell. The smoothing moves the node of a cell, so the distance to
        // the nearest river can be more. The largest distance byte in this
        // test is 66, which is 2.1 channel texels.
        const FARTHEST: u8 = 72;
        let m = 48;
        let heights = CoarseHeights::from_fn(m, lumpy);
        let flow = FlowMap::new(&heights);
        let channels = flow.channels_with(RIVER_MIN_CELLS_LOWEST);
        assert_eq!(channels.min_cells(), 1.0);
        let (mut land, mut lowest, mut farthest) = (0, 0, 0);
        for i in 0..FACES * m * m {
            let (face, x, y) = split(m, i);
            let ocean = is_ocean(&heights, (face, x, y));
            assert_eq!(flow.is_river_with(face, x, y, 1.0), !ocean);
            // The cells at the coast have ocean texels next to them.
            let near = neighbors(m, i);
            if ocean
                || near
                    .iter()
                    .any(|&j| is_ocean(&heights, split(m, j as usize)))
            {
                continue;
            }
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let [distance, size] = channels.texel(face, 2 * x + dx, 2 * y + dy);
                assert!(
                    size > 0 && distance <= FARTHEST,
                    "{face} {x} {y}: {distance}"
                );
                land += 1;
                lowest += usize::from(size == 1);
                farthest = farthest.max(distance);
            }
        }
        eprintln!("global: {lowest} of {land} texels have size 1, farthest {farthest}");
        assert!(land > 1000 && lowest < land);

        let (_, flow) = high_window(RIVER_MIN_CELLS_LOWEST);
        assert_eq!(flow.rivers_count(), 64 * 64);
        let channels = flow.channels();
        let (mut land, mut lowest, mut farthest) = (0, 0, 0);
        for y in 8..120 {
            for x in 8..120 {
                let [distance, size] = channels.texel(x, y);
                assert!(size > 0 && distance <= FARTHEST, "{x} {y}: {distance}");
                land += 1;
                lowest += usize::from(size == 1);
                farthest = farthest.max(distance);
            }
        }
        eprintln!("window: {lowest} of {land} texels have size 1, farthest {farthest}");
        assert!(lowest > 0 && lowest < land);
    }

    #[test]
    fn a_segment_writes_only_its_own_reach() {
        let size = 64;
        let (a, b) = ((20.3, 30.0), (40.0, 33.5));
        let mut written = Vec::new();
        for flow in [1, 120, 255] {
            let mut data = [255, 0].repeat(size * size);
            draw_line(&mut data, size, a, b, flow);
            let reach = reach(flow);
            let mut count = 0;
            for (i, texel) in data.as_chunks::<2>().0.iter().enumerate() {
                // The distance from the texel to the line.
                let (px, py) = ((i % size) as f64 - a.0, (i / size) as f64 - a.1);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = ((px * dx + py * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                let distance = (px - t * dx).hypot(py - t * dy);
                let byte = (distance * CHANNEL_SCALE).round();
                if distance <= reach && byte < 255.0 {
                    assert_eq!(*texel, [byte as u8, flow], "texel {i}");
                    count += 1;
                } else {
                    assert_eq!(*texel, [255, 0], "texel {i}");
                }
            }
            written.push(count);
        }
        assert!(written[0] > 100 && written[0] < written[1] && written[1] < written[2]);
        // A small river has a narrow valley, and a large river stops at the
        // largest distance that a texel holds.
        assert!((reach(1) - (1.5 * VALLEY_MAX + 1.5)).abs() < 0.03);
        assert_eq!(reach(255), CHANNEL_REACH);
        assert!(reach(200) > VALLEY_WIDEST.min((1.5 + 4.5 * 200.0 / 255.0) * VALLEY_MAX) + 1.4);
    }

    /// Run with `--release --ignored --nocapture` for the time of a window
    /// of `WINDOW_CELLS` cells on a face of 8192 texels.
    #[test]
    #[ignore]
    fn window_time_at_the_full_size() {
        let n = 8192;
        let mut map = Heightmap::new(n, meters_to_level(OCEAN));
        let land = TexelRect {
            x0: 3000,
            y0: 3000,
            x1: 5400,
            y1: 5400,
        };
        let levels: Vec<u16> = (0..2400 * 2400)
            .map(|i| slope(map.texel_dir(0, 3000 + i % 2400, 3000 + i / 2400)))
            .collect();
        map.store_rect(0, land, &levels);
        let global = FlowMap::new(&CoarseHeights::new(&map));
        for cell in [1, 4] {
            let window = Window::centered(map.texel_dir(0, 4200, 4200), n, cell, WINDOW_CELLS);
            let start = std::time::Instant::now();
            let heights = WindowHeights::new(&map, window);
            let read = start.elapsed();
            for min_cells in [RIVER_MIN_CELLS, RIVER_MIN_CELLS_LOWEST] {
                let start = std::time::Instant::now();
                let channels = window_channels_with(&heights, &global, min_cells);
                let time = start.elapsed();
                let k = channels.size();
                let inside = (8..k - 8).flat_map(|y| (8..k - 8).map(move |x| (x, y)));
                let texels: Vec<[u8; 2]> = inside.map(|(x, y)| channels.texel(x, y)).collect();
                eprintln!(
                    "cell {cell}, {min_cells} cells: heights {read:?}, channels {time:?}, {}",
                    shares(&texels)
                );
                assert!(channels.has_rivers());
            }
        }
    }

    /// The shares of the texels with the lowest size, with no river, and at
    /// more than 1.5 and 1 channel texels from a river.
    fn shares(texels: &[[u8; 2]]) -> String {
        let share = |test: &dyn Fn(&[u8; 2]) -> bool| {
            texels.iter().filter(|t| test(t)).count() as f64 / texels.len() as f64
        };
        format!(
            "size 1: {:.3}, no river: {:.3}, past 1.5: {:.4}, past 1: {:.3}, at 0.5 or less: {:.3}",
            share(&|t| t[1] == 1),
            share(&|t| t[1] == 0),
            share(&|t| t[1] > 0 && t[0] > 48),
            share(&|t| t[1] > 0 && t[0] > 32),
            share(&|t| t[0] <= 16),
        )
    }

    /// Run with `--release --ignored --nocapture` for the time of the flow
    /// and the channel map at the full size.
    #[test]
    #[ignore]
    fn flow_time_at_the_full_size() {
        let heights = CoarseHeights::from_fn(FLOW_SIZE, lumpy);
        let land = heights
            .levels
            .iter()
            .filter(|&&level| level >= sea())
            .count();
        let start = std::time::Instant::now();
        let flow = FlowMap::new(&heights);
        let flow_time = start.elapsed();
        for min_cells in [RIVER_MIN_CELLS, RIVER_MIN_CELLS_LOWEST] {
            let start = std::time::Instant::now();
            let channels = flow.channels_with(min_cells);
            let time = start.elapsed();
            // The texels of the land cells.
            let k = channels.size();
            let texels: Vec<[u8; 2]> = (0..FACES * k * k)
                .filter(|i| heights.level(i / (k * k), i % k / 2, i / k % k / 2) >= sea())
                .map(|i| channels.texel(i / (k * k), i % k, i / k % k))
                .collect();
            eprintln!(
                "{min_cells} cells: channels {time:?}, on land {}",
                shares(&texels)
            );
        }
        let start = std::time::Instant::now();
        let channels = flow.channels();
        let rivers = flow
            .area
            .iter()
            .filter(|&&a| f64::from(a) >= RIVER_MIN_AREA);
        eprintln!(
            "land {:.2}, river cells {}, flow {flow_time:?}, with channels {:?}",
            land as f64 / heights.levels.len() as f64,
            rivers.count(),
            start.elapsed()
        );
        assert!(channels.has_rivers());
    }
}
