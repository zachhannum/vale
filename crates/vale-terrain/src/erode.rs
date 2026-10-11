//! The erode brush.
//!
//! A step lowers each cell under the brush toward the cell that takes its
//! water. The sea level has no effect: the water drains over the land and
//! the sea floor to the lowest ground. The rule is stream power: the change grows with the area that
//! drains through the cell and with the slope to the receiver. The cells are
//! the cells of the coarse copy of the heightmap or the cells of a window.

use std::f64::consts::{FRAC_PI_2, SQRT_2};

use crate::cube::{FACES, face_dir, unwarp};
use crate::flow::{
    CoarseHeights, CubeGrid, Drain, FlowMap, Grid, OUT, RIVER_MIN_AREA, Window, WindowHeights,
    cell_past_edge, drain, solid_angles,
};
use crate::heightmap::{MAX_BRUSH_RADIUS, TexelRect, brush_rect, falloff};
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

    /// The texels of each face that the step can change: the circle of each
    /// brush and one cell around it.
    pub fn rects(&self) -> [Option<TexelRect>; FACES] {
        self.rects
    }

    /// Whether no cell goes down.
    pub fn is_empty(&self) -> bool {
        self.rects.iter().all(Option::is_none)
    }

    /// The drop of a cell. `x` and `y` can be one cell past the end of the
    /// grid. Past a face edge of the global grid, the cell is the nearest
    /// cell of the face that is there. A cell past the side of a window has
    /// no drop.
    pub(crate) fn cell_drop(&self, face: usize, x: i64, y: i64) -> u16 {
        match self.grid {
            ErodeGrid::Global { size } => {
                let m = size as i64;
                let (face, x, y) = if x >= 0 && y >= 0 && x < m && y < m {
                    (face, x as usize, y as usize)
                } else {
                    cell_past_edge(size, face, x, y)
                };
                self.drops[(face * size + y) * size + x]
            }
            ErodeGrid::Window(window) => {
                let cells = window.cells as i64;
                if x < 0 || y < 0 || x >= cells || y >= cells {
                    return 0;
                }
                self.drops[y as usize * window.cells + x as usize]
            }
        }
    }

    /// The place of a texel on the cell grid, in cells. The center of the
    /// first cell is at 0. The face has `n` texels along one side. A texel
    /// too far from the face of a window has no place.
    pub(crate) fn place(&self, n: usize, face: usize, x: usize, y: usize) -> Option<(f64, f64)> {
        match self.grid {
            ErodeGrid::Global { size } => {
                let at = |x: usize| (x as f64 + 0.5) * size as f64 / n as f64 - 0.5;
                Some((at(x), at(y)))
            }
            // On the face of the window, the place is exact.
            ErodeGrid::Window(window) if window.face == face => {
                let at = |x: usize, first: i64| {
                    ((x as i64 - first) as f64 + 0.5) / window.cell as f64 - 0.5
                };
                Some((at(x, window.x0), at(y, window.y0)))
            }
            ErodeGrid::Window(window) => {
                let flat = |x: usize| unwarp((x as f64 + 0.5) / n as f64 * 2.0 - 1.0);
                let (u, v) = window.place(face_dir(face, flat(x), flat(y)))?;
                Some((u - 0.5, v - 0.5))
            }
        }
    }

    /// The drop at a texel, in levels, from the drops of the 4 cells around
    /// the texel. A line between the two opposite cells with the larger sum
    /// splits the square of the 4 centers into two triangles. The drop is
    /// linear on each triangle, so a diagonal channel has an even depth.
    pub(crate) fn drop_at(&self, n: usize, face: usize, x: usize, y: usize) -> f64 {
        let Some((u, v)) = self.place(n, face, x, y) else {
            return 0.0;
        };
        let (i, j) = (u.floor(), v.floor());
        let (tu, tv) = (u - i, v - j);
        let cell = |du: i64, dv: i64| f64::from(self.cell_drop(face, i as i64 + du, j as i64 + dv));
        let (a, b, c, d) = (cell(0, 0), cell(1, 0), cell(0, 1), cell(1, 1));
        if a + d >= b + c {
            if tu >= tv {
                a + (b - a) * tu + (d - b) * tv
            } else {
                a + (c - a) * tv + (d - c) * tu
            }
        } else if tu + tv <= 1.0 {
            a + (b - a) * tu + (c - a) * tv
        } else {
            d + (c - d) * (1.0 - tu) + (b - d) * (1.0 - tv)
        }
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
    /// The water drains to the cells below this level. They are the cells at
    /// the lowest level of the grid, and they do not change.
    outlet: u16,
    /// The flow of the last step.
    flow: Option<Drain>,
    /// Whether `levels` changed after `flow`.
    stale: bool,
}

