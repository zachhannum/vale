//! The Atlas workspace in the iPad layout: pages, map frames, and their
//! panels.

mod screenshot;
mod striped;

use std::time::Instant;

use eframe::egui::{self, Modifiers, Pos2, Rect, Vec2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kurbo::Point;
use vale_app::atlas::{Handle, anchor, frame_transform, shape_bbox};
use vale_app::document::{Document, Target};
use vale_app::furniture::rect_of;
use vale_app::projects::Locations;
use vale_app::session::{Session, new_project_path};
use vale_app::ui::atlas::measure::measure_lines;
use vale_app::ui::atlas::{self as atlas, BarPage, BarPick, ItemKind, ItemPage, TextPart};
use vale_app::ui::atlas::{AtlasSelection, AtlasTool, FrameTab};
use vale_app::ui::map_texture::MapRenderer;
use vale_app::ui::pad::atlas::{
    ADD, ADD_HOLE, ADD_PAGE, AUTO_INTERVAL, BORDER, BORDER_DISTANCE, BORDER_STROKE, BORDER_STYLES,
    BORDER_WIDTH, CENTER_ON_FRAME, COPY_GLOBE_STYLE, DELETE_PAGE, DELETE_PROJECTION,
    EXPORT_ATLAS_PDF, EXPORT_PAGE_PDF, EXPORT_PAGE_PNG, FIRST_PARALLEL, FRAME_TABS, FRAME_TOOL,
    GRATICULE_COLOR, GRATICULE_WIDTH, HEIGHT, INTERVAL, LABEL_FORMATS, MAP_FRAME, MAP_NAME,
    NEATLINE, NEATLINE_COLOR, NEATLINE_WIDTH, NEXT_PAGE, OBLIQUE, PAPER_COLOR, POLYGON,
    PREVIOUS_PAGE, PROJECTION, REFERENCE_GRID, SAVE_PROJECTION, SCALE, SECOND_COLOR, SIZE,
    STROKE_COLOR, STROKE_WIDTH as BORDER_STROKE_WIDTH, VERTEX, WIDTH, custom_row, page_title,
    readout_text,
};
use vale_app::ui::pad::atlas::{BORDER_GAP, DISTANCE_BY_SIDE, GRID_COLUMN_NAMES, SIDE_DISTANCES};
use vale_app::ui::pad::atlas::{
    EDITED, MAP_STYLE, MAP_STYLE_NAME, NO_MAP_STYLE_ROW, RESET_TO_MAP_STYLE, SAVE_AS_MAP_STYLE,
    UPDATE_MAP_STYLE, map_style_row,
};
use vale_app::ui::pad::atlas::{GRID_COLOR, GRID_SPACING, LAYOUT_GRID, SHOW_GRID, SNAP_TO_LAYOUT};
use vale_app::ui::pad::edge_labels::{
    EDGE_LABELS, HALO as EDGE_HALO, HALO_WIDTH as EDGE_HALO_WIDTH, OFFSET as EDGE_OFFSET,
    ORIENTATIONS, PLACES, SETS, SHOW_LABELS, SIDES, SIZE as EDGE_SIZE, STYLE as EDGE_STYLE,
};
use vale_app::ui::pad::furniture::{
    self as item, ALIGNMENTS, COLOR as ITEM_COLOR, ENTRIES, ENTRY_NAME, ITEM_TEXT,
    MAP_FRAME as ITEM_MAP_FRAME, RESET_ENTRIES, STYLE as ITEM_STYLE, entry_names,
};
use vale_app::ui::pad::labels::{
    ADD_CLASS, BUFFER, CASE, COLOR, CURVE_LIMIT, DELETE_CLASS, DUPLICATE_DISTANCE,
    FOLLOW_PARALLELS, HALO, HALO_COLOR, HALO_WIDTH, LABEL_SIZE, LAYER, MOVE_DOWN, MOVE_UP, OFFSET,
    PLACEMENT, POSITIONS, REMOVE_DUPLICATES, REORDER, REPEAT, REPEAT_INTERVAL, SEGMENTS, SMOOTHING,
    SPREAD, STYLE, TEXT, UPSIDE_DOWN,
};
use vale_app::ui::pad::labels::{
    ANCHOR_GAP, AVOID_POLYGONS, DASH, DASH_GAP, DASHED, END_SIZE, LABEL_GAP, LARGEST_PART,
    LEADER_LINE, LINE_WIDTH, MATCH_HALO, MATCH_TEXT, MIN_AREA, OVER_HOLES, OVERRUN,
    REPEAT_DISTANCE, REPEAT_LABEL, SHOW_LINE, ZONES, zone_name,
};
use vale_app::ui::pad::rules::STROKE_WIDTH;
use vale_app::ui::pad::widgets::eye_name;
use vale_app::ui::pad::{
    CLEAR_PATH, DELETE_VERTEX, DESELECT, GRATICULE, MEASURE, Panel, SNAP, SNAP_DISTANCE,
    SNAP_EDGES, SNAP_VERTICES, SNAPPING, readout_line,
};
use vale_app::ui::select::{DELETE as DELETE_SELECTION, TOOL as SELECT_TOOL};
use vale_app::ui::{Action, AppState, ExportFormat, Layout, Screen, Workspace, draw};
use vale_labeler::{Placement, PolygonMode};
use vale_render::Item;
use vale_sphere::measure::{path_length, ring_area};
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::{
    CoordFormat, EdgeOrientation, Feature, FrameRecord, FrameShape, Furniture, FurnitureRecord,
    Geometry, GeometryKind, LayerId, MapId, PageUnits, Project, ProjectFile, TextAlign, World,
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

/// The name of the value field of a slider or a stepper.
fn field(name: &str) -> String {
    format!("{name} value")
}

/// Types `text` in the value field of the slider or the stepper `name`
/// with its keypad, and closes the keypad.
#[allow(dead_code)]
fn type_value(h: &mut Pad, name: &str, text: &str) {
    let field = format!("{name} value");
    tap(h, &field);
    for c in text.chars() {
        tap(h, &format!("{field} {c}"));
    }
    tap(h, &format!("{field} Done"));
}

/// A tap on the plus button (`up`) or the minus button of the stepper
/// `name`. A `tap` lasts long enough for the button to repeat, and a click
/// of the mouse does not.
#[allow(dead_code)]
fn step(h: &mut Pad, name: &str, up: bool) {
    let button = if up { "Increase" } else { "Decrease" };
    let name = format!("{button} {name}");
    let rect = control(h, &name).unwrap_or_else(|| panic!("no control `{name}`"));
    tap_with(h, rect.center(), Input::Mouse);
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

/// Scrolls the panel up in small steps until a control is inside the
/// card.
fn nudge_up_to(h: &mut Pad, name: &str) {
    for _ in 0..12 {
        let card = h.state().atlas.panel_rect.unwrap();
        let inside = |rect: Rect| card.shrink2(vec2(0.0, 40.0)).contains_rect(rect);
        if control(h, name).is_some_and(inside) {
            return;
        }
        let from = pos2(card.left() + 6.0, card.bottom() - 120.0);
        drag(h, from, from + vec2(0.0, 60.0), Input::Finger);
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
    scroll_to(&mut h, ADD_PAGE);
    assert!(control(&h, ADD_PAGE).is_some());
    assert!(control(&h, SCALE).is_none());
}

#[test]
fn the_add_button_adds_a_frame_that_shows_the_map_of_the_project() {
    let mut h = pad();
    tap(&mut h, ADD);
    assert!(h.state().atlas.add_rect.is_some());
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
    h.state_mut().atlas.snap = false;
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

fn rings(h: &Pad) -> Vec<Vec<[f64; 2]>> {
    match frame(h, 1).shape {
        FrameShape::Polygon(rings) => rings,
        FrameShape::Rectangle(_) => panic!("not a polygon"),
    }
}

#[test]
fn a_polygon_frame_moves_and_scales_with_a_finger_a_pen_and_a_mouse() {
    let mut h = inset_pad(BOARDS[0]);
    h.state_mut().atlas.snap = false;
    tap(&mut h, POLYGON);
    tap(&mut h, ADD_HOLE);
    assert_eq!((ring_len(&h, 0), ring_len(&h, 1)), (4, 4));
    let ppm = h.state().atlas.place.unwrap().pts_per_mm;
    for input in INPUTS {
        // A drag on the body, between the outer ring and the hole, moves
        // the shape.
        let before = rings(&h);
        let b = shape_bbox(&frame(&h, 1).shape);
        let body = Point::new(b.x0 + b.width() * 0.3, b.y0 + b.height() * 0.125);
        let from = screen(&h, body);
        drag(&mut h, from, from - vec2(40.0, 30.0), input);
        let moved = rings(&h);
        for (a, b) in before.iter().flatten().zip(moved.iter().flatten()) {
            assert!((b[0] - a[0] + 40.0 / ppm).abs() < 0.1, "{input:?}");
            assert!((b[1] - a[1] + 30.0 / ppm).abs() < 0.1, "{input:?}");
        }

        // The bottom-right handle scales the shape and its hole.
        let b = shape_bbox(&frame(&h, 1).shape);
        let corner = handle(&h, Handle::Resize(4));
        drag(&mut h, corner, corner - vec2(20.0, 10.0), input);
        let scaled = rings(&h);
        let a = shape_bbox(&frame(&h, 1).shape);
        assert_eq!((a.x0, a.y0), (b.x0, b.y0), "{input:?}");
        assert!(
            (b.width() - a.width() - 20.0 / ppm).abs() < 0.1,
            "{input:?}"
        );
        assert!(
            (b.height() - a.height() - 10.0 / ppm).abs() < 0.1,
            "{input:?}"
        );
        let (sx, sy) = (a.width() / b.width(), a.height() / b.height());
        for (p, q) in moved.iter().flatten().zip(scaled.iter().flatten()) {
            assert!(
                (q[0] - (b.x0 + (p[0] - b.x0) * sx)).abs() < 1e-6,
                "{input:?}"
            );
            assert!(
                (q[1] - (b.y0 + (p[1] - b.y0) * sy)).abs() < 1e-6,
                "{input:?}"
            );
        }
        assert_eq!((ring_len(&h, 0), ring_len(&h, 1)), (4, 4), "{input:?}");
    }
    // The main frame stays.
    let main = shape_bbox(&frame(&h, 0).shape);
    assert_eq!((main.x0, main.y0), (10.0, 10.0));
}

#[test]
fn the_frame_tool_shows_no_vertex_handles() {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, POLYGON);
    let handles = &h.state().atlas.handles;
    assert_eq!(handles.len(), 8);
    assert!(handles.iter().all(|(h, _)| matches!(h, Handle::Resize(_))));

    // A double tap on a corner does not remove a vertex.
    for input in INPUTS {
        let at = handle(&h, Handle::Resize(0));
        double_tap(&mut h, at, input);
        assert_eq!(ring_len(&h, 0), 4, "{input:?}");
        assert!(h.state().atlas.vertices.is_empty(), "{input:?}");
    }

    // The Delete key deletes the frame, not a vertex.
    let id = frame(&h, 1).id;
    h.state_mut().atlas.vertices = vec![(0, 0)];
    h.key_press(egui::Key::Delete);
    h.run_steps(2);
    assert!(h.state().doc.project.atlas.frame(id).is_none());
}

#[test]
fn the_neatline_the_border_and_the_crop_stay_in_place_while_the_map_moves() {
    let mut h = inset_pad(BOARDS[0]);
    let id = frame(&h, 0).id;
    let f = h.state_mut().doc.project.atlas.frame_mut(id).unwrap();
    f.neatline.on = true;
    f.border.on = true;
    h.run_steps(4);
    tap(&mut h, "Move");
    let at = middle(&h, 0) - vec2(60.0, 0.0);
    tap_with(&mut h, at, Input::Finger);
    h.run_steps(4);
    let [_, spill, decor] = h.state().atlas.painted_rects(id);
    // At rest, the spill does not show.
    assert_eq!(spill, None);
    let decor = decor.unwrap();
    let place = h.state().atlas.place.unwrap();
    let shape = place.rect_to_screen(shape_bbox(&frame(&h, 0).shape));

    // During the drag, the map moves with the finger inside the shape, the
    // spill fills the rest of the shape, and the decorations stay.
    let from = middle(&h, 0) - vec2(60.0, 20.0);
    let to = from + vec2(50.0, 30.0);
    drag_steps(&mut h, from, to, Input::Finger, 6);
    let [map, spill, held] = h.state().atlas.painted_rects(id);
    let (map, spill) = (map.unwrap(), spill.unwrap());
    let near = |a: Pos2, b: Pos2| a.distance(b) < 1.0;
    assert!(shape.expand(0.5).contains_rect(map), "{map:?} {shape:?}");
    assert!(near(map.max, shape.max), "{map:?} {shape:?}");
    let texture = h.state().atlas.texture_rect(id).unwrap();
    let start = (texture.min + vec2(50.0, 30.0)).max(shape.min);
    assert!(near(map.min, start), "{map:?} {start:?}");
    assert!(near(spill.min, shape.min) && near(spill.max, shape.max));
    assert_eq!(held, Some(decor));
    release(&mut h, to, Input::Finger);
    h.run_steps(8);
    assert_eq!(h.state().atlas.painted_rects(id)[2], Some(decor));
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

    // Two fingers zoom the map about their middle, and a twist does not
    // turn it.
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
    assert_eq!(frame(&h, 0).rotation, 0.0);
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
        assert!(control(&h, FRAME_TABS[3]).is_some());
        tap(&mut h, POLYGON);
        check_controls(&h, "polygon");
        scroll_to(&mut h, SCALE);
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
        // A place on the page, outside the frames. A sheet that covers it is
        // pulled down to its title row, and a pull up opens it again after the tap.
        let at = screen(&h, Point::new(4.0, 50.0));
        let covered = h
            .state()
            .pad
            .rects
            .unwrap()
            .sheet
            .filter(|s| s.contains(at));
        if let Some(sheet) = covered {
            let bar = pos2(sheet.center().x, sheet.top() + 26.0);
            drag(&mut h, bar, bar + vec2(0.0, sheet.height()), Input::Finger);
            assert!(!h.state().pad.rects.unwrap().sheet.unwrap().contains(at));
        }
        tap_with(&mut h, at, Input::Finger);
        assert_eq!(h.state().atlas.selection, AtlasSelection::Page, "{size:?}");
        if covered.is_some() {
            let sheet = h.state().pad.rects.unwrap().sheet.unwrap();
            let bar = pos2(sheet.center().x, sheet.top() + 26.0);
            drag(&mut h, bar, bar - vec2(0.0, 300.0), Input::Finger);
            let travel = h.state().pad.rects.unwrap().sheet_travel;
            assert!(h.state().pad.sheet_offset < travel, "{size:?}");
        }
        check_controls(&h, "page panel");
        tap(&mut h, SIZE);
        check_controls(&h, "page sizes");
        let inset = frame(&h, 1).id;
        h.state_mut().atlas.selection = AtlasSelection::Frame(inset);
        h.state_mut().atlas.tab = FrameTab::Frame;
        h.run_steps(3);
        assert!(control(&h, DELETE_SELECTION).is_some(), "{size:?}");
        check_controls(&h, "context strip of the frame");
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
    for tool in ["Move", "Frame tool"] {
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
fn with_the_frame_tool_a_pinch_and_the_wheel_in_the_selected_frame_zoom_the_page() {
    let mut h = inset_pad(BOARDS[0]);
    assert_eq!(h.state().atlas.tool, AtlasTool::Frame);
    let (before, maps_before) = (place(&h), maps(&h));
    let scale = frame(&h, 1).scale;
    let at = middle(&h, 1);
    pinch(&mut h, at, 40.0, 60.0, 0.0, Vec2::ZERO);
    let zoom = place(&h).pts_per_mm / before.pts_per_mm;
    assert!((zoom - 1.5).abs() < 0.01, "{zoom}");
    let zoomed = place(&h);
    let at = middle(&h, 1);
    h.event(egui::Event::PointerMoved(at));
    h.step();
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 200.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(10);
    assert!(place(&h).pts_per_mm > zoomed.pts_per_mm * 1.05);
    assert_eq!(frame(&h, 1).scale, scale);
    assert_eq!(maps(&h), maps_before);
}

#[test]
fn with_the_move_tool_a_pinch_in_the_selected_frame_changes_only_that_frame() {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, "Move");
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
    scroll_to(&mut h, ADD_PAGE);
    assert!(control(&h, DELETE_PAGE).is_none());
    tap(&mut h, ADD_PAGE);
    let atlas = &h.state().doc.project.atlas;
    assert_eq!(atlas.pages.len(), 2);
    assert_eq!(h.state().atlas.page, atlas.pages[1].id);
    assert!(atlas.pages[1].frames.is_empty());
    assert_eq!(h.state().atlas.selection, AtlasSelection::Page);
    assert_eq!(readout_text(h.state()), "297 × 210 mm · 2 / 2");
    scroll_to(&mut h, DELETE_PAGE);
    tap(&mut h, DELETE_PAGE);
    let atlas = &h.state().doc.project.atlas;
    assert_eq!(atlas.pages.len(), 1);
    assert_eq!(h.state().atlas.page, atlas.pages[0].id);
    assert_eq!(atlas.pages[0].frames.len(), 2);
}

#[test]
fn the_context_strip_deletes_the_frame_and_undo_brings_it_back() {
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

    assert!(control(&h, SNAP_TO_LAYOUT).is_some());
    assert!(control(&h, DESELECT).is_some());
    screenshot::save(&mut h, "pad-atlas-select-strip");
    tap(&mut h, DELETE_SELECTION);
    let page = open_page(&h);
    assert_eq!(page.frames.len(), 1);
    assert!(page.frames.iter().all(|f| f.id != inset.id));
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
    assert!(h.state().atlas.vertices.is_empty());
    assert!(!h.state().atlas.has_image(inset.id));
    assert_eq!(h.state().atlas.panel_rect, None);
    assert!(control(&h, DELETE_SELECTION).is_none());
    assert!(control(&h, DESELECT).is_none());

    h.state_mut().save_waiting().unwrap();
    tap(&mut h, "Undo");
    let page = open_page(&h);
    assert_eq!(page.frames.len(), 2);
    assert_eq!(page.frames[1], inset);
}

#[test]
fn deselect_clears_the_atlas_selection() {
    let mut h = inset_pad(BOARDS[0]);
    let inset = frame(&h, 1).id;
    assert_eq!(h.state().atlas.tool, AtlasTool::Frame);
    // Deselect in the context strip clears the selected frame.
    tap(&mut h, DESELECT);
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
    assert!(control(&h, DESELECT).is_none());
    assert!(control(&h, DELETE_SELECTION).is_none());
    assert_eq!(open_page(&h).frames.len(), 2);
    // It clears a selected item of furniture.
    add_item(&mut h, ItemKind::NorthArrow);
    assert!(matches!(h.state().atlas.selection, AtlasSelection::Item(_)));
    assert!(control(&h, SNAP_TO_LAYOUT).is_none());
    tap(&mut h, DESELECT);
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
    assert_eq!(open_page(&h).furniture.len(), 1);
    // Esc clears the selection on the desktop.
    h.state_mut().atlas.selection = AtlasSelection::Frame(inset);
    h.run_steps(2);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
}

#[test]
fn the_move_tool_strip_has_deselect() {
    let mut h = inset_pad(BOARDS[0]);
    let inset = frame(&h, 1).id;
    tap(&mut h, "Move");
    assert_eq!(h.state().atlas.tool, AtlasTool::Move);
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(inset));
    assert!(control(&h, DESELECT).is_some());
    for name in [SNAP_TO_LAYOUT, DELETE_SELECTION, DELETE_VERTEX] {
        assert!(control(&h, name).is_none(), "{name}");
    }
    tap(&mut h, DESELECT);
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
    assert!(control(&h, DESELECT).is_none());
    assert_eq!(open_page(&h).frames.len(), 2);
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
        for name in [ADD, "Frame tool", "Move"] {
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
    scroll_to(&mut h, ADD_PAGE);
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
#[ignore = "gpu"]
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
    tap(&mut h, DELETE_SELECTION);
    assert_eq!(map_names(&h), ["Map"]);
    let first = h.state().doc.project.atlas.pages[0].frames[0].id;
    h.state_mut().atlas.selection = AtlasSelection::Frame(first);
    h.run_steps(2);
    tap(&mut h, DELETE_SELECTION);
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
fn strokes(h: &mut Pad, index: usize, rgba: [u8; 4], line: f64) -> usize {
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
        .compose_frame(&s.doc, &page, &f, target, &mut fonts, Quality::Final)
        .unwrap();
    let [r, g, b, a] = rgba;
    let color = vale_render::Color::rgba(r, g, b, a);
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
    scroll_to(&mut h, &field(INTERVAL));
    step(&mut h, INTERVAL, false);
    let set = frame(&h, 1).graticule.interval;
    assert!(set.is_some());
    assert!(control(&h, AUTO_INTERVAL).is_some());

    tap(&mut h, "Move");
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
fn the_interval_stepper_goes_to_the_previous_interval_and_back() {
    use vale_sphere::INTERVALS;
    let mut h = inset_pad(BOARDS[0]);
    let id = frame(&h, 1).id;
    let (drawn, _) = vale_app::ui::atlas::graticule_intervals(h.state_mut(), id).unwrap();
    let i = INTERVALS
        .iter()
        .position(|v| *v >= drawn * (1.0 - 1e-9))
        .unwrap();
    assert!(i > 0, "{drawn}");
    scroll_to(&mut h, &field(INTERVAL));
    step(&mut h, INTERVAL, false);
    assert_eq!(frame(&h, 1).graticule.interval, Some(INTERVALS[i - 1]));
    step(&mut h, INTERVAL, true);
    assert_eq!(frame(&h, 1).graticule.interval, Some(INTERVALS[i]));
    // A typed value in degrees takes the nearest interval.
    type_value(&mut h, INTERVAL, "14");
    assert_eq!(frame(&h, 1).graticule.interval, Some(15.0));
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
        assert!(control(&h, &field(INTERVAL)).is_none(), "{size:?}");
        tap(&mut h, GRATICULE);
        assert!(frame(&h, 1).graticule.on, "{size:?}");
        check_controls(&h, "graticule");

        scroll_to(&mut h, AUTO_INTERVAL);
        tap(&mut h, AUTO_INTERVAL);
        assert!(frame(&h, 1).graticule.interval.is_some(), "{size:?}");

        scroll_to(&mut h, GRATICULE_COLOR);
        tap(&mut h, GRATICULE_COLOR);
        scroll_to(&mut h, &hue);
        nudge_up_to(&mut h, &shade);
        check_controls(&h, "graticule color");
        let before = frame(&h, 1).graticule.color;
        let square = control(&h, &shade).unwrap();
        tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
        assert_ne!(frame(&h, 1).graticule.color, before, "{size:?}");

        scroll_up_to(&mut h, GRATICULE_COLOR);
        tap(&mut h, GRATICULE_COLOR);
        scroll_to(&mut h, &field(GRATICULE_WIDTH));
        type_value(&mut h, GRATICULE_WIDTH, "1.73");
        let width = frame(&h, 1).graticule.width;
        assert!(width > 1.5, "{size:?}: {width}");
        assert_eq!((width * 10.0).round() / 10.0, width);
        check_controls(&h, "graticule width");

        scroll_to(&mut h, EDGE_LABELS);
        tap(&mut h, EDGE_LABELS);
        assert!(control(&h, SETS[0].0).is_some(), "{size:?}");
        check_controls(&h, "edge labels");
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-edge-labels");
        }
        tap(&mut h, LABEL_FORMATS[1]);
        assert_eq!(
            frame(&h, 1).graticule.format,
            CoordFormat::Decimal,
            "{size:?}"
        );

        tap(&mut h, SETS[1].0);
        scroll_to(&mut h, PLACES[1]);
        tap(&mut h, PLACES[1]);
        scroll_to(&mut h, ORIENTATIONS[1]);
        tap(&mut h, ORIENTATIONS[1]);
        scroll_to(&mut h, SIDES[0].0);
        tap(&mut h, SIDES[0].0);
        let g = frame(&h, 1).graticule;
        assert!(g.parallels.outside && !g.meridians.outside, "{size:?}");
        assert_eq!(
            g.parallels.orientation,
            EdgeOrientation::Horizontal,
            "{size:?}"
        );
        assert!(!g.parallels.edges.top && g.meridians.edges.top, "{size:?}");

        scroll_up_to(&mut h, EDGE_HALO);
        tap(&mut h, EDGE_HALO);
        assert!(frame(&h, 1).graticule.parallels.halo, "{size:?}");
        scroll_to(&mut h, &value_of(EDGE_HALO_WIDTH));
        check_controls(&h, "edge label halo");

        scroll_up_to(&mut h, EDGE_STYLE);
        tap(&mut h, EDGE_STYLE);
        assert!(control(&h, &value_of(EDGE_SIZE)).is_none(), "{size:?}");
        tap(&mut h, &format!("Back to {EDGE_LABELS}"));
        scroll_up_to(&mut h, &value_of(EDGE_SIZE));
        scroll_to(&mut h, &value_of(EDGE_SIZE));
        assert!(control(&h, &value_of(EDGE_SIZE)).is_some(), "{size:?}");

        scroll_up_to(&mut h, SHOW_LABELS);
        tap(&mut h, SHOW_LABELS);
        assert!(!frame(&h, 1).graticule.parallels.on, "{size:?}");
        assert!(control(&h, &value_of(EDGE_OFFSET)).is_none(), "{size:?}");
        tap(&mut h, &format!("Back to {}", FRAME_TABS[0]));
        assert!(control(&h, SETS[0].0).is_none(), "{size:?}");
        scroll_to(&mut h, EDGE_LABELS);
    }
}

#[test]
fn the_graticule_settings_survive_a_close_and_an_open() {
    use vale_app::ui::atlas::{
        EdgeSet, edit_edge_labels, set_graticule, set_graticule_color, set_graticule_format,
        set_graticule_interval, set_graticule_width,
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
    set_graticule_color(st, main, [10, 20, 30, 128]);
    set_graticule_width(st, main, 1.3);
    set_graticule_format(st, main, CoordFormat::Decimal);
    edit_edge_labels(st, main, EdgeSet::Parallels, |e| {
        e.outside = true;
        e.size = 9.5;
        e.halo = true;
        e.edges.left = false;
    });
    edit_edge_labels(st, inset, EdgeSet::Meridians, |e| e.on = false);
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
    assert_eq!(g.color, [10, 20, 30, 128]);
    assert_eq!(g.width, 1.3);
    assert_eq!(g.format, CoordFormat::Decimal);
    assert!(g.parallels.outside && g.parallels.halo && !g.parallels.edges.left);
    assert_eq!(g.parallels.size, 9.5);
    let inset = &doc.project.atlas.frame(inset).unwrap().graticule;
    assert!(!inset.on);
    assert!(!inset.meridians.on && inset.parallels.on);
}

#[test]
fn two_frames_of_one_map_have_their_own_neatline() {
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
    let first = frame(&h, 0).neatline;
    let shade = format!("{NEATLINE_COLOR} shade");
    scroll_to(&mut h, NEATLINE_COLOR);
    tap(&mut h, NEATLINE_COLOR);
    scroll_to(&mut h, &shade);
    let square = control(&h, &shade).unwrap();
    tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
    let color = frame(&h, 1).neatline.color;
    assert_ne!(color, first.color);
    assert_eq!(frame(&h, 0).neatline, first);
    assert!(strokes(&mut h, 0, first.color, first.width) > 0);
    assert_eq!(strokes(&mut h, 0, color, first.width), 0);
    assert!(strokes(&mut h, 1, color, first.width) > 0);

    scroll_up_to(&mut h, NEATLINE);
    tap(&mut h, NEATLINE);
    assert!(!frame(&h, 1).neatline.on);
    assert_eq!(frame(&h, 0).neatline, first);
    assert_eq!(strokes(&mut h, 1, color, first.width), 0);
    assert!(strokes(&mut h, 0, first.color, first.width) > 0);
}

#[test]
fn the_neatline_controls_work_with_a_finger() {
    let hue = format!("{NEATLINE_COLOR} hue");
    let shade = format!("{NEATLINE_COLOR} shade");
    for size in BOARDS {
        let mut h = inset_pad(size);
        scroll_to(&mut h, NEATLINE);
        assert!(frame(&h, 1).neatline.on, "{size:?}");
        check_controls(&h, "neatline");
        tap(&mut h, NEATLINE);
        assert!(!frame(&h, 1).neatline.on, "{size:?}");
        assert!(control(&h, &field(NEATLINE_WIDTH)).is_none(), "{size:?}");
        tap(&mut h, NEATLINE);
        assert!(frame(&h, 1).neatline.on, "{size:?}");

        scroll_to(&mut h, NEATLINE_COLOR);
        tap(&mut h, NEATLINE_COLOR);
        scroll_to(&mut h, &hue);
        nudge_up_to(&mut h, &shade);
        check_controls(&h, "neatline color");
        let before = frame(&h, 1).neatline.color;
        scroll_up_to(&mut h, &shade);
        let square = control(&h, &shade).unwrap();
        tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
        assert_ne!(frame(&h, 1).neatline.color, before, "{size:?}");

        scroll_up_to(&mut h, NEATLINE_COLOR);
        tap(&mut h, NEATLINE_COLOR);
        scroll_to(&mut h, &field(NEATLINE_WIDTH));
        type_value(&mut h, NEATLINE_WIDTH, "3.6");
        let width = frame(&h, 1).neatline.width;
        assert_eq!(width, 3.6, "{size:?}");
        check_controls(&h, "neatline width");
    }
}

#[test]
fn the_neatline_survives_a_close_and_an_open() {
    use vale_app::ui::atlas::{set_neatline, set_neatline_color, set_neatline_width};
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
    set_neatline_color(st, main, [10, 20, 30, 128]);
    set_neatline_width(st, main, 2.32);
    set_neatline(st, inset, false);
    h.run_steps(2);
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.project.atlas, project.atlas);
    let b = &doc.project.atlas.frame(main).unwrap().neatline;
    assert!(b.on);
    assert_eq!(b.color, [10, 20, 30, 128]);
    assert_eq!(b.width, 2.3);
    assert!(!doc.project.atlas.frame(inset).unwrap().neatline.on);
}

#[test]
fn the_panel_of_the_page_sets_the_paper_color_with_a_finger() {
    let shade = format!("{PAPER_COLOR} shade");
    for size in BOARDS {
        let mut h = pad_with(state(), size);
        let page = h.state().atlas.place.unwrap().rect;
        tap_with(&mut h, page.center(), Input::Finger);
        assert_eq!(h.state().atlas.selection, AtlasSelection::Page);
        scroll_to(&mut h, PAPER_COLOR);
        tap(&mut h, PAPER_COLOR);
        scroll_to(&mut h, &shade);
        check_controls(&h, "paper color");
        let square = control(&h, &shade).unwrap();
        tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
        assert_ne!(open_page(&h).paper, [255, 255, 255], "{size:?}");
    }
}

#[test]
fn a_typed_border_distance_is_set_and_clamped() {
    let mut h = inset_pad(BOARDS[0]);
    scroll_to(&mut h, BORDER);
    tap(&mut h, BORDER);
    scroll_to(&mut h, &field(BORDER_DISTANCE));
    type_value(&mut h, BORDER_DISTANCE, "12.4");
    assert_eq!(frame(&h, 1).border.distances_mm, [12.5; 4]);
    type_value(&mut h, BORDER_DISTANCE, "200");
    assert_eq!(frame(&h, 1).border.distances_mm, [30.0; 4]);
    type_value(&mut h, BORDER_DISTANCE, "-500");
    assert_eq!(frame(&h, 1).border.distances_mm, [-100.0; 4]);
}

#[test]
fn the_border_and_grid_controls_work_with_a_finger() {
    use vale_app::ui::pad::grid_labels::{GRID_LABELS, SIZE as GRID_SIZE};
    use vale_store::BorderStyle;
    for size in BOARDS {
        let mut h = inset_pad(size);
        scroll_to(&mut h, BORDER);
        assert!(!frame(&h, 1).border.on, "{size:?}");
        assert!(control(&h, &field(BORDER_WIDTH)).is_none(), "{size:?}");
        tap(&mut h, BORDER);
        assert!(frame(&h, 1).border.on, "{size:?}");

        scroll_to(&mut h, BORDER_STYLES[1]);
        assert!(control(&h, SECOND_COLOR).is_none(), "{size:?}");
        tap(&mut h, BORDER_STYLES[1]);
        assert_eq!(frame(&h, 1).border.style, BorderStyle::Graduated);
        scroll_to(&mut h, SECOND_COLOR);
        tap(&mut h, SECOND_COLOR);
        let shade = format!("{SECOND_COLOR} shade");
        scroll_to(&mut h, &shade);
        check_controls(&h, "second color");
        let before = frame(&h, 1).border.color2;
        let square = control(&h, &shade).unwrap();
        tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
        assert_ne!(frame(&h, 1).border.color2, before, "{size:?}");
        scroll_up_to(&mut h, SECOND_COLOR);
        tap(&mut h, SECOND_COLOR);

        for (name, text) in [(BORDER_WIDTH, "5.6"), (BORDER_DISTANCE, "16.2")] {
            scroll_to(&mut h, &field(name));
            type_value(&mut h, name, text);
        }
        let b = &frame(&h, 1).border;
        let d = b.distances_mm[0];
        assert!(b.width_mm > 5.0 && d > 15.0, "{size:?}: {b:?}");
        assert_eq!(b.distances_mm, [d; 4], "{size:?}");
        assert_eq!(b.width_mm, 5.6, "{size:?}");
        assert_eq!((d * 2.0).round() / 2.0, d);
        check_controls(&h, "border");

        // One side can go inside the box.
        scroll_to(&mut h, DISTANCE_BY_SIDE);
        assert!(control(&h, &field(SIDE_DISTANCES[0])).is_none(), "{size:?}");
        tap(&mut h, DISTANCE_BY_SIDE);
        assert!(control(&h, &field(BORDER_DISTANCE)).is_none(), "{size:?}");
        scroll_to(&mut h, &field(SIDE_DISTANCES[0]));
        type_value(&mut h, SIDE_DISTANCES[0], "-60");
        let b = &frame(&h, 1).border;
        assert!(b.distances_mm[0] < -50.0, "{size:?}: {b:?}");
        assert_eq!(b.distances_mm[1..], [d; 3], "{size:?}");
        for name in &SIDE_DISTANCES[1..] {
            scroll_to(&mut h, &field(name));
            assert!(control(&h, &field(name)).is_some(), "{size:?}: {name}");
        }
        check_controls(&h, "distance by side");
        // Off, all sides take the top distance.
        scroll_up_to(&mut h, DISTANCE_BY_SIDE);
        tap(&mut h, DISTANCE_BY_SIDE);
        let b = &frame(&h, 1).border;
        assert_eq!(b.distances_mm, [b.distances_mm[0]; 4], "{size:?}");

        scroll_to(&mut h, &field(BORDER_GAP));
        type_value(&mut h, BORDER_GAP, "5.6");
        let gap = frame(&h, 1).border.gap_mm;
        assert!(gap > 5.0, "{size:?}: {gap}");
        assert_eq!((gap * 4.0).round() / 4.0, gap);

        scroll_to(&mut h, BORDER_STROKE);
        assert!(
            control(&h, &field(BORDER_STROKE_WIDTH)).is_none(),
            "{size:?}"
        );
        tap(&mut h, BORDER_STROKE);
        assert!(frame(&h, 1).border.stroke, "{size:?}");
        scroll_to(&mut h, &field(BORDER_STROKE_WIDTH));
        type_value(&mut h, BORDER_STROKE_WIDTH, "2.6");
        let width = frame(&h, 1).border.stroke_width;
        assert_eq!(width, 2.6, "{size:?}");
        scroll_to(&mut h, STROKE_COLOR);
        tap(&mut h, STROKE_COLOR);
        let shade = format!("{STROKE_COLOR} shade");
        scroll_to(&mut h, &shade);
        let before = frame(&h, 1).border.stroke_color;
        let square = control(&h, &shade).unwrap();
        tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
        assert_ne!(frame(&h, 1).border.stroke_color, before, "{size:?}");
        check_controls(&h, "border stroke");
        scroll_up_to(&mut h, STROKE_COLOR);
        tap(&mut h, STROKE_COLOR);

        scroll_to(&mut h, REFERENCE_GRID);
        assert!(control(&h, GRID_LABELS).is_none(), "{size:?}");
        tap(&mut h, REFERENCE_GRID);
        assert!(frame(&h, 1).border.grid, "{size:?}");
        scroll_to(&mut h, GRID_COLUMN_NAMES[1]);
        tap(&mut h, GRID_COLUMN_NAMES[1]);
        let columns = frame(&h, 1).border.grid_columns;
        assert_eq!(columns, vale_store::GridColumns::Numbers, "{size:?}");
        check_controls(&h, "reference grid");
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-frame-border");
        }

        scroll_to(&mut h, GRID_LABELS);
        tap(&mut h, GRID_LABELS);
        assert_eq!(h.state().atlas.grid, Some(frame(&h, 1).id));
        set_number(&mut h, GRID_SIZE, "24");
        assert!(frame(&h, 1).border.grid_text.size > 20.0, "{size:?}");
        check_controls(&h, "grid labels");

        // The grid turns off with the border, and the border turns on with
        // the grid.
        tap(&mut h, &format!("Back to {}", FRAME_TABS[0]));
        scroll_up_to(&mut h, BORDER);
        tap(&mut h, BORDER);
        let b = &frame(&h, 1).border;
        assert!(!b.on && !b.grid, "{size:?}");
        scroll_to(&mut h, REFERENCE_GRID);
        tap(&mut h, REFERENCE_GRID);
        let b = &frame(&h, 1).border;
        assert!(b.on && b.grid, "{size:?}");
        // The border works without the grid.
        scroll_to(&mut h, REFERENCE_GRID);
        tap(&mut h, REFERENCE_GRID);
        let b = &frame(&h, 1).border;
        assert!(b.on && !b.grid, "{size:?}");
        check_controls(&h, "border without grid");
    }
}

#[test]
fn the_paper_the_border_and_the_grid_survive_a_close_and_an_open() {
    use vale_app::ui::atlas::{
        edit_grid_text, set_border, set_border_distance, set_border_grid, set_border_stroke,
        set_border_style, set_paper,
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut s = state();
    s.doc = doc;
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    let id = frame(&h, 0).id;
    let st = h.state_mut();
    set_paper(st, [0xf3, 0xef, 0xdf]);
    set_border(st, id, true);
    set_border_style(st, id, vale_store::BorderStyle::Graduated);
    set_border_distance(st, id, None, 4.0);
    set_border_distance(st, id, Some(0), -12.3);
    set_border_stroke(st, id, true);
    set_border_grid(st, id, true);
    vale_app::ui::atlas::set_border_gap(st, id, 2.6);
    vale_app::ui::atlas::set_grid_columns(st, id, vale_store::GridColumns::Numbers);
    edit_grid_text(st, id, true, |t| t.italic = true);
    h.run_steps(2);
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.project.atlas, project.atlas);
    assert_eq!(doc.project.atlas.pages[0].paper, [0xf3, 0xef, 0xdf]);
    let f = doc.project.atlas.frame(id).unwrap();
    assert!(f.border.on && f.border.style == vale_store::BorderStyle::Graduated);
    assert_eq!(f.border.distances_mm, [-12.5, 4.0, 4.0, 4.0]);
    assert!(f.border.stroke && f.border.grid && f.border.grid_text.italic);
    assert_eq!(f.border.gap_mm, 2.5);
    assert_eq!(f.border.grid_columns, vale_store::GridColumns::Numbers);
}

#[test]
fn the_screen_draws_the_line_widths_of_the_page_at_each_zoom() {
    let mut h = inset_pad(BOARDS[0]);
    let id = frame(&h, 0).id;
    let frame = h.state_mut().doc.project.atlas.frame_mut(id).unwrap();
    frame.neatline.width = 2.5;
    frame.graticule.on = true;
    frame.graticule.width = 0.7;
    let (neatline, graticule) = (frame.neatline.clone(), frame.graticule.clone());
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
        let wide = |c: [u8; 4], w: f64| {
            let c = vale_render::Color::rgba(c[0], c[1], c[2], c[3]);
            (list.items.iter()).any(
                |i| matches!(i, Item::Stroke { color, width, .. } if *color == c && *width == w),
            )
        };
        assert!(wide(neatline.color, neatline.width), "neatline at {zoom}");
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
fn the_texture_of_a_frame_holds_the_whole_neatline() {
    let mut h = inset_pad(BOARDS[0]);
    let id = frame(&h, 1).id;
    let width = 8.0;
    let set = |h: &mut Pad, on: bool| {
        let frame = h.state_mut().doc.project.atlas.frame_mut(id).unwrap();
        frame.neatline.width = width;
        frame.neatline.on = on;
    };
    set(&mut h, true);
    h.run_steps(3);
    let place = h.state().atlas.place.unwrap();
    let bbox = place.rect_to_screen(shape_bbox(&frame(&h, 1).shape));
    let reach = |h: &Pad| {
        let r = h.state().atlas.texture_rect(id).unwrap();
        bbox.min.x - r.min.x
    };
    // Half the width of the neatline is outside the shape.
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

/// The UI of the atlas with a frame and a scale bar, with or without the GPU
/// renderer of the maps. Returns the picture and the number of textures that
/// the GPU drew.
fn atlas_picture(gpu: bool) -> (image::RgbaImage, usize) {
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let render_state = egui_kittest::wgpu::create_render_state(
        setup,
        eframe::egui_wgpu::RendererOptions::PREDICTABLE,
    );
    let mut s = state();
    if gpu {
        s.map_renderer = Some(MapRenderer::new(&render_state).expect("the device has compute"));
    }
    let mut h = vale_app::headless::ui_harness_on(s, (1194.0, 834.0), 1.0, render_state);
    h.set_render_every_step(true);
    h.run_steps(3);
    atlas::add_frame(h.state_mut());
    atlas::add_item(h.state_mut(), ItemKind::ScaleBar);
    h.run_steps(4);
    let drawn = h.state().atlas.gpu_textures();
    (h.render().unwrap(), drawn)
}

#[test]
#[ignore = "gpu"]
fn gpu_vello_draws_the_atlas_with_the_pixels_of_the_cpu() {
    let _gpu = one_gpu_test();
    let (cpu, none) = atlas_picture(false);
    let (gpu, drawn) = atlas_picture(true);
    assert_eq!(none, 0);
    // The map of the frame and the scale bar.
    assert_eq!(drawn, 2);
    std::fs::create_dir_all("../../target/app").unwrap();
    gpu.save("../../target/app/test-atlas-gpu.png").unwrap();
    let differ = cpu
        .pixels()
        .zip(gpu.pixels())
        .filter(|(a, b)| a.0.iter().zip(b.0).any(|(x, y)| x.abs_diff(y) > 16))
        .count();
    let part = differ as f64 / f64::from(cpu.width() * cpu.height());
    // Only the antialiasing of some edges differs.
    assert!(part < 0.002, "{differ} pixels differ");
}

#[test]
#[ignore = "gpu"]
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

/// The width of the stroke of the first rule of Rivers: in the style of the
/// layer, or in the style of a map.
fn rivers_width(h: &Pad, map: Option<MapId>) -> f64 {
    let rivers = layer_id(h, "Rivers");
    let doc = &h.state().doc;
    let style = match map {
        Some(map) => doc.map_style(map, rivers).unwrap(),
        None => doc.entry(rivers).unwrap().style.clone(),
    };
    style.style.rules[0].stroke().unwrap().width
}

fn rivers_rgb(h: &Pad) -> [u8; 4] {
    let rivers = layer_id(h, "Rivers");
    let style = &h.state().doc.entry(rivers).unwrap().style;
    let c = style.style.rules[0].stroke().unwrap().color;
    [c.r, c.g, c.b, c.a]
}

/// The style of a layer that a target holds itself, as JSON.
fn own_text(h: &Pad, target: Target, layer: LayerId) -> Option<String> {
    let atlas = &h.state().doc.project.atlas;
    let styles = match target {
        Target::Map(m) => &atlas.map(m)?.styles,
        Target::Style(s) => &atlas.map_style(s)?.styles,
    };
    styles.get(&layer).cloned()
}

/// The style of Rivers that a target holds itself, as JSON.
fn own_rivers(h: &Pad, target: Target) -> Option<String> {
    own_text(h, target, layer_id(h, "Rivers"))
}

/// Opens the page of the first rule of Rivers in the Layers tab of the
/// selected frame.
fn open_rivers_rule(h: &mut Pad) {
    tap(h, FRAME_TABS[2]);
    scroll_to(h, "Open Rivers");
    tap(h, "Open Rivers");
    scroll_to(h, "Default");
    tap(h, "Default");
    scroll_to(h, &format!("{STROKE_WIDTH} value"));
}

/// A tap on the plus button of the width stepper.
fn widen(h: &mut Pad) {
    step(h, STROKE_WIDTH, true);
}

/// The strokes of the flat view with a color and a width.
fn flat_strokes(h: &mut Pad, rgba: [u8; 4], line: f64) -> usize {
    use vale_app::pipeline::{Quality, fonts};
    let mut fonts = fonts().unwrap();
    let s = h.state_mut();
    let composed = s
        .pipeline
        .compose(&s.doc, (1200.0, 800.0), &mut fonts, Quality::Final)
        .unwrap();
    let [r, g, b, a] = rgba;
    let color = vale_render::Color::rgba(r, g, b, a);
    (composed.list.items.iter())
        .filter(
            |i| matches!(i, Item::Stroke { color: c, width, .. } if *c == color && *width == line),
        )
        .count()
}

#[test]
fn a_change_of_the_globe_style_does_not_change_a_map() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    assert_ne!(main, inset);
    let rgb = rivers_rgb(&h);
    let before = rivers_width(&h, None);
    assert!(strokes(&mut h, 0, rgb, before) > 0);
    assert!(strokes(&mut h, 1, rgb, before) > 0);
    assert!(flat_strokes(&mut h, rgb, before) > 0);

    // A change of the width in the World workspace.
    let rivers = layer_id(&h, "Rivers");
    let s = h.state_mut();
    let rule = &mut s.doc.entry_mut(rivers).unwrap().style.style.rules[0];
    let mut stroke = rule.stroke().unwrap();
    stroke.width = before + 1.5;
    rule.set_stroke(Some(stroke));
    s.touch();
    h.run_steps(2);
    let after = rivers_width(&h, None);
    assert_ne!(after, before);
    // The globe and the flat view draw the globe style, and the maps keep
    // their own style.
    assert!(flat_strokes(&mut h, rgb, after) > 0);
    assert_eq!(rivers_width(&h, Some(main)), before);
    assert_eq!(rivers_width(&h, Some(inset)), before);
    for i in 0..2 {
        assert!(strokes(&mut h, i, rgb, before) > 0, "frame {i}");
        assert_eq!(strokes(&mut h, i, rgb, after), 0, "frame {i}");
    }

    // Copy globe style copies the globe style into the map of the inset.
    tap(&mut h, FRAME_TABS[2]);
    scroll_to(&mut h, "Open Rivers");
    tap(&mut h, "Open Rivers");
    scroll_to(&mut h, COPY_GLOBE_STYLE);
    tap(&mut h, COPY_GLOBE_STYLE);
    assert_eq!(rivers_width(&h, Some(inset)), after);
    assert_eq!(rivers_width(&h, Some(main)), before);
    assert!(strokes(&mut h, 1, rgb, after) > 0);
    assert!(strokes(&mut h, 0, rgb, before) > 0);

    // In a linked map, the copy is a local change of the map. Update map
    // style moves it into the map style, and each map of the map style
    // changes.
    let style = h.state_mut().save_as_map_style(main).unwrap();
    h.state_mut().set_map_style_link(inset, Some(style));
    h.run_steps(2);
    assert_eq!(rivers_width(&h, Some(inset)), before);
    h.state_mut().copy_globe_style(inset, rivers);
    h.run_steps(2);
    assert_eq!(rivers_width(&h, Some(main)), before);
    assert_eq!(rivers_width(&h, Some(inset)), after);
    assert!(own_rivers(&h, Target::Map(inset)).is_some());
    h.state_mut().update_map_style(inset);
    h.run_steps(2);
    assert_eq!(rivers_width(&h, Some(main)), after);
    assert_eq!(rivers_width(&h, Some(inset)), after);
    assert_eq!(own_rivers(&h, Target::Map(inset)), None);
}

#[test]
fn the_panel_of_a_layer_in_a_frame_changes_only_its_map() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let before = rivers_width(&h, None);
    let main_text = own_rivers(&h, Target::Map(main));
    assert!(main_text.is_some());
    open_rivers_rule(&mut h);
    // The panel has no switch to the style of the layer.
    assert!(control(&h, "All maps").is_none());
    widen(&mut h);
    let width = rivers_width(&h, Some(inset));
    assert_ne!(width, before);
    assert_eq!(rivers_width(&h, None), before);
    assert_eq!(rivers_width(&h, Some(main)), before);
    assert_eq!(own_rivers(&h, Target::Map(main)), main_text);
    assert_ne!(own_rivers(&h, Target::Map(inset)), main_text);
}

#[test]
fn a_change_in_a_frame_shows_in_each_frame_of_its_map_and_not_in_another_map() {
    let mut h = inset_pad(BOARDS[0]);
    add_frame(&mut h);
    // The second frame and the third frame show one map.
    let (main, inset, third) = (frame(&h, 0).map, frame(&h, 1).map, frame(&h, 2).id);
    h.state_mut()
        .doc
        .project
        .atlas
        .frame_mut(third)
        .unwrap()
        .map = inset;
    h.run_steps(2);
    let rgb = rivers_rgb(&h);
    let before = rivers_width(&h, None);
    open_rivers_rule(&mut h);
    let composes = h.state().atlas.composes;
    widen(&mut h);
    let width = rivers_width(&h, Some(inset));
    assert_ne!(width, before);
    assert!(h.state().atlas.composes > composes);
    assert!(strokes(&mut h, 1, rgb, width) > 0);
    assert!(strokes(&mut h, 2, rgb, width) > 0);
    assert_eq!(strokes(&mut h, 0, rgb, width), 0);
    assert!(strokes(&mut h, 0, rgb, before) > 0);
    assert_eq!(rivers_width(&h, Some(main)), before);
    assert!(flat_strokes(&mut h, rgb, before) > 0);
}

#[test]
fn the_style_controls_of_a_frame_work_with_a_finger() {
    use vale_app::ui::pad::rules::{ADD_RULE, DELETE_RULE, MOVE_DOWN, MOVE_UP};
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = inset_pad(size);
        let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
        let globe = rivers_rules(&h, None);
        tap(&mut h, FRAME_TABS[2]);
        check_controls(&h, &format!("{what}: layers tab"));
        scroll_to(&mut h, "Open Rivers");
        tap(&mut h, "Open Rivers");
        check_controls(&h, &format!("{what}: layer"));
        // A map with no map style has no Reset to map style, and a map with
        // the globe style has nothing to copy.
        scroll_to(&mut h, COPY_GLOBE_STYLE);
        assert!(control(&h, RESET_TO_MAP_STYLE).is_none(), "{what}");
        assert!(control(&h, COPY_GLOBE_STYLE).is_none(), "{what}");

        // Two new rules, and the page of the middle rule.
        for _ in 0..2 {
            scroll_to(&mut h, ADD_RULE);
            check_controls(&h, &format!("{what}: add rule"));
            tap(&mut h, ADD_RULE);
            assert!(h.state().atlas.layer_page.is_some(), "{what}");
            tap(&mut h, "Back to Rivers");
        }
        assert_eq!(rivers_rules(&h, Some(inset)), globe + 2, "{what}");
        let last = h.state().doc.map_style(inset, layer_id(&h, "Rivers"));
        let last = last.unwrap().style.rules.last().unwrap().name.clone();
        scroll_to(&mut h, &last);
        tap(&mut h, &last);
        scroll_to(&mut h, MOVE_UP);
        tap(&mut h, MOVE_UP);
        for name in [MOVE_UP, MOVE_DOWN, DELETE_RULE] {
            scroll_to(&mut h, name);
            assert!(control(&h, name).is_some(), "{what}: {name}");
            check_controls(&h, &format!("{what}: {name}"));
        }
        assert_eq!(rivers_rules(&h, None), globe, "{what}");
        assert_eq!(rivers_rules(&h, Some(main)), globe, "{what}");

        // Copy globe style brings back the globe style.
        tap(&mut h, "Back to Rivers");
        scroll_to(&mut h, COPY_GLOBE_STYLE);
        check_controls(&h, &format!("{what}: changed"));
        tap(&mut h, COPY_GLOBE_STYLE);
        assert_eq!(rivers_rules(&h, Some(inset)), globe, "{what}");

        // A linked map with a style of its own has Reset to map style.
        let style = h.state_mut().save_as_map_style(main).unwrap();
        h.state_mut().set_map_style_link(inset, Some(style));
        h.run_steps(2);
        scroll_to(&mut h, COPY_GLOBE_STYLE);
        assert!(control(&h, RESET_TO_MAP_STYLE).is_none(), "{what}");
        scroll_up_to(&mut h, "Default");
        tap(&mut h, "Default");
        scroll_to(&mut h, &format!("{STROKE_WIDTH} value"));
        widen(&mut h);
        tap(&mut h, "Back to Rivers");
        scroll_to(&mut h, COPY_GLOBE_STYLE);
        assert!(control(&h, RESET_TO_MAP_STYLE).is_some(), "{what}");
        assert!(control(&h, COPY_GLOBE_STYLE).is_some(), "{what}");
        check_controls(&h, &format!("{what}: own style"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-frame-layer-style");
        }
        tap(&mut h, RESET_TO_MAP_STYLE);
        assert_eq!(own_rivers(&h, Target::Map(inset)), None, "{what}");
        assert!(control(&h, RESET_TO_MAP_STYLE).is_none(), "{what}");
    }
}

#[test]
fn the_layers_tab_marks_each_layer_with_a_local_change() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let style = h.state_mut().save_as_map_style(main).unwrap();
    h.state_mut().set_map_style_link(inset, Some(style));
    h.run_steps(2);
    tap(&mut h, FRAME_TABS[2]);
    assert_eq!(h.query_all_by_label(EDITED).count(), 0);
    open_rivers_rule(&mut h);
    widen(&mut h);
    tap(&mut h, "Back to Rivers");
    tap(&mut h, "Back to Layers");
    assert!(h.state().atlas.open_layer().is_none());
    assert_eq!(h.query_all_by_label(EDITED).count(), 1);
    // A switch is a local change too.
    tap(&mut h, "Hide Land");
    assert_eq!(h.query_all_by_label(EDITED).count(), 2);
    check_controls(&h, "layers tab with marks");
    let rivers = layer_id(&h, "Rivers");
    let land = layer_id(&h, "Land");
    assert_eq!(h.state().doc.local_changes(inset), [land, rivers].into());
    assert!(h.state().doc.local_changes(main).is_empty());
}

#[test]
fn each_change_in_a_frame_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    h.state_mut().save_waiting().unwrap();
    let map = frame(&h, 0).map;
    let before = rivers_width(&h, Some(map));
    let text = own_rivers(&h, Target::Map(map));
    open_rivers_rule(&mut h);
    widen(&mut h);
    h.state_mut().save_waiting().unwrap();
    assert_ne!(rivers_width(&h, Some(map)), before);
    tap(&mut h, "Undo");
    assert_eq!(rivers_width(&h, Some(map)), before);
    assert_eq!(own_rivers(&h, Target::Map(map)), text);
    assert!(h.state().can_redo());
}

#[test]
fn the_styles_of_the_maps_survive_a_close_and_an_open() {
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
    open_rivers_rule(&mut h);
    widen(&mut h);
    let width = rivers_width(&h, Some(inset));
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.project.atlas, project.atlas);
    let rivers = layer_id(&h, "Rivers");
    let style = doc.map_style(inset, rivers).unwrap();
    assert_eq!(style.style.rules[0].stroke().unwrap().width, width);
    let main_style = doc.map_style(main, rivers).unwrap();
    assert_eq!(Some(main_style), doc.layer_style(rivers));
}

#[test]
fn the_panel_of_a_raster_layer_changes_its_opacity_and_blend_mode_in_this_map() {
    use vale_app::ui::pad::OPACITY;
    use vale_app::ui::pad::raster::BLEND;
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = inset_pad(size);
        let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
        let height = h.state_mut().add_height();
        let layer = h.state().doc.project.raster_layer(height).unwrap();
        let (layer, name) = (layer.id, layer.name.clone());
        h.run_steps(2);
        tap(&mut h, FRAME_TABS[2]);
        let open = format!("Open {name}");
        scroll_to(&mut h, &open);
        tap(&mut h, &open);
        assert_eq!(h.state().atlas.open_layer(), Some(layer), "{what}");
        check_controls(&h, &format!("{what}: look"));
        let rect = control(&h, OPACITY).unwrap();
        let y = rect.center().y;
        let (from, to) = (pos2(rect.right() - 20.0, y), pos2(rect.left() + 40.0, y));
        drag(&mut h, from, to, Input::Finger);
        let look = h.state().look(Some(inset), layer).unwrap();
        assert!(look.opacity < 0.5, "{what}: {look:?}");
        assert_eq!(h.state().doc.project.raster(height).unwrap().opacity, 1.0);
        assert_eq!(h.state().look(Some(main), layer).unwrap().opacity, 1.0);

        scroll_to(&mut h, BLEND);
        tap(&mut h, BLEND);
        check_controls(&h, &format!("{what}: blend"));
        let multiply = format!("{BLEND} Multiply");
        scroll_to(&mut h, &multiply);
        tap(&mut h, &multiply);
        let look = h.state().look(Some(inset), layer).unwrap();
        assert_eq!(look.blend, vale_store::BlendMode::Multiply, "{what}");
        assert!(look.opacity < 0.5, "{what}: {look:?}");
        let main_look = h.state().look(Some(main), layer).unwrap();
        assert_eq!(main_look.blend, vale_store::BlendMode::Normal, "{what}");
        assert_eq!(
            h.state().look(None, layer).unwrap().blend,
            vale_store::BlendMode::Normal
        );
    }
}

