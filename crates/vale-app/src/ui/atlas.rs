//! The canvas of the Atlas workspace: the open page and its map frames.
//! The iPad layout and the desktop layout use the same canvas.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use eframe::egui::{self, Color32, Pos2, Rect, pos2, vec2};
use kurbo::{Point, Vec2};
use vale_sphere::Projection;
use vale_store::{FrameId, FrameRecord, FrameShape, PageId, PageRecord};

use super::{AppState, LayerRef};
use crate::atlas::{self as math, FrameTarget, Handle, Vertex};
use crate::pipeline::Quality;
use crate::view::ViewTransform;

/// The side of the square that takes the touch of a handle.
pub const HANDLE_TARGET: f32 = 44.0;
/// The radius of the circle of a handle.
const HANDLE_RADIUS: f32 = 6.0;
/// A wheel zoom is part of one gesture until the wheel rests this long, in
/// seconds.
const WHEEL_REST: f64 = 0.25;
/// Two taps less than this time apart are a double tap, in seconds.
const DOUBLE_TAP: f64 = 0.4;
const PAPER: Color32 = Color32::WHITE;
const ACCENT: Color32 = Color32::from_rgb(69, 196, 168);

/// The thing that the panel of the Atlas workspace shows.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum AtlasSelection {
    #[default]
    None,
    Page,
    Frame(FrameId),
}

/// The tool of a pen, a finger, and the mouse on the page.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum AtlasTool {
    /// A tap selects, and a drag moves a frame or a handle.
    #[default]
    Select,
    /// A drag moves the map in a frame.
    Move,
}

/// A tab of the panel of a frame.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum FrameTab {
    #[default]
    Frame,
    Projection,
}

/// The place of the page on the screen.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PagePlace {
    pub rect: Rect,
    pub pts_per_mm: f64,
}

impl PagePlace {
    /// The page at its true proportions, as large as fits in `free`, in the
    /// middle of it.
    pub fn fit(page: &PageRecord, free: Rect) -> PagePlace {
        let (w, h) = (page.width_mm.max(1.0), page.height_mm.max(1.0));
        let scale = (f64::from(free.width()) / w)
            .min(f64::from(free.height()) / h)
            .max(1e-3);
        let size = vec2((w * scale) as f32, (h * scale) as f32);
        PagePlace {
            rect: Rect::from_center_size(free.center(), size),
            pts_per_mm: scale,
        }
    }

    pub fn target(&self) -> FrameTarget {
        FrameTarget {
            origin: Point::new(f64::from(self.rect.min.x), f64::from(self.rect.min.y)),
            pts_per_mm: self.pts_per_mm,
        }
    }

    /// A screen position to page millimeters.
    pub fn to_mm(&self, p: Pos2) -> Point {
        self.target()
            .from_target(Point::new(f64::from(p.x), f64::from(p.y)))
    }

    /// Page millimeters to a screen position.
    pub fn to_screen(&self, p: Point) -> Pos2 {
        let q = self.target().to_target(p);
        pos2(q.x as f32, q.y as f32)
    }

    /// A box in page millimeters to the screen.
    fn rect_to_screen(&self, r: kurbo::Rect) -> Rect {
        Rect::from_min_max(
            self.to_screen(Point::new(r.x0, r.y0)),
            self.to_screen(Point::new(r.x1, r.y1)),
        )
    }
}

/// The last compose of a frame.
struct FrameImage {
    texture: egui::TextureHandle,
    /// The key of what the frame shows.
    content: u64,
    /// The key of the place of the frame and of its view.
    place: u64,
    /// The view of the frame on the screen at the compose.
    vt: ViewTransform,
    /// The texture on the screen at the compose.
    rect: Rect,
    /// The compose was at the final quality.
    settled: bool,
}

/// What a drag on the page changes.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Drag {
    /// A handle of a frame, with the distance from the pointer to the handle
    /// in millimeters.
    Handle(FrameId, Handle, Vec2),
    /// The whole frame.
    Body(FrameId),
    /// The map in the frame.
    Pan(FrameId),
    /// The map in the frame turns about the anchor. The angle of the pointer
    /// in the last frame, in degrees.
    Rotate(FrameId, f64),
    /// Nothing.
    Idle,
}

