//! The rule editor of the desktop layout: the list of the rules, and the
//! filter, the scale range, the stroke, the fill, the point symbol, and the
//! scatter of the selected rule.

use eframe::egui;
use vale_render::Symbols;
use vale_store::{Field, FieldType, GeometryKind, LayerId};
use vale_style::expr::{Filter, Property};
use vale_style::{Color, LayerStyle, Rule, Stroke, StrokeKind};

use super::pad::filter::{self, ADD_CONDITION, REMOVE_CONDITION};
use super::pad::rules::{
    ADD_RULE, ALL_FEATURES, BAD_FILTER, BAD_RANGE, BRUSH, DASH, DASHED, DELETE_RULE, EXPRESSION,
    FILL, FILTER, GAP, LINE, MAX_SCALE, MIN_SCALE, MOVE_DOWN, MOVE_UP, NO_LIMIT, SOLID, STROKE,
    STROKE_WIDTH, TAPER, TAPER_START, parse_scale, scale_text,
};
use super::pad::rules::{
    ADD_SYMBOL, CIRCLE, MAX_SPACING, NO_FIELD, POINT_SIZES, ROTATION, ROTATION_FIELD,
    ROTATION_VARIATION, SCATTER, SCATTER_SIZES, SIZE, SIZE_FIELD, SIZE_VARIATION, SPACING,
    SVG_SYMBOL, SYMBOL, TINT, new_point_symbol, new_scatter, new_tint, numeric_fields, pick_symbol,
    property_field, to_circle, with_field,
};
use super::{Action, Note};
use crate::globe::symbol::paint_symbol;

/// The symbols that the editor lists, the text of the last addition of a
/// symbol, and the actions of the app.
pub struct Library<'a> {
    pub symbols: &'a Symbols,
    pub note: Option<&'a Note>,
    pub actions: &'a mut Vec<Action>,
}

/// The editor of the rules of the style of layer `id`. Returns true if the
/// user changed the style.
pub fn rules(
    ui: &mut egui::Ui,
    id: LayerId,
    style: &mut LayerStyle,
    kind: GeometryKind,
    fields: &[Field],
    library: &mut Library,
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
        let numbers = numeric_fields(fields);
        if kind == GeometryKind::Point {
            changed |= symbol_rows(ui, rule, &numbers, library, salt);
        }
        // An SVG symbol has no stroke and no fill.
        if rule.symbol().is_none() {
            changed |= paint_rows(ui, rule, kind, salt);
        }
        if kind == GeometryKind::Polygon {
            changed |= scatter_rows(ui, rule, library, salt);
        }
    }
    changed
}

/// The choice of a circle or an SVG symbol, and the symbol, the size, the
/// rotation, their fields, and the tint of an SVG symbol.
fn symbol_rows(
    ui: &mut egui::Ui,
    rule: &mut Rule,
    fields: &[String],
    library: &mut Library,
    salt: egui::Id,
) -> bool {
    let mut changed = false;
    let mut svg = rule.symbol().is_some();
    ui.horizontal(|ui| {
        ui.label(SYMBOL);
        egui::ComboBox::from_id_salt(salt.with("symbol-kind"))
            .selected_text(if svg { SVG_SYMBOL } else { CIRCLE })
            .show_ui(ui, |ui| {
                changed |= ui.selectable_value(&mut svg, false, CIRCLE).changed();
                changed |= ui.selectable_value(&mut svg, true, SVG_SYMBOL).changed();
            });
    });
    if changed {
        if svg {
            rule.set_symbol(Some(new_point_symbol()));
        } else {
            to_circle(rule);
        }
    }
    let Some(before) = rule.symbol() else {
        return changed;
    };
    let mut s = before.clone();
    let tint = s.tint.as_ref().map(|t| t.fallback);
    if let Some(name) = library_row(ui, library, &s.symbol, tint, salt) {
        pick_symbol(&mut s.symbol, &mut s.tint, &name);
    }
    number_row(ui, SIZE, &mut s.size.fallback, POINT_SIZES, " pt");
    number_row(
        ui,
        ROTATION,
        &mut s.rotation.fallback,
        (0.0, 359.0),
        "\u{b0}",
    );
    field_row(ui, SIZE_FIELD, &mut s.size, fields, salt);
    field_row(ui, ROTATION_FIELD, &mut s.rotation, fields, salt);
    tint_row(ui, &mut s.tint, &s.symbol);
    if Some(&s) != rule.symbol() {
        rule.set_symbol(Some(s));
        changed = true;
    }
    changed
}