/// The raster symbolizer of the style of a raster layer, in the style of the
/// layer or in the style of a map.
fn raster_style(h: &Pad, map: Option<MapId>, layer: LayerId) -> vale_style::RasterSymbolizer {
    let doc = &h.state().doc;
    let style = match map {
        Some(map) => doc.map_style(map, layer).unwrap(),
        None => doc.layer_style(layer).unwrap(),
    };
    style.raster_symbolizer().unwrap().clone()
}

#[test]
fn the_raster_panel_changes_a_band_color_and_the_hillshade_in_this_map_only() {
    use vale_app::ui::gradient::band_row;
    use vale_app::ui::pad::Page;
    use vale_app::ui::pad::raster::{COLOR, COLOR_STOPS, HILLSHADE, HILLSHADE_ON};
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = inset_pad(size);
        let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
        let height = h.state_mut().add_height();
        let layer = h.state().doc.project.raster_layer(height).unwrap();
        let (layer, name) = (layer.id, layer.name.clone());
        h.run_steps(2);
        let base_colors = h
            .state_mut()
            .globe
            .bands_mut(height)
            .unwrap()
            .colors()
            .to_vec();
        tap(&mut h, FRAME_TABS[2]);
        let open = format!("Open {name}");
        scroll_to(&mut h, &open);
        tap(&mut h, &open);
        check_controls(&h, &format!("{what}: raster"));

        // A band color.
        scroll_to(&mut h, COLOR_STOPS);
        tap(&mut h, COLOR_STOPS);
        assert_eq!(h.state().atlas.layer_page, Some(Page::Stops), "{what}");
        check_controls(&h, &format!("{what}: stops"));
        scroll_to(&mut h, &band_row(0));
        tap(&mut h, &band_row(0));
        let shade = format!("{COLOR} shade");
        let square = control(&h, &shade).unwrap_or_else(|| panic!("{what}: no band card"));
        check_controls(&h, &format!("{what}: band card"));
        tap_with(&mut h, square.center() + vec2(40.0, 30.0), Input::Finger);
        let colors = raster_style(&h, Some(inset), layer).colors;
        assert_ne!(colors[0], base_colors[0], "{what}");
        assert_eq!(colors[1..], base_colors[1..], "{what}");
        assert_eq!(raster_style(&h, None, layer).colors, base_colors, "{what}");
        assert_eq!(
            raster_style(&h, Some(main), layer).colors,
            base_colors,
            "{what}"
        );
        let globe_colors = h
            .state_mut()
            .globe
            .bands_mut(height)
            .unwrap()
            .colors()
            .to_vec();
        assert_eq!(globe_colors, base_colors, "{what}");

        // The hillshade.
        // In a narrow sheet, the card can cover the back button, and a tap
        // outside the card closes it.
        let back = format!("Back to {name}");
        let state = h.state_mut();
        state.pad.style = Default::default();
        state.globe.preview.selected = None;
        h.run_steps(2);
        tap(&mut h, &back);
        assert_eq!(h.state().atlas.layer_page, None, "{what}");
        scroll_to(&mut h, HILLSHADE);
        tap(&mut h, HILLSHADE);
        assert_eq!(h.state().atlas.layer_page, Some(Page::Hillshade), "{what}");
        tap(&mut h, HILLSHADE_ON);
        check_controls(&h, &format!("{what}: hillshade"));
        assert!(raster_style(&h, Some(inset), layer).hillshade.on, "{what}");
        assert!(!raster_style(&h, None, layer).hillshade.on, "{what}");
        assert!(!h.state().doc.project.raster(height).unwrap().hillshade.on);
        assert_eq!(
            raster_style(&h, Some(inset), layer).colors,
            colors,
            "{what}"
        );
    }
}

#[test]
fn a_page_point_grows_and_shrinks_with_the_zoom_of_the_page() {
    use vale_app::ui::atlas::{PagePlace, PageView};
    let place = PagePlace {
        rect: Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)),
        pts_per_mm: vale_app::atlas::PT_PER_MM,
    };
    assert_eq!(place.pixels_per_page_point(2.0), 2.0);
    let zoomed = place.viewed(PageView {
        zoom: 3.0,
        offset: Vec2::ZERO,
    });
    assert_eq!(zoomed.pixels_per_page_point(2.0), 6.0);
}

/// A page with one frame, or with a main frame and an inset frame, and the
/// Measure tool, with no snap.
fn measure_pad(frames: usize) -> Pad {
    let mut h = pad();
    for _ in 0..frames {
        add_frame(&mut h);
    }
    h.state_mut().globe.snap.on = false;
    tap(&mut h, MEASURE);
    assert!(h.state().measuring());
    h
}

/// A screen position in a frame, as fractions of the box of its shape.
fn in_box(h: &Pad, index: usize, x: f64, y: f64) -> Pos2 {
    let b = shape_bbox(&frame(h, index).shape);
    screen(h, Point::new(b.x0 + x * b.width(), b.y0 + y * b.height()))
}

fn toggled(h: &Pad, name: &str) -> Option<bool> {
    use egui_kittest::kittest::NodeT;
    let node = h.query_by_label(name)?;
    Some(node.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True))
}

fn close_to(a: f64, b: f64, fraction: f64) -> bool {
    ((a - b) / b).abs() < fraction
}

fn same_place(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

#[test]
fn a_tap_in_a_frame_adds_the_place_under_it_with_the_great_circle_length() {
    for input in INPUTS {
        let mut h = measure_pad(1);
        let mut places = Vec::new();
        for (x, y) in [(0.3, 0.3), (0.7, 0.35), (0.5, 0.7)] {
            let at = in_box(&h, 0, x, y);
            places.push(place_at(&mut h, 0, at));
            tap_with(&mut h, at, input);
        }
        let points = h.state().measure.positions();
        assert_eq!(points.len(), 3, "{input:?}");
        for (a, b) in points.iter().zip(&places) {
            assert!(same_place(*a, *b), "{input:?}: {a:?} {b:?}");
        }
        let radius = h.state().doc.project.world.radius_km;
        let length = h.state().measure.summary(radius).length_km;
        let exact = path_length(&places, false, radius);
        assert!(close_to(length, exact, 1e-4), "{input:?}: {length} {exact}");
        let text = readout_line(h.state());
        assert!(text.starts_with("Length "), "{input:?}: {text}");
        // The overlay draws each segment in the frame.
        let place = place(&h);
        let lines = measure_lines(h.state_mut(), place);
        assert!(lines.iter().all(|l| !l.is_empty()), "{input:?}");
    }
}

#[test]
fn a_tap_outside_the_frames_adds_nothing_and_a_drag_moves_the_page() {
    for input in INPUTS {
        let mut h = measure_pad(1);
        let at = margin(&h);
        tap_with(&mut h, at, input);
        assert!(h.state().measure.is_empty(), "{input:?}");
        let (before, maps_before) = (place(&h), maps(&h));
        let from = middle(&h, 0);
        drag(&mut h, from, from + vec2(60.0, 30.0), input);
        let moved = place(&h).rect.min - before.rect.min;
        assert!(
            (moved - vec2(60.0, 30.0)).length() < 1.0,
            "{input:?}: {moved:?}"
        );
        assert!(h.state().measure.is_empty(), "{input:?}");
        assert_eq!(maps(&h), maps_before, "{input:?}");
        // Two fingers move and zoom the page, and add no point.
        let (center, before) = (middle(&h, 0), place(&h));
        pinch(&mut h, center, 100.0, 150.0, 0.0, Vec2::ZERO);
        let zoom = place(&h).pts_per_mm / before.pts_per_mm;
        assert!((zoom - 1.5).abs() < 0.01, "{input:?}: {zoom}");
        assert!(h.state().measure.is_empty(), "{input:?}");
        assert_eq!(maps(&h), maps_before, "{input:?}");
    }
}

#[test]
fn a_path_crosses_two_frames_and_closes_on_its_first_point_in_either_frame() {
    let ring = [[-60.0, 20.0], [100.0, 30.0], [30.0, -10.0]];
    for (closing, radius) in [(1, 6371.0), (0, 1500.0)] {
        let mut h = measure_pad(2);
        h.state_mut().doc.project.world.radius_km = radius;
        // The inset is above the main frame at the bottom right of the page.
        for (place, index) in ring.iter().zip([0, 1, 0]) {
            let at = pos_of(&mut h, index, *place);
            tap_with(&mut h, at, Input::Finger);
        }
        let points = h.state().measure.positions();
        assert_eq!(points.len(), 3);
        for (a, b) in points.iter().zip(&ring) {
            assert!(same_place(*a, *b), "{a:?} {b:?}");
        }
        // The segment into the inset shows in both frames.
        let place = place(&h);
        let lines = measure_lines(h.state_mut(), place);
        let inset = place.rect_to_screen(shape_bbox(&frame(&h, 1).shape));
        let parts = &lines[0];
        assert!(
            parts
                .iter()
                .flatten()
                .any(|p| inset.shrink(1.0).contains(*p))
        );
        assert!(
            parts
                .iter()
                .flatten()
                .any(|p| !inset.expand(1.0).contains(*p))
        );
        let first = pos_of(&mut h, closing, ring[0]);
        tap_with(&mut h, first + vec2(3.0, 2.0), Input::Finger);
        assert!(h.state().measure.closed, "frame {closing}");
        assert_eq!(h.state().measure.points.len(), 3);
        let summary = h.state().measure.summary(radius);
        let exact = ring_area(&ring, radius);
        assert!(
            close_to(summary.area_km2.unwrap(), exact, 1e-3),
            "{summary:?}"
        );
        let text = readout_line(h.state());
        assert!(text.contains(" · Area "), "{text}");
    }
}

#[test]
fn a_point_in_a_frame_snaps_to_a_vertex_only_when_snap_is_on() {
    for on in [true, false] {
        let mut h = measure_pad(1);
        let layers = h.state().doc.project.layers();
        let places = layers.iter().find(|l| l.name == "Places").unwrap();
        let target = match &places.features[0].geometry {
            Geometry::Points(points) => points[0],
            _ => panic!("not a point"),
        };
        let state = h.state_mut();
        state.globe.snap.on = on;
        // The frame shows the place at a large scale, away from the coast.
        let id = frame(&h, 0).id;
        let record = h.state_mut().doc.project.atlas.frame_mut(id).unwrap();
        record.center = target;
        record.scale = 2.0e5;
        h.run_steps(2);
        let at = pos_of(&mut h, 0, target) + vec2(6.0, 6.0);
        tap_with(&mut h, at, Input::Pen);
        let point = h.state().measure.points[0];
        assert_eq!(point.pos == target, on, "{on} {point:?} {target:?}");
        assert_eq!(point.exact, on, "{on}");
    }
}

#[test]
fn the_path_is_not_saved_and_a_page_change_or_a_tool_change_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = measure_pad(1);
    let state = h.state_mut();
    state.session = Some(Session::open(&dir.path().join("w.gpkg"), &mut state.doc).unwrap());
    state.save_waiting().unwrap();
    h.run_steps(2);
    let project = h.state().doc.project.clone();
    let saves = h.state().session.as_ref().unwrap().saves();
    let log = undo_log(&h);
    let taps = |h: &mut Pad| {
        for (x, y) in [(0.3, 0.3), (0.7, 0.35), (0.5, 0.7)] {
            let at = in_box(h, 0, x, y);
            tap_with(h, at, Input::Finger);
        }
    };
    taps(&mut h);
    let first = in_box(&h, 0, 0.3, 0.3);
    tap_with(&mut h, first, Input::Finger);
    assert!(h.state().measure.closed);
    let later = Instant::now() + std::time::Duration::from_secs(5);
    h.state_mut().save_tick(false, later);
    assert_eq!(h.state().doc.project, project);
    assert_eq!(h.state().session.as_ref().unwrap().saves(), saves);
    assert_eq!(undo_log(&h), log);

    // A page change clears the path.
    vale_app::ui::atlas::add_page(h.state_mut());
    h.run_steps(2);
    assert!(h.state().measuring());
    assert!(h.state().measure.is_empty());
    tap(&mut h, PREVIOUS_PAGE);
    taps(&mut h);
    assert_eq!(h.state().measure.points.len(), 3);
    tap(&mut h, NEXT_PAGE);
    assert!(h.state().measure.is_empty());
    tap(&mut h, PREVIOUS_PAGE);
    taps(&mut h);
    // A cell of the tool strip leaves the Measure tool and clears the path.
    assert_ne!(toggled(&h, "Frame tool"), Some(true));
    tap(&mut h, "Frame tool");
    assert_eq!(h.state().atlas.tool, AtlasTool::Frame);
    assert!(h.state().measure.is_empty());
    // The Measure cell chooses the tool again, and Move leaves it.
    tap(&mut h, MEASURE);
    assert_eq!(h.state().atlas.tool, AtlasTool::Measure);
    assert_eq!(toggled(&h, MEASURE), Some(true));
    taps(&mut h);
    tap(&mut h, "Move");
    assert_eq!(h.state().atlas.tool, AtlasTool::Move);
    assert!(h.state().measure.is_empty());
}

