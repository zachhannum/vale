use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use vale_app::cli::Args;
use vale_app::document::Document;
use vale_app::projects;
use vale_app::session::new_project_path;
use vale_app::ui::browser::Dialog;
use vale_app::ui::pad::WORLDS;
use vale_app::ui::pad::browser as pad_browser;
use vale_app::ui::pad::geometry::TOUCH;
use vale_app::ui::panels::{ALL_WORLDS, FILE_MENU};
use vale_app::ui::world_form::{CREATE, NEW_WORLD};
use vale_app::ui::{Action, AppState, Layout, Screen, draw};
use vale_store::{Project, ProjectFile, World};
use vale_terrain::math::lonlat_to_dir;
use vale_terrain::{Heightmap, Mode, Stamp};

type App = Harness<'static, AppState>;

/// The app on the start screen of a projects folder.
fn app(dir: &Path, layout: Layout) -> App {
    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.set_layout(layout);
    state.globe.set_face_size(256);
    state.projects = Some(dir.to_path_buf());
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

/// The first text field of the screen.
fn field(h: &App) -> egui_kittest::Node<'_> {
    let mut fields = h.get_all_by_role(egui::accesskit::Role::TextInput);
    fields.next().unwrap()
}

/// Puts this text in the first text field, in place of its text.
fn type_name(h: &mut App, text: &str) {
    field(h).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(2);
    field(h).type_text(text);
    run(h);
}

/// Makes a world of this name with the form of a new world.
fn new_world(h: &mut App, name: &str) {
    click(h, NEW_WORLD);
    type_name(h, name);
    click(h, CREATE);
    assert_eq!(h.state().screen, Screen::Editor);
    assert_eq!(h.state().doc.project.world.name, name);
}

/// Clicks the button with this label in the card of the project at `index`.
fn click_in_card(h: &mut App, label: &str, index: usize) {
    let mut buttons: Vec<_> = h.get_all_by_label(label).collect();
    assert_eq!(buttons.len(), h.state().browser.entries.len());
    // The cards are in one row, in the order of the entries.
    buttons.sort_by(|a, b| a.rect().left().total_cmp(&b.rect().left()));
    buttons[index].click();
    run(h);
}

/// An action on a project of the start screen.
#[derive(Clone, Copy)]
enum CardAction {
    Rename,
    Copy,
    Delete,
}

/// Starts an action on the project with this name. The desktop layout has a
/// button for each action. The iPad layout has a menu of the actions.
fn card_action(h: &mut App, world: &str, action: CardAction) {
    let (desktop, pad) = match action {
        CardAction::Rename => ("Rename", pad_browser::RENAME),
        CardAction::Copy => ("Copy", pad_browser::COPY),
        CardAction::Delete => ("Delete", pad_browser::DELETE),
    };
    match h.state().layout {
        Layout::Desktop => {
            let entries = &h.state().browser.entries;
            let index = entries.iter().position(|e| e.name == world).unwrap();
            click_in_card(h, desktop, index);
        }
        Layout::Pad => {
            click(h, &pad_browser::actions_name(world));
            assert!(h.state().browser.menu.is_some());
            click(h, pad);
            assert_eq!(h.state().browser.menu, None);
        }
    }
}

/// Goes from the editor to the start screen with the control of the layout.
fn all_worlds(h: &mut App) {
    match h.state().layout {
        Layout::Desktop => click(h, FILE_MENU),
        Layout::Pad => click(h, WORLDS),
    }
    click(h, ALL_WORLDS);
    assert_eq!(h.state().screen, Screen::Browser);
    assert!(h.state().session.is_none());
}

/// Puts this text in the field of the rename dialog and saves it.
fn save_name(h: &mut App, text: &str) {
    let dialog = &h.state().browser.dialog;
    assert!(matches!(dialog, Some(Dialog::Rename { .. })), "{dialog:?}");
    type_name(h, text);
    click(h, "Save name");
    assert_eq!(h.state().browser.dialog, None);
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = projects::list(dir).into_iter().map(|e| e.name).collect();
    names.sort();
    names
}

fn open_path(h: &App) -> PathBuf {
    h.state().session.as_ref().unwrap().path().to_path_buf()
}

/// One stroke on the heightmap, with no GPU.
fn paint(map: &mut Heightmap) {
    map.begin_stroke();
    map.stamp(&Stamp {
        center: lonlat_to_dir(15.0, 25.0),
        radius: 0.4,
        hardness: 0.3,
        flow: 1.0,
        mode: Mode::Raise,
        level: 0,
        strength: 9000.0,
        talus: 0.0,
    });
    assert!(map.end_stroke());
}

