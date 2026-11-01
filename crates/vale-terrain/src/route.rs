//! The terrain fit of a road. An A* search finds the path of least effort
//! over the height field, in a corridor around a stroke. A second pass
//! straightens the path.
//!
//! The search runs on the texels of the cube map. A texel has 16 neighbors:
//! the 8 texels around it and the 8 texels at a knight move. A neighbor past
//! a face edge is the texel of the next face at the same direction.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::f64::consts::FRAC_PI_2;
use std::hash::{BuildHasherDefault, Hasher};

use crate::bands::SEA_LEVEL;
use crate::cube::{face_dir, unwarp};
use crate::heightmap::Heightmap;
use crate::math::{V3, add, angle, dot, length, scale};

/// The steepest grade of a road step on land, as rise over run.
pub const MAX_GRADE: f64 = 0.10;

/// The cost factor of a step at the grade limit, over a flat step.
const SLOPE: f64 = 4.0;
/// The cost factor of a step along a road.
const ROAD: f64 = 0.5;
/// The cost of one texel of water.
const WATER_STEP: f64 = 3.0;
/// The fixed cost of a step from land into water.
const CROSSING: f64 = 30.0;
/// The weight of the distance from the stroke at the middle of the slider.
const STROKE: f64 = 4.0;
/// The half width of the corridor at the sketch end and at the terrain end
/// of the slider, in texels.
const WIDTH_MIN: f64 = 6.0;
const WIDTH_MAX: f64 = 32.0;
/// The lowest slider value that the search uses.
const TERRAIN_MIN: f64 = 0.02;
/// The spacing of the samples along a line, in texels.
const SAMPLE: f64 = 0.5;
/// The spacing of the samples of the straightening pass, in texels.
const PULL_SAMPLE: f64 = 0.25;
/// The longest distance between two output points, in texels.
const OUTPUT_STEP: f64 = 2.25;
/// A straight line replaces a part of the path when it costs at most this
/// factor of the part.
const PULL_SLACK: f64 = 1.02;
/// The most path steps that one straight line replaces.
const PULL_SPAN: usize = 32;
/// The stroke samples on each side of the last nearest sample that the
/// corridor compares with a texel.
const NEAR_WINDOW: usize = 6;
/// A texel outside the corridor.
const OUTSIDE: u32 = u32::MAX;

/// The 16 neighbors of a texel. The first 8 are the texels around it.
const OFFSETS: [(i64, i64); 16] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
    (2, 1),
    (2, -1),
    (-2, 1),
    (-2, -1),
    (1, 2),
    (1, -2),
    (-1, 2),
    (-1, -2),
];

/// The settings of a terrain fit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteSpec {
    /// The radius of the world, in meters.
    pub radius_m: f64,
    /// The balance between the sketch and the terrain, from 0 to 1. At 0, the
    /// road is the stroke, and the caller does not run the fit.
    pub terrain: f64,
}

/// Whether a direction is in the sea or in a river of the heightmap.
pub fn is_water(map: &Heightmap, d: V3) -> bool {
    if map.bands.value(map.sample(d)) < SEA_LEVEL {
        return true;
    }
    map.channels().is_some_and(|channels| {
        let (distance, size) = channels.at(d);
        size > 0 && distance < 0.5
    })
}

/// The road line of least effort between the ends of `stroke`, near the
/// stroke. `water` tells the sea and the rivers. A step along `roads` costs
/// less. The line starts and ends at the ends of the stroke, and two
/// neighbor points are at most a few texels apart. The result is `None`
/// for a stroke of less than 2 points.
pub fn fit_route(
    map: &Heightmap,
    stroke: &[V3],
    spec: &RouteSpec,
    water: &dyn Fn(V3) -> bool,
    roads: &[Vec<V3>],
) -> Option<Vec<V3>> {
    fit(map, stroke, spec, water, roads).map(|fit| fit.line)
}

/// A fitted line, and the counts that the tests read.
struct Fit {
    line: Vec<V3>,
    /// The texels in the corridor.
    #[cfg_attr(not(test), expect(dead_code))]
    corridor: usize,
    /// The texels that the search expanded.
    #[cfg_attr(not(test), expect(dead_code))]
    expanded: usize,
}

