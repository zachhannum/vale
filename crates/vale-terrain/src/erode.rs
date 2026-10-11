//! The erode brush.
//!
//! A step lowers each land cell under the brush toward the cell that takes
//! its water. The rule is stream power: the change grows with the area that
//! drains through the cell and with the slope to the receiver. The cells are
//! the cells of the coarse copy of the heightmap or the cells of a window.

use std::f64::consts::SQRT_2;

use crate::cube::{FACES, face_dir, face_of, unwarp, warp};
use crate::flow::{
    CoarseHeights, CubeGrid, Drain, FlowMap, Grid, OUT, RIVER_MIN_AREA, Window, WindowHeights,
    drain, sea, solid_angles,
};
use crate::heightmap::{MAX_BRUSH_RADIUS, TexelRect};
use crate::math::V3;

/// The number of cells at each side of a window where the effect of the
/// brush goes down to 0.
const FADE: usize = 4;

/// One touch of the erode brush on the sphere.
#[derive(Clone, Copy, Debug)]
pub struct ErodeBrush {
    /// The brush center, a unit vector.
    pub center: V3,
    /// The brush radius as an angle, in radians.
    pub radius: f64,
    /// The part of the radius with full effect, from 0 to 1.
    pub hardness: f64,
    /// The effect at the center, from 0 to 1.
    pub flow: f64,
}

/// The cells of an erosion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErodeGrid {
    /// The cells of a `CoarseHeights`, with `size` cells along one side of a
    /// face.
    Global {
        size: usize,
    },
    Window(Window),
}

/// The change of one step.
pub struct ErodeStep {
    grid: ErodeGrid,
    drops: Vec<u16>,
    rects: [Option<TexelRect>; FACES],
}

impl ErodeStep {
    pub fn grid(&self) -> ErodeGrid {
        self.grid
    }

    /// The number of levels that each cell goes down. The cells are in the
    /// order of `CoarseHeights` or of `WindowHeights`.
    pub fn drops(&self) -> &[u16] {
        &self.drops
    }

    /// The texels of each face that the step can change: the cells under the
    /// brushes and one cell around them.
    pub fn rects(&self) -> [Option<TexelRect>; FACES] {
        self.rects
    }

    /// Whether no cell goes down.
    pub fn is_empty(&self) -> bool {
        self.rects.iter().all(Option::is_none)
    }
}

enum Cells {
    Global(CubeGrid),
    Window {
        window: Window,
        /// The water of the global rivers that come into the window.
        inflow: Vec<(usize, f64)>,
    },
}

/// The cells of one stroke of the erode brush. The heights are fractions of
/// a level, so a weak brush adds up over the steps.
pub struct Erosion {
    cells: Cells,
    /// The number of cells along one side of a face or of the window.
    side: usize,
    /// The number of texels along one side of a face of the heightmap.
    face_size: usize,
    /// The flat coordinate of each cell center along `u` and along `v`.
    flat_u: Vec<f64>,
    flat_v: Vec<f64>,
    /// The area that gives a stream power of 1, in steradians.
    min_area: f64,
    heights: Vec<f32>,
    /// The heights as whole levels, after the last step.
    levels: Vec<u16>,
    /// The flow of the last step.
    flow: Option<Drain>,
    /// Whether `levels` changed after `flow`.
    stale: bool,
}

/// The effect of a brush at `k` radii from its center, from 0 to 1. The
/// curve is the curve of `Heightmap::stamp`.
fn falloff(k: f64, hard: f64) -> f64 {
    if k <= hard {
        return 1.0;
    }
    let k = (k - hard) / (1.0 - hard);
    1.0 - k * k * (3.0 - 2.0 * k)
}

impl Erosion {
    /// An erosion of the cells of the coarse copy. `face_size` is the number
    /// of texels along one side of a face of the heightmap.
    pub fn global(heights: &CoarseHeights, face_size: usize) -> Erosion {
        let m = heights.m;
        let flat: Vec<f64> = (0..m)
            .map(|i| unwarp((i as f64 + 0.5) / m as f64 * 2.0 - 1.0))
            .collect();
        let grid = CubeGrid {
            m,
            solid: solid_angles(m),
        };
        Erosion::new(
            Cells::Global(grid),
            m,
            face_size,
            [flat.clone(), flat],
            RIVER_MIN_AREA,
            &heights.levels,
        )
    }

