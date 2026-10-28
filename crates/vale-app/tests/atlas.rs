//! The Atlas workspace in the iPad layout: pages, map frames, and their
//! panels.

use std::time::Instant;

use eframe::egui::{self, Modifiers, Pos2, Rect, Vec2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kurbo::Point;
use vale_app::atlas::{Handle, anchor, frame_transform, shape_bbox};
use vale_app::document::Document;
use vale_app::projects::Locations;
use vale_app::session::{Session, new_project_path};
use vale_app::ui::atlas::{AtlasSelection, AtlasTool, FrameTab};
use vale_app::ui::pad::GRATICULE;
use vale_app::ui::pad::atlas::{
    ADD, ADD_HOLE, ADD_PAGE, AUTO_INTERVAL, BORDER, BORDER_COLOR, BORDER_WIDTH, CENTER_ON_FRAME,
    DELETE_FRAME, DELETE_PAGE, DELETE_PROJECTION, EXPORT_ATLAS_PDF, EXPORT_PAGE_PDF,
    EXPORT_PAGE_PNG, FIRST_PARALLEL, FRAME_TABS, GRATICULE_COLOR, GRATICULE_WIDTH, HEIGHT,
    INTERVAL, MAP_FRAME, MAP_NAME, NEXT_PAGE, OBLIQUE, POLYGON, PREVIOUS_PAGE, PROJECTION,
    REMOVE_VERTEX, SAVE_PROJECTION, SCALE, SIZE, WIDTH, custom_row, page_title, readout_text,
};
use vale_app::ui::pad::widgets::eye_name;
use vale_app::ui::{Action, AppState, ExportFormat, Layout, Screen, Workspace, draw};
use vale_render::Item;
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::{
    Feature, FrameRecord, FrameShape, Geometry, GeometryKind, LayerId, MapId, PageUnits, Project,
    ProjectFile, World,
};

type Pad = Harness<'static, AppState>;

/// The boards of the mockup: landscape, portrait, half split view, and
/// narrow split view.
const BOARDS: [(f32, f32); 4] = [
    (1194.0, 834.0),
    (834.0, 1194.0),
    (597.0, 834.0),
    (375.0, 834.0),
];
/// The bands that the system uses at the top and at the bottom.
const BANDS: (f32, f32) = (24.0, 20.0);
/// The ways to touch the screen: a finger, a pen, and the mouse.
#[derive(Clone, Copy, Debug)]
enum Input {
    Finger,
    Pen,
    Mouse,
}
const INPUTS: [Input; 3] = [Input::Finger, Input::Pen, Input::Mouse];

fn state() -> AppState {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.set_layout(Layout::Pad);
    state.workspace = Workspace::Atlas;
    state
}

fn pad_with(state: AppState, (width, height): (f32, f32)) -> Pad {
    let mut h = Harness::builder()
        .with_size(vec2(width, height))
        .with_pixels_per_point(1.0)
        .with_step_dt(0.05)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(3);
    h
}

fn pad() -> Pad {
    pad_with(state(), BOARDS[0])
}

fn control(h: &Pad, name: &str) -> Option<Rect> {
    let controls = &h.state().pad.controls;
    controls.iter().find(|c| c.name == name).map(|c| c.rect)
}

/// One touch. The touch with id 1 is also the pointer. A touch with a force
/// is a pen.
fn touch(h: &mut Pad, id: u64, phase: egui::TouchPhase, pos: Pos2, force: Option<f32>) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(id),
        phase,
        pos,
        force,
    });
    if id != 1 {
        return;
    }
    h.event(egui::Event::PointerMoved(pos));
    if phase != egui::TouchPhase::Move {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: phase == egui::TouchPhase::Start,
            modifiers: Modifiers::NONE,
        });
    }
    if phase == egui::TouchPhase::End {
        h.event(egui::Event::PointerGone);
    }
}

fn button(h: &mut Pad, pos: Pos2, pressed: bool) {
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
}

/// A tap at a place.
fn tap_with(h: &mut Pad, pos: Pos2, input: Input) {
    match input {
        Input::Mouse => {
            h.event(egui::Event::PointerMoved(pos));
            h.step();
            button(h, pos, true);
            h.step();
            button(h, pos, false);
        }
        _ => {
            let force = matches!(input, Input::Pen).then_some(0.5);
            touch(h, 1, egui::TouchPhase::Start, pos, force);
            h.step();
            touch(h, 1, egui::TouchPhase::End, pos, force);
        }
    }
    h.run_steps(3);
}

/// Two quick taps at a place. The second tap starts in the frame of the
/// end of the first tap.
fn double_tap(h: &mut Pad, pos: Pos2, input: Input) {
    let force = matches!(input, Input::Pen).then_some(0.5);
    let press = |h: &mut Pad, pressed: bool| match input {
        Input::Mouse => button(h, pos, pressed),
        _ => {
            let phase = if pressed {
                egui::TouchPhase::Start
            } else {
                egui::TouchPhase::End
            };
            touch(h, 1, phase, pos, force);
        }
    };
    if matches!(input, Input::Mouse) {
        h.event(egui::Event::PointerMoved(pos));
        h.step();
    }
    press(h, true);
    h.step();
    press(h, false);
    press(h, true);
    h.step();
    press(h, false);
    h.run_steps(3);
}

/// A tap of a finger on the middle of a control.
fn tap(h: &mut Pad, name: &str) {
    let rect = control(h, name).unwrap_or_else(|| panic!("no control `{name}`"));
    tap_with(h, rect.center(), Input::Finger);
}

/// A drag from one place to another in `steps` steps. The release is in a
/// later frame.
fn drag_steps(h: &mut Pad, from: Pos2, to: Pos2, input: Input, steps: usize) {
    let force = matches!(input, Input::Pen).then_some(0.5);
    match input {
        Input::Mouse => {
            h.event(egui::Event::PointerMoved(from));
            h.step();
            button(h, from, true);
        }
        _ => touch(h, 1, egui::TouchPhase::Start, from, force),
    }
    h.step();
    for i in 1..=steps {
        let at = from.lerp(to, i as f32 / steps as f32);
        match input {
            Input::Mouse => h.event(egui::Event::PointerMoved(at)),
            _ => touch(h, 1, egui::TouchPhase::Move, at, force),
        }
        h.step();
    }
}

fn release(h: &mut Pad, at: Pos2, input: Input) {
    let force = matches!(input, Input::Pen).then_some(0.5);
    match input {
        Input::Mouse => button(h, at, false),
        _ => touch(h, 1, egui::TouchPhase::End, at, force),
    }
    h.run_steps(3);
}

fn drag(h: &mut Pad, from: Pos2, to: Pos2, input: Input) {
    drag_steps(h, from, to, input, 6);
    release(h, to, input);
}

/// Two fingers about `center`: they start `from` points apart on a
/// horizontal line, and they end `to` points apart, turned clockwise by
/// `turn` degrees, with the middle moved by `shift`.
fn pinch(h: &mut Pad, center: Pos2, from: f32, to: f32, turn: f32, shift: Vec2) {
    let fingers = |t: f32| {
        let half = (from + (to - from) * t) / 2.0;
        let angle = (turn * t).to_radians();
        let d = vec2(angle.cos(), angle.sin()) * half;
        let middle = center + shift * t;
        (middle - d, middle + d)
    };
    let (a, b) = fingers(0.0);
    touch(h, 1, egui::TouchPhase::Start, a, None);
    h.step();
    touch(h, 2, egui::TouchPhase::Start, b, None);
    h.step();
    for i in 1..=8 {
        let (a, b) = fingers(i as f32 / 8.0);
        touch(h, 1, egui::TouchPhase::Move, a, None);
        touch(h, 2, egui::TouchPhase::Move, b, None);
        h.step();
    }
    let (a, b) = fingers(1.0);
    touch(h, 2, egui::TouchPhase::End, b, None);
    h.step();
    touch(h, 1, egui::TouchPhase::End, a, None);
    h.run_steps(8);
}

fn first_projection(h: &Pad) -> ProjectionSpec {
    let doc = &h.state().doc;
    doc.projection(doc.first_map())
}

/// The projection of the map of the selected frame.
fn selected_projection(h: &Pad) -> ProjectionSpec {
    let doc = &h.state().doc;
    doc.projection(vale_app::ui::atlas::selected_map(h.state()))
}

fn open_page(h: &Pad) -> vale_store::PageRecord {
    vale_app::ui::atlas::open_page(h.state()).clone()
}

fn frame(h: &Pad, index: usize) -> FrameRecord {
    open_page(h).frames[index].clone()
}

/// A page point in millimeters on the screen.
fn screen(h: &Pad, p: Point) -> Pos2 {
    h.state().atlas.place.unwrap().to_screen(p)
}

/// The middle of a frame on the screen.
fn middle(h: &Pad, index: usize) -> Pos2 {
    screen(h, anchor(&frame(h, index).shape))
}