#[test]
fn the_start_screen_lists_each_project_with_its_picture_name_and_date() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = app(dir.path(), Layout::Desktop);
    h.get_by_label_contains("No worlds yet");

    for name in ["Aurelia", "Borea"] {
        let path = create(dir.path(), name);
        h.state_mut().run_action(Action::OpenWorld(path));
        assert_eq!(h.state().screen, Screen::Editor);
    }
    h.state_mut().show_projects();
    run(&mut h);

    let today = projects::date_text(SystemTime::now());
    assert_eq!(h.get_all_by_label(today.as_str()).count(), 2);
    for name in ["Aurelia", "Borea"] {
        h.get_by_label(name);
        h.get_by_label(format!("Open {name}").as_str());
    }
    let browser = &h.state().browser;
    assert!(browser.entries.iter().all(|e| e.thumbnail.is_some()));
    assert_eq!(browser.picture_count(), 2);
}

#[test]
fn the_picture_shows_the_painted_world() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = app(dir.path(), Layout::Desktop);
    new_world(&mut h, "Aurelia");
    let path = open_path(&h);
    h.state_mut().show_projects();
    let empty = vale_store::summary(&path).unwrap().thumbnail.unwrap();
    let changed = std::fs::metadata(&path).unwrap().modified().unwrap();

    // A project that opens and closes with no edit keeps its file.
    h.state_mut().run_action(Action::OpenWorld(path.clone()));
    h.state_mut().show_projects();
    let unchanged = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(unchanged, changed);

    h.state_mut().run_action(Action::OpenWorld(path.clone()));
    paint(&mut h.state_mut().globe.map);
    h.state_mut().show_projects();
    let painted = vale_store::summary(&path).unwrap().thumbnail.unwrap();
    assert_ne!(painted, empty);
}

#[test]
fn new_open_rename_copy_and_delete_work_in_both_layouts() {
    for layout in [Layout::Desktop, Layout::Pad] {
        let dir = tempfile::tempdir().unwrap();
        let mut h = app(dir.path(), layout);

        // New: the app makes two worlds, and each one opens in the editor.
        new_world(&mut h, "Aurelia");
        let first = open_path(&h);
        paint(&mut h.state_mut().globe.map);
        let painted = h.state().globe.map.allocated_tiles();
        assert!(painted > 0);
        all_worlds(&mut h);
        new_world(&mut h, "Borea");
        assert_ne!(open_path(&h), first);
        assert_eq!(h.state().globe.map.allocated_tiles(), 0, "a new world");
        all_worlds(&mut h);
        assert_eq!(names(dir.path()), ["Aurelia", "Borea"]);

        // Rename.
        card_action(&mut h, "Aurelia", CardAction::Rename);
        save_name(&mut h, "Cassia");
        assert_eq!(names(dir.path()), ["Borea", "Cassia"]);
        assert_eq!(vale_store::summary(&first).unwrap().name, "Cassia");

        // Copy.
        card_action(&mut h, "Cassia", CardAction::Copy);
        assert_eq!(names(dir.path()), ["Borea", "Cassia", "Cassia copy"]);

        // Open: the copy has the name and the heightmap of its source.
        click(&mut h, "Open Cassia copy");
        assert_eq!(h.state().screen, Screen::Editor);
        assert_ne!(open_path(&h), first);
        assert_eq!(h.state().doc.project.world.name, "Cassia copy");
        assert_eq!(h.state().globe.map.allocated_tiles(), painted);
        all_worlds(&mut h);

        // Delete.
        card_action(&mut h, "Cassia", CardAction::Delete);
        click(&mut h, "Delete world");
        assert_eq!(names(dir.path()), ["Borea", "Cassia copy"]);
        assert!(!first.exists());
    }
}

#[test]
fn delete_asks_before_it_removes_the_project() {
    for layout in [Layout::Pad, Layout::Desktop] {
        let dir = tempfile::tempdir().unwrap();
        let path = create(dir.path(), "Aurelia");
        let mut h = app(dir.path(), layout);

        card_action(&mut h, "Aurelia", CardAction::Delete);
        let (question, cancel) = match layout {
            Layout::Desktop => ("Delete Aurelia?", "Cancel".to_string()),
            Layout::Pad => (
                "\"Aurelia\" and its undo steps",
                pad_browser::close_name(pad_browser::DELETE_TITLE),
            ),
        };
        h.get_by_label_contains(question);
        assert!(path.exists());
        click(&mut h, &cancel);
        assert_eq!(h.state().browser.dialog, None);
        assert!(path.exists());

        card_action(&mut h, "Aurelia", CardAction::Delete);
        assert!(path.exists());
        click(&mut h, "Delete world");
        assert!(!path.exists(), "{layout:?}");
        h.get_by_label_contains("No worlds yet");
    }
}

