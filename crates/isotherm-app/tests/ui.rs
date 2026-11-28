//! The workspace switch, with no GPU.

use eframe::egui;
use egui_kittest::kittest::Queryable;
use isotherm_app::app::{ALL_WORLDS, WORKSPACE_MENU, Workspace};
use isotherm_app::headless::harness;

const NAMES: [&str; 5] = ["Terrain", "Planet", "Simulate", "Explore", "Export"];

#[test]
fn the_ipad_switch_shows_the_five_workspaces_of_a_world() {
    let mut harness = harness((1194.0, 834.0));
    harness.run_steps(2);
    for name in NAMES {
        harness.get_by_label(name);
    }
    assert!(harness.query_by_label("Worlds").is_none());
    assert!(harness.query_by_label(WORKSPACE_MENU).is_none());
}

#[test]
fn the_start_screen_has_no_workspace_switch() {
    for (size, layout) in [
        ((1194.0, 834.0), Layout::Pad),
        ((393.0, 852.0), Layout::Pad),
        ((1440.0, 900.0), Layout::Desktop),
    ] {
        let mut app = isotherm_app::app::IsothermApp::sample(layout).unwrap();
        app.workspace = Workspace::Worlds;
        let mut harness = isotherm_app::headless::harness_with(size, app);
        harness.run_steps(2);
        for name in NAMES.iter().chain(&[WORKSPACE_MENU, ALL_WORLDS]) {
            assert!(harness.query_by_label(name).is_none(), "{name} at {size:?}");
        }
    }
}

#[test]
fn all_worlds_opens_the_start_screen_in_each_layout() {
    for (size, layout) in [
        ((1194.0, 834.0), Layout::Pad),
        ((393.0, 852.0), Layout::Pad),
        ((1440.0, 900.0), Layout::Desktop),
    ] {
        let app = isotherm_app::app::IsothermApp::sample(layout).unwrap();
        let mut harness = isotherm_app::headless::harness_with(size, app);
        harness.run_steps(2);
        harness.get_by_label(ALL_WORLDS).click();
        harness.run_steps(2);
        assert_eq!(harness.state().workspace, Workspace::Worlds, "{size:?}");
    }
}

#[test]
fn a_tap_on_terrain_opens_the_terrain_workspace() {
    let mut harness = harness((1194.0, 834.0));
    harness.run_steps(2);
    harness.get_by_label("Terrain").click();
    harness.run_steps(2);
    assert_eq!(harness.state().workspace, Workspace::Terrain);
    harness.get_by_label(isotherm_app::app::IMPORT);
}

#[test]
fn the_phone_switch_is_one_menu_button() {
    let mut harness = harness((393.0, 852.0));
    harness.run_steps(2);
    assert!(harness.query_by_label("Terrain").is_none());
    harness.get_by_label(WORKSPACE_MENU).click();
    harness.run_steps(2);
    for name in NAMES {
        harness.get_by_label(name);
    }
    harness.get_by_label("Terrain").click();
    harness.run_steps(2);
    assert_eq!(harness.state().workspace, Workspace::Terrain);
    assert!(!harness.state().menu);
}

#[test]
fn the_worlds_workspace_shows_the_build() {
    let mut harness = harness((1194.0, 834.0));
    harness.run_steps(2);
    harness.get_by_label(ALL_WORLDS).click();
    harness.run_steps(2);
    harness.get_by_label(&isotherm_app::build::label());
}

type H = egui_kittest::Harness<'static, isotherm_app::app::IsothermApp>;

fn touch(h: &mut H, phase: egui::TouchPhase, pos: egui::Pos2) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(1),
        phase,
        pos,
        force: None,
    });
    h.event(egui::Event::PointerMoved(pos));
    if phase != egui::TouchPhase::Move {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: phase == egui::TouchPhase::Start,
            modifiers: egui::Modifiers::NONE,
        });
    }
    if phase == egui::TouchPhase::End {
        h.event(egui::Event::PointerGone);
    }
}

