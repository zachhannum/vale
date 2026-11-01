//! The globe workspace: the layers of the world on a globe that you rotate,
//! and the raster layer that you paint.

use std::collections::VecDeque;
use std::f64::consts::{FRAC_PI_2, PI};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use eframe::egui::{Pos2, Rect};
use eframe::egui_wgpu::{self, wgpu};
use vale_store::{
    BlendMode, Hillshade, ImageId, LayerId, LonLat, RasterDisplay, RasterId, Smoothing,
};
use vale_terrain::{
    Bands, ChannelMap, ChannelWindow, ErodeBrush, ErodeStep, FACES, Heightmap, Mode, Pyramid, Rain,
    RangeLine, RidgeLine, SEA_LEVEL, Stamp, TexelRect, Window, WindowHeights, max_level,
    meters_to_level, reduced_cap, reduced_face_size, smoothing_level,
};

pub mod backdrop;
pub mod brush;
pub mod edges;
pub mod erode;
pub mod export;
pub mod fit;
pub mod flat;
pub mod gpu;
pub mod image;
pub mod import;
pub mod line;
pub mod math;
pub mod nav;
pub mod polygon;
pub mod preview;
pub mod rivers;
pub mod snap;
pub mod stats;
pub mod stops;
pub mod stroke_test;
pub mod symbol;
pub mod thumbnail;
pub mod vector;
pub mod view;
pub mod zoom_test;

use crate::pen::Pen;
use backdrop::Canvas;
use brush::{Backlog, BrushSettings, STAMP_SPACING, Sample, Stroke, plan_texels, set_world};
use erode::{Eroder, Ground, Lift, brush_window, erode_window, near_side, range_window};
use export::ExportResult;
use flat::FlatView;
use gpu::{
    Capture, Captured, Changes, Draw, Event, Frame, GlobeCallback, Group, ImageKey, Job as GpuJob,
    LayerKey, LineOp, Link, MapKey, Mask, Op, Pass, TargetId, Uniforms, Uploads,
};
use import::{ImportResult, Job, Source, Step};
use line::LineSettings;
use math::{V3, angle};
use nav::Nav;
use preview::{Preview, band_uniforms, line_uniforms};
use rivers::{Rivers, choose_window};
use snap::{Highlight, Run, SnapSettings, Targets, Tracer};
use stats::Stats;
use stops::StopTargets;
use stroke_test::StrokeTest;
use view::{Camera, GlobeView};
use zoom_test::ZoomTest;

/// Texels on one edge of a cube face. The GPU texture holds six full faces.
pub const FACE_SIZE: usize = 1024;

/// The face size of the app in a window.
pub const WINDOW_FACE_SIZE: usize = 8192;

/// The elevation of a new height layer, in meters.
pub const START_ELEVATION: f64 = -2500.0;

/// The largest face size of the reduced copy of a raster layer at first.
pub const DISPLAY_CAP: u32 = 2048;
/// The smallest of the largest face sizes of a reduced copy.
const DISPLAY_CAP_MIN: u32 = 512;
/// The memory of the GPU for the reduced copies of all raster layers, in
/// bytes.
pub const DISPLAY_BUDGET: u64 = 256 << 20;

/// The key of a heightmap that the GPU does not hold.
const NO_MAP: MapKey = MapKey {
    layer: LayerKey::Solo,
    generation: u64::MAX,
    display: false,
    face_size: 0,
};

/// The rate of an erode step of one second, for a strength of one meter.
const ERODE_RATE: f64 = 0.01;

/// The limits of the time that one erode step stands for, in seconds. A step
/// of a slow worker stands for the time from the step before it, so the
/// ground goes down at the same speed.
const ERODE_SECONDS: std::ops::RangeInclusive<f64> = 1.0 / 60.0..=0.25;

/// The time that one erode step stands for in the sync mode, in seconds.
const ERODE_SYNC_SECONDS: f64 = 1.0 / 30.0;

/// The rate of the erode steps of one uplift stamp, for an erosion setting
/// of 1 and a brush radius of 16 cells.
const UPLIFT_ERODE_RATE: f64 = 12.0;

/// An uplift stroke makes one weather stamp for this number of its stamps.
const UPLIFT_WEATHER_EVERY: usize = 4;

/// A stroke of the line tool or the polygon tool.
pub struct LineStroke {
    pub screen: Vec<Pos2>,
    /// The samples on the unit sphere.
    pub dirs: Vec<V3>,
    /// The points in one radian at the start of the stroke.
    pub scale: f64,
    /// The pen left the world, and the stroke takes no more samples.
    off: bool,
    /// Snaps and traces the stroke while the pen is down.
    tracer: Option<Tracer>,
    /// The parts of the stroke after the pen comes up.
    pub runs: Vec<Run>,
}

/// A point that the point tool placed.
#[derive(Clone, Copy, Debug)]
pub struct PlacedPoint {
    /// The position on the unit sphere.
    pub dir: V3,
    /// The stored position of the vertex that the point snapped to.
    pub exact: Option<LonLat>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Navigate,
    Brush,
    Line,
    Polygon,
    Point,
    /// Measures a path on the world. It works with any layer selected.
    Measure,
}

/// The way that the world workspace shows the world.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum WorldView {
    #[default]
    Globe,
    /// The whole world as a flat map.
    Flat,
}

/// The pen or the mouse button that paints. The canvas sets it.
#[derive(Default)]
pub struct BrushInput {
    /// The touch of the pen that is down.
    pub pen: Option<u64>,
    /// The primary mouse button paints.
    pub mouse: bool,
    /// The last position of the pen.
    pub pos: Option<Pos2>,
    /// The pen or the button that is down picks the flatten level, and it
    /// does not paint.
    pub picking: bool,
}

#[derive(Clone, Copy)]
enum Input {
    Down,
    /// A sample, and the time of the frame that read it.
    Sample(Sample, Instant),
    Up,
}

/// The pen of an erode stroke. The stroke makes steps, and it makes no
/// stamps.
#[derive(Default)]
struct Eroding {
    /// The places of the pen after the last step.
    brushes: Vec<ErodeBrush>,
    /// The last place of the pen, or `None` off the world. A pen that holds
    /// still makes steps there.
    held: Option<ErodeBrush>,
    /// The time of the last request for a step.
    stepped: Option<Instant>,
    /// The stroke asked for its first step.
    begun: bool,
    /// The window of the brush. `None`: the stroke works on the cells of the
    /// view.
    window: Option<Window>,
}

impl Eroding {
    /// Adds the place of one pen sample. A sample near the place before it
    /// adds no place.
    fn sample(&mut self, brush: &BrushSettings, sample: Sample) {
        self.held = sample.dir.map(|center| ErodeBrush {
            center,
            radius: sample.radius,
            hardness: brush.hardness,
            flow: sample.flow * brush.flow,
        });
        let Some(new) = self.held else {
            return;
        };
        let far = |last: &ErodeBrush| angle(last.center, new.center) >= new.radius * STAMP_SPACING;
        if self.brushes.last().is_none_or(far) {
            self.brushes.push(new);
        }
    }
}

/// The erosion of an uplift stroke. The stamps of the stroke raise the
/// ground. Each step erodes the ground under the stamps after the step
/// before it.
#[derive(Default)]
struct Uplifting {
    /// The stamps after the last request for a step.
    stamps: Vec<Stamp>,
    /// The stamps of the step that is in work.
    asked: Vec<Stamp>,
    /// The window of the cells of the stroke, after its first request.
    window: Option<Window>,
}

/// The line of an ocean ridge stroke or of a range stroke.
enum BrushLine {
    Ridge(RidgeLine),
    Range(RangeLine),
}

impl BrushLine {
    fn len(&self) -> usize {
        match self {
            BrushLine::Ridge(line) => line.len(),
            BrushLine::Range(line) => line.len(),
        }
    }

    /// The plan for the points from `from` on, and the number of points
    /// that the plans cover after it.
    fn plan(&self, from: usize) -> (LineOp, usize) {
        match self {
            BrushLine::Ridge(line) => {
                let plan = line.plan(from);
                let end = plan.end;
                (LineOp::Ridge(Box::new(plan)), end)
            }
            BrushLine::Range(line) => {
                let plan = line.plan(from);
                let end = plan.end;
                (LineOp::Range(Box::new(plan)), end)
            }
        }
    }
}

/// The line of an ocean ridge stroke or of a range stroke. The stamps of
/// the stroke are the points of the line, and they do not go to the GPU.
struct Ridging {
    line: BrushLine,
    /// The number of points of the line that the plans cover.
    sent: usize,
    /// `Some`: the stroke is a range stroke with erosion.
    range: Option<Ranging>,
}

impl Ridging {
    fn extend(&mut self, stamps: &[Stamp]) {
        for stamp in stamps {
            match &mut self.line {
                BrushLine::Ridge(line) => line.push(stamp.center, stamp.flow),
                BrushLine::Range(line) => line.push(stamp.center, stamp.flow),
            }
        }
    }
}

/// The line of an ocean ridge stroke or of a range stroke with a brush of
/// this radius, in radians, on a world of `km` kilometers with a heightmap
/// of face size `n`. Other modes have no line.
fn line_of(brush: &BrushSettings, km: f64, seed: u32, n: usize, radius: f64) -> Option<Ridging> {
    let (line, range) = match brush.mode {
        Mode::OceanRidge => {
            let spec = brush.ridge_spec(radius, km, seed);
            (BrushLine::Ridge(RidgeLine::new(spec, n)), None)
        }
        Mode::Range => {
            let spec = brush.range_spec(radius, km, seed);
            let range = (brush.erosion > 0.0).then(Ranging::default);
            (BrushLine::Range(RangeLine::new(spec, n)), range)
        }
        _ => return None,
    };
    Some(Ridging {
        line,
        sent: 0,
        range,
    })
}

/// The erosion of a range stroke. The worker computes the steps while the
/// pen paints, and the steps go to the GPU after the last plan of the
/// line, because a plan puts back the ground from before the stroke.
#[derive(Default)]
struct Ranging {
    /// The distance along the line that the requests cover, in radians.
    lifted: f64,
    /// The steps that arrived. A step on the cells of the step before it
    /// adds its drops to that step.
    held: Vec<Arc<ErodeStep>>,
    /// The window of the cells of the stroke, after its first request.
    window: Option<Window>,
    /// The held steps went to the GPU.
    done: bool,
}

/// The stroke that the GPU works on.
struct Active {
    id: u64,
    stroke: Stroke,
    /// The texels of each face that the stamps can change.
    touched: [Option<TexelRect>; FACES],
    /// The time of the last sample.
    sampled: Option<Instant>,
    /// `Some`: the stroke is an erode stroke.
    erode: Option<Eroding>,
    /// `Some`: the stroke is an uplift stroke with erosion.
    uplift: Option<Uplifting>,
    /// `Some`: the stroke is an ocean ridge stroke or a range stroke.
    ridge: Option<Ridging>,
    /// The pen is up. Stamps of the stroke can still wait in the backlog.
    ended: bool,
    /// The number of rectangles that the GPU has not given back. `None`: the
    /// stroke did not ask for its texels yet.
    reading: Option<usize>,
}

fn union(a: Option<TexelRect>, b: TexelRect) -> TexelRect {
    match a {
        Some(a) => TexelRect {
            x0: a.x0.min(b.x0),
            y0: a.y0.min(b.y0),
            x1: a.x1.max(b.x1),
            y1: a.y1.max(b.y1),
        },
        None => b,
    }
}

/// One visible layer of the world, for the draw order of the globe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StackItem {
    /// A raster layer, how much it hides the layers below it, from 0 to 1,
    /// how it colors its values, how it smooths them by the scale, and
    /// whether it shows its rivers and the lines of its stops.
    Raster {
        id: RasterId,
        opacity: f32,
        display: RasterDisplay,
        smoothing: Smoothing,
        /// The hillshade. It is off for a layer that is not a height layer.
        hillshade: Hillshade,
        rivers: bool,
        lines: bool,
        /// How the layer mixes with the layers below it.
        blend: BlendMode,
    },
    /// An image layer, how much it hides the layers below it, from 0 to 1,
    /// on top of the alpha of each pixel, and how it mixes with them.
    Image {
        id: ImageId,
        opacity: f32,
        blend: BlendMode,
    },
    Vector(LayerId),
}

/// One thing that the canvas paints for the globe, from the bottom.
pub enum Paint {
    /// A pass of the GPU.
    Callback(GlobeCallback),
    /// The shapes of a vector layer.
    Vector(LayerId),
}

/// The reduced copy of a raster layer on the GPU. The globe draws it.
struct DisplayCopy {
    key: MapKey,
    /// The changed texels that wait for the copy.
    uploads: Uploads,
    /// The copy got each texel one time. The globe does not draw a copy
    /// before that.
    loaded: bool,
}

impl DisplayCopy {
    /// The number of texels of the heightmap along one side of a texel of
    /// the copy.
    fn factor(&self, map: &Heightmap) -> usize {
        let factor = map.face_size().div_ceil(self.key.face_size as usize);
        factor.max(1).next_power_of_two()
    }

    /// Adds the changes of the heightmap of the layer.
    fn add(&mut self, map: &Heightmap, changes: &Changes, budget: usize) {
        let factor = self.factor(map);
        if changes.reset {
            let side = self.key.face_size as usize;
            let all = TexelRect {
                x0: 0,
                y0: 0,
                x1: side,
                y1: side,
            };
            self.uploads.restart();
            for face in 0..FACES {
                self.uploads.add(face, all, factor, budget, true);
            }
        } else {
            for &(face, rect) in &changes.rects {
                self.add_rect(map, face, rect, budget);
            }
        }
    }

    /// Adds changed texels of the heightmap of the layer.
    fn add_rect(&mut self, map: &Heightmap, face: usize, rect: TexelRect, budget: usize) {
        let factor = self.factor(map);
        self.uploads
            .add(face, rect.reduced(factor), factor, budget, false);
    }
}

/// The rivers of a visible height layer that the brush does not paint.
struct LayerRivers {
    rivers: Rivers,
    /// The newest channel map of the layer.
    channels: Option<Arc<ChannelMap>>,
    /// The GPU has `channels`.
    sent: bool,
}

/// A raster layer of the world.
///
/// The brush layer has its whole heightmap in `Globe::map`. Each other
/// layer has its whole heightmap here, or only its reduced heightmap. The
/// project file then holds the tiles of the whole heightmap.
struct Layer {
    id: RasterId,
    height: bool,
    /// Texels on one edge of a face of the whole heightmap.
    face_size: usize,
    /// The whole heightmap. `None`: the brush paints the layer, or the
    /// project file holds the tiles.
    full: Option<Heightmap>,
    /// The heightmap at a face size of `DISPLAY_CAP` or less, for a layer
    /// with a larger face. The picture, the rivers, and the readout of a
    /// layer that the brush does not paint come from it. `None`: the whole
    /// heightmap is not larger, and it does that work.
    reduced: Option<Heightmap>,
    /// `Some`: the layer is visible.
    copy: Option<DisplayCopy>,
    /// The copy before `copy`. The globe draws it until `copy` has each
    /// texel.
    old_copy: Option<MapKey>,
    /// `Some`: the layer is a visible height layer that the brush does not
    /// paint.
    rivers: Option<LayerRivers>,
}

/// The number of texels of a heightmap along one side of a texel of its
/// reduced heightmap.
fn reduction(face_size: usize, reduced: &Heightmap) -> usize {
    let factor = face_size.div_ceil(reduced.face_size());
    factor.max(1).next_power_of_two()
}

/// The smooth levels of a copy of `map` on the GPU with a face of
/// `face_size` texels. The copy has the texels that `Uploads::send` sends.
fn copy_pyramid(map: &Heightmap, face_size: u32) -> Pyramid {
    let factor = map.face_size().div_ceil(face_size as usize).max(1);
    let factor = factor.next_power_of_two();
    if factor == 1 {
        return Pyramid::new(map);
    }
    let mut copy = Heightmap::new(face_size as usize, map.base());
    copy.set_undo_steps(false);
    let mut data = Vec::new();
    for (face, rect) in map.allocated_rects() {
        let rect = rect.reduced(factor);
        data.clear();
        map.read_reduced(face, rect, factor, &mut data);
        copy.store_rect(face, rect, &data);
    }
    Pyramid::new(&copy)
}

/// The reduced heightmap of a whole heightmap with a face larger than
/// `DISPLAY_CAP`.
pub fn reduce_map(full: &Heightmap) -> Heightmap {
    let size = reduced_face_size(full.face_size() as u32, DISPLAY_CAP) as usize;
    let mut reduced = Heightmap::new(size, full.base());
    reduced.set_undo_steps(false);
    reduced.bands = full.bands.clone();
    let factor = reduction(full.face_size(), &reduced);
    let mut data = Vec::new();
    for (face, rect) in full.allocated_rects() {
        let rect = rect.reduced(factor);
        data.clear();
        full.read_reduced(face, rect, factor, &mut data);
        reduced.store_rect(face, rect, &data);
    }
    reduced.clear_changes();
    reduced
}

