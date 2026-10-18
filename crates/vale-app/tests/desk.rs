//! The desktop layout.

use eframe::egui::accesskit::{Role, Toggled};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use vale_app::document::Document;
use vale_app::globe::Tool;
use vale_app::session::Session;
use vale_app::ui::desk::brush::{LOCK, PICK, STROKE_START};
use vale_app::ui::desk::command::Command;
use vale_app::ui::desk::dock::{BRUSH_DEBUG, FRAME, HEIGHT, IMPORT, WORLD_PROPERTIES, grouped};
use vale_app::ui::desk::float::SHEET;
use vale_app::ui::desk::options::{BRUSH_SETTINGS, tool_name};
use vale_app::ui::desk::rail::MORE_TOOLS;
use vale_app::ui::desk::status::zoom_text;
use vale_app::ui::desk::title::NOT_IN_PROTOTYPE;
use vale_app::ui::desk::{MapTool, Tab, theme};
use vale_app::ui::pad::{ADD_LAYER, readout_text};
use vale_app::ui::panels::PROJECTION;
use vale_app::ui::world_form::{CANCEL, SAVE};
use vale_app::ui::{AppState, Layout, Workspace, draw};
use vale_sphere::ProjectionKind;

type App = Harness<'static, AppState>;

const MENUS: [&str; 6] = ["File", "Edit", "View", "Layer", "Window", "Help"];

fn harness_with(state: AppState) -> App {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(3);
    h
}

fn state() -> AppState {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.set_layout(Layout::Desktop);
    state
}

fn harness() -> App {
    harness_with(state())
}

#[test]
fn the_title_bar_has_the_switches_and_the_actions() {
    let h = harness();
    // The world inspector also has the name.
    let bar = theme::MENU + theme::BAR;
    let names = h.query_all_by_label("Earth (sample)");
    assert_eq!(names.filter(|n| n.rect().bottom() < bar).count(), 1);
    for label in ["World", "Maps", "Globe", "Flat", "Undo", "Redo"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert!(h.get_by_label("Atlas").accesskit_node().is_disabled());
    assert!(h.get_by_label("Undo").accesskit_node().is_disabled());
    let dock = h.get_by_label("Dock");
    assert_eq!(dock.accesskit_node().toggled(), Some(Toggled::True));
}

#[test]
fn the_title_bar_is_below_the_menu_row_and_its_tools_are_28_points() {
    let h = harness();
    let undo = h.get_by_label("Undo").rect();
    assert_eq!(undo.size(), egui::vec2(theme::TOOL, theme::TOOL));
    assert_eq!(undo.center().y, theme::MENU + theme::BAR / 2.0);
    assert!(undo.right() > 1150.0, "{undo:?}");
    // The view switch is at the middle of the bar.
    let globe = h.get_by_label("Globe").rect();
    let flat = h.get_by_label("Flat").rect();
    assert!(
        (globe.left() + flat.right() - 1280.0).abs() < 6.0,
        "{globe:?} {flat:?}"
    );
}

#[test]
fn the_maps_workspace_shows_the_name_of_the_frame() {
    let mut h = harness();
    h.get_by_label("Maps").click();
    h.run_steps(3);
    assert_eq!(h.state().workspace, Workspace::Map);
    assert!(h.query_by_label("World map").is_some());
    assert!(h.query_by_label("Flat").is_none());
}

#[test]
fn the_flat_view_has_the_projection_and_the_center_buttons() {
    let mut h = harness();
    h.get_by_label("Flat").click();
    h.run_steps(3);
    assert!(h.query_by_role(egui::accesskit::Role::ComboBox).is_some());
    assert!(h.query_by_label("Recenter").is_some());
    assert!(h.query_by_label("Reset").is_some());
}

#[test]
fn the_projection_menu_lists_the_projections_and_sets_one() {
    let mut h = harness();
    h.get_by_label("Flat").click();
    h.run_steps(3);
    let button = h.get_by_role_and_label(Role::ComboBox, PROJECTION);
    assert_eq!(
        button.rect().bottom(),
        theme::MENU + theme::BAR / 2.0 + 12.0
    );
    button.click();
    h.run_steps(2);
    for kind in ProjectionKind::ALL {
        assert!(h.query_by_label(kind.name()).is_some(), "{kind:?}");
    }
    let kind = *ProjectionKind::ALL.last().unwrap();
    h.get_by_label(kind.name()).click();
    h.run_steps(2);
    assert_eq!(h.state().globe.flat.spec().kind, kind);
    let button = h.get_by_role_and_label(Role::ComboBox, PROJECTION);
    assert_eq!(
        button.accesskit_node().value().as_deref(),
        Some(kind.name())
    );
}

/// The rectangle of the sheet of the world form.
fn sheet_rect(h: &App) -> egui::Rect {
    let id = egui::Id::new("world-form");
    h.ctx.memory(|m| m.area_rect(id)).unwrap()
}

#[test]
fn the_world_form_is_a_sheet_below_the_title_bar() {
    let mut h = harness();
    h.get_by_label(WORLD_PROPERTIES).click();
    h.run_steps(3);
    let rect = sheet_rect(&h);
    assert_eq!(rect.top(), theme::MENU + theme::BAR);
    assert_eq!(rect.center().x, 640.0);
    assert_eq!(rect.width(), SHEET);
    for label in ["Moon", "Miles", CANCEL, SAVE] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }

    // Escape drops the changes.
    h.get_by_label("Moon").click();
    h.run_steps(2);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().world_form.is_none());
    assert_eq!(h.state().doc.project.world.radius_km, 6371.0);

    // Return saves them.
    h.get_by_label(WORLD_PROPERTIES).click();
    h.run_steps(3);
    h.get_by_label("Moon").click();
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert!(h.state().world_form.is_none());
    assert_eq!(h.state().doc.project.world.radius_km, 1737.4);
}