/// Drags from the middle of the globe to the right, with a finger or with the mouse.
fn drag_globe(finger: bool) -> bool {
    let mut h = harness((1194.0, 834.0));
    h.run_steps(2);
    let before = h.state().globe.view.rot.0;
    let from = h.state().globe.rect.center();
    let to = from + egui::vec2(150.0, 0.0);
    if finger {
        touch(&mut h, egui::TouchPhase::Start, from);
    } else {
        h.event(egui::Event::PointerMoved(from));
        h.event(egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.step();
    for i in 1..=6 {
        let pos = from.lerp(to, i as f32 / 6.0);
        if finger {
            touch(&mut h, egui::TouchPhase::Move, pos);
        } else {
            h.event(egui::Event::PointerMoved(pos));
        }
        h.step();
    }
    if finger {
        touch(&mut h, egui::TouchPhase::End, to);
    } else {
        h.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(3);
    h.state().globe.view.rot.0 != before
}

#[test]
fn one_finger_turns_the_globe() {
    assert!(drag_globe(true));
}

#[test]
fn the_mouse_turns_the_globe() {
    assert!(drag_globe(false));
}

#[test]
fn the_cards_are_plain_with_no_gpu() {
    let mut harness = harness((1194.0, 834.0));
    harness.run_steps(2);
    assert!(!vale_shell::pad::widgets::is_glass(&harness.ctx));
}

#[test]
fn the_globe_fills_the_bands_outside_the_safe_area() {
    // The outer screen of iPhone Duo in portrait, with the column of the
    // status bar at the right and the band of the home indicator at the bottom.
    let mut h = harness((466.0, 678.0));
    // egui gets the safe area from the system, as on the device.
    h.input_mut().safe_area_insets = Some(egui::SafeAreaInsets(egui::epaint::MarginF32 {
        left: 0.0,
        right: 84.0,
        top: 0.0,
        bottom: 34.0,
    }));
    h.run_steps(2);
    let screen = h.ctx.viewport_rect();
    assert_eq!(h.state().globe.rect, screen);
    // The canvas covers the whole screen, with no clip to the safe area.
    let canvas = vale_shell::pad::theme::CANVAS;
    let covers = h.output().shapes.iter().any(|clipped| {
        let egui::Shape::Rect(rect) = &clipped.shape else {
            return false;
        };
        rect.fill == canvas && rect.rect == screen && clipped.clip_rect.contains_rect(screen)
    });
    assert!(covers, "no canvas over the whole screen");
    // The card stays in the safe area.
    let menu = h.get_by_label(WORKSPACE_MENU).rect();
    assert!(menu.right() <= screen.right() - 84.0);
}

// The import, the sea level, the views, and the layouts.

use isotherm_app::app::{
    ATMOSPHERE, BLACK, IMPORT, IsothermApp, LAYERS_HINT, LOCKED, Layout, PROJECTION, SCALE_HINT,
    SEA_LEVEL, STAR, Setting, UNDO, preset_name,
};
use isotherm_app::globe::WorldView;
use isotherm_app::headless::harness_with;
use vale_shell::pad::metrics::TOUCH;
use vale_shell::pad::number::plus_name;

const PAD: (f32, f32) = (1194.0, 834.0);
const DESKTOP: (f32, f32) = (1440.0, 900.0);
const PHONE: (f32, f32) = (390.0, 844.0);

/// The safe area of an iPhone in portrait.
fn phone_safe_area() -> egui::SafeAreaInsets {
    egui::SafeAreaInsets(egui::epaint::MarginF32 {
        left: 0.0,
        right: 0.0,
        top: 59.0,
        bottom: 34.0,
    })
}

/// The sample world in the pad layout, with `dir` as the folder of the
/// worlds.
fn app_in(dir: &std::path::Path, workspace: Workspace) -> IsothermApp {
    let mut app = IsothermApp::sample(Layout::Pad).unwrap();
    app.folders = Some(isotherm_app::worlds::Folders::new(
        dir.to_path_buf(),
        dir.to_path_buf(),
    ));
    app.state_dir = Some(dir.to_path_buf());
    app.workspace = workspace;
    app
}

/// Writes an 8-bit grey PNG: black in the west half, white in the east half.
fn write_png(path: &std::path::Path) {
    let (w, h) = (64u32, 32u32);
    let pixels: Vec<u8> = (0..w * h)
        .map(|i| if i % w < w / 2 { 0 } else { 255 })
        .collect();
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&pixels)
        .unwrap();
}

/// Writes a Vale project with `count` height layers.
fn write_project(path: &std::path::Path, count: usize) {
    use vale_store::{HEIGHT_UNIT, Project, ProjectFile, World};
    let mut project = Project::new(World::default());
    for i in 0..count {
        let name = format!("Height {}", i + 1);
        project.add_raster(name, 256, HEIGHT_UNIT.into(), -6000.0, 6000.0);
    }
    ProjectFile::create(path, &project).unwrap();
}

/// Drags the mouse from `from` by `by`.
fn mouse_drag(h: &mut H, from: egui::Pos2, by: egui::Vec2) {
    h.event(egui::Event::PointerMoved(from));
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    for i in 1..=6 {
        h.event(egui::Event::PointerMoved(from + by * (i as f32 / 6.0)));
        h.step();
    }
    h.event(egui::Event::PointerButton {
        pos: from + by,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(2);
}

/// Drags the sea level slider to the right.
fn drag_sea(h: &mut H) {
    let rect = h.get_by_label(SEA_LEVEL).rect();
    mouse_drag(h, rect.center(), egui::vec2(rect.width() / 4.0, 0.0));
}

#[test]
fn a_drag_of_the_sea_slider_moves_the_sea_and_uploads_no_texels() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness_with(PAD, app_in(dir.path(), Workspace::Terrain));
    h.run_steps(3);
    let uploads = h.state().globe.uploads();
    let before = h.state().doc.world.terrain.sea_level_m;
    drag_sea(&mut h);
    let after = h.state().doc.world.terrain.sea_level_m;
    assert!(after > before + 100.0, "{before} -> {after}");
    assert_eq!(h.state().globe.uploads(), uploads);
}

#[test]
fn an_8_bit_png_asks_for_black_and_white_and_makes_a_world_file() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("Pangaea.png");
    write_png(&png);
    let mut h = harness_with(PAD, app_in(dir.path(), Workspace::Terrain));
    h.run_steps(2);
    h.state_mut().import_path(&png);
    h.run_steps(3);
    h.get_by_label(SCALE_HINT);
    // Black goes up from −8,000 m to −7,900 m.
    h.get_by_label(&plus_name(BLACK)).click();
    h.run_steps(2);
    h.get_by_label("Import").click();
    h.run_steps(2);
    let app = h.state();
    assert!(app.pending.is_none(), "{:?}", app.status);
    let path = app.doc.path.clone().expect("a world file");
    assert_eq!(path, dir.path().join("Pangaea.gpkg"));
    assert!(path.is_file());
    assert_eq!(app.doc.world.terrain.low_m, -7900.0);
    assert_eq!(app.doc.world.terrain.high_m, 8000.0);
    assert_eq!(app.globe.uploads(), 2);
    // The next start opens this world.
    let last = isotherm_app::worlds::last_world(dir.path()).map(|(path, _)| path);
    assert_eq!(last, Some(path));
    h.get_by_label(IMPORT);
}

#[test]
fn undo_puts_back_the_sea_level_of_before_the_drag() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("Pangaea.png");
    write_png(&png);
    let mut h = harness_with(PAD, app_in(dir.path(), Workspace::Terrain));
    h.run_steps(2);
    h.state_mut().import_path(&png);
    h.run_steps(2);
    h.get_by_label("Import").click();
    h.run_steps(3);
    let before = h.state().doc.world.terrain.sea_level_m;
    drag_sea(&mut h);
    let after = h.state().doc.world.terrain.sea_level_m;
    assert_ne!(before, after);
    h.get_by_label(UNDO).click();
    h.run_steps(2);
    assert_eq!(h.state().doc.world.terrain.sea_level_m, before);
    // The file has the old sea level too.
    let path = h.state().doc.path.clone().unwrap();
    let (_, world) = isotherm_store::WorldFile::open(&path).unwrap();
    assert_eq!(world.terrain.sea_level_m, before);
}

#[test]
fn a_higher_sea_covers_more_of_the_world() {
    let app = IsothermApp::sample(Layout::Pad).unwrap();
    let mut terrain = app.doc.world.terrain;
    let sea = terrain.sea_fraction();
    terrain.sea_level_m = terrain.clamp_sea(5000.0);
    assert_eq!(terrain.sea_level_m, 5000.0);
    assert!(terrain.sea_fraction() > sea + 0.2, "{sea}");
}

#[test]
fn a_vale_project_with_two_height_layers_shows_a_list() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("Vale world.gpkg");
    write_project(&project, 2);
    let worlds = dir.path().join("worlds");
    let mut h = harness_with(PAD, app_in(&worlds, Workspace::Terrain));
    h.run_steps(2);
    h.state_mut().import_path(&project);
    h.run_steps(3);
    h.get_by_label(LAYERS_HINT);
    h.get_by_label("Layer Height 1");
    h.get_by_label("Layer Height 2").click();
    h.run_steps(2);
    let app = h.state();
    assert!(app.pending.is_none(), "{:?}", app.status);
    assert_eq!(app.doc.path, Some(worlds.join("Vale world.gpkg")));
    assert_eq!(app.doc.world.terrain.low_m, -6000.0);
}