/// Adds a frame with the Add menu.
fn add_frame(h: &mut Pad) {
    tap(h, ADD);
    tap(h, MAP_FRAME);
}

/// A page with a main frame and an inset frame. The inset is selected.
fn inset_pad(size: (f32, f32)) -> Pad {
    let mut h = pad_with(state(), size);
    add_frame(&mut h);
    add_frame(&mut h);
    h
}

fn handle(h: &Pad, which: Handle) -> Pos2 {
    let handles = &h.state().atlas.handles;
    let found = handles.iter().find(|(handle, _)| *handle == which);
    found
        .unwrap_or_else(|| panic!("no handle {which:?}"))
        .1
        .center()
}

/// The place of a screen position in a frame, and the screen position of a
/// place in a frame.
fn place_at(h: &mut Pad, index: usize, pos: Pos2) -> [f64; 2] {
    let f = frame(h, index);
    let target = h.state().atlas.place.unwrap().target();
    let s = h.state_mut();
    let projection = s.pipeline.projection(&s.doc, f.map).unwrap();
    let vt = frame_transform(projection, &f, target);
    let q = Point::new(f64::from(pos.x), f64::from(pos.y));
    projection.inverse(vt.from_page(q)).unwrap()
}

fn pos_of(h: &mut Pad, index: usize, lonlat: [f64; 2]) -> Pos2 {
    let f = frame(h, index);
    let target = h.state().atlas.place.unwrap().target();
    let s = h.state_mut();
    let projection = s.pipeline.projection(&s.doc, f.map).unwrap();
    let vt = frame_transform(projection, &f, target);
    let p = vt.to_page(projection.forward(lonlat).unwrap());
    pos2(p.x as f32, p.y as f32)
}

/// Scrolls the panel of the selection until a control is inside the card,
/// with a drag up of a finger on the margin at the left of the rows.
fn scroll_to(h: &mut Pad, name: &str) {
    for _ in 0..8 {
        let card = h.state().atlas.panel_rect.unwrap();
        let inside = |rect: Rect| card.shrink2(vec2(0.0, 40.0)).contains_rect(rect);
        if control(h, name).is_some_and(inside) {
            return;
        }
        let from = pos2(card.left() + 6.0, card.bottom() - 40.0);
        drag(h, from, from - vec2(0.0, 200.0), Input::Finger);
    }
}

/// Scrolls the panel up until a control shows.
fn scroll_up_to(h: &mut Pad, name: &str) {
    for _ in 0..8 {
        let card = h.state().atlas.panel_rect.unwrap();
        let inside = |rect: Rect| card.shrink2(vec2(0.0, 40.0)).contains_rect(rect);
        if control(h, name).is_some_and(inside) {
            return;
        }
        let from = pos2(card.left() + 6.0, card.top() + 200.0);
        let to = pos2(from.x, card.bottom() - 60.0);
        drag(h, from, to, Input::Finger);
    }
}

/// Makes sure that a finger can hit each control of the frame.
fn check_controls(h: &Pad, what: &str) {
    let screen = h.state().pad.rects.unwrap().screen;
    let controls = &h.state().pad.controls;
    assert!(controls.len() >= 4, "{what}: {} controls", controls.len());
    for (i, a) in controls.iter().enumerate() {
        let (name, rect) = (&a.name, a.rect);
        assert!(
            rect.width() >= 44.0 && rect.height() >= 44.0,
            "{what}: {name} is {rect:?}"
        );
        assert!(rect.top() >= BANDS.0, "{what}: {name} is in the top band");
        assert!(
            rect.bottom() <= screen.bottom() - BANDS.1,
            "{what}: {name} is in the bottom band"
        );
        assert!(
            rect.left() >= 0.0 && rect.right() <= screen.right(),
            "{what}: {name}"
        );
        for b in &controls[i + 1..] {
            assert_ne!(a.name, b.name, "{what}");
        }
    }
    // The handles are 44 points wide, up to the rounding of a float.
    for (handle, rect) in &h.state().atlas.handles {
        assert!(rect.width() > 43.99 && rect.height() > 43.99, "{handle:?}");
    }
}

#[test]
fn the_panel_of_the_page_sets_the_page_size() {
    let mut h = pad();
    let page = h.state().atlas.place.unwrap().rect;
    tap_with(&mut h, page.center(), Input::Finger);
    assert_eq!(h.state().atlas.selection, AtlasSelection::Page);
    assert_eq!(readout_text(h.state()), "297 × 210 mm · 1 / 1");

    // A preset keeps the orientation.
    tap(&mut h, SIZE);
    tap(&mut h, "A3");
    let p = open_page(&h);
    assert_eq!((p.width_mm, p.height_mm), (420.0, 297.0));
    tap(&mut h, "Portrait");
    let p = open_page(&h);
    assert_eq!((p.width_mm, p.height_mm), (297.0, 420.0));

    // A free width, from the keypad.
    tap(&mut h, WIDTH);
    for key in ["3", "0", "5"] {
        tap(&mut h, &format!("{WIDTH} {key}"));
    }
    tap(&mut h, &format!("{WIDTH} Done"));
    let p = open_page(&h);
    assert_eq!((p.width_mm, p.height_mm), (305.0, 420.0));
    assert!(control(&h, SIZE).is_some());

    // The units of the page.
    tap(&mut h, "in");
    assert_eq!(open_page(&h).units, PageUnits::In);
    assert_eq!(readout_text(h.state()), "12.01 × 16.54 in · 1 / 1");
    tap(&mut h, HEIGHT);
    for key in ["1", "1"] {
        tap(&mut h, &format!("{HEIGHT} {key}"));
    }
    tap(&mut h, &format!("{HEIGHT} Done"));
    assert!((open_page(&h).height_mm - 279.4).abs() < 1e-9);
}

#[test]
fn the_page_panel_exports_one_page_or_the_atlas() {
    let mut h = pad();
    h.state_mut().atlas.selection = AtlasSelection::Page;
    h.run_steps(2);
    for name in [EXPORT_PAGE_PNG, EXPORT_PAGE_PDF, EXPORT_ATLAS_PDF] {
        assert!(control(&h, name).is_some(), "{name}");
    }

    let at = control(&h, EXPORT_ATLAS_PDF).unwrap().center();
    tap_with(&mut h, at, Input::Finger);
    h.step();
    assert!(
        h.state()
            .actions
            .contains(&Action::ExportDialog(ExportFormat::AtlasPdf))
    );
}

#[test]
fn the_atlas_shows_the_page_with_its_true_proportions() {
    for size in BOARDS {
        let mut h = pad_with(state(), size);
        for (w, h_mm) in [(297.0, 210.0), (100.0, 400.0), (500.0, 120.0)] {
            let page = h.state().atlas.page;
            let atlas = &mut h.state_mut().doc.project.atlas;
            let record = atlas.page_mut(page).unwrap();
            (record.width_mm, record.height_mm) = (w, h_mm);
            h.run_steps(2);
            let rect = h.state().atlas.place.unwrap().rect;
            let want = w / h_mm;
            let got = f64::from(rect.width() / rect.height());
            assert!((got / want - 1.0).abs() < 0.005, "{size:?}: {got} {want}");
            let screen = Rect::from_min_size(Pos2::ZERO, size.into());
            assert!(screen.contains_rect(rect), "{size:?}");
        }
    }
}

#[test]
fn with_no_selection_no_panel_shows() {
    let mut h = inset_pad(BOARDS[0]);
    assert!(h.state().atlas.panel_rect.is_some());
    // A tap off the page clears the selection.
    let page = h.state().atlas.place.unwrap().rect;
    tap_with(&mut h, page.left_center() - vec2(4.0, 0.0), Input::Finger);
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
    assert_eq!(h.state().atlas.panel_rect, None);
    assert!(control(&h, SCALE).is_none() && control(&h, ADD_PAGE).is_none());
}

#[test]
fn a_tap_opens_the_panel_of_a_frame_or_of_the_page() {
    let mut h = inset_pad(BOARDS[0]);
    let inset = frame(&h, 1).id;
    let main = frame(&h, 0).id;
    let page = h.state().atlas.place.unwrap().rect;
    tap_with(&mut h, page.left_center() - vec2(4.0, 0.0), Input::Finger);

    let at = middle(&h, 1);
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(inset));
    assert!(h.state().atlas.panel_rect.is_some());
    assert!(control(&h, "Frame").is_some() && control(&h, SCALE).is_some());
    let at = middle(&h, 0);
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(main));

    // The margin of the page is outside the frames.
    let at = screen(&h, Point::new(4.0, 50.0));
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(h.state().atlas.selection, AtlasSelection::Page);
    assert!(h.state().atlas.panel_rect.is_some());
    assert!(control(&h, ADD_PAGE).is_some());
    assert!(control(&h, SCALE).is_none());
}

