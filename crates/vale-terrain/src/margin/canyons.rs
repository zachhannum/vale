//! The canyons of a margin stroke.
//!
//! Turbidity currents and slides cut the canyons of a continental slope.
//! The erosion of a canyon floor grows with the slope above a threshold and
//! with the drainage area to a small power (Mitchell 2004). A canyon starts
//! where the drainage area passes a limit, and the walls between the canyons
//! only fail. Only steep ground feeds the flow, so the shelf and the rise
//! give no water. A few seeded heads also get the water of a
//! part of the shelf, and they cut back into it. The walls of a canyon fail
//! above an angle.
//!
//! The law runs on the cells, so its canyons follow the 8 directions of the
//! grid. Thus the pass traces each canyon as a line down the flow, smooths
//! the line and its depth, and cuts a V profile along the smooth line. The
//! ground between the canyons does not change.

use super::par_map;
use crate::heightmap::noise3;
use crate::line::smoothstep;
use crate::math::{V3, scale};

/// The area exponent of the erosion (Mitchell 2004).
const AREA_EXPONENT: f32 = 0.25;

/// The slope below which a floor does not erode.
const MIN_SLOPE: f32 = 0.02;

/// The drainage area from which the threshold is half as large, in square
/// kilometers, so the largest canyons go on across the rise.
const LONG_KM2: f32 = 250.0;

/// The drainage area of the head of a canyon, in square kilometers. A cell
/// with less area does not cut, and a cell with twice the area cuts at the
/// full rate. The walls between the canyons only fail.
const CHANNEL_KM2: f32 = 60.0;

/// The flow of a cell goes to the neighbor with the largest drop times a
/// seeded factor from 1 - w to 1 + w, where w is `WANDER` times the
/// branching. Thus the canyons do not follow the 8 directions of the grid.
const WANDER: f32 = 0.6;

/// The slopes over which a cell starts to feed the flow.
const SOURCE_SLOPE: (f64, f64) = (0.025, 0.05);

/// The number of steps of a stroke.
const STEPS: usize = 40;

/// The erosion of one step at an amount of 1, for an area of 1 square
/// kilometer and a cell of 1 kilometer.
const RATE: f32 = 0.6;

/// The angle above which a wall fails, in degrees, and the part of it that
/// a seeded noise changes.
const FAIL_DEG: f32 = 22.0;
const FAIL_NOISE: f32 = 0.25;

/// The size of the features of the roughness that the flow starts on, in
/// kilometers, at least 3 cells wide, and their heights as a part of the
/// drop of the slope over their size. The flow gathers in the troughs, so
/// the size sets the spacing of the canyons.
const ROUGH: [(f64, f32); 2] = [(25.0, 0.3), (4.5, 0.144)];

/// The depths over which the shelf starts to cut, as parts of the depth of
/// the shelf break. A seeded head cuts back into the shelf near the break
/// only.
const SHELF_CUT: (f64, f64) = (0.8, 1.2);

/// The size of the shelf areas that feed a seeded head, in kilometers, and
/// the part of the shelf that they cover.
const FED_KM: f64 = 40.0;
const FED_PART: f64 = 0.3;

const SALT_ROUGH: u32 = 0x1f83_d9ab;
const SALT_FAIL: u32 = 0x5be0_cd19;
const SALT_FED: u32 = 0x9b05_688c;

/// The cells of a canyon pass: a grid of `side` columns and rows of whole
/// faces of `side` rows each, with no flow between faces.
pub(super) struct Grid<'a> {
    pub side: usize,
    /// The width of a cell in kilometers.
    pub cell_km: f64,
    /// The unit direction of each cell.
    pub dir: &'a (dyn Fn(usize) -> V3 + Sync),
    /// The weight of the brushes at each cell, from 0 to 1. A cell with a
    /// weight of 0 is not under a brush.
    pub weights: &'a [f32],
    pub world_km: f64,
    pub seed: u32,
    /// The depth of the shelf break, in meters.
    pub shelf_m: f64,
    /// The branching of the Erode brush, from 0 to 1.
    pub branching: f64,
}

