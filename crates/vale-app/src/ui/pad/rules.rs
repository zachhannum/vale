//! The Style section of the panel of a vector layer: the list of the rules,
//! and the page of a rule with its filter, its scale range, its stroke, its
//! fill, its point symbol, and its scatter, and the page of the symbols.

use eframe::egui;
use vale_render::Symbols;
use vale_store::{Field, FieldType, GeometryKind, LayerId};
use vale_style::expr::{Filter, Property, field_ref};
use vale_style::{
    Color, LayerStyle, PointSymbolizer, Rule, ScatterSymbolizer, Stroke, StrokeKind, Symbolizer,
};

use super::raster::heading;
use super::widgets::{self, Controls, Keys, PANEL_ROW};
use super::{Page, filter, theme};
use crate::globe::symbol::paint_symbol;
use crate::ui::{Action, AppState};

/// The heading of the section.
pub const STYLE: &str = "Style";
pub const ADD_RULE: &str = "Add rule";
/// The text at the right of the row of a rule with no filter.
pub const ALL_FEATURES: &str = "All features";
pub const FILTER: &str = "Filter";
/// The field of the whole filter as text.
pub const EXPRESSION: &str = "Expression";
/// The text below a filter that does not parse.
pub const BAD_FILTER: &str = "Not a valid filter.";
pub const MIN_SCALE: &str = "Min scale";
pub const MAX_SCALE: &str = "Max scale";
/// The text of a scale field with no limit.
pub const NO_LIMIT: &str = "None";
/// The text below the scale range when the minimum is not below the maximum.
pub const BAD_RANGE: &str = "Min scale must be less than max scale.";
pub const STROKE: &str = "Stroke";
pub const STROKE_COLOR: &str = "Stroke color";
pub const STROKE_WIDTH: &str = "Width";
pub const LINE: &str = "Line";
pub const SOLID: &str = "Solid";
pub const DASHED: &str = "Dashed";
pub const BRUSH: &str = "Brush";
pub const TAPER: &str = "Taper";
pub const DASH: &str = "Dash";
pub const GAP: &str = "Gap";
pub const TAPER_START: &str = "Start width";
pub const FILL: &str = "Fill";
pub const FILL_COLOR: &str = "Fill color";
/// The choice of the marker of a point layer, and its two choices.
pub const MARKER: &str = "Marker";
pub const CIRCLE: &str = "Circle";
pub const SVG_SYMBOL: &str = "SVG symbol";
/// The row of the SVG symbol of a point symbol or of a scatter. A tap opens
/// the symbol page.
pub const SYMBOL: &str = "Symbol";
/// The title of the symbol page.
pub const SYMBOLS: &str = "Symbols";
/// The button of the symbol page that asks for an SVG file.
pub const ADD_SYMBOL: &str = "Add symbol…";
/// The text of the list of the symbols of the desktop editor when the
/// project has no symbols.
pub const NO_SYMBOL_LIST: &str = "No symbols";
/// The text of the symbol page when the project has no symbols.
pub const NO_SYMBOLS: &str = "Add an SVG file to use it as a symbol.";
/// The size of a circle or of a symbol, in points. The size of a circle is
/// its diameter.
pub const SIZE: &str = "Size";
pub const ROTATION: &str = "Rotation";
pub const SIZE_FIELD: &str = "Size from field";
pub const ROTATION_FIELD: &str = "Rotation from field";
/// The text of a field row with no field, and its first chip.
pub const NO_FIELD: &str = "None";
pub const TINT: &str = "Tint";
pub const TINT_COLOR: &str = "Tint color";
pub const SCATTER: &str = "Scatter";
pub const SPACING: &str = "Spacing";
pub const SIZE_VARIATION: &str = "Size variation";
pub const ROTATION_VARIATION: &str = "Rotation variation";
pub const MOVE_UP: &str = "Move rule up";
pub const MOVE_DOWN: &str = "Move rule down";
pub const DELETE_RULE: &str = "Delete rule";
/// The range of the width slider, in points. The slider has a log scale.
const WIDTHS: (f64, f64) = (0.1, 20.0);
/// The range of the dash and gap sliders, in points. The sliders have a log scale.
const DASHES: (f64, f64) = (0.5, 20.0);
/// The range of the diameter of a circle, in points.
pub const CIRCLE_SIZES: (f64, f64) = (2.0, 32.0);
/// The range of the size of a point symbol, in points.
pub const POINT_SIZES: (f64, f64) = (4.0, 64.0);
/// The range of the size of a scatter symbol, in points.
pub const SCATTER_SIZES: (f64, f64) = (4.0, 48.0);
/// The largest spacing of a scatter, in points. The smallest is its size.
pub const MAX_SPACING: f64 = 100.0;
/// The size of a new SVG symbol of a point layer.
const NEW_SYMBOL: f64 = 12.0;
/// The size and the spacing of a new scatter.
const NEW_SCATTER: (f64, f64) = (8.0, 10.0);
/// The color behind the preview of a symbol.
pub const PAPER: egui::Color32 = egui::Color32::from_rgb(236, 232, 220);
/// The color that a tint switch turns on.
const TINT_GRAY: Color = Color::rgb(60, 60, 60);
/// The side of the preview of a symbol in its row.
const PREVIEW: f32 = 30.0;
/// The height of the keypad of the scale fields.
const KEYPAD: f32 = 4.0 * PANEL_ROW;
/// The longest filter text that the row of a rule shows, in characters.
const ROW_FILTER: usize = 18;