#[test]
fn the_add_button_adds_a_frame_that_shows_the_map_of_the_project() {
    let mut h = pad();
    tap(&mut h, ADD);
    assert!(h.state().atlas.add_rect.is_some());
    // The other items come in other tasks.
    for name in ["Text", "Legend", "Scale bar", "North arrow"] {
        assert!(control(&h, name).is_none(), "{name}");
    }
    check_controls(&h, "add menu");
    tap(&mut h, MAP_FRAME);
    assert_eq!(h.state().atlas.add_rect, None);
    let page = open_page(&h);
    assert_eq!(page.frames.len(), 1);
    let f = &page.frames[0];
    assert_eq!(f.map, h.state().doc.first_map());
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(f.id));
    assert!(h.state().atlas.has_image(f.id));
    let list = &h.state().composed.as_ref().unwrap().list;
    let fills = list.items.iter().filter(|i| matches!(i, Item::Fill { .. }));
    assert!(fills.count() > 10);
}

#[test]
fn a_frame_moves_and_resizes_with_a_finger_a_pen_and_a_mouse() {
    let mut h = inset_pad(BOARDS[0]);
    let ppm = h.state().atlas.place.unwrap().pts_per_mm as f32;
    for input in INPUTS {
        let before = shape_bbox(&frame(&h, 1).shape);
        let from = middle(&h, 1);
        drag(&mut h, from, from - vec2(40.0, 30.0), input);
        let after = shape_bbox(&frame(&h, 1).shape);
        let moved = (after.x0 - before.x0, after.y0 - before.y0);
        let want = (-40.0 / f64::from(ppm), -30.0 / f64::from(ppm));
        assert!((moved.0 - want.0).abs() < 0.1, "{input:?}: {moved:?}");
        assert!((moved.1 - want.1).abs() < 0.1, "{input:?}: {moved:?}");
        let (dw, dh) = (
            after.width() - before.width(),
            after.height() - before.height(),
        );
        assert!(dw.abs() < 1e-9 && dh.abs() < 1e-9, "{input:?}");

        // The bottom-right handle makes the frame smaller.
        let corner = handle(&h, Handle::Resize(4));
        drag(&mut h, corner, corner - vec2(20.0, 10.0), input);
        let resized = shape_bbox(&frame(&h, 1).shape);
        assert_eq!((resized.x0, resized.y0), (after.x0, after.y0), "{input:?}");
        let less = (
            after.width() - resized.width(),
            after.height() - resized.height(),
        );
        assert!(
            (less.0 - 20.0 / f64::from(ppm)).abs() < 0.1,
            "{input:?}: {less:?}"
        );
        assert!(
            (less.1 - 10.0 / f64::from(ppm)).abs() < 0.1,
            "{input:?}: {less:?}"
        );
    }
    // The main frame stays.
    let main = shape_bbox(&frame(&h, 0).shape);
    assert_eq!((main.x0, main.y0), (10.0, 10.0));
}

fn ring_len(h: &Pad, ring: usize) -> usize {
    match &frame(h, 1).shape {
        FrameShape::Polygon(rings) => rings.get(ring).map_or(0, Vec::len),
        FrameShape::Rectangle(_) => 0,
    }
}

#[test]
fn the_vertices_of_a_polygon_frame_move_add_and_go_away() {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, POLYGON);
    assert_eq!(ring_len(&h, 0), 4);
    let ppm = h.state().atlas.place.unwrap().pts_per_mm;
    for input in INPUTS {
        // A drag moves a vertex.
        let FrameShape::Polygon(rings) = frame(&h, 1).shape else {
            panic!("not a polygon");
        };
        let [x, y] = rings[0][0];
        let from = handle(&h, Handle::Vertex((0, 0)));
        drag(&mut h, from, from + vec2(-15.0, 10.0), input);
        let FrameShape::Polygon(rings) = frame(&h, 1).shape else {
            panic!("not a polygon");
        };
        let [nx, ny] = rings[0][0];
        assert!((nx - x + 15.0 / ppm).abs() < 0.1, "{input:?}");
        assert!((ny - y - 10.0 / ppm).abs() < 0.1, "{input:?}");

        // A drag of the middle of an edge adds a vertex.
        let edge = handle(&h, Handle::Edge((0, 1)));
        drag(&mut h, edge, edge + vec2(12.0, 12.0), input);
        assert_eq!(ring_len(&h, 0), 5, "{input:?}");
        assert_eq!(h.state().atlas.vertex, Some((0, 2)));

        // A tap selects a vertex, and two taps remove it.
        let at = handle(&h, Handle::Vertex((0, 3)));
        tap_with(&mut h, at, input);
        assert_eq!(h.state().atlas.vertex, Some((0, 3)), "{input:?}");
        let at = handle(&h, Handle::Vertex((0, 2)));
        double_tap(&mut h, at, input);
        assert_eq!(ring_len(&h, 0), 4, "{input:?}");
    }

    // The Remove vertex button and the Delete key remove the selected vertex.
    assert!(control(&h, REMOVE_VERTEX).is_none());
    let at = handle(&h, Handle::Vertex((0, 1)));
    tap_with(&mut h, at, Input::Finger);
    tap(&mut h, REMOVE_VERTEX);
    assert_eq!(ring_len(&h, 0), 3);
    // The outer ring keeps three vertices.
    let at = handle(&h, Handle::Vertex((0, 1)));
    tap_with(&mut h, at, Input::Finger);
    tap(&mut h, REMOVE_VERTEX);
    assert_eq!(ring_len(&h, 0), 3);
    tap(&mut h, ADD_HOLE);
    assert_eq!(ring_len(&h, 1), 4);
    let at = handle(&h, Handle::Vertex((1, 0)));
    tap_with(&mut h, at, Input::Mouse);
    h.key_press(egui::Key::Delete);
    h.run_steps(2);
    assert_eq!(ring_len(&h, 1), 3);
}

#[test]
fn pan_zoom_and_rotate_in_a_frame_set_its_center_scale_and_rotation() {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, "Move");
    assert_eq!(h.state().atlas.tool, AtlasTool::Move);
    // Only the map of the selected frame moves.
    let at = middle(&h, 0) - vec2(60.0, 0.0);
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(
        h.state().atlas.selection,
        AtlasSelection::Frame(frame(&h, 0).id)
    );

    // One finger, a pen, and the mouse move the map, and the place under
    // the pointer stays under it.
    for input in INPUTS {
        let from = middle(&h, 0) - vec2(60.0, 20.0);
        let to = from + vec2(50.0, 30.0);
        let place = place_at(&mut h, 0, from);
        let center = frame(&h, 0).center;
        drag(&mut h, from, to, input);
        assert_ne!(frame(&h, 0).center, center, "{input:?}");
        let at = pos_of(&mut h, 0, place);
        assert!(at.distance(to) < 1.0, "{input:?}: {at:?} {to:?}");
    }
    // The frame does not move.
    assert_eq!(shape_bbox(&frame(&h, 0).shape).x0, 10.0);

    // Two fingers zoom and turn the map about their middle.
    let id = frame(&h, 0).id;
    h.state_mut()
        .doc
        .project
        .atlas
        .frame_mut(id)
        .unwrap()
        .center = [0.0, 0.0];
    h.run_steps(2);
    let at = middle(&h, 0) - vec2(60.0, 0.0);
    let scale = frame(&h, 0).scale;
    pinch(&mut h, at, 100.0, 150.0, 0.0, Vec2::ZERO);
    let zoomed = frame(&h, 0).scale;
    assert!((scale / zoomed - 1.5).abs() < 0.01, "{scale} {zoomed}");
    let place = place_at(&mut h, 0, at);
    pinch(&mut h, at, 120.0, 120.0, 30.0, Vec2::ZERO);
    let rotation = frame(&h, 0).rotation;
    assert!((rotation - 30.0).abs() < 0.5, "{rotation}");
    assert!(pos_of(&mut h, 0, place).distance(at) < 1.0);
    assert!((frame(&h, 0).scale / zoomed - 1.0).abs() < 0.01);
    // Two fingers also move the map, and never the frame.
    let center = frame(&h, 0).center;
    pinch(&mut h, at, 120.0, 120.0, 0.0, vec2(40.0, 0.0));
    assert_ne!(frame(&h, 0).center, center);
    assert_eq!(shape_bbox(&frame(&h, 0).shape).x0, 10.0);

    // The wheel zooms at the pointer.
    let scale = frame(&h, 0).scale;
    h.event(egui::Event::PointerMoved(at));
    h.step();
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 200.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(10);
    assert!(frame(&h, 0).scale < scale / 1.05);

    // Alt and a drag turn the map.
    let rotation = frame(&h, 0).rotation;
    h.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    let c = middle(&h, 0);
    drag(
        &mut h,
        c + vec2(100.0, 0.0),
        c + vec2(0.0, 100.0),
        Input::Mouse,
    );
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    let turned = (frame(&h, 0).rotation - rotation).rem_euclid(360.0);
    assert!((turned - 90.0).abs() < 1.0, "{turned}");
}

