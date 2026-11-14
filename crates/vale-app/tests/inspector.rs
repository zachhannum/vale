mod common;
mod screenshot;

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use vale_app::document::Document;
use vale_app::headless;
use vale_app::ui::atlas::{self, AtlasSelection};
use vale_app::ui::pad::label_text::LABEL_TEXT;
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

/// Copies the globe style of a layer into each map, because a change of the
/// globe style does not change a map.
fn copy_globe_style(h: &mut Harness<'static, AppState>, layer: LayerId) {
    let maps: Vec<_> = h
        .state()
        .doc
        .project
        .atlas
        .maps
        .iter()
        .map(|m| m.id)
        .collect();
    for map in maps {
        h.state_mut().copy_globe_style(map, layer);
    }
    h.run_steps(2);
}

#[test]
fn empty_states() {
    let mut h = harness();
    h.state_mut().atlas.selection = AtlasSelection::Page;
    h.run_steps(2);
    assert!(h.query_by_label("Select a layer in the list.").is_some());
    assert!(h.query_by_label("Unplaced labels").is_some());
}

#[test]
fn places_controls_and_labels_switch() {
    let mut h = harness();
    let places = id_of(&h, "Places");
    h.state_mut().selected_layer = Some(LayerRef::Vector(places));
    h.run_steps(2);
    assert!(h.query_by_label("Show labels").is_some());
    assert!(
        h.query_by_role_and_label(
            egui::accesskit::Role::SpinButton,
            vale_app::ui::pad::rules::SIZE
        )
        .is_some()
    );
    assert!(
        h.query_by_role_and_label(egui::accesskit::Role::TextInput, LABEL_TEXT)
            .is_some()
    );
    let before = h.state().composed.as_ref().unwrap().labels.placed;
    h.get_by_label("Show labels").click();
    h.run_steps(2);
    copy_globe_style(&mut h, places);
    let s = h.state();
    assert!(!s.doc.entry(places).unwrap().style.label.enabled);
    assert!(s.composed.as_ref().unwrap().labels.placed < before);
}

/// Selects Places with label text `text`, and clicks the label text field
/// at its right end, which puts the caret at the end.
fn focus_label_text(h: &mut Harness<'static, AppState>, text: &str) -> LayerId {
    let places = id_of(h, "Places");
    h.state_mut()
        .doc
        .entry_mut(places)
        .unwrap()
        .style
        .label
        .text = text.into();
    h.state_mut().selected_layer = Some(LayerRef::Vector(places));
    h.run_steps(2);
    let input = egui::accesskit::Role::TextInput;
    let field = h.get_by_role_and_label(input, LABEL_TEXT).rect();
    let at = egui::pos2(field.right() - 4.0, field.center().y);
    click_at(h, at);
    h.run_steps(2);
    places
}

fn label_text(h: &Harness<'static, AppState>, id: LayerId) -> String {
    h.state().doc.entry(id).unwrap().style.label.text.clone()
}

fn type_chars(h: &mut Harness<'static, AppState>, text: &str) {
    h.event(egui::Event::Text(text.into()));
    h.run_steps(2);
}

#[test]
fn the_label_text_field_keeps_typed_text() {
    let mut h = harness();
    let places = focus_label_text(&mut h, "");
    type_chars(&mut h, "Pop ");
    assert_eq!(label_text(&h, places), "'Pop '");
}

#[test]
fn brace_opens_field_completion() {
    use vale_app::ui::expr_field::completion_name;
    let mut h = harness();
    focus_label_text(&mut h, "name");
    assert!(h.query_by_label(&completion_name("name")).is_none());
    type_chars(&mut h, " {na");
    assert!(h.query_by_label(&completion_name("name")).is_some());
    assert!(h.query_by_label(&completion_name("adm0name")).is_none());
    h.run_steps(1);
    screenshot::save(&mut h, "inspector-completion");
}

#[test]
fn return_inserts_completion() {
    use vale_app::ui::expr_field::completion_name;
    let mut h = harness();
    let places = focus_label_text(&mut h, "");
    type_chars(&mut h, "{na");
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(label_text(&h, places), "name");
    // Down moves the highlight to the second field.
    type_chars(&mut h, " {na");
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(2);
    assert!(h.query_by_label(&completion_name("namealt")).is_some());
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(label_text(&h, places), "concat(name, ' ', namealt)");
}

#[test]
fn escape_keeps_typed_text() {
    use vale_app::ui::expr_field::completion_name;
    let mut h = harness();
    let places = focus_label_text(&mut h, "");
    type_chars(&mut h, "{na");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.query_by_label(&completion_name("name")).is_none());
    assert_eq!(label_text(&h, places), "'{na'");
    // The field keeps the focus.
    type_chars(&mut h, "!");
    assert_eq!(label_text(&h, places), "'{na!'");
}

#[test]
fn backspace_selects_a_chip_then_deletes_it() {
    let mut h = harness();
    let places = focus_label_text(&mut h, "name");
    h.key_press(egui::Key::Backspace);
    h.run_steps(2);
    assert_eq!(label_text(&h, places), "name");
    h.key_press(egui::Key::Backspace);
    h.run_steps(2);
    assert_eq!(label_text(&h, places), "");
}

