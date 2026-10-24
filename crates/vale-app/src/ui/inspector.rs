//! The right panel: layer style, feature information, and unplaced labels.

use eframe::egui;
use vale_store::{Field, FieldType, GeometryKind, LayerId, Value};
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
    let id = state.selected_vector()?;
    let layer = state.doc.layer(id)?;
    Some(LayerInfo {
        id,
        name: layer.name.clone(),
        kind: layer.geometry()?,
        count: layer.features.len(),
        fields: layer.field_names(),
    })
}

fn layer_section(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Layer");
    let Some(info) = layer_info(state) else {
        ui.label("Select a layer in the list.");
        ui.add_space(6.0);
        return;
    };
    // The text of the field, from the first key to the end of the edit.
    let key = ui.id().with(("layer-name", info.id.0));
    let mut name = ui
        .data(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| info.name.clone());
    let field = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Layer name"));
    if field.lost_focus() {
        state.doc.rename_layer(info.id, &name);
        ui.data_mut(|d| d.remove::<String>(key));
    } else if field.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, name));
    }
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
            state.select(None);
            if state.selection.is_some_and(|s| s.layer == info.id) {
                state.selection = None;
            }
            state.touch();
        }
    });
    fields_section(ui, state, info.id);
    ui.add_space(4.0);

    let Some(entry) = state.doc.entry_mut(info.id) else {
        return;
    };
    let default = LayerStyle::default_for(info.kind, 0, &info.fields);
    let style = &mut entry.style;
    let mut changed = false;

    if style.is_simple() {
        changed |= simple_style(ui, style, &default, info.kind);
    } else {
        ui.label("This layer has a rule-based style.");
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

/// The largest number of fields that the list of the fields shows open at
/// the start.
const OPEN_FIELDS: usize = 10;

/// The text of a text edit, from the first key to the end of the edit.
/// Returns the text at the end of an edit.
fn edit_text(
    ui: &mut egui::Ui,
    key: egui::Id,
    text: &str,
    hint: &str,
    width: f32,
) -> Option<String> {
    let mut text = ui
        .data(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| text.to_owned());
    let edit = egui::TextEdit::singleline(&mut text)
        .hint_text(hint)
        .desired_width(width);
    let resp = ui.add(edit);
    if resp.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(key));
        return Some(text);
    }
    if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, text));
    }
    None
}

fn type_box(ui: &mut egui::Ui, salt: egui::Id, kind: &mut FieldType) {
    egui::ComboBox::from_id_salt(salt)
        .selected_text(kind.label())
        .width(110.0)
        .show_ui(ui, |ui| {
            for option in FieldType::ALL {
                ui.selectable_value(kind, option, option.label());
            }
        });
}

/// The attribute schema of the layer: the name, the type, and the default
/// value of each field, and a row that adds a field. Each change is one undo
/// step.
fn fields_section(ui: &mut egui::Ui, state: &mut AppState, id: LayerId) {
    let Some(fields) = state.doc.layer(id).map(|l| l.fields.clone()) else {
        return;
    };
    egui::CollapsingHeader::new("Fields")
        .id_salt(("fields", id.0))
        .default_open(fields.len() <= OPEN_FIELDS)
        .show(ui, |ui| {
            egui::Grid::new(("field-grid", id.0))
                .num_columns(4)
                .show(ui, |ui| {
                    for (i, field) in fields.iter().enumerate() {
                        let key = ui.id().with(("field-name", id.0, i));
                        if let Some(name) = edit_text(ui, key, &field.name, "Name", 90.0) {
                            let name = name.trim();
                            state.edit_schema(id, |doc| doc.rename_field(id, &field.name, name));
                        }
                        let mut kind = field.kind;
                        type_box(ui, egui::Id::new(("field-type", id.0, i)), &mut kind);
                        if kind != field.kind {
                            state.edit_schema(id, |doc| {
                                doc.project.set_field_type(id, &field.name, kind)
                            });
                        }
                        let key = ui.id().with(("field-default", id.0, i));
                        let default = field.default.label().unwrap_or_default();
                        if let Some(text) = edit_text(ui, key, &default, "Default", 70.0) {
                            let text = text.trim();
                            let value = if text.is_empty() {
                                Some(Value::Null)
                            } else {
                                field.kind.parse(text)
                            };
                            if let Some(value) = value {
                                state.edit_schema(id, |doc| {
                                    doc.project.set_field_default(id, &field.name, value)
                                });
                            }
                        }
                        if ui.button("Remove").clicked() {
                            state.edit_schema(id, |doc| doc.project.remove_field(id, &field.name));
                        }
                        ui.end_row();
                    }
                });
            // The name and the type of the new field, until it is added.
            let key = ui.id().with(("new-field", id.0));
            let (mut name, mut kind) = ui
                .data(|d| d.get_temp::<(String, FieldType)>(key))
                .unwrap_or_else(|| (String::new(), FieldType::Text));
            let free = state
                .doc
                .layer(id)
                .is_some_and(|l| l.name_free(name.trim(), None));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .hint_text("Field name")
                        .desired_width(90.0),
                );
                type_box(ui, egui::Id::new(("new-field-type", id.0)), &mut kind);
                if ui
                    .add_enabled(free, egui::Button::new("Add field"))
                    .clicked()
                {
                    let field = Field::new(name.trim(), kind);
                    if state.edit_schema(id, |doc| doc.project.add_field(id, field)) {
                        name.clear();
                    }
                }
            });
            ui.data_mut(|d| d.insert_temp(key, (name, kind)));
        });
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

/// The controls of a style with one rule. Returns true if the style changed.
fn simple_style(
    ui: &mut egui::Ui,
    style: &mut LayerStyle,
    default: &LayerStyle,
    kind: GeometryKind,
) -> bool {
    let mut changed = false;
    if kind != GeometryKind::Line {
        ui.horizontal(|ui| {
            let mut fill = style.fill();
            let mut on = fill.is_some();
            if ui.checkbox(&mut on, "Fill").changed() {
                fill = on.then(|| {
                    default
                        .fill()
                        .unwrap_or(vale_style::Color::rgb(180, 180, 180))
                });
                changed = true;
            }
            if let Some(c) = &mut fill {
                changed |= color_button(ui, c);
            }
            if fill != style.fill() {
                style.set_fill(fill);
            }
        });
    }
    let mut stroke = style.stroke();
    ui.horizontal(|ui| {
        let mut on = stroke.is_some();
        if ui.checkbox(&mut on, "Stroke").changed() {
            stroke = on.then(|| {
                default.stroke().unwrap_or(Stroke {
                    color: vale_style::Color::rgb(80, 80, 80),
                    width: 1.0,
                })
            });
            changed = true;
        }
        if let Some(s) = &mut stroke {
            changed |= color_button(ui, &mut s.color);
        }
    });
    if let Some(s) = &mut stroke {
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
    if stroke != style.stroke() {
        style.set_stroke(stroke);
    }
    if kind == GeometryKind::Point {
        ui.horizontal(|ui| {
            ui.label("Point size");
            let mut radius = style.point_radius();
            if ui
                .add(
                    egui::DragValue::new(&mut radius)
                        .speed(0.1)
                        .range(0.0..=30.0),
                )
                .changed()
            {
                style.set_point_radius(radius);
                changed = true;
            }
        });
    }
    changed
}
