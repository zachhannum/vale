//! The desktop layout.

use eframe::egui::accesskit::Toggled;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use vale_app::document::Document;
use vale_app::session::Session;
use vale_app::ui::desk::command::Command;
use vale_app::ui::desk::title::NOT_IN_PROTOTYPE;
use vale_app::ui::desk::{Tab, theme};
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
