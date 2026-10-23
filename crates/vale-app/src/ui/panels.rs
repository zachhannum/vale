//! Tool bar, left panel, and status bar.

use eframe::egui;
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::{GeometryKind, RasterId, World};

use super::raster_form::RasterForm;
use super::world_form::{self, WorldForm};
use super::{Action, AppState, ExportFormat, LayerRef, SaveMark, Workspace, elevation, pad};
use vale_terrain::Mode;

use crate::globe::brush::{
    EROSION, FLOW, SCALE_KM, SEED, STRENGTH_M, TALUS_DEG, WARP_KM, needs_height,
};
use crate::globe::{Globe, Tool, WorldView, rivers, snap, stats, stroke_test, zoom_test};
use crate::projects;
use crate::world::{self, three_significant};

/// The button of the tool bar that opens the properties of the world.
pub const WORLD_BUTTON: &str = "World…";
/// The button of the tool bar that exports the heightmap.
pub const EXPORT_GEOTIFF: &str = "Export GeoTIFF…";
/// The button of the line tool.
pub const LINE_TOOL: &str = "Line";
/// The button of the polygon tool.
pub const POLYGON_TOOL: &str = "Polygon";
/// The button of the point tool.
pub const POINT_TOOL: &str = "Point";
/// The slider of the smoothing of the line tool.
pub const SMOOTHING: &str = "Smoothing";
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
    format!("Snap to the stops of {layer}")
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
            ui.selectable_value(&mut state.workspace, Workspace::Map, "Map");
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
                let navigate = !globe.brushing() && !globe.editing();
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
                if globe.editing() {
                    if globe.drawing() {
                        let mut percent = globe.line.smoothing * 100.0;
                        let slider = egui::Slider::new(&mut percent, 0.0..=100.0)
                            .suffix(" %")
                            .text(SMOOTHING);
                        if ui.add(slider).changed() {
                            globe.line.smoothing = percent / 100.0;
                        }
                    }
                    let rasters = raster_layers(state);
                    let snap = &mut state.globe.snap;
                    ui.checkbox(&mut snap.on, SNAP);
                    ui.menu_button(SNAP_OPTIONS, |ui| {
                        ui.checkbox(&mut snap.vertices, SNAP_VERTICES);
                        ui.checkbox(&mut snap.edges, SNAP_EDGES);
                        for (id, name) in &rasters {
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
                // The menu stays open while the user works on the list.
                let stay = egui::containers::menu::MenuConfig::new()
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
                let menu = egui::containers::menu::MenuButton::new(LAYERS_MENU).config(stay);
                menu.ui(ui, |ui| {
                    if ui.button(ADD_RASTER).clicked() {
                        state.raster_form = Some(RasterForm::open(state));
                        ui.close();
                    }
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
            }
            if !state.file_buttons || state.workspace != Workspace::Map {
                return;
            }
            ui.separator();
            if ui.button("Open GeoJSON…").clicked() {
                state.actions.push(Action::OpenDialog);
            }
            if ui.button("Export PNG…").clicked() {
                state.actions.push(Action::ExportDialog(ExportFormat::Png));
            }
            if ui.button("Export PDF…").clicked() {
                state.actions.push(Action::ExportDialog(ExportFormat::Pdf));
            }
        });
    });
}

