//! The two folders of the projects: the start screen, the form of a new
//! world, the download of a project, and the versions of a project.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use vale_app::document::Document;
use vale_app::files::{Availability, Files, Version};
use vale_app::projects::{Location, Locations};
use vale_app::session::new_project_path;
use vale_app::ui::browser::{Dialog, info_line, keep_name};
use vale_app::ui::world_form::{CREATE, NEW_WORLD};
use vale_app::ui::{AppState, Layout, Screen, draw};
use vale_store::{Project, ProjectFile, World};

type App = Harness<'static, AppState>;

/// A sync service that the test controls.
#[derive(Clone, Default)]
struct FakeFiles(Rc<RefCell<Fake>>);

#[derive(Default)]
struct Fake {
    availability: HashMap<PathBuf, Availability>,
    conflicts: HashMap<PathBuf, Vec<Version>>,
    kept: Vec<(PathBuf, Option<usize>)>,
}

impl Files for FakeFiles {
    fn availability(&self, path: &Path) -> Availability {
        let fake = self.0.borrow();
        fake.availability.get(path).copied().unwrap_or_default()
    }

    fn start_download(&self, path: &Path) -> std::io::Result<()> {
        let mut fake = self.0.borrow_mut();
        fake.availability
            .insert(path.to_path_buf(), Availability::Downloading);
        Ok(())
    }

    fn conflicts(&self, path: &Path) -> Vec<Version> {
        let fake = self.0.borrow();
        fake.conflicts.get(path).cloned().unwrap_or_default()
    }

    fn resolve(&self, path: &Path, keep: Option<usize>) -> std::io::Result<()> {
        let mut fake = self.0.borrow_mut();
        fake.conflicts.remove(path);
        fake.kept.push((path.to_path_buf(), keep));
        Ok(())
    }
}

/// The folders of a test: the local folder, the iCloud folder, and the
/// folder of the state.
struct Dirs {
    _root: tempfile::TempDir,
    local: PathBuf,
    cloud: PathBuf,
    state: PathBuf,
}

impl Dirs {
    fn new() -> Dirs {
        let root = tempfile::tempdir().unwrap();
        let dir = |name: &str| {
            let dir = root.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            dir
        };
        Dirs {
            local: dir("local"),
            cloud: dir("cloud"),
            state: dir("state"),
            _root: root,
        }
    }

    /// `cloud`: the device has iCloud Drive.
    fn locations(&self, cloud: bool) -> Locations {
        let mut locations = Locations::new(Some(self.local.clone()), Some(self.state.clone()));
        locations.set_cloud(cloud.then(|| self.cloud.clone()));
        locations
    }
}

/// The app on the start screen.
fn app(locations: Locations, files: FakeFiles, layout: Layout) -> App {
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.set_layout(layout);
    state.globe.set_face_size(256);
    state.projects = locations;
    state.files = Box::new(files);
    state.show_projects();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1194.0, 834.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(2);
    h
}

/// Runs frames, and runs the actions of the UI as the window does.
fn run(h: &mut App) {
    for _ in 0..3 {
        h.run_steps(2);
        for action in std::mem::take(&mut h.state_mut().actions) {
            h.state_mut().run_action(action);
        }
    }
    h.run_steps(2);
}

fn click(h: &mut App, label: &str) {
    h.get_by_label(label).click();
    run(h);
}

/// Makes a project file of an empty world with this name.
fn create(dir: &Path, name: &str) -> PathBuf {
    let path = new_project_path(dir, name);
    let world = World {
        name: name.to_string(),
        ..World::default()
    };
    ProjectFile::create(&path, &Project::new(world)).unwrap();
    path
}

fn open_path(h: &App) -> PathBuf {
    h.state().session.as_ref().unwrap().path().to_path_buf()
}

#[test]
fn the_start_screen_lists_the_projects_of_both_folders() {
    for layout in [Layout::Pad, Layout::Desktop] {
        let dirs = Dirs::new();
        let local = create(&dirs.local, "Aurelia");
        let cloud = create(&dirs.cloud, "Borea");
        let mut h = app(dirs.locations(true), FakeFiles::default(), layout);

        let entries = h.state().browser.entries.clone();
        let location = |path: &Path| entries.iter().find(|e| e.path == path).unwrap().location;
        assert_eq!(entries.len(), 2);
        assert_eq!(location(&local), Location::Local);
        assert_eq!(location(&cloud), Location::Cloud);
        let pad = layout == Layout::Pad;
        let lines: Vec<String> = entries.iter().map(|e| info_line(e, pad, true).0).collect();
        assert!(lines.iter().any(|l| l.starts_with("iCloud Drive · ")));
        let here = if pad {
            "On this iPad"
        } else {
            "On this computer"
        };
        assert!(lines.iter().any(|l| l.starts_with(here)), "{lines:?}");

        // A project of each folder opens in place.
        click(&mut h, "Open Borea");
        assert_eq!(h.state().screen, Screen::Editor);
        assert_eq!(open_path(&h), cloud);
    }
}

