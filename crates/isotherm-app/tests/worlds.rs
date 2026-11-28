//! The worlds of the folders: open, make, rename, duplicate, and delete,
//! with no harness.

use std::path::{Path, PathBuf};

use isotherm_app::app::{IsothermApp, Layout, Workspace};
use isotherm_app::document::Document;
use isotherm_app::worlds::{self, Folders};

/// The sample world with a local folder and a cloud folder in `dir`.
fn app_in(dir: &Path) -> IsothermApp {
    let mut app = IsothermApp::sample(Layout::Pad).unwrap();
    let mut folders = Folders::new(dir.join("local"), dir.join("state"));
    folders.set_cloud(Some(dir.join("cloud")));
    folders.set_choice(worlds::Location::Local);
    app.folders = Some(folders);
    app.state_dir = Some(dir.join("state"));
    app
}

/// A small world file in `dir`.
fn create(dir: &Path, name: &str) -> PathBuf {
    let world = isotherm_world::World::flat(name, isotherm_world::Planet::earth(), 8);
    let (doc, _) = Document::create(world, dir, None, &vale_shell::files::PlainFiles).unwrap();
    doc.path.unwrap()
}

fn has_thumbnail(path: &Path) -> bool {
    isotherm_store::summary(path).unwrap().thumbnail.is_some()
}

#[test]
fn open_world_shows_the_world_and_keeps_it_for_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    let path = create(&dir.path().join("local"), "Borea");
    app.workspace = Workspace::Planet;
    let uploads = app.globe.uploads();
    app.open_world(&path);
    assert_eq!(app.status, None);
    assert_eq!(app.doc.path, Some(path.clone()));
    assert_eq!(app.doc.world.name, "Borea");
    assert_eq!(app.workspace, Workspace::Explore);
    assert!(app.globe.uploads() > uploads);
    assert_eq!(
        worlds::last_path(&dir.path().join("state")),
        Some(path.clone())
    );
    assert!(has_thumbnail(&path));
}

#[test]
fn a_new_world_from_the_earth_can_be_made_while_a_world_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    let borea = create(&dir.path().join("local"), "Borea");
    app.open_world(&borea);
    app.new_world();
    assert_eq!(app.status, None);
    let earth = dir.path().join("local/Earth.gpkg");
    assert_eq!(app.doc.path, Some(earth.clone()));
    assert_eq!(app.doc.world.name, "Earth");
    assert!(has_thumbnail(&earth));
    assert!(has_thumbnail(&borea));
}

#[test]
fn a_new_world_in_icloud_drive_has_a_working_copy() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    app.folders
        .as_mut()
        .unwrap()
        .set_choice(worlds::Location::Cloud);
    app.new_world();
    let earth = dir.path().join("cloud/Earth.gpkg");
    assert_eq!(app.doc.path, Some(earth.clone()));
    let work = dir.path().join("state/work/Earth.gpkg");
    assert_eq!(app.run.borrow().path, Some(work));
    // The picture is in the working copy until the write back.
    assert!(!has_thumbnail(&earth));
    app.flush();
    assert!(has_thumbnail(&earth));
}

#[test]
fn a_world_from_a_vale_project_opens() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    let vale = dir.path().join("Project.gpkg");
    write_vale(&vale);
    app.new_from_vale(&vale);
    assert_eq!(app.status, None);
    assert_eq!(app.doc.world.name, "Aurelia");
    assert_eq!(app.doc.path, Some(dir.path().join("local/Aurelia.gpkg")));

    app.open_file(&vale);
    assert_eq!(
        app.status.as_deref(),
        Some("Project.gpkg is a Vale project. Use New from Vale project…")
    );
}

#[test]
fn a_world_from_outside_the_folders_opens_as_a_copy() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    let outside = create(&dir.path().join("outside"), "Cirra");
    app.open_file(&outside);
    assert_eq!(app.status, None);
    assert_eq!(app.doc.path, Some(dir.path().join("local/Cirra.gpkg")));
    assert!(outside.is_file());
}

#[test]
fn rename_and_duplicate_change_the_open_world_and_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    let path = create(&dir.path().join("local"), "Borea");
    app.open_world(&path);
    app.rename_world(&path, "Delta");
    assert_eq!(app.doc.world.name, "Delta");
    assert_eq!(isotherm_store::summary(&path).unwrap().name, "Delta");

    let copy = app.duplicate_world(&path).unwrap();
    assert_eq!(copy, dir.path().join("local/Delta copy.gpkg"));
    assert_eq!(isotherm_store::summary(&copy).unwrap().name, "Delta copy");
    assert_eq!(app.doc.path, Some(path));

    app.rename_world(&copy, "Echo");
    assert_eq!(isotherm_store::summary(&copy).unwrap().name, "Echo");
    assert_eq!(app.doc.world.name, "Delta");
}

#[test]
fn delete_of_the_open_world_opens_the_sample() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    let path = create(&dir.path().join("local"), "Borea");
    app.open_world(&path);
    app.delete_world(&path);
    assert_eq!(app.status, None);
    assert!(!path.exists());
    assert!(app.doc.file.is_none());
    assert_eq!(app.doc.path, None);
    assert_eq!(app.doc.world.name, "Earth");
    let entries = app.folders.as_ref().unwrap().list(&*app.files);
    assert_eq!(entries, []);
}

/// Writes a Vale project with one height layer.
fn write_vale(path: &Path) {
    use vale_store::{HEIGHT_UNIT, Project, ProjectFile};
    let world = vale_store::World {
        name: "Aurelia".to_owned(),
        ..vale_store::World::default()
    };
    let mut project = Project::new(world);
    project.add_raster("Height".to_owned(), 64, HEIGHT_UNIT.into(), -6000.0, 6000.0);
    ProjectFile::create(path, &project).unwrap();
}