pub struct AtlasState {
    /// The open page.
    pub page: PageId,
    pub selection: AtlasSelection,
    /// The selected vertex of the selected polygon frame.
    pub vertex: Option<Vertex>,
    pub tool: AtlasTool,
    pub tab: FrameTab,
    /// The list of the page sizes of the panel of the page is open.
    pub sizes: bool,
    /// The Add menu is open.
    pub add: bool,
    /// The Add menu in the last frame.
    pub add_rect: Option<Rect>,
    /// The panel of the selection in the last frame.
    pub panel_rect: Option<Rect>,
    /// The page on the screen in the last frame.
    pub place: Option<PagePlace>,
    /// The handles of the selected frame in the last frame, with their
    /// touch areas.
    pub handles: Vec<(Handle, Rect)>,
    /// The count of the frame composes.
    pub composes: u64,
    /// The panel of this selection and tab showed in the last frame.
    pub(crate) shown: Option<(AtlasSelection, FrameTab)>,
    /// The frame of `handles`.
    handles_of: Option<FrameId>,
    images: HashMap<FrameId, FrameImage>,
    drag: Option<Drag>,
    /// The pointer at the last step of the drag.
    drag_last: Pos2,
    /// The frame of the two-finger gesture.
    pinch: Option<FrameId>,
    /// More than one finger touched the screen since the first finger came
    /// down. A drag of one finger then does nothing.
    fingers: bool,
    /// The time of the last wheel zoom.
    wheel_at: Option<f64>,
    /// The time and the place of the last tap.
    last_tap: Option<(f64, Pos2)>,
}

impl Default for AtlasState {
    fn default() -> AtlasState {
        AtlasState {
            page: PageId(0),
            selection: AtlasSelection::None,
            vertex: None,
            tool: AtlasTool::Select,
            tab: FrameTab::Frame,
            sizes: false,
            add: false,
            add_rect: None,
            panel_rect: None,
            place: None,
            handles: Vec::new(),
            composes: 0,
            shown: None,
            handles_of: None,
            images: HashMap::new(),
            drag: None,
            drag_last: Pos2::ZERO,
            pinch: None,
            fingers: false,
            wheel_at: None,
            last_tap: None,
        }
    }
}

impl AtlasState {
    /// The number of frames with a texture.
    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    /// True if the frame has a texture.
    pub fn has_image(&self, id: FrameId) -> bool {
        self.images.contains_key(&id)
    }

    /// The selected frame.
    pub fn frame(&self) -> Option<FrameId> {
        match self.selection {
            AtlasSelection::Frame(id) => Some(id),
            _ => None,
        }
    }
}

/// Makes the open page, the selection, and the selected vertex valid for
/// the atlas of the document.
pub fn sync(state: &mut AppState) {
    let atlas = &state.doc.project.atlas;
    let a = &mut state.atlas;
    if atlas.page(a.page).is_none() {
        a.page = atlas.pages[0].id;
        a.selection = AtlasSelection::None;
    }
    let page = atlas.page(a.page).expect("the open page is in the atlas");
    let frame = a
        .frame()
        .and_then(|id| page.frames.iter().find(|f| f.id == id));
    if a.frame().is_some() && frame.is_none() {
        a.selection = AtlasSelection::None;
    }
    let vertex_ok = |(r, i): Vertex| match frame.map(|f| &f.shape) {
        Some(FrameShape::Polygon(rings)) => rings.get(r).is_some_and(|ring| i < ring.len()),
        _ => false,
    };
    if a.vertex.is_some_and(|v| !vertex_ok(v)) {
        a.vertex = None;
    }
    a.images.retain(|id, _| atlas.frame(*id).is_some());
}

/// The place of the open page in the atlas order, from 0, and the number
/// of pages.
pub fn page_index(state: &AppState) -> (usize, usize) {
    let atlas = &state.doc.project.atlas;
    (
        atlas.page_index(state.atlas.page).unwrap_or(0),
        atlas.pages.len(),
    )
}

/// The open page.
pub fn open_page(state: &AppState) -> &PageRecord {
    let atlas = &state.doc.project.atlas;
    atlas.page(state.atlas.page).unwrap_or(&atlas.pages[0])
}