/// Cuts the canyons into `heights`, in meters. `amount` from 0 to 1 sets
/// the rate. The ground above the sea level does not change.
pub(super) fn carve(grid: &Grid, heights: &mut [f32], amount: f64) {
    let n = heights.len();
    let side = grid.side;
    if amount <= 0.0 || n == 0 || side < 3 {
        return;
    }
    let cell = grid.cell_km as f32;
    let noise =
        |dir: V3, km: f64, salt: u32| noise3(scale(dir, grid.world_km / km), grid.seed ^ salt);
    let least = 3.0 * grid.cell_km;
    // The roughness that the flow starts on, the angle of failure, and the
    // seeded areas of the shelf that feed a head.
    let start: Vec<f32> = heights.to_vec();
    let cut: Vec<f32> = start
        .iter()
        .map(|&h| {
            let depth = -f64::from(h) / -grid.shelf_m;
            smoothstep(SHELF_CUT.0, SHELF_CUT.1, depth) as f32
        })
        .collect();
    let cells = par_map(n, |i| {
        if grid.weights[i] <= 0.0 {
            return (0.0, 0.0, false);
        }
        let dir = (grid.dir)(i);
        let slope = steepest(&start, side, i).0 / (1000.0 * cell);
        let mut rough = 0.0;
        for (k, &(km, part)) in ROUGH.iter().enumerate() {
            let km = km.max(least);
            let m = part * slope * 1000.0 * km as f32;
            rough += m * noise(dir, km, SALT_ROUGH.wrapping_add(k as u32))[0] as f32;
        }
        let n_fail = noise(dir, least.max(5.0), SALT_FAIL)[0] as f32;
        let deg = FAIL_DEG * (1.0 + FAIL_NOISE * n_fail);
        let fail = deg.to_radians().tan() * 1000.0 * cell;
        let depth = -f64::from(start[i]);
        let shelf = depth > 0.2 * -grid.shelf_m && depth < -grid.shelf_m;
        let fed = shelf && noise(dir, FED_KM, SALT_FED)[0] > 1.0 - 2.0 * FED_PART;
        (rough, fail, fed)
    });
    let rough: Vec<f32> = cells.iter().map(|c| c.0).collect();
    let fail: Vec<f32> = cells.iter().map(|c| c.1).collect();
    let fed: Vec<bool> = cells.iter().map(|c| c.2).collect();
    for i in 0..n {
        heights[i] += rough[i];
    }
    let wet = |i: usize| grid.weights[i] > 0.0 && start[i] < 0.0;
    // The water of each cell from the slope of the ground before the
    // canyons.
    let area = (grid.cell_km * grid.cell_km) as f32;
    let water: Vec<f32> = (0..n)
        .map(|i| {
            if !wet(i) {
                return 0.0;
            }
            let s = f64::from(steepest(heights, side, i).0) / grid.cell_km / 1000.0;
            let from_slope = smoothstep(SOURCE_SLOPE.0, SOURCE_SLOPE.1, s) as f32;
            area * if fed[i] { 1.0 } else { from_slope }
        })
        .collect();
    let rate = RATE * amount as f32;
    let wander = WANDER * grid.branching.clamp(0.0, 1.0) as f32;
    let mut receiver = vec![0usize; n];
    let mut stack = Stack::default();
    let mut flow = vec![0f32; n];
    for step in 0..STEPS {
        {
            let heights = &*heights;
            par_fill(&mut receiver, |i| {
                if wet(i) {
                    receiver_of(heights, side, i, grid.seed, wander)
                } else {
                    i
                }
            });
        }
        stack.build(&receiver);
        let order = &stack.order;
        flow.copy_from_slice(&water);
        for &i in order.iter().rev() {
            let r = receiver[i];
            if r != i {
                flow[r] += flow[i];
            }
        }
        // The erosion, from the outlets up, toward a floor at the threshold
        // slope below the receiver (Braun and Willett 2013).
        for &i in order {
            let r = receiver[i];
            if r == i || !wet(i) {
                continue;
            }
            let d = distance(side, i, r) * cell;
            let a = flow[i];
            let threshold = MIN_SLOPE * 1000.0 * LONG_KM2 / (LONG_KM2 + a);
            let floor = heights[r] + threshold * d;
            if heights[i] > floor {
                let head = ((a - CHANNEL_KM2) / CHANNEL_KM2).clamp(0.0, 1.0);
                let f = rate * grid.weights[i] * cut[i] * head * a.powf(AREA_EXPONENT) / d;
                heights[i] = (heights[i] + f * floor) / (1.0 + f);
            }
        }
        fail_walls(heights, side, &fail, grid.weights, &wet, step % 2 == 1);
    }
    let order = &stack.order;
    // The depth of the cut of each cell. The roughness does not stay.
    // A gully of steep ground gets a depth from its area, at least.
    let depth: Vec<f32> = (0..n)
        .map(|i| {
            let cut = (start[i] + rough[i] - heights[i]).max(0.0);
            let s = steepest(&start, side, i).0 / (1000.0 * cell);
            let steep = smoothstep(SOURCE_SLOPE.0, SOURCE_SLOPE.1, f64::from(s)) as f32;
            let gully = if flow[i] >= GULLY_KM2 && wet(i) {
                GULLY_M * (flow[i] / CHANNEL_KM2).sqrt().min(1.0) * steep * grid.weights[i]
            } else {
                0.0
            };
            cut.max(gully)
        })
        .collect();
    let lines = trace(&receiver, &flow, order, &depth, side, GULLY_KM2);
    let wall = FAIL_DEG.to_radians().tan() * 1000.0 * cell;
    let mut cut = vec![0f32; n];
    for line in &lines {
        cut_line(line, side, wall, &wet, &mut cut);
    }
    for i in 0..n {
        heights[i] = start[i] - cut[i];
    }
}