    /// An erosion of the cells of a window. `global` gives the rivers that
    /// come into the window from outside.
    pub fn window(heights: &WindowHeights, global: &FlowMap) -> Erosion {
        let window = heights.window;
        let flat = |first: i64| {
            let cells = 0..window.cells;
            let flat = cells.map(|i| unwarp(window.angle_at(first, i as f64 + 0.5)));
            flat.collect::<Vec<f64>>()
        };
        let cells = Cells::Window {
            window,
            inflow: window.inflow(global),
        };
        Erosion::new(
            cells,
            window.cells,
            window.face_size,
            [flat(window.x0), flat(window.y0)],
            window.river_min_area(),
            &heights.levels,
        )
    }

    fn new(
        cells: Cells,
        side: usize,
        face_size: usize,
        [flat_u, flat_v]: [Vec<f64>; 2],
        min_area: f64,
        levels: &[u16],
    ) -> Erosion {
        Erosion {
            cells,
            side,
            face_size,
            flat_u,
            flat_v,
            min_area,
            heights: levels.iter().map(|&level| f32::from(level)).collect(),
            levels: levels.to_vec(),
            flow: None,
            stale: true,
        }
    }

    fn grid(&self) -> ErodeGrid {
        match &self.cells {
            Cells::Global(grid) => ErodeGrid::Global { size: grid.m },
            Cells::Window { window, .. } => ErodeGrid::Window(*window),
        }
    }

    /// The face of each square of `side` by `side` cells.
    fn faces(&self) -> std::ops::Range<usize> {
        match &self.cells {
            Cells::Global(_) => 0..FACES,
            Cells::Window { window, .. } => window.face..window.face + 1,
        }
    }

    fn drain(&self) -> Drain {
        match &self.cells {
            Cells::Global(grid) => drain(grid, &self.levels, &[]),
            Cells::Window { window, inflow } => drain(window, &self.levels, inflow),
        }
    }

    fn neighbors(&self, i: usize) -> [u32; 8] {
        match &self.cells {
            Cells::Global(grid) => grid.neighbors(i),
            Cells::Window { window, .. } => window.neighbors(i),
        }
    }

    /// The direction of a place on the cell grid of a face. `u` and `v` are
    /// in cells, and the center of the first cell is at 0.5.
    fn dir(&self, face: usize, u: f64, v: f64) -> V3 {
        match &self.cells {
            Cells::Global(grid) => {
                let flat = |p: f64| unwarp((p / grid.m as f64 * 2.0 - 1.0).clamp(-1.99, 1.99));
                face_dir(face, flat(u), flat(v))
            }
            Cells::Window { window, .. } => window.dir(u, v),
        }
    }

