//! The Planet workspace with no GPU: the presets, the sliders, the undo
//! steps, and the question before a change that deletes the run.

use eframe::egui;
use egui_kittest::kittest::Queryable;
use isotherm_app::app::{
    ASK_TEXT, ASK_TITLE, ATMOSPHERE, IsothermApp, LOCKED, Layout, ORBIT, REDO, ROTATION, SEA_LEVEL,
    STAR, START_AGAIN, Setting, UNDO, Workspace, preset_name,
};
use isotherm_app::document::Document;
use isotherm_app::headless::harness_with;
use isotherm_app::sim::Simulation;
use isotherm_store::WorldFile;
use isotherm_world::presets::{AtmospherePreset, OrbitPreset, StarPreset};
use isotherm_world::{Planet, World};

type H = egui_kittest::Harness<'static, IsothermApp>;

/// A window that shows all the rows of the panel.
const TALL: (f32, f32) = (1440.0, 1200.0);

/// The app with a new world file in `dir`, in the desktop layout.
fn app_with_file(dir: &std::path::Path, workspace: Workspace) -> IsothermApp {
    let path = dir.join("world.gpkg");
    WorldFile::create(&path, &World::flat("Test", Planet::earth(), 16)).unwrap();
    let mut app = IsothermApp::new(Document::open(&path).unwrap(), Layout::Desktop);
    app.workspace = workspace;
    app
}

fn harness(app: IsothermApp) -> H {
    let mut h = harness_with(TALL, app);
    h.run_steps(3);
    h
}

/// Starts a paused run of the coarsest grid.
fn start_run(h: &mut H) {
    let sim = Simulation::new(&h.state().doc.world, 8, false);
    h.state().run.borrow_mut().sim = Some(sim);
    h.run_steps(2);
}

fn click(h: &mut H, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(2);
}

/// Drags a slider to the right by a quarter of its width.
fn drag_slider(h: &mut H, label: &str) {
    let rect = h.get_by_label(label).rect();
    let from = rect.center();
    let by = egui::vec2(rect.width() / 4.0, 0.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(egui::Event::PointerMoved(from));
    h.event(button(from, true));
    h.step();
    for i in 1..=6 {
        h.event(egui::Event::PointerMoved(from + by * (i as f32 / 6.0)));
        h.step();
    }
    h.event(button(from + by, false));
    h.run_steps(2);
}

fn planet(h: &H) -> Planet {
    h.state().doc.world.planet.clone()
}

/// The planet in the world file.
fn saved_planet(h: &H) -> Planet {
    let path = h.state().doc.path.clone().unwrap();
    WorldFile::open(&path).unwrap().1.planet
}

fn asks(h: &H) -> bool {
    h.query_by_label(ASK_TITLE).is_some()
}

#[test]
fn the_panel_shows_each_section_and_its_presets() {
    let h = harness(
        IsothermApp::sample(Layout::Desktop)
            .map(|mut app| {
                app.workspace = Workspace::Planet;
                app
            })
            .unwrap(),
    );
    for setting in Setting::ALL {
        h.get_by_label(setting.label());
    }
    h.get_by_label(LOCKED);
    for name in StarPreset::ALL.map(StarPreset::name) {
        h.get_by_label(&preset_name(STAR, name));
    }
    for name in OrbitPreset::ALL.map(OrbitPreset::name) {
        h.get_by_label(&preset_name(ORBIT, name));
    }
    h.get_by_label(&preset_name(ROTATION, "Locked"));
    h.get_by_label(&preset_name(ATMOSPHERE, "Thin"));
    assert!(
        h.query_by_label("The planet settings come in a later version.")
            .is_none()
    );
}

#[test]
fn a_preset_fills_the_fields_of_its_group() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(app_with_file(dir.path(), Workspace::Planet));
    click(&mut h, &preset_name(STAR, "Red"));
    let p = planet(&h);
    assert!(StarPreset::Red.matches(&p));
    assert_eq!(p.star.temperature_k, 3200.0);
    assert_eq!(p.orbit, Planet::earth().orbit);
    // The temperate orbit of a red dwarf is close to the star.
    click(&mut h, &preset_name(ORBIT, "Temperate"));
    let p = planet(&h);
    assert!(OrbitPreset::Temperate.matches(&p));
    assert!(p.orbit.semi_major_m < 0.2 * isotherm_world::presets::AU_M);
}

#[test]
fn a_preset_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(app_with_file(dir.path(), Workspace::Planet));
    click(&mut h, &preset_name(ATMOSPHERE, "Thick"));
    assert!(AtmospherePreset::Thick.matches(&planet(&h)));
    assert!(AtmospherePreset::Thick.matches(&saved_planet(&h)));
    click(&mut h, UNDO);
    assert_eq!(planet(&h), Planet::earth());
    assert_eq!(saved_planet(&h), Planet::earth());
    assert!(!h.state().doc.can_undo());
    click(&mut h, REDO);
    assert!(AtmospherePreset::Thick.matches(&planet(&h)));
}

#[test]
fn a_slider_drag_changes_the_planet_and_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(app_with_file(dir.path(), Workspace::Planet));
    drag_slider(&mut h, Setting::Tilt.label());
    let tilt = planet(&h).rotation.tilt_deg;
    assert!(tilt > 40.0, "{tilt}");
    assert_eq!(saved_planet(&h).rotation.tilt_deg, tilt);
    click(&mut h, UNDO);
    assert_eq!(planet(&h).rotation.tilt_deg, 23.44);
    assert!(!h.state().doc.can_undo());
}

