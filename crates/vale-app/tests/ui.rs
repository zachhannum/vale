use std::path::Path;

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use vale_app::document::Document;
use vale_app::globe::StackItem;
use vale_app::headless;
use vale_app::projects::Locations;
use vale_app::session::Session;
use vale_app::ui::atlas::{AtlasSelection, AtlasTool, FrameTab};
use vale_app::ui::pad::atlas::{
    ADD_PAGE, CENTER_ON_FRAME, DELETE_FRAME, FRAME_TABS, MAP_NAME, NEXT_PAGE, PAGE_NAME,
    PREVIOUS_PAGE, SCALE,
};
use vale_app::ui::pad::{SCULPT, TERRAIN};
use vale_app::ui::panels::{ADD_MAP_FRAME, ADD_MENU};
use vale_app::ui::panels::{DELETE_LAYER, LAYER_NAME, LAYERS_MENU, OPACITY, WORLD_BUTTON};
use vale_app::ui::world_form::{CREATE, FormMode, NEW_WORLD, WorldForm};
use vale_app::ui::{
    Action, AppState, ExportFormat, LayerRef, Layout, RasterSpec, Workspace, draw,
    is_heightmap_path,
};
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::{Project, ProjectFile, RasterId, Units, World};
use vale_terrain::{FACES, Heightmap, TexelRect};

const SIZE: (f64, f64) = (1280.0, 800.0);

/// The Atlas workspace with one frame on the page.
fn state_with(doc: Document) -> AppState {
    let mut state = AppState::new(doc).unwrap();
    state.headless = true;
    state.workspace = Workspace::Atlas;
    vale_app::ui::atlas::add_frame(&mut state).unwrap();
    state.atlas.selection = AtlasSelection::None;
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

fn land_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/natural-earth/110m/ne_110m_land.geojson")
}

#[test]
fn first_frames_compose_the_map() {
    let mut h = harness();
    h.run_steps(3);
    let s = h.state();
    assert!(s.composed.is_some());
    assert_eq!(s.atlas.image_count(), 1);
    // With no selection, the canvas has the room of the right dock.
    assert!(
        s.canvas_size.0 > 1000.0 && s.canvas_size.0 < 1200.0,
        "{:?}",
        s.canvas_size
    );
    let composes = s.atlas.composes;
    h.run_steps(2);
    assert_eq!(h.state().atlas.composes, composes);
}

#[test]
fn graticule_checkbox_recomposes() {
    let mut h = harness();
    h.run_steps(3);
    let before = h.state().composed.as_ref().unwrap().list.items.len();
    let frame = h.state().doc.project.atlas.pages[0].frames[0].id;
    h.state_mut().atlas.selection = AtlasSelection::Frame(frame);
    h.run_steps(3);
    assert!(
        h.state()
            .doc
            .project
            .atlas
            .frame(frame)
            .unwrap()
            .graticule
            .on
    );
    h.get_by_label("Graticule").click();
    h.run_steps(3);
    let s = h.state();
    assert!(!s.doc.project.atlas.frame(frame).unwrap().graticule.on);
    assert_ne!(s.composed.as_ref().unwrap().list.items.len(), before);
}

#[test]
fn the_desktop_edge_label_controls_edit_one_set() {
    let mut h = harness();
    h.run_steps(3);
    let frame = h.state().doc.project.atlas.pages[0].frames[0].id;
    h.state_mut().atlas.selection = AtlasSelection::Frame(frame);
    h.run_steps(3);
    h.get_by_label("Edge labels").click();
    h.run_steps(2);
    h.get_by_label("Parallels segment").click();
    h.run_steps(2);
    h.get_by_label("Outside").click();
    h.run_steps(2);
    h.get_by_label("Top edge").click();
    h.run_steps(2);
    let g = &h.state().doc.project.atlas.frame(frame).unwrap().graticule;
    assert!(g.parallels.outside && !g.meridians.outside);
    assert!(!g.parallels.edges.top && g.meridians.edges.top);
    if std::env::var_os("VALE_SCREENSHOTS").is_some() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/app");
        std::fs::create_dir_all(dir).unwrap();
        h.render()
            .unwrap()
            .save(format!("{dir}/test-desktop-edge-labels.png"))
            .unwrap();
    }
}

#[test]
fn the_desktop_frame_panel_sets_the_neatline_the_border_and_the_grid() {
    let mut h = harness();
    h.run_steps(3);
    let frame = h.state().doc.project.atlas.pages[0].frames[0].id;
    h.state_mut().atlas.selection = AtlasSelection::Frame(frame);
    h.run_steps(3);
    assert!(h.get_all_by_label("Neatline color").next().is_some());
    assert!(h.get_all_by_label("Neatline width").next().is_some());
    h.get_by_label("Border").click();
    h.run_steps(2);
    h.get_by_label("Graduated").click();
    h.run_steps(2);
    assert!(h.get_all_by_label("Second color").next().is_some());
    assert!(h.get_all_by_label("Border distance").next().is_some());
    h.get_by_label("Border stroke").click();
    h.run_steps(2);
    assert!(h.get_all_by_label("Stroke width").next().is_some());
    h.get_by_label("Reference grid").click();
    h.run_steps(2);
    assert!(h.get_all_by_label("Grid labels").next().is_some());
    assert!(h.get_all_by_label("Border gap").next().is_some());
    h.get_by_label("Numbers").click();
    h.run_steps(2);
    let f = h.state().doc.project.atlas.frame(frame).unwrap();
    assert!(f.border.on && f.border.stroke && f.border.grid);
    assert_eq!(f.border.grid_columns, vale_store::GridColumns::Numbers);
    assert_eq!(f.border.style, vale_store::BorderStyle::Graduated);
    if std::env::var_os("VALE_SCREENSHOTS").is_some() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/app");
        std::fs::create_dir_all(dir).unwrap();
        h.render()
            .unwrap()
            .save(format!("{dir}/test-desktop-frame-border.png"))
            .unwrap();
    }
}

#[test]
fn the_desktop_page_panel_has_the_paper_color() {
    let mut h = harness();
    h.run_steps(3);
    h.state_mut().atlas.selection = AtlasSelection::Page;
    h.run_steps(3);
    assert!(h.get_all_by_label("Paper color").next().is_some());
}