fn fit(
    map: &Heightmap,
    stroke: &[V3],
    spec: &RouteSpec,
    water: &dyn Fn(V3) -> bool,
    roads: &[Vec<V3>],
) -> Option<Fit> {
    let (&first, &last) = (stroke.first()?, stroke.last()?);
    if stroke.len() < 2 {
        return None;
    }
    let b = spec.terrain.clamp(TERRAIN_MIN, 1.0);
    let width = WIDTH_MIN + (WIDTH_MAX - WIDTH_MIN) * b;
    let mut corridor = Corridor::new(map, &resample(stroke, map), width, water);
    let any_road = corridor.mark_roads(roads);
    let costs = Costs {
        radius_m: spec.radius_m,
        stroke: STROKE * ((1.0 - b) / b).powi(2) / (width * width),
        rate: if any_road { ROAD } else { 1.0 },
    };
    let start = corridor.slot_at(first)?;
    let goal = corridor.slot_at(last)?;
    let mut search = Search::new(&corridor, costs);
    let (path, limit) = match search.run(start, goal, true) {
        Some(path) => (path, true),
        None => (search.run(start, goal, false)?, false),
    };
    let line = search.pull(&path, first, last, limit);
    Some(Fit {
        line,
        corridor: corridor.len(),
        expanded: search.expanded,
    })
}

/// The points of a stroke at a spacing of at most `SAMPLE` texels.
fn resample(stroke: &[V3], map: &Heightmap) -> Vec<V3> {
    let texel = texel_angle(map);
    let mut out = vec![stroke[0]];
    for pair in stroke.windows(2) {
        let parts = (angle(pair[0], pair[1]) / (SAMPLE * texel)).ceil().max(1.0) as usize;
        out.extend((1..=parts).map(|i| slerp(pair[0], pair[1], i as f64 / parts as f64)));
    }
    out
}

/// The angle of a texel at the center of a face, in radians.
fn texel_angle(map: &Heightmap) -> f64 {
    FRAC_PI_2 / map.face_size() as f64
}

/// The point at `t` on the short arc from `a` to `b`.
fn slerp(a: V3, b: V3, t: f64) -> V3 {
    let w = angle(a, b);
    if w < 1e-12 {
        return a;
    }
    let s = w.sin();
    add(
        scale(a, ((1.0 - t) * w).sin() / s),
        scale(b, (t * w).sin() / s),
    )
}

fn chord(a: V3, b: V3) -> f64 {
    length(add(a, scale(b, -1.0)))
}

/// The distance from `p` to the segment from `a` to `b`, in the units of
/// the vectors.
fn segment_distance(p: V3, a: V3, b: V3) -> f64 {
    let ab = add(b, scale(a, -1.0));
    let ap = add(p, scale(a, -1.0));
    let len2 = dot(ab, ab);
    let t = if len2 > 0.0 {
        (dot(ap, ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    chord(p, add(a, scale(ab, t)))
}

/// A hasher for texel keys.
#[derive(Default)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u32(u32::from(byte) ^ (self.0 as u32));
        }
    }

    fn write_u32(&mut self, value: u32) {
        let h = u64::from(value).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 = h ^ (h >> 32);
    }
}

type KeyMap = HashMap<u32, u32, BuildHasherDefault<KeyHasher>>;

/// The texels near a stroke, with the values that the search reads. A slot
/// is the index of a texel in the arrays.
struct Corridor<'a> {
    map: &'a Heightmap,
    n: usize,
    texel: f64,
    slots: KeyMap,
    keys: Vec<u32>,
    dirs: Vec<V3>,
    meters: Vec<f64>,
    water: Vec<bool>,
    /// The distance from the stroke, in texels.
    distance: Vec<f64>,
    road: Vec<bool>,
}

