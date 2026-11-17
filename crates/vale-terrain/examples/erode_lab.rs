//! An experiment harness for the Erode mode and the Uplift mode. It paints
//! strokes on the CPU the way the app does, scores the result with measures
//! of drainage networks, and writes pictures of the heights.
//!
//! ```sh
//! cargo run --release -p vale-terrain --example erode_lab -- target/erode-lab
//! ```

use std::f64::consts::{FRAC_PI_2, SQRT_2};
use std::path::{Path, PathBuf};

use image::{Rgb, RgbImage};
use vale_terrain::{
    ChannelShape, CoarseHeights, ELEV_MAX, ELEV_MIN, ErodeBrush, ErodeTuning, Erosion, Heightmap,
    Limits, Mode, OuterFlow, Stamp, WINDOW_CELLS, Window, WindowHeights, face_dir, meters_to_level,
    talus_levels, unwarp,
};

const N: usize = 2048;
const SEED: u32 = 7;
const WORLD_KM: f64 = 6371.0;
/// The brush settings of the app: strength, erosion, hardness, talus.
const STRENGTH_M: f64 = 1500.0;
const EROSION: f64 = 0.5;
const HARDNESS: f64 = 0.3;
const TALUS_DEG: f64 = 0.5;
const STAMP_SPACING: f64 = 0.12;
const UPLIFT_GAIN: f64 = 1.0;
const UPLIFT_ERODE_RATE: f64 = 12.0;
const UPLIFT_STEPS: usize = 4;
const UPLIFT_WEATHER_EVERY: usize = 4;
const ERODE_RATE: f64 = 0.01;
const ERODE_SECONDS: f64 = 1.0 / 30.0;
const BRUSH_WINDOW_ROOM: f64 = 3.0;
/// The stamps of one request of an uplift stroke, and of one step of an
/// erode stroke.
const UPLIFT_GROUP: usize = 4;
const ERODE_GROUP: usize = 2;
const UPLIFT_PASSES: usize = 1;
const ERODE_PASSES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
enum StrokeMode {
    Uplift,
    Erode,
}

#[derive(Clone, Debug)]
struct Variant {
    name: String,
    /// The cells in the brush radius of `brush_window`.
    cells: f64,
    tuning: ErodeTuning,
    /// With two grids, the cells in the brush radius of the fine grid, its
    /// settings, and the factor of its rate.
    fine: Option<f64>,
    fine_rough: bool,
    fine_tuning: ErodeTuning,
    fine_rate: f64,
    /// Whether the fine cells get the rough ground of an uplift.
    fine_uplift: bool,
}

fn variant(name: &str, cells: f64, rough: bool) -> Variant {
    Variant {
        name: name.to_string(),
        cells,
        // The rough ground of the branch, and no noise of the flow.
        tuning: ErodeTuning {
            rough: if rough { 0.3 } else { 0.0 },
            route_noise: 0.0,
            ..ErodeTuning::default()
        },
        fine: None,
        fine_rough: false,
        fine_tuning: ErodeTuning {
            rough: 0.3,
            route_noise: 0.0,
            ..ErodeTuning::default()
        },
        fine_rate: 1.0,
        fine_uplift: false,
    }
}

/// The direction at a place on face 0, in texels. The center of the first
/// texel is at 0.
fn dir_at(x: f64, y: f64) -> [f64; 3] {
    let flat = |p: f64| unwarp((p + 0.5) / N as f64 * 2.0 - 1.0);
    face_dir(0, flat(x), flat(y))
}

/// The smooth ground: a tilted plateau with gentle hills, in meters.
fn ground_m(x: f64, y: f64) -> f64 {
    let tilt = 0.6 * (x * 0.94 + y * 0.34);
    let waves = [
        (420.0, 0.3, 1.1, 260.0),
        (610.0, 1.9, 2.3, 300.0),
        (300.0, 2.7, 0.4, 140.0),
        (870.0, 4.2, 5.0, 220.0),
    ];
    let hills: f64 = waves
        .iter()
        .map(|&(length, angle, phase, amp): &(f64, f64, f64, f64)| {
            let (s, c) = f64::sin_cos(angle);
            amp * ((x * c + y * s) / length * std::f64::consts::TAU + phase).sin()
        })
        .sum();
    300.0 + tilt + hills
}

fn start_map() -> Heightmap {
    let mut map = Heightmap::new(N, meters_to_level(0.0));
    for y in 0..N {
        for x in 0..N {
            map.set(0, x, y, meters_to_level(ground_m(x as f64, y as f64)));
        }
    }
    map
}