/// The state of the undo log of the project file.
fn undo_log(h: &Pad) -> (bool, bool, u64) {
    let session = h.state().session.as_ref().unwrap();
    (session.can_undo(), session.can_redo(), session.undo_bytes())
}

#[test]
fn undo_and_redo_act_on_the_path_in_the_atlas_and_not_on_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = state();
    s.session = Some(Session::open(&dir.path().join("w.gpkg"), &mut s.doc).unwrap());
    // A tap lasts less than 0.3 seconds, and a step of the harness is short.
    let mut h = Harness::builder()
        .with_size(vec2(BOARDS[0].0, BOARDS[0].1))
        .with_pixels_per_point(1.0)
        .with_step_dt(0.01)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), s);
    h.run_steps(3);
    add_frame(&mut h);
    h.state_mut().save_waiting().unwrap();
    h.state_mut().globe.snap.on = false;
    tap(&mut h, MEASURE);
    let (frames, log) = (open_page(&h).frames.clone(), undo_log(&h));
    assert!(log.0);
    let undo_able = |h: &Pad| {
        use egui_kittest::kittest::NodeT;
        let off = |name| h.get_by_label(name).accesskit_node().is_disabled();
        (!off("Undo"), !off("Redo"))
    };
    assert_eq!(undo_able(&h), (false, false));
    for (x, y) in [(0.3, 0.3), (0.7, 0.35), (0.5, 0.7), (0.3, 0.3)] {
        let at = in_box(&h, 0, x, y);
        tap_with(&mut h, at, Input::Finger);
    }
    let path = |h: &Pad| (h.state().measure.points.len(), h.state().measure.closed);
    assert_eq!(path(&h), (3, true));
    assert_eq!(undo_able(&h), (true, false));
    // A tap of two fingers reopens the path, and a tap of three closes it.
    let empty = pos2(200.0, 780.0);
    fingers_tap(&mut h, 2, empty);
    assert_eq!(path(&h), (3, false));
    assert_eq!(undo_able(&h), (true, true));
    fingers_tap(&mut h, 3, empty);
    assert_eq!(path(&h), (3, true));
    // The buttons and the keys do the same.
    tap(&mut h, "Undo");
    tap(&mut h, "Undo");
    assert_eq!(path(&h), (2, false));
    tap(&mut h, "Redo");
    assert_eq!(path(&h), (3, false));
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
    assert_eq!(path(&h), (2, false));
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT);
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(path(&h), (3, true));
    // Clear path is one step.
    tap(&mut h, CLEAR_PATH);
    assert!(h.state().measure.is_empty());
    tap(&mut h, "Undo");
    assert_eq!(path(&h), (3, true));

    assert_eq!(open_page(&h).frames, frames);
    assert_eq!(undo_log(&h), log);
    // After the Measure tool, undo takes away the frame.
    tap(&mut h, "Frame tool");
    tap(&mut h, "Undo");
    assert!(open_page(&h).frames.is_empty());
}

#[test]
fn the_measure_card_acts_on_the_path_in_the_atlas() {
    let mut h = measure_pad(1);
    // With no points, the readout is the readout of the page.
    assert_eq!(h.state().measure_readout(), None);
    for (x, y) in [(0.3, 0.3), (0.7, 0.35), (0.5, 0.7)] {
        let at = in_box(&h, 0, x, y);
        tap_with(&mut h, at, Input::Finger);
    }
    let card = h.state().pad.rects.unwrap().brush.unwrap();
    for name in [SNAP, CLEAR_PATH, "Brush settings"] {
        let rect = control(&h, name).unwrap_or_else(|| panic!("{name}"));
        assert!(card.contains_rect(rect), "{name}");
    }
    for name in ["Close path", "Remove point"] {
        assert!(control(&h, name).is_none(), "{name}");
    }
    tap(&mut h, CLEAR_PATH);
    assert!(h.state().measure.is_empty());
    tap(&mut h, SNAP);
    assert!(h.state().globe.snap.on);
    tap(&mut h, "Brush settings");
    assert!(h.state().pad.is_open(Panel::Brush));
    for name in [SNAPPING, SNAP_VERTICES, SNAP_EDGES, SNAP_DISTANCE] {
        assert!(control(&h, name).is_some(), "{name}");
    }
    check_controls(&h, "Atlas with the snapping settings");
}

/// The label classes of a layer in a map.
fn classes(h: &Pad, map: MapId, layer: &str) -> Vec<vale_style::LabelClass> {
    h.state().doc.label_classes(Some(map), layer_id(h, layer))
}

/// The items of a frame of the open page.
fn frame_items(h: &mut Pad, index: usize) -> Vec<Item> {
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
        .compose_frame(&s.doc, &page, &f, target, &mut fonts, Quality::Final)
        .unwrap();
    composed.list.items
}

/// The label runs of a frame of the open page.
fn label_runs(h: &mut Pad, index: usize) -> Vec<vale_render::GlyphRun> {
    (frame_items(h, index).into_iter())
        .filter_map(|i| match i {
            Item::Glyphs(g) => Some(g),
            _ => None,
        })
        .collect()
}

/// The color of the labels of the places in the sample.
const PLACES_COLOR: vale_render::Color = vale_render::Color::rgb(30, 30, 30);

/// The texts of the labels of a frame in the color of the places.
fn place_texts(h: &mut Pad, index: usize) -> Vec<String> {
    (label_runs(h, index).into_iter())
        .filter(|g| g.color == PLACES_COLOR)
        .map(|g| g.text)
        .collect()
}

fn uppercase(text: &str) -> bool {
    text.chars().any(char::is_alphabetic) && text == text.to_uppercase()
}

/// A pad with one frame and its Labels tab.
fn labels_pad(size: (f32, f32)) -> Pad {
    let mut h = pad_with(state(), size);
    add_frame(&mut h);
    tap(&mut h, FRAME_TABS[3]);
    h
}

#[test]
fn the_labels_tab_makes_a_label_class() {
    use vale_app::ui::FontTarget;
    use vale_app::ui::pad::fonts::FONT;
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    let places = layer_id(&h, "Places");
    assert_eq!(classes(&h, map, "Places").len(), 1);
    tap(&mut h, ADD_CLASS);
    assert_eq!(h.state().atlas.open_class(), Some((places, 1)));
    // The default class stays.
    let list = classes(&h, map, "Places");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].name, "Places");
    assert_eq!(list[1].name, "Class 1");

    type_text(&mut h, TEXT, "upper(name)");
    tap(&mut h, FONT);
    let target = FontTarget::Class(map, places, 1);
    assert!(h.state().actions.contains(&Action::PickFont(target)));
    h.state_mut()
        .run_action(Action::SetFont(target, "Noto Serif".into()));
    h.run_steps(2);
    // 14 pt, a size that fits the frame.
    set_number(&mut h, LABEL_SIZE, "14");
    scroll_to(&mut h, COLOR);
    tap(&mut h, COLOR);
    let shade = format!("{COLOR} shade");
    scroll_to(&mut h, &shade);
    let square = control(&h, &shade).unwrap();
    tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
    tap(&mut h, COLOR);
    scroll_to(&mut h, HALO);
    tap(&mut h, HALO);
    assert!(!classes(&h, map, "Places")[1].halo);
    tap(&mut h, HALO);

    let class = classes(&h, map, "Places")[1].clone();
    assert_eq!(class.text, "upper(name)");
    assert_eq!(class.font.as_deref(), Some("Noto Serif"));
    assert!(
        class.size > 11.0 && class.size == class.size.round(),
        "{}",
        class.size
    );
    assert_ne!(class.color, PLACES_COLOR);
    assert!(class.halo);
    let runs = label_runs(&mut h, 0);
    let ours: Vec<_> = (runs.iter()).filter(|g| g.color == class.color).collect();
    assert!(!ours.is_empty());
    for g in ours {
        assert!(uppercase(&g.text), "{}", g.text);
        assert_eq!(f64::from(g.font_size), class.size, "{}", g.text);
    }
}

#[test]
fn the_placement_choices_match_the_feature_type() {
    use vale_style::LabelPlacement;
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    let names = |kind| LabelPlacement::choices(kind).iter().map(|p| p.name());
    tap(&mut h, "Places");
    tap(&mut h, SEGMENTS[1].0);
    tap(&mut h, PLACEMENT);
    for name in names(GeometryKind::Point) {
        assert!(control(&h, name).is_some(), "{name}");
    }
    for name in names(GeometryKind::Line) {
        assert!(control(&h, name).is_none(), "{name}");
    }
    tap(&mut h, LabelPlacement::Around.name());
    assert_eq!(
        classes(&h, map, "Places")[0].placement,
        LabelPlacement::Around
    );
    tap(&mut h, "Back to Labels");
    tap(&mut h, "Rivers");
    assert!(control(&h, PLACEMENT).is_none());
    tap(&mut h, SEGMENTS[1].0);
    tap(&mut h, PLACEMENT);
    for name in names(GeometryKind::Line) {
        assert!(control(&h, name).is_some(), "{name}");
    }
    for name in names(GeometryKind::Point) {
        assert!(control(&h, name).is_none(), "{name}");
    }
    tap(&mut h, "Back to Rivers");
    tap(&mut h, "Back to Labels");

    // A class that moves to a line layer gets a line placement.
    tap(&mut h, "Places");
    tap(&mut h, LAYER);
    assert!(control(&h, "Places").is_some());
    assert!(control(&h, "Land").is_some());
    tap(&mut h, "Rivers");
    assert!(classes(&h, map, "Places").is_empty());
    let rivers = classes(&h, map, "Rivers");
    assert_eq!(rivers.len(), 2);
    assert_eq!(rivers[1].name, "Places");
    assert_eq!(rivers[1].placement, LabelPlacement::AboveOrBelow);
    assert_eq!(
        h.state().atlas.open_class(),
        Some((layer_id(&h, "Rivers"), 1))
    );
}

#[test]
fn the_size_stepper_of_a_class_moves_one_step_and_a_typed_size_stays_in_the_range() {
    use vale_app::ui::pad::labels::SIZES;
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    let size = |h: &Pad| classes(h, map, "Places")[0].size;
    tap(&mut h, "Places");
    let before = size(&h);
    step(&mut h, LABEL_SIZE, true);
    assert_eq!(size(&h), before + 1.0);
    step(&mut h, LABEL_SIZE, false);
    assert_eq!(size(&h), before);
    set_number(&mut h, LABEL_SIZE, "200");
    assert_eq!(size(&h), SIZES.1);
    set_number(&mut h, LABEL_SIZE, "1");
    assert_eq!(size(&h), SIZES.0);
}

#[test]
fn a_change_of_a_class_places_the_labels_again_while_you_edit() {
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    tap(&mut h, "Places");
    let composes = h.state().atlas.composes;
    // A hold on the plus button of the size repeats.
    let plus = control(&h, &format!("Increase {LABEL_SIZE}"))
        .unwrap()
        .center();
    drag_steps(&mut h, plus, plus, Input::Finger, 12);
    // The finger is still down.
    let size = classes(&h, map, "Places")[0].size;
    assert_ne!(size, 11.0);
    assert!(h.state().atlas.composes > composes);
    let runs = label_runs(&mut h, 0);
    let places: Vec<_> = runs.iter().filter(|g| g.color == PLACES_COLOR).collect();
    assert!(!places.is_empty());
    assert!(places.iter().all(|g| f64::from(g.font_size) == size));
    release(&mut h, plus, Input::Finger);

    // One edit of the text, and the field keeps the focus.
    let composes = h.state().atlas.composes;
    type_text(&mut h, TEXT, "upper(name)");
    assert_eq!(classes(&h, map, "Places")[0].text, "upper(name)");
    assert!(h.state().atlas.composes > composes);
    let texts = place_texts(&mut h, 0);
    assert!(
        !texts.is_empty() && texts.iter().all(|t| uppercase(t)),
        "{texts:?}"
    );
    // A text that is not an expression waits and keeps the labels.
    type_text(&mut h, TEXT, "upper(");
    assert!(control(&h, TEXT).is_some());
    assert_eq!(classes(&h, map, "Places")[0].text, "upper(name)");
}

#[test]
fn two_maps_label_one_layer_with_their_own_classes() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    tap(&mut h, FRAME_TABS[3]);
    tap(&mut h, "Places");
    type_text(&mut h, TEXT, "upper(name)");
    assert_eq!(classes(&h, inset, "Places")[0].text, "upper(name)");
    assert_eq!(classes(&h, main, "Places")[0].text, "name");
    assert!(
        h.state()
            .doc
            .project
            .atlas
            .map(main)
            .unwrap()
            .labels
            .is_empty()
    );
    let main_texts = place_texts(&mut h, 0);
    let inset_texts = place_texts(&mut h, 1);
    assert!(main_texts.iter().any(|t| !uppercase(t)), "{main_texts:?}");
    assert!(!inset_texts.is_empty());
    assert!(inset_texts.iter().all(|t| uppercase(t)), "{inset_texts:?}");
}

#[test]
fn each_frame_of_a_map_places_labels_with_its_classes() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).id);
    let atlas = &mut h.state_mut().doc.project.atlas;
    atlas.frame_mut(inset).unwrap().map = main;
    h.run_steps(2);
    tap(&mut h, FRAME_TABS[3]);
    tap(&mut h, "Places");
    type_text(&mut h, TEXT, "upper(name)");
    for index in 0..2 {
        let texts = place_texts(&mut h, index);
        assert!(!texts.is_empty(), "frame {index}");
        assert!(texts.iter().all(|t| uppercase(t)), "{index}: {texts:?}");
    }
}

#[test]
fn the_controls_of_the_labels_tab_work_with_a_finger() {
    use vale_app::ui::atlas::ClassSegment;
    use vale_app::ui::pad::filter::{ADD_CONDITION, VALUE};
    use vale_style::LabelPlacement;
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = labels_pad(size);
        let map = frame(&h, 0).map;
        check_controls(&h, &format!("{what}: labels tab"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-label-list");
        }
        tap(&mut h, ADD_CLASS);
        assert_eq!(classes(&h, map, "Places").len(), 2, "{what}");
        check_controls(&h, &format!("{what}: class"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-label-class");
        }
        tap(&mut h, LAYER);
        check_controls(&h, &format!("{what}: layer page"));
        tap(&mut h, "Back to Class 1");

        scroll_to(&mut h, ADD_CONDITION);
        tap(&mut h, ADD_CONDITION);
        check_controls(&h, &format!("{what}: condition page"));
        scroll_to(&mut h, "scalerank");
        tap(&mut h, "scalerank");
        check_controls(&h, &format!("{what}: condition operators"));
        scroll_to(&mut h, VALUE);
        tap(&mut h, VALUE);
        scroll_to(&mut h, &format!("{VALUE} Done"));
        check_controls(&h, &format!("{what}: condition keypad"));
        tap(&mut h, &format!("{VALUE} 2"));
        tap(&mut h, &format!("{VALUE} Done"));
        scroll_to(&mut h, ADD_CONDITION);
        tap(&mut h, ADD_CONDITION);
        assert_eq!(classes(&h, map, "Places")[1].filter, "scalerank = 2");

        scroll_to(&mut h, STYLE);
        tap(&mut h, STYLE);
        check_controls(&h, &format!("{what}: style page"));
        tap(&mut h, "Regular");
        scroll_to(&mut h, CASE);
        tap(&mut h, CASE);
        check_controls(&h, &format!("{what}: case page"));
        tap(&mut h, "Title case");
        scroll_up_to(&mut h, COLOR);
        scroll_to(&mut h, COLOR);
        tap(&mut h, COLOR);
        scroll_to(&mut h, &format!("{COLOR} hue"));
        check_controls(&h, &format!("{what}: color"));
        scroll_up_to(&mut h, COLOR);
        tap(&mut h, COLOR);
        scroll_to(&mut h, HALO_COLOR);
        tap(&mut h, HALO_COLOR);
        scroll_to(&mut h, &format!("{HALO_COLOR} hue"));
        check_controls(&h, &format!("{what}: halo color"));
        scroll_up_to(&mut h, HALO_COLOR);
        tap(&mut h, HALO_COLOR);
        scroll_to(&mut h, DELETE_CLASS);
        check_controls(&h, &format!("{what}: bottom of the class"));

        tap(&mut h, SEGMENTS[1].0);
        scroll_up_to(&mut h, PLACEMENT);
        check_controls(&h, &format!("{what}: placement segment"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-label-placement");
        }
        tap(&mut h, PLACEMENT);
        check_controls(&h, &format!("{what}: placement page"));
        tap(&mut h, LabelPlacement::Around.name());
        tap(&mut h, POSITIONS);
        check_controls(&h, &format!("{what}: positions page"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-label-positions");
        }
        tap(&mut h, "Top right");
        assert!(!classes(&h, map, "Places")[1].positions[0].on, "{what}");
        tap(&mut h, "Back to Class 1");
        for name in [&value_of(OFFSET), MOVE_UP] {
            scroll_to(&mut h, name);
            assert!(control(&h, name).is_some(), "{what}: {name}");
        }
        tap(&mut h, SEGMENTS[2].0);
        scroll_up_to(&mut h, &value_of(BUFFER));
        check_controls(&h, &format!("{what}: conflicts segment"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-label-conflicts");
        }
        for name in [&value_of(BUFFER), REMOVE_DUPLICATES] {
            assert!(control(&h, name).is_some(), "{what}: {name}");
        }
        let distance = value_of(DUPLICATE_DISTANCE);
        assert!(control(&h, &distance).is_none(), "{what}");
        tap(&mut h, REMOVE_DUPLICATES);
        assert!(classes(&h, map, "Places")[1].remove_duplicates, "{what}");
        assert!(control(&h, &distance).is_some(), "{what}");
        check_controls(&h, &format!("{what}: duplicate distance"));
        tap(&mut h, "Back to Labels");

        // The placement options of a line class.
        tap(&mut h, "Rivers");
        assert_eq!(h.state().atlas.label_segment, ClassSegment::Text, "{what}");
        tap(&mut h, SEGMENTS[1].0);
        let numbers = [OFFSET, REPEAT_INTERVAL, CURVE_LIMIT].map(value_of);
        for name in [&numbers[0], REPEAT, &numbers[1], &numbers[2], SMOOTHING] {
            scroll_to(&mut h, name);
            assert!(control(&h, name).is_some(), "{what}: {name}");
        }
        assert!(control(&h, POSITIONS).is_none(), "{what}");
        check_controls(&h, &format!("{what}: line placement segment"));
        tap(&mut h, "Back to Labels");
        tap(&mut h, "Class 1");
        scroll_to(&mut h, DELETE_CLASS);
        tap(&mut h, DELETE_CLASS);
        assert_eq!(classes(&h, map, "Places").len(), 1, "{what}");
        assert!(control(&h, ADD_CLASS).is_some(), "{what}");
    }
}

/// The halo strokes of a frame with this color and width.
fn halo_strokes(h: &mut Pad, index: usize, of: vale_render::Color, line: f64) -> usize {
    (frame_items(h, index).iter())
        .filter(
            |i| matches!(i, Item::Stroke { color, width, .. } if *color == of && *width == line),
        )
        .count()
}

#[test]
fn the_style_row_lists_the_faces_of_the_family() {
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    tap(&mut h, "Places");
    tap(&mut h, STYLE);
    let faces = h.state_mut().fonts.faces("Noto Sans");
    assert!(faces.len() >= 2, "{faces:?}");
    for face in &faces {
        assert!(control(&h, &face.name()).is_some(), "{}", face.name());
    }
    assert!(control(&h, "Bold").is_none());
    tap(&mut h, "Italic");
    let class = classes(&h, map, "Places")[0].clone();
    assert!(class.italic);
    assert_eq!(class.weight, 400.0);
    assert_eq!(h.state().atlas.label_page, None);
    const ITALIC_LEN: usize = include_bytes!("../../../assets/fonts/NotoSans-Italic.ttf").len();
    let runs = label_runs(&mut h, 0);
    let places: Vec<_> = runs.iter().filter(|g| g.color == PLACES_COLOR).collect();
    assert!(!places.is_empty());
    for g in places {
        assert_eq!(g.font.data.as_ref().len(), ITALIC_LEN, "{}", g.text);
    }
}

#[test]
fn the_case_row_changes_the_text() {
    use vale_style::TextCase;
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    let texts = place_texts(&mut h, 0);
    assert!(texts.iter().any(|t| !uppercase(t)), "{texts:?}");
    tap(&mut h, "Places");
    scroll_to(&mut h, CASE);
    tap(&mut h, CASE);
    for case in TextCase::ALL {
        assert!(control(&h, case.name()).is_some(), "{}", case.name());
    }
    tap(&mut h, TextCase::Upper.name());
    assert_eq!(classes(&h, map, "Places")[0].case, TextCase::Upper);
    let texts = place_texts(&mut h, 0);
    assert!(
        !texts.is_empty() && texts.iter().all(|t| uppercase(t)),
        "{texts:?}"
    );
}

#[test]
fn the_halo_has_a_color_and_a_width() {
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    let before = classes(&h, map, "Places")[0].clone();
    tap(&mut h, "Places");
    scroll_to(&mut h, HALO_COLOR);
    tap(&mut h, HALO_COLOR);
    let shade = format!("{HALO_COLOR} shade");
    scroll_to(&mut h, &shade);
    let square = control(&h, &shade).unwrap();
    tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
    tap(&mut h, HALO_COLOR);
    scroll_to(&mut h, &value_of(HALO_WIDTH));
    step(&mut h, HALO_WIDTH, true);
    let class = classes(&h, map, "Places")[0].clone();
    let (old, new) = (before.halo_color, class.halo_color);
    assert_ne!((new.r, new.g, new.b), (old.r, old.g, old.b));
    assert_eq!(new.a, old.a);
    assert!(class.halo_width > before.halo_width, "{}", class.halo_width);
    let places = place_texts(&mut h, 0).len();
    assert!(places > 0);
    assert_eq!(
        halo_strokes(&mut h, 0, class.halo_color, class.halo_width),
        places
    );

    scroll_to(&mut h, HALO);
    tap(&mut h, HALO);
    assert!(control(&h, HALO_COLOR).is_none());
    assert!(control(&h, &value_of(HALO_WIDTH)).is_none());
    assert_eq!(
        halo_strokes(&mut h, 0, class.halo_color, class.halo_width),
        0
    );
}

#[test]
fn the_alpha_of_the_text_color_reaches_the_label() {
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    tap(&mut h, "Places");
    // The Text section has no opacity slider. The color holds the opacity.
    assert!(control(&h, &value_of("Opacity")).is_none());
    pick_opacity(&mut h, COLOR, 0.5);
    let class = classes(&h, map, "Places")[0].clone();
    let alpha = class.color.a;
    assert!((alpha as i32 - 128).abs() <= 2, "{alpha}");
    let runs = label_runs(&mut h, 0);
    let rgb = |c: vale_render::Color| (c.r, c.g, c.b);
    let places: Vec<_> = (runs.iter())
        .filter(|g| rgb(g.color) == rgb(PLACES_COLOR))
        .collect();
    assert!(!places.is_empty());
    assert!(places.iter().all(|g| g.color.a == alpha));
    // The halo keeps its own alpha.
    let halo = class.halo_color;
    assert!(halo_strokes(&mut h, 0, halo, 2.2) >= places.len());
}

#[test]
fn the_filter_of_a_class_has_the_condition_builder() {
    use vale_app::ui::atlas::ClassPage;
    use vale_app::ui::pad::filter::{ADD_CONDITION, NO_CLASS_CONDITION, REMOVE_CONDITION};
    use vale_app::ui::pad::rules::EXPRESSION;
    use vale_store::Value;
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    tap(&mut h, "Places");
    assert!(h.query_by_label(NO_CLASS_CONDITION).is_some());
    scroll_to(&mut h, ADD_CONDITION);
    tap(&mut h, ADD_CONDITION);
    assert_eq!(h.state().atlas.label_page, Some(ClassPage::Condition));
    scroll_to(&mut h, "featurecla");
    tap(&mut h, "featurecla");
    scroll_to(&mut h, "Admin-1 capital");
    tap(&mut h, "Admin-1 capital");
    scroll_to(&mut h, ADD_CONDITION);
    tap(&mut h, ADD_CONDITION);
    assert_eq!(h.state().atlas.label_page, None);
    let text = "featurecla = 'Admin-1 capital'";
    assert_eq!(classes(&h, map, "Places")[0].filter, text);
    assert!(control(&h, &format!("{REMOVE_CONDITION} 1")).is_some());

    let layer = h.state().doc.layer(layer_id(&h, "Places")).unwrap();
    let wanted = Value::Text("Admin-1 capital".into());
    let names: Vec<String> = (layer.features.iter())
        .filter(|f| f.attributes.get("featurecla") == Some(&wanted))
        .filter_map(|f| f.attributes.get("name")?.label())
        .collect();
    let texts = place_texts(&mut h, 0);
    assert!(!texts.is_empty());
    for t in &texts {
        assert!(names.contains(t), "{t}");
    }

    // The Expression field holds the whole filter.
    type_text(&mut h, EXPRESSION, "scalerank < 1");
    assert_eq!(classes(&h, map, "Places")[0].filter, "scalerank < 1");
    let first = format!("{REMOVE_CONDITION} 1");
    scroll_to(&mut h, &first);
    tap(&mut h, &first);
    assert_eq!(classes(&h, map, "Places")[0].filter, "");
}

#[test]
fn the_class_list_is_in_priority_order_and_a_drag_reorders_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    h.state_mut().save_waiting().unwrap();
    let map = frame(&h, 0).map;
    tap(&mut h, FRAME_TABS[3]);
    let order = |h: &Pad| -> Vec<String> {
        (h.state().label_class_list(map).into_iter())
            .map(|(_, _, c)| c.name)
            .collect()
    };
    let above =
        |h: &Pad, a: &str, b: &str| control(h, a).unwrap().top() < control(h, b).unwrap().top();
    assert_eq!(order(&h), ["Places", "Rivers"]);
    assert!(above(&h, "Places", "Rivers"));

    let from = control(&h, &format!("{REORDER} Places")).unwrap().center();
    let to = from + vec2(0.0, 60.0);
    drag_steps(&mut h, from, to, Input::Finger, 8);
    release(&mut h, to, Input::Finger);
    assert_eq!(order(&h), ["Rivers", "Places"]);
    let priority = |h: &Pad, layer: &str| classes(h, map, layer)[0].priority;
    assert!(priority(&h, "Rivers") > priority(&h, "Places"));
    assert!(above(&h, "Rivers", "Places"));

    tap(&mut h, "Undo");
    assert_eq!(order(&h), ["Places", "Rivers"]);
    assert!(
        h.state()
            .doc
            .project
            .atlas
            .map(map)
            .unwrap()
            .labels
            .is_empty()
    );

    // Move up and Move down on the page of a class.
    tap(&mut h, "Rivers");
    scroll_to(&mut h, MOVE_UP);
    tap(&mut h, MOVE_UP);
    assert_eq!(order(&h), ["Rivers", "Places"]);
    tap(&mut h, MOVE_DOWN);
    assert_eq!(order(&h), ["Places", "Rivers"]);
    tap(&mut h, "Undo");
    assert_eq!(order(&h), ["Rivers", "Places"]);
}

#[test]
fn deleting_the_last_class_of_a_layer_takes_its_labels_away() {
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    assert!(!place_texts(&mut h, 0).is_empty());
    tap(&mut h, "Places");
    scroll_to(&mut h, DELETE_CLASS);
    tap(&mut h, DELETE_CLASS);
    assert!(classes(&h, map, "Places").is_empty());
    assert!(place_texts(&mut h, 0).is_empty());
    assert!(control(&h, "Places").is_none());
    assert!(control(&h, "Rivers").is_some());
}

#[test]
fn the_label_classes_of_each_map_survive_a_close_and_an_open() {
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
    tap(&mut h, FRAME_TABS[3]);
    tap(&mut h, ADD_CLASS);
    type_text(&mut h, TEXT, "upper(name)");
    scroll_to(&mut h, HALO);
    tap(&mut h, HALO);
    let saved = classes(&h, inset, "Places");
    assert_eq!(saved.len(), 2);
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.project.atlas, project.atlas);
    let places = layer_id(&h, "Places");
    assert_eq!(doc.label_classes(Some(inset), places), saved);
    assert_eq!(doc.label_classes(Some(main), places).len(), 1);
}

#[test]
fn each_change_of_a_class_is_one_undo_step() {
    use vale_style::TextCase;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    h.state_mut().save_waiting().unwrap();
    let map = frame(&h, 0).map;
    tap(&mut h, FRAME_TABS[3]);
    tap(&mut h, ADD_CLASS);
    scroll_to(&mut h, HALO);
    tap(&mut h, HALO);
    scroll_up_to(&mut h, CASE);
    tap(&mut h, CASE);
    tap(&mut h, TextCase::Upper.name());
    let class = |h: &Pad| classes(h, map, "Places").get(1).cloned();
    let c = class(&h).unwrap();
    assert!(!c.halo);
    assert_eq!(c.case, TextCase::Upper);

    tap(&mut h, "Undo");
    let c = class(&h).unwrap();
    assert!(!c.halo);
    assert_eq!(c.case, TextCase::Normal);
    tap(&mut h, "Undo");
    assert!(class(&h).unwrap().halo);
    tap(&mut h, "Undo");
    assert!(class(&h).is_none());
    assert!(
        h.state()
            .doc
            .project
            .atlas
            .map(map)
            .unwrap()
            .labels
            .is_empty()
    );
    assert_eq!(classes(&h, map, "Places").len(), 1);
}

#[test]
fn the_class_page_has_text_placement_and_conflict_segments() {
    use vale_app::ui::atlas::ClassSegment;
    use vale_app::ui::pad::fonts::FONT;
    let mut h = labels_pad(BOARDS[0]);
    tap(&mut h, "Places");
    let shows = |h: &mut Pad, name: &str| {
        scroll_to(h, name);
        control(h, name).is_some()
    };
    assert_eq!(h.state().atlas.label_segment, ClassSegment::Text);
    let size = value_of(LABEL_SIZE);
    for name in [TEXT, FONT, STYLE, &size, CASE, COLOR, HALO] {
        assert!(shows(&mut h, name), "{name}");
    }
    let (offset, buffer) = (value_of(OFFSET), value_of(BUFFER));
    for name in [PLACEMENT, &offset, &buffer] {
        assert!(control(&h, name).is_none(), "{name}");
    }
    assert!(shows(&mut h, DELETE_CLASS));

    tap(&mut h, SEGMENTS[1].0);
    assert_eq!(h.state().atlas.label_segment, ClassSegment::Placement);
    scroll_up_to(&mut h, PLACEMENT);
    for name in [PLACEMENT, POSITIONS, &offset, DELETE_CLASS] {
        assert!(shows(&mut h, name), "{name}");
    }
    for name in [TEXT, &buffer, REPEAT] {
        assert!(control(&h, name).is_none(), "{name}");
    }

    tap(&mut h, SEGMENTS[2].0);
    scroll_up_to(&mut h, &buffer);
    let distance = value_of(DUPLICATE_DISTANCE);
    for name in [&buffer, REMOVE_DUPLICATES, &distance, DELETE_CLASS] {
        assert!(shows(&mut h, name), "{name}");
    }
    for name in [TEXT, &offset] {
        assert!(control(&h, name).is_none(), "{name}");
    }

    // Another class opens on the Text segment.
    tap(&mut h, "Back to Labels");
    tap(&mut h, "Rivers");
    assert_eq!(h.state().atlas.label_segment, ClassSegment::Text);
    assert!(control(&h, TEXT).is_some());
}

/// The place on the sphere of the feature of a point layer with this name.
fn place_of(h: &Pad, layer: &str, name: &str) -> [f64; 2] {
    use vale_store::Value;
    let l = h.state().doc.layer(layer_id(h, layer)).unwrap();
    let f = (l.features.iter())
        .find(|f| f.attributes.get("name") == Some(&Value::Text(name.into())))
        .unwrap();
    match &f.geometry {
        Geometry::Points(p) => p[0],
        _ => panic!("not a point"),
    }
}

/// A place on the sphere in the composed frame of `frame_items`.
fn frame_point(h: &mut Pad, index: usize, lonlat: [f64; 2]) -> Point {
    use vale_app::atlas::{FrameTarget, PT_PER_MM};
    let f = frame(h, index);
    let target = FrameTarget {
        origin: Point::ZERO,
        pts_per_mm: PT_PER_MM,
    };
    let s = h.state_mut();
    let projection = s.pipeline.projection(&s.doc, f.map).unwrap();
    let vt = frame_transform(projection, &f, target);
    vt.to_page(projection.forward(lonlat).unwrap())
}

/// The labels of the places in frame 0: the text, the point of the place,
/// and the origins of the glyphs.
fn place_labels(h: &mut Pad) -> Vec<(String, Point, Vec<Point>)> {
    let runs: Vec<_> = (label_runs(h, 0).into_iter())
        .filter(|g| g.color == PLACES_COLOR)
        .collect();
    runs.into_iter()
        .map(|g| {
            let at = place_of(h, "Places", &g.text);
            let point = frame_point(h, 0, at);
            let origins = g.glyphs.iter().map(|(_, t)| *t * Point::ZERO).collect();
            (g.text, point, origins)
        })
        .collect()
}

#[test]
fn the_positions_page_ranks_and_blocks_the_point_positions() {
    use vale_style::LabelPosition;
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    tap(&mut h, "Places");
    tap(&mut h, SEGMENTS[1].0);
    tap(&mut h, POSITIONS);
    let order = |h: &Pad| -> Vec<LabelPosition> {
        (classes(h, map, "Places")[0].positions.iter())
            .map(|r| r.position)
            .collect()
    };
    assert_eq!(order(&h), LabelPosition::DEFAULT_ORDER);

    // A finger drags Right from the fifth place to the top.
    let right = LabelPosition::Right.name();
    let from = control(&h, &format!("{REORDER} {right}")).unwrap().center();
    let to = control(&h, &format!("{REORDER} Top right"))
        .unwrap()
        .center();
    drag_steps(&mut h, from, to - vec2(0.0, 10.0), Input::Finger, 24);
    release(&mut h, to - vec2(0.0, 10.0), Input::Finger);
    let mut wanted = LabelPosition::DEFAULT_ORDER.to_vec();
    wanted.retain(|p| *p != LabelPosition::Right);
    wanted.insert(0, LabelPosition::Right);
    assert_eq!(order(&h), wanted);
    let above =
        |h: &Pad, a: &str, b: &str| control(h, a).unwrap().top() < control(h, b).unwrap().top();
    assert!(above(&h, right, "Top right"));

    // A tap blocks each position but Right.
    for p in LabelPosition::DEFAULT_ORDER {
        if p != LabelPosition::Right {
            tap(&mut h, p.name());
        }
    }
    let ranks = classes(&h, map, "Places")[0].positions.clone();
    assert_eq!(ranks[0].position, LabelPosition::Right);
    assert!(ranks[0].on);
    assert!(ranks[1..].iter().all(|r| !r.on));
    tap(&mut h, "Back to Places");

    let labels = place_labels(&mut h);
    assert!(!labels.is_empty());
    let size = classes(&h, map, "Places")[0].size;
    for (text, point, origins) in labels {
        for o in &origins {
            assert!(o.x > point.x, "{text}: {o:?} {point:?}");
        }
        let y = origins.iter().map(|o| o.y).sum::<f64>() / origins.len() as f64;
        // The baseline is a little under the middle of the text.
        assert!((y - point.y).abs() < size, "{text}: {y} {point:?}");
    }
}

/// The name of the value field of the number `name`.
fn value_of(name: &str) -> String {
    format!("{name} value")
}

/// Scrolls to the number `name` and types `text` on its keypad.
fn set_number(h: &mut Pad, name: &str, text: &str) {
    scroll_to(h, &value_of(name));
    type_value(h, name, text);
}

#[test]
fn the_offset_moves_the_labels_away_from_the_feature() {
    use vale_style::LabelPosition;
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    let places = layer_id(&h, "Places");
    // Only Right, so that the offset is a distance along x.
    let mut list = classes(&h, map, "Places");
    for r in &mut list[0].positions {
        r.on = r.position == LabelPosition::Right;
    }
    h.state_mut().doc.set_label_classes(map, places, &list);
    h.state_mut().touch();
    tap(&mut h, "Places");
    tap(&mut h, SEGMENTS[1].0);
    let gaps = |h: &mut Pad| -> std::collections::HashMap<String, f64> {
        (place_labels(h).into_iter())
            .map(|(text, point, origins)| {
                let left = origins.iter().map(|o| o.x).fold(f64::MAX, f64::min);
                (text, left - point.x)
            })
            .collect()
    };
    set_number(&mut h, OFFSET, "0");
    assert_eq!(classes(&h, map, "Places")[0].offset, 0.0);
    let near = gaps(&mut h);
    set_number(&mut h, OFFSET, "20");
    assert_eq!(classes(&h, map, "Places")[0].offset, 20.0);
    let far = gaps(&mut h);
    let both: Vec<_> = near.keys().filter(|t| far.contains_key(*t)).collect();
    assert!(!both.is_empty());
    for t in both {
        let gain = far[t] - near[t];
        assert!(gain > 15.0 && gain < 25.0, "{t}: {gain}");
    }
}

/// The count of the labels of the rivers in frame 0.
fn river_labels(h: &mut Pad) -> usize {
    let color = classes(h, frame(h, 0).map, "Rivers")[0].color;
    let color = vale_render::Color::rgba(color.r, color.g, color.b, color.a);
    (label_runs(h, 0).iter())
        .filter(|g| g.color == color)
        .count()
}

#[test]
fn the_repeat_options_change_the_line_labels() {
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    // Only the rivers have labels, at a scale that shows the long rivers
    // long, and a repeat may come near the label before it.
    let id = frame(&h, 0).id;
    let (places, river_layer) = (layer_id(&h, "Places"), layer_id(&h, "Rivers"));
    h.state_mut().doc.set_label_classes(map, places, &[]);
    let mut rivers = classes(&h, map, "Rivers");
    rivers[0].remove_duplicates = false;
    h.state_mut()
        .doc
        .set_label_classes(map, river_layer, &rivers);
    let f = h.state_mut().doc.project.atlas.frame_mut(id).unwrap();
    f.scale /= 4.0;
    f.center = [-60.0, -5.0];
    h.state_mut().touch();
    h.run_steps(2);
    tap(&mut h, "Rivers");
    tap(&mut h, SEGMENTS[1].0);
    assert!(classes(&h, map, "Rivers")[0].repeat);
    set_number(&mut h, REPEAT_INTERVAL, "0");
    assert_eq!(classes(&h, map, "Rivers")[0].repeat_distance, 50.0);
    let repeated = river_labels(&mut h);
    scroll_to(&mut h, REPEAT);
    tap(&mut h, REPEAT);
    assert!(!classes(&h, map, "Rivers")[0].repeat);
    assert!(control(&h, &value_of(REPEAT_INTERVAL)).is_none());
    let once = river_labels(&mut h);
    assert!(once > 0 && once < repeated, "{once} {repeated}");

    // The curve limit is in degrees per letter.
    set_number(&mut h, CURVE_LIMIT, "90");
    let limit = classes(&h, map, "Rivers")[0].curve_limit;
    assert!((limit - 60f64.to_radians()).abs() < 1e-9, "{limit}");
    set_number(&mut h, SMOOTHING, "0");
    assert_eq!(classes(&h, map, "Rivers")[0].smoothing, 0.0);
}

#[test]
fn the_labels_tab_edits_a_polygon_class() {
    use vale_style::LabelPlacement;
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    let land = layer_id(&h, "Land");
    let mut class = vale_style::LabelClass::new("Land", GeometryKind::Polygon, 9).unwrap();
    class.text = "'Land'".into();
    h.state_mut().doc.set_label_classes(map, land, &[class]);
    h.state_mut().touch();
    h.run_steps(2);
    assert!(label_runs(&mut h, 0).iter().any(|g| g.text == "LAND"));
    tap(&mut h, "Land");
    tap(&mut h, SEGMENTS[1].0);
    for name in [SPREAD, CURVE_LIMIT] {
        assert!(control(&h, &value_of(name)).is_some(), "{name}");
    }
    for name in [OFFSET, REPEAT_INTERVAL, SMOOTHING] {
        assert!(control(&h, &value_of(name)).is_none(), "{name}");
    }
    assert!(control(&h, REPEAT).is_none());
    assert!(control(&h, POSITIONS).is_none());
    set_number(&mut h, SPREAD, "2");
    assert_eq!(classes(&h, map, "Land")[0].spread, 2.0);
    set_number(&mut h, CURVE_LIMIT, "20");
    let limit = classes(&h, map, "Land")[0].curve_limit;
    assert!((limit - 20f64.to_radians()).abs() < 1e-9, "{limit}");

    scroll_up_to(&mut h, PLACEMENT);
    tap(&mut h, PLACEMENT);
    for p in LabelPlacement::choices(GeometryKind::Polygon) {
        assert!(control(&h, p.name()).is_some(), "{}", p.name());
    }
    for p in LabelPlacement::choices(GeometryKind::Line) {
        assert!(control(&h, p.name()).is_none(), "{}", p.name());
    }
    tap(&mut h, LabelPlacement::Horizontal.name());
    assert_eq!(
        classes(&h, map, "Land")[0].placement,
        LabelPlacement::Horizontal
    );
    assert!(control(&h, &value_of(SPREAD)).is_some());
    assert!(control(&h, &value_of(CURVE_LIMIT)).is_none());
    assert!(label_runs(&mut h, 0).iter().any(|g| g.text == "LAND"));

    tap(&mut h, PLACEMENT);
    tap(&mut h, LabelPlacement::Curved.name());
    assert_eq!(
        classes(&h, map, "Land")[0].placement,
        LabelPlacement::Curved
    );
    for name in [SPREAD, CURVE_LIMIT] {
        assert!(control(&h, &value_of(name)).is_some(), "{name}");
    }
    assert!(label_runs(&mut h, 0).iter().any(|g| g.text == "LAND"));
}

/// The labeler placement of the first class of a layer.
fn labeler_placement(h: &Pad, map: MapId, layer: &str, kind: GeometryKind) -> Placement {
    vale_app::labels::placement(&classes(h, map, layer)[0], kind)
}

/// A pad with the Labels tab of a frame, and a polygon class "Land" with the text "Land".
fn land_pad(size: (f32, f32)) -> (Pad, MapId) {
    let mut h = labels_pad(size);
    let map = frame(&h, 0).map;
    let land = layer_id(&h, "Land");
    let mut class = vale_style::LabelClass::new("Land", GeometryKind::Polygon, 9).unwrap();
    class.text = "'Land'".into();
    h.state_mut().doc.set_label_classes(map, land, &[class]);
    h.state_mut().touch();
    h.run_steps(2);
    (h, map)
}

#[test]
fn the_labels_tab_places_a_polygon_class_along_a_parallel() {
    use vale_style::LabelPlacement;
    let (mut h, map) = land_pad(BOARDS[0]);
    tap(&mut h, "Land");
    tap(&mut h, SEGMENTS[1].0);
    tap(&mut h, PLACEMENT);
    tap(&mut h, LabelPlacement::Parallel.name());
    assert_eq!(LabelPlacement::Parallel.name(), "Along parallel");
    assert_eq!(
        classes(&h, map, "Land")[0].placement,
        LabelPlacement::Parallel
    );
    let Placement::Polygon(p) = labeler_placement(&h, map, "Land", GeometryKind::Polygon) else {
        panic!("not a polygon placement");
    };
    assert_eq!(p.mode, PolygonMode::Parallel);
    for name in [SPREAD, CURVE_LIMIT] {
        assert!(control(&h, &value_of(name)).is_some(), "{name}");
    }
    set_number(&mut h, SPREAD, "2");
    let Placement::Polygon(p) = labeler_placement(&h, map, "Land", GeometryKind::Polygon) else {
        panic!("not a polygon placement");
    };
    assert_eq!(p.max_spread, 2.0);
    // The text of this placement always follows the parallels.
    assert!(control(&h, FOLLOW_PARALLELS).is_none());
    assert!(control(&h, UPSIDE_DOWN).is_some());
    assert!(label_runs(&mut h, 0).iter().any(|g| g.text == "LAND"));
}

#[test]
fn the_parallel_switches_reach_the_labeler() {
    use vale_style::LabelPlacement;
    let (mut h, map) = land_pad(BOARDS[0]);

    tap(&mut h, "Places");
    tap(&mut h, SEGMENTS[1].0);
    assert!(control(&h, UPSIDE_DOWN).is_none());
    scroll_to(&mut h, FOLLOW_PARALLELS);
    tap(&mut h, FOLLOW_PARALLELS);
    assert!(classes(&h, map, "Places")[0].graticule);
    scroll_to(&mut h, UPSIDE_DOWN);
    tap(&mut h, UPSIDE_DOWN);
    assert!(classes(&h, map, "Places")[0].no_flip);
    let Placement::Point(p) = labeler_placement(&h, map, "Places", GeometryKind::Point) else {
        panic!("not a point placement");
    };
    assert!(p.graticule && p.no_flip);
    tap(&mut h, "Back to Labels");

    tap(&mut h, "Land");
    tap(&mut h, SEGMENTS[1].0);
    assert!(control(&h, UPSIDE_DOWN).is_none());
    scroll_to(&mut h, FOLLOW_PARALLELS);
    tap(&mut h, FOLLOW_PARALLELS);
    scroll_to(&mut h, UPSIDE_DOWN);
    tap(&mut h, UPSIDE_DOWN);
    let c = &classes(&h, map, "Land")[0];
    assert!(c.graticule && c.no_flip);
    let Placement::Polygon(p) = labeler_placement(&h, map, "Land", GeometryKind::Polygon) else {
        panic!("not a polygon placement");
    };
    assert!(p.graticule && p.no_flip);
    assert!(label_runs(&mut h, 0).iter().any(|g| g.text == "LAND"));

    // Curved text does not follow the parallels.
    scroll_up_to(&mut h, PLACEMENT);
    tap(&mut h, PLACEMENT);
    tap(&mut h, LabelPlacement::Curved.name());
    assert!(control(&h, FOLLOW_PARALLELS).is_none());
    assert!(control(&h, UPSIDE_DOWN).is_none());
    tap(&mut h, PLACEMENT);
    tap(&mut h, LabelPlacement::Horizontal.name());
    for name in [FOLLOW_PARALLELS, UPSIDE_DOWN] {
        scroll_to(&mut h, name);
        assert!(control(&h, name).is_some(), "{name}");
    }
}

#[test]
fn the_parallel_switches_work_with_a_finger() {
    use vale_style::LabelPlacement;
    for size in BOARDS {
        let what = format!("{size:?}");
        let (mut h, map) = land_pad(size);
        tap(&mut h, "Places");
        tap(&mut h, SEGMENTS[1].0);
        scroll_to(&mut h, FOLLOW_PARALLELS);
        tap(&mut h, FOLLOW_PARALLELS);
        scroll_to(&mut h, UPSIDE_DOWN);
        check_controls(&h, &format!("{what}: point class"));
        assert!(control(&h, UPSIDE_DOWN).is_some(), "{what}");
        tap(&mut h, "Back to Labels");
        tap(&mut h, "Land");
        tap(&mut h, SEGMENTS[1].0);
        tap(&mut h, PLACEMENT);
        check_controls(&h, &format!("{what}: placement page"));
        tap(&mut h, LabelPlacement::Parallel.name());
        assert_eq!(
            classes(&h, map, "Land")[0].placement,
            LabelPlacement::Parallel,
            "{what}"
        );
        scroll_to(&mut h, UPSIDE_DOWN);
        check_controls(&h, &format!("{what}: polygon class"));
        assert!(control(&h, UPSIDE_DOWN).is_some(), "{what}");
    }
}

#[test]
fn each_change_of_a_parallel_switch_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    h.state_mut().save_waiting().unwrap();
    let map = frame(&h, 0).map;
    let class = |h: &Pad| classes(h, map, "Places")[0].clone();
    tap(&mut h, FRAME_TABS[3]);
    tap(&mut h, "Places");
    tap(&mut h, SEGMENTS[1].0);
    let start = class(&h);
    scroll_to(&mut h, FOLLOW_PARALLELS);
    tap(&mut h, FOLLOW_PARALLELS);
    scroll_to(&mut h, UPSIDE_DOWN);
    tap(&mut h, UPSIDE_DOWN);
    let c = class(&h);
    assert!(c.graticule && c.no_flip);

    tap(&mut h, "Undo");
    let c = class(&h);
    assert!(c.graticule && !c.no_flip);
    tap(&mut h, "Undo");
    assert_eq!(class(&h), start);
}

#[test]
fn the_parallel_options_survive_a_close_and_an_open() {
    use vale_style::LabelPlacement;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut s = state();
    s.doc = doc;
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    let map = frame(&h, 0).map;
    tap(&mut h, FRAME_TABS[3]);
    tap(&mut h, "Places");
    tap(&mut h, SEGMENTS[1].0);
    scroll_to(&mut h, FOLLOW_PARALLELS);
    tap(&mut h, FOLLOW_PARALLELS);
    scroll_to(&mut h, UPSIDE_DOWN);
    tap(&mut h, UPSIDE_DOWN);
    let land = layer_id(&h, "Land");
    let mut area = vale_style::LabelClass::new("Land", GeometryKind::Polygon, 9).unwrap();
    area.placement = LabelPlacement::Parallel;
    area.no_flip = true;
    h.state_mut().doc.set_label_classes(map, land, &[area]);
    let places = classes(&h, map, "Places");
    assert!(places[0].graticule && places[0].no_flip);
    let lands = classes(&h, map, "Land");
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.label_classes(Some(map), layer_id(&h, "Places")), places);
    assert_eq!(doc.label_classes(Some(map), land), lands);
}

