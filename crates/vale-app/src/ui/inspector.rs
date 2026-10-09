//! The right panel: layer style, feature information, and unplaced labels.

use eframe::egui;
use vale_store::{GeometryKind, LayerId};
use vale_style::{LayerStyle, Stroke};

use super::AppState;

const MAX_UNPLACED: usize = 200;

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    egui::Panel::right("inspector")
        .default_size(280.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_width(ui.available_width());
                layer_section(ui, state);
                feature_section(ui, state);
                unplaced_section(ui, state);
            });
        });
}

/// What the style controls need to know about the layer.
struct LayerInfo {
    id: LayerId,
    name: String,
    kind: GeometryKind,
    count: usize,
    fields: Vec<String>,
}

fn layer_info(state: &AppState) -> Option<LayerInfo> {
    let id = state.selected_layer?;
    let layer = state.doc.layer(id)?;
    Some(LayerInfo {
        id,
        name: layer.name.clone(),
        kind: layer.kind,
        count: layer.features.len(),
        fields: layer.fields.clone(),
    })
}

fn layer_section(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Layer");
    let Some(info) = layer_info(state) else {
        ui.label("Select a layer in the list.");
        ui.add_space(6.0);
        return;
    };
    ui.strong(&info.name);
    ui.label(format!("{}, {} features", info.kind.name(), info.count));

    ui.horizontal(|ui| {
        if ui.button("Move up").clicked() && state.doc.move_entry(info.id, true) {
            state.touch();
        }
        if ui.button("Move down").clicked() && state.doc.move_entry(info.id, false) {
            state.touch();
        }
        if ui.button("Remove").clicked() {
            state.doc.remove_layer(info.id);
            state.selected_layer = None;
            if state.selection.is_some_and(|s| s.layer == info.id) {
                state.selection = None;
            }
            state.touch();
        }
    });
    ui.add_space(4.0);

    let Some(entry) = state.doc.entry_mut(info.id) else {
        return;
    };
    let default = LayerStyle::default_for(info.kind, 0, &info.fields);
    let style = &mut entry.style;
    let mut changed = false;

    if info.kind != GeometryKind::Line {
        ui.horizontal(|ui| {
            let mut on = style.fill.is_some();
            if ui.checkbox(&mut on, "Fill").changed() {
                style.fill = on.then(|| {
                    default
                        .fill
                        .unwrap_or(vale_style::Color::rgb(180, 180, 180))
                });
                changed = true;
            }
            if let Some(c) = &mut style.fill {
                changed |= color_button(ui, c);
            }
        });
    }
    ui.horizontal(|ui| {
        let mut on = style.stroke.is_some();
        if ui.checkbox(&mut on, "Stroke").changed() {
            style.stroke = on.then(|| {
                default.stroke.unwrap_or(Stroke {
                    color: vale_style::Color::rgb(80, 80, 80),
                    width: 1.0,
                })
            });
            changed = true;
        }
        if let Some(s) = &mut style.stroke {
            changed |= color_button(ui, &mut s.color);
        }
    });
    if let Some(s) = &mut style.stroke {
        ui.horizontal(|ui| {
            ui.label("Stroke width");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut s.width)
                        .speed(0.1)
                        .range(0.1..=20.0),
                )
                .changed();
        });
    }
    if info.kind == GeometryKind::Point {
        ui.horizontal(|ui| {
            ui.label("Point size");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut style.point_radius)
                        .speed(0.1)
                        .range(0.0..=30.0),
                )
                .changed();
        });
    }

    if info.kind == GeometryKind::Polygon {
        ui.add_space(4.0);
        ui.label("Polygon labels are not in the prototype.");
    } else {
        ui.add_space(4.0);
        let label = &mut style.label;
        changed |= ui.checkbox(&mut label.enabled, "Show labels").changed();
        ui.horizontal(|ui| {
            ui.label("Label field");
            egui::ComboBox::from_id_salt("label_field")
                .selected_text(label.field.as_deref().unwrap_or("(none)"))
                .width(130.0)
                .show_ui(ui, |ui| {
                    for f in &info.fields {
                        changed |= ui
                            .selectable_value(&mut label.field, Some(f.clone()), f)
                            .changed();
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Priority field");
            egui::ComboBox::from_id_salt("priority_field")
                .selected_text(label.priority_field.as_deref().unwrap_or("(none)"))
                .width(120.0)
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut label.priority_field, None, "(none)")
                        .changed();
                    for f in &info.fields {
                        changed |= ui
                            .selectable_value(&mut label.priority_field, Some(f.clone()), f)
                            .changed();
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Label size");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut label.size)
                        .speed(0.1)
                        .range(4.0..=72.0),
                )
                .changed();
        });
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut label.italic, "Italic").changed();
            changed |= color_button(ui, &mut label.color);
        });
    }
    if changed {
        state.touch();
    }
    ui.add_space(6.0);
}

/// Edits the RGB part of a color and keeps the alpha.
fn color_button(ui: &mut egui::Ui, c: &mut vale_style::Color) -> bool {
    let mut rgb = [c.r, c.g, c.b];
    if ui.color_edit_button_srgb(&mut rgb).changed() {
        (c.r, c.g, c.b) = (rgb[0], rgb[1], rgb[2]);
        true
    } else {
        false
    }
}

fn feature_section(ui: &mut egui::Ui, state: &mut AppState) {
    ui.separator();
    ui.heading("Feature");
    let Some(sel) = state.selection else {
        ui.label("Click a feature on the map.");
        ui.add_space(6.0);
        return;
    };
    let Some(layer) = state.doc.layer(sel.layer) else {
        ui.label("Click a feature on the map.");
        return;
    };
    ui.strong(&layer.name);
    ui.label(format!("Feature {}", sel.feature));
    if let Some(feature) = layer.features.get(sel.feature) {
        egui::Grid::new("attributes")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                for (name, value) in &feature.attributes {
                    ui.label(name);
                    ui.add(egui::Label::new(value.label().unwrap_or_default()).wrap());
                    ui.end_row();
                }
            });
    }
    if ui.button("Clear selection").clicked() {
        state.selection = None;
        state.touch();
    }
    ui.add_space(6.0);
}

fn unplaced_section(ui: &mut egui::Ui, state: &AppState) {
    ui.separator();
    ui.heading("Unplaced labels");
    let Some(composed) = &state.composed else {
        return;
    };
    let unplaced = &composed.labels.unplaced;
    if unplaced.is_empty() {
        ui.label("All labels fit.");
        return;
    }
    ui.label(format!("{} labels did not fit.", unplaced.len()));
    egui::CollapsingHeader::new("Show list")
        .default_open(false)
        .show(ui, |ui| {
            for u in unplaced.iter().take(MAX_UNPLACED) {
                ui.add(egui::Label::new(format!("{} ({}): {}", u.text, u.layer, u.reason)).wrap());
            }
            if unplaced.len() > MAX_UNPLACED {
                ui.weak(format!("… and {} more", unplaced.len() - MAX_UNPLACED));
            }
        });
}
