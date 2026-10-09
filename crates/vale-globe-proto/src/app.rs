//! The app: the tools, the pen and touch input, and the floating panels.

use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;

use eframe::egui::{self, Color32, Pos2, Rect};
use eframe::egui_wgpu::{self, wgpu};

use vale_terrain::{
    ELEV_MAX, ELEV_MIN, Heightmap, MAX_BRUSH_RADIUS, Mode, Stamp, level_to_meters, meters_to_level,
};

use crate::gpu::{GlobeCallback, MAX_BANDS, Uniforms, Upload, UploadQueue};
use crate::math::{V3, angle, dir_to_lonlat, slerp};
use crate::view::GlobeView;

pub const DEFAULT_FACE_SIZE: usize = 1024;

/// The elevation of the empty world, in meters.
const START_ELEVATION: f64 = -2500.0;

/// The lower limit of each band in meters, and the tint of the band.
pub const BANDS: [(f64, [u8; 3]); 12] = [
    (ELEV_MIN, [0x24, 0x55, 0x86]),
    (-4000.0, [0x35, 0x6f, 0xa3]),
    (-2000.0, [0x54, 0x8f, 0xbd]),
    (-1000.0, [0x80, 0xb0, 0xd3]),
    (-200.0, [0xb4, 0xd5, 0xe8]),
    (0.0, [0xa9, 0xc7, 0x8e]),
    (200.0, [0xc8, 0xd7, 0x9f]),
    (500.0, [0xe4, 0xdc, 0xa6]),
    (1000.0, [0xdb, 0xc0, 0x8a]),
    (2000.0, [0xc5, 0x9b, 0x6c]),
    (3000.0, [0xa9, 0x7c, 0x5b]),
    (4500.0, [0xf3, 0xf0, 0xea]),
];

const BACKGROUND: Color32 = Color32::from_rgb(22, 25, 31);
const LINE_COLOR: Color32 = Color32::from_rgb(20, 66, 140);

/// The distance between stamps, as a part of the brush radius.
const STAMP_SPACING: f64 = 0.12;
/// The flow of a mouse or a finger, which have no pressure.
const FIXED_FLOW: f64 = 0.5;
/// A finger that lands this soon after the pen lifts is a resting palm.
const PALM_SECONDS: f64 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Paint,
    Line,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Pen,
    Finger,
    Mouse,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Source::Pen => "pen",
            Source::Finger => "finger",
            Source::Mouse => "mouse",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BrushSettings {
    pub mode: Mode,
    /// The radius on screen in points. The app uses it when `lock_km` is off.
    pub size_points: f32,
    /// The radius on the ground in kilometers. The app uses it when `lock_km` is on.
    pub size_km: f64,
    pub lock_km: bool,
    pub hardness: f64,
    /// The height that one pass adds at full pressure, in meters, roughly.
    pub strength_m: f64,
    /// The multiplier from pen force to flow.
    pub pressure_gain: f64,
}