#[test]
fn a_vale_project_with_one_height_layer_imports_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("Vale world.gpkg");
    write_project(&project, 1);
    let worlds = dir.path().join("worlds");
    let mut h = harness_with(PAD, app_in(&worlds, Workspace::Terrain));
    h.run_steps(2);
    h.state_mut().import_path(&project);
    h.run_steps(2);
    assert!(h.query_by_label(LAYERS_HINT).is_none());
    let app = h.state();
    assert!(app.pending.is_none(), "{:?}", app.status);
    assert_eq!(app.doc.path, Some(worlds.join("Vale world.gpkg")));
    // A second import replaces the terrain of the open world, as one undo
    // step.
    h.state_mut().import_path(&project);
    h.run_steps(2);
    assert_eq!(h.state().doc.path, Some(worlds.join("Vale world.gpkg")));
    assert!(h.state().doc.can_undo());
    assert_eq!(h.state().globe.uploads(), 3);
    h.get_by_label(UNDO).click();
    h.run_steps(2);
    assert_eq!(h.state().globe.uploads(), 4);
}

#[test]
fn a_file_that_is_not_a_heightmap_shows_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("notes.png");
    std::fs::write(&bad, b"not an image").unwrap();
    let mut h = harness_with(PAD, app_in(dir.path(), Workspace::Terrain));
    h.run_steps(2);
    h.state_mut().import_path(&bad);
    h.run_steps(2);
    let status = h.state().status.clone().expect("an error");
    assert!(status.starts_with("Could not read notes.png"), "{status}");
    h.get_by_label(&status);
}

