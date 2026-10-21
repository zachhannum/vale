//! The globe workspace: the layers of the world on a globe that you rotate,
//! and the raster layer that you paint.

use std::collections::VecDeque;
use std::f64::consts::{FRAC_PI_2, PI};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use eframe::egui::{Pos2, Rect};
use eframe::egui_wgpu::{self, wgpu};
use vale_store::{LayerId, RasterDisplay, RasterId, Smoothing};
use vale_terrain::{
    Bands, ChannelMap, ChannelWindow, ErodeBrush, FACES, Heightmap, Mode, Pyramid, SEA_LEVEL,
    Stamp, TexelRect, Window, WindowHeights, max_level, meters_to_level, reduced_cap,
    reduced_face_size, smoothing_level,
};

pub mod backdrop;
pub mod brush;
pub mod erode;
pub mod export;
pub mod flat;
pub mod gpu;
pub mod import;
pub mod line;
pub mod math;
pub mod nav;
pub mod preview;
pub mod rivers;
pub mod stats;
pub mod stroke_test;
pub mod thumbnail;
pub mod vector;
pub mod view;

use crate::pen::Pen;
use backdrop::Canvas;
use brush::{Backlog, BrushSettings, STAMP_SPACING, Sample, Stroke, plan_texels, set_world};
use erode::{Eroder, Ground, brush_window, erode_window, near_side};
use export::ExportResult;
use flat::FlatView;
use gpu::{
    Changes, Draw, Event, Frame, GlobeCallback, Job as GpuJob, LayerKey, Link, MapKey, Op, Pass,
    Uniforms, Uploads,
};
use import::{ImportResult, Job, Step};
use line::LineSettings;
use math::{V3, angle};
use nav::Nav;
use preview::{Preview, band_uniforms, line_uniforms};
use rivers::{Rivers, choose_window};
use stats::Stats;
use stroke_test::StrokeTest;
use view::GlobeView;

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