/// A stroke in sphere coordinates. Screen input and the fixed test strokes
/// both feed it.
struct Stroke {
    source: Source,
    tool: Tool,
    /// The last sample: direction and flow. `None` after the pen left the globe.
    last: Option<(V3, f64)>,
    /// The distance to the next stamp, in radians.
    carry: f64,
    /// The target of the flatten mode, read at the first sample.
    level: Option<u16>,
    raw_line: Vec<V3>,
    samples: u32,
    started: f64,
    pressures: Vec<f32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TouchKind {
    Pen,
    /// A finger that draws.
    Draw,
    /// A finger that rotates or zooms the globe.
    Nav,
    /// A palm, or a touch that started on a panel.
    Ignored,
}

struct Touch {
    kind: TouchKind,
    pos: Pos2,
    prev: Pos2,
}

/// The numbers that the diagnostics panel shows.
#[derive(Default)]
pub struct Stats {
    frame_ms: VecDeque<f32>,
    pub last_source: Option<Source>,
    pub last_force: Option<f32>,
    pub pen_touches: u32,
    pub finger_touches: u32,
    pub palms_rejected: u32,
    pub hover_seen: bool,
    /// The input samples per second of the last stroke.
    pub stroke_rate: f32,
    pub stroke_samples: u32,
    pub max_moves_per_frame: u32,
    pub stamps: u32,
    pub stamp_ms: f32,
    pub stamp_ms_peak: f32,
    pub upload_texels: usize,
    pub pressure_trace: Vec<f32>,
}

pub struct ProtoApp {
    pub map: Heightmap,
    pub view: GlobeView,
    pub lines: Vec<Vec<V3>>,
    pub tool: Tool,
    pub brush: BrushSettings,
    pub world_radius_km: f64,
    pub stepped: bool,
    pub graticule: bool,
    pub one_finger_rotates: bool,
    pub finger_draws: bool,
    pub continuous: bool,
    pub stats: Stats,
    format: wgpu::TextureFormat,
    uploads: UploadQueue,
    stroke: Option<Stroke>,
    touches: BTreeMap<u64, Touch>,
    /// True after the first touch event. The mouse path is then off.
    touch_mode: bool,
    /// The view from before the fingers moved it, and the time.
    nav_restore: Option<(GlobeView, f64)>,
    last_pen_time: f64,
    mouse_rotates: bool,
    /// The place of the brush cursor on the globe.
    cursor: Option<V3>,
    was_active: bool,
}

/// Makes the controls large enough for a finger.
pub fn apply_touch_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.interact_size = egui::vec2(48.0, 38.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.item_spacing = egui::vec2(10.0, 9.0);
        style.spacing.slider_width = 170.0;
        style.spacing.icon_width = 22.0;
        for (text_style, font) in &mut style.text_styles {
            font.size = match text_style {
                egui::TextStyle::Heading => 19.0,
                egui::TextStyle::Small => 12.0,
                egui::TextStyle::Monospace => 13.0,
                _ => 15.0,
            };
        }
    });
}

/// True if the touch is a pen.
///
/// egui has no pen type. On iPad, winit reports a force for Apple Pencil only,
/// because no iPad screen measures finger force. So a force means a pen.
fn is_pen(force: Option<f32>) -> bool {
    force.is_some()
}

impl ProtoApp {
    pub fn new(format: wgpu::TextureFormat, face_size: usize) -> ProtoApp {
        ProtoApp {
            map: Heightmap::new(face_size, meters_to_level(START_ELEVATION)),
            view: GlobeView::centered(15.0, 25.0),
            lines: Vec::new(),
            tool: Tool::Paint,
            brush: BrushSettings {
                mode: Mode::Raise,
                size_points: 36.0,
                size_km: 600.0,
                lock_km: false,
                hardness: 0.3,
                strength_m: 1500.0,
                pressure_gain: 2.0,
            },
            world_radius_km: 6371.0,
            stepped: true,
            graticule: true,
            one_finger_rotates: true,
            finger_draws: false,
            continuous: false,
            stats: Stats::default(),
            format,
            uploads: UploadQueue::default(),
            stroke: None,
            touches: BTreeMap::new(),
            touch_mode: false,
            nav_restore: None,
            last_pen_time: f64::MIN,
            mouse_rotates: false,
            cursor: None,
            was_active: false,
        }
    }

    /// The brush radius as an angle on the sphere, in radians.
    pub fn brush_radius(&self, rect: Rect) -> f64 {
        let radius = if self.brush.lock_km {
            self.brush.size_km / self.world_radius_km
        } else {
            f64::from(self.brush.size_points) / self.view.radius(rect)
        };
        radius.clamp(1e-4, MAX_BRUSH_RADIUS)
    }

    /// The width of one texel on the ground, in kilometers.
    pub fn texel_km(&self) -> f64 {
        std::f64::consts::FRAC_PI_2 / self.map.face_size() as f64 * self.world_radius_km
    }

    pub fn begin_stroke(&mut self, source: Source, now: f64) {
        if self.tool == Tool::Paint {
            self.map.begin_stroke();
        }
        self.stats.last_source = Some(source);
        self.stroke = Some(Stroke {
            source,
            tool: self.tool,
            last: None,
            carry: 0.0,
            level: None,
            raw_line: Vec::new(),
            samples: 0,
            started: now,
            pressures: Vec::new(),
        });
    }