impl<'a> Corridor<'a> {
    /// The texels at most `width` texels from the stroke samples, from a
    /// breadth-first fill out from the texels of the samples.
    fn new(
        map: &'a Heightmap,
        samples: &[V3],
        width: f64,
        water: &dyn Fn(V3) -> bool,
    ) -> Corridor<'a> {
        let n = map.face_size();
        assert!(6 * n * n <= OUTSIDE as usize, "face too large for a route");
        let mut corridor = Corridor {
            map,
            n,
            texel: texel_angle(map),
            slots: KeyMap::default(),
            keys: Vec::new(),
            dirs: Vec::new(),
            meters: Vec::new(),
            water: Vec::new(),
            distance: Vec::new(),
            road: Vec::new(),
        };
        let mut near = Vec::new();
        let mut queue = VecDeque::new();
        for (i, &s) in samples.iter().enumerate() {
            let key = corridor.key_at(s);
            if corridor.slots.contains_key(&key) {
                continue;
            }
            let dir = corridor.dir_of(key);
            let (distance, index) = nearest(samples, dir, i, corridor.texel);
            let slot = corridor.add(key, distance, water);
            near.push(index);
            queue.push_back(slot);
        }
        while let Some(slot) = queue.pop_front() {
            let key = corridor.keys[slot];
            for &(dx, dy) in &OFFSETS[..8] {
                let next = corridor.neighbor(key, dx, dy);
                if corridor.slots.contains_key(&next) {
                    continue;
                }
                let dir = corridor.dir_of(next);
                let (distance, index) = nearest(samples, dir, near[slot], corridor.texel);
                if distance <= width {
                    let added = corridor.add(next, distance, water);
                    near.push(index);
                    queue.push_back(added);
                } else {
                    corridor.slots.insert(next, OUTSIDE);
                }
            }
        }
        corridor
    }

    fn len(&self) -> usize {
        self.keys.len()
    }

    fn add(&mut self, key: u32, distance: f64, water: &dyn Fn(V3) -> bool) -> usize {
        let slot = self.keys.len();
        let (face, x, y) = self.decode(key);
        let dir = self.map.texel_dir(face, x, y);
        let level = self.map.get(face, x as i64, y as i64);
        self.slots.insert(key, slot as u32);
        self.keys.push(key);
        self.dirs.push(dir);
        self.meters.push(self.map.bands.value(level));
        self.water.push(water(dir));
        self.distance.push(distance);
        self.road.push(false);
        slot
    }

    /// Marks the texels of the corridor under the lines of `roads`. The
    /// result is whether any texel is under a road.
    fn mark_roads(&mut self, roads: &[Vec<V3>]) -> bool {
        let mut any = false;
        for road in roads {
            for pair in road.windows(2) {
                let parts = (angle(pair[0], pair[1]) / (SAMPLE * self.texel))
                    .ceil()
                    .max(1.0) as usize;
                for i in 0..=parts {
                    let p = slerp(pair[0], pair[1], i as f64 / parts as f64);
                    if let Some(slot) = self.slot_at(p) {
                        self.road[slot] = true;
                        any = true;
                    }
                }
            }
        }
        any
    }

    fn key(&self, face: usize, x: usize, y: usize) -> u32 {
        ((face * self.n + y) * self.n + x) as u32
    }

    fn decode(&self, key: u32) -> (usize, usize, usize) {
        let key = key as usize;
        (key / (self.n * self.n), key % self.n, key / self.n % self.n)
    }

    fn dir_of(&self, key: u32) -> V3 {
        let (face, x, y) = self.decode(key);
        self.map.texel_dir(face, x, y)
    }

    fn key_at(&self, d: V3) -> u32 {
        let (face, x, y) = self.map.texel_of(d);
        self.key(face, x, y)
    }

    /// The key of the texel `dx` and `dy` texels from a texel. Past a face
    /// edge, the texel grid continues, and the texel is the one of the next
    /// face at the same direction.
    fn neighbor(&self, key: u32, dx: i64, dy: i64) -> u32 {
        let (face, x, y) = self.decode(key);
        let n = self.n as i64;
        let (nx, ny) = (x as i64 + dx, y as i64 + dy);
        if (0..n).contains(&nx) && (0..n).contains(&ny) {
            return self.key(face, nx as usize, ny as usize);
        }
        let flat = |i: i64| unwarp((i as f64 + 0.5) / self.n as f64 * 2.0 - 1.0);
        self.key_at(face_dir(face, flat(nx), flat(ny)))
    }

    fn slot(&self, key: u32) -> Option<usize> {
        match self.slots.get(&key) {
            Some(&slot) if slot != OUTSIDE => Some(slot as usize),
            _ => None,
        }
    }

    fn slot_at(&self, d: V3) -> Option<usize> {
        self.slot(self.key_at(d))
    }
}

