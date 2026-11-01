//! The erode brush.
//!
//! A step lowers each cell under the brush toward the cell that takes its
//! water. The sea level has no effect: the water drains over the land and
//! the sea floor to the lowest ground. The rule is stream power: the change grows with the area that
//! drains through the cell and with the slope to the receiver. The cells are
//! the cells of the coarse copy of the heightmap or the cells of a window.

use std::f64::consts::{FRAC_PI_2, SQRT_2};

use crate::channel::{ChannelCut, ChannelLines, ChannelShape};
use crate::cube::{FACES, face_dir, face_of, unwarp, warp};
use crate::flow::{
    CoarseHeights, CubeGrid, Drain, FlowMap, Grid, NONE, OUT, RIVER_MIN_AREA, RIVER_MIN_CELLS,
    STEPS, Window, WindowHeights, cell_past_edge, drain, drop_curves, index_corners, index_dir,
    solid_angles,
};
use crate::heightmap::{Limits, MAX_BRUSH_RADIUS, TexelRect, brush_rect, falloff};
use crate::math::{V3, dot};
use crate::rain::{Rain, TRACE_SPACING};

/// The number of cells at each side of a window where the effect of the
/// brush goes down to 0.
const FADE: usize = 4;

/// The smallest number of cells between the cells of the brushes and the
/// side of the rectangle that gives the flow of a step of `Erosion::steps`.
const NEAR_ROOM: usize = 8;

/// The number of cells between two places where a step computes the rain.
/// The rain of a cell is a blend of the 4 places around it.
const RAIN_STRIDE: usize = 8;

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
    limits: Limits,
    /// With `lines`, the drops of the cells away from the rivers. A river
    /// cell has the mean drop of the cells next to it that are not rivers.
    hill: Vec<u16>,
    /// The channels of the rivers of a step on the global grid. The level of
    /// a curve is its drop.
    lines: Option<ChannelLines>,
    channel: ChannelShape,
}

impl ErodeStep {
    /// The limits of the erosion of the step.
    pub fn limits(&self) -> Limits {
        self.limits
    }

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

    /// The drops that the texels blend. With `lines`, a river cell has the
    /// drop of the ground beside it, and the channel of the river gives the
    /// drop of the texels near the river.
    pub fn hill(&self) -> &[u16] {
        if self.lines.is_some() {
            &self.hill
        } else {
            &self.drops
        }
    }

    /// The channels of the rivers. Only a step on the global grid has them.
    pub fn lines(&self) -> Option<&ChannelLines> {
        self.lines.as_ref()
    }

    /// The width of the channels.
    pub fn channel(&self) -> ChannelShape {
        self.channel
    }

    /// Whether no cell goes down.
    pub fn is_empty(&self) -> bool {
        self.rects.iter().all(Option::is_none)
    }

    /// Adds the drops of `other` to the drops of this step. The sum of two
    /// drops stops at the largest level. Returns `false` and changes nothing
    /// if the two steps have different cells or if a step has channels.
    pub fn merge(&mut self, other: &ErodeStep) -> bool {
        if self.grid != other.grid || self.lines.is_some() || other.lines.is_some() {
            return false;
        }
        for (drop, add) in self.drops.iter_mut().zip(&other.drops) {
            *drop = drop.saturating_add(*add);
        }
        for (rect, add) in self.rects.iter_mut().zip(other.rects) {
            *rect = match (*rect, add) {
                (Some(a), Some(b)) => Some(a.union(b)),
                (a, b) => a.or(b),
            };
        }
        true
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
                self.hill()[(face * size + y) * size + x]
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

    /// The drop at a texel, in levels. If a cell is one texel wide, the drop
    /// is that of the 4 cells around the texel, in the ratio of its distances
    /// to their centers. If a cell is wider, the drop is a smooth blend of
    /// the 9 cells around the texel, with the weights of a quadratic
    /// B-spline. That blend has no edge at a cell and no corner between two
    /// cells, so a channel one cell wide has no row of dots. With `lines`,
    /// the drop is the larger of that blend and the drop of the channels.
    pub(crate) fn drop_at(&self, n: usize, face: usize, x: usize, y: usize) -> f64 {
        let blend = self.blend_at(n, face, x, y);
        let Some(lines) = &self.lines else {
            return blend;
        };
        let flat = |x: usize| unwarp((x as f64 + 0.5) / n as f64 * 2.0 - 1.0);
        let p = face_dir(face, flat(x), flat(y));
        let cut = ChannelCut {
            shape: self.channel,
            depth: 0.0,
            texel: FRAC_PI_2 / n as f64,
        };
        let channel = match lines.texel_bin(face, x, y, n) {
            Some(bin) => lines.channel_drop(bin, p, &cut),
            None => 0.0,
        };
        blend.max(channel)
    }

    /// The blend of the drops of the cells around a texel.
    fn blend_at(&self, n: usize, face: usize, x: usize, y: usize) -> f64 {
        let Some((u, v)) = self.place(n, face, x, y) else {
            return 0.0;
        };
        let wide = match self.grid {
            ErodeGrid::Global { size } => size < n,
            ErodeGrid::Window(window) => window.cell > 1,
        };
        if !wide {
            let (i, j) = (u.floor(), v.floor());
            let (tu, tv) = (u - i, v - j);
            let cell =
                |du: i64, dv: i64| f64::from(self.cell_drop(face, i as i64 + du, j as i64 + dv));
            let top = cell(0, 0) * (1.0 - tu) + cell(1, 0) * tu;
            let bottom = cell(0, 1) * (1.0 - tu) + cell(1, 1) * tu;
            return top * (1.0 - tv) + bottom * tv;
        }
        let (i, j) = ((u + 0.5).floor(), (v + 0.5).floor());
        let weights = |t: f64| {
            [
                0.5 * (0.5 - t) * (0.5 - t),
                0.75 - t * t,
                0.5 * (0.5 + t) * (0.5 + t),
            ]
        };
        let (wu, wv) = (weights(u - i), weights(v - j));
        let mut drop = 0.0;
        for (dv, wv) in wv.into_iter().enumerate() {
            for (du, wu) in wu.into_iter().enumerate() {
                let cell = self.cell_drop(face, i as i64 + du as i64 - 1, j as i64 + dv as i64 - 1);
                drop += wu * wv * f64::from(cell);
            }
        }
        drop
    }
}

enum Cells {
    Global(CubeGrid),
    Window {
        window: Window,
        /// The water of the global rivers that come into the window.
        inflow: Vec<(usize, f64)>,
        /// The solid angle of each cell, in steradians.
        solid: Vec<f64>,
    },
}

/// The cells of a window with the solid angle of each cell. A step asks for
/// each solid angle, and the window computes it with tangents.
struct WindowGrid<'a> {
    window: &'a Window,
    solid: &'a [f64],
}

/// A rectangle of the cells of a window. The water leaves it at its sides.
struct BoxGrid<'a> {
    window: &'a Window,
    solid: &'a [f64],
    /// The first cell of the rectangle in the window, and its width.
    x0: usize,
    y0: usize,
    width: usize,
    height: usize,
}

impl BoxGrid<'_> {
    /// The index of a cell of the rectangle in the window.
    fn cell(&self, i: usize) -> usize {
        (self.y0 + i / self.width) * self.window.cells + self.x0 + i % self.width
    }
}

impl Grid for BoxGrid<'_> {
    fn neighbors(&self, i: usize) -> [u32; 8] {
        let (w, h) = (self.width as i64, self.height as i64);
        let (x, y) = ((i % self.width) as i64, (i / self.width) as i64);
        STEPS.map(|(dx, dy)| {
            let (x, y) = (x + dx, y + dy);
            if x < 0 || y < 0 || x >= w || y >= h {
                return NONE;
            }
            (y * w + x) as u32
        })
    }

    fn scatter(&self, i: usize, k: usize) -> f64 {
        self.window.scatter(self.cell(i), k)
    }

    fn solid_angle(&self, i: usize) -> f64 {
        self.solid[self.cell(i)]
    }
}

/// A grid where the water of each cell is its solid angle times its rain.
struct Rainy<'a, G> {
    grid: &'a G,
    rain: &'a [f32],
}

impl<G: Grid> Grid for Rainy<'_, G> {
    fn neighbors(&self, i: usize) -> [u32; 8] {
        self.grid.neighbors(i)
    }

    fn scatter(&self, i: usize, k: usize) -> f64 {
        self.grid.scatter(i, k)
    }

    fn solid_angle(&self, i: usize) -> f64 {
        self.grid.solid_angle(i) * f64::from(self.rain[i])
    }
}

impl Grid for WindowGrid<'_> {
    fn neighbors(&self, i: usize) -> [u32; 8] {
        self.window.neighbors(i)
    }

    fn scatter(&self, i: usize, k: usize) -> f64 {
        self.window.scatter(i, k)
    }

    fn solid_angle(&self, i: usize) -> f64 {
        self.solid[i]
    }
}

