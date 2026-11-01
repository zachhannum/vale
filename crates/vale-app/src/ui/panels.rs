//! Tool bar, left panel, and status bar.

use eframe::egui;
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::{GeometryKind, RasterId, World};

use super::pad::atlas::{LAYOUT_GRID, NEXT_PAGE, PREVIOUS_PAGE, SNAP_TO_LAYOUT, page_title};
use super::raster_form::RasterForm;
use super::world_form::{self, WorldForm};
use super::{Action, AppState, ExportFormat, LayerRef, SaveMark, Workspace, atlas, elevation, pad};
use vale_terrain::Mode;

use crate::globe::brush::{
    CHANNEL_HELP, CHANNEL_KM, EROSION, FLOODPLAIN_WIDTH_KM, FLOW, RANGE_SPACING_KM, RIDGE_CREST_M,
    RIDGE_MAGMA, RIDGE_OFFSET_KM, RIDGE_SEGMENT_KM, RIDGE_SPREAD_MM, SCALE_KM, SEED, STRENGTH_M,
    TALUS_DEG, WARP_KM, needs_height,
};
use crate::globe::{Globe, Tool, WorldView, rivers, snap, stats, stroke_test, zoom_test};
use crate::projects;
use crate::world::{self, three_significant};

/// The name of the ocean ridge mode of the brush.
pub const OCEAN_RIDGE: &str = "Ocean ridge";
/// The name of the range mode of the brush.
pub const RANGE: &str = "Range";

/// The button of the tool bar that opens the properties of the world.
pub const WORLD_BUTTON: &str = "World…";
/// The button of the tool bar that exports the heightmap.
pub const EXPORT_GEOTIFF: &str = "Export GeoTIFF…";
/// The button of the tool bar that exports the heightmap as a PNG.
pub const EXPORT_PNG: &str = "Export PNG…";
/// The button of the line tool.
pub const LINE_TOOL: &str = "Line";
/// The button of the polygon tool.
pub const POLYGON_TOOL: &str = "Polygon";
/// The button of the point tool.
pub const POINT_TOOL: &str = "Point";
/// The button of the Measure tool.
pub const MEASURE_TOOL: &str = "Measure";
/// The slider of the smoothing of the line tool.
pub const SMOOTHING: &str = "Smoothing";
/// The check box of the terrain fit of the line tool.
pub const TERRAIN_FIT: &str = "Terrain fit";
/// The slider of the balance between the sketch and the terrain.
pub const BALANCE: &str = "Balance";
/// The check box that turns all snapping on and off.
pub const SNAP: &str = "Snap";
/// The menu of the snap targets and the snap distance.
pub const SNAP_OPTIONS: &str = "Snap options";
pub const SNAP_VERTICES: &str = "Snap to vertices";
pub const SNAP_EDGES: &str = "Snap to edges";
pub const SNAP_DISTANCE: &str = "Snap distance";

/// The name of the switch that makes the color stops of a raster layer
/// snap targets.
pub fn snap_stops_name(layer: &str) -> String {
    format!("Snap to stops of {layer}")
}

/// The raster layers of the project, with their names, in the list order.
pub fn raster_layers(state: &AppState) -> Vec<(RasterId, String)> {
    let layers = state.doc.project.layers().iter();
    layers
        .filter_map(|l| Some((l.raster()?, l.name.clone())))
        .collect()
}