#[test]
fn the_desktop_scale_bar_controls_edit_the_bar_and_its_numbers() {
    use vale_app::ui::atlas::{ItemKind, add_item};
    use vale_store::Furniture;
    let mut h = harness();
    h.run_steps(3);
    let id = add_item(h.state_mut(), ItemKind::ScaleBar).unwrap();
    h.run_steps(3);
    h.get_by_label("Numbers").click();
    h.run_steps(2);
    h.get_by_label("Show zero").click();
    h.run_steps(2);
    h.get_by_label("Halo").click();
    h.run_steps(2);
    let bar = |h: &Harness<'static, AppState>| match &h
        .state()
        .doc
        .project
        .atlas
        .furniture(id)
        .unwrap()
        .item
    {
        Furniture::ScaleBar(b) => b.clone(),
        _ => panic!("not a scale bar"),
    };
    let b = bar(&h);
    assert!(!b.zero_label);
    assert!(b.number_style.halo && !b.unit_style.halo);
    if std::env::var_os("VALE_SCREENSHOTS").is_some() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/app");
        std::fs::create_dir_all(dir).unwrap();
        h.render()
            .unwrap()
            .save(format!("{dir}/test-desktop-scale-bar.png"))
            .unwrap();
    }
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
    assert!(!doc.shown(doc.first_map(), land));
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
        (top.name.as_str(), top.geometry()),
        ("Points 1", Some(vale_store::GeometryKind::Point))
    );
    assert_eq!(state.selected_layer, Some(LayerRef::Vector(top.id)));
}

/// The names of the layers of the project, from the bottom of the drawing.
fn layer_names(state: &AppState) -> Vec<&str> {
    let layers = state.doc.project.layers();
    layers.iter().map(|l| l.name.as_str()).collect()
}

#[test]
fn the_map_workspace_does_not_list_a_raster_layer() {
    let mut h = harness();
    let rain = RasterSpec {
        name: "Rain".to_string(),
        unit: "mm".to_string(),
        min: 0.0,
        max: 4000.0,
        face_size: 64,
    };
    let rain = h.state_mut().add_raster(rain).unwrap();
    let layer = h.state().doc.project.raster_layer(rain).unwrap().id;
    // The raster layer is between two vector layers in the draw order.
    assert!(h.state_mut().move_layer(layer, 1));
    h.state_mut().select(None);
    h.run_steps(3);
    let state = h.state();
    assert_eq!(layer_names(state), ["Land", "Rain", "Rivers", "Places"]);

    // The map frame has an entry for each vector layer only.
    let ids = |names: [&str; 3]| {
        let layers = state.doc.project.layers();
        names.map(|name| layers.iter().find(|l| l.name == name).unwrap().id)
    };
    let [land, rivers, places] = ids(["Land", "Rivers", "Places"]);
    let entries: Vec<_> = state.doc.frame.entries.iter().map(|e| e.layer).collect();
    assert_eq!(entries, [land, rivers, places]);
    assert!(state.doc.entry(layer).is_none());
    // The list, the map, and the labels have the vector layers.
    assert!(h.query_by_label("Select Rain").is_none());
    assert!(h.query_by_label("Rain").is_none());
    for name in ["Select Land", "Select Rivers", "Select Places"] {
        assert!(h.query_by_label(name).is_some(), "{name}");
    }
    let composed = state.composed.as_ref().expect("a composed map");
    assert!(composed.labels.placed > 0);

    // A raster layer has no inspector.
    h.state_mut().selected_layer = Some(LayerRef::Vector(layer));
    h.run_steps(2);
    assert!(h.query_by_label("Move up").is_none());

    // A vector layer moves past the next vector layer of the map.
    h.state_mut().select(Some(LayerRef::Vector(land)));
    h.run_steps(2);
    h.get_by_label("Move up").click();
    h.run_steps(2);
    let state = h.state();
    assert_eq!(layer_names(state), ["Rain", "Rivers", "Land", "Places"]);
    let entries: Vec<_> = state.doc.frame.entries.iter().map(|e| e.layer).collect();
    assert_eq!(entries, [rivers, land, places]);
}

/// The desktop layout of the globe workspace of the sample world.
fn globe_harness() -> Harness<'static, AppState> {
    let mut state = state_with(Document::sample());
    state.workspace = Workspace::Globe;
    let mut h = Harness::builder()
        .with_size(egui::vec2(SIZE.0 as f32, SIZE.1 as f32))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(3);
    h
}

#[test]
fn the_toolbar_toggles_the_graticule() {
    let mut h = globe_harness();
    h.state_mut()
        .add_raster(RasterSpec::height("Height", 64))
        .unwrap();
    h.state_mut().globe.preview.panel = true;
    h.run_steps(3);
    // The panel of the height layer has the rivers, and the graticule is
    // in the tool bar only.
    assert!(h.query_by_label("Rivers").is_some());
    assert_eq!(h.query_all_by_label("Graticule").count(), 1);
    let before = h.state().globe.preview.graticule;
    h.get_by_label("Graticule").click();
    h.run_steps(2);
    assert_eq!(h.state().globe.preview.graticule, !before);
    h.get_by_label("Graticule").click();
    h.run_steps(2);
    assert_eq!(h.state().globe.preview.graticule, before);
}

#[test]
fn the_side_list_shows_a_raster_layer_per_map() {
    let mut h = harness();
    let height = RasterSpec::height("Height", 64);
    let height = h.state_mut().add_raster(height).unwrap();
    let layer = h.state().doc.project.raster_layer(height).unwrap().id;
    h.run_steps(2);
    let map = vale_app::ui::atlas::selected_map(h.state());
    assert!(h.state().doc.shown(map, layer));
    h.get_by_label("Show Height in map").click();
    h.run_steps(2);
    assert!(!h.state().doc.shown(map, layer));
    h.get_by_label("Show Height in map").click();
    h.run_steps(2);
    assert!(h.state().doc.shown(map, layer));
}