/// The texels of a reduced heightmap under one tile of the whole heightmap,
/// with their rectangle. `pixels` has the tile row by row, with `tile` texels
/// in a row. `None`: each texel of the tile has the level `base`. The whole
/// heightmap has `face_size` texels along one side of a face, and a reduced
/// texel is the mean of `factor` by `factor` of them.
pub fn reduce_tile(
    pixels: Option<&[u16]>,
    base: u16,
    tile: usize,
    face_size: usize,
    factor: usize,
    column: usize,
    row: usize,
) -> Option<(TexelRect, Vec<u16>)> {
    let (x0, y0) = (column * tile, row * tile);
    if x0 >= face_size || y0 >= face_size {
        return None;
    }
    let (x1, y1) = ((x0 + tile).min(face_size), (y0 + tile).min(face_size));
    let rect = TexelRect { x0, y0, x1, y1 }.reduced(factor);
    let mut out = Vec::with_capacity((rect.x1 - rect.x0) * (rect.y1 - rect.y0));
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            let Some(pixels) = pixels else {
                out.push(base);
                continue;
            };
            // The mean of the texels of the face in one block.
            let (sx0, sx1) = (x * factor, ((x + 1) * factor).min(face_size));
            let (sy0, sy1) = (y * factor, ((y + 1) * factor).min(face_size));
            let mut sum = 0u64;
            for sy in sy0..sy1 {
                let at = (sy - y0) * tile;
                let texels = &pixels[at + sx0 - x0..at + sx1 - x0];
                sum += texels.iter().map(|level| u64::from(*level)).sum::<u64>();
            }
            let count = ((sx1 - sx0) * (sy1 - sy0)) as u64;
            out.push(((sum + count / 2) / count) as u16);
        }
    }
    Some((rect, out))
}

impl Layer {
    fn new(id: RasterId, height: bool, face_size: usize) -> Layer {
        Layer {
            id,
            height,
            face_size,
            full: None,
            reduced: None,
            copy: None,
            old_copy: None,
            rivers: None,
        }
    }

    /// The heightmap of the picture of the layer. `brush` is the heightmap
    /// that the brush paints.
    fn display<'a>(&'a self, brush: &'a Heightmap) -> &'a Heightmap {
        let own = self.reduced.as_ref().or(self.full.as_ref());
        own.unwrap_or(brush)
    }

    /// Takes the changes of the whole heightmap of the layer to its reduced
    /// heightmap, to its copy on the GPU, and to its rivers. `brush` is the
    /// heightmap that the brush paints, if that is the whole heightmap of
    /// this layer.
    fn changed(&mut self, brush: Option<&Heightmap>, changes: &Changes, budget: usize) {
        let Some(full) = brush.or(self.full.as_ref()) else {
            return;
        };
        let Some(reduced) = &mut self.reduced else {
            return Layer::display_changed(&mut self.copy, &mut self.rivers, full, changes, budget);
        };
        if reduced.bands != full.bands {
            reduced.bands = full.bands.clone();
        }
        if !changes.reset && changes.rects.is_empty() {
            return;
        }
        let mut display = Changes {
            reset: changes.reset,
            rects: Vec::new(),
        };
        if changes.reset {
            *reduced = reduce_map(full);
        } else {
            let factor = reduction(full.face_size(), reduced);
            let mut data = Vec::new();
            for &(face, rect) in &changes.rects {
                let rect = rect.reduced(factor);
                data.clear();
                full.read_reduced(face, rect, factor, &mut data);
                reduced.store_rect(face, rect, &data);
                display.rects.push((face, rect));
            }
        }
        Layer::display_changed(&mut self.copy, &mut self.rivers, reduced, &display, budget);
    }

    /// Takes the changes of the heightmap of the picture to the copy on the
    /// GPU and to the rivers.
    fn display_changed(
        copy: &mut Option<DisplayCopy>,
        rivers: &mut Option<LayerRivers>,
        map: &Heightmap,
        changes: &Changes,
        budget: usize,
    ) {
        if !changes.reset && changes.rects.is_empty() {
            return;
        }
        if let Some(copy) = copy {
            copy.add(map, changes, budget);
        }
        if let Some(rivers) = rivers {
            if changes.reset {
                rivers.rivers.rebuild(map);
            }
            for &(face, rect) in changes.rects.iter().filter(|_| !changes.reset) {
                rivers.rivers.update(map, face, rect);
            }
            rivers.rivers.request();
        }
    }

    /// Puts one tile of the whole heightmap from the project file into the
    /// reduced heightmap. `pixels` is `None` if the file has no such tile.
    fn restore_tile(
        &mut self,
        column: usize,
        row: usize,
        face: usize,
        pixels: Option<&[u16]>,
        budget: usize,
    ) {
        let Some(reduced) = &mut self.reduced else {
            return;
        };
        let tile = reduced.tile_size();
        let factor = reduction(self.face_size, reduced);
        let base = reduced.base();
        let block = reduce_tile(pixels, base, tile, self.face_size, factor, column, row);
        let Some((rect, data)) = block else {
            return;
        };
        reduced.store_rect(face, rect, &data);
        let changes = Changes {
            reset: false,
            rects: vec![(face, rect)],
        };
        Layer::display_changed(&mut self.copy, &mut self.rivers, reduced, &changes, budget);
    }
}

pub struct Globe {
    /// The heightmap that the brush paints. During a stroke, the GPU
    /// texture is ahead of it. The texels come back when the stroke ends.
    pub map: Heightmap,
    /// The raster layer of `map`. `None`: the brush paints no raster layer,
    /// and `map` is an empty heightmap that the project does not hold.
    raster: Option<RasterId>,
    /// The values of `map` are heights in meters, so it has a sea and
    /// rivers.
    height: bool,
    /// The raster layers of the world.
    layers: Vec<Layer>,
    /// The image layers of the world.
    images: Vec<image::ImageLayer>,
    /// The visible layers in draw order, from the bottom. `None`: the globe
    /// draws `map` as one layer.
    stack: Option<Vec<StackItem>>,
    /// The raster layer that the brush paints after the stroke or the
    /// import that runs.
    pending: Option<Option<RasterId>>,
    /// The count of the heightmaps that the GPU got.
    generations: u64,
    /// The full heightmap of `map` on the GPU. `None`: the GPU holds none.
    full: Option<MapKey>,
    /// The GPU got each texel of `map` one time after `full` changed.
    full_loaded: bool,
    /// The largest face size of a reduced copy.
    display_cap: u32,
    /// The memory of the GPU for the reduced copies, in bytes.
    pub display_budget: u64,
    /// The count of the sets of reduced copies before this one.
    epoch: u64,
    /// The app wants the brush to paint `map`.
    paint_wanted: bool,
    /// The GPU has the channel map of the layer that the brush paints, for
    /// the picture.
    channels_sent: bool,
    /// The length of the stroke test that starts after the brush has its
    /// raster layer.
    test_wanted: Option<f64>,
    /// The face size of the empty heightmap, and of a raster layer that the
    /// app makes with no choice of the user.
    new_face_size: usize,
    /// The largest face size that the device can show.
    max_face_size: usize,
    pub view: GlobeView,
    pub flat: FlatView,
    /// The view that the canvas shows.
    pub world_view: WorldView,
    pub nav: Nav,
    /// The canvas in screen points. Set by the canvas each frame.
    pub rect: Rect,
    /// The format of the render target. `None`: no wgpu renderer draws the UI.
    pub format: Option<wgpu::TextureFormat>,
    pub tool: Tool,
    /// The brush can paint `map`. It is false while the brush waits for
    /// another raster layer.
    pub paintable: bool,
    pub preview: Preview,
    pub brush: BrushSettings,
    pub line: LineSettings,
    /// The line tool can draw: the selected layer is a visible line layer.
    pub lineable: bool,
    /// The height layer that the terrain fit of the line tool follows: the
    /// first height layer that the globe holds.
    pub fit_layer: Option<RasterId>,
    /// The polygon tool can draw: the selected layer is a visible polygon
    /// layer.
    pub fillable: bool,
    /// The point tool can place: the selected layer is a visible point
    /// layer.
    pub pointable: bool,
    /// The selected vector layer that the line tool, the polygon tool, or
    /// the point tool edits.
    pub draw_layer: Option<LayerId>,
    /// The snap settings of the tools that edit vector layers.
    pub snap: SnapSettings,
    /// The snap targets of the visible vector layers.
    vectors: Option<Arc<Targets>>,
    /// The snap targets of the color stops of the raster layers that the
    /// globe draws, while the line tool or the polygon tool is the tool.
    stops: Vec<(StopKey, Arc<StopTargets>)>,
    /// The snap targets of the vector layers and of the color stops.
    targets: Option<Arc<Targets>>,
    /// The stroke of the line or polygon tool while the pen is down.
    line_stroke: Option<LineStroke>,
    /// The last stroke of the line or polygon tool, for the layer.
    line_done: Option<LineStroke>,
    /// The position of the pen of the point tool while it is down. It is
    /// `None` while the pen is off the world.
    point_press: Option<Option<V3>>,
    /// The last point that the point tool placed, for the layer.
    point_done: Option<PlacedPoint>,
    /// The press of the Measure tool that can be a tap.
    pub measure_press: crate::measure::TapPress,
    /// The last tap of the Measure tool: its screen position and the place
    /// under it.
    measure_tap: Option<(Pos2, V3)>,
    /// The radius of the world, in kilometers. Set by the canvas each frame.
    pub world_km: f64,
    /// The seed of the world. Set by the canvas each frame.
    pub world_seed: u32,
    /// The wind and the rain shadow of the world. Set by the canvas each
    /// frame.
    pub world_rain: Rain,
    pub input: BrushInput,
    pub pen: Pen,
    /// The next press on the globe picks the flatten level.
    pub pick_level: bool,
    pub stats: Stats,
    /// The brush debug panel is open.
    pub debug: bool,
    /// The cards of the iPad layout show the blurred canvas.
    pub blur: bool,
    /// The text of the last stroke test.
    pub test_report: Option<String>,
    /// The text of the last zoom test.
    pub zoom_report: Option<String>,
    /// The changed texels that wait for the full heightmap on the GPU.
    pub uploads: Uploads,
    link: Link,
    events: mpsc::Receiver<Event>,
    /// Pen input that waits for the stroke before it.
    inputs: VecDeque<Input>,
    stroke: Option<Active>,
    backlog: Backlog,
    strokes: u64,
    test: Option<StrokeTest>,
    zoom_test: Option<ZoomTest>,
    rivers: Rivers,
    eroder: Eroder,
    /// The newest channel map that went to the GPU.
    channels: Option<Arc<ChannelMap>>,
    /// The window of small rivers that the view asks for.
    chosen: Option<Window>,
    /// The newest window of small rivers that went to the GPU.
    window: Option<Arc<ChannelWindow>>,
    /// The import that runs.
    import: Option<Job>,
    /// The raster layer of the import.
    import_target: Option<RasterId>,
    /// The result of the last import, until the UI takes it.
    import_result: Option<ImportResult>,
    /// The export that runs.
    export: Option<export::Job>,
    /// The result of the last export, until the UI takes it.
    export_result: Option<ExportResult>,
    edits: u64,
    memory_undo: bool,
    /// The smooth levels on the CPU of the heightmaps on the GPU that the
    /// globe draws, made when `shown_field` asks for them. A change of a
    /// heightmap removes them.
    fields: Mutex<Vec<(MapKey, Arc<Pyramid>)>>,
}

/// What the snap targets of the color stops of a raster layer come from.
#[derive(Clone, Debug, PartialEq)]
struct StopKey {
    id: RasterId,
    key: MapKey,
    edits: u64,
    levels: Vec<u16>,
    smoothing: Smoothing,
}

impl Default for Globe {
    fn default() -> Globe {
        Globe::with_face_size(FACE_SIZE)
    }
}

impl Globe {
    pub fn with_face_size(face_size: usize) -> Globe {
        let (link, events) = Link::new();
        let map = Heightmap::new(face_size, meters_to_level(START_ELEVATION));
        let mut globe = Globe {
            rivers: Rivers::new(&map, false),
            eroder: Eroder::new(false),
            channels: None,
            chosen: None,
            window: None,
            map,
            raster: None,
            height: true,
            layers: Vec::new(),
            images: Vec::new(),
            stack: None,
            pending: None,
            generations: 0,
            full: None,
            full_loaded: false,
            display_cap: DISPLAY_CAP,
            display_budget: DISPLAY_BUDGET,
            epoch: 0,
            paint_wanted: true,
            line: LineSettings::default(),
            lineable: false,
            fit_layer: None,
            fillable: false,
            pointable: false,
            draw_layer: None,
            snap: SnapSettings::default(),
            vectors: None,
            stops: Vec::new(),
            targets: None,
            line_stroke: None,
            line_done: None,
            point_press: None,
            point_done: None,
            measure_press: Default::default(),
            measure_tap: None,
            channels_sent: false,
            test_wanted: None,
            new_face_size: face_size,
            max_face_size: usize::MAX,
            view: GlobeView::centered(15.0, 25.0),
            flat: FlatView::default(),
            world_view: WorldView::default(),
            nav: Nav::default(),
            rect: Rect::ZERO,
            format: None,
            tool: Tool::default(),
            paintable: true,
            preview: Preview::default(),
            brush: BrushSettings::default(),
            world_km: 6371.0,
            world_seed: 0,
            world_rain: Rain::default(),
            input: BrushInput::default(),
            pen: Pen::default(),
            pick_level: false,
            stats: Stats::new(face_size as u32),
            debug: false,
            blur: true,
            test_report: None,
            zoom_report: None,
            uploads: Uploads::default(),
            link,
            events,
            inputs: VecDeque::new(),
            stroke: None,
            backlog: Backlog::default(),
            strokes: 0,
            test: None,
            zoom_test: None,
            import: None,
            import_target: None,
            import_result: None,
            export: None,
            export_result: None,
            edits: 0,
            memory_undo: true,
            fields: Mutex::default(),
        };
        globe.begin_full();
        globe
    }

    /// Replaces the heightmap on the globe with an empty one of this face
    /// size. The band limits and the ramp stay. A raster layer that the app
    /// makes with no choice of the user also gets this face size. Call this
    /// while the globe shows no raster layer.
    pub fn set_face_size(&mut self, face_size: usize) {
        self.new_face_size = face_size;
        let mut map = self.empty_map();
        map.bands = std::mem::take(&mut self.map.bands);
        self.set_map(map);
    }

    /// The face size of `set_face_size`.
    pub fn new_face_size(&self) -> usize {
        self.new_face_size
    }

    /// The largest face size that the device can show.
    pub fn max_face_size(&self) -> usize {
        self.max_face_size
    }

    /// The heightmap of a globe with no raster layer.
    fn empty_map(&self) -> Heightmap {
        Heightmap::new(self.new_face_size, meters_to_level(START_ELEVATION))
    }

    /// Makes the globe that of a new world: no raster layer, an empty
    /// heightmap with no undo step, and the first views.
    pub fn clear(&mut self) {
        self.layers.clear();
        self.images.clear();
        self.raster = None;
        self.height = true;
        self.pending = None;
        self.set_map(self.empty_map());
        self.sync_copies();
        self.view = GlobeView::centered(15.0, 25.0);
        self.flat = FlatView::default();
    }

    /// The raster layer that the brush paints.
    pub fn raster(&self) -> Option<RasterId> {
        self.raster
    }

    /// True if the values of the heightmap that the brush paints are
    /// heights in meters. The sea, the rivers, and the brush modes that
    /// need them are for such a heightmap only.
    pub fn is_height(&self) -> bool {
        self.height
    }