/// Opens the page before (`-1`) or after (`1`) the open page.
pub fn go_to_page(state: &mut AppState, step: isize) {
    let (index, count) = page_index(state);
    let next = index as isize + step;
    if (0..count as isize).contains(&next) {
        state.atlas.page = state.doc.project.atlas.pages[next as usize].id;
        if state.atlas.frame().is_some() {
            state.atlas.selection = AtlasSelection::None;
        }
        state.atlas.vertex = None;
    }
}

/// Adds a frame of the map of the project to the open page and selects
/// it.
pub fn add_frame(state: &mut AppState) -> Option<FrameId> {
    let page = open_page(state).clone();
    let map_id = state.doc.map_id();
    let map = state.doc.project.atlas.map(map_id)?.clone();
    let projection = match state.pipeline.projection(&state.doc) {
        Ok(p) => p,
        Err(e) => {
            state.status = format!("{e:#}");
            return None;
        }
    };
    let frame = math::new_frame(&page, &map, projection);
    let id = state.doc.project.atlas.add_frame(page.id, frame)?;
    state.atlas.selection = AtlasSelection::Frame(id);
    state.atlas.vertex = None;
    state.atlas.tab = FrameTab::Frame;
    Some(id)
}

/// Changes a frame with the projection of its map. The frame changes only
/// if `edit` returns true. Returns that value.
pub fn edit_frame(
    state: &mut AppState,
    id: FrameId,
    edit: impl FnOnce(&mut FrameRecord, &Projection) -> bool,
) -> bool {
    let Some(mut frame) = state.doc.project.atlas.frame(id).cloned() else {
        return false;
    };
    let Ok(projection) = state.pipeline.projection(&state.doc) else {
        return false;
    };
    if !edit(&mut frame, projection) {
        return false;
    }
    if let Some(slot) = state.doc.project.atlas.frame_mut(id) {
        *slot = frame;
    }
    true
}

/// The top frame of a page that holds a point in millimeters.
fn frame_at(page: &PageRecord, p: Point) -> Option<FrameId> {
    let mut frames = page.frames.iter().rev();
    frames.find(|f| math::contains(&f.shape, p)).map(|f| f.id)
}

/// The handle of the selected frame whose touch area holds a screen
/// position. The nearest handle wins.
fn handle_at(state: &AppState, pos: Pos2) -> Option<(Handle, Pos2)> {
    if state.atlas.handles_of.is_none() || state.atlas.handles_of != state.atlas.frame() {
        return None;
    }
    let near = state.atlas.handles.iter().filter(|(_, r)| r.contains(pos));
    let best = near.min_by(|(_, a), (_, b)| {
        let (da, db) = (a.center().distance(pos), b.center().distance(pos));
        da.total_cmp(&db)
    });
    best.map(|(h, r)| (*h, r.center()))
}

fn hash_of(value: impl std::fmt::Debug) -> u64 {
    let mut hasher = DefaultHasher::new();
    format!("{value:?}").hash(&mut hasher);
    hasher.finish()
}

/// Draws the open page in the middle of `free` and takes the touches, the
/// pen, and the mouse in `area`.
pub fn canvas(ui: &mut egui::Ui, state: &mut AppState, area: Rect, free: Rect, back: Color32) {
    sync(state);
    let resp = ui.interact(
        area,
        ui.id().with("atlas-canvas"),
        egui::Sense::click_and_drag(),
    );
    state.canvas_size = (f64::from(area.width()), f64::from(area.height()));
    // The pen queue of the globe is not for the page.
    if let Some(queue) = &state.globe.pen.queue {
        queue.take();
    }
    let place = PagePlace::fit(open_page(state), free);
    state.atlas.place = Some(place);
    input(ui, state, &resp, place);
    sync(state);

    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, back);
    let shadow = egui::Shadow {
        offset: [0, 4],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(90),
    };
    painter.add(shadow.as_shape(place.rect, 0.0));
    painter.rect_filled(place.rect, 0.0, PAPER);
    frames(ui, state, place, area);
    selection(ui, state, place);
}

/// True while a gesture moves the maps, so the frames show their last
/// textures.
fn gesture(ui: &egui::Ui, state: &AppState) -> bool {
    let (down, fingers, now) =
        ui.input(|i| (i.pointer.any_down(), i.multi_touch().is_some(), i.time));
    let wheel = state.atlas.wheel_at.is_some_and(|t| now - t < WHEEL_REST);
    down || fingers || wheel
}