/// The polygon placement of the class "Land".
fn land_placement(h: &Pad, map: MapId) -> vale_labeler::PolygonPlacement {
    match labeler_placement(h, map, "Land", GeometryKind::Polygon) {
        Placement::Polygon(p) => p,
        _ => panic!("not a polygon placement"),
    }
}

/// Opens the Placement segment of the class "Land".
fn open_land_placement(h: &mut Pad) {
    tap(h, "Land");
    tap(h, SEGMENTS[1].0);
}

#[test]
fn a_tap_on_a_zone_changes_its_rank() {
    let (mut h, map) = land_pad(BOARDS[0]);
    open_land_placement(&mut h);
    scroll_to(&mut h, ZONES);
    tap(&mut h, ZONES);
    let top_left = zone_name(true, 0);
    assert_eq!(top_left, "Inside top left");
    assert_eq!(land_placement(&h, map).zones.inside[0], 1);
    tap(&mut h, &top_left);
    assert_eq!(classes(&h, map, "Land")[0].zones_inside[0], 2);
    assert_eq!(land_placement(&h, map).zones.inside[0], 2);
    tap(&mut h, &top_left);
    assert_eq!(land_placement(&h, map).zones.inside[0], 3);
    tap(&mut h, &top_left);
    assert_eq!(land_placement(&h, map).zones.inside[0], 0);
    tap(&mut h, &top_left);
    assert_eq!(land_placement(&h, map).zones.inside[0], 1);
    // The outside grid has no center.
    assert_eq!(zone_name(false, 4), "Outside right");
    assert!(control(&h, "Outside center").is_none());
    tap(&mut h, &zone_name(false, 4));
    assert_eq!(land_placement(&h, map).zones.outside[4], 1);
}

#[test]
fn an_outside_zone_shows_the_leader_line() {
    use vale_labeler::LeaderAnchor;
    let (mut h, map) = land_pad(BOARDS[0]);
    open_land_placement(&mut h);
    scroll_to(&mut h, ZONES);
    assert!(control(&h, LEADER_LINE).is_none());
    tap(&mut h, ZONES);
    tap(&mut h, "Outside right");
    tap(&mut h, "Back to Land");
    scroll_to(&mut h, LEADER_LINE);
    tap(&mut h, LEADER_LINE);
    tap(&mut h, vale_style::LeaderAnchor::Outline.name());
    assert_eq!(vale_style::LeaderAnchor::Outline.name(), "Nearest edge");
    assert_eq!(land_placement(&h, map).anchor, LeaderAnchor::Outline);
}

#[test]
fn the_polygon_switches_reach_the_labeler() {
    let (mut h, map) = land_pad(BOARDS[0]);
    open_land_placement(&mut h);
    let p = land_placement(&h, map);
    assert!(!p.over_holes && !p.avoid_other_polygons && p.largest_part_only);
    assert_eq!(p.repeat_distance, None);
    for name in [OVER_HOLES, AVOID_POLYGONS, LARGEST_PART, REPEAT_LABEL] {
        scroll_to(&mut h, name);
        tap(&mut h, name);
    }
    let p = land_placement(&h, map);
    assert!(p.over_holes && p.avoid_other_polygons && !p.largest_part_only);
    assert_eq!(p.repeat_distance, Some(vale_style::POLYGON_REPEAT_DISTANCE));
    scroll_to(&mut h, REPEAT_LABEL);
    tap(&mut h, REPEAT_LABEL);
    assert_eq!(land_placement(&h, map).repeat_distance, None);
    assert!(control(&h, &value_of(REPEAT_DISTANCE)).is_none());
}

#[test]
fn the_polygon_numbers_reach_the_labeler_in_points() {
    let (mut h, map) = land_pad(BOARDS[0]);
    open_land_placement(&mut h);
    scroll_to(&mut h, REPEAT_LABEL);
    tap(&mut h, REPEAT_LABEL);
    set_number(&mut h, REPEAT_DISTANCE, "800");
    set_number(&mut h, OVERRUN, "3");
    set_number(&mut h, MIN_AREA, "500");
    let c = &classes(&h, map, "Land")[0];
    assert_eq!(
        (c.polygon_repeat_distance, c.overrun, c.min_area),
        (800.0, 3.0, 500.0)
    );
    // A page unit is one point.
    let p = land_placement(&h, map);
    assert_eq!(p.repeat_distance, Some(800.0));
    assert_eq!(p.overrun, 3.0);
    assert_eq!(p.min_area, 500.0);
}

#[test]
fn the_polygon_options_work_with_a_finger() {
    for size in BOARDS {
        let what = format!("{size:?}");
        let (mut h, map) = land_pad(size);
        open_land_placement(&mut h);
        scroll_to(&mut h, ZONES);
        tap(&mut h, ZONES);
        check_controls(&h, &format!("{what}: zones page"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-label-zones");
        }
        for k in 0..9 {
            assert!(control(&h, &zone_name(true, k)).is_some(), "{what}: {k}");
        }
        // A small sheet shows the outside grid after a scroll.
        for k in 0..8 {
            scroll_to(&mut h, &zone_name(false, k));
            assert!(control(&h, &zone_name(false, k)).is_some(), "{what}: {k}");
        }
        check_controls(&h, &format!("{what}: outside zones"));
        tap(&mut h, &zone_name(false, 0));
        assert_eq!(classes(&h, map, "Land")[0].zones_outside[0], 1, "{what}");
        tap(&mut h, "Back to Land");
        scroll_to(&mut h, LEADER_LINE);
        tap(&mut h, LEADER_LINE);
        check_controls(&h, &format!("{what}: leader page"));
        tap(&mut h, "Back to Land");
        scroll_to(&mut h, REPEAT_LABEL);
        tap(&mut h, REPEAT_LABEL);
        for name in [
            OVER_HOLES,
            AVOID_POLYGONS,
            LARGEST_PART,
            &value_of(REPEAT_DISTANCE),
            &value_of(OVERRUN),
            &value_of(MIN_AREA),
        ] {
            scroll_up_to(&mut h, name);
            scroll_to(&mut h, name);
            assert!(control(&h, name).is_some(), "{what}: {name}");
            check_controls(&h, &format!("{what}: {name}"));
        }
    }
}

#[test]
fn each_tap_on_a_zone_or_a_polygon_switch_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    let map = frame(&h, 0).map;
    let land = layer_id(&h, "Land");
    let area = vale_style::LabelClass::new("Land", GeometryKind::Polygon, 9).unwrap();
    h.state_mut().doc.set_label_classes(map, land, &[area]);
    h.state_mut().touch();
    h.state_mut().save_waiting().unwrap();
    let class = |h: &Pad| classes(h, map, "Land")[0].clone();
    tap(&mut h, FRAME_TABS[3]);
    open_land_placement(&mut h);
    let start = class(&h);
    scroll_to(&mut h, ZONES);
    tap(&mut h, ZONES);
    tap(&mut h, "Inside center");
    tap(&mut h, "Back to Land");
    scroll_to(&mut h, OVER_HOLES);
    tap(&mut h, OVER_HOLES);
    let c = class(&h);
    assert!(c.over_holes);
    assert_eq!(c.zones_inside[4], 2);

    tap(&mut h, "Undo");
    let c = class(&h);
    assert!(!c.over_holes);
    assert_eq!(c.zones_inside[4], 2);
    tap(&mut h, "Undo");
    assert_eq!(class(&h), start);
}

#[test]
fn the_polygon_options_survive_a_close_and_an_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut s = state();
    s.doc = doc;
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    let map = frame(&h, 0).map;
    let land = layer_id(&h, "Land");
    let area = vale_style::LabelClass::new("Land", GeometryKind::Polygon, 9).unwrap();
    h.state_mut().doc.set_label_classes(map, land, &[area]);
    h.state_mut().touch();
    h.run_steps(2);
    tap(&mut h, FRAME_TABS[3]);
    open_land_placement(&mut h);
    scroll_to(&mut h, ZONES);
    tap(&mut h, ZONES);
    tap(&mut h, "Inside top");
    tap(&mut h, "Outside bottom");
    tap(&mut h, "Back to Land");
    scroll_to(&mut h, LEADER_LINE);
    tap(&mut h, LEADER_LINE);
    tap(&mut h, vale_style::LeaderAnchor::Center.name());
    tap(&mut h, "Back to Land");
    for name in [OVER_HOLES, AVOID_POLYGONS, LARGEST_PART, REPEAT_LABEL] {
        scroll_to(&mut h, name);
        tap(&mut h, name);
    }
    set_number(&mut h, REPEAT_DISTANCE, "600");
    set_number(&mut h, OVERRUN, "2");
    set_number(&mut h, MIN_AREA, "250");
    let lands = classes(&h, map, "Land");
    let c = &lands[0];
    assert_eq!((c.zones_inside[1], c.zones_outside[6]), (2, 1));
    assert_eq!(c.anchor, vale_style::LeaderAnchor::Center);
    assert!(c.over_holes && c.avoid_polygons && !c.largest_part && c.polygon_repeat);
    assert_eq!(
        (c.polygon_repeat_distance, c.overrun, c.min_area),
        (600.0, 2.0, 250.0)
    );
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.label_classes(Some(map), land), lands);
}

/// The color of the leader lines in the leader tests.
const LEADER_COLOR: vale_render::Color = vale_render::Color::rgb(200, 10, 90);
/// The color of the halo of the leader lines in the leader tests.
const LEADER_HALO: vale_render::Color = vale_render::Color::rgb(10, 200, 90);

/// A leader line in the colors of the leader tests.
fn leader_style() -> vale_style::LeaderStyle {
    vale_style::LeaderStyle {
        color: Some(LEADER_COLOR),
        halo_color: Some(LEADER_HALO),
        ..Default::default()
    }
}

/// The items of the first frame, with the class "Land" outside its polygons and its leader
/// line in `leader`.
fn leader_frame(h: &mut Pad, map: MapId, leader: vale_style::LeaderStyle) -> Vec<Item> {
    let land = layer_id(h, "Land");
    let mut class = classes(h, map, "Land")[0].clone();
    class.zones_inside = [0; 9];
    class.zones_outside = [1; 8];
    class.leader = leader;
    h.state_mut().doc.set_label_classes(map, land, &[class]);
    h.state_mut().touch();
    h.run_steps(2);
    frame_items(h, 0)
}

/// The paths and the widths of the strokes in `color`.
fn strokes_in(items: &[Item], of: vale_render::Color) -> Vec<(kurbo::BezPath, f64)> {
    (items.iter())
        .filter_map(|i| match i {
            Item::Stroke {
                path, color, width, ..
            } if *color == of => Some((path.clone(), *width)),
            _ => None,
        })
        .collect()
}

/// The paths of the fills in `color`.
fn fills_in(items: &[Item], of: vale_render::Color) -> Vec<kurbo::BezPath> {
    (items.iter())
        .filter_map(|i| match i {
            Item::Fill { path, color, .. } if *color == of => Some(path.clone()),
            _ => None,
        })
        .collect()
}

/// The first and the last point of a path.
fn path_ends(path: &kurbo::BezPath) -> (Point, Point) {
    use kurbo::PathEl;
    let point = |e: &PathEl| match *e {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => p,
        _ => panic!("not a line: {e:?}"),
    };
    let els = path.elements();
    (point(&els[0]), point(&els[els.len() - 1]))
}

/// The number of subpaths of a path.
fn subpaths(path: &kurbo::BezPath) -> usize {
    (path.elements().iter())
        .filter(|e| matches!(e, kurbo::PathEl::MoveTo(_)))
        .count()
}

#[test]
fn a_label_outside_its_polygon_draws_a_leader_line() {
    let (mut h, map) = land_pad(BOARDS[0]);
    let land = layer_id(&h, "Land");
    let mut class = classes(&h, map, "Land")[0].clone();
    class.leader = leader_style();
    h.state_mut()
        .doc
        .set_label_classes(map, land, std::slice::from_ref(&class));
    h.state_mut().touch();
    h.run_steps(2);
    assert!(strokes_in(&frame_items(&mut h, 0), LEADER_COLOR).is_empty());

    let items = leader_frame(&mut h, map, leader_style());
    assert!(label_runs(&mut h, 0).iter().any(|g| g.text == "LAND"));
    let lines = strokes_in(&items, LEADER_COLOR);
    assert!(!lines.is_empty());
    for (path, width) in &lines {
        assert_eq!(*width, vale_style::LeaderStyle::default().width);
        assert_eq!(subpaths(path), 1);
    }
    // The leaders are under the text.
    let first_glyphs = items.iter().position(|i| matches!(i, Item::Glyphs(_)));
    let last_leader = items
        .iter()
        .rposition(|i| matches!(i, Item::Stroke { color, .. } if *color == LEADER_COLOR));
    assert!(last_leader < first_glyphs);
}

#[test]
fn the_leader_line_style_reaches_the_display_list() {
    use vale_style::{LeaderEnd, LeaderStyle};
    let (mut h, map) = land_pad(BOARDS[0]);
    let base = leader_style();
    let items = leader_frame(&mut h, map, base);
    let lines = strokes_in(&items, LEADER_COLOR);
    assert!(!lines.is_empty());
    let (anchor, end) = path_ends(&lines[0].0);
    let length = anchor.distance(end);
    assert!(length > 0.0);
    assert!(fills_in(&items, LEADER_COLOR).is_empty());

    // A hidden line draws nothing, and the label keeps its place.
    let items = leader_frame(
        &mut h,
        map,
        LeaderStyle {
            show: false,
            ..base
        },
    );
    assert!(strokes_in(&items, LEADER_COLOR).is_empty());
    assert!(label_runs(&mut h, 0).iter().any(|g| g.text == "LAND"));

    let items = leader_frame(&mut h, map, LeaderStyle { width: 2.0, ..base });
    assert_eq!(strokes_in(&items, LEADER_COLOR)[0].1, 2.0);

    // The alpha of the leader color reaches the strokes.
    let half = vale_render::Color {
        a: 128,
        ..LEADER_COLOR
    };
    let faded = LeaderStyle {
        color: Some(half),
        ..base
    };
    let items = leader_frame(&mut h, map, faded);
    assert_eq!(strokes_in(&items, half).len(), lines.len());

    let dashed = LeaderStyle {
        dashed: true,
        dash: 1.0,
        dash_gap: 1.0,
        ..base
    };
    let items = leader_frame(&mut h, map, dashed);
    let (path, _) = &strokes_in(&items, LEADER_COLOR)[0];
    assert!(subpaths(path) > 2, "{}", subpaths(path));

    let halo = LeaderStyle {
        halo: true,
        halo_width: 1.5,
        ..base
    };
    let items = leader_frame(&mut h, map, halo);
    let halos = strokes_in(&items, LEADER_HALO);
    assert_eq!(halos.len(), lines.len());
    assert_eq!(halos[0].1, base.width + 3.0);
    assert_eq!(strokes_in(&items, LEADER_COLOR).len(), lines.len());
    // The halo is under the line.
    let at =
        |c| (items.iter()).position(|i| matches!(i, Item::Stroke { color, .. } if *color == c));
    assert!(at(LEADER_HALO) < at(LEADER_COLOR));

    let dot = LeaderStyle {
        end: LeaderEnd::Dot,
        end_size: 4.0,
        ..base
    };
    let items = leader_frame(&mut h, map, dot);
    let marks = fills_in(&items, LEADER_COLOR);
    assert_eq!(marks.len(), lines.len());
    let bounds = kurbo::Shape::bounding_box(&marks[0]);
    assert!(bounds.center().distance(anchor) < 0.01, "{bounds:?}");
    assert!((bounds.width() - 4.0).abs() < 0.05, "{bounds:?}");

    let arrow = LeaderStyle {
        end: LeaderEnd::Arrow,
        end_size: 4.0,
        ..base
    };
    let items = leader_frame(&mut h, map, arrow);
    let marks = fills_in(&items, LEADER_COLOR);
    assert_eq!(marks.len(), lines.len());
    let bounds = kurbo::Shape::bounding_box(&marks[0]);
    assert!(bounds.inflate(0.01, 0.01).contains(anchor), "{bounds:?}");
    // The line stops at the base of the arrow.
    let (from, to) = path_ends(&strokes_in(&items, LEADER_COLOR)[0].0);
    assert!((from.distance(anchor) - 4.0).abs() < 1e-6);
    assert!(to.distance(end) < 1e-6);

    let gaps = LeaderStyle {
        label_gap: 3.0,
        anchor_gap: 2.0,
        ..base
    };
    let items = leader_frame(&mut h, map, gaps);
    let (from, to) = path_ends(&strokes_in(&items, LEADER_COLOR)[0].0);
    assert!((from.distance(anchor) - 2.0).abs() < 1e-6);
    // The default gap at the label is 1 point.
    assert!((from.distance(to) - (length - 4.0)).abs() < 1e-6);
}

/// Opens the Leader line page of the class "Land", with one outside zone on.
fn open_leader_page(h: &mut Pad) {
    open_land_placement(h);
    scroll_to(h, ZONES);
    tap(h, ZONES);
    scroll_to(h, "Outside right");
    tap(h, "Outside right");
    tap(h, "Back to Land");
    scroll_to(h, LEADER_LINE);
    tap(h, LEADER_LINE);
}

/// Taps the color square of the color row `name`, near its top right corner, and closes the
/// picker.
fn pick_color(h: &mut Pad, name: &str) {
    scroll_to(h, name);
    tap(h, name);
    let shade = format!("{name} shade");
    scroll_to(h, &shade);
    let square = control(h, &shade).unwrap();
    tap_with(h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
    scroll_up_to(h, name);
    tap(h, name);
}

/// Opens the color picker of the color row `name`, taps its opacity strip
/// at the fraction `t` of its width, and closes the picker.
fn pick_opacity(h: &mut Pad, name: &str, t: f32) {
    scroll_to(h, name);
    tap(h, name);
    let strip = format!("{name} opacity");
    scroll_to(h, &strip);
    let rect = control(h, &strip).unwrap();
    let at = pos2(rect.left() + t * rect.width(), rect.center().y);
    tap_with(h, at, Input::Finger);
    scroll_up_to(h, name);
    tap(h, name);
}

/// Scrolls to the control `name` and taps it.
fn scroll_tap(h: &mut Pad, name: &str) {
    scroll_up_to(h, name);
    scroll_to(h, name);
    tap(h, name);
}

#[test]
fn the_leader_line_page_edits_the_style() {
    use vale_style::{LeaderEnd, LeaderStyle};
    let (mut h, map) = land_pad(BOARDS[0]);
    let leader = |h: &Pad| classes(h, map, "Land")[0].leader;
    let text = classes(&h, map, "Land")[0].color;
    open_leader_page(&mut h);
    assert_eq!(leader(&h), LeaderStyle::default());

    scroll_tap(&mut h, SHOW_LINE);
    assert!(!leader(&h).show);
    assert!(control(&h, MATCH_TEXT).is_none());
    assert!(control(&h, &value_of(LINE_WIDTH)).is_none());
    scroll_tap(&mut h, SHOW_LINE);
    assert!(leader(&h).show);

    assert!(control(&h, COLOR).is_none());
    scroll_tap(&mut h, MATCH_TEXT);
    assert_eq!(leader(&h).color, Some(text));
    pick_color(&mut h, COLOR);
    let color = leader(&h).color.unwrap();
    assert_ne!((color.r, color.g, color.b), (text.r, text.g, text.b));

    set_number(&mut h, LINE_WIDTH, "2");
    assert!(control(&h, &value_of("Opacity")).is_none());
    pick_opacity(&mut h, COLOR, 0.5);
    scroll_tap(&mut h, DASHED);
    set_number(&mut h, DASH, "4");
    set_number(&mut h, DASH_GAP, "1");
    scroll_tap(&mut h, HALO);
    set_number(&mut h, HALO_WIDTH, "2");
    assert!(control(&h, HALO_COLOR).is_none());
    scroll_tap(&mut h, MATCH_HALO);
    let halo = classes(&h, map, "Land")[0].halo_color;
    assert_eq!(leader(&h).halo_color, Some(halo));
    pick_color(&mut h, HALO_COLOR);
    scroll_tap(&mut h, LeaderEnd::Arrow.name());
    set_number(&mut h, END_SIZE, "5");
    set_number(&mut h, LABEL_GAP, "2");
    set_number(&mut h, ANCHOR_GAP, "1");

    let l = leader(&h);
    assert_eq!(l.width, 2.0);
    assert!(
        (l.color.unwrap().a as i32 - 128).abs() <= 2,
        "{:?}",
        l.color
    );
    assert!(l.dashed);
    assert_eq!((l.dash, l.dash_gap), (4.0, 1.0));
    assert!(l.halo);
    assert_eq!(l.halo_width, 2.0);
    let c = l.halo_color.unwrap();
    assert_ne!((c.r, c.g, c.b), (halo.r, halo.g, halo.b));
    assert_eq!((l.end, l.end_size), (LeaderEnd::Arrow, 5.0));
    assert_eq!((l.label_gap, l.anchor_gap), (2.0, 1.0));

    scroll_tap(&mut h, LeaderEnd::None.name());
    assert_eq!(leader(&h).end, LeaderEnd::None);
    assert!(control(&h, &value_of(END_SIZE)).is_none());
    scroll_tap(&mut h, MATCH_TEXT);
    assert_eq!(leader(&h).color, None);
    scroll_tap(&mut h, MATCH_HALO);
    assert_eq!(leader(&h).halo_color, None);
    assert!(control(&h, HALO_COLOR).is_none());
}

#[test]
fn the_leader_line_page_works_with_a_finger() {
    use vale_style::{LeaderAnchor, LeaderEnd};
    for size in BOARDS {
        let what = format!("{size:?}");
        let (mut h, _) = land_pad(size);
        open_leader_page(&mut h);
        check_controls(&h, &format!("{what}: leader page"));
        for name in [
            MATCH_TEXT,
            DASHED,
            HALO,
            MATCH_HALO,
            LeaderEnd::Arrow.name(),
        ] {
            scroll_tap(&mut h, name);
        }
        scroll_up_to(&mut h, LeaderAnchor::Center.name());
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-leader-line");
        }
        if size == BOARDS[3] {
            screenshot::save(&mut h, "pad-atlas-leader-line-small");
        }
        let names = [
            LeaderAnchor::Center.name().to_owned(),
            LeaderAnchor::Inside.name().to_owned(),
            LeaderAnchor::Outline.name().to_owned(),
            SHOW_LINE.to_owned(),
            MATCH_TEXT.to_owned(),
            COLOR.to_owned(),
            value_of(LINE_WIDTH),
            DASHED.to_owned(),
            value_of(DASH),
            value_of(DASH_GAP),
            HALO.to_owned(),
            MATCH_HALO.to_owned(),
            HALO_COLOR.to_owned(),
            value_of(HALO_WIDTH),
            LeaderEnd::None.name().to_owned(),
            LeaderEnd::Dot.name().to_owned(),
            LeaderEnd::Arrow.name().to_owned(),
            value_of(END_SIZE),
            value_of(LABEL_GAP),
            value_of(ANCHOR_GAP),
        ];
        for name in &names {
            scroll_up_to(&mut h, name);
            scroll_to(&mut h, name);
            assert!(control(&h, name).is_some(), "{what}: {name}");
            check_controls(&h, &format!("{what}: {name}"));
        }
    }
}

