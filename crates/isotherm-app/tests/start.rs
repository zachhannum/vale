//! The start screen of the Worlds workspace, with no GPU.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui_kittest::kittest::Queryable;
use isotherm_app::app::{
    CANCEL, DELETE, DELETE_BUTTON, DUPLICATE, IsothermApp, Layout, NAME, NEW_FROM_VALE, NEW_WORLD,
    NO_WORLDS, OPEN, Pick, RENAME, RENAME_BUTTON, Workspace, actions_name, open_name,
};
use isotherm_app::headless::harness_with;
use isotherm_app::worlds::Folders;
use isotherm_world::Planet;

type H = egui_kittest::Harness<'static, IsothermApp>;

const PAD: (f32, f32) = (1194.0, 834.0);

/// A world file named `name` in `dir`.
fn create(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(format!("{name}.gpkg"));
    let world = isotherm_world::World::flat(name, Planet::earth(), 16);
    isotherm_store::WorldFile::create(&path, &world).unwrap();
    path
}

/// The start screen of the sample world, with the worlds of `dir`.
fn start(dir: &Path) -> H {
    let mut app = IsothermApp::sample(Layout::Pad).unwrap();
    app.folders = Some(Folders::new(dir.to_path_buf(), dir.to_path_buf()));
    app.state_dir = Some(dir.to_path_buf());
    app.workspace = Workspace::Worlds;
    let mut h = harness_with(PAD, app);
    h.run_steps(3);
    h
}

fn worlds(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .filter(|e| {
            let path = e.as_ref().unwrap().path();
            path.extension().is_some_and(|x| x == "gpkg")
        })
        .count()
}

fn menu(h: &mut H, world: &str) {
    h.get_by_label(&actions_name(world)).click();
    h.run_steps(3);
}

#[test]
fn the_grid_shows_each_world() {
    let dir = tempfile::tempdir().unwrap();
    create(dir.path(), "A");
    create(dir.path(), "B");
    let h = start(dir.path());
    h.get_by_label(&open_name("A"));
    h.get_by_label(&open_name("B"));
    assert!(h.query_by_label(NO_WORLDS).is_none());
}

#[test]
fn a_tap_on_a_card_opens_the_world() {
    let dir = tempfile::tempdir().unwrap();
    create(dir.path(), "A");
    let b = create(dir.path(), "B");
    let mut h = start(dir.path());
    h.get_by_label(&open_name("B")).click();
    h.run_steps(3);
    assert_eq!(h.state().doc.path, Some(b));
    assert_ne!(h.state().workspace, Workspace::Worlds);
}

#[test]
fn the_actions_menu_has_rename_duplicate_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    create(dir.path(), "A");
    let mut h = start(dir.path());
    menu(&mut h, "A");
    h.get_by_label(RENAME);
    h.get_by_label(DUPLICATE);
    h.get_by_label(DELETE);
}

#[test]
fn rename_in_the_dialog_changes_the_name_of_the_world() {
    let dir = tempfile::tempdir().unwrap();
    let a = create(dir.path(), "A");
    let mut h = start(dir.path());
    menu(&mut h, "A");
    h.get_by_label(RENAME).click();
    h.run_steps(3);
    h.get_by_label(NAME);
    // The field has the focus and its text is selected, so the text
    // replaces the old name.
    h.event(egui::Event::Text("Borea".to_owned()));
    h.run_steps(2);
    h.get_by_label(RENAME_BUTTON).click();
    h.run_steps(3);
    assert_eq!(h.state().status, None);
    assert_eq!(isotherm_store::summary(&a).unwrap().name, "Borea");
    assert!(h.state().start.dialog.is_none());
    h.get_by_label(&open_name("Borea"));
}

#[test]
fn rename_with_an_empty_name_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let a = create(dir.path(), "A");
    let mut h = start(dir.path());
    menu(&mut h, "A");
    h.get_by_label(RENAME).click();
    h.run_steps(3);
    h.key_press(egui::Key::Backspace);
    h.run_steps(2);
    h.get_by_label(RENAME_BUTTON).click();
    h.run_steps(2);
    assert!(h.state().start.dialog.is_some());
    h.get_by_label(CANCEL).click();
    h.run_steps(2);
    assert!(h.state().start.dialog.is_none());
    assert_eq!(isotherm_store::summary(&a).unwrap().name, "A");
}

#[test]
fn duplicate_adds_a_world_file() {
    let dir = tempfile::tempdir().unwrap();
    create(dir.path(), "A");
    let mut h = start(dir.path());
    menu(&mut h, "A");
    h.get_by_label(DUPLICATE).click();
    h.run_steps(3);
    assert_eq!(worlds(dir.path()), 2);
    h.get_by_label(&open_name("A copy"));
}

#[test]
fn delete_after_the_question_removes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let a = create(dir.path(), "A");
    let mut h = start(dir.path());
    menu(&mut h, "A");
    h.get_by_label(DELETE).click();
    h.run_steps(3);
    assert!(a.exists());
    h.get_by_label(DELETE_BUTTON).click();
    h.run_steps(3);
    assert!(!a.exists());
    h.get_by_label(NO_WORLDS);
}

#[test]
fn new_world_makes_an_earth_while_a_world_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let a = create(dir.path(), "A");
    let mut h = start(dir.path());
    h.state_mut().open_world(&a);
    h.state_mut().workspace = Workspace::Worlds;
    h.run_steps(3);
    h.get_by_label(NEW_WORLD).click();
    h.run_steps(3);
    let earth = dir.path().join("Earth.gpkg");
    assert_eq!(h.state().doc.path, Some(earth.clone()));
    assert_eq!(isotherm_store::summary(&earth).unwrap().name, "Earth");
    assert_eq!(worlds(dir.path()), 2);
}

#[test]
fn the_pick_buttons_ask_for_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = start(dir.path());
    h.get_by_label(NEW_FROM_VALE).click();
    h.run_steps(1);
    assert_eq!(h.state().pick, Some(Pick::ValeProject));
    h.state_mut().pick = None;
    h.get_by_label(OPEN).click();
    h.run_steps(1);
    assert_eq!(h.state().pick, Some(Pick::Open));
}

#[test]
fn no_worlds_shows_the_empty_state() {
    let dir = tempfile::tempdir().unwrap();
    let h = start(dir.path());
    h.get_by_label(NO_WORLDS);
}
