//! The attribute table of the world workspace.

mod screenshot;

use std::collections::BTreeMap;

use eframe::egui::{self, Pos2, Rect, vec2};
use egui_kittest::Harness;
use vale_app::document::Document;
use vale_app::globe::select::FeatureRef;
use vale_app::globe::{Tool, WorldView};
use vale_app::session::Session;
use vale_app::ui::pad::LayerRef;
use vale_app::ui::pad::Panel;
use vale_app::ui::pad::table::FEATURES;
use vale_app::ui::table::{ALL, FILTER, SELECTED, SHOW_IN_TABLE, cell_name, sort_name};
use vale_app::ui::{AppState, Layout, draw};
use vale_store::{Feature, Field, FieldType, Geometry, GeometryKind, LayerId, Uuid, Value};

type Pad = Harness<'static, AppState>;

const LANDSCAPE: (f32, f32) = (1194.0, 834.0);
const PORTRAIT: (f32, f32) = (834.0, 1194.0);

fn harness(size: (f32, f32), layout: Layout) -> Pad {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.set_layout(layout);
    let mut h = Harness::builder()
        .with_size(vec2(size.0, size.1))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(3);
    h
}

/// A point layer of `count` towns with a name and a rank, hidden from the
/// other layers. Town `front` is at 0°, 0°, and the others are on the far
/// side of the globe. The rank of town `i` is `i % 5`.
fn towns(h: &mut Pad, count: usize, front: usize) -> (LayerId, Vec<Uuid>) {
    let state = h.state_mut();
    let shown: Vec<_> = state.doc.project.layers().iter().map(|l| l.id).collect();
    for id in shown {
        state.doc.project.set_layer_visible(id, false);
    }
    let id = state.doc.add_layer(GeometryKind::Point);
    let project = &mut state.doc.project;
    project.add_field(id, Field::new("rank", FieldType::Integer));
    let mut uuids = Vec::with_capacity(count);
    for i in 0..count {
        let place = if i == front { [0.0, 0.0] } else { [180.0, 0.0] };
        let attributes = BTreeMap::from([
            ("name".to_string(), Value::Text(format!("Town {i:05}"))),
            ("rank".to_string(), Value::Number((i % 5) as f64)),
        ]);
        let feature = Feature::new(Geometry::Points(vec![place]), attributes);
        uuids.push(feature.uuid());
        assert!(project.add_feature(id, feature));
    }
    state.select(Some(LayerRef::Vector(id)));
    h.run_steps(2);
    (id, uuids)
}

/// Adds a project file. The file holds each change.
fn save(h: &mut Pad, dir: &std::path::Path) {
    let state = h.state_mut();
    state.session = Some(Session::open(&dir.join("w.gpkg"), &mut state.doc).unwrap());
    state.save_waiting().unwrap();
    h.run_steps(2);
}

fn saves(h: &Pad) -> u64 {
    h.state().session.as_ref().unwrap().saves()
}

fn control(h: &Pad, name: &str) -> Option<Rect> {
    let controls = &h.state().pad.controls;
    controls.iter().find(|c| c.name == name).map(|c| c.rect)
}

fn touch(h: &mut Pad, phase: egui::TouchPhase, pos: Pos2) {
    touch_with(h, phase, pos, None);
}

/// A touch with a force is a pen.
fn touch_with(h: &mut Pad, phase: egui::TouchPhase, pos: Pos2, force: Option<f32>) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(1),
        phase,
        pos,
        force,
    });
    h.event(egui::Event::PointerMoved(pos));
    if phase != egui::TouchPhase::Move {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: phase == egui::TouchPhase::Start,
            modifiers: egui::Modifiers::NONE,
        });
    }
    if phase == egui::TouchPhase::End {
        h.event(egui::Event::PointerGone);
    }
}

fn tap_at(h: &mut Pad, pos: Pos2) {
    touch(h, egui::TouchPhase::Start, pos);
    h.step();
    touch(h, egui::TouchPhase::End, pos);
    h.run_steps(3);
}

/// Opens or closes a panel with its tile in the Panels menu.
fn open_panel(h: &mut Pad, name: &str) {
    tap(h, vale_app::ui::pad::PANELS);
    tap(h, name);
}

fn tap(h: &mut Pad, name: &str) {
    let rect = control(h, name).unwrap_or_else(|| panic!("no control `{name}`"));
    tap_at(h, rect.center());
}

