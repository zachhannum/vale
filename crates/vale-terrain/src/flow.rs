//! The rivers of a heightmap.
//!
//! A coarse copy of the heightmap gives the flow of water from each land cell
//! to the sea. The cells that drain a large area are rivers. The channel map
//! holds the distance from each of its texels to the nearest river, and the
//! size of that river.

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

const NONE: u32 = u32::MAX;

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
    /// Fills each pit to the level of its rim, from the coast to the land
    /// inside. Then each cell drains to its steepest neighbor that is lower
    /// in the filled land. A world with no ocean or no land has no flow.
    pub fn new(heights: &CoarseHeights) -> FlowMap {
        let m = heights.m;
        let count = FACES * m * m;
        let levels = &heights.levels;
        let sea = sea();
        let is_land = |i: usize| levels[i] >= sea;
        let mut flow = FlowMap {
            m,
            receiver: vec![NONE; count],
            area: vec![0.0; count],
        };
        let lands = levels.iter().filter(|&&level| level >= sea).count();
        if lands == 0 || lands == count {
            return flow;
        }

        // The heap gives the lowest cell first. Of two cells at one level,
        // it gives first the cell that came in first. Thus a flat drains to
        // the nearest way out.
        let mut heap = BinaryHeap::new();
        let mut pushes = 0u32;
        let mut filled = levels.clone();
        let receiver = &mut flow.receiver;
        for i in (0..count).filter(|&i| is_land(i)) {
            let near = neighbors(m, i);
            if let Some(&ocean) = near.iter().find(|&&j| !is_land(j as usize)) {
                receiver[i] = ocean;
                heap.push(Reverse((levels[i], pushes, i as u32)));
                pushes += 1;
            }
        }
        // The land cells in the order that they leave the heap.
        let mut order: Vec<u32> = Vec::with_capacity(lands);
        let mut done = vec![false; count];
        while let Some(Reverse((level, _, i))) = heap.pop() {
            let i = i as usize;
            let mut steepest = 0.0;
            for (k, &j) in neighbors(m, i).iter().enumerate() {
                let j = j as usize;
                if is_land(j) && !done[j] {
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
                let drop = f64::from(level) - f64::from(filled[j]);
                let slope = if k < 4 { drop } else { drop * FRAC_1_SQRT_2 };
                if slope > steepest {
                    steepest = slope;
                    receiver[i] = j as u32;
                }
            }
            done[i] = true;
            order.push(i as u32);
        }

        // Each receiver left the heap before its cells, so the reverse order
        // goes downstream.
        let solid = solid_angles(m);
        let mut area = vec![0.0f64; count];
        for &i in &order {
            area[i as usize] = solid[i as usize % (m * m)];
        }
        for &i in order.iter().rev() {
            let to = receiver[i as usize] as usize;
            if is_land(to) {
                area[to] += area[i as usize];
            }
        }
        for (out, area) in flow.area.iter_mut().zip(area) {
            *out = area as f32;
        }
        flow
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
        self.area(face, x, y) >= RIVER_MIN_AREA
    }

    /// The river cells as nodes, and the first ocean cell after each river.
    fn rivers(&self) -> Rivers {
        let m = self.m;
        let is_river = |i: usize| f64::from(self.area[i]) >= RIVER_MIN_AREA;
        let dir = |i: usize| {
            let (face, x, y) = split(m, i);
            cell_dir(m, face, x, y)
        };
        let mut rivers = Rivers::default();
        // The node of each river cell.
        let mut node = vec![NONE; self.area.len()];
        for i in (0..self.area.len()).filter(|&i| is_river(i)) {
            node[i] = rivers.cells.len() as u32;
            rivers.cells.push(i as u32);
            rivers.pos.push(dir(i));
        }
        let count = rivers.cells.len();
        rivers.down = vec![NONE; count];
        rivers.donor = vec![NONE; count];
        for k in 0..count {
            let to = self.receiver[rivers.cells[k] as usize] as usize;
            if node[to] == NONE {
                // The receiver is ocean. It gets a node that has no cell.
                rivers.down[k] = rivers.pos.len() as u32;
                rivers.pos.push(dir(to));
                continue;
            }
            let down = node[to] as usize;
            rivers.down[k] = down as u32;
            let area = |k: usize| self.area[rivers.cells[k] as usize];
            let donor = rivers.donor[down];
            if donor == NONE || area(k) > area(donor as usize) {
                rivers.donor[down] = k as u32;
            }
        }
        rivers
    }

    /// Draws each river into a channel map with 2 texels for each cell along
    /// one axis.
    pub fn channels(&self) -> ChannelMap {
        let mut map = ChannelMap::empty(2 * self.m);
        let mut rivers = self.rivers();
        rivers.smooth();
        for (k, &cell) in rivers.cells.iter().enumerate() {
            let flow = flow_byte(f64::from(self.area[cell as usize]));
            map.draw(rivers.pos[k], rivers.pos[rivers.down[k] as usize], flow);
        }
        map
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
    /// Moves each node toward the nodes before and after it. The first node
    /// and the last node of a river stay in place.
    fn smooth(&mut self) {
        for _ in 0..SMOOTH_PASSES {
            let old = self.pos.clone();
            for k in 0..self.cells.len() {
                if self.donor[k] == NONE {
                    continue;
                }
                let ends = add(old[self.down[k] as usize], old[self.donor[k] as usize]);
                self.pos[k] = normalize(add(scale(old[k], 2.0), ends));
            }
        }
    }
}

fn flow_byte(area: f64) -> u8 {
    let octaves = (area / RIVER_MIN_AREA).log2();
    (255.0 * octaves / RIVER_OCTAVES).round().clamp(1.0, 255.0) as u8
}

/// The rivers as a texture. Each texel has two bytes. The first byte is the
/// distance to the nearest river in channel texels, times `CHANNEL_SCALE`.
/// The second byte is the size of that river from 1 to 255, or 0 if no river
/// is near. A texel with no river near holds 255 and 0.
pub struct ChannelMap {
    size: usize,
    data: Vec<u8>,
}

impl ChannelMap {
    /// A map with no rivers, with `size` by `size` texels on each face.
    pub fn empty(size: usize) -> ChannelMap {
        assert!(size > 0, "the size must not be zero");
        ChannelMap {
            size,
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
        let blend =
            |a: [u8; 2], b: [u8; 2], t: f64| f64::from(a[0]) * (1.0 - t) + f64::from(b[0]) * t;
        let (top, bottom) = (blend(a, b, tx), blend(c, d, tx));
        let distance = (top * (1.0 - ty) + bottom * ty) / CHANNEL_SCALE;
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
        for face in 0..FACES {
            let axis = face / 2;
            let sign = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
            let project = |d: V3| {
                let depth = d[axis] * sign;
                let texel = |a: f64| (warp(a / depth) + 1.0) * 0.5 * k - 0.5;
                // A place within reach of the face has more depth than this.
                (depth > 0.2).then(|| (texel(d[(axis + 1) % 3]), texel(d[(axis + 2) % 3])))
            };
            let (Some((ax, ay)), Some((bx, by))) = (project(a), project(b)) else {
                continue;
            };
            let from = |a: f64, b: f64| (a.min(b) - CHANNEL_REACH).ceil().max(0.0);
            let to = |a: f64, b: f64| (a.max(b) + CHANNEL_REACH).floor().min(k - 1.0);
            let (x0, x1, y0, y1) = (from(ax, bx), to(ax, bx), from(ay, by), to(ay, by));
            if x0 > x1 || y0 > y1 {
                continue;
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
                    let distance = (px - t * dx).hypot(py - t * dy);
                    let byte = (distance * CHANNEL_SCALE).round();
                    if byte >= 255.0 {
                        continue;
                    }
                    let byte = byte as u8;
                    let at = ((face * self.size + y) * self.size + x) * 2;
                    let old = &mut self.data[at..at + 2];
                    if byte < old[0] || (byte == old[0] && flow > old[1]) {
                        old.copy_from_slice(&[byte, flow]);
                    }
                }
            }
        }
    }
}

/// The channel map of the rivers on `heights`.
pub fn channel_map(heights: &CoarseHeights) -> ChannelMap {
    FlowMap::new(heights).channels()
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
            assert!(a[0].abs_diff(b[0]) <= 32, "row {j}: {a:?} {b:?}");
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
                    let expected = (j as f64 + 0.5) * CHANNEL_SCALE;
                    if expected < 250.0 {
                        assert!((f64::from(distance) - expected).abs() <= 2.0, "{x} {j}");
                        assert!(flow > 0);
                    } else {
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