    fn layer(&self, id: RasterId) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.id == id)
    }

    fn layer_mut(&mut self, id: RasterId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|layer| layer.id == id)
    }

    /// Adds the whole heightmap of a raster layer. The globe draws the layer
    /// when it is in the list of `set_stack`, and the brush paints it after
    /// `activate`. `height`: the unit of the layer is the meter. Returns
    /// `false` if the device cannot show the heightmap. The globe then does
    /// not hold it.
    pub fn add_raster(&mut self, id: RasterId, mut map: Heightmap, height: bool) -> bool {
        if map.face_size() > self.max_face_size {
            return false;
        }
        self.remove_raster(id);
        map.set_undo_steps(self.memory_undo);
        let mut layer = Layer::new(id, height, map.face_size());
        if map.face_size() > DISPLAY_CAP as usize {
            layer.reduced = Some(reduce_map(&map));
        }
        layer.full = Some(map);
        self.layers.push(layer);
        true
    }

    /// Adds a raster layer with its reduced heightmap. The project file
    /// holds the tiles of the whole heightmap, which has `face_size` texels
    /// along one side of a face. `give_full` gives the whole heightmap when
    /// the brush needs it. Returns `false` if the device cannot show the
    /// whole heightmap.
    pub fn add_reduced(
        &mut self,
        id: RasterId,
        mut reduced: Heightmap,
        face_size: usize,
        height: bool,
    ) -> bool {
        if face_size > self.max_face_size {
            return false;
        }
        self.remove_raster(id);
        reduced.set_undo_steps(false);
        let mut layer = Layer::new(id, height, face_size);
        layer.reduced = Some(reduced);
        self.layers.push(layer);
        true
    }

    /// Removes the heightmap of a raster layer. If the brush paints it, a
    /// stroke or an import that runs ends with no result, and the brush then
    /// paints no raster layer.
    pub fn remove_raster(&mut self, id: RasterId) {
        if self.pending == Some(Some(id)) {
            self.pending = None;
        }
        if self.raster == Some(id) {
            self.pending = None;
            self.switch(None);
        }
        self.layers.retain(|layer| layer.id != id);
        if let Some(stack) = &mut self.stack {
            stack.retain(|item| !matches!(item, StackItem::Raster { id: of, .. } if *of == id));
        }
        self.set_paintable(self.paint_wanted);
    }

    /// Makes a raster layer the one that the brush paints. `None`: the
    /// brush paints no raster layer. A stroke or an import that runs ends
    /// first, and the layer changes after it. If the globe has only the
    /// reduced heightmap of the layer, the layer changes after `give_full`.
    /// Returns `false` if the globe does not hold the layer.
    pub fn activate(&mut self, id: Option<RasterId>) -> bool {
        if id == self.raster {
            self.pending = None;
        } else if id.is_some_and(|id| !self.holds(id)) {
            return false;
        } else if self.switch_blocked() || self.needs_full(id) {
            self.pending = Some(id);
        } else {
            self.switch(id);
        }
        self.set_paintable(self.paint_wanted);
        true
    }

    /// Tells the globe if the app wants the brush to paint the raster layer
    /// of the last `activate`.
    pub fn set_paintable(&mut self, paintable: bool) {
        self.paint_wanted = paintable;
        self.paintable = paintable && self.pending.is_none();
    }

    /// True while a stroke or an import of `map` is not complete.
    fn switch_blocked(&self) -> bool {
        let import = self.import.is_some() && self.import_target == self.raster;
        self.stroke.is_some() || !self.inputs.is_empty() || self.test.is_some() || import
    }

    /// True if the globe does not have the whole heightmap of the layer.
    fn needs_full(&self, id: Option<RasterId>) -> bool {
        let layer = id.and_then(|id| self.layer(id));
        layer.is_some_and(|layer| layer.full.is_none())
    }

    /// The raster layer that waits for its whole heightmap from the project
    /// file, with its face size, before the brush can paint it. `None`
    /// while a stroke or an import runs.
    pub fn load_request(&self) -> Option<(RasterId, usize)> {
        let id = self.pending.flatten().filter(|_| !self.switch_blocked())?;
        let layer = self.layer(id).filter(|layer| layer.full.is_none())?;
        Some((id, layer.face_size))
    }

    /// Gives the whole heightmap of a raster layer from the project file.
    pub fn give_full(&mut self, id: RasterId, mut map: Heightmap) {
        let memory_undo = self.memory_undo;
        if let Some(layer) = self
            .layer_mut(id)
            .filter(|l| l.face_size == map.face_size())
        {
            map.set_undo_steps(memory_undo);
            layer.full = Some(map);
        }
        self.switch_if_idle();
    }

    /// Keeps the raster layer that the brush paints, and not the one that
    /// waits.
    pub fn cancel_activation(&mut self) {
        self.pending = None;
        self.set_paintable(self.paint_wanted);
    }

    /// True if a raster layer other than the brush layer has its whole
    /// heightmap and its reduced heightmap in memory.
    pub fn has_spare(&self) -> bool {
        let spare = |layer: &Layer| layer.full.is_some() && layer.reduced.is_some();
        self.layers.iter().any(spare)
    }

    /// Takes the whole heightmap from the main memory for each raster layer
    /// that has a reduced heightmap, that the brush does not paint, and
    /// that has no change that the project file does not hold. Call this
    /// only with a project file.
    pub fn release_saved(&mut self) {
        self.sync_maps();
        let waits = self.pending.flatten();
        for layer in &mut self.layers {
            let saved = layer.full.as_ref().is_some_and(|map| !map.has_changes());
            if saved && layer.reduced.is_some() && Some(layer.id) != waits {
                layer.full = None;
            }
        }
    }

    /// Makes the raster layer that waits the one that the brush paints,
    /// after the end of the stroke and of the import.
    fn switch_if_idle(&mut self) {
        let free = |id: &Option<RasterId>| !self.switch_blocked() && !self.needs_full(*id);
        if let Some(id) = self.pending.filter(free) {
            self.switch(id);
            self.set_paintable(self.paint_wanted);
        }
        if let Some(seconds) = self.test_wanted.filter(|_| self.pending.is_none()) {
            self.test_wanted = None;
            self.start_stroke_test(seconds);
        }
    }

    /// Changes the raster layer that the brush paints. A stroke that runs
    /// ends with no result.
    fn switch(&mut self, id: Option<RasterId>) {
        self.pending = None;
        let next = match id {
            Some(id) => {
                let layer = self.layer_mut(id);
                let Some((map, height)) = layer.and_then(|l| Some((l.full.take()?, l.height)))
                else {
                    return;
                };
                (map, height)
            }
            None => (self.empty_map(), true),
        };
        // The stroke has the undo entry of the old heightmap.
        self.map.cancel_stroke();
        self.sync_maps();
        let old = std::mem::replace(&mut self.map, next.0);
        let channels = self.channels.take();
        if let Some(layer) = self.raster.and_then(|id| self.layer_mut(id)) {
            layer.full = Some(old);
        }
        let last = self.raster;
        (self.raster, self.height) = (id, next.1);
        if self.import_target != id {
            self.import = None;
        }
        self.reset_state();
        self.begin_full();
        self.request_rivers();
        // The layer that the brush painted keeps its rivers.
        self.sync_rivers();
        let rivers = last.and_then(|id| self.layer_mut(id));
        if let Some(rivers) = rivers.and_then(|layer| layer.rivers.as_mut()) {
            rivers.channels = channels;
            rivers.sent = false;
        }
    }

    /// The ID of each raster layer that the globe holds.
    pub fn rasters(&self) -> Vec<RasterId> {
        self.layers.iter().map(|layer| layer.id).collect()
    }

    /// True if the globe holds the raster layer.
    /// True if the list of layers has the raster layer, so the globe draws
    /// it and a map frame can draw it.
    pub fn draws(&self, id: RasterId) -> bool {
        let stack = self.stack.as_deref().unwrap_or_default();
        let found =
            |item: &StackItem| matches!(item, StackItem::Raster { id: of, .. } if *of == id);
        stack.iter().any(found) && self.source(id).is_some()
    }

    pub fn holds(&self, id: RasterId) -> bool {
        self.layer(id).is_some()
    }

    /// The whole heightmap of a raster layer. `None`: the globe does not
    /// hold the layer, or the project file holds its tiles.
    pub fn raster_map(&self, id: RasterId) -> Option<&Heightmap> {
        if self.raster == Some(id) {
            return Some(&self.map);
        }
        self.layer(id).and_then(|layer| layer.full.as_ref())
    }

    /// The whole heightmap of a raster layer. A change goes to the GPU in
    /// the next frame.
    pub fn raster_map_mut(&mut self, id: RasterId) -> Option<&mut Heightmap> {
        if self.raster == Some(id) {
            return Some(&mut self.map);
        }
        self.layer_mut(id).and_then(|layer| layer.full.as_mut())
    }

    /// The heightmap of the picture of a raster layer: the whole heightmap
    /// of a layer with a face of `DISPLAY_CAP` or less, or else the reduced
    /// heightmap.
    pub fn display_map(&self, id: RasterId) -> Option<&Heightmap> {
        Some(self.layer(id)?.display(&self.map))
    }

    /// The reduced heightmap of each raster layer that has no whole
    /// heightmap in the main memory.
    pub fn reduced_maps(&self) -> impl Iterator<Item = (RasterId, &Heightmap)> {
        let alone = |layer: &Layer| layer.full.is_none() && Some(layer.id) != self.raster;
        let layers = self.layers.iter().filter(move |layer| alone(layer));
        layers.filter_map(|layer| Some((layer.id, layer.reduced.as_ref()?)))
    }

    /// The bands of a raster layer.
    pub fn bands_mut(&mut self, id: RasterId) -> Option<&mut Bands> {
        if self.raster == Some(id) {
            return Some(&mut self.map.bands);
        }
        let layer = self.layer_mut(id)?;
        let map = layer.full.as_mut().or(layer.reduced.as_mut())?;
        Some(&mut map.bands)
    }

    /// Replaces the reduced heightmap of a raster layer that has no whole
    /// heightmap in the main memory.
    pub fn set_reduced(&mut self, id: RasterId, mut reduced: Heightmap) {
        let budget = self.uploads.budget;
        let Some(layer) = self.layer_mut(id).filter(|layer| layer.reduced.is_some()) else {
            return;
        };
        reduced.set_undo_steps(false);
        let changes = Changes {
            reset: true,
            rects: Vec::new(),
        };
        Layer::display_changed(
            &mut layer.copy,
            &mut layer.rivers,
            &reduced,
            &changes,
            budget,
        );
        layer.reduced = Some(reduced);
    }

    /// Puts one tile of the whole heightmap of a raster layer into its
    /// reduced heightmap, after the project file got the tile back. `pixels`
    /// is `None` if the file has no such tile.
    pub fn restore_tile(
        &mut self,
        id: RasterId,
        face: usize,
        column: usize,
        row: usize,
        pixels: Option<&[u16]>,
    ) {
        let budget = self.uploads.budget;
        self.forget_fields();
        if let Some(layer) = self.layer_mut(id).filter(|_| face < FACES) {
            layer.restore_tile(column, row, face, pixels, budget);
        }
    }

    /// The heightmap of each raster layer that has its whole heightmap in
    /// the main memory, for a save.
    pub fn raster_maps(&mut self) -> Vec<(RasterId, &mut Heightmap)> {
        let shown = self.raster.map(|id| (id, &mut self.map));
        let layers = self.layers.iter_mut();
        let others = layers.filter_map(|layer| Some((layer.id, layer.full.as_mut()?)));
        shown.into_iter().chain(others).collect()
    }

    /// The heightmap of the picture and the opacity of each visible raster
    /// layer, from the bottom.
    pub fn picture(&self) -> Vec<(&Heightmap, f32)> {
        let Some(stack) = &self.stack else {
            return vec![(&self.map, 1.0)];
        };
        let layer = |item: &StackItem| match *item {
            StackItem::Raster { id, opacity, .. } => Some((self.display_map(id)?, opacity)),
            StackItem::Vector(_) | StackItem::Image { .. } => None,
        };
        stack.iter().filter_map(layer).collect()
    }

    /// Gives its own rivers to each visible height layer that the brush
    /// does not paint, and takes them from each other layer.
    fn sync_rivers(&mut self) {
        let sync = self.rivers.sync();
        for at in 0..self.layers.len() {
            let layer = &self.layers[at];
            let wanted =
                layer.height && self.shows_rivers(layer.id) && Some(layer.id) != self.raster;
            let wanted = wanted && self.format.is_some();
            if wanted == layer.rivers.is_some() {
                continue;
            }
            let rivers = wanted.then(|| {
                let mut rivers = Rivers::new(layer.display(&self.map), sync);
                rivers.request();
                LayerRivers {
                    rivers,
                    channels: None,
                    sent: false,
                }
            });
            self.layers[at].rivers = rivers;
        }
    }

    /// Sets the visible layers of the world in draw order, from the bottom.
    /// The globe draws each raster layer of the list with its opacity. A
    /// raster layer that is not in the list holds no memory of the GPU for
    /// the picture.
    pub fn set_stack(&mut self, stack: Vec<StackItem>) {
        let solo = self.stack.is_none();
        let rasters = |stack: &[StackItem]| -> Vec<RasterId> {
            let ids = stack.iter().filter_map(|item| match item {
                StackItem::Raster { id, .. } => Some(*id),
                StackItem::Vector(_) | StackItem::Image { .. } => None,
            });
            let mut ids: Vec<RasterId> = ids.collect();
            ids.sort();
            ids
        };
        let same = self
            .stack
            .as_ref()
            .is_some_and(|old| rasters(old) == rasters(&stack));
        self.stack = Some(stack);
        if solo && self.raster.is_none() {
            // The GPU holds no heightmap that no layer has.
            self.begin_full();
        }
        if !same {
            self.display_cap = self.first_cap();
        }
        self.sync_copies();
        self.sync_images();
        self.sync_rivers();
    }

    /// The visible layers of `set_stack`.
    pub fn stack(&self) -> Option<&[StackItem]> {
        self.stack.as_deref()
    }

    /// True if the list of `set_stack` has the raster layer.
    fn visible(&self, id: RasterId) -> bool {
        let stack = self.stack.iter().flatten();
        stack
            .into_iter()
            .any(|item| matches!(item, StackItem::Raster { id: of, .. } if *of == id))
    }

    /// The layer is in the list of visible layers, and its rivers are on.
    fn shows_rivers(&self, id: RasterId) -> bool {
        let stack = self.stack.iter().flatten();
        stack
            .into_iter()
            .any(|item| matches!(item, StackItem::Raster { id: of, rivers: true, .. } if *of == id))
    }

    /// The largest face size of a reduced copy by the estimate of the
    /// memory of the visible layers.
    fn first_cap(&self) -> u32 {
        let sizes = self.layers.iter().filter(|layer| self.visible(layer.id));
        let sizes: Vec<(u32, usize)> = sizes
            .map(|layer| layer.display(&self.map))
            .map(|map| (map.face_size() as u32, map.allocated_tiles()))
            .collect();
        reduced_cap(&sizes, self.display_budget)
    }

    /// Gives each visible raster layer a reduced copy with the face size of
    /// the cap, and takes the copy from each other raster layer. One call
    /// starts one new copy in the place of a copy of another size, and the
    /// globe draws the old copy until the new one has each texel.
    fn sync_copies(&mut self) {
        let cap = self.display_cap;
        let budget = self.uploads.budget;
        let mut changed = false;
        let mut replacing = self.layers.iter().any(|layer| layer.old_copy.is_some());
        for at in 0..self.layers.len() {
            let id = self.layers[at].id;
            let map = self.layers[at].display(&self.map);
            // With no renderer, no layer has a copy.
            let size = (self.format.is_some() && self.visible(id))
                .then(|| reduced_face_size(map.face_size() as u32, cap));
            let layer = &self.layers[at];
            let old = layer.copy.as_ref().map(|copy| copy.key.face_size);
            if old == size {
                continue;
            }
            // A loaded copy of another size stays until its new copy is
            // ready. One copy at a time has two sizes.
            let keep = layer
                .copy
                .as_ref()
                .filter(|copy| copy.loaded && size.is_some());
            if keep.is_some() && replacing {
                continue;
            }
            let keep = keep.map(|copy| copy.key);
            replacing |= keep.is_some();
            changed = true;
            self.generations += 1;
            let copy = size.map(|face_size| {
                let mut copy = DisplayCopy {
                    key: MapKey {
                        layer: LayerKey::Raster(id),
                        generation: self.generations,
                        display: true,
                        face_size,
                    },
                    uploads: Uploads::default(),
                    loaded: false,
                };
                let fresh = Changes {
                    reset: true,
                    rects: Vec::new(),
                };
                copy.add(map, &fresh, budget);
                copy
            });
            let layer = &mut self.layers[at];
            layer.copy = copy;
            layer.old_copy = keep.filter(|_| layer.copy.is_some());
        }
        if changed {
            self.epoch += 1;
        }
    }

    /// The face size of the reduced copy of a raster layer on the GPU.
    /// `None`: the layer is not visible.
    pub fn display_face_size(&self, id: RasterId) -> Option<u32> {
        let copy = self.layer(id)?.copy.as_ref()?;
        Some(copy.key.face_size)
    }

    /// True while a visible raster layer waits for the texels of its
    /// reduced copy.
    pub fn loading(&self) -> bool {
        let copies = self.layers.iter().filter_map(|layer| layer.copy.as_ref());
        let waits = copies.into_iter().any(|copy| !copy.loaded);
        let wrong = |layer: &Layer| {
            let size = reduced_face_size(
                layer.display(&self.map).face_size() as u32,
                self.display_cap,
            );
            layer
                .copy
                .as_ref()
                .is_some_and(|copy| copy.key.face_size != size)
        };
        let images = self.images_loading();
        self.format.is_some() && (waits || images || self.layers.iter().any(wrong))
    }

    /// The memory of the channel maps of the rivers of the layers on the GPU
    /// after the last frame, in bytes. The budget of the reduced copies
    /// does not count it.
    pub fn river_bytes(&self) -> u64 {
        self.link.copy_bytes().2
    }

    /// The memory of the heightmaps of the raster layers in the main
    /// memory, in bytes: the whole heightmaps and the reduced heightmaps.
    pub fn map_bytes(&self, id: RasterId) -> usize {
        let Some(layer) = self.layer(id) else {
            return 0;
        };
        let brush = (self.raster == Some(id)).then_some(&self.map);
        let maps = [brush, layer.full.as_ref(), layer.reduced.as_ref()];
        maps.into_iter()
            .flatten()
            .map(Heightmap::memory_bytes)
            .sum()
    }

    /// True while texels wait for the full heightmap on the GPU. With no
    /// renderer, no texel waits.
    fn uploading(&self) -> bool {
        self.format.is_some() && self.full.is_some() && !self.uploads.is_empty()
    }

    /// The memory of the reduced copies on the GPU after the last frame, in
    /// bytes.
    pub fn display_bytes(&self) -> u64 {
        self.link.copy_bytes().1
    }

    /// True if the globe draws the raster layer from the full heightmap that
    /// the brush paints, and not from its reduced copy.
    pub fn draws_full(&self, id: RasterId) -> bool {
        self.raster == Some(id) && self.full.is_some() && self.full_loaded
    }

    /// Makes each reduced copy and each copy of an image smaller if the
    /// copies hold more memory than the budget.
    fn check_budget(&mut self) {
        let (epoch, bytes, _) = self.link.copy_bytes();
        let bytes = bytes + self.image_bytes();
        let over = epoch == self.epoch && bytes > self.display_budget && !self.loading();
        if over && self.display_cap > DISPLAY_CAP_MIN {
            self.display_cap /= 2;
        }
        self.sync_copies();
        self.sync_images();
    }

    /// The key of the full heightmap on the GPU for the changes in the
    /// queue of the GPU.
    fn tag(&self) -> MapKey {
        self.full.unwrap_or(NO_MAP)
    }

    /// Replaces the heightmap that the brush paints. The raster layer of
    /// the heightmap stays. A stroke or an import that runs ends with no
    /// result.
    pub fn set_map(&mut self, map: Heightmap) {
        self.map = map;
        self.reset_map();
    }

    /// Makes the state of the globe that of a heightmap that it did not
    /// hold before. The GPU gets each texel again.
    fn reset_map(&mut self) {
        self.map.mark_reset();
        self.import = None;
        self.import_result = None;
        self.reset_state();
        self.begin_full();
    }

    /// Makes the state of the brush and of the rivers that of a new
    /// heightmap in `map`.
    fn reset_state(&mut self) {
        self.map.set_undo_steps(self.memory_undo);
        self.rivers = Rivers::new(&self.map, self.rivers.sync());
        self.eroder.cancel();
        self.channels = None;
        self.channels_sent = false;
        self.chosen = None;
        self.window = None;
        self.stats.face_size = self.map.face_size() as u32;
        self.inputs.clear();
        self.backlog.clear();
        self.stroke = None;
        if let Some(test) = self.test.take() {
            self.brush = test.saved;
        }
        self.brush.value_range = (self.map.bands.min(), self.map.bands.max());
    }

    /// Starts a new full heightmap on the GPU for `map`. The GPU gets each
    /// texel. The GPU holds a full heightmap only for a raster layer, or for
    /// a globe that draws `map` as one layer.
    fn begin_full(&mut self) {
        self.generations += 1;
        let layer = match (self.raster, &self.stack) {
            (Some(id), _) => Some(LayerKey::Raster(id)),
            (None, None) => Some(LayerKey::Solo),
            (None, Some(_)) => None,
        };
        self.full = layer.map(|layer| MapKey {
            layer,
            generation: self.generations,
            display: false,
            face_size: self.map.face_size() as u32,
        });
        self.full_loaded = false;
        self.uploads.restart();
        let budget = self.uploads.budget;
        for (face, rect) in self.map.allocated_rects() {
            self.uploads.add(face, rect, 1, budget, false);
        }
    }

    /// Connects the globe to the renderer of the UI. A face is not larger
    /// than the largest texture of the device.
    pub fn attach(&mut self, render_state: &egui_wgpu::RenderState) {
        self.format = Some(render_state.target_format);
        let info = render_state.adapter.get_info();
        self.stats.adapter = format!("{} ({:?})", info.name, info.backend);
        let limit = render_state.device.limits().max_texture_dimension_2d as usize;
        self.max_face_size = limit;
        if self.map.face_size() > limit {
            let id = self.raster.take();
            self.layers.retain(|layer| Some(layer.id) != id);
            self.height = true;
            self.pending = None;
            self.set_face_size(self.new_face_size.min(limit));
        }
        self.layers.retain(|layer| layer.face_size <= limit);
        self.new_face_size = self.new_face_size.min(limit);
        // The renderer has no texel and no river of a layer.
        self.sync_maps();
        for layer in &mut self.layers {
            (layer.copy, layer.old_copy, layer.rivers) = (None, None, None);
        }
        self.sync_copies();
        self.reset_images();
        self.sync_rivers();
        self.reset_state();
        self.begin_full();
        self.request_rivers();
    }

    /// The format of the blurred canvas behind a card. `None`: a card has a
    /// plain fill.
    pub fn backdrop(&self) -> Option<wgpu::TextureFormat> {
        self.format.filter(|_| self.blur)
    }

    /// True if the brush is the tool and it can paint the heightmap. If
    /// not, a pen and the mouse move the view.
    pub fn brushing(&self) -> bool {
        self.tool == Tool::Brush && self.paintable
    }

    /// True if the line tool is the tool and it can draw in the selected
    /// layer.
    pub fn lining(&self) -> bool {
        self.tool == Tool::Line && self.lineable
    }

    /// True if the polygon tool is the tool and it can draw in the selected
    /// layer.
    pub fn filling(&self) -> bool {
        self.tool == Tool::Polygon && self.fillable
    }

    /// True if the line tool or the polygon tool can draw now.
    pub fn drawing(&self) -> bool {
        self.lining() || self.filling()
    }

    /// True if the point tool is the tool and it can place a point in the
    /// selected layer.
    pub fn placing(&self) -> bool {
        self.tool == Tool::Point && self.pointable
    }

    /// True if a tool that edits a vector layer can edit now.
    pub fn editing(&self) -> bool {
        self.drawing() || self.placing()
    }

    /// True if the Measure tool is the tool.
    pub fn measuring(&self) -> bool {
        self.tool == Tool::Measure
    }

    /// True if the tool snaps, so the snap targets are needed.
    pub fn snapping(&self) -> bool {
        self.editing() || self.measuring()
    }

    /// Records a tap of the Measure tool at a screen position on the world.
    pub fn measure_tap(&mut self, pos: Pos2) {
        self.measure_tap = self.unproject(pos).map(|dir| (pos, dir));
    }

    /// The last tap of the Measure tool.
    pub fn take_measure_tap(&mut self) -> Option<(Pos2, V3)> {
        self.measure_tap.take()
    }

    /// The point of the Measure tool at `dir`, snapped to the targets.
    pub fn measure_point(&self, dir: V3) -> crate::measure::MeasurePoint {
        let snap = self
            .targets
            .as_ref()
            .and_then(|t| t.snap(dir, self.radius(), &self.snap, None));
        let lonlat = |dir| {
            let (lon, lat) = math::dir_to_lonlat(dir);
            [lon, lat]
        };
        let (pos, exact) = match snap {
            Some(snap) => match snap.exact {
                Some(exact) => (exact, true),
                None => (lonlat(snap.dir), false),
            },
            None => (lonlat(dir), false),
        };
        crate::measure::MeasurePoint { pos, exact }
    }

    /// Starts a tap of the point tool.
    pub fn point_down(&mut self) {
        self.point_press = Some(None);
    }

    /// Moves the pen of the point tool.
    pub fn point_sample(&mut self, dir: Option<V3>) {
        if let Some(press) = &mut self.point_press {
            *press = dir;
        }
    }

    /// Ends the tap of the point tool. The point is where the pen comes
    /// up, snapped to the targets. `take_point` gives it.
    pub fn point_up(&mut self) {
        let Some(Some(dir)) = self.point_press.take() else {
            return;
        };
        let scale = self.radius();
        let snap = self
            .targets
            .as_ref()
            .and_then(|t| t.snap(dir, scale, &self.snap, None));
        self.point_done = Some(match snap {
            Some(snap) => PlacedPoint {
                dir: snap.dir,
                exact: snap.exact,
            },
            None => PlacedPoint { dir, exact: None },
        });
    }

    /// True while the pen of the point tool is down.
    pub fn point_pressed(&self) -> bool {
        self.point_press.is_some()
    }

    /// The last point that the point tool placed.
    pub fn take_point(&mut self) -> Option<PlacedPoint> {
        self.point_done.take()
    }

    /// Sets the snap targets of the visible vector layers.
    pub fn set_targets(&mut self, targets: Option<Arc<Targets>>) {
        self.vectors = targets;
        self.join_targets();
    }

    /// Makes the snap targets of the color stops again where a raster layer
    /// changed. Each raster layer that the globe draws in the Bands display
    /// has targets while a tool that edits a vector layer is the tool.
    pub fn refresh_stops(&mut self) {
        let mut keys = Vec::new();
        let stack = self.stack.as_deref().unwrap_or_default();
        let drawn: Vec<(RasterId, f32, RasterDisplay, Smoothing)> = stack
            .iter()
            .filter_map(|item| match *item {
                StackItem::Raster {
                    id,
                    opacity,
                    display,
                    smoothing,
                    blend,
                    ..
                } => Some((id, hides(opacity, blend), display, smoothing)),
                StackItem::Vector(_) | StackItem::Image { .. } => None,
            })
            .filter(|&(id, ..)| self.source(id).is_some())
            .collect();
        // The layers below the top layer that hides them do not show.
        let start = drawn.iter().rposition(|l| l.1 >= 1.0).unwrap_or(0);
        for &(id, _, display, smoothing) in &drawn[start..] {
            let (Some(key), Some(map)) = (self.source(id), self.display_map(id)) else {
                continue;
            };
            if !self.snapping() || display != RasterDisplay::Bands {
                continue;
            }
            let bands = &map.bands;
            keys.push(StopKey {
                id,
                key,
                edits: self.edits,
                levels: bands.stops().iter().map(|s| bands.level(s.value)).collect(),
                smoothing,
            });
        }
        if keys.len() == self.stops.len() && keys.iter().zip(&self.stops).all(|(k, s)| *k == s.0) {
            return;
        }
        let mut old = std::mem::take(&mut self.stops);
        for key in keys {
            let built = match old.iter().position(|s| s.0 == key) {
                Some(i) => Some(old.swap_remove(i).1),
                None => {
                    let map = match self.draws_full(key.id) {
                        true => &self.map,
                        false => self.display_map(key.id).expect("a drawn layer"),
                    };
                    let levels = key.levels.iter().map(|&l| f64::from(l) / 65535.0);
                    let face_size = key.key.face_size;
                    let snapshot = map.snapshot();
                    let built =
                        StopTargets::new(key.id, snapshot, face_size, key.smoothing, levels);
                    built.map(Arc::new)
                }
            };
            if let Some(built) = built {
                self.stops.push((key, built));
            }
        }
        self.join_targets();
    }

    fn join_targets(&mut self) {
        let stops: Vec<_> = self.stops.iter().map(|s| s.1.clone()).collect();
        self.targets = match (&self.vectors, stops.is_empty()) {
            (None, true) => None,
            (Some(vectors), true) => Some(vectors.clone()),
            (vectors, false) => {
                let layers = vectors.as_ref().map(|v| v.layers().to_vec());
                Some(Arc::new(Targets::new(layers.unwrap_or_default(), stops)))
            }
        };
    }

    /// Starts a stroke of the line or polygon tool.
    pub fn line_down(&mut self) {
        let scale = self.radius();
        let targets = self.targets.clone().zip(self.draw_layer);
        let snap = &self.snap;
        let tracer = targets.map(|(t, layer)| Tracer::new(t, snap.clone(), scale, layer));
        self.line_stroke = Some(LineStroke {
            screen: Vec::new(),
            dirs: Vec::new(),
            scale,
            off: false,
            tracer,
            runs: Vec::new(),
        });
    }

    /// Adds one sample to the stroke of the line or polygon tool. The stroke ends at
    /// the first sample off the world.
    pub fn line_sample(&mut self, pos: Pos2, dir: Option<V3>) {
        let Some(stroke) = self.line_stroke.as_mut().filter(|s| !s.off) else {
            return;
        };
        match dir {
            Some(dir) => {
                stroke.screen.push(pos);
                stroke.dirs.push(dir);
                if let Some(tracer) = &mut stroke.tracer {
                    tracer.push(dir);
                }
            }
            None => stroke.off = !stroke.dirs.is_empty(),
        }
    }

    /// Ends the stroke of the line or polygon tool. `take_line` gives it.
    pub fn line_up(&mut self) {
        self.line_done = self.line_stroke.take().map(|mut stroke| {
            stroke.runs = match stroke.tracer.take() {
                Some(tracer) => tracer.finish(),
                None => vec![Run::Free(stroke.dirs.clone())],
            };
            stroke
        });
    }

    /// The target that the stroke of the line or polygon tool snaps to or traces now.
    /// Before the stroke, it is the point where a stroke from the pen or
    /// the mouse starts.
    pub fn snap_highlight(&self) -> Option<Highlight> {
        if let Some(stroke) = &self.line_stroke {
            return stroke.tracer.as_ref()?.highlight();
        }
        if self.measuring() {
            let dir = self.unproject(self.input.pos?)?;
            let snap = self
                .targets
                .as_ref()?
                .snap(dir, self.radius(), &self.snap, None)?;
            return Some(Highlight {
                span: Vec::new(),
                point: Some(snap.dir),
            });
        }
        if !self.editing() {
            return None;
        }
        let dir = self.unproject(self.input.pos?)?;
        let targets = self.targets.as_ref()?;
        let point = targets.start(self.draw_layer?, dir, self.radius(), &self.snap)?;
        Some(Highlight {
            span: Vec::new(),
            point: Some(point),
        })
    }

    /// The stroke of the line or polygon tool while the pen is down, in screen points.
    pub fn line_preview(&self) -> Option<&[Pos2]> {
        self.line_stroke.as_ref().map(|s| s.screen.as_slice())
    }

    /// The last stroke of the line or polygon tool that ended.
    pub fn take_line(&mut self) -> Option<LineStroke> {
        self.line_done.take()
    }

    pub fn pen_down(&mut self) {
        self.inputs.push_back(Input::Down);
    }

    /// Adds one sample to the stroke. `time` is the start of the frame that
    /// read the sample.
    pub fn pen_sample(&mut self, sample: Sample, time: Instant) {
        self.inputs.push_back(Input::Sample(sample, time));
    }

    pub fn pen_up(&mut self) {
        self.inputs.push_back(Input::Up);
    }

    /// The size of one radian on screen, in points. On the globe, this is
    /// the radius of the globe. On the flat map, it is true where the
    /// projection has its true scale.
    pub fn radius(&self) -> f64 {
        match self.world_view {
            WorldView::Globe => self.view.radius(self.rect),
            WorldView::Flat => self.flat.scale(self.rect),
        }
    }

    /// The world direction under a screen position, or `None` off the world.
    pub fn unproject(&self, pos: Pos2) -> Option<V3> {
        match self.world_view {
            WorldView::Globe => self.view.unproject(self.rect, pos),
            WorldView::Flat => self.flat.unproject(self.rect, pos),
        }
    }

    /// The screen position of a world direction, or `None` if it does not
    /// show.
    pub fn project(&self, dir: V3) -> Option<Pos2> {
        match self.world_view {
            WorldView::Globe => self.view.project(self.rect, dir),
            WorldView::Flat => self.flat.project(self.rect, dir),
        }
    }

    /// Moves the view, so the place at the screen position `from` goes to
    /// `to`.
    pub fn move_place(&mut self, from: Pos2, to: Pos2) {
        match self.world_view {
            WorldView::Globe => self.view.gesture(self.rect, from, to, 1.0, 0.0),
            WorldView::Flat => self.flat.gesture(self.rect, from, to, 1.0, 0.0),
        }
    }

    /// Sets the flatten level from the heightmap at a place. During a stroke
    /// the heightmap is behind the GPU, and the level does not change.
    pub fn pick(&mut self, dir: Option<V3>) {
        if let Some(dir) = dir.filter(|_| !self.busy()) {
            self.brush.flatten_level = Some(self.map.sample(dir));
            self.pick_level = false;
        }
    }

    /// The smoothing of a raster layer in the list of layers. A layer that
    /// is not in the list has the default smoothing.
    fn smoothing(&self, id: RasterId) -> Smoothing {
        let stack = self.stack.as_deref().unwrap_or_default();
        let found = stack.iter().find_map(|item| match *item {
            StackItem::Raster {
                id: of, smoothing, ..
            } if of == id => Some(smoothing),
            _ => None,
        });
        found.unwrap_or_default()
    }

    /// The level of the smooth field for a source of `face_size` texels at
    /// the zoom of the view. 0 is the full detail.
    fn level_of(&self, smoothing: Smoothing, face_size: u32) -> f64 {
        if !smoothing.on {
            return 0.0;
        }
        smoothing_level(f64::from(smoothing.feature_size), self.radius(), face_size)
    }

    /// The level of the smooth field that the globe shows for a raster
    /// layer at the zoom of the view. 0 is the full detail, and so is a
    /// layer that the globe does not draw.
    pub fn smoothing_level(&self, id: RasterId) -> f64 {
        let Some(key) = self.source(id) else {
            return 0.0;
        };
        self.level_of(self.smoothing(id), key.face_size)
    }

    /// The value that the globe shows for a raster layer at a direction,
    /// from 0 to 1 as a level of the heightmap over 65,535. It is the field
    /// of `smoothing_level`, from the same heightmap as the GPU. `None`:
    /// the globe does not draw the layer.
    pub fn shown_field(&self, id: RasterId, dir: V3) -> Option<f64> {
        let key = self.source(id)?;
        let map = match self.draws_full(id) {
            true => &self.map,
            false => self.display_map(id)?,
        };
        let level = self.level_of(self.smoothing(id), key.face_size);
        let mut fields = self.fields.lock().expect("no panic holds the lock");
        let found = fields
            .iter()
            .find(|(of, _)| *of == key)
            .map(|(_, p)| p.clone());
        let pyramid = found.unwrap_or_else(|| {
            let pyramid = Arc::new(copy_pyramid(map, key.face_size));
            fields.retain(|(of, _)| of.layer != key.layer);
            fields.push((key, pyramid.clone()));
            pyramid
        });
        Some(pyramid.field(dir, level))
    }

    fn forget_fields(&mut self) {
        self.stops.clear();
        self.join_targets();
        self.fields
            .get_mut()
            .expect("no panic holds the lock")
            .clear();
    }

    /// True while a stroke, a stroke test, or an import is not complete,
    /// texels wait for the full heightmap on the GPU, or the brush waits for
    /// another raster layer.
    pub fn busy(&self) -> bool {
        self.pending.is_some()
            || self.test_wanted.is_some()
            || self.stroke.is_some()
            || !self.inputs.is_empty()
            || self.test.is_some()
            || self.uploading()
            || self.import.is_some()
    }

    /// The count of the strokes and the imports that changed the heightmap.
    pub fn edits(&self) -> u64 {
        self.edits
    }

    /// With `false`, the heightmap keeps no undo and redo steps, because
    /// the project file keeps them.
    pub fn set_memory_undo(&mut self, on: bool) {
        if self.memory_undo != on {
            self.memory_undo = on;
            self.map.set_undo_steps(on);
            for map in self.layers.iter_mut().filter_map(|l| l.full.as_mut()) {
                map.set_undo_steps(on);
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.busy() && self.map.can_undo()
    }

    /// Puts back the heightmap from before the last stroke.
    pub fn undo(&mut self) -> bool {
        self.can_undo() && self.map.undo()
    }

    pub fn can_redo(&self) -> bool {
        !self.busy() && self.map.can_redo()
    }

    /// Puts back the last stroke that `undo` took away.
    pub fn redo(&mut self) -> bool {
        self.can_redo() && self.map.redo()
    }

    /// True until the rivers of the last change of the heightmap arrive.
    pub fn rivers_pending(&self) -> bool {
        let layers = self.layers.iter().filter_map(|layer| layer.rivers.as_ref());
        self.rivers.pending() || layers.into_iter().any(|rivers| rivers.rivers.pending())
    }

    /// With `sync`, a change of the heightmap waits for its rivers, and an
    /// erode stroke waits for each step. Without it, worker threads compute
    /// them.
    pub fn set_rivers_sync(&mut self, sync: bool) {
        self.rivers.set_sync(sync);
        let layers = self.layers.iter_mut();
        for rivers in layers.filter_map(|layer| layer.rivers.as_mut()) {
            rivers.rivers.set_sync(sync);
        }
        self.eroder.set_sync(sync);
    }

    /// True until each step that the erode stroke asked for arrives.
    pub fn erode_pending(&self) -> bool {
        self.eroder.pending()
    }

    /// The window of the brush of the erode stroke in work. `None`: no
    /// erode stroke is in work, or the stroke works on the cells of the
    /// view.
    pub fn erode_window(&self) -> Option<Window> {
        self.stroke.as_ref()?.erode.as_ref()?.window
    }

    /// The ground of an erode stroke in the view of this frame. With
    /// `brush`, the cells are those of that window, and the water of the
    /// view comes in. Without it, the cells are those of the view: the
    /// cells of the window of small rivers, or the cells of the global map
    /// in a far view.
    fn erode_ground(&self, brush: Option<Window>) -> Ground {
        let global = self.rivers.coarse().clone();
        let view = self.chosen.map(|view| WindowHeights::new(&self.map, view));
        match (brush, view) {
            (Some(brush), view) => Ground::Nested {
                heights: WindowHeights::new(&self.map, brush),
                view,
                global,
            },
            (None, Some(heights)) => Ground::Window { heights, global },
            (None, None) => Ground::Global {
                heights: global,
                face_size: self.map.face_size(),
            },
        }
    }

    /// The newest channel map of the rivers that went to the GPU.
    pub fn channels(&self) -> Option<&ChannelMap> {
        self.channels.as_deref()
    }

    /// The newest window of small rivers that went to the GPU. A near view
    /// has one. Its rivers come before the rivers of `channels` on the ground
    /// that it covers.
    pub fn window(&self) -> Option<&ChannelWindow> {
        self.window.as_deref()
    }

    /// The angle that the canvas covers on the world along its larger side,
    /// in radians.
    fn span(&self) -> f64 {
        let side = f64::from(self.rect.width().max(self.rect.height()));
        let radius = self.radius();
        match self.world_view {
            // The canvas shows one half of the globe at most.
            WorldView::Globe if side >= 2.0 * radius => PI,
            WorldView::Globe => 2.0 * (side / (2.0 * radius)).asin(),
            WorldView::Flat => side / radius,
        }
    }

    /// The window of small rivers for the view of this frame.
    fn view_window(&self) -> Option<Window> {
        if !self.height || self.rect.width() <= 0.0 || self.rect.height() <= 0.0 {
            return None;
        }
        let middle = self.unproject(self.rect.center())?;
        choose_window(middle, self.span(), self.map.face_size(), self.chosen)
    }

    /// Asks for the rivers of the heightmap as it is now. A heightmap with
    /// no heights in meters has no rivers.
    fn request_rivers(&mut self) {
        if !self.height {
            return;
        }
        self.rivers.request();
        if let Some(window) = self.chosen {
            self.rivers.request_window(&self.map, window);
        }
    }

    /// Takes the changes of the whole heightmap of each raster layer to its
    /// reduced heightmap. With a renderer, the changes also go to the queues
    /// of the GPU and to the rivers. `attach` gives a new renderer each
    /// texel.
    pub fn sync_maps(&mut self) {
        let gpu = self.format.is_some();
        let budget = self.uploads.budget;
        let changes = gpu::take_changes(&mut self.map);
        if changes.reset || !changes.rects.is_empty() {
            self.forget_fields();
        }
        if gpu {
            if changes.reset {
                self.uploads.restart();
                self.rivers.rebuild(&self.map);
            }
            for &(face, rect) in &changes.rects {
                self.uploads.add(face, rect, 1, budget, false);
                if !changes.reset {
                    self.rivers.update(&self.map, face, rect);
                }
            }
            if changes.reset || !changes.rects.is_empty() {
                self.request_rivers();
            }
        }
        let active = self.raster;
        for layer in &mut self.layers {
            if Some(layer.id) == active {
                layer.changed(Some(&self.map), &changes, budget);
            } else if let Some(map) = &mut layer.full {
                let changes = gpu::take_changes(map);
                if changes.reset || !changes.rects.is_empty() {
                    self.fields
                        .get_mut()
                        .expect("no panic holds the lock")
                        .clear();
                }
                layer.changed(None, &changes, budget);
            }
        }
    }

    /// Moves the changes of each CPU heightmap to the GPU: to the full
    /// heightmap that the brush paints, and to the reduced copy of each
    /// visible layer. One call sends the texels of one budget. Asks for the
    /// rivers of the changes. Asks for the window of the view if the view
    /// left the last one. Sends the rivers that arrived to the GPU.
    fn queue_changes(&mut self) {
        let tag = self.tag();
        let budget = self.uploads.budget;
        self.sync_maps();
        for layer in &mut self.layers {
            // The rivers of the layer that arrived.
            if let Some(rivers) = &mut layer.rivers
                && let Some(channels) = rivers.rivers.poll()
            {
                rivers.channels = Some(channels);
                rivers.sent = false;
            }
        }

        // The full heightmap comes first. While a copy waits for texels, the
        // full heightmap gets one half of the budget.
        let copies = self.layers.iter().filter_map(|layer| layer.copy.as_ref());
        let waiting = copies.into_iter().any(|copy| !copy.uploads.is_empty());
        let mut room = budget;
        let mut first = true;
        match self.full {
            Some(key) => {
                let part = if waiting { budget / 2 } else { budget };
                let sent = self.uploads.send(&self.map, key, &self.link, part, true);
                room = room.saturating_sub(sent);
                first = sent == 0;
                self.full_loaded |= self.uploads.is_empty();
            }
            None => self.uploads.clear(),
        }
        for layer in &mut self.layers {
            let own = layer.reduced.as_ref().or(layer.full.as_ref());
            let map = own.unwrap_or(&self.map);
            let Some(copy) = &mut layer.copy else {
                continue;
            };
            let sent = copy.uploads.send(map, copy.key, &self.link, room, first);
            room = room.saturating_sub(sent);
            first &= sent == 0;
            copy.loaded |= copy.uploads.is_empty();
            if copy.loaded {
                layer.old_copy = None;
            }
        }

        let window = self.view_window();
        if window != self.chosen {
            self.chosen = window;
            match window {
                Some(window) => self.rivers.request_window(&self.map, window),
                None => {
                    self.rivers.drop_window();
                    // The window on the GPU stays until the next one
                    // arrives. A far view has no next one.
                    if self.window.take().is_some() {
                        self.link.push(tag, Op::Window(None));
                    }
                }
            }
        }
        if let Some(channels) = self.rivers.poll() {
            self.link.push(tag, Op::Channels(channels.clone()));
            self.channels = Some(channels);
            self.channels_sent = false;
        }
        if let Some(window) = self.rivers.poll_window() {
            self.link.push(tag, Op::Window(Some(window.clone())));
            self.window = Some(window);
        }
    }

    /// Starts the import of an equirectangular image on a worker thread. The
    /// image replaces the heightmap. One import runs at a time.
    pub fn start_import(&mut self, path: PathBuf) {
        // The import is for the raster layer that the brush paints after
        // the stroke that runs.
        let target = self.pending.unwrap_or(self.raster);
        self.start_import_into(target, Source::Path(path));
    }

    /// Starts the import of an image into a raster layer. The faces wait
    /// until the brush paints that layer.
    pub fn start_import_into(&mut self, target: Option<RasterId>, source: Source) {
        if let Some(job) = &self.import {
            let name = match &source {
                Source::Path(path) => import::file_name(path),
                Source::Image { name, .. } => name.clone(),
            };
            let text = format!("Cannot import {name}: {} is still importing", job.name);
            self.import_result = Some(Err(text));
            return;
        }
        self.import_result = None;
        let layer = target.and_then(|id| self.layer(id));
        let face_size = layer.map_or(self.map.face_size(), |layer| layer.face_size);
        self.import = Some(Job::start(source, face_size));
        self.import_target = target;
    }

    pub fn importing(&self) -> bool {
        self.import.is_some()
    }

    /// The text that tells how far the import is, or `None` with no import.
    pub fn import_progress(&self) -> Option<String> {
        self.import.as_ref().map(Job::progress)
    }

    /// Waits for the end of the import. If a stroke is not complete, the call
    /// returns before the first face, and `advance` continues the import.
    pub fn wait_import(&mut self) {
        self.pump_import(true);
    }

    /// Takes the result of the last import.
    pub fn take_import_result(&mut self) -> Option<ImportResult> {
        self.import_result.take()
    }

    /// Moves the faces that the worker made into the heightmap. Without
    /// `block`, one call takes one face at most.
    fn pump_import(&mut self, block: bool) {
        loop {
            self.switch_if_idle();
            if self.import.is_some() && self.import_target != self.raster {
                return;
            }
            // A stroke that is open owns the undo entry of the heightmap, and
            // its texels come back from the GPU later. The first face waits
            // for the end of each stroke.
            let idle = self.stroke.is_none() && self.inputs.is_empty() && self.test.is_none();
            let Some(job) = &mut self.import else {
                return;
            };
            // Before the worker has the file, it sends no face.
            if job.info.is_some() && job.faces == 0 && !idle {
                return;
            }
            match job.next(block) {
                Step::Wait => return,
                Step::Read => {}
                Step::Face(face, levels) => {
                    if job.faces == 0 {
                        self.map.begin_stroke();
                    }
                    self.map.store_face(face, &levels);
                    job.faces += 1;
                    if job.faces == FACES {
                        self.edits += u64::from(self.map.end_stroke());
                        self.import_result = self.import.take().and_then(|job| job.info).map(Ok);
                    }
                    if !block {
                        return;
                    }
                }
                Step::Failed(text) => {
                    // The faces that arrived go back out of the heightmap.
                    if job.faces > 0 {
                        self.map.cancel_stroke();
                    }
                    self.import = None;
                    self.import_result = Some(Err(text));
                }
            }
        }
    }

    /// Starts the export of a heightmap to a GeoTIFF file on a worker
    /// thread. The file gets the heightmap of this moment, and painting
    /// continues. `raster` is the record of the raster layer in the project.
    /// With no record, the file gets the heightmap that the brush paints.
    /// One export runs at a time. Returns `false` with an error result if
    /// the globe is busy or an export runs.
    pub fn start_export(
        &mut self,
        path: PathBuf,
        world: &vale_store::World,
        raster: Option<&vale_store::Raster>,
    ) -> bool {
        self.start_export_of(path, world, raster, None)
    }

    /// The same as `start_export`. `loaded` is the whole heightmap of the
    /// layer from the project file, for a layer that has only its reduced
    /// heightmap here.
    pub fn start_export_of(
        &mut self,
        path: PathBuf,
        world: &vale_store::World,
        raster: Option<&vale_store::Raster>,
        loaded: Option<Heightmap>,
    ) -> bool {
        let name = import::file_name(&path);
        let refusal = match &self.export {
            Some(job) => Some(format!("{} is still exporting", job.name)),
            // The heightmap is behind the GPU during a stroke.
            None if self.busy() => Some("an edit is still in progress".to_string()),
            None => None,
        };
        if let Some(why) = refusal {
            self.export_result = Some(Err(format!("Cannot export {name}: {why}")));
            return false;
        }
        self.export_result = None;
        let map = raster.and_then(|raster| self.raster_map(raster.id));
        let map = match (map, loaded) {
            (Some(map), _) => map.snapshot(),
            (None, Some(loaded)) => loaded,
            (None, None) => self.map.snapshot(),
        };
        let job = export::Job::start(map, world.clone(), raster.cloned(), path);
        self.export = Some(job);
        true
    }

    pub fn exporting(&self) -> bool {
        self.export.is_some()
    }

    /// The text that tells how far the export is, or `None` with no export.
    pub fn export_progress(&self) -> Option<String> {
        self.export.as_ref().map(export::Job::progress)
    }

    /// Waits for the end of the export.
    pub fn wait_export(&mut self) {
        self.pump_export(true);
    }

    /// Takes the result of the last export.
    pub fn take_export_result(&mut self) -> Option<ExportResult> {
        self.export_result.take()
    }

    /// Gives the export that did not start its error text.
    pub fn fail_export(&mut self, text: String) {
        self.export_result = Some(Err(text));
    }

    /// Moves the result of the worker to `export_result`.
    fn pump_export(&mut self, block: bool) {
        let result = self.export.as_mut().and_then(|job| job.result(block));
        if result.is_some() {
            self.export = None;
            self.export_result = result;
        }
    }

    /// Starts the stroke test. A slow stroke is `seconds` long.
    pub fn start_stroke_test(&mut self, seconds: f64) {
        // The test paints the raster layer that waits for the brush.
        if self.pending.is_some() {
            self.test_wanted = Some(seconds);
            return;
        }
        // The first stroke waits for the texels that go to the GPU.
        let free = !self.switch_blocked() && self.import.is_none();
        if self.format.is_some() && free {
            self.test = Some(StrokeTest::new(self.brush, seconds));
            self.test_report = None;
        }
    }

    /// Starts the zoom test in the current view. It takes `seconds`.
    pub fn start_zoom_test(&mut self, seconds: f64) {
        self.zoom_test = Some(ZoomTest::new(self.zoom(), seconds));
        self.zoom_report = None;
    }

    /// True while the zoom test runs.
    pub fn zoom_testing(&self) -> bool {
        self.zoom_test.is_some()
    }

    /// The zoom of the current view.
    fn zoom(&self) -> f64 {
        match self.world_view {
            WorldView::Globe => self.view.zoom,
            WorldView::Flat => self.flat.zoom,
        }
    }

    fn set_zoom(&mut self, zoom: f64) {
        match self.world_view {
            WorldView::Globe => self.view.zoom = zoom.clamp(view::ZOOM_MIN, view::ZOOM_MAX),
            WorldView::Flat => self.flat.zoom = zoom.clamp(flat::ZOOM_MIN, flat::ZOOM_MAX),
        }
    }

    /// Sets the zoom of the frame that starts at `now`. Returns `true`
    /// while the zoom test runs.
    fn run_zoom_test(&mut self, now: Instant) -> bool {
        let Some(mut test) = self.zoom_test.take() else {
            return false;
        };
        match test.zoom(now) {
            Some(zoom) => {
                self.set_zoom(zoom);
                self.zoom_test = Some(test);
                true
            }
            None => {
                self.set_zoom(test.saved);
                self.zoom_report = Some(test.report());
                false
            }
        }
    }

    /// Runs the brush for one frame. `now` is the start of the frame. Returns
    /// `true` if the brush needs one more frame.
    pub fn advance(&mut self, now: Instant) -> bool {
        let zooming = self.run_zoom_test(now);
        if self.format.is_none() {
            self.sync_maps();
            self.inputs.clear();
            self.pump_import(false);
            self.pump_export(false);
            return zooming || self.import.is_some() || self.export.is_some();
        }
        // The count comes first. A result that arrives after this line makes
        // one more frame.
        let in_flight = self.link.busy.any();
        self.switch_if_idle();
        self.check_budget();
        // An import starts in the frame after the end of a stroke, so that
        // the stroke and the import are two edits.
        self.pump_import(false);
        self.pump_export(false);
        self.read_events();
        // An undo or a redo goes to the GPU before the stamps of the next
        // stroke.
        self.queue_changes();
        self.run_test(now, in_flight);
        self.read_inputs();
        self.erode(now);
        self.uplift(now);
        self.ridge(now);
        self.range();
        self.send_stamps(now);
        let busy = self.busy() || self.loading();
        zooming || in_flight || busy || self.rivers.pending() || self.export.is_some()
    }

    fn read_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Done {
                    stroke,
                    sample,
                    done,
                } => self.stats.delay(stroke, sample, done),
                Event::Passes { stroke, passes } => self.stats.passes(stroke, passes),
                Event::Texels {
                    stroke,
                    face,
                    rect,
                    data,
                } => {
                    let Some(active) = self.stroke.as_mut().filter(|a| a.id == stroke) else {
                        continue;
                    };
                    self.map.store_rect(face as usize, rect, &data);
                    self.rivers.update(&self.map, face as usize, rect);
                    // The reduced heightmap and the reduced copy of the
                    // layer get the stroke.
                    let budget = self.uploads.budget;
                    let stored = Changes {
                        reset: false,
                        rects: vec![(face as usize, rect)],
                    };
                    let layer = self.layers.iter_mut().find(|l| Some(l.id) == self.raster);
                    if let Some(layer) = layer {
                        layer.changed(Some(&self.map), &stored, budget);
                    }
                    let left = active.reading.get_or_insert(1);
                    *left -= 1;
                    if *left == 0 {
                        self.request_rivers();
                        self.edits += u64::from(self.map.end_stroke());
                        self.stroke = None;
                    }
                }
            }
        }
    }

    fn run_test(&mut self, now: Instant, in_flight: bool) {
        let Some(mut test) = self.test.take() else {
            return;
        };
        let idle = self.stroke.is_none() && self.inputs.is_empty();
        if !test.drawing() && idle {
            match test.next() {
                Some(part) => {
                    self.brush = test.brush(part);
                    self.inputs.push_back(Input::Down);
                    test.begin(now, part);
                }
                // The last delay must arrive before the report.
                None if in_flight => {}
                None => {
                    self.brush = test.saved;
                    self.test_report = Some(self.stats.report(stroke_test::PARTS.len()));
                    return;
                }
            }
        }
        let radius = self.brush.radius(self.radius());
        for dir in test.due(now) {
            let sample = Sample {
                dir: Some(dir),
                flow: brush::FIXED_FLOW,
                radius,
            };
            self.inputs.push_back(Input::Sample(sample, now));
        }
        if test.end_if_complete() {
            self.inputs.push_back(Input::Up);
        }
        self.test = Some(test);
    }

    fn read_inputs(&mut self) {
        while let Some(&input) = self.inputs.front() {
            match input {
                Input::Down => {
                    if let Some(active) = &mut self.stroke {
                        // The stroke before this one still waits for its texels.
                        active.ended = true;
                        break;
                    }
                    // A strip that waits has texels from before the stroke.
                    // It must reach the GPU before the first stamp.
                    if self.uploading() {
                        break;
                    }
                    // The import has the undo entry of the heightmap.
                    if self.import.as_ref().is_some_and(|job| job.faces > 0) {
                        break;
                    }
                    self.strokes += 1;
                    self.map.begin_stroke();
                    let points = self.brush.points(self.radius());
                    self.stats
                        .begin_stroke(self.strokes, self.brush.mode, points);
                    let erode = (self.brush.mode == Mode::Erode).then(Eroding::default);
                    let eroding = self.brush.mode == Mode::Uplift && self.brush.erosion > 0.0;
                    let uplift = eroding.then(Uplifting::default);
                    self.stroke = Some(Active {
                        id: self.strokes,
                        stroke: Stroke::default(),
                        touched: [None; FACES],
                        sampled: None,
                        erode,
                        uplift,
                        ridge: None,
                        ended: false,
                        reading: None,
                    });
                }
                Input::Sample(sample, time) => {
                    if let Some(active) = self.stroke.as_mut().filter(|a| !a.ended) {
                        // The first sample gives the radius of the line.
                        if active.sampled.is_none() {
                            let (km, seed, n) =
                                (self.world_km, self.world_seed, self.map.face_size());
                            active.ridge = line_of(&self.brush, km, seed, n, sample.radius);
                        }
                        active.sampled = Some(time);
                        if let Some(erode) = &mut active.erode {
                            erode.sample(&self.brush, sample);
                        } else {
                            let map = &self.map;
                            let level_at = |dir: V3| map.sample(dir);
                            let mut stamps = Vec::new();
                            active.stroke.add_sample(
                                &self.brush,
                                map.face_size(),
                                sample,
                                level_at,
                                &mut stamps,
                            );
                            if let Some(ridge) = &mut active.ridge {
                                ridge.extend(&stamps);
                                stamps.clear();
                            }
                            for stamp in &mut stamps {
                                set_world(
                                    stamp,
                                    &self.brush,
                                    map.face_size(),
                                    self.world_km,
                                    self.world_seed,
                                );
                                // The relief of the noise mode needs heights.
                                if !self.height {
                                    stamp.relief = 0.0;
                                }
                                self.backlog.push(map.stamp_plan(stamp), time);
                            }
                            if let Some(uplift) = &mut active.uplift {
                                uplift.stamps.extend(stamps);
                            }
                        }
                    }
                }
                Input::Up => {
                    if let Some(active) = self.stroke.as_mut().filter(|a| !a.ended) {
                        active.ended = true;
                        let map = &self.map;
                        let mut stamps = Vec::new();
                        active
                            .stroke
                            .finish(&self.brush, map.face_size(), &mut stamps);
                        if let Some(ridge) = &mut active.ridge {
                            ridge.extend(&stamps);
                            stamps.clear();
                        }
                        for stamp in &mut stamps {
                            set_world(
                                stamp,
                                &self.brush,
                                map.face_size(),
                                self.world_km,
                                self.world_seed,
                            );
                            if !self.height {
                                stamp.relief = 0.0;
                            }
                            let time = active.sampled.unwrap_or_else(Instant::now);
                            self.backlog.push(map.stamp_plan(stamp), time);
                        }
                        if let Some(uplift) = &mut active.uplift {
                            uplift.stamps.extend(stamps);
                        }
                    }
                }
            }
            self.inputs.pop_front();
        }
    }

    /// Sends the erode step that arrived to the GPU. Then asks for the next
    /// step, with the places of the pen after the last one. A pen that holds
    /// still makes a step at its place. One step is in work at a time.
    ///
    /// The first request sets the cells of the stroke. If the stroke has the
    /// window of the brush, that window follows the pen.
    fn erode(&mut self, now: Instant) {
        let tag = self.tag();
        let Some(active) = &mut self.stroke else {
            return;
        };
        let Some(erode) = &mut active.erode else {
            return;
        };
        if let Some(step) = self.eroder.poll().filter(|step| !step.is_empty()) {
            for (touched, rect) in active.touched.iter_mut().zip(step.rects()) {
                if let Some(rect) = rect {
                    *touched = Some(union(*touched, rect));
                }
            }
            let stroke = active.id;
            self.link.push(tag, Op::Erode { stroke, step });
        }
        if self.eroder.pending() {
            return;
        }
        let mut brushes = std::mem::take(&mut erode.brushes);
        if brushes.is_empty() && !active.ended {
            brushes.extend(erode.held);
        }
        if brushes.is_empty() {
            return;
        }
        let seconds = match erode.stepped {
            Some(last) if !self.rivers.sync() => {
                let seconds = now.saturating_duration_since(last).as_secs_f64();
                seconds.clamp(*ERODE_SECONDS.start(), *ERODE_SECONDS.end())
            }
            _ => ERODE_SYNC_SECONDS,
        };
        erode.stepped = Some(now);
        let (first, last) = (brushes[0], brushes[brushes.len() - 1]);
        let face_size = self.map.face_size();
        let (begun, window) = (erode.begun, erode.window);
        let mut next = window;
        if !begun {
            next = erode_window(self.chosen, first.center, first.radius, face_size);
            self.eroder.begin(self.erode_ground(next));
        } else if let Some(window) = window
            && near_side(&window, last.center, last.radius)
        {
            let moved = brush_window(last.center, last.radius, face_size);
            if moved != window {
                let heights = WindowHeights::new(&self.map, moved);
                self.eroder.move_to(Ground::Follow(heights));
                next = Some(moved);
            }
        }
        if let Some(erode) = self.stroke.as_mut().and_then(|a| a.erode.as_mut()) {
            (erode.begun, erode.window) = (true, next);
        }
        let rate = self.brush.strength_m * ERODE_RATE * seconds;
        self.eroder.set_rain(self.world_rain);
        self.eroder
            .request(brushes, rate, Lift::None, self.brush.limits());
    }

    /// Sends the erode step of an uplift stroke that arrived to the GPU, and
    /// then weather stamps on the ground of that step. Then asks for the
    /// next step, with the stamps after the last one. One step is in work at
    /// a time.
    ///
    /// The rate of a step grows with the number of its stamps. Thus the
    /// erosion follows the uplift, and not the speed of the worker.
    fn uplift(&mut self, now: Instant) {
        let tag = self.tag();
        let Some(active) = &mut self.stroke else {
            return;
        };
        let Some(uplift) = &mut active.uplift else {
            return;
        };
        let erosion = self.brush.erosion.clamp(0.0, 1.0);
        if let Some(step) = self.eroder.poll() {
            if !step.is_empty() {
                for (touched, rect) in active.touched.iter_mut().zip(step.rects()) {
                    if let Some(rect) = rect {
                        *touched = Some(union(*touched, rect));
                    }
                }
                let stroke = active.id;
                self.link.push(tag, Op::Erode { stroke, step });
            }
            let stamps = std::mem::take(&mut uplift.asked);
            for stamp in stamps.iter().step_by(UPLIFT_WEATHER_EVERY) {
                let stamp = Stamp {
                    mode: Mode::Weather,
                    flow: stamp.flow * erosion,
                    ..*stamp
                };
                self.backlog.push(self.map.stamp_plan(&stamp), now);
            }
        }
        if self.eroder.pending() || uplift.stamps.is_empty() {
            return;
        }
        let stamps = std::mem::take(&mut uplift.stamps);
        // The cells follow the size of the brush, and the window of the
        // cells follows the pen.
        let (first, last) = (stamps[0], stamps[stamps.len() - 1]);
        let face_size = self.map.face_size();
        match uplift.window {
            None => {
                let window = brush_window(first.center, first.radius, face_size);
                let heights = WindowHeights::new(&self.map, window);
                self.eroder.begin(Ground::Alone(heights));
                uplift.window = Some(window);
            }
            Some(window) if near_side(&window, last.center, last.radius) => {
                let next = brush_window(last.center, last.radius, face_size);
                if next != window {
                    let heights = WindowHeights::new(&self.map, next);
                    self.eroder.move_to(Ground::Alone(heights));
                    uplift.window = Some(next);
                }
            }
            Some(_) => {}
        }
        let brushes = stamps.iter().map(|stamp| ErodeBrush {
            center: stamp.center,
            radius: stamp.radius,
            hardness: stamp.hardness,
            flow: stamp.flow,
        });
        // A slope with no valley erodes less with smaller cells, so the
        // rate grows with the number of cells in the radius.
        let cells = uplift.window.map_or(1.0, |window| {
            let cell = FRAC_PI_2 * window.cell as f64 / window.face_size as f64;
            first.radius / cell
        });
        let rate = erosion * UPLIFT_ERODE_RATE * (cells / 16.0).sqrt() * stamps.len() as f64;
        let limits = self.brush.limits();
        self.eroder.set_rain(self.world_rain);
        let lift = Lift::Raise(stamps[0].strength);
        self.eroder.request(brushes.collect(), rate, lift, limits);
        uplift.asked = stamps;
    }

    /// Sends the ocean ridge or the range of the new points of the line to
    /// the GPU. Each plan goes with the ground from before the stroke, which
    /// the heightmap holds until the texels of the stroke come back.
    fn ridge(&mut self, now: Instant) {
        let tag = self.tag();
        let Some(active) = &mut self.stroke else {
            return;
        };
        let Some(ridge) = &mut active.ridge else {
            return;
        };
        let time = active.sampled.unwrap_or(now);
        while ridge.sent < ridge.line.len() {
            let (plan, end) = ridge.line.plan(ridge.sent);
            ridge.sent = end;
            let mut ground = Vec::new();
            for (face, rect) in plan.rects().iter().enumerate() {
                let Some(rect) = *rect else { continue };
                active.touched[face] = Some(union(active.touched[face], rect));
                let mut data = Vec::new();
                self.map.read_rect(face, rect, &mut data);
                ground.push((face as u32, rect, data));
            }
            if ground.is_empty() {
                continue;
            }
            let texels = ground.iter().map(|(_, _, data)| data.len() as u64).sum();
            self.stats.stamps(active.id, 1, texels);
            let stroke = active.id;
            let op = Op::Line {
                plan,
                ground,
                stroke,
                time,
            };
            self.link.push(tag, op);
        }
    }

    /// Holds the erode step of a range stroke that arrived. Then asks for
    /// the steps of the part of the line that new points no longer change.
    /// After the last plan and the last step, sends the held steps to the
    /// GPU. One request is in work at a time.
    fn range(&mut self) {
        let tag = self.tag();
        let Some(active) = &mut self.stroke else {
            return;
        };
        let Some(Ridging {
            line: BrushLine::Range(line),
            sent,
            range: Some(range),
        }) = &mut active.ridge
        else {
            return;
        };
        if range.done {
            return;
        }
        if let Some(step) = self.eroder.poll().filter(|step| !step.is_empty()) {
            for (touched, rect) in active.touched.iter_mut().zip(step.rects()) {
                if let Some(rect) = rect {
                    *touched = Some(union(*touched, rect));
                }
            }
            let last = range.held.last_mut().and_then(Arc::get_mut);
            if !last.is_some_and(|last| last.merge(&step)) {
                range.held.push(step);
            }
        }
        if self.eroder.pending() {
            return;
        }
        let all = active.ended && *sent == line.len();
        let settled = if all { line.length() } else { line.settled() };
        let spec = line.spec();
        if settled - range.lifted >= spec.radius || (all && settled > range.lifted) {
            let u = range.lifted..settled;
            let brushes = line.brushes(u.clone());
            let Some(center) = brushes.get(brushes.len() / 2).map(|b| b.center) else {
                range.lifted = settled;
                return;
            };
            let face_size = self.map.face_size();
            match range.window {
                None => {
                    let window = range_window(center, &spec, face_size);
                    let heights = WindowHeights::new(&self.map, window);
                    self.eroder.begin(Ground::Alone(heights));
                    range.window = Some(window);
                }
                Some(window) if near_side(&window, center, spec.radius) => {
                    let next = range_window(center, &spec, face_size);
                    if next != window {
                        let heights = WindowHeights::new(&self.map, next);
                        self.eroder.move_to(Ground::Alone(heights));
                        range.window = Some(next);
                    }
                }
                Some(_) => {}
            }
            self.eroder.set_rain(self.world_rain);
            let erosion = self.brush.erosion.clamp(0.0, 1.0);
            let lift = Lift::Range(Arc::new(line.clone()), u);
            self.eroder.request(Vec::new(), erosion, lift, spec.limits);
            range.lifted = settled;
        } else if all {
            for step in range.held.drain(..) {
                let stroke = active.id;
                self.link.push(tag, Op::Erode { stroke, step });
            }
            range.done = true;
        }
    }

    /// Sends the stamps of this frame to the GPU. After the last stamp of a
    /// stroke and the last step of an erode stroke or of an uplift stroke, asks for the texels that
    /// the stroke changed.
    fn send_stamps(&mut self, now: Instant) {
        let tag = self.tag();
        let Some(active) = &mut self.stroke else {
            return;
        };
        if !active.ended || !self.backlog.is_empty() {
            self.stats.frame(active.id, now);
        }
        let frame = self.backlog.take_frame();
        if !frame.is_empty() {
            let texels = frame.iter().map(|(plan, _)| plan_texels(plan)).sum();
            self.stats.stamps(active.id, frame.len(), texels);
        }
        for (plan, time) in frame {
            for (touched, rect) in active.touched.iter_mut().zip(plan.rects) {
                if let Some(rect) = rect {
                    *touched = Some(union(*touched, rect));
                }
            }
            let (plan, stroke) = (Box::new(plan), active.id);
            self.link.push(tag, Op::Stamp { plan, stroke, time });
        }
        self.stats.backlog = self.backlog.len();
        if !active.ended || !self.backlog.is_empty() || active.reading.is_some() {
            return;
        }
        // The readback follows the last step in the queue.
        if let Some(erode) = &active.erode
            && (!erode.brushes.is_empty() || self.eroder.pending())
        {
            return;
        }
        if let Some(uplift) = &active.uplift
            && (!uplift.stamps.is_empty() || self.eroder.pending())
        {
            return;
        }
        // The readback of a range stroke follows its held steps, so one undo
        // step has the whole stroke.
        if let Some(ridge) = &active.ridge
            && (ridge.sent < ridge.line.len() || ridge.range.as_ref().is_some_and(|r| !r.done))
        {
            return;
        }
        let rects = active.touched.iter().enumerate();
        let rects: Vec<(usize, TexelRect)> = rects.filter_map(|(f, r)| Some((f, (*r)?))).collect();
        if rects.is_empty() {
            self.map.end_stroke();
            self.stroke = None;
            return;
        }
        active.reading = Some(rects.len());
        for (face, rect) in rects {
            let (stroke, face) = (active.id, face as u32);
            self.link.push(tag, Op::Readback { stroke, face, rect });
        }
    }

    /// The uniforms of one pass. `layer` is the raster layer of a layer
    /// pass. `small`: the pass draws into the small target of the blurred
    /// copy.
    fn uniforms(
        &self,
        canvas: Canvas,
        layer: Option<&LayerDraw>,
        graticule: bool,
        small: bool,
        frame: Option<&FramePlace>,
    ) -> Uniforms {
        let row = |r: V3| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0];
        let center = self.rect.center();
        let radius = self.radius() as f32;
        let (zoom, flat_center) = match (frame, self.world_view) {
            (Some(frame), _) => (1.0, frame.map_center),
            (None, WorldView::Globe) => (self.view.zoom, [0.0; 2]),
            (None, WorldView::Flat) => {
                let center = self.flat.center;
                (self.flat.zoom, [center.x as f32, center.y as f32])
            }
        };
        let pixels_per_point = canvas.pixels_per_point;
        let scale = point_scale(pixels_per_point, frame, small);
        // The blurred copy has no rivers.
        let rivers = layer.and_then(|layer| layer.rivers).filter(|_| !small);
        let sea = layer.map_or(0.0, |layer| {
            f32::from(layer.bands.level(SEA_LEVEL)) / 65535.0
        });
        let window = self.window.as_deref();
        let window = window.filter(|_| rivers.is_some() && layer.is_some_and(|l| l.window));
        let (window, cell) = match window.map(ChannelWindow::window) {
            Some(w) => ([1.0, w.face as f32, w.x0 as f32, w.y0 as f32], w.cell),
            None => ([0.0; 4], 1),
        };
        let window_size = (2 * vale_terrain::WINDOW_CELLS) as f32;
        let graticule_degrees: f64 = if !graticule {
            0.0
        } else if zoom < 3.0 {
            15.0
        } else if zoom < 12.0 {
            5.0
        } else {
            1.0
        };
        let (bands, band_count) = match layer {
            Some(layer) => band_uniforms(layer.bands),
            None => ([[[0.0; 4]; 3]; vale_terrain::MAX_BANDS], 0),
        };
        let lines = match layer {
            Some(layer) if layer.lines => line_uniforms(layer.bands, scale),
            _ => [[[0.0; 4]; 2]; vale_terrain::MAX_BANDS + 1],
        };
        let color = |c: eframe::egui::Color32| {
            let [r, g, b] = [c.r(), c.g(), c.b()].map(|c| f32::from(c) / 255.0);
            [r, g, b, 1.0]
        };
        Uniforms {
            rot: self.view.rot.0.map(row),
            globe: match frame {
                Some(frame) => [
                    frame.center[0],
                    frame.center[1],
                    frame.units_to_pixels,
                    pixels_per_point,
                ],
                None => [
                    center.x * scale,
                    center.y * scale,
                    radius * scale,
                    pixels_per_point,
                ],
            },
            params: [
                layer.map_or(1.0, |layer| layer.key.face_size as f32),
                graticule_degrees.to_radians() as f32,
                preview::mode(layer.map_or(self.preview.display, |layer| layer.display)),
                band_count as f32,
            ],
            flat: [
                sea,
                rivers.map_or(0.0, |size| size as f32),
                flat_center[0],
                flat_center[1],
            ],
            screen: [0.0; 4],
            window,
            rivers: [
                cell as f32,
                window_size,
                self.preview.river_width * scale,
                0.0,
            ],
            layer: [layer.map_or(1.0, |layer| layer.opacity), 0.0, 0.0, 0.0],
            shade: match layer {
                Some(layer) => {
                    let radius_m = self.world_km * 1000.0;
                    let range = layer.bands.max() - layer.bands.min();
                    let relief = (range / radius_m) as f32;
                    let h = &layer.hillshade;
                    let opacity = if h.on { h.opacity.clamp(0.0, 1.0) } else { 0.0 };
                    let highlights = if h.highlights { 1.0 } else { 0.0 };
                    [opacity, highlights, relief, radius_m as f32]
                }
                None => [0.0; 4],
            },
            light: layer.map_or([0.0, 0.0, 1.0, 1.0], |layer| light(&layer.hillshade)),
            shadow: match layer {
                Some(layer) => {
                    let [r, g, b] = layer.hillshade.color;
                    color(eframe::egui::Color32::from_rgb(r, g, b))
                }
                None => [0.0; 4],
            },
            ground: color(crate::ui::pad::theme::GROUND),
            back: color(canvas.background),
            detail: match layer {
                Some(layer) => {
                    let size = layer.key.face_size;
                    let level = match frame {
                        // A map frame smooths by the scale of the page, so
                        // the zoom of the screen does not change it.
                        Some(frame) if layer.smoothing.on => smoothing_level(
                            f64::from(layer.smoothing.feature_size),
                            frame.points_per_radian,
                            size,
                        ),
                        Some(_) => 0.0,
                        None => self.level_of(layer.smoothing, size),
                    } as f32;
                    [level, max_level(size) as f32, 0.0, 0.0]
                }
                None => [0.0; 4],
            },
            edges: [0.0; 4],
            turn: {
                let (sin, cos) = frame.map_or(0.0, |frame| frame.rotation).sin_cos();
                let blend = layer.map_or(0, |layer| layer.blend.index());
                [cos as f32, sin as f32, blend as f32, 0.0]
            },
            bands,
            lines,
        }
    }

    /// The heightmap on the GPU that the globe draws a raster layer from:
    /// the full heightmap of the layer that the brush paints, or else the
    /// reduced copy. `None`: the GPU does not have each texel of the layer.
    fn source(&self, id: RasterId) -> Option<MapKey> {
        if self.draws_full(id) {
            return self.full;
        }
        let layer = self.layer(id)?;
        let copy = layer.copy.as_ref()?;
        copy.loaded.then_some(copy.key).or(layer.old_copy)
    }

    /// Sends the channel map of each layer that the GPU does not have, and
    /// returns the layers that have rivers in the picture, each with the
    /// size of its channel map. The rivers of the layer that the brush
    /// paints are those of the brush.
    fn send_rivers(&mut self) -> Vec<(LayerKey, usize)> {
        let key = |layer| MapKey { layer, ..NO_MAP };
        let mut out = Vec::new();
        let brush = match (self.raster, &self.stack) {
            (Some(id), _) if self.shows_rivers(id) => Some(LayerKey::Raster(id)),
            (None, None) => Some(LayerKey::Solo),
            _ => None,
        };
        match (brush, &self.channels) {
            (Some(layer), Some(channels)) if self.height => {
                if !std::mem::replace(&mut self.channels_sent, true) {
                    let op = Op::LayerChannels(channels.clone());
                    self.link.push(key(layer), op);
                }
                out.push((layer, channels.size()));
            }
            _ => self.channels_sent = false,
        }
        for layer in &mut self.layers {
            let Some(rivers) = &mut layer.rivers else {
                continue;
            };
            let Some(channels) = &rivers.channels else {
                continue;
            };
            let layer = LayerKey::Raster(layer.id);
            if !std::mem::replace(&mut rivers.sent, true) {
                self.link
                    .push(key(layer), Op::LayerChannels(channels.clone()));
            }
            out.push((layer, channels.size()));
        }
        out
    }

    /// The heightmaps on the GPU for the callbacks of one frame of the UI.
    fn gpu_frame(
        &self,
        format: wgpu::TextureFormat,
        smooth_full: bool,
        river_layers: &[(LayerKey, usize)],
        flat: Option<Arc<flat::FlatMesh>>,
    ) -> Arc<Frame> {
        Arc::new(Frame {
            format,
            link: self.link.clone(),
            full: self.full,
            smooth_full,
            copies: {
                let layers = self.layers.iter();
                let keys = |l: &Layer| [l.copy.as_ref().map(|copy| copy.key), l.old_copy];
                layers.flat_map(keys).flatten().collect()
            },
            rivers: river_layers.iter().map(|(layer, _)| *layer).collect(),
            images: self.image_keys(),
            flat,
            epoch: self.epoch,
        })
    }

    /// The raster layers and the image layers of the list that the GPU has,
    /// for the map frames, and `true` if one reads the full heightmap.
    fn frame_layers(&self) -> (Vec<FrameLayer<'_>>, bool) {
        let stack = self.stack.as_deref().unwrap_or_default();
        let mut smooth_full = false;
        let mut layers = Vec::new();
        for item in stack {
            match *item {
                StackItem::Raster {
                    id,
                    opacity,
                    display,
                    smoothing,
                    hillshade,
                    lines,
                    blend,
                    ..
                } => {
                    let source = self.source(id).zip(self.display_map(id));
                    let Some((key, map)) = source else { continue };
                    smooth_full |= Some(key) == self.full;
                    let layer = LayerDraw {
                        key,
                        bands: &map.bands,
                        opacity: opacity.clamp(0.0, 1.0),
                        display,
                        smoothing,
                        hillshade,
                        rivers: None,
                        window: false,
                        lines,
                        blend,
                    };
                    layers.push(FrameLayer::Raster(id, layer));
                }
                StackItem::Image { id, opacity, blend } => {
                    if let Some(key) = self.image_source(id) {
                        let opacity = opacity.clamp(0.0, 1.0);
                        layers.push(FrameLayer::Image(ImageDraw {
                            key,
                            opacity,
                            blend,
                        }));
                    }
                }
                StackItem::Vector(_) => {}
            }
        }
        (layers, smooth_full)
    }

    /// The passes of the layers that a map frame shows. The layers below the
    /// top raster layer that hides them do not show.
    fn frame_passes<'a>(
        &self,
        canvas: Canvas,
        layers: &[FrameLayer<'a>],
        frame: &'a FrameDraw,
        clip: [f32; 4],
    ) -> Vec<Draw> {
        let shown: Vec<FrameLayer> = layers
            .iter()
            .filter(|layer| match layer {
                FrameLayer::Raster(id, _) => frame.shown.contains(id),
                FrameLayer::Image(image) => frame.images.contains(&image.key.image),
            })
            .map(|layer| match *layer {
                FrameLayer::Raster(id, mut draw) => {
                    if let Some((_, look)) = frame.raster_looks.iter().find(|(r, _)| *r == id) {
                        draw.opacity = look.look.opacity.clamp(0.0, 1.0);
                        draw.blend = look.look.blend;
                        draw.display = look.look.display;
                        draw.lines = look.lines;
                        draw.smoothing = look.smoothing;
                        draw.hillshade = look.hillshade;
                        if let Some(bands) = &look.bands {
                            draw.bands = bands;
                        }
                    }
                    FrameLayer::Raster(id, draw)
                }
                FrameLayer::Image(mut image) => {
                    let key = image.key.image;
                    if let Some((_, look)) = frame.image_looks.iter().find(|(i, _)| *i == key) {
                        image.opacity = look.opacity.clamp(0.0, 1.0);
                        image.blend = look.blend;
                    }
                    FrameLayer::Image(image)
                }
            })
            .collect();
        let hides = |layer: &FrameLayer| matches!(layer, FrameLayer::Raster(_, l) if l.opacity >= 1.0 && l.blend == BlendMode::Normal);
        let start = shown.iter().rposition(hides).unwrap_or(0);
        let place = Some(&frame.place);
        let pass = |layer: &FrameLayer| match layer {
            FrameLayer::Raster(_, layer) => Draw {
                pass: Pass::Layer(layer.key.layer),
                source: Some(layer.key),
                image: None,
                rivers: false,
                lines: layer.lines
                    && layer.display == RasterDisplay::Bands
                    && layer.bands.stops().iter().any(|stop| stop.line.on),
                clip,
                uniforms: self.uniforms(canvas, Some(layer), false, false, place),
            },
            FrameLayer::Image(image) => self.image_pass(canvas, image, clip, false, place),
        };
        shown[start..].iter().map(pass).collect()
    }

    /// The pass of an image layer. `small`: the pass draws into the small
    /// target of the blurred copy, which mixes nothing.
    fn image_pass(
        &self,
        canvas: Canvas,
        image: &ImageDraw,
        clip: [f32; 4],
        small: bool,
        frame: Option<&FramePlace>,
    ) -> Draw {
        let mut uniforms = self.uniforms(canvas, None, false, small, frame);
        let key = image.key;
        uniforms.params = [key.width as f32, key.mips as f32, 0.0, 0.0];
        uniforms.layer[0] = image.opacity;
        uniforms.turn[2] = if small {
            0.0
        } else {
            image.blend.index() as f32
        };
        Draw {
            pass: Pass::Image(key.image),
            source: None,
            image: Some(key),
            rivers: false,
            lines: false,
            clip,
            uniforms,
        }
    }

    /// The callbacks that draw the raster layers and the image layers in the
    /// map frames of a page, one for each frame, or `None` with no wgpu
    /// renderer. A frame draws the layers of the list that its map shows, in
    /// the flat map of its projection, in a target of its own. It draws no
    /// rivers.
    pub fn paint_frames(
        &mut self,
        canvas: Canvas,
        frames: Vec<FrameDraw>,
    ) -> Option<Vec<(u32, GlobeCallback)>> {
        let format = self.format?;
        self.queue_changes();
        let river_layers = self.send_rivers();
        let (layers, smooth_full) = self.frame_layers();
        let mut draws = Vec::new();
        for frame in frames {
            let [x, y, w, h] = frame.rect;
            let clip = [0.0, 0.0, w as f32, h as f32];
            let group = self.frame_passes(canvas, &layers, &frame, clip);
            draws.push((
                frame.id,
                Group {
                    target: FRAME_TARGET + u64::from(frame.id),
                    place: Some([x, y, w, h]),
                    mesh: Some(frame.mesh),
                    mask: frame.mask,
                    draws: group,
                    capture: None,
                },
            ));
        }
        let gpu = self.gpu_frame(format, smooth_full, &river_layers, None);
        Some(
            draws
                .into_iter()
                .map(|(id, group)| {
                    let job = GpuJob::Group(Box::new(group));
                    (
                        id,
                        GlobeCallback {
                            frame: gpu.clone(),
                            job,
                        },
                    )
                })
                .collect(),
        )
    }

    /// Makes a callback that draws and returns the raster layers and the
    /// image layers of one map frame. The callback does not paint on the
    /// screen.
    pub fn capture_frame(
        &mut self,
        canvas: Canvas,
        frame: FrameDraw,
    ) -> Option<(GlobeCallback, mpsc::Receiver<Result<Captured, String>>)> {
        let format = self.format?;
        self.queue_changes();
        let river_layers = self.send_rivers();
        let (layers, smooth_full) = self.frame_layers();
        let clip = [0.0, 0.0, frame.rect[2] as f32, frame.rect[3] as f32];
        let draws = self.frame_passes(canvas, &layers, &frame, clip);
        let (capture, receiver) = Capture::new();
        let group = Group {
            target: CAPTURE_TARGET + u64::from(frame.id),
            place: Some([0, 0, frame.rect[2], frame.rect[3]]),
            mesh: Some(frame.mesh),
            mask: frame.mask,
            draws,
            capture: Some(capture),
        };
        let gpu = self.gpu_frame(format, smooth_full, &river_layers, None);
        let callback = GlobeCallback {
            frame: gpu,
            job: GpuJob::Group(Box::new(group)),
        };
        Some((callback, receiver))
    }

    /// The things that the canvas paints in this frame, from the bottom, or
    /// `None` with no wgpu renderer.
    ///
    /// The list has one pass for each visible raster layer, between the
    /// vector layers, in the draw order. It starts at the top raster layer
    /// that hides each layer below it. With no such layer, it starts with
    /// the plain ground. The graticule and the soft edge of the globe are
    /// above the top raster layer. With `blur`, the first item draws the
    /// raster layers into the blurred copy that the cards show.
    pub fn paint(&mut self, canvas: Canvas, blur: bool) -> Option<Vec<Paint>> {
        let format = self.format?;
        self.queue_changes();
        let river_layers = self.send_rivers();
        let rivers = |layer: LayerKey, display: RasterDisplay, on: bool| {
            let found = river_layers.iter().find(|(of, _)| *of == layer);
            let shown = on && display != RasterDisplay::Grey;
            found.map(|(_, size)| *size).filter(|_| shown)
        };

        // Each layer of the list, with the heightmap of a raster layer.
        enum Item<'a> {
            Raster(LayerDraw<'a>),
            Image(ImageDraw),
            Vector(LayerId),
        }
        let mut items: Vec<Item> = Vec::new();
        match &self.stack {
            None => {
                let key = self.full.expect("a globe with no list has a heightmap");
                let display = self.preview.display;
                items.push(Item::Raster(LayerDraw {
                    key,
                    bands: &self.map.bands,
                    opacity: 1.0,
                    display,
                    smoothing: Smoothing::default(),
                    hillshade: Hillshade::default(),
                    rivers: rivers(key.layer, display, self.preview.rivers),
                    window: true,
                    lines: true,
                    blend: BlendMode::Normal,
                }));
            }
            Some(stack) => {
                for item in stack {
                    match *item {
                        StackItem::Vector(id) => items.push(Item::Vector(id)),
                        StackItem::Image { id, opacity, blend } => {
                            if let Some(key) = self.image_source(id) {
                                let opacity = opacity.clamp(0.0, 1.0);
                                items.push(Item::Image(ImageDraw {
                                    key,
                                    opacity,
                                    blend,
                                }));
                            }
                        }
                        StackItem::Raster {
                            id,
                            opacity,
                            display,
                            smoothing,
                            hillshade,
                            rivers: on,
                            lines,
                            blend,
                        } => {
                            let source = self.source(id).zip(self.display_map(id));
                            if let Some((key, map)) = source {
                                items.push(Item::Raster(LayerDraw {
                                    key,
                                    bands: &map.bands,
                                    opacity: opacity.clamp(0.0, 1.0),
                                    display,
                                    smoothing,
                                    hillshade,
                                    rivers: rivers(key.layer, display, on),
                                    window: !key.display,
                                    lines,
                                    blend,
                                }));
                            }
                        }
                    }
                }
            }
        }
        // A layer that blends shows the layers below it through itself.
        let opaque = |item: &Item| matches!(item, Item::Raster(layer) if layer.opacity >= 1.0 && layer.blend == BlendMode::Normal);
        let start = items.iter().rposition(opaque);
        let top = items
            .iter()
            .rposition(|i| matches!(i, Item::Raster(..) | Item::Image(..)));

        let smooth_full = items
            .iter()
            .any(|item| matches!(item, Item::Raster(layer) if Some(layer.key) == self.full));
        let flat_mesh = (self.world_view == WorldView::Flat).then(|| self.flat.mesh());
        let frame = self.gpu_frame(format, smooth_full, &river_layers, flat_mesh.clone());
        let draw_job = |draw| GpuJob::Draw(Box::new(draw));
        let callback = |job| {
            Paint::Callback(GlobeCallback {
                frame: frame.clone(),
                job,
            })
        };
        let clip = {
            let r = self.rect * canvas.pixels_per_point;
            [r.min.x, r.min.y, r.max.x, r.max.y]
        };
        let plain = |pass, graticule, small| Draw {
            pass,
            source: None,
            image: None,
            rivers: false,
            lines: false,
            clip,
            uniforms: self.uniforms(canvas, None, graticule, small, None),
        };
        let layer = |item: &Item, small: bool| match item {
            Item::Raster(layer) => Some(Draw {
                pass: Pass::Layer(layer.key.layer),
                source: Some(layer.key),
                image: None,
                rivers: layer.rivers.is_some() && !small,
                // The blurred copy has no lines of the stops.
                lines: !small
                    && layer.lines
                    && layer.display == RasterDisplay::Bands
                    && layer.bands.stops().iter().any(|stop| stop.line.on),
                clip,
                uniforms: {
                    let mut uniforms = self.uniforms(canvas, Some(layer), false, small, None);
                    // The blurred copy mixes nothing.
                    if small {
                        uniforms.turn[2] = 0.0;
                    }
                    uniforms
                },
            }),
            Item::Image(image) => Some(self.image_pass(canvas, image, clip, small, None)),
            Item::Vector(_) => None,
        };

        let mut out = Vec::new();
        let drawn = &items[start.unwrap_or(0)..];
        if blur {
            // The blurred copy has no vector layers and no graticule.
            let ground = start.is_none().then(|| plain(Pass::Ground, false, true));
            let layers = drawn.iter().filter_map(|item| layer(item, true));
            let rim = plain(Pass::Rim, false, true);
            let draws = ground.into_iter().chain(layers).chain([rim]).collect();
            out.push(callback(GpuJob::Backdrop(canvas, draws)));
        }
        let graticule = self.preview.graticule;
        // The graticule, and then the soft edge of the globe.
        let over = || {
            let graticule = graticule.then(|| plain(Pass::Graticule, true, false));
            let passes = graticule
                .into_iter()
                .chain([plain(Pass::Rim, false, false)]);
            passes.map(|draw| callback(draw_job(draw)))
        };
        // The passes of the raster layers between two vector layers. A run
        // with a blend mode draws into a target of its own, because a blend
        // reads the pixels below the layer.
        let mut run: Vec<Draw> = Vec::new();
        let mut groups = 0;
        let mut flush = |run: &mut Vec<Draw>, out: &mut Vec<Paint>| {
            let draws = std::mem::take(run);
            if draws.iter().all(|draw| draw.uniforms.turn[2] < 1.0) {
                out.extend(draws.into_iter().map(|draw| callback(draw_job(draw))));
                return;
            }
            groups += 1;
            let group = Group {
                target: groups,
                place: None,
                mesh: flat_mesh.clone(),
                mask: None,
                draws,
                capture: None,
            };
            out.push(callback(GpuJob::Group(Box::new(group))));
        };
        if start.is_none() {
            run.push(plain(Pass::Ground, false, false));
        }
        if top.is_none() {
            flush(&mut run, &mut out);
            out.extend(over());
        }
        for (at, item) in items.iter().enumerate().skip(start.unwrap_or(0)) {
            match item {
                Item::Vector(id) => {
                    flush(&mut run, &mut out);
                    out.push(Paint::Vector(*id));
                }
                Item::Raster(..) | Item::Image(..) => {
                    run.extend(layer(item, false));
                    if top == Some(at) {
                        flush(&mut run, &mut out);
                        out.extend(over());
                    }
                }
            }
        }
        flush(&mut run, &mut out);
        Some(out)
    }
}

