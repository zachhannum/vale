//! Tool bar, left panel, and status bar.

use eframe::egui;
use vale_sphere::{ProjectionKind, ProjectionSpec};
use vale_store::{GeometryKind, World};

use super::raster_form::RasterForm;
use super::world_form::{self, WorldForm};
use super::{Action, AppState, ExportFormat, LayerRef, SaveMark, Workspace, elevation, pad};
use vale_terrain::Mode;

use crate::globe::brush::{
    EROSION, FLOW, SCALE_KM, SEED, STRENGTH_M, TALUS_DEG, WARP_KM, needs_height,
};
use crate::globe::{Globe, Tool, WorldView, rivers, stats, stroke_test};
use crate::projects;
use crate::world::{self, three_significant};

/// The button of the tool bar that opens the properties of the world.
pub const WORLD_BUTTON: &str = "World…";
/// The button of the tool bar that exports the heightmap.
pub const EXPORT_GEOTIFF: &str = "Export GeoTIFF…";

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
                // With no raster layer to paint, the tool is Navigate.
                let (paintable, tool) = (state.globe.paintable, state.globe.tool);
                let navigate = !paintable || tool == Tool::Navigate;
                if ui.selectable_label(navigate, "Navigate").clicked() {
                    state.globe.tool = Tool::Navigate;
                }
                let brush = egui::Button::selectable(state.globe.brushing(), "Brush");
                if ui.add_enabled(paintable, brush).clicked() {
                    state.globe.tool = Tool::Brush;
                }
                ui.menu_button(RASTERS_MENU, |ui| {
                    if ui.button(ADD_RASTER).clicked() {
                        state.raster_form = Some(RasterForm::open(state));
                        ui.close();
                    }
                    raster_rows(ui, state);
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
                ui.toggle_value(&mut state.globe.preview.greyscale, "Greyscale");
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

/// The button of the tool bar that opens the list of the raster layers.
pub const RASTERS_MENU: &str = "Rasters";
/// The button that opens the form of a new raster layer.
pub const ADD_RASTER: &str = "Add raster…";

/// The rows of the raster layers, with the top layer first. A checkbox
/// shows or hides a layer, and the name selects it.
fn raster_rows(ui: &mut egui::Ui, state: &mut AppState) {
    let rasters = state.doc.project.rasters().iter().rev();
    let rows: Vec<_> = rasters.map(|r| (r.id, r.name.clone(), r.visible)).collect();
    for (id, name, mut visible) in rows {
        let selected = state.selected_raster() == Some(id);
        ui.horizontal(|ui| {
            let eye = ui.checkbox(&mut visible, "");
            eye.widget_info(|| {
                let name = format!("Show {name}");
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, visible, name)
            });
            if eye.changed() {
                state.set_raster_visible(id, visible);
            }
            let pick = ui.selectable_label(selected, &name);
            pick.widget_info(|| {
                let name = format!("Select {name}");
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, name)
            });
            if pick.clicked() {
                state.select(Some(LayerRef::Raster(id)));
            }
        });
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
        if ui.button("Raster…").clicked() {
            state.raster_form = Some(RasterForm::open(state));
            ui.close();
        }
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
    if state.doc.frame.entries.is_empty() && !state.has_raster() {
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
    raster_rows(ui, state);
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