#[test]
fn the_desktop_list_moves_hides_renames_and_deletes_a_raster_layer() {
    let mut h = globe_harness();
    let height = RasterSpec::height("Height", 64);
    let height = h.state_mut().add_raster(height).unwrap();
    h.state_mut().select(None);
    h.run_steps(2);
    assert_eq!(
        layer_names(h.state()),
        ["Height", "Land", "Rivers", "Places"]
    );
    h.get_by_label(LAYERS_MENU).click();
    h.run_steps(2);
    let click = |h: &mut Harness<'static, AppState>, name: &str| {
        h.get_by_label(name).click();
        h.run_steps(2);
    };

    // The list has each layer of each kind, and the arrows move a raster
    // layer past a vector layer.
    for name in ["Select Height", "Select Land", "Select Places"] {
        assert!(h.query_by_label(name).is_some(), "{name}");
    }
    click(&mut h, "Move Height up");
    click(&mut h, "Move Height up");
    assert_eq!(
        layer_names(h.state()),
        ["Land", "Rivers", "Height", "Places"]
    );
    click(&mut h, "Move Places down");
    assert_eq!(
        layer_names(h.state()),
        ["Land", "Rivers", "Places", "Height"]
    );
    click(&mut h, "Move Height down");

    // The name selects the layer, and the checkbox hides it.
    click(&mut h, "Select Height");
    assert_eq!(h.state().selected_layer, Some(LayerRef::Raster(height)));
    assert_eq!(h.state().globe.raster(), Some(height));
    click(&mut h, "Show Height");
    let layer = h.state().doc.project.raster_layer(height).unwrap();
    assert!(!layer.visible);
    let stack = h.state().globe.stack().unwrap();
    let raster = |item: &StackItem| matches!(item, StackItem::Raster { .. });
    assert!(!stack.iter().any(raster));
    assert!(h.state().shown_raster().is_none() && !h.state().globe.paintable);
    click(&mut h, "Show Height");
    assert_eq!(h.state().shown_raster().map(|r| r.id), Some(height));
    assert!(h.state().globe.paintable);

    // The selected layer has a name field, an opacity, and a delete button.
    assert!(h.query_by_label(LAYER_NAME).is_some());
    assert!(h.query_by_label(OPACITY).is_some());
    assert!(h.state_mut().rename_raster(height, "Ground"));
    assert!(h.state_mut().set_raster_opacity(height, 0.4));
    h.run_steps(2);
    assert!(h.query_by_label("Select Ground").is_some());
    assert_eq!(h.state().doc.project.raster(height).unwrap().opacity, 0.4);
    click(&mut h, DELETE_LAYER);
    let state = h.state();
    assert_eq!(layer_names(state), ["Land", "Rivers", "Places"]);
    assert!(state.doc.project.rasters().is_empty());
    assert!(state.globe.raster_map(height).is_none());
    assert_eq!(state.selected_layer, None);
}

#[test]
fn the_desktop_layers_menu_asks_for_a_geojson_file() {
    let mut h = globe_harness();
    h.get_by_label(LAYERS_MENU).click();
    h.run_steps(2);
    h.get_by_label(vale_app::ui::panels::ADD_GEOJSON).click();
    h.run_steps(1);
    assert!(h.state().actions.contains(&Action::GeoJsonDialog));
}

#[test]
fn the_desktop_list_shows_the_panel_of_an_image_layer() {
    use vale_store::BlendMode;
    let mut h = globe_harness();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plates.png");
    image::RgbaImage::from_pixel(64, 32, image::Rgba([255, 0, 0, 128]))
        .save(&file)
        .unwrap();
    h.get_by_label(LAYERS_MENU).click();
    h.run_steps(2);
    h.get_by_label(vale_app::ui::panels::ADD_IMAGE).click();
    h.run_steps(1);
    assert!(h.state().actions.contains(&Action::ImageLayerDialog));
    h.state_mut().actions.clear();
    h.state_mut().run_action(Action::ImportImageLayer(file));
    let id = h.state().doc.project.images()[0].id;
    // An image layer gets no entry of a vector layer in the map frame.
    let layer = h.state().doc.project.image_layer(id).unwrap().id;
    assert!(h.state().doc.entry(layer).is_none());
    h.get_by_label(LAYERS_MENU).click();
    h.run_steps(2);
    assert_eq!(h.state().selected_layer, Some(LayerRef::Image(id)));
    for name in [
        "Select plates",
        LAYER_NAME,
        OPACITY,
        "Blend mode",
        DELETE_LAYER,
    ] {
        assert!(h.query_by_label(name).is_some(), "{name}");
    }
    assert!(h.query_by_label("64 × 32 px").is_some());
    assert!(h.state_mut().set_image_blend(id, BlendMode::Screen));
    assert!(h.state_mut().set_image_opacity(id, 0.25));
    h.run_steps(2);
    let stack = h.state().globe.stack().unwrap();
    let item = StackItem::Image {
        id,
        opacity: 0.25,
        blend: BlendMode::Screen,
    };
    assert!(stack.contains(&item));
    h.get_by_label(DELETE_LAYER).click();
    h.run_steps(2);
    assert!(h.state().doc.project.images().is_empty());
}