#[test]
fn the_title_bar_shows_the_file_and_its_save() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state();
    let path = dir.path().join("terra.gpkg");
    state.session = Some(Session::open(&path, &mut state.doc).unwrap());
    let h = harness_with(state);
    assert!(h.query_by_label("terra.gpkg · Saved").is_some());
}

#[test]
fn the_menu_row_has_the_menus_without_a_menu_bar_of_the_system() {
    let h = harness();
    for menu in MENUS {
        let rect = h.get_by_label(menu).rect();
        assert!(rect.bottom() <= theme::MENU, "{menu} {rect:?}");
    }

    let mut state = state();
    state.native_menu = true;
    let h = harness_with(state);
    for menu in MENUS {
        assert!(h.query_by_label(menu).is_none(), "{menu}");
    }
    let undo = h.get_by_label("Undo").rect();
    assert_eq!(undo.center().y, theme::BAR / 2.0);
}

#[test]
fn a_menu_row_runs_its_command() {
    let mut h = harness();
    h.get_by_label("Window").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "Toolbox").click();
    h.run_steps(2);
    assert_eq!(h.state().desk.tab, Tab::Toolbox);
    assert!(h.query_by_label("Brush debug").is_some());

    h.get_by_label("Layer").click();
    h.run_steps(2);
    h.get_by_label("Add layer").click();
    h.run_steps(2);
    let count = h.state().doc.project.layers().len();
    h.get_by_label("Lines").click();
    h.run_steps(2);
    assert_eq!(h.state().doc.project.layers().len(), count + 1);
}

#[test]
fn the_dock_button_shows_and_hides_the_dock() {
    let mut h = harness();
    let width = |h: &App| h.state().globe.rect.width();
    assert_eq!(width(&h), 1280.0 - theme::RAIL - theme::DOCK);
    let tab = h.get_by_role_and_label(Role::Tab, "Layers").rect();
    assert_eq!(
        tab.center().y,
        theme::MENU + theme::BAR + theme::OPTIONS + theme::TABS / 2.0
    );
    h.get_by_label("Dock").click();
    h.run_steps(2);
    assert!(!h.state().desk.dock_open);
    assert!(h.query_by_role(Role::Tab).is_none());
    assert_eq!(width(&h), 1280.0 - theme::RAIL);
    let dock = h.get_by_label("Dock");
    assert_eq!(dock.accesskit_node().toggled(), Some(Toggled::False));
    dock.click();
    h.run_steps(2);
    assert!(h.state().desk.dock_open);
    assert_eq!(width(&h), 1280.0 - theme::RAIL - theme::DOCK);
}

#[test]
fn the_tabs_of_the_dock_switch_between_the_layers_and_the_toolbox() {
    let mut h = harness();
    let layers = h.get_by_role_and_label(Role::Tab, "Layers");
    assert_eq!(layers.accesskit_node().toggled(), Some(Toggled::True));
    assert!(h.query_by_label(ADD_LAYER).is_some());
    assert!(!h.state().globe.debug);
    h.get_by_role_and_label(Role::Tab, "Toolbox").click();
    h.run_steps(2);
    assert_eq!(h.state().desk.tab, Tab::Toolbox);
    assert!(
        h.query_by_role_and_label(Role::Heading, BRUSH_DEBUG)
            .is_some()
    );
    assert!(h.query_by_label(ADD_LAYER).is_none());
    assert!(h.query_by_label("Hide Rivers").is_none());
    assert!(h.state().globe.debug);
    h.get_by_role_and_label(Role::Tab, "Layers").click();
    h.run_steps(2);
    assert_eq!(h.state().desk.tab, Tab::Layers);
    assert!(h.query_by_label("Hide Rivers").is_some());
    assert!(!h.state().globe.debug);
}

/// The heading of the inspector.
fn heading(h: &App) -> String {
    let node = h.get_by_role(Role::Heading);
    node.accesskit_node().label().unwrap_or_default()
}

#[test]
fn the_inspector_shows_the_panel_of_the_selection() {
    let mut h = harness();
    // The heading is the name of the world, and the world has no raster.
    assert_eq!(heading(&h), "Earth (sample)");
    assert!(h.query_by_label(WORLD_PROPERTIES).is_some());
    assert!(h.query_by_label(IMPORT).is_some());
    assert!(h.query_by_role_and_label(Role::Button, HEIGHT).is_none());
    h.get_by_label(WORLD_PROPERTIES).click();
    h.run_steps(2);
    assert!(h.state().world_form.is_some());
    h.state_mut().world_form = None;

    let map = &mut h.state_mut().globe.map;
    map.set(0, 0, 0, map.base() + 1);
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, HEIGHT).click();
    h.run_steps(2);
    assert_eq!(heading(&h), HEIGHT);
    let height = h.get_by_role_and_label(Role::Button, HEIGHT);
    assert_eq!(height.accesskit_node().toggled(), Some(Toggled::True));
    for label in [
        "Levels",
        "Greyscale",
        "Import heightmap…",
        "Export GeoTIFF…",
    ] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }

    h.get_by_role_and_label(Role::Button, "Rivers").click();
    h.run_steps(2);
    assert_eq!(heading(&h), "Rivers");
    assert!(h.query_by_label("Levels").is_none());
    assert!(h.query_by_label("Lines").is_some());
    let height = h.get_by_role_and_label(Role::Button, HEIGHT);
    assert_eq!(height.accesskit_node().toggled(), Some(Toggled::False));

    // A click on the selected row selects nothing.
    h.get_by_role_and_label(Role::Button, "Rivers").click();
    h.run_steps(2);
    assert_eq!(heading(&h), "Earth (sample)");
    assert_eq!(h.state().selected_layer, None);

    let count = h.state().doc.project.layers().len();
    h.get_by_role_and_label(Role::Button, "Rivers").click();
    h.run_steps(2);
    h.get_by_label("Delete layer").click();
    h.run_steps(2);
    assert_eq!(h.state().doc.project.layers().len(), count - 1);
    assert_eq!(heading(&h), "Earth (sample)");
}