/// The button of the tool bar that opens the list of the layers.
pub const LAYERS_MENU: &str = "Layers";
/// The button that opens the form of a new raster layer.
pub const ADD_RASTER: &str = "Add raster…";
/// The field of the Layers menu with the name of the selected layer.
pub const LAYER_NAME: &str = "Layer name";
/// The slider of the Layers menu with the opacity of the selected raster
/// layer.
pub const OPACITY: &str = "Layer opacity";
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
            if let LayerRef::Raster(_) = layer {
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
    if let LayerRef::Raster(raster) = layer {
        let opacity = state.doc.project.raster(raster).map_or(1.0, |r| r.opacity);
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
            state.set_raster_opacity(raster, percent / 100.0);
        }
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
            if ui.button("Make a copy").clicked() {
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
    let help = "Makes the place at the middle of the view the center of the projection. \
                Move the map first.";
    if button
        .on_hover_text(help)
        .on_disabled_hover_text(help)
        .clicked()
    {
        flat.center_here();
    }
    let button = ui.add_enabled(flat.can_reset(), egui::Button::new(pad::RESET));
    let help = "Puts the center of the projection back, and shows the whole map.";
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

/// The name of the strength slider. In the uplift mode, the strength is the
/// rate of the uplift.
pub(super) fn strength_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Uplift => "Uplift",
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
    ui.add(egui::Slider::new(&mut globe.brush.hardness, 0.0..=1.0).text("Hardness"));
    ui.add(egui::Slider::new(&mut globe.brush.flow, FLOW).text("Flow"));
    if globe.brush.mode == Mode::Warp {
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
        // The seed is a property of the world, so each brush gets the same
        // noise field.
        let seed = &mut state.doc.project.world.seed;
        ui.add(egui::Slider::new(seed, SEED).text("Seed"));
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
    if globe.brush.mode == Mode::Uplift {
        ui.add(egui::Slider::new(&mut globe.brush.erosion, EROSION).text("Erosion"));
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
    lock_box.on_hover_text("The brush keeps its size on the ground when you zoom.");
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
            ui.label("The level under the start of the stroke");
        }
    }
    let pick = egui::Button::new("Pick from globe").selected(globe.pick_level);
    let pick = ui.add(pick);
    if pick.clicked() {
        globe.pick_level = !globe.pick_level;
    }
    pick.on_hover_text("The next press on the globe sets the level.");
}

pub fn left(ui: &mut egui::Ui, state: &mut AppState) {
    egui::Panel::left("left")
        .default_size(260.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                world_section(ui, state);
                projection_section(ui, state);
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

fn projection_section(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Projection");
    let mut spec = state.doc.frame.projection;
    egui::ComboBox::from_id_salt("projection")
        .selected_text(spec.kind.name())
        .width(180.0)
        .show_ui(ui, |ui| {
            for kind in ProjectionKind::ALL {
                ui.selectable_value(&mut spec.kind, kind, kind.name());
            }
        });
    ui.horizontal(|ui| {
        ui.label("Center longitude");
        ui.add(
            egui::DragValue::new(&mut spec.lon0)
                .speed(1.0)
                .range(-180.0..=180.0),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Center latitude");
        ui.add_enabled(
            spec.kind.is_azimuthal(),
            egui::DragValue::new(&mut spec.lat0)
                .speed(1.0)
                .range(-90.0..=90.0),
        );
    });
    if spec != state.doc.frame.projection {
        state.doc.set_projection(ProjectionSpec { ..spec });
        state.touch();
    }
    if ui.button("Fit world").clicked() {
        state.doc.frame.view = None;
        state.touch();
    }
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
            let mut visible = state.doc.visible(id);
            let text = if selected {
                egui::RichText::new(name).strong()
            } else {
                egui::RichText::new(name)
            };
            let resp = ui.checkbox(&mut visible, text);
            ui.weak(format!("{count}"));
            if resp.changed() {
                state.doc.set_visible(id, visible);
                state.dirty = true;
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
    ui.add_space(6.0);
}

fn map_section(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Map");
    if ui
        .checkbox(&mut state.doc.frame.graticule, "Graticule")
        .changed()
    {
        state.touch();
    }
    if ui.checkbox(&mut state.doc.frame.labels, "Labels").changed() {
        state.touch();
    }
}

pub fn status(ui: &mut egui::Ui, state: &mut AppState) {
    egui::Panel::bottom("status").show(ui, |ui| {
        ui.horizontal(|ui| {
            match state.cursor_lonlat {
                Some([lon, lat]) => ui.label(format!("lon {lon:.2}, lat {lat:.2}")),
                None => ui.label("lon –, lat –"),
            };
            if let Some(value) = state.readout_value() {
                ui.label(value);
            }
            ui.separator();
            if let Some(c) = &state.composed
                && state.workspace == Workspace::Map
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
            let progress = progress.or_else(|| state.globe.export_progress());
            let text = progress.as_deref().unwrap_or(&state.status);
            if !text.is_empty() {
                ui.separator();
                ui.label(text);
            }
        });
    });
}
