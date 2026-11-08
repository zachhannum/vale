//! The rule editor of the desktop layout: the list of the rules, and the
//! filter, the scale range, the stroke, the fill, the point symbol, and the
//! scatter of the selected rule.

use eframe::egui;
use vale_render::Symbols;
use vale_store::{Field, FillKind, GeometryKind, LayerId};
use vale_style::expr::{Filter, Property, field_ref};
use vale_style::{Color, Hatch, LayerStyle, MarkerShape, Mask, Rule, Stroke, StrokeKind};

use super::expr_field;
use super::expr_help::{self, ExprHelp};
use super::pad::classify::{
    CLASS_COUNT, CLASSIFY, COLOR_RAMP, FIELD, METHOD, REPLACE_RULES, REVERSE, Request,
};
use super::pad::filter;
use super::pad::furniture::{ANGLE, LINE_COLOR, LINE_WIDTH};
use super::pad::raster::IMPORT_RAMP;
use super::pad::rules::{
    ADD_RULE, ALL_FEATURES, BAD_RANGE, BRUSH, DASH, DASHED, DASHES, DELETE_RULE, EXPRESSION, FILL,
    FILTER, GAP, LINE, MAX_SCALE, MIN_SCALE, MOVE_DOWN, MOVE_UP, NO_LIMIT, SOLID, STROKE,
    STROKE_WIDTH, TAPER, TAPER_START, parse_scale, scale_text,
};
use super::pad::rules::{
    ADD_SYMBOL, BAD_MASK_RANGE, CIRCLE_SIZES, COLOR, MARKER, MARKER_SHAPE, MASK, MASK_MAX,
    MASK_MIN, MAX_SPACING, NO_FIELD, NO_MASK, NO_SYMBOL_LIST, POINT_SIZES, ROTATION,
    ROTATION_FIELD, ROTATION_VARIATION, SCATTER, SCATTER_SIZES, SHAPE, SIZE, SIZE_FIELD,
    SIZE_VARIATION, SPACING, SVG_SYMBOL, SYMBOL, TINT, marker_radius, new_tint, numeric_fields,
    pick_symbol, property_field, to_circle, with_field,
};
use super::{Action, Note, number};
use crate::classify::{self, Method, Step};
use crate::globe::symbol::paint_symbol;
use crate::ramps::Ramp;

/// The symbols that the editor lists, the text of the last addition of a
/// symbol, the actions of the app, and the help of the expression fields.
pub struct Library<'a> {
    pub symbols: &'a Symbols,
    pub note: Option<&'a Note>,
    pub actions: &'a mut Vec<Action>,
    pub help: &'a ExprHelp,
    /// The match counts of the filters of the rules, by filter text.
    pub counts: &'a std::collections::HashMap<String, Option<(usize, usize)>>,
}