/// The color of a rule that the color picker edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Stroke,
    Fill,
    Tint,
}

/// The state of the Style section of a vector layer.
#[derive(Default)]
pub struct RulesState {
    /// The text of the filter field while the user types.
    filter: Option<(LayerId, usize, String)>,
    /// The color that the color picker edits.
    picker: Option<(LayerId, usize, Part)>,
    /// The picker showed in this frame.
    shown: bool,
    /// The field row that shows the chips of the fields.
    field: Option<(LayerId, usize, &'static str)>,
    /// The condition of the page of a new condition.
    pub(super) draft: filter::Draft,
}

/// The Style section: a row for each rule, first rule first, and the row
/// that adds a rule. A later rule draws on top. A tap on a rule opens its
/// page.
pub fn section(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: LayerId) {
    let Some(kind) = state.doc.layer(id).and_then(|l| l.geometry()) else {
        return;
    };
    let Some(entry) = state.doc.entry(id) else {
        return;
    };
    let rows: Vec<(String, String)> = entry
        .style
        .style
        .rules
        .iter()
        .map(|r| (r.name.clone(), row_filter(&r.filter)))
        .collect();
    heading(ui, STYLE);
    for (i, (name, filter)) in rows.iter().enumerate() {
        if widgets::open_row(ui, controls, name, filter).clicked() {
            state.pad.page = Some(Page::Rule(i));
        }
    }
    if widgets::button_row(ui, controls, ADD_RULE, !state.pad.rules_fixed).clicked()
        && let Some(entry) = state.doc.entry_mut(id)
    {
        let i = entry.style.add_rule(kind);
        state.pad.page = Some(Page::Rule(i));
        state.touch();
    }
}

/// The filter text of the row of a rule.
fn row_filter(filter: &Filter) -> String {
    let Some(source) = filter.source() else {
        return ALL_FEATURES.to_owned();
    };
    let text = source.text().trim();
    if text.chars().count() > ROW_FILTER {
        let short: String = text.chars().take(ROW_FILTER - 1).collect();
        format!("{short}…")
    } else {
        text.to_owned()
    }
}

/// The page of rule `i`: the conditions of the filter, the scale range, the stroke, the fill, and the buttons that move and delete the
/// rule. A tap on a color opens a color picker below it.
pub fn rule_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
    i: usize,
) {
    let Some(layer) = state.doc.layer(id) else {
        return;
    };
    let Some(kind) = layer.geometry() else {
        return;
    };
    let Some(rule) = state.doc.entry(id).and_then(|e| e.style.style.rules.get(i)) else {
        return;
    };
    let rule = rule.clone();
    let count = state.doc.entry(id).map_or(0, |e| e.style.style.rules.len());
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut changed = rule.clone();

    filter_rows(ui, state, controls, (id, i), &mut changed);
    if scale_rows(ui, controls, &mut changed) {
        state.pad.rules.picker = None;
    }
    ui.add_space(8.0);
    paint_rows(ui, state, controls, (id, i), kind, &mut changed);

    if changed != rule
        && let Some(r) = state
            .doc
            .entry_mut(id)
            .and_then(|e| e.style.style.rules.get_mut(i))
    {
        *r = changed;
        state.touch();
    }

    ui.add_space(8.0);
    let free = !state.pad.rules_fixed;
    let moved = if widgets::button_row(ui, controls, MOVE_UP, free && i > 0).clicked() {
        Some(true)
    } else if widgets::button_row(ui, controls, MOVE_DOWN, free && i + 1 < count).clicked() {
        Some(false)
    } else {
        None
    };
    if let Some(up) = moved
        && let Some(entry) = state.doc.entry_mut(id)
    {
        let to = entry.style.style.move_rule(i, up);
        state.pad.page = Some(Page::Rule(to));
        state.pad.rules = RulesState::default();
        state.touch();
    }
    if widgets::button_row_in(ui, controls, DELETE_RULE, free, theme::WARN).clicked()
        && let Some(entry) = state.doc.entry_mut(id)
    {
        entry.style.style.rules.remove(i);
        state.pad.page = None;
        state.pad.rules = RulesState::default();
        state.touch();
    }
}