#[test]
fn the_eye_of_a_layer_hides_and_shows_it() {
    let mut h = harness();
    let visible = |h: &App| {
        let layers = h.state().doc.project.layers();
        layers.iter().find(|l| l.name == "Rivers").unwrap().visible
    };
    assert!(visible(&h));
    h.get_by_label("Hide Rivers").click();
    h.run_steps(2);
    assert!(!visible(&h));
    assert!(h.query_by_label("Hide Rivers").is_none());
    h.get_by_label("Show Rivers").click();
    h.run_steps(2);
    assert!(visible(&h));
    // The eye does not select the layer.
    assert_eq!(h.state().selected_layer, None);
}

#[test]
fn the_add_button_of_the_dock_adds_a_layer() {
    let mut h = harness();
    let count = h.state().doc.project.layers().len();
    h.get_by_label(ADD_LAYER).click();
    h.run_steps(2);
    h.get_by_label("Polygons").click();
    h.run_steps(2);
    let layers = h.state().doc.project.layers();
    assert_eq!(layers.len(), count + 1);
    let added = layers.last().unwrap();
    assert_eq!(h.state().selected_layer, Some(added.id));
    assert_eq!(heading(&h), added.name);
}

#[test]
fn the_status_bar_shows_the_readout_the_scale_and_the_zoom() {
    let mut h = harness();
    let face = format!("Face {}", grouped(h.state().globe.map.face_size()));
    let label = h.get_by_label(&face);
    assert!(label.rect().top() >= 800.0 - theme::STATUS);
    assert!(h.query_by_label("100%").is_some());
    assert!(h.query_by_label("–").is_some());

    let c = h.state().globe.rect.center();
    h.hover_at(c);
    h.run_steps(2);
    let s = h.state();
    let text = readout_text(s.cursor_lonlat, s.cursor_meters);
    assert!(text.ends_with(" m"), "{text}");
    assert!(h.query_by_label(&text).is_some(), "{text}");

    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 2.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(3);
    let zoom = h.state().globe.view.zoom;
    assert!(zoom > 1.0, "{zoom}");
    assert!(h.query_by_label("100%").is_none());
    assert!(h.query_by_label(&zoom_text(zoom)).is_some());

    h.get_by_label("Maps").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("1 pt = ").is_some());
    assert!(h.query_by_label("100%").is_some());
}

#[test]
fn the_maps_inspector_shows_the_frame_with_no_selection() {
    let mut h = harness();
    h.get_by_label("Maps").click();
    h.run_steps(3);
    assert_eq!(heading(&h), FRAME);
    for label in ["Graticule", "Labels", "Fit world", "Unplaced labels"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    let projection = h.get_by_role(Role::ComboBox);
    assert_eq!(
        projection.accesskit_node().value().as_deref(),
        Some("Equal Earth")
    );
    h.get_by_label("Graticule").click();
    h.run_steps(3);
    assert!(!h.state().doc.frame.graticule);

    h.get_by_role_and_label(Role::Button, "Places").click();
    h.run_steps(2);
    assert_eq!(heading(&h), "Places");
    assert!(h.query_by_label("Show labels").is_some());
}

#[test]
fn a_key_of_the_menus_runs_its_command() {
    let mut h = harness();
    // Option and Command and 2 is Toolbox, not Maps.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::Num2);
    h.run_steps(2);
    assert_eq!(h.state().desk.tab, Tab::Toolbox);
    assert_eq!(h.state().workspace, Workspace::Globe);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num2);
    h.run_steps(2);
    assert_eq!(h.state().workspace, Workspace::Map);

    // The menu bar of the system has the keys.
    let mut state = state();
    state.native_menu = true;
    let mut h = harness_with(state);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num2);
    h.run_steps(2);
    assert_eq!(h.state().workspace, Workspace::Globe);
}

#[test]
fn a_tool_shows_its_name_and_its_key_in_a_tooltip() {
    let mut h = harness();
    let key = h
        .ctx
        .format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z));
    let tip = format!("Undo ({key})");
    assert_eq!(Command::Undo.tip(&h.ctx), tip);
    assert!(h.query_by_label(&tip).is_none());
    h.get_by_label("Undo").hover();
    h.run_steps(60);
    assert!(h.query_by_label(&tip).is_some());

    h.get_by_label("Atlas").hover();
    h.run_steps(60);
    assert!(h.query_by_label(NOT_IN_PROTOTYPE).is_some());
}