fn input(ui: &egui::Ui, state: &mut AppState, resp: &egui::Response, place: PagePlace) {
    let page = open_page(state).clone();
    let (any_down, now, alt) = ui.input(|i| (i.pointer.any_down(), i.time, i.modifiers.alt));
    let fingers = ui.input(|i| i.multi_touch());
    if fingers.is_some() {
        state.atlas.fingers = true;
    } else {
        state.atlas.pinch = None;
        if !any_down {
            state.atlas.fingers = false;
        }
    }

    // Two fingers move, zoom, and turn the map of the frame below them.
    if let Some(touch) = fingers {
        let pinch = match state.atlas.pinch {
            Some(id) => Some(id),
            None if resp.rect.contains(touch.center_pos) => {
                frame_at(&page, place.to_mm(touch.center_pos))
            }
            None => None,
        };
        state.atlas.pinch = pinch;
        state.atlas.drag = None;
        if let Some(id) = pinch {
            let at = place.to_mm(touch.center_pos);
            let d = touch.translation_delta;
            let d = Vec2::new(f64::from(d.x), f64::from(d.y)) / place.pts_per_mm;
            let factor = f64::from(touch.zoom_delta);
            let degrees = f64::from(touch.rotation_delta).to_degrees();
            edit_frame(state, id, |frame, projection| {
                let panned = d != Vec2::ZERO && math::pan(frame, projection, d);
                let zoomed =
                    (factor - 1.0).abs() > 1e-6 && math::zoom(frame, projection, at, factor);
                let turned = degrees.abs() > 1e-6 && math::rotate(frame, projection, at, degrees);
                panned || zoomed || turned
            });
        }
    }

    // The wheel and a pinch of the trackpad zoom about the pointer.
    if fingers.is_none()
        && let Some(pos) = resp.hover_pos()
    {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (f64::from(scroll) * 0.002).exp() * f64::from(pinch);
        let at = place.to_mm(pos);
        if (factor - 1.0).abs() > 1e-4
            && let Some(id) = frame_at(&page, at)
            && edit_frame(state, id, |f, p| math::zoom(f, p, at, factor))
        {
            state.atlas.wheel_at = Some(now);
        }
        state.cursor_lonlat = frame_at(&page, at).and_then(|id| {
            let frame = page.frames.iter().find(|f| f.id == id)?;
            let projection = state.pipeline.projection(&state.doc).ok()?;
            let vt = math::frame_transform(projection, frame, place.target());
            let q = Point::new(f64::from(pos.x), f64::from(pos.y));
            projection.inverse(vt.from_page(q))
        });
    } else {
        state.cursor_lonlat = None;
    }
    if let Some(at) = state.atlas.wheel_at
        && now - at < WHEEL_REST
    {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(WHEEL_REST));
    }

    if resp.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin());
        state.atlas.drag = origin.map(|pos| start_drag(state, &page, place, pos, alt));
        state.atlas.drag_last = origin.unwrap_or_default();
    }
    if resp.dragged()
        && fingers.is_none()
        && !state.atlas.fingers
        && let Some(drag) = state.atlas.drag
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let d = pos - std::mem::replace(&mut state.atlas.drag_last, pos);
        let d = Vec2::new(f64::from(d.x), f64::from(d.y)) / place.pts_per_mm;
        drag_to(state, drag, place, pos, d);
    }
    if resp.drag_stopped() || !any_down {
        state.atlas.drag = None;
    }

    // A second quick tap on a vertex removes it.
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let last = state.atlas.last_tap.replace((now, pos));
        let again = last.is_some_and(|(t, p)| now - t < DOUBLE_TAP && p.distance(pos) < 12.0);
        match handle_at(state, pos) {
            Some((Handle::Vertex(v), _)) if again && state.atlas.vertex == Some(v) => {
                remove_vertex(state, v);
                state.atlas.last_tap = None;
            }
            _ => tap(state, &page, place, pos),
        }
    }

    let typing = ui.ctx().memory(|m| m.focused().is_some());
    let delete =
        ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
    if delete
        && !typing
        && state.atlas.tool == AtlasTool::Select
        && let Some(v) = state.atlas.vertex
    {
        remove_vertex(state, v);
    }
}

