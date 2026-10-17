use std::path::Path;

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kurbo::Point;
use vale_app::document::Document;
use vale_app::headless;
use vale_app::projects::Locations;
use vale_app::session::Session;
use vale_app::ui::desk::command::Command;
use vale_app::ui::world_form::{CREATE, FormMode, NEW_WORLD, WorldForm};
use vale_app::ui::{Action, AppState, ExportFormat, Workspace, draw, is_heightmap_path};
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::{Project, ProjectFile, Units, World};
use vale_terrain::Heightmap;

const SIZE: (f64, f64) = (1280.0, 800.0);

fn state_with(doc: Document) -> AppState {
    let mut state = AppState::new(doc).unwrap();
    state.headless = true;
    state.workspace = Workspace::Map;
    state
}

fn harness_with(doc: Document) -> Harness<'static, AppState> {
    Harness::builder()
        .with_size(egui::vec2(SIZE.0 as f32, SIZE.1 as f32))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state_with(doc))
}

fn harness() -> Harness<'static, AppState> {
    harness_with(Document::sample())
}

/// Clicks a row of a menu of the menu row.
fn menu_row(h: &mut Harness<'static, AppState>, menu: &str, row: &str) {
    h.get_by_label(menu).click();
    h.run_steps(2);
    h.get_by_label(row).click();
    h.run_steps(1);
}

fn land_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/natural-earth/110m/ne_110m_land.geojson")
}

#[test]
fn first_frames_compose_the_map() {
    let mut h = harness();
    h.run_steps(3);
    let s = h.state();
    assert!(s.composed.is_some());
    assert!(s.texture.is_some());
    assert!(
        s.canvas_size.0 > 650.0 && s.canvas_size.0 < 850.0,
        "{:?}",
        s.canvas_size
    );
    assert!(!s.dirty);
}

#[test]
fn graticule_checkbox_recomposes() {
    let mut h = harness();
    h.run_steps(3);
    let before = h.state().composed.as_ref().unwrap().list.items.len();
    h.get_by_label("Graticule").click();
    h.run_steps(3);
    let s = h.state();
    assert!(!s.doc.frame.graticule);
    assert_ne!(s.composed.as_ref().unwrap().list.items.len(), before);
}

#[test]
fn land_checkbox_hides_layer() {
    let mut h = harness();
    h.run_steps(3);
    h.get_by_label("Land").click();
    h.run_steps(2);
    let doc = &h.state().doc;
    let land = doc
        .project
        .layers()
        .iter()
        .find(|l| l.name == "Land")
        .unwrap()
        .id;
    assert!(!doc.visible(land));
}

#[test]
fn the_layer_list_adds_a_layer() {
    let mut h = harness();
    h.run_steps(3);
    h.get_by_label("Add layer").click();
    h.run_steps(2);
    h.get_by_label("Points").click();
    h.run_steps(2);
    let state = h.state();
    let top = state.doc.project.layers().last().unwrap();
    assert_eq!(
        (top.name.as_str(), top.kind),
        ("Points 1", vale_store::GeometryKind::Point)
    );
    assert_eq!(state.selected_layer, Some(top.id));
}

#[test]
fn file_menu_rows_push_actions() {
    let mut h = harness();
    h.run_steps(2);
    menu_row(&mut h, "File", "Open GeoJSON…");
    assert!(h.state().actions.contains(&Action::OpenDialog));
    menu_row(&mut h, "File", "Export PDF…");
    assert!(
        h.state()
            .actions
            .contains(&Action::ExportDialog(ExportFormat::Pdf))
    );
}

#[test]
fn file_buttons_can_be_hidden() {
    let mut state = state_with(Document::sample());
    state.file_buttons = false;
    let mut h = Harness::builder()
        .with_size(egui::vec2(SIZE.0 as f32, SIZE.1 as f32))
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(2);
    h.get_by_label("File").click();
    h.run_steps(2);
    assert!(h.query_by_label("Import heightmap…").is_some());
    assert!(h.query_by_label("Open GeoJSON…").is_none());
    assert!(h.query_by_label("Export PDF…").is_none());
}