/// The tools of the World workspace, and their tooltips.
const WORLD_TOOLS: [(&str, &str); 5] = [
    ("Raise", "Raise (R)"),
    ("Lower", "Lower (F)"),
    ("Smooth", "Smooth (S)"),
    ("Flatten", "Flatten (E)"),
    ("Hand", "Hand (H)"),
];

fn press(h: &mut App, key: Key) {
    h.key_press(key);
    h.run_steps(2);
}

fn mode(h: &App) -> (Tool, vale_terrain::Mode) {
    let globe = &h.state().globe;
    (globe.tool, globe.brush.mode)
}

#[test]
fn the_tool_rail_has_the_tools_with_their_keys() {
    let mut h = harness();
    let top = theme::MENU + theme::BAR + theme::OPTIONS;
    for name in WORLD_TOOLS
        .map(|(name, _)| name)
        .into_iter()
        .chain([MORE_TOOLS])
    {
        let rect = h.get_by_label(name).rect();
        assert_eq!(rect.size(), egui::vec2(theme::TOOL, theme::TOOL), "{name}");
        assert!(
            rect.right() < theme::RAIL && rect.top() > top,
            "{name} {rect:?}"
        );
    }
    for (name, tip) in WORLD_TOOLS {
        h.get_by_label(name).hover();
        h.run_steps(60);
        assert!(h.query_by_label(tip).is_some(), "{tip}");
    }
    assert!(h.query_by_label("Line").is_none());
    h.get_by_label("Smooth").click();
    h.run_steps(2);
    assert_eq!(mode(&h), (Tool::Brush, vale_terrain::Mode::Smooth));
    let smooth = h.get_by_label("Smooth");
    assert_eq!(smooth.accesskit_node().toggled(), Some(Toggled::True));
}

#[test]
fn the_keys_choose_the_tools_and_show_the_brush_panel() {
    use vale_terrain::Mode;
    let mut h = harness();
    assert!(!h.state().desk.brush_shows());
    for (key, mode) in [
        (Key::R, Mode::Raise),
        (Key::F, Mode::Lower),
        (Key::S, Mode::Smooth),
        (Key::E, Mode::Flatten),
    ] {
        press(&mut h, key);
        assert_eq!(self::mode(&h), (Tool::Brush, mode));
        assert!(h.state().desk.brush_shows());
    }
    assert!(h.query_by_label(LOCK).is_some());
    // B goes back to the world, and then shows the Brush panel again.
    press(&mut h, Key::B);
    assert!(!h.state().desk.brush_shows());
    assert!(h.query_by_label(LOCK).is_none());
    assert!(h.query_by_label(WORLD_PROPERTIES).is_some());
    press(&mut h, Key::B);
    assert!(h.state().desk.brush_shows());
    // B opens the dock.
    h.state_mut().desk.dock_open = false;
    press(&mut h, Key::B);
    assert!(h.state().desk.dock_open && h.state().desk.brush_shows());
    press(&mut h, Key::H);
    assert_eq!(h.state().globe.tool, Tool::Navigate);
    // The hand has no brush.
    assert!(h.query_by_label(LOCK).is_none());
    press(&mut h, Key::B);
    assert!(h.query_by_label(LOCK).is_none());

    h.get_by_label("Maps").click();
    h.run_steps(3);
    for (name, tip) in [("Select", "Select (V)"), ("Hand", "Hand (H)")] {
        assert!(h.query_by_label(name).is_some(), "{name}");
        h.get_by_label(name).hover();
        h.run_steps(60);
        assert!(h.query_by_label(tip).is_some(), "{tip}");
    }
    press(&mut h, Key::H);
    assert_eq!(h.state().desk.map_tool, MapTool::Hand);
    press(&mut h, Key::V);
    assert_eq!(h.state().desk.map_tool, MapTool::Select);
}

#[test]
fn the_keys_of_the_tools_do_nothing_while_a_text_field_has_the_keyboard() {
    let mut state = state();
    state.workspace = Workspace::Map;
    state.selected_layer = state.doc.project.layers().first().map(|l| l.id);
    let mut h = harness_with(state);
    h.get_by_role(Role::TextInput).focus();
    h.run_steps(2);
    assert!(h.ctx.egui_wants_keyboard_input());
    press(&mut h, Key::H);
    assert_eq!(h.state().desk.map_tool, MapTool::Select);
}

#[test]
fn the_more_button_lists_the_other_modes() {
    let mut h = harness();
    let modes = [
        "Carve", "Ridge", "Deposit", "Erode", "Weather", "Uplift", "Warp",
    ];
    assert!(h.query_by_label("Warp").is_none());
    h.get_by_label(MORE_TOOLS).click();
    h.run_steps(2);
    for mode in modes {
        assert!(h.query_by_label(mode).is_some(), "{mode}");
    }
    h.get_by_label("Warp").click();
    h.run_steps(2);
    assert_eq!(mode(&h), (Tool::Brush, vale_terrain::Mode::Warp));
    assert!(h.query_by_label("Carve").is_none());
    // The More button shows the mode.
    let more = h.get_by_label(MORE_TOOLS);
    assert_eq!(more.accesskit_node().toggled(), Some(Toggled::True));
    assert_eq!(tool_name(h.state()), "Warp");
    h.get_by_label("Raise").click();
    h.run_steps(2);
    let more = h.get_by_label(MORE_TOOLS);
    assert_eq!(more.accesskit_node().toggled(), Some(Toggled::False));
}