/// The largest depth of a gully of steep ground, in meters, and the least
/// drainage area of a gully, in square kilometers.
const GULLY_M: f32 = 120.0;
const GULLY_KM2: f32 = 6.0;

/// The least depth of a canyon cell, in meters.
const LINE_DEPTH_M: f32 = 5.0;

/// The length over which the depth of a canyon grows from its head, and
/// the least length of a canyon, in cells.
const HEAD_CELLS: f32 = 3.0;
const LEAST_CELLS: usize = 4;

/// The radius of the round floor of a canyon, in cells.
const FLOOR_CELLS: f32 = 0.75;

/// The number of rounds of corner cutting of a canyon line.
const SMOOTH_ROUNDS: usize = 3;

/// The half width of the moving mean of the depth along a line, in points.
const DEPTH_MEAN: usize = 2;

/// A point of a canyon line: the place in cells, with `y` the row, and the
/// depth of the cut in meters.
#[derive(Clone, Copy, Debug)]
struct Point {
    x: f32,
    y: f32,
    depth: f32,
}

/// The canyon lines of the flow: the paths down the receivers from each
/// head, the longest first. A line ends where it joins an earlier line, so
/// a trunk is one line and each branch ends on it. The lines are smooth.
fn trace(
    receiver: &[usize],
    flow: &[f32],
    order: &[usize],
    depth: &[f32],
    side: usize,
    least_km2: f32,
) -> Vec<Vec<Point>> {
    let n = receiver.len();
    let channel: Vec<bool> = (0..n)
        .map(|i| flow[i] >= least_km2 && depth[i] > LINE_DEPTH_M)
        .collect();
    // The length of the path from each cell to its outlet, in cells.
    let mut length = vec![0f32; n];
    for &i in order {
        let r = receiver[i];
        if r != i {
            length[i] = length[r] + distance(side, i, r);
        }
    }
    let mut fed = vec![false; n];
    for i in 0..n {
        if channel[i] && receiver[i] != i {
            fed[receiver[i]] = true;
        }
    }
    let mut heads: Vec<usize> = (0..n).filter(|&i| channel[i] && !fed[i]).collect();
    heads.sort_by(|&a, &b| length[b].total_cmp(&length[a]).then(a.cmp(&b)));
    let mut seen = vec![false; n];
    let mut lines = Vec::new();
    for head in heads {
        let mut cells = Vec::new();
        let mut i = head;
        loop {
            cells.push(i);
            if seen[i] {
                break;
            }
            seen[i] = true;
            let r = receiver[i];
            if r == i || !channel[r] {
                break;
            }
            i = r;
        }
        if cells.len() < LEAST_CELLS {
            continue;
        }
        let mut points: Vec<Point> = cells
            .iter()
            .map(|&i| Point {
                x: (i % side) as f32,
                y: (i / side) as f32,
                depth: depth[i],
            })
            .collect();
        // The depth of the end on an earlier line is the depth there, so a
        // branch meets its trunk at the floor of the trunk.
        let means: Vec<f32> = (0..points.len())
            .map(|k| {
                let lo = k.saturating_sub(DEPTH_MEAN);
                let hi = (k + DEPTH_MEAN + 1).min(points.len());
                points[lo..hi].iter().map(|p| p.depth).sum::<f32>() / (hi - lo) as f32
            })
            .collect();
        let last = points.len() - 1;
        let mut along = 0.0;
        for k in 0..points.len() {
            if k > 0 {
                along += (points[k].x - points[k - 1].x).hypot(points[k].y - points[k - 1].y);
            }
            if k != last {
                points[k].depth =
                    means[k] * smoothstep(0.0, f64::from(HEAD_CELLS), f64::from(along)) as f32;
            }
        }
        for _ in 0..SMOOTH_ROUNDS {
            points = cut_corners(&points);
        }
        lines.push(points);
    }
    lines
}