/// The opacity of a layer for the test of what it hides. A layer that
/// blends hides nothing.
fn hides(opacity: f32, blend: BlendMode) -> f32 {
    if blend == BlendMode::Normal {
        opacity
    } else {
        0.0
    }
}

/// The light of a hillshade for the shader: its direction in east, north,
/// and up, and the exaggeration of the slopes.
fn light(h: &Hillshade) -> [f32; 4] {
    let (az, alt) = (h.azimuth.to_radians(), h.altitude.to_radians());
    [
        az.sin() * alt.cos(),
        az.cos() * alt.cos(),
        alt.sin(),
        h.exaggeration,
    ]
}

/// One raster layer in a frame.
#[derive(Clone, Copy)]
struct LayerDraw<'a> {
    /// The heightmap on the GPU that the pass reads.
    key: MapKey,
    bands: &'a Bands,
    opacity: f32,
    display: RasterDisplay,
    smoothing: Smoothing,
    hillshade: Hillshade,
    /// The size of the channel map of the rivers of the layer. `None`: the
    /// pass draws no rivers.
    rivers: Option<usize>,
    /// The pass reads the full heightmap, so it has the window of small
    /// rivers.
    window: bool,
    /// The pass draws the lines of the stops.
    lines: bool,
    blend: BlendMode,
}