#[test]
fn the_import_row_shows_with_and_without_the_file_buttons() {
    for file_buttons in [true, false] {
        let mut state = state_with(Document::sample());
        state.workspace = Workspace::Globe;
        state.file_buttons = file_buttons;
        let mut h = Harness::builder()
            .with_size(egui::vec2(SIZE.0 as f32, SIZE.1 as f32))
            .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
        h.run_steps(2);
        menu_row(&mut h, "File", "Import heightmap…");
        let actions = &h.state().actions;
        assert_eq!(actions, &[Action::ImportHeightmapDialog], "{file_buttons}");
    }
}

#[test]
fn a_heightmap_file_has_the_extension_of_a_png_or_tiff_image() {
    for name in ["a.png", "b.TIF", "dir/c.Tiff", "d.tar.png"] {
        assert!(is_heightmap_path(Path::new(name)), "{name}");
    }
    for name in ["a.geojson", "png", "b.png.json", "c.jpg", ""] {
        assert!(!is_heightmap_path(Path::new(name)), "{name}");
    }
}

#[test]
fn drag_pans_the_map() {
    let mut h = harness();
    h.run_steps(3);
    let size = h.state().canvas_size;
    let before = {
        let s = h.state_mut();
        s.pipeline
            .lonlat_to_page(&s.doc, size, [0.0, 0.0])
            .unwrap()
            .unwrap()
    };
    h.drag_at(egui::pos2(700.0, 400.0));
    h.run_steps(1);
    h.hover_at(egui::pos2(750.0, 420.0));
    h.run_steps(1);
    h.drop_at(egui::pos2(750.0, 420.0));
    h.run_steps(2);
    let s = h.state_mut();
    assert!(s.doc.frame.view.is_some());
    let after = s
        .pipeline
        .lonlat_to_page(&s.doc, size, [0.0, 0.0])
        .unwrap()
        .unwrap();
    assert!(
        (after.x - before.x - 50.0).abs() < 1.0,
        "{before:?} {after:?}"
    );
    assert!(
        (after.y - before.y - 20.0).abs() < 1.0,
        "{before:?} {after:?}"
    );
}

#[test]
fn scroll_zooms_the_map() {
    let mut h = harness();
    h.run_steps(3);
    let size = h.state().canvas_size;
    let fit = {
        let s = h.state_mut();
        s.pipeline.fit_scale(&s.doc, size).unwrap()
    };
    h.hover_at(egui::pos2(700.0, 400.0));
    h.run_steps(1);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 200.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(3);
    let s = h.state();
    let view = s.doc.frame.view.expect("the scroll sets a view");
    assert!(view.scale > fit * 1.05, "{} vs {fit}", view.scale);
}

fn touch(h: &mut Harness<'static, AppState>, id: u64, phase: egui::TouchPhase, pos: egui::Pos2) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(id),
        phase,
        pos,
        force: None,
    });
}

