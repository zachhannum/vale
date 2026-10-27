//! The rule editor of the desktop layout: the list of the rules, and the
//! filter, the scale range, the stroke, and the fill of the selected rule.

use eframe::egui;
use vale_store::{Field, FieldType, GeometryKind, LayerId};
use vale_style::expr::Filter;
use vale_style::{Color, LayerStyle, Rule, Stroke};

use super::pad::filter::{self, ADD_CONDITION, REMOVE_CONDITION};
use super::pad::rules::{
    ADD_RULE, ALL_FEATURES, BAD_FILTER, BAD_RANGE, DELETE_RULE, EXPRESSION, FILL, FILTER,
    MAX_SCALE, MIN_SCALE, MOVE_DOWN, MOVE_UP, NO_LIMIT, STROKE, STROKE_WIDTH, parse_scale,
    scale_text,
};

/// The editor of the rules of the style of layer `id`. Returns true if the
/// user changed the style.
pub fn rules(
    ui: &mut egui::Ui,
    id: LayerId,
    style: &mut LayerStyle,
    kind: GeometryKind,
    fields: &[Field],
) -> bool {
    let key = egui::Id::new(("style-rule", id.0));
    let count = style.style.rules.len();
    let mut at: usize = ui.data(|d| d.get_temp(key)).unwrap_or(0);
    at = at.min(count.saturating_sub(1));
    let mut changed = false;

    for (i, rule) in style.style.rules.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui.selectable_label(i == at, &rule.name).clicked() {
                at = i;
            }
            let filter = rule.filter.source().map_or(ALL_FEATURES, |s| s.text());
            ui.weak(filter);
        });
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button(ADD_RULE).clicked() {
            at = style.add_rule(kind);
            changed = true;
        }
        let some = at < style.style.rules.len();
        if ui
            .add_enabled(some && at > 0, egui::Button::new(MOVE_UP))
            .clicked()
        {
            at = style.style.move_rule(at, true);
            changed = true;
        }
        let down = at + 1 < style.style.rules.len();
        if ui.add_enabled(down, egui::Button::new(MOVE_DOWN)).clicked() {
            at = style.style.move_rule(at, false);
            changed = true;
        }
        if ui
            .add_enabled(some, egui::Button::new(DELETE_RULE))
            .clicked()
        {
            style.style.rules.remove(at);
            at = at.saturating_sub(1);
            changed = true;
        }
    });
    ui.data_mut(|d| d.insert_temp(key, at));
    if let Some(rule) = style.style.rules.get_mut(at) {
        ui.separator();
        let salt = key.with(at);
        changed |= filter_rows(ui, rule, fields, salt);
        changed |= scale_rows(ui, rule, salt);
        changed |= paint_rows(ui, rule, kind);
    }
    changed
}

/// The conditions of the filter, each with a button that removes it, the
/// controls of a new condition, and the text of the whole filter.
fn filter_rows(ui: &mut egui::Ui, rule: &mut Rule, fields: &[Field], salt: egui::Id) -> bool {
    let current = rule
        .filter
        .source()
        .map_or(String::new(), |s| s.text().to_owned());
    let mut parts = filter::conditions(&current);
    let mut out = None;
    ui.label(FILTER);
    if parts.is_empty() {
        ui.weak(filter::NO_CONDITION);
    }
    let mut remove = None;
    for (n, part) in parts.iter().enumerate() {
        ui.horizontal(|ui| {
            let name = format!("{REMOVE_CONDITION} {}", n + 1);
            let button = ui.small_button("×");
            button.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
            if button.clicked() {
                remove = Some(n);
            }
            ui.monospace(part);
        });
    }
    if let Some(n) = remove {
        parts.remove(n);
        out = Some(filter::join(&parts));
    }

    // The new condition: a field, an operator, and a value.
    let key = salt.with("condition");
    let (mut field, mut op, mut value): (usize, usize, String) =
        ui.data(|d| d.get_temp(key)).unwrap_or_default();
    field = field.min(fields.len().saturating_sub(1));
    if let Some(f) = fields.get(field) {
        let number = matches!(f.kind, FieldType::Integer | FieldType::Real);
        let count = if number { filter::OPERATORS.len() } else { 2 };
        op = op.min(count - 1);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(key.with("field"))
                .selected_text(&f.name)
                .width(110.0)
                .show_ui(ui, |ui| {
                    for (i, f) in fields.iter().enumerate() {
                        ui.selectable_value(&mut field, i, &f.name);
                    }
                });
            egui::ComboBox::from_id_salt(key.with("op"))
                .selected_text(filter::OPERATORS[op].0)
                .width(40.0)
                .show_ui(ui, |ui| {
                    for (i, (label, _)) in filter::OPERATORS[..count].iter().enumerate() {
                        ui.selectable_value(&mut op, i, *label);
                    }
                });
        });
        ui.horizontal(|ui| {
            match f.kind {
                FieldType::Bool => {
                    for choice in [filter::TRUE, filter::FALSE] {
                        if ui.selectable_label(value == choice, choice).clicked() {
                            value = choice.to_owned();
                        }
                    }
                }
                _ => {
                    let edit = egui::TextEdit::singleline(&mut value)
                        .hint_text(filter::VALUE)
                        .desired_width(110.0);
                    let resp = ui.add(edit);
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, filter::VALUE)
                    });
                }
            }
            let made = filter::condition(&f.name, f.kind, op, &value);
            if ui
                .add_enabled(made.is_some(), egui::Button::new(ADD_CONDITION))
                .clicked()
                && let Some(made) = made
            {
                parts.push(made);
                out = Some(filter::join(&parts));
                value.clear();
            }
        });
    }
    ui.data_mut(|d| d.insert_temp(key, (field, op, value)));

    // The whole filter as text.
    let mut text = out.clone().unwrap_or(current);
    ui.horizontal(|ui| {
        ui.label(EXPRESSION);
        let edit = egui::TextEdit::singleline(&mut text)
            .hint_text(ALL_FEATURES)
            .desired_width(150.0);
        let field = ui.add(edit);
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, EXPRESSION)
        });
        if field.changed() {
            out = Some(text.clone());
        }
    });
    let changed = out.is_some();
    if let Some(text) = out {
        rule.filter = Filter::new(&text);
    }
    if rule.filter.error().is_some() {
        ui.colored_label(ui.visuals().warn_fg_color, BAD_FILTER);
    }
    changed
}

