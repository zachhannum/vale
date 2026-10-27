//! The Style section of the panel of a vector layer: the list of the rules,
//! and the page of a rule with its filter, its scale range, its stroke, and
//! its fill.

use eframe::egui;
use vale_store::{GeometryKind, LayerId};
use vale_style::expr::Filter;
use vale_style::{Color, LayerStyle, Rule, Stroke};

use super::raster::heading;
use super::widgets::{self, Controls, Keys, PANEL_ROW};
use super::{Page, filter, theme};
use crate::ui::AppState;

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
pub const FILL: &str = "Fill";
pub const FILL_COLOR: &str = "Fill color";
pub const MOVE_UP: &str = "Move rule up";
pub const MOVE_DOWN: &str = "Move rule down";
pub const DELETE_RULE: &str = "Delete rule";
/// The range of the width slider, in points. The slider has a log scale.
const WIDTHS: (f64, f64) = (0.1, 20.0);
/// The height of the keypad of the scale fields.
const KEYPAD: f32 = 4.0 * PANEL_ROW;
/// The longest filter text that the row of a rule shows, in characters.
const ROW_FILTER: usize = 18;

/// The color of a rule that the color picker edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Stroke,
    Fill,
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
    if widgets::button_row(ui, controls, ADD_RULE, true).clicked()
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

/// The words of the filter syntax that a plain field name cannot be.
const KEYWORDS: [&str; 15] = [
    "and", "or", "not", "in", "is", "null", "true", "false", "like", "between", "case", "when",
    "then", "else", "end",
];

/// A field name as the filter syntax writes it: plain, or in double quotes.
pub fn field_name(name: &str) -> String {
    let mut chars = name.chars();
    let plain = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && !KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(name));
    if plain {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
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

    let mut on = rule.stroke().is_some();
    if widgets::switch_row(ui, controls, STROKE, STROKE, &mut on) {
        let stroke = default.stroke().unwrap_or(Stroke {
            color: Color::rgb(90, 90, 90),
            width: 1.0,
        });
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
        let (lo, hi) = WIDTHS;
        let mut fraction = (stroke.width.clamp(lo, hi) / lo).ln() / (hi / lo).ln();
        let text = format!("{} pt", width_text(stroke.width));
        if widgets::row_slider(
            ui,
            controls,
            STROKE_WIDTH,
            STROKE_WIDTH,
            &mut fraction,
            &text,
        ) {
            // The steps are twentieths of a point.
            let width = lo * (hi / lo).powf(fraction);
            stroke.width = ((width * 20.0).round() / 20.0).max(lo);
            rule.set_stroke(Some(stroke));
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
        assert_eq!(field_name("scalerank"), "scalerank");
        assert_eq!(field_name("name en"), "\"name en\"");
        assert_eq!(field_name("2nd"), "\"2nd\"");
        assert_eq!(field_name("Not"), "\"Not\"");
        assert_eq!(field_name("a\"b"), "\"a\"\"b\"");
        assert_eq!(scale_text(1_000_000.0), "1,000,000");
        assert_eq!(scale_text(500.0), "500");
        assert_eq!(parse_scale("50,000"), Some(50_000.0));
        assert_eq!(parse_scale("0"), None);
        assert_eq!(width_text(1.5), "1.5");
        assert_eq!(width_text(2.0), "2");
    }
}