#[test]
fn each_tap_on_a_leader_switch_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    let map = frame(&h, 0).map;
    let land = layer_id(&h, "Land");
    let mut area = vale_style::LabelClass::new("Land", GeometryKind::Polygon, 9).unwrap();
    area.zones_outside[4] = 1;
    h.state_mut().doc.set_label_classes(map, land, &[area]);
    h.state_mut().touch();
    h.state_mut().save_waiting().unwrap();
    let leader = |h: &Pad| classes(h, map, "Land")[0].leader;
    tap(&mut h, FRAME_TABS[3]);
    open_land_placement(&mut h);
    scroll_to(&mut h, LEADER_LINE);
    tap(&mut h, LEADER_LINE);
    let start = leader(&h);
    scroll_tap(&mut h, DASHED);
    scroll_tap(&mut h, vale_style::LeaderEnd::Dot.name());
    let l = leader(&h);
    assert!(l.dashed);
    assert_eq!(l.end, vale_style::LeaderEnd::Dot);

    tap(&mut h, "Undo");
    let l = leader(&h);
    assert!(l.dashed);
    assert_eq!(l.end, vale_style::LeaderEnd::None);
    tap(&mut h, "Undo");
    assert_eq!(leader(&h), start);
}

#[test]
fn the_leader_style_survives_a_close_and_an_open() {
    use vale_style::{LeaderEnd, LeaderStyle};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let mut s = state();
    s.doc = doc;
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    let map = frame(&h, 0).map;
    let land = layer_id(&h, "Land");
    let mut area = vale_style::LabelClass::new("Land", GeometryKind::Polygon, 9).unwrap();
    area.zones_outside[4] = 1;
    h.state_mut().doc.set_label_classes(map, land, &[area]);
    h.state_mut().touch();
    h.run_steps(2);
    tap(&mut h, FRAME_TABS[3]);
    open_land_placement(&mut h);
    scroll_to(&mut h, LEADER_LINE);
    tap(&mut h, LEADER_LINE);
    for name in [MATCH_TEXT, DASHED, HALO, LeaderEnd::Arrow.name()] {
        scroll_tap(&mut h, name);
    }
    scroll_up_to(&mut h, &value_of(LINE_WIDTH));
    set_number(&mut h, LINE_WIDTH, "2");
    set_number(&mut h, ANCHOR_GAP, "3");
    let lands = classes(&h, map, "Land");
    let l = lands[0].leader;
    assert_ne!(l, LeaderStyle::default());
    assert!(l.color.is_some() && l.dashed && l.halo);
    assert_eq!((l.end, l.width, l.anchor_gap), (LeaderEnd::Arrow, 2.0, 3.0));
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.label_classes(Some(map), land), lands);
}

/// Saves a picture of a leader line with a halo, a dash, and an arrow, at four times the size.
#[test]
fn manual_a_leader_line_with_a_halo_a_dash_and_an_arrow() {
    use vale_style::{LeaderEnd, LeaderStyle};
    if std::env::var_os("VALE_SCREENSHOTS").is_none() {
        return;
    }
    let (mut h, map) = land_pad(BOARDS[0]);
    let style = LeaderStyle {
        dashed: true,
        halo: true,
        halo_color: Some(vale_render::Color::rgb(255, 255, 255)),
        end: LeaderEnd::Arrow,
        end_size: 4.0,
        ..leader_style()
    };
    let items = leader_frame(&mut h, map, style);
    let (path, _) = &strokes_in(&items, LEADER_COLOR)[0];
    let around = kurbo::Shape::bounding_box(path).inflate(60.0, 30.0);
    let mut list = vale_render::DisplayList::new(around.x1, around.y1);
    list.push(Item::Fill {
        path: kurbo::Shape::to_path(&kurbo::Rect::new(0.0, 0.0, around.x1, around.y1), 0.1),
        color: vale_render::Color::rgb(255, 255, 255),
        rule: vale_render::FillRule::NonZero,
    });
    list.items.extend(items);
    let scale = 4.0;
    let png = vale_render::render_png(&list, scale).unwrap();
    let image = image::load_from_memory(&png).unwrap();
    let crop = image.crop_imm(
        (around.x0.max(0.0) * scale) as u32,
        (around.y0.max(0.0) * scale) as u32,
        (around.width() * scale) as u32,
        (around.height() * scale) as u32,
    );
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/app");
    std::fs::create_dir_all(dir).unwrap();
    crop.save(format!("{dir}/test-leader-line.png")).unwrap();
}

#[test]
fn the_conflict_options_reach_the_labeler() {
    let mut h = labels_pad(BOARDS[0]);
    let map = frame(&h, 0).map;
    let count = |h: &mut Pad| place_texts(h, 0).len();
    tap(&mut h, "Places");
    tap(&mut h, SEGMENTS[2].0);
    set_number(&mut h, BUFFER, "0");
    assert_eq!(classes(&h, map, "Places")[0].buffer, 0.0);
    let tight = count(&mut h);
    set_number(&mut h, BUFFER, "10");
    assert_eq!(classes(&h, map, "Places")[0].buffer, 10.0);
    let loose = count(&mut h);
    assert!(loose < tight, "{loose} {tight}");

    // Each place has the same text.
    tap(&mut h, SEGMENTS[0].0);
    scroll_up_to(&mut h, TEXT);
    type_text(&mut h, TEXT, "'Town'");
    assert_eq!(classes(&h, map, "Places")[0].text, "'Town'");
    tap(&mut h, SEGMENTS[2].0);
    set_number(&mut h, DUPLICATE_DISTANCE, "3000");
    assert_eq!(classes(&h, map, "Places")[0].duplicate_distance, 2000.0);
    let apart = count(&mut h);
    tap(&mut h, REMOVE_DUPLICATES);
    assert!(!classes(&h, map, "Places")[0].remove_duplicates);
    let all = count(&mut h);
    assert!(apart > 0 && apart < all, "{apart} {all}");
}

#[test]
fn each_change_of_a_placement_option_is_one_undo_step() {
    use vale_style::LabelPosition;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    h.state_mut().save_waiting().unwrap();
    let map = frame(&h, 0).map;
    let class = |h: &Pad, layer: &str| classes(h, map, layer)[0].clone();
    tap(&mut h, FRAME_TABS[3]);
    tap(&mut h, "Places");
    tap(&mut h, SEGMENTS[1].0);
    let start = class(&h, "Places");

    // One step of the offset.
    step(&mut h, OFFSET, true);
    let offset = class(&h, "Places").offset;
    assert_ne!(offset, start.offset);

    tap(&mut h, POSITIONS);
    tap(&mut h, "Top left");
    let from = control(&h, &format!("{REORDER} Bottom")).unwrap().center();
    let to = control(&h, &format!("{REORDER} Top right"))
        .unwrap()
        .center();
    drag_steps(&mut h, from, to - vec2(0.0, 10.0), Input::Finger, 24);
    release(&mut h, to - vec2(0.0, 10.0), Input::Finger);
    assert_eq!(
        class(&h, "Places").positions[0].position,
        LabelPosition::Bottom
    );
    tap(&mut h, "Back to Places");
    tap(&mut h, SEGMENTS[2].0);
    tap(&mut h, REMOVE_DUPLICATES);
    assert!(!class(&h, "Places").remove_duplicates);

    tap(&mut h, "Undo");
    let c = class(&h, "Places");
    assert!(c.remove_duplicates);
    assert_eq!(c.positions[0].position, LabelPosition::Bottom);
    tap(&mut h, "Undo");
    let c = class(&h, "Places");
    assert_eq!(c.positions[0].position, LabelPosition::TopRight);
    assert!(!c.positions[1].on);
    tap(&mut h, "Undo");
    let c = class(&h, "Places");
    assert!(c.positions.iter().all(|r| r.on));
    assert_eq!(c.offset, offset);
    tap(&mut h, "Undo");
    assert_eq!(class(&h, "Places"), start);

    // The repeat switch of a line class.
    tap(&mut h, "Back to Labels");
    tap(&mut h, "Rivers");
    tap(&mut h, SEGMENTS[1].0);
    scroll_to(&mut h, REPEAT);
    tap(&mut h, REPEAT);
    assert!(!class(&h, "Rivers").repeat);
    tap(&mut h, "Undo");
    assert!(class(&h, "Rivers").repeat);
}

fn item(h: &Pad, index: usize) -> FurnitureRecord {
    open_page(h).furniture[index].clone()
}

/// The middle of an item of furniture on the screen.
fn item_middle(h: &Pad, index: usize) -> Pos2 {
    screen(h, rect_of(&item(h, index)).center())
}

fn add_item(h: &mut Pad, kind: ItemKind) {
    tap(h, ADD);
    tap(h, kind.name());
}

#[test]
fn the_add_button_adds_each_item_of_furniture() {
    let mut h = pad();
    add_frame(&mut h);
    let main = frame(&h, 0).id;
    for (n, kind) in ItemKind::ALL.into_iter().enumerate() {
        add_item(&mut h, kind);
        assert_eq!(h.state().atlas.add_rect, None, "{kind:?}");
        let page = open_page(&h);
        assert_eq!(page.furniture.len(), n + 1, "{kind:?}");
        let rec = item(&h, n);
        assert_eq!(ItemKind::of(&rec.item), kind);
        assert_eq!(h.state().atlas.selection, AtlasSelection::Item(rec.id));
        // Each item is inside the margin of the page.
        let r = rect_of(&rec);
        assert!(r.x0 >= 10.0 && r.y0 >= 10.0, "{kind:?}: {r:?}");
        assert!(r.x1 <= page.width_mm - 10.0 + 1e-9, "{kind:?}: {r:?}");
        assert!(r.y1 <= page.height_mm - 10.0 + 1e-9, "{kind:?}: {r:?}");
        if kind != ItemKind::Text {
            assert_eq!(rec.item.frame(), Some(main), "{kind:?}");
        }
        assert!(h.state().atlas.has_item_image(rec.id), "{kind:?}");
        // The panel of the item opens.
        assert!(h.state().atlas.panel_rect.is_some(), "{kind:?}");
        assert!(control(&h, DELETE_SELECTION).is_some(), "{kind:?}");
        check_controls(&h, kind.name());
    }
    // Delete in the context strip takes the item away.
    tap(&mut h, DELETE_SELECTION);
    assert_eq!(open_page(&h).furniture.len(), 3);
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
}

#[test]
fn a_tap_opens_the_panel_of_an_item_and_its_controls_work_with_a_finger() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    add_item(&mut h, ItemKind::Text);
    let id = item(&h, 0).id;
    // A tap on the frame selects the frame, and a tap on the item, which
    // is over the frame, selects the item.
    let frame_id = frame(&h, 0).id;
    let at = middle(&h, 0);
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(frame_id));
    let at = item_middle(&h, 0);
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(h.state().atlas.selection, AtlasSelection::Item(id));
    check_controls(&h, "text panel");
    tap(&mut h, ALIGNMENTS[1]);
    let Furniture::Text(text) = &item(&h, 0).item else {
        panic!("not text");
    };
    assert_eq!(text.align, TextAlign::Center);
    tap(&mut h, ITEM_COLOR);
    assert!(h.state().atlas.label_color.is_some());
    check_controls(&h, "color picker");
    tap(&mut h, ITEM_COLOR);
    tap(&mut h, ITEM_STYLE);
    assert_eq!(
        h.state().atlas.item_page,
        Some(ItemPage::Face(TextPart::Text))
    );
    check_controls(&h, "style page");
    tap(&mut h, "Back to Text");
    assert_eq!(h.state().atlas.item_page, None);
    type_text(&mut h, ITEM_TEXT, "Vesperan");
    let Furniture::Text(text) = &item(&h, 0).item else {
        panic!("not text");
    };
    assert_eq!(text.text, "Vesperan");
    h.state_mut().save_waiting().unwrap();

    // A drag of a finger moves the item, and a drag of a handle resizes
    // it. Each drag is one undo step.
    let ppm = f64::from(h.state().atlas.place.unwrap().pts_per_mm as f32);
    let before = rect_of(&item(&h, 0));
    let from = item_middle(&h, 0);
    drag(&mut h, from, from + vec2(40.0, 30.0), Input::Finger);
    let moved = rect_of(&item(&h, 0));
    assert!((moved.x0 - before.x0 - 40.0 / ppm).abs() < 0.1, "{moved:?}");
    assert!((moved.y0 - before.y0 - 30.0 / ppm).abs() < 0.1, "{moved:?}");
    assert!((moved.width() - before.width()).abs() < 1e-9);
    assert_eq!(h.state().atlas.selection, AtlasSelection::Item(id));
    h.state_mut().save_waiting().unwrap();
    check_controls(&h, "handles");
    let corner = handle(&h, Handle::Resize(4));
    drag(&mut h, corner, corner + vec2(20.0, 10.0), Input::Finger);
    let resized = rect_of(&item(&h, 0));
    assert_eq!((resized.x0, resized.y0), (moved.x0, moved.y0));
    assert!((resized.width() - moved.width() - 20.0 / ppm).abs() < 0.1);
    assert!((resized.height() - moved.height() - 10.0 / ppm).abs() < 0.1);
    h.state_mut().save_waiting().unwrap();
    // The frame stays.
    assert_eq!(shape_bbox(&frame(&h, 0).shape).x0, 10.0);

    tap(&mut h, "Undo");
    assert_eq!(rect_of(&item(&h, 0)), moved);
    tap(&mut h, "Undo");
    assert_eq!(rect_of(&item(&h, 0)), before);
}

#[test]
fn a_new_item_links_to_the_largest_frame() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).id, frame(&h, 1).id);
    add_item(&mut h, ItemKind::Legend);
    assert_eq!(item(&h, 0).item.frame(), Some(main));
    // With the inset larger than the main frame, a new item links to the
    // inset.
    let s = h.state_mut();
    let f = s.doc.project.atlas.frame_mut(main).unwrap();
    f.shape = FrameShape::Rectangle([10.0, 10.0, 30.0, 30.0]);
    h.run_steps(2);
    add_item(&mut h, ItemKind::ScaleBar);
    assert_eq!(item(&h, 1).item.frame(), Some(inset));
    // Text has no frame.
    add_item(&mut h, ItemKind::Text);
    assert_eq!(item(&h, 2).item.frame(), None);
}

#[test]
fn the_link_changes_from_the_panel() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).id, frame(&h, 1).id);
    add_item(&mut h, ItemKind::ScaleBar);
    let id = item(&h, 0).id;
    assert_eq!(item(&h, 0).item.frame(), Some(main));
    tap(&mut h, ITEM_MAP_FRAME);
    assert_eq!(h.state().atlas.item_page, Some(ItemPage::Frame));
    check_controls(&h, "frame page");
    let names = atlas::frame_names(h.state());
    assert_eq!(names, [(main, "Map".into()), (inset, "Map 2".into())]);
    tap(&mut h, "Map 2");
    assert_eq!(h.state().atlas.item_page, None);
    assert_eq!(item(&h, 0).item.frame(), Some(inset));

    // The scale follows the scale of the linked frame.
    let text = |h: &mut Pad| {
        let rows = atlas::bar_rows(h.state_mut(), id);
        rows[0].numbers.clone()
    };
    let before = text(&mut h);
    assert!(before.len() >= 2, "{before:?}");
    let content = h.state().atlas.item_rect(id);
    assert!(content.is_some());
    let scale = frame(&h, 1).scale;
    atlas::set_scale(h.state_mut(), inset, scale * 2.0);
    h.run_steps(2);
    let after = text(&mut h);
    assert_ne!(before, after);
    // A change of the main frame does not change it.
    let scale = frame(&h, 0).scale;
    atlas::set_scale(h.state_mut(), main, scale * 3.0);
    assert_eq!(text(&mut h), after);
}

#[test]
fn deleting_a_frame_relinks_its_items() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).id, frame(&h, 1).id);
    add_item(&mut h, ItemKind::Legend);
    add_item(&mut h, ItemKind::NorthArrow);
    let arrow = item(&h, 1).id;
    atlas::set_item_frame(h.state_mut(), arrow, Some(inset));
    add_item(&mut h, ItemKind::Text);

    h.state_mut().atlas.selection = AtlasSelection::Frame(main);
    h.run_steps(2);
    tap(&mut h, DELETE_SELECTION);
    assert_eq!(open_page(&h).frames.len(), 1);
    assert_eq!(item(&h, 0).item.frame(), Some(inset));
    assert_eq!(item(&h, 1).item.frame(), Some(inset));
    assert_eq!(item(&h, 2).item.frame(), None);

    h.state_mut().atlas.selection = AtlasSelection::Frame(inset);
    h.run_steps(2);
    tap(&mut h, DELETE_SELECTION);
    assert_eq!(item(&h, 0).item.frame(), None);
    assert_eq!(item(&h, 1).item.frame(), None);
    assert_eq!(open_page(&h).furniture.len(), 3);
}

#[test]
fn the_legend_panel_renames_and_hides_its_entries() {
    let mut h = pad();
    add_frame(&mut h);
    add_item(&mut h, ItemKind::Legend);
    let id = item(&h, 0).id;
    let legend = |h: &Pad| match &item(h, 0).item {
        Furniture::Legend(l) => l.clone(),
        _ => panic!("not a legend"),
    };
    let legend_rows = |h: &mut Pad| {
        let l = legend(h);
        atlas::legend_rows(h.state_mut(), &l)
    };
    let rows = legend_rows(&mut h);
    assert!(rows.len() >= 2, "{rows:?}");
    let labels: Vec<String> = rows.iter().map(|r| r.label.clone()).collect();
    let names = entry_names(&labels);
    let key = (rows[0].layer, rows[0].rule.clone());

    // The eye hides the entry, and a second tap shows it again.
    tap(&mut h, ENTRIES);
    assert_eq!(
        h.state().atlas.item_page,
        Some(ItemPage::Text(TextPart::Entries))
    );
    scroll_to(&mut h, &eye_name(&names[0], true));
    check_controls(&h, "legend panel");
    tap(&mut h, &eye_name(&names[0], true));
    assert!(legend(&h).entries[&key].hidden);
    let rows = legend_rows(&mut h);
    assert!(rows[0].hidden);
    tap(&mut h, &eye_name(&names[0], false));
    assert!(!legend(&h).entries.contains_key(&key));

    // A tap on the name opens its page, where a new name replaces it.
    tap(&mut h, &names[0]);
    assert_eq!(
        h.state().atlas.item_page,
        Some(ItemPage::Entry(key.0, key.1.clone()))
    );
    check_controls(&h, "entry page");
    type_text(&mut h, ENTRY_NAME, "Roads");
    assert_eq!(legend(&h).entries[&key].name.as_deref(), Some("Roads"));
    let rows = legend_rows(&mut h);
    assert_eq!(rows[0].label, "Roads");
    // An empty name clears the name.
    tap(&mut h, ENTRY_NAME);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.key_press(egui::Key::Backspace);
    h.run_steps(2);
    assert!(!legend(&h).entries.contains_key(&key));
    type_text(&mut h, ENTRY_NAME, "Roads");
    tap(&mut h, "Back to Entries");
    assert_eq!(h.state().atlas.selection, AtlasSelection::Item(id));
    assert_eq!(
        h.state().atlas.item_page,
        Some(ItemPage::Text(TextPart::Entries))
    );

    // Reset entries takes away each change.
    scroll_to(&mut h, RESET_ENTRIES);
    tap(&mut h, RESET_ENTRIES);
    assert!(legend(&h).entries.is_empty());
}

fn bar(h: &Pad, index: usize) -> vale_store::ScaleBarItem {
    match item(h, index).item {
        Furniture::ScaleBar(b) => b,
        other => panic!("not a scale bar: {other:?}"),
    }
}

/// Scrolls to a control and taps it.
fn tap_row(h: &mut Pad, name: &str) {
    scroll_to(h, name);
    if control(h, name).is_none() {
        scroll_up_to(h, name);
    }
    tap(h, name);
}

/// The text of the labels in the panel of the selection. A panel of
/// controls has none.
fn panel_labels(h: &Pad) -> Vec<String> {
    use egui::accesskit::Role;
    use egui_kittest::kittest::NodeT;
    let card = h.state().atlas.panel_rect.unwrap();
    h.query_all_by_role(Role::Label)
        .filter(|n| card.intersects(n.rect()))
        .map(|n| format!("{:?} {:?}", n.accesskit_node().label(), n.value()))
        .collect()
}

/// Checks the controls and the labels of a page of a panel, and saves its
/// picture if `picture` is not empty.
fn check_page(h: &mut Pad, what: &str, picture: &str) {
    check_controls(h, what);
    assert_eq!(panel_labels(h), Vec::<String>::new(), "{what}");
    if !picture.is_empty() {
        screenshot::save(h, picture);
    }
}

#[test]
fn every_page_of_the_scale_bar_panel_works_with_a_finger() {
    use vale_store::{ScaleBarKind, ScaleFit, ScalePosition, ScaleUnit, TickFrequency};
    // A large iPad and a phone.
    for (size, prefix) in [(BOARDS[0], "pad"), ((393.0, 852.0), "phone")] {
        let what = format!("{size:?}");
        let name = |page: &str| format!("{prefix}-atlas-{page}");
        let mut h = pad_with(state(), size);
        add_frame(&mut h);
        add_item(&mut h, ItemKind::ScaleBar);
        check_page(&mut h, &what, &name("scale-bar"));
        assert!(
            h.query_by_label("Measured at the center of the frame.")
                .is_none()
        );
        let back = |h: &mut Pad| tap(h, "Back to Scale bar");

        // The style gallery sets the kind and goes back.
        tap_row(&mut h, BarPage::Style.name());
        assert_eq!(
            h.state().atlas.item_page,
            Some(ItemPage::Bar(BarPage::Style))
        );
        check_page(&mut h, &what, &name("bar-style"));
        assert_eq!(h.state().atlas.bar_preview_count(), ScaleBarKind::ALL.len());
        tap_row(&mut h, ScaleBarKind::DoubleAlternating.label());
        assert_eq!(bar(&h, 0).kind, ScaleBarKind::DoubleAlternating);
        assert_eq!(h.state().atlas.item_page, None);

        // The Units page: a second unit and a custom unit label.
        tap_row(&mut h, BarPage::Units.name());
        check_page(&mut h, &what, &name("bar-units"));
        tap_row(&mut h, BarPick::SecondUnit.name());
        check_page(&mut h, &what, &name("bar-pick"));
        tap_row(&mut h, ScaleUnit::Miles.label());
        assert_eq!(bar(&h, 0).second_unit, Some(ScaleUnit::Miles));
        assert_eq!(
            h.state().atlas.item_page,
            Some(ItemPage::Bar(BarPage::Units))
        );
        tap_row(&mut h, BarPick::UnitLabel.name());
        tap_row(&mut h, "Custom");
        scroll_to(&mut h, item::CUSTOM_LABEL);
        check_page(&mut h, &what, &name("bar-custom"));
        type_text(&mut h, item::CUSTOM_LABEL, "Leagues");
        assert_eq!(bar(&h, 0).unit_label, "Leagues");
        type_text(&mut h, item::SECOND_LABEL, "Versts");
        assert_eq!(bar(&h, 0).second_unit_label, "Versts");
        back(&mut h);

        // The Divisions page.
        tap_row(&mut h, BarPage::Divisions.name());
        check_page(&mut h, &what, &name("bar-divisions"));
        type_value(&mut h, item::DIVISIONS, "20");
        type_value(&mut h, item::SUBDIVISIONS, "10");
        let b = bar(&h, 0);
        assert_eq!((b.divisions, b.subdivisions), (20, 10));
        tap_row(&mut h, item::BEFORE_ZERO);
        assert!(bar(&h, 0).before_zero);
        assert!(control(&h, item::AUTO_VALUE).is_none());
        tap_row(&mut h, BarPick::Fit.name());
        tap_row(&mut h, ScaleFit::AdjustWidth.label());
        assert_eq!(bar(&h, 0).fit, ScaleFit::AdjustWidth);
        // Off, the automatic value gives the values that the bar draws.
        tap_row(&mut h, item::AUTO_VALUE);
        let b = bar(&h, 0);
        let id = item(&h, 0).id;
        let drawn = atlas::bar_rows(h.state_mut(), id);
        assert_eq!(b.division_value, Some(drawn[0].value));
        assert_eq!(b.second_division_value, Some(drawn[1].value));
        scroll_to(&mut h, item::SECOND_VALUE);
        check_page(&mut h, &what, &name("bar-values"));
        back(&mut h);

        // The Bar page: a fill color.
        tap_row(&mut h, BarPage::Bar.name());
        check_page(&mut h, &what, &name("bar-bar"));
        let before = bar(&h, 0).fill1;
        tap_row(&mut h, item::FILL_1);
        assert_eq!(h.state().atlas.item_page, Some(ItemPage::Fill(false)));
        tap_row(&mut h, item::COLOR);
        let shade = format!("{} shade", item::COLOR);
        scroll_to(&mut h, &shade);
        check_controls(&h, &format!("{what}: fill picker"));
        let square = control(&h, &shade).unwrap();
        tap_with(&mut h, square.right_top() + vec2(-4.0, 4.0), Input::Finger);
        assert_ne!(bar(&h, 0).fill1.color, before.color);
        assert_eq!(bar(&h, 0).fill2, vale_store::ScaleBarItem::default().fill2);
        tap(&mut h, "Back to Bar");
        back(&mut h);

        // The Marks page.
        tap_row(&mut h, BarPage::Marks.name());
        check_page(&mut h, &what, &name("bar-marks"));
        tap_row(&mut h, BarPick::MarkFrequency.name());
        tap_row(&mut h, TickFrequency::Ends.label());
        assert_eq!(bar(&h, 0).mark_frequency, TickFrequency::Ends);
        tap(&mut h, item::POSITIONS[1]);
        assert_eq!(bar(&h, 0).mark_position, ScalePosition::Below);
        back(&mut h);

        // The Numbers page.
        tap_row(&mut h, BarPage::Numbers.name());
        check_page(&mut h, &what, &name("bar-numbers"));
        tap(&mut h, item::POSITIONS[2]);
        assert_eq!(bar(&h, 0).number_position, ScalePosition::Center);
        tap_row(&mut h, BarPick::Decimals.name());
        tap_row(&mut h, "2");
        assert_eq!(bar(&h, 0).decimals, Some(2));
        tap_row(&mut h, item::THOUSANDS);
        tap_row(&mut h, item::SHOW_ZERO);
        let b = bar(&h, 0);
        assert!(!b.thousands && !b.zero_label);
        scroll_to(&mut h, item::HALO);
        check_page(&mut h, &what, &name("bar-number-text"));
        back(&mut h);

        // A line has no fills and no bar height.
        tap_row(&mut h, BarPage::Style.name());
        tap_row(&mut h, ScaleBarKind::Line.label());
        tap_row(&mut h, BarPage::Bar.name());
        for name in [item::FILL_1, item::FILL_2, item::FILL] {
            assert!(control(&h, name).is_none(), "{what}: {name}");
        }
        assert!(control(&h, &field(item::BAR_HEIGHT)).is_none(), "{what}");
        assert!(control(&h, item::LINE_COLOR).is_some());
        back(&mut h);
        tap(&mut h, DELETE_SELECTION);
        assert!(open_page(&h).furniture.is_empty());
    }
}

#[test]
fn the_fill_page_sets_a_hatch_with_a_finger() {
    use vale_store::{FillKind, FillPattern};
    for (size, prefix) in [(BOARDS[0], "pad"), ((393.0, 852.0), "phone")] {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.gpkg");
        let mut s = state();
        s.session = Some(Session::open(&path, &mut s.doc).unwrap());
        let mut h = pad_with(s, size);
        add_frame(&mut h);
        add_item(&mut h, ItemKind::ScaleBar);
        h.state_mut().save_waiting().unwrap();
        tap_row(&mut h, BarPage::Bar.name());
        tap_row(&mut h, item::FILL_2);
        assert_eq!(h.state().atlas.item_page, Some(ItemPage::Fill(true)));
        check_page(&mut h, &what, "");
        // A solid fill has no hatch rows.
        for name in [item::BACKGROUND, &field(item::ANGLE), &field(item::SPACING)] {
            assert!(control(&h, name).is_none(), "{what}: {name}");
        }
        // The solid color becomes the background, and the lines take the
        // color of the outline.
        tap_row(&mut h, FillKind::CrossHatch.label());
        let b = bar(&h, 0);
        assert_eq!(b.fill2.kind, FillKind::CrossHatch);
        assert_eq!(b.fill2.background, Some([0xff, 0xff, 0xff, 0xff]));
        assert_eq!(b.fill2.color, b.outline);
        tap_row(&mut h, item::BACKGROUND);
        assert_eq!(bar(&h, 0).fill2.background, None);
        assert_eq!(bar(&h, 0).fill1, vale_store::ScaleBarItem::default().fill1);
        assert!(control(&h, item::LINE_COLOR).is_some(), "{what}");
        scroll_to(&mut h, &field(item::LINE_WIDTH));
        check_page(&mut h, &what, &format!("{prefix}-atlas-bar-fill"));
        type_value(&mut h, item::ANGLE, "180");
        type_value(&mut h, item::SPACING, "5");
        type_value(&mut h, item::LINE_WIDTH, "0.1");
        let fill = bar(&h, 0).fill2;
        assert_eq!(fill.angle, *FillPattern::ANGLES.end());
        assert_eq!(fill.spacing, *FillPattern::SPACINGS.end());
        assert_eq!(fill.line_width, *FillPattern::LINE_WIDTHS.start());
        h.state_mut().save_waiting().unwrap();

        // Each tap is one undo step.
        tap_row(&mut h, FillKind::None.label());
        assert_eq!(bar(&h, 0).fill2.kind, FillKind::None);
        assert!(control(&h, item::LINE_COLOR).is_none(), "{what}");
        tap(&mut h, "Undo");
        assert_eq!(bar(&h, 0).fill2.kind, FillKind::CrossHatch);
        assert_eq!(bar(&h, 0).fill2.angle, *FillPattern::ANGLES.end());
        tap(&mut h, "Undo");
        assert_eq!(bar(&h, 0).fill2.spacing, FillPattern::default().spacing);
        tap(&mut h, "Undo");
        assert_eq!(bar(&h, 0).fill2.background, Some([0xff, 0xff, 0xff, 0xff]));
        tap(&mut h, "Undo");
        assert_eq!(bar(&h, 0).fill2.kind, FillKind::Solid);
        if prefix == "pad" {
            // A large hatched bar, for a person to look at.
            let id = item(&h, 0).id;
            let atlas = &mut h.state_mut().doc.project.atlas;
            let rec = atlas.furniture_mut(id).unwrap();
            rec.rect = [20.0, 150.0, 200.0, 190.0];
            if let Furniture::ScaleBar(b) = &mut rec.item {
                b.bar_height = 8.0;
                b.fill1.kind = FillKind::Hatch;
                item::set_kind(&mut b.fill2, FillKind::CrossHatch, b.outline);
            }
            h.run_steps(3);
            screenshot::save(&mut h, "pad-atlas-hatched-bar");
        }
    }
}

#[test]
fn the_divisions_page_sets_doubling_divisions_with_a_finger() {
    use vale_store::DivisionSpacing;
    for (size, prefix) in [(BOARDS[0], "pad"), ((393.0, 852.0), "phone")] {
        let what = format!("{size:?}");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.gpkg");
        let mut s = state();
        s.session = Some(Session::open(&path, &mut s.doc).unwrap());
        let mut h = pad_with(s, size);
        add_frame(&mut h);
        add_item(&mut h, ItemKind::ScaleBar);
        h.state_mut().save_waiting().unwrap();
        tap_row(&mut h, BarPage::Divisions.name());
        tap_row(&mut h, DivisionSpacing::Doubling.label());
        assert_eq!(bar(&h, 0).spacing, DivisionSpacing::Doubling, "{what}");
        check_page(&mut h, &what, &format!("{prefix}-atlas-bar-doubling"));
        // The last of 4 divisions ends at 8 division values.
        let id = item(&h, 0).id;
        let row = atlas::bar_rows(h.state_mut(), id).remove(0);
        let (text, x) = row.numbers.last().unwrap().clone();
        let last: f64 = text.replace(',', "").parse().unwrap();
        assert_eq!(last, 8.0 * row.value, "{what}");
        assert!((x - row.end).abs() < 1e-6, "{what}: {row:?}");
        assert_eq!(row.divisions, 4);
        tap(&mut h, "Undo");
        assert_eq!(bar(&h, 0).spacing, DivisionSpacing::Equal, "{what}");
    }
}

#[test]
fn each_change_of_a_scale_bar_is_one_undo_step() {
    use vale_store::ScaleBarKind;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    add_item(&mut h, ItemKind::ScaleBar);
    h.state_mut().save_waiting().unwrap();
    tap(&mut h, BarPage::Divisions.name());
    tap(&mut h, item::BEFORE_ZERO);
    tap(&mut h, "Back to Scale bar");
    tap(&mut h, BarPage::Style.name());
    tap(&mut h, ScaleBarKind::Hollow.label());
    tap(&mut h, BarPage::Numbers.name());
    scroll_to(&mut h, item::HALO);
    tap(&mut h, item::HALO);
    let b = bar(&h, 0);
    assert!(b.before_zero && b.number_style.halo);
    assert_eq!(b.kind, ScaleBarKind::Hollow);

    tap(&mut h, "Undo");
    assert!(!bar(&h, 0).number_style.halo);
    assert_eq!(bar(&h, 0).kind, ScaleBarKind::Hollow);
    tap(&mut h, "Undo");
    assert_eq!(bar(&h, 0).kind, ScaleBarKind::Alternating);
    assert!(bar(&h, 0).before_zero);
    tap(&mut h, "Undo");
    assert!(!bar(&h, 0).before_zero);
}