#[test]
fn two_fingers_zoom_the_globe() {
    let mut h = harness(PAD);
    h.run_steps(2);
    let before = h.state().globe.view.zoom;
    let c = h.state().globe.rect.center();
    let finger = |h: &mut H, id: u64, phase: egui::TouchPhase, pos: egui::Pos2| {
        h.event(egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(id),
            phase,
            pos,
            force: None,
        });
    };
    let at = |d: f32| (c - egui::vec2(d, 0.0), c + egui::vec2(d, 0.0));
    let (a, b) = at(50.0);
    finger(&mut h, 1, egui::TouchPhase::Start, a);
    finger(&mut h, 2, egui::TouchPhase::Start, b);
    h.step();
    for i in 1..=6 {
        let (a, b) = at(50.0 + 15.0 * i as f32);
        finger(&mut h, 1, egui::TouchPhase::Move, a);
        finger(&mut h, 2, egui::TouchPhase::Move, b);
        h.step();
    }
    let (a, b) = at(140.0);
    finger(&mut h, 1, egui::TouchPhase::End, a);
    finger(&mut h, 2, egui::TouchPhase::End, b);
    h.run_steps(2);
    let after = h.state().globe.view.zoom;
    assert!(after > before * 1.5, "{before} -> {after}");
}

#[test]
fn the_mouse_wheel_zooms_the_globe() {
    let mut h = harness(PAD);
    h.run_steps(2);
    let before = h.state().globe.view.zoom;
    let c = h.state().globe.rect.center();
    h.event(egui::Event::PointerMoved(c));
    h.step();
    for _ in 0..4 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 2.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(20);
    let after = h.state().globe.view.zoom;
    assert!(after > before, "{before} -> {after}");
}

#[test]
fn the_flat_switch_and_the_projection_list_change_the_view() {
    let mut app = IsothermApp::sample(Layout::Pad).unwrap();
    app.workspace = Workspace::Explore;
    let mut h = harness_with(PAD, app);
    h.run_steps(2);
    assert!(h.query_by_label(PROJECTION).is_none());
    h.get_by_label("Flat").click();
    h.run_steps(2);
    assert_eq!(h.state().globe.world_view, WorldView::Flat);
    h.get_by_label(PROJECTION).click();
    h.run_steps(2);
    assert!(h.state().projections);
    h.get_by_label("Mercator").click();
    h.run_steps(2);
    let kind = h.state().globe.flat.spec().kind;
    assert_eq!(kind, vale_sphere::ProjectionKind::Mercator);
    assert!(!h.state().projections);
    // A drag pans the map.
    let center = h.state().globe.flat.center;
    let from = h.state().globe.rect.center();
    mouse_drag(&mut h, from, egui::vec2(120.0, 0.0));
    assert_ne!(h.state().globe.flat.center, center);
}