/// One round of corner cutting (Chaikin 1974): each segment gives the
/// points at a quarter and at three quarters of it. The ends stay.
fn cut_corners(points: &[Point]) -> Vec<Point> {
    let mix = |a: Point, b: Point, t: f32| Point {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        depth: a.depth + (b.depth - a.depth) * t,
    };
    let mut out = Vec::with_capacity(2 * points.len());
    out.push(points[0]);
    for pair in points.windows(2) {
        out.push(mix(pair[0], pair[1], 0.25));
        out.push(mix(pair[0], pair[1], 0.75));
    }
    out.push(points[points.len() - 1]);
    out
}

/// Cuts a V profile along a line into `cut`: at a distance `r` from the
/// line in cells, the cut is the depth of the line less `wall` meters for
/// each cell, with a round floor. Each cell keeps its deepest cut. The
/// cells of a face other than the face of the line do not change.
fn cut_line(line: &[Point], side: usize, wall: f32, wet: &impl Fn(usize) -> bool, cut: &mut [f32]) {
    let n = cut.len();
    let face = (line[0].y as usize) / side;
    let rows = (face * side, ((face + 1) * side).min(n / side));
    for pair in line.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let reach = a.depth.max(b.depth) / wall + FLOOR_CELLS + 1.0;
        let x0 = (a.x.min(b.x) - reach).floor().max(0.0) as usize;
        let x1 = ((a.x.max(b.x) + reach).ceil() as usize).min(side - 1);
        let y0 = ((a.y.min(b.y) - reach).floor().max(rows.0 as f32)) as usize;
        let y1 = ((a.y.max(b.y) + reach).ceil() as usize).min(rows.1 - 1);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let length2 = (dx * dx + dy * dy).max(1e-9);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let i = y * side + x;
                if !wet(i) {
                    continue;
                }
                let (px, py) = (x as f32 - a.x, y as f32 - a.y);
                let t = ((px * dx + py * dy) / length2).clamp(0.0, 1.0);
                let (ex, ey) = (px - t * dx, py - t * dy);
                let r = (ex * ex + ey * ey).sqrt();
                let depth = a.depth + (b.depth - a.depth) * t;
                let round = (r * r + FLOOR_CELLS * FLOOR_CELLS).sqrt() - FLOOR_CELLS;
                let v = depth - wall * round;
                if v > cut[i] {
                    cut[i] = v;
                }
            }
        }
    }
}

/// The drop to the lowest of the 8 cells around cell `i`, in meters for
/// each cell of distance, and that cell. A cell with no lower cell drains to itself.
fn steepest(heights: &[f32], side: usize, i: usize) -> (f32, usize) {
    let (row, x) = (i / side, i % side);
    let face = row / side;
    let mut best = (0.0, i);
    for (dr, dx, d) in NEIGHBORS {
        let (r, c) = (row as i64 + dr, x as i64 + dx);
        if c < 0 || c >= side as i64 || r < 0 {
            continue;
        }
        let (r, c) = (r as usize, c as usize);
        let j = r * side + c;
        if r / side != face || j >= heights.len() {
            continue;
        }
        let drop = (heights[i] - heights[j]) / d;
        if drop > best.0 {
            best = (drop, j);
        }
    }
    best
}