#[test]
fn a_globe_edit_composes_each_frame_again() {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, "World");
    assert_eq!(h.state().workspace, Workspace::Globe);
    let composes = h.state().atlas.composes;
    // The point tool adds a point as this does.
    let s = h.state_mut();
    let places = s
        .doc
        .project
        .layers()
        .iter()
        .find(|l| l.name == "Places")
        .unwrap()
        .id;
    let point = Feature::new(Geometry::Points(vec![[10.0, 10.0]]), Default::default());
    assert!(s.doc.project.add_feature(places, point));
    s.pipeline.forget(places);
    s.touch();
    h.run_steps(2);
    assert_eq!(h.state().atlas.composes, composes);
    tap(&mut h, "Atlas");
    assert_eq!(h.state().workspace, Workspace::Atlas);
    assert_eq!(h.state().atlas.composes, composes + 2);
}

#[test]
fn a_change_of_the_projection_shows_in_each_frame_of_the_map() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).id);
    h.state_mut()
        .doc
        .project
        .atlas
        .frame_mut(inset)
        .unwrap()
        .map = main;
    h.run_steps(2);
    tap(&mut h, "Projection");
    assert_eq!(h.state().atlas.tab, FrameTab::Projection);
    let composes = h.state().atlas.composes;
    tap(&mut h, PROJECTION);
    tap(&mut h, ProjectionKind::Mercator.name());
    assert_eq!(first_projection(&h).kind, ProjectionKind::Mercator);
    assert_eq!(h.state().atlas.composes, composes + 2);

    // The projection centers on the place in the middle of the frame.
    let id = frame(&h, 1).id;
    let s = h.state_mut();
    s.doc.project.atlas.frame_mut(id).unwrap().center = [40.0, 20.0];
    h.run_steps(2);
    tap(&mut h, CENTER_ON_FRAME);
    assert_eq!(first_projection(&h).lon0, 40.0);
}

#[test]
fn a_frame_changes_the_projection_of_its_own_map_only() {
    let mut h = inset_pad(BOARDS[0]);
    let source = frame(&h, 0).map;
    let copy = frame(&h, 1).map;
    h.run_steps(2);
    tap(&mut h, "Projection");
    tap(&mut h, PROJECTION);
    tap(&mut h, ProjectionKind::Mercator.name());
    let doc = &h.state().doc;
    assert_eq!(doc.projection(copy).kind, ProjectionKind::Mercator);
    assert_ne!(doc.projection(source).kind, ProjectionKind::Mercator);
}

#[test]
fn the_projection_tab_switches_the_map_through_all_presets() {
    for size in BOARDS {
        let mut h = inset_pad(size);
        let map = frame(&h, 1).map;
        tap(&mut h, "Projection");
        for kind in ProjectionKind::ALL {
            scroll_up_to(&mut h, PROJECTION);
            tap(&mut h, PROJECTION);
            scroll_to(&mut h, kind.name());
            tap(&mut h, kind.name());
            assert_eq!(h.state().doc.projection(map).kind, kind, "{size:?}");
            h.run_steps(3);
            check_controls(&h, kind.name());
            let state = h.state_mut();
            let projection = state.pipeline.projection(&state.doc, map).unwrap();
            assert!(projection.bounds().width() > 0.0, "{kind:?}");
            let text = state.doc.project.atlas.map(map).unwrap().projection.clone();
            assert!(text.starts_with("+proj="), "{text}");
        }
    }
}

#[test]
fn a_conic_projection_has_two_parallels_and_an_oblique_aspect_a_tilt() {
    let mut h = inset_pad(BOARDS[0]);
    let map = frame(&h, 1).map;
    tap(&mut h, "Projection");
    assert!(control(&h, FIRST_PARALLEL).is_none());
    tap(&mut h, PROJECTION);
    tap(&mut h, ProjectionKind::LambertConformal.name());
    h.run_steps(2);
    assert!(control(&h, FIRST_PARALLEL).is_some());
    let spec = h.state().doc.projection(map);
    assert_eq!((spec.lat1, spec.lat2), (30.0, 60.0));

    tap(&mut h, PROJECTION);
    tap(&mut h, ProjectionKind::EqualEarth.name());
    h.run_steps(2);
    assert!(control(&h, FIRST_PARALLEL).is_none());
    assert_eq!(h.state().doc.projection(map).azimuth, None);
    tap(&mut h, OBLIQUE);
    h.run_steps(2);
    assert!(h.state().doc.projection(map).azimuth.is_some());
    let state = h.state_mut();
    let text = state.doc.project.atlas.map(map).unwrap().projection.clone();
    assert!(text.starts_with("+proj=ob_tran +o_proj=eqearth"), "{text}");
    assert!(state.pipeline.projection(&state.doc, map).is_ok());
    tap(&mut h, OBLIQUE);
    h.run_steps(2);
    assert_eq!(h.state().doc.projection(map).azimuth, None);
}

#[test]
fn a_custom_projection_is_saved_in_the_project_and_picked_again() {
    let mut h = inset_pad(BOARDS[0]);
    let map = frame(&h, 1).map;
    tap(&mut h, "Projection");
    tap(&mut h, PROJECTION);
    tap(&mut h, ProjectionKind::AlbersEqualArea.name());
    h.run_steps(2);
    assert!(h.state().atlas.preset_name.is_empty());
    h.state_mut().atlas.preset_name = "Home".into();
    h.run_steps(2);
    tap(&mut h, SAVE_PROJECTION);
    let saved = h.state().doc.projection(map).to_proj();
    assert_eq!(h.state().doc.presets().len(), 1);
    assert_eq!(h.state().doc.presets()[0].projection, saved);

    // Another projection, and then the custom one again.
    tap(&mut h, PROJECTION);
    tap(&mut h, ProjectionKind::Mercator.name());
    h.run_steps(2);
    assert_eq!(h.state().doc.projection(map).kind, ProjectionKind::Mercator);
    tap(&mut h, PROJECTION);
    scroll_to(&mut h, &custom_row("Home"));
    tap(&mut h, &custom_row("Home"));
    assert_eq!(h.state().doc.projection(map).to_proj(), saved);

    tap(&mut h, DELETE_PROJECTION);
    assert!(h.state().doc.presets().is_empty());
    assert_eq!(h.state().doc.projection(map).to_proj(), saved);
}

#[test]
fn the_tabs_of_a_frame_work_with_a_finger() {
    for size in BOARDS {
        let mut h = inset_pad(size);
        check_controls(&h, "frame tab");
        // The Labels tab comes in another task.
        assert!(control(&h, "Labels").is_none());
        tap(&mut h, POLYGON);
        check_controls(&h, "polygon");
        tap(&mut h, SCALE);
        for key in ["5", "0", "0", "0", "0", "0", "0", "0"] {
            tap(&mut h, &format!("{SCALE} {key}"));
        }
        check_controls(&h, "keypad");
        tap(&mut h, &format!("{SCALE} Done"));
        assert_eq!(frame(&h, 1).scale, 50_000_000.0, "{size:?}");
        tap(&mut h, "Projection");
        check_controls(&h, "projection tab");
        tap(&mut h, PROJECTION);
        check_controls(&h, "projection list");
        tap(&mut h, ProjectionKind::Mercator.name());
        assert_eq!(selected_projection(&h).kind, ProjectionKind::Mercator);
        let at = screen(&h, Point::new(4.0, 50.0));
        tap_with(&mut h, at, Input::Finger);
        assert_eq!(h.state().atlas.selection, AtlasSelection::Page, "{size:?}");
        check_controls(&h, "page panel");
        tap(&mut h, SIZE);
        check_controls(&h, "page sizes");
        let inset = frame(&h, 1).id;
        h.state_mut().atlas.selection = AtlasSelection::Frame(inset);
        h.state_mut().atlas.tab = FrameTab::Frame;
        h.run_steps(3);
        scroll_to(&mut h, DELETE_FRAME);
        assert!(control(&h, DELETE_FRAME).is_some(), "{size:?}");
        check_controls(&h, "bottom of the frame tab");
    }
}

#[test]
fn the_maps_stay_still_during_a_drag_and_compose_after_it() {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, "Move");
    let from = middle(&h, 0) - vec2(60.0, 0.0);
    tap_with(&mut h, from, Input::Finger);
    let composes = h.state().atlas.composes;
    let center = frame(&h, 0).center;
    let to = from + vec2(80.0, 40.0);
    drag_steps(&mut h, from, to, Input::Finger, 10);
    assert_eq!(h.state().atlas.composes, composes);
    assert_ne!(frame(&h, 0).center, center);
    release(&mut h, to, Input::Finger);
    assert_eq!(h.state().atlas.composes, composes + 1);
}

/// The center, the scale, the rotation, and the shape of each frame of the
/// open page.
fn maps(h: &Pad) -> Vec<String> {
    let frames = open_page(h).frames;
    let map = |f: &FrameRecord| format!("{:?}", (f.center, f.scale, f.rotation, &f.shape));
    frames.iter().map(map).collect()
}