/// Removes the selected vertex of the selected frame.
pub fn remove_vertex(state: &mut AppState, v: Vertex) -> bool {
    let Some(id) = state.atlas.frame() else {
        return false;
    };
    let removed = edit_frame(state, id, |f, _| math::remove_vertex(&mut f.shape, v));
    if removed {
        state.atlas.vertex = None;
    }
    removed
}

fn start_drag(
    state: &mut AppState,
    page: &PageRecord,
    place: PagePlace,
    pos: Pos2,
    alt: bool,
) -> Drag {
    if state.atlas.fingers {
        return Drag::Idle;
    }
    let at = place.to_mm(pos);
    let under = frame_at(page, at);
    match state.atlas.tool {
        AtlasTool::Move => match under {
            Some(id) if alt => {
                let frame = page.frames.iter().find(|f| f.id == id);
                let anchor = frame.map_or(at, |f| math::anchor(&f.shape));
                Drag::Rotate(id, angle(anchor, at))
            }
            Some(id) => Drag::Pan(id),
            None => Drag::Idle,
        },
        AtlasTool::Select => {
            if let (Some(id), Some((handle, center))) = (state.atlas.frame(), handle_at(state, pos))
            {
                let grab = place.to_mm(center) - at;
                let handle = match handle {
                    Handle::Edge(v) => {
                        let mut new = None;
                        edit_frame(state, id, |f, _| {
                            new = math::insert_vertex(&mut f.shape, v);
                            new.is_some()
                        });
                        match new {
                            Some(v) => Handle::Vertex(v),
                            None => return Drag::Idle,
                        }
                    }
                    other => other,
                };
                if let Handle::Vertex(v) = handle {
                    state.atlas.vertex = Some(v);
                }
                return Drag::Handle(id, handle, grab);
            }
            match under {
                Some(id) => {
                    if state.atlas.frame() != Some(id) {
                        state.atlas.vertex = None;
                    }
                    state.atlas.selection = AtlasSelection::Frame(id);
                    Drag::Body(id)
                }
                None => Drag::Idle,
            }
        }
    }
}

/// The angle of a point about a center, in degrees clockwise on the page.
fn angle(center: Point, p: Point) -> f64 {
    (p.y - center.y).atan2(p.x - center.x).to_degrees()
}

fn drag_to(state: &mut AppState, drag: Drag, place: PagePlace, pos: Pos2, d: Vec2) {
    let at = place.to_mm(pos);
    match drag {
        Drag::Handle(id, handle, grab) => {
            let to = at + grab;
            edit_frame(state, id, |f, _| match handle {
                Handle::Resize(i) => math::resize(&mut f.shape, i, to),
                Handle::Vertex(v) => math::move_vertex(&mut f.shape, v, to),
                Handle::Edge(_) => false,
            });
        }
        Drag::Body(id) => {
            if d != Vec2::ZERO {
                edit_frame(state, id, |f, _| {
                    math::move_shape(&mut f.shape, d);
                    true
                });
            }
        }
        Drag::Pan(id) => {
            if d != Vec2::ZERO {
                edit_frame(state, id, |f, p| math::pan(f, p, d));
            }
        }
        Drag::Rotate(id, last) => {
            let Some(anchor) = state
                .doc
                .project
                .atlas
                .frame(id)
                .map(|f| math::anchor(&f.shape))
            else {
                return;
            };
            let now = angle(anchor, at);
            let degrees = (now - last + 180.0).rem_euclid(360.0) - 180.0;
            edit_frame(state, id, |f, p| math::rotate(f, p, anchor, degrees));
            state.atlas.drag = Some(Drag::Rotate(id, now));
        }
        Drag::Idle => {}
    }
}