thread_local! {
    /// Whether the stroke of this thread has the second path.
    static ALT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The path of the stroke, in texels: an S curve through the face center,
/// about 7 radii long, at an angle to the texel grid.
fn path(radius: f64) -> Vec<(f64, f64)> {
    let length = 7.0 * radius;
    let (c, s) = if ALT.get() {
        (0.6f64, 0.8f64)
    } else {
        (0.93f64, -0.37f64)
    };
    let center = (N as f64 / 2.0, N as f64 / 2.0);
    (0..=200)
        .map(|k| {
            let t = k as f64 / 200.0 - 0.5;
            let along = t * length;
            let across = 0.5 * radius * (t * std::f64::consts::TAU).sin();
            (
                center.0 + along * c - across * s,
                center.1 + along * s + across * c,
            )
        })
        .collect()
}

/// The stamp centers of `passes` passes along the path, back and forth,
/// spaced as the app spaces them.
fn stamp_places(radius: f64, passes: usize) -> Vec<(f64, f64)> {
    let line = path(radius);
    let spacing = radius * STAMP_SPACING;
    let mut out = Vec::new();
    for pass in 0..passes {
        let mut points = line.clone();
        if pass % 2 == 1 {
            points.reverse();
        }
        let mut carry = 0.0;
        out.push(points[0]);
        for w in points.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            let mut at = spacing - carry;
            while at <= len {
                let t = at / len;
                out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
                at += spacing;
            }
            carry = len - (at - spacing);
        }
    }
    out
}

fn stamp(mode: Mode, center: [f64; 3], radius: f64, flow: f64, strength: f64) -> Stamp {
    Stamp {
        center,
        radius,
        hardness: HARDNESS,
        flow,
        mode,
        level: 0,
        strength,
        lower: 0.0,
        talus: talus_levels(TALUS_DEG, N, WORLD_KM * 1000.0),
        scale: 0.0,
        seed: SEED,
        motion: [0.0; 3],
        ridged: false,
        invert: false,
        relief: 0.0,
        grain: 0.0,
        channel: ChannelShape::default(),
        limits: Limits::default(),
    }
}

/// The window of `brush_window` of the app, with `cells` cells in the radius.
fn brush_window(center: [f64; 3], radius: f64, cells_in_radius: f64) -> Window {
    let texels = radius * N as f64 / FRAC_PI_2;
    let global = N / CoarseHeights::size_for(N);
    let mut cell = 1;
    while 2 * cell <= global && texels / (2 * cell) as f64 >= cells_in_radius {
        cell *= 2;
    }
    let cells = WINDOW_CELLS.min(N / cell);
    Window::centered(center, N, cell, cells)
}

fn near_side(window: &Window, center: [f64; 3], radius: f64) -> bool {
    let Some((u, v)) = window.place(center) else {
        return true;
    };
    let cell = FRAC_PI_2 * window.cell as f64 / window.face_size as f64;
    let cells = window.cells as f64;
    let room = (BRUSH_WINDOW_ROOM * radius / cell).min(cells / 4.0);
    u.min(v).min(cells - u).min(cells - v) < room
}

/// Makes or moves the window of the cells of a stroke as the app does.
fn track(
    erosion: &mut Option<(Erosion, Window)>,
    map: &Heightmap,
    cells: f64,
    first: [f64; 3],
    last: [f64; 3],
    radius: f64,
    outer: Option<&OuterFlow>,
) {
    match erosion {
        None => {
            let window = brush_window(first, radius, cells);
            let heights = WindowHeights::new(map, window);
            let e = match outer {
                Some(outer) => Erosion::window_in(&heights, outer),
                None => Erosion::window_alone(&heights),
            };
            *erosion = Some((e, window));
        }
        Some((e, window)) if near_side(window, last, radius) => {
            let next = brush_window(last, radius, cells);
            if next != *window {
                let heights = WindowHeights::new(map, next);
                *e = e.moved(&heights, outer);
                *window = next;
            }
        }
        Some(_) => {}
    }
}

/// The rate of the steps of one uplift request on the cells of `window`.
fn uplift_rate(window: &Window, radius: f64, stamps: usize) -> f64 {
    let cell = FRAC_PI_2 * window.cell as f64 / N as f64;
    let cells = radius / cell;
    EROSION * UPLIFT_ERODE_RATE * (cells / 16.0).sqrt() * stamps as f64
}