/// One image layer in a frame.
#[derive(Clone, Copy)]
struct ImageDraw {
    /// The copy on the GPU that the pass reads.
    key: ImageKey,
    opacity: f32,
    blend: BlendMode,
}

/// One layer of a map frame.
#[derive(Clone, Copy)]
enum FrameLayer<'a> {
    Raster(RasterId, LayerDraw<'a>),
    Image(ImageDraw),
}

/// Where a map frame draws the flat map in its target of pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FramePlace {
    /// The pixel of the target that shows the center of the map.
    pub center: [f32; 2],
    /// The center of the map, in units of the projection of radius 1.
    pub map_center: [f32; 2],
    /// Pixels in one unit of the projection.
    pub units_to_pixels: f32,
    /// The turn of the map, in radians clockwise.
    pub rotation: f64,
    /// Points of the page in one radian of the world.
    pub points_per_radian: f64,
    /// Pixels in one point of the page.
    pub pixels_per_page_point: f32,
}

/// Pixels in one point of a width, with `pixels_per_point` pixels in one
/// point of the screen. A map frame draws a width in points of the page.
/// `small`: the pass draws into the small target of the blurred copy.
fn point_scale(pixels_per_point: f32, frame: Option<&FramePlace>, small: bool) -> f32 {
    let point = frame.map_or(pixels_per_point, |frame| frame.pixels_per_page_point);
    match small {
        true => point / backdrop::REDUCTION as f32,
        false => point,
    }
}