/// The first finger is also the pointer, as in the window on iPad.
fn first_finger(h: &mut Harness<'static, AppState>, phase: egui::TouchPhase, pos: egui::Pos2) {
    touch(h, 1, phase, pos);
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

fn page_of_origin(h: &mut Harness<'static, AppState>) -> Point {
    let s = h.state_mut();
    s.pipeline
        .lonlat_to_page(&s.doc, s.canvas_size, [0.0, 0.0])
        .unwrap()
        .unwrap()
}

#[test]
fn one_finger_pans_the_map() {
    let mut h = harness();
    h.run_steps(3);
    let before = page_of_origin(&mut h);
    first_finger(&mut h, egui::TouchPhase::Start, egui::pos2(700.0, 400.0));
    h.run_steps(1);
    first_finger(&mut h, egui::TouchPhase::Move, egui::pos2(750.0, 420.0));
    h.run_steps(1);
    first_finger(&mut h, egui::TouchPhase::End, egui::pos2(750.0, 420.0));
    h.run_steps(2);
    let after = page_of_origin(&mut h);
    assert!(
        (after.x - before.x - 50.0).abs() < 1.0,
        "{before:?} {after:?}"
    );
    assert!(
        (after.y - before.y - 20.0).abs() < 1.0,
        "{before:?} {after:?}"
    );
}

#[test]
fn two_fingers_zoom_the_map() {
    let mut h = harness();
    h.run_steps(3);
    let size = h.state().canvas_size;
    let origin = h.state().canvas_origin;
    let fit = {
        let s = h.state_mut();
        s.pipeline.fit_scale(&s.doc, size).unwrap()
    };
    // The fingers start 200 points apart, with the middle at 700, 400.
    let middle = Point::new(700.0 - f64::from(origin.0), 400.0 - f64::from(origin.1));
    let place = {
        let s = h.state_mut();
        s.pipeline
            .page_to_lonlat(&s.doc, size, middle)
            .unwrap()
            .unwrap()
    };
    first_finger(&mut h, egui::TouchPhase::Start, egui::pos2(600.0, 400.0));
    h.run_steps(1);
    touch(&mut h, 2, egui::TouchPhase::Start, egui::pos2(800.0, 400.0));
    h.run_steps(1);

    // The fingers move apart to 300 points. The middle stays.
    first_finger(&mut h, egui::TouchPhase::Move, egui::pos2(550.0, 400.0));
    touch(&mut h, 2, egui::TouchPhase::Move, egui::pos2(850.0, 400.0));
    h.run_steps(1);
    let view = h.state().doc.frame.view.expect("the pinch sets a view");
    assert!(
        (view.scale / fit - 1.5).abs() < 0.01,
        "{} vs {fit}",
        view.scale
    );
    let at = {
        let s = h.state_mut();
        s.pipeline
            .lonlat_to_page(&s.doc, size, place)
            .unwrap()
            .unwrap()
    };
    assert!((at - middle).hypot() < 1.0, "{at:?} {middle:?}");

    // Both fingers move by 30, 10. The map follows, and the scale stays.
    first_finger(&mut h, egui::TouchPhase::Move, egui::pos2(580.0, 410.0));
    touch(&mut h, 2, egui::TouchPhase::Move, egui::pos2(880.0, 410.0));
    h.run_steps(1);
    let moved = h.state().doc.frame.view.unwrap();
    assert!((moved.scale / view.scale - 1.0).abs() < 0.01);
    let at = {
        let s = h.state_mut();
        s.pipeline
            .lonlat_to_page(&s.doc, size, place)
            .unwrap()
            .unwrap()
    };
    let want = middle + kurbo::Vec2::new(30.0, 10.0);
    assert!((at - want).hypot() < 1.0, "{at:?} {want:?}");
}

#[test]
fn zoom_helper_zooms() {
    let mut h = harness();
    h.run_steps(3);
    let state = h.state_mut();
    let size = state.canvas_size;
    let fit = state.pipeline.fit_scale(&state.doc, size).unwrap();
    vale_app::ui::canvas::zoom_at(state, Point::new(300.0, 300.0), 2.0, 0.0);
    let view = state.doc.frame.view.unwrap();
    assert!((view.scale - 2.0 * fit).abs() < 1e-9 * fit);
}

#[test]
fn open_files_action() {
    let mut state = state_with(Document::empty());
    state.run_action(Action::OpenFiles(vec![land_path()]));
    assert_eq!(state.doc.frame.entries.len(), 1);
    assert!(state.status.starts_with("Opened"), "{}", state.status);

    let mut state = state_with(Document::empty());
    state.run_action(Action::OpenFiles(vec!["/no/such/file.geojson".into()]));
    assert!(state.doc.frame.entries.is_empty());
    assert!(!state.status.is_empty());
}

#[test]
fn export_action_writes_pdf() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/app/test-ui-export.pdf");
    let _ = std::fs::remove_file(&path);
    let mut state = state_with(Document::sample());
    state.canvas_size = (800.0, 600.0);
    state.run_action(Action::Export(path.clone()));
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(state.status.starts_with("Exported"));
}