/// The conditions of the filter, and the text of the whole filter.
fn filter_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (id, i): (LayerId, usize),
    rule: &mut Rule,
) {
    let current = rule
        .filter
        .source()
        .map_or(String::new(), |s| s.text().to_owned());
    let mut typed = match state.pad.rules.filter.take() {
        Some((layer, at, text)) if layer == id && at == i => text,
        _ => current.clone(),
    };
    if let Some(text) = filter::rows(ui, state, controls, (id, i), &current, &mut typed) {
        rule.filter = Filter::new(&text);
    }
    state.pad.rules.filter = Some((id, i, typed));
}

/// The fields of the scale range, the keypad of the field that the user
/// types in, and a note when the range is empty. Returns true while the
/// keypad shows.
fn scale_rows(ui: &mut egui::Ui, controls: &mut Controls, rule: &mut Rule) -> bool {
    let none = |text: &str| text.trim().is_empty() || text == NO_LIMIT;
    let valid = |text: &str| none(text) || parse_scale(text).is_some();
    for (name, value) in [
        (MIN_SCALE, &mut rule.min_scale),
        (MAX_SCALE, &mut rule.max_scale),
    ] {
        let shown = value.map_or(NO_LIMIT.to_owned(), scale_text);
        let typed = widgets::key_field(ui, controls, name, name, "", &shown, Keys::Number, valid);
        if let Some(text) = typed {
            if none(&text) {
                *value = None;
            } else if let Some(scale) = parse_scale(&text) {
                *value = Some(scale);
            }
        }
    }
    let open = widgets::keypad_field(ui.ctx());
    let keypad = open
        .as_deref()
        .is_some_and(|f| f == MIN_SCALE || f == MAX_SCALE);
    if keypad {
        ui.add_space(4.0);
        widgets::keypad(ui, controls, KEYPAD);
    }
    if let (Some(min), Some(max)) = (rule.min_scale, rule.max_scale)
        && min >= max
    {
        widgets::form_note(ui, BAD_RANGE, 20.0);
    }
    keypad
}

/// A scale denominator as the user types it: a number above zero.
pub fn parse_scale(text: &str) -> Option<f64> {
    super::raster::parse_value(text).filter(|v| v.is_finite() && *v > 0.0)
}