/// The first number of a target of a map frame. The numbers below it are for
/// the groups of the canvas.
pub const FRAME_TARGET: TargetId = 1 << 32;
const CAPTURE_TARGET: TargetId = 1 << 48;

/// A map frame to draw.
#[derive(Clone)]
pub struct FrameDraw {
    pub id: u32,
    pub place: FramePlace,
    /// The left, the top, the width, and the height of the target on the
    /// screen, in pixels.
    pub rect: [u32; 4],
    pub mesh: Arc<flat::FlatMesh>,
    /// A shape that cuts the layers. The mask has the size of the target.
    pub mask: Option<Arc<Mask>>,
    /// The raster layers that the map shows.
    pub shown: Vec<RasterId>,
    /// The image layers that the map shows.
    pub images: Vec<ImageId>,
    /// The raster layers whose look the map overrides.
    pub raster_looks: Vec<(RasterId, RasterLook)>,
    /// The image layers whose look the map overrides.
    pub image_looks: Vec<(ImageId, Look)>,
}

/// The look of a raster layer in a map.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterLook {
    pub look: Look,
    pub lines: bool,
    pub smoothing: Smoothing,
    pub hillshade: Hillshade,
    /// The bands of the map. `None`: the bands of the layer.
    pub bands: Option<Bands>,
}