#[test]
fn toolbar_buttons_push_actions() {
    let mut h = harness();
    h.run_steps(2);
    h.get_by_label("Open GeoJSON…").click();
    h.run_steps(1);
    assert!(h.state().actions.contains(&Action::OpenDialog));
    h.get_by_label("Export atlas PDF…").click();
    h.run_steps(1);
    assert!(
        h.state()
            .actions
            .contains(&Action::ExportDialog(ExportFormat::AtlasPdf))
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
    assert!(h.query_by_label("Open GeoJSON…").is_none());
    assert!(h.query_by_label("Export atlas PDF…").is_none());
}

#[test]
fn the_import_button_shows_with_and_without_the_file_buttons() {
    for file_buttons in [true, false] {
        let mut state = state_with(Document::sample());
        state.workspace = Workspace::Globe;
        state.file_buttons = file_buttons;
        let mut h = Harness::builder()
            .with_size(egui::vec2(SIZE.0 as f32, SIZE.1 as f32))
            .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
        h.run_steps(2);
        h.get_by_label("Import heightmap…").click();
        h.run_steps(1);
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
fn the_export_writes_a_png_and_a_pdf_of_the_open_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with(Document::sample());
    vale_app::ui::atlas::add_page(&mut state);
    vale_app::ui::atlas::edit_page(&mut state, |p| {
        p.width_mm = 100.0;
        p.height_mm = 50.0;
    });
    vale_app::ui::atlas::add_frame(&mut state).unwrap();

    let png = dir.path().join("page.png");
    state.run_action(Action::Export(png.clone()));
    assert!(state.status.starts_with("Exported"), "{}", state.status);
    let img = image::open(&png).unwrap();
    let px = |mm: f64| (mm * 72.0 / 25.4 * headless::PAGE_PIXEL_RATIO).ceil() as u32;
    let (w, h) = (img.width(), img.height());
    assert!(
        w.abs_diff(px(100.0)) <= 1 && h.abs_diff(px(50.0)) <= 1,
        "{w} × {h}"
    );

    let pdf = dir.path().join("page.pdf");
    state.run_action(Action::Export(pdf.clone()));
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    // The media box is the page in points.
    let text = String::from_utf8_lossy(&bytes);
    let points = |mm: f64| format!("{}", (mm * 72.0 / 25.4 * 1000.0).round() / 1000.0);
    let pt = |mm: f64| points(mm).split('.').next().unwrap().to_owned();
    let mediabox = text
        .split("/MediaBox")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .unwrap_or_default()
        .to_owned();
    assert!(
        mediabox.contains(&pt(100.0)) && mediabox.contains(&pt(50.0)),
        "{mediabox}"
    );
}

#[test]
fn the_atlas_export_writes_each_page_and_can_be_canceled() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("atlas.pdf");
    let mut state = state_with(Document::sample());
    state.headless = false;
    for i in 1..20 {
        vale_app::ui::atlas::add_page(&mut state);
        vale_app::ui::atlas::edit_page(&mut state, |page| {
            page.width_mm = 100.0 + f64::from(i);
            page.height_mm = 50.0;
        });
    }

    state.run_action(Action::ExportAtlas(out.clone()));
    assert_eq!(
        state.atlas_export_progress().as_deref(),
        Some("Exporting atlas.pdf: page 1 of 20")
    );
    state.poll_atlas_export(None);
    assert_eq!(
        state.atlas_export_progress().as_deref(),
        Some("Exporting atlas.pdf: page 2 of 20")
    );
    state.run_action(Action::CancelExport);
    assert!(state.atlas_export.is_none() && !out.exists());
    assert_eq!(state.status, "Canceled export of atlas.pdf");

    state.headless = true;
    state.run_action(Action::ExportAtlas(out.clone()));
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Count 20"), "{text}");
}

#[test]
fn projection_change_recomposes() {
    let mut h = harness();
    h.run_steps(3);
    let before = h.state().atlas.composes;
    let s = h.state_mut();
    s.doc.set_projection(
        s.doc.first_map(),
        ProjectionSpec::new(ProjectionKind::Mercator, 0.0, 0.0),
    );
    s.touch();
    h.run_steps(2);
    assert_eq!(h.state().atlas.composes, before + 1);
}

#[test]
#[ignore = "gpu"]
fn full_ui_renders_offscreen() {
    let _gpu = one_gpu_test();
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
    state.add_height();
    state.globe.map.set(0, 0, 0, 40000);
    assert!(state.has_raster());
    let world = World {
        name: "Aerth".to_string(),
        radius_km: 1737.4,
        units: Units::Imperial,
        ..World::default()
    };
    state.run_action(Action::NewWorld(world.clone()));
    assert_eq!(state.doc.project.world, world);
    assert!(state.doc.project.layers().is_empty());
    assert!(state.doc.frame.entries.is_empty());
    assert!(state.doc.project.rasters().is_empty());
    assert!(state.globe.map.allocated().next().is_none());
    assert_eq!((state.globe.raster(), state.selected_layer), (None, None));
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
    // The height layer of the file has its own face size.
    state.globe.set_face_size(512);
    let height = RasterSpec::height("Height", 1024);
    state.add_raster(height).unwrap();
    state.globe.map.set(0, 0, 0, 40000);
    let layers = state.doc.project.layers().to_vec();

    let world = World {
        name: "Pebble".to_string(),
        radius_km: 200.0,
        units: Units::Metric,
        ..World::default()
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
    state.add_height();
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
    h.get_by_label(NEW_WORLD).click();
    h.run_steps(2);
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
        ..World::default()
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
    h.get_by_label(WORLD_BUTTON).click();
    h.run_steps(2);
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
    // Each feature keeps its longitude and its latitude. The scale of the
    // frame is a ratio, so a point is the same distance on each world.
    assert_eq!(state.doc.project.layers(), layers);
    let small = state.composed.as_ref().unwrap().km_per_point;
    assert!((small / scale - 1.0).abs() < 1e-6, "{small} {scale}");
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

/// One test at a time makes a wgpu device. Software adapters fail when two
/// threads make devices at the same time.
fn one_gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The face size of the raster layers of the brush tests.
const BRUSH_FACE_SIZE: usize = 256;

/// The iPad layout of the globe workspace on the wgpu renderer, for a world
/// with no layer. Each step renders, so each step runs the GPU work of the
/// globe.
fn gpu_globe() -> Harness<'static, AppState> {
    let mut state = AppState::new(Document::empty()).unwrap();
    state.set_layout(Layout::Pad);
    state.globe.set_face_size(BRUSH_FACE_SIZE);
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut h = headless::ui_harness(state, (1194.0, 834.0), 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(3);
    h
}

/// One event of a pen. The first touch is also the pointer.
fn pen(h: &mut Harness<'static, AppState>, phase: egui::TouchPhase, pos: egui::Pos2) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(1),
        phase,
        pos,
        force: Some(0.5),
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

/// A pen stroke across the middle of the canvas. The call returns after the
/// texels of the stroke are in the CPU heightmap.
fn pen_stroke(h: &mut Harness<'static, AppState>) {
    let c = h.state().globe.rect.center();
    let (from, to) = (c - egui::vec2(40.0, 0.0), c + egui::vec2(40.0, 0.0));
    pen(h, egui::TouchPhase::Start, from);
    h.step();
    for i in 1..=6 {
        pen(h, egui::TouchPhase::Move, from.lerp(to, i as f32 / 6.0));
        h.step();
    }
    pen(h, egui::TouchPhase::End, to);
    h.run_steps(3);
    for _ in 0..400 {
        if !h.state().globe.busy() {
            return;
        }
        h.step();
    }
    panic!("the stroke did not end");
}

/// Each texel of a heightmap, face by face.
fn texels(map: &Heightmap) -> Vec<u16> {
    let n = map.face_size();
    let all = TexelRect {
        x0: 0,
        y0: 0,
        x1: n,
        y1: n,
    };
    let mut out = Vec::new();
    (0..FACES).for_each(|face| map.read_rect(face, all, &mut out));
    out
}

/// Each texel of a raster layer of the app.
fn layer_texels(h: &Harness<'static, AppState>, id: RasterId) -> Vec<u16> {
    texels(h.state().globe.raster_map(id).unwrap())
}

/// The level of the heightmap on the globe at the middle of the canvas.
fn middle_level(h: &Harness<'static, AppState>) -> u16 {
    let globe = &h.state().globe;
    let middle = globe.unproject(globe.rect.center()).unwrap();
    globe.map.sample(middle)
}

#[test]
#[ignore = "gpu"]
fn a_stroke_on_the_second_raster_layer_leaves_the_first() {
    let _gpu = one_gpu_test();
    let mut h = gpu_globe();
    let height = h.state_mut().add_height();
    let rain = RasterSpec {
        name: "Rain".to_string(),
        unit: "mm".to_string(),
        min: 0.0,
        max: 4000.0,
        face_size: BRUSH_FACE_SIZE,
    };
    let rain = h.state_mut().add_raster(rain).unwrap();
    let empty = [layer_texels(&h, height), layer_texels(&h, rain)];
    assert!(empty[0] != empty[1]);

    h.state_mut().select(Some(LayerRef::Raster(height)));
    h.run_steps(2);
    pen_stroke(&mut h);
    let first = layer_texels(&h, height);
    let first_middle = middle_level(&h);
    assert!(first != empty[0]);
    assert!(layer_texels(&h, rain) == empty[1]);

    // The stroke on the second layer goes over the same place. The rivers
    // of the first layer do not show on the second layer.
    assert!(h.state().globe.channels().is_some());
    h.state_mut().select(Some(LayerRef::Raster(rain)));
    assert!(h.state().globe.channels().is_none());
    h.run_steps(2);
    assert!(h.state().globe.channels().is_none());
    assert_eq!(h.state().globe.raster(), Some(rain));
    pen_stroke(&mut h);
    let second = layer_texels(&h, rain);
    assert!(second != empty[1]);
    assert!(
        layer_texels(&h, height) == first,
        "the stroke on the second layer changed the first"
    );
    // The stroke started from the ground of the second layer.
    let base = h.state().globe.map.base();
    assert_eq!(base, 0);
    assert!(middle_level(&h) > base && middle_level(&h) < first_middle);

    // The GPU has the first layer again, and a stroke goes on from its
    // ground.
    h.state_mut().select(Some(LayerRef::Raster(height)));
    assert!(h.state().globe.channels().is_none());
    h.run_steps(2);
    assert!(h.state().globe.channels().is_some());
    pen_stroke(&mut h);
    assert!(middle_level(&h) > first_middle);
    assert!(layer_texels(&h, rain) == second);
    let project = &h.state().doc.project;
    let units: Vec<&str> = project.rasters().iter().map(|r| r.unit.as_str()).collect();
    assert_eq!(units, ["m", "mm"]);
}

#[test]
#[ignore = "gpu"]
fn a_new_world_has_no_raster_layer() {
    let _gpu = one_gpu_test();
    let dir = tempfile::tempdir().unwrap();
    let mut h = gpu_globe();
    h.state_mut().projects = Locations::single(dir.path());
    let world = World {
        name: "Pebble".to_string(),
        ..World::default()
    };
    h.state_mut().run_action(Action::NewWorld(world));
    h.run_steps(3);
    let state = h.state();
    assert!(state.session.is_some() && !state.has_raster());
    assert!(state.doc.project.rasters().is_empty());
    assert_eq!((state.globe.raster(), state.selected_layer), (None, None));

    // With no raster layer, the brush cells are off, and the pen moves the
    // globe.
    let cell = |h: &Harness<'static, AppState>, name: &str| {
        let controls = &h.state().pad.controls;
        controls.iter().any(|control| control.name == name)
    };
    assert!(!cell(&h, SCULPT) && !cell(&h, TERRAIN) && !cell(&h, "Size"));
    assert!(cell(&h, "Pan"));
    let view = h.state().globe.view;
    let before = texels(&h.state().globe.map);
    pen_stroke(&mut h);
    let state = h.state();
    assert_ne!(state.globe.view, view);
    assert!(texels(&state.globe.map) == before);
    assert!(state.globe.map.allocated().next().is_none());
    assert!(state.doc.project.rasters().is_empty());
    assert!(!state.can_undo());

    // The file has no raster layer after a save.
    assert!(h.state_mut().save_now());
    let (file, project) = ProjectFile::open(&dir.path().join("Pebble.gpkg")).unwrap();
    assert!(project.rasters().is_empty());
    assert_eq!(project.world.name, "Pebble");
    drop(file);

    // A vector layer is not a layer that the brush paints.
    let lines = h.state_mut().doc.add_layer(vale_store::GeometryKind::Line);
    h.state_mut().select(Some(LayerRef::Vector(lines)));
    h.run_steps(2);
    pen_stroke(&mut h);
    assert!(h.state().doc.project.rasters().is_empty());
    assert!(h.state().globe.map.allocated().next().is_none());

    // The brush paints a raster layer that is selected and visible.
    let height = h.state_mut().add_height();
    h.run_steps(2);
    assert!(cell(&h, SCULPT) && cell(&h, TERRAIN) && cell(&h, "Size"));
    h.state_mut().set_raster_visible(height, false);
    h.run_steps(2);
    assert!(!cell(&h, SCULPT) && !cell(&h, TERRAIN));
    pen_stroke(&mut h);
    assert!(
        h.state()
            .globe
            .raster_map(height)
            .unwrap()
            .allocated()
            .next()
            .is_none()
    );
    h.state_mut().set_raster_visible(height, true);
    h.run_steps(2);
    pen_stroke(&mut h);
    assert!(
        h.state()
            .globe
            .raster_map(height)
            .unwrap()
            .allocated()
            .next()
            .is_some()
    );
}

/// The screen position of a point of the open page, in millimeters.
fn page_pos(h: &Harness<'static, AppState>, x: f64, y: f64) -> egui::Pos2 {
    let place = h.state().atlas.place.unwrap();
    place.to_screen(kurbo::Point::new(x, y))
}

/// The screen position of the middle of the first frame of the open page.
fn frame_middle(h: &Harness<'static, AppState>) -> egui::Pos2 {
    let page = vale_app::ui::atlas::open_page(h.state());
    let b = vale_app::atlas::shape_bbox(&page.frames[0].shape);
    page_pos(h, b.center().x, b.center().y)
}

/// A primary click at a screen position, in three steps.
fn click_at(h: &mut Harness<'static, AppState>, pos: egui::Pos2) {
    h.hover_at(pos);
    h.step();
    h.drag_at(pos);
    h.step();
    h.drop_at(pos);
    h.run_steps(3);
}

#[test]
fn the_dock_shows_only_with_a_selection() {
    let mut h = harness();
    h.run_steps(3);
    assert_eq!(h.state().selected_layer, None);
    assert!(h.query_by_label("Unplaced labels").is_none());
    assert!(h.query_by_label(FRAME_TABS[0]).is_none());
    assert!(h.query_by_label(ADD_PAGE).is_none());
}

#[test]
fn a_click_on_a_frame_shows_the_frame_and_projection_tabs() {
    let mut h = harness();
    h.run_steps(3);
    let pos = frame_middle(&h);
    click_at(&mut h, pos);
    let frame = h.state().doc.project.atlas.pages[0].frames[0].id;
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(frame));
    assert!(h.query_by_label(FRAME_TABS[0]).is_some());
    h.get_by_label(FRAME_TABS[1]).click();
    h.run_steps(2);
    assert_eq!(h.state().atlas.tab, FrameTab::Projection);
    assert!(h.query_by_label(CENTER_ON_FRAME).is_some());
}

#[test]
fn a_click_on_the_paper_shows_the_page_panel() {
    let mut h = harness();
    h.run_steps(3);
    let pos = page_pos(&h, 2.0, 2.0);
    click_at(&mut h, pos);
    assert_eq!(h.state().atlas.selection, AtlasSelection::Page);
    assert!(h.query_by_label(ADD_PAGE).is_some());
    assert!(h.query_by_label(PAGE_NAME).is_some());
    h.get_by_label(ADD_PAGE).click();
    h.run_steps(2);
    assert_eq!(h.state().doc.project.atlas.pages.len(), 2);
}

/// Types this text in place of the text of the field `name`, and ends the
/// edit with Enter.
fn type_in(h: &mut Harness<'static, AppState>, name: &str, text: &str) {
    let role = egui::accesskit::Role::TextInput;
    h.get_by_role_and_label(role, name).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_role_and_label(role, name).type_text(text);
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
}

#[test]
fn a_typed_scale_sets_the_scale_of_the_frame() {
    let mut h = harness();
    let frame = h.state().doc.project.atlas.pages[0].frames[0].id;
    h.state_mut().atlas.selection = AtlasSelection::Frame(frame);
    h.run_steps(3);
    type_in(&mut h, SCALE, "1:5000000");
    let scale = h.state().doc.project.atlas.frame(frame).unwrap().scale;
    assert!((scale - 5e6).abs() < 1.0, "{scale}");
}

#[test]
fn a_projection_preset_changes_the_projection() {
    let mut h = harness();
    let frame = h.state().doc.project.atlas.pages[0].frames[0].id;
    h.state_mut().atlas.selection = AtlasSelection::Frame(frame);
    h.state_mut().atlas.tab = FrameTab::Projection;
    h.run_steps(3);
    let kind = ProjectionKind::ALL
        .into_iter()
        .find(|k| *k != h.state().doc.projection(h.state().doc.first_map()).kind)
        .unwrap();
    h.get_by_label(kind.name()).click();
    h.run_steps(2);
    assert_eq!(
        h.state().doc.projection(h.state().doc.first_map()).kind,
        kind
    );
}

#[test]
fn add_map_frame_adds_a_frame_to_the_open_page() {
    let mut h = harness();
    h.run_steps(3);
    h.get_by_label(ADD_MENU).click();
    h.run_steps(2);
    h.get_by_label(ADD_MAP_FRAME).click();
    h.run_steps(3);
    let s = h.state();
    let page = &s.doc.project.atlas.pages[0];
    assert_eq!(page.frames.len(), 2);
    assert_eq!(s.atlas.selection, AtlasSelection::Frame(page.frames[1].id));
}

#[test]
fn the_page_buttons_open_the_page_before_and_after() {
    let mut h = harness();
    vale_app::ui::atlas::add_page(h.state_mut());
    h.state_mut().atlas.selection = AtlasSelection::None;
    h.run_steps(3);
    let pages: Vec<_> = h
        .state()
        .doc
        .project
        .atlas
        .pages
        .iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(h.state().atlas.page, pages[1]);
    assert!(h.get_by_label(NEXT_PAGE).accesskit_node().is_disabled());
    h.get_by_label(PREVIOUS_PAGE).click();
    h.run_steps(2);
    assert_eq!(h.state().atlas.page, pages[0]);
    assert!(h.get_by_label(PREVIOUS_PAGE).accesskit_node().is_disabled());
    h.get_by_label(NEXT_PAGE).click();
    h.run_steps(2);
    assert_eq!(h.state().atlas.page, pages[1]);
}

#[test]
fn the_wheel_zooms_the_page_and_in_the_selected_frame_the_map() {
    let mut h = harness();
    h.run_steps(3);
    let frame = h.state().doc.project.atlas.pages[0].frames[0].clone();
    let wheel = |h: &mut Harness<'static, AppState>, pos: egui::Pos2| {
        h.hover_at(pos);
        h.step();
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 200.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(10);
    };
    // On a frame that is not selected, the wheel zooms the page about the
    // pointer, and the map stays the same.
    let before = h.state().atlas.place.unwrap();
    let pos = frame_middle(&h);
    let mm = before.to_mm(pos);
    wheel(&mut h, pos);
    let after = h.state().atlas.place.unwrap();
    assert!(after.pts_per_mm > before.pts_per_mm * 1.05);
    assert!(after.to_mm(pos).distance(mm) < 0.5);
    let page = vale_app::ui::atlas::open_page(h.state());
    assert_eq!(page.frames[0], frame);
    // In the selected frame, the wheel zooms the map.
    h.state_mut().atlas.selection = AtlasSelection::Frame(frame.id);
    h.run_steps(3);
    let before = h.state().atlas.place.unwrap();
    let pos = frame_middle(&h);
    wheel(&mut h, pos);
    assert_eq!(h.state().atlas.place.unwrap(), before);
    let page = vale_app::ui::atlas::open_page(h.state());
    assert!(page.frames[0].scale < frame.scale / 1.05);
}

#[test]
fn the_dock_deletes_the_selected_frame() {
    let mut h = harness();
    let frame = h.state().doc.project.atlas.pages[0].frames[0].id;
    h.state_mut().atlas.selection = AtlasSelection::Frame(frame);
    h.run_steps(3);
    h.get_by_label(DELETE_FRAME).click();
    h.run_steps(3);
    assert!(h.state().doc.project.atlas.frame(frame).is_none());
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
    assert!(!h.state().atlas.has_image(frame));
    assert!(h.query_by_label(DELETE_FRAME).is_none());
}

#[test]
fn the_delete_key_removes_a_selected_vertex_or_else_the_frame() {
    let mut h = harness();
    let frame = h.state().doc.project.atlas.pages[0].frames[0].id;
    vale_app::ui::atlas::set_polygon(h.state_mut(), frame, true);
    h.state_mut().atlas.selection = AtlasSelection::Frame(frame);
    h.state_mut().atlas.vertex = Some((0, 0));
    h.run_steps(3);
    let ring = |h: &Harness<'static, AppState>| match &h.state().doc.project.atlas.frame(frame) {
        Some(f) => match &f.shape {
            vale_store::FrameShape::Polygon(rings) => rings[0].len(),
            _ => 0,
        },
        None => 0,
    };
    let before = ring(&h);
    h.key_press(egui::Key::Delete);
    h.run_steps(2);
    assert_eq!(ring(&h), before - 1);
    assert_eq!(h.state().atlas.vertex, None);
    assert_eq!(h.state().atlas.selection, AtlasSelection::Frame(frame));

    h.key_press(egui::Key::Backspace);
    h.run_steps(2);
    assert!(h.state().doc.project.atlas.frame(frame).is_none());
    assert_eq!(h.state().atlas.selection, AtlasSelection::None);
}