/// A tap selects a vertex of the selected frame, a frame, the page, or
/// nothing. A tap on a frame also selects the feature below it.
fn tap(state: &mut AppState, page: &PageRecord, place: PagePlace, pos: Pos2) {
    if state.atlas.tool == AtlasTool::Select
        && let Some((handle, _)) = handle_at(state, pos)
    {
        if let Handle::Vertex(v) = handle {
            state.atlas.vertex = Some(v);
        }
        return;
    }
    let at = place.to_mm(pos);
    state.atlas.vertex = None;
    state.atlas.sizes = false;
    let picked = match frame_at(page, at) {
        Some(id) => {
            state.atlas.selection = AtlasSelection::Frame(id);
            let frame = page.frames.iter().find(|f| f.id == id).cloned();
            let q = Point::new(f64::from(pos.x), f64::from(pos.y));
            frame.and_then(|f| {
                let target = place.target();
                state
                    .pipeline
                    .pick_frame(&state.doc, &f, target, q)
                    .ok()
                    .flatten()
            })
        }
        None if place.rect.contains(pos) => {
            state.atlas.selection = AtlasSelection::Page;
            None
        }
        None => {
            state.atlas.selection = AtlasSelection::None;
            None
        }
    };
    if picked != state.selection {
        state.selection = picked;
        if let Some(sel) = picked {
            state.select(Some(LayerRef::Vector(sel.layer)));
        }
    }
}