    /// Adds one input sample to the stroke. `dir` is `None` off the globe.
    /// `flow` is from 0 to 1, and `radius` is the brush radius in radians.
    pub fn add_sample(&mut self, dir: Option<V3>, flow: f64, radius: f64) {
        let Some(stroke) = &mut self.stroke else {
            return;
        };
        stroke.samples += 1;
        stroke.pressures.push(flow as f32);
        let Some(dir) = dir else {
            stroke.last = None;
            return;
        };
        self.cursor = Some(dir);
        if stroke.tool == Tool::Line {
            stroke.raw_line.push(dir);
            return;
        }
        let texel = std::f64::consts::FRAC_PI_2 / self.map.face_size() as f64;
        let spacing = (radius * STAMP_SPACING).max(texel * 0.5);
        let level = *stroke.level.get_or_insert_with(|| self.map.sample(dir));
        let brush = self.brush;
        let span = (ELEV_MAX - ELEV_MIN) / 65535.0;
        let stamp_at = |center: V3, flow: f64| Stamp {
            center,
            radius,
            hardness: brush.hardness,
            flow: match brush.mode {
                Mode::Raise | Mode::Lower => flow,
                // These two modes move toward a target, so each stamp does less.
                Mode::Smooth | Mode::Flatten => flow * 0.3,
            },
            mode: brush.mode,
            level,
            strength: brush.strength_m * STAMP_SPACING / span,
        };
        let start = Instant::now();
        match stroke.last {
            None => {
                self.map.stamp(&stamp_at(dir, flow));
                self.stats.stamps += 1;
                stroke.carry = spacing;
            }
            Some((prev, prev_flow)) => {
                let len = angle(prev, dir);
                let mut at = 0.0;
                while len - at >= stroke.carry {
                    at += stroke.carry;
                    let t = at / len;
                    let stamp = stamp_at(slerp(prev, dir, t), prev_flow + (flow - prev_flow) * t);
                    self.map.stamp(&stamp);
                    self.stats.stamps += 1;
                    stroke.carry = spacing;
                }
                stroke.carry -= len - at;
            }
        }
        self.stats.stamp_ms += start.elapsed().as_secs_f32() * 1000.0;
        stroke.last = Some((dir, flow));
    }