/// The look of a raster layer or an image layer in a map. An image layer
/// has no display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub opacity: f32,
    pub blend: BlendMode,
    pub display: RasterDisplay,
}

#[cfg(test)]
mod tests {
    use eframe::egui::Vec2;
    use vale_store::{Geometry, GeometryKind, LonLat};

    use super::*;
    use math::lonlat_to_dir;

    #[test]
    fn a_frame_scales_the_widths_by_the_page() {
        let frame = |pixels_per_page_point| FramePlace {
            center: [0.0; 2],
            map_center: [0.0; 2],
            units_to_pixels: 1.0,
            rotation: 0.0,
            points_per_radian: 1.0,
            pixels_per_page_point,
        };
        assert_eq!(point_scale(2.0, None, false), 2.0);
        assert_eq!(point_scale(2.0, Some(&frame(0.5)), false), 0.5);
        assert_eq!(point_scale(2.0, Some(&frame(3.0)), false), 3.0);
        let small = 3.0 / backdrop::REDUCTION as f32;
        assert_eq!(point_scale(2.0, Some(&frame(3.0)), true), small);
    }
    use snap::LayerTargets;
    use snap::tests::{layer, pen};

    #[test]
    fn the_default_light_is_from_the_northwest_and_45_degrees_up() {
        let [east, north, up, exaggeration] = light(&Hillshade::default());
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(near(east, -0.5), "{east}");
        assert!(near(north, 0.5), "{north}");
        assert!(near(up, std::f32::consts::FRAC_1_SQRT_2), "{up}");
        assert_eq!(exaggeration, 1.0);
    }