pub fn toolbar(ui: &mut egui::Ui, state: &mut AppState) {
    egui::Panel::top("toolbar").show(ui, |ui| {
        // In a narrow window, the buttons go to a second row.
        ui.horizontal_wrapped(|ui| {
            ui.strong("Vale");
            ui.separator();
            if state.projects.any() {
                file_menu(ui, state);
                save_mark(ui, state);
                ui.separator();
            }
            ui.selectable_value(&mut state.workspace, Workspace::Globe, "Globe");
            ui.selectable_value(&mut state.workspace, Workspace::Atlas, "Atlas");
            ui.separator();
            if ui.button(world_form::NEW_WORLD).clicked() {
                state.world_form = Some(WorldForm::new_world());
            }
            if ui.button(WORLD_BUTTON).clicked() {
                let form = WorldForm::properties(&state.doc.project.world);
                state.world_form = Some(form);
            }
            if state.workspace == Workspace::Globe {
                ui.separator();
                // With no layer for the tool, the tool is Navigate.
                let globe = &mut state.globe;
                let (paintable, lineable) = (globe.paintable, globe.lineable);
                let navigate = !globe.brushing() && !globe.editing() && !globe.measuring();
                if ui.selectable_label(navigate, "Navigate").clicked() {
                    globe.tool = Tool::Navigate;
                }
                let brush = egui::Button::selectable(globe.brushing(), "Brush");
                if ui.add_enabled(paintable, brush).clicked() {
                    globe.tool = Tool::Brush;
                }
                // A polygon layer has the polygon tool in the place of the
                // line tool, and a point layer has the point tool.
                let (name, tool) = if globe.fillable {
                    (POLYGON_TOOL, Tool::Polygon)
                } else if globe.pointable {
                    (POINT_TOOL, Tool::Point)
                } else {
                    (LINE_TOOL, Tool::Line)
                };
                let draw = egui::Button::selectable(globe.editing(), name);
                let able = lineable || globe.fillable || globe.pointable;
                if ui.add_enabled(able, draw).clicked() {
                    globe.tool = tool;
                }
                if ui
                    .selectable_label(globe.measuring(), MEASURE_TOOL)
                    .clicked()
                {
                    globe.tool = Tool::Measure;
                }
                if globe.editing() || globe.measuring() {
                    if globe.drawing() {
                        let mut percent = globe.line.smoothing * 100.0;
                        let slider = egui::Slider::new(&mut percent, 0.0..=100.0)
                            .suffix(" %")
                            .text(SMOOTHING);
                        if ui.add(slider).changed() {
                            globe.line.smoothing = percent / 100.0;
                        }
                    }
                    if globe.lining() {
                        let terrain = globe.fit_layer.is_some();
                        let fit = egui::Checkbox::new(&mut globe.line.fit, TERRAIN_FIT);
                        ui.add_enabled(terrain, fit);
                        let mut percent = globe.line.terrain * 100.0;
                        let slider = egui::Slider::new(&mut percent, 0.0..=100.0)
                            .suffix(" %")
                            .text(BALANCE);
                        if ui.add_enabled(terrain && globe.line.fit, slider).changed() {
                            globe.line.terrain = percent / 100.0;
                        }
                    }
                    let rasters = raster_layers(state);
                    snap_controls(ui, state, &rasters);
                }
                if state.globe.measuring() {
                    path_buttons(ui, state);
                }
                // The menu stays open while the user works on the list.
                let stay = egui::containers::menu::MenuConfig::new()
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
                let menu = egui::containers::menu::MenuButton::new(LAYERS_MENU).config(stay);
                menu.ui(ui, |ui| {
                    ui.label(egui::RichText::new(pad::NEW_SECTION).weak().small());
                    if ui.button(ADD_RASTER).clicked() {
                        state.raster_form = Some(RasterForm::open(state));
                        ui.close();
                    }
                    ui.separator();
                    ui.label(egui::RichText::new(pad::IMPORT_SECTION).weak().small());
                    let free = state.image_import.is_none();
                    if ui.add_enabled(free, egui::Button::new(ADD_IMAGE)).clicked() {
                        state.actions.push(Action::ImageLayerDialog);
                        ui.close();
                    }
                    if ui.button(ADD_GEOJSON).clicked() {
                        state.actions.push(Action::GeoJsonDialog);
                        ui.close();
                    }
                    ui.separator();
                    layer_rows(ui, state);
                    selected_layer(ui, state);
                });
                ui.separator();
                let mut flat = state.globe.world_view == WorldView::Flat;
                if ui.toggle_value(&mut flat, "Flat").changed() {
                    state.globe.world_view = if flat {
                        WorldView::Flat
                    } else {
                        WorldView::Globe
                    };
                }
                if flat {
                    flat_projection(ui, state);
                }
                ui.toggle_value(&mut state.globe.preview.graticule, "Graticule");
                ui.toggle_value(&mut state.globe.preview.panel, elevation::PANEL);
                ui.toggle_value(&mut state.globe.debug, "Debug");
                ui.separator();
                let import = egui::Button::new("Import heightmap…");
                if ui.add_enabled(!state.globe.busy(), import).clicked() {
                    state.actions.push(Action::ImportHeightmapDialog);
                }
                let export = egui::Button::new(EXPORT_GEOTIFF);
                if ui.add_enabled(state.can_export(), export).clicked() {
                    state.actions.push(Action::ExportHeightmapDialog);
                }
                let export_png = egui::Button::new(EXPORT_PNG);
                if ui.add_enabled(state.can_export(), export_png).clicked() {
                    state.actions.push(Action::ExportHeightmapPngDialog);
                }
            }
            if state.workspace != Workspace::Atlas {
                return;
            }
            ui.separator();
            atlas_buttons(ui, state);
            if !state.file_buttons {
                return;
            }
            ui.separator();
            if ui.button("Open GeoJSON…").clicked() {
                state.actions.push(Action::OpenDialog);
            }
            let ready = state.atlas_export.is_none();
            if ui
                .add_enabled(ready, egui::Button::new("Export page PNG…"))
                .clicked()
            {
                state.actions.push(Action::ExportDialog(ExportFormat::Png));
            }
            if ui
                .add_enabled(ready, egui::Button::new("Export page PDF…"))
                .clicked()
            {
                state.actions.push(Action::ExportDialog(ExportFormat::Pdf));
            }
            if ui
                .add_enabled(ready, egui::Button::new("Export atlas PDF…"))
                .clicked()
            {
                state
                    .actions
                    .push(Action::ExportDialog(ExportFormat::AtlasPdf));
            }
            if state.atlas_export.is_some() && ui.button("Cancel export").clicked() {
                state.actions.push(Action::CancelExport);
            }
        });
    });
}

/// The Snap check box and the menu of the snap settings. `rasters` are the
/// raster layers whose color stops can be targets.
fn snap_controls(ui: &mut egui::Ui, state: &mut AppState, rasters: &[(RasterId, String)]) {
    let snap = &mut state.globe.snap;
    ui.checkbox(&mut snap.on, SNAP);
    ui.menu_button(SNAP_OPTIONS, |ui| {
        ui.checkbox(&mut snap.vertices, SNAP_VERTICES);
        ui.checkbox(&mut snap.edges, SNAP_EDGES);
        for (id, name) in rasters {
            let mut on = snap.stops(*id);
            if ui.checkbox(&mut on, snap_stops_name(name)).changed() {
                snap.set_stops(*id, on);
            }
        }
        let slider = egui::Slider::new(&mut snap.distance, snap::DISTANCE_RANGE)
            .step_by(1.0)
            .suffix(" pt")
            .text(SNAP_DISTANCE);
        ui.add(slider);
    });
}

