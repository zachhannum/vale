//! Tool bar, left panel, and status bar.

use eframe::egui;
use vale_sphere::{ProjectionKind, ProjectionSpec};

use super::{Action, AppState, ExportFormat, Workspace};

pub fn toolbar(ui: &mut egui::Ui, state: &mut AppState) {
    egui::Panel::top("toolbar").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.strong("Vale");
            ui.separator();
            ui.selectable_value(&mut state.workspace, Workspace::Globe, "Globe");
            ui.selectable_value(&mut state.workspace, Workspace::Map, "Map");
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

fn three_significant(v: f64) -> String {
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