/// The switch of the scatter, and its symbol, size, spacing, variations, and
/// tint.
fn scatter_rows(ui: &mut egui::Ui, rule: &mut Rule, library: &mut Library, salt: egui::Id) -> bool {
    let mut on = rule.scatter().is_some();
    if ui.checkbox(&mut on, SCATTER).changed() {
        rule.set_scatter(on.then(new_scatter));
        return true;
    }
    let Some(before) = rule.scatter() else {
        return false;
    };
    let mut s = before.clone();
    let tint = s.tint.as_ref().map(|t| t.fallback);
    if let Some(name) = library_row(ui, library, &s.symbol, tint, salt) {
        pick_symbol(&mut s.symbol, &mut s.tint, &name);
    }
    number_row(ui, SIZE, &mut s.size.fallback, SCATTER_SIZES, " pt");
    let size = s.size.fallback;
    s.spacing.fallback = s.spacing.fallback.max(size);
    number_row(
        ui,
        SPACING,
        &mut s.spacing.fallback,
        (size, MAX_SPACING.max(size)),
        " pt",
    );
    let mut percent = s.size_jitter * 100.0;
    if number_row(ui, SIZE_VARIATION, &mut percent, (0.0, 50.0), " %") {
        s.size_jitter = percent / 100.0;
    }
    number_row(
        ui,
        ROTATION_VARIATION,
        &mut s.rotation_jitter,
        (0.0, 180.0),
        "\u{b0}",
    );
    tint_row(ui, &mut s.tint, &s.symbol);
    if Some(&s) != rule.scatter() {
        rule.set_scatter(Some(s));
        return true;
    }
    false
}

/// The side of the preview of a symbol.
const PREVIEW: f32 = 20.0;

/// A preview of `current`, a list of the symbols of the library, and the
/// button that adds a symbol. Returns the symbol that the user picked.
fn library_row(
    ui: &mut egui::Ui,
    library: &mut Library,
    current: &str,
    tint: Option<Color>,
    salt: egui::Id,
) -> Option<String> {
    let mut picked = None;
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(PREVIEW, PREVIEW), egui::Sense::hover());
        if let Some(symbol) = library.symbols.get(current) {
            ui.painter()
                .rect_filled(rect, 3.0, super::pad::rules::PAPER);
            paint_symbol(
                ui.painter(),
                symbol,
                rect.center(),
                PREVIEW - 4.0,
                0.0,
                tint,
            );
        }
        egui::ComboBox::from_id_salt(salt.with("symbol"))
            .selected_text(current)
            .show_ui(ui, |ui| {
                for name in library.symbols.names() {
                    if ui.selectable_label(name == current, name).clicked() && name != current {
                        picked = Some(name.to_owned());
                    }
                }
            });
        if ui.button(ADD_SYMBOL).clicked() {
            library.actions.push(Action::AddSymbolDialog);
        }
    });
    if let Some(note) = library.note {
        let color = if note.warning {
            ui.visuals().warn_fg_color
        } else {
            ui.visuals().weak_text_color()
        };
        ui.label(egui::RichText::new(&note.text).color(color));
    }
    picked
}

/// A label and a drag value from `lo` to `hi`. Returns true if the user
/// changed the value.
fn number_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    (lo, hi): (f64, f64),
    suffix: &str,
) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        let drag = egui::DragValue::new(value)
            .speed(0.5)
            .range(lo..=hi)
            .suffix(suffix);
        ui.add(drag).changed()
    })
    .inner
}