/// The distance in texels from `p` to the stroke near sample `around`, and
/// the nearest sample.
fn nearest(samples: &[V3], p: V3, around: usize, texel: f64) -> (f64, usize) {
    let lo = around.saturating_sub(NEAR_WINDOW);
    let hi = (around + NEAR_WINDOW).min(samples.len() - 1);
    let mut best = (f64::INFINITY, around);
    for i in lo..=hi {
        let d = if i < hi {
            segment_distance(p, samples[i], samples[i + 1])
        } else {
            chord(p, samples[i])
        };
        if d < best.0 {
            best = (d, i);
        }
    }
    (best.0 / texel, best.1)
}

/// The weights of the cost of a step.
#[derive(Clone, Copy)]
struct Costs {
    radius_m: f64,
    /// The weight of the squared distance from the stroke, per texel of
    /// length and per squared texel of distance.
    stroke: f64,
    /// The lowest cost of one texel of length.
    rate: f64,
}

/// An entry of the open list, with the lowest estimate first.
struct Open {
    f: f64,
    g: f64,
    slot: u32,
}

impl PartialEq for Open {
    fn eq(&self, other: &Open) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Open {}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Open) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Open {
    fn cmp(&self, other: &Open) -> Ordering {
        other.f.total_cmp(&self.f).then(self.g.total_cmp(&other.g))
    }
}

/// A path of the search, from the start to the goal.
struct Path {
    slots: Vec<usize>,
    /// The cost from the start to each point.
    g: Vec<f64>,
    /// Whether the step to each point is in water.
    wet: Vec<bool>,
}

struct Search<'c, 'a> {
    corridor: &'c Corridor<'a>,
    costs: Costs,
    /// For each neighbor, the two neighbors that the step to it passes.
    between: [Option<(usize, usize)>; 16],
    expanded: usize,
}

impl<'c, 'a> Search<'c, 'a> {
    fn new(corridor: &'c Corridor<'a>, costs: Costs) -> Search<'c, 'a> {
        let find = |o: (i64, i64)| OFFSETS.iter().position(|&p| p == o);
        let between = OFFSETS.map(|(dx, dy)| match (dx.abs(), dy.abs()) {
            (1, 1) => Some((find((dx, 0))?, find((0, dy))?)),
            (2, 1) => Some((find((dx / 2, 0))?, find((dx / 2, dy))?)),
            (1, 2) => Some((find((0, dy / 2))?, find((dx, dy / 2))?)),
            _ => None,
        });
        Search {
            corridor,
            costs,
            between,
            expanded: 0,
        }
    }

    /// The grade of the step from slot `a` to slot `b`.
    fn grade(&self, a: usize, b: usize) -> f64 {
        let c = self.corridor;
        let run = chord(c.dirs[a], c.dirs[b]) * self.costs.radius_m;
        if run > 0.0 {
            (c.meters[b] - c.meters[a]).abs() / run
        } else {
            0.0
        }
    }

    /// The cost of `length` texels of travel from slot `a` to slot `b`,
    /// without the distance from the stroke. `wet` is whether the step is
    /// in water.
    fn travel(&self, a: usize, b: usize, length: f64, wet: bool) -> f64 {
        let c = self.corridor;
        if wet {
            let crossing = if c.water[a] { 0.0 } else { CROSSING };
            return length * WATER_STEP + crossing;
        }
        let g = self.grade(a, b) / MAX_GRADE;
        let road = if c.road[a] && c.road[b] { ROAD } else { 1.0 };
        length * (1.0 + SLOPE * g * g) * road
    }

    /// The cost of the step from slot `a` to slot `b`, and whether it is in
    /// water. `beside` is whether the step passes a water texel, and such a
    /// step is in water. The result is `None` for a land step above the
    /// grade limit.
    fn step(&self, a: usize, b: usize, beside: bool, limit: bool) -> Option<(f64, bool)> {
        let c = self.corridor;
        let wet = c.water[a] || c.water[b] || beside;
        if limit && !wet && self.grade(a, b) > MAX_GRADE {
            return None;
        }
        let length = chord(c.dirs[a], c.dirs[b]) / c.texel;
        let d = 0.5 * (c.distance[a] + c.distance[b]);
        let stroke = length * self.costs.stroke * d * d;
        Some((self.travel(a, b, length, wet) + stroke, wet))
    }

    fn heuristic(&self, slot: usize, goal: usize) -> f64 {
        let c = self.corridor;
        chord(c.dirs[slot], c.dirs[goal]) / c.texel * self.costs.rate
    }

