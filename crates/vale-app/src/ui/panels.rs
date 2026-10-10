//! Tool bar, left panel, and status bar.

use eframe::egui;
use vale_sphere::{ProjectionKind, ProjectionSpec};

use super::{Action, AppState, ExportFormat, Workspace, pad};
use vale_terrain::{ELEV_MAX, ELEV_MIN, Mode, level_to_meters, meters_to_level};

use crate::globe::brush::{FLOW, STRENGTH_M};
use crate::globe::{Tool, WorldView, stats, stroke_test};

pub fn toolbar(ui: &mut egui::Ui, state: &mut AppState) {
    egui::Panel::top("toolbar").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.strong("Vale");
            ui.separator();
            ui.selectable_value(&mut state.workspace, Workspace::Globe, "Globe");
            ui.selectable_value(&mut state.workspace, Workspace::Map, "Map");
            if state.workspace == Workspace::Globe {
                ui.separator();
                ui.selectable_value(&mut state.globe.tool, Tool::Navigate, "Navigate");
                ui.selectable_value(&mut state.globe.tool, Tool::Brush, "Brush");
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
                ui.toggle_value(&mut state.globe.preview.panel, "Elevation");
                ui.toggle_value(&mut state.globe.debug, "Debug");
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

/// The projection of the flat view.
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

pub(super) fn debug_controls(ui: &mut egui::Ui, state: &mut AppState) {
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
    ui.label(format!(
        "Undo memory: {:.1} MB of {:.0} MB",
        globe.map.undo_memory_bytes() as f64 / MB,
        globe.map.undo_memory_limit() as f64 / MB,
    ));
    ui.separator();

    let test = egui::Button::new("Run stroke test");
    if ui.add_enabled(!globe.busy(), test).clicked() {
        globe.start_stroke_test(stroke_test::SECONDS);
    }
    if let Some(report) = &globe.test_report {
        ui.separator();
        ui.strong("Stroke test");
        ui.label(report);
    }
}

/// The controls of the brush. The panel is open in the brush tool.
pub fn brush(ui: &mut egui::Ui, state: &mut AppState) {
    if state.globe.tool != Tool::Brush {
        state.globe.pick_level = false;
        return;
    }
    egui::Panel::left("brush")
        .default_size(240.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Brush");
                brush_modes(ui, &mut state.globe.brush.mode);
                ui.add_space(6.0);
                brush_size(ui, state);
                brush_settings(ui, state);
            });
        });
}

fn brush_modes(ui: &mut egui::Ui, current: &mut Mode) {
    ui.horizontal_wrapped(|ui| {
        for (mode, name) in [
            (Mode::Raise, "Raise"),
            (Mode::Lower, "Lower"),
            (Mode::Smooth, "Smooth"),
            (Mode::Flatten, "Flatten"),
            (Mode::Carve, "Carve"),
        ] {
            ui.selectable_value(current, mode, name);
        }
    });
}

fn brush_size(ui: &mut egui::Ui, state: &mut AppState) {
    let world_km = state.doc.project.world.radius_km;
    let globe = &mut state.globe;
    let globe_radius = globe.radius();
    let mut km = globe.brush.radius_km(globe_radius, world_km);
    let range = globe.brush.radius_km_range(globe_radius, world_km);
    let slider = egui::Slider::new(&mut km, range)
        .logarithmic(true)
        .custom_formatter(|km, _| three_significant(km))
        .suffix(" km")
        .text("Radius");
    if ui.add(slider).changed() {
        globe.brush.set_radius_km(km, globe_radius, world_km);
    }
    ui.add(egui::Slider::new(&mut globe.brush.hardness, 0.0..=1.0).text("Hardness"));
    ui.add(egui::Slider::new(&mut globe.brush.flow, FLOW).text("Flow"));
    let strength = egui::Slider::new(&mut globe.brush.strength_m, STRENGTH_M)
        .logarithmic(true)
        .fixed_decimals(0)
        .suffix(" m")
        .text("Strength");
    ui.add(strength);
}

/// The lock of the size and the flatten level.
pub(super) fn brush_settings(ui: &mut egui::Ui, state: &mut AppState) {
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
                let mut meters = level_to_meters(level);
                let value = egui::DragValue::new(&mut meters)
                    .speed(10.0)
                    .range(ELEV_MIN..=ELEV_MAX)
                    .fixed_decimals(0)
                    .suffix(" m");
                if ui.add(value).changed() {
                    globe.brush.flatten_level = Some(meters_to_level(meters));
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
    ui.add(
        egui::TextEdit::singleline(&mut state.doc.project.world.name).desired_width(f32::INFINITY),
    );
    ui.horizontal(|ui| {
        ui.label("Radius (km)");
        let mut km = state.doc.project.world.radius_km;
        if ui
            .add(
                egui::DragValue::new(&mut km)
                    .speed(10.0)
                    .range(100.0..=100000.0),
            )
            .changed()
        {
            state.doc.set_radius_km(km);
            state.touch();
        }
    });
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
        let selected = state.selected_layer == Some(id);
        ui.horizontal(|ui| {
            let Some(entry) = state.doc.entry_mut(id) else {
                return;
            };
            let text = if selected {
                egui::RichText::new(name).strong()
            } else {
                egui::RichText::new(name)
            };
            let resp = ui.checkbox(&mut entry.visible, text);
            ui.weak(format!("{count}"));
            if resp.changed() {
                state.dirty = true;
            }
            if resp.clicked() {
                state.selected_layer = Some(id);
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
                state.selected_layer = Some(id);
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

pub(super) fn three_significant(v: f64) -> String {
    if !v.is_finite() || v <= 0.0 {
        return "–".to_string();
    }
    let digits = 2 - v.log10().floor() as i32;
    format!("{:.*}", digits.max(0) as usize, v)
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
                ui.label(format!("1 pt = {} km", three_significant(c.km_per_point)));
                ui.separator();
                ui.label(format!(
                    "labels: {} placed, {} unplaced",
                    c.labels.placed,
                    c.labels.unplaced.len()
                ));
            }
            if !state.status.is_empty() {
                ui.separator();
                ui.label(&state.status);
            }
        });
    });
}