/// The button of the Measure tool that clears the path.
fn path_buttons(ui: &mut egui::Ui, state: &mut AppState) {
    let path = &mut state.measure;
    let clear = egui::Button::new(pad::CLEAR_PATH);
    if ui.add_enabled(!path.is_empty(), clear).clicked() {
        path.clear();
    }
}

/// The row of the Add menu that adds a map frame to the open page.
pub const ADD_MAP_FRAME: &str = "Map frame";
/// The menu of the tool bar that adds a frame or an item of furniture.
pub const ADD_MENU: &str = "Add";

/// The page buttons of the tool bar in the Atlas workspace: the page before,
/// the name of the open page, the page after, and a new map frame.
fn atlas_buttons(ui: &mut egui::Ui, state: &mut AppState) {
    let (index, count) = atlas::page_index(state);
    let back = ui.add_enabled(index > 0, egui::Button::new("◀"));
    named(&back, egui::WidgetType::Button, false, PREVIOUS_PAGE.into());
    if back.clicked() {
        atlas::go_to_page(state, -1);
    }
    let (index, _) = atlas::page_index(state);
    ui.label(page_title(index, &atlas::open_page(state).name));
    let next = ui.add_enabled(index + 1 < count, egui::Button::new("▶"));
    named(&next, egui::WidgetType::Button, false, NEXT_PAGE.into());
    if next.clicked() {
        atlas::go_to_page(state, 1);
    }
    ui.menu_button(ADD_MENU, |ui| {
        if ui.button(ADD_MAP_FRAME).clicked() {
            atlas::add_frame(state);
        }
        for kind in atlas::ItemKind::ALL {
            if ui.button(kind.name()).clicked() {
                atlas::add_item(state, kind);
            }
        }
    });
    let editing = matches!(state.atlas.selection, atlas::AtlasSelection::Frame(_));
    if editing && state.atlas.tool == atlas::AtlasTool::Select {
        let snap = ui.selectable_label(state.atlas.snap, SNAP_TO_LAYOUT);
        if snap.clicked() {
            state.atlas.snap = !state.atlas.snap;
        }
    }
    let on = state.doc.project.atlas.grid.on;
    if ui.selectable_label(on, LAYOUT_GRID).clicked() {
        atlas::edit_grid(state, |g| g.on = !on);
    }
    ui.separator();
    let measuring = state.measuring();
    if ui.selectable_label(measuring, MEASURE_TOOL).clicked() {
        pad::toggle_measure(state);
    }
    if measuring {
        snap_controls(ui, state, &[]);
        path_buttons(ui, state);
    }
}

/// The button of the tool bar that opens the list of the layers.
pub const LAYERS_MENU: &str = "Layers";
/// The button that opens the form of a new raster layer.
pub const ADD_RASTER: &str = "Add raster…";
/// The button that adds an image layer from a file.
pub const ADD_IMAGE: &str = "Add image…";
/// The button that adds vector layers from a GeoJSON file.
pub const ADD_GEOJSON: &str = "Add GeoJSON…";
/// The field of the Layers menu with the name of the selected layer.
pub const LAYER_NAME: &str = "Layer name";
/// The slider of the Layers menu with the opacity of the selected raster
/// layer.
pub const OPACITY: &str = "Layer opacity";
/// The menu of the blend mode of a raster layer.
pub const BLEND: &str = "Blend mode";
/// The button of the Layers menu that deletes the selected layer.
pub const DELETE_LAYER: &str = "Delete layer";

/// A widget with a name for a screen reader and for a test.
fn named(resp: &egui::Response, kind: egui::WidgetType, on: bool, name: String) {
    resp.widget_info(|| egui::WidgetInfo::selected(kind, true, on, name.clone()));
}

/// The rows of the layers of each kind, with the top layer first. A
/// checkbox shows or hides a layer, the name selects it, and the arrows
/// move it in the draw order. The arrow at the end of the row of a raster
/// layer opens its panel.
fn layer_rows(ui: &mut egui::Ui, state: &mut AppState) {
    let layers = state.doc.project.layers().iter().rev();
    let rows: Vec<_> = layers
        .map(|l| (l.id, LayerRef::of(l), l.name.clone(), l.visible))
        .collect();
    let count = rows.len();
    for (index, (id, layer, name, mut visible)) in rows.into_iter().enumerate() {
        let selected = state.selected_layer == Some(layer);
        ui.horizontal(|ui| {
            let eye = ui.checkbox(&mut visible, "");
            named(
                &eye,
                egui::WidgetType::Checkbox,
                visible,
                format!("Show {name}"),
            );
            if eye.changed() {
                state.set_layer_visible(id, visible);
            }
            for (text, up, end) in [("▲", true, 0), ("▼", false, count - 1)] {
                let button = ui.add_enabled(index != end, egui::Button::new(text).small());
                let way = if up { "up" } else { "down" };
                named(
                    &button,
                    egui::WidgetType::Button,
                    false,
                    format!("Move {name} {way}"),
                );
                if button.clicked() {
                    state.move_layer_by_one(id, up);
                }
            }
            let kind = match layer {
                LayerRef::Raster(_) => "raster",
                LayerRef::Image(_) => "image",
                LayerRef::Vector(_) => "vector",
            };
            let pick = ui.selectable_label(selected, &name);
            named(
                &pick,
                egui::WidgetType::SelectableLabel,
                selected,
                format!("Select {name}"),
            );
            if pick.clicked() {
                state.select(Some(layer));
            }
            ui.weak(kind);
            let open = ui.small_button("›");
            named(
                &open,
                egui::WidgetType::Button,
                false,
                format!("Open {name}"),
            );
            if open.clicked() {
                state.select(Some(layer));
                state.globe.preview.panel = true;
            }
        });
    }
}