    pub fn end_stroke(&mut self, now: f64) {
        let Some(stroke) = self.stroke.take() else {
            return;
        };
        match stroke.tool {
            Tool::Paint => {
                self.map.end_stroke();
            }
            Tool::Line => {
                if stroke.raw_line.len() >= 2 {
                    // About three points of screen distance between vertices.
                    let step = (3.0
                        / self
                            .view
                            .radius(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 800.0))))
                    .max(1e-4);
                    self.lines
                        .push(crate::lines::smooth(&stroke.raw_line, step));
                }
            }
        }
        let seconds = (now - stroke.started).max(1e-3);
        self.stats.stroke_samples = stroke.samples;
        self.stats.stroke_rate = stroke.samples as f32 / seconds as f32;
        self.stats.pressure_trace = stroke.pressures;
        eprintln!(
            "stroke: {} {:?}, {} samples in {:.2} s, {:.0} samples/s",
            stroke.source.name(),
            stroke.tool,
            stroke.samples,
            seconds,
            self.stats.stroke_rate
        );
    }

    /// Drops the stroke and takes back what it painted.
    fn cancel_stroke(&mut self) {
        if let Some(stroke) = self.stroke.take()
            && stroke.tool == Tool::Paint
            && self.map.end_stroke()
        {
            self.map.undo();
        }
    }

    pub fn undo(&mut self) {
        match self.tool {
            Tool::Paint => {
                self.map.undo();
            }
            Tool::Line => {
                self.lines.pop();
            }
        }
    }

    fn can_undo(&self) -> bool {
        match self.tool {
            Tool::Paint => self.map.can_undo(),
            Tool::Line => !self.lines.is_empty(),
        }
    }

    /// The flow of a sample from this source.
    fn flow(&self, source: Source, force: Option<f32>) -> f64 {
        match (source, force) {
            (Source::Pen, Some(force)) => {
                (f64::from(force) * self.brush.pressure_gain).clamp(0.0, 1.0)
            }
            _ => FIXED_FLOW,
        }
    }

    fn screen_sample(&mut self, rect: Rect, pos: Pos2, source: Source, force: Option<f32>) {
        let dir = self.view.unproject(rect, pos);
        let flow = self.flow(source, force);
        let radius = self.brush_radius(rect);
        self.add_sample(dir, flow, radius);
    }

    #[allow(clippy::too_many_arguments)]
    fn on_touch(
        &mut self,
        id: u64,
        phase: egui::TouchPhase,
        pos: Pos2,
        force: Option<f32>,
        on_canvas: bool,
        rect: Rect,
        now: f64,
    ) {
        match phase {
            egui::TouchPhase::Start => {
                let pen = is_pen(force);
                if pen {
                    self.stats.pen_touches += 1;
                } else {
                    self.stats.finger_touches += 1;
                }
                let pen_down = self.touches.values().any(|t| t.kind == TouchKind::Pen);
                let kind = if !on_canvas {
                    TouchKind::Ignored
                } else if pen {
                    if pen_down {
                        TouchKind::Ignored
                    } else {
                        self.reject_fingers(now);
                        self.begin_stroke(Source::Pen, now);
                        TouchKind::Pen
                    }
                } else if pen_down || now - self.last_pen_time < PALM_SECONDS {
                    self.stats.palms_rejected += 1;
                    TouchKind::Ignored
                } else {
                    self.finger_kind(now)
                };
                self.touches.insert(
                    id,
                    Touch {
                        kind,
                        pos,
                        prev: pos,
                    },
                );
                match kind {
                    TouchKind::Pen => self.screen_sample(rect, pos, Source::Pen, force),
                    TouchKind::Draw => self.screen_sample(rect, pos, Source::Finger, None),
                    TouchKind::Nav | TouchKind::Ignored => {}
                }
            }
            egui::TouchPhase::Move => {
                let Some(touch) = self.touches.get_mut(&id) else {
                    return;
                };
                touch.pos = pos;
                match touch.kind {
                    TouchKind::Pen => {
                        self.stats.last_force = force;
                        self.screen_sample(rect, pos, Source::Pen, force);
                    }
                    TouchKind::Draw => self.screen_sample(rect, pos, Source::Finger, None),
                    TouchKind::Nav | TouchKind::Ignored => {}
                }
            }
            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                let Some(touch) = self.touches.remove(&id) else {
                    return;
                };
                match touch.kind {
                    TouchKind::Pen => {
                        self.last_pen_time = now;
                        self.end_stroke(now);
                    }
                    TouchKind::Draw => self.end_stroke(now),
                    TouchKind::Nav | TouchKind::Ignored => {}
                }
                if !self.touches.values().any(|t| t.kind == TouchKind::Nav) {
                    self.nav_restore = None;
                }
            }
        }
    }

    /// The pen came down. Fingers on the canvas are a palm from now on, and a
    /// view change that they made a moment ago is taken back.
    fn reject_fingers(&mut self, now: f64) {
        if let Some((view, time)) = self.nav_restore.take()
            && now - time < PALM_SECONDS * 2.0
        {
            self.view = view;
        }
        if self.stroke.is_some() {
            self.cancel_stroke();
        }
        for touch in self.touches.values_mut() {
            if matches!(touch.kind, TouchKind::Nav | TouchKind::Draw) {
                touch.kind = TouchKind::Ignored;
                self.stats.palms_rejected += 1;
            }
        }
    }

    /// The job of a new finger on the canvas.
    fn finger_kind(&mut self, now: f64) -> TouchKind {
        let drawing = self.touches.values().any(|t| t.kind == TouchKind::Draw);
        let navigating = self.touches.values().any(|t| t.kind == TouchKind::Nav);
        if drawing {
            let young = self
                .stroke
                .as_ref()
                .is_some_and(|s| now - s.started < PALM_SECONDS);
            if !young {
                self.stats.palms_rejected += 1;
                return TouchKind::Ignored;
            }
            // A second finger came down at once: the two fingers navigate.
            self.cancel_stroke();
            for touch in self.touches.values_mut() {
                if touch.kind == TouchKind::Draw {
                    touch.kind = TouchKind::Nav;
                    touch.prev = touch.pos;
                }
            }
            self.nav_restore = Some((self.view, now));
            return TouchKind::Nav;
        }
        if self.finger_draws && !navigating {
            self.begin_stroke(Source::Finger, now);
            return TouchKind::Draw;
        }
        if !navigating {
            self.nav_restore = Some((self.view, now));
        }
        TouchKind::Nav
    }

    /// Moves the view with the fingers: one finger rotates, two fingers
    /// rotate, zoom, and twist.
    fn navigate_with_fingers(&mut self, rect: Rect) {
        let nav: Vec<(Pos2, Pos2)> = self
            .touches
            .values()
            .filter(|t| t.kind == TouchKind::Nav)
            .map(|t| (t.prev, t.pos))
            .collect();
        match nav.as_slice() {
            [(prev, pos)] => {
                if self.one_finger_rotates && prev != pos {
                    self.view.gesture(rect, *prev, *pos, 1.0, 0.0);
                }
            }
            [(prev_a, a), (prev_b, b), ..] => {
                let (old, new) = (*prev_b - *prev_a, *b - *a);
                if old.length() > 1.0 && new.length() > 1.0 {
                    let scale = f64::from(new.length() / old.length());
                    // Screen y points down, so the angle changes sign.
                    let mut twist = f64::from(old.angle() - new.angle());
                    if twist > std::f64::consts::PI {
                        twist -= std::f64::consts::TAU;
                    } else if twist < -std::f64::consts::PI {
                        twist += std::f64::consts::TAU;
                    }
                    let from = *prev_a + old * 0.5;
                    let to = *a + new * 0.5;
                    self.view.gesture(rect, from, to, scale, twist);
                }
            }
            [] => {}
        }
        for touch in self.touches.values_mut() {
            touch.prev = touch.pos;
        }
    }

    fn mouse_input(&mut self, ui: &egui::Ui, rect: Rect, resp: &egui::Response, now: f64) {
        let (pos, pressed, down, other_pressed, other_down, shift, delta, scroll, pinch) = ui
            .input(|i| {
                (
                    i.pointer.latest_pos(),
                    i.pointer.primary_pressed(),
                    i.pointer.primary_down(),
                    i.pointer.button_pressed(egui::PointerButton::Secondary)
                        || i.pointer.button_pressed(egui::PointerButton::Middle),
                    i.pointer.secondary_down() || i.pointer.middle_down(),
                    i.modifiers.shift,
                    i.pointer.delta(),
                    i.smooth_scroll_delta.y,
                    i.zoom_delta(),
                )
            });
        let over = resp.contains_pointer();
        if over && (other_pressed || (pressed && shift)) {
            self.mouse_rotates = true;
        } else if over && pressed {
            self.begin_stroke(Source::Mouse, now);
        }
        if self.mouse_rotates {
            if let (true, Some(pos)) = (down || other_down, pos) {
                if delta != egui::Vec2::ZERO {
                    self.view.gesture(rect, pos - delta, pos, 1.0, 0.0);
                }
            } else {
                self.mouse_rotates = false;
            }
        } else if self.stroke.is_some() {
            if let (true, Some(pos)) = (down, pos) {
                self.screen_sample(rect, pos, Source::Mouse, None);
            } else {
                self.end_stroke(now);
            }
        }
        self.cursor = None;
        if let Some(hover) = resp.hover_pos() {
            let factor = (f64::from(scroll) * 0.002).exp() * f64::from(pinch);
            if (factor - 1.0).abs() > 1e-4 {
                self.view.zoom_at(rect, hover, factor);
            }
            self.cursor = self.view.unproject(rect, hover);
        }
    }

    fn handle_input(&mut self, ui: &egui::Ui, rect: Rect, resp: &egui::Response, now: f64) {
        let events = ui.input(|i| i.events.clone());
        let layer = ui.layer_id();
        let mut moves = 0;
        let mut touched = false;
        for event in &events {
            match event {
                egui::Event::Touch {
                    id,
                    phase,
                    pos,
                    force,
                    ..
                } => {
                    self.touch_mode = true;
                    touched = true;
                    let on_canvas = rect.contains(*pos)
                        && ui.ctx().layer_id_at(*pos).is_none_or(|top| top == layer);
                    if *phase == egui::TouchPhase::Move {
                        moves += 1;
                    }
                    self.on_touch(id.0, *phase, *pos, *force, on_canvas, rect, now);
                }
                // A pointer that moves with no touch on the screen is a hover.
                egui::Event::PointerMoved(_)
                    if self.touch_mode && !touched && self.touches.is_empty() =>
                {
                    self.stats.hover_seen = true;
                }
                _ => {}
            }
        }
        self.stats.max_moves_per_frame = self.stats.max_moves_per_frame.max(moves);
        if self.touch_mode {
            self.navigate_with_fingers(rect);
            if self.stroke.is_none() {
                self.cursor = None;
            }
        } else {
            self.mouse_input(ui, rect, resp, now);
        }
    }

    fn uniforms(&self, rect: Rect, pixels_per_point: f32) -> Uniforms {
        let row = |r: V3| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0];
        let center = rect.center();
        let radius = self.view.radius(rect) as f32;
        let mut limits = [[0.0; 4]; MAX_BANDS];
        let mut colors = [[0.0; 4]; MAX_BANDS];
        for (i, (limit, rgb)) in BANDS.iter().enumerate() {
            limits[i][0] = ((limit - ELEV_MIN) / (ELEV_MAX - ELEV_MIN)) as f32;
            colors[i] = [
                f32::from(rgb[0]) / 255.0,
                f32::from(rgb[1]) / 255.0,
                f32::from(rgb[2]) / 255.0,
                1.0,
            ];
        }
        let show_cursor = self.tool == Tool::Paint && self.cursor.is_some();
        let cursor = self.cursor.unwrap_or([0.0, 0.0, 1.0]);
        let graticule_degrees: f64 = if self.view.zoom < 3.0 {
            15.0
        } else if self.view.zoom < 12.0 {
            5.0
        } else {
            1.0
        };
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        Uniforms {
            rot: [
                row(self.view.rot.0[0]),
                row(self.view.rot.0[1]),
                row(self.view.rot.0[2]),
            ],
            globe: [
                center.x * pixels_per_point,
                center.y * pixels_per_point,
                radius * pixels_per_point,
                pixels_per_point,
            ],
            brush: [
                cursor[0] as f32,
                cursor[1] as f32,
                cursor[2] as f32,
                self.brush_radius(rect) as f32,
            ],
            flags: [
                self.brush.hardness as f32,
                flag(show_cursor),
                flag(self.stepped),
                flag(self.graticule),
            ],
            params: [
                self.map.face_size() as f32,
                BANDS.len() as f32,
                graticule_degrees.to_radians() as f32,
                0.0,
            ],
            limits,
            colors,
        }
    }

    fn draw_lines(&self, painter: &egui::Painter, rect: Rect) {
        let stroke = egui::Stroke::new(2.5, LINE_COLOR);
        let live = self
            .stroke
            .as_ref()
            .filter(|s| s.tool == Tool::Line)
            .map(|s| s.raw_line.as_slice());
        for line in self.lines.iter().map(Vec::as_slice).chain(live) {
            let mut run: Vec<Pos2> = Vec::new();
            for &dir in line {
                match self.view.project(rect, dir) {
                    Some(pos) => run.push(pos),
                    None => {
                        if run.len() >= 2 {
                            painter.add(egui::Shape::line(std::mem::take(&mut run), stroke));
                        }
                        run.clear();
                    }
                }
            }
            if run.len() >= 2 {
                painter.add(egui::Shape::line(run, stroke));
            }
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        let (now, dt) = ui.input(|i| (i.time, i.unstable_dt));
        if self.was_active {
            if self.stats.frame_ms.len() == 240 {
                self.stats.frame_ms.pop_front();
            }
            self.stats.frame_ms.push_back(dt * 1000.0);
        }
        self.stats.stamps = 0;
        self.stats.stamp_ms = 0.0;
        self.handle_input(ui, rect, &resp, now);
        self.stats.stamp_ms_peak = self.stats.stamp_ms_peak.max(self.stats.stamp_ms);
        if !self.brush.lock_km {
            self.brush.size_km = self.brush_radius(rect) * self.world_radius_km;
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BACKGROUND);
        let mut uploads = Vec::new();
        for (face, rect) in self.map.take_dirty().into_iter().enumerate() {
            let Some(rect) = rect else {
                continue;
            };
            let mut data = Vec::new();
            self.map.read_rect(face, rect, &mut data);
            uploads.push(Upload {
                face: face as u32,
                x: rect.x0 as u32,
                y: rect.y0 as u32,
                width: (rect.x1 - rect.x0) as u32,
                height: (rect.y1 - rect.y0) as u32,
                data,
            });
        }
        self.stats.upload_texels = uploads.iter().map(|u| u.data.len()).sum();
        let face_size = self.map.face_size() as u32;
        self.uploads
            .lock()
            .expect("no panic holds the lock")
            .extend(uploads.into_iter().map(|u| (face_size, u)));
        painter.add(egui_wgpu::Callback::new_paint_callback(
            rect,
            GlobeCallback {
                format: self.format,
                face_size: self.map.face_size() as u32,
                uniforms: self.uniforms(rect, ui.ctx().pixels_per_point()),
                uploads: self.uploads.clone(),
            },
        ));
        self.draw_lines(&painter, rect);

        self.was_active = self.continuous
            || self.stroke.is_some()
            || self.mouse_rotates
            || !self.touches.is_empty();
        if self.was_active {
            ui.ctx().request_repaint();
        }
    }

    fn tools_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Paint, "Paint");
            ui.selectable_value(&mut self.tool, Tool::Line, "Line");
            ui.separator();
            if ui
                .add_enabled(self.can_undo(), egui::Button::new("Undo"))
                .clicked()
            {
                self.undo();
            }
        });
        if self.tool == Tool::Paint {
            ui.horizontal(|ui| {
                for (mode, name) in [
                    (Mode::Raise, "Raise"),
                    (Mode::Lower, "Lower"),
                    (Mode::Smooth, "Smooth"),
                    (Mode::Flatten, "Flatten"),
                ] {
                    ui.selectable_value(&mut self.brush.mode, mode, name);
                }
            });
            if self.brush.lock_km {
                let max = MAX_BRUSH_RADIUS * self.world_radius_km;
                ui.add(
                    egui::Slider::new(&mut self.brush.size_km, max / 400.0..=max)
                        .logarithmic(true)
                        .fixed_decimals(0)
                        .suffix(" km")
                        .text("Size"),
                );
            } else {
                ui.add(
                    egui::Slider::new(&mut self.brush.size_points, 4.0..=160.0)
                        .fixed_decimals(0)
                        .suffix(" pt")
                        .text("Size"),
                );
            }
            ui.checkbox(
                &mut self.brush.lock_km,
                format!("Lock size at {:.0} km", self.brush.size_km),
            );
            ui.add(egui::Slider::new(&mut self.brush.hardness, 0.0..=0.95).text("Hardness"));
            ui.add(
                egui::Slider::new(&mut self.brush.strength_m, 100.0..=8000.0)
                    .logarithmic(true)
                    .fixed_decimals(0)
                    .suffix(" m")
                    .text("Strength"),
            );
            ui.add(egui::Slider::new(&mut self.brush.pressure_gain, 0.5..=6.0).text("Pen gain"));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.stepped, "Stepped tints");
            ui.checkbox(&mut self.graticule, "Graticule");
        });
        ui.checkbox(&mut self.one_finger_rotates, "One finger rotates");
        ui.checkbox(&mut self.finger_draws, "Finger draws");
        ui.horizontal(|ui| {
            if ui.button("Reset view").clicked() {
                self.view = GlobeView::centered(15.0, 25.0);
            }
            if ui.button("North pole").clicked() {
                self.view = GlobeView::centered(15.0, 89.9);
            }
            if ui.button("Clear").clicked() {
                self.map.fill(meters_to_level(START_ELEVATION));
                self.lines.clear();
            }
        });
        ui.horizontal(|ui| {
            ui.label("World radius");
            ui.add(
                egui::DragValue::new(&mut self.world_radius_km)
                    .range(100.0..=100_000.0)
                    .speed(10.0)
                    .suffix(" km"),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Face");
            let current = self.map.face_size();
            for size in [512, 1024, 2048] {
                if ui
                    .selectable_label(current == size, size.to_string())
                    .clicked()
                    && current != size
                {
                    self.map = Heightmap::new(size, meters_to_level(START_ELEVATION));
                }
            }
            ui.label(format!("{:.1} km per texel", self.texel_km()));
        });
    }

    /// The text of the diagnostics panel, one fact on each line.
    pub fn report(&self) -> String {
        let s = &self.stats;
        let mut sorted: Vec<f32> = s.frame_ms.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        let pick = |q: f32| {
            sorted
                .get(((sorted.len() as f32 - 1.0) * q) as usize)
                .copied()
                .unwrap_or(0.0)
        };
        let mut out = String::new();
        let mut line = |text: String| {
            out.push_str(&text);
            out.push('\n');
        };
        line(format!(
            "Input         {}",
            s.last_source.map_or("none yet", Source::name)
        ));
        line(format!(
            "Pen force     {}",
            s.last_force
                .map_or("none".to_owned(), |f| format!("{f:.3} of 1"))
        ));
        line(format!(
            "Touches       {} pen, {} finger",
            s.pen_touches, s.finger_touches
        ));
        line(format!("Palms ignored {}", s.palms_rejected));
        line(format!(
            "Hover         {}",
            if s.hover_seen { "seen" } else { "not seen" }
        ));
        line(format!(
            "Last stroke   {} samples, {:.0} per second",
            s.stroke_samples, s.stroke_rate
        ));
        line(format!("Moves/frame   {} at most", s.max_moves_per_frame));
        line(format!(
            "Frame         {:.1} ms median, {:.1} ms worst",
            pick(0.5),
            pick(1.0)
        ));
        line(format!(
            "Brush on CPU  {:.2} ms now, {:.2} ms peak",
            s.stamp_ms, s.stamp_ms_peak
        ));
        line(format!(
            "Stamps        {} this frame, {} texels to GPU",
            s.stamps, s.upload_texels
        ));
        if let Some(dir) = self.cursor {
            let (lon, lat) = dir_to_lonlat(dir);
            line(format!(
                "Cursor        {lon:.2}, {lat:.2}, {:.0} m",
                level_to_meters(self.map.sample(dir))
            ));
        }
        out
    }

    fn stats_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(self.report()).monospace());
        ui.label("Flow of the last stroke");
        let (rect, _) = ui.allocate_exact_size(egui::vec2(300.0, 48.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
        let trace = &self.stats.pressure_trace;
        if trace.len() >= 2 {
            let points: Vec<Pos2> = trace
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    Pos2::new(
                        rect.left() + rect.width() * i as f32 / (trace.len() - 1) as f32,
                        rect.bottom() - 2.0 - (rect.height() - 4.0) * p,
                    )
                })
                .collect();
            let color = ui.visuals().selection.bg_fill;
            painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, color)));
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.continuous, "Draw every frame");
            if ui.button("Reset numbers").clicked() {
                self.stats = Stats::default();
            }
        });
    }

    /// Draws the canvas and the floating panels.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.canvas(ui));
        let ctx = ui.ctx().clone();
        egui::Window::new("Tools")
            .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
            .resizable(false)
            .show(&ctx, |ui| self.tools_panel(ui));
        egui::Window::new("Pen and timing")
            .anchor(egui::Align2::RIGHT_TOP, [-12.0, 12.0])
            .resizable(false)
            .show(&ctx, |ui| self.stats_panel(ui));
    }
}

impl eframe::App for ProtoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}