    /// The path of least cost from `start` to `goal`, with or without the
    /// grade limit.
    fn run(&mut self, start: usize, goal: usize, limit: bool) -> Option<Path> {
        let c = self.corridor;
        let len = c.len();
        let mut g = vec![f64::INFINITY; len];
        let mut parent = vec![OUTSIDE; len];
        let mut wet = vec![false; len];
        let mut closed = vec![false; len];
        let mut open = BinaryHeap::new();
        g[start] = 0.0;
        open.push(Open {
            f: self.heuristic(start, goal),
            g: 0.0,
            slot: start as u32,
        });
        while let Some(entry) = open.pop() {
            let s = entry.slot as usize;
            if closed[s] || entry.g > g[s] {
                continue;
            }
            closed[s] = true;
            self.expanded += 1;
            if s == goal {
                break;
            }
            let key = c.keys[s];
            let near = OFFSETS.map(|(dx, dy)| c.slot(c.neighbor(key, dx, dy)));
            for (i, &next) in near.iter().enumerate() {
                let Some(t) = next else { continue };
                if t == s || closed[t] {
                    continue;
                }
                let wet_side = |side: Option<usize>| side.is_some_and(|side| c.water[side]);
                let beside =
                    self.between[i].is_some_and(|(p, q)| wet_side(near[p]) || wet_side(near[q]));
                let Some((cost, step_wet)) = self.step(s, t, beside, limit) else {
                    continue;
                };
                let next_g = g[s] + cost;
                if next_g < g[t] {
                    g[t] = next_g;
                    parent[t] = s as u32;
                    wet[t] = step_wet;
                    open.push(Open {
                        f: next_g + self.heuristic(t, goal),
                        g: next_g,
                        slot: t as u32,
                    });
                }
            }
        }
        if !closed[goal] {
            return None;
        }
        let mut slots = vec![goal];
        while let Some(&s) = slots.last()
            && s != start
        {
            slots.push(parent[s] as usize);
        }
        slots.reverse();
        Some(Path {
            g: slots.iter().map(|&s| g[s]).collect(),
            wet: slots.iter().map(|&s| s != start && wet[s]).collect(),
            slots,
        })
    }

    /// The path as a line from `first` to `last`. A straight line replaces a
    /// part of the path when it stays in the corridor, keeps the grade limit
    /// at the output points, crosses water as many times, and costs about
    /// the same.
    fn pull(&self, path: &Path, first: V3, last: V3, limit: bool) -> Vec<V3> {
        let c = self.corridor;
        let end = path.slots.len() - 1;
        let mut points: Vec<V3> = path.slots.iter().map(|&s| c.dirs[s]).collect();
        points[0] = first;
        points[end] = last;
        let mut out = vec![first];
        let mut i = 0;
        while i < end {
            let mut best = i + 1;
            let mut j = i + 2;
            while j <= end && j - i <= PULL_SPAN {
                let runs = wet_runs(path.wet[i + 1..=j].iter().copied());
                let fits = self.straight(points[i], points[j], limit).is_some_and(
                    |(cost, straight_runs)| {
                        straight_runs == runs && cost <= PULL_SLACK * (path.g[j] - path.g[i]) + 1e-9
                    },
                );
                if !fits {
                    break;
                }
                best = j;
                j += 1;
            }
            let parts = (angle(points[i], points[best]) / (OUTPUT_STEP * c.texel))
                .ceil()
                .max(1.0) as usize;
            out.extend(
                (1..=parts).map(|k| slerp(points[i], points[best], k as f64 / parts as f64)),
            );
            i = best;
        }
        out
    }