/// The controls of the selected layer: its name, the opacity of a raster
/// layer, and the button that deletes the layer.
fn selected_layer(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(layer) = state.selected_layer else {
        return;
    };
    let Some((id, name)) = state.layer_of(layer).map(|l| (l.id, l.name.clone())) else {
        return;
    };
    ui.separator();
    let mut text = name.clone();
    let field = egui::TextEdit::singleline(&mut text).desired_width(160.0);
    let field = ui.add(field);
    named(
        &field,
        egui::WidgetType::TextEdit,
        false,
        LAYER_NAME.to_string(),
    );
    if field.changed() && state.doc.rename_layer(id, &text) {
        state.touch();
    }
    let project = &state.doc.project;
    let look = match layer {
        LayerRef::Raster(id) => project.raster(id).map(|r| (r.opacity, r.blend)),
        LayerRef::Image(id) => project.image(id).map(|i| (i.opacity, i.blend)),
        LayerRef::Vector(_) => None,
    };
    if let Some((opacity, blend)) = look {
        let mut percent = (opacity * 100.0).round();
        let slider = egui::Slider::new(&mut percent, 0.0..=100.0)
            .suffix(" %")
            .fixed_decimals(0)
            .text("Opacity");
        let slider = ui.add(slider);
        named(
            &slider,
            egui::WidgetType::Slider,
            false,
            OPACITY.to_string(),
        );
        if slider.changed() {
            match layer {
                LayerRef::Raster(id) => state.set_raster_opacity(id, percent / 100.0),
                LayerRef::Image(id) => state.set_image_opacity(id, percent / 100.0),
                LayerRef::Vector(_) => false,
            };
        }
        let mut chosen = blend;
        let menu = egui::ComboBox::from_label(BLEND)
            .selected_text(elevation::blend_label(blend))
            .show_ui(ui, |ui| {
                for mode in vale_store::BlendMode::ALL {
                    let label = elevation::blend_label(mode);
                    ui.selectable_value(&mut chosen, mode, label);
                }
            });
        named(
            &menu.response,
            egui::WidgetType::ComboBox,
            false,
            BLEND.to_string(),
        );
        if chosen != blend {
            match layer {
                LayerRef::Raster(id) => state.set_raster_blend(id, chosen),
                LayerRef::Image(id) => state.set_image_blend(id, chosen),
                LayerRef::Vector(_) => false,
            };
        }
    }
    if let LayerRef::Image(id) = layer
        && let Some(image) = state.doc.project.image(id)
    {
        ui.label(format!("{} × {} px", image.width, image.height));
    }
    if let Some((note, of)) = &state.image_note
        && (*of == state.selected_image() || of.is_none())
    {
        ui.label(&note.text);
    }
    if ui.button(DELETE_LAYER).clicked() {
        state.remove_layer(id);
    }
}

/// The button of the tool bar that opens the File menu.
pub const FILE_MENU: &str = "File";
/// The row of a menu that closes the project and shows the start screen.
pub const ALL_WORLDS: &str = "All worlds";

/// The File menu: the actions of the start screen, and the recent list.
fn file_menu(ui: &mut egui::Ui, state: &mut AppState) {
    let open = state.open_project_entry();
    ui.menu_button(FILE_MENU, |ui| {
        if ui.button("New…").clicked() {
            state.world_form = Some(WorldForm::new_world());
        }
        if state.file_buttons && ui.button("Open file…").clicked() {
            state.actions.push(Action::OpenProjectDialog);
        }
        ui.menu_button("Open recent", |ui| {
            let dir = state.projects.state_dir();
            let recent = dir.map(projects::recent).unwrap_or_default();
            if recent.is_empty() {
                ui.weak("No recent worlds");
            }
            for entry in recent.iter().map(|path| projects::entry(path)) {
                if ui.button(&entry.name).clicked() {
                    state.actions.push(Action::OpenWorld(entry.path));
                }
            }
        });
        ui.separator();
        ui.add_enabled_ui(open.is_some(), |ui| {
            let (path, name) = open.clone().unwrap_or_default();
            let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
            let save =
                egui::Button::new(pad::SAVE).shortcut_text(ui.ctx().format_shortcut(&shortcut));
            if ui.add(save).clicked() {
                state.actions.push(Action::Save);
            }
            if ui.button("Rename…").clicked() {
                state.browser.ask_rename(path.clone(), name.clone());
            }
            if ui.button("Duplicate").clicked() {
                state.actions.push(Action::CopyProject(path.clone()));
            }
            if ui.button("Delete…").clicked() {
                state.browser.ask_delete(path, name);
            }
        });
        ui.separator();
        if ui.button(ALL_WORLDS).clicked() {
            state.actions.push(Action::ShowProjects);
        }
    });
}

/// The state of the save of the open project, next to the File menu.
fn save_mark(ui: &mut egui::Ui, state: &AppState) {
    match state.save_mark(std::time::Instant::now()) {
        // A test has no clock, so there the mark does not turn.
        Some(SaveMark::Saving) if !state.headless => {
            ui.add(egui::Spinner::new().size(12.0));
        }
        Some(SaveMark::Saving) => drop(ui.weak("Saving")),
        Some(SaveMark::Saved) => drop(ui.weak("Saved")),
        Some(SaveMark::Failed) => drop(ui.colored_label(ui.visuals().warn_fg_color, "Not saved")),
        None => {}
    }
}