/// The field of the options bar with this name.
fn bar_field<'a>(h: &'a App, name: &'a str) -> egui_kittest::Node<'a> {
    let bar = theme::MENU + theme::BAR + theme::OPTIONS;
    let mut fields = h.query_all_by_role_and_label(Role::Slider, name);
    fields.find(|f| f.rect().bottom() < bar).expect(name)
}

fn drag(h: &mut App, from: egui::Pos2, dx: f32) {
    h.hover_at(from);
    h.step();
    h.drag_at(from);
    h.step();
    for i in 1..=4 {
        h.hover_at(from + egui::vec2(dx * i as f32 / 4.0, 0.0));
        h.step();
    }
    h.drop_at(from + egui::vec2(dx, 0.0));
    h.run_steps(2);
}

#[test]
fn the_options_bar_has_the_value_fields_of_the_brush() {
    let mut h = harness();
    let middle = theme::MENU + theme::BAR + theme::OPTIONS / 2.0;
    let values = [("Size", None), ("Strength", Some(1500.0))];
    let values = values
        .into_iter()
        .chain([("Flow", Some(100.0)), ("Hardness", Some(30.0))]);
    let mut left = 0.0;
    for (name, value) in values {
        let field = bar_field(&h, name);
        assert_eq!(field.rect().center().y, middle, "{name}");
        assert!(field.rect().left() > left, "{name}");
        left = field.rect().left();
        if let Some(value) = value {
            assert_eq!(
                field.accesskit_node().numeric_value(),
                Some(value),
                "{name}"
            );
        }
    }
    let button = h.get_by_label(BRUSH_SETTINGS);
    assert!(button.rect().right() > 1200.0);
    button.hover();
    h.run_steps(60);
    assert!(h.query_by_label("Brush settings (B)").is_some());

    // In the warp mode, the strength is a distance.
    h.get_by_label(MORE_TOOLS).click();
    h.run_steps(2);
    h.get_by_label("Warp").click();
    h.run_steps(2);
    let strength = bar_field(&h, "Strength");
    assert_eq!(strength.accesskit_node().numeric_value(), Some(50.0));
    for name in ["Size", "Flow", "Hardness"] {
        bar_field(&h, name);
    }

    h.get_by_label("Hand").click();
    h.run_steps(2);
    assert_eq!(tool_name(h.state()), "Hand");
    assert!(h.query_by_role(Role::Slider).is_none());
    assert!(h.query_by_label(BRUSH_SETTINGS).is_none());
}

#[test]
fn a_drag_on_a_field_or_its_label_changes_the_value() {
    let mut h = harness();
    let hardness = bar_field(&h, "Hardness").rect().center();
    drag(&mut h, hardness, 60.0);
    let brush = h.state().globe.brush;
    assert!((brush.hardness - 0.5).abs() < 0.01, "{}", brush.hardness);
    // The label of Flow is at the left of its box.
    let flow = bar_field(&h, "Flow").rect().left_center() - egui::vec2(12.0, 0.0);
    drag(&mut h, flow, -30.0);
    let brush = h.state().globe.brush;
    assert!((brush.flow - 0.9).abs() < 0.01, "{}", brush.flow);
    // A drag does not open the slider.
    assert_eq!(
        h.query_all_by_role_and_label(Role::Slider, "Flow").count(),
        1
    );
}

#[test]
fn a_click_on_a_field_opens_its_slider_and_a_double_click_types_the_value() {
    // A harness with short steps, so that two clicks make a double click.
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state());
    h.run_steps(3);
    let size = |h: &App| h.state().globe.brush.size_points;
    let before = size(&h);
    bar_field(&h, "Size").click();
    h.run_steps(2);
    let bar = theme::MENU + theme::BAR + theme::OPTIONS;
    let slider = {
        let mut sliders = h.query_all_by_role_and_label(Role::Slider, "Size");
        sliders.find(|s| s.rect().top() > bar).unwrap().rect()
    };
    let field = bar_field(&h, "Size").rect();
    assert!(slider.top() < field.bottom() + 20.0 && slider.left() < field.right());
    // A click at the left end of the slider makes the brush small.
    h.hover_at(slider.left_center() + egui::vec2(1.0, 0.0));
    h.step();
    h.drag_at(slider.left_center() + egui::vec2(1.0, 0.0));
    h.step();
    h.drop_at(slider.left_center() + egui::vec2(1.0, 0.0));
    h.run_steps(2);
    assert!(size(&h) < before);
    assert_eq!(
        h.query_all_by_role_and_label(Role::Slider, "Size").count(),
        2
    );
    // A click outside closes the slider.
    let canvas = h.state().globe.rect.center();
    h.hover_at(canvas);
    h.step();
    h.drag_at(canvas);
    h.step();
    h.drop_at(canvas);
    h.run_steps(2);
    assert_eq!(
        h.query_all_by_role_and_label(Role::Slider, "Size").count(),
        1
    );

    // The clicks before are not part of the double click.
    h.run_steps(30);
    let strength = bar_field(&h, "Strength");
    strength.click();
    strength.click();
    h.run_steps(3);
    let text = h.get_by_role_and_label(Role::TextInput, "Strength");
    assert!(text.is_focused());
    text.type_text("800");
    h.run_steps(1);
    press(&mut h, Key::Enter);
    assert_eq!(h.state().globe.brush.strength_m, 800.0);
    assert!(h.query_by_role(Role::TextInput).is_none());
}