#[test]
fn the_divisions_stepper_adds_one_division_per_tap_up_to_20() {
    let mut h = pad_with(state(), BOARDS[0]);
    add_frame(&mut h);
    add_item(&mut h, ItemKind::ScaleBar);
    tap_row(&mut h, BarPage::Divisions.name());
    let n = bar(&h, 0).divisions;
    assert!(n < 20, "{n}");
    step(&mut h, item::DIVISIONS, true);
    assert_eq!(bar(&h, 0).divisions, n + 1);
    for i in n + 2..=20 {
        step(&mut h, item::DIVISIONS, true);
        assert_eq!(bar(&h, 0).divisions, i);
    }
    // At 20, the plus button is off.
    assert!(control(&h, "Increase Divisions").is_none());
}

#[test]
fn a_new_scale_bar_takes_the_units_of_the_world() {
    use vale_store::{ScaleUnit, Units};
    for (units, unit) in [
        (Units::Metric, ScaleUnit::Kilometers),
        (Units::Imperial, ScaleUnit::Miles),
    ] {
        let mut s = state();
        s.doc.project.world.units = units;
        let mut h = pad_with(s, BOARDS[0]);
        add_frame(&mut h);
        add_item(&mut h, ItemKind::ScaleBar);
        assert_eq!(bar(&h, 0).unit, unit, "{units:?}");
    }
}

/// Sets the case of the Text section on the page, with a tap on the Case
/// row and a tap on the case. The page comes back.
fn set_case(h: &mut Pad, case: vale_store::TextCase, part: TextPart) {
    let page = h.state().atlas.item_page.clone();
    tap_row(h, item::CASE);
    assert_eq!(h.state().atlas.item_page, Some(ItemPage::Case(part)));
    check_controls(h, "case page");
    tap_row(h, case.name());
    assert_eq!(h.state().atlas.item_page, page);
}

/// A family of the device that is not the default family.
fn font_family(h: &Pad) -> String {
    let families = &h.state().font_families;
    let other = families
        .iter()
        .find(|f| *f != vale_app::fonts::DEFAULT_FAMILY);
    other.cloned().unwrap_or_else(|| "Noto Serif".into())
}

/// A tap on the Font row asks for the font picker of `part`, and the picker
/// sets the family of `font_family`.
fn picks_font(h: &mut Pad, part: TextPart) {
    use vale_app::ui::FontTarget;
    h.state_mut().actions.clear();
    tap_row(h, item::FONT);
    let id = h.state().atlas.item().unwrap();
    let target = FontTarget::Item(id, part);
    assert!(
        h.state().actions.contains(&Action::PickFont(target)),
        "{part:?}"
    );
    h.state_mut().actions.clear();
    let family = font_family(h);
    h.state_mut().run_action(Action::SetFont(target, family));
    h.run_steps(2);
}

#[test]
fn each_text_part_edits_its_own_style() {
    use vale_store::{FurnitureText, TextCase};
    let mut h = pad();
    add_frame(&mut h);
    let style = |h: &Pad, index: usize, part: TextPart| -> FurnitureText {
        part.of(&item(h, index).item).unwrap().clone()
    };
    let serif = Some(font_family(&h));

    // A text item.
    add_item(&mut h, ItemKind::Text);
    check_page(&mut h, "text panel", "pad-atlas-text-item");
    set_case(&mut h, TextCase::Upper, TextPart::Text);
    tap_row(&mut h, item::HALO);
    scroll_to(&mut h, item::HALO_WIDTH);
    check_page(&mut h, "text halo", "pad-atlas-text-halo");
    picks_font(&mut h, TextPart::Text);
    let s = style(&h, 0, TextPart::Text);
    assert_eq!(
        (s.case, s.halo, s.font),
        (TextCase::Upper, true, serif.clone())
    );

    // A legend: the title and the entries.
    add_item(&mut h, ItemKind::Legend);
    check_page(&mut h, "legend panel", "pad-atlas-legend");
    tap(&mut h, item::TITLE);
    assert_eq!(
        h.state().atlas.item_page,
        Some(ItemPage::Text(TextPart::Title))
    );
    check_page(&mut h, "title page", "pad-atlas-legend-title");
    type_text(&mut h, item::TITLE, "Key");
    set_case(&mut h, TextCase::Upper, TextPart::Title);
    tap_row(&mut h, item::HALO);
    picks_font(&mut h, TextPart::Title);
    tap(&mut h, "Back to Legend");
    tap(&mut h, item::ENTRIES);
    set_case(&mut h, TextCase::Lower, TextPart::Entries);
    picks_font(&mut h, TextPart::Entries);
    let Furniture::Legend(l) = item(&h, 1).item else {
        panic!("not a legend");
    };
    assert_eq!(l.title, "Key");
    assert_eq!(
        (l.title_style.case, l.title_style.halo),
        (TextCase::Upper, true)
    );
    assert_eq!(
        (l.entry_style.case, l.entry_style.halo),
        (TextCase::Lower, false)
    );
    assert_eq!(
        (l.title_style.font, l.entry_style.font),
        (serif.clone(), serif.clone())
    );

    // A scale bar: the numbers and the units.
    add_item(&mut h, ItemKind::ScaleBar);
    tap(&mut h, BarPage::Numbers.name());
    set_case(&mut h, TextCase::Upper, TextPart::Numbers);
    tap_row(&mut h, item::HALO);
    picks_font(&mut h, TextPart::Numbers);
    tap(&mut h, "Back to Scale bar");
    tap(&mut h, BarPage::Units.name());
    set_case(&mut h, TextCase::Title, TextPart::Units);
    let b = bar(&h, 2);
    assert_eq!(
        (b.number_style.case, b.number_style.halo),
        (TextCase::Upper, true)
    );
    assert_eq!(
        (b.unit_style.case, b.unit_style.halo),
        (TextCase::Title, false)
    );
    assert_eq!(
        (b.number_style.font, b.unit_style.font),
        (serif.clone(), None)
    );

    // A north arrow: the letter.
    add_item(&mut h, ItemKind::NorthArrow);
    check_page(&mut h, "north arrow panel", "pad-atlas-north-arrow");
    set_case(&mut h, TextCase::Lower, TextPart::Letter);
    tap_row(&mut h, item::HALO);
    picks_font(&mut h, TextPart::Letter);
    let s = style(&h, 3, TextPart::Letter);
    assert_eq!((s.case, s.halo, s.font), (TextCase::Lower, true, serif));
    // The other items keep their styles.
    assert_eq!(style(&h, 0, TextPart::Text).case, TextCase::Upper);
}

#[test]
#[ignore = "gpu"]
fn manual_the_pad_atlas_renders_furniture() {
    let _gpu = one_gpu_test();
    let prepare = |s: &mut AppState| {
        atlas::add_frame(s);
        atlas::add_frame(s);
        atlas::add_item(s, ItemKind::Text);
        atlas::add_item(s, ItemKind::ScaleBar);
        atlas::add_item(s, ItemKind::NorthArrow);
        atlas::add_item(s, ItemKind::Legend);
        Ok(())
    };
    std::fs::create_dir_all("../../target/app").unwrap();
    let (img, _) = vale_app::headless::ui_png_with(state(), (1194.0, 834.0), 1.0, prepare)
        .unwrap_or_else(|e| panic!("{e}"));
    img.save("../../target/app/pad-atlas-furniture.png")
        .unwrap();
    // The panel of a text item, and the sheet of a north arrow on a phone.
    for (kind, size, name) in [
        (ItemKind::Text, (834.0, 1194.0), "text"),
        (ItemKind::NorthArrow, (393.0, 852.0), "phone"),
    ] {
        let prepare = |s: &mut AppState| {
            atlas::add_frame(s);
            atlas::add_item(s, kind);
            Ok(())
        };
        let (img, _) = vale_app::headless::ui_png_with(state(), size, 1.0, prepare)
            .unwrap_or_else(|e| panic!("{e}"));
        img.save(format!("../../target/app/pad-atlas-{name}.png"))
            .unwrap();
    }
}

/// The page center, the page size, and the points per millimeter.
fn page_metrics(h: &Pad) -> (Point, f64, f64, f64) {
    let page = open_page(h);
    let (w, hh) = (page.width_mm, page.height_mm);
    let ppm = h.state().atlas.place.unwrap().pts_per_mm;
    (Point::new(w / 2.0, hh / 2.0), w, hh, ppm)
}

#[test]
fn a_moved_frame_snaps_to_the_page_center_and_shows_a_guide() {
    let mut h = inset_pad(BOARDS[0]);
    for input in INPUTS {
        let (center, _, _, ppm) = page_metrics(&h);
        let before = shape_bbox(&frame(&h, 1).shape);
        // The left edge goes to 3 points right of the page center.
        let d = (center.x - before.x0) * ppm + 3.0;
        let from = middle(&h, 1);
        let to = from + vec2(d as f32, -20.0);
        drag_steps(&mut h, from, to, input, 6);
        assert!(!h.state().atlas.guides.is_empty(), "{input:?}");
        release(&mut h, to, input);
        assert!(h.state().atlas.guides.is_empty(), "{input:?}");
        let after = shape_bbox(&frame(&h, 1).shape);
        assert!((after.x0 - center.x).abs() < 1e-9, "{input:?}: {after:?}");
        // Put the frame back for the next input.
        h.state_mut().undo();
        h.run_steps(2);
    }
}

#[test]
fn a_moved_frame_snaps_to_the_edges_and_the_center_lines_of_another_frame() {
    let mut h = inset_pad(BOARDS[0]);
    let (_, _, _, ppm) = page_metrics(&h);
    let main = shape_bbox(&frame(&h, 0).shape);
    let inset = shape_bbox(&frame(&h, 1).shape);
    // The top edge of the inset goes 2 points below the middle of the main
    // frame, and the right edge 3 points past the right edge of the main
    // frame.
    let dy = (main.center().y - inset.y0) * ppm + 2.0;
    let from = middle(&h, 1);
    drag(&mut h, from, from + vec2(3.0, dy as f32), Input::Finger);
    let after = shape_bbox(&frame(&h, 1).shape);
    assert!((after.y0 - main.center().y).abs() < 1e-9, "{after:?}");
    assert!((after.x1 - main.x1).abs() < 1e-9, "{after:?}");
}

#[test]
fn a_resized_frame_snaps_the_edge_of_its_handle() {
    let mut h = inset_pad(BOARDS[0]);
    let (center, _, _, ppm) = page_metrics(&h);
    let inset = shape_bbox(&frame(&h, 1).shape);
    let left = handle(&h, Handle::Resize(7));
    let d = (center.x - inset.x0) * ppm - 4.0;
    drag(&mut h, left, left + vec2(d as f32, 0.0), Input::Pen);
    let after = shape_bbox(&frame(&h, 1).shape);
    assert!((after.x0 - center.x).abs() < 1e-9, "{after:?}");
    assert_eq!(
        (after.y0, after.x1, after.y1),
        (inset.y0, inset.x1, inset.y1)
    );
}

#[test]
fn a_scaled_polygon_snaps_the_edge_of_its_handle() {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, POLYGON);
    let (center, _, _, ppm) = page_metrics(&h);
    let inset = shape_bbox(&frame(&h, 1).shape);
    let left = handle(&h, Handle::Resize(7));
    let d = (center.x - inset.x0) * ppm - 4.0;
    drag(&mut h, left, left + vec2(d as f32, 0.0), Input::Pen);
    assert!(!matches!(frame(&h, 1).shape, FrameShape::Rectangle(_)));
    let after = shape_bbox(&frame(&h, 1).shape);
    assert!((after.x0 - center.x).abs() < 1e-9, "{after:?}");
    assert_eq!(
        (after.y0, after.x1, after.y1),
        (inset.y0, inset.x1, inset.y1)
    );
}

/// The middle of the edge of the outer ring of the inset from vertex `i` to
/// the next vertex, on the screen.
fn edge_middle(h: &Pad, i: usize) -> Pos2 {
    let ring = &frame(h, 1).shape.rings()[0];
    let ([ax, ay], [bx, by]) = (ring[i], ring[(i + 1) % ring.len()]);
    screen(h, Point::new((ax + bx) / 2.0, (ay + by) / 2.0))
}

fn ring(h: &Pad, index: usize) -> Vec<[f64; 2]> {
    match &frame(h, index).shape {
        FrameShape::Polygon(rings) => rings[0].clone(),
        FrameShape::Rectangle(_) => panic!("not a polygon"),
    }
}

#[test]
fn a_vertex_snaps_to_a_vertex_of_another_frame_and_to_the_page_center() {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, POLYGON);
    tap(&mut h, VERTEX);
    let (center, _, _, _) = page_metrics(&h);
    let main = shape_bbox(&frame(&h, 0).shape);
    // The top-right corner of the main frame.
    let from = handle(&h, Handle::Vertex((0, 0)));
    let to = screen(&h, Point::new(main.x1, main.y0)) + vec2(-4.0, 5.0);
    drag_steps(&mut h, from, to, Input::Finger, 6);
    assert!(!h.state().atlas.guides.is_empty());
    screenshot::save(&mut h, "pad-atlas-snap-vertex");
    let mut phone = pad_with(state(), (393.0, 852.0));
    add_frame(&mut phone);
    screenshot::save(&mut phone, "phone-atlas-frame-card");
    release(&mut h, to, Input::Finger);
    assert_eq!(ring(&h, 1)[0], [main.x1, main.y0]);

    let from = handle(&h, Handle::Vertex((0, 0)));
    let to = screen(&h, center) + vec2(6.0, -5.0);
    drag(&mut h, from, to, Input::Mouse);
    let [x, y] = ring(&h, 1)[0];
    assert!((x - center.x).abs() < 1e-9 && (y - center.y).abs() < 1e-9);
}

#[test]
fn shift_locks_the_edge_of_a_vertex_to_horizontal() {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, POLYGON);
    tap(&mut h, VERTEX);
    h.state_mut().atlas.snap = false;
    let start = ring(&h, 1);
    let from = handle(&h, Handle::Vertex((0, 0)));
    h.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    drag(&mut h, from, from + vec2(-60.0, 7.0), Input::Mouse);
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    // The edge to the next vertex is horizontal.
    let after = ring(&h, 1);
    assert_eq!(after[0][1], start[1][1]);
    assert!(after[0][0] < start[0][0] - 5.0);

    // With no Shift, the same drag does not lock.
    let from = handle(&h, Handle::Vertex((0, 0)));
    drag(&mut h, from, from + vec2(0.0, 7.0), Input::Mouse);
    assert!(ring(&h, 1)[0][1] > start[1][1] + 1.0);
}

#[test]
fn a_second_finger_locks_the_edge_of_a_vertex_to_vertical() {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, POLYGON);
    tap(&mut h, VERTEX);
    h.state_mut().atlas.snap = false;
    let start = ring(&h, 1);
    let view = h.state().atlas.view;
    let from = handle(&h, Handle::Vertex((0, 0)));
    let other = middle(&h, 0) - vec2(200.0, 0.0);
    touch(&mut h, 1, egui::TouchPhase::Start, from, None);
    h.step();
    for i in 1..=3 {
        let at = from + vec2(-2.0, -10.0) * i as f32;
        touch(&mut h, 1, egui::TouchPhase::Move, at, None);
        h.step();
    }
    // The second finger holds on the canvas while the first finger drags.
    // The pinch does not start, and the fingers are no tap that undoes.
    touch(&mut h, 2, egui::TouchPhase::Start, other, None);
    h.step();
    let end = from + vec2(-5.0, -60.0);
    for i in 1..=4 {
        let at = (from + vec2(-6.0, -30.0)).lerp(end, i as f32 / 4.0);
        touch(&mut h, 1, egui::TouchPhase::Move, at, None);
        h.step();
    }
    touch(&mut h, 1, egui::TouchPhase::End, end, None);
    h.step();
    touch(&mut h, 2, egui::TouchPhase::End, other, None);
    h.run_steps(4);
    // The edge to the vertex before is vertical, the page did not move, and
    // nothing was undone.
    let after = ring(&h, 1);
    assert_eq!(after[0][0], start[3][0]);
    assert!(after[0][1] < start[0][1] - 5.0);
    assert_eq!(h.state().atlas.view, view);
}

/// Zooms the page about the inset frame, so the inset stays on the screen.
fn zoom_on_inset(h: &mut Pad, zoom: f64) {
    h.state_mut().atlas.view = vale_app::ui::atlas::PageView {
        zoom,
        offset: egui::Vec2::ZERO,
    };
    h.run_steps(2);
    let screen_middle = h.state().pad.rects.unwrap().screen.center();
    let off = screen_middle - middle(h, 1);
    h.state_mut().atlas.view.offset += off;
    h.run_steps(2);
}

#[test]
fn the_snap_distance_is_the_same_on_the_screen_at_each_zoom() {
    for zoom in [1.0, 3.0] {
        for (pts, snaps) in [(10.0, true), (14.0, false)] {
            let mut h = inset_pad(BOARDS[0]);
            zoom_on_inset(&mut h, zoom);
            let main = shape_bbox(&frame(&h, 0).shape);
            // The right edge of the inset is on the right edge of the main
            // frame. A drag to the left moves it off.
            let from = middle(&h, 1);
            drag(&mut h, from, from - vec2(pts, 0.0), Input::Finger);
            let after = shape_bbox(&frame(&h, 1).shape);
            let on = (after.x1 - main.x1).abs() < 1e-9;
            assert_eq!(on, snaps, "zoom {zoom}, {pts} pt: {after:?}");
        }
    }
}

#[test]
fn the_snap_switch_turns_snapping_off_with_a_finger() {
    let mut h = inset_pad(BOARDS[0]);
    check_controls(&h, "snap switch");
    assert!(h.state().atlas.snap);
    tap(&mut h, SNAP_TO_LAYOUT);
    assert!(!h.state().atlas.snap);
    let main = shape_bbox(&frame(&h, 0).shape);
    let from = middle(&h, 1);
    drag_steps(&mut h, from, from - vec2(20.0, 0.0), Input::Finger, 6);
    assert!(h.state().atlas.guides.is_empty());
    release(&mut h, from - vec2(20.0, 0.0), Input::Finger);
    let after = shape_bbox(&frame(&h, 1).shape);
    assert!(after.x1 < main.x1 - 1.0, "{after:?}");
    tap(&mut h, SNAP_TO_LAYOUT);
    assert!(h.state().atlas.snap);

    // The switch shows only while the Frame tool or the Vertex tool edits a
    // frame.
    tap(&mut h, "Move");
    assert!(control(&h, SNAP_TO_LAYOUT).is_none());
    tap(&mut h, VERTEX);
    assert!(control(&h, SNAP_TO_LAYOUT).is_some());
    tap(&mut h, "Frame tool");
    assert!(control(&h, SNAP_TO_LAYOUT).is_some());
    h.state_mut().atlas.selection = AtlasSelection::Page;
    h.run_steps(2);
    assert!(control(&h, SNAP_TO_LAYOUT).is_none());
}

#[test]
fn frames_snap_to_the_layout_grid_of_the_page_panel() {
    let mut h = inset_pad(BOARDS[0]);
    let id = frame(&h, 1).id;
    h.state_mut().atlas.selection = AtlasSelection::Page;
    h.run_steps(2);
    scroll_to(&mut h, LAYOUT_GRID);
    assert!(control(&h, GRID_SPACING).is_none());
    tap(&mut h, LAYOUT_GRID);
    scroll_to(&mut h, GRID_COLOR);
    assert!(h.state().doc.project.atlas.grid.on);
    for name in [GRID_SPACING, GRID_COLOR] {
        assert!(control(&h, name).is_some(), "{name}");
    }
    check_controls(&h, "page panel with the grid");
    h.state_mut().doc.project.atlas.grid.spacing_mm = 7.0;
    h.state_mut().atlas.selection = AtlasSelection::Frame(id);
    h.run_steps(2);

    // The left edge of the inset goes 2 points past a grid line that is far
    // from the other targets.
    let (_, _, _, ppm) = page_metrics(&h);
    let inset = shape_bbox(&frame(&h, 1).shape);
    let line = (inset.x0 / 7.0).floor() * 7.0 - 14.0;
    let d = (line - inset.x0) * ppm + 2.0;
    let from = middle(&h, 1);
    let to = from + vec2(d as f32, -60.0);
    drag_steps(&mut h, from, to, Input::Finger, 6);
    screenshot::save(&mut h, "pad-atlas-layout-grid");
    release(&mut h, to, Input::Finger);
    let after = shape_bbox(&frame(&h, 1).shape);
    assert!((after.x0 - line).abs() < 1e-9, "{line}: {after:?}");

    // With the grid off, the grid lines are no targets.
    h.state_mut().doc.project.atlas.grid.on = false;
    h.run_steps(2);
    let from = middle(&h, 1);
    drag(&mut h, from, from + vec2(-20.0, 0.0), Input::Finger);
    let moved = shape_bbox(&frame(&h, 1).shape);
    let want = line - 20.0 / ppm;
    assert!((moved.x0 - want).abs() < 0.1, "{want}: {moved:?}");
}

#[test]
fn the_grid_button_of_the_top_row_shows_the_layout_grid() {
    for size in BOARDS.into_iter().chain([(393.0, 852.0)]) {
        let mut h = pad_with(state(), size);
        check_controls(&h, "top row");
        // The buttons of the top row do not overlap.
        let controls = h.state().pad.controls.clone();
        let top: Vec<_> = controls.iter().filter(|c| c.rect.top() < 90.0).collect();
        for (i, a) in top.iter().enumerate() {
            for b in &top[i + 1..] {
                let overlap = a.rect.intersect(b.rect);
                assert!(
                    overlap.width() <= 0.0 || overlap.height() <= 0.0,
                    "{size:?}: {} and {}",
                    a.name,
                    b.name
                );
            }
        }
        tap(&mut h, SHOW_GRID);
        assert!(h.state().doc.project.atlas.grid.on, "{size:?}");
        tap(&mut h, SHOW_GRID);
        assert!(!h.state().doc.project.atlas.grid.on, "{size:?}");
    }
}

#[test]
fn a_vertex_lines_up_with_another_vertex_of_its_frame() {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, POLYGON);
    tap(&mut h, VERTEX);
    // A tap on the middle of the top edge adds a vertex, and a drag moves
    // it below the edge.
    let edge = edge_middle(&h, 0);
    tap_with(&mut h, edge, Input::Finger);
    let new = handle(&h, Handle::Vertex((0, 1)));
    drag(&mut h, new, new + vec2(0.0, 37.0), Input::Finger);
    let [_, y] = ring(&h, 1)[1];
    // The first vertex goes to 3 points below the new vertex, far from
    // the other targets.
    let from = handle(&h, Handle::Vertex((0, 0)));
    let to = from + vec2(-30.0, 40.0);
    drag_steps(&mut h, from, to, Input::Finger, 6);
    assert!(!h.state().atlas.guides.is_empty());
    screenshot::save(&mut h, "pad-atlas-vertex-alignment");
    release(&mut h, to, Input::Finger);
    assert_eq!(ring(&h, 1)[0][1], y);
}

/// A page with a main frame and a polygon inset, with the Vertex tool and
/// no snapping.
fn vertex_pad() -> Pad {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, POLYGON);
    tap(&mut h, VERTEX);
    h.state_mut().atlas.snap = false;
    h.run_steps(2);
    h
}

fn selected_vertices(h: &Pad) -> Vec<(usize, usize)> {
    let mut v = h.state().atlas.vertices.clone();
    v.sort_unstable();
    v
}

fn is_disabled(h: &Pad, name: &str) -> bool {
    use egui_kittest::kittest::NodeT;
    h.get_by_label(name).accesskit_node().is_disabled()
}

#[test]
fn a_vertex_edit_turns_a_rectangle_into_a_polygon() {
    for edit in ["move", "add", "delete"] {
        let mut h = inset_pad(BOARDS[0]);
        tap(&mut h, VERTEX);
        h.state_mut().atlas.snap = false;
        h.run_steps(2);
        let handles = &h.state().atlas.handles;
        assert_eq!(handles.len(), 4, "{edit}");
        assert!(handles.iter().all(|(h, _)| matches!(h, Handle::Vertex(_))));
        // A tap on a corner selects it, and the frame stays a rectangle.
        let corner = handle(&h, Handle::Vertex((0, 2)));
        tap_with(&mut h, corner, Input::Pen);
        assert_eq!(selected_vertices(&h), [(0, 2)], "{edit}");
        let before = frame(&h, 1).shape.rings()[0].clone();
        assert!(matches!(frame(&h, 1).shape, FrameShape::Rectangle(_)));
        match edit {
            "move" => drag(&mut h, corner, corner + vec2(-30.0, -20.0), Input::Pen),
            "add" => {
                let at = edge_middle(&h, 0);
                tap_with(&mut h, at, Input::Pen)
            }
            _ => tap(&mut h, DELETE_VERTEX),
        }
        assert!(
            matches!(frame(&h, 1).shape, FrameShape::Polygon(_)),
            "{edit}"
        );
        let after = ring(&h, 1);
        match edit {
            "move" => {
                assert_eq!(after.len(), 4);
                assert!(after[2][0] < before[2][0] - 1.0 && after[2][1] < before[2][1] - 1.0);
                assert_eq!(after[0], before[0]);
            }
            "add" => assert_eq!(after.len(), 5),
            _ => assert_eq!(after, [before[0], before[1], before[3]]),
        }
    }
}

#[test]
fn taps_select_a_vertex_and_clear_the_vertices_and_then_the_frame() {
    let mut h = vertex_pad();
    let inset = frame(&h, 1).id;
    let main = shape_bbox(&frame(&h, 0).shape);
    let empty = screen(&h, Point::new(main.x0 / 2.0, main.y0 / 2.0));
    for input in INPUTS {
        let at = handle(&h, Handle::Vertex((0, 1)));
        tap_with(&mut h, at, input);
        assert_eq!(selected_vertices(&h), [(0, 1)], "{input:?}");
        let at = handle(&h, Handle::Vertex((0, 3)));
        tap_with(&mut h, at, input);
        assert_eq!(selected_vertices(&h), [(0, 3)], "{input:?}");
        tap_with(&mut h, empty, input);
        assert!(selected_vertices(&h).is_empty(), "{input:?}");
        assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(inset));
        tap_with(&mut h, empty, input);
        assert_eq!(h.state().atlas.selection, AtlasSelection::None, "{input:?}");
        let at = middle(&h, 1);
        tap_with(&mut h, at, input);
        assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(inset));
    }
    // A tap on another frame selects it.
    let other = screen(&h, Point::new(main.x0 + 2.0, main.y0 + 2.0));
    tap_with(&mut h, other, Input::Finger);
    let id = frame(&h, 0).id;
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(id));

    // Deselect clears the vertices, and then the frame.
    let at = middle(&h, 1);
    tap_with(&mut h, at, Input::Finger);
    let at = handle(&h, Handle::Vertex((0, 0)));
    tap_with(&mut h, at, Input::Finger);
    tap(&mut h, DESELECT);
    assert!(selected_vertices(&h).is_empty());
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(inset));
    tap(&mut h, DESELECT);
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);

    // Another tool clears the vertices and keeps the frame.
    let at = middle(&h, 1);
    tap_with(&mut h, at, Input::Finger);
    let at = handle(&h, Handle::Vertex((0, 0)));
    tap_with(&mut h, at, Input::Finger);
    tap(&mut h, "Frame tool");
    assert!(selected_vertices(&h).is_empty());
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(inset));
}

#[test]
fn a_pen_box_selects_frame_vertices() {
    let mut h = vertex_pad();
    let before = frame(&h, 1).shape;
    for input in [Input::Pen, Input::Mouse] {
        h.state_mut().atlas.vertices.clear();
        // From the body of the frame past its top-left corner.
        let corner = handle(&h, Handle::Vertex((0, 0)));
        let at = middle(&h, 1);
        drag(&mut h, at, corner - vec2(30.0, 30.0), input);
        assert_eq!(selected_vertices(&h), [(0, 0)], "{input:?}");
        // From above the left of the top edge to below the right.
        let right = handle(&h, Handle::Vertex((0, 1)));
        drag(
            &mut h,
            corner - vec2(30.0, 30.0),
            right + vec2(30.0, 30.0),
            input,
        );
        assert_eq!(selected_vertices(&h), [(0, 0), (0, 1)], "{input:?}");
        assert_eq!(frame(&h, 1).shape, before, "{input:?}");
    }
    screenshot::save(&mut h, "pad-atlas-vertex");
}

#[test]
fn a_finger_drag_away_from_a_handle_pans_the_page() {
    let mut h = vertex_pad();
    let before = frame(&h, 1).shape;
    let main = shape_bbox(&frame(&h, 0).shape);
    let empty = screen(&h, Point::new(main.x0 / 2.0, main.y0 / 2.0));
    for from in [middle(&h, 1), empty] {
        let view = h.state().atlas.view.offset;
        drag(&mut h, from, from + vec2(40.0, 25.0), Input::Finger);
        let moved = h.state().atlas.view.offset - view;
        assert!((moved - vec2(40.0, 25.0)).length() < 0.01, "{moved:?}");
        assert_eq!(frame(&h, 1).shape, before);
        assert!(selected_vertices(&h).is_empty());
    }
}

#[test]
fn a_drag_of_a_selected_vertex_moves_all_selected() {
    let mut h = vertex_pad();
    let ppm = h.state().atlas.place.unwrap().pts_per_mm;
    let corner = handle(&h, Handle::Vertex((0, 0)));
    let right = handle(&h, Handle::Vertex((0, 1)));
    drag(
        &mut h,
        corner - vec2(30.0, 30.0),
        right + vec2(30.0, 30.0),
        Input::Pen,
    );
    assert_eq!(selected_vertices(&h), [(0, 0), (0, 1)]);
    let before = ring(&h, 1);
    drag(&mut h, right, right + vec2(-20.0, 15.0), Input::Finger);
    let after = ring(&h, 1);
    let d = |k: usize| [after[k][0] - before[k][0], after[k][1] - before[k][1]];
    let want = [-20.0 / ppm, 15.0 / ppm];
    for k in [0, 1] {
        let (dx, dy) = (d(k)[0] - want[0], d(k)[1] - want[1]);
        assert!(
            dx.abs() < 1e-3 && dy.abs() < 1e-3,
            "{k}: {:?} {want:?}",
            d(k)
        );
    }
    assert_eq!((after[2], after[3]), (before[2], before[3]));
    assert_eq!(selected_vertices(&h), [(0, 0), (0, 1)]);

    // A drag of a vertex that is not selected moves it alone.
    let before = after;
    let other = handle(&h, Handle::Vertex((0, 2)));
    drag(&mut h, other, other + vec2(10.0, 10.0), Input::Mouse);
    let after = ring(&h, 1);
    assert_ne!(after[2], before[2]);
    assert_eq!(
        (after[0], after[1], after[3]),
        (before[0], before[1], before[3])
    );
    assert_eq!(selected_vertices(&h), [(0, 2)]);
}

#[test]
fn a_tap_on_an_edge_adds_a_vertex() {
    for input in INPUTS {
        let mut h = vertex_pad();
        let ppm = h.state().atlas.place.unwrap().pts_per_mm;
        let start = ring(&h, 1);
        let ([ax, ay], [bx, _]) = (start[0], start[1]);
        // A third of the way along the top edge, a little off the edge.
        let x = ax + (bx - ax) / 3.0;
        let at = screen(&h, Point::new(x, ay)) + vec2(0.0, 6.0);
        tap_with(&mut h, at, input);
        let after = ring(&h, 1);
        assert_eq!(after.len(), 5, "{input:?}");
        let [nx, ny] = after[1];
        assert!(
            (nx - x).abs() < 0.5 / ppm && ny == ay,
            "{input:?}: {nx} {ny}"
        );
        assert_eq!(selected_vertices(&h), [(0, 1)], "{input:?}");
    }
}

#[test]
fn each_vertex_edit_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    add_frame(&mut h);
    tap(&mut h, VERTEX);
    h.state_mut().atlas.snap = false;
    h.state_mut().save_waiting().unwrap();
    let later = || Instant::now() + std::time::Duration::from_secs(5);
    let mut shapes = vec![frame(&h, 1).shape];

    // A move of two vertices. The app does not save while the pointer is
    // down.
    let corner = handle(&h, Handle::Vertex((0, 0)));
    let right = handle(&h, Handle::Vertex((0, 1)));
    drag(
        &mut h,
        corner - vec2(30.0, 30.0),
        right + vec2(30.0, 30.0),
        Input::Pen,
    );
    drag_steps(&mut h, right, right + vec2(-20.0, 15.0), Input::Finger, 3);
    h.state_mut().save_tick(true, later());
    let end = right + vec2(-30.0, 25.0);
    touch(&mut h, 1, egui::TouchPhase::Move, end, None);
    h.step();
    release(&mut h, end, Input::Finger);
    h.state_mut().save_waiting().unwrap();
    shapes.push(frame(&h, 1).shape);
    // An add.
    let at = edge_middle(&h, 2);
    tap_with(&mut h, at, Input::Finger);
    h.state_mut().save_waiting().unwrap();
    shapes.push(frame(&h, 1).shape);
    // A delete.
    tap(&mut h, DELETE_VERTEX);
    h.state_mut().save_waiting().unwrap();
    shapes.push(frame(&h, 1).shape);
    assert_eq!(ring(&h, 1).len(), 4);

    for want in shapes.iter().rev().skip(1) {
        tap(&mut h, "Undo");
        assert_eq!(&frame(&h, 1).shape, want);
    }
    assert!(matches!(frame(&h, 1).shape, FrameShape::Rectangle(_)));
}

#[test]
fn delete_vertex_keeps_three_and_removes_a_small_hole() {
    let mut h = inset_pad(BOARDS[0]);
    tap(&mut h, POLYGON);
    scroll_to(&mut h, ADD_HOLE);
    tap(&mut h, ADD_HOLE);
    tap(&mut h, VERTEX);
    assert_eq!(ring_len(&h, 1), 4);
    assert!(is_disabled(&h, DELETE_VERTEX));
    // A box selects two vertices of the hole, and the hole goes away.
    let a = handle(&h, Handle::Vertex((1, 0)));
    let b = handle(&h, Handle::Vertex((1, 1)));
    drag(&mut h, a - vec2(30.0, 30.0), b + vec2(8.0, 8.0), Input::Pen);
    assert_eq!(selected_vertices(&h), [(1, 0), (1, 1)]);
    tap(&mut h, DELETE_VERTEX);
    assert_eq!(rings(&h).len(), 1);
    assert!(selected_vertices(&h).is_empty());

    // The outer ring keeps 3 vertices.
    let at = handle(&h, Handle::Vertex((0, 0)));
    tap_with(&mut h, at, Input::Finger);
    tap(&mut h, DELETE_VERTEX);
    assert_eq!(ring_len(&h, 0), 3);
    let at = handle(&h, Handle::Vertex((0, 0)));
    tap_with(&mut h, at, Input::Finger);
    assert!(is_disabled(&h, DELETE_VERTEX));
    h.key_press(egui::Key::Delete);
    h.run_steps(2);
    assert_eq!(ring_len(&h, 0), 3);
    assert_eq!(selected_vertices(&h), [(0, 0)]);
}