fn flat_projection(ui: &mut egui::Ui, state: &mut AppState) {
    let flat = &mut state.globe.flat;
    let spec = flat.spec();
    let mut kind = spec.kind;
    egui::ComboBox::from_id_salt("flat-projection")
        .selected_text(kind.name())
        .show_ui(ui, |ui| {
            for k in ProjectionKind::ALL {
                ui.selectable_value(&mut kind, k, k.name());
            }
        });
    flat.set_spec(ProjectionSpec { kind, ..spec });
    let button = ui.add_enabled(flat.can_center_here(), egui::Button::new(pad::RECENTER));
    let help = "Center the projection on the middle of the view. Move the map first.";
    if button
        .on_hover_text(help)
        .on_disabled_hover_text(help)
        .clicked()
    {
        flat.center_here();
    }
    let button = ui.add_enabled(flat.can_reset(), egui::Button::new(pad::RESET));
    let help = "Reset center and show the whole map";
    if button
        .on_hover_text(help)
        .on_disabled_hover_text(help)
        .clicked()
    {
        flat.reset();
    }
}

/// The numbers of the brush on the GPU, and controls for a test of the brush.
pub fn brush_debug(ui: &mut egui::Ui, state: &mut AppState) {
    if !state.globe.debug {
        return;
    }
    egui::Panel::right("brush_debug")
        .default_size(300.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Brush debug");
                debug_controls(ui, state);
            });
        });
}

/// The state of the window of small rivers.
fn window_line(globe: &Globe, world: &World) -> String {
    match globe.window() {
        Some(channels) => {
            let km = rivers::cell_angle(&channels.window()) * world.radius_km;
            let size = world::distance(km, world.units);
            format!("River window: cells of {size}")
        }
        None => "River window: off".to_string(),
    }
}

pub(super) fn debug_controls(ui: &mut egui::Ui, state: &mut AppState) {
    let world = &state.doc.project.world;
    // The undo log of the project file has the strokes.
    let session = state.session.as_ref();
    let log = session.map(|s| (s.undo_bytes(), s.undo_limit()));
    let globe = &mut state.globe;
    ui.label(globe.stats.gpu_line());
    ui.label(globe.stats.frame_time_line());
    ui.checkbox(&mut globe.blur, "Blur behind the cards");
    match globe.stats.last() {
        Some(stroke) => {
            ui.label(stroke.title());
            ui.label(stroke.delay_line());
            ui.label(stroke.frame_line());
            ui.label(stroke.stamps_line());
            ui.label(stroke.passes_line());
            ui.label(stroke.texels_line());
        }
        None => {
            ui.label("Stroke delay: no stroke");
        }
    }
    ui.label(globe.stats.worst_line());
    ui.label(format!("Backlog: {} stamps", globe.stats.backlog));
    ui.label(window_line(globe, world));
    ui.weak(stats::DELAY_NOTE);
    ui.separator();

    ui.heading("Pen and timing");
    ui.label(globe.pen.source_line());
    ui.label(globe.pen.stats.stroke_line());
    ui.label(globe.pen.stats.force_line());
    ui.label(globe.pen.stats.tilt_line());
    ui.label(globe.pen.stats.hover_line());
    let (pens, fingers) = globe.nav.counts();
    ui.label(format!("Touches: {pens} pen, {fingers} finger"));
    ui.label(format!("Palms ignored: {}", globe.nav.palms));
    ui.separator();

    const MB: f64 = (1 << 20) as f64;
    ui.label(match log {
        Some((bytes, limit)) => format!(
            "Undo log: {:.1} MB of {:.0} MB",
            bytes as f64 / MB,
            limit as f64 / MB
        ),
        None => format!(
            "Undo memory: {:.1} MB of {:.0} MB",
            globe.map.undo_memory_bytes() as f64 / MB,
            globe.map.undo_memory_limit() as f64 / MB,
        ),
    });
    ui.separator();

    let test = egui::Button::new("Run stroke test");
    if ui.add_enabled(!globe.busy(), test).clicked() {
        // The test paints a height layer.
        state.select_height();
        state.globe.start_stroke_test(stroke_test::SECONDS);
    }
    if let Some(report) = &state.globe.test_report {
        ui.separator();
        ui.strong("Stroke test");
        ui.label(report);
    }
    ui.separator();
    let test = egui::Button::new("Run zoom test");
    if ui.add_enabled(!state.globe.zoom_testing(), test).clicked() {
        state.globe.start_zoom_test(zoom_test::SECONDS);
    }
    if let Some(report) = &state.globe.zoom_report {
        ui.strong("Zoom test");
        ui.label(report);
    }
}

/// The controls of the brush. The panel is open in the brush tool.
pub fn brush(ui: &mut egui::Ui, state: &mut AppState) {
    if !state.globe.brushing() {
        state.globe.pick_level = false;
        return;
    }
    egui::Panel::left("brush")
        .default_size(240.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Brush");
                let height = state.globe.is_height();
                brush_modes(ui, &mut state.globe.brush.mode, height);
                ui.add_space(6.0);
                brush_size(ui, state);
                brush_limits(ui, state);
                brush_settings(ui, state);
            });
        });
}

/// The buttons of the brush modes. Without `height`, the modes that need
/// heights in meters are off.
fn brush_modes(ui: &mut egui::Ui, current: &mut Mode, height: bool) {
    ui.horizontal_wrapped(|ui| {
        for (mode, name) in [
            (Mode::Raise, "Raise"),
            (Mode::Lower, "Lower"),
            (Mode::Smooth, "Smooth"),
            (Mode::Flatten, "Flatten"),
            (Mode::Carve, "Carve"),
            (Mode::Ridge, "Ridge"),
            (Mode::Deposit, "Deposit"),
            (Mode::Erode, "Erode"),
            (Mode::Weather, "Weather"),
            (Mode::Uplift, "Uplift"),
            (Mode::Warp, "Warp"),
            (Mode::Push, "Push"),
            (Mode::Noise, "Noise"),
            (Mode::OceanRidge, OCEAN_RIDGE),
            (Mode::Range, RANGE),
        ] {
            let button = egui::Button::selectable(*current == mode, name);
            if ui
                .add_enabled(height || !needs_height(mode), button)
                .clicked()
            {
                *current = mode;
            }
        }
    });
}

