//! The canvas of the Atlas workspace: the open page and its map frames.
//! The iPad layout and the desktop layout use the same canvas.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use eframe::egui::{self, Color32, Pos2, Rect, pos2, vec2};
use kurbo::{Point, Vec2};
use vale_sphere::{Projection, ProjectionKind, ProjectionSpec};
use vale_store::{FrameId, FrameRecord, FrameShape, MapId, PageId, PageRecord, PageUnits};

use std::sync::Arc;

use vale_store::RasterId;

use super::{AppState, LayerRef};
use crate::atlas::{self as math, FrameTarget, Handle, PagePreset, Vertex};
use crate::globe::backdrop::Canvas;
use crate::globe::flat::FlatMesh;
use crate::globe::gpu::Mask;
use crate::globe::{FrameDraw, FramePlace};
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
/// The smallest and the largest side of a page, in millimeters.
const PAGE_SIDE: (f64, f64) = (10.0, 2000.0);
/// The smallest and the largest zoom of the page, relative to the page
/// that fits the screen.
pub const PAGE_ZOOM: (f64, f64) = (0.25, 40.0);
/// The part of the page that stays on the screen, in points.
const PAGE_KEEP: f32 = 48.0;

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
    Layers,
}

/// The pan and the zoom of the page on the screen.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PageView {
    /// The zoom, relative to the page that fits the screen.
    pub zoom: f64,
    /// The middle of the page from the middle of the page that fits, in
    /// points.
    pub offset: egui::Vec2,
}

impl Default for PageView {
    fn default() -> PageView {
        PageView {
            zoom: 1.0,
            offset: egui::Vec2::ZERO,
        }
    }
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