/// Each control of the start screen of the iPad is large enough for a
/// finger, and it is on the screen.
#[test]
fn the_ipad_start_screen_has_touch_size_controls() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["Aurelia", "Borea", "Cassia"] {
        create(dir.path(), name);
    }
    for size in [(1194.0, 834.0), (744.0, 1133.0), (390.0, 700.0)] {
        let mut h = app(dir.path(), Layout::Pad);
        h.set_size(egui::vec2(size.0, size.1));
        run(&mut h);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0, size.1));
        let check = |h: &App, expected: &[String]| {
            let controls = &h.state().pad.controls;
            let names: Vec<&str> = controls.iter().map(|c| c.name.as_str()).collect();
            for name in expected {
                assert!(names.contains(&name.as_str()), "{name} in {names:?}");
            }
            for control in controls {
                let rect = control.rect;
                assert!(
                    rect.width() >= TOUCH && rect.height() >= TOUCH,
                    "{control:?}"
                );
                assert!(screen.contains_rect(rect), "{control:?} at {size:?}");
            }
        };
        let mut list = vec![NEW_WORLD.to_string()];
        for name in ["Aurelia", "Borea", "Cassia"] {
            list.push(pad_browser::open_name(name));
            list.push(pad_browser::actions_name(name));
        }
        check(&h, &list);

        click(&mut h, &pad_browser::actions_name("Aurelia"));
        let rows = [pad_browser::RENAME, pad_browser::COPY, pad_browser::DELETE];
        check(&h, &rows.map(str::to_string));
        click(&mut h, pad_browser::RENAME);
        let close = pad_browser::close_name(pad_browser::RENAME_TITLE);
        let names = [pad_browser::NAME, pad_browser::SAVE_NAME, close.as_str()];
        check(&h, &names.map(str::to_string));
        // The keyboard of the system covers the lower half of the screen.
        let save = h
            .state()
            .pad
            .controls
            .iter()
            .find(|c| c.name == pad_browser::SAVE_NAME);
        assert!(save.unwrap().rect.bottom() < size.1 / 2.0, "{size:?}");
    }
}

#[test]
fn the_file_menu_has_the_actions_and_the_recent_list() {
    let dir = tempfile::tempdir().unwrap();
    let other = create(dir.path(), "Borea");
    let mut h = app(dir.path(), Layout::Desktop);
    click(&mut h, "Open Borea");
    all_worlds(&mut h);
    new_world(&mut h, "Aurelia");
    let path = open_path(&h);
    assert_eq!(projects::recent(dir.path()), [path.clone(), other.clone()]);

    // Rename: the open project gets the name, and a save writes it.
    click(&mut h, FILE_MENU);
    click(&mut h, "Rename…");
    save_name(&mut h, "Cassia");
    assert_eq!(h.state().doc.project.world.name, "Cassia");
    h.state_mut().save_tick(false, std::time::Instant::now());
    let later = std::time::Instant::now() + vale_app::session::SAVE_DELAY;
    h.state_mut().save_tick(false, later);
    assert_eq!(vale_store::summary(&path).unwrap().name, "Cassia");

    // Copy: the copy has the changes of the open project.
    click(&mut h, FILE_MENU);
    click(&mut h, "Make a copy");
    assert_eq!(names(dir.path()), ["Borea", "Cassia", "Cassia copy"]);
    assert_eq!(open_path(&h), path);

    // Open recent.
    click(&mut h, FILE_MENU);
    h.get_by_label_contains("Open recent").click();
    run(&mut h);
    click(&mut h, "Borea");
    assert_eq!(open_path(&h), other);
    assert_eq!(h.state().doc.project.world.name, "Borea");

    // Delete: the app asks, removes the open project, and shows the list.
    click(&mut h, FILE_MENU);
    click(&mut h, "Delete…");
    assert!(other.exists());
    click(&mut h, "Delete world");
    assert!(!other.exists());
    assert_eq!(h.state().screen, Screen::Browser);
    assert_eq!(names(dir.path()), ["Cassia", "Cassia copy"]);
}

#[test]
fn the_window_starts_on_the_start_screen_with_no_project() {
    use clap::Parser;
    let args = |flags: &[&str]| Args::parse_from(["vale-app"].iter().chain(flags));
    assert!(args(&[]).starts_on_projects(true));
    assert!(!args(&[]).starts_on_projects(false));
    assert!(!args(&["--project", "a.gpkg"]).starts_on_projects(true));
    assert!(!args(&["land.geojson"]).starts_on_projects(true));
    assert!(args(&["--start-screen"]).starts_on_projects(false));
    let folder = |flags: &[&str]| args(flags).projects_folder(true);
    assert_eq!(folder(&["--projects-dir", "worlds"]), Some("worlds".into()));
    assert_eq!(folder(&["--project", "dir/a.gpkg"]), Some("dir".into()));
    assert_eq!(folder(&["land.geojson"]), None);
}
