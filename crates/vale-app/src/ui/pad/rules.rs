//! The Style section of the panel of a vector layer: the list of the rules,
//! and the page of a rule with its filter, its scale range, its stroke, its
//! fill, its point symbol, and its scatter, the page of the symbols, and the
//! page of the mask.

use eframe::egui;
use vale_render::Symbols;
use vale_store::{Field, FieldType, FillKind, GeometryKind, LayerId};
use vale_style::expr::{Filter, Property, field_ref};
use vale_style::{
    Color, Hatch, LayerStyle, MarkerShape, Mask, PointSymbolizer, Rule, ScatterSymbolizer, Stroke,
    StrokeKind, Symbolizer,
};

use super::classify::CLASSIFY;
use super::furniture::{ANGLE, LINE_COLOR, LINE_WIDTH, fill_kind_rows};
use super::number::Number;
use super::raster::{self, heading};
use super::widgets::{self, Controls, Keys, PANEL_ROW};
use super::{Page, filter, theme};
use crate::document::Document;
use crate::globe::symbol::paint_symbol;
use crate::ui::expr_help::{self, ExprHelp};
use crate::ui::{Action, AppState};

/// The heading of the section.
pub const STYLE: &str = "Style";
pub const ADD_RULE: &str = "Add rule";
/// The text at the right of the row of a rule with no filter.
pub const ALL_FEATURES: &str = "All features";
pub const FILTER: &str = "Filter";
/// The field of the whole filter as text.
pub const EXPRESSION: &str = "Expression";
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
pub const SHAPE: &str = "Shape";
pub const SVG_SYMBOL: &str = "SVG symbol";
/// The row of the shape of a marker, and the title of its page. A tap opens the page.
pub const MARKER_SHAPE: &str = "Marker shape";
/// The color of a solid fill of a marker.
pub const COLOR: &str = "Color";
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
/// The size of a shape or of a symbol, in points. The size of a circle is
/// its diameter, and the size of another shape is its width.
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
/// The row of the mask of a layer, and the title of its page.
pub const MASK: &str = "Mask";
/// The row of the mask page and the text of the Mask row with no mask.
pub const NO_MASK: &str = "None";
pub const RASTER_LAYERS: &str = "Raster layers";
pub const POLYGON_LAYERS: &str = "Polygon layers";
pub const MASK_MIN: &str = "Minimum";
pub const MASK_MAX: &str = "Maximum";
/// The text below the range of a raster mask when the minimum is not below the maximum.
pub const BAD_MASK_RANGE: &str = "Minimum must be less than maximum.";
/// The hint of the mask page.
pub const MASK_HINT: &str = "The fill and the stroke draw only inside the mask.";
/// The range of the width slider, in points. The slider has a log scale.
pub const WIDTHS: (f64, f64) = (0.1, 20.0);
/// The range of the dash and gap sliders, in points. The sliders have a log scale.
pub const DASHES: (f64, f64) = (0.1, 20.0);
/// The range of the size of a shape, in points.
pub const CIRCLE_SIZES: (f64, f64) = (0.1, 32.0);
/// The range of the size of a point symbol, in points.
pub const POINT_SIZES: (f64, f64) = (0.1, 64.0);
/// The range of the size of a scatter symbol, in points.
pub const SCATTER_SIZES: (f64, f64) = (0.1, 48.0);
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
    /// The fill of a marker. True: the fill is hatched.
    Marker(bool),
}