/// A scale denominator with the separators of the thousands.
pub fn scale_text(value: f64) -> String {
    let digits = format!("{:.0}", value.round());
    let mut out = String::new();
    for (n, c) in digits.chars().enumerate() {
        if n > 0 && (digits.len() - n) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The switches, the colors, and the width of the stroke and the fill. A
/// line layer has no fill.
fn paint_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (id, i): (LayerId, usize),
    kind: GeometryKind,
    rule: &mut Rule,
) {
    let default = LayerStyle::default_for(kind, 0, &[]);
    let open = state.pad.rules.picker.filter(|p| p.0 == id && p.1 == i);
    // An SVG symbol has no stroke and no fill.
    if kind == GeometryKind::Point && symbol_rows(ui, state, controls, (id, i), rule) {
        return;
    }

    let mut on = rule.stroke().is_some();
    if widgets::switch_row(ui, controls, STROKE, STROKE, &mut on) {
        let stroke = default
            .stroke()
            .unwrap_or(Stroke::solid(Color::rgb(90, 90, 90), 1.0));
        rule.set_stroke(on.then_some(stroke));
    }
    if let Some(mut stroke) = rule.stroke() {
        let mut rgb = [stroke.color.r, stroke.color.g, stroke.color.b];
        if color_rows(ui, state, controls, (id, i, Part::Stroke), open, &mut rgb) {
            stroke.color = Color {
                r: rgb[0],
                g: rgb[1],
                b: rgb[2],
                ..stroke.color
            };
            rule.set_stroke(Some(stroke));
        }
        if let Some(width) = points_slider(ui, controls, STROKE_WIDTH, stroke.width, WIDTHS) {
            stroke.width = width;
            rule.set_stroke(Some(stroke));
        }
        // A marker ignores the line type.
        if kind != GeometryKind::Point {
            line_rows(ui, controls, rule, stroke, kind);
        }
    }
    if kind == GeometryKind::Line {
        return;
    }

    let mut on = rule.fill().is_some();
    if widgets::switch_row(ui, controls, FILL, FILL, &mut on) {
        let fill = default.fill().unwrap_or(Color::rgb(200, 200, 200));
        rule.set_fill(on.then_some(fill));
    }
    if let Some(fill) = rule.fill() {
        let mut rgb = [fill.r, fill.g, fill.b];
        if color_rows(ui, state, controls, (id, i, Part::Fill), open, &mut rgb) {
            rule.set_fill(Some(Color {
                r: rgb[0],
                g: rgb[1],
                b: rgb[2],
                ..fill
            }));
        }
    }
    if kind == GeometryKind::Polygon {
        scatter_rows(ui, state, controls, (id, i), rule);
    }
}

/// The choice of a circle or an SVG symbol at each point. A circle has its
/// size. An SVG symbol has the row of its symbol, its size, its rotation,
/// the fields of the size and the rotation, and its tint. A tap on "SVG
/// symbol" opens the symbol page, and the circle stays until the user picks
/// a symbol. Returns true if the rule has an SVG symbol.
fn symbol_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (id, i): (LayerId, usize),
    rule: &mut Rule,
) -> bool {
    let active = usize::from(rule.symbol().is_some());
    if let Some(index) =
        widgets::choice_row(ui, controls, MARKER, &[CIRCLE, SVG_SYMBOL], Some(active))
    {
        if index == 1 {
            open_symbols(state, i);
        } else {
            state.pad.rules.picker = None;
            to_circle(rule);
        }
    }
    let Some(before) = rule.symbol() else {
        if let Some(radius) = marker_radius(rule) {
            let size = 2.0 * radius.fallback;
            if let Some(size) = step_slider(ui, controls, SIZE, size, CIRCLE_SIZES, 0.5, "pt") {
                radius.fallback = size / 2.0;
            }
        }
        return false;
    };
    let mut s = before.clone();
    let tint = s.tint.as_ref().map(|t| t.fallback);
    if symbol_row(ui, state, controls, &s.symbol, tint).clicked() {
        open_symbols(state, i);
    }
    if let Some(size) = step_slider(ui, controls, SIZE, s.size.fallback, POINT_SIZES, 1.0, "pt") {
        s.size.fallback = size;
    }
    let mut degrees = s.rotation.fallback.rem_euclid(360.0) as f32;
    if widgets::compass_row(ui, controls, ROTATION, ROTATION, &mut degrees) {
        s.rotation.fallback = f64::from(degrees);
    }
    let fields = numeric_fields(state.doc.layer(id).map_or(&[][..], |l| &l.fields));
    field_row(
        ui,
        state,
        controls,
        (id, i, SIZE_FIELD),
        &mut s.size,
        &fields,
    );
    field_row(
        ui,
        state,
        controls,
        (id, i, ROTATION_FIELD),
        &mut s.rotation,
        &fields,
    );
    tint_rows(ui, state, controls, (id, i), &mut s.tint);
    if Some(&s) != rule.symbol() {
        rule.set_symbol(Some(s));
    }
    true
}

/// The radius of the circle of a rule.
pub fn marker_radius(rule: &mut Rule) -> Option<&mut Property<f64>> {
    rule.symbolizers.iter_mut().find_map(|s| match s {
        Symbolizer::Marker { radius, .. } => Some(radius),
        _ => None,
    })
}