#[test]
fn the_keys_v_and_h_choose_the_tools() {
    let mut h = harness();
    h.run_steps(3);
    h.key_press(egui::Key::H);
    h.run_steps(2);
    assert_eq!(h.state().atlas.tool, AtlasTool::Move);
    h.key_press(egui::Key::V);
    h.run_steps(2);
    assert_eq!(h.state().atlas.tool, AtlasTool::Select);
}

#[test]
#[ignore = "gpu"]
fn the_desktop_atlas_renders_offscreen() {
    let _gpu = one_gpu_test();
    let mut state = state_with(Document::sample());
    let frame = state.doc.project.atlas.pages[0].frames[0].id;
    state.atlas.selection = AtlasSelection::Frame(frame);
    let (img, _) = match headless::ui_png(state, (1440.0, 900.0), 1.0) {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    };
    std::fs::create_dir_all("../../target/app").unwrap();
    img.save("../../target/app/atlas.png").unwrap();
    assert_eq!((img.width(), img.height()), (1440, 900));
}

#[test]
fn the_desktop_frame_tab_renames_the_map_of_the_frame() {
    let mut h = harness();
    let s = h.state_mut();
    let frame = s.doc.project.atlas.pages[0].frames[0].id;
    s.atlas.selection = AtlasSelection::Frame(frame);
    h.run_steps(3);
    type_in(&mut h, MAP_NAME, "Dravenhold");
    let atlas = &h.state().doc.project.atlas;
    let map = atlas.pages[0].frames[0].map;
    assert_eq!(atlas.map(map).unwrap().name, "Dravenhold");
}