    /// The effect of the brushes at each cell, and the first and the last
    /// cell with an effect in each row.
    fn weights(&self, brushes: &[ErodeBrush]) -> (Vec<f32>, Vec<Option<(usize, usize)>>) {
        let side = self.side;
        let faces = self.faces();
        let global = matches!(self.cells, Cells::Global(_));
        let mut weights = vec![0.0f32; self.heights.len()];
        let mut spans = vec![None; faces.len() * side];
        for brush in brushes {
            let radius = brush.radius.clamp(1e-6, MAX_BRUSH_RADIUS);
            let hard = brush.hardness.clamp(0.0, 0.999);
            let (sin_radius, cos_radius) = radius.sin_cos();
            let c = brush.center;
            for (square, face) in faces.clone().enumerate() {
                let axis = face / 2;
                let sign = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
                let (cn, cu, cv) = (c[axis] * sign, c[(axis + 1) % 3], c[(axis + 2) % 3]);
                // A face point is at most 54.74 degrees from the face center.
                if global && cn < (0.9554 + radius).cos() {
                    continue;
                }
                // The rows or the columns that the brush circle can touch. A
                // row is on a great circle.
                let near = |flat: &[f64], along: f64| {
                    let touches =
                        |&f: &f64| (along - f * cn).abs() < sin_radius * (1.0 + f * f).sqrt();
                    let first = flat.iter().position(touches)?;
                    Some(first..=flat.iter().rposition(touches)?)
                };
                let (Some(xs), Some(ys)) = (near(&self.flat_u, cu), near(&self.flat_v, cv)) else {
                    continue;
                };
                for y in ys {
                    let fv = self.flat_v[y];
                    let row = square * side + y;
                    for x in xs.clone() {
                        let fu = self.flat_u[x];
                        let cos_angle = (cn + fu * cu + fv * cv) / (1.0 + fu * fu + fv * fv).sqrt();
                        if cos_angle <= cos_radius {
                            continue;
                        }
                        let k = cos_angle.min(1.0).acos() / radius;
                        let mut weight = falloff(k, hard) * brush.flow;
                        if !global {
                            let edge = x.min(y).min(side - 1 - x).min(side - 1 - y);
                            weight *= edge.min(FADE) as f64 / FADE as f64;
                        }
                        if weight <= 0.0 {
                            continue;
                        }
                        let cell = &mut weights[row * side + x];
                        *cell = cell.max(weight as f32);
                        let (first, last) = spans[row].unwrap_or((x, x));
                        spans[row] = Some((first.min(x), last.max(x)));
                    }
                }
            }
        }
        (weights, spans)
    }

    /// The texels of each face that hold the cells of `spans` and one cell
    /// around them.
    fn rects(&self, spans: &[Option<(usize, usize)>]) -> [Option<TexelRect>; FACES] {
        let n = self.face_size;
        let texel = |a: f64| {
            let texel = (warp(a.clamp(-1.0, 1.0)) + 1.0) * 0.5 * n as f64;
            (texel.max(0.0) as usize).min(n - 1)
        };
        let mut rects = [None; FACES];
        for (row, span) in spans.iter().enumerate() {
            let Some((first, last)) = *span else {
                continue;
            };
            let face = self.faces().start + row / self.side;
            let y = (row % self.side) as f64;
            let (u0, u1) = (first as f64 - 1.0, last as f64 + 2.0);
            let corners = [(u0, y - 1.0), (u1, y - 1.0), (u0, y + 2.0), (u1, y + 2.0)]
                .map(|(u, v)| self.dir(face, u, v));
            // A corner on another face counts as a point on the edge of this
            // face. Thus a span that crosses the edge has its full part of
            // each face.
            for on in corners.map(|d| face_of(d).0) {
                let axis = on / 2;
                let sign = if on.is_multiple_of(2) { 1.0 } else { -1.0 };
                for d in corners {
                    let depth = (d[axis] * sign).max(0.02);
                    let x = texel(d[(axis + 1) % 3] / depth);
                    let y = texel(d[(axis + 2) % 3] / depth);
                    let point = TexelRect {
                        x0: x.saturating_sub(1),
                        y0: y.saturating_sub(1),
                        x1: (x + 2).min(n),
                        y1: (y + 2).min(n),
                    };
                    rects[on] = Some(rects[on].map_or(point, |rect: TexelRect| rect.union(point)));
                }
            }
        }
        rects
    }