/// A part as a percent with its sign, for example "+5%". Zero has no sign.
pub(super) fn signed_percent(part: f64) -> String {
    let percent = (part * 100.0).round();
    if percent == 0.0 {
        "0%".to_string()
    } else {
        format!("{percent:+.0}%")
    }
}

/// The line with the age of the crust at the edge of an ocean ridge
/// stroke. `radius_km` is the brush radius on the ground.
pub(super) fn edge_age_text(brush: &crate::globe::brush::BrushSettings, radius_km: f64) -> String {
    let age = brush.ridge_edge_age(radius_km);
    let age = if age >= 10.0 {
        format!("{age:.0}")
    } else {
        three_significant(age)
    };
    format!("Edge age {age} Myr")
}

/// The name of the strength slider. In the uplift mode, the strength is the
/// rate of the uplift, and in the range mode the height of the range.
pub(super) fn strength_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Uplift => "Uplift",
        Mode::Range => "Height",
        _ => "Strength",
    }
}

fn brush_size(ui: &mut egui::Ui, state: &mut AppState) {
    let unit = state.shown_unit().to_string();
    let world_km = state.doc.project.world.radius_km;
    let (unit_km, symbol) = world::large_unit(state.doc.project.world.units);
    let globe = &mut state.globe;
    let globe_radius = globe.radius();
    let mut size = globe.brush.radius_km(globe_radius, world_km) / unit_km;
    let range = globe.brush.radius_km_range(globe_radius, world_km);
    let range = range.start() / unit_km..=range.end() / unit_km;
    let slider = egui::Slider::new(&mut size, range)
        .logarithmic(true)
        .custom_formatter(|size, _| three_significant(size))
        .suffix(format!(" {symbol}"))
        .text("Radius");
    if ui.add(slider).changed() {
        globe
            .brush
            .set_radius_km(size * unit_km, globe_radius, world_km);
    }
    // The ocean ridge mode has no falloff and no strength, and the range
    // mode has no falloff.
    let ridge = globe.brush.mode == Mode::OceanRidge;
    if !matches!(globe.brush.mode, Mode::OceanRidge | Mode::Range) {
        ui.add(egui::Slider::new(&mut globe.brush.hardness, 0.0..=1.0).text("Hardness"));
    }
    ui.add(egui::Slider::new(&mut globe.brush.flow, FLOW).text("Flow"));
    if ridge {
        let brush = &mut globe.brush;
        let crest = egui::Slider::new(&mut brush.ridge_crest_m, RIDGE_CREST_M)
            .step_by(50.0)
            .fixed_decimals(0)
            .suffix(" m")
            .text("Crest");
        ui.add(crest);
        let spread = egui::Slider::new(&mut brush.ridge_spread_mm, RIDGE_SPREAD_MM)
            .logarithmic(true)
            .step_by(1.0)
            .fixed_decimals(0)
            .suffix(" mm/yr")
            .text("Spreading");
        ui.add(spread);
        let mut percent = brush.ridge_magma * 100.0;
        let range = RIDGE_MAGMA.start() * 100.0..=RIDGE_MAGMA.end() * 100.0;
        let magma = egui::Slider::new(&mut percent, range)
            .step_by(1.0)
            .custom_formatter(|percent, _| signed_percent(percent / 100.0))
            .text("Magma");
        if ui.add(magma).changed() {
            brush.ridge_magma = percent / 100.0;
        }
        let segment = egui::Slider::new(&mut brush.ridge_segment_km, RIDGE_SEGMENT_KM)
            .logarithmic(true)
            .custom_formatter(|km, _| three_significant(km))
            .suffix(" km")
            .text("Segment");
        ui.add(segment);
        let offset = egui::Slider::new(&mut brush.ridge_offset_km, RIDGE_OFFSET_KM)
            .fixed_decimals(0)
            .suffix(" km")
            .text("Offset");
        ui.add(offset);
        let radius_km = brush.radius_km(globe_radius, world_km);
        ui.label(edge_age_text(brush, radius_km));
    } else if globe.brush.mode == Mode::Warp {
        for (value, range, name) in [
            (&mut globe.brush.warp_km, WARP_KM, "Strength"),
            (&mut globe.brush.scale_km, SCALE_KM, "Scale"),
        ] {
            let slider = egui::Slider::new(value, range)
                .logarithmic(true)
                .custom_formatter(|km, _| three_significant(km))
                .suffix(" km")
                .text(name);
            ui.add(slider);
        }
        seed_slider(ui, state);
    } else if globe.brush.mode == Mode::Push {
        // The motion of the pen and the flow set the distance.
    } else {
        // The slider shows the strength as a value of the layer.
        let (min, max) = globe.brush.value_range;
        let (span, scale) = (
            max - min,
            globe.brush.strength_value() / globe.brush.strength_m,
        );
        let strength = egui::Slider::new(&mut globe.brush.strength_m, STRENGTH_M)
            .logarithmic(true)
            .custom_formatter(move |meters, _| world::value_text(meters * scale, span, &unit))
            .custom_parser(move |text| {
                let number: String = text.chars().take_while(|c| !c.is_alphabetic()).collect();
                Some(number.trim().parse::<f64>().ok()? / scale)
            })
            .text(strength_name(globe.brush.mode));
        ui.add(strength);
    }
    let globe = &mut state.globe;
    if globe.brush.mode == Mode::Noise {
        let scale = egui::Slider::new(&mut globe.brush.scale_km, SCALE_KM)
            .logarithmic(true)
            .custom_formatter(|km, _| three_significant(km))
            .suffix(" km")
            .text("Scale");
        ui.add(scale);
        ui.checkbox(&mut globe.brush.noise_ridged, "Ridged");
        if globe.brush.noise_ridged {
            ui.checkbox(&mut globe.brush.noise_inverted, "Invert");
        }
        // The relief follows the height above sea level.
        if globe.is_height() {
            let mut percent = globe.brush.noise_relief.clamp(0.0, 1.0) * 100.0;
            let relief = egui::Slider::new(&mut percent, 0.0..=100.0)
                .step_by(1.0)
                .suffix(" %")
                .text("Relief");
            if ui.add(relief).changed() {
                globe.brush.noise_relief = percent / 100.0;
            }
        }
        seed_slider(ui, state);
    }
    let globe = &mut state.globe;
    if globe.brush.mode == Mode::Range {
        let spacing = egui::Slider::new(&mut globe.brush.range_spacing_km, RANGE_SPACING_KM)
            .logarithmic(true)
            .custom_formatter(|km, _| three_significant(km))
            .suffix(" km")
            .text("Spacing");
        ui.add(spacing);
        seed_slider(ui, state);
    }
    let globe = &mut state.globe;
    if matches!(globe.brush.mode, Mode::Carve | Mode::Erode) {
        let channel = egui::Slider::new(&mut globe.brush.channel_km, CHANNEL_KM)
            .logarithmic(true)
            .custom_formatter(|km, _| three_significant(km))
            .suffix(" km")
            .text("Channel width");
        ui.add(channel).on_hover_text(CHANNEL_HELP);
    }
    if globe.brush.mode == Mode::Carve {
        let floodplain = egui::Slider::new(&mut globe.brush.floodplain_km, FLOODPLAIN_WIDTH_KM)
            .step_by(1.0)
            .fixed_decimals(0)
            .suffix(" km")
            .text("Floodplain");
        ui.add(floodplain);
    }
    if matches!(globe.brush.mode, Mode::Uplift | Mode::Range) {
        ui.add(egui::Slider::new(&mut globe.brush.erosion, EROSION).text("Erosion"));
    }
    if matches!(globe.brush.mode, Mode::Erode | Mode::Uplift | Mode::Range) {
        // The wind and the rain shadow are properties of the world, so each
        // brush gets the same rain.
        let world = &mut state.doc.project.world;
        let wind = egui::Slider::new(&mut world.wind_deg, 0.0..=360.0)
            .step_by(1.0)
            .custom_formatter(|deg, _| {
                let point = super::pad::widgets::compass_point(deg as f32);
                format!("{deg:.0}\u{b0} {point}")
            })
            .custom_parser(|text| {
                let number: String = text
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                number.parse().ok()
            })
            .text("Wind");
        ui.add(wind);
        let mut percent = world.rain_shadow.clamp(0.0, 1.0) * 100.0;
        let shadow = egui::Slider::new(&mut percent, 0.0..=100.0)
            .step_by(1.0)
            .suffix(" %")
            .text("Rain shadow");
        if ui.add(shadow).changed() {
            world.rain_shadow = percent / 100.0;
        }
    }
    if matches!(globe.brush.mode, Mode::Weather | Mode::Uplift) {
        let angle = egui::Slider::new(&mut globe.brush.talus_deg, TALUS_DEG)
            .logarithmic(true)
            .custom_formatter(|deg, _| three_significant(deg))
            .suffix("°")
            .text("Angle");
        ui.add(angle);
    }
}