/// Opens `workspace` in a layout and checks its cards and its controls.
fn check_layout(
    size: (f32, f32),
    layout: Layout,
    safe_area: Option<egui::SafeAreaInsets>,
    workspace: Workspace,
) {
    let mut app = IsothermApp::sample(layout).unwrap();
    app.workspace = workspace;
    app.safe_area = safe_area;
    // The start screen lists the worlds of a local folder and of iCloud
    // Drive.
    let dir = tempfile::tempdir().unwrap();
    if workspace == Workspace::Worlds {
        let local = dir.path().join("local");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::create_dir_all(dir.path().join("cloud")).unwrap();
        for name in ["Borea", "Austral", "Meridian"] {
            let world = isotherm_world::World::flat(name, isotherm_world::Planet::earth(), 16);
            isotherm_store::WorldFile::create(local.join(format!("{name}.gpkg")), &world).unwrap();
        }
        let mut folders = isotherm_app::worlds::Folders::new(local, dir.path().join("state"));
        folders.set_cloud(Some(dir.path().join("cloud")));
        app.folders = Some(folders);
    }
    let mut h = harness_with(size, app);
    h.run_steps(4);
    let what = format!("{workspace:?} at {size:?} in {layout:?}");
    let screen = h.ctx.viewport_rect();
    let safe = match safe_area {
        Some(insets) => screen - insets.0,
        None => screen,
    };
    let app = h.state();
    for (name, rect) in &app.cards {
        assert!(
            safe.expand(0.5).contains_rect(*rect),
            "{what}: {name} {rect:?} is not in {safe:?}"
        );
    }
    if let Some(hint) = workspace.hint() {
        h.get_by_label(hint);
    }
    if workspace == Workspace::Worlds {
        for name in ["Borea", "Austral", "Meridian"] {
            h.get_by_label(&isotherm_app::app::open_name(name));
        }
        h.get_by_label(isotherm_app::app::NEW_WORLD);
        h.get_by_label(isotherm_app::worlds::Location::Cloud.label(size.0 < 600.0));
    }
    if workspace == Workspace::Terrain {
        h.get_by_label(IMPORT);
        h.get_by_label(SEA_LEVEL);
    }
    if workspace == Workspace::Planet {
        for setting in Setting::ALL {
            h.get_by_label(setting.label());
        }
        h.get_by_label(LOCKED);
        h.get_by_label(&preset_name(STAR, "Yellow"));
        h.get_by_label(&preset_name(ATMOSPHERE, "Thick"));
    }
    if workspace.has_view() {
        h.get_by_label("Globe");
        h.get_by_label("Flat");
    }
    let sheet = app.cards.iter().any(|(name, _)| *name == "sheet");
    assert_eq!(
        sheet,
        layout == Layout::Pad && size.0 < 600.0 && workspace.has_panel(),
        "{what}"
    );
    if layout == Layout::Pad {
        for control in &app.controls {
            let r = control.rect;
            assert!(
                r.width() >= TOUCH - 0.01 && r.height() >= TOUCH - 0.01,
                "{what}: {} is {:?}",
                control.name,
                r.size()
            );
        }
    }
}

#[test]
fn each_workspace_draws_in_the_desktop_layout() {
    for workspace in Workspace::ALL {
        check_layout(DESKTOP, Layout::Desktop, None, workspace);
    }
}

#[test]
fn each_workspace_draws_in_the_ipad_layout() {
    for workspace in Workspace::ALL {
        check_layout(PAD, Layout::Pad, None, workspace);
    }
}

#[test]
fn each_workspace_draws_in_the_iphone_layout() {
    for workspace in Workspace::ALL {
        check_layout(PHONE, Layout::Pad, Some(phone_safe_area()), workspace);
    }
}

#[test]
fn each_workspace_draws_in_split_view_on_the_ipad() {
    for workspace in Workspace::ALL {
        check_layout((678.0, 834.0), Layout::Pad, None, workspace);
    }
}
