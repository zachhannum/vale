//! The canyons of a margin stroke.
//!
//! Turbidity currents and slides cut the canyons of a continental slope.
//! The erosion of a canyon floor grows with the slope above a threshold and
//! with the drainage area to a small power (Mitchell 2004). A canyon starts
//! where the drainage area passes a limit, and the walls between the canyons
//! only fail. Only steep ground feeds the flow, so the shelf and the rise
//! give no water. A few seeded heads also get the water of a
//! part of the shelf, and they cut back into it. The walls of a canyon fail
//! above an angle, so a canyon has a V profile.

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
const CHANNEL_KM2: f32 = 10.0;

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
const RATE: f32 = 1.0;

/// The angle above which a wall fails, in degrees, and the part of it that
/// a seeded noise changes.
const FAIL_DEG: f32 = 28.0;
const FAIL_NOISE: f32 = 0.25;

/// The size of the features of the roughness that the flow starts on, in
/// kilometers, at least 3 cells wide, and their heights as a part of the
/// drop of the slope over their size. The flow gathers in the troughs, so
/// the size sets the spacing of the canyons.
const ROUGH: [(f64, f32); 2] = [(12.0, 0.1), (4.5, 0.144)];

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
    /// The direction of each cell under a brush, and `None` elsewhere.
    pub dirs: &'a [Option<V3>],
    /// The weight of the brushes at each cell, from 0 to 1.
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
    let mut fail = vec![0f32; n];
    let mut fed = vec![false; n];
    let mut rough = vec![0f32; n];
    let start: Vec<f32> = heights.to_vec();
    let cut: Vec<f32> = start
        .iter()
        .map(|&h| {
            let depth = -f64::from(h) / -grid.shelf_m;
            smoothstep(SHELF_CUT.0, SHELF_CUT.1, depth) as f32
        })
        .collect();
    for i in 0..n {
        let Some(dir) = grid.dirs[i] else {
            continue;
        };
        let slope = steepest(&start, side, i).0 / (1000.0 * cell);
        for (k, &(km, part)) in ROUGH.iter().enumerate() {
            let km = km.max(least);
            let m = part * slope * 1000.0 * km as f32;
            rough[i] += m * noise(dir, km, SALT_ROUGH.wrapping_add(k as u32))[0] as f32;
        }
        let n_fail = noise(dir, least.max(5.0), SALT_FAIL)[0] as f32;
        let deg = FAIL_DEG * (1.0 + FAIL_NOISE * n_fail);
        fail[i] = deg.to_radians().tan() * 1000.0 * cell;
        let depth = -f64::from(start[i]);
        let shelf = depth > 0.2 * -grid.shelf_m && depth < -grid.shelf_m;
        fed[i] = shelf && noise(dir, FED_KM, SALT_FED)[0] > 1.0 - 2.0 * FED_PART;
    }
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
    let mut order = Vec::with_capacity(n);
    let mut flow = vec![0f32; n];
    for _ in 0..STEPS {
        for (i, r) in receiver.iter_mut().enumerate() {
            *r = receiver_of(heights, side, i, grid.seed, wander);
        }
        stack(&receiver, &mut order);
        flow.copy_from_slice(&water);
        for &i in order.iter().rev() {
            let r = receiver[i];
            if r != i {
                flow[r] += flow[i];
            }
        }
        // The erosion, from the outlets up, toward a floor at the threshold
        // slope below the receiver (Braun and Willett 2013).
        for &i in &order {
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
        fail_walls(heights, side, &fail, grid.weights, &wet);
    }
    // The roughness goes, and only the canyons stay. No cell rises.
    for i in 0..n {
        heights[i] = start[i] + (heights[i] - start[i] - rough[i]).min(0.0);
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

/// The cells in an order where each receiver comes before its donors.
fn stack(receiver: &[usize], order: &mut Vec<usize>) {
    let n = receiver.len();
    let mut count = vec![0u32; n + 1];
    for (i, &r) in receiver.iter().enumerate() {
        if r != i {
            count[r + 1] += 1;
        }
    }
    for i in 0..n {
        count[i + 1] += count[i];
    }
    let mut donors = vec![0usize; count[n] as usize];
    let mut fill = count.clone();
    for (i, &r) in receiver.iter().enumerate() {
        if r != i {
            donors[fill[r] as usize] = i;
            fill[r] += 1;
        }
    }
    order.clear();
    for (i, &r) in receiver.iter().enumerate() {
        if r == i {
            order.push(i);
            let mut k = order.len() - 1;
            while k < order.len() {
                let c = order[k];
                order.extend_from_slice(&donors[count[c] as usize..count[c + 1] as usize]);
                k += 1;
            }
        }
    }
}

/// Moves ground down each wall that is steeper than its angle of failure,
/// in two sweeps. `fail` holds the largest drop to a cell next to it, in
/// meters. Only wet cells move.
fn fail_walls(
    heights: &mut [f32],
    side: usize,
    fail: &[f32],
    weights: &[f32],
    wet: &impl Fn(usize) -> bool,
) {
    let n = heights.len();
    for sweep in 0..2 {
        for k in 0..n {
            let i = if sweep == 0 { k } else { n - 1 - k };
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
