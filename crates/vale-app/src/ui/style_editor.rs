//! The rule editor of the desktop layout: the list of the rules, and the
//! filter, the scale range, the stroke, and the fill of the selected rule.

use eframe::egui;
use vale_store::{GeometryKind, LayerId};
use vale_style::expr::Filter;
use vale_style::{Color, LayerStyle, Rule, Stroke};

use super::pad::rules::{
    ADD_RULE, ALL_FEATURES, BAD_FILTER, BAD_RANGE, DELETE_RULE, FILL, FILTER, MAX_SCALE, MIN_SCALE,
    MOVE_DOWN, MOVE_UP, NO_LIMIT, STROKE, STROKE_WIDTH, field_name, parse_scale, scale_text,
};

/// The editor of the rules of the style of layer `id`. Returns true if the
/// user changed the style.
pub fn rules(
    ui: &mut egui::Ui,
    id: LayerId,
    style: &mut LayerStyle,
    kind: GeometryKind,
    fields: &[String],
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
        changed |= filter_rows(ui, rule, fields);
        changed |= scale_rows(ui, rule, salt);
        changed |= paint_rows(ui, rule, kind);
    }
    changed
}

/// The filter field, its error, and a menu that adds a field.
fn filter_rows(ui: &mut egui::Ui, rule: &mut Rule, fields: &[String]) -> bool {
    let mut text = rule
        .filter
        .source()
        .map_or(String::new(), |s| s.text().to_owned());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(FILTER);
        let edit = egui::TextEdit::singleline(&mut text)
            .hint_text(ALL_FEATURES)
            .desired_width(150.0);
        let field = ui.add(edit);
        field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, FILTER));
        changed |= field.changed();
        ui.menu_button("+", |ui| {
            for name in fields {
                if ui.button(name).clicked() {
                    let start = text.trim_end();
                    let name = field_name(name);
                    text = if start.is_empty() {
                        name
                    } else {
                        format!("{start} {name}")
                    };
                    changed = true;
                }
            }
        });
    });
    if changed {
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