/// Paints one stroke on `map` as the app does. With `erode` false, an
/// uplift stroke only raises the texels.
fn paint(map: &mut Heightmap, mode: StrokeMode, radius_texels: f64, v: &Variant, erode: bool) {
    let radius = radius_texels * FRAC_PI_2 / N as f64;
    let span = (ELEV_MAX - ELEV_MIN) / 65535.0;
    let (passes, group) = match mode {
        StrokeMode::Uplift => (UPLIFT_PASSES, UPLIFT_GROUP),
        StrokeMode::Erode => (ERODE_PASSES, ERODE_GROUP),
    };
    let places = stamp_places(radius_texels, passes);
    let strength = STAMP_SPACING / span * STRENGTH_M * (1.0 + UPLIFT_GAIN * EROSION);
    let mut erosion: Option<(Erosion, Window)> = None;
    let mut fine: Option<(Erosion, Window)> = None;
    let outer = (mode == StrokeMode::Erode).then(|| OuterFlow::global(&CoarseHeights::new(map), N));
    let seed = Some(SEED);
    for chunk in places.chunks(group) {
        let stamps: Vec<Stamp> = chunk
            .iter()
            .map(|&(x, y)| stamp(Mode::Uplift, dir_at(x, y), radius, 1.0, strength))
            .collect();
        let lift = |map: &mut Heightmap| {
            if mode == StrokeMode::Uplift {
                for s in &stamps {
                    map.stamp(s);
                }
            }
        };
        if !erode {
            lift(map);
            continue;
        }
        let (first, last) = (stamps[0].center, stamps[stamps.len() - 1].center);
        track(
            &mut erosion,
            map,
            v.cells,
            first,
            last,
            radius,
            outer.as_ref(),
        );
        if let Some(cells) = v.fine {
            track(&mut fine, map, cells, first, last, radius, outer.as_ref());
        }
        // The cells of the worker read the ground before the stamps.
        lift(map);
        let brushes: Vec<ErodeBrush> = stamps
            .iter()
            .map(|s| ErodeBrush {
                center: s.center,
                radius: s.radius,
                hardness: s.hardness,
                flow: s.flow,
            })
            .collect();
        let (e, window) = erosion.as_mut().unwrap();
        e.set_tuning(v.tuning);
        e.set_rough(seed);
        let step = match mode {
            StrokeMode::Uplift => {
                let rate = uplift_rate(window, radius, stamps.len());
                e.uplift(&brushes, strength, SEED);
                e.steps(&brushes, rate, UPLIFT_STEPS)
            }
            StrokeMode::Erode => e.step(&brushes, STRENGTH_M * ERODE_RATE * ERODE_SECONDS),
        };
        let step = match &mut fine {
            Some((f, fw)) if fw.cell == window.cell => {
                // The grids are the same, so the fine cells only follow.
                let _ = f;
                step
            }
            None => step,
            Some((f, window)) => {
                let mut ft = v.fine_tuning;
                if !v.fine_rough {
                    ft.rough = 0.0;
                }
                f.set_tuning(ft);
                f.set_rough(Some(SEED));
                match mode {
                    StrokeMode::Uplift => {
                        let rate = uplift_rate(window, radius, stamps.len()) * v.fine_rate;
                        match v.fine_uplift {
                            true => f.uplift(&brushes, strength, SEED),
                            false => f.raise(&brushes, strength),
                        }
                        f.detail_steps(&brushes, rate, UPLIFT_STEPS, &step)
                    }
                    StrokeMode::Erode => {
                        let rate = STRENGTH_M * ERODE_RATE * ERODE_SECONDS * v.fine_rate;
                        f.detail_steps(&brushes, rate, 1, &step)
                    }
                }
            }
        };
        map.erode(&step);
        if mode == StrokeMode::Uplift {
            for s in stamps.iter().step_by(UPLIFT_WEATHER_EVERY) {
                map.stamp(&Stamp {
                    mode: Mode::Weather,
                    flow: s.flow * EROSION,
                    ..*s
                });
            }
        }
    }
}

/// The texels of a rectangle of face 0.
struct Region {
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    /// The texels under the stroke.
    mask: Vec<bool>,
}

fn region(radius: f64) -> Region {
    let line = path(radius);
    let margin = radius * 1.3;
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in &line {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    let (x0, y0) = ((x0 - margin) as usize, (y0 - margin) as usize);
    let (x1, y1) = ((x1 + margin) as usize, (y1 + margin) as usize);
    let (w, h) = (x1 - x0, y1 - y0);
    let inner = 0.8 * radius;
    let mut mask = vec![false; w * h];
    for (i, m) in mask.iter_mut().enumerate() {
        let (x, y) = ((x0 + i % w) as f64, (y0 + i / w) as f64);
        *m = line
            .iter()
            .any(|&(px, py)| (px - x).powi(2) + (py - y).powi(2) < inner * inner);
    }
    Region { x0, y0, w, h, mask }
}

fn heights(map: &Heightmap, r: &Region) -> Vec<f64> {
    let mut out = Vec::with_capacity(r.w * r.h);
    for y in 0..r.h {
        for x in 0..r.w {
            out.push(f64::from(map.get(0, (r.x0 + x) as i64, (r.y0 + y) as i64)));
        }
    }
    out
}

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

#[derive(Clone, Copy, Debug, Default)]
struct Measures {
    bifurcation: f64,
    hack: f64,
    concavity: f64,
    junction: f64,
    straight: f64,
    /// Horton's length ratio of the Strahler streams.
    length_ratio: f64,
    /// The median elongation ratio of Schumm of the small basins.
    elongation: f64,
    channels: usize,
    /// The mean rise of the texels under the stroke, as a part of the rise
    /// of the stroke without erosion. For an erode stroke, the mean drop in
    /// meters.
    kept: f64,
    /// The relief of the region, in meters: the 99th minus the 1st
    /// percentile.
    relief: f64,
}

/// The target ranges, and the width that a distance of 1 has.
const TARGETS: [(&str, f64, f64, f64); 7] = [
    ("Rb", 3.0, 5.0, 1.0),
    ("Hack h", 0.5, 0.6, 0.1),
    ("theta", 0.4, 0.6, 0.1),
    ("junction", 40.0, 75.0, 15.0),
    ("straight", 0.0, 0.15, 0.1),
    ("RL", 1.5, 3.5, 0.5),
    ("elongation", 0.5, 1.0, 0.1),
];

impl Measures {
    fn values(&self) -> [f64; 7] {
        [
            self.bifurcation,
            self.hack,
            self.concavity,
            self.junction,
            self.straight,
            self.length_ratio,
            self.elongation,
        ]
    }

    fn score(&self) -> f64 {
        self.values()
            .iter()
            .zip(TARGETS)
            .map(|(&x, (_, lo, hi, width))| {
                if x.is_nan() {
                    return 3.0;
                }
                (lo - x).max(x - hi).max(0.0) / width
            })
            .sum()
    }
}

fn fit(points: &[(f64, f64)]) -> f64 {
    let n = points.len() as f64;
    if n < 2.0 {
        return f64::NAN;
    }
    let (mx, my) = points
        .iter()
        .fold((0.0, 0.0), |(a, b), &(x, y)| (a + x / n, b + y / n));
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for &(x, y) in points {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx) * (x - mx);
    }
    sxy / sxx
}