/// The match counts of `rules` of layer `id`, for the `counts` of a
/// `Library`.
pub fn filter_counts(
    state: &super::AppState,
    id: LayerId,
    rules: &[Rule],
) -> std::collections::HashMap<String, Option<(usize, usize)>> {
    (rules.iter())
        .filter_map(|r| r.filter.source().map(|s| s.text().to_owned()))
        .map(|t| {
            let count = expr_help::matches(state, id, &t);
            (t, count)
        })
        .collect()
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
        changed |= filter_rows(ui, rule, library, salt);
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

/// The mask of a style: a choice of "None" or a layer of `choices`, and the range of a raster mask. A blank limit has no limit. Returns true if the user changed the mask.
pub fn mask_rows(
    ui: &mut egui::Ui,
    id: LayerId,
    mask: &mut Option<Mask>,
    choices: &[(String, Mask)],
) -> bool {
    let before = *mask;
    let name = mask
        .and_then(|m| choices.iter().find(|(_, c)| c.layer() == m.layer()))
        .map_or(NO_MASK, |(n, _)| n.as_str());
    ui.horizontal(|ui| {
        ui.label(MASK);
        egui::ComboBox::from_id_salt(("style-mask", id.0))
            .selected_text(name)
            .show_ui(ui, |ui| {
                if ui.selectable_label(mask.is_none(), NO_MASK).clicked() {
                    *mask = None;
                }
                for (n, choice) in choices {
                    let on = mask.is_some_and(|m| m.layer() == choice.layer());
                    if ui.selectable_label(on, n).clicked() && !on {
                        *mask = Some(super::pad::rules::keep_offset(*choice, *mask));
                    }
                }
            });
    });
    if let Some(Mask::Raster {
        min, max, offset, ..
    }) = mask
    {
        for (label, value) in [(MASK_MIN, min), (MASK_MAX, max)] {
            ui.horizontal(|ui| {
                ui.label(label);
                let key = egui::Id::new(("style-mask", id.0, label));
                let shown = value.map_or(String::new(), |v| v.to_string());
                let mut text: String = ui.data(|d| d.get_temp(key)).unwrap_or(shown);
                let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0));
                if resp.changed() {
                    if text.trim().is_empty() {
                        *value = None;
                    } else if let Some(v) = super::pad::raster::parse_value(&text) {
                        *value = Some(v);
                    }
                }
                if resp.has_focus() {
                    ui.data_mut(|d| d.insert_temp(key, text));
                } else {
                    ui.data_mut(|d| d.remove::<String>(key));
                }
            });
        }
        let (lo, hi) = super::pad::rules::MASK_OFFSETS;
        ui.horizontal(|ui| {
            ui.label(super::pad::rules::MASK_OFFSET);
            ui.add(
                egui::DragValue::new(offset)
                    .speed(0.05)
                    .range(lo..=hi)
                    .suffix(" pt"),
            );
        });
        if let Some(Mask::Raster {
            min: Some(min),
            max: Some(max),
            ..
        }) = mask
            && min >= max
        {
            ui.weak(BAD_MASK_RANGE);
        }
    }
    *mask != before
}

/// The choice of a shape or an SVG symbol, the shape and the size of a marker, and the
/// symbol, the size, the rotation, their fields, and the tint of an SVG
/// symbol. "SVG symbol" picks the first symbol of the library. With no
/// symbols, the rule keeps its shape until the user adds one.
fn symbol_rows(
    ui: &mut egui::Ui,
    rule: &mut Rule,
    fields: &[String],
    library: &mut Library,
    salt: egui::Id,
) -> bool {
    let mut changed = false;
    let key = salt.with("want-symbol");
    let mut want = ui.data(|d| d.get_temp(key)).unwrap_or(false) && rule.symbol().is_none();
    let mut svg = rule.symbol().is_some() || want;
    let mut chose = false;
    ui.horizontal(|ui| {
        ui.label(MARKER);
        egui::ComboBox::from_id_salt(salt.with("symbol-kind"))
            .selected_text(if svg { SVG_SYMBOL } else { SHAPE })
            .show_ui(ui, |ui| {
                chose |= ui.selectable_value(&mut svg, false, SHAPE).changed();
                chose |= ui.selectable_value(&mut svg, true, SVG_SYMBOL).changed();
            });
    });
    if chose && !svg {
        to_circle(rule);
        want = false;
        changed = true;
    }
    want |= chose && svg;
    if want && let Some(first) = library.symbols.names().first() {
        pick_symbol(rule, GeometryKind::Point, first);
        want = false;
        changed = true;
    }
    ui.data_mut(|d| d.insert_temp(key, want));
    let Some(before) = rule.symbol() else {
        if want {
            library_row(ui, library, None, None, salt);
        }
        if let Some(current) = rule.marker_shape() {
            let mut shape = current;
            ui.horizontal(|ui| {
                ui.label(MARKER_SHAPE);
                egui::ComboBox::from_id_salt(salt.with("marker-shape"))
                    .selected_text(shape.label())
                    .show_ui(ui, |ui| {
                        for s in MarkerShape::ALL {
                            ui.selectable_value(&mut shape, s, s.label());
                        }
                    });
            });
            if shape != current {
                rule.set_marker_shape(shape);
                changed = true;
            }
        }
        if let Some(radius) = marker_radius(rule) {
            let mut size = 2.0 * radius.fallback;
            let (lo, hi) = CIRCLE_SIZES;
            let stepper = number::stepper(SIZE, &mut size).range(lo..=hi);
            if stepper.suffix(" pt").show(ui).changed() {
                radius.fallback = size / 2.0;
                changed = true;
            }
        }
        return changed;
    };
    let mut s = before.clone();
    let tint = s.tint.as_ref().map(|t| t.fallback);
    if let Some(name) = library_row(ui, library, Some(&s.symbol), tint, salt) {
        s.symbol = name;
    }
    let (lo, hi) = POINT_SIZES;
    number::stepper(SIZE, &mut s.size.fallback)
        .range(lo..=hi)
        .suffix(" pt")
        .show(ui);
    number::stepper(ROTATION, &mut s.rotation.fallback)
        .range(0.0..=359.0)
        .step(1.0)
        .suffix("\u{b0}")
        .show(ui);
    field_row(ui, SIZE_FIELD, &mut s.size, fields, salt);
    field_row(ui, ROTATION_FIELD, &mut s.rotation, fields, salt);
    tint_row(ui, &mut s.tint);
    if Some(&s) != rule.symbol() {
        rule.set_symbol(Some(s));
        changed = true;
    }
    changed
}