#[test]
fn the_style_editor_opens_from_layers_on_the_globe_and_the_flat_view() {
    use vale_app::globe::WorldView;
    use vale_app::ui::pad::filter::{ADD_CONDITION, REMOVE_CONDITION, VALUE};
    use vale_app::ui::pad::rules::{ADD_RULE, EXPRESSION, MIN_SCALE};
    for view in [WorldView::Globe, WorldView::Flat] {
        let mut h = globe_harness();
        h.state_mut().globe.world_view = view;
        h.run_steps(2);
        h.get_by_label(LAYERS_MENU).click();
        h.run_steps(2);
        h.get_by_label("Open Rivers").click();
        h.run_steps(2);
        let rivers = h.state().selected_vector().unwrap();
        assert!(h.state().globe.preview.panel, "{view:?}");
        h.get_by_label(ADD_RULE).click();
        h.run_steps(2);
        let rules = &h.state().doc.entry(rivers).unwrap().style.style.rules;
        assert_eq!(rules.len(), 2, "{view:?}");
        // The new rule is the selected rule. The first field is the field
        // of a new condition.
        let filter = |h: &Harness<'static, AppState>| {
            let rule = &h.state().doc.entry(rivers).unwrap().style.style.rules[1];
            rule.filter.source().map(|s| s.text().to_owned())
        };
        type_in(&mut h, VALUE, "River");
        h.get_by_label(ADD_CONDITION).click();
        h.run_steps(2);
        assert_eq!(
            filter(&h).as_deref(),
            Some("featurecla = 'River'"),
            "{view:?}"
        );
        h.get_by_label(&format!("{REMOVE_CONDITION} 1")).click();
        h.run_steps(2);
        assert_eq!(filter(&h), None, "{view:?}");
        type_in(&mut h, EXPRESSION, "scalerank < 3");
        type_in(&mut h, MIN_SCALE, "25000");
        assert_eq!(filter(&h).as_deref(), Some("scalerank < 3"), "{view:?}");
        let rule = &h.state().doc.entry(rivers).unwrap().style.style.rules[1];
        assert_eq!(rule.min_scale, Some(25_000.0), "{view:?}");
    }
}