/// The D8 flow of the texels of the mask, and the measures of its network.
/// Water leaves at the side of the mask. `min_area` is the area of a
/// channel, in texels. `reach` is the number of steps that a direction
/// spans.
fn measure(h: &[f64], r: &Region, min_area: f64, reach: usize) -> Measures {
    let (w, hh) = (r.w as i64, r.h as i64);
    let count = r.w * r.h;
    let neighbor = |i: usize, k: usize| -> Option<usize> {
        let (x, y) = ((i % r.w) as i64 + STEPS[k].0, (i / r.w) as i64 + STEPS[k].1);
        (x >= 0 && y >= 0 && x < w && y < hh).then(|| (y * w + x) as usize)
    };
    // A priority flood from the side of the mask fills the pits, with a
    // tiny rise across a flat.
    let mut filled = h.to_vec();
    let mut done = vec![false; count];
    let mut heap = std::collections::BinaryHeap::new();
    let key = |v: f64| std::cmp::Reverse((v * 1e6) as i64);
    for i in 0..count {
        if !r.mask[i] {
            continue;
        }
        let side = (0..8).any(|k| neighbor(i, k).is_none_or(|j| !r.mask[j]));
        if side {
            done[i] = true;
            heap.push((key(filled[i]), i));
        }
    }
    let mut order = Vec::new();
    while let Some((_, i)) = heap.pop() {
        order.push(i);
        for k in 0..8 {
            if let Some(j) = neighbor(i, k)
                && r.mask[j]
                && !done[j]
            {
                done[j] = true;
                filled[j] = filled[j].max(filled[i] + 1e-4);
                heap.push((key(filled[j]), j));
            }
        }
    }
    // Steepest descent on the filled ground. A cell at the side drains out.
    const OUT: usize = usize::MAX;
    let mut receiver = vec![OUT; count];
    for &i in &order {
        let mut best = 0.0;
        for k in 0..8 {
            let Some(j) = neighbor(i, k) else { continue };
            if !r.mask[j] {
                continue;
            }
            let run = if k < 4 { 1.0 } else { SQRT_2 };
            let s = (filled[i] - filled[j]) / run;
            if s > best {
                best = s;
                receiver[i] = j;
            }
        }
    }
    // The order from the outlets up: sort by the filled level.
    let mut up = order.clone();
    up.sort_by(|&a, &b| filled[a].total_cmp(&filled[b]));
    let mut area = vec![1.0f64; count];
    let mut length = vec![0.0f64; count];
    for &i in up.iter().rev() {
        if receiver[i] != OUT {
            let j = receiver[i];
            area[j] += area[i];
            let (dx, dy) = (
                (i % r.w) as f64 - (j % r.w) as f64,
                (i / r.w) as f64 - (j / r.w) as f64,
            );
            length[j] = length[j].max(length[i] + (dx * dx + dy * dy).sqrt());
        }
    }
    let channel = |i: usize| r.mask[i] && area[i] >= min_area;
    // Strahler orders, from the heads down.
    let mut order_of = vec![0u32; count];
    let mut top = vec![(0u32, 0u32); count];
    let mut streams = vec![0usize; 64];
    for &i in up.iter().rev() {
        if !channel(i) {
            continue;
        }
        let (most, times) = top[i];
        let (o, head) = match (most, times) {
            (0, _) => (1, true),
            (m, t) if t >= 2 => (m + 1, true),
            (m, _) => (m, false),
        };
        order_of[i] = o;
        if head {
            streams[o as usize] += 1;
        }
        let j = receiver[i];
        if j != OUT && channel(j) {
            let t = &mut top[j];
            if o > t.0 {
                *t = (o, 1);
            } else if o == t.0 {
                t.1 += 1;
            }
        }
    }
    let highest = streams.iter().rposition(|&n| n > 0).unwrap_or(0);
    let last = if highest >= 3 { highest - 1 } else { highest };
    let points: Vec<(f64, f64)> = (1..=last)
        .filter(|&o| streams[o] > 0)
        .map(|o| (o as f64, (streams[o] as f64).ln()))
        .collect();
    let bifurcation = (-fit(&points)).exp();
    // The mean length of the streams of each order. A cell adds its step to
    // its receiver to the length of its stream.
    let mut lengths = vec![0.0f64; 64];
    for i in 0..count {
        if !channel(i) {
            continue;
        }
        let j = receiver[i];
        let step = if j == OUT {
            1.0
        } else {
            let (dx, dy) = (
                (i % r.w) as f64 - (j % r.w) as f64,
                (i / r.w) as f64 - (j / r.w) as f64,
            );
            (dx * dx + dy * dy).sqrt()
        };
        lengths[order_of[i] as usize] += step;
    }
    let points: Vec<(f64, f64)> = (1..=last)
        .filter(|&o| streams[o] > 0)
        .map(|o| (o as f64, (lengths[o] / streams[o] as f64).ln()))
        .collect();
    let length_ratio = fit(&points).exp();

    let cells: Vec<usize> = (0..count).filter(|&i| channel(i)).collect();
    let hack_points: Vec<(f64, f64)> = cells
        .iter()
        .filter(|&&i| length[i] > 0.0)
        .map(|&i| (area[i].ln(), length[i].ln()))
        .collect();
    let hack = fit(&hack_points);

    // Slope and area, in bins of log area.
    let mut slopes: Vec<(f64, f64)> = Vec::new();
    for &i in &cells {
        let j = receiver[i];
        if j == OUT {
            continue;
        }
        let run = if (i % r.w) != (j % r.w) && (i / r.w) != (j / r.w) {
            SQRT_2
        } else {
            1.0
        };
        let s = (h[i] - h[j]) / run;
        if s > 0.0 {
            slopes.push((area[i].ln(), s.ln()));
        }
    }
    let concavity = if slopes.len() > 20 {
        let (lo, hi) = slopes
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), &(x, _)| (a.min(x), b.max(x)));
        let bins = 10;
        let mut binned = vec![Vec::new(); bins];
        for &(x, y) in &slopes {
            let b = (((x - lo) / (hi - lo + 1e-9)) * bins as f64) as usize;
            binned[b.min(bins - 1)].push(y);
        }
        let points: Vec<(f64, f64)> = binned
            .iter_mut()
            .enumerate()
            .filter(|(_, ys)| ys.len() >= 5)
            .map(|(b, ys)| {
                ys.sort_by(f64::total_cmp);
                let x = lo + (b as f64 + 0.5) / bins as f64 * (hi - lo);
                (x, ys[ys.len() / 2])
            })
            .collect();
        -fit(&points)
    } else {
        f64::NAN
    };

    // Donors of each channel cell.
    let mut donors: Vec<Vec<usize>> = vec![Vec::new(); count];
    for &i in &cells {
        let j = receiver[i];
        if j != OUT && channel(j) {
            donors[j].push(i);
        }
    }
    let pos = |i: usize| ((i % r.w) as f64, (i / r.w) as f64);
    let trace_up = |mut i: usize| {
        for _ in 0..reach {
            match donors[i]
                .iter()
                .max_by(|&&a, &&b| area[a].total_cmp(&area[b]))
            {
                Some(&d) => i = d,
                None => break,
            }
        }
        i
    };
    let mut angles = Vec::new();
    for &j in &cells {
        if donors[j].len() < 2 {
            continue;
        }
        let mut ds = donors[j].clone();
        ds.sort_by(|&a, &b| area[b].total_cmp(&area[a]));
        let (pj, pa, pb) = (pos(j), pos(trace_up(ds[0])), pos(trace_up(ds[1])));
        let (ax, ay) = (pj.0 - pa.0, pj.1 - pa.1);
        let (bx, by) = (pj.0 - pb.0, pj.1 - pb.1);
        let dot = (ax * bx + ay * by) / ((ax * ax + ay * ay) * (bx * bx + by * by)).sqrt();
        angles.push(dot.clamp(-1.0, 1.0).acos().to_degrees());
    }
    let junction = angles.iter().sum::<f64>() / angles.len() as f64;

    // The part of the channel cells whose next `reach` steps all go in one
    // direction.
    let (mut straight, mut total) = (0usize, 0usize);
    for &i in &cells {
        let mut at = i;
        let mut first = None;
        let mut same = true;
        let mut steps = 0;
        while steps < reach {
            let j = receiver[at];
            if j == OUT || !r.mask[j] {
                break;
            }
            let d = (j as i64 - at as i64, 0);
            match first {
                None => first = Some(d),
                Some(f) if f != d => same = false,
                _ => {}
            }
            at = j;
            steps += 1;
        }
        if steps == reach {
            total += 1;
            straight += usize::from(same);
        }
    }
    Measures {
        bifurcation,
        hack,
        concavity,
        junction,
        straight: straight as f64 / total.max(1) as f64,
        length_ratio,
        elongation: {
            // The elongation ratio of Schumm of the small basins: the
            // diameter of the circle of the area over the length.
            let mut ratios: Vec<f64> = cells
                .iter()
                .filter(|&&i| area[i] <= 10.0 * min_area && length[i] > 0.0)
                .map(|&i| 2.0 * (area[i] / std::f64::consts::PI).sqrt() / length[i])
                .collect();
            ratios.sort_by(f64::total_cmp);
            ratios.get(ratios.len() / 2).copied().unwrap_or(f64::NAN)
        },
        channels: cells.len(),
        kept: f64::NAN,
        relief: f64::NAN,
    }
}