#[test]
fn projection_change_recomposes() {
    let mut h = harness();
    h.run_steps(3);
    let before = h.state().composed.as_ref().unwrap().view;
    let s = h.state_mut();
    s.doc.set_projection(ProjectionSpec {
        kind: ProjectionKind::Mercator,
        lon0: 0.0,
        lat0: 0.0,
    });
    s.touch();
    h.run_steps(2);
    assert_ne!(h.state().composed.as_ref().unwrap().view, before);
}

#[test]
fn full_ui_renders_offscreen() {
    let state = state_with(Document::sample());
    let (img, state) = match headless::ui_png(state, SIZE, 1.0) {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    };
    std::fs::create_dir_all("../../target/app").unwrap();
    img.save("../../target/app/test-ui.png").unwrap();
    assert_eq!((img.width(), img.height()), (1280, 800));
    assert!(state.composed.is_some());
    let panel = *img.get_pixel(20, 300);
    let mid = *img.get_pixel(640, 400);
    assert_ne!(panel, mid);
    let colors: std::collections::HashSet<_> = img.pixels().map(|p| p.0).collect();
    assert!(colors.len() > 50, "{}", colors.len());
}

#[test]
fn a_new_world_starts_from_a_name_a_radius_and_units() {
    let mut state = state_with(Document::sample());
    state.globe.map.set(0, 0, 0, 40000);
    assert!(state.has_raster());
    let world = World {
        name: "Aerth".to_string(),
        radius_km: 1737.4,
        units: Units::Imperial,
        seed: 0,
    };
    state.run_action(Action::NewWorld(world.clone()));
    assert_eq!(state.doc.project.world, world);
    assert!(state.doc.project.layers().is_empty());
    assert!(state.doc.frame.entries.is_empty());
    assert!(state.doc.project.rasters().is_empty());
    assert!(state.globe.map.allocated().next().is_none());
    assert!(!state.has_raster() && !state.globe.can_undo());
    assert!(state.session.is_none());
}

#[test]
fn a_new_world_is_a_new_file_and_the_old_file_stays() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("Earth.gpkg");
    let mut state = state_with(Document::sample());
    state.session = Some(Session::open(&old, &mut state.doc).unwrap());
    state.projects = Locations::single(dir.path());
    // The heightmap of the file has its own face size.
    state.globe.set_face_size(512);
    let base = state.globe.map.base();
    state.globe.set_map(Heightmap::new(1024, base));
    state.globe.map.set(0, 0, 0, 40000);
    let layers = state.doc.project.layers().to_vec();

    let world = World {
        name: "Pebble".to_string(),
        radius_km: 200.0,
        units: Units::Metric,
        seed: 0,
    };
    state.run_action(Action::NewWorld(world.clone()));
    assert!(state.session.is_some() && !state.can_undo());
    assert_eq!(state.globe.map.face_size(), 512);
    let new = dir.path().join("Pebble.gpkg");
    let (_, project) = ProjectFile::open(&new).unwrap();
    assert_eq!(project, Project::new(world.clone()));

    // The old file has the old world, with the stroke that waited.
    let (_, project) = ProjectFile::open(&old).unwrap();
    assert_eq!(project.world.name, "Earth (sample)");
    assert_eq!(project.layers(), layers);
    assert_eq!(project.rasters().len(), 1);
    assert_eq!(project.rasters()[0].face_size, 1024);

    // A stroke on the new world goes to the new file only.
    state.globe.map.set(1, 0, 0, 40000);
    state.run_action(Action::NewWorld(world));
    assert!(dir.path().join("Pebble 2.gpkg").exists());
    let (_, project) = ProjectFile::open(&new).unwrap();
    assert_eq!(project.rasters()[0].face_size, 512);
    let (_, project) = ProjectFile::open(&old).unwrap();
    assert_eq!(project.rasters().len(), 1);
}