#[test]
fn the_desktop_form_makes_a_raster_layer_from_an_image_file() {
    use vale_app::ui::raster_form::{self, RasterForm};
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/app/test-raster-file");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("desktop.png");
    let pixels: Vec<u16> = (0..64 * 32)
        .map(|i| if i % 64 < 32 { 2000 } else { 50000 })
        .collect();
    let image = image::ImageBuffer::<image::Luma<u16>, _>::from_raw(64, 32, pixels).unwrap();
    image.save(&path).unwrap();
    let notes = dir.join("desktop-notes.png");
    std::fs::write(&notes, "these bytes are not an image").unwrap();

    let mut h = globe_harness();
    let rasters = h.state().doc.project.rasters().len();
    let state = h.state_mut();
    state.form_face_size = Some(64);
    state.raster_form = Some(RasterForm::open(state));
    // The modal takes clicks after it fades in.
    h.run_steps(10);
    h.get_by_label(raster_form::SOURCES[1]).click();
    h.run_steps(2);
    // A form with no image opens the file picker at once.
    assert_eq!(h.state().actions, [Action::RasterImageDialog]);
    h.state_mut().actions.clear();
    h.get_by_label(raster_form::CHOOSE_IMAGE).click();
    h.run_steps(2);
    assert_eq!(h.state().actions, [Action::RasterImageDialog]);
    h.state_mut().actions.clear();

    // A file that is not an image shows a note, and the add button is off.
    h.state_mut().run_action(Action::RasterImage(notes));
    h.run_steps(2);
    assert!(
        h.query_by_label("desktop-notes is not a PNG or TIFF image")
            .is_some()
    );
    assert!(
        h.get_by_label(raster_form::ADD)
            .accesskit_node()
            .is_disabled()
    );

    h.state_mut().run_action(Action::RasterImage(path));
    h.run_steps(2);
    assert!(h.query_by_label("2,000 of 65,535").is_some());
    assert!(h.query_by_label("50,000 of 65,535").is_some());
    for label in [raster_form::DARKEST_VALUE, raster_form::BRIGHTEST_VALUE] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    h.get_by_label(raster_form::ADD).click();
    h.run_steps(2);
    let state = h.state();
    assert!(state.raster_form.is_none());
    let raster = state.doc.project.rasters().last().unwrap();
    assert_eq!(state.doc.project.rasters().len(), rasters + 1);
    let map = state.globe.raster_map(raster.id).unwrap();
    let mut texels = Vec::new();
    for face in 0..FACES {
        let full = TexelRect {
            x0: 0,
            y0: 0,
            x1: 64,
            y1: 64,
        };
        map.read_rect(face, full, &mut texels);
    }
    assert_eq!(texels.iter().min(), Some(&0));
    assert_eq!(texels.iter().max(), Some(&65535));
}