#[test]
fn the_brush_panel_is_the_inspector_until_a_layer_is_chosen() {
    let mut h = harness();
    let heading = |h: &App| {
        let mut headings = h.query_all_by_role(Role::Heading);
        headings.find_map(|n| n.accesskit_node().label()).unwrap()
    };
    assert_eq!(heading(&h), "Earth (sample)");
    h.get_by_label(BRUSH_SETTINGS).click();
    h.run_steps(2);
    assert_eq!(heading(&h), "Brush · Raise");
    let button = h.get_by_label(BRUSH_SETTINGS);
    assert_eq!(button.accesskit_node().toggled(), Some(Toggled::True));
    let role = Role::Button;
    h.get_by_role_and_label(role, "Land").click();
    h.run_steps(2);
    assert_eq!(heading(&h), "Land");
    let button = h.get_by_label(BRUSH_SETTINGS);
    assert_eq!(button.accesskit_node().toggled(), Some(Toggled::False));
    // B shows the Brush panel, and a click on the selected row shows the
    // layer again.
    press(&mut h, Key::B);
    assert_eq!(heading(&h), "Brush · Raise");
    h.get_by_role_and_label(role, "Land").click();
    h.run_steps(2);
    assert_eq!(heading(&h), "Land");
    // A second click selects nothing.
    h.get_by_role_and_label(role, "Land").click();
    h.run_steps(2);
    assert_eq!(heading(&h), "Earth (sample)");
    // B goes back to the subject before the Brush panel.
    h.get_by_role_and_label(role, "Rivers").click();
    h.run_steps(2);
    press(&mut h, Key::B);
    press(&mut h, Key::B);
    assert_eq!(heading(&h), "Rivers");
}

#[test]
fn the_brush_panel_has_the_tip_the_stroke_and_the_limits() {
    let mut h = harness();
    press(&mut h, Key::B);
    for section in ["Tip", "Stroke", "Limits"] {
        assert!(h.query_by_label(section).is_some(), "{section}");
    }
    let dock = 1280.0 - theme::DOCK;
    let in_panel = |h: &App, name: &str| {
        let fields = h.query_all_by_role_and_label(Role::Slider, name);
        fields.filter(|f| f.rect().left() > dock).count()
    };
    for name in ["Size", "Hardness", "Strength", "Flow", "Floor", "Ceiling"] {
        // The slider and the box.
        assert_eq!(in_panel(&h, name), 2, "{name}");
    }
    let floor = |h: &App| {
        let mut fields = h.query_all_by_role_and_label(Role::Slider, "Floor");
        fields.all(|f| !f.accesskit_node().is_disabled())
    };
    assert!(!floor(&h));
    let switch = h.get_by_role_and_label(Role::CheckBox, "Floor");
    assert_eq!(switch.accesskit_node().toggled(), Some(Toggled::False));
    switch.click();
    h.run_steps(2);
    assert!(h.state().globe.brush.floor_on && floor(&h));
    // The panel and the options bar show the same value.
    let hardness = h.query_all_by_role_and_label(Role::Slider, "Hardness");
    let values: Vec<_> = hardness
        .map(|f| f.accesskit_node().numeric_value())
        .collect();
    assert_eq!(values, vec![Some(30.0); 3]);

    // Each mode has its own settings in the Stroke section.
    h.get_by_label(MORE_TOOLS).click();
    h.run_steps(2);
    h.get_by_label("Warp").click();
    h.run_steps(2);
    for name in ["Strength", "Scale", "Seed"] {
        assert_eq!(in_panel(&h, name), 2, "{name}");
    }
    h.get_by_label(MORE_TOOLS).click();
    h.run_steps(2);
    h.get_by_label("Uplift").click();
    h.run_steps(2);
    for name in ["Uplift", "Erosion", "Angle"] {
        assert_eq!(in_panel(&h, name), 2, "{name}");
    }
}

#[test]
fn the_lock_of_the_brush_panel_locks_the_size() {
    let mut h = harness();
    press(&mut h, Key::B);
    let lock = h.get_by_label(LOCK);
    assert_eq!(lock.accesskit_node().toggled(), Some(Toggled::False));
    lock.click();
    h.run_steps(2);
    assert!(h.state().globe.brush.lock.is_some());
    h.get_by_label(LOCK).click();
    h.run_steps(2);
    assert!(h.state().globe.brush.lock.is_none());
}

#[test]
fn the_flatten_level_comes_from_the_globe_or_the_start_of_the_stroke() {
    let mut h = harness();
    press(&mut h, Key::E);
    assert!(h.get_by_label(STROKE_START).accesskit_node().is_disabled());
    h.get_by_label(PICK).click();
    h.run_steps(2);
    assert!(h.state().globe.pick_level);
    let pick = h.get_by_label(PICK);
    assert_eq!(pick.accesskit_node().toggled(), Some(Toggled::True));
    h.state_mut().globe.brush.flatten_level = Some(vale_terrain::meters_to_level(500.0));
    h.run_steps(2);
    h.get_by_label(STROKE_START).click();
    h.run_steps(2);
    assert_eq!(h.state().globe.brush.flatten_level, None);
}