/// The switch of the scatter, and its symbol, size, spacing, variations, and
/// tint. The switch picks the first symbol of the library. With no symbols,
/// the scatter starts when the user adds one.
fn scatter_rows(ui: &mut egui::Ui, rule: &mut Rule, library: &mut Library, salt: egui::Id) -> bool {
    let key = salt.with("want-scatter");
    let mut want = ui.data(|d| d.get_temp(key)).unwrap_or(false) && rule.scatter().is_none();
    let mut on = rule.scatter().is_some() || want;
    let mut changed = false;
    if ui.checkbox(&mut on, SCATTER).changed() {
        want = on;
        if !on {
            rule.set_scatter(None);
            changed = true;
        }
    }
    if want && let Some(first) = library.symbols.names().first() {
        pick_symbol(rule, GeometryKind::Polygon, first);
        want = false;
        changed = true;
    }
    ui.data_mut(|d| d.insert_temp(key, want));
    let Some(before) = rule.scatter() else {
        if want {
            library_row(ui, library, None, None, salt);
        }
        return changed;
    };
    let mut s = before.clone();
    let tint = s.tint.as_ref().map(|t| t.fallback);
    if let Some(name) = library_row(ui, library, Some(&s.symbol), tint, salt) {
        s.symbol = name;
    }
    let (lo, hi) = SCATTER_SIZES;
    number::stepper(SIZE, &mut s.size.fallback)
        .range(lo..=hi)
        .suffix(" pt")
        .show(ui);
    let size = s.size.fallback;
    s.spacing.fallback = s.spacing.fallback.max(size);
    number::stepper(SPACING, &mut s.spacing.fallback)
        .range(size..=MAX_SPACING.max(size))
        .step(1.0)
        .suffix(" pt")
        .show(ui);
    number::slider(SIZE_VARIATION, &mut s.size_jitter, 0.0..=0.5)
        .percent()
        .step(1.0)
        .show(ui);
    number::slider(ROTATION_VARIATION, &mut s.rotation_jitter, 0.0..=180.0)
        .step(1.0)
        .suffix("\u{b0}")
        .show(ui);
    tint_row(ui, &mut s.tint);
    if Some(&s) != rule.scatter() {
        rule.set_scatter(Some(s));
        return true;
    }
    changed
}

/// The side of the preview of a symbol.
const PREVIEW: f32 = 20.0;

