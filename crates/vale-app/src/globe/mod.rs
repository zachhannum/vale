//! The globe workspace: the heightmap of the world on a globe that you rotate
//! and paint.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::time::Instant;

use eframe::egui::{Pos2, Rect};
use eframe::egui_wgpu::{self, wgpu};
use vale_terrain::{FACES, Heightmap, TexelRect, meters_to_level};

pub mod backdrop;
pub mod brush;
pub mod gpu;
pub mod math;
pub mod nav;
pub mod preview;
pub mod stats;
pub mod stroke_test;
pub mod view;

use crate::pen::Pen;
use backdrop::Canvas;
use brush::{Backlog, BrushSettings, Sample, Stroke, plan_texels};
use gpu::{Event, GlobeCallback, Link, Op, Uniforms};
use math::V3;
use nav::Nav;
use preview::{Preview, band_uniforms};
use stats::Stats;
use stroke_test::StrokeTest;
use view::GlobeView;

/// Texels on one edge of a cube face. The GPU texture holds six full faces.
pub const FACE_SIZE: usize = 1024;

/// The face size of the app in a window.
pub const WINDOW_FACE_SIZE: usize = 8192;

/// The elevation of the empty world, in meters.
const START_ELEVATION: f64 = -2500.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Navigate,
    Brush,
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

/// The stroke that the GPU works on.
struct Active {
    id: u64,
    stroke: Stroke,
    /// The texels of each face that the stamps can change.
    touched: [Option<TexelRect>; FACES],
    /// The time of the last sample.
    sampled: Option<Instant>,
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

pub struct Globe {
    /// The heightmap of the document. During a stroke, the GPU texture is
    /// ahead of it. The texels come back when the stroke ends.
    pub map: Heightmap,
    pub view: GlobeView,
    pub nav: Nav,
    /// The canvas in screen points. Set by the canvas each frame.
    pub rect: Rect,
    /// The format of the render target. `None`: no wgpu renderer draws the UI.
    pub format: Option<wgpu::TextureFormat>,
    pub tool: Tool,
    pub preview: Preview,
    pub brush: BrushSettings,
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
    link: Link,
    events: mpsc::Receiver<Event>,
    /// Pen input that waits for the stroke before it.
    inputs: VecDeque<Input>,
    stroke: Option<Active>,
    backlog: Backlog,
    strokes: u64,
    test: Option<StrokeTest>,
}

impl Default for Globe {
    fn default() -> Globe {
        Globe::with_face_size(FACE_SIZE)
    }
}

impl Globe {
    pub fn with_face_size(face_size: usize) -> Globe {
        let (link, events) = Link::new();
        Globe {
            map: Heightmap::new(face_size, meters_to_level(START_ELEVATION)),
            view: GlobeView::centered(15.0, 25.0),
            nav: Nav::default(),
            rect: Rect::ZERO,
            format: None,
            tool: Tool::default(),
            preview: Preview::default(),
            brush: BrushSettings::default(),
            input: BrushInput::default(),
            pen: Pen::default(),
            pick_level: false,
            stats: Stats::new(face_size as u32),
            debug: false,
            blur: true,
            test_report: None,
            link,
            events,
            inputs: VecDeque::new(),
            stroke: None,
            backlog: Backlog::default(),
            strokes: 0,
            test: None,
        }
    }

    /// Replaces the heightmap with an empty one of this face size. The band
    /// limits and the ramp stay.
    pub fn set_face_size(&mut self, face_size: usize) {
        let bands = std::mem::take(&mut self.map.bands);
        self.map = Heightmap::new(face_size, meters_to_level(START_ELEVATION));
        self.map.bands = bands;
        self.stats.face_size = face_size as u32;
        self.inputs.clear();
        self.backlog.clear();
        self.stroke = None;
        if let Some(test) = self.test.take() {
            self.brush = test.saved;
        }
    }

    /// Connects the globe to the renderer of the UI. A face is not larger
    /// than the largest texture of the device.
    pub fn attach(&mut self, render_state: &egui_wgpu::RenderState) {
        self.format = Some(render_state.target_format);
        let info = render_state.adapter.get_info();
        self.stats.adapter = format!("{} ({:?})", info.name, info.backend);
        let limit = render_state.device.limits().max_texture_dimension_2d as usize;
        if self.map.face_size() > limit {
            self.set_face_size(limit);
        }
    }