fn place(h: &Pad) -> vale_app::ui::atlas::PagePlace {
    h.state().atlas.place.unwrap()
}

/// A place on the paper in the margin of the page.
fn margin(h: &Pad) -> Pos2 {
    screen(h, Point::new(4.0, 50.0))
}

#[test]
fn a_pinch_on_the_paper_zooms_and_moves_the_page_and_not_the_maps() {
    let mut h = inset_pad(BOARDS[0]);
    let (before, maps_before) = (place(&h), maps(&h));
    let at = margin(&h);
    let mm = before.to_mm(at);
    pinch(&mut h, at, 100.0, 150.0, 0.0, Vec2::ZERO);
    let after = place(&h);
    let zoom = after.pts_per_mm / before.pts_per_mm;
    assert!((zoom - 1.5).abs() < 0.01, "{zoom}");
    assert!(after.to_mm(at).distance(mm) < 0.5);
    assert_eq!(maps(&h), maps_before);
    // The middle of the fingers moves the page.
    pinch(&mut h, at, 120.0, 120.0, 0.0, vec2(40.0, 20.0));
    let moved = place(&h).rect.min - after.rect.min;
    assert!((moved - vec2(40.0, 20.0)).length() < 1.0, "{moved:?}");
    assert_eq!(maps(&h), maps_before);
}

#[test]
fn a_drag_on_the_paper_moves_the_page() {
    let mut h = inset_pad(BOARDS[0]);
    let maps_before = maps(&h);
    for tool in ["Move", "Select"] {
        tap(&mut h, tool);
        for input in INPUTS {
            let before = place(&h);
            let from = margin(&h);
            drag(&mut h, from, from + vec2(60.0, 30.0), input);
            let moved = place(&h).rect.min - before.rect.min;
            assert!(
                (moved - vec2(60.0, 30.0)).length() < 1.0,
                "{tool} {input:?}: {moved:?}"
            );
            assert_eq!(place(&h).pts_per_mm, before.pts_per_mm);
        }
    }
    // A drag on a frame that is not selected also moves the page with the
    // Move tool.
    tap(&mut h, "Move");
    let before = place(&h);
    let from = middle(&h, 0) - vec2(60.0, 0.0);
    drag(&mut h, from, from + vec2(-40.0, 20.0), Input::Finger);
    let moved = place(&h).rect.min - before.rect.min;
    assert!((moved - vec2(-40.0, 20.0)).length() < 1.0, "{moved:?}");
    assert_eq!(maps(&h), maps_before);
}

#[test]
fn a_pinch_in_the_selected_frame_changes_only_that_frame() {
    let mut h = inset_pad(BOARDS[0]);
    let (before, maps_before) = (place(&h), maps(&h));
    let scale = frame(&h, 1).scale;
    let at = middle(&h, 1);
    pinch(&mut h, at, 40.0, 60.0, 0.0, Vec2::ZERO);
    assert_eq!(place(&h), before);
    assert!((scale / frame(&h, 1).scale - 1.5).abs() < 0.01);
    assert_eq!(maps(&h)[0], maps_before[0]);
}

#[test]
fn a_pinch_in_a_frame_that_is_not_selected_zooms_the_page() {
    let mut h = inset_pad(BOARDS[0]);
    let (before, maps_before) = (place(&h), maps(&h));
    let at = middle(&h, 0) - vec2(60.0, 0.0);
    pinch(&mut h, at, 100.0, 150.0, 0.0, Vec2::ZERO);
    let zoom = place(&h).pts_per_mm / before.pts_per_mm;
    assert!((zoom - 1.5).abs() < 0.01, "{zoom}");
    assert_eq!(maps(&h), maps_before);
}

#[test]
fn a_double_tap_off_the_page_fits_the_page_again() {
    let mut h = inset_pad(BOARDS[0]);
    let fit = place(&h);
    let at = margin(&h);
    pinch(&mut h, at, 150.0, 100.0, 0.0, vec2(30.0, 0.0));
    assert_ne!(place(&h), fit);
    let off = pos2(place(&h).rect.center().x, place(&h).rect.bottom() + 30.0);
    double_tap(&mut h, off, Input::Finger);
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
    assert!((place(&h).pts_per_mm - fit.pts_per_mm).abs() < 1e-9);
    assert!(place(&h).rect.min.distance(fit.rect.min) < 0.01);
    // A page that opens also fits.
    let s = h.state_mut();
    s.atlas.view.zoom = 3.0;
    vale_app::ui::atlas::add_page(s);
    h.run_steps(2);
    assert_eq!(h.state().atlas.view.zoom, 1.0);
}

#[test]
fn at_a_page_zoom_of_20_the_texture_of_a_frame_is_no_larger_than_the_canvas() {
    let mut h = inset_pad(BOARDS[0]);
    h.state_mut().atlas.view.zoom = 20.0;
    h.run_steps(3);
    let (w, hh) = h.state().canvas_size;
    let main = frame(&h, 0).id;
    let size = h.state().atlas.image_size(main).unwrap();
    assert!(size[0] as f64 <= w && size[1] as f64 <= hh, "{size:?}");
    for f in open_page(&h).frames {
        if let Some(size) = h.state().atlas.image_size(f.id) {
            assert!(size[0] as f64 <= w && size[1] as f64 <= hh, "{size:?}");
        }
    }
    // The handles keep their size.
    for (handle, rect) in &h.state().atlas.handles {
        assert!(
            (rect.size() - vec2(44.0, 44.0)).length() < 0.01,
            "{handle:?}"
        );
    }
}

#[test]
fn each_tab_of_a_frame_is_as_wide_as_its_text_and_a_padding() {
    for size in BOARDS {
        let h = inset_pad(size);
        for name in FRAME_TABS {
            let rect = h.get_by_label(name).rect();
            let font = vale_app::ui::pad::theme::body();
            let text = h.ctx.fonts_mut(|f| {
                let galley = f.layout_no_wrap(name.to_owned(), font, egui::Color32::WHITE);
                galley.size().x
            });
            assert!(
                rect.width() >= text + 24.0,
                "{size:?} {name}: {rect:?} {text}"
            );
        }
    }
}

#[test]
fn the_panel_of_the_page_adds_a_page_and_deletes_the_open_page() {
    let mut h = inset_pad(BOARDS[0]);
    let at = screen(&h, Point::new(4.0, 50.0));
    tap_with(&mut h, at, Input::Finger);
    assert!(control(&h, DELETE_PAGE).is_none());
    tap(&mut h, ADD_PAGE);
    let atlas = &h.state().doc.project.atlas;
    assert_eq!(atlas.pages.len(), 2);
    assert_eq!(h.state().atlas.page, atlas.pages[1].id);
    assert!(atlas.pages[1].frames.is_empty());
    assert_eq!(h.state().atlas.selection, AtlasSelection::Page);
    assert_eq!(readout_text(h.state()), "297 × 210 mm · 2 / 2");
    tap(&mut h, DELETE_PAGE);
    let atlas = &h.state().doc.project.atlas;
    assert_eq!(atlas.pages.len(), 1);
    assert_eq!(h.state().atlas.page, atlas.pages[0].id);
    assert_eq!(atlas.pages[0].frames.len(), 2);
}

#[test]
fn the_frame_tab_deletes_the_frame_and_an_undo_brings_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    add_frame(&mut h);
    tap(&mut h, POLYGON);
    h.state_mut().save_waiting().unwrap();
    let inset = frame(&h, 1);
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(inset.id));

    scroll_to(&mut h, DELETE_FRAME);
    tap(&mut h, DELETE_FRAME);
    let page = open_page(&h);
    assert_eq!(page.frames.len(), 1);
    assert!(page.frames.iter().all(|f| f.id != inset.id));
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
    assert_eq!(h.state().atlas.vertex, None);
    assert!(!h.state().atlas.has_image(inset.id));
    assert_eq!(h.state().atlas.panel_rect, None);
    assert!(control(&h, DELETE_FRAME).is_none());

    h.state_mut().save_waiting().unwrap();
    tap(&mut h, "Undo");
    let page = open_page(&h);
    assert_eq!(page.frames.len(), 2);
    assert_eq!(page.frames[1], inset);
}

#[test]
fn the_page_controls_go_to_the_previous_page_and_the_next_page() {
    let mut s = state();
    let atlas = &mut s.doc.project.atlas;
    let first = atlas.pages[0].id;
    let second = atlas.add_page(Some(first), 297.0, 210.0, PageUnits::Mm);
    let third = atlas.add_page(Some(second), 279.4, 215.9, PageUnits::In);
    atlas.page_mut(third).unwrap().name = "Vesperan".to_string();
    let mut h = pad_with(s, BOARDS[0]);
    assert_eq!(h.state().atlas.page, first);
    assert!(control(&h, PREVIOUS_PAGE).is_none());
    tap(&mut h, NEXT_PAGE);
    assert_eq!(h.state().atlas.page, second);
    tap(&mut h, NEXT_PAGE);
    assert_eq!(h.state().atlas.page, third);
    assert!(control(&h, NEXT_PAGE).is_none());
    assert_eq!(page_title(2, "Vesperan"), "Page 3 · Vesperan");
    assert_eq!(readout_text(h.state()), "11 × 8.5 in · 3 / 3");
    tap(&mut h, PREVIOUS_PAGE);
    assert_eq!(h.state().atlas.page, second);
    for size in BOARDS {
        let h = pad_with(state(), size);
        for name in [ADD, "Select", "Move"] {
            assert!(control(&h, name).is_some(), "{size:?}: {name}");
        }
        check_controls(&h, "top row");
    }
}