/// The switch of the scatter of a polygon layer and, while it is on, the
/// row of its symbol, its size, its spacing, its variations, and its tint.
/// A switch that turns on opens the symbol page, and the scatter starts when
/// the user picks a symbol.
fn scatter_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (id, i): (LayerId, usize),
    rule: &mut Rule,
) {
    let mut on = rule.scatter().is_some();
    if widgets::switch_row(ui, controls, SCATTER, SCATTER, &mut on) {
        if on {
            open_symbols(state, i);
        } else {
            rule.set_scatter(None);
        }
    }
    let Some(before) = rule.scatter() else {
        return;
    };
    let mut s = before.clone();
    let tint = s.tint.as_ref().map(|t| t.fallback);
    if symbol_row(ui, state, controls, &s.symbol, tint).clicked() {
        open_symbols(state, i);
    }
    if let Some(size) = step_slider(
        ui,
        controls,
        SIZE,
        s.size.fallback,
        SCATTER_SIZES,
        1.0,
        "pt",
    ) {
        s.size.fallback = size;
    }
    let size = s.size.fallback;
    s.spacing.fallback = s.spacing.fallback.max(size);
    let spacings = (size, MAX_SPACING.max(size));
    if let Some(spacing) = step_slider(
        ui,
        controls,
        SPACING,
        s.spacing.fallback,
        spacings,
        1.0,
        "pt",
    ) {
        s.spacing.fallback = spacing.max(size);
    }
    let mut fraction = (s.size_jitter / 0.5).clamp(0.0, 1.0);
    let text = format!("{:.0} %", s.size_jitter * 100.0);
    if widgets::row_slider(
        ui,
        controls,
        SIZE_VARIATION,
        SIZE_VARIATION,
        &mut fraction,
        &text,
    ) {
        s.size_jitter = (fraction * 50.0).round() / 100.0;
    }
    let mut fraction = (s.rotation_jitter / 180.0).clamp(0.0, 1.0);
    let text = format!("{:.0}\u{b0}", s.rotation_jitter);
    let name = ROTATION_VARIATION;
    if widgets::row_slider(ui, controls, name, name, &mut fraction, &text) {
        s.rotation_jitter = (fraction * 180.0).round();
    }
    tint_rows(ui, state, controls, (id, i), &mut s.tint);
    if Some(&s) != rule.scatter() {
        rule.set_scatter(Some(s));
    }
}

/// Opens the symbol page of rule `i`.
fn open_symbols(state: &mut AppState, i: usize) {
    state.pad.page = Some(Page::Symbols(i));
    state.pad.rules.picker = None;
    state.pad.rules.field = None;
    state.symbol_note = None;
}

/// The row of the symbol `current`, with its preview in `tint` and its name.
/// A tap opens the symbol page.
fn symbol_row(
    ui: &mut egui::Ui,
    state: &AppState,
    controls: &mut Controls,
    current: &str,
    tint: Option<Color>,
) -> egui::Response {
    let resp = widgets::open_row(ui, controls, SYMBOL, current);
    let row = resp.rect;
    let name = ui
        .painter()
        .layout_no_wrap(current.to_owned(), theme::body(), theme::MUTE);
    let right = row.right() - widgets::ICON - 8.0 - name.size().x - 10.0;
    let square = egui::Rect::from_center_size(
        egui::pos2(right - PREVIEW / 2.0, row.center().y),
        egui::vec2(PREVIEW, PREVIEW),
    );
    paint_preview(ui.painter(), state.doc.symbols(), current, square, tint);
    resp
}

/// Paints symbol `name` on the color of paper, as on a map. A name that the
/// library does not have paints the paper only.
fn paint_preview(
    painter: &egui::Painter,
    symbols: &Symbols,
    name: &str,
    square: egui::Rect,
    tint: Option<Color>,
) {
    painter.rect_filled(square, theme::CONTROL_RADIUS.min(6.0), PAPER);
    if let Some(symbol) = symbols.get(name) {
        let side = 0.75 * square.width();
        paint_symbol(painter, symbol, square.center(), side, 0.0, tint);
    }
}

