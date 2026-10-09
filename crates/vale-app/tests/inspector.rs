use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use vale_app::document::Document;
use vale_app::headless;
use vale_app::ui::{AppState, Workspace, draw};
use vale_render::Color;
use vale_store::LayerId;

const SIZE: (f32, f32) = (1400.0, 900.0);

fn harness_with(doc: Document) -> Harness<'static, AppState> {
    let mut state = AppState::new(doc).unwrap();
    state.headless = true;
    state.workspace = Workspace::Map;
    let mut h = Harness::builder()
        .with_size(egui::vec2(SIZE.0, SIZE.1))
        .with_pixels_per_point(1.0)
        .with_step_dt(0.05)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(3);
    h
}

fn harness() -> Harness<'static, AppState> {
    harness_with(Document::sample())
}

fn id_of(h: &Harness<'static, AppState>, name: &str) -> LayerId {
    h.state()
        .doc
        .project
        .layers()
        .iter()
        .find(|l| l.name == name)
        .unwrap()
        .id
}

/// The screen position of a place.
fn screen_pos(h: &mut Harness<'static, AppState>, lonlat: [f64; 2]) -> egui::Pos2 {
    let s = h.state_mut();
    let size = s.canvas_size;
    let p = s
        .pipeline
        .lonlat_to_page(&s.doc, size, lonlat)
        .unwrap()
        .unwrap();
    egui::pos2(
        s.canvas_origin.0 + p.x as f32,
        s.canvas_origin.1 + p.y as f32,
    )
}

/// A primary click at a screen position, in three steps.
fn click_at(h: &mut Harness<'static, AppState>, pos: egui::Pos2) {
    h.hover_at(pos);
    h.step();
    h.drag_at(pos);
    h.step();
    h.drop_at(pos);
    h.step();
}

#[test]
fn empty_states() {
    let h = harness();
    assert!(h.query_by_label("Select a layer in the list.").is_some());
    assert!(h.query_by_label("Click a feature on the map.").is_some());
    assert!(h.query_by_label("Unplaced labels").is_some());
}

#[test]
fn places_controls_and_labels_switch() {
    let mut h = harness();
    let places = id_of(&h, "Places");
    h.state_mut().selected_layer = Some(places);
    h.run_steps(2);
    assert!(h.query_by_label("Show labels").is_some());
    assert!(h.query_by_label("Point size").is_some());
    assert!(h.query_by_label("Label field").is_some());
    let before = h.state().composed.as_ref().unwrap().labels.placed;
    h.get_by_label("Show labels").click();
    h.run_steps(2);
    let s = h.state();
    assert!(!s.doc.entry(places).unwrap().style.label.enabled);
    assert!(s.composed.as_ref().unwrap().labels.placed < before);
}

#[test]
fn polygon_layer_has_no_point_controls() {
    let mut h = harness();
    let land = id_of(&h, "Land");
    h.state_mut().selected_layer = Some(land);
    h.run_steps(2);
    assert!(
        h.query_by_label("Polygon labels are not in the prototype.")
            .is_some()
    );
    assert!(h.query_by_label("Point size").is_none());
}

#[test]
fn select_label_selects_layer() {
    let mut h = harness();
    h.get_by_label("Select Rivers").click();
    h.run_steps(2);
    let rivers = id_of(&h, "Rivers");
    assert_eq!(h.state().selected_layer, Some(rivers));
}

#[test]
fn move_down_and_remove() {
    let mut h = harness();
    let places = id_of(&h, "Places");
    let rivers = id_of(&h, "Rivers");
    h.state_mut().selected_layer = Some(places);
    h.run_steps(2);
    h.get_by_label("Move down").click();
    h.run_steps(2);
    let pos = |h: &Harness<'static, AppState>, id| {
        h.state()
            .doc
            .frame
            .entries
            .iter()
            .position(|e| e.layer == id)
            .unwrap()
    };
    assert!(pos(&h, places) < pos(&h, rivers));
    h.get_by_label("Remove").click();
    h.run_steps(2);
    assert_eq!(h.state().doc.frame.entries.len(), 2);
    assert_eq!(h.state().selected_layer, None);
}

#[test]
fn style_change_reaches_the_pixels() {
    let mut h = harness();
    let land = id_of(&h, "Land");
    h.state_mut().doc.entry_mut(land).unwrap().style.fill = Some(Color::rgb(200, 0, 0));
    h.state_mut().touch();
    h.run_steps(2);
    let state = h.state();
    let size = (900.0, 700.0);
    let (_, rgb) = headless::probe(&state.doc, size, 1.0, [107.0, 58.0])
        .unwrap()
        .unwrap();
    assert_eq!(rgb, [200, 0, 0]);
}

#[test]
fn click_identifies_a_feature() {
    let mut h = harness();
    let places = id_of(&h, "Places");
    let tokyo = screen_pos(&mut h, [139.75, 35.68]);
    click_at(&mut h, tokyo);
    h.run_steps(3);
    let s = h.state();
    let sel = s.selection.expect("Tokyo is selected");
    assert_eq!(sel.layer, places);
    assert_eq!(s.selected_layer, Some(places));
    assert!(h.query_all_by_label("Tokyo").count() >= 1);
    assert!(h.query_by_label("Clear selection").is_some());
    let list = &h.state().composed.as_ref().unwrap().list;
    let found = format!("{:?}", list.items).contains("r: 230, g: 70, b: 30");
    assert!(found, "no selection stroke in the display list");

    let ocean = screen_pos(&mut h, [-20.0, -30.0]);
    click_at(&mut h, ocean);
    h.run_steps(3);
    assert_eq!(h.state().selection, None);
}

#[test]
fn double_click_centers_the_projection() {
    let mut h = harness();
    let at = screen_pos(&mut h, [100.0, 20.0]);
    h.hover_at(at);
    h.step();
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.step();
    }
    h.run_steps(3);
    let lon0 = h.state().doc.frame.projection.lon0;
    assert!((lon0 - 100.0).abs() < 0.5, "{lon0}");
    assert!(h.state().status.starts_with("Centered the projection"));
}

#[test]
fn unplaced_summary() {
    let h = harness();
    assert!(h.query_by_label("Unplaced labels").is_some());
    let mut doc = Document::sample();
    doc.frame.labels = false;
    let h = harness_with(doc);
    assert!(h.query_by_label("All labels fit.").is_some());
}

#[test]
fn inspector_screenshots() {
    use vale_app::pipeline::Selection;
    let mut state = AppState::new(Document::sample()).unwrap();
    state.workspace = Workspace::Map;
    let places = state
        .doc
        .project
        .layers()
        .iter()
        .find(|l| l.name == "Places")
        .unwrap()
        .id;
    state.selected_layer = Some(places);
    state.selection = Some(Selection {
        layer: places,
        feature: 0,
    });
    state.doc.entry_mut(places).unwrap().style.fill = Some(Color::rgb(200, 0, 0));
    let (img, _) = headless::ui_png(state, (1440.0, 900.0), 1.0).unwrap();
    std::fs::create_dir_all("../../target/app").unwrap();
    img.save("../../target/app/test-inspector.png").unwrap();
    // No dark margin: the top-left pixel is the tool bar, not the outer background.
    assert_eq!(img.get_pixel(0, 0), img.get_pixel(900, 10));
}