#[test]
fn pages_and_frames_survive_a_close_and_an_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut s = state();
    s.doc = doc;
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    add_frame(&mut h);
    tap(&mut h, POLYGON);
    tap(&mut h, ADD_HOLE);
    let at = screen(&h, Point::new(4.0, 50.0));
    tap_with(&mut h, at, Input::Finger);
    tap(&mut h, ADD_PAGE);
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.project.atlas, project.atlas);
    assert_eq!(doc.project.atlas.pages[0].frames.len(), 2);
}

#[test]
fn the_next_start_opens_the_last_page() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_project_path(dir.path(), "Borea");
    let world = World {
        name: "Borea".to_string(),
        ..World::default()
    };
    let mut project = Project::new(world);
    let first = project.atlas.pages[0].id;
    let second = project
        .atlas
        .add_page(Some(first), 297.0, 210.0, PageUnits::Mm);
    ProjectFile::create(&path, &project).unwrap();
    let app = |dir: &std::path::Path| {
        let mut s = AppState::new(Document::empty()).unwrap();
        s.headless = true;
        s.set_layout(Layout::Pad);
        s.globe.set_face_size(256);
        s.projects = Locations::single(dir);
        s.restore();
        pad_with(s, BOARDS[0])
    };

    let mut h = app(dir.path());
    h.state_mut().show_projects();
    h.run_steps(2);
    h.get_by_label("Open Borea").click();
    for _ in 0..3 {
        h.run_steps(2);
        for action in std::mem::take(&mut h.state_mut().actions) {
            h.state_mut().run_action(action);
        }
    }
    assert_eq!(h.state().screen, Screen::Editor);
    h.state_mut().workspace = Workspace::Atlas;
    h.run_steps(2);
    tap(&mut h, NEXT_PAGE);
    assert_eq!(h.state().atlas.page, second);
    h.state_mut().save_tick(false, Instant::now());
    drop(h);

    let h = app(dir.path());
    assert_eq!(h.state().workspace, Workspace::Atlas);
    assert_eq!(h.state().atlas.page, second);
}

/// One test at a time makes a wgpu device.
fn one_gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn the_pad_atlas_renders_a_page_with_a_main_frame_and_an_inset() {
    let _gpu = one_gpu_test();
    let prepare = |s: &mut AppState| {
        vale_app::ui::atlas::add_frame(s);
        vale_app::ui::atlas::add_frame(s);
        Ok(())
    };
    let (img, state) = vale_app::headless::ui_png_with(state(), (1194.0, 834.0), 1.0, prepare)
        .unwrap_or_else(|e| panic!("{e}"));
    std::fs::create_dir_all("../../target/app").unwrap();
    img.save("../../target/app/pad-atlas.png").unwrap();
    let place = state.atlas.place.unwrap();
    // The paper is white in the margin of the page.
    let margin = place.to_screen(Point::new(4.0, 50.0));
    let paper = img.get_pixel(margin.x as u32, margin.y as u32).0;
    assert_eq!(paper, [255, 255, 255, 255]);
    let colors: std::collections::HashSet<_> = img.pixels().map(|p| p.0).collect();
    assert!(colors.len() > 50, "{}", colors.len());
}

/// The names of the maps of the project, in atlas order.
fn map_names(h: &Pad) -> Vec<String> {
    let maps = &h.state().doc.project.atlas.maps;
    maps.iter().map(|m| m.name.clone()).collect()
}

/// The name of the map of a frame of the open page.
fn map_of(h: &Pad, index: usize) -> String {
    let map = frame(h, index).map;
    h.state().doc.project.atlas.map(map).unwrap().name.clone()
}

/// Types a text in place of the text of a field.
fn type_text(h: &mut Pad, name: &str, text: &str) {
    tap(h, name);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.event(egui::Event::Text(text.to_owned()));
    h.run_steps(2);
}

#[test]
fn each_new_frame_shows_a_map_of_its_own() {
    let h = inset_pad(BOARDS[0]);
    assert_eq!(map_names(&h), ["Map", "Map 2"]);
    assert_eq!(map_of(&h, 0), "Map");
    assert_eq!(map_of(&h, 1), "Map 2");
    assert_ne!(frame(&h, 0).map, frame(&h, 1).map);
}

#[test]
fn the_frame_tab_renames_the_map_of_the_frame() {
    for size in BOARDS {
        let mut h = inset_pad(size);
        check_controls(&h, "frame tab");
        type_text(&mut h, MAP_NAME, "Vesperan");
        assert_eq!(map_names(&h), ["Map", "Vesperan"], "{size:?}");
        assert_eq!(map_of(&h, 1), "Vesperan");
        check_controls(&h, "renamed");
    }
}

#[test]
fn deleting_a_frame_deletes_its_map_but_the_last_map_stays() {
    let mut h = inset_pad(BOARDS[0]);
    scroll_to(&mut h, DELETE_FRAME);
    tap(&mut h, DELETE_FRAME);
    assert_eq!(map_names(&h), ["Map"]);
    let first = h.state().doc.project.atlas.pages[0].frames[0].id;
    h.state_mut().atlas.selection = AtlasSelection::Frame(first);
    h.run_steps(2);
    scroll_to(&mut h, DELETE_FRAME);
    tap(&mut h, DELETE_FRAME);
    assert_eq!(map_names(&h), ["Map"]);
    // The next frame takes the map that no frame shows.
    add_frame(&mut h);
    assert_eq!(map_names(&h), ["Map"]);
}

#[test]
fn many_maps_survive_a_close_and_an_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut s = state();
    s.doc = doc;
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    add_frame(&mut h);
    type_text(&mut h, MAP_NAME, "Vesperan");
    assert_eq!(map_names(&h), ["Map", "Vesperan"]);
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.project.atlas, project.atlas);
    assert_eq!(doc.project.atlas.maps.len(), 2);
}

/// The ID of a layer by its name.
fn layer_id(h: &Pad, name: &str) -> LayerId {
    let layers = h.state().doc.project.layers();
    layers.iter().find(|l| l.name == name).unwrap().id
}

/// True if a map shows a layer.
fn shows(h: &Pad, map: MapId, layer: &str) -> bool {
    h.state().doc.shown(map, layer_id(h, layer))
}

#[test]
fn the_layers_tab_turns_a_layer_off_in_the_map_of_the_frame_only() {
    for size in BOARDS {
        let mut h = inset_pad(size);
        let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
        tap(&mut h, FRAME_TABS[2]);
        assert_eq!(h.state().atlas.tab, FrameTab::Layers);
        check_controls(&h, "layers tab");
        tap(&mut h, &eye_name("Rivers", true));
        assert!(!shows(&h, inset, "Rivers"), "{size:?}");
        assert!(shows(&h, main, "Rivers") && shows(&h, inset, "Land"));
        assert!(control(&h, &eye_name("Rivers", false)).is_some());
        check_controls(&h, "a layer is off");

        // The frame of the other map shows its own switches.
        let first = frame(&h, 0).id;
        h.state_mut().atlas.selection = AtlasSelection::Frame(first);
        h.run_steps(3);
        assert!(control(&h, &eye_name("Rivers", true)).is_some(), "{size:?}");
    }
}

#[test]
fn a_change_of_a_switch_shows_in_each_frame_of_the_map() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).id);
    h.state_mut()
        .doc
        .project
        .atlas
        .frame_mut(inset)
        .unwrap()
        .map = main;
    h.run_steps(2);
    tap(&mut h, FRAME_TABS[2]);
    let composes = h.state().atlas.composes;
    tap(&mut h, &eye_name("Land", true));
    assert!(!shows(&h, main, "Land"));
    assert_eq!(h.state().atlas.composes, composes + 2);
}

#[test]
fn the_eye_of_the_world_workspace_does_not_change_a_map() {
    let mut h = inset_pad(BOARDS[0]);
    let land = layer_id(&h, "Land");
    h.state_mut().set_layer_visible(land, false);
    h.run_steps(2);
    let maps: Vec<MapId> = h
        .state()
        .doc
        .project
        .atlas
        .maps
        .iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(maps.len(), 2);
    for map in maps {
        assert!(shows(&h, map, "Land"));
    }
    tap(&mut h, FRAME_TABS[2]);
    assert!(control(&h, &eye_name("Land", true)).is_some());
}