/// A list of "None" and the number fields, which sets the field of a
/// property.
fn field_row(
    ui: &mut egui::Ui,
    label: &str,
    property: &mut Property<f64>,
    fields: &[String],
    salt: egui::Id,
) {
    let current = property_field(property, fields);
    let mut field = None;
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(salt.with(label))
            .selected_text(current.as_deref().unwrap_or(NO_FIELD))
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), NO_FIELD).clicked() {
                    field = Some(None);
                }
                for f in fields {
                    let on = current.as_deref() == Some(f.as_str());
                    if ui.selectable_label(on, f).clicked() {
                        field = Some(Some(f.as_str()));
                    }
                }
            });
    });
    if let Some(field) = field {
        *property = with_field(property, field);
    }
}

/// The switch of a tint and its color button.
fn tint_row(ui: &mut egui::Ui, tint: &mut Option<Property<Color>>, symbol: &str) {
    ui.horizontal(|ui| {
        let mut on = tint.is_some();
        if ui.checkbox(&mut on, TINT).changed() {
            *tint = on.then(|| Property::constant(new_tint(symbol)));
        }
        if let Some(t) = tint {
            color_button(ui, &mut t.fallback);
        }
    });
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
fn paint_rows(ui: &mut egui::Ui, rule: &mut Rule, kind: GeometryKind, salt: egui::Id) -> bool {
    let default = LayerStyle::default_for(kind, 0, &[]);
    let mut changed = false;
    let mut stroke = rule.stroke();
    ui.horizontal(|ui| {
        let mut on = stroke.is_some();
        if ui.checkbox(&mut on, STROKE).changed() {
            stroke = on.then(|| {
                default
                    .stroke()
                    .unwrap_or(Stroke::solid(Color::rgb(90, 90, 90), 1.0))
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
        // A marker ignores the line type.
        if kind != GeometryKind::Point {
            changed |= line_rows(ui, s, kind, salt);
        }
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

/// The line type of a stroke, and the lengths of its dashes or its start width.
fn line_rows(ui: &mut egui::Ui, s: &mut Stroke, kind: GeometryKind, salt: egui::Id) -> bool {
    let mut changed = false;
    let mut kinds = vec![
        (SOLID, StrokeKind::Solid),
        (DASHED, StrokeKind::dashed()),
        (BRUSH, StrokeKind::Brush),
    ];
    if kind == GeometryKind::Line {
        kinds.push((TAPER, StrokeKind::taper()));
    }
    let name = |k: StrokeKind| match k {
        StrokeKind::Solid => SOLID,
        StrokeKind::Dashed { .. } => DASHED,
        StrokeKind::Brush => BRUSH,
        StrokeKind::Taper { .. } => TAPER,
    };
    ui.horizontal(|ui| {
        ui.label(LINE);
        egui::ComboBox::from_id_salt(salt.with("line"))
            .selected_text(name(s.kind))
            .show_ui(ui, |ui| {
                for (label, k) in kinds {
                    let on = name(s.kind) == label;
                    if ui.selectable_label(on, label).clicked() && !on {
                        s.kind = k;
                        changed = true;
                    }
                }
            });
    });
    let length = |ui: &mut egui::Ui, label: &str, value: &mut f64| {
        ui.horizontal(|ui| {
            ui.label(label);
            let drag = egui::DragValue::new(value)
                .speed(0.05)
                .range(0.5..=20.0)
                .suffix(" pt");
            ui.add(drag).changed()
        })
        .inner
    };
    match &mut s.kind {
        StrokeKind::Dashed { dash, gap } => {
            changed |= length(ui, DASH, dash);
            changed |= length(ui, GAP, gap);
        }
        StrokeKind::Taper { start } => {
            ui.horizontal(|ui| {
                ui.label(TAPER_START);
                let mut percent = *start * 100.0;
                let drag = egui::DragValue::new(&mut percent)
                    .speed(1.0)
                    .range(0.0..=100.0)
                    .suffix(" %");
                if ui.add(drag).changed() {
                    *start = percent / 100.0;
                    changed = true;
                }
            });
        }
        StrokeKind::Solid | StrokeKind::Brush => {}
    }
    changed
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