/// The seed slider of the warp mode, the noise mode, and the range mode. The seed is a
/// property of the world, so each brush gets the same pattern.
fn seed_slider(ui: &mut egui::Ui, state: &mut AppState) {
    let seed = &mut state.doc.project.world.seed;
    ui.add(egui::Slider::new(seed, SEED).text("Seed"));
}

/// A field for a value of the layer that the brush paints, in the unit of
/// the layer. `range` has the values of the lowest and of the highest level.
fn value_drag<'a>(value: &'a mut f64, (min, max): (f64, f64), unit: &str) -> egui::DragValue<'a> {
    let span = max - min;
    egui::DragValue::new(value)
        .speed(span / 1200.0)
        .range(min..=max)
        .fixed_decimals(world::value_decimals(span))
        .suffix(format!(" {unit}"))
}

/// The floor and the ceiling of the brush. Each one has a checkbox and a
/// value.
fn brush_limits(ui: &mut egui::Ui, state: &mut AppState) {
    let unit = state.shown_unit().to_string();
    let brush = &mut state.globe.brush;
    let range = brush.value_range;
    for (name, on, value) in [
        ("Floor", &mut brush.floor_on, &mut brush.floor_m),
        ("Ceiling", &mut brush.ceiling_on, &mut brush.ceiling_m),
    ] {
        ui.horizontal(|ui| {
            ui.checkbox(on, name);
            ui.add_enabled(*on, value_drag(value, range, &unit));
        });
    }
}