/// Types a text in the focused field and ends the edit with Return.
fn type_enter(h: &mut Pad, text: &str) {
    h.event(egui::Event::Text(text.into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
}

fn rank(h: &Pad, id: LayerId, uuid: Uuid) -> Value {
    let layer = h.state().doc.project.layer(id).unwrap();
    let f = layer.features.iter().find(|f| f.uuid() == uuid).unwrap();
    f.attributes["rank"].clone()
}

fn refs(id: LayerId, uuids: &[Uuid]) -> Vec<FeatureRef> {
    uuids
        .iter()
        .map(|&uuid| FeatureRef { layer: id, uuid })
        .collect()
}

fn rows(h: &Pad) -> Vec<usize> {
    h.state().table.as_ref().unwrap().rows.rows().to_vec()
}

fn rebuilds(h: &Pad) -> u64 {
    h.state().table.as_ref().unwrap().rows.rebuilds()
}

/// The Layers panel in the iPad layout, and the dock on the desktop.
fn table_rect(h: &Pad) -> Rect {
    if h.state().layout == Layout::Pad {
        let panels = &h.state().pad.panels;
        let layers = panels.iter().find(|(p, _)| *p == Panel::Layers);
        return layers.map(|(_, r)| *r).expect("no Layers panel");
    }
    h.state()
        .table
        .as_ref()
        .and_then(|t| t.rect)
        .expect("no table")
}

/// The card with the controls of the Select tool: the row above the tool
/// strip, or the tool column.
fn select_card(h: &Pad) -> Rect {
    let rects = h.state().pad.rects.as_ref().expect("no layout");
    rects.brush.unwrap_or(rects.tools)
}

/// The center of the globe on the canvas, in the Select tool.
fn select_tool(h: &mut Pad) -> Pos2 {
    h.state_mut().globe.tool = Tool::Select;
    h.run_steps(2);
    let dir = vale_app::globe::math::lonlat_to_dir(0.0, 0.0);
    let at = h.state().globe.project(dir).expect("0°, 0° does not show");
    let center = h.state().globe.rect.center() - vec2(0.0, 80.0);
    h.state_mut().globe.move_place(at, center);
    h.run_steps(2);
    h.state().globe.project(dir).unwrap()
}

#[test]
fn the_table_sorts_and_filters_fifty_thousand_rows_and_builds_them_only_on_a_change() {
    let mut h = harness(LANDSCAPE, Layout::Pad);
    let (id, uuids) = towns(&mut h, 50_000, 0);
    h.state_mut().open_table(id);
    h.run_steps(3);
    assert_eq!(rows(&h).len(), 50_000);
    // Frames with no change do not build the rows again.
    let built = rebuilds(&h);
    h.run_steps(5);
    assert_eq!(rebuilds(&h), built);
    // Only the rows that show are drawn.
    let cells = h
        .state()
        .pad
        .controls
        .iter()
        .filter(|c| c.name.ends_with(&uuids[0].to_string()));
    assert_eq!(cells.count(), 2);
    assert!(h.state().pad.controls.len() < 200);

    tap(&mut h, &sort_name("rank"));
    assert_eq!(rebuilds(&h), built + 1);
    let rank_of = |h: &Pad, row: usize| {
        let layer = h.state().doc.project.layer(id).unwrap();
        layer.features[rows(h)[row]].attributes["rank"].clone()
    };
    assert_eq!(rank_of(&h, 0), Value::Number(0.0));
    assert_eq!(rank_of(&h, 49_999), Value::Number(4.0));
    tap(&mut h, &sort_name("rank"));
    assert_eq!(rank_of(&h, 0), Value::Number(4.0));

    tap(&mut h, FILTER);
    type_enter(&mut h, "rank = 3 and name like 'Town 1%'");
    let filtered = rows(&h);
    assert_eq!(filtered.len(), 2_000);
    let layer = h.state().doc.project.layer(id).unwrap();
    assert!(
        filtered
            .iter()
            .all(|&i| layer.features[i].attributes["rank"] == Value::Number(3.0))
    );
}

#[test]
fn an_edit_of_a_selected_row_sets_the_field_of_all_selected_rows_in_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(LANDSCAPE, Layout::Pad);
    let (id, uuids) = towns(&mut h, 30, 0);
    save(&mut h, dir.path());
    h.state_mut().open_table(id);
    let twelve: Vec<Uuid> = uuids.iter().copied().step_by(2).take(12).collect();
    h.state_mut().set_feature_selection(refs(id, &twelve));
    h.run_steps(3);
    tap(&mut h, SELECTED);
    assert_eq!(rows(&h).len(), 12);
    let saved = saves(&h);
    tap(&mut h, &cell_name("rank", twelve[3]));
    assert!(h.state().table.as_ref().unwrap().edit.is_some());
    type_enter(&mut h, "7");
    assert!(h.state().table.as_ref().unwrap().edit.is_none());
    assert_eq!(saves(&h), saved + 1);
    for &uuid in &twelve {
        assert_eq!(rank(&h, id, uuid), Value::Number(7.0));
    }
    assert_eq!(rank(&h, id, uuids[1]), Value::Number(1.0));
    h.state_mut().undo();
    h.run_steps(2);
    for (i, &uuid) in uuids.iter().enumerate().step_by(2).take(12) {
        assert_eq!(rank(&h, id, uuid), Value::Number((i % 5) as f64));
    }
    // A value that is not a number of the field changes nothing.
    let saved = saves(&h);
    tap(&mut h, &cell_name("rank", twelve[3]));
    type_enter(&mut h, "many");
    assert_eq!(saves(&h), saved);
    assert_eq!(rank(&h, id, twelve[3]), Value::Number(1.0));
}

#[test]
fn all_and_selected_work_together_with_the_filter() {
    let mut h = harness(LANDSCAPE, Layout::Pad);
    let (id, uuids) = towns(&mut h, 20, 0);
    h.state_mut().open_table(id);
    h.state_mut().set_feature_selection(refs(id, &uuids[..6]));
    h.run_steps(3);
    assert!(!h.state().table.as_ref().unwrap().selected_only);
    assert_eq!(rows(&h).len(), 20);
    tap(&mut h, FILTER);
    type_enter(&mut h, "rank >= 3");
    assert_eq!(rows(&h).len(), 8);
    tap(&mut h, SELECTED);
    assert_eq!(rows(&h), [3, 4]);
    tap(&mut h, ALL);
    assert_eq!(rows(&h).len(), 8);
}

#[test]
fn the_selection_of_the_table_is_the_selection_of_the_globe_and_the_flat_view() {
    let mut h = harness(LANDSCAPE, Layout::Pad);
    let (id, uuids) = towns(&mut h, 20, 0);
    h.state_mut().open_table(id);
    h.run_steps(3);
    // A selection on the globe is the selection of the table.
    let center = select_tool(&mut h);
    tap_at(&mut h, center);
    assert_eq!(h.state().feature_selection, refs(id, &uuids[..1]));
    assert_eq!(h.state().table_selection(), [uuids[0]].into());
    tap(&mut h, &cell_name("name", uuids[2]));
    assert_eq!(h.state().feature_selection, refs(id, &uuids[2..3]));
    for view in [WorldView::Flat, WorldView::Globe] {
        h.state_mut().globe.world_view = view;
        h.run_steps(3);
        assert_eq!(h.state().feature_selection, refs(id, &uuids[2..3]));
    }
}

#[test]
fn a_tap_on_the_globe_scrolls_the_table_to_the_row_and_the_next_tap_edits_it() {
    let mut h = harness(LANDSCAPE, Layout::Pad);
    let (id, uuids) = towns(&mut h, 5_000, 4_000);
    h.state_mut().open_table(id);
    h.run_steps(3);
    let cell = cell_name("rank", uuids[4_000]);
    assert!(control(&h, &cell).is_none());
    let center = select_tool(&mut h);
    tap_at(&mut h, center);
    assert_eq!(h.state().feature_selection, refs(id, &uuids[4_000..4_001]));
    assert!(control(&h, &cell).is_some());
    tap(&mut h, &cell);
    type_enter(&mut h, "9");
    assert_eq!(rank(&h, id, uuids[4_000]), Value::Number(9.0));
}

#[test]
fn a_new_point_is_the_selected_row_of_the_table() {
    let mut h = harness(LANDSCAPE, Layout::Pad);
    let (id, _) = towns(&mut h, 3, 0);
    h.state_mut().open_table(id);
    h.state_mut().globe.tool = Tool::Point;
    h.state_mut().globe.snap.on = false;
    h.run_steps(2);
    let at = h.state().globe.rect.center() + vec2(0.0, -120.0);
    touch_with(&mut h, egui::TouchPhase::Start, at, Some(0.5));
    h.step();
    touch_with(&mut h, egui::TouchPhase::End, at, Some(0.5));
    h.run_steps(3);
    let layer = h.state().doc.project.layer(id).unwrap();
    assert_eq!(layer.features.len(), 4, "no point");
    let new = layer.features[3].uuid();
    assert_eq!(h.state().feature_selection, refs(id, &[new]));
    assert_eq!(h.state().table_selection(), [new].into());
    assert!(control(&h, &cell_name("rank", new)).is_some());
}

#[test]
fn the_page_of_a_vector_layer_and_the_selection_card_open_the_table() {
    let mut h = harness(LANDSCAPE, Layout::Pad);
    let (id, uuids) = towns(&mut h, 5, 0);
    let name = h.state().doc.project.layer(id).unwrap().name.clone();
    open_panel(&mut h, "Layers");
    tap(&mut h, &format!("Open {name}"));
    tap(&mut h, FEATURES);
    assert_eq!(h.state().table.as_ref().map(|t| t.layer), Some(id));
    assert!(control(&h, &cell_name("rank", uuids[1])).is_some());
    // The table is a page of the layer, and Back closes it.
    tap(&mut h, &format!("Back to {name}"));
    assert!(h.state().table.is_none());
    assert!(control(&h, FEATURES).is_some());

    let center = select_tool(&mut h);
    tap_at(&mut h, center);
    open_panel(&mut h, vale_app::ui::select::ATTRIBUTES);
    tap(&mut h, SHOW_IN_TABLE);
    let table = h.state().table.as_ref().expect("no table");
    assert_eq!(table.layer, id);
    assert!(control(&h, &cell_name("rank", uuids[0])).is_some());
}

#[test]
fn on_ipad_the_table_is_a_wide_right_panel_beside_the_select_card() {
    use vale_app::ui::pad::dock_ui::{close_name, move_name};
    for size in [LANDSCAPE, PORTRAIT] {
        let mut h = harness(size, Layout::Pad);
        let (id, uuids) = towns(&mut h, 5, 0);
        // The open pane moves the globe, so it opens first.
        open_panel(&mut h, "Layers");
        let center = select_tool(&mut h);
        let normal = table_rect(&h);
        h.state_mut().open_table(id);
        tap_at(&mut h, center);
        let (card, table) = (select_card(&h), table_rect(&h));
        assert!(!card.intersects(table), "{size:?}: {card:?} {table:?}");
        // The panel keeps its top and its right edge, and it is wider.
        assert_eq!((table.top(), table.right()), (normal.top(), normal.right()));
        assert!(table.width() > normal.width(), "{size:?}");
        // The edit of a cell moves neither the panel nor the card.
        tap(&mut h, &cell_name("rank", uuids[0]));
        assert!(h.state().table.as_ref().unwrap().edit.is_some());
        assert_eq!((select_card(&h), table_rect(&h)), (card, table), "{size:?}");
        // The table closes with the panel, which closes with the menu of its
        // title, and the panel is narrow again.
        let title = control(&h, &move_name("Layers")).unwrap().center();
        touch(&mut h, egui::TouchPhase::Start, title);
        h.run_steps(4);
        touch(&mut h, egui::TouchPhase::End, title);
        h.run_steps(3);
        tap(&mut h, &close_name("Layers"));
        assert!(h.state().table.is_none());
        open_panel(&mut h, "Layers");
        assert_eq!(table_rect(&h).width(), normal.width());
    }
}

#[test]
fn on_the_desktop_the_table_is_in_a_dock_below_the_canvas() {
    let mut h = harness((1440.0, 900.0), Layout::Desktop);
    let (id, _) = towns(&mut h, 5, 0);
    h.state_mut().open_table(id);
    h.run_steps(3);
    let table = table_rect(&h);
    let canvas = h.state().globe.rect;
    assert!(table.top() >= canvas.bottom() - 1.0, "{table:?} {canvas:?}");
    assert!(table.width() > 0.5 * canvas.width());
    assert!(table.bottom() < 900.0);
}

#[test]
fn the_controls_of_the_table_are_touch_size_and_a_drag_scrolls_it() {
    let mut h = harness(LANDSCAPE, Layout::Pad);
    let (id, uuids) = towns(&mut h, 200, 0);
    h.state_mut().open_table(id);
    h.run_steps(3);
    let table = table_rect(&h);
    let inside: Vec<_> = h
        .state()
        .pad
        .controls
        .iter()
        .filter(|c| table.contains_rect(c.rect))
        .cloned()
        .collect();
    assert!(inside.len() > 10);
    for c in &inside {
        assert!(
            c.rect.width() >= 44.0 && c.rect.height() >= 44.0,
            "{}",
            c.name
        );
    }
    // A drag of a finger on the rows scrolls them and selects no row.
    let first = cell_name("name", uuids[0]);
    let from = control(&h, &cell_name("name", uuids[3])).unwrap().center();
    touch(&mut h, egui::TouchPhase::Start, from);
    h.step();
    for i in 1..=8 {
        touch(
            &mut h,
            egui::TouchPhase::Move,
            from - vec2(0.0, 25.0 * i as f32),
        );
        h.step();
    }
    touch(&mut h, egui::TouchPhase::End, from - vec2(0.0, 200.0));
    h.run_steps(3);
    assert!(control(&h, &first).is_none());
    assert!(h.state().feature_selection.is_empty());
}

/// An iPhone 16 in portrait, with its safe area.
fn phone() -> Pad {
    let mut h = harness((393.0, 852.0), Layout::Pad);
    let insets = egui::epaint::MarginF32 {
        left: 0.0,
        right: 0.0,
        top: 59.0,
        bottom: 34.0,
    };
    h.input_mut().safe_area_insets = Some(egui::SafeAreaInsets(insets));
    h.run_steps(3);
    h
}

/// A drag of a finger from `from` by `by`, in eight moves.
fn drag(h: &mut Pad, from: Pos2, by: egui::Vec2) {
    touch(h, egui::TouchPhase::Start, from);
    h.step();
    for i in 1..=8 {
        touch(h, egui::TouchPhase::Move, from + by * (i as f32 / 8.0));
        h.step();
    }
    touch(h, egui::TouchPhase::End, from + by);
    h.run_steps(3);
}

#[test]
fn on_an_iphone_the_table_is_a_sheet_that_scrolls_sideways_with_its_header() {
    let mut h = phone();
    let (id, uuids) = towns(&mut h, 200, 0);
    let project = &mut h.state_mut().doc.project;
    for name in ["kind", "region", "notes"] {
        project.add_field(id, Field::new(name, FieldType::Text));
    }
    h.state_mut().open_table(id);
    h.run_steps(3);
    let rects = h.state().pad.rects.unwrap();
    let sheet = rects.sheet.expect("no sheet");
    assert_eq!(table_rect(&h), sheet);

    // The first column shows, and the last column is out of the sheet at
    // the right.
    let (name, notes) = (sort_name("name"), sort_name("notes"));
    let cell = |h: &Pad, field: &str, i: usize| control(h, &cell_name(field, uuids[i]));
    let header = control(&h, &name).expect("no header of the first column");
    assert_eq!(cell(&h, "name", 2).expect("no cell").left(), header.left());
    assert!(control(&h, &notes).is_none());
    assert!(cell(&h, "notes", 2).is_none());

    // A drag to the left on the rows moves the rows and the header, and
    // selects no row.
    let from = cell(&h, "name", 2).unwrap().center();
    drag(&mut h, from, vec2(-600.0, 0.0));
    assert!(h.state().feature_selection.is_empty());
    assert!(control(&h, &name).is_none());
    let moved = control(&h, &notes).expect("no header of the last column");
    let width = vale_app::ui::table::column_width(FieldType::Text);
    assert!(moved.width() >= width, "{moved:?}");
    assert!(moved.right() <= sheet.right(), "{moved:?}");
    let row = cell(&h, "notes", 2).expect("no cell of the last column");
    assert_eq!((row.left(), row.width()), (moved.left(), moved.width()));
    // The header stays at the top of the rows.
    assert_eq!(moved.top(), header.top());

    // A drag up still scrolls the rows, and the columns keep their place.
    let from = cell(&h, "notes", 3).unwrap().center();
    drag(&mut h, from, vec2(0.0, -200.0));
    assert!(cell(&h, "notes", 0).is_none());
    assert!(cell(&h, "notes", 7).is_some());
    assert_eq!(control(&h, &notes).unwrap(), moved);
}

#[test]
#[ignore = "gpu"]
fn manual_the_attribute_table() {
    let mut h = harness(LANDSCAPE, Layout::Pad);
    let (id, uuids) = towns(&mut h, 2_000, 0);
    h.state_mut().open_table(id);
    h.state_mut().set_feature_selection(refs(id, &uuids[2..3]));
    h.run_steps(3);
    tap(&mut h, &sort_name("name"));
    screenshot::save(&mut h, "table-pad");
    tap(&mut h, &cell_name("rank", uuids[2]));
    screenshot::save(&mut h, "table-pad-edit");
}