fn percentile(values: &[f64], p: f64) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p) as usize]
}

/// A turbo-like ramp from 0 to 1.
fn turbo(t: f64) -> Rgb<u8> {
    let t = t.clamp(0.0, 1.0);
    let r = 34.61 + t * (1172.33 - t * (10793.56 - t * (33300.12 - t * (38394.49 - t * 14825.05))));
    let g = 23.31 + t * (557.33 + t * (1225.33 - t * (3574.96 - t * (1073.77 + t * 707.56))));
    let b = 27.2 + t * (3211.1 - t * (15327.97 - t * (27814.0 - t * (22569.18 - t * 6838.66))));
    Rgb([
        r.clamp(0.0, 255.0) as u8,
        g.clamp(0.0, 255.0) as u8,
        b.clamp(0.0, 255.0) as u8,
    ])
}

/// The pictures of a region at 2 times its size: a ramp from the 0.5 to the
/// 99.5 percentile, and 20 bands of color over the same range.
fn pictures(h: &[f64], r: &Region) -> (RgbImage, RgbImage) {
    let lo = percentile(h, 0.005);
    let hi = percentile(h, 0.995);
    let ramp = RgbImage::from_fn(2 * r.w as u32, 2 * r.h as u32, |x, y| {
        let i = (y as usize / 2) * r.w + x as usize / 2;
        turbo((h[i] - lo) / (hi - lo))
    });
    let bands = RgbImage::from_fn(2 * r.w as u32, 2 * r.h as u32, |x, y| {
        let i = (y as usize / 2) * r.w + x as usize / 2;
        let t = ((h[i] - lo) / (hi - lo)).clamp(0.0, 0.9999);
        let band = (t * 20.0).floor();
        turbo((band + 0.5) / 20.0)
    });
    (ramp, bands)
}

