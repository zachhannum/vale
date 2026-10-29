mod common;
mod screenshot;

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use vale_app::document::Document;
use vale_app::headless;
use vale_app::ui::atlas::{self, AtlasSelection};
use vale_app::ui::{AppState, LayerRef, Workspace, draw};
use vale_render::Color;
use vale_store::LayerId;

const SIZE: (f32, f32) = (1400.0, 900.0);

fn harness_with(doc: Document) -> Harness<'static, AppState> {
    let mut state = AppState::new(doc).unwrap();
    state.headless = true;
    state.workspace = Workspace::Atlas;
    atlas::add_frame(&mut state).unwrap();
    state.atlas.selection = AtlasSelection::None;
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

/// The screen position of a place in the frame of the page.
fn screen_pos(h: &mut Harness<'static, AppState>, lonlat: [f64; 2]) -> egui::Pos2 {
    let s = h.state_mut();
    let place = s.atlas.place.unwrap();
    let frame = s.doc.project.atlas.pages[0].frames[0].clone();
    let projection = s.pipeline.projection(&s.doc, frame.map).unwrap();
    let vt = vale_app::atlas::frame_transform(projection, &frame, place.target());
    let p = vt.to_page(projection.forward(lonlat).unwrap());
    egui::pos2(p.x as f32, p.y as f32)
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
    let mut h = harness();
    h.state_mut().atlas.selection = AtlasSelection::Page;
    h.run_steps(2);
    assert!(h.query_by_label("Select a layer in the list.").is_some());
    assert!(h.query_by_label("Click a feature on the map.").is_some());
    assert!(h.query_by_label("Unplaced labels").is_some());
}

#[test]
fn places_controls_and_labels_switch() {
    let mut h = harness();
    let places = id_of(&h, "Places");
    h.state_mut().selected_layer = Some(LayerRef::Vector(places));
    h.run_steps(2);
    assert!(h.query_by_label("Show labels").is_some());
    assert!(h.query_by_label(vale_app::ui::pad::rules::SIZE).is_some());
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
    h.state_mut().selected_layer = Some(LayerRef::Vector(land));
    h.run_steps(2);
    assert!(
        h.query_by_label("Polygon labels are not available yet.")
            .is_some()
    );
    assert!(h.query_by_label(vale_app::ui::pad::rules::SIZE).is_none());
}

#[test]
fn the_dock_edits_the_rules_of_a_layer() {
    use vale_app::ui::pad::rules::{ADD_RULE, DELETE_RULE, MOVE_UP};
    let mut h = harness();
    let land = id_of(&h, "Land");
    h.state_mut().selected_layer = Some(LayerRef::Vector(land));
    h.run_steps(2);
    let names = |h: &Harness<'static, AppState>| -> Vec<String> {
        let rules = &h.state().doc.entry(land).unwrap().style.style.rules;
        rules.iter().map(|r| r.name.clone()).collect()
    };
    let generation = h.state().map_generation;
    h.get_by_label(ADD_RULE).click();
    h.run_steps(2);
    assert_eq!(names(&h), ["Default", "Rule 1"]);
    assert!(h.state().map_generation > generation);
    h.get_by_label(MOVE_UP).click();
    h.run_steps(2);
    assert_eq!(names(&h), ["Rule 1", "Default"]);
    h.get_by_label(DELETE_RULE).click();
    h.run_steps(2);
    assert_eq!(names(&h), ["Default"]);
}

#[test]
fn the_dock_sets_a_point_symbol_and_a_scatter() {
    use vale_app::ui::Action;
    use vale_app::ui::pad::rules::{
        ADD_SYMBOL, CIRCLE, NO_SYMBOL_LIST, SCATTER, SIZE, SVG_SYMBOL, SYMBOL,
    };
    let mut h = harness();
    let places = id_of(&h, "Places");
    let rule = |h: &Harness<'static, AppState>, id| {
        h.state().doc.entry(id).unwrap().style.style.rules[0].clone()
    };
    h.state_mut().selected_layer = Some(LayerRef::Vector(places));
    h.run_steps(2);
    assert!(h.query_by_label(ADD_SYMBOL).is_none());
    assert!(h.query_by_label(SIZE).is_some());

    // With no symbols, the rule stays a circle, and the list is off.
    h.get_by_value(CIRCLE).click();
    h.run_steps(2);
    h.get_by_label(SVG_SYMBOL).click();
    h.run_steps(2);
    assert!(rule(&h, places).symbol().is_none());
    assert!(h.query_by_label(SYMBOL).is_some());
    let list = h.get_by_value(NO_SYMBOL_LIST);
    assert!(list.accesskit_node().is_disabled());
    h.get_by_label(ADD_SYMBOL).click();
    h.run_steps(2);
    assert!(h.state().actions.contains(&Action::AddSymbolDialog));
    screenshot::save(&mut h, "dock-symbol-empty");

    // The first symbol that the user adds becomes the symbol of the rule.
    common::add_symbols(&mut h.state_mut().doc, &["pine", "castle", "tree"]);
    h.run_steps(2);
    assert_eq!(rule(&h, places).symbol().unwrap().symbol, "pine");
    h.get_by_value("pine").click();
    h.run_steps(2);
    h.get_by_label("castle").click();
    h.run_steps(2);
    let s = rule(&h, places).symbol().cloned().unwrap();
    assert_eq!(s.symbol, "castle");
    assert_eq!(s.size.fallback, 12.0);
    assert!(s.tint.is_none());
    screenshot::save(&mut h, "dock-symbol");

    let land = id_of(&h, "Land");
    h.state_mut().selected_layer = Some(LayerRef::Vector(land));
    h.run_steps(2);
    h.get_by_label(SCATTER).click();
    h.run_steps(2);
    let scatter = rule(&h, land).scatter().cloned().unwrap();
    assert_eq!(scatter.symbol, "pine");
    assert_eq!(
        (scatter.size.fallback, scatter.spacing.fallback),
        (8.0, 10.0)
    );
    assert!(scatter.tint.is_none());
    screenshot::save(&mut h, "dock-scatter");
}