/// A stroke of the line tool.
#[derive(Clone, Debug, PartialEq)]
pub struct LineStroke {
    pub screen: Vec<Pos2>,
    /// The samples on the unit sphere.
    pub dirs: Vec<V3>,
    /// The points in one radian at the start of the stroke.
    pub scale: f64,
    /// The pen left the world, and the stroke takes no more samples.
    off: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Navigate,
    Brush,
    Line,
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
    /// how it colors its values, and how it smooths them by the scale.
    Raster {
        id: RasterId,
        opacity: f32,
        display: RasterDisplay,
        smoothing: Smoothing,
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
    /// The stroke of the line tool while the pen is down.
    line_stroke: Option<LineStroke>,
    /// The last stroke of the line tool, for the layer.
    line_done: Option<LineStroke>,
    /// The radius of the world, in kilometers. Set by the canvas each frame.
    pub world_km: f64,
    /// The seed of the world. Set by the canvas each frame.
    pub world_seed: u32,
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
            line_stroke: None,
            line_done: None,
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
            input: BrushInput::default(),
            pen: Pen::default(),
            pick_level: false,
            stats: Stats::new(face_size as u32),
            debug: false,
            blur: true,
            test_report: None,
            uploads: Uploads::default(),
            link,
            events,
            inputs: VecDeque::new(),
            stroke: None,
            backlog: Backlog::default(),
            strokes: 0,
            test: None,
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
            StackItem::Vector(_) => None,
        };
        stack.iter().filter_map(layer).collect()
    }

    /// Gives its own rivers to each visible height layer that the brush
    /// does not paint, and takes them from each other layer.
    fn sync_rivers(&mut self) {
        let sync = self.rivers.sync();
        for at in 0..self.layers.len() {
            let layer = &self.layers[at];
            let wanted = layer.height && self.visible(layer.id) && Some(layer.id) != self.raster;
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
                StackItem::Vector(_) => None,
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
        self.format.is_some() && (waits || self.layers.iter().any(wrong))
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

    /// Makes each reduced copy smaller if the copies hold more memory than
    /// the budget.
    fn check_budget(&mut self) {
        let (epoch, bytes, _) = self.link.copy_bytes();
        let over = epoch == self.epoch && bytes > self.display_budget && !self.loading();
        if over && self.display_cap > DISPLAY_CAP_MIN {
            self.display_cap /= 2;
        }
        self.sync_copies();
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

    /// Starts a stroke of the line tool.
    pub fn line_down(&mut self) {
        self.line_stroke = Some(LineStroke {
            screen: Vec::new(),
            dirs: Vec::new(),
            scale: self.radius(),
            off: false,
        });
    }

    /// Adds one sample to the stroke of the line tool. The stroke ends at
    /// the first sample off the world.
    pub fn line_sample(&mut self, pos: Pos2, dir: Option<V3>) {
        let Some(stroke) = self.line_stroke.as_mut().filter(|s| !s.off) else {
            return;
        };
        match dir {
            Some(dir) => {
                stroke.screen.push(pos);
                stroke.dirs.push(dir);
            }
            None => stroke.off = !stroke.dirs.is_empty(),
        }
    }

    /// Ends the stroke of the line tool. `take_line` gives it.
    pub fn line_up(&mut self) {
        self.line_done = self.line_stroke.take();
    }

    /// The stroke of the line tool while the pen is down, in screen points.
    pub fn line_preview(&self) -> Option<&[Pos2]> {
        self.line_stroke.as_ref().map(|s| s.screen.as_slice())
    }

    /// The last stroke of the line tool that ended.
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
        if let Some(job) = &self.import {
            let name = import::file_name(&path);
            let text = format!(
                "cannot import {name}: the import of {} is not complete",
                job.name
            );
            self.import_result = Some(Err(text));
            return;
        }
        self.import_result = None;
        // The import is for the raster layer that the brush paints after
        // the stroke that runs.
        let target = self.pending.unwrap_or(self.raster);
        let layer = target.and_then(|id| self.layer(id));
        let face_size = layer.map_or(self.map.face_size(), |layer| layer.face_size);
        self.import = Some(Job::start(path, face_size));
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
            Some(job) => Some(format!("the export of {} is not complete", job.name)),
            // The heightmap is behind the GPU during a stroke.
            None if self.busy() => Some("an edit of the heightmap is not complete".to_string()),
            None => None,
        };
        if let Some(why) = refusal {
            self.export_result = Some(Err(format!("cannot export {name}: {why}")));
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

    /// Runs the brush for one frame. `now` is the start of the frame. Returns
    /// `true` if the brush needs one more frame.
    pub fn advance(&mut self, now: Instant) -> bool {
        if self.format.is_none() {
            self.sync_maps();
            self.inputs.clear();
            self.pump_import(false);
            self.pump_export(false);
            return self.import.is_some() || self.export.is_some();
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
        self.send_stamps(now);
        let busy = self.busy() || self.loading();
        in_flight || busy || self.rivers.pending() || self.export.is_some()
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
                        ended: false,
                        reading: None,
                    });
                }
                Input::Sample(sample, time) => {
                    if let Some(active) = self.stroke.as_mut().filter(|a| !a.ended) {
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
                            for stamp in &mut stamps {
                                set_world(
                                    stamp,
                                    &self.brush,
                                    map.face_size(),
                                    self.world_km,
                                    self.world_seed,
                                );
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
                        for stamp in &mut stamps {
                            set_world(
                                stamp,
                                &self.brush,
                                map.face_size(),
                                self.world_km,
                                self.world_seed,
                            );
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
        self.eroder.request(brushes, rate, 0.0, self.brush.limits());
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
        self.eroder
            .request(brushes.collect(), rate, stamps[0].strength, limits);
        uplift.asked = stamps;
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
    ) -> Uniforms {
        let row = |r: V3| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0];
        let center = self.rect.center();
        let radius = self.radius() as f32;
        let (zoom, flat_center) = match self.world_view {
            WorldView::Globe => (self.view.zoom, [0.0; 2]),
            WorldView::Flat => {
                let center = self.flat.center;
                (self.flat.zoom, [center.x as f32, center.y as f32])
            }
        };
        let pixels_per_point = canvas.pixels_per_point;
        let scale = match small {
            true => pixels_per_point / backdrop::REDUCTION as f32,
            false => pixels_per_point,
        };
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
            Some(layer) => line_uniforms(layer.bands, scale),
            None => [[[0.0; 4]; 2]; vale_terrain::MAX_BANDS + 1],
        };
        let color = |c: eframe::egui::Color32| {
            let [r, g, b] = [c.r(), c.g(), c.b()].map(|c| f32::from(c) / 255.0);
            [r, g, b, 1.0]
        };
        Uniforms {
            rot: self.view.rot.0.map(row),
            globe: [
                center.x * scale,
                center.y * scale,
                radius * scale,
                pixels_per_point,
            ],
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
            rivers: [cell as f32, window_size, self.preview.river_width, 0.0],
            layer: [layer.map_or(1.0, |layer| layer.opacity), 0.0, 0.0, 0.0],
            ground: color(crate::ui::pad::theme::GROUND),
            back: color(canvas.background),
            detail: match layer {
                Some(layer) => {
                    let size = layer.key.face_size;
                    let level = self.level_of(layer.smoothing, size) as f32;
                    [level, max_level(size) as f32, 0.0, 0.0]
                }
                None => [0.0; 4],
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
            (Some(id), _) if self.visible(id) => Some(LayerKey::Raster(id)),
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
        let show_rivers = self.preview.rivers;
        let river_layers = self.send_rivers();
        let rivers = |layer: LayerKey, display: RasterDisplay| {
            let found = river_layers.iter().find(|(of, _)| *of == layer);
            let shown = show_rivers && display != RasterDisplay::Grey;
            found.map(|(_, size)| *size).filter(|_| shown)
        };

        // Each layer of the list, with the heightmap of a raster layer.
        enum Item<'a> {
            Raster(LayerDraw<'a>),
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
                    rivers: rivers(key.layer, display),
                    window: true,
                }));
            }
            Some(stack) => {
                for item in stack {
                    match *item {
                        StackItem::Vector(id) => items.push(Item::Vector(id)),
                        StackItem::Raster {
                            id,
                            opacity,
                            display,
                            smoothing,
                        } => {
                            let source = self.source(id).zip(self.display_map(id));
                            if let Some((key, map)) = source {
                                items.push(Item::Raster(LayerDraw {
                                    key,
                                    bands: &map.bands,
                                    opacity: opacity.clamp(0.0, 1.0),
                                    display,
                                    smoothing,
                                    rivers: rivers(key.layer, display),
                                    window: !key.display,
                                }));
                            }
                        }
                    }
                }
            }
        }
        let opaque = |item: &Item| matches!(item, Item::Raster(layer) if layer.opacity >= 1.0);
        let start = items.iter().rposition(opaque);
        let top = items.iter().rposition(|i| matches!(i, Item::Raster(..)));

        let frame = Arc::new(Frame {
            format,
            link: self.link.clone(),
            full: self.full,
            smooth_full: items
                .iter()
                .any(|item| matches!(item, Item::Raster(layer) if Some(layer.key) == self.full)),
            copies: {
                let layers = self.layers.iter();
                let keys = |l: &Layer| [l.copy.as_ref().map(|copy| copy.key), l.old_copy];
                layers.flat_map(keys).flatten().collect()
            },
            rivers: river_layers.iter().map(|(layer, _)| *layer).collect(),
            flat: (self.world_view == WorldView::Flat).then(|| self.flat.mesh()),
            epoch: self.epoch,
        });
        let draw_job = |draw| GpuJob::Draw(Box::new(draw));
        let callback = |job| {
            Paint::Callback(GlobeCallback {
                frame: frame.clone(),
                job,
            })
        };
        let plain = |pass, graticule, small| Draw {
            pass,
            source: None,
            rivers: false,
            uniforms: self.uniforms(canvas, None, graticule, small),
        };
        let layer = |item: &Item, small: bool| match item {
            Item::Raster(layer) => Some(Draw {
                pass: Pass::Layer(layer.key.layer),
                source: Some(layer.key),
                rivers: layer.rivers.is_some() && !small,
                uniforms: self.uniforms(canvas, Some(layer), false, small),
            }),
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
        if start.is_none() {
            out.push(callback(draw_job(plain(Pass::Ground, false, false))));
        }
        if top.is_none() {
            out.extend(over());
        }
        for (at, item) in items.iter().enumerate().skip(start.unwrap_or(0)) {
            match item {
                Item::Vector(id) => out.push(Paint::Vector(*id)),
                Item::Raster(..) => {
                    out.extend(layer(item, false).map(|draw| callback(draw_job(draw))));
                    if top == Some(at) {
                        out.extend(over());
                    }
                }
            }
        }
        Some(out)
    }
}

/// One raster layer in a frame.
struct LayerDraw<'a> {
    /// The heightmap on the GPU that the pass reads.
    key: MapKey,
    bands: &'a Bands,
    opacity: f32,
    display: RasterDisplay,
    smoothing: Smoothing,
    /// The size of the channel map of the rivers of the layer. `None`: the
    /// pass draws no rivers.
    rivers: Option<usize>,
    /// The pass reads the full heightmap, so it has the window of small
    /// rivers.
    window: bool,
}