#[test]
fn polygon_layer_has_no_point_controls() {
    let mut h = harness();
    let land = id_of(&h, "Land");
    h.state_mut().selected_layer = Some(LayerRef::Vector(land));
    h.run_steps(2);
    assert!(h.query_by_label("Show labels").is_some());
    assert!(
        h.query_by_role_and_label(
            egui::accesskit::Role::SpinButton,
            vale_app::ui::pad::rules::SIZE
        )
        .is_none()
    );
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
fn the_dock_edits_more_than_one_rule() {
    use vale_app::ui::pad::rules::{ADD_RULE, DELETE_RULES};
    let mut h = harness();
    let land = id_of(&h, "Land");
    h.state_mut().selected_layer = Some(LayerRef::Vector(land));
    h.run_steps(2);
    let rules = |h: &Harness<'static, AppState>| {
        h.state().doc.entry(land).unwrap().style.style.rules.clone()
    };
    for _ in 0..2 {
        h.get_by_label(ADD_RULE).click();
        h.run_steps(2);
    }
    h.get_by_label("Default").click();
    h.run_steps(2);
    h.get_by_label("Rule 2")
        .click_modifiers(egui::Modifiers::SHIFT);
    h.run_steps(2);
    // Three rules are selected, so the editor has no filter.
    assert!(h.query_by_label(DELETE_RULES).is_some());
    assert!(h.query_by_label(vale_app::ui::pad::rules::FILTER).is_none());
    let fills: Vec<_> = rules(&h).iter().map(|r| r.fill()).collect();
    let increase = format!("Increase {}", vale_app::ui::pad::rules::STROKE_WIDTH);
    h.get_by_label(&increase).click();
    h.run_steps(2);
    let widths: Vec<_> = rules(&h)
        .iter()
        .map(|r| r.stroke().unwrap().width)
        .collect();
    assert!(widths.iter().all(|w| *w == widths[0]), "{widths:?}");
    assert_eq!(
        rules(&h).iter().map(|r| r.fill()).collect::<Vec<_>>(),
        fills
    );

    h.get_by_label("Rule 1")
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run_steps(2);
    h.get_by_label(DELETE_RULES).click();
    h.run_steps(2);
    let names: Vec<_> = rules(&h).into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["Rule 1"]);
}

#[test]
fn the_dock_sets_a_point_symbol_and_a_scatter() {
    use vale_app::ui::Action;
    use vale_app::ui::pad::rules::{
        ADD_SYMBOL, NO_SYMBOL_LIST, SCATTER, SHAPE, SIZE, SVG_SYMBOL, SYMBOL,
    };
    let mut h = harness();
    let places = id_of(&h, "Places");
    let rule = |h: &Harness<'static, AppState>, id| {
        h.state().doc.entry(id).unwrap().style.style.rules[0].clone()
    };
    h.state_mut().selected_layer = Some(LayerRef::Vector(places));
    h.run_steps(2);
    assert!(h.query_by_label(ADD_SYMBOL).is_none());
    assert!(
        h.query_by_role_and_label(egui::accesskit::Role::SpinButton, SIZE)
            .is_some()
    );

    // With no symbols, the rule stays a circle, and the list is off.
    h.get_by_value(SHAPE).click();
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
fn a_frame_tool_click_on_a_feature_selects_only_the_frame() {
    let mut h = harness();
    h.state_mut().atlas.tool = vale_app::ui::atlas::AtlasTool::Frame;
    let tokyo = screen_pos(&mut h, [139.75, 35.68]);
    click_at(&mut h, tokyo);
    h.run_steps(3);
    let s = h.state();
    assert!(s.feature_selection.is_empty());
    assert!(matches!(s.atlas.selection, AtlasSelection::Frame(_)));
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
fn the_font_dialog_searches_and_sets_the_font_of_the_labels() {
    use vale_app::ui::font_dialog::{CANCEL, NO_MATCH, SEARCH};
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
    h.get_by_label("Noto Sans…").click();
    h.run_steps(2);
    assert!(h.state().font_dialog.is_some());
    h.get_by_label(SEARCH).type_text("silk");
    h.run_steps(2);
    screenshot::save(&mut h, "font-dialog");
    assert!(h.query_by_label("Noto Sans").is_none());
    h.get_by_label("Silkscreen").click();
    h.run_steps(3);
    copy_globe_style(&mut h, places);
    let s = h.state();
    assert!(s.font_dialog.is_none());
    let label = &s.doc.entry(places).unwrap().style.label;
    assert_eq!(label.font.as_deref(), Some("Silkscreen"));
    let in_silkscreen = s.composed.as_ref().unwrap().list.items.iter().any(|i| {
        matches!(i, vale_render::Item::Glyphs(g) if g.font.data.as_ref() == silkscreen.as_slice())
    });
    assert!(in_silkscreen, "no label is in Silkscreen");

    // A search with no match, and Cancel.
    h.get_by_label("Silkscreen…").click();
    h.run_steps(2);
    h.get_by_label(SEARCH).type_text("zzz");
    h.run_steps(2);
    assert!(h.query_by_label(NO_MATCH).is_some());
    h.get_by_label(CANCEL).click();
    h.run_steps(2);
    assert!(h.state().font_dialog.is_none());

    let entry = h.state_mut().doc.entry_mut(places).unwrap();
    entry.style.label.font = Some("Lora".into());
    h.run_steps(2);
    assert!(
        h.query_by_label("Font Lora is missing. Labels use Noto Sans.")
            .is_some()
    );
}