/// The effect of the brushes of one step.
struct Weights {
    /// The effect at each cell.
    cells: Vec<f32>,
    /// The first and the last cell with an effect in each row.
    spans: Vec<Option<(usize, usize)>>,
    /// Whether each brush has an effect on a cell.
    hits: Vec<bool>,
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
            outlet: levels
                .iter()
                .min()
                .map_or(0, |&lowest| lowest.saturating_add(1)),
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
            Cells::Global(grid) => drain(grid, &self.levels, &[], self.outlet),
            Cells::Window { window, inflow } => drain(window, &self.levels, inflow, self.outlet),
        }
    }

    fn neighbors(&self, i: usize) -> [u32; 8] {
        match &self.cells {
            Cells::Global(grid) => grid.neighbors(i),
            Cells::Window { window, .. } => window.neighbors(i),
        }
    }

    /// The effect of the brushes at each cell.
    fn weights(&self, brushes: &[ErodeBrush]) -> Weights {
        let side = self.side;
        let faces = self.faces();
        let global = matches!(self.cells, Cells::Global(_));
        let mut weights = vec![0.0f32; self.heights.len()];
        let mut spans = vec![None; faces.len() * side];
        let mut hits = vec![false; brushes.len()];
        for (brush, hit) in brushes.iter().zip(&mut hits) {
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
                        *hit = true;
                    }
                }
            }
        }
        Weights {
            cells: weights,
            spans,
            hits,
        }
    }

    /// The texels of each face that the cells under the brushes can change.
    /// A texel takes the drops of the cells around it, so the circle of each
    /// brush grows by the width of a cell and a half.
    fn rects(&self, brushes: &[ErodeBrush], hits: &[bool]) -> [Option<TexelRect>; FACES] {
        let n = self.face_size;
        let cell = match &self.cells {
            Cells::Global(grid) => FRAC_PI_2 / grid.m as f64,
            Cells::Window { window, .. } => FRAC_PI_2 * window.cell as f64 / n as f64,
        };
        let mut rects = [None; FACES];
        for (brush, _) in brushes.iter().zip(hits).filter(|&(_, &hit)| hit) {
            let radius = brush.radius.clamp(1e-6, MAX_BRUSH_RADIUS) + 1.5 * cell;
            for (face, rect) in rects.iter_mut().enumerate() {
                if let Some(new) = brush_rect(n, face, brush.center, radius) {
                    *rect = Some(rect.map_or(new, |rect: TexelRect| rect.union(new)));
                }
            }
        }
        rects
    }

    /// Lowers the cells under the brushes one time. `rate` is the
    /// strength of the step. The step computes the flow from the heights
    /// that the step before it left.
    ///
    /// A cell with stream power `f` goes to the height `r + (h - r) / (1 + f)`,
    /// where `r` is the new height of its receiver. Thus a cell does not go
    /// below its receiver at any rate. A cell that is not above the new
    /// height of its receiver does not change. Thus a flat sea floor stays,
    /// and the floor of a pit stays until the rim comes down to it.
    pub fn step(&mut self, brushes: &[ErodeBrush], rate: f64) -> ErodeStep {
        let mut step = ErodeStep {
            grid: self.grid(),
            drops: vec![0; self.heights.len()],
            rects: [None; FACES],
        };
        if rate.is_nan() || rate <= 0.0 {
            return step;
        }
        let Weights {
            cells: weights,
            spans,
            hits,
        } = self.weights(brushes);
        if spans.iter().all(Option::is_none) {
            return step;
        }
        let flow = match self.flow.take() {
            Some(flow) if !self.stale => flow,
            _ => self.drain(),
        };
        self.stale = false;
        // Each receiver has its new height before the cells above it.
        for &i in &flow.order {
            let i = i as usize;
            let to = flow.receiver[i];
            if weights[i] <= 0.0 || to == OUT {
                continue;
            }
            let to = to as usize;
            let below = self.heights[to];
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
            step.rects = self.rects(brushes, &hits);
        }
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::meters_to_level;
    use crate::flow::tests::{OCEAN, equator_valley, from_cells, lumpy};
    use crate::flow::{FLOW_SIZE, cell_at, cell_dir, sea};
    use crate::heightmap::{Heightmap, Mode, Stamp};
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
            let (mut land, mut sea_floor) = (0, 0);
            for _ in 0..4 {
                let before = erosion.heights.clone();
                let step = erosion.step(&brushes, rate);
                for (&drop, &level) in step.drops().iter().zip(&heights.levels) {
                    land += usize::from(drop > 0 && level >= sea);
                    sea_floor += usize::from(drop > 0 && level < sea);
                }
                let flow = erosion.flow.as_ref().expect("a step ran");
                for &i in &flow.order {
                    let (i, to) = (i as usize, flow.receiver[i as usize] as usize);
                    assert!(erosion.heights[i] <= before[i], "rate {rate}, cell {i}");
                    if before[i] <= before[to] {
                        continue;
                    }
                    assert!(
                        erosion.heights[i] >= erosion.heights[to],
                        "rate {rate}, cell {i}"
                    );
                    assert!(
                        erosion.levels[i] >= erosion.levels[to],
                        "rate {rate}, cell {i}"
                    );
                }
            }
            assert!(
                land > 100 && sea_floor > 100,
                "rate {rate}: {land} {sea_floor}"
            );
        }
    }

    /// `strip` with a sea floor past the mouth of the valley. The floor goes
    /// down from 290 m below the sea to 920 m below it. The other sea floor
    /// is flat.
    fn strip_with_a_shelf(m: usize) -> CoarseHeights {
        from_cells(m, |face, x, y| {
            let across = (y as f64 - 24.0).abs();
            if face != 0 || x >= m - 8 || across > 3.4 {
                return OCEAN;
            }
            if x < 8 {
                return -200.0 - 90.0 * (8 - x) as f64;
            }
            100.0 + 20.0 * x as f64 + 300.0 * across
        })
    }

    #[test]
    fn a_river_mouth_and_a_sloped_sea_floor_go_down() {
        let m = 48;
        let heights = strip_with_a_shelf(m);
        let sea = sea();
        let cell = |x: usize| 24 * m + x;
        assert!(heights.levels[cell(8)] > sea && heights.levels[cell(7)] < sea);
        let mut erosion = Erosion::global(&heights, 8192);
        // The brush covers the mouth of the river, the sea floor that goes
        // down from it, and a part of the flat sea floor.
        let brush = ErodeBrush {
            center: cell_dir(m, 0, 6, 24),
            radius: 0.3,
            hardness: 1.0,
            flow: 1.0,
        };
        for _ in 0..5 {
            assert!(!erosion.step(&[brush], 0.2).is_empty());
        }
        // The cell at the coast is under water, so the coast is at a new
        // place.
        assert!(erosion.levels[cell(8)] < sea, "{}", erosion.levels[cell(8)]);
        // The valley goes on down the sea floor.
        for x in 2..8 {
            assert!(erosion.levels[cell(x)] < heights.levels[cell(x)], "{x}");
        }
        let flat = meters_to_level(OCEAN);
        let mut under_the_brush = 0;
        for (i, (&old, &new)) in heights.levels.iter().zip(&erosion.levels).enumerate() {
            assert!(new >= flat && new <= old, "cell {i}");
            if old == flat {
                assert_eq!(new, old, "cell {i}");
                let d = cell_dir(m, i / (m * m), i % m, i / m % m);
                under_the_brush += usize::from(angle(d, brush.center) < 0.25);
            }
        }
        assert!(under_the_brush > 100, "{under_the_brush}");
    }

    /// `lumpy`, moved down so that the low ground is at level 0.
    fn deep(d: V3) -> u16 {
        lumpy(d).saturating_sub(25_000)
    }

    #[test]
    fn no_cell_goes_below_level_0() {
        let heights = CoarseHeights::from_fn(48, deep);
        let at_0 = heights.levels.iter().filter(|&&level| level == 0).count();
        assert!(at_0 > 500, "{at_0}");
        let brushes = brushes_on_all_sides();
        let mut erosion = Erosion::global(&heights, 8192);
        for _ in 0..10 {
            let before = erosion.levels.clone();
            let step = erosion.step(&brushes, 1e6);
            for (i, (&old, &new)) in before.iter().zip(&erosion.levels).enumerate() {
                assert_eq!(old - step.drops()[i], new, "cell {i}");
                assert!(erosion.heights[i] >= 0.0, "cell {i}");
            }
        }
        let to_0 = heights.levels.iter().zip(&erosion.levels);
        let to_0 = to_0.filter(|&(&old, &new)| old > 0 && new == 0).count();
        assert!(to_0 > 100, "{to_0}");
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

    /// A heightmap with `level` at each texel, in tiles of 64 texels.
    fn map_of(n: usize, level: impl Fn(V3) -> u16) -> Heightmap {
        let mut map = Heightmap::with_tile_size(n, 64, 0);
        for face in 0..FACES {
            let levels: Vec<u16> = (0..n * n)
                .map(|i| level(map.texel_dir(face, i % n, i / n)))
                .collect();
            map.store_face(face, &levels);
        }
        map.take_dirty();
        map
    }

    fn texels(map: &Heightmap) -> Vec<u16> {
        let n = map.face_size();
        let all = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        let mut out = Vec::new();
        for face in 0..FACES {
            map.read_rect(face, all, &mut out);
        }
        out
    }

    /// A step on the global grid with a drop for each cell, on all texels.
    fn step_of(m: usize, n: usize, drop: impl Fn(usize, V3) -> u16) -> ErodeStep {
        let all = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        let cells = 0..FACES * m * m;
        let drops = cells.map(|i| drop(i, cell_dir(m, i / (m * m), i % m, i / m % m)));
        ErodeStep {
            grid: ErodeGrid::Global { size: m },
            drops: drops.collect(),
            rects: [Some(all); FACES],
        }
    }

    /// The largest difference of the drops of two neighbor cells.
    fn largest_cell_step(step: &ErodeStep, m: usize) -> f64 {
        let grid = CubeGrid {
            m,
            solid: Vec::new(),
        };
        let mut largest = 0;
        for (i, &drop) in step.drops().iter().enumerate() {
            for j in grid.neighbors(i) {
                largest = largest.max(drop.abs_diff(step.drops()[j as usize]));
            }
        }
        f64::from(largest)
    }

    #[test]
    fn erode_lowers_the_sea_floor_and_stops_at_level_0() {
        let n = 256;
        let mut map = map_of(n, deep);
        let before = texels(&map);
        let sea = sea();
        assert!(before.iter().all(|&level| level < sea));
        let mut erosion = Erosion::global(&CoarseHeights::from_fn(64, deep), n);
        let brushes = brushes_on_all_sides();
        let mut changed = 0;
        for _ in 0..30 {
            changed += map.erode(&erosion.step(&brushes, 100.0));
        }
        assert!(changed > 10_000, "{changed}");
        let after = texels(&map);
        let mut to_0 = 0;
        for (&old, &new) in before.iter().zip(&after) {
            // A level that goes below 0 comes back as a high level.
            assert!(new <= old, "{old} {new}");
            to_0 += usize::from(old > 0 && new == 0);
        }
        assert!(to_0 > 1000, "{to_0}");
        assert!(map.take_dirty().iter().flatten().count() >= 3);

        // A texel that is lower than its cell stops at 0.
        let mut low = Heightmap::new(n, 3);
        let step = step_of(16, n, |_, _| 500);
        assert_eq!(low.erode(&step), FACES * n * n);
        assert!(texels(&low).iter().all(|&level| level == 0));
    }

    #[test]
    fn the_drop_of_the_texels_has_no_cell_step() {
        let (m, n) = (16, 256);
        // The drops of two neighbor cells are far apart.
        let step = step_of(m, n, |i, _| (i * 7919 % 1000) as u16);
        let largest = largest_cell_step(&step, m);
        assert!(largest > 900.0);
        let mut map = Heightmap::new(n, 60_000);
        assert!(map.erode(&step) > FACES * n * n * 9 / 10);
        let drop = |face: usize, x: usize, y: usize| {
            60_000.0 - f64::from(map.get(face, x as i64, y as i64))
        };
        // A cell is 16 texels wide, so a texel has a 16th of the cell step.
        let limit = largest / (n / m) as f64 + 1.0;
        let mut most: f64 = 0.0;
        for face in 0..FACES {
            for y in 0..n {
                for x in 0..n - 1 {
                    let along = (drop(face, x + 1, y) - drop(face, x, y)).abs();
                    let across = (drop(face, y, x + 1) - drop(face, y, x)).abs();
                    most = most.max(along).max(across);
                }
            }
        }
        assert!(most <= limit && most > 0.5 * limit, "{most} {limit}");
        // The texel at a cell center has the drop of the cell.
        assert_eq!(
            step.drop_at(m, 2, 2, 4),
            f64::from(step.drops()[(2 * m + 4) * m + 2])
        );
    }

    #[test]
    fn a_diagonal_channel_has_an_even_depth() {
        let (m, n) = (16, 256);
        // The cells of a diagonal of each face have a drop.
        let step = step_of(m, n, |i, _| if i % m == i / m % m { 800 } else { 0 });
        let width = n / m;
        for face in 0..FACES {
            // The texels on the diagonal, between the first and the last
            // cell center.
            for x in width / 2..n - width / 2 {
                assert_eq!(step.drop_at(n, face, x, x), 800.0, "{face} {x}");
            }
        }
        // The channel is one cell wide. A texel at the center of the next
        // cell has the drop of half a texel.
        let side = step.drop_at(n, 0, 5 * width + width / 2, 4 * width + width / 2);
        assert_eq!(side, 800.0 * 0.5 / width as f64);
    }

    #[test]
    fn the_drop_of_the_texels_has_no_seam_at_a_face_edge() {
        let (m, n) = (16, 256);
        let width = (n / m) as f64;
        let wave = |d: V3| 600.0 + 500.0 * (4.0 * d[0] + 1.0).sin() * (3.0 * d[1] - d[2]).cos();
        let step = step_of(m, n, |_, d| wave(d) as u16);
        let largest = largest_cell_step(&step, m);
        assert!(largest > 50.0 && largest < 250.0, "{largest}");
        let mut map = Heightmap::new(n, 60_000);
        map.erode(&step);
        let drop = |face: usize, x: i64, y: i64| 60_000.0 - f64::from(map.get(face, x, y));
        let last = n as i64 - 1;
        let (mut middle, mut end, mut corner): (f64, f64, f64) = (0.0, 0.0, 0.0);
        for face in 0..FACES {
            for i in 0..n as i64 {
                // The texel pairs across the 4 edges of the face.
                let pairs = [
                    ((last, i), (last + 1, i)),
                    ((0, i), (-1, i)),
                    ((i, last), (i, last + 1)),
                    ((i, 0), (i, -1)),
                ];
                for (a, b) in pairs {
                    let seam = (drop(face, a.0, a.1) - drop(face, b.0, b.1)).abs();
                    // The middle half of the edge.
                    if (n as i64 / 4..3 * n as i64 / 4).contains(&i) {
                        middle = middle.max(seam);
                    } else {
                        end = end.max(seam);
                    }
                }
            }
            // The 3 texels that meet at a cube corner.
            for (x, y) in [(0, 0), (last, 0), (0, last), (last, last)] {
                let (dx, dy) = (if x == 0 { -1 } else { 1 }, if y == 0 { -1 } else { 1 });
                let here = drop(face, x, y);
                for there in [drop(face, x + dx, y), drop(face, x, y + dy)] {
                    corner = corner.max((here - there).abs());
                }
            }
        }
        // In the middle half of an edge, the cells of the two faces are in
        // line, and the seam is as smooth as the inside of a face.
        assert!(middle > 0.0 && middle <= largest / width + 1.0, "{middle}");
        // Toward a corner, the cell past the edge can be half a cell to one
        // side. Thus the seam can be larger by half a cell step.
        assert!(end <= largest / width + largest / 2.0 + 1.0, "{end}");
        // At a corner, the difference is one cell step at most.
        assert!(corner <= largest + 1.0, "{corner}");
    }

    #[test]
    fn erode_in_a_window_lowers_the_texels_of_two_faces() {
        let n = 256;
        let mut map = map_of(n, equator_valley);
        let before = texels(&map);
        // The faces meet at longitude 45.
        let window = Window::centered(lonlat_to_dir(44.0, 0.4), n, 1, 64);
        assert_eq!(window.face, 0);
        let global = FlowMap::new(&CoarseHeights::from_fn(64, equator_valley));
        let mut erosion = Erosion::window(&WindowHeights::new(&map, window), &global);
        let brush = ErodeBrush {
            center: lonlat_to_dir(45.0, 0.4),
            radius: 0.05,
            hardness: 0.5,
            flow: 1.0,
        };
        let step = erosion.step(&[brush], 1.0);
        assert!(map.erode(&step) > 100);
        let dirty = map.take_dirty();
        assert!(dirty[0].is_some() && dirty[2].is_some());
        let after = texels(&map);
        let mut lowered = [0; FACES];
        for (i, (&old, &new)) in before.iter().zip(&after).enumerate() {
            let (face, x, y) = (i / (n * n), i % n, i / n % n);
            let cell = window.cell_at(map.texel_dir(face, x, y));
            if face == 0 {
                // A cell is one texel of the face of the window.
                let drop = cell.map_or(0, |(x, y)| step.drops()[y * 64 + x]);
                assert_eq!(old - new, drop, "{x} {y}");
            }
            if new != old {
                assert!(cell.is_some(), "{face} {x} {y}");
                lowered[face] += 1;
            }
        }
        assert!(lowered[0] > 50 && lowered[2] > 50, "{lowered:?}");
    }

    #[test]
    fn one_undo_puts_back_a_stroke_of_many_steps() {
        let n = 256;
        let mut map = map_of(n, lumpy);
        let before = texels(&map);
        let mut erosion = Erosion::global(&CoarseHeights::from_fn(64, lumpy), n);
        let brushes = brushes_on_all_sides();
        map.begin_stroke();
        let mut changed = 0;
        for _ in 0..5 {
            changed += map.erode(&erosion.step(&brushes, 1.0));
        }
        assert!(changed > 1000, "{changed}");
        assert!(map.end_stroke());
        let after = texels(&map);
        assert!(after != before);
        assert!(map.undo() && !map.can_undo());
        assert!(texels(&map) == before);
        assert!(map.redo());
        assert!(texels(&map) == after);

        // A stamp of the erode mode changes nothing.
        let stamp = Stamp {
            center: [1.0, 0.0, 0.0],
            radius: 0.2,
            hardness: 0.5,
            flow: 1.0,
            mode: Mode::Erode,
            level: 0,
            strength: 1000.0,
        };
        map.begin_stroke();
        assert_eq!(map.stamp(&stamp), 0);
        assert!(!map.end_stroke());
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