#[test]
fn the_hand_of_the_maps_workspace_selects_nothing() {
    let mut h = harness();
    h.get_by_label("Maps").click();
    h.run_steps(3);
    let click = |h: &mut App| {
        let s = h.state_mut();
        let size = s.canvas_size;
        let p = s
            .pipeline
            .lonlat_to_page(&s.doc, size, [139.75, 35.68])
            .unwrap()
            .unwrap();
        let pos = egui::pos2(
            s.canvas_origin.0 + p.x as f32,
            s.canvas_origin.1 + p.y as f32,
        );
        h.hover_at(pos);
        h.step();
        h.drag_at(pos);
        h.step();
        h.drop_at(pos);
        h.run_steps(3);
    };
    h.get_by_label("Hand").click();
    h.run_steps(2);
    click(&mut h);
    assert_eq!(h.state().selection, None);
    h.get_by_label("Select").click();
    h.run_steps(2);
    click(&mut h);
    assert!(h.state().selection.is_some());
}

#[test]
fn without_the_glass_each_pixel_is_opaque() {
    for workspace in [Workspace::Globe, Workspace::Map] {
        let mut state = state();
        state.workspace = workspace;
        state.globe.tool = Tool::Brush;
        state.desk.toggle_brush();
        let (img, state) = vale_app::headless::ui_png(state, (1280.0, 800.0), 1.0).unwrap();
        assert!(!state.translucent);
        assert!(state.desk.brush_shows());
        let clear = img.pixels().filter(|p| p.0[3] < 255).count();
        assert_eq!(clear, 0, "{workspace:?}");
    }
}

mod start {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    use super::App;
    use eframe::egui;
    use eframe::egui::accesskit::Toggled;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use vale_app::document::Document;
    use vale_app::projects::{self, Location, Locations};
    use vale_app::session::new_project_path;
    use vale_app::ui::browser::Dialog;
    use vale_app::ui::desk::command::REVEAL;
    use vale_app::ui::desk::start::{ALL_WORLDS, OPEN, RECENT, SEARCH, SORT, Sort, TITLE};
    use vale_app::ui::pad::browser::{COPY, DELETE, RENAME, actions_name};
    use vale_app::ui::world_form::NEW_WORLD;
    use vale_app::ui::{Action, AppState, Layout, Screen, draw};
    use vale_store::{Project, ProjectFile, World};

    struct Dirs {
        _root: tempfile::TempDir,
        local: PathBuf,
        cloud: PathBuf,
        state: PathBuf,
    }

    /// The local folder holds Aurelia and Cassia, and the iCloud folder
    /// holds borea. Cassia has the last change and Aurelia the first.
    fn dirs() -> Dirs {
        let root = tempfile::tempdir().unwrap();
        let dir = |name: &str| {
            let dir = root.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            dir
        };
        let dirs = Dirs {
            local: dir("local"),
            cloud: dir("cloud"),
            state: dir("state"),
            _root: root,
        };
        let day = Duration::from_secs(86_400);
        create(&dirs.local, "Aurelia", 1, day);
        create(&dirs.cloud, "borea", 2, day);
        create(&dirs.local, "Cassia", 3, day);
        dirs
    }