    #[test]
    fn a_hover_shows_where_the_stroke_starts() {
        let line = |id: u32, part: Vec<LonLat>| {
            layer(id, GeometryKind::Line, vec![Geometry::Lines(vec![part])])
        };
        let ours = line(9, vec![[0.0, 0.0], [10.0, 0.0]]);
        let other = line(1, vec![[0.0, 5.0], [5.0, 5.0], [10.0, 5.0]]);
        let built = [&ours, &other].map(|l| Arc::new(LayerTargets::build(l.id, l)));
        let mut globe = Globe {
            rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(300.0 / 0.45)),
            view: GlobeView::centered(5.0, 0.0),
            draw_layer: Some(LayerId(9)),
            lineable: true,
            tool: Tool::Line,
            ..Globe::default()
        };
        globe.set_targets(Some(Arc::new(Targets::new(built.to_vec(), Vec::new()))));
        let at = |globe: &Globe, lon: f64, lat: f64| {
            globe.view.project(globe.rect, lonlat_to_dir(lon, lat))
        };
        let lit = |globe: &mut Globe, lon: f64, lat: f64| {
            globe.input.pos = at(globe, lon, lat);
            globe.snap_highlight().and_then(|h| h.point)
        };
        let near = |p: Option<V3>, lon: f64, lat: f64| {
            p.is_some_and(|p| angle(p, lonlat_to_dir(lon, lat)) < 1e-9)
        };
        // Near the end of a line in the layer: the stroke extends it.
        assert!(near(lit(&mut globe, 10.5, 0.5), 10.0, 0.0));
        // Near a vertex of another layer: the start snaps to it.
        assert!(near(lit(&mut globe, 5.3, 5.3), 5.0, 5.0));
        // Far from each target.
        assert_eq!(lit(&mut globe, 5.0, 20.0), None);
        // With snapping off, only the end of a line shows.
        globe.snap.on = false;
        assert_eq!(lit(&mut globe, 5.3, 5.3), None);
        assert!(near(lit(&mut globe, 10.5, 0.5), 10.0, 0.0));
        // While the stroke starts at that end, the end shows.
        globe.line_down();
        globe.line_sample(Pos2::ZERO, Some(lonlat_to_dir(10.5, 0.5)));
        assert!(near(
            globe.snap_highlight().and_then(|h| h.point),
            10.0,
            0.0
        ));
    }

    #[test]
    fn the_highlight_shows_the_traced_span() {
        let part = (0..=10).map(|i| [3.0 * f64::from(i), 0.0]).collect();
        let river = layer(1, GeometryKind::Line, vec![Geometry::Lines(vec![part])]);
        let targets = Targets::new(
            vec![Arc::new(LayerTargets::build(river.id, &river))],
            Vec::new(),
        );
        // A globe of 300 points radius.
        let mut globe = Globe {
            rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(300.0 / 0.45)),
            view: GlobeView::centered(15.0, 0.0),
            draw_layer: Some(LayerId(9)),
            lineable: true,
            tool: Tool::Line,
            ..Globe::default()
        };
        globe.set_targets(Some(Arc::new(targets)));
        globe.line_down();
        let samples = pen(&[[0.0, 6.0], [4.0, 0.0], [20.0, 0.0], [24.0, 8.0]], 0.0);
        let mut traced = false;
        let mut left = false;
        for &p in &samples {
            globe.line_sample(Pos2::ZERO, Some(p));
            let lit = globe.snap_highlight().filter(|h| h.span.len() > 1);
            if let Some(h) = lit {
                traced = true;
                assert!(h.point.is_some());
                assert!(h.span.iter().all(|d| d[2].abs() < 1e-9));
            } else if traced {
                left = true;
            }
        }
        assert!(traced, "no highlight while the stroke traces");
        assert!(left, "the highlight stays after the stroke leaves");
        assert!(globe.snap_highlight().is_none());
        globe.line_up();
        assert!(globe.snap_highlight().is_none());
        let stroke = globe.take_line().expect("a stroke");
        assert!(
            stroke
                .runs
                .iter()
                .any(|r| matches!(r, Run::Traced(v) if v.len() > 1))
        );
    }
}