#[test]
fn the_new_world_form_makes_a_world_from_a_preset() {
    let mut h = harness();
    h.state_mut().workspace = Workspace::Globe;
    h.run_steps(2);
    menu_row(&mut h, "File", NEW_WORLD);
    h.run_steps(1);
    let form = h.state().world_form.clone().unwrap();
    assert_eq!(form.mode, FormMode::New);
    assert_eq!(
        (form.name.as_str(), form.radius.as_str()),
        ("New world", "6371")
    );
    assert_eq!(form.preset(), Some(0));

    h.get_by_label("Mars").click();
    h.run_steps(2);
    h.get_by_label("Miles").click();
    h.run_steps(2);
    let form = h.state().world_form.as_ref().unwrap();
    assert_eq!(
        (form.radius.as_str(), form.radius_km()),
        ("2106.1", Some(3389.5))
    );
    // The world of the project stays until the form makes the new one.
    assert_eq!(h.state().doc.project.world.radius_km, 6371.0);

    h.get_by_label(CREATE).click();
    h.run_steps(2);
    assert!(h.state().world_form.is_none());
    let world = World {
        name: "New world".to_string(),
        radius_km: 3389.5,
        units: Units::Imperial,
        seed: 0,
    };
    assert_eq!(h.state().actions, [Action::NewWorld(world)]);
}

#[test]
fn the_form_does_not_make_a_world_from_a_wrong_radius_or_an_empty_name() {
    let mut form = WorldForm::new_world();
    assert!(form.world().is_some());
    for (text, km) in [
        ("99", None),
        ("100", Some(100.0)),
        ("abc", None),
        ("100001", None),
    ] {
        form.radius = text.to_string();
        form.radius_changed();
        assert_eq!(form.radius_km(), km, "{text}");
        assert_eq!(form.world().map(|w| w.radius_km), km, "{text}");
    }
    form.set_radius_km(200.0);
    form.name = "  ".to_string();
    assert_eq!(form.world(), None);

    let mut h = harness();
    form.name = "Aerth".to_string();
    form.radius = "x".to_string();
    form.radius_changed();
    h.state_mut().world_form = Some(form);
    h.run_steps(2);
    assert!(h.query_by_label_contains("From 100 to 100000 km").is_some());
    h.get_by_label(CREATE).click();
    h.run_steps(2);
    assert!(h.state().actions.is_empty() && h.state().world_form.is_some());
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.state().world_form.is_none());
}

#[test]
fn the_properties_form_changes_the_radius_and_keeps_each_feature() {
    let mut h = harness();
    h.run_steps(3);
    let layers = h.state().doc.project.layers().to_vec();
    let scale = h.state().composed.as_ref().unwrap().km_per_point;
    menu_row(&mut h, "File", Command::WorldProperties.label());
    h.run_steps(1);
    assert_eq!(
        h.state().world_form.as_ref().unwrap().mode,
        FormMode::Properties
    );

    h.get_by_label("Moon").click();
    h.run_steps(2);
    h.get_by_label("Miles").click();
    h.run_steps(3);
    let state = h.state();
    let world = &state.doc.project.world;
    assert_eq!((world.radius_km, world.units), (1737.4, Units::Imperial));
    assert_eq!(world.name, "Earth (sample)");
    // Each feature keeps its longitude and its latitude, and the picture stays.
    assert_eq!(state.doc.project.layers(), layers);
    let small = state.composed.as_ref().unwrap().km_per_point;
    assert!((small / scale - 1737.4 / 6371.0).abs() < 1e-9);
    assert!(h.query_by_label_contains("Radius 1079.6 mi").is_some());

    let form = h.state_mut().world_form.as_mut().unwrap();
    form.name = "Luna".to_string();
    h.run_steps(2);
    assert_eq!(h.state().doc.project.world.name, "Luna");
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(h.state().world_form.is_none());
    assert_eq!(h.state().doc.project.world.radius_km, 1737.4);
}