#[test]
fn a_new_layer_is_on_in_each_map() {
    let mut h = inset_pad(BOARDS[0]);
    let inset = frame(&h, 1).map;
    tap(&mut h, FRAME_TABS[2]);
    tap(&mut h, &eye_name("Land", true));
    let added = h.state_mut().doc.add_layer(GeometryKind::Line);
    for map in h.state().doc.project.atlas.maps.iter() {
        assert!(map.hidden.iter().all(|l| *l != added));
    }
    assert!(h.state().doc.shown(inset, added));
    assert!(!shows(&h, inset, "Land"));
}

#[test]
fn each_change_of_a_switch_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    h.state_mut().save_waiting().unwrap();
    let map = frame(&h, 0).map;
    tap(&mut h, FRAME_TABS[2]);
    tap(&mut h, &eye_name("Land", true));
    tap(&mut h, &eye_name("Rivers", true));
    assert!(!shows(&h, map, "Land") && !shows(&h, map, "Rivers"));

    tap(&mut h, "Undo");
    assert!(!shows(&h, map, "Land") && shows(&h, map, "Rivers"));
    tap(&mut h, "Undo");
    assert!(shows(&h, map, "Land") && shows(&h, map, "Rivers"));
}

#[test]
fn the_switches_of_each_map_survive_a_close_and_an_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut s = state();
    s.doc = doc;
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    add_frame(&mut h);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    tap(&mut h, FRAME_TABS[2]);
    tap(&mut h, &eye_name("Places", true));
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.project.atlas, project.atlas);
    let places = layer_id(&h, "Places");
    assert!(!doc.shown(inset, places) && doc.shown(main, places));
}

/// The strokes of a frame of the open page with a color and a width.
fn strokes(h: &mut Pad, index: usize, rgb: [u8; 3], line: f64) -> usize {
    use vale_app::atlas::{FrameTarget, PT_PER_MM};
    use vale_app::pipeline::{Quality, fonts};
    let page = open_page(h);
    let f = page.frames[index].clone();
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let mut fonts = fonts().unwrap();
    let s = h.state_mut();
    let composed = s
        .pipeline
        .compose_frame(&s.doc, &page, &f, target, &mut fonts, Quality::Final, None)
        .unwrap();
    let color = vale_render::Color::rgb(rgb[0], rgb[1], rgb[2]);
    (composed.list.items.iter())
        .filter(
            |i| matches!(i, Item::Stroke { color: c, width, .. } if *c == color && *width == line),
        )
        .count()
}

#[test]
fn a_hand_set_interval_stays_through_a_zoom() {
    let mut h = inset_pad(BOARDS[0]);
    assert_eq!(frame(&h, 1).graticule.interval, None);
    scroll_to(&mut h, INTERVAL);
    let slider = control(&h, INTERVAL).unwrap();
    let from = slider.center();
    drag(&mut h, from, from - vec2(60.0, 0.0), Input::Finger);
    let set = frame(&h, 1).graticule.interval;
    assert!(set.is_some());
    assert!(control(&h, AUTO_INTERVAL).is_some());

    let scale = frame(&h, 1).scale;
    let at = middle(&h, 1);
    pinch(&mut h, at, 40.0, 80.0, 0.0, Vec2::ZERO);
    assert!(frame(&h, 1).scale < scale / 1.5);
    assert_eq!(frame(&h, 1).graticule.interval, set);

    // The switch puts the interval back on the scale of the frame.
    scroll_to(&mut h, AUTO_INTERVAL);
    tap(&mut h, AUTO_INTERVAL);
    assert_eq!(frame(&h, 1).graticule.interval, None);
    tap(&mut h, AUTO_INTERVAL);
    let id = frame(&h, 1).id;
    let (_, auto) = vale_app::ui::atlas::graticule_intervals(h.state_mut(), id).unwrap();
    assert_eq!(frame(&h, 1).graticule.interval, Some(auto));
}

#[test]
fn two_frames_of_one_map_have_their_own_graticule() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).id);
    h.state_mut()
        .doc
        .project
        .atlas
        .frame_mut(inset)
        .unwrap()
        .map = main;
    h.run_steps(2);
    let first = frame(&h, 0).graticule;
    scroll_to(&mut h, GRATICULE_COLOR);
    tap(&mut h, GRATICULE_COLOR);
    let shade = format!("{GRATICULE_COLOR} shade");
    scroll_to(&mut h, &shade);
    let square = control(&h, &shade).unwrap();
    tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
    let color = frame(&h, 1).graticule.color;
    assert_ne!(color, first.color);
    assert_eq!(frame(&h, 0).graticule, first);
    assert!(strokes(&mut h, 0, first.color, first.width) > 0);
    assert_eq!(strokes(&mut h, 0, color, first.width), 0);
    assert!(strokes(&mut h, 1, color, first.width) > 0);

    scroll_up_to(&mut h, GRATICULE);
    tap(&mut h, GRATICULE);
    assert!(!frame(&h, 1).graticule.on);
    assert_eq!(frame(&h, 0).graticule, first);
    assert_eq!(strokes(&mut h, 1, color, first.width), 0);
    assert!(strokes(&mut h, 0, first.color, first.width) > 0);
}

#[test]
fn the_graticule_controls_work_with_a_finger() {
    let hue = format!("{GRATICULE_COLOR} hue");
    let shade = format!("{GRATICULE_COLOR} shade");
    for size in BOARDS {
        let mut h = inset_pad(size);
        scroll_to(&mut h, GRATICULE);
        tap(&mut h, GRATICULE);
        assert!(!frame(&h, 1).graticule.on, "{size:?}");
        assert!(control(&h, INTERVAL).is_none(), "{size:?}");
        tap(&mut h, GRATICULE);
        assert!(frame(&h, 1).graticule.on, "{size:?}");
        check_controls(&h, "graticule");

        scroll_to(&mut h, AUTO_INTERVAL);
        tap(&mut h, AUTO_INTERVAL);
        assert!(frame(&h, 1).graticule.interval.is_some(), "{size:?}");

        scroll_to(&mut h, GRATICULE_COLOR);
        tap(&mut h, GRATICULE_COLOR);
        scroll_to(&mut h, &hue);
        check_controls(&h, "graticule color");
        let before = frame(&h, 1).graticule.color;
        let square = control(&h, &shade).unwrap();
        tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
        assert_ne!(frame(&h, 1).graticule.color, before, "{size:?}");

        scroll_to(&mut h, GRATICULE_WIDTH);
        let slider = control(&h, GRATICULE_WIDTH).unwrap();
        tap_with(
            &mut h,
            slider.right_center() - vec2(70.0, 0.0),
            Input::Finger,
        );
        let width = frame(&h, 1).graticule.width;
        assert!(width > 1.5, "{size:?}: {width}");
        assert_eq!((width * 10.0).round() / 10.0, width);
        check_controls(&h, "graticule width");
    }
}

#[test]
fn the_graticule_settings_survive_a_close_and_an_open() {
    use vale_app::ui::atlas::{
        set_graticule, set_graticule_color, set_graticule_interval, set_graticule_width,
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut s = state();
    s.doc = doc;
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    add_frame(&mut h);
    let (main, inset) = (frame(&h, 0).id, frame(&h, 1).id);
    let st = h.state_mut();
    set_graticule_interval(st, main, Some(5.0 / 60.0));
    set_graticule_color(st, main, [10, 20, 30]);
    set_graticule_width(st, main, 1.3);
    set_graticule(st, inset, false);
    h.run_steps(2);
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.project.atlas, project.atlas);
    let g = &doc.project.atlas.frame(main).unwrap().graticule;
    assert!(g.on);
    assert_eq!(g.interval, Some(5.0 / 60.0));
    assert_eq!(g.color, [10, 20, 30]);
    assert_eq!(g.width, 1.3);
    assert!(!doc.project.atlas.frame(inset).unwrap().graticule.on);
}

#[test]
fn two_frames_of_one_map_have_their_own_border() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).id);
    h.state_mut()
        .doc
        .project
        .atlas
        .frame_mut(inset)
        .unwrap()
        .map = main;
    h.run_steps(2);
    let first = frame(&h, 0).border;
    let shade = format!("{BORDER_COLOR} shade");
    scroll_to(&mut h, BORDER_COLOR);
    tap(&mut h, BORDER_COLOR);
    scroll_to(&mut h, &shade);
    let square = control(&h, &shade).unwrap();
    tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
    let color = frame(&h, 1).border.color;
    assert_ne!(color, first.color);
    assert_eq!(frame(&h, 0).border, first);
    assert!(strokes(&mut h, 0, first.color, first.width) > 0);
    assert_eq!(strokes(&mut h, 0, color, first.width), 0);
    assert!(strokes(&mut h, 1, color, first.width) > 0);

    scroll_up_to(&mut h, BORDER);
    tap(&mut h, BORDER);
    assert!(!frame(&h, 1).border.on);
    assert_eq!(frame(&h, 0).border, first);
    assert_eq!(strokes(&mut h, 1, color, first.width), 0);
    assert!(strokes(&mut h, 0, first.color, first.width) > 0);
}

