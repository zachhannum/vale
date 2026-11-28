//! A run that the app saves, and that continues when the world opens again.

use isotherm_app::app::{IsothermApp, Layout, Workspace};
use isotherm_app::document::Document;
use isotherm_app::headless::harness_with;
use isotherm_store::WorldFile;
use isotherm_world::{Planet, World};

fn app(path: &std::path::Path) -> IsothermApp {
    let mut app = IsothermApp::new(Document::open(path).unwrap(), Layout::Pad);
    app.workspace = Workspace::Simulate;
    app
}

fn steps(app: &IsothermApp) -> u64 {
    app.run
        .borrow()
        .sim
        .as_ref()
        .map_or(0, |sim| sim.step_count())
}

#[test]
fn a_saved_run_continues_when_the_world_opens_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.gpkg");
    WorldFile::create(&path, &World::flat("Resume", Planet::earth(), 16)).unwrap();

    let mut h = harness_with((1194.0, 834.0), app(&path));
    h.state_mut().toggle_run();
    while steps(h.state()) < 40 {
        h.step();
    }
    // The save of the life cycle: the run stays on.
    h.state().save_run().unwrap();
    assert!(h.state().running());
    let saved = h
        .state()
        .run
        .borrow_mut()
        .sim
        .as_mut()
        .unwrap()
        .snapshot()
        .unwrap();
    drop(h);

    let mut h = harness_with((1194.0, 834.0), app(&path));
    let restored = h
        .state()
        .run
        .borrow_mut()
        .sim
        .as_mut()
        .unwrap()
        .snapshot()
        .unwrap();
    assert_eq!(restored, saved);
    assert!(h.state().means().is_some());
    h.state_mut().toggle_run();
    h.run_steps(2);
    assert!(steps(h.state()) > saved.step);
}

#[test]
fn run_years_saves_the_run_and_gives_the_yearly_means() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.gpkg");
    WorldFile::create(&path, &World::flat("Years", Planet::earth(), 16)).unwrap();
    let mut app = app(&path);
    let text = app.run_years(1, 8).unwrap();
    assert!(text.starts_with("year 1: global mean "), "{text}");
    let (file, _) = WorldFile::open(&path).unwrap();
    let run = file.load_run().unwrap().expect("a saved run");
    assert_eq!(run.step, 360);
    assert_eq!(run.face_size, 8);
}

#[test]
fn a_change_of_the_sea_level_deletes_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.gpkg");
    WorldFile::create(&path, &World::flat("Sea", Planet::earth(), 16)).unwrap();
    let mut app = app(&path);
    app.run_years(1, 8).unwrap();
    app.doc.world.terrain.sea_level_m = 100.0;
    app.doc.save_sea_level(0.0).unwrap();
    app.undo();
    assert!(app.run.borrow().sim.is_none());
    assert!(app.means().is_none());
    let (file, _) = WorldFile::open(&path).unwrap();
    assert_eq!(file.load_run().unwrap(), None);
}