/// The fields of the scale range. A field keeps its text while the user
/// types.
fn scale_rows(ui: &mut egui::Ui, rule: &mut Rule, salt: egui::Id) -> bool {
    let mut changed = false;
    for (name, value) in [
        (MIN_SCALE, &mut rule.min_scale),
        (MAX_SCALE, &mut rule.max_scale),
    ] {
        let key = salt.with(name);
        let mut text = ui
            .data(|d| d.get_temp::<String>(key))
            .unwrap_or_else(|| value.map_or(String::new(), scale_text));
        ui.horizontal(|ui| {
            ui.label(name);
            let edit = egui::TextEdit::singleline(&mut text)
                .hint_text(NO_LIMIT)
                .desired_width(100.0);
            let field = ui.add(edit);
            field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name));
            if field.changed() {
                let parsed = match text.trim() {
                    "" => Some(None),
                    t => parse_scale(t).map(Some),
                };
                if let Some(parsed) = parsed
                    && parsed != *value
                {
                    *value = parsed;
                    changed = true;
                }
            }
            if field.has_focus() {
                ui.data_mut(|d| d.insert_temp(key, text.clone()));
            } else {
                ui.data_mut(|d| d.remove::<String>(key));
            }
        });
    }
    if let (Some(min), Some(max)) = (rule.min_scale, rule.max_scale)
        && min >= max
    {
        ui.colored_label(ui.visuals().warn_fg_color, BAD_RANGE);
    }
    changed
}

/// The stroke and its width, and the fill. A line layer has no fill.
fn paint_rows(ui: &mut egui::Ui, rule: &mut Rule, kind: GeometryKind) -> bool {
    let default = LayerStyle::default_for(kind, 0, &[]);
    let mut changed = false;
    let mut stroke = rule.stroke();
    ui.horizontal(|ui| {
        let mut on = stroke.is_some();
        if ui.checkbox(&mut on, STROKE).changed() {
            stroke = on.then(|| {
                default.stroke().unwrap_or(Stroke {
                    color: Color::rgb(90, 90, 90),
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
            ui.label(STROKE_WIDTH);
            let drag = egui::DragValue::new(&mut s.width)
                .speed(0.05)
                .range(0.1..=20.0)
                .suffix(" pt");
            changed |= ui.add(drag).changed();
        });
    }
    if changed {
        rule.set_stroke(stroke);
    }
    if kind == GeometryKind::Line {
        return changed;
    }
    let mut fill = rule.fill();
    let mut fill_changed = false;
    ui.horizontal(|ui| {
        let mut on = fill.is_some();
        if ui.checkbox(&mut on, FILL).changed() {
            fill = on.then(|| default.fill().unwrap_or(Color::rgb(200, 200, 200)));
            fill_changed = true;
        }
        if let Some(c) = &mut fill {
            fill_changed |= color_button(ui, c);
        }
    });
    if fill_changed {
        rule.set_fill(fill);
    }
    changed || fill_changed
}

/// A color button that keeps the alpha of `c`.
pub fn color_button(ui: &mut egui::Ui, c: &mut Color) -> bool {
    let mut rgb = [c.r, c.g, c.b];
    if ui.color_edit_button_srgb(&mut rgb).changed() {
        (c.r, c.g, c.b) = (rgb[0], rgb[1], rgb[2]);
        true
    } else {
        false
    }
}