/// The receiver of cell `i`: the lower neighbor with the largest drop
/// times a seeded factor of the cell and the neighbor. A cell with no lower
/// neighbor drains to itself.
fn receiver_of(heights: &[f32], side: usize, i: usize, seed: u32, wander: f32) -> usize {
    let (row, x) = (i / side, i % side);
    let face = row / side;
    let mut best = (0.0, i);
    for (k, (dr, dx, d)) in NEIGHBORS.into_iter().enumerate() {
        let (r, c) = (row as i64 + dr, x as i64 + dx);
        if c < 0 || c >= side as i64 || r < 0 {
            continue;
        }
        let (r, c) = (r as usize, c as usize);
        let j = r * side + c;
        if r / side != face || j >= heights.len() {
            continue;
        }
        let drop = (heights[i] - heights[j]) / d;
        if drop <= 0.0 {
            continue;
        }
        let h = mix(mix(i as u32 ^ seed) ^ k as u32);
        let factor = 1.0 + wander * (2.0 * (h as f32 / u32::MAX as f32) - 1.0);
        if drop * factor > best.0 {
            best = (drop * factor, j);
        }
    }
    best.1
}

/// A hash of a number.
fn mix(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

const D: f32 = std::f32::consts::SQRT_2;
const NEIGHBORS: [(i64, i64, f32); 8] = [
    (-1, -1, D),
    (-1, 0, 1.0),
    (-1, 1, D),
    (0, -1, 1.0),
    (0, 1, 1.0),
    (1, -1, D),
    (1, 0, 1.0),
    (1, 1, D),
];

/// The distance between two cells next to each other, in cells.
fn distance(side: usize, i: usize, j: usize) -> f32 {
    if i % side != j % side && i / side != j / side {
        D
    } else {
        1.0
    }
}

/// Fills `out` with `f` of each index, on the threads of the machine.
fn par_fill<T: Send>(out: &mut [T], f: impl Fn(usize) -> T + Sync) {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = out.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let f = &f;
        for (k, part) in out.chunks_mut(chunk).enumerate() {
            scope.spawn(move || {
                for (j, v) in part.iter_mut().enumerate() {
                    *v = f(k * chunk + j);
                }
            });
        }
    });
}

/// The cells in an order where each receiver comes before its donors, with
/// the buffers that build it.
#[derive(Default)]
struct Stack {
    order: Vec<usize>,
    count: Vec<u32>,
    donors: Vec<usize>,
    fill: Vec<u32>,
}

impl Stack {
    fn build(&mut self, receiver: &[usize]) {
        let n = receiver.len();
        self.count.clear();
        self.count.resize(n + 1, 0);
        for (i, &r) in receiver.iter().enumerate() {
            if r != i {
                self.count[r + 1] += 1;
            }
        }
        for i in 0..n {
            self.count[i + 1] += self.count[i];
        }
        self.donors.clear();
        self.donors.resize(self.count[n] as usize, 0);
        self.fill.clear();
        self.fill.extend_from_slice(&self.count);
        for (i, &r) in receiver.iter().enumerate() {
            if r != i {
                self.donors[self.fill[r] as usize] = i;
                self.fill[r] += 1;
            }
        }
        self.order.clear();
        for (i, &r) in receiver.iter().enumerate() {
            if r == i {
                self.order.push(i);
                let mut k = self.order.len() - 1;
                while k < self.order.len() {
                    let c = self.order[k];
                    let (lo, hi) = (self.count[c] as usize, self.count[c + 1] as usize);
                    self.order.extend_from_slice(&self.donors[lo..hi]);
                    k += 1;
                }
            }
        }
    }
}

/// Moves ground down each wall that is steeper than its angle of failure,
/// in one sweep, backward if `back`. `fail` holds the largest drop to a cell next to it, in
/// meters. Only wet cells move.
fn fail_walls(
    heights: &mut [f32],
    side: usize,
    fail: &[f32],
    weights: &[f32],
    wet: &impl Fn(usize) -> bool,
    back: bool,
) {
    let n = heights.len();
    {
        for k in 0..n {
            let i = if back { n - 1 - k } else { k };
            if !wet(i) {
                continue;
            }
            let (row, x) = (i / side, i % side);
            let face = row / side;
            for (dr, dx, d) in NEIGHBORS {
                let (r, c) = (row as i64 + dr, x as i64 + dx);
                if c < 0 || c >= side as i64 || r < 0 {
                    continue;
                }
                let (r, c) = (r as usize, c as usize);
                let j = r * side + c;
                if r / side != face || j >= n || !wet(j) {
                    continue;
                }
                let excess = heights[i] - heights[j] - fail[i] * d;
                if excess > 0.0 {
                    let moved = 0.5 * excess * weights[i].min(weights[j]);
                    heights[i] -= moved;
                    heights[j] += moved;
                }
            }
        }
    }
}