    fn create(dir: &Path, name: &str, days: u32, day: Duration) -> PathBuf {
        let path = new_project_path(dir, name);
        let world = World {
            name: name.to_string(),
            ..World::default()
        };
        ProjectFile::create(&path, &Project::new(world)).unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + 20_000 * day + days * day)
            .unwrap();
        path
    }

    fn app(dirs: &Dirs) -> App {
        let mut state = AppState::new(Document::empty()).unwrap();
        state.headless = true;
        state.set_layout(Layout::Desktop);
        let mut locations = Locations::new(Some(dirs.local.clone()), Some(dirs.state.clone()));
        locations.set_cloud(Some(dirs.cloud.clone()));
        state.projects = locations;
        state.show_projects();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1440.0, 900.0))
            .with_pixels_per_point(1.0)
            .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
        h.run_steps(3);
        h
    }

    fn click(h: &mut App, label: &str) {
        h.get_by_label(label).click();
        h.run_steps(3);
    }

    /// The row of the sidebar with this label. A card shows its place too.
    fn side<'a>(h: &'a App, label: &'a str) -> egui_kittest::Node<'a> {
        let rows = h.get_all_by_label(label);
        rows.min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
            .unwrap()
    }

    /// The names of the cards, row by row.
    fn cards(h: &App) -> Vec<String> {
        let mut cards: Vec<_> = h
            .query_all_by_label_contains("Open ")
            .map(|n| (n.rect(), n.accesskit_node().label().unwrap()))
            .collect();
        let key = |r: &egui::Rect| (r.top(), r.left());
        cards.sort_by(|a, b| key(&a.0).partial_cmp(&key(&b.0)).unwrap());
        let name = |(_, label): (egui::Rect, String)| label["Open ".len()..].to_string();
        cards.into_iter().map(name).collect()
    }

    #[test]
    fn the_title_bar_has_the_count_the_search_the_sort_and_new_world() {
        let dirs = dirs();
        let mut h = app(&dirs);
        // The title, and the group of the places in the sidebar.
        assert_eq!(h.query_all_by_label(TITLE).count(), 2);
        h.get_by_label("3 worlds");
        h.get_by_label(SEARCH);
        let sort = h.get_by_label(SORT);
        assert_eq!(
            sort.accesskit_node().value().as_deref(),
            Some("Last changed")
        );
        click(&mut h, NEW_WORLD);
        assert!(h.state().world_form.is_some());
    }

    #[test]
    fn the_grid_has_four_columns_at_1440_points() {
        let dirs = dirs();
        create(&dirs.local, "Dune", 4, Duration::from_secs(86_400));
        create(&dirs.local, "Erebus", 5, Duration::from_secs(86_400));
        let mut h = app(&dirs);
        h.state_mut().show_projects();
        h.run_steps(3);
        let tops: Vec<f32> = h
            .query_all_by_label_contains("Open ")
            .map(|n| n.rect().top())
            .collect();
        assert_eq!(tops.len(), 5);
        let first = tops.iter().copied().fold(f32::INFINITY, f32::min);
        assert_eq!(tops.iter().filter(|t| **t == first).count(), 4);
    }

    #[test]
    fn a_place_of_the_sidebar_shows_only_its_cards() {
        let dirs = dirs();
        let mut h = app(&dirs);
        let selected =
            |h: &App, label: &str| side(h, label).accesskit_node().toggled() == Some(Toggled::True);
        assert!(selected(&h, ALL_WORLDS));
        assert_eq!(cards(&h), ["Cassia", "borea", "Aurelia"]);

        side(&h, Location::Cloud.label(false)).click();
        h.run_steps(3);
        assert_eq!(cards(&h), ["borea"]);
        assert!(!selected(&h, ALL_WORLDS));
        assert!(selected(&h, Location::Cloud.label(false)));
        h.get_by_label("1 world");

        side(&h, Location::Local.label(false)).click();
        h.run_steps(3);
        assert_eq!(cards(&h), ["Cassia", "Aurelia"]);
        click(&mut h, ALL_WORLDS);
        assert_eq!(cards(&h).len(), 3);
    }

    #[test]
    fn the_search_shows_the_cards_with_the_text_in_the_name() {
        let dirs = dirs();
        let mut h = app(&dirs);
        h.get_by_label(SEARCH).focus();
        h.run_steps(2);
        h.get_by_label(SEARCH).type_text("SI");
        h.run_steps(3);
        assert_eq!(h.state().desk.start.search, "SI");
        assert_eq!(cards(&h), ["Cassia"]);
        h.get_by_label(SEARCH).type_text("x");
        h.run_steps(3);
        assert!(cards(&h).is_empty());
        h.get_by_label_contains("No worlds match");
    }

    #[test]
    fn the_sort_menu_changes_the_order_of_the_cards() {
        let dirs = dirs();
        let mut h = app(&dirs);
        assert_eq!(cards(&h), ["Cassia", "borea", "Aurelia"]);
        click(&mut h, SORT);
        click(&mut h, Sort::Name.label());
        assert_eq!(h.state().desk.start.sort, Sort::Name);
        assert_eq!(cards(&h), ["Aurelia", "borea", "Cassia"]);
        let sort = h.get_by_label(SORT);
        assert_eq!(sort.accesskit_node().value().as_deref(), Some("Name"));
        click(&mut h, SORT);
        click(&mut h, Sort::LastChanged.label());
        assert_eq!(cards(&h), ["Cassia", "borea", "Aurelia"]);
    }

    #[test]
    fn the_card_menu_has_five_rows_and_each_one_works() {
        let dirs = dirs();
        let path = dirs.local.join("Aurelia.gpkg");
        let rows = [OPEN, RENAME, COPY, REVEAL, DELETE];
        let menu = actions_name("Aurelia");
        let mut h = app(&dirs);
        for row in rows {
            assert!(h.query_by_label(row).is_none());
        }
        click(&mut h, &menu);
        for row in rows {
            assert!(!h.get_by_label(row).accesskit_node().is_disabled(), "{row}");
        }

        click(&mut h, REVEAL);
        assert_eq!(h.state().actions, [Action::Reveal(path.clone())]);
        assert!(h.query_by_label(REVEAL).is_none(), "the menu closes");
        h.state_mut().actions.clear();

        click(&mut h, &menu);
        click(&mut h, COPY);
        assert_eq!(h.state().actions, [Action::CopyProject(path.clone())]);
        h.state_mut().actions.clear();

        click(&mut h, &menu);
        click(&mut h, RENAME);
        let dialog = h.state().browser.dialog.clone();
        assert!(matches!(dialog, Some(Dialog::Rename { .. })), "{dialog:?}");
        h.state_mut().browser.dialog = None;
        h.run_steps(2);

        click(&mut h, &menu);
        click(&mut h, DELETE);
        let dialog = h.state().browser.dialog.clone();
        assert!(matches!(dialog, Some(Dialog::Delete { .. })), "{dialog:?}");
        h.state_mut().browser.dialog = None;
        h.run_steps(2);

        click(&mut h, &menu);
        click(&mut h, OPEN);
        assert_eq!(h.state().actions, [Action::OpenWorld(path)]);
    }

    #[test]
    fn the_recent_group_shows_after_a_world_opens_and_its_row_opens_it() {
        let dirs = dirs();
        let mut h = app(&dirs);
        assert!(h.query_by_label(RECENT).is_none());

        let path = dirs.local.join("Aurelia.gpkg");
        h.state_mut().run_action(Action::OpenWorld(path.clone()));
        assert_eq!(h.state().screen, Screen::Editor);
        h.state_mut().show_projects();
        h.run_steps(3);
        assert_eq!(projects::recent(&dirs.state).len(), 1);
        h.get_by_label(RECENT);
        // The card and the row of the recent group.
        let rows: Vec<_> = h.get_all_by_label("Aurelia").collect();
        assert_eq!(rows.len(), 2);
        let row = rows
            .into_iter()
            .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()));
        row.unwrap().click();
        h.run_steps(3);
        let absolute = std::path::absolute(&path).unwrap();
        assert_eq!(h.state().actions, [Action::OpenWorld(absolute)]);
    }
}