    /// Lowers the land cells under the brushes one time. `rate` is the
    /// strength of the step. The step computes the flow from the heights
    /// that the step before it left.
    ///
    /// A cell with stream power `f` goes to the height `r + (h - r) / (1 + f)`,
    /// where `r` is the new height of its receiver. Thus a cell does not go
    /// below its receiver at any rate. The receiver of a cell at the coast
    /// is at sea level. A cell that is not above the new height of its
    /// receiver does not change. Thus the floor of a pit stays until the rim
    /// comes down to it.
    pub fn step(&mut self, brushes: &[ErodeBrush], rate: f64) -> ErodeStep {
        let mut step = ErodeStep {
            grid: self.grid(),
            drops: vec![0; self.heights.len()],
            rects: [None; FACES],
        };
        if rate.is_nan() || rate <= 0.0 {
            return step;
        }
        let (weights, spans) = self.weights(brushes);
        if spans.iter().all(Option::is_none) {
            return step;
        }
        let flow = match self.flow.take() {
            Some(flow) if !self.stale => flow,
            _ => self.drain(),
        };
        self.stale = false;
        let sea = sea();
        // Each receiver has its new height before the cells above it.
        for &i in &flow.order {
            let i = i as usize;
            let to = flow.receiver[i];
            if weights[i] <= 0.0 || to == OUT {
                continue;
            }
            let to = to as usize;
            let below = if self.levels[to] < sea {
                f32::from(sea)
            } else {
                self.heights[to]
            };
            let height = self.heights[i];
            if height <= below {
                continue;
            }
            let diagonal = !self.neighbors(i)[..4].contains(&(to as u32));
            let length = if diagonal { SQRT_2 } else { 1.0 };
            let area = f64::from(flow.area[i]) / self.min_area;
            let power = rate * f64::from(weights[i]) * area.sqrt() / length;
            let new = f64::from(below) + f64::from(height - below) / (1.0 + power);
            self.heights[i] = (new as f32).clamp(below, height);
        }
        self.flow = Some(flow);

        // A cell goes down by whole levels. The rest stays in `heights`.
        for (row, span) in spans.iter().enumerate() {
            let Some((first, last)) = *span else {
                continue;
            };
            for i in row * self.side + first..=row * self.side + last {
                let level = self.heights[i].round() as u16;
                if level < self.levels[i] {
                    step.drops[i] = self.levels[i] - level;
                    self.levels[i] = level;
                    self.stale = true;
                }
            }
        }
        if self.stale {
            step.rects = self.rects(&spans);
        }
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::meters_to_level;
    use crate::flow::tests::{OCEAN, equator_valley, from_cells, lumpy};
    use crate::flow::{FLOW_SIZE, cell_at, cell_dir};
    use crate::math::{angle, lonlat_to_dir, normalize};

    const _: fn() = || {
        fn is_send<T: Send>() {}
        is_send::<Erosion>();
    };

    /// A strip of land on face 0, 7 cells wide. The middle row is a valley
    /// that goes down to the cell with the lowest `x`, by `fall` meters for
    /// each cell.
    fn strip(m: usize, fall: f64) -> CoarseHeights {
        from_cells(m, |face, x, y| {
            let across = (y as f64 - 24.0).abs();
            if face != 0 || !(8..m - 8).contains(&x) || across > 3.4 {
                return OCEAN;
            }
            100.0 + fall * x as f64 + 300.0 * across
        })
    }

    /// The drop of the valley cell at `x` after one step of a brush that
    /// covers that cell only.
    fn drop_of_one_cell(heights: &CoarseHeights, x: usize) -> u16 {
        let m = heights.size();
        let mut erosion = Erosion::global(heights, 8192);
        let brush = ErodeBrush {
            center: cell_dir(m, 0, x, 24),
            radius: 0.02,
            hardness: 1.0,
            flow: 1.0,
        };
        let step = erosion.step(&[brush], 0.01);
        let cell = 24 * m + x;
        let others = step.drops().iter().enumerate().filter(|&(i, _)| i != cell);
        assert!(others.into_iter().all(|(_, &drop)| drop == 0));
        assert_eq!(step.grid(), ErodeGrid::Global { size: m });
        step.drops()[cell]
    }

    /// Brushes on the 26 directions of the faces, the edges, and the corners
    /// of a cube.
    fn brushes_on_all_sides() -> Vec<ErodeBrush> {
        let mut brushes = Vec::new();
        for i in (0..27).filter(|&i| i != 13) {
            let part = |k: i32| f64::from(i / k % 3 - 1);
            brushes.push(ErodeBrush {
                center: normalize([part(1), part(3), part(9)]),
                radius: 0.3,
                hardness: 0.3,
                flow: 1.0,
            });
        }
        brushes
    }

    /// The cells that a step left below their receivers.
    fn pit_cells(erosion: &Erosion) -> usize {
        let flow = erosion.flow.as_ref().expect("a step ran");
        let cells = flow.order.iter().filter(|&&i| {
            let to = flow.receiver[i as usize];
            to != OUT && erosion.levels[i as usize] < erosion.levels[to as usize]
        });
        cells.count()
    }

    #[test]
    fn the_drop_grows_with_the_area_and_with_the_slope() {
        let m = 48;
        let (gentle, steep) = (strip(m, 20.0), strip(m, 40.0));
        // The valley has one slope. The cell at 14 drains more land than the
        // cell at 30.
        let high = drop_of_one_cell(&gentle, 30);
        let low = drop_of_one_cell(&gentle, 14);
        assert!(high > 0 && low > high, "{low} {high}");
        // The two valleys drain the same area at one cell.
        assert!(drop_of_one_cell(&steep, 30) > high);
    }

    #[test]
    fn the_flow_follows_the_lowered_land() {
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
        // The brushes cover the pit and the path of its water to the sea.
        let flow = FlowMap::new(&heights);
        let mut cell = cell_at(m, middle);
        let mut brushes = Vec::new();
        while let Some(next) = flow.receiver(cell.0, cell.1, cell.2) {
            brushes.push(ErodeBrush {
                center: cell_dir(m, cell.0, cell.1, cell.2),
                radius: 0.3,
                hardness: 1.0,
                flow: 1.0,
            });
            cell = next;
        }
        let mut erosion = Erosion::global(&heights, 8192);
        erosion.step(&brushes, 1e-9);
        let first = erosion.flow.as_ref().expect("a step ran");
        let (receiver, area) = (first.receiver.clone(), first.area.clone());
        let lake = pit_cells(&erosion);
        assert!(lake > 50, "{lake}");
        for _ in 0..30 {
            erosion.step(&brushes, 0.05);
        }
        let last = erosion.flow.as_ref().expect("a step ran");
        assert!(last.receiver != receiver && last.area != area);
        // The rim goes down, and the pit drains.
        let lake_after = pit_cells(&erosion);
        assert!(lake_after < lake / 2, "{lake_after} of {lake}");
    }

    #[test]
    fn a_cell_stays_above_its_receiver_at_each_rate() {
        let heights = CoarseHeights::from_fn(48, lumpy);
        let brushes = brushes_on_all_sides();
        let sea = sea();
        for rate in [0.01, 0.1, 1.0, 10.0, 1e3, 1e6] {
            let mut erosion = Erosion::global(&heights, 8192);
            let mut lowered = 0;
            for _ in 0..4 {
                let before = erosion.heights.clone();
                let step = erosion.step(&brushes, rate);
                lowered += step.drops().iter().filter(|&&drop| drop > 0).count();
                let flow = erosion.flow.as_ref().expect("a step ran");
                for &i in &flow.order {
                    let (i, to) = (i as usize, flow.receiver[i as usize] as usize);
                    let ocean = erosion.levels[to] < sea;
                    let old = if ocean { f32::from(sea) } else { before[to] };
                    assert!(erosion.heights[i] <= before[i], "rate {rate}, cell {i}");
                    if before[i] <= old {
                        continue;
                    }
                    let new = if ocean {
                        f32::from(sea)
                    } else {
                        erosion.heights[to]
                    };
                    assert!(erosion.heights[i] >= new, "rate {rate}, cell {i}");
                    assert!(erosion.levels[i] >= erosion.levels[to].max(sea));
                }
            }
            assert!(lowered > 100, "rate {rate}: {lowered}");
        }
    }

    #[test]
    fn the_sea_floor_and_the_sea_level_hold() {
        let heights = CoarseHeights::from_fn(48, lumpy);
        let brushes = brushes_on_all_sides();
        let sea = sea();
        let mut erosion = Erosion::global(&heights, 8192);
        let mut at_sea_level = 0;
        for _ in 0..40 {
            let step = erosion.step(&brushes, 100.0);
            assert!(!step.is_empty());
            for (i, &old) in heights.levels.iter().enumerate() {
                let new = erosion.levels[i];
                if old < sea {
                    assert_eq!((new, step.drops()[i]), (old, 0), "ocean cell {i}");
                } else {
                    assert!(new >= sea && new <= old, "land cell {i}");
                    assert!(erosion.heights[i] >= f32::from(sea), "land cell {i}");
                }
            }
            let coast = heights.levels.iter().zip(&erosion.levels);
            at_sea_level = coast
                .filter(|&(&old, &new)| old > sea && new == sea)
                .count();
        }
        // The erosion comes down to sea level and stops there.
        assert!(at_sea_level > 100, "{at_sea_level}");
    }

    #[test]
    fn a_step_lowers_the_cells_of_a_window_and_of_the_global_map() {
        let n = 8192;
        let coarse = CoarseHeights::from_fn(64, equator_valley);
        let center = lonlat_to_dir(20.0, 0.4);
        let brush = |radius: f64| ErodeBrush {
            center,
            radius,
            hardness: 0.5,
            flow: 1.0,
        };

        let mut far = Erosion::global(&coarse, n);
        let step = far.step(&[brush(0.1)], 1.0);
        assert_eq!(step.grid(), ErodeGrid::Global { size: 64 });
        let lowered = step.drops().iter().filter(|&&drop| drop > 0).count();
        assert!(lowered > 10, "{lowered}");
        // The texels of each lowered cell are in the rectangle of its face.
        for (i, _) in step.drops().iter().enumerate().filter(|&(_, &d)| d > 0) {
            let (face, x, y) = (i / (64 * 64), i % 64, i / 64 % 64);
            let rect = step.rects()[face].expect("the face has a rectangle");
            let texels = n / 64;
            assert!(rect.x0 <= x * texels && rect.x1 >= (x + 1) * texels);
            assert!(rect.y0 <= y * texels && rect.y1 >= (y + 1) * texels);
            assert!(angle(cell_dir(64, face, x, y), center) < 0.1);
        }
        let rect = step.rects()[0].expect("the brush is on face 0");
        assert!(rect.x1 - rect.x0 < n / 4 && rect.y1 - rect.y0 < n / 4);

        // The window is 2 cells of the global map wide, on the low part of
        // the valley.
        let window = Window::centered(center, n, 2, 128);
        let heights = WindowHeights::from_fn(window, equator_valley);
        let global = FlowMap::new(&coarse);
        let dry = FlowMap::new(&CoarseHeights::from_fn(64, |_| meters_to_level(OCEAN)));
        // The sum of the heights that 5 steps take away.
        let lowered = |global: &FlowMap| {
            let mut near = Erosion::window(&heights, global);
            let mut cells = 0;
            for _ in 0..5 {
                let step = near.step(&[brush(0.015)], 1.0);
                assert_eq!(step.grid(), ErodeGrid::Window(window));
                let rect = step.rects()[window.face].expect("the window has a rectangle");
                for (i, _) in step.drops().iter().enumerate().filter(|&(_, &d)| d > 0) {
                    cells += 1;
                    let (x, y) = (i % 128, i / 128);
                    // No cell at a side of the window changes.
                    assert!((1..127).contains(&x) && (1..127).contains(&y));
                    let (x, y) = (window.x0 as usize + 2 * x, window.y0 as usize + 2 * y);
                    assert!(rect.x0 <= x && rect.x1 >= x + 2 && rect.y0 <= y && rect.y1 >= y + 2);
                }
            }
            assert!(cells > 100, "{cells}");
            let sum = heights.levels.iter().zip(&near.heights);
            sum.map(|(&old, &new)| f64::from(old) - f64::from(new))
                .sum::<f64>()
        };
        // The river that comes in from the global map makes the drop larger.
        let (alone, fed) = (lowered(&dry), lowered(&global));
        assert!(alone > 0.0 && fed > 1.05 * alone, "{fed} {alone}");
    }

    /// Run with `--release --ignored --nocapture` for the time of one step
    /// at the full size.
    #[test]
    #[ignore]
    fn step_time_at_the_full_size() {
        let heights = CoarseHeights::from_fn(FLOW_SIZE, lumpy);
        let mut erosion = Erosion::global(&heights, 8192);
        let brushes = brushes_on_all_sides();
        for brushes in [&brushes[..1], &brushes[..]] {
            let start = std::time::Instant::now();
            let step = erosion.step(brushes, 1.0);
            let lowered = step.drops().iter().filter(|&&drop| drop > 0).count();
            eprintln!(
                "brushes {}, lowered cells {lowered}, step {:?}",
                brushes.len(),
                start.elapsed()
            );
        }
        assert!(erosion.stale);
    }
}