/// A hillshade from the northwest at 2 times the size, for a person who
/// looks at the valleys.
fn shade(h: &[f64], r: &Region) -> RgbImage {
    let span = (ELEV_MAX - ELEV_MIN) / 65535.0;
    let texel_m = WORLD_KM * 1000.0 * FRAC_PI_2 / N as f64;
    let z = 40.0 * span / texel_m;
    let at = |x: usize, y: usize| h[y.min(r.h - 1) * r.w + x.min(r.w - 1)];
    RgbImage::from_fn(2 * r.w as u32, 2 * r.h as u32, |x, y| {
        let (x, y) = (x as usize / 2, y as usize / 2);
        let dx = (at(x + 1, y) - at(x.saturating_sub(1), y)) * z / 2.0;
        let dy = (at(x, y + 1) - at(x, y.saturating_sub(1))) * z / 2.0;
        let n = (dx * dx + dy * dy + 1.0).sqrt();
        let light = ((dx - dy) * -0.5 + std::f64::consts::FRAC_1_SQRT_2) / n;
        let v = (light.clamp(0.0, 1.0) * 255.0) as u8;
        Rgb([v, v, v])
    })
}

/// Joins the pictures of `names` side by side, for each mode, radius, and
/// style.
fn montage(out: &Path, names: &[&str]) {
    for mode in ["uplift", "erode"] {
        for radius in [60, 150] {
            for style in ["ramp", "bands", "shade"] {
                let pics: Vec<RgbImage> = names
                    .iter()
                    .filter_map(|name| {
                        image::open(out.join(format!(
                            "{}-{mode}-{radius}-{style}.png",
                            name.to_lowercase()
                        )))
                        .ok()
                        .map(|p| p.to_rgb8())
                    })
                    .collect();
                if pics.is_empty() {
                    continue;
                }
                let gap = 12;
                let w = pics.iter().map(|p| p.width() + gap).sum::<u32>() - gap;
                let h = pics.iter().map(RgbImage::height).max().unwrap();
                let mut all = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
                let mut x = 0;
                for p in &pics {
                    image::imageops::replace(&mut all, p, i64::from(x), 0);
                    x += p.width() + gap;
                }
                all.save(out.join(format!("montage-{mode}-{radius}-{style}.png")))
                    .unwrap();
                // The same pictures one above the other.
                let w = pics.iter().map(RgbImage::width).max().unwrap();
                let h = pics.iter().map(|p| p.height() + gap).sum::<u32>() - gap;
                let mut all = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
                let mut y = 0;
                for p in &pics {
                    image::imageops::replace(&mut all, p, 0, i64::from(y));
                    y += p.height() + gap;
                }
                all.save(out.join(format!("stack-{mode}-{radius}-{style}.png")))
                    .unwrap();
            }
        }
    }
}

struct Run {
    variant: String,
    mode: StrokeMode,
    radius: f64,
    alt: bool,
    measures: Measures,
    seconds: f64,
}