/// The flow of the ground around the window of a stroke. It gives the water
/// that comes into that window from outside. The window must be wider than
/// one cell of the ground.
pub struct OuterFlow {
    grid: ErodeGrid,
    /// The flat coordinate of each cell center along `u` and along `v`.
    flat_u: Vec<f64>,
    flat_v: Vec<f64>,
    flow: Drain,
    /// The number of cells along one side of a face of the global map.
    global_size: usize,
    /// The rivers of the global map that start outside the window of `grid`:
    /// the cell, its receiver, and the solid angle that the cell drains.
    rivers: Vec<(u32, u32, f32)>,
}

impl OuterFlow {
    /// The flow of the cells of the coarse copy. `face_size` is the number
    /// of texels along one side of a face of the heightmap.
    pub fn global(heights: &CoarseHeights, face_size: usize) -> OuterFlow {
        OuterFlow::of(Erosion::global(heights, face_size), heights.m, Vec::new())
    }

    /// The flow of the cells of a window. `global` gives the rivers that
    /// come into the window from outside.
    pub fn window(heights: &WindowHeights, global: &FlowMap) -> OuterFlow {
        let (window, m) = (heights.window, global.size());
        let rivers = global.river_cells().filter_map(|(from, to, area)| {
            let outside = window.cell_at(index_dir(m, from)).is_none();
            outside.then_some((from as u32, to as u32, area as f32))
        });
        OuterFlow::of(Erosion::window(heights, global), m, rivers.collect())
    }

    fn of(erosion: Erosion, global_size: usize, rivers: Vec<(u32, u32, f32)>) -> OuterFlow {
        OuterFlow {
            grid: erosion.grid(),
            flow: erosion.drain(),
            flat_u: erosion.flat_u,
            flat_v: erosion.flat_v,
            global_size,
            rivers,
        }
    }

    /// The directions of the 4 corners of a cell.
    fn corners(&self, i: usize) -> [V3; 4] {
        match self.grid {
            ErodeGrid::Global { size } => index_corners(size, i),
            ErodeGrid::Window(window) => {
                let (x, y) = ((i % window.cells) as f64, (i / window.cells) as f64);
                [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                    .map(|(dx, dy)| window.dir(x + dx, y + dy))
            }
        }
    }

    /// The water that comes into the window of `inner` from outside: a cell
    /// of the window and a solid angle. A cell outside the window with its
    /// receiver in the window gives the area that it drains. The water goes
    /// to the lowest cell of the window in the square of the receiver, so a
    /// river comes in at its channel.
    fn inflow(&self, inner: &WindowHeights) -> Vec<(usize, f64)> {
        let window = inner.window;
        let end = window.cells as f64;
        let center = window.dir(end / 2.0, end / 2.0);
        // No direction of the window is farther from the center than a
        // corner.
        let corners = [(0.0, 0.0), (end, 0.0), (0.0, end), (end, end)];
        let reach = corners.map(|(u, v)| dot(center, window.dir(u, v)));
        let reach = reach.into_iter().fold(1.0, f64::min);

        let (side, view) = match self.grid {
            ErodeGrid::Global { size } => (size, None),
            ErodeGrid::Window(view) => (view.cells, Some(view)),
        };
        let dir = |i: usize| {
            let face = view.map_or(i / (side * side), |view| view.face);
            face_dir(face, self.flat_u[i % side], self.flat_v[i / side % side])
        };
        let mut inflow = Vec::new();
        for &from in &self.flow.order {
            let to = self.flow.receiver[from as usize];
            if to == OUT || to == NONE {
                continue;
            }
            let (from, to) = (from as usize, to as usize);
            let d = dir(to);
            if dot(center, d) < reach {
                continue;
            }
            if let Some(cell) = window.entry(dir(from), d) {
                let cell = lowest_in(inner, self.corners(to), cell);
                inflow.push((cell, f64::from(self.flow.area[from])));
            }
        }

        // A river of the global map can come into a part of the window that
        // is outside the window of `self`.
        let Some(view) = view else {
            return inflow;
        };
        let m = self.global_size;
        for &(from, to, area) in &self.rivers {
            let (from, to) = (from as usize, to as usize);
            let d = index_dir(m, to);
            if dot(center, d) < reach {
                continue;
            }
            let Some(cell) = window.entry(index_dir(m, from), d) else {
                continue;
            };
            // A cell of `self` outside the window of `inner` has this water
            // in its area.
            let counted = view
                .cell_at(d)
                .is_some_and(|(x, y)| window.cell_at(view.cell_dir(x, y)).is_none());
            if !counted {
                let cell = lowest_in(inner, index_corners(m, to), cell);
                inflow.push((cell, f64::from(area)));
            }
        }
        inflow
    }
}

/// The lowest cell of `heights` with its center in the square with these
/// corners. `middle` is the cell at the middle of the square. Of two cells
/// at one level, the result is the cell nearer to `middle`.
fn lowest_in(heights: &WindowHeights, corners: [V3; 4], middle: usize) -> usize {
    let window = heights.window;
    let (mut low, mut high) = ([f64::MAX; 2], [f64::MIN; 2]);
    for corner in corners {
        let Some((u, v)) = window.place(corner) else {
            return middle;
        };
        low = [low[0].min(u), low[1].min(v)];
        high = [high[0].max(u), high[1].max(v)];
    }
    let last = (window.cells - 1) as f64;
    let span = |axis: usize| {
        let first = (low[axis] - 0.5).ceil().clamp(0.0, last) as usize;
        first..=(high[axis] - 0.5).floor().clamp(0.0, last) as usize
    };
    let (mx, my) = (middle % window.cells, middle / window.cells);
    let mut best = (heights.levels[middle], 0, middle);
    for y in span(1) {
        for x in span(0) {
            let i = y * window.cells + x;
            let far = x.abs_diff(mx).pow(2) + y.abs_diff(my).pow(2);
            best = best.min((heights.levels[i], far, i));
        }
    }
    best.2
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
    limits: Limits,
    /// The width of the channels of a step on the global grid.
    channel: ChannelShape,
    rain: Rain,
    /// The rain of each cell, from the heights of the first step after a
    /// change of `rain`. It is empty when each cell gets the same rain.
    rain_cells: Vec<f32>,
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
        Erosion::of_window(heights, heights.window.inflow(global))
    }

    /// An erosion of the cells of a window that no river comes into from
    /// outside.
    pub fn window_alone(heights: &WindowHeights) -> Erosion {
        Erosion::of_window(heights, Vec::new())
    }

    fn of_window(heights: &WindowHeights, inflow: Vec<(usize, f64)>) -> Erosion {
        let window = heights.window;
        let flat = |first: i64| {
            let cells = 0..window.cells;
            let flat = cells.map(|i| unwarp(window.angle_at(first, i as f64 + 0.5)));
            flat.collect::<Vec<f64>>()
        };
        let (flat_u, flat_v) = (flat(window.x0), flat(window.y0));
        // The steps of `Window::cell_solid_angle`.
        let step =
            |f: f64| (1.0 + f * f) * FRAC_PI_2 * window.cell as f64 / window.face_size as f64;
        let mut solid = Vec::with_capacity(window.cells * window.cells);
        for &fv in &flat_v {
            for &fu in &flat_u {
                let r = 1.0 + fu * fu + fv * fv;
                solid.push(step(fu) * step(fv) / (r * r.sqrt()));
            }
        }
        let min_area = RIVER_MIN_CELLS * solid.iter().sum::<f64>() / solid.len() as f64;
        let cells = Cells::Window {
            window,
            inflow,
            solid,
        };
        Erosion::new(
            cells,
            window.cells,
            window.face_size,
            [flat_u, flat_v],
            min_area,
            &heights.levels,
        )
    }

    /// An erosion of the cells of a window in the ground of `outer`. The
    /// water of `outer` comes into the window from outside.
    pub fn window_in(heights: &WindowHeights, outer: &OuterFlow) -> Erosion {
        Erosion::of_window(heights, outer.inflow(heights))
    }

