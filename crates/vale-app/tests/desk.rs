//! The desktop layout.

use eframe::egui::accesskit::{Role, Toggled};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use vale_app::document::Document;
use vale_app::globe::Tool;
use vale_app::session::Session;
use vale_app::ui::desk::command::Command;
use vale_app::ui::desk::options::{BRUSH_BUTTON, tool_name};
use vale_app::ui::desk::rail::MORE_TOOLS;
use vale_app::ui::desk::title::NOT_IN_PROTOTYPE;
use vale_app::ui::desk::{MapTool, Tab, theme};
use vale_app::ui::{AppState, Layout, Workspace, draw};

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
    for label in [
        "Earth (sample)",
        "World",
        "Maps",
        "Globe",
        "Flat",
        "Undo",
        "Redo",
    ] {
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
    h.get_by_label("Toolbox").click();
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
    h.get_by_label("Dock").click();
    h.run_steps(2);
    assert!(!h.state().desk.dock_open);
    let dock = h.get_by_label("Dock");
    assert_eq!(dock.accesskit_node().toggled(), Some(Toggled::False));
    dock.click();
    h.run_steps(2);
    assert!(h.state().desk.dock_open);
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
fn the_keys_choose_the_tools_and_open_the_brush_popover() {
    use vale_terrain::Mode;
    let mut h = harness();
    for (key, mode) in [
        (Key::R, Mode::Raise),
        (Key::F, Mode::Lower),
        (Key::S, Mode::Smooth),
        (Key::E, Mode::Flatten),
    ] {
        press(&mut h, key);
        assert_eq!(self::mode(&h), (Tool::Brush, mode));
    }
    press(&mut h, Key::B);
    assert!(h.state().desk.brush_open);
    assert!(h.query_by_label("Lock size").is_some());
    press(&mut h, Key::B);
    assert!(!h.state().desk.brush_open);
    press(&mut h, Key::B);
    press(&mut h, Key::Escape);
    assert!(!h.state().desk.brush_open);
    press(&mut h, Key::H);
    assert_eq!(h.state().globe.tool, Tool::Navigate);
    // The hand has no brush.
    press(&mut h, Key::B);
    assert!(!h.state().desk.brush_open);

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

#[test]
fn the_options_bar_has_the_settings_of_the_brush() {
    let mut h = harness();
    h.get_by_label("Raise").click();
    h.run_steps(2);
    let middle = theme::MENU + theme::BAR + theme::OPTIONS / 2.0;
    for name in ["Size", "Flow", "Hardness"] {
        let slider = h.get_by_role_and_label(Role::Slider, name);
        assert_eq!(slider.rect().center().y, middle, "{name}");
    }
    let button = h.get_by_label(BRUSH_BUTTON);
    assert!(button.rect().right() > 1200.0);
    button.hover();
    h.run_steps(60);
    assert!(h.query_by_label("Brush… (B)").is_some());

    h.get_by_label("Hand").click();
    h.run_steps(2);
    assert_eq!(tool_name(h.state()), "Hand");
    assert!(h.query_by_role(Role::Slider).is_none());
    assert!(h.query_by_label(BRUSH_BUTTON).is_none());
}

#[test]
fn the_brush_button_opens_the_popover_with_the_other_settings() {
    let mut h = harness();
    h.get_by_label("Raise").click();
    h.run_steps(2);
    assert!(h.query_by_label("Strength").is_none());
    h.get_by_label(BRUSH_BUTTON).click();
    h.run_steps(2);
    let button = h.get_by_label(BRUSH_BUTTON);
    assert_eq!(button.accesskit_node().toggled(), Some(Toggled::True));
    for label in ["Strength", "Floor", "Ceiling", "Lock size"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    // The popover is below the button.
    let lock = h.get_by_label("Lock size").rect();
    assert!(lock.top() > theme::MENU + theme::BAR + theme::OPTIONS);
    assert!(lock.left() > 1280.0 - 340.0, "{lock:?}");

    // A click outside the popover closes it.
    let canvas = h.state().globe.rect.center();
    h.hover_at(canvas);
    h.step();
    h.drag_at(canvas);
    h.step();
    h.drop_at(canvas);
    h.run_steps(2);
    assert!(!h.state().desk.brush_open);
    assert!(h.query_by_label("Lock size").is_none());
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