#[test]
fn select_label_selects_layer() {
    let mut h = harness();
    h.get_by_label("Select Rivers").click();
    h.run_steps(2);
    let rivers = id_of(&h, "Rivers");
    assert_eq!(h.state().selected_layer, Some(LayerRef::Vector(rivers)));
}

#[test]
fn move_down_and_remove() {
    let mut h = harness();
    let places = id_of(&h, "Places");
    let rivers = id_of(&h, "Rivers");
    h.state_mut().selected_layer = Some(LayerRef::Vector(places));
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
    h.get_by_label("Delete layer").click();
    h.run_steps(2);
    assert_eq!(h.state().doc.frame.entries.len(), 2);
    assert_eq!(h.state().selected_layer, None);
}

#[test]
fn style_change_reaches_the_pixels() {
    let mut h = harness();
    let land = id_of(&h, "Land");
    h.state_mut()
        .doc
        .entry_mut(land)
        .unwrap()
        .style
        .set_fill(Some(Color::rgb(200, 0, 0)));
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
    assert_eq!(s.selected_layer, Some(LayerRef::Vector(places)));
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
fn unplaced_summary() {
    let mut h = harness();
    h.state_mut().atlas.selection = AtlasSelection::Page;
    h.run_steps(2);
    assert!(h.query_by_label("Unplaced labels").is_some());
    let mut doc = Document::sample();
    doc.frame.labels = false;
    let mut h = harness_with(doc);
    h.state_mut().atlas.selection = AtlasSelection::Page;
    h.run_steps(2);
    assert!(h.query_by_label("All labels fit.").is_some());
}

#[test]
#[ignore = "gpu"]
fn inspector_screenshots() {
    use vale_app::pipeline::Selection;
    let mut state = AppState::new(Document::sample()).unwrap();
    state.workspace = Workspace::Atlas;
    atlas::add_frame(&mut state).unwrap();
    let places = state
        .doc
        .project
        .layers()
        .iter()
        .find(|l| l.name == "Places")
        .unwrap()
        .id;
    state.selected_layer = Some(LayerRef::Vector(places));
    state.selection = Some(Selection {
        layer: places,
        feature: 0,
    });
    state
        .doc
        .entry_mut(places)
        .unwrap()
        .style
        .set_fill(Some(Color::rgb(200, 0, 0)));
    let (img, _) = headless::ui_png(state, (1440.0, 900.0), 1.0).unwrap();
    std::fs::create_dir_all("../../target/app").unwrap();
    img.save("../../target/app/test-inspector.png").unwrap();
    // No dark margin: the top-left pixel is the tool bar, not the outer background.
    assert_eq!(img.get_pixel(0, 0), img.get_pixel(1300, 10));
}

#[test]
fn the_font_box_sets_the_font_of_the_labels() {
    let silkscreen = include_bytes!("data/fonts/Silkscreen-Regular.ttf");
    let mut h = harness();
    // Silkscreen stands for a font of the system.
    let mut fonts = vale_app::fonts::bundled().unwrap();
    fonts.register(silkscreen.to_vec()).unwrap();
    h.state_mut().set_fonts(fonts);
    let places = id_of(&h, "Places");
    h.state_mut().selected_layer = Some(LayerRef::Vector(places));
    h.run_steps(2);
    assert!(h.query_by_label("Font").is_some());
    let is_font =
        |n: &egui_kittest::Node<'_>| n.accesskit_node().value().as_deref() == Some("Noto Sans");
    (h.get_all_by_role(egui::accesskit::Role::ComboBox))
        .find(is_font)
        .unwrap()
        .click();
    h.run_steps(2);
    h.get_by_label("Silkscreen").click();
    h.run_steps(3);
    let s = h.state();
    let label = &s.doc.entry(places).unwrap().style.label;
    assert_eq!(label.font.as_deref(), Some("Silkscreen"));
    let in_silkscreen = s.composed.as_ref().unwrap().list.items.iter().any(|i| {
        matches!(i, vale_render::Item::Glyphs(g) if g.font.data.as_ref() == silkscreen.as_slice())
    });
    assert!(in_silkscreen, "no label is in Silkscreen");

    let entry = h.state_mut().doc.entry_mut(places).unwrap();
    entry.style.label.font = Some("Lora".into());
    h.run_steps(2);
    assert!(
        h.query_by_label("Font Lora is missing. Labels use Noto Sans.")
            .is_some()
    );
}