/// The state of the Style section of a vector layer.
#[derive(Default)]
pub struct RulesState {
    /// The color that the color picker edits.
    picker: Option<(LayerId, usize, Part)>,
    /// The picker showed in this frame.
    shown: bool,
    /// The field row that shows the chips of the fields.
    field: Option<(LayerId, usize, &'static str)>,
    /// The Classify form.
    pub(super) classify: super::classify::Draft,
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
    if widgets::button_row(ui, controls, ADD_RULE, true).clicked()
        && let Some(entry) = state.doc.entry_mut(id)
    {
        let i = entry.style.add_rule(kind);
        state.pad.page = Some(Page::Rule(i));
        state.touch();
    }
    if widgets::button_row(ui, controls, CLASSIFY, true).clicked() {
        state.pad.page = Some(Page::Classify);
    }
    if kind != GeometryKind::Point {
        let text = mask_text(state, id);
        if widgets::open_row(ui, controls, MASK, &text).clicked() {
            state.pad.page = Some(Page::Mask);
        }
    }
}

/// The text of the Mask row: the name of the layer of the mask, or "None".
fn mask_text(state: &AppState, id: LayerId) -> String {
    let mask = state.doc.entry(id).and_then(|e| e.style.style.mask);
    mask.and_then(|m| state.doc.layer(m.layer()))
        .map_or(NO_MASK.to_owned(), |l| l.name.clone())
}

/// The layers that can be the mask of layer `id`: the raster layers, and the polygon layers other than `id`, each with its name and the mask that a tap on it sets. The mask of a height layer is its land.
pub fn mask_choices(doc: &Document, id: LayerId) -> (Vec<MaskChoice>, Vec<MaskChoice>) {
    let mut rasters = Vec::new();
    let mut polygons = Vec::new();
    for layer in doc.project.layers() {
        if let Some(rid) = layer.raster() {
            let height = doc.project.raster(rid).is_some_and(crate::ui::is_height);
            let mask = if height {
                Mask::land(layer.id)
            } else {
                Mask::Raster {
                    layer: layer.id,
                    min: None,
                    max: None,
                }
            };
            rasters.push((layer.name.clone(), mask));
        } else if layer.geometry() == Some(GeometryKind::Polygon) && layer.id != id {
            polygons.push((layer.name.clone(), Mask::Polygons { layer: layer.id }));
        }
    }
    (rasters, polygons)
}

/// The name of a layer, and the mask that a tap on it sets.
pub type MaskChoice = (String, Mask);

/// Sets the mask of layer `id`.
fn set_mask(state: &mut AppState, id: LayerId, mask: Option<Mask>) {
    if let Some(entry) = state.doc.entry_mut(id)
        && entry.style.style.mask != mask
    {
        entry.style.style.mask = mask;
        state.touch();
    }
}

/// The mask page of layer `id`: "None", a row for each raster layer and for each other polygon layer, and the range of a raster mask.
pub fn mask_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: LayerId) {
    let Some(entry) = state.doc.entry(id) else {
        return;
    };
    let current = entry.style.style.mask;
    let (rasters, polygons) = mask_choices(&state.doc, id);
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(8.0);
    if widgets::option_row(ui, controls, NO_MASK, NO_MASK, &[], current.is_none()).clicked() {
        set_mask(state, id, None);
    }
    for (title, list) in [(RASTER_LAYERS, rasters), (POLYGON_LAYERS, polygons)] {
        if list.is_empty() {
            continue;
        }
        heading(ui, title);
        for (name, mask) in list {
            let on = current.is_some_and(|m| m.layer() == mask.layer());
            if widgets::option_row(ui, controls, &name, &name, &[], on).clicked() && !on {
                set_mask(state, id, Some(mask));
            }
        }
    }
    if let Some(Mask::Raster { layer, min, max }) = current {
        let mut range = (min, max);
        heading(ui, "Range");
        if range_rows(ui, controls, &mut range) {
            let mask = Mask::Raster {
                layer,
                min: range.0,
                max: range.1,
            };
            set_mask(state, id, Some(mask));
        }
    }
    ui.add_space(8.0);
    raster::note(ui, MASK_HINT);
}