/// The symbol page of rule `i`: a tile for each symbol of the project, with
/// the tile of the current symbol on, the button that adds an SVG file, and
/// the text of the last addition. A tap on a tile picks the symbol and goes
/// back to the page of the rule.
pub fn symbols_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
    i: usize,
) {
    let Some(kind) = state.doc.layer(id).and_then(|l| l.geometry()) else {
        return;
    };
    let Some(rule) = state.doc.entry(id).and_then(|e| e.style.style.rules.get(i)) else {
        return;
    };
    let (current, tint) = match kind {
        GeometryKind::Point => rule.symbol().map(|s| (s.symbol.clone(), s.tint.clone())),
        _ => rule.scatter().map(|s| (s.symbol.clone(), s.tint.clone())),
    }
    .unzip();
    let tint = tint.flatten().map(|t| t.fallback);
    ui.spacing_mut().item_spacing.y = 0.0;
    let symbols = state.doc.symbols().clone();
    let names = symbols.names();
    let mut picked = None;
    ui.add_space(8.0);
    if names.is_empty() {
        let text = egui::RichText::new(NO_SYMBOLS)
            .font(theme::body())
            .color(theme::MUTE);
        ui.vertical_centered(|ui| {
            ui.add_space(12.0);
            ui.add(egui::Label::new(text).wrap().selectable(false));
            ui.add_space(12.0);
        });
    } else {
        let active = names.iter().position(|n| Some(*n) == current.as_deref());
        let paint = |painter: &egui::Painter, square: egui::Rect, index: usize| {
            let tint = if Some(index) == active { tint } else { None };
            paint_preview(painter, &symbols, names[index], square, tint);
        };
        picked = widgets::tiles(ui, controls, &names, active, paint).map(|n| names[n].to_owned());
    }
    ui.add_space(16.0);
    if widgets::button_row(ui, controls, ADD_SYMBOL, true).clicked() {
        state.actions.push(Action::AddSymbolDialog);
    }
    ui.add_space(4.0);
    super::note_text(ui, None, &mut state.symbol_note, None);
    let Some(name) = picked else {
        return;
    };
    if let Some(rule) = state
        .doc
        .entry_mut(id)
        .and_then(|e| e.style.style.rules.get_mut(i))
    {
        let before = rule.clone();
        pick_symbol(rule, kind, &name);
        if *rule != before {
            state.touch();
        }
    }
    state.pad.page = Some(Page::Rule(i));
}

/// The row of the field of a property and, while it is open, a chip for
/// "None" and for each field in `fields`. A field makes the property an
/// expression on the field, with the current value as its fallback.
fn field_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (id, i, name): (LayerId, usize, &'static str),
    property: &mut Property<f64>,
    fields: &[String],
) {
    let open = state.pad.rules.field == Some((id, i, name));
    let row = widgets::text_row(ui, controls, name, 0.0, open, !fields.is_empty());
    let value = property_field(property, fields);
    let at = egui::pos2(row.rect.right(), row.rect.center().y);
    ui.painter().text(
        at,
        egui::Align2::RIGHT_CENTER,
        value.as_deref().unwrap_or(NO_FIELD),
        theme::body(),
        theme::MUTE,
    );
    if row.clicked() {
        state.pad.rules.field = (!open).then_some((id, i, name));
        state.pad.rules.picker = None;
        widgets::close_keypad(ui.ctx());
    }
    if !open {
        return;
    }
    let mut choices = vec![NO_FIELD];
    choices.extend(fields.iter().map(String::as_str));
    let active = value
        .and_then(|v| fields.iter().position(|f| *f == v))
        .map_or(0, |at| at + 1);
    if let Some(chosen) = widgets::chips(ui, controls, &choices, active) {
        let field = chosen.checked_sub(1).map(|at| fields[at].as_str());
        *property = with_field(property, field);
        state.pad.rules.field = None;
    }
    ui.add_space(4.0);
}

/// The switch of a tint and, while it is on, its color.
fn tint_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (id, i): (LayerId, usize),
    tint: &mut Option<Property<Color>>,
) {
    let mut on = tint.is_some();
    if widgets::switch_row(ui, controls, TINT, TINT, &mut on) {
        *tint = on.then(|| Property::constant(TINT_GRAY));
    }
    let open = state.pad.rules.picker.filter(|p| p.0 == id && p.1 == i);
    if let Some(t) = tint {
        let c = t.fallback;
        let mut rgb = [c.r, c.g, c.b];
        if color_rows(ui, state, controls, (id, i, Part::Tint), open, &mut rgb) {
            t.fallback = Color {
                r: rgb[0],
                g: rgb[1],
                b: rgb[2],
                ..c
            };
        }
    }
}