/// The lock of the size and the flatten level.
pub(super) fn brush_settings(ui: &mut egui::Ui, state: &mut AppState) {
    let unit = state.shown_unit().to_string();
    let globe = &mut state.globe;
    let globe_radius = globe.radius();
    let mut lock = globe.brush.lock.is_some();
    let lock_box = ui.checkbox(&mut lock, "Lock size");
    lock_box.on_hover_text("Keep the brush size on the ground while zooming");
    globe.brush.set_lock(lock, globe_radius);

    if globe.brush.mode != Mode::Flatten {
        globe.pick_level = false;
        return;
    }
    ui.add_space(6.0);
    ui.strong("Flatten level");
    match globe.brush.flatten_level {
        Some(level) => {
            ui.horizontal(|ui| {
                let mut value = globe.brush.value(level);
                let range = globe.brush.value_range;
                if ui.add(value_drag(&mut value, range, &unit)).changed() {
                    globe.brush.flatten_level = Some(globe.brush.level(value));
                }
                if ui.button("Use stroke start").clicked() {
                    globe.brush.flatten_level = None;
                }
            });
        }
        None => {
            ui.label("Level at stroke start");
        }
    }
    let pick = egui::Button::new("Pick from globe").selected(globe.pick_level);
    let pick = ui.add(pick);
    if pick.clicked() {
        globe.pick_level = !globe.pick_level;
    }
    pick.on_hover_text("Set level from the next press on the globe");
}

pub fn left(ui: &mut egui::Ui, state: &mut AppState) {
    egui::Panel::left("left")
        .default_size(260.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                world_section(ui, state);
                layers_section(ui, state);
                map_section(ui, state);
            });
        });
}

fn world_section(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("World");
    let world = &state.doc.project.world;
    let symbol = world::large_unit(world.units).1;
    let radius = world::radius_text(world.radius_km, world.units);
    ui.label(&world.name);
    ui.label(format!("Radius {radius} {symbol}"));
    ui.add_space(6.0);
}

fn layers_section(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Layers");
    ui.menu_button("Add layer", |ui| {
        for (name, kind) in [
            ("Lines", GeometryKind::Line),
            ("Points", GeometryKind::Point),
            ("Polygons", GeometryKind::Polygon),
        ] {
            if ui.button(name).clicked() {
                let id = state.doc.add_layer(kind);
                state.select(Some(LayerRef::Vector(id)));
                state.touch();
                ui.close();
            }
        }
    });
    if state.doc.frame.entries.is_empty() {
        ui.label("No layers. Open a GeoJSON file.");
    }
    let ids: Vec<_> = state
        .doc
        .frame
        .entries
        .iter()
        .rev()
        .map(|e| e.layer)
        .collect();
    for id in ids {
        let Some((name, count)) = state
            .doc
            .layer(id)
            .map(|l| (l.name.clone(), l.features.len()))
        else {
            continue;
        };
        let selected = state.selected_vector() == Some(id);
        ui.horizontal(|ui| {
            let map = atlas::selected_map(state);
            let mut visible = state.doc.shown(map, id);
            let text = if selected {
                egui::RichText::new(name).strong()
            } else {
                egui::RichText::new(name)
            };
            let resp = ui.checkbox(&mut visible, text);
            ui.weak(format!("{count}"));
            if resp.changed() {
                state.set_map_shown(map, id, visible);
            }
            if resp.clicked() {
                state.select(Some(LayerRef::Vector(id)));
            }
            let pick = ui.selectable_label(selected, "Edit");
            let accessible = format!("Select {}", state.doc.layer(id).map_or("", |l| &l.name));
            pick.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    true,
                    selected,
                    accessible.clone(),
                )
            });
            if pick.clicked() {
                state.select(Some(LayerRef::Vector(id)));
            }
        });
    }
    let map = atlas::selected_map(state);
    let rasters: Vec<_> = state
        .doc
        .project
        .layers()
        .iter()
        .rev()
        .filter(|l| l.raster().is_some() || l.image().is_some())
        .map(|l| (l.id, l.name.clone()))
        .collect();
    for (id, name) in rasters {
        let mut on = state.doc.shown(map, id);
        let resp = ui.checkbox(&mut on, name.as_str());
        named(
            &resp,
            egui::WidgetType::Checkbox,
            on,
            format!("Show {name} in map"),
        );
        if resp.changed() && state.doc.set_shown(map, id, on) {
            state.touch();
        }
    }
    ui.add_space(6.0);
}

fn map_section(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Map");
    if ui.checkbox(&mut state.doc.frame.labels, "Labels").changed() {
        state.touch();
    }
}

pub fn status(ui: &mut egui::Ui, state: &mut AppState) {
    egui::Panel::bottom("status").show(ui, |ui| {
        ui.horizontal(|ui| {
            if let Some(text) = state.measure_readout() {
                ui.label(text);
            } else {
                match state.cursor_lonlat {
                    Some([lon, lat]) => ui.label(format!("lon {lon:.2}, lat {lat:.2}")),
                    None => ui.label("lon –, lat –"),
                };
                if let Some(value) = state.readout_value() {
                    ui.label(value);
                }
            }
            ui.separator();
            if let Some(c) = &state.composed
                && state.workspace == Workspace::Atlas
            {
                let units = state.doc.project.world.units;
                ui.label(format!("1 pt = {}", world::distance(c.km_per_point, units)));
                ui.separator();
                ui.label(format!(
                    "labels: {} placed, {} unplaced",
                    c.labels.placed,
                    c.labels.unplaced.len()
                ));
            }
            let progress = state.globe.import_progress();
            let progress = progress.or_else(|| state.image_progress());
            let progress = progress.or_else(|| state.globe.export_progress());
            let text = progress.as_deref().unwrap_or(&state.status);
            if !text.is_empty() {
                ui.separator();
                ui.label(text);
            }
        });
    });
}