#[test]
fn the_border_controls_work_with_a_finger() {
    let hue = format!("{BORDER_COLOR} hue");
    let shade = format!("{BORDER_COLOR} shade");
    for size in BOARDS {
        let mut h = inset_pad(size);
        scroll_to(&mut h, BORDER);
        assert!(frame(&h, 1).border.on, "{size:?}");
        check_controls(&h, "border");
        tap(&mut h, BORDER);
        assert!(!frame(&h, 1).border.on, "{size:?}");
        assert!(control(&h, BORDER_WIDTH).is_none(), "{size:?}");
        tap(&mut h, BORDER);
        assert!(frame(&h, 1).border.on, "{size:?}");

        scroll_to(&mut h, BORDER_COLOR);
        tap(&mut h, BORDER_COLOR);
        scroll_to(&mut h, &hue);
        check_controls(&h, "border color");
        let before = frame(&h, 1).border.color;
        let square = control(&h, &shade).unwrap();
        tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
        assert_ne!(frame(&h, 1).border.color, before, "{size:?}");

        scroll_to(&mut h, BORDER_WIDTH);
        let slider = control(&h, BORDER_WIDTH).unwrap();
        tap_with(
            &mut h,
            slider.right_center() - vec2(70.0, 0.0),
            Input::Finger,
        );
        let width = frame(&h, 1).border.width;
        assert!(width > 3.0, "{size:?}: {width}");
        assert_eq!((width * 4.0).round() / 4.0, width);
        check_controls(&h, "border width");
    }
}

#[test]
fn the_border_survives_a_close_and_an_open() {
    use vale_app::ui::atlas::{set_border, set_border_color, set_border_width};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut s = state();
    s.doc = doc;
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    add_frame(&mut h);
    let (main, inset) = (frame(&h, 0).id, frame(&h, 1).id);
    let st = h.state_mut();
    set_border_color(st, main, [10, 20, 30]);
    set_border_width(st, main, 2.3);
    set_border(st, inset, false);
    h.run_steps(2);
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.project.atlas, project.atlas);
    let b = &doc.project.atlas.frame(main).unwrap().border;
    assert!(b.on);
    assert_eq!(b.color, [10, 20, 30]);
    assert_eq!(b.width, 2.25);
    assert!(!doc.project.atlas.frame(inset).unwrap().border.on);
}

#[test]
fn the_screen_draws_the_line_widths_of_the_page_at_each_zoom() {
    let mut h = inset_pad(BOARDS[0]);
    let id = frame(&h, 0).id;
    let frame = h.state_mut().doc.project.atlas.frame_mut(id).unwrap();
    frame.border.width = 2.5;
    frame.graticule.on = true;
    frame.graticule.width = 0.7;
    let (border, graticule) = (frame.border.clone(), frame.graticule.clone());
    let mut ground = None;
    for zoom in [1.0, 3.0, 0.5] {
        h.state_mut().atlas.view.zoom = zoom;
        h.state_mut().atlas.selection = AtlasSelection::Frame(id);
        h.run_steps(3);
        // The list is in points of paper, so the widths are the ones of the
        // export at every zoom.
        let composed = h.state().composed.as_ref().unwrap();
        // One point is the same ground distance at every zoom.
        let km = *ground.get_or_insert(composed.km_per_point);
        assert!(
            (composed.km_per_point / km - 1.0).abs() < 1e-9,
            "zoom {zoom}"
        );
        let list = &composed.list;
        let wide = |c: [u8; 3], w: f64| {
            let c = vale_render::Color::rgb(c[0], c[1], c[2]);
            (list.items.iter()).any(
                |i| matches!(i, Item::Stroke { color, width, .. } if *color == c && *width == w),
            )
        };
        assert!(wide(border.color, border.width), "border at {zoom}");
        assert!(
            wide(graticule.color, graticule.width),
            "graticule at {zoom}"
        );
    }
}

#[test]
fn the_texture_of_a_frame_is_on_whole_pixels() {
    let mut h = inset_pad(BOARDS[0]);
    h.state_mut().atlas.view.zoom = 1.37;
    h.state_mut().atlas.view.offset = vec2(13.3, -7.9);
    h.run_steps(3);
    let rects = h.state().atlas.texture_rects();
    assert!(!rects.is_empty());
    for r in rects {
        for v in [r.min.x, r.min.y, r.max.x, r.max.y] {
            assert_eq!(v, v.round(), "{r:?}");
        }
    }
}

#[test]
fn the_texture_of_a_frame_holds_the_whole_border() {
    let mut h = inset_pad(BOARDS[0]);
    let id = frame(&h, 1).id;
    let width = 8.0;
    let set = |h: &mut Pad, on: bool| {
        let frame = h.state_mut().doc.project.atlas.frame_mut(id).unwrap();
        frame.border.width = width;
        frame.border.on = on;
    };
    set(&mut h, true);
    h.run_steps(3);
    let place = h.state().atlas.place.unwrap();
    let bbox = place.rect_to_screen(shape_bbox(&frame(&h, 1).shape));
    let reach = |h: &Pad| {
        let r = h.state().atlas.texture_rect(id).unwrap();
        bbox.min.x - r.min.x
    };
    // Half the width of the border is outside the shape.
    assert!(reach(&h) >= width as f32 / 2.0, "{}", reach(&h));
    set(&mut h, false);
    h.run_steps(3);
    assert!(reach(&h) < 1.0);
}

/// A quick tap of `count` fingers side by side about `center`.
fn fingers_tap(h: &mut Pad, count: u64, center: Pos2) {
    let at = |id: u64| center + vec2(40.0 * id as f32, 0.0);
    for id in 1..=count {
        touch(h, id, egui::TouchPhase::Start, at(id), None);
    }
    h.step();
    for id in (1..=count).rev() {
        touch(h, id, egui::TouchPhase::End, at(id), None);
    }
    h.run_steps(3);
}

#[test]
fn a_tap_of_two_fingers_undoes_and_a_tap_of_three_redoes_in_the_atlas() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    // A tap lasts less than 0.3 seconds, and a step of the harness is short.
    let mut h = Harness::builder()
        .with_size(vec2(BOARDS[0].0, BOARDS[0].1))
        .with_pixels_per_point(1.0)
        .with_step_dt(0.01)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), s);
    h.run_steps(3);
    add_frame(&mut h);
    h.state_mut().save_waiting().unwrap();
    add_frame(&mut h);
    h.state_mut().save_waiting().unwrap();
    assert_eq!(open_page(&h).frames.len(), 2);

    let empty = pos2(200.0, 780.0);
    fingers_tap(&mut h, 2, empty);
    assert_eq!(open_page(&h).frames.len(), 1);
    fingers_tap(&mut h, 3, empty);
    assert_eq!(open_page(&h).frames.len(), 2);

    // Command+Z and Shift+Command+Z do the same.
    let key = |h: &mut Pad, modifiers| {
        h.event(egui::Event::Key {
            key: egui::Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
        h.run_steps(3);
    };
    key(&mut h, Modifiers::COMMAND);
    assert_eq!(open_page(&h).frames.len(), 1);
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(open_page(&h).frames.len(), 2);
}

/// The red values of a row of pixels near the bottom of a card, inside its
/// round corners.
fn card_row(image: &image::RgbaImage, card: Rect) -> Vec<i32> {
    let y = card.bottom() as u32 - 3;
    let (left, right) = (card.left() as u32 + 16, card.right() as u32 - 16);
    (left..right)
        .map(|x| i32::from(image.get_pixel(x, y).0[0]))
        .collect()
}

#[test]
fn the_cards_of_the_atlas_show_the_blurred_page() {
    let _gpu = one_gpu_test();
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = vale_app::headless::ui_harness(state(), (1194.0, 834.0), 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(3);
    // The left edge of the page goes through the middle of the card of the
    // workspace switch, and the page reaches above the card.
    let card = h.state().pad.rects.unwrap().workspace;
    let rect = h.state().atlas.place.unwrap().rect;
    let view = &mut h.state_mut().atlas.view;
    view.offset.x += card.center().x - rect.left();
    view.offset.y += card.top() - 20.0 - rect.top();
    h.run_steps(3);
    let blurred = h.render().unwrap();
    std::fs::create_dir_all("../../target/app").unwrap();
    blurred
        .save("../../target/app/test-atlas-blur.png")
        .unwrap();
    h.state_mut().globe.blur = false;
    h.run_steps(2);
    let plain = h.render().unwrap();

    let row = card_row(&blurred, card);
    let width = row.len() as i32;
    let step = row.windows(2).map(|w| (w[0] - w[1]).abs()).max().unwrap();
    assert!(step <= 5, "a step of {step}");
    let sharp = card_row(&plain, card);
    let difference: i32 = row.iter().zip(&sharp).map(|(a, b)| (a - b).abs()).sum();
    assert!(difference > width * 2, "{difference}");
}