fn run(v: &Variant, mode: StrokeMode, radius: f64, base: &Heightmap, out: &Path) -> Run {
    let r = region(radius);
    let before = heights(base, &r);
    // The range of an uplift stroke without erosion. An erode stroke
    // starts on it.
    let mut lifted = base.snapshot();
    paint(&mut lifted, StrokeMode::Uplift, radius, v, false);
    let mut map = match mode {
        StrokeMode::Uplift => base.snapshot(),
        StrokeMode::Erode => lifted.snapshot(),
    };
    let lifted = heights(&lifted, &r);
    let start = std::time::Instant::now();
    paint(&mut map, mode, radius, v, true);
    let seconds = start.elapsed().as_secs_f64();
    let after = heights(&map, &r);
    let scale = radius / 60.0;
    let mut m = measure(
        &after,
        &r,
        40.0 * scale * scale,
        (6.0 * scale).round() as usize,
    );
    let span = (ELEV_MAX - ELEV_MIN) / 65535.0;
    let inside: Vec<usize> = (0..r.mask.len()).filter(|&i| r.mask[i]).collect();
    let mean_change =
        inside.iter().map(|&i| after[i] - before[i]).sum::<f64>() / inside.len() as f64;
    let rise = inside.iter().map(|&i| lifted[i] - before[i]).sum::<f64>() / inside.len() as f64;
    m.kept = mean_change / rise;
    let values: Vec<f64> = inside.iter().map(|&i| after[i]).collect();
    m.relief = (percentile(&values, 0.99) - percentile(&values, 0.01)) * span;
    if ALT.get() {
        return Run {
            variant: v.name.clone(),
            mode,
            radius,
            alt: true,
            measures: m,
            seconds,
        };
    }
    let (ramp, bands) = pictures(&after, &r);
    let stem = format!("{}-{:?}-{}", v.name, mode, radius).to_lowercase();
    ramp.save(out.join(format!("{stem}-ramp.png"))).unwrap();
    bands.save(out.join(format!("{stem}-bands.png"))).unwrap();
    shade(&after, &r)
        .save(out.join(format!("{stem}-shade.png")))
        .unwrap();
    Run {
        variant: v.name.clone(),
        mode,
        radius,
        alt: false,
        measures: m,
        seconds,
    }
}

fn tuned(name: &str, cells: f64, rough: bool, f: impl Fn(&mut ErodeTuning)) -> Variant {
    let mut v = variant(name, cells, rough);
    f(&mut v.tuning);
    v
}