#[test]
fn delete_backspace_and_esc_in_the_vertex_tool() {
    let mut h = vertex_pad();
    let inset = frame(&h, 1).id;
    let at = edge_middle(&h, 0);
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(ring_len(&h, 0), 5);
    h.key_press(egui::Key::Backspace);
    h.run_steps(2);
    assert_eq!(ring_len(&h, 0), 4);
    assert!(selected_vertices(&h).is_empty());
    let at = handle(&h, Handle::Vertex((0, 0)));
    tap_with(&mut h, at, Input::Mouse);
    h.key_press(egui::Key::Delete);
    h.run_steps(2);
    assert_eq!(ring_len(&h, 0), 3);
    // With no vertex selected, Delete does not delete the frame.
    h.key_press(egui::Key::Delete);
    h.run_steps(2);
    assert_eq!(frame(&h, 1).id, inset);

    let at = handle(&h, Handle::Vertex((0, 0)));

    tap_with(&mut h, at, Input::Mouse);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(selected_vertices(&h).is_empty());
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(inset));
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
}

/// A frame of 500 by 500 pixels on the middle of face 0, at about one
/// texel of `striped::FACE` in each pixel.
fn zoomed_draw(id: u32, raster: vale_store::RasterId) -> vale_app::globe::FrameDraw {
    use vale_app::globe::flat::{DEFAULT_SPEC, FlatMesh};
    let projection = vale_sphere::Projection::new(DEFAULT_SPEC, 1.0).unwrap();
    vale_app::globe::FrameDraw {
        id,
        place: vale_app::globe::FramePlace {
            center: [250.0; 2],
            map_center: [0.0; 2],
            units_to_pixels: 20000.0,
            rotation: 0.0,
            points_per_radian: 20000.0,
            pixels_per_page_point: 1.0,
        },
        rect: [0, 0, 500, 500],
        mesh: std::sync::Arc::new(FlatMesh::new(&projection)),
        mask: None,
        shown: vec![raster],
        images: Vec::new(),
        raster_looks: Vec::new(),
        image_looks: Vec::new(),
    }
}

#[test]
fn a_zoomed_frame_loads_the_tiles_of_a_layer_from_the_project_file() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, height, _) = striped::project(&dir.path().join("world.gpkg"), 4.0e5);
    // A thread reads the tiles.
    state.headless = false;
    state.sync_globe();
    let draw = zoomed_draw(1, height);
    state.want_frames(vec![draw.clone()]);
    let globe = &state.globe;
    assert!(globe.raster_map(height).is_none(), "in the main memory");
    assert_eq!(globe.frame_detail_level(1, height), Some(0));
    let tiles = globe.frame_detail_tiles(1, height);
    assert!(!tiles.is_empty() && tiles.len() <= 16, "{tiles:?}");
    assert!(!globe.frame_details_ready(1) && globe.details_loading());

    let start = Instant::now();
    while !state.globe.frame_details_ready(1) {
        assert!(start.elapsed().as_secs() < 60, "the tiles did not load");
        std::thread::sleep(std::time::Duration::from_millis(1));
        state.pump_details(false);
        state.want_frames(vec![draw.clone()]);
    }
    assert!(state.globe.frame_ready(&draw));
    assert!(!state.globe.details_loading());
    assert!(state.status.is_empty(), "{}", state.status);
}

/// Starts Map from region with the globe centered on a place.
fn start_region(h: &mut Pad, lon: f64, lat: f64) {
    tap(h, ADD);
    tap(h, vale_app::ui::region::MAP_FROM_REGION);
    assert_eq!(h.state().workspace, Workspace::Globe);
    h.state_mut().globe.view = vale_app::globe::view::GlobeView::centered(lon, lat);
    h.run_steps(3);
}

fn globe_at(h: &Pad, lon: f64, lat: f64) -> Pos2 {
    let dir = vale_app::globe::math::lonlat_to_dir(lon, lat);
    let pos = h.state().globe.project(dir);
    pos.unwrap_or_else(|| panic!("{lon}, {lat} does not show"))
}

/// A pen drag on the globe over 10 W to 10 E, 40 N to 50 N.
fn drag_region(h: &mut Pad) {
    start_region(h, 0.0, 45.0);
    let (from, to) = (globe_at(h, -10.0, 50.0), globe_at(h, 10.0, 40.0));
    drag(h, from, to, Input::Pen);
}

#[test]
fn a_pen_drag_on_the_globe_makes_a_map_centered_on_the_region() {
    let mut h = pad();
    let maps = h.state().doc.project.atlas.maps.len();
    drag_region(&mut h);
    assert_eq!(h.state().workspace, Workspace::Atlas);
    assert!(h.state().region.is_none());
    assert!(h.state().doc.project.atlas.maps.len() >= maps.max(1));
    let spec = selected_projection(&h);
    assert_eq!(spec.kind, ProjectionKind::LambertConformal);
    assert!(spec.lon0.abs() < 1.5, "{spec:?}");
    assert!((spec.lat0 - 45.0).abs() < 1.0, "{spec:?}");
    // The parallels are one sixth of the range in from each edge.
    assert!(((spec.lat0 - spec.lat1) - (spec.lat2 - spec.lat0)).abs() < 1e-9);
    assert!((spec.lat1 - 41.7).abs() < 1.0, "{spec:?}");
    assert!((spec.lat2 - 48.3).abs() < 1.0, "{spec:?}");
    let f = frame(&h, open_page(&h).frames.len() - 1);
    assert!(f.center[0].abs() < 1.5 && (f.center[1] - 45.0).abs() < 1.0);
}

#[test]
fn the_map_of_a_region_is_in_a_new_frame_on_the_open_page_with_the_region_in_view() {
    let mut h = pad();
    let page = open_page(&h).id;
    let frames = open_page(&h).frames.len();
    drag_region(&mut h);
    let page = h.state().doc.project.atlas.page(page).unwrap().clone();
    assert_eq!(open_page(&h).id, page.id);
    assert_eq!(page.frames.len(), frames + 1);
    let new = page.frames.last().unwrap().clone();
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(new.id));
    assert_eq!(h.state().atlas.tab, FrameTab::Projection);
    let others: Vec<_> = page.frames.iter().map(|f| f.map).collect();
    assert_eq!(others.iter().filter(|m| **m == new.map).count(), 1);
    let bbox = shape_bbox(&new.shape);
    let (min, max) = (
        screen(&h, Point::new(bbox.x0, bbox.y0)),
        screen(&h, Point::new(bbox.x1, bbox.y1)),
    );
    let shown = Rect::from_min_max(min, max).expand(1.0);
    for lon in [-10.0, 0.0, 10.0] {
        for lat in [40.0, 45.0, 50.0] {
            let at = pos_of(&mut h, frames, [lon, lat]);
            assert!(
                shown.contains(at),
                "{lon}, {lat} at {at:?} is not in {shown:?}"
            );
        }
    }
}

#[test]
fn a_finger_drag_in_map_from_region_turns_the_globe() {
    let mut h = pad();
    let frames = open_page(&h).frames.len();
    start_region(&mut h, 0.0, 45.0);
    screenshot::save(&mut h, "pad-map-from-region");
    let before = globe_at(&h, 0.0, 45.0);
    drag(&mut h, before, before + vec2(80.0, 0.0), Input::Finger);
    assert_ne!(globe_at(&h, 0.0, 45.0), before);
    assert_eq!(h.state().workspace, Workspace::Globe);
    assert!(h.state().region.is_some());
    assert_eq!(open_page(&h).frames.len(), frames);
}

#[test]
fn the_card_of_map_from_region_closes_back_to_the_atlas() {
    let mut h = pad();
    let frames = open_page(&h).frames.len();
    let tool = h.state().globe.tool;
    start_region(&mut h, 0.0, 45.0);
    tap(
        &mut h,
        &format!("Close {}", vale_app::ui::region::MAP_FROM_REGION),
    );
    assert_eq!(h.state().workspace, Workspace::Atlas);
    assert!(h.state().region.is_none());
    assert_eq!(h.state().globe.tool, tool);
    assert_eq!(open_page(&h).frames.len(), frames);
}

#[test]
fn the_projection_tab_of_a_region_frame_shows_the_reason_in_one_line() {
    let reason = vale_app::region::RegionKind::MidLatitude.reason();
    let mut h = pad();
    drag_region(&mut h);
    screenshot::save(&mut h, "pad-region-projection");
    let rect = h.get_by_label(reason).rect();
    assert!(rect.height() < 30.0, "{rect:?}");
    // A change of the projection ends the reason, and a change back does
    // not show it again.
    let id = h.state().atlas.frame().unwrap();
    atlas::set_projection_kind(h.state_mut(), id, ProjectionKind::Mercator);
    h.run_steps(3);
    assert!(h.query_by_label(reason).is_none());
    atlas::set_projection_kind(h.state_mut(), id, ProjectionKind::LambertConformal);
    h.run_steps(3);
    assert!(h.query_by_label(reason).is_none());
    assert!(h.state().atlas.suggestion.is_none());
}

#[test]
fn the_reason_ends_when_the_projection_tab_closes() {
    let reason = vale_app::region::RegionKind::MidLatitude.reason();
    for leave in [FrameTab::Frame, FrameTab::Layers] {
        let mut h = pad();
        drag_region(&mut h);
        assert!(h.query_by_label(reason).is_some());
        h.state_mut().atlas.tab = leave;
        h.run_steps(3);
        h.state_mut().atlas.tab = FrameTab::Projection;
        h.run_steps(3);
        assert!(h.query_by_label(reason).is_none(), "{leave:?}");
    }
    let mut h = pad();
    drag_region(&mut h);
    h.state_mut().atlas.selection = AtlasSelection::None;
    h.run_steps(3);
    let id = open_page(&h).frames.last().unwrap().id;
    h.state_mut().atlas.selection = AtlasSelection::Frame(id);
    h.run_steps(3);
    assert!(h.query_by_label(reason).is_none());
}

#[test]
fn the_project_file_stores_no_region() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let before = doc.project.atlas.clone();
    let mut s = state();
    s.doc = doc;
    let mut h = pad_with(s, BOARDS[0]);
    drag_region(&mut h);
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    // The same atlas made by hand, with no region, is the atlas of the file.
    let new = project.atlas.pages.iter().flat_map(|p| &p.frames);
    let new = new.max_by_key(|f| f.id.0).unwrap().clone();
    let page = project.atlas.pages.iter().find(|p| p.frames.contains(&new));
    let mut by_hand = before;
    let map = by_hand.unshown_map().unwrap_or_else(|| by_hand.add_map());
    assert_eq!(map, new.map);
    by_hand.map_mut(map).unwrap().projection = project.atlas.map(map).unwrap().projection.clone();
    by_hand.add_frame(
        page.unwrap().id,
        FrameRecord {
            id: vale_store::FrameId(0),
            ..new
        },
    );
    assert_eq!(by_hand, project.atlas);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.project.atlas, project.atlas);
    let mut s = state();
    s.doc = doc;
    let h = pad_with(s, BOARDS[0]);
    let reason = vale_app::region::RegionKind::MidLatitude.reason();
    assert!(h.query_by_label(reason).is_none());
}

#[test]
fn a_box_around_a_pole_suggests_polar_stereographic() {
    let mut h = pad();
    start_region(&mut h, 0.0, 90.0);
    let (from, to) = (globe_at(&h, -135.0, 70.0), globe_at(&h, 45.0, 70.0));
    drag(&mut h, from, to, Input::Pen);
    let spec = selected_projection(&h);
    assert_eq!(spec.kind, ProjectionKind::PolarStereographic);
    assert_eq!(spec.lat0, 90.0);
    let reason = vale_app::region::RegionKind::Polar.reason();
    assert!(h.query_by_label(reason).is_some());
}

/// Widens the first rule of Rivers in the target of a map.
fn widen_in(h: &mut Pad, map: MapId) {
    let rivers = layer_id(h, "Rivers");
    h.state_mut().edit_map_style(map, rivers, |s| {
        let rule = &mut s.doc.entry_mut(rivers).unwrap().style.style.rules[0];
        let mut stroke = rule.stroke().unwrap();
        stroke.width += 0.5;
        rule.set_stroke(Some(stroke));
    });
    h.run_steps(2);
}

/// The settings of a map that are its own.
fn own_settings(h: &Pad, map: MapId) -> vale_store::MapRecord {
    h.state().doc.project.atlas.map(map).unwrap().clone()
}

fn is_plain(record: &vale_store::MapRecord) -> bool {
    record.hidden.is_empty()
        && record.shown.is_empty()
        && record.styles.is_empty()
        && record.labels.is_empty()
}

#[test]
fn update_map_style_shows_a_change_in_each_map_that_links_to_it() {
    let mut h = inset_pad(BOARDS[0]);
    add_frame(&mut h);
    let maps = [0, 1, 2].map(|i| frame(&h, i).map);
    let style = h.state_mut().save_as_map_style(maps[2]).unwrap();
    h.state_mut().set_map_style_link(maps[1], Some(style));
    h.run_steps(2);
    let before = rivers_width(&h, None);
    let rgb = rivers_rgb(&h);

    // The selected frame shows the map of frame 2. A change in its panel
    // is a local change of that map.
    open_rivers_rule(&mut h);
    widen(&mut h);
    let width = rivers_width(&h, Some(maps[2]));
    assert_ne!(width, before);
    assert_eq!(rivers_width(&h, Some(maps[1])), before);
    let land = layer_id(&h, "Land");
    h.state_mut().set_map_shown(maps[2], land, false);
    assert!(shows(&h, maps[1], "Land"));

    // Update map style moves the local changes into the map style.
    frame_tab_of(&mut h, 2);
    find(&mut h, UPDATE_MAP_STYLE);
    tap(&mut h, UPDATE_MAP_STYLE);
    assert!(is_plain(&own_settings(&h, maps[2])));
    assert!(control(&h, UPDATE_MAP_STYLE).is_none());
    assert_eq!(rivers_width(&h, Some(maps[1])), width);
    assert_eq!(rivers_width(&h, Some(maps[2])), width);
    assert_eq!(rivers_width(&h, Some(maps[0])), before);
    assert_eq!(rivers_width(&h, None), before);
    let rivers = layer_id(&h, "Rivers");
    let doc = &h.state().doc;
    let styled = doc.map_style(Target::Style(style), rivers).unwrap();
    assert_eq!(styled.style.rules[0].stroke().unwrap().width, width);
    assert!(!shows(&h, maps[1], "Land") && !shows(&h, maps[2], "Land"));
    assert!(shows(&h, maps[0], "Land"));
    assert!(strokes(&mut h, 1, rgb, width) > 0);
    assert!(strokes(&mut h, 2, rgb, width) > 0);
    assert_eq!(strokes(&mut h, 0, rgb, width), 0);
}

#[test]
fn a_linked_map_holds_its_own_style_for_a_layer() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let style = h.state_mut().save_as_map_style(inset).unwrap();
    h.state_mut().set_map_style_link(main, Some(style));
    let styled = rivers_width(&h, Some(inset));
    let style_text = own_rivers(&h, Target::Style(style));
    open_rivers_rule(&mut h);
    widen(&mut h);
    let own = rivers_width(&h, Some(inset));
    assert!(own > styled);
    assert_eq!(rivers_width(&h, Some(main)), styled);
    assert_eq!(own_rivers(&h, Target::Style(style)), style_text);
    assert!(own_rivers(&h, Target::Map(inset)).is_some());
    assert!(own_rivers(&h, Target::Map(main)).is_none());

    // Reset to map style in the panel of the layer takes the own style away.
    tap(&mut h, "Back to Rivers");
    scroll_to(&mut h, RESET_TO_MAP_STYLE);
    tap(&mut h, RESET_TO_MAP_STYLE);
    assert_eq!(rivers_width(&h, Some(inset)), styled);
    assert!(own_rivers(&h, Target::Map(inset)).is_none());
    assert!(control(&h, RESET_TO_MAP_STYLE).is_none());

    // A switch of the map changes only the map.
    let land = layer_id(&h, "Land");
    h.state_mut().set_map_shown(inset, land, false);
    assert!(shows(&h, main, "Land") && !shows(&h, inset, "Land"));
    assert!(h.state().doc.shown(Target::Style(style), land));
    // A switch that is the switch of the map style is no local change.
    h.state_mut().set_map_shown(inset, land, true);
    assert!(is_plain(&own_settings(&h, inset)));
    // The map shows a layer that its map style hides.
    h.state_mut().set_map_shown(main, land, false);
    h.state_mut().update_map_style(main);
    assert!(!shows(&h, main, "Land") && !shows(&h, inset, "Land"));
    h.state_mut().set_map_shown(inset, land, true);
    assert!(shows(&h, inset, "Land") && !shows(&h, main, "Land"));
    assert!(own_settings(&h, inset).shown.contains(&land));
}

/// A map frame with a style of its own for Rivers, a layer that is off,
/// and a label class of its own. The inset is selected.
fn styled_pad() -> Pad {
    let mut h = inset_pad(BOARDS[0]);
    let inset = frame(&h, 1).map;
    widen_in(&mut h, inset);
    let land = layer_id(&h, "Land");
    h.state_mut().set_map_shown(inset, land, false);
    h.state_mut().add_label_class(inset).unwrap();
    h.run_steps(2);
    h
}

#[test]
fn save_as_map_style_keeps_the_look_of_the_frames() {
    let mut h = styled_pad();
    let inset = frame(&h, 1).map;
    let before = look(&mut h, 1);
    let record = own_settings(&h, inset);
    assert!(!record.hidden.is_empty() && !record.styles.is_empty());
    assert!(!record.labels.is_empty());
    let drawn = record.styles.clone();
    let style = h.state_mut().save_as_map_style(inset).unwrap();
    h.run_steps(2);
    same_look(&look(&mut h, 1), &before, "save as");
    let record = own_settings(&h, inset);
    assert_eq!(record.style, Some(style));
    assert!(is_plain(&record));
    let s = h.state().doc.project.atlas.map_style(style).unwrap();
    assert_eq!(s.name, "Map style");
    assert!(!s.hidden.is_empty() && !s.labels.is_empty());
    // The map style holds a style for each layer.
    assert_eq!(s.styles, drawn);
    let layers = h.state().doc.project.layers().iter();
    assert!(layers.into_iter().all(|l| s.styles.contains_key(&l.id)));

    // A second save gives the map a new map style with the same look.
    let second = h.state_mut().save_as_map_style(inset).unwrap();
    assert_ne!(second, style);
    same_look(&look(&mut h, 1), &before, "second save");
    let names: Vec<String> = (h.state().doc.project.atlas.map_styles.iter())
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(names, ["Map style", "Map style 2"]);
}

#[test]
fn unlinking_a_map_style_keeps_the_look_of_the_frames() {
    let mut h = styled_pad();
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let style = h.state_mut().save_as_map_style(inset).unwrap();
    // A map style that hides Rivers, from the main map.
    let other = h.state_mut().save_as_map_style(main).unwrap();
    let rivers = layer_id(&h, "Rivers");
    h.state_mut().set_map_shown(main, rivers, false);
    h.state_mut().update_map_style(main);

    // A link to another map style changes the look.
    let before = look(&mut h, 1);
    h.state_mut().set_map_style_link(inset, Some(other));
    assert_ne!(look(&mut h, 1), before);
    assert!(!shows(&h, inset, "Rivers") && shows(&h, inset, "Land"));
    h.state_mut().set_map_style_link(inset, Some(style));
    same_look(&look(&mut h, 1), &before, "link back");

    // A style of the map stays through the unlink.
    widen_in(&mut h, inset);
    let width = rivers_width(&h, Some(inset));
    let before = look(&mut h, 1);
    h.state_mut().set_map_style_link(inset, None);
    same_look(&look(&mut h, 1), &before, "unlink");
    assert_eq!(rivers_width(&h, Some(inset)), width);
    let record = own_settings(&h, inset);
    assert_eq!(record.style, None);
    assert!(!record.hidden.is_empty() && !record.labels.is_empty());
    assert!(h.state().doc.project.atlas.map_style(style).is_some());
}

#[test]
fn deleting_a_map_style_copies_its_settings_into_each_map() {
    let mut h = styled_pad();
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let style = h.state_mut().save_as_map_style(inset).unwrap();
    h.state_mut().set_map_style_link(main, Some(style));
    widen_in(&mut h, inset);
    let before = [look(&mut h, 0), look(&mut h, 1)];
    assert_ne!(before[0], before[1]);
    let layers: Vec<LayerId> = (h.state().doc.project.layers().iter())
        .map(|l| l.id)
        .collect();
    let drawn = |h: &Pad, map: MapId| -> Vec<_> {
        let doc = &h.state().doc;
        layers.iter().map(|l| doc.map_style(map, *l)).collect()
    };
    let styles = [drawn(&h, main), drawn(&h, inset)];
    h.state_mut().delete_map_style(style);
    h.run_steps(2);
    same_look(&look(&mut h, 0), &before[0], "main");
    same_look(&look(&mut h, 1), &before[1], "inset");
    let atlas = &h.state().doc.project.atlas;
    assert!(atlas.map_styles.is_empty());
    assert!(atlas.maps.iter().all(|m| m.style.is_none()));
    assert!(!is_plain(&own_settings(&h, main)));
    // Each map holds a copy of each style that it drew.
    for (map, styles) in [main, inset].into_iter().zip(&styles) {
        let record = own_settings(&h, map);
        assert!(layers.iter().all(|l| record.styles.contains_key(l)));
        assert_eq!(&drawn(&h, map), styles);
    }
}

#[test]
fn map_styles_survive_a_close_and_an_open() {
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
    let style = h.state_mut().save_as_map_style(inset).unwrap();
    h.state_mut().set_map_style_link(main, Some(style));
    widen_in(&mut h, inset);
    h.state_mut().update_map_style(inset);
    // A local change of the inset.
    widen_in(&mut h, inset);
    assert!(atlas::rename_map_style(h.state_mut(), style, "Coast"));
    let width = rivers_width(&h, Some(main));
    let own = rivers_width(&h, Some(inset));
    assert!(own > width);
    let mut project = h.state().doc.project.clone();
    session.flush(&mut project, &mut []).unwrap();
    drop(session);

    let mut doc = Document::sample();
    let _session = Session::open(&path, &mut doc).unwrap();
    assert_eq!(doc.project.atlas, project.atlas);
    let rivers = layer_id(&h, "Rivers");
    let style = doc.map_style(main, rivers).unwrap();
    assert_eq!(style.style.rules[0].stroke().unwrap().width, width);
    let style = doc.map_style(inset, rivers).unwrap();
    assert_eq!(style.style.rules[0].stroke().unwrap().width, own);
    assert_eq!(doc.local_changes(inset), [rivers].into());
    assert_eq!(doc.project.atlas.map_styles[0].name, "Coast");
}

#[test]
fn a_map_style_gets_a_name_that_no_other_map_style_has() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let a = h.state_mut().save_as_map_style(main).unwrap();
    let b = h.state_mut().save_as_map_style(inset).unwrap();
    let s = h.state_mut();
    assert!(atlas::rename_map_style(s, a, "  Coast "));
    assert!(!atlas::rename_map_style(s, b, "Coast"));
    assert!(!atlas::rename_map_style(s, b, "   "));
    let names: Vec<&str> = (s.doc.project.atlas.map_styles.iter())
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["Coast", "Map style 2"]);
}

#[test]
fn each_change_of_a_map_style_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut s = state();
    s.session = Some(Session::open(&path, &mut s.doc).unwrap());
    let mut h = pad_with(s, BOARDS[0]);
    add_frame(&mut h);
    add_frame(&mut h);
    h.state_mut().save_waiting().unwrap();
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let settings = |h: &Pad| {
        let atlas = &h.state().doc.project.atlas;
        (atlas.maps.clone(), atlas.map_styles.clone())
    };
    let check = |h: &mut Pad, what: &str, change: &dyn Fn(&mut Pad)| {
        let before = settings(h);
        change(h);
        h.state_mut().save_waiting().unwrap();
        let after = settings(h);
        assert_ne!(after, before, "{what}");
        h.state_mut().undo();
        h.run_steps(2);
        assert_eq!(settings(h), before, "{what}");
        assert!(h.state().can_redo(), "{what}");
        h.state_mut().redo();
        h.run_steps(2);
        assert_eq!(settings(h), after, "{what}");
    };
    check(&mut h, "save as", &|h| {
        h.state_mut().save_as_map_style(inset).unwrap();
    });
    let style = h.state().doc.project.atlas.map_styles[0].id;
    // A link drops the own switches, styles, and label classes of the map.
    let land = layer_id(&h, "Land");
    h.state_mut().set_map_shown(main, land, false);
    h.state_mut().add_label_class(main).unwrap();
    h.state_mut().save_waiting().unwrap();
    let own = own_settings(&h, main);
    assert!(!own.hidden.is_empty() && !own.styles.is_empty() && !own.labels.is_empty());
    check(&mut h, "link", &|h| {
        h.state_mut().set_map_style_link(main, Some(style))
    });
    assert!(is_plain(&own_settings(&h, main)));
    h.state_mut().undo();
    h.run_steps(2);
    assert_eq!(own_settings(&h, main), own);
    h.state_mut().redo();
    h.run_steps(2);
    check(&mut h, "local change", &|h| widen_in(h, inset));
    check(&mut h, "update map style", &|h| {
        h.state_mut().update_map_style(inset)
    });
    check(&mut h, "own style", &|h| widen_in(h, inset));
    let rivers = layer_id(&h, "Rivers");
    check(&mut h, "reset a layer", &|h| {
        h.state_mut().reset_layer_to_map_style(inset, rivers)
    });
    check(&mut h, "own switch", &|h| {
        h.state_mut().set_map_shown(inset, land, false)
    });
    check(&mut h, "reset to map style", &|h| {
        h.state_mut().reset_to_map_style(inset)
    });
    check(&mut h, "copy globe style", &|h| {
        h.state_mut().copy_globe_style(inset, rivers)
    });
    check(&mut h, "mask", &|h| {
        h.state_mut().edit_map_style(inset, rivers, |s| {
            let mask = vale_style::Mask::Polygons { layer: land };
            s.doc.entry_mut(rivers).unwrap().style.style.mask = Some(mask);
        });
        h.run_steps(2);
    });
    check(&mut h, "add rule", &|h| {
        h.state_mut().edit_map_style(inset, rivers, |s| {
            let kind = s.doc.layer(rivers).unwrap().geometry().unwrap();
            s.doc.entry_mut(rivers).unwrap().style.add_rule(kind);
        });
        h.run_steps(2);
    });
    let globe = rivers_rules(&h, None);
    assert_eq!(rivers_rules(&h, Some(inset)), globe + 1);
    assert_eq!(rivers_rules(&h, Some(main)), globe);
    check(&mut h, "rename", &|h| {
        assert!(atlas::rename_map_style(h.state_mut(), style, "Coast"));
    });
    check(&mut h, "unlink", &|h| {
        h.state_mut().set_map_style_link(main, None)
    });
    check(&mut h, "delete", &|h| h.state_mut().delete_map_style(style));
}

/// The count of the rules of Rivers: in the globe style, or in a map.
fn rivers_rules(h: &Pad, map: Option<MapId>) -> usize {
    let rivers = layer_id(h, "Rivers");
    let doc = &h.state().doc;
    let style = match map {
        Some(map) => doc.map_style(map, rivers).unwrap(),
        None => doc.layer_style(rivers).unwrap(),
    };
    style.style.rules.len()
}

/// The items of a frame of the open page as text, with no font IDs. Each
/// compose loads the fonts again, with new IDs.
fn look(h: &mut Pad, index: usize) -> Vec<String> {
    (frame_items(h, index).iter())
        .map(|item| {
            let text = format!("{item:?}");
            let mut out = String::new();
            let mut rest = text.as_str();
            while let Some(at) = rest.find("Blob { id: ") {
                out.push_str(&rest[..at]);
                rest = rest[at + 11..].trim_start_matches(|c: char| c.is_ascii_digit());
            }
            out.push_str(rest);
            out
        })
        .collect()
}

/// Asserts that two looks are the same, with a short message.
fn same_look(a: &[String], b: &[String], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: the count of the items");
    if let Some(i) = (0..a.len()).find(|&i| a[i] != b[i]) {
        let short = |t: &str| t.chars().take(300).collect::<String>();
        panic!("{what}: item {i}: {} != {}", short(&a[i]), short(&b[i]));
    }
}

/// The map style that a map links to.
fn linked(h: &Pad, map: MapId) -> Option<vale_store::MapStyleId> {
    own_settings(h, map).style
}

/// Selects frame `index` of the open page and shows its Frame tab.
fn frame_tab_of(h: &mut Pad, index: usize) {
    let atlas = &mut h.state_mut().atlas;
    atlas.layer = None;
    atlas.map_styles = None;
    let id = frame(h, index).id;
    h.state_mut().atlas.selection = AtlasSelection::Frame(id);
    h.run_steps(2);
    tap(h, FRAME_TABS[0]);
}

/// Opens the Map style page of frame `index`.
fn map_style_page_of(h: &mut Pad, index: usize) {
    frame_tab_of(h, index);
    find(h, MAP_STYLE);
    tap(h, MAP_STYLE);
    assert_eq!(h.state().atlas.open_map_styles(), Some(frame(h, index).id));
}

/// A sideways drag of a finger on a row, from near the right end of it.
fn swipe_row(h: &mut Pad, name: &str, dx: f32) {
    let row = control(h, name).unwrap_or_else(|| panic!("no row {name}"));
    let from = row.right_center() - vec2(10.0, 0.0);
    drag(h, from, from + vec2(dx, 0.0), Input::Finger);
    // The row snaps open or shut.
    h.run_steps(5);
}

#[test]
fn the_map_style_page_saves_and_links_a_map_style() {
    let mut h = styled_pad();
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let before = look(&mut h, 1);
    map_style_page_of(&mut h, 1);
    find(&mut h, SAVE_AS_MAP_STYLE);
    tap(&mut h, SAVE_AS_MAP_STYLE);
    let style = linked(&h, inset).expect("the inset links a map style");
    assert!(is_plain(&own_settings(&h, inset)));
    same_look(&look(&mut h, 1), &before, "save as");

    // The main frame picks the map style from the page.
    map_style_page_of(&mut h, 0);
    let row = map_style_row("Map style");
    find(&mut h, &row);
    tap(&mut h, &row);
    assert_eq!(linked(&h, main), Some(style));
    assert!(!shows(&h, main, "Land"));
    assert_eq!(rivers_width(&h, Some(main)), rivers_width(&h, Some(inset)));

    // None unlinks the map, and its frame keeps its look.
    let before = look(&mut h, 0);
    find(&mut h, NO_MAP_STYLE_ROW);
    tap(&mut h, NO_MAP_STYLE_ROW);
    assert_eq!(linked(&h, main), None);
    assert!(!is_plain(&own_settings(&h, main)));
    same_look(&look(&mut h, 0), &before, "none");
    assert_eq!(linked(&h, inset), Some(style));

    // Back goes to the Frame tab.
    tap(&mut h, &format!("Back to {}", FRAME_TABS[0]));
    assert_eq!(h.state().atlas.open_map_styles(), None);
    assert!(control(&h, MAP_NAME).is_some());
}

#[test]
fn the_map_style_page_renames_a_map_style_and_a_swipe_deletes_it() {
    let mut h = styled_pad();
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    map_style_page_of(&mut h, 1);
    assert!(control(&h, MAP_STYLE_NAME).is_none());
    find(&mut h, SAVE_AS_MAP_STYLE);
    tap(&mut h, SAVE_AS_MAP_STYLE);
    // Return keeps the name that the new map style gets.
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    let style = linked(&h, inset).unwrap();
    h.state_mut().set_map_style_link(main, Some(style));
    h.run_steps(2);

    let first = h
        .state()
        .doc
        .project
        .atlas
        .map_style(style)
        .unwrap()
        .name
        .clone();
    rename_by_swipe(&mut h, &first, "Coast");
    let name = |h: &Pad| {
        h.state()
            .doc
            .project
            .atlas
            .map_style(style)
            .unwrap()
            .name
            .clone()
    };
    assert_eq!(name(&h), "Coast");
    // An empty name keeps the old name.
    rename_by_swipe(&mut h, "Coast", "");
    assert_eq!(name(&h), "Coast");
    rename_by_swipe(&mut h, "Coast", "Coast map");
    assert_eq!(name(&h), "Coast map");

    // A swipe shows the Delete button of the row, and a tap deletes the map
    // style. Each map keeps its look.
    let before = [look(&mut h, 0), look(&mut h, 1)];
    let row = map_style_row("Coast map");
    find(&mut h, &row);
    // A short swipe snaps shut.
    swipe_row(&mut h, &row, -30.0);
    assert_eq!(h.state().atlas.map_style_swipe, None);
    assert!(control(&h, "Delete Coast map").is_none());
    swipe_row(&mut h, &row, -120.0);
    let open = h.state().atlas.map_style_swipe.unwrap();
    assert_eq!((open.row, open.moving), (style, false));
    assert_eq!(linked(&h, inset), Some(style));
    tap(&mut h, "Delete Coast map");
    assert!(h.state().doc.project.atlas.map_styles.is_empty());
    assert_eq!((linked(&h, main), linked(&h, inset)), (None, None));
    same_look(&look(&mut h, 0), &before[0], "main");
    same_look(&look(&mut h, 1), &before[1], "inset");
    assert!(control(&h, &row).is_none());
    assert!(control(&h, MAP_STYLE_NAME).is_none());
}

#[test]
fn the_frame_tab_updates_and_resets_the_map_style() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let style = h.state_mut().save_as_map_style(inset).unwrap();
    h.state_mut().set_map_style_link(main, Some(style));
    h.run_steps(2);
    frame_tab_of(&mut h, 1);
    assert!(control(&h, UPDATE_MAP_STYLE).is_none());
    assert!(control(&h, RESET_TO_MAP_STYLE).is_none());
    let before = [look(&mut h, 0), look(&mut h, 1)];

    // A change in a frame is a local change.
    widen_in(&mut h, inset);
    assert!(control(&h, UPDATE_MAP_STYLE).is_some());
    find(&mut h, RESET_TO_MAP_STYLE);
    tap(&mut h, RESET_TO_MAP_STYLE);
    assert!(is_plain(&own_settings(&h, inset)));
    same_look(&look(&mut h, 1), &before[1], "reset");
    assert!(control(&h, RESET_TO_MAP_STYLE).is_none());

    widen_in(&mut h, inset);
    let width = rivers_width(&h, Some(inset));
    assert_ne!(rivers_width(&h, Some(main)), width);
    find(&mut h, UPDATE_MAP_STYLE);
    tap(&mut h, UPDATE_MAP_STYLE);
    assert!(is_plain(&own_settings(&h, inset)));
    assert_eq!(rivers_width(&h, Some(main)), width);
    assert_ne!(look(&mut h, 0), before[0]);
    assert!(control(&h, UPDATE_MAP_STYLE).is_none());
}