#[test]
fn the_sample_world_changes_in_memory() {
    let mut app = IsothermApp::sample(Layout::Desktop).unwrap();
    app.workspace = Workspace::Planet;
    let mut h = harness(app);
    click(&mut h, &preset_name(STAR, "White"));
    assert!(StarPreset::White.matches(&planet(&h)));
    assert!(!h.state().doc.can_undo());
}

#[test]
fn the_locked_switch_hides_the_day_and_a_new_distance_keeps_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(app_with_file(dir.path(), Workspace::Planet));
    h.get_by_label(Setting::DayLength.label());
    click(&mut h, LOCKED);
    assert!(planet(&h).is_locked());
    assert!(h.query_by_label(Setting::DayLength.label()).is_none());
    drag_slider(&mut h, Setting::Distance.label());
    assert!(planet(&h).orbit.semi_major_m > isotherm_world::presets::AU_M);
    assert!(planet(&h).is_locked());
    click(&mut h, LOCKED);
    assert!(!planet(&h).is_locked());
    assert!((planet(&h).solar_day_s() - 86_400.0).abs() < 1e-3);
}

#[test]
fn with_no_run_a_change_asks_no_question() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(app_with_file(dir.path(), Workspace::Planet));
    click(&mut h, &preset_name(ROTATION, "Fast"));
    assert!(!asks(&h));
    assert!(h.state().ask.is_none());
    assert_eq!(planet(&h).rotation.sidereal_day_s, 36_000.0);
}

#[test]
fn with_a_run_cancel_keeps_the_run_and_the_planet() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(app_with_file(dir.path(), Workspace::Planet));
    start_run(&mut h);
    click(&mut h, &preset_name(ATMOSPHERE, "Thin"));
    assert!(asks(&h));
    h.get_by_label(ASK_TEXT);
    assert_eq!(planet(&h), Planet::earth());
    // The card covers the panel.
    click(&mut h, &preset_name(ATMOSPHERE, "Thick"));
    assert!(asks(&h));
    assert_eq!(planet(&h), Planet::earth());
    click(&mut h, "Cancel");
    assert!(!asks(&h));
    assert!(h.state().has_run());
    assert_eq!(planet(&h), Planet::earth());
    assert!(!h.state().doc.can_undo());
}

#[test]
fn with_a_run_start_again_deletes_the_run_and_applies_the_preset() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(app_with_file(dir.path(), Workspace::Planet));
    start_run(&mut h);
    click(&mut h, &preset_name(ATMOSPHERE, "Thin"));
    click(&mut h, START_AGAIN);
    assert!(!asks(&h));
    assert!(!h.state().has_run());
    assert!(AtmospherePreset::Thin.matches(&planet(&h)));
    assert!(AtmospherePreset::Thin.matches(&saved_planet(&h)));
    assert!(h.state().doc.can_undo());
}

#[test]
fn with_a_run_a_slider_drag_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(app_with_file(dir.path(), Workspace::Planet));
    start_run(&mut h);
    drag_slider(&mut h, Setting::Tilt.label());
    assert!(asks(&h));
    assert_eq!(planet(&h).rotation.tilt_deg, 23.44);
    click(&mut h, START_AGAIN);
    assert!(!h.state().has_run());
    // The next drag changes the planet.
    drag_slider(&mut h, Setting::Tilt.label());
    assert!(!asks(&h));
    assert!(planet(&h).rotation.tilt_deg > 40.0);
}

#[test]
fn with_a_run_undo_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(app_with_file(dir.path(), Workspace::Planet));
    click(&mut h, &preset_name(STAR, "Orange"));
    start_run(&mut h);
    click(&mut h, UNDO);
    assert!(asks(&h));
    assert!(StarPreset::Orange.matches(&planet(&h)));
    click(&mut h, START_AGAIN);
    assert!(!h.state().has_run());
    assert_eq!(planet(&h), Planet::earth());
}

#[test]
fn with_a_run_a_drag_of_the_sea_level_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(app_with_file(dir.path(), Workspace::Terrain));
    start_run(&mut h);
    let before = h.state().doc.world.terrain.sea_level_m;
    drag_slider(&mut h, SEA_LEVEL);
    assert!(asks(&h));
    assert_eq!(h.state().doc.world.terrain.sea_level_m, before);
    click(&mut h, "Cancel");
    assert!(h.state().has_run());
    assert_eq!(h.state().doc.world.terrain.sea_level_m, before);
}

#[test]
fn with_a_run_escape_cancels_the_question() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(app_with_file(dir.path(), Workspace::Planet));
    start_run(&mut h);
    let before = planet(&h);
    click(&mut h, &preset_name(STAR, StarPreset::Red.name()));
    assert!(asks(&h));
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(!asks(&h));
    assert_eq!(planet(&h), before);
    assert!(h.state().run.borrow().sim.is_some());
}

#[test]
fn two_presets_with_the_same_orbit_show_the_one_of_the_last_tap() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(app_with_file(dir.path(), Workspace::Planet));
    // Around the yellow star, the temperate orbit is the orbit of the Earth.
    assert_eq!(h.state().orbit_preset(), Some(OrbitPreset::Earth));
    click(&mut h, &preset_name(ORBIT, OrbitPreset::Temperate.name()));
    assert!(OrbitPreset::Earth.matches(&planet(&h)));
    assert_eq!(h.state().orbit_preset(), Some(OrbitPreset::Temperate));
    click(&mut h, &preset_name(ORBIT, OrbitPreset::Earth.name()));
    assert_eq!(h.state().orbit_preset(), Some(OrbitPreset::Earth));
    // A slider that leaves both presets shows none.
    drag_slider(&mut h, Setting::Distance.label());
    assert_eq!(h.state().orbit_preset(), None);
}