    /// The format of the blurred canvas behind a card. `None`: a card has a
    /// plain fill.
    pub fn backdrop(&self) -> Option<wgpu::TextureFormat> {
        self.format.filter(|_| self.blur)
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

    /// The radius of the globe on screen, in points.
    pub fn radius(&self) -> f64 {
        self.view.radius(self.rect)
    }

    /// Sets the flatten level from the heightmap at a place. During a stroke
    /// the heightmap is behind the GPU, and the level does not change.
    pub fn pick(&mut self, dir: Option<V3>) {
        if let Some(dir) = dir.filter(|_| !self.busy()) {
            self.brush.flatten_level = Some(self.map.sample(dir));
            self.pick_level = false;
        }
    }

    /// True while a stroke or a stroke test is not complete.
    pub fn busy(&self) -> bool {
        self.stroke.is_some() || !self.inputs.is_empty() || self.test.is_some()
    }

    pub fn can_undo(&self) -> bool {
        !self.busy() && self.map.can_undo()
    }

    /// Puts back the heightmap from before the last stroke.
    pub fn undo(&mut self) -> bool {
        self.can_undo() && self.map.undo()
    }

    /// Starts the stroke test. A slow stroke is `seconds` long.
    pub fn start_stroke_test(&mut self, seconds: f64) {
        if self.format.is_some() && !self.busy() {
            self.test = Some(StrokeTest::new(self.brush, seconds));
            self.test_report = None;
        }
    }

    /// Runs the brush for one frame. `now` is the start of the frame. Returns
    /// `true` if the brush needs one more frame.
    pub fn advance(&mut self, now: Instant) -> bool {
        if self.format.is_none() {
            self.inputs.clear();
            return false;
        }
        // The count comes first. A result that arrives after this line makes
        // one more frame.
        let in_flight = self.link.busy.any();
        self.read_events();
        // An undo goes to the GPU before the stamps of the next stroke.
        gpu::queue_changes(&mut self.map, &self.link);
        self.run_test(now, in_flight);
        self.read_inputs();
        self.send_stamps(now);
        in_flight || self.busy()
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
                    let left = active.reading.get_or_insert(1);
                    *left -= 1;
                    if *left == 0 {
                        self.map.end_stroke();
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
                    self.strokes += 1;
                    self.map.begin_stroke();
                    let points = self.brush.points(self.radius());
                    self.stats
                        .begin_stroke(self.strokes, self.brush.mode, points);
                    self.stroke = Some(Active {
                        id: self.strokes,
                        stroke: Stroke::default(),
                        touched: [None; FACES],
                        sampled: None,
                        ended: false,
                        reading: None,
                    });
                }
                Input::Sample(sample, time) => {
                    if let Some(active) = self.stroke.as_mut().filter(|a| !a.ended) {
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
                        active.sampled = Some(time);
                        for stamp in &stamps {
                            self.backlog.push(map.stamp_plan(stamp), time);
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
                        for stamp in &stamps {
                            let time = active.sampled.unwrap_or_else(Instant::now);
                            self.backlog.push(map.stamp_plan(stamp), time);
                        }
                    }
                }
            }
            self.inputs.pop_front();
        }
    }

    /// Sends the stamps of this frame to the GPU. After the last stamp of a
    /// stroke, asks for the texels that the stroke changed.
    fn send_stamps(&mut self, now: Instant) {
        let Some(active) = &mut self.stroke else {
            return;
        };
        let face_size = self.map.face_size() as u32;
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
            self.link.push(face_size, Op::Stamp { plan, stroke, time });
        }
        self.stats.backlog = self.backlog.len();
        if !active.ended || !self.backlog.is_empty() || active.reading.is_some() {
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
            self.link
                .push(face_size, Op::Readback { stroke, face, rect });
        }
    }

    /// The paint callback of this frame, or `None` with no wgpu renderer.
    pub fn callback(
        &mut self,
        pixels_per_point: f32,
        backdrop: Option<Canvas>,
    ) -> Option<GlobeCallback> {
        let format = self.format?;
        gpu::queue_changes(&mut self.map, &self.link);
        let row = |r: V3| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0];
        let center = self.rect.center();
        let radius = self.view.radius(self.rect) as f32;
        let graticule_degrees: f64 = if !self.preview.graticule {
            0.0
        } else if self.view.zoom < 3.0 {
            15.0
        } else if self.view.zoom < 12.0 {
            5.0
        } else {
            1.0
        };
        let (bands, band_count) = band_uniforms(&self.map.bands);
        Some(GlobeCallback {
            format,
            face_size: self.map.face_size() as u32,
            uniforms: Uniforms {
                rot: self.view.rot.0.map(row),
                globe: [
                    center.x * pixels_per_point,
                    center.y * pixels_per_point,
                    radius * pixels_per_point,
                    pixels_per_point,
                ],
                params: [
                    self.map.face_size() as f32,
                    graticule_degrees.to_radians() as f32,
                    self.preview.mode(),
                    band_count as f32,
                ],
                bands,
            },
            link: self.link.clone(),
            backdrop,
        })
    }
}