#[test]
fn the_map_style_controls_work_with_a_finger() {
    use vale_app::ui::pad::rules::MASK;
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = inset_pad(size);
        let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
        let style = h.state_mut().save_as_map_style(inset).unwrap();
        h.state_mut().set_map_style_link(main, Some(style));
        h.state_mut().save_as_map_style(main).unwrap();
        h.state_mut().set_map_style_link(main, Some(style));
        widen_in(&mut h, inset);
        let land = layer_id(&h, "Land");
        h.state_mut().set_map_shown(inset, land, false);
        h.run_steps(2);

        // The Frame tab of a linked map with local changes.
        frame_tab_of(&mut h, 1);
        find(&mut h, RESET_TO_MAP_STYLE);
        assert!(control(&h, UPDATE_MAP_STYLE).is_some(), "{what}");
        check_controls(&h, &format!("{what}: frame tab"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-frame-edited");
        }

        // The Map style page.
        tap(&mut h, MAP_STYLE);
        let row = map_style_row("Map style");
        find(&mut h, &row);
        check_controls(&h, &format!("{what}: map style page"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-map-styles");
        }
        swipe_row(&mut h, &row, -120.0);
        assert!(control(&h, "Delete Map style").is_some(), "{what}");
        assert!(control(&h, "Rename Map style").is_some(), "{what}");
        check_controls(&h, &format!("{what}: open row"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-map-style-swipe");
        }
        tap(&mut h, "Rename Map style");
        assert!(control(&h, MAP_STYLE_NAME).is_some(), "{what}");
        check_controls(&h, &format!("{what}: rename"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-map-style-rename");
        }
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        find(&mut h, SAVE_AS_MAP_STYLE);
        check_controls(&h, &format!("{what}: map style buttons"));
        tap(&mut h, &format!("Back to {}", FRAME_TABS[0]));

        // The Layers tab with the marks of the local changes.
        tap(&mut h, FRAME_TABS[2]);
        assert_eq!(h.query_all_by_label(EDITED).count(), 2, "{what}");
        check_controls(&h, &format!("{what}: layers tab"));
        if size == BOARDS[0] {
            screenshot::save(&mut h, "pad-atlas-frame-layers-edited");
        }
        scroll_to(&mut h, "Open Rivers");
        tap(&mut h, "Open Rivers");
        find(&mut h, RESET_TO_MAP_STYLE);
        check_controls(&h, &format!("{what}: layer panel"));

        // The Mask page of the layer in the frame.
        find(&mut h, MASK);
        tap(&mut h, MASK);
        assert_eq!(
            h.state().atlas.layer_page,
            Some(vale_app::ui::pad::Page::Mask),
            "{what}"
        );
        check_controls(&h, &format!("{what}: mask page"));
        tap(&mut h, "Back to Rivers");
        assert_eq!(h.state().atlas.layer_page, None, "{what}");
    }
}

#[test]
fn a_mask_in_a_frame_changes_only_its_map() {
    use vale_app::ui::pad::rules::MASK;
    use vale_style::Mask;
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let (rivers, land) = (layer_id(&h, "Rivers"), layer_id(&h, "Land"));
    let mask = |h: &Pad, map: Option<MapId>| {
        let doc = &h.state().doc;
        let style = match map {
            Some(map) => doc.map_style(map, rivers).unwrap(),
            None => doc.layer_style(rivers).unwrap(),
        };
        style.style.mask
    };
    let before = [look(&mut h, 0), look(&mut h, 1)];
    tap(&mut h, FRAME_TABS[2]);
    scroll_to(&mut h, "Open Rivers");
    tap(&mut h, "Open Rivers");
    find(&mut h, MASK);
    tap(&mut h, MASK);
    find(&mut h, "Land");
    tap(&mut h, "Land");
    let by_land = Some(Mask::Polygons { layer: land });
    assert_eq!(mask(&h, Some(inset)), by_land);
    assert_eq!(mask(&h, Some(main)), None);
    assert_eq!(mask(&h, None), None);
    same_look(&look(&mut h, 0), &before[0], "main");
    assert_ne!(look(&mut h, 1), before[1]);

    // A mask of the globe style does not reach a map.
    let masked = look(&mut h, 1);
    h.state_mut()
        .doc
        .entry_mut(rivers)
        .unwrap()
        .style
        .style
        .mask = by_land;
    h.state_mut().touch();
    h.run_steps(2);
    assert_eq!(mask(&h, Some(main)), None);
    same_look(&look(&mut h, 0), &before[0], "main after the globe mask");
    same_look(&look(&mut h, 1), &masked, "inset after the globe mask");
}

/// Scrolls the panel up or down until a control shows.
fn find(h: &mut Pad, name: &str) {
    scroll_up_to(h, name);
    scroll_to(h, name);
}

#[test]
fn linking_and_unlinking_in_the_map_style_page_keep_the_look() {
    let mut h = styled_pad();
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let before = look(&mut h, 1);
    map_style_page_of(&mut h, 1);
    find(&mut h, SAVE_AS_MAP_STYLE);
    tap(&mut h, SAVE_AS_MAP_STYLE);
    let style = linked(&h, inset).unwrap();
    same_look(&look(&mut h, 1), &before, "save as");

    // None gives the map a copy of the settings that it drew.
    find(&mut h, NO_MAP_STYLE_ROW);
    tap(&mut h, NO_MAP_STYLE_ROW);
    assert_eq!(linked(&h, inset), None);
    let own = own_settings(&h, inset);
    assert!(!own.hidden.is_empty() && !own.styles.is_empty() && !own.labels.is_empty());
    same_look(&look(&mut h, 1), &before, "none");

    // A link drops the settings of the map, and the map style draws the
    // same look.
    let row = map_style_row("Map style");
    find(&mut h, &row);
    tap(&mut h, &row);
    assert_eq!(linked(&h, inset), Some(style));
    assert!(is_plain(&own_settings(&h, inset)));
    same_look(&look(&mut h, 1), &before, "link");

    // The main map drops its own switches, styles, and label classes too.
    let rivers = layer_id(&h, "Rivers");
    h.state_mut().set_map_shown(main, rivers, false);
    h.state_mut().add_label_class(main).unwrap();
    h.run_steps(2);
    assert!(!is_plain(&own_settings(&h, main)));
    map_style_page_of(&mut h, 0);
    find(&mut h, &row);
    tap(&mut h, &row);
    assert_eq!(linked(&h, main), Some(style));
    assert!(is_plain(&own_settings(&h, main)));
    assert!(shows(&h, main, "Rivers") && !shows(&h, main, "Land"));
}

/// The names of the rules of Rivers: in the globe style, or in a map.
fn rivers_rule_names(h: &Pad, map: Option<MapId>) -> Vec<String> {
    let rivers = layer_id(h, "Rivers");
    let doc = &h.state().doc;
    let style = match map {
        Some(map) => doc.map_style(map, rivers).unwrap(),
        None => doc.layer_style(rivers).unwrap(),
    };
    style.style.rules.iter().map(|r| r.name.clone()).collect()
}

/// Adds a rule in the panel of Rivers of the selected frame, moves it up,
/// and deletes it. After each step, `check` gets the rules that the panel
/// shows.
fn add_move_and_delete(h: &mut Pad, map: MapId, check: &dyn Fn(&Pad, &str)) {
    use vale_app::ui::pad::rules::{ADD_RULE, DELETE_RULE, MOVE_UP};
    let globe = rivers_rule_names(h, None);
    let n = globe.len();
    tap(h, FRAME_TABS[2]);
    scroll_to(h, "Open Rivers");
    tap(h, "Open Rivers");
    scroll_to(h, ADD_RULE);
    tap(h, ADD_RULE);
    let added = rivers_rule_names(h, Some(map));
    assert_eq!(added.len(), n + 1);
    assert_eq!(added[..n], globe[..]);
    check(h, "add");

    scroll_to(h, MOVE_UP);
    tap(h, MOVE_UP);
    let moved = rivers_rule_names(h, Some(map));
    assert_eq!(moved[n - 1], added[n]);
    assert_eq!(moved[n], added[n - 1]);
    check(h, "move");

    scroll_to(h, DELETE_RULE);
    tap(h, DELETE_RULE);
    assert_eq!(rivers_rule_names(h, Some(map)), globe);
    check(h, "delete");
}

#[test]
fn a_rule_added_moved_and_deleted_in_a_frame_changes_only_that_map() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let globe = rivers_rule_names(&h, None);
    let main_text = own_rivers(&h, Target::Map(main));
    add_move_and_delete(&mut h, inset, &|h, what| {
        assert_eq!(rivers_rule_names(h, None), globe, "{what}");
        assert_eq!(rivers_rule_names(h, Some(main)), globe, "{what}");
        assert_eq!(own_rivers(h, Target::Map(main)), main_text, "{what}");
    });

    // In a linked map, the change is a local change of the map. Update map
    // style moves it into each map of the map style.
    let mut h = inset_pad(BOARDS[0]);
    add_frame(&mut h);
    let maps = [0, 1, 2].map(|i| frame(&h, i).map);
    let style = h.state_mut().save_as_map_style(maps[1]).unwrap();
    h.state_mut().set_map_style_link(maps[2], Some(style));
    h.state_mut().atlas.selection = AtlasSelection::Frame(frame(&h, 1).id);
    h.run_steps(2);
    add_move_and_delete(&mut h, maps[1], &|h, what| {
        assert_eq!(rivers_rule_names(h, Some(maps[2])), globe, "{what}");
        assert_eq!(rivers_rule_names(h, Some(maps[0])), globe, "{what}");
        assert_eq!(rivers_rule_names(h, None), globe, "{what}");
    });
    let rivers = layer_id(&h, "Rivers");
    h.state_mut().edit_map_style(maps[1], rivers, |s| {
        let kind = s.doc.layer(rivers).unwrap().geometry().unwrap();
        s.doc.entry_mut(rivers).unwrap().style.add_rule(kind);
    });
    assert_eq!(rivers_rule_names(&h, Some(maps[2])), globe);
    h.state_mut().update_map_style(maps[1]);
    let added = rivers_rule_names(&h, Some(maps[1]));
    assert_eq!(added.len(), globe.len() + 1);
    assert_eq!(rivers_rule_names(&h, Some(maps[2])), added);
    assert_eq!(rivers_rule_names(&h, Some(maps[0])), globe);
}

/// The names of the rules of Places in a map, or in the globe style.
fn places_rule_names(h: &Pad, map: Option<MapId>) -> Vec<String> {
    let places = layer_id(h, "Places");
    let doc = &h.state().doc;
    let style = match map {
        Some(map) => doc.map_style(map, places).unwrap(),
        None => doc.layer_style(places).unwrap(),
    };
    style.style.rules.iter().map(|r| r.name.clone()).collect()
}

/// Classifies Places by its population in the panel of the selected frame,
/// with quantiles.
fn classify_places_in_frame(h: &mut Pad) {
    use vale_app::ui::pad::classify::{CLASSIFY, EXPRESSION, REPLACE_RULES};
    tap(h, FRAME_TABS[2]);
    scroll_to(h, "Open Places");
    tap(h, "Open Places");
    scroll_to(h, CLASSIFY);
    tap(h, CLASSIFY);
    assert_eq!(
        h.state().atlas.layer_page,
        Some(vale_app::ui::pad::Page::Classify)
    );
    check_controls(h, "classify in a frame");
    scroll_to(h, EXPRESSION);
    type_text(h, EXPRESSION, "pop_max");
    scroll_to(h, "Quantiles");
    tap(h, "Quantiles");
    scroll_to(h, REPLACE_RULES);
    tap(h, REPLACE_RULES);
    assert_eq!(h.state().atlas.layer_page, None);
}

#[test]
fn classify_in_a_frame_changes_only_that_map() {
    let mut h = inset_pad(BOARDS[0]);
    let (main, inset) = (frame(&h, 0).map, frame(&h, 1).map);
    let globe = places_rule_names(&h, None);
    classify_places_in_frame(&mut h);
    let made = places_rule_names(&h, Some(inset));
    assert!((2..=5).contains(&made.len()), "{made:?}");
    assert_ne!(made, globe);
    assert_eq!(places_rule_names(&h, None), globe);
    assert_eq!(places_rule_names(&h, Some(main)), globe);

    // In a linked map, the change is a local change of the map. Update map
    // style moves it into each map of the map style.
    let mut h = inset_pad(BOARDS[0]);
    add_frame(&mut h);
    let maps = [0, 1, 2].map(|i| frame(&h, i).map);
    let style = h.state_mut().save_as_map_style(maps[1]).unwrap();
    h.state_mut().set_map_style_link(maps[2], Some(style));
    h.state_mut().atlas.selection = AtlasSelection::Frame(frame(&h, 1).id);
    h.run_steps(2);
    classify_places_in_frame(&mut h);
    let made = places_rule_names(&h, Some(maps[1]));
    assert_ne!(made, globe);
    assert_eq!(places_rule_names(&h, Some(maps[2])), globe);
    h.state_mut().update_map_style(maps[1]);
    assert_eq!(places_rule_names(&h, Some(maps[2])), made);
    assert_eq!(places_rule_names(&h, Some(maps[0])), globe);
    assert_eq!(places_rule_names(&h, None), globe);
}

/// Renames a map style on the Map style page: a swipe on its row, a tap on
/// Rename, the new text, and Return.
fn rename_by_swipe(h: &mut Pad, name: &str, text: &str) {
    use vale_app::ui::pad::widgets::RENAME;
    let row = map_style_row(name);
    find(h, &row);
    swipe_row(h, &row, -120.0);
    tap(h, &format!("{RENAME} {name}"));
    assert!(control(h, MAP_STYLE_NAME).is_some());
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    match text {
        "" => h.key_press(egui::Key::Backspace),
        text => h.event(egui::Event::Text(text.to_owned())),
    }
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert!(control(h, MAP_STYLE_NAME).is_none());
}

#[test]
fn the_name_of_a_map_style_takes_one_key_at_a_time() {
    use vale_app::ui::pad::widgets::RENAME;
    let mut h = styled_pad();
    let inset = frame(&h, 1).map;
    map_style_page_of(&mut h, 1);
    find(&mut h, SAVE_AS_MAP_STYLE);
    tap(&mut h, SAVE_AS_MAP_STYLE);
    // Return keeps the name that the new map style gets.
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    let style = linked(&h, inset).unwrap();
    let name = |h: &Pad| {
        h.state()
            .doc
            .project
            .atlas
            .map_style(style)
            .unwrap()
            .name
            .clone()
    };
    let before = name(&h);
    let row = map_style_row(&before);
    find(&mut h, &row);
    swipe_row(&mut h, &row, -120.0);
    tap(&mut h, &format!("{RENAME} {before}"));
    h.key_press(egui::Key::End);
    h.run_steps(1);
    for _ in 0..before.chars().count() {
        h.key_press(egui::Key::Backspace);
        h.run_steps(1);
    }
    // The name changes when the field loses the focus.
    for c in "Coast".chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run_steps(1);
    }
    assert_eq!(name(&h), before);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(name(&h), "Coast");
}

#[test]
fn save_as_map_style_asks_for_the_name() {
    let mut h = styled_pad();
    let inset = frame(&h, 1).map;
    map_style_page_of(&mut h, 1);
    find(&mut h, SAVE_AS_MAP_STYLE);
    tap(&mut h, SAVE_AS_MAP_STYLE);
    let style = linked(&h, inset).unwrap();
    // The row of the new map style is its name field, with the name
    // selected, so the first key replaces it.
    assert!(control(&h, MAP_STYLE_NAME).is_some());
    h.run_steps(1);
    h.event(egui::Event::Text("Coast".to_owned()));
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert!(control(&h, MAP_STYLE_NAME).is_none());
    let name = &h.state().doc.project.atlas.map_style(style).unwrap().name;
    assert_eq!(name, "Coast");
    assert!(control(&h, &map_style_row("Coast")).is_some());
}

/// A page with a main frame and an inset frame, and a new point layer with
/// a town at the middle of the inset. The layer is the selected layer, and
/// the Select tool is the tool.
fn towns_pad(size: (f32, f32)) -> (Pad, Pos2, vale_app::globe::select::FeatureRef) {
    towns_pad_with(state(), size)
}

/// `towns_pad` from a state, for example a state with a session.
fn towns_pad_with(
    state: AppState,
    size: (f32, f32),
) -> (Pad, Pos2, vale_app::globe::select::FeatureRef) {
    use vale_app::globe::select::FeatureRef;
    use vale_app::ui::LayerRef;
    let mut h = pad_with(state, size);
    add_frame(&mut h);
    add_frame(&mut h);
    let at = middle(&h, 1);
    let lonlat = place_at(&mut h, 1, at);
    let s = h.state_mut();
    let layer = s.doc.add_layer(GeometryKind::Point);
    let town = Feature::new(Geometry::Points(vec![lonlat]), Default::default());
    let found = FeatureRef {
        layer,
        uuid: town.uuid(),
    };
    assert!(s.doc.project.add_feature(layer, town));
    s.pipeline.forget(layer);
    s.touch();
    s.select(Some(LayerRef::Vector(layer)));
    h.run_steps(2);
    tap(&mut h, SELECT_TOOL);
    assert_eq!(h.state().atlas.tool, AtlasTool::Select);
    let at = pos_of(&mut h, 1, lonlat);
    (h, at, found)
}

/// Adds a point to the layer of `found` at a screen position in a frame.
fn add_town(
    h: &mut Pad,
    layer: LayerId,
    index: usize,
    pos: Pos2,
) -> vale_app::globe::select::FeatureRef {
    let lonlat = place_at(h, index, pos);
    let s = h.state_mut();
    let town = Feature::new(Geometry::Points(vec![lonlat]), Default::default());
    let found = vale_app::globe::select::FeatureRef {
        layer,
        uuid: town.uuid(),
    };
    assert!(s.doc.project.add_feature(layer, town));
    s.pipeline.forget(layer);
    s.touch();
    h.run_steps(2);
    found
}

#[test]
fn with_the_select_tool_a_tap_selects_the_feature_in_reach_and_its_frame() {
    for (input, near, far) in [
        (Input::Finger, 20.0, 25.0),
        (Input::Pen, 10.0, 15.0),
        (Input::Mouse, 6.0, 11.0),
    ] {
        let (mut h, at, town) = towns_pad(BOARDS[0]);
        let inset = frame(&h, 1).id;
        tap_with(&mut h, at + vec2(near, 0.0), input);
        assert_eq!(h.state().feature_selection, vec![town], "{input:?}");
        assert_eq!(
            h.state().atlas.selection,
            AtlasSelection::Frame(inset),
            "{input:?}"
        );
        tap_with(&mut h, at + vec2(0.0, far), input);
        assert!(h.state().feature_selection.is_empty(), "{input:?}");
        assert_eq!(
            h.state().atlas.selection,
            AtlasSelection::Frame(inset),
            "{input:?}"
        );
    }
}

#[test]
fn with_the_select_tool_a_layer_that_the_map_hides_has_no_hit() {
    let (mut h, at, town) = towns_pad(BOARDS[0]);
    let map = frame(&h, 1).map;
    h.state_mut().set_map_shown(map, town.layer, false);
    h.run_steps(2);
    tap_with(&mut h, at, Input::Finger);
    assert!(h.state().feature_selection.is_empty());
}

#[test]
fn with_the_select_tool_a_tap_outside_the_frames_clears_the_selection() {
    let (mut h, at, town) = towns_pad(BOARDS[0]);
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(h.state().feature_selection, vec![town]);
    let off = margin(&h);
    tap_with(&mut h, off, Input::Finger);
    assert!(h.state().feature_selection.is_empty());
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
}

#[test]
fn with_the_select_tool_a_pen_or_a_mouse_selects_in_a_box_and_a_finger_pans() {
    for input in [Input::Pen, Input::Mouse] {
        let (mut h, at, town) = towns_pad(BOARDS[0]);
        let other = add_town(&mut h, town.layer, 1, at + vec2(30.0, 20.0));
        drag(&mut h, at - vec2(15.0, 15.0), at + vec2(45.0, 35.0), input);
        let mut selected = h.state().feature_selection.clone();
        selected.sort_by_key(|f| f.uuid);
        let mut both = vec![town, other];
        both.sort_by_key(|f| f.uuid);
        assert_eq!(selected, both, "{input:?}");
        assert_eq!(
            h.state().atlas.selection,
            AtlasSelection::Frame(frame(&h, 1).id),
            "{input:?}"
        );
    }
    let (mut h, at, town) = towns_pad(BOARDS[0]);
    tap_with(&mut h, at, Input::Finger);
    let offset = h.state().atlas.view.offset;
    drag(
        &mut h,
        at - vec2(15.0, 15.0),
        at + vec2(45.0, 35.0),
        Input::Finger,
    );
    assert_ne!(h.state().atlas.view.offset, offset);
    assert_eq!(h.state().feature_selection, vec![town]);
}

#[test]
fn the_selection_shows_in_each_frame_that_shows_the_layer_cut_at_the_frames() {
    use vale_app::ui::LayerRef;
    let (mut h, at, _) = towns_pad(BOARDS[0]);
    tap_with(&mut h, at, Input::Finger);
    let place = place(&h);
    let shown = atlas::features::highlight(h.state_mut(), place);
    assert!(!shown.points.is_empty());
    // A river from the town to far outside the inset.
    let start = place_at(&mut h, 1, at);
    let s = h.state_mut();
    let rivers = s.doc.add_layer(GeometryKind::Line);
    let line = Feature::new(
        Geometry::Lines(vec![vec![start, [start[0] + 60.0, start[1]]]]),
        Default::default(),
    );
    let river = vale_app::globe::select::FeatureRef {
        layer: rivers,
        uuid: line.uuid(),
    };
    assert!(s.doc.project.add_feature(rivers, line));
    s.pipeline.forget(rivers);
    s.touch();
    s.select(Some(LayerRef::Vector(rivers)));
    s.set_feature_selection(vec![river]);
    h.run_steps(2);
    let shown = atlas::features::highlight(h.state_mut(), place);
    assert!(!shown.lines.is_empty());
    let page = open_page(&h);
    let inside = |p: Pos2| {
        let mm = place.to_mm(p);
        // A point on the edge of a frame is in the frame.
        page.frames.iter().any(|f| {
            let at = |d: f64| vale_app::atlas::contains(&f.shape, mm + kurbo::Vec2::new(d, d));
            at(0.0) || at(0.05) || at(-0.05)
        })
    };
    for line in &shown.lines {
        for p in line {
            assert!(inside(*p), "{p:?}");
        }
    }
    for map in page.frames.iter().map(|f| f.map) {
        h.state_mut().set_map_shown(map, rivers, false);
    }
    h.run_steps(2);
    let shown = atlas::features::highlight(h.state_mut(), place);
    assert!(shown.lines.is_empty());
}

#[test]
fn the_selection_of_the_atlas_is_the_selection_of_the_world() {
    let (mut h, at, town) = towns_pad(BOARDS[0]);
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(h.state().feature_selection, vec![town]);
    tap(&mut h, "World");
    assert_eq!(h.state().workspace, Workspace::Globe);
    h.run_steps(2);
    assert_eq!(h.state().feature_selection, vec![town]);
}

#[test]
fn with_the_select_tool_the_context_strip_has_deselect_only() {
    let (mut h, at, town) = towns_pad(BOARDS[0]);
    tap_with(&mut h, at, Input::Finger);
    assert!(control(&h, DESELECT).is_some());
    for name in [SNAP_TO_LAYOUT, DELETE_SELECTION, DELETE_VERTEX] {
        assert!(control(&h, name).is_none(), "{name}");
    }
    let frames = open_page(&h).frames.len();
    for key in [egui::Key::Delete, egui::Key::Backspace] {
        h.key_press(key);
        h.run_steps(2);
    }
    assert_eq!(open_page(&h).frames.len(), frames);
    assert_eq!(h.state().feature_selection, vec![town]);
    let layer = h.state().doc.layer(town.layer).unwrap();
    assert!(layer.features.iter().any(|f| f.uuid() == town.uuid));
    let inset = frame(&h, 1).id;
    tap(&mut h, DESELECT);
    assert!(h.state().feature_selection.is_empty());
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(inset));
    tap(&mut h, DESELECT);
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
    assert!(control(&h, DESELECT).is_none());
}

#[test]
fn a_row_of_the_layers_tab_selects_its_vector_layer() {
    let (mut h, _, town) = towns_pad(BOARDS[0]);
    h.state_mut().select(None);
    tap(&mut h, FRAME_TOOL);
    let inset = middle(&h, 1);
    tap_with(&mut h, inset, Input::Finger);
    tap(&mut h, FRAME_TABS[2]);
    let name = h.state().doc.layer(town.layer).unwrap().name.clone();
    tap(&mut h, &format!("Layer {name}"));
    assert_eq!(h.state().selected_vector(), Some(town.layer));
}

#[test]
fn the_frame_tool_does_not_box_select_features() {
    let (mut h, at, _) = towns_pad(BOARDS[0]);
    tap(&mut h, FRAME_TOOL);
    let off = margin(&h);
    drag(&mut h, off, at + vec2(10.0, 10.0), Input::Pen);
    assert!(h.state().feature_selection.is_empty());
}

/// The value of the field `name` of a feature, as text.
fn attribute(h: &Pad, f: vale_app::globe::select::FeatureRef, name: &str) -> String {
    let layer = h.state().doc.layer(f.layer).unwrap();
    let feature = layer.features.iter().find(|x| x.uuid() == f.uuid).unwrap();
    let value = feature.attributes.get(name).and_then(|v| v.label());
    value.unwrap_or_default()
}

#[test]
fn a_tap_of_the_select_tool_shows_the_inspector_of_the_feature() {
    use vale_app::ui::pad::atlas::ATTRIBUTES_HINT;
    let (mut h, at, town) = towns_pad(BOARDS[0]);
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(h.state().feature_selection, vec![town]);
    let layer = h.state().doc.layer(town.layer).unwrap().name.clone();
    assert!(h.query_by_label(ATTRIBUTES_HINT).is_some());
    assert!(h.query_all_by_label(&layer).count() >= 1);
    assert!(control(&h, "name").is_some());
    assert!(control(&h, FRAME_TABS[0]).is_none());
    // Back goes to the panel of the frame, with its tabs.
    let map = map_of(&h, 1);
    tap(&mut h, &format!("Back to {map}"));
    assert!(control(&h, "name").is_none());
    assert!(control(&h, FRAME_TABS[0]).is_some());
    assert_eq!(h.state().feature_selection, vec![town]);
}

#[test]
fn an_attribute_edit_in_the_atlas_is_one_undo_step_and_redraws_the_frames() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = state();
    s.session = Some(Session::open(&dir.path().join("project.gpkg"), &mut s.doc).unwrap());
    let (mut h, at, town) = towns_pad_with(s, BOARDS[0]);
    h.state_mut().save_waiting().unwrap();
    tap_with(&mut h, at, Input::Finger);
    let generation = h.state().map_generation;
    type_text(&mut h, "name", "Avalon");
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(attribute(&h, town, "name"), "Avalon");
    assert_ne!(h.state().map_generation, generation);
    h.state_mut().save_waiting().unwrap();
    h.state_mut().undo();
    h.run_steps(2);
    assert_eq!(attribute(&h, town, "name"), "");
}

#[test]
fn the_selection_of_the_atlas_shows_in_the_inspector_of_the_world() {
    let (mut h, at, town) = towns_pad(BOARDS[0]);
    tap_with(&mut h, at, Input::Finger);
    let form = h.state().selection_form.as_ref().unwrap();
    assert_eq!(form.feature, town);
    h.state_mut().workspace = Workspace::Globe;
    h.run_steps(3);
    assert_eq!(h.state().feature_selection, vec![town]);
    tap(&mut h, vale_app::ui::select::ATTRIBUTES);
    let form = h.state().selection_form.as_ref().unwrap();
    assert_eq!(form.feature, town);
    assert!(control(&h, "name").is_some());
}

#[test]
fn the_features_row_of_the_layers_tab_opens_the_attribute_table() {
    use vale_app::ui::pad::Page;
    use vale_app::ui::pad::table::FEATURES;
    use vale_app::ui::table::cell_name;
    let (mut h, at, town) = towns_pad(BOARDS[0]);
    let other = add_town(&mut h, town.layer, 1, at + vec2(30.0, 20.0));
    tap(&mut h, FRAME_TOOL);
    let inset = middle(&h, 1);
    tap_with(&mut h, inset, Input::Finger);
    tap(&mut h, FRAME_TABS[2]);
    let name = h.state().doc.layer(town.layer).unwrap().name.clone();
    tap(&mut h, &format!("Layer {name}"));
    tap(&mut h, FEATURES);
    assert_eq!(h.state().atlas.layer_page, Some(Page::Table));
    assert_eq!(h.state().table.as_ref().unwrap().layer, town.layer);
    // A tap on a row selects its feature, and the frames show it.
    tap(&mut h, &cell_name("name", town.uuid));
    assert_eq!(h.state().feature_selection, vec![town]);
    // The table follows a selection that the table did not make.
    h.state_mut().set_feature_selection(vec![other]);
    h.run_steps(2);
    let seen = h.state().table_selection();
    assert!(seen.contains(&other.uuid) && !seen.contains(&town.uuid));
    // Back goes to the page of the layer, and the table closes.
    tap(&mut h, &format!("Back to {name}"));
    assert_eq!(h.state().atlas.layer_page, None);
    assert!(h.state().table.is_none());
    assert!(control(&h, FEATURES).is_some());
}

#[test]
fn show_in_table_opens_the_table_in_the_layers_tab_of_the_frame() {
    use vale_app::ui::pad::Page;
    use vale_app::ui::table::SHOW_IN_TABLE;
    let (mut h, at, town) = towns_pad(BOARDS[0]);
    let lonlat = place_at(&mut h, 1, at);
    tap_with(&mut h, at, Input::Finger);
    tap(&mut h, SHOW_IN_TABLE);
    let s = h.state();
    assert_eq!(s.atlas.tab, FrameTab::Layers);
    assert_eq!(s.atlas.layer_page, Some(Page::Table));
    assert_eq!(s.atlas.open_layer(), Some(town.layer));
    assert_eq!(s.table.as_ref().unwrap().layer, town.layer);
    assert!(!s.atlas.attributes);
    // A tap on the town keeps the table.
    let at = pos_of(&mut h, 1, lonlat);
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(h.state().feature_selection, vec![town]);
    assert_eq!(h.state().atlas.layer_page, Some(Page::Table));
    assert!(h.state().table.is_some());
}

/// `towns_pad` with a second point layer, Wells, above Towns, and a well
/// 30 points right of the town. Towns stays the active layer.
fn wells_pad() -> (
    Pad,
    Pos2,
    vale_app::globe::select::FeatureRef,
    vale_app::globe::select::FeatureRef,
) {
    let (mut h, at, town) = towns_pad(BOARDS[0]);
    let wells = h.state_mut().doc.add_layer(GeometryKind::Point);
    let well = add_town(&mut h, wells, 1, at + vec2(30.0, 0.0));
    assert_eq!(h.state().selected_vector(), Some(town.layer));
    (h, at, town, well)
}

#[test]
fn by_default_the_atlas_select_tool_selects_in_the_active_layer() {
    let (mut h, at, town, well) = wells_pad();
    tap_with(&mut h, at + vec2(30.0, 0.0), Input::Finger);
    assert!(h.state().feature_selection.is_empty());
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(h.state().feature_selection, vec![town]);
    let _ = well;
}

#[test]
fn with_checked_layers_the_atlas_select_tool_selects_in_each_checked_layer() {
    let (mut h, at, town, well) = wells_pad();
    h.state_mut().toggle_selectable(town.layer);
    h.state_mut().toggle_selectable(well.layer);
    tap_with(&mut h, at + vec2(30.0, 0.0), Input::Finger);
    assert_eq!(h.state().feature_selection, vec![well]);
    // A selection in one layer makes that layer the active layer.
    assert_eq!(h.state().selected_vector(), Some(well.layer));
    let place = place(&h);
    let one = atlas::features::highlight(h.state_mut(), place)
        .points
        .len();
    assert!(one > 0);
    drag(
        &mut h,
        at - vec2(15.0, 15.0),
        at + vec2(45.0, 15.0),
        Input::Pen,
    );
    let mut selected = h.state().feature_selection.clone();
    selected.sort_by_key(|f| (f.layer, f.uuid));
    let mut both = vec![town, well];
    both.sort_by_key(|f| (f.layer, f.uuid));
    assert_eq!(selected, both);
    assert_eq!(h.state().selected_vector(), Some(well.layer));
    // The highlight shows the features of both layers.
    let shown = atlas::features::highlight(h.state_mut(), place);
    assert_eq!(shown.points.len(), 2 * one);
    // A layer that the map hides has no hit.
    let map = frame(&h, 1).map;
    h.state_mut().set_map_shown(map, well.layer, false);
    h.run_steps(2);
    tap_with(&mut h, at + vec2(30.0, 0.0), Input::Finger);
    assert!(h.state().feature_selection.is_empty());
}

#[test]
fn the_selectable_list_of_the_select_tool_chooses_the_layers() {
    use vale_app::ui::pad::selectable::{
        ACTIVE_LAYER, DESELECT_ALL, SELECT_ALL, SELECTABLE, layer_row_name,
    };
    use vale_app::ui::select::Selectable;
    let (mut h, at, town) = towns_pad(BOARDS[0]);
    tap(&mut h, SELECTABLE);
    assert!(h.state().pad.selectable);
    let name = h.state().doc.layer(town.layer).unwrap().name.clone();
    let row = layer_row_name(&name);
    for name in [ACTIVE_LAYER, SELECT_ALL, DESELECT_ALL, row.as_str()] {
        let rect = control(&h, name).unwrap_or_else(|| panic!("no row {name}"));
        assert!(rect.height() >= 44.0, "{name}");
    }
    tap(&mut h, DESELECT_ALL);
    assert!(h.state().selectable_layers().is_empty());
    assert!(h.state().pad.selectable, "the list stays open");
    // A tap outside the list closes it.
    tap_with(&mut h, at, Input::Finger);
    assert!(!h.state().pad.selectable);
    tap_with(&mut h, at, Input::Finger);
    assert!(h.state().feature_selection.is_empty());
    tap(&mut h, SELECTABLE);
    tap(&mut h, &row);
    assert!(h.state().selectable_checked(town.layer));
    screenshot::save(&mut h, "pad-atlas-selectable");
    tap_with(&mut h, at, Input::Finger);
    tap_with(&mut h, at, Input::Finger);
    assert_eq!(h.state().feature_selection, vec![town]);
    tap(&mut h, SELECTABLE);
    tap(&mut h, ACTIVE_LAYER);
    assert_eq!(h.state().selectable, Selectable::Active);
    tap(&mut h, SELECT_ALL);
    assert!(h.state().selectable_checked(town.layer));
}

#[test]
fn the_selectable_cell_and_list_fit_in_the_atlas_on_each_screen() {
    use vale_app::ui::pad::selectable::{ACTIVE_LAYER, SELECTABLE};
    let phones = [(393.0, 852.0), (852.0, 393.0)];
    for size in BOARDS.into_iter().chain(phones) {
        let what = format!("{size:?}");
        let (mut h, _, _) = towns_pad(size);
        let screen = Rect::from_min_size(Pos2::ZERO, size.into());
        let cell = control(&h, SELECTABLE).unwrap_or_else(|| panic!("{what}"));
        assert!(screen.contains_rect(cell), "{what}");
        check_controls(&h, &what);
        tap(&mut h, SELECTABLE);
        assert!(h.state().pad.selectable, "{what}");
        let row = control(&h, ACTIVE_LAYER).unwrap_or_else(|| panic!("{what}"));
        assert!(screen.contains_rect(row), "{what}");
        assert!(!row.intersects(cell), "{what}");
    }
}