    /// The cost of the straight line from `a` to `b`, and the number of its
    /// parts in water. The result is `None` when the line leaves the
    /// corridor, or when two output points on it are a land step above the
    /// grade limit.
    fn straight(&self, a: V3, b: V3, limit: bool) -> Option<(f64, usize)> {
        let c = self.corridor;
        let span = angle(a, b) / c.texel;
        let samples = (span / PULL_SAMPLE).ceil().max(1.0) as usize;
        let spacing = span / samples as f64;
        let mut slots = Vec::with_capacity(samples + 1);
        let mut stroke = 0.0;
        let mut last_distance = None;
        for k in 0..=samples {
            let slot = c.slot_at(slerp(a, b, k as f64 / samples as f64))?;
            let d = c.distance[slot];
            if let Some(last) = last_distance {
                let mid: f64 = 0.5 * (last + d);
                stroke += spacing * self.costs.stroke * mid * mid;
            }
            last_distance = Some(d);
            if slots.last() != Some(&slot) {
                slots.push(slot);
            }
        }
        if limit {
            let parts = (span / OUTPUT_STEP).ceil().max(1.0) as usize;
            let mut prev = c.slot_at(a)?;
            for k in 1..=parts {
                let next = c.slot_at(slerp(a, b, k as f64 / parts as f64))?;
                let land = !c.water[prev] && !c.water[next];
                if land && prev != next && self.grade(prev, next) > MAX_GRADE {
                    return None;
                }
                prev = next;
            }
        }
        let steps = slots.len().saturating_sub(1);
        if steps == 0 {
            return Some((stroke, 0));
        }
        let each = span / steps as f64;
        let mut cost = stroke;
        let mut wet = Vec::with_capacity(steps);
        for pair in slots.windows(2) {
            let step_wet = c.water[pair[0]] || c.water[pair[1]];
            cost += self.travel(pair[0], pair[1], each, step_wet);
            wet.push(step_wet);
        }
        Some((cost, wet_runs(wet.into_iter())))
    }
}