/// Composes the frames that changed and paints each frame of the page.
fn frames(ui: &egui::Ui, state: &mut AppState, place: PagePlace, area: Rect) {
    let page = open_page(state).clone();
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let moving = gesture(ui, state);
    let primary = state.atlas.frame().or(page.frames.first().map(|f| f.id));
    let target = place.target();
    let painter = ui.painter_at(area);
    for frame in &page.frames {
        let Some(map) = state.doc.project.atlas.map(frame.map) else {
            continue;
        };
        let content = hash_of((
            map,
            state.map_generation,
            frame.map,
            frame.graticule,
            ppp.to_bits(),
            state.selection,
        ));
        let place_key = hash_of((
            &frame.shape,
            frame.center,
            frame.scale,
            frame.rotation,
            target,
        ));
        let Ok(projection) = state.pipeline.projection(&state.doc) else {
            continue;
        };
        let vt = math::frame_transform(projection, frame, target);
        let fresh = match state.atlas.images.get(&frame.id) {
            None => false,
            Some(image) if image.content != content => false,
            Some(image) if image.place != place_key || !image.settled => moving,
            Some(_) => true,
        };
        if !fresh {
            let quality = if moving && !state.headless {
                Quality::Interactive
            } else {
                Quality::Final
            };
            let keys = (content, place_key);
            let primary = primary == Some(frame.id);
            compose(&ctx, state, &page, frame, place, vt, keys, quality, primary);
        }
        if let Some(image) = state.atlas.images.get(&frame.id) {
            let clip = place.rect_to_screen(math::shape_bbox(&frame.shape));
            paint_image(&painter, image, &vt, clip.intersect(area));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compose(
    ctx: &egui::Context,
    state: &mut AppState,
    page: &PageRecord,
    frame: &FrameRecord,
    place: PagePlace,
    vt: ViewTransform,
    (content, place_key): (u64, u64),
    quality: Quality,
    primary: bool,
) {
    let ppp = ctx.pixels_per_point();
    let ppm = place.pts_per_mm;
    let bbox = math::shape_bbox(&frame.shape);
    let screen = place.rect_to_screen(bbox);
    let px = |points: f64| ((points * f64::from(ppp)).ceil() as usize).clamp(1, 8192);
    let (w, h) = (px(bbox.width() * ppm), px(bbox.height() * ppm));
    // The list starts at the top-left corner of the box of the frame.
    let target = FrameTarget {
        origin: Point::new(-bbox.x0 * ppm, -bbox.y0 * ppm),
        pts_per_mm: ppm,
    };
    let selection = state.selection;
    let composed = state.pipeline.compose_frame(
        &state.doc,
        page,
        frame,
        target,
        &mut state.fonts,
        quality,
        selection,
    );
    state.atlas.composes += 1;
    let mut composed = match composed {
        Ok(c) => c,
        Err(e) => {
            state.status = format!("{e:#}");
            return;
        }
    };
    composed.list.width = w as f64 / f64::from(ppp);
    composed.list.height = h as f64 / f64::from(ppp);
    let img = match vale_render::render_rgba(&composed.list, f64::from(ppp)) {
        Ok(img) => img,
        Err(e) => {
            state.status = format!("{e}");
            return;
        }
    };
    let image = egui::ColorImage::from_rgba_premultiplied([img.width, img.height], &img.data);
    let options = egui::TextureOptions::LINEAR;
    let texture = match state.atlas.images.remove(&frame.id) {
        Some(mut old) => {
            old.texture.set(image, options);
            old.texture
        }
        None => ctx.load_texture(format!("frame-{}", frame.id.0), image, options),
    };
    let size = vec2(img.width as f32, img.height as f32) / ppp;
    let image = FrameImage {
        texture,
        content,
        place: place_key,
        vt,
        rect: Rect::from_min_size(screen.min, size),
        settled: quality == Quality::Final,
    };
    state.atlas.images.insert(frame.id, image);
    if primary {
        state.composed = Some(composed);
    }
}

/// Paints the texture of a frame. While a gesture moves the map, the
/// texture moves with the view.
fn paint_image(painter: &egui::Painter, image: &FrameImage, vt: &ViewTransform, clip: Rect) {
    let moved = |p: Pos2| {
        let q = Point::new(f64::from(p.x), f64::from(p.y));
        let q = vt.to_page(image.vt.from_page(q));
        pos2(q.x as f32, q.y as f32)
    };
    let r = image.rect;
    let mut mesh = egui::Mesh::with_texture(image.texture.id());
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
    ];
    let uvs = [
        pos2(0.0, 0.0),
        pos2(1.0, 0.0),
        pos2(1.0, 1.0),
        pos2(0.0, 1.0),
    ];
    for (corner, uv) in corners.into_iter().zip(uvs) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: moved(corner),
            uv,
            color: Color32::WHITE,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.with_clip_rect(clip).add(mesh);
}

/// Paints the outline of the selected page or frame and the handles of the
/// selected frame.
fn selection(ui: &egui::Ui, state: &mut AppState, place: PagePlace) {
    state.atlas.handles.clear();
    state.atlas.handles_of = state.atlas.frame();
    let painter = ui.painter();
    let stroke = egui::Stroke::new(2.0, ACCENT);
    match state.atlas.selection {
        AtlasSelection::None => {}
        AtlasSelection::Page => {
            let rect = place.rect.shrink(1.0);
            painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Inside);
        }
        AtlasSelection::Frame(id) => {
            let Some(frame) = state.doc.project.atlas.frame(id) else {
                return;
            };
            for ring in frame.shape.rings() {
                let mut points: Vec<Pos2> = ring
                    .iter()
                    .map(|&[x, y]| place.to_screen(Point::new(x, y)))
                    .collect();
                if let Some(&first) = points.first() {
                    points.push(first);
                }
                painter.add(egui::Shape::line(points, stroke));
            }
            let dark = egui::Stroke::new(1.5, Color32::from_rgb(11, 31, 26));
            let mut handles = Vec::new();
            for (handle, p) in math::handles(&frame.shape) {
                let center = place.to_screen(p);
                let on = matches!(handle, Handle::Vertex(v) if state.atlas.vertex == Some(v));
                match handle {
                    Handle::Edge(_) => {
                        let r = HANDLE_RADIUS - 1.0;
                        painter.circle(center, r, Color32::from_rgb(11, 31, 26), stroke);
                        let s = egui::Stroke::new(1.5, ACCENT);
                        painter.line_segment([center - vec2(3.0, 0.0), center + vec2(3.0, 0.0)], s);
                        painter.line_segment([center - vec2(0.0, 3.0), center + vec2(0.0, 3.0)], s);
                    }
                    _ => {
                        let r = if on {
                            HANDLE_RADIUS + 2.0
                        } else {
                            HANDLE_RADIUS
                        };
                        let fill = if on { Color32::WHITE } else { ACCENT };
                        painter.circle(center, r, fill, dark);
                    }
                }
                let size = vec2(HANDLE_TARGET, HANDLE_TARGET);
                handles.push((handle, Rect::from_center_size(center, size)));
            }
            state.atlas.handles = handles;
        }
    }
}

/// The canvas of the desktop layout, in the space between the panels.
pub fn desktop(ui: &mut egui::Ui, state: &mut AppState) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            let back = Color32::from_rgb(58, 62, 68);
            canvas(ui, state, area, area.shrink(24.0), back);
        });
}