/// A slider from `lo` to `hi` in steps of `step`, with `unit` after the
/// value. Returns the new value.
fn step_slider(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    value: f64,
    (lo, hi): (f64, f64),
    step: f64,
    unit: &str,
) -> Option<f64> {
    let span = (hi - lo).max(f64::EPSILON);
    let mut fraction = ((value - lo) / span).clamp(0.0, 1.0);
    let text = format!("{} {unit}", width_text(value));
    if !widgets::row_slider(ui, controls, name, name, &mut fraction, &text) {
        return None;
    }
    let value = lo + fraction * span;
    Some(((value / step).round() * step).clamp(lo, hi))
}

/// The SVG symbol that replaces the circle of a point layer.
pub fn new_point_symbol(name: &str) -> PointSymbolizer {
    PointSymbolizer::constant(name, NEW_SYMBOL)
}

/// The scatter that starts when the user picks its symbol.
pub fn new_scatter(name: &str) -> ScatterSymbolizer {
    let (size, spacing) = NEW_SCATTER;
    ScatterSymbolizer::new(name, size, spacing)
}

/// The tint that a tint switch turns on.
pub fn new_tint() -> Color {
    TINT_GRAY
}

/// Replaces the SVG symbol of a rule with the circle of a new point layer.
/// A tint becomes the fill of the circle.
pub fn to_circle(rule: &mut Rule) {
    let Some(at) = rule
        .symbolizers
        .iter()
        .position(|s| matches!(s, Symbolizer::Symbol(_)))
    else {
        return;
    };
    let tint = rule.symbol().and_then(|s| s.tint.clone());
    let default = LayerStyle::default_for(GeometryKind::Point, 0, &[]);
    let mut marker = default.style.rules[0].symbolizers[0].clone();
    if let (Symbolizer::Marker { fill, .. }, Some(tint)) = (&mut marker, tint) {
        *fill = Some(tint);
    }
    rule.symbolizers[at] = marker;
}

/// Sets the symbol of the point symbol of a point layer, or of the scatter
/// of a polygon layer, to `name`. A circle becomes a new point symbol, and a
/// polygon rule with no scatter gets a new scatter.
pub fn pick_symbol(rule: &mut Rule, kind: GeometryKind, name: &str) {
    match kind {
        GeometryKind::Point => {
            let mut s = rule
                .symbol()
                .cloned()
                .unwrap_or_else(|| new_point_symbol(name));
            s.symbol = name.to_owned();
            rule.set_symbol(Some(s));
        }
        GeometryKind::Polygon => {
            let mut s = rule.scatter().cloned().unwrap_or_else(|| new_scatter(name));
            s.symbol = name.to_owned();
            rule.set_scatter(Some(s));
        }
        GeometryKind::Line => {}
    }
}

/// The names of the number fields of a layer.
pub fn numeric_fields(fields: &[Field]) -> Vec<String> {
    let number = |f: &&Field| matches!(f.kind, FieldType::Integer | FieldType::Real);
    fields
        .iter()
        .filter(number)
        .map(|f| f.name.clone())
        .collect()
}

/// The field of a property that is an expression on one field, or the text
/// of another expression. `None`: the property is a constant.
pub fn property_field(property: &Property<f64>, fields: &[String]) -> Option<String> {
    let text = property.source()?.text().trim();
    let field = fields.iter().find(|f| field_ref(f) == text);
    Some(field.map_or(text, |f| f.as_str()).to_owned())
}

/// The property with its value from `field`, and the fallback of
/// `property`. `None`: a constant.
pub fn with_field(property: &Property<f64>, field: Option<&str>) -> Property<f64> {
    match field {
        Some(f) => Property::expression(&field_ref(f), property.fallback),
        None => Property::constant(property.fallback),
    }
}