fn variants() -> Vec<Variant> {
    let mut out = vec![
        variant("main", 16.0, false),
        variant("branch", 32.0, true),
        variant("b32-smooth", 32.0, false),
        variant("b16-rough", 16.0, true),
        tuned("b32-m06", 32.0, true, |t| t.area_exponent = 0.6),
        tuned("b32-w05", 32.0, true, |t| t.weight_power = 0.5),
        tuned("b32-rough05", 32.0, true, |t| t.rough = 0.5),
        tuned("b32-rough8", 32.0, true, |t| t.rough_cells = 8.0),
    ];
    for d in [0.05, 0.1, 0.2, 0.4] {
        out.push(tuned(&format!("b32-fill{d}"), 32.0, true, |t| t.infill = d));
    }
    out.push(variant("c8", 8.0, false));
    let layered = |name: &str, coarse: f64, f: &dyn Fn(&mut Variant)| {
        let mut v = variant(name, coarse, false);
        v.fine = Some(32.0);
        v.fine_rough = true;
        v.fine_tuning.detail_depth = 2.0;
        f(&mut v);
        v
    };
    out.push(layered("L8-2w", 8.0, &|v| v.fine_tuning.warp = 1.5));
    out.push(layered("L16-1w", 16.0, &|v| {
        v.fine_tuning.detail_depth = 1.0;
        v.fine_tuning.warp = 1.5;
    }));
    out.push(layered("L8-2u", 8.0, &|v| v.fine_uplift = true));
    out.push(layered("L8-2wu", 8.0, &|v| {
        v.fine_uplift = true;
        v.fine_tuning.warp = 1.5;
    }));
    out.push(layered("L8-4w", 8.0, &|v| {
        v.fine_tuning.detail_depth = 4.0;
        v.fine_tuning.warp = 1.5;
    }));
    out.push(layered("L8-2r2", 8.0, &|v| v.fine_rate = 2.0));
    for amount in [0.5, 1.0, 2.0] {
        for size in [4.0, 8.0] {
            out.push(tuned(&format!("n{amount}-{size}"), 16.0, false, |t| {
                t.route_noise = amount;
                t.route_cells = size;
            }));
            out.push(layered(&format!("L8-4n{amount}-{size}"), 8.0, &|v| {
                v.fine_tuning.detail_depth = 4.0;
                v.tuning.route_noise = amount;
                v.tuning.route_cells = size;
            }));
        }
    }
    for (amount, size) in [
        (1.0, 16.0),
        (1.0, 8.0),
        (2.0, 16.0),
        (0.5, 16.0),
        (1.5, 16.0),
    ] {
        out.push(tuned(&format!("b32n{amount}-{size}"), 32.0, false, |t| {
            t.route_noise = amount;
            t.route_cells = size;
        }));
        out.push(tuned(&format!("b32rn{amount}-{size}"), 32.0, true, |t| {
            t.route_noise = amount;
            t.route_cells = size;
        }));
    }
    for (amount, size) in [
        (0.75, 8.0),
        (0.75, 12.0),
        (1.0, 12.0),
        (0.5, 8.0),
        (0.5, 12.0),
        (1.0, 6.0),
    ] {
        out.push(tuned(&format!("b32n{amount}-{size}"), 32.0, false, |t| {
            t.route_noise = amount;
            t.route_cells = size;
        }));
    }
    for (amount, size) in [(0.75, 8.0), (1.5, 8.0), (1.0, 6.0), (1.0, 12.0)] {
        out.push(tuned(&format!("n{amount}-{size}"), 16.0, false, |t| {
            t.route_noise = amount;
            t.route_cells = size;
        }));
    }
    out.push(tuned("rn1-8", 16.0, true, |t| {
        t.route_noise = 1.0;
        t.route_cells = 8.0;
    }));
    out.push(tuned("n1-8m06", 16.0, false, |t| {
        t.route_noise = 1.0;
        t.route_cells = 8.0;
        t.area_exponent = 0.6;
    }));
    let noisy = |t: &mut ErodeTuning| {
        t.route_noise = 1.0;
        t.route_cells = 8.0;
    };
    out.push(tuned("b32n1-8f05", 32.0, false, |t| {
        noisy(t);
        t.infill = 0.05;
    }));
    out.push(tuned("b32n1-8w05", 32.0, false, |t| {
        noisy(t);
        t.weight_power = 0.5;
    }));
    out.push(tuned("b32n1-8m06", 32.0, false, |t| {
        noisy(t);
        t.area_exponent = 0.6;
    }));
    for depth in [3.0, 6.0, 8.0] {
        out.push(layered(&format!("L8-{depth}"), 8.0, &|v| {
            v.fine_tuning.detail_depth = depth
        }));
    }
    for coarse in [8.0, 16.0] {
        for depth in [1.0, 2.0, 4.0] {
            let mut v = variant(&format!("L{coarse}-{depth}"), coarse, false);
            v.fine = Some(32.0);
            v.fine_rough = true;
            v.fine_tuning.detail_depth = depth;
            out.push(v);
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // CROP=in,out,x,y,w,h writes a part of a picture.
    if let Ok(crop) = std::env::var("CROP") {
        let c: Vec<&str> = crop.split(',').collect();
        let n = |i: usize| c[i].parse::<u32>().unwrap();
        let img = image::open(c[0]).unwrap();
        img.crop_imm(n(2), n(3), n(4), n(5)).save(c[1]).unwrap();
        return;
    }
    let out = PathBuf::from(args.first().map_or("target/erode-lab", String::as_str));
    let only: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    std::fs::create_dir_all(&out).unwrap();
    let base = start_map();
    let all: Vec<Variant> = variants()
        .into_iter()
        .filter(|v| only.is_empty() || only.contains(&v.name.as_str()))
        .collect();
    let mut jobs = Vec::new();
    for v in &all {
        for mode in [StrokeMode::Uplift, StrokeMode::Erode] {
            for radius in [60.0, 150.0] {
                for alt in [false, true] {
                    jobs.push((v, mode, radius, alt));
                }
            }
        }
    }
    let runs: Vec<Run> = std::thread::scope(|s| {
        let handles: Vec<_> = jobs
            .iter()
            .map(|&(v, mode, radius, alt)| {
                let (base, out) = (&base, &out);
                s.spawn(move || {
                    ALT.set(alt);
                    run(v, mode, radius, base, out)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    println!(
        "{:<16} {:<6} {:>4} {:>6} | {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} | {:>6} {:>6} {:>6} {:>5}",
        "variant",
        "mode",
        "r",
        "score",
        "Rb",
        "hack",
        "theta",
        "junc",
        "strt",
        "RL",
        "Re",
        "chan",
        "kept",
        "relief",
        "sec"
    );
    for run in &runs {
        let m = &run.measures;
        println!(
            "{:<16} {:<6} {:>4} {:>6.2} | {:>5.2} {:>5.2} {:>5.2} {:>5.1} {:>5.2} {:>5.2} {:>5.2} | {:>6} {:>6.2} {:>6.0} {:>5.1}",
            run.variant,
            format!("{:?}", run.mode),
            format!("{}{}", run.radius, if run.alt { "a" } else { "" }),
            m.score(),
            m.bifurcation,
            m.hack,
            m.concavity,
            m.junction,
            m.straight,
            m.length_ratio,
            m.elongation,
            m.channels,
            m.kept,
            m.relief,
            run.seconds
        );
    }
    let mut totals: Vec<(String, f64)> = all
        .iter()
        .map(|v| {
            let sum = runs
                .iter()
                .filter(|r| r.variant == v.name)
                .map(|r| r.measures.score())
                .sum::<f64>();
            (v.name.clone(), sum)
        })
        .collect();
    totals.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (name, score) in totals {
        println!("total {name:<16} {score:.2}");
    }
    if let Ok(names) = std::env::var("MONTAGE") {
        let names: Vec<&str> = names.split(',').collect();
        montage(&out, &names);
    }
}