#[test]
fn a_new_world_goes_to_the_location_of_the_form() {
    for layout in [Layout::Pad, Layout::Desktop] {
        let dirs = Dirs::new();
        let local = Location::Local.label(layout == Layout::Pad);
        let mut h = app(dirs.locations(true), FakeFiles::default(), layout);

        // A device with iCloud Drive starts with it.
        click(&mut h, NEW_WORLD);
        click(&mut h, CREATE);
        assert_eq!(open_path(&h).parent(), Some(dirs.cloud.as_path()));

        h.state_mut().show_projects();
        run(&mut h);
        click(&mut h, NEW_WORLD);
        click(&mut h, local);
        click(&mut h, CREATE);
        assert_eq!(open_path(&h).parent(), Some(dirs.local.as_path()));

        // The form keeps the choice, and so does the next run of the app.
        assert_eq!(h.state().projects.choice(), Location::Local);
        assert_eq!(dirs.locations(true).choice(), Location::Local);
        h.state_mut().show_projects();
        run(&mut h);
        click(&mut h, NEW_WORLD);
        click(&mut h, CREATE);
        assert_eq!(open_path(&h).parent(), Some(dirs.local.as_path()));
    }
}

#[test]
fn with_no_icloud_the_app_has_the_local_folder_alone() {
    let dirs = Dirs::new();
    create(&dirs.local, "Aurelia");
    create(&dirs.cloud, "Borea");
    let mut h = app(dirs.locations(false), FakeFiles::default(), Layout::Pad);
    let names: Vec<String> = h
        .state()
        .browser
        .entries
        .iter()
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(names, ["Aurelia"]);

    click(&mut h, NEW_WORLD);
    h.get_by_label("On this iPad");
    assert!(h.query_by_label("iCloud Drive").is_none());
    click(&mut h, CREATE);
    assert_eq!(open_path(&h).parent(), Some(dirs.local.as_path()));
    assert_eq!(h.state().projects.choice(), Location::Local);
}

#[test]
fn a_tap_downloads_a_project_and_then_opens_it() {
    for layout in [Layout::Pad, Layout::Desktop] {
        let dirs = Dirs::new();
        let path = create(&dirs.cloud, "Aurelia");
        let files = FakeFiles::default();
        let set = |availability| {
            let mut fake = files.0.borrow_mut();
            fake.availability.insert(path.clone(), availability);
        };
        set(Availability::InCloud);
        let mut h = app(dirs.locations(true), files.clone(), layout);
        let entry = h.state().browser.entries[0].clone();
        assert_eq!(entry.availability, Availability::InCloud);
        assert_eq!(entry.thumbnail, None);
        assert!(info_line(&entry, true, false).0.contains("Not downloaded"));

        click(&mut h, "Open Aurelia");
        assert_eq!(h.state().screen, Screen::Browser);
        let entry = h.state().browser.entries[0].clone();
        assert_eq!(entry.availability, Availability::Downloading);

        set(Availability::Here);
        let later = Instant::now() + Duration::from_secs(2);
        assert!(h.state_mut().poll_downloads(later));
        run(&mut h);
        assert_eq!(h.state().screen, Screen::Editor, "{layout:?}");
        assert_eq!(open_path(&h), path);
    }
}

#[test]
fn a_placeholder_of_icloud_drive_is_a_project_that_is_not_on_the_device() {
    let dirs = Dirs::new();
    std::fs::write(dirs.cloud.join(".Aurelia.gpkg.icloud"), "x").unwrap();
    std::fs::write(dirs.cloud.join(".notes.txt.icloud"), "x").unwrap();
    let h = app(dirs.locations(true), FakeFiles::default(), Layout::Pad);
    let entries = &h.state().browser.entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "Aurelia");
    assert_eq!(entries[0].path, dirs.cloud.join("Aurelia.gpkg"));
    assert_eq!(entries[0].availability, Availability::InCloud);
    assert_eq!(entries[0].error, None);
}

#[test]
fn you_pick_one_version_of_a_project_that_two_devices_changed() {
    for layout in [Layout::Pad, Layout::Desktop] {
        let dirs = Dirs::new();
        let path = create(&dirs.cloud, "Aurelia");
        let files = FakeFiles::default();
        let other = Version {
            device: "Mac of Zach".to_string(),
            modified: None,
        };
        let versions = vec![other.clone()];
        files
            .0
            .borrow_mut()
            .conflicts
            .insert(path.clone(), versions);
        let mut h = app(dirs.locations(true), files.clone(), layout);
        let entry = h.state().browser.entries[0].clone();
        assert_eq!(
            info_line(&entry, true, false),
            ("2 versions".to_string(), true)
        );

        // The world does not open until one version stays.
        click(&mut h, "Open Aurelia");
        assert_eq!(h.state().screen, Screen::Browser);
        assert!(matches!(
            h.state().browser.dialog,
            Some(Dialog::Versions { .. })
        ));
        assert_eq!(h.get_all_by_label_contains("Keep ").count(), 2);

        click(&mut h, &keep_name(&other.device, None));
        assert_eq!(h.state().browser.dialog, None);
        assert_eq!(files.0.borrow().kept, [(path.clone(), Some(0))]);
        assert!(h.state().browser.entries[0].conflicts.is_empty());
        click(&mut h, "Open Aurelia");
        assert_eq!(h.state().screen, Screen::Editor, "{layout:?}");
    }
}