/// The line type of a stroke, with its dash and gap or its start width.
fn line_rows(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    rule: &mut Rule,
    mut stroke: Stroke,
    kind: GeometryKind,
) {
    let mut kinds = vec![SOLID, DASHED, BRUSH];
    if kind == GeometryKind::Line {
        kinds.push(TAPER);
    }
    let active = match stroke.kind {
        StrokeKind::Solid => 0,
        StrokeKind::Dashed { .. } => 1,
        StrokeKind::Brush => 2,
        StrokeKind::Taper { .. } => 3,
    };
    if let Some(index) = widgets::choice_row(ui, controls, LINE, &kinds, Some(active))
        && index != active
    {
        stroke.kind = match index {
            1 => StrokeKind::dashed(),
            2 => StrokeKind::Brush,
            3 => StrokeKind::taper(),
            _ => StrokeKind::Solid,
        };
        rule.set_stroke(Some(stroke));
    }
    match stroke.kind {
        StrokeKind::Dashed { dash, gap } => {
            if let Some(dash) = points_slider(ui, controls, DASH, dash, DASHES) {
                stroke.kind = StrokeKind::Dashed { dash, gap };
                rule.set_stroke(Some(stroke));
            }
            if let Some(gap) = points_slider(ui, controls, GAP, gap, DASHES) {
                stroke.kind = StrokeKind::Dashed { dash, gap };
                rule.set_stroke(Some(stroke));
            }
        }
        StrokeKind::Taper { start } => {
            let mut fraction = start.clamp(0.0, 1.0);
            let text = format!("{:.0} %", fraction * 100.0);
            if widgets::row_slider(ui, controls, TAPER_START, TAPER_START, &mut fraction, &text) {
                let start = (fraction * 100.0).round() / 100.0;
                stroke.kind = StrokeKind::Taper { start };
                rule.set_stroke(Some(stroke));
            }
        }
        StrokeKind::Solid | StrokeKind::Brush => {}
    }
}

/// A slider of a length in points, with a log scale from `lo` to `hi`.
/// Returns the new length, in steps of a twentieth of a point.
fn points_slider(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    value: f64,
    (lo, hi): (f64, f64),
) -> Option<f64> {
    let mut fraction = (value.clamp(lo, hi) / lo).ln() / (hi / lo).ln();
    let text = format!("{} pt", width_text(value));
    if !widgets::row_slider(ui, controls, name, name, &mut fraction, &text) {
        return None;
    }
    let value = lo * (hi / lo).powf(fraction);
    Some(((value * 20.0).round() / 20.0).max(lo))
}

/// A width with at most two decimals and no trailing zeros.
fn width_text(width: f64) -> String {
    let text = format!("{width:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The row of a color and, while it is open, its color picker. Returns true
/// if the user changed the color.
fn color_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    this: (LayerId, usize, Part),
    open: Option<(LayerId, usize, Part)>,
    rgb: &mut [u8; 3],
) -> bool {
    let name = match this.2 {
        Part::Stroke => STROKE_COLOR,
        Part::Fill => FILL_COLOR,
        Part::Tint => TINT_COLOR,
    };
    let is_open = open == Some(this);
    if widgets::color_row(ui, controls, name, *rgb, is_open).clicked() {
        state.pad.rules.picker = (!is_open).then_some(this);
        // The keypad of a scale field or of the other picker closes.
        widgets::close_keypad(ui.ctx());
    }
    if state.pad.rules.picker != Some(this) {
        return false;
    }
    state.pad.rules.shown = true;
    ui.add_space(4.0);
    widgets::color_picker(ui, controls, name, rgb)
}

/// Closes the color picker when it did not show in this frame, with the
/// keypad of its field.
pub fn picker_frame(ctx: &egui::Context, state: &mut AppState) {
    let rules = &mut state.pad.rules;
    let shown = std::mem::take(&mut rules.shown);
    if !shown && rules.picker.take().is_some() {
        widgets::close_keypad(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_names_and_scales() {
        assert_eq!(field_ref("scalerank"), "scalerank");
        assert_eq!(field_ref("name en"), "\"name en\"");
        assert_eq!(field_ref("2nd"), "\"2nd\"");
        assert_eq!(field_ref("Not"), "\"Not\"");
        assert_eq!(field_ref("a\"b"), "\"a\"\"b\"");
        assert_eq!(scale_text(1_000_000.0), "1,000,000");
        assert_eq!(scale_text(500.0), "500");
        assert_eq!(parse_scale("50,000"), Some(50_000.0));
        assert_eq!(parse_scale("0"), None);
        assert_eq!(width_text(1.5), "1.5");
        assert_eq!(width_text(2.0), "2");
    }
}