/// The number of runs of wet steps.
fn wet_runs(wet: impl Iterator<Item = bool>) -> usize {
    let mut runs = 0;
    let mut before = false;
    for w in wet {
        if w && !before {
            runs += 1;
        }
        before = w;
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::{FACES, face_of, meters_to_level};
    use crate::math::{dir_to_lonlat, lonlat_to_dir};
    use std::time::Instant;

    const RADIUS: f64 = 1.0e6;

    fn spec(terrain: f64) -> RouteSpec {
        RouteSpec {
            radius_m: RADIUS,
            terrain,
        }
    }

    fn paint(n: usize, h: impl Fn(f64, f64) -> f64) -> Heightmap {
        let mut map = Heightmap::new(n, meters_to_level(200.0));
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n {
                    let (lon, lat) = dir_to_lonlat(map.texel_dir(face, x, y));
                    map.set(face, x, y, meters_to_level(h(lon, lat)));
                }
            }
        }
        map
    }

    fn dry(_: V3) -> bool {
        false
    }

    fn height(map: &Heightmap, d: V3) -> f64 {
        map.bands.value(map.sample(d))
    }

    /// Points along a line at most a quarter texel apart.
    fn dense(map: &Heightmap, line: &[V3]) -> Vec<V3> {
        let texel = texel_angle(map);
        let mut out = vec![line[0]];
        for pair in line.windows(2) {
            let parts = (angle(pair[0], pair[1]) / (0.25 * texel)).ceil().max(1.0) as usize;
            out.extend((1..=parts).map(|i| slerp(pair[0], pair[1], i as f64 / parts as f64)));
        }
        out
    }

    fn center(map: &Heightmap, d: V3) -> V3 {
        let (face, x, y) = map.texel_of(d);
        map.texel_dir(face, x, y)
    }

    /// The distance from `p` to a line, in texels.
    fn distance_to(map: &Heightmap, line: &[V3], p: V3) -> f64 {
        dense(map, line)
            .iter()
            .map(|&q| angle(p, q))
            .fold(f64::INFINITY, f64::min)
            / texel_angle(map)
    }

    /// The grades between the texels of neighbor points of a line.
    fn grades(map: &Heightmap, line: &[V3]) -> Vec<f64> {
        line.windows(2)
            .filter_map(|pair| {
                let (a, b) = (center(map, pair[0]), center(map, pair[1]));
                let run = chord(a, b) * RADIUS;
                (run > 0.0).then(|| (height(map, a) - height(map, b)).abs() / run)
            })
            .collect()
    }

    fn check_no_gap(map: &Heightmap, stroke: &[V3], line: &[V3]) {
        let texel = texel_angle(map);
        assert_eq!(line[0], stroke[0]);
        assert_eq!(line[line.len() - 1], stroke[stroke.len() - 1]);
        let mut progress = 0.0;
        for pair in line.windows(2) {
            let step = angle(pair[0], pair[1]) / texel;
            assert!(step <= 2.3, "step of {step} texels");
            let along = angle(stroke[0], pair[1]) / texel;
            assert!(along > progress - 0.5, "step back at {along} texels");
            progress = f64::max(progress, along);
        }
    }

    #[test]
    fn a_stroke_across_a_ridge_goes_through_the_pass() {
        let map = paint(256, |lon, lat| {
            let crest = 1700.0 - 1400.0 * (-((lat - 2.0) / 1.5).powi(2)).exp();
            300.0 + crest * (-(lon / 1.0).powi(2)).exp()
        });
        let stroke = [lonlat_to_dir(-8.0, 0.0), lonlat_to_dir(8.0, 0.0)];
        let line = fit_route(&map, &stroke, &spec(0.7), &dry, &[]).unwrap();
        let top = |line: &[V3]| {
            dense(&map, line)
                .iter()
                .map(|&d| height(&map, d))
                .fold(f64::MIN, f64::max)
        };
        let (route_top, stroke_top) = (top(&line), top(&stroke));
        assert!(
            route_top < stroke_top - 500.0,
            "route top {route_top} m, stroke top {stroke_top} m"
        );
        let pass = lonlat_to_dir(0.0, 2.0);
        let miss = distance_to(&map, &line, pass);
        assert!(miss < 2.0, "the route misses the pass by {miss} texels");
    }

    #[test]
    fn no_step_is_steeper_than_the_limit_when_a_path_below_it_exists() {
        let map = paint(256, |lon, lat| {
            let gap = 1.0 - (-((lat - 3.0) / 1.2).powi(2)).exp();
            200.0 + 2500.0 * gap * (-(lon / 0.6).powi(2)).exp()
        });
        let stroke = [lonlat_to_dir(-8.0, 0.0), lonlat_to_dir(8.0, 0.0)];
        let steep = |line: &[V3]| grades(&map, line).into_iter().fold(0.0, f64::max);
        assert!(steep(&dense(&map, &stroke)) > MAX_GRADE);
        let line = fit_route(&map, &stroke, &spec(0.5), &dry, &[]).unwrap();
        let worst = steep(&line);
        assert!(worst <= MAX_GRADE + 1e-9, "a step has a grade of {worst}");
    }

    #[test]
    fn a_wall_with_no_pass_still_gives_a_route() {
        let map = paint(256, |lon, _| 200.0 + 3000.0 * (-(lon / 0.6).powi(2)).exp());
        let stroke = [lonlat_to_dir(-8.0, 0.0), lonlat_to_dir(8.0, 0.0)];
        let line = fit_route(&map, &stroke, &spec(0.5), &dry, &[]).unwrap();
        check_no_gap(&map, &stroke, &line);
    }

    #[test]
    fn a_road_across_a_river_crosses_it_once() {
        let map = paint(256, |_, _| 200.0);
        let river = |d: V3| {
            let (lon, lat) = dir_to_lonlat(d);
            (lat - 2.5 * (std::f64::consts::PI * lon / 6.0).sin()).abs() < 0.8
        };
        let crossings = |line: &[V3]| {
            let wet: Vec<bool> = dense(&map, line)
                .iter()
                .map(|&d| river(center(&map, d)))
                .collect();
            wet.windows(2).filter(|w| !w[0] && w[1]).count()
        };
        let stroke = [lonlat_to_dir(-8.0, 0.0), lonlat_to_dir(8.0, 0.0)];
        assert_eq!(crossings(&stroke), 3);
        let line = fit_route(&map, &stroke, &spec(0.5), &river, &[]).unwrap();
        assert_eq!(crossings(&line), 1);
    }

    #[test]
    fn a_route_across_a_face_edge_has_no_gap() {
        let map = paint(256, |_, _| 200.0);
        let stroke = [lonlat_to_dir(40.0, 10.0), lonlat_to_dir(50.0, 12.0)];
        assert_ne!(face_of(stroke[0]).0, face_of(stroke[1]).0);
        let line = fit_route(&map, &stroke, &spec(0.5), &dry, &[]).unwrap();
        check_no_gap(&map, &stroke, &line);
    }

    #[test]
    fn a_route_across_180_degrees_has_no_gap() {
        let map = paint(256, |_, _| 200.0);
        let stroke = [lonlat_to_dir(175.0, -5.0), lonlat_to_dir(-175.0, 5.0)];
        let line = fit_route(&map, &stroke, &spec(0.5), &dry, &[]).unwrap();
        check_no_gap(&map, &stroke, &line);
        assert!(line.len() > 10);
    }

    #[test]
    fn a_route_follows_a_road_that_runs_the_same_way() {
        let map = paint(256, |_, _| 200.0);
        let off = 3.0 * 90.0 / 256.0;
        let road: Vec<V3> = (-6..=6)
            .map(|lon| lonlat_to_dir(f64::from(lon), off))
            .collect();
        let stroke = [lonlat_to_dir(-8.0, 0.0), lonlat_to_dir(8.0, 0.0)];
        let line = fit_route(&map, &stroke, &spec(0.5), &dry, std::slice::from_ref(&road)).unwrap();
        for lon in [-4.0, -2.0, 0.0, 2.0, 4.0] {
            let p = dense(&map, &line)
                .into_iter()
                .min_by(|&a, &b| {
                    let da = (dir_to_lonlat(a).0 - lon).abs();
                    da.total_cmp(&(dir_to_lonlat(b).0 - lon).abs())
                })
                .unwrap();
            let miss = distance_to(&map, &road, p);
            assert!(
                miss <= 1.0,
                "at {lon} degrees the route is {miss} texels off"
            );
        }
    }

    #[test]
    fn on_flat_ground_the_route_stays_near_the_stroke() {
        let map = paint(512, |_, _| 200.0);
        let stroke = [lonlat_to_dir(-5.0, -3.0), lonlat_to_dir(5.0, 1.0)];
        let line = fit_route(&map, &stroke, &spec(0.5), &dry, &[]).unwrap();
        let worst = dense(&map, &line)
            .into_iter()
            .map(|p| distance_to(&map, &stroke, p))
            .fold(0.0, f64::max);
        assert!(worst <= 1.5, "the route is {worst} texels from the stroke");
    }

    #[test]
    fn a_fit_visits_a_bounded_number_of_texels() {
        let n = 1024;
        let map = paint(n, |lon, lat| {
            300.0 + 400.0 * (lon * 1.3).sin() * (lat * 1.7).cos()
        });
        let texel = 90.0 / n as f64;
        let stroke = [
            lonlat_to_dir(-100.0 * texel, 0.0),
            lonlat_to_dir(100.0 * texel, 0.0),
        ];
        let fit = fit(&map, &stroke, &spec(0.5), &dry, &[]).unwrap();
        let width = WIDTH_MIN + (WIDTH_MAX - WIDTH_MIN) * 0.5;
        let bound = (200.0 + 2.0 * width + 2.0) * (2.0 * width + 3.0);
        assert!(
            (fit.corridor as f64) < bound,
            "{} texels in the corridor",
            fit.corridor
        );
        assert!(fit.expanded <= fit.corridor);
    }

    /// Run with `--ignored --nocapture` for the time of a fit of 1000
    /// texels on a face of 8192 texels.
    #[test]
    #[ignore]
    fn manual_a_fit_of_1000_texels_takes_less_than_100_ms() {
        let n = 8192;
        let mut map = Heightmap::new(n, meters_to_level(500.0));
        let radius_m = 6.371e6;
        for y in n / 2 - 120..n / 2 + 120 {
            for x in n / 2 - 560..n / 2 + 560 {
                let (fx, fy) = (x as f64, y as f64);
                let h = 500.0
                    + 600.0 * (fx / 23.0).sin() * (fy / 17.0).cos()
                    + 300.0 * (fx / 7.0 + fy / 11.0).sin();
                map.set(0, x, y, meters_to_level(h));
            }
        }
        let stroke = [
            map.texel_dir(0, n / 2 - 500, n / 2 - 10),
            map.texel_dir(0, n / 2 + 500, n / 2 + 10),
        ];
        let spec = RouteSpec {
            radius_m,
            terrain: 0.5,
        };
        let water = |d: V3| is_water(&map, d);
        let start = Instant::now();
        let fit = fit(&map, &stroke, &spec, &water, &[]).unwrap();
        let time = start.elapsed();
        println!(
            "{time:?}, {} texels in the corridor, {} expanded, {} points",
            fit.corridor,
            fit.expanded,
            fit.line.len()
        );
        assert!(time.as_millis() < 100, "the fit took {time:?}");
    }
}