/// A preview of `current`, a list of the symbols of the library, and the
/// button that adds a symbol. With no symbols, the list is off. Returns the
/// symbol that the user picked.
fn library_row(
    ui: &mut egui::Ui,
    library: &mut Library,
    current: Option<&str>,
    tint: Option<Color>,
    salt: egui::Id,
) -> Option<String> {
    let mut picked = None;
    let names = library.symbols.names();
    ui.horizontal(|ui| {
        ui.label(SYMBOL);
        if let Some(symbol) = current.and_then(|c| library.symbols.get(c)) {
            let size = egui::vec2(PREVIEW, PREVIEW);
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
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
        let text = if names.is_empty() {
            NO_SYMBOL_LIST
        } else {
            current.unwrap_or_default()
        };
        ui.add_enabled_ui(!names.is_empty(), |ui| {
            egui::ComboBox::from_id_salt(salt.with("symbol"))
                .selected_text(text)
                .show_ui(ui, |ui| {
                    for name in &names {
                        let on = Some(*name) == current;
                        if ui.selectable_label(on, *name).clicked() && !on {
                            picked = Some((*name).to_owned());
                        }
                    }
                });
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
fn tint_row(ui: &mut egui::Ui, tint: &mut Option<Property<Color>>) {
    ui.horizontal(|ui| {
        let mut on = tint.is_some();
        if ui.checkbox(&mut on, TINT).changed() {
            *tint = on.then(|| Property::constant(new_tint()));
        }
        if let Some(t) = tint {
            color_button(ui, &mut t.fallback);
        }
    });
}

/// The filter of the rule as a line of tokens. A click opens the
/// conditions in a popover.
fn filter_rows(ui: &mut egui::Ui, rule: &mut Rule, library: &Library, salt: egui::Id) -> bool {
    let mut text = rule
        .filter
        .source()
        .map_or(String::new(), |s| s.text().to_owned());
    let count = library.counts.get(&text).copied().flatten();
    let changed = ui
        .push_id(salt.with("filter"), |ui| {
            let none = filter::NO_CONDITION;
            expr_field::filter(ui, FILTER, &mut text, library.help, count, none)
        })
        .inner;
    if changed {
        rule.filter = Filter::new(&text);
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
        changed |= number::stepper(STROKE_WIDTH, &mut s.width)
            .range(0.1..=20.0)
            .step(0.25)
            .suffix(" pt")
            .show(ui)
            .changed();
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
    if kind == GeometryKind::Point {
        return marker_fill_rows(ui, rule, salt) || changed;
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

/// The fill kind of a marker, its color, and the angle, the spacing, and the
/// line width of a hatch.
fn marker_fill_rows(ui: &mut egui::Ui, rule: &mut Rule, salt: egui::Id) -> bool {
    let Some(current) = rule.marker_fill_kind() else {
        return false;
    };
    let mut changed = false;
    let mut kind = current;
    ui.horizontal(|ui| {
        ui.label(FILL);
        egui::ComboBox::from_id_salt(salt.with("marker-fill"))
            .selected_text(kind.label())
            .show_ui(ui, |ui| {
                for k in FillKind::ALL {
                    ui.selectable_value(&mut kind, k, k.label());
                }
            });
    });
    if kind != current {
        rule.set_marker_fill_kind(kind);
        changed = true;
    }
    let hatch = rule.marker_hatch();
    if let Some(mut c) = rule.fill() {
        let name = if hatch.is_some() { LINE_COLOR } else { COLOR };
        let mut picked = false;
        ui.horizontal(|ui| {
            ui.label(name);
            picked = color_button(ui, &mut c);
        });
        if picked {
            rule.set_fill(Some(c));
            changed = true;
        }
    }
    let Some(mut h) = hatch else {
        return changed;
    };
    let mut hatch_changed = number::stepper(ANGLE, &mut h.angle)
        .range(Hatch::ANGLES)
        .step(5.0)
        .suffix("°")
        .show(ui)
        .changed();
    hatch_changed |= number::stepper(SPACING, &mut h.spacing)
        .range(Hatch::SPACINGS)
        .step(0.25)
        .suffix(" pt")
        .show(ui)
        .changed();
    hatch_changed |= number::stepper(LINE_WIDTH, &mut h.line_width)
        .range(Hatch::LINE_WIDTHS)
        .step(0.05)
        .suffix(" pt")
        .show(ui)
        .changed();
    if hatch_changed {
        rule.set_marker_hatch(Some(h));
    }
    changed || hatch_changed
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
    match &mut s.kind {
        StrokeKind::Dashed { dash, gap } => {
            for (label, value) in [(DASH, dash), (GAP, gap)] {
                changed |= number::stepper(label, value)
                    .range(DASHES.0..=DASHES.1)
                    .suffix(" pt")
                    .show(ui)
                    .changed();
            }
        }
        StrokeKind::Taper { start } => {
            changed |= number::slider(TAPER_START, start, 0.0..=1.0)
                .percent()
                .step(1.0)
                .show(ui)
                .changed();
        }
        StrokeKind::Solid | StrokeKind::Brush => {}
    }
    changed
}

/// A color button that edits the color and its alpha.
pub fn color_button(ui: &mut egui::Ui, c: &mut Color) -> bool {
    let mut rgba = [c.r, c.g, c.b, c.a];
    if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
        [c.r, c.g, c.b, c.a] = rgba;
        true
    } else {
        false
    }
}

/// The Classify section of the desktop layout. It returns a request when
/// the user clicks "Replace rules".
pub fn classify(
    ui: &mut egui::Ui,
    id: LayerId,
    fields: &[Field],
    ramps: &[Ramp],
    library: &mut Library,
) -> Option<Request> {
    let key = egui::Id::new(("classify", id.0));
    let (mut text, mut method, mut classes, mut ramp, mut reverse): (
        String,
        usize,
        usize,
        usize,
        bool,
    ) = ui.data(|d| d.get_temp(key)).unwrap_or((
        String::new(),
        1,
        classify::DEFAULT_CLASSES,
        0,
        false,
    ));
    let mut out = None;
    egui::CollapsingHeader::new(CLASSIFY)
        .id_salt(key)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(FIELD);
                egui::ComboBox::from_id_salt(("classify-field", id.0))
                    .selected_text(text.clone())
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for f in fields {
                            let r = field_ref(&f.name);
                            ui.selectable_value(&mut text, r, &f.name);
                        }
                    });
            });
            expr_field::value(ui, (EXPRESSION, EXPRESSION), &mut text, library.help);
            ui.horizontal(|ui| {
                ui.label(METHOD);
                egui::ComboBox::from_id_salt(("classify-method", id.0))
                    .selected_text(Method::ALL[method].label())
                    .show_ui(ui, |ui| {
                        for (i, m) in Method::ALL.iter().enumerate() {
                            ui.selectable_value(&mut method, i, m.label());
                        }
                    });
            });
            let (min, max) = classify::CLASSES;
            ui.horizontal(|ui| {
                ui.label(CLASS_COUNT);
                ui.add(egui::DragValue::new(&mut classes).range(min..=max));
            });
            ui.horizontal(|ui| {
                ui.label(COLOR_RAMP);
                let name = ramps.get(ramp).map_or("", |r| r.name.as_str());
                egui::ComboBox::from_id_salt(("classify-ramp", id.0))
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        for (i, r) in ramps.iter().enumerate() {
                            ui.selectable_value(&mut ramp, i, &r.name);
                        }
                    });
            });
            if ui.button(IMPORT_RAMP).clicked() {
                library.actions.push(Action::ImportRampDialog(None));
            }
            ui.checkbox(&mut reverse, REVERSE);
            if ui.button(REPLACE_RULES).clicked()
                && let Some(r) = ramps.get(ramp)
            {
                out = Some(Request {
                    text: text.clone(),
                    method: Method::ALL[method],
                    classes,
                    ramp: r.clone(),
                    reverse,
                    step: Step::None,
                });
            }
        });
    ui.data_mut(|d| d.insert_temp(key, (text, method, classes, ramp, reverse)));
    out
}