/// The minimum and the maximum of a raster mask. A blank field has no limit. True if a value changed.
fn range_rows(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    range: &mut (Option<f64>, Option<f64>),
) -> bool {
    let none = |text: &str| text.trim().is_empty() || text == NO_LIMIT;
    let valid = |text: &str| none(text) || raster::parse_value(text).is_some();
    let before = *range;
    for (name, value) in [(MASK_MIN, &mut range.0), (MASK_MAX, &mut range.1)] {
        let shown = value.map_or(NO_LIMIT.to_owned(), value_text);
        let typed = widgets::key_field(ui, controls, name, name, "", &shown, Keys::Number, valid);
        if let Some(text) = typed {
            if none(&text) {
                *value = None;
            } else if let Some(v) = raster::parse_value(&text) {
                *value = Some(v);
            }
        }
    }
    let open = widgets::keypad_field(ui.ctx());
    if open
        .as_deref()
        .is_some_and(|f| f == MASK_MIN || f == MASK_MAX)
    {
        ui.add_space(4.0);
        widgets::keypad(ui, controls, KEYPAD);
    }
    if let (Some(min), Some(max)) = *range
        && min >= max
    {
        widgets::form_note(ui, BAD_MASK_RANGE, 20.0);
    }
    *range != before
}

/// A value of a raster mask with no needless decimals.
fn value_text(v: f64) -> String {
    let text = format!("{v:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
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
    let moved = if widgets::button_row(ui, controls, MOVE_UP, i > 0).clicked() {
        Some(true)
    } else if widgets::button_row(ui, controls, MOVE_DOWN, i + 1 < count).clicked() {
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
    if widgets::button_row_in(ui, controls, DELETE_RULE, true, theme::WARN).clicked()
        && let Some(entry) = state.doc.entry_mut(id)
    {
        entry.style.style.rules.remove(i);
        state.pad.page = None;
        state.pad.rules = RulesState::default();
        state.touch();
    }
}

/// The filter as conditions, and as text in Code.
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
    let help = ExprHelp::for_layer(state, id);
    let count = expr_help::matches(state, id, &current);
    let none = filter::NO_CONDITION;
    let edit = filter::editor(ui, controls, (id, i), &current, &help, count, none);
    if let Some(text) = edit.text {
        rule.filter = Filter::new(&text);
    }
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
        if color_rows(
            ui,
            state,
            controls,
            (id, i, Part::Stroke),
            open,
            &mut stroke.color,
        ) {
            rule.set_stroke(Some(stroke));
        }
        let width = Number::new(STROKE_WIDTH, stroke.width, WIDTHS.0..=WIDTHS.1)
            .nice()
            .step(0.05)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(width) = width {
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
    if kind == GeometryKind::Point {
        let value = rule.marker_fill_kind().map_or("", FillKind::label);
        if widgets::open_row(ui, controls, FILL, value).clicked() {
            state.pad.page = Some(Page::MarkerFill(i));
            state.pad.rules.picker = None;
        }
        return;
    }

    let mut on = rule.fill().is_some();
    if widgets::switch_row(ui, controls, FILL, FILL, &mut on) {
        let fill = default.fill().unwrap_or(Color::rgb(200, 200, 200));
        rule.set_fill(on.then_some(fill));
    }
    if let Some(mut fill) = rule.fill()
        && color_rows(ui, state, controls, (id, i, Part::Fill), open, &mut fill)
    {
        rule.set_fill(Some(fill));
    }
    if kind == GeometryKind::Polygon {
        scatter_rows(ui, state, controls, (id, i), rule);
    }
}

/// The choice of a shape or an SVG symbol at each point. A shape has the
/// row of its shape page, and its size. An SVG symbol has the row of its symbol, its size, its rotation,
/// the fields of the size and the rotation, and its tint. A tap on "SVG
/// symbol" opens the symbol page, and the shape stays until the user picks
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
        widgets::choice_row(ui, controls, MARKER, &[SHAPE, SVG_SYMBOL], Some(active))
    {
        if index == 1 {
            open_symbols(state, i);
        } else {
            state.pad.rules.picker = None;
            to_circle(rule);
        }
    }
    let Some(before) = rule.symbol() else {
        let shape = rule.marker_shape().map_or("", MarkerShape::label);
        if widgets::open_row(ui, controls, MARKER_SHAPE, shape).clicked() {
            state.pad.page = Some(Page::MarkerShape(i));
            state.pad.rules.picker = None;
        }
        if let Some(radius) = marker_radius(rule) {
            let size = 2.0 * radius.fallback;
            let size = Number::new(SIZE, size, CIRCLE_SIZES.0..=CIRCLE_SIZES.1)
                .nice()
                .step(0.05)
                .unit("pt")
                .stepper(ui, controls);
            if let Some(size) = size {
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
    let size = Number::new(SIZE, s.size.fallback, POINT_SIZES.0..=POINT_SIZES.1)
        .nice()
        .step(0.05)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(size) = size {
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

/// The radius of the marker of a rule.
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
    let size = Number::new(SIZE, s.size.fallback, SCATTER_SIZES.0..=SCATTER_SIZES.1)
        .nice()
        .step(0.05)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(size) = size {
        s.size.fallback = size;
    }
    let size = s.size.fallback;
    s.spacing.fallback = s.spacing.fallback.max(size);
    let spacing = Number::new(SPACING, s.spacing.fallback, size..=MAX_SPACING.max(size))
        .step(1.0)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(spacing) = spacing {
        s.spacing.fallback = spacing.max(size);
    }
    let jitter = Number::new(SIZE_VARIATION, s.size_jitter, 0.0..=0.5)
        .percent()
        .slider(ui, controls);
    if let Some(jitter) = jitter {
        s.size_jitter = jitter;
    }
    let jitter = Number::new(ROTATION_VARIATION, s.rotation_jitter, 0.0..=180.0)
        .step(1.0)
        .unit("\u{b0}")
        .slider(ui, controls);
    if let Some(jitter) = jitter {
        s.rotation_jitter = jitter;
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

/// The shape page of rule `i`: a row for each shape, with the row of the
/// current shape on.
pub fn shape_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
    i: usize,
) {
    let Some(current) = rule_at(state, id, i).and_then(Rule::marker_shape) else {
        return;
    };
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(8.0);
    for shape in MarkerShape::ALL {
        let name = shape.label();
        if widgets::option_row(ui, controls, name, name, &[], shape == current).clicked() {
            edit_rule(state, id, i, |r| r.set_marker_shape(shape));
        }
    }
}

/// The fill page of the marker of rule `i`: a row for each fill kind, the
/// color, and for a hatch the angle, the spacing, and the line width. The
/// lines of a hatch have the color of the fill.
pub fn marker_fill_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
    i: usize,
) {
    let Some(rule) = rule_at(state, id, i) else {
        return;
    };
    let (Some(kind), fill, hatch) = (rule.marker_fill_kind(), rule.fill(), rule.marker_hatch())
    else {
        return;
    };
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(8.0);
    if let Some(kind) = fill_kind_rows(ui, controls, kind) {
        state.pad.rules.picker = None;
        edit_rule(state, id, i, |r| r.set_marker_fill_kind(kind));
    }
    let Some(fill) = fill else {
        return;
    };
    ui.add_space(6.0);
    let open = state.pad.rules.picker.filter(|p| p.0 == id && p.1 == i);
    let mut picked = fill;
    if color_rows(
        ui,
        state,
        controls,
        (id, i, Part::Marker(hatch.is_some())),
        open,
        &mut picked,
    ) {
        edit_rule(state, id, i, |rule| rule.set_fill(Some(picked)));
    }
    let Some(mut h) = hatch else {
        return;
    };
    let angle = Number::new(ANGLE, h.angle, Hatch::ANGLES)
        .step(5.0)
        .unit("°")
        .stepper(ui, controls);
    let spacing = Number::new(SPACING, h.spacing, Hatch::SPACINGS)
        .step(0.25)
        .unit("pt")
        .stepper(ui, controls);
    let width = Number::new(LINE_WIDTH, h.line_width, Hatch::LINE_WIDTHS)
        .step(0.05)
        .unit("pt")
        .stepper(ui, controls);
    if angle.is_none() && spacing.is_none() && width.is_none() {
        return;
    }
    h.angle = angle.unwrap_or(h.angle);
    h.spacing = spacing.unwrap_or(h.spacing);
    h.line_width = width.unwrap_or(h.line_width);
    edit_rule(state, id, i, |r| r.set_marker_hatch(Some(h)));
}

/// Rule `i` of the style of layer `id`.
fn rule_at(state: &AppState, id: LayerId, i: usize) -> Option<&Rule> {
    state.doc.entry(id)?.style.style.rules.get(i)
}

/// Changes rule `i` of the style of layer `id`, and marks the document as
/// changed if the rule changed.
fn edit_rule(state: &mut AppState, id: LayerId, i: usize, edit: impl FnOnce(&mut Rule)) {
    let Some(rule) = state
        .doc
        .entry_mut(id)
        .and_then(|e| e.style.style.rules.get_mut(i))
    else {
        return;
    };
    let before = rule.clone();
    edit(rule);
    if *rule != before {
        state.touch();
    }
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
        color_rows(
            ui,
            state,
            controls,
            (id, i, Part::Tint),
            open,
            &mut t.fallback,
        );
    }
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

/// Replaces the SVG symbol of a rule with the marker of a new point layer.
/// A tint becomes the fill of the marker.
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
            let points = |name, value| {
                Number::new(name, value, DASHES.0..=DASHES.1)
                    .nice()
                    .step(0.05)
                    .unit("pt")
            };
            if let Some(dash) = points(DASH, dash).stepper(ui, controls) {
                stroke.kind = StrokeKind::Dashed { dash, gap };
                rule.set_stroke(Some(stroke));
            }
            if let Some(gap) = points(GAP, gap).stepper(ui, controls) {
                stroke.kind = StrokeKind::Dashed { dash, gap };
                rule.set_stroke(Some(stroke));
            }
        }
        StrokeKind::Taper { start } => {
            let start = Number::new(TAPER_START, start, 0.0..=1.0)
                .percent()
                .slider(ui, controls);
            if let Some(start) = start {
                stroke.kind = StrokeKind::Taper { start };
                rule.set_stroke(Some(stroke));
            }
        }
        StrokeKind::Solid | StrokeKind::Brush => {}
    }
}

/// The row of a color and, while it is open, its color picker with an
/// opacity strip. Returns true if the user changed the color.
fn color_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    this: (LayerId, usize, Part),
    open: Option<(LayerId, usize, Part)>,
    color: &mut Color,
) -> bool {
    let name = match this.2 {
        Part::Stroke => STROKE_COLOR,
        Part::Fill => FILL_COLOR,
        Part::Tint => TINT_COLOR,
        Part::Marker(false) => COLOR,
        Part::Marker(true) => LINE_COLOR,
    };
    let is_open = open == Some(this);
    let mut rgba = [color.r, color.g, color.b, color.a];
    if widgets::color_row(ui, controls, name, rgba, is_open).clicked() {
        state.pad.rules.picker = (!is_open).then_some(this);
        // The keypad of a scale field or of the other picker closes.
        widgets::close_keypad(ui.ctx());
    }
    if state.pad.rules.picker != Some(this) {
        return false;
    }
    state.pad.rules.shown = true;
    ui.add_space(4.0);
    let changed = widgets::color_picker_alpha(ui, controls, name, &mut rgba);
    let [r, g, b, a] = rgba;
    *color = Color { r, g, b, a };
    changed
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
    }
}