/// Command+Z, or Shift+Command+Z with `shift`.
fn command_z(h: &mut Harness<'static, AppState>, shift: bool) {
    let shift = if shift {
        egui::Modifiers::SHIFT
    } else {
        egui::Modifiers::NONE
    };
    h.event(egui::Event::Key {
        key: egui::Key::Z,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND | shift,
    });
    h.run_steps(2);
}

#[test]
fn the_toolbar_and_the_keys_work_the_measure_tool() {
    use vale_app::ui::pad::CLEAR_PATH;
    use vale_app::ui::panels::{MEASURE_TOOL, SNAP, SNAP_OPTIONS};
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.select(None);
    let mut h = Harness::builder()
        .with_size(egui::vec2(SIZE.0 as f32, SIZE.1 as f32))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(3);
    assert_eq!(h.state().workspace, Workspace::Globe);
    assert!(h.query_by_label(CLEAR_PATH).is_none());
    h.get_by_label(MEASURE_TOOL).click();
    h.run_steps(3);
    assert!(h.state().globe.measuring());
    for name in [SNAP, SNAP_OPTIONS, CLEAR_PATH] {
        assert!(h.query_by_label(name).is_some(), "{name}");
    }
    for name in ["Close path", "Remove point"] {
        assert!(h.query_by_label(name).is_none(), "{name}");
    }
    // With no points, the status bar shows the place under the cursor.
    assert!(h.query_by_label("lon –, lat –").is_some());

    let center = h.state().globe.rect.center();
    for offset in [(-50.0, -40.0), (50.0, -40.0), (0.0, 50.0)] {
        click_at(&mut h, center + egui::vec2(offset.0, offset.1));
    }
    assert_eq!(h.state().measure.points.len(), 3);
    let text = h.state().measure_readout().unwrap();
    assert!(text.starts_with("Length "), "{text}");
    assert!(h.query_by_label(&text).is_some(), "{text}");

    // Return closes the path, Command+Z reopens it and removes a point,
    // Shift+Command+Z puts back the point, and Escape clears the path.
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert!(h.state().measure.closed);
    command_z(&mut h, false);
    assert!(!h.state().measure.closed);
    command_z(&mut h, false);
    assert_eq!(h.state().measure.points.len(), 2);
    command_z(&mut h, true);
    assert_eq!(h.state().measure.points.len(), 3);
    h.key_press(egui::Key::Backspace);
    h.run_steps(2);
    assert_eq!(h.state().measure.points.len(), 3);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().measure.is_empty());
    command_z(&mut h, false);
    assert_eq!(h.state().measure.points.len(), 3);

    // The button of the tool bar.
    h.get_by_label(CLEAR_PATH).click();
    h.run_steps(3);
    assert!(h.state().measure.is_empty());
    assert!(h.get_by_label(CLEAR_PATH).accesskit_node().is_disabled());

    // Navigate leaves the tool, and M chooses it.
    h.get_by_label("Navigate").click();
    h.run_steps(3);
    assert!(!h.state().globe.measuring());
    h.key_press(egui::Key::M);
    h.run_steps(2);
    assert!(h.state().globe.measuring());
}

#[test]
fn the_toolbar_and_the_keys_work_the_measure_tool_in_the_atlas() {
    use vale_app::ui::atlas::AtlasTool;
    use vale_app::ui::pad::CLEAR_PATH;
    use vale_app::ui::panels::{MEASURE_TOOL, SNAP, SNAP_OPTIONS};
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.workspace = Workspace::Atlas;
    vale_app::ui::atlas::add_frame(&mut state);
    let mut h = Harness::builder()
        .with_size(egui::vec2(SIZE.0 as f32, SIZE.1 as f32))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(3);
    h.get_by_label(MEASURE_TOOL).click();
    h.run_steps(3);
    assert_eq!(h.state().atlas.tool, AtlasTool::Measure);
    for name in [SNAP, SNAP_OPTIONS, CLEAR_PATH] {
        assert!(h.query_by_label(name).is_some(), "{name}");
    }
    assert!(h.query_by_label("lon –, lat –").is_some());
    let page = vale_app::ui::atlas::open_page(h.state()).clone();
    let anchor = vale_app::atlas::anchor(&page.frames[0].shape);
    let center = h.state().atlas.place.unwrap().to_screen(anchor);
    let taps = |h: &mut Harness<'static, AppState>| {
        for offset in [(-50.0, -40.0), (50.0, -40.0), (0.0, 50.0)] {
            click_at(h, center + egui::vec2(offset.0, offset.1));
        }
    };
    taps(&mut h);
    assert_eq!(h.state().measure.points.len(), 3);
    let text = h.state().measure_readout().unwrap();
    assert!(text.starts_with("Length "), "{text}");
    assert!(h.query_by_label(&text).is_some(), "{text}");

    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert!(h.state().measure.closed);
    command_z(&mut h, false);
    command_z(&mut h, false);
    assert_eq!(h.state().measure.points.len(), 2);
    command_z(&mut h, true);
    command_z(&mut h, true);
    assert!(h.state().measure.closed);
    assert_eq!(open_page_frames(&h), page.frames);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().measure.is_empty());

    taps(&mut h);
    h.get_by_label(CLEAR_PATH).click();
    h.run_steps(3);
    assert!(h.state().measure.is_empty());
    assert_eq!(open_page_frames(&h), page.frames);

    // V leaves the tool, and M chooses it.
    h.key_press(egui::Key::V);
    h.run_steps(2);
    assert_eq!(h.state().atlas.tool, AtlasTool::Select);
    h.key_press(egui::Key::M);
    h.run_steps(2);
    assert_eq!(h.state().atlas.tool, AtlasTool::Measure);
}

fn open_page_frames(h: &Harness<'_, AppState>) -> Vec<vale_store::FrameRecord> {
    vale_app::ui::atlas::open_page(h.state()).frames.clone()
}