    /// An erosion of the cells of a window, for a stroke that leaves the
    /// window of `self`. A cell that the window of `self` holds keeps the
    /// height that the stroke gave it there. Each other cell has the height
    /// of `heights`. The water of `outer` comes into the new window from
    /// outside. With no `outer`, no river comes in. `self` must be an
    /// erosion of a window.
    pub fn moved(&self, heights: &WindowHeights, outer: Option<&OuterFlow>) -> Erosion {
        let Cells::Window { window: old, .. } = &self.cells else {
            panic!("the erosion has no window");
        };
        let (window, side) = (heights.window, self.side);
        let mut kept: Vec<f32> = heights.levels.iter().map(|&l| f32::from(l)).collect();
        let last = (side - 1) as f64;
        for (i, height) in kept.iter_mut().enumerate() {
            let dir = window.cell_dir(i % window.cells, i / window.cells);
            let Some((u, v)) = old.place(dir) else {
                continue;
            };
            let (u, v) = (u - 0.5, v - 0.5);
            if u < 0.0 || v < 0.0 || u > last || v > last {
                continue;
            }
            // The height between the centers of the 4 old cells around.
            let (x, y) = ((u as usize).min(side - 2), (v as usize).min(side - 2));
            let (tu, tv) = ((u - x as f64) as f32, (v - y as f64) as f32);
            let at = |x: usize, y: usize| self.heights[y * side + x];
            let top = at(x, y) * (1.0 - tu) + at(x + 1, y) * tu;
            let bottom = at(x, y + 1) * (1.0 - tu) + at(x + 1, y + 1) * tu;
            *height = top * (1.0 - tv) + bottom * tv;
        }
        let levels: Vec<u16> = kept.iter().map(|h| h.round() as u16).collect();
        let heights = WindowHeights { window, levels };
        let inflow = outer.map_or_else(Vec::new, |outer| outer.inflow(&heights));
        let mut erosion = Erosion::of_window(&heights, inflow);
        erosion.heights = kept;
        erosion.limits = self.limits;
        erosion.rain = self.rain;
        erosion
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
            limits: Limits::default(),
            channel: ChannelShape::default(),
            rain: Rain::default(),
            rain_cells: Vec::new(),
        }
    }

    /// Sets the limits of each later step and of each later call of `raise`.
    /// A step and `raise` do not change a cell outside the limits, and they
    /// do not move a cell past a limit.
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }

    /// Sets the width of the channels of each later step on the global
    /// grid.
    pub fn set_channel(&mut self, channel: ChannelShape) {
        self.channel = channel;
    }

    /// Sets the rain of each later step.
    pub fn set_rain(&mut self, rain: Rain) {
        if rain != self.rain {
            self.rain = rain;
            self.rain_cells.clear();
            self.stale = true;
        }
    }

    /// The rain of each cell of a window, in row order.
    pub(crate) fn cell_rain(&self) -> Vec<f32> {
        self.rain_of(0, 0, self.side, self.side)
    }

    /// The rain of the cells of a rectangle of each square, in the order of
    /// the squares, the rows, and the cells. The rectangle has its first
    /// cell at `x0` and `y0`, and `width` by `height` cells. The rain is
    /// computed at the corners of squares of `RAIN_STRIDE` cells, and a cell
    /// takes a blend of the 4 corners around it. A corner on a face edge is
    /// one direction on the sphere for the two faces, so the rain is
    /// continuous across the edge.
    fn rain_of(&self, x0: usize, y0: usize, width: usize, height: usize) -> Vec<f32> {
        let side = self.side;
        let (step, global) = match &self.cells {
            Cells::Global(grid) => (FRAC_PI_2 / grid.m as f64, true),
            Cells::Window { window, .. } => (
                FRAC_PI_2 * window.cell as f64 / window.face_size as f64,
                false,
            ),
        };
        let at = |x: usize, y: usize| f64::from(self.levels[y * side + x]);
        // The level at a place on the cells of one square. The center of
        // the first cell is at 0.
        let blend = |square: usize, u: f64, v: f64| {
            let (i, j) = (u.floor(), v.floor());
            let (tu, tv) = (u - i, v - j);
            let (i, j) = (i as i64, j as i64);
            let cell = |x: i64, y: i64| {
                let last = side as i64 - 1;
                if global {
                    let (face, x, y) = if (0..=last).contains(&x) && (0..=last).contains(&y) {
                        (square, x as usize, y as usize)
                    } else {
                        cell_past_edge(side, square, x, y)
                    };
                    f64::from(self.levels[(face * side + y) * side + x])
                } else {
                    at(x.clamp(0, last) as usize, y.clamp(0, last) as usize)
                }
            };
            let top = cell(i, j) * (1.0 - tu) + cell(i + 1, j) * tu;
            let bottom = cell(i, j + 1) * (1.0 - tu) + cell(i + 1, j + 1) * tu;
            top * (1.0 - tv) + bottom * tv
        };
        let level = |d: V3| match &self.cells {
            Cells::Global(_) => {
                let (face, a, b) = face_of(d);
                let place = |a: f64| (warp(a) + 1.0) * 0.5 * side as f64 - 0.5;
                Some(blend(face, place(a), place(b)))
            }
            Cells::Window { window, .. } => {
                let (u, v) = window.place(d)?;
                Some(blend(0, u - 0.5, v - 0.5))
            }
        };
        // The corner places along one side, in cells from the first cell.
        let corners = |count: usize| {
            let mut places: Vec<usize> = (0..count).step_by(RAIN_STRIDE).collect();
            places.push(count);
            places
        };
        let (us, vs) = (corners(width), corners(height));
        let mut rain = Vec::with_capacity(self.faces().len() * width * height);
        for face in self.faces() {
            let dir = |x: usize, y: usize| match &self.cells {
                Cells::Global(_) => {
                    let flat = |i: usize| unwarp(i as f64 / side as f64 * 2.0 - 1.0);
                    face_dir(face, flat(x0 + x), flat(y0 + y))
                }
                Cells::Window { window, .. } => window.dir((x0 + x) as f64, (y0 + y) as f64),
            };
            let mut corner = Vec::with_capacity(us.len() * vs.len());
            for &v in &vs {
                for &u in &us {
                    corner.push(self.rain.at(dir(u, v), step * TRACE_SPACING, level));
                }
            }
            // The corner before a place and the part of the way to the next.
            let between = |places: &[usize], p: f64| {
                let k = ((p / RAIN_STRIDE as f64) as usize).min(places.len() - 2);
                let (a, b) = (places[k] as f64, places[k + 1] as f64);
                (k, ((p - a) / (b - a)).clamp(0.0, 1.0))
            };
            for y in 0..height {
                let (j, tv) = between(&vs, y as f64 + 0.5);
                for x in 0..width {
                    let (i, tu) = between(&us, x as f64 + 0.5);
                    let c = |i: usize, j: usize| corner[j * us.len() + i];
                    let top = c(i, j) * (1.0 - tu) + c(i + 1, j) * tu;
                    let bottom = c(i, j + 1) * (1.0 - tu) + c(i + 1, j + 1) * tu;
                    rain.push((top * (1.0 - tv) + bottom * tv) as f32);
                }
            }
        }
        rain
    }

    /// The cells of the erosion.
    pub fn grid(&self) -> ErodeGrid {
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
        let rain = &self.rain_cells;
        match &self.cells {
            Cells::Global(grid) => self.drain_on(grid, &self.levels, &[], rain),
            Cells::Window {
                window,
                inflow,
                solid,
            } => self.drain_on(&WindowGrid { window, solid }, &self.levels, inflow, rain),
        }
    }

    /// The flow on a grid. With no `rain`, each cell gets the same rain.
    fn drain_on(
        &self,
        grid: &impl Grid,
        levels: &[u16],
        inflow: &[(usize, f64)],
        rain: &[f32],
    ) -> Drain {
        if rain.is_empty() {
            drain(grid, levels, inflow, self.outlet)
        } else {
            drain(&Rainy { grid, rain }, levels, inflow, self.outlet)
        }
    }

    fn neighbors(&self, i: usize) -> [u32; 8] {
        match &self.cells {
            Cells::Global(grid) => grid.neighbors(i),
            Cells::Window { window, .. } => window.neighbors(i),
        }
    }

    /// Calls `visit` for each cell in the circle of each brush. It gives the
    /// index of the brush, the row and the column of the cell, and the effect
    /// of the brush there.
    fn cells_under(&self, brushes: &[ErodeBrush], mut visit: impl FnMut(usize, usize, usize, f64)) {
        let side = self.side;
        let faces = self.faces();
        let global = matches!(self.cells, Cells::Global(_));
        for (index, brush) in brushes.iter().enumerate() {
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
                        visit(index, row, x, falloff(k, hard) * brush.flow);
                    }
                }
            }
        }
    }

    /// The effect of the brushes at each cell.
    fn weights(&self, brushes: &[ErodeBrush]) -> Weights {
        let side = self.side;
        let global = matches!(self.cells, Cells::Global(_));
        let mut weights = vec![0.0f32; self.heights.len()];
        let mut spans = vec![None; self.faces().len() * side];
        let mut hits = vec![false; brushes.len()];
        self.cells_under(brushes, |brush, row, x, mut weight| {
            if !global {
                let y = row % side;
                let edge = x.min(y).min(side - 1 - x).min(side - 1 - y);
                weight *= edge.min(FADE) as f64 / FADE as f64;
            }
            if weight <= 0.0 {
                return;
            }
            let cell = &mut weights[row * side + x];
            *cell = cell.max(weight as f32);
            let (first, last) = spans[row].unwrap_or((x, x));
            spans[row] = Some((first.min(x), last.max(x)));
            hits[brush] = true;
        });
        Weights {
            cells: weights,
            spans,
            hits,
        }
    }

    /// Raises the cells under the brushes, as raise stamps with this
    /// strength raise the texels. The effects of the brushes add up. The
    /// next step has no drop for a cell that only went up.
    pub fn raise(&mut self, brushes: &[ErodeBrush], strength: f64) {
        if strength.is_nan() || strength <= 0.0 {
            return;
        }
        let side = self.side;
        let mut rises = Vec::new();
        self.cells_under(brushes, |_, row, x, weight| {
            rises.push((row * side + x, weight * strength));
        });
        let limits = self.limits;
        for (i, rise) in rises {
            let old = f64::from(self.heights[i]);
            let height = limits.apply(old, (old + rise).min(65535.0)) as f32;
            self.heights[i] = height;
            let level = height.round() as u16;
            if level != self.levels[i] {
                self.levels[i] = level;
                self.stale = true;
            }
        }
    }

    /// Sets each cell under the brushes one time. `level` gives the new
    /// level of a cell from its direction and its level. With `None`, the
    /// cell does not change. The next step has no drop for a cell that only
    /// went up.
    pub fn lift(&mut self, brushes: &[ErodeBrush], level: impl Fn(V3, u16) -> Option<u16>) {
        let side = self.side;
        let mut cells = Vec::new();
        self.cells_under(brushes, |_, row, x, _| cells.push((row, x)));
        cells.sort_unstable();
        cells.dedup();
        let first = self.faces().start;
        for (row, x) in cells {
            let face = first + row / side;
            let dir = face_dir(face, self.flat_u[x], self.flat_v[row % side]);
            let i = row * side + x;
            let Some(new) = level(dir, self.levels[i]) else {
                continue;
            };
            self.heights[i] = f32::from(new);
            if new != self.levels[i] {
                self.levels[i] = new;
                self.stale = true;
            }
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
    ///
    /// On the global grid, a river cell goes down in a channel at the full
    /// resolution of the heightmap, and the ground beside the channel goes
    /// down as the cells beside the river do.
    pub fn step(&mut self, brushes: &[ErodeBrush], rate: f64) -> ErodeStep {
        let mut step = self.step_on(brushes, rate, false);
        self.add_channels(&mut step);
        step
    }

    /// Gives a step on the global grid the channels of its rivers, from the
    /// flow of the last step.
    fn add_channels(&self, step: &mut ErodeStep) {
        let (Cells::Global(grid), Some(flow)) = (&self.cells, &self.flow) else {
            return;
        };
        if step.is_empty() {
            return;
        }
        let river = |i: usize| f64::from(flow.area[i]) >= self.min_area;
        step.hill = (0..step.drops.len())
            .map(|i| {
                if !river(i) {
                    return step.drops[i];
                }
                let beside = grid.neighbors(i).into_iter().map(|j| j as usize);
                let (sum, count) = beside
                    .filter(|&j| !river(j))
                    .fold((0u32, 0u32), |(sum, count), j| {
                        (sum + u32::from(step.drops[j]), count + 1)
                    });
                (sum + count / 2).checked_div(count).unwrap_or(0) as u16
            })
            .collect();
        step.channel = self.channel;
        let curves = drop_curves(grid.m, flow, self.min_area, &step.drops);
        // The curves have no floodplain, so the world size has no effect.
        step.lines = Some(ChannelLines::faces(curves, grid.m, 6371.0));
    }

    /// The rectangle of window cells around the cells of `spans`: the first
    /// cell and the size. The room at each side is a quarter of the larger
    /// side of the cells, and `NEAR_ROOM` cells at least.
    fn near(&self, spans: &[Option<(usize, usize)>]) -> (usize, usize, usize, usize) {
        let rows = || {
            spans
                .iter()
                .enumerate()
                .filter_map(|(y, span)| Some((y, (*span)?)))
        };
        let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0, usize::MAX, 0);
        for (y, (first, last)) in rows() {
            (x0, x1) = (x0.min(first), x1.max(last));
            (y0, y1) = (y0.min(y), y1.max(y));
        }
        let room = ((x1 - x0).max(y1 - y0) / 4).max(NEAR_ROOM);
        let last = self.side - 1;
        let (x0, y0) = (x0.saturating_sub(room), y0.saturating_sub(room));
        let (x1, y1) = ((x1 + room).min(last), (y1 + room).min(last));
        (x0, y0, x1 - x0 + 1, y1 - y0 + 1)
    }

    /// One step. If `near` is true and the cells are those of a window, the
    /// flow is that of the cells near the brushes only: the water leaves at
    /// the sides of a rectangle around them, and no water comes in. Thus the
    /// time of the step follows the size of the brushes, and not the size of
    /// the window.
    fn step_on(&mut self, brushes: &[ErodeBrush], rate: f64, near: bool) -> ErodeStep {
        let mut step = ErodeStep {
            grid: self.grid(),
            drops: vec![0; self.heights.len()],
            rects: [None; FACES],
            limits: self.limits,
            hill: Vec::new(),
            lines: None,
            channel: self.channel,
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
        // The flow, and the index in the grid of each cell of the flow.
        let (flow, place) = match &self.cells {
            Cells::Window { window, solid, .. } if near => {
                let (x0, y0, width, height) = self.near(&spans);
                let grid = BoxGrid {
                    window,
                    solid,
                    x0,
                    y0,
                    width,
                    height,
                };
                let rows = (y0..y0 + height).map(|y| y * self.side + x0);
                let levels: Vec<u16> = rows
                    .flat_map(|first| &self.levels[first..first + width])
                    .copied()
                    .collect();
                let rain = if self.rain.is_even() {
                    Vec::new()
                } else {
                    self.rain_of(x0, y0, width, height)
                };
                let flow = self.drain_on(&grid, &levels, &[], &rain);
                self.flow = None;
                (flow, Some((x0, y0, width)))
            }
            _ => {
                let flow = match self.flow.take() {
                    Some(flow) if !self.stale => flow,
                    _ => {
                        if !self.rain.is_even() && self.rain_cells.is_empty() {
                            self.rain_cells = self.rain_of(0, 0, self.side, self.side);
                        }
                        self.drain()
                    }
                };
                (flow, None)
            }
        };
        self.stale = false;
        let side = self.side;
        let limits = self.limits;
        let cell = |i: u32| match place {
            Some((x0, y0, width)) => (y0 + i as usize / width) * side + x0 + i as usize % width,
            None => i as usize,
        };
        // Each receiver has its new height before the cells above it.
        for &at in &flow.order {
            let i = cell(at);
            let to = flow.receiver[at as usize];
            if weights[i] <= 0.0 || to == OUT {
                continue;
            }
            let to = cell(to);
            let below = self.heights[to];
            let height = self.heights[i];
            if height <= below {
                continue;
            }
            let diagonal = !self.neighbors(i)[..4].contains(&(to as u32));
            let length = if diagonal { SQRT_2 } else { 1.0 };
            let area = f64::from(flow.area[at as usize]) / self.min_area;
            let power = rate * f64::from(weights[i]) * area.sqrt() / length;
            let new = f64::from(below) + f64::from(height - below) / (1.0 + power);
            let new = (new as f32).clamp(below, height);
            self.heights[i] = limits.apply(f64::from(height), f64::from(new)) as f32;
        }
        if place.is_none() {
            self.flow = Some(flow);
        }

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

    /// Lowers the cells under the brushes `count` times. The strengths of
    /// the steps add up to `rate`. The result has the sum of the drops. Each
    /// step computes the flow again, so the valleys of one step collect the
    /// water of the next step. In a window, the flow is that of the cells
    /// near the brushes only.
    pub fn steps(&mut self, brushes: &[ErodeBrush], rate: f64, count: usize) -> ErodeStep {
        let count = count.max(1);
        let mut sum = self.step_on(brushes, rate / count as f64, true);
        for _ in 1..count {
            let step = self.step_on(brushes, rate / count as f64, true);
            for (sum, drop) in sum.drops.iter_mut().zip(&step.drops) {
                *sum += drop;
            }
            for (sum, rect) in sum.rects.iter_mut().zip(step.rects) {
                *sum = match (*sum, rect) {
                    (Some(a), Some(b)) => Some(a.union(b)),
                    (a, b) => a.or(b),
                };
            }
        }
        self.add_channels(&mut sum);
        sum
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
            limits: Limits::default(),
            hill: Vec::new(),
            lines: None,
            channel: ChannelShape::default(),
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
    fn a_channel_one_cell_wide_has_no_dots_and_no_diamonds() {
        let (m, n) = (16, 256);
        let width = n / m;
        // The least and the most drop on the texels of the diagonal of face
        // 0, between the centers of cells 3 and 12.
        let range = |step: &ErodeStep| {
            let on =
                (3 * width + width / 2..12 * width + width / 2).map(|x| step.drop_at(n, 0, x, x));
            on.fold((f64::MAX, f64::MIN), |(least, most), drop| {
                (least.min(drop), most.max(drop))
            })
        };
        // The cells of a diagonal of each face have a drop. With the blend
        // of 4 cells, the least drop is half of the most.
        let channel = step_of(m, n, |i, _| if i % m == i / m % m { 800 } else { 0 });
        let (least, most) = range(&channel);
        assert!(least > 0.8 * most && most > 400.0, "{least} {most}");
        // The cells of a diagonal have no drop, and each other cell has one.
        let ridge = step_of(m, n, |i, _| if i % m == i / m % m { 0 } else { 800 });
        let (least, most) = range(&ridge);
        assert!(800.0 - most > 0.8 * (800.0 - least), "{least} {most}");
        // The slope of the drop has no jump from one texel to the next. With
        // the blend of 4 cells, the slope changes by 100 at a cell center.
        let limit = 2.0 * 800.0 / (width * width) as f64 * 1.01;
        for step in [&channel, &ridge] {
            for y in width..n - width {
                for x in width..n - width - 2 {
                    let at = |x: usize| step.drop_at(n, 0, x, y);
                    let bend = at(x + 2) - 2.0 * at(x + 1) + at(x);
                    assert!(bend.abs() <= limit, "{x} {y} {bend}");
                }
            }
        }
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

    /// A wide valley along the equator with gentle sides, from face 2 to
    /// face 0.
    fn gentle_valley(d: V3) -> u16 {
        let (lon, lat) = crate::math::dir_to_lonlat(d);
        let across = (lat - 0.4).abs();
        meters_to_level(if lon > 10.0 && lon < 80.0 && across < 10.0 {
            100.0 + 20.0 * (lon - 10.0) + 40.0 * across
        } else {
            OCEAN
        })
    }

    #[test]
    fn a_global_step_lowers_a_channel_much_more_than_the_slopes() {
        let n = 2048;
        let mut map = Heightmap::new(n, meters_to_level(OCEAN));
        for face in [0, 2] {
            let levels: Vec<u16> = (0..n * n)
                .map(|i| gentle_valley(map.texel_dir(face, i % n, i / n)))
                .collect();
            map.store_face(face, &levels);
        }
        let heights = CoarseHeights::new(&map);
        let m = heights.m;
        let mut erosion = Erosion::global(&heights, n);
        let center = lonlat_to_dir(40.0, 0.4);
        let brushes = [ErodeBrush {
            center,
            radius: 0.1,
            hardness: 0.8,
            flow: 1.0,
        }];
        let step = erosion.step(&brushes, 2.0);
        let lines = step.lines().expect("a global step has channels");
        assert!(lines.has_curves());
        let before = texels(&map);
        map.erode(&step);
        let after = texels(&map);
        let flow = erosion.flow.as_ref().expect("the step keeps its flow");
        let cut = ChannelCut {
            shape: step.channel(),
            depth: 0.0,
            texel: FRAC_PI_2 / n as f64,
        };
        let (mut slopes, mut channels) = (Vec::new(), Vec::new());
        for y in 0..n {
            for x in 0..n {
                let p = map.texel_dir(0, x, y);
                if angle(p, center) > 0.08 {
                    continue;
                }
                let at = y * n + x;
                let drop = f64::from(before[at] - after[at]);
                let bin = lines.texel_bin(0, x, y, n).unwrap();
                let weight = lines
                    .near(bin)
                    .map(|curve| cut.channel_weight(&cut.place(curve, p)))
                    .fold(0.0, f64::max);
                let cell = (y * m / n) * m + x * m / n;
                if weight >= 1.0 {
                    channels.push(drop);
                } else if weight <= 0.0 && f64::from(flow.area[cell]) < RIVER_MIN_AREA {
                    slopes.push(drop);
                }
            }
        }
        let mean = |drops: &[f64]| drops.iter().sum::<f64>() / drops.len() as f64;
        let (slope, channel) = (mean(&slopes), mean(&channels));
        assert!(
            channels.len() > 50 && slopes.len() > 1000,
            "{} {}",
            channels.len(),
            slopes.len()
        );
        assert!(
            channel > 20.0 && slope < 0.25 * channel,
            "{slope} {channel}"
        );
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
    fn raise_lifts_the_cells_and_a_step_cuts_only_the_lifted_land() {
        let m = 64;
        let flat = CoarseHeights::from_fn(m, |_| meters_to_level(OCEAN));
        let floor = meters_to_level(OCEAN);
        let center = lonlat_to_dir(10.0, 5.0);
        let brush = ErodeBrush {
            center,
            radius: 0.2,
            hardness: 0.5,
            flow: 0.5,
        };
        let (face, x, y) = cell_at(m, center);
        let middle = (face * m + y) * m + x;
        let run = || {
            let mut erosion = Erosion::global(&flat, 1024);
            // A raise without a strength changes nothing.
            erosion.raise(&[brush], 0.0);
            assert!(erosion.levels.iter().all(|&level| level == floor));
            // The effects of two brushes add up: 2 times 0.5 times 4000.
            erosion.raise(&[brush, brush], 4000.0);
            assert_eq!(erosion.levels[middle], floor + 4000);
            let lifted: Vec<bool> = erosion.levels.iter().map(|&l| l > floor).collect();
            let hill = erosion.levels.clone();
            for (i, _) in lifted.iter().enumerate().filter(|&(_, &lifted)| lifted) {
                let dir = cell_dir(m, i / (m * m), i % m, i / m % m);
                assert!(angle(dir, center) < 0.2);
            }
            assert!(lifted.iter().filter(|&&lifted| lifted).count() > 100);

            // The rise is not a drop of the step, and no cell goes below the
            // floor around the hill.
            let step = erosion.step(&[brush], 4.0);
            let mut cut = 0;
            for (i, &drop) in step.drops().iter().enumerate() {
                assert!(drop == 0 || lifted[i], "cell {i}");
                assert!(hill[i] - drop >= floor, "cell {i}");
                assert_eq!(erosion.levels[i], hill[i] - drop);
                cut += usize::from(drop > 0);
            }
            assert!(cut > 50, "{cut}");
            erosion.levels.clone()
        };
        // Two equal runs give equal levels.
        assert!(run() == run());
    }

    #[test]
    fn lift_sets_the_cells_under_the_brushes_once() {
        let m = 64;
        let floor = meters_to_level(OCEAN);
        let flat = CoarseHeights::from_fn(m, |_| floor);
        let center = lonlat_to_dir(10.0, 5.0);
        let brush = ErodeBrush {
            center,
            radius: 0.2,
            hardness: 0.5,
            flow: 0.5,
        };
        let mut erosion = Erosion::global(&flat, 1024);
        let calls = std::cell::RefCell::new(Vec::new());
        // Two brushes on the same cells visit each cell one time. Cells
        // north of the center do not change.
        erosion.lift(&[brush, brush], |dir, level| {
            calls.borrow_mut().push(dir);
            assert_eq!(level, floor);
            (dir[2] < center[2]).then_some(floor + 1000)
        });
        let calls = calls.into_inner();
        let mut lifted = 0;
        for (i, &level) in erosion.levels.iter().enumerate() {
            let dir = cell_dir(m, i / (m * m), i % m, i / m % m);
            let under = angle(dir, center) < 0.2;
            let count = calls.iter().filter(|&&d| angle(d, dir) < 1e-9).count();
            assert_eq!(count, usize::from(under), "cell {i}");
            if under && dir[2] < center[2] {
                assert_eq!(level, floor + 1000, "cell {i}");
                lifted += 1;
            } else {
                assert_eq!(level, floor, "cell {i}");
            }
        }
        assert!(lifted > 50, "{lifted}");
        // The lift is not a drop of the next step.
        let step = erosion.step(&[brush], 4.0);
        assert!(
            step.drops()
                .iter()
                .zip(&erosion.levels)
                .all(|(&drop, &level)| { drop == 0 || level + drop == floor + 1000 })
        );
    }

    #[test]
    fn merge_adds_the_drops_of_two_steps() {
        let (m, n) = (8, 64);
        let small = TexelRect {
            x0: 70,
            y0: 2,
            x1: 80,
            y1: 9,
        };
        let mut a = step_of(m, n, |i, _| (i % 7) as u16);
        a.rects[1] = None;
        a.rects[2] = None;
        let mut b = step_of(m, n, |i, _| if i == 3 { u16::MAX } else { (i % 5) as u16 });
        b.rects[0] = Some(small);
        b.rects[1] = Some(small);
        b.rects[2] = None;
        let mut sum = step_of(m, n, |i, _| (i % 7) as u16);
        sum.rects = a.rects;
        assert!(sum.merge(&b));
        for (i, &drop) in sum.drops().iter().enumerate() {
            let expected = a.drops()[i].saturating_add(b.drops()[i]);
            assert_eq!(drop, expected, "cell {i}");
        }
        for (face, rect) in sum.rects().into_iter().enumerate() {
            let joined = match (a.rects()[face], b.rects()[face]) {
                (Some(x), Some(y)) => Some(x.union(y)),
                (x, y) => x.or(y),
            };
            assert_eq!(rect, joined, "face {face}");
        }
        // Steps of other cells do not merge.
        let other = step_of(2 * m, n, |_, _| 1);
        assert!(!sum.merge(&other));
        assert_eq!(sum.drops()[3], u16::MAX);
    }

    #[test]
    fn a_step_with_a_floor_leaves_no_cell_and_no_texel_below_it() {
        let (m, n) = (48, 96);
        let heights = CoarseHeights::from_fn(m, lumpy);
        let mut sorted = heights.levels.clone();
        sorted.sort_unstable();
        // Half of the cells are above the floor.
        let limits = Limits {
            floor: sorted[sorted.len() / 2],
            ..Limits::default()
        };
        let brushes = brushes_on_all_sides();
        let mut erosion = Erosion::global(&heights, n);
        erosion.set_limits(limits);
        let mut free = Erosion::global(&heights, n);
        let mut map = map_of(n, lumpy);
        // The texels with the drops of the steps and no floor.
        let mut loose = map_of(n, lumpy);
        let first = texels(&map);
        for _ in 0..10 {
            let before = erosion.heights.clone();
            let step = erosion.step(&brushes, 1e6);
            free.step(&brushes, 1e6);
            assert_eq!(step.limits(), limits);
            let floor = f32::from(limits.floor);
            for (i, (&old, &new)) in before.iter().zip(&erosion.heights).enumerate() {
                assert!(new >= old.min(floor), "cell {i}: {old} {new}");
            }
            let old = texels(&map);
            map.erode(&step);
            for (i, (&old, &new)) in old.iter().zip(&texels(&map)).enumerate() {
                assert!(new >= old.min(limits.floor), "texel {i}: {old} {new}");
            }
            loose.erode(&ErodeStep {
                limits: Limits::default(),
                drops: step.drops.clone(),
                ..step
            });
        }
        // The floor stops cells and texels that go below it with no floor.
        let cells = heights.levels.iter().zip(&erosion.levels).zip(&free.levels);
        let stopped = cells.filter(|&((&old, &new), &free)| {
            old > limits.floor && new == limits.floor && free < limits.floor
        });
        assert!(stopped.count() > 100);
        let last = texels(&map);
        let all = first.iter().zip(&last).zip(texels(&loose));
        let stopped = all.filter(|&((&old, &new), loose)| {
            old > limits.floor && new == limits.floor && loose < limits.floor
        });
        assert!(stopped.count() > 100);
        assert!(first.iter().zip(&last).any(|(old, new)| new < old));
    }

    #[test]
    fn a_moved_erosion_keeps_the_limits() {
        let n = 256;
        let map = map_of(n, equator_valley);
        let window = |lon: f64| Window::centered(lonlat_to_dir(lon, 0.4), n, 1, 64);
        let limits = Limits {
            floor: 1000,
            ceiling: 50_000,
        };
        let mut erosion = Erosion::window_alone(&WindowHeights::new(&map, window(10.0)));
        erosion.set_limits(limits);
        let moved = erosion.moved(&WindowHeights::new(&map, window(12.0)), None);
        assert_eq!(moved.limits, limits);
    }

    #[test]
    fn raise_with_a_ceiling_stops_the_cells_at_it() {
        let m = 64;
        let floor = meters_to_level(OCEAN);
        let flat = CoarseHeights::from_fn(m, |_| floor);
        let center = lonlat_to_dir(10.0, 5.0);
        let brush = ErodeBrush {
            center,
            radius: 0.2,
            hardness: 0.5,
            flow: 0.5,
        };
        let (face, x, y) = cell_at(m, center);
        let middle = (face * m + y) * m + x;
        let mut erosion = Erosion::global(&flat, 1024);
        erosion.set_limits(Limits {
            ceiling: floor + 1500,
            ..Limits::default()
        });
        // With no ceiling, the cell in the middle goes up by 4000 levels.
        erosion.raise(&[brush, brush], 4000.0);
        assert_eq!(erosion.levels[middle], floor + 1500);
        assert!(erosion.levels.iter().all(|&level| level <= floor + 1500));
        let between = |&&level: &&u16| level > floor && level < floor + 1500;
        assert!(erosion.levels.iter().filter(between).count() > 20);

        // A cell above the ceiling does not go up.
        let hill = erosion.levels.clone();
        erosion.set_limits(Limits {
            ceiling: floor + 500,
            ..Limits::default()
        });
        erosion.raise(&[brush], 4000.0);
        for (i, (&old, &new)) in hill.iter().zip(&erosion.levels).enumerate() {
            assert!(new >= old && new <= old.max(floor + 500), "cell {i}");
        }
        assert_eq!(erosion.levels[middle], floor + 1500);
        assert!(erosion.levels != hill);
    }

    #[test]
    fn a_moved_erosion_keeps_the_heights_of_the_stroke() {
        let n = 1024;
        let floor = meters_to_level(OCEAN);
        let ground = |window: Window| WindowHeights::from_fn(window, |_| floor);
        // The window is near the edge of its face.
        let first = Window::centered(lonlat_to_dir(38.0, 0.0), n, 1, 128);
        let center = first.cell_dir(100, 64);
        let brush = ErodeBrush {
            center,
            radius: 0.02,
            hardness: 0.5,
            flow: 1.0,
        };
        let mut erosion = Erosion::window_alone(&ground(first));
        erosion.raise(&[brush], 4000.0);
        erosion.steps(&[brush], 8.0, 4);

        // The next window is 60 cells to the side on the same face.
        let next = Window {
            x0: first.x0 + 60,
            ..first
        };
        let moved = erosion.moved(&ground(next), None);
        assert_eq!(moved.grid(), ErodeGrid::Window(next));
        let mut hill = 0;
        for y in 0..128 {
            for x in 0..128 {
                let new = moved.heights[y * 128 + x];
                if x + 60 < 128 {
                    assert_eq!(new, erosion.heights[y * 128 + x + 60], "{x} {y}");
                    hill += usize::from(new > f32::from(floor) + 100.0);
                } else {
                    assert_eq!(new, f32::from(floor), "{x} {y}");
                }
                assert_eq!(moved.levels[y * 128 + x], new.round() as u16);
            }
        }
        assert!(hill > 50, "{hill}");

        // A window on the next face keeps the hill at its place on the
        // sphere.
        let far = Window::centered(lonlat_to_dir(50.0, 0.0), n, 1, 512);
        assert!(far.face != first.face);
        let moved = erosion.moved(&ground(far), None);
        let (x, y) = far.cell_at(center).unwrap();
        let (old_x, old_y) = first.cell_at(center).unwrap();
        let (new, old) = (
            moved.heights[y * 512 + x],
            erosion.heights[old_y * 128 + old_x],
        );
        assert!(old > f32::from(floor) + 1000.0);
        assert!(
            (new - old).abs() < 0.2 * (old - f32::from(floor)),
            "{new} {old}"
        );
    }

    /// The largest water that comes into an erosion of a window from
    /// outside: the cell, the solid angle of the water, and the solid angle
    /// that drains through the cell.
    fn largest_inflow(erosion: &Erosion) -> (usize, f64, f64) {
        let Cells::Window { inflow, .. } = &erosion.cells else {
            panic!("the erosion has no window");
        };
        let largest = inflow.iter().max_by(|a, b| a.1.total_cmp(&b.1));
        let &(cell, water) = largest.expect("water comes in");
        (cell, water, f64::from(erosion.drain().area[cell]))
    }

    /// The direction of a cell of a window.
    fn dir_of(window: Window, cell: usize) -> V3 {
        window.cell_dir(cell % window.cells, cell / window.cells)
    }

    /// The ocean on all of the sphere.
    fn dry() -> FlowMap {
        FlowMap::new(&CoarseHeights::from_fn(64, |_| meters_to_level(OCEAN)))
    }

    #[test]
    fn a_stream_of_the_view_keeps_its_area_in_the_window_of_the_brush() {
        let n = 8192;
        let center = lonlat_to_dir(20.0, 0.4);
        // The window of the brush is 16 cells of the view wide.
        let view = WindowHeights::from_fn(Window::centered(center, n, 16, 128), equator_valley);
        let inner = WindowHeights::from_fn(Window::centered(center, n, 2, 128), equator_valley);
        let outer = OuterFlow::window(&view, &dry());
        let erosion = Erosion::window_in(&inner, &outer);
        let (cell, water, inside) = largest_inflow(&erosion);
        // The stream drains a large part of the view.
        let view_cell = view.window.cell_solid_angle(64, 64);
        assert!(water > 500.0 * view_cell, "{water} {view_cell}");
        // The area in the view at the place where the stream comes in.
        let (x, y) = view.window.cell_at(dir_of(inner.window, cell)).unwrap();
        let outside = f64::from(outer.flow.area[y * 128 + x]);
        eprintln!("the area inside and outside: {inside} {outside}");
        assert!(inside >= water && (inside / outside - 1.0).abs() < 0.05);
        // Without the view, the stream is small.
        let alone = Erosion::window_alone(&inner).drain();
        let alone = alone.area.iter().fold(0.0f32, |a, &b| a.max(b));
        assert!(f64::from(alone) < 0.2 * inside, "{alone} {inside}");
    }

    #[test]
    fn a_global_river_keeps_its_area_in_the_window_of_the_brush() {
        let n = 8192;
        let coarse = CoarseHeights::from_fn(64, equator_valley);
        let global = FlowMap::new(&coarse);
        // The area of the global map at a cell of a window.
        let global_area = |window: Window, cell: usize| {
            let (face, x, y) = cell_at(64, dir_of(window, cell));
            global.area(face, x, y)
        };
        let check = |inner: &WindowHeights, outer: &OuterFlow| {
            let erosion = Erosion::window_in(inner, outer);
            let (cell, water, inside) = largest_inflow(&erosion);
            let outside = global_area(inner.window, cell);
            eprintln!("the area inside and outside: {inside} {outside}");
            assert!(water >= RIVER_MIN_AREA, "{water}");
            assert!(inside >= water && (inside / outside - 1.0).abs() < 0.05);
            // No water counts two times.
            let Cells::Window { inflow, .. } = &erosion.cells else {
                panic!("the erosion has no window");
            };
            let sum: f64 = inflow.iter().map(|&(_, water)| water).sum();
            eprintln!("the sum {sum} {water}");
            assert!(sum < 1.05 * outside, "{sum} {outside}");
            cell
        };

        // The window of the brush is 4 cells of the global map wide.
        let center = lonlat_to_dir(20.0, 0.4);
        let inner = WindowHeights::from_fn(Window::centered(center, n, 2, 256), equator_valley);
        check(&inner, &OuterFlow::global(&coarse, n));

        // The east half of the window of the brush is outside the view. The
        // river comes into that half.
        let view = WindowHeights::from_fn(Window::centered(center, n, 16, 64), equator_valley);
        let east = view.window.dir(64.0, 32.0);
        let inner = WindowHeights::from_fn(Window::centered(east, n, 2, 256), equator_valley);
        let cell = check(&inner, &OuterFlow::window(&view, &global));
        assert!(view.window.cell_at(dir_of(inner.window, cell)).is_none());
    }

    #[test]
    fn a_moved_erosion_keeps_the_heights_and_the_river_of_the_ground() {
        let n = 8192;
        let coarse = CoarseHeights::from_fn(64, equator_valley);
        let outer = OuterFlow::global(&coarse, n);
        let ground = |window: Window| WindowHeights::from_fn(window, equator_valley);
        let center = lonlat_to_dir(20.0, 0.4);
        let first = Window::centered(center, n, 2, 256);
        let brush = ErodeBrush {
            center,
            radius: 0.01,
            hardness: 0.5,
            flow: 1.0,
        };
        let mut erosion = Erosion::window_in(&ground(first), &outer);
        for _ in 0..4 {
            erosion.step(&[brush], 1.0);
        }

        // The next window is 60 cells to the side.
        let next = Window {
            x0: first.x0 + 120,
            ..first
        };
        let moved = erosion.moved(&ground(next), Some(&outer));
        assert_eq!(moved.grid(), ErodeGrid::Window(next));
        let fresh = ground(next);
        let mut lowered = 0;
        for y in 0..256 {
            for x in 0..256 {
                let i = y * 256 + x;
                let new = moved.heights[i];
                if x + 60 < 256 {
                    assert_eq!(new, erosion.heights[i + 60], "{x} {y}");
                    lowered += usize::from(new < f32::from(fresh.levels[i]) - 2.0);
                } else {
                    assert_eq!(new, f32::from(fresh.levels[i]), "{x} {y}");
                }
            }
        }
        assert!(lowered > 50, "{lowered}");

        let (_, water, inside) = largest_inflow(&moved);
        assert!(water >= RIVER_MIN_AREA && inside >= water, "{water}");
        let (_, same, _) = largest_inflow(&Erosion::window_in(&fresh, &outer));
        assert!((water / same - 1.0).abs() < 0.05, "{water} {same}");
        // With no ground around the window, no river comes in.
        let alone = erosion.moved(&ground(next), None);
        let Cells::Window { inflow, .. } = &alone.cells else {
            panic!("the erosion has no window");
        };
        assert!(inflow.is_empty());
    }

    /// The valleys of one brush on the equator valley, after 8 steps on the
    /// cells of the view and on the cells of a window of the brush. `rate`
    /// is the strength of one step. The result is the largest drop of each,
    /// in levels, and the width of the valley of the window of the brush,
    /// in cells. The width is the number of cells in a line through the
    /// deepest cell that have half of its drop or more. The line is along
    /// the axis that gives the smaller number.
    fn valleys(rate: f64) -> (f64, f64, usize) {
        let n = 8192;
        let center = lonlat_to_dir(20.0, 0.4);
        let global = FlowMap::new(&CoarseHeights::from_fn(64, equator_valley));
        let view = WindowHeights::from_fn(Window::centered(center, n, 16, 128), equator_valley);
        let inner = WindowHeights::from_fn(Window::centered(center, n, 2, 128), equator_valley);
        let outer = OuterFlow::window(&view, &global);
        let brush = ErodeBrush {
            center,
            radius: 0.01,
            hardness: 0.5,
            flow: 1.0,
        };
        let drops = |heights: &WindowHeights, mut erosion: Erosion| {
            for _ in 0..8 {
                erosion.step(&[brush], rate);
            }
            let drops = heights.levels.iter().zip(&erosion.heights);
            drops
                .map(|(&old, &new)| f64::from(old) - f64::from(new))
                .collect::<Vec<f64>>()
        };
        let largest = |drops: &[f64]| drops.iter().fold(0.0f64, |a, &b| a.max(b));
        let coarse = drops(&view, Erosion::window(&view, &global));
        let fine = drops(&inner, Erosion::window_in(&inner, &outer));
        let deepest = largest(&fine);
        let at = fine.iter().position(|&drop| drop == deepest).unwrap();
        let run = |step: usize| {
            let deep = |&i: &usize| fine[i] >= 0.5 * deepest;
            let up = (1..).map(|k| at + k * step).take_while(deep).count();
            let down = (1..).map(|k| at - k * step).take_while(deep).count();
            up + 1 + down
        };
        (largest(&coarse), deepest, run(1).min(run(128)))
    }

    #[test]
    fn the_window_of_the_brush_cuts_as_deep_as_the_view() {
        for rate in [0.05, 1.0] {
            let (coarse, fine, _) = valleys(rate);
            eprintln!("the largest drop at the rate {rate}: {coarse} and {fine}");
            assert!(coarse > 10.0 && fine > 10.0, "{coarse} {fine}");
            assert!(
                fine < 2.0 * coarse && coarse < 2.0 * fine,
                "{coarse} {fine}"
            );
        }
    }

    #[test]
    fn the_valley_of_the_window_of_the_brush_is_narrower_than_a_cell_of_the_view() {
        // A cell of the view is 8 cells of the window of the brush wide.
        for rate in [0.05, 1.0] {
            let (_, _, width) = valleys(rate);
            eprintln!("the width at the rate {rate}: {width} cells");
            assert!(width < 8, "{width}");
        }
    }

    #[test]
    fn steps_give_the_sum_of_the_drops_of_each_step() {
        let m = 128;
        let flat = CoarseHeights::from_fn(m, |_| meters_to_level(OCEAN));
        let brush = ErodeBrush {
            center: lonlat_to_dir(10.0, 5.0),
            radius: 0.2,
            hardness: 0.5,
            flow: 1.0,
        };
        let hill = || {
            let mut erosion = Erosion::global(&flat, 1024);
            erosion.raise(&[brush], 4000.0);
            erosion
        };
        let mut together = hill();
        let before = together.levels.clone();
        let sum = together.steps(&[brush], 8.0, 4);
        assert!(!sum.is_empty());
        for (i, &drop) in sum.drops().iter().enumerate() {
            assert_eq!(together.levels[i], before[i] - drop, "cell {i}");
        }

        let mut apart = hill();
        for _ in 0..4 {
            apart.step(&[brush], 2.0);
        }
        assert!(apart.levels == together.levels);

        // In a window, the flow of the steps is that of the cells near the
        // brush, and the cells far from the brush do not change.
        let window = Window::centered(brush.center, 1024, 1, 512);
        let ground = WindowHeights::from_fn(window, lumpy);
        let small = ErodeBrush {
            radius: 0.03,
            ..brush
        };
        let mut near = Erosion::window_alone(&ground);
        near.raise(&[small], 4000.0);
        let before = near.levels.clone();
        let sum = near.steps(&[small], 8.0, 4);
        let lowered = sum.drops().iter().filter(|&&drop| drop > 0).count();
        assert!(lowered > 100, "{lowered}");
        for (i, &drop) in sum.drops().iter().enumerate() {
            assert_eq!(near.levels[i], before[i] - drop, "cell {i}");
            let dir = window.cell_dir(i % 512, i / 512);
            assert!(drop == 0 || angle(dir, small.center) < 0.03, "cell {i}");
        }

        // The flow of a step follows the valleys of the step before it.
        let mut one = hill();
        one.step(&[brush], 8.0);
        assert!(one.levels != together.levels);
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
            lower: 0.0,
            talus: 0.0,
            scale: 0.1,
            seed: 1,
            motion: [0.0; 3],
            ridged: false,
            relief: 0.0,
            channel: crate::channel::ChannelShape::default(),
            invert: false,
            limits: Limits::default(),
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

    const WEST_WIND: Rain = Rain {
        wind: 270.0,
        shadow: 1.0,
    };

    /// A range from north to south on face 0, with sea at each side. The
    /// range is 2,500 m high, and it has a ridge at each 4 cells along its
    /// sides, so the slopes drain in many valleys.
    fn ridge_range(m: usize) -> CoarseHeights {
        from_cells(m, |face, x, y| {
            let across = (x as f64 - m as f64 / 2.0 + 0.5).abs();
            if face != 0 || across > 14.0 || !(m / 4..3 * m / 4).contains(&y) {
                return OCEAN;
            }
            let ribs = 150.0 * ((y as f64 * std::f64::consts::FRAC_PI_2).sin() + 1.0);
            100.0 + 2400.0 * (1.0 - across / 14.0) + ribs
        })
    }

    /// The sum of the drops of the cells to the west and to the east of the
    /// crest, after 4 weak steps of a brush over the range.
    fn west_and_east_drops(rain: Rain) -> (u64, u64) {
        let m = 64;
        let heights = ridge_range(m);
        let mut erosion = Erosion::global(&heights, 8192);
        erosion.set_rain(rain);
        let brush = ErodeBrush {
            center: cell_dir(m, 0, m / 2, m / 2),
            radius: 0.45,
            hardness: 0.9,
            flow: 1.0,
        };
        let (mut west, mut east) = (0, 0);
        for _ in 0..4 {
            let step = erosion.step(&[brush], 0.02);
            for (i, &drop) in step.drops().iter().enumerate().take(m * m) {
                // The coast cells drop to the sea floor, so they are left out.
                let across = (i % m) as f64 - m as f64 / 2.0 + 0.5;
                if across.abs() > 12.0 {
                    continue;
                }
                if across < 0.0 {
                    west += u64::from(drop);
                } else {
                    east += u64::from(drop);
                }
            }
        }
        (west, east)
    }

    #[test]
    fn the_wind_side_of_a_range_gets_more_rain_and_deeper_valleys() {
        let m = 64;
        let mut erosion = Erosion::global(&ridge_range(m), 8192);
        erosion.set_rain(WEST_WIND);
        let rain = erosion.rain_of(0, 0, m, m);
        let row = m / 2 * m;
        let (mut wet, mut dry) = (0.0, 0.0);
        for k in 3..12 {
            let (west, east) = (rain[row + m / 2 - 1 - k], rain[row + m / 2 + k]);
            assert!(west > east, "{k} {west} {east}");
            (wet, dry) = (wet + west, dry + east);
        }
        assert!(wet > 1.5 * dry, "{wet} {dry}");

        // The two sides of the range erode about the same with no wind. The
        // wind takes much more from the erosion of the east side.
        let (west, east) = west_and_east_drops(Rain::default());
        assert!(west.abs_diff(east) * 4 < west + east, "{west} {east}");
        let (wet, dry) = west_and_east_drops(WEST_WIND);
        assert!(wet > dry, "{wet} {dry}");
        let loss = |rain: u64, even: u64| 1.0 - rain as f64 / even as f64;
        assert!(
            loss(dry, east) > 2.0 * loss(wet, west),
            "{wet} {west} {dry} {east}"
        );
    }

    #[test]
    fn no_rain_shadow_erodes_as_before() {
        let m = 64;
        let heights = ridge_range(m);
        let brush = ErodeBrush {
            center: cell_dir(m, 0, m / 2, m / 2),
            radius: 0.45,
            hardness: 0.5,
            flow: 1.0,
        };
        let mut before = Erosion::global(&heights, 8192);
        let mut after = Erosion::global(&heights, 8192);
        after.set_rain(Rain {
            shadow: 0.0,
            ..WEST_WIND
        });
        for _ in 0..5 {
            assert_eq!(
                before.step(&[brush], 0.5).drops(),
                after.step(&[brush], 0.5).drops()
            );
        }
        let window = Window::centered(cell_dir(m, 0, m / 2, m / 2), 8192, 32, 128);
        let heights = WindowHeights::from_fn(window, lumpy);
        let mut before = Erosion::window_alone(&heights);
        let mut after = Erosion::window_alone(&heights);
        after.set_rain(Rain {
            shadow: 0.0,
            ..WEST_WIND
        });
        assert_eq!(
            before.steps(&[brush], 1.0, 4).drops(),
            after.steps(&[brush], 1.0, 4).drops()
        );
    }

    #[test]
    fn the_rain_is_continuous_across_a_face_edge() {
        let m = 64;
        // A range from north to south that crosses the edge of face 0 and
        // face 4, with the wind from the west.
        let heights = CoarseHeights::from_fn(m, |d| {
            let (lon, lat) = crate::math::dir_to_lonlat(d);
            let meters = if lon < -30.0 {
                OCEAN
            } else {
                300.0 + 2500.0 * (1.0 - lon.abs() / 12.0).max(0.0) + 3.0 * lat
            };
            meters_to_level(meters)
        });
        let mut erosion = Erosion::global(&heights, 8192);
        erosion.set_rain(WEST_WIND);
        let rain = erosion.rain_of(0, 0, m, m);
        let at = |face: usize, x: usize, y: usize| f64::from(rain[(face * m + y) * m + x]);
        let (mut across, mut along) = (0.0f64, 0.0f64);
        for x in 0..m {
            // The last row of face 0 and the cell past it on face 4.
            let (face, px, py) = cell_past_edge(m, 0, x as i64, m as i64);
            let here = at(0, x, m - 1);
            across = across.max((here - at(face, px, py)).abs());
            along = along.max((here - at(0, x, m - 2)).abs());
            if x > 0 {
                along = along.max((here - at(0, x - 1, m - 1)).abs());
            }
        }
        assert!(along > 0.0);
        assert!(across < 1.5 * along, "{across} {along}");
    }

    /// Run with `--release --ignored --nocapture` for the time of the steps
    /// with and without the rain. The first step of a stroke computes the
    /// rain.
    #[test]
    #[ignore]
    fn manual_rain_step_time() {
        let heights = CoarseHeights::from_fn(FLOW_SIZE, lumpy);
        let brushes = brushes_on_all_sides();
        let window = Window::centered([1.0, 0.0, 0.0], 8192, 4, crate::flow::WINDOW_CELLS);
        let window = WindowHeights::from_fn(window, lumpy);
        for rain in [Rain::default(), WEST_WIND] {
            let mut erosion = Erosion::global(&heights, 8192);
            erosion.set_rain(rain);
            let start = std::time::Instant::now();
            erosion.step(&brushes, 1.0);
            let global = start.elapsed();
            let start = std::time::Instant::now();
            erosion.step(&brushes, 1.0);
            let second = start.elapsed();
            let mut erosion = Erosion::window_alone(&window);
            erosion.set_rain(rain);
            let brush = ErodeBrush {
                center: [1.0, 0.0, 0.0],
                radius: 0.01,
                ..brushes[0]
            };
            let start = std::time::Instant::now();
            erosion.step(&[brush], 1.0);
            let window_step = start.elapsed();
            let start = std::time::Instant::now();
            erosion.steps(&[brush], 1.0, 4);
            let near = start.elapsed();
            eprintln!(
                "shadow {}: global {global:?}, second {second:?}, window {window_step:?}, 4 near steps {near:?}",
                rain.shadow
            );
        }
    }
}