    /// The fitted page with a pan and a zoom.
    pub fn viewed(&self, view: PageView) -> PagePlace {
        if view == PageView::default() {
            return *self;
        }
        let size = self.rect.size() * view.zoom as f32;
        PagePlace {
            rect: Rect::from_center_size(self.rect.center() + view.offset, size),
            pts_per_mm: self.pts_per_mm * view.zoom,
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
    /// The page on the screen.
    Page,
    /// The map in the frame turns about the anchor. The angle of the pointer
    /// in the last frame, in degrees.
    Rotate(FrameId, f64),
    /// Nothing.
    Idle,
}

/// What a gesture of two fingers changes.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Pinch {
    /// The map in the selected frame.
    Frame(FrameId),
    Page,
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
    /// The list of the projections of the Projection tab is open.
    pub kinds: bool,
    /// The text of the name field of the custom projections.
    pub preset_name: String,
    /// The Add menu is open.
    pub add: bool,
    /// The Add menu in the last frame.
    pub add_rect: Option<Rect>,
    /// The panel of the selection in the last frame.
    pub panel_rect: Option<Rect>,
    /// The page on the screen in the last frame.
    pub place: Option<PagePlace>,
    /// The pan and the zoom of the open page.
    pub view: PageView,
    /// The page of `view`.
    view_of: Option<PageId>,
    /// The page that fits the screen, and the canvas, in the last frame.
    fit: Option<(PagePlace, Rect)>,
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
    /// The water of the frames that draw raster layers.
    unders: HashMap<FrameId, FrameImage>,
    /// The shape of each frame as a mask for its raster layers: the key of
    /// what it shows, and the mask.
    masks: HashMap<FrameId, (u64, Arc<Mask>)>,
    mask_count: u64,
    /// The meshes of the projections of the maps, on a sphere of radius 1.
    meshes: Vec<(ProjectionSpec, Arc<FlatMesh>)>,
    drag: Option<Drag>,
    /// The pointer at the last step of the drag.
    drag_last: Pos2,
    /// What the two-finger gesture changes.
    pinch: Option<Pinch>,
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
            kinds: false,
            preset_name: String::new(),
            add: false,
            add_rect: None,
            panel_rect: None,
            place: None,
            view: PageView::default(),
            view_of: None,
            fit: None,
            handles: Vec::new(),
            composes: 0,
            shown: None,
            handles_of: None,
            images: HashMap::new(),
            unders: HashMap::new(),
            masks: HashMap::new(),
            mask_count: 0,
            meshes: Vec::new(),
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

    /// The size of the texture of a frame, in pixels.
    pub fn image_size(&self, id: FrameId) -> Option<[usize; 2]> {
        self.images.get(&id).map(|image| image.texture.size())
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

/// The map of the selected frame, or the first map if no frame is
/// selected.
pub fn selected_map(state: &AppState) -> MapId {
    let frame = state.atlas.frame();
    let frame = frame.and_then(|id| state.doc.project.atlas.frame(id));
    frame.map_or_else(|| state.doc.first_map(), |f| f.map)
}

/// Adds a frame to the open page and selects it. The frame shows a new
/// map, or a map that no frame shows.
pub fn add_frame(state: &mut AppState) -> Option<FrameId> {
    let page = open_page(state).clone();
    let atlas = &mut state.doc.project.atlas;
    let map_id = atlas.unshown_map().unwrap_or_else(|| atlas.add_map());
    let map = atlas.map(map_id)?.clone();
    let projection = match state.pipeline.projection(&state.doc, map_id) {
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
    let Ok(projection) = state.pipeline.projection(&state.doc, frame.map) else {
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

/// The selected frame if it is the top frame at a screen position.
fn selected_at(
    state: &AppState,
    page: &PageRecord,
    place: PagePlace,
    pos: Pos2,
) -> Option<FrameId> {
    let id = state.atlas.frame()?;
    (frame_at(page, place.to_mm(pos)) == Some(id)).then_some(id)
}

/// Moves the page on the screen by `d` points.
fn pan_page(state: &mut AppState, d: egui::Vec2) {
    state.atlas.view.offset += d;
    keep_page(state);
}

/// Zooms the page about a screen position.
fn zoom_page(state: &mut AppState, at: Pos2, factor: f64) {
    let Some((fit, _)) = state.atlas.fit else {
        return;
    };
    let view = &mut state.atlas.view;
    let zoom = (view.zoom * factor).clamp(PAGE_ZOOM.0, PAGE_ZOOM.1);
    let f = (zoom / view.zoom) as f32;
    let center = fit.rect.center() + view.offset;
    view.offset = at + (center - at) * f - fit.rect.center();
    view.zoom = zoom;
    keep_page(state);
}

/// Moves the page so that a part of it stays on the canvas.
fn keep_page(state: &mut AppState) {
    let Some((fit, area)) = state.atlas.fit else {
        return;
    };
    let r = fit.viewed(state.atlas.view).rect;
    let keep = |lo: f32, hi: f32, min: f32, max: f32| {
        let k = PAGE_KEEP.min(hi - lo);
        (min + k - hi).max(0.0) + (max - k - lo).min(0.0)
    };
    state.atlas.view.offset += vec2(
        keep(r.left(), r.right(), area.left(), area.right()),
        keep(r.top(), r.bottom(), area.top(), area.bottom()),
    );
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
    if state.atlas.view_of != Some(state.atlas.page) {
        state.atlas.view = PageView::default();
        state.atlas.view_of = Some(state.atlas.page);
    }
    let fit = PagePlace::fit(open_page(state), free);
    state.atlas.fit = Some((fit, area));
    let place = fit.viewed(state.atlas.view);
    state.atlas.place = Some(place);
    input(ui, state, &resp, place);
    sync(state);
    let place = PagePlace::fit(open_page(state), free).viewed(state.atlas.view);
    state.atlas.place = Some(place);

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

    // Two fingers move, zoom, and turn the map of the selected frame below
    // them. Elsewhere, they move and zoom the page.
    if let Some(touch) = fingers {
        let pinch = match state.atlas.pinch {
            Some(pinch) => Some(pinch),
            None if resp.rect.contains(touch.center_pos) => {
                Some(match selected_at(state, &page, place, touch.center_pos) {
                    Some(id) => Pinch::Frame(id),
                    None => Pinch::Page,
                })
            }
            None => None,
        };
        state.atlas.pinch = pinch;
        state.atlas.drag = None;
        if pinch == Some(Pinch::Page) {
            pan_page(state, touch.translation_delta);
            zoom_page(state, touch.center_pos, f64::from(touch.zoom_delta));
        }
        if let Some(Pinch::Frame(id)) = pinch {
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

    // The wheel and a pinch of the trackpad zoom the map of the selected
    // frame, or the page, about the pointer.
    if fingers.is_none()
        && let Some(pos) = resp.hover_pos()
    {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (f64::from(scroll) * 0.002).exp() * f64::from(pinch);
        let at = place.to_mm(pos);
        if (factor - 1.0).abs() > 1e-4 {
            match selected_at(state, &page, place, pos) {
                Some(id) => {
                    if edit_frame(state, id, |f, p| math::zoom(f, p, at, factor)) {
                        state.atlas.wheel_at = Some(now);
                    }
                }
                None => {
                    zoom_page(state, pos, factor);
                    state.atlas.wheel_at = Some(now);
                }
            }
        }
        state.cursor_lonlat = frame_at(&page, at).and_then(|id| {
            let frame = page.frames.iter().find(|f| f.id == id)?;
            let projection = state.pipeline.projection(&state.doc, frame.map).ok()?;
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
        if drag == Drag::Page {
            pan_page(state, d);
        } else {
            let d = Vec2::new(f64::from(d.x), f64::from(d.y)) / place.pts_per_mm;
            drag_to(state, drag, place, pos, d);
        }
    }
    if resp.drag_stopped() || !any_down {
        state.atlas.drag = None;
    }

    // A second quick tap on a vertex removes it. A second quick tap off the
    // page fits the page to the screen.
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
            _ => {
                tap(state, &page, place, pos);
                if again && !place.rect.contains(pos) {
                    state.atlas.view = PageView::default();
                    state.atlas.last_tap = None;
                }
            }
        }
    }

    let typing = ui.ctx().memory(|m| m.focused().is_some());
    let delete =
        ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
    if delete && !typing && state.atlas.tool == AtlasTool::Select {
        if let Some(v) = state.atlas.vertex {
            remove_vertex(state, v);
        } else if state.atlas.frame().is_some() {
            delete_frame(state);
        }
    }
}

/// Deletes the selected frame and clears the selection.
pub fn delete_frame(state: &mut AppState) {
    let Some(id) = state.atlas.frame() else {
        return;
    };
    state.doc.project.atlas.remove_frame(id);
    state.doc.project.atlas.prune_maps();
    state.atlas.selection = AtlasSelection::None;
    state.atlas.vertex = None;
    state.atlas.images.remove(&id);
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
        AtlasTool::Move => match selected_at(state, page, place, pos) {
            Some(id) if alt => {
                let frame = page.frames.iter().find(|f| f.id == id);
                let anchor = frame.map_or(at, |f| math::anchor(&f.shape));
                Drag::Rotate(id, angle(anchor, at))
            }
            Some(id) => Drag::Pan(id),
            None => Drag::Page,
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
                None => Drag::Page,
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
        Drag::Page | Drag::Idle => {}
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

/// The raster layers that a map shows and that the globe draws, from the
/// bottom.
fn map_rasters(state: &AppState, map: MapId) -> Vec<RasterId> {
    let layers = state.doc.project.layers().iter();
    let on = layers.filter(|layer| state.doc.shown(map, layer.id));
    on.filter_map(|layer| layer.raster())
        .filter(|id| state.globe.draws(*id))
        .collect()
}

/// A frame of the page to draw in this pass.
struct Placed {
    id: FrameId,
    vt: ViewTransform,
    shown: Rect,
    /// The frame draws raster layers on the GPU.
    layered: bool,
}

/// The pixels of the part of a frame on the canvas: the left, the top, the
/// width, and the height. The pixels cover `shown`.
fn pixel_rect(shown: Rect, ppp: f32) -> [u32; 4] {
    let (x0, y0) = ((shown.min.x * ppp).floor(), (shown.min.y * ppp).floor());
    let (x1, y1) = ((shown.max.x * ppp).ceil(), (shown.max.y * ppp).ceil());
    let side = |a: f32, b: f32| (b - a).clamp(1.0, 8192.0) as u32;
    [
        x0.max(0.0) as u32,
        y0.max(0.0) as u32,
        side(x0, x1),
        side(y0, y1),
    ]
}

/// The mesh of the projection of a map, on a sphere of radius 1.
fn mesh_of(state: &mut AppState, spec: ProjectionSpec) -> Option<Arc<FlatMesh>> {
    if let Some((_, mesh)) = state.atlas.meshes.iter().find(|(of, _)| *of == spec) {
        return Some(mesh.clone());
    }
    let projection = Projection::new(spec, 1.0).ok()?;
    let mesh = Arc::new(FlatMesh::new(&projection));
    if state.atlas.meshes.len() >= 8 {
        state.atlas.meshes.remove(0);
    }
    state.atlas.meshes.push((spec, mesh.clone()));
    Some(mesh)
}

/// The shape of a frame as pixels of coverage, for the target of the frame.
fn shape_mask(
    state: &mut AppState,
    frame: &FrameRecord,
    place: PagePlace,
    rect: [u32; 4],
    ppp: f32,
) -> Option<Arc<Mask>> {
    let [x, y, w, h] = rect;
    let key = hash_of((&frame.shape, place.target(), rect, ppp.to_bits()));
    if let Some((of, mask)) = state.atlas.masks.get(&frame.id)
        && *of == key
    {
        return Some(mask.clone());
    }
    let top_left = vec2(x as f32, y as f32) / ppp;
    let origin = place.rect.min - top_left;
    let target = FrameTarget {
        origin: Point::new(f64::from(origin.x), f64::from(origin.y)),
        pts_per_mm: place.pts_per_mm,
    };
    let size = (f64::from(w) / f64::from(ppp), f64::from(h) / f64::from(ppp));
    let list = crate::pipeline::Pipeline::frame_mask(frame, target, size);
    let image = vale_render::render_rgba(&list, f64::from(ppp)).ok()?;
    let alpha: Vec<u8> = image.data.chunks_exact(4).map(|px| px[3]).collect();
    let size = [image.width as u32, image.height as u32];
    state.atlas.mask_count += 1;
    let mask = Arc::new(Mask {
        id: state.atlas.mask_count,
        size,
        alpha,
    });
    state.atlas.masks.insert(frame.id, (key, mask.clone()));
    Some(mask)
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
    // The copies of the raster layers arrive on the GPU while the globe
    // advances.
    if state.globe.advance(std::time::Instant::now()) {
        ctx.request_repaint();
    }
    let radius = state.doc.project.world.radius_km * 1000.0;
    let mut placed: Vec<Placed> = Vec::new();
    let mut draws: Vec<FrameDraw> = Vec::new();
    for frame in &page.frames {
        let Some(map) = state.doc.project.atlas.map(frame.map) else {
            continue;
        };
        let rasters = map_rasters(state, frame.map);
        let layered = !rasters.is_empty();
        let content = hash_of((
            map,
            state.map_generation,
            frame.map,
            frame.graticule,
            ppp.to_bits(),
            state.selection,
            layered,
        ));
        // Only the part of the frame on the canvas has a texture.
        let shown = place
            .rect_to_screen(math::shape_bbox(&frame.shape))
            .intersect(area);
        if !shown.is_positive() {
            continue;
        }
        let place_key = hash_of((
            &frame.shape,
            frame.center,
            frame.scale,
            frame.rotation,
            target,
            shown,
        ));
        let Ok(projection) = state.pipeline.projection(&state.doc, frame.map) else {
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
            let at = (place, shown);
            compose(
                &ctx,
                state,
                frame,
                at,
                vt,
                keys,
                quality,
                (primary, layered),
            );
        }
        if layered {
            let spec = state.doc.projection(frame.map);
            let rect = pixel_rect(shown, ppp);
            let mesh = mesh_of(state, spec);
            let mask = shape_mask(state, frame, place, rect, ppp);
            if let Some(mesh) = mesh {
                let [x, y, ..] = rect;
                let center = vt.origin;
                let units = vt.view.scale * radius;
                let scale = frame.scale;
                draws.push(FrameDraw {
                    id: frame.id.0,
                    place: FramePlace {
                        center: [
                            center.x as f32 * ppp - x as f32,
                            center.y as f32 * ppp - y as f32,
                        ],
                        map_center: [
                            (vt.view.center.x / radius) as f32,
                            (vt.view.center.y / radius) as f32,
                        ],
                        units_to_pixels: (units * f64::from(ppp)) as f32,
                        rotation: vt.view.rotation,
                        points_per_radian: radius * crate::atlas::PT_PER_MM * 1000.0 / scale,
                    },
                    rect,
                    mesh,
                    mask,
                    shown: rasters,
                });
            }
        }
        placed.push(Placed {
            id: frame.id,
            vt,
            shown,
            layered,
        });
    }
    let canvas = Canvas {
        background: PAPER,
        pixels_per_point: ppp,
    };
    let mut callbacks: HashMap<u32, _> = if draws.is_empty() {
        HashMap::new()
    } else {
        let painted = state.globe.paint_frames(canvas, draws);
        painted.unwrap_or_default().into_iter().collect()
    };
    for p in placed {
        let by = p.layered.then(|| state.atlas.unders.get(&p.id)).flatten();
        if let Some(image) = by {
            paint_image(&painter, image, &p.vt, p.shown);
        }
        if let Some(callback) = callbacks.remove(&p.id.0) {
            painter.add(eframe::egui_wgpu::Callback::new_paint_callback(
                area, callback,
            ));
        }
        if let Some(image) = state.atlas.images.get(&p.id) {
            paint_image(&painter, image, &p.vt, p.shown);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compose(
    ctx: &egui::Context,
    state: &mut AppState,
    frame: &FrameRecord,
    (place, shown): (PagePlace, Rect),
    vt: ViewTransform,
    (content, place_key): (u64, u64),
    quality: Quality,
    (primary, layered): (bool, bool),
) {
    let ppp = ctx.pixels_per_point();
    let px = |points: f32| ((points * ppp).ceil() as usize).clamp(1, 8192);
    let (w, h) = (px(shown.width()), px(shown.height()));
    // The list starts at the top-left corner of the part of the frame on
    // the canvas.
    let origin = place.rect.min - shown.min;
    let target = FrameTarget {
        origin: Point::new(f64::from(origin.x), f64::from(origin.y)),
        pts_per_mm: place.pts_per_mm,
    };
    let part = kurbo::Rect::new(
        0.0,
        0.0,
        f64::from(shown.width()),
        f64::from(shown.height()),
    );
    let selection = state.selection;
    // A frame with raster layers has the water below them and the rest
    // above them. The GPU draws them between the two.
    let composed = if layered {
        state.pipeline.compose_frame_over(
            &state.doc,
            frame,
            target,
            part,
            &mut state.fonts,
            quality,
            selection,
        )
    } else {
        state.pipeline.compose_frame_part(
            &state.doc,
            frame,
            target,
            part,
            &mut state.fonts,
            quality,
            selection,
        )
    };
    state.atlas.composes += 1;
    let mut composed = match composed {
        Ok(c) => c,
        Err(e) => {
            state.status = format!("{e:#}");
            return;
        }
    };
    let (width, height) = (w as f64 / f64::from(ppp), h as f64 / f64::from(ppp));
    composed.list.width = width;
    composed.list.height = height;
    let settled = quality == Quality::Final;
    let keys = (content, place_key);
    if layered {
        let under = state
            .pipeline
            .compose_frame_under(&state.doc, frame, target, part);
        match under {
            Ok(mut list) => {
                list.width = width;
                list.height = height;
                let images = &mut state.atlas.unders;
                match upload(ctx, images, frame.id, &list, (shown, vt), keys, settled) {
                    Ok(()) => {}
                    Err(e) => state.status = e,
                }
            }
            Err(e) => state.status = format!("{e:#}"),
        }
    } else {
        state.atlas.unders.remove(&frame.id);
    }
    let images = &mut state.atlas.images;
    if let Err(e) = upload(
        ctx,
        images,
        frame.id,
        &composed.list,
        (shown, vt),
        keys,
        settled,
    ) {
        state.status = e;
        return;
    }
    if primary {
        state.composed = Some(composed);
    }
}

/// Renders a list to the texture of a frame in `images`.
fn upload(
    ctx: &egui::Context,
    images: &mut HashMap<FrameId, FrameImage>,
    id: FrameId,
    list: &vale_render::DisplayList,
    (shown, vt): (Rect, ViewTransform),
    (content, place): (u64, u64),
    settled: bool,
) -> Result<(), String> {
    let ppp = ctx.pixels_per_point();
    let img = vale_render::render_rgba(list, f64::from(ppp)).map_err(|e| format!("{e}"))?;
    let image = egui::ColorImage::from_rgba_premultiplied([img.width, img.height], &img.data);
    let options = egui::TextureOptions::LINEAR;
    let texture = match images.remove(&id) {
        Some(mut old) => {
            old.texture.set(image, options);
            old.texture
        }
        None => ctx.load_texture(format!("frame-{}", id.0), image, options),
    };
    let size = vec2(img.width as f32, img.height as f32) / ppp;
    images.insert(
        id,
        FrameImage {
            texture,
            content,
            place,
            vt,
            rect: Rect::from_min_size(shown.min, size),
            settled,
        },
    );
    Ok(())
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

/// Makes the shape of a frame a polygon or a rectangle.
pub fn set_polygon(state: &mut AppState, id: FrameId, polygon: bool) {
    edit_frame(state, id, |f, _| {
        f.shape = if polygon {
            math::to_polygon(&f.shape)
        } else {
            math::to_rectangle(&f.shape)
        };
        true
    });
    state.atlas.vertex = None;
}

/// Adds a hole to a polygon frame.
pub fn add_hole(state: &mut AppState, id: FrameId) {
    edit_frame(state, id, |f, _| {
        math::add_hole(&mut f.shape);
        true
    });
}

/// Sets the scale of a frame, in the range of the scales.
pub fn set_scale(state: &mut AppState, id: FrameId, scale: f64) {
    let scale = scale.clamp(math::SCALE_RANGE.0, math::SCALE_RANGE.1);
    edit_frame(state, id, |f, _| {
        f.scale = scale;
        true
    });
}

/// Sets the longitude of the center of a frame, in degrees.
pub fn set_longitude(state: &mut AppState, id: FrameId, lon: f64) {
    let lon = (lon + 180.0).rem_euclid(360.0) - 180.0;
    edit_frame(state, id, |f, _| {
        f.center[0] = lon;
        true
    });
}

/// Sets the latitude of the center of a frame, in degrees. A latitude past
/// a pole does nothing.
pub fn set_latitude(state: &mut AppState, id: FrameId, lat: f64) {
    if lat.abs() <= 90.0 {
        edit_frame(state, id, |f, _| {
            f.center[1] = lat;
            true
        });
    }
}

/// Sets the rotation of the map in a frame, in degrees.
pub fn set_rotation(state: &mut AppState, id: FrameId, degrees: f64) {
    edit_frame(state, id, |f, _| {
        f.rotation = degrees;
        true
    });
}

pub fn set_graticule(state: &mut AppState, id: FrameId, on: bool) {
    edit_frame(state, id, |f, _| {
        f.graticule = on;
        true
    });
}

/// The map of a frame.
fn map_of(state: &AppState, id: FrameId) -> Option<vale_store::MapId> {
    state.doc.project.atlas.frame(id).map(|f| f.map)
}

/// Changes the projection of the map of a frame.
pub fn edit_projection(state: &mut AppState, id: FrameId, edit: impl FnOnce(&mut ProjectionSpec)) {
    let Some(map) = map_of(state, id) else {
        return;
    };
    let before = state.doc.projection(map);
    let mut spec = before;
    edit(&mut spec);
    state.doc.set_projection(map, spec);
    if state.doc.projection(map).to_proj() != before.to_proj() {
        state.touch();
    }
}

/// Sets the kind of the projection of the map of a frame and keeps its
/// center.
pub fn set_projection_kind(state: &mut AppState, id: FrameId, kind: ProjectionKind) {
    edit_projection(state, id, |s| *s = s.with_kind(kind));
}

/// Sets the central meridian, or the longitude of the center.
pub fn set_projection_longitude(state: &mut AppState, id: FrameId, lon: f64) {
    edit_projection(state, id, |s| s.lon0 = lon);
}

/// Sets the latitude of origin, or the latitude of the center.
pub fn set_projection_latitude(state: &mut AppState, id: FrameId, lat: f64) {
    edit_projection(state, id, |s| s.lat0 = lat);
}

/// Sets the standard parallels of a conic projection.
pub fn set_parallels(state: &mut AppState, id: FrameId, lat1: f64, lat2: f64) {
    edit_projection(state, id, |s| (s.lat1, s.lat2) = (lat1, lat2));
}

/// Turns the oblique aspect on or off.
pub fn set_oblique(state: &mut AppState, id: FrameId, on: bool) {
    edit_projection(state, id, |s| {
        s.azimuth = on.then_some(s.azimuth.unwrap_or(OBLIQUE_AZIMUTH));
    });
}

/// Sets the azimuth of the center line of an oblique aspect.
pub fn set_azimuth(state: &mut AppState, id: FrameId, azimuth: f64) {
    edit_projection(state, id, |s| s.azimuth = Some(azimuth));
}

/// The azimuth of a new oblique aspect. A tilt of 45 degrees shows well.
pub const OBLIQUE_AZIMUTH: f64 = 45.0;

/// Saves the projection of the map of a frame under the typed name. Returns
/// false if the name is empty.
pub fn save_preset(state: &mut AppState, id: FrameId) -> bool {
    let Some(map) = map_of(state, id) else {
        return false;
    };
    let name = state.atlas.preset_name.trim().to_string();
    let saved = state.doc.save_preset(map, &name);
    if saved {
        state.atlas.preset_name = name;
    }
    saved
}

/// Gives the map of a frame the projection of a custom projection, and puts
/// its name in the name field.
pub fn apply_preset(state: &mut AppState, id: FrameId, name: &str) {
    let Some(map) = map_of(state, id) else {
        return;
    };
    let before = state.doc.projection(map).to_proj();
    if state.doc.apply_preset(map, name) {
        state.atlas.preset_name = name.to_string();
        if state.doc.projection(map).to_proj() != before {
            state.touch();
        }
    }
}

/// Deletes the custom projection with the name in the name field.
pub fn delete_preset(state: &mut AppState) {
    let name = state.atlas.preset_name.clone();
    if state.doc.remove_preset(&name) {
        state.atlas.preset_name.clear();
    }
}

/// The custom projection that has the projection of the map of a frame.
pub fn current_preset(state: &AppState, id: FrameId) -> Option<String> {
    let now = state.doc.projection(map_of(state, id)?).to_proj();
    let same = |p: &&vale_store::ProjectionPreset| {
        ProjectionSpec::from_proj(&p.projection).is_some_and(|s| s.to_proj() == now)
    };
    state
        .doc
        .presets()
        .iter()
        .find(same)
        .map(|p| p.name.clone())
}

/// Moves the center of the projection of the map of a frame to the center
/// of the frame. A projection that is not azimuthal keeps its latitude.
pub fn center_projection(state: &mut AppState, id: FrameId) {
    let Some(frame) = state.doc.project.atlas.frame(id) else {
        return;
    };
    let [lon, lat] = frame.center;
    let map = frame.map;
    let mut spec = state.doc.projection(map);
    spec.lon0 = lon;
    if spec.kind.is_azimuthal() || spec.azimuth.is_some() {
        spec.lat0 = lat;
    }
    state.doc.set_projection(map, spec);
    state.touch();
}

/// Changes the open page.
pub fn edit_page(state: &mut AppState, edit: impl FnOnce(&mut PageRecord)) {
    let id = open_page(state).id;
    if let Some(page) = state.doc.project.atlas.page_mut(id) {
        edit(page);
    }
}

/// Gives the open page the size of a preset and keeps its orientation.
pub fn set_page_preset(state: &mut AppState, preset: &PagePreset) {
    edit_page(state, |page| {
        let (w, h) = (preset.width_mm, preset.height_mm);
        let (w, h) = if page.width_mm > page.height_mm {
            (h, w)
        } else {
            (w, h)
        };
        page.width_mm = w;
        page.height_mm = h;
    });
}

/// Turns the open page between portrait and landscape.
pub fn turn_page(state: &mut AppState) {
    edit_page(state, |p| std::mem::swap(&mut p.width_mm, &mut p.height_mm));
}

/// A length of a page from the text of a field, in millimeters. A length
/// outside the sides of a page is `None`.
pub fn parse_length(text: &str, units: PageUnits) -> Option<f64> {
    let factor = match units {
        PageUnits::Mm => 1.0,
        PageUnits::In => 25.4,
    };
    super::pad::raster::parse_value(text)
        .map(|v| v * factor)
        .filter(|mm| (PAGE_SIDE.0..=PAGE_SIDE.1).contains(mm))
}

/// Adds a page after the open page, with its size and units, and opens it
/// with the page selected.
pub fn add_page(state: &mut AppState) {
    let page = open_page(state);
    let (id, w, h, units) = (page.id, page.width_mm, page.height_mm, page.units);
    let new = state.doc.project.atlas.add_page(Some(id), w, h, units);
    state.atlas.page = new;
    state.atlas.selection = AtlasSelection::Page;
}

/// Deletes the open page and opens the page before it, or else the page
/// after it. The last page stays.
pub fn delete_page(state: &mut AppState) {
    let id = open_page(state).id;
    let atlas = &mut state.doc.project.atlas;
    let index = atlas.page_index(id).unwrap_or(0);
    if atlas.remove_page(id).is_some() {
        atlas.prune_maps();
        let open = index.saturating_sub(1).min(atlas.pages.len() - 1);
        state.atlas.page = atlas.pages[open].id;
        state.atlas.selection = AtlasSelection::Page;
    }
}

/// Gives the map of a frame a new name.
pub fn rename_map(state: &mut AppState, id: FrameId, name: &str) {
    let atlas = &mut state.doc.project.atlas;
    let Some(map) = atlas.frame(id).map(|f| f.map) else {
        return;
    };
    if let Some(record) = atlas.map_mut(map) {
        record.name = name.to_string();
    }
}

/// The canvas of the desktop layout, in the space between the panels. The
/// keys V and H choose the Select tool and the Move tool.
pub fn desktop(ui: &mut egui::Ui, state: &mut AppState) {
    let typing = ui.ctx().memory(|m| m.focused().is_some());
    if !typing {
        let (v, h) = ui.input(|i| (i.key_pressed(egui::Key::V), i.key_pressed(egui::Key::H)));
        if v {
            state.atlas.tool = AtlasTool::Select;
        }
        if h {
            state.atlas.tool = AtlasTool::Move;
        }
    }
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            let back = Color32::from_rgb(58, 62, 68);
            canvas(ui, state, area, area.shrink(24.0), back);
        });
}
