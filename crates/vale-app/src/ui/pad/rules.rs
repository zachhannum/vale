//! The Style section of the panel of a vector layer: the list of the rules,
//! and the page of a rule with its filter, its scale range, its stroke, its
//! fill, its point symbol, and its scatter, the page of the symbols, and the
//! page of the mask.

use eframe::egui;
use vale_render::Symbols;
use vale_store::{Field, FieldType, FillKind, GeometryKind, LayerId};
use vale_style::expr::{Filter, Property, field_ref};
use vale_style::{
    Color, DashPattern, DashPreset, Hatch, LayerStyle, MarkerShape, Mask, PointSymbolizer, Rule,
    ScatterSymbolizer, Stroke, StrokeKind, Symbolizer,
};

use super::classify::CLASSIFY;
use super::furniture::{ANGLE, LINE_COLOR, LINE_WIDTH, fill_kind_rows};
use super::geometry::TOUCH;
use super::number::Number;
use super::raster::{self, heading};
use super::widgets::{self, Controls, Keys, PANEL_ROW};
use super::{Icon, Page, filter, list, theme};
use crate::document::Document;
use crate::globe::symbol::paint_symbol;
use crate::rule_edit::{self, Change};
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
pub const REFERENCE_SCALE: &str = "Reference scale";
pub const USE_CURRENT_SCALE: &str = "Use current scale";
/// The text below the scale range when the minimum is not below the maximum.
pub const BAD_RANGE: &str = "Min scale must be less than max scale.";
pub const STROKE: &str = "Stroke";
pub const STROKE_COLOR: &str = "Stroke color";
pub const STROKE_WIDTH: &str = "Width";
/// The switch that gives a line its width from an expression.
pub const WIDTH_FROM_EXPRESSION: &str = "Width from expression";
/// The field of the width expression of a line.
pub const WIDTH_EXPRESSION: &str = "Width expression";
pub const LINE: &str = "Line";
pub const SOLID: &str = "Solid";
pub const DASHED: &str = "Dashed";
pub const BRUSH: &str = "Brush";
pub const TAPER: &str = "Taper";
/// The row of the dash pattern, which opens its page.
pub const PATTERN: &str = "Pattern";
pub const DOTTED: &str = "Dotted";
pub const DASH_DOT: &str = "Dash-dot";
pub const DASH_DOT_DOT: &str = "Dash-dot-dot";
/// The value of the Pattern row when the lengths match no preset.
pub const CUSTOM: &str = "Custom";
/// The field of the dash and gap lengths of a pattern.
pub const LENGTHS: &str = "Lengths";
/// The ends of the dashes.
pub const ENDS: &str = "Ends";
pub const FLAT: &str = "Flat";
pub const ROUND: &str = "Round";
pub const TAPER_START: &str = "Start width";
/// The line type whose width follows the drainage of the height layer.
pub const FLOW: &str = "Flow";
/// The width of a Flow line with no flow, as a part of the width.
pub const FLOW_MIN: &str = "Smallest width";
/// The hint of a Flow line in a project with no height layer.
pub const FLOW_NEEDS_HEIGHT: &str = "Flow needs a height layer.";
/// The heading of the height layers that the Flow lines of a layer can read.
pub const HEIGHT_LAYER: &str = "Height layer";
pub const FILL: &str = "Fill";
pub const FILL_COLOR: &str = "Fill color";
/// The label of the switch of a fill or a stroke that follows the mask of the style.
pub const APPLY_MASK: &str = "Apply mask";
/// The names of the "Apply mask" switches of the stroke and of the fill.
pub const STROKE_MASK: &str = "Stroke apply mask";
pub const FILL_MASK: &str = "Fill apply mask";
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
/// The buttons of the desktop editor that move a rule.
pub const MOVE_UP: &str = "Move rule up";
pub const MOVE_DOWN: &str = "Move rule down";
pub const DELETE_RULE: &str = "Delete rule";
pub const EDIT_RULES: &str = "Edit rules";
pub const DELETE_RULES: &str = "Delete rules";
/// The name field of a rule that the user renames in the list.
pub const RULE_NAME: &str = "Rule name";
/// The row of the mask of a layer, and the title of its page.
pub const MASK: &str = "Mask";
/// The row of the mask page and the text of the Mask row with no mask.
pub const NO_MASK: &str = "None";
pub const RASTER_LAYERS: &str = "Raster layers";
pub const POLYGON_LAYERS: &str = "Polygon layers";
pub const POINT_LAYERS: &str = "Point layers";
pub const MASK_MIN: &str = "Minimum";
pub const MASK_MAX: &str = "Maximum";
pub const MASK_OFFSET: &str = "Offset";
/// The limits of the offset of a raster mask, in points.
pub const MASK_OFFSETS: (f64, f64) = (-20.0, 20.0);
/// The stepper of the gap of a markers mask.
pub const MASK_GAP: &str = "Gap";
/// The text below the range of a raster mask when the minimum is not below the maximum.
pub const BAD_MASK_RANGE: &str = "Minimum must be less than maximum.";
/// The hint of the mask page.
pub const MASK_HINT: &str = "A fill or a stroke with Apply mask on draws only inside the mask. A negative offset clears a line at its edge.";
/// The hint of the mask page with a markers mask.
pub const MARKERS_HINT: &str = "Masked fills and strokes stop short of each marker.";
/// The range of the width slider, in points. The slider has a log scale.
pub const WIDTHS: (f64, f64) = (0.1, 20.0);
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

/// The index of the color picker of the page of the selected rules.
const SELECTION: usize = usize::MAX;

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
    /// The list of the rules. A key is a layer and the index of a rule.
    pub list: list::ListState<(LayerId, usize)>,
}

impl RulesState {
    /// The indices of the selected rules of layer `id`, first rule first.
    pub fn picked(&self, id: LayerId) -> Vec<usize> {
        let picked = self.list.picked.iter();
        picked.filter(|k| k.0 == id).map(|k| k.1).collect()
    }

    /// Selects the rules `picked` of layer `id`.
    pub fn pick(&mut self, id: LayerId, picked: &[usize]) {
        self.list.picked = picked.iter().map(|i| (id, *i)).collect();
    }
}

/// The title of the page of the selected rules of layer `id`.
pub fn selection_title(state: &AppState, id: LayerId) -> String {
    match state.pad.rules.picked(id).len() {
        1 => "1 rule".to_owned(),
        n => format!("{n} rules"),
    }
}

/// The Style section: a row for each rule, first rule first, and the row
/// that adds a rule. A later rule draws on top. A tap on a rule opens its
/// page, and a tap on a selected rule opens the page of the selected rules.
/// [`selection_buttons`] gives the actions of the selected rules. `current`
/// is the scale denominator of the view, for the reference scale.
pub fn section(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
    current: f64,
) {
    let Some(kind) = state.doc.layer(id).and_then(|l| l.geometry()) else {
        return;
    };
    let Some(entry) = state.doc.entry(id) else {
        return;
    };
    let names: Vec<String> = entry
        .style
        .style
        .rules
        .iter()
        .map(|r| r.name.clone())
        .collect();
    let filters: Vec<String> = (entry.style.style.rules.iter())
        .map(|r| row_filter(&r.filter))
        .collect();
    let rows: Vec<list::Row<(LayerId, usize)>> = (names.iter().enumerate())
        .map(|(i, name)| list::Row {
            rename: true,
            delete: true,
            trailing: TOUCH,
            ..list::Row::new((id, i), name.clone())
        })
        .collect();

    heading(ui, STYLE);
    let renamed = state.pad.rules.list.rename.as_ref().map(|r| r.0.1);
    let valid = |name: &str| rule_name_free(&names, renamed, name);
    let spec = list::Spec {
        select: true,
        reorder: true,
        field: RULE_NAME,
        valid: &valid,
        ..list::Spec::default()
    };
    let before = state.pad.rules.list.picked.clone();
    let rules = &mut state.pad.rules.list;
    let look = |i: usize| list::Look {
        text: &names[i],
        note: Some(&filters[i]),
        open: true,
        ..list::Look::default()
    };
    let events = list::show(ui, controls, rules, spec, &rows, |ui, controls, cell, i| {
        list::draw(ui, controls, cell, &look(i))
    });
    let picked = &state.pad.rules.list.picked;
    if *picked != before && !picked.is_empty() {
        state.pad.field_list.picked.clear();
    }
    for event in events {
        match event {
            list::Event::Tap(key) | list::Event::Open(key) => {
                let on = state.pad.rules.list.is_picked(key);
                state.pad.page = Some(if on { Page::Rules } else { Page::Rule(key.1) });
            }
            list::Event::Rename((_, i), name) => {
                state.rename_rule(id, i, &name);
            }
            list::Event::Delete((_, i)) => remove_rules(state, id, &[i]),
            list::Event::Move { from, to, .. } => {
                if let Some(entry) = state.doc.entry_mut(id) {
                    entry.style.style.move_rule_to(from, to);
                    state.touch();
                }
            }
            list::Event::Control(..) | list::Event::Lift(_) | list::Event::Action(..) => {}
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
    reference_scale_rows(ui, state, controls, id, current);
}

/// The field of the reference scale, its keypad, and the button that sets
/// it to the scale of the view.
fn reference_scale_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
    current: f64,
) {
    let Some(value) = state.doc.entry(id).map(|e| e.style.reference_scale) else {
        return;
    };
    let none = |text: &str| text.trim().is_empty() || text == NO_LIMIT;
    let valid = |text: &str| none(text) || parse_scale(text).is_some();
    let shown = value.map_or(NO_LIMIT.to_owned(), scale_text);
    let name = REFERENCE_SCALE;
    let typed = widgets::key_field(ui, controls, name, name, "", &shown, Keys::Number, valid);
    let mut set = typed.map(|text| {
        if none(&text) {
            None
        } else {
            parse_scale(&text)
        }
    });
    let open = widgets::keypad_field(ui.ctx()).is_some_and(|f| f == REFERENCE_SCALE);
    if open {
        ui.add_space(4.0);
        widgets::keypad(ui, controls, KEYPAD);
    }
    let can_use = current.is_finite() && current > 0.0;
    if widgets::button_row(ui, controls, USE_CURRENT_SCALE, can_use).clicked() && can_use {
        set = Some(Some(current.round()));
    }
    if let Some(new) = set.filter(|new| *new != value)
        && let Some(entry) = state.doc.entry_mut(id)
    {
        entry.style.reference_scale = new;
        state.touch();
    }
}

/// True if `name` can be the name of rule `renamed` of a style with the
/// rule names `names`: it is not blank, and no other rule has it.
pub fn rule_name_free(names: &[String], renamed: Option<usize>, name: &str) -> bool {
    let name = name.trim();
    let other = |(i, n): (usize, &String)| Some(i) != renamed && n == name;
    !name.is_empty() && !names.iter().enumerate().any(other)
}

/// Removes the rules `picked` of layer `id`, in one undo step.
fn remove_rules(state: &mut AppState, id: LayerId, picked: &[usize]) {
    if let Some(entry) = state.doc.entry_mut(id) {
        let mut picked = picked.to_vec();
        picked.sort_unstable();
        picked.dedup();
        for &i in picked.iter().rev() {
            if i < entry.style.style.rules.len() {
                entry.style.style.rules.remove(i);
            }
        }
        state.pad.rules = RulesState::default();
        state.touch();
    }
}

/// The buttons of the selected rules of layer `id`, which has `count`
/// rules, in the back row of the panel of the layer: "Select all" while a
/// rule is not selected, "Edit rules", "Delete rules", and "Deselect all".
/// `None`: no rule is selected.
pub fn selection_buttons(
    state: &AppState,
    id: LayerId,
    count: usize,
) -> Option<Vec<(&'static str, Icon)>> {
    let picked = state.pad.rules.picked(id).len();
    let extras = [(EDIT_RULES, Icon::Brush)];
    list::group_buttons(picked, count, &extras, Some(DELETE_RULES))
}

/// Does the action of the button `name` of [`selection_buttons`].
pub fn selection_action(state: &mut AppState, id: LayerId, name: &'static str) {
    let count = state.doc.entry(id).map_or(0, |e| e.style.style.rules.len());
    let keys: Vec<(LayerId, usize)> = (0..count).map(|i| (id, i)).collect();
    let rules = &mut state.pad.rules.list;
    match list::group_action(rules, &keys, name, Some(DELETE_RULES)) {
        Some(list::Group::Delete(picked)) => {
            let picked: Vec<usize> = picked.iter().map(|k| k.1).collect();
            remove_rules(state, id, &picked);
        }
        Some(list::Group::Extra(EDIT_RULES)) => state.pad.page = Some(Page::Rules),
        _ => {}
    }
}

/// The page of the selected rules: the scale range, the stroke, the fill,
/// and the size of a marker. A change sets one property in each selected
/// rule, and the other properties of each rule stay. A value that is
/// different in the selected rules shows "Mixed".
pub fn selection_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
) {
    let Some(kind) = state.doc.layer(id).and_then(|l| l.geometry()) else {
        return;
    };
    let Some(rules) = state.doc.entry(id).map(|e| e.style.style.rules.clone()) else {
        return;
    };
    let masked = has_mask(state, id);
    let picked = state.pad.rules.picked(id);
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut changes = Vec::new();
    let shared = |get: &dyn Fn(&Rule) -> Option<f64>| rule_edit::shared(&rules, &picked, get);

    for (name, min) in [(MIN_SCALE, true), (MAX_SCALE, false)] {
        let value = |r: &Rule| Some(if min { r.min_scale } else { r.max_scale });
        let shown = match rule_edit::shared(&rules, &picked, value) {
            Some((_, true)) => crate::ui::select::MIXED.to_owned(),
            Some((Some(v), false)) => scale_text(v),
            _ => NO_LIMIT.to_owned(),
        };
        let none = |text: &str| text.trim().is_empty() || text == NO_LIMIT;
        let valid = |text: &str| none(text) || parse_scale(text).is_some();
        let typed = widgets::key_field(ui, controls, name, name, "", &shown, Keys::Number, valid);
        if let Some(text) = typed {
            let v = if none(&text) {
                None
            } else {
                parse_scale(&text)
            };
            if none(&text) || v.is_some() {
                changes.push(if min {
                    Change::MinScale(v)
                } else {
                    Change::MaxScale(v)
                });
            }
        }
    }
    let open = widgets::keypad_field(ui.ctx());
    if open
        .as_deref()
        .is_some_and(|f| f == MIN_SCALE || f == MAX_SCALE)
    {
        state.pad.rules.picker = None;
        ui.add_space(4.0);
        widgets::keypad(ui, controls, KEYPAD);
    }
    ui.add_space(8.0);

    // An SVG symbol has no stroke and no fill.
    let paints: Vec<usize> = (picked.iter().copied())
        .filter(|&i| rules[i].symbol().is_none())
        .collect();
    let strokes = rule_edit::shared(&rules, &paints, Rule::stroke);
    if !paints.is_empty() {
        let mut on = paints.iter().all(|&i| rules[i].stroke().is_some());
        if widgets::switch_row(ui, controls, STROKE, STROKE, &mut on) {
            let default = LayerStyle::default_for(kind, 0, &[]).stroke();
            let stroke = default.unwrap_or(Stroke::solid(Color::rgb(90, 90, 90), 1.0));
            changes.push(Change::Stroke(on.then_some(stroke)));
        }
    }
    if strokes.is_some() {
        let color = rule_edit::shared(&rules, &paints, |r| r.stroke().map(|s| s.color));
        if let Some(c) = selection_color(ui, state, controls, (id, Part::Stroke), color) {
            changes.push(Change::StrokeColor(c));
        }
        let (width, mixed) = shared(&|r| r.stroke().map(|s| s.width)).unwrap_or((1.0, false));
        let width = Number::new(STROKE_WIDTH, width, WIDTHS.0..=WIDTHS.1)
            .nice()
            .step(0.05)
            .unit("pt")
            .mixed(mixed)
            .stepper(ui, controls);
        if let Some(w) = width {
            changes.push(Change::StrokeWidth(w));
        }
        // A marker ignores the line type.
        if kind != GeometryKind::Point
            && selection_line_rows(ui, controls, kind, &rules, &paints, &mut changes)
        {
            state.pad.page = Some(Page::DashPattern(None));
            state.pad.rules.picker = None;
        }
        let flow = |i: &usize| {
            matches!(
                rules[*i].stroke().map(|s| s.kind),
                Some(StrokeKind::Flow { .. })
            )
        };
        if kind == GeometryKind::Line && paints.iter().any(flow) {
            flow_height_row(ui, state, controls, (id, None));
        }
        let value = rule_edit::shared(&rules, &paints, Rule::stroke_applies_mask);
        if let Some(v) = mask_row(ui, controls, masked, STROKE_MASK, value) {
            changes.push(Change::StrokeAppliesMask(v));
        }
    }
    if kind == GeometryKind::Polygon {
        let mut on = paints.iter().all(|&i| rules[i].fill().is_some());
        if !paints.is_empty() && widgets::switch_row(ui, controls, FILL, FILL, &mut on) {
            let default = LayerStyle::default_for(kind, 0, &[]).fill();
            let fill = default.unwrap_or(Color::rgb(200, 200, 200));
            changes.push(Change::Fill(on.then_some(fill)));
        }
    }
    let fill = rule_edit::shared(&rules, &paints, Rule::fill);
    if kind != GeometryKind::Line && fill.is_some() {
        let part = if kind == GeometryKind::Point {
            Part::Marker(false)
        } else {
            Part::Fill
        };
        if let Some(c) = selection_color(ui, state, controls, (id, part), fill) {
            changes.push(Change::FillColor(c));
        }
        let value = rule_edit::shared(&rules, &paints, Rule::fill_applies_mask);
        if let Some(v) = mask_row(ui, controls, masked, FILL_MASK, value) {
            changes.push(Change::FillAppliesMask(v));
        }
    }
    if kind == GeometryKind::Point
        && let Some((size, mixed)) = shared(&rule_edit::size)
    {
        let symbols = picked.iter().any(|&i| rules[i].symbol().is_some());
        let range = if symbols { POINT_SIZES } else { CIRCLE_SIZES };
        let size = Number::new(SIZE, size, range.0..=range.1)
            .nice()
            .step(0.05)
            .unit("pt")
            .mixed(mixed)
            .stepper(ui, controls);
        if let Some(size) = size {
            changes.push(Change::Size(size));
        }
    }

    let Some(entry) = state.doc.entry_mut(id) else {
        return;
    };
    let mut changed = false;
    for change in changes {
        changed |= rule_edit::apply(&mut entry.style.style.rules, &picked, kind, change);
    }
    if changed {
        state.touch();
    }
}

/// The line type of the selected strokes, with the dash and the gap when
/// each stroke is dashed, or the start width when each stroke tapers.
fn selection_line_rows(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    kind: GeometryKind,
    rules: &[Rule],
    paints: &[usize],
    changes: &mut Vec<Change>,
) -> bool {
    let mut open = false;
    let mut kinds = vec![SOLID, DASHED, BRUSH];
    if kind == GeometryKind::Line {
        kinds.extend([TAPER, FLOW]);
    }
    let index = |k: &StrokeKind| match k {
        StrokeKind::Solid => 0,
        StrokeKind::Dashed { .. } => 1,
        StrokeKind::Brush => 2,
        StrokeKind::Taper { .. } => 3,
        StrokeKind::Flow { .. } => 4,
    };
    let line = rule_edit::shared(rules, paints, |r| r.stroke().map(|s| index(&s.kind)));
    let active = line.filter(|l| !l.1).map(|l| l.0);
    if let Some(picked) = widgets::choice_row(ui, controls, LINE, &kinds, active)
        && Some(picked) != active
    {
        changes.push(Change::LineKind(match picked {
            1 => StrokeKind::dashed(),
            2 => StrokeKind::Brush,
            3 => StrokeKind::taper(),
            4 => StrokeKind::flow(),
            _ => StrokeKind::Solid,
        }));
    }
    let number = |get: &dyn Fn(StrokeKind) -> Option<f64>| {
        rule_edit::shared(rules, paints, |r| r.stroke().and_then(|s| get(s.kind)))
    };
    match active {
        Some(1) => {
            let pattern = |r: &Rule| match r.stroke().map(|s| s.kind) {
                Some(StrokeKind::Dashed(p)) => Some(p),
                _ => None,
            };
            let name = rule_edit::shared(rules, paints, |r| pattern(r).map(pattern_name));
            let name = match name {
                Some((_, true)) => crate::ui::select::MIXED,
                Some((name, false)) => name,
                None => "",
            };
            open = widgets::open_row(ui, controls, PATTERN, name).clicked();
            let text = rule_edit::shared(rules, paints, |r| pattern(r).map(|p| p.text()));
            let text = text.filter(|t| !t.1).map(|t| t.0).unwrap_or_default();
            if let Some(q) = lengths_field(ui, controls, text) {
                changes.push(Change::DashLengths(q));
            }
            let round = rule_edit::shared(rules, paints, |r| pattern(r).map(|p| p.round));
            let active = round.filter(|r| !r.1).map(|r| usize::from(r.0));
            if let Some(picked) = widgets::choice_row(ui, controls, ENDS, &[FLAT, ROUND], active)
                && Some(picked) != active
            {
                changes.push(Change::DashRound(picked == 1));
            }
        }
        Some(3) => {
            let start = number(&|k| match k {
                StrokeKind::Taper { start } => Some(start),
                _ => None,
            });
            let (v, mixed) = start.unwrap_or((StrokeKind::TAPER_START, false));
            let v = Number::new(TAPER_START, v, 0.0..=1.0)
                .percent()
                .mixed(mixed)
                .slider(ui, controls);
            if let Some(v) = v {
                changes.push(Change::TaperStart(v));
            }
        }
        Some(4) => {
            let min = number(&|k| match k {
                StrokeKind::Flow { min } => Some(min),
                _ => None,
            });
            let (v, mixed) = min.unwrap_or((StrokeKind::FLOW_MIN, false));
            let v = Number::new(FLOW_MIN, v, 0.0..=1.0)
                .percent()
                .mixed(mixed)
                .slider(ui, controls);
            if let Some(v) = v {
                changes.push(Change::FlowMin(v));
            }
        }
        _ => {}
    }
    open
}

/// The row of a color of the selected rules and, while it is open, its
/// color picker. `color`: the color of the first rule, and true if the
/// rules have different colors. Returns the color that the user picked.
fn selection_color(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (id, part): (LayerId, Part),
    color: Option<(Color, bool)>,
) -> Option<Color> {
    let (color, mixed) = color?;
    let this = (id, SELECTION, part);
    let name = match part {
        Part::Stroke => STROKE_COLOR,
        Part::Fill => FILL_COLOR,
        _ => COLOR,
    };
    let is_open = state.pad.rules.picker == Some(this);
    let mut rgba = [color.r, color.g, color.b, color.a];
    let row = if mixed {
        widgets::mixed_color_row(ui, controls, name, is_open)
    } else {
        widgets::color_row(ui, controls, name, rgba, is_open)
    };
    if row.clicked() {
        state.pad.rules.picker = (!is_open).then_some(this);
        widgets::close_keypad(ui.ctx());
    }
    if state.pad.rules.picker != Some(this) {
        return None;
    }
    state.pad.rules.shown = true;
    ui.add_space(4.0);
    let changed = widgets::color_picker_alpha(ui, controls, name, &mut rgba);
    let [r, g, b, a] = rgba;
    changed.then_some(Color { r, g, b, a })
}

/// True if the style of layer `id` has a mask.
fn has_mask(state: &AppState, id: LayerId) -> bool {
    state
        .doc
        .entry(id)
        .is_some_and(|e| e.style.style.mask.is_some())
}

/// The "Apply mask" switch of a fill or a stroke, when the style has a mask
/// and the rules have a `value`. A mixed value shows off, and a tap turns
/// it on. Returns the new value after a tap.
fn mask_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    masked: bool,
    name: &str,
    value: Option<(bool, bool)>,
) -> Option<bool> {
    let (on, mixed) = value.filter(|_| masked)?;
    let mut on = on && !mixed;
    widgets::switch_row(ui, controls, name, APPLY_MASK, &mut on).then_some(on)
}

/// The text of the Mask row: the name of the layer of the mask, or "None".
fn mask_text(state: &AppState, id: LayerId) -> String {
    let mask = state.doc.entry(id).and_then(|e| e.style.style.mask);
    mask.and_then(|m| state.doc.layer(m.layer()))
        .map_or(NO_MASK.to_owned(), |l| l.name.clone())
}

/// The layers that can be the mask of layer `id`, in groups.
#[derive(Default)]
pub struct MaskChoices {
    pub rasters: Vec<MaskChoice>,
    pub polygons: Vec<MaskChoice>,
    pub points: Vec<MaskChoice>,
}

impl MaskChoices {
    /// Each choice, group by group.
    pub fn all(self) -> Vec<MaskChoice> {
        let mut all = self.rasters;
        all.extend(self.polygons);
        all.extend(self.points);
        all
    }
}

/// The layers that can be the mask of layer `id`: the raster layers, and the polygon layers and the point layers other than `id`, each with its name and the mask that a tap on it sets. The mask of a height layer is its land.
pub fn mask_choices(doc: &Document, id: LayerId) -> MaskChoices {
    let mut rasters = Vec::new();
    let mut polygons = Vec::new();
    let mut points = Vec::new();
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
                    offset: 0.0,
                }
            };
            rasters.push((layer.name.clone(), mask));
        } else if layer.geometry() == Some(GeometryKind::Polygon) && layer.id != id {
            polygons.push((layer.name.clone(), Mask::Polygons { layer: layer.id }));
        } else if layer.geometry() == Some(GeometryKind::Point) && layer.id != id {
            points.push((layer.name.clone(), Mask::markers(layer.id)));
        }
    }
    MaskChoices {
        rasters,
        polygons,
        points,
    }
}

/// The name of a layer, and the mask that a tap on it sets.
pub type MaskChoice = (String, Mask);

/// The mask `choice` with the offset of the raster mask or the gap of the markers mask `current`, so a change of the layer keeps the offset or the gap.
pub fn keep_offset(choice: Mask, current: Option<Mask>) -> Mask {
    match (choice, current) {
        (
            Mask::Raster {
                layer, min, max, ..
            },
            Some(Mask::Raster { offset, .. }),
        ) => Mask::Raster {
            layer,
            min,
            max,
            offset,
        },
        (Mask::Markers { layer, .. }, Some(Mask::Markers { gap, .. })) => {
            Mask::Markers { layer, gap }
        }
        _ => choice,
    }
}

/// Sets the mask of layer `id`.
fn set_mask(state: &mut AppState, id: LayerId, mask: Option<Mask>) {
    if let Some(entry) = state.doc.entry_mut(id)
        && entry.style.style.mask != mask
    {
        entry.style.style.mask = mask;
        state.touch();
    }
}

/// The mask page of layer `id`: "None", a row for each raster layer and for each other polygon layer and point layer, the range of a raster mask, and the gap of a markers mask.
pub fn mask_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: LayerId) {
    let Some(entry) = state.doc.entry(id) else {
        return;
    };
    let current = entry.style.style.mask;
    let choices = mask_choices(&state.doc, id);
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(8.0);
    let groups = [
        (None, vec![(NO_MASK.to_owned(), None)]),
        (Some(RASTER_LAYERS), with_masks(choices.rasters)),
        (Some(POLYGON_LAYERS), with_masks(choices.polygons)),
        (Some(POINT_LAYERS), with_masks(choices.points)),
    ];
    let on = |mask: &Option<Mask>| current.map(|m| m.layer()) == mask.map(|m| m.layer());
    for (title, choices) in groups {
        if choices.is_empty() {
            continue;
        }
        if let Some(title) = title {
            heading(ui, title);
        }
        let rows: Vec<list::Row<Option<LayerId>>> = (choices.iter())
            .map(|(name, mask)| list::Row::new(mask.map(|m| m.layer()), name.clone()))
            .collect();
        let mut plain = list::ListState::default();
        let spec = list::Spec::default();
        let events = list::show(
            ui,
            controls,
            &mut plain,
            spec,
            &rows,
            |ui, controls, cell, i| {
                let look = list::Look {
                    text: &choices[i].0,
                    check: Some(on(&choices[i].1)),
                    ..list::Look::default()
                };
                list::draw(ui, controls, cell, &look)
            },
        );
        for event in events {
            if let list::Event::Tap(key) = event
                && let Some((_, mask)) = choices.iter().find(|c| c.1.map(|m| m.layer()) == key)
                && !on(mask)
            {
                set_mask(state, id, mask.map(|m| keep_offset(m, current)));
            }
        }
    }
    if let Some(Mask::Raster {
        layer,
        min,
        max,
        offset,
    }) = current
    {
        let mut range = (min, max);
        heading(ui, "Range");
        if range_rows(ui, controls, &mut range) {
            let mask = Mask::Raster {
                layer,
                min: range.0,
                max: range.1,
                offset,
            };
            set_mask(state, id, Some(mask));
        }
        let (lo, hi) = MASK_OFFSETS;
        let offset = Number::new(MASK_OFFSET, f64::from(offset), lo..=hi)
            .step(0.05)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(offset) = offset {
            let mask = Mask::Raster {
                layer,
                min,
                max,
                offset: offset as f32,
            };
            set_mask(state, id, Some(mask));
        }
    }
    if let Some(Mask::Markers { layer, gap }) = current {
        let (lo, hi) = (*Mask::GAPS.start(), *Mask::GAPS.end());
        let gap = Number::new(MASK_GAP, f64::from(gap), f64::from(lo)..=f64::from(hi))
            .step(0.25)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(gap) = gap {
            let mask = Mask::Markers {
                layer,
                gap: gap as f32,
            };
            set_mask(state, id, Some(mask));
        }
    }
    ui.add_space(8.0);
    let hint = match current {
        Some(Mask::Markers { .. }) => MARKERS_HINT,
        _ => MASK_HINT,
    };
    raster::note(ui, hint);
}

/// The choices of a mask, each with its mask.
fn with_masks(choices: Vec<MaskChoice>) -> Vec<(String, Option<Mask>)> {
    choices
        .into_iter()
        .map(|(name, mask)| (name, Some(mask)))
        .collect()
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

/// The page of rule `i`: the conditions of the filter, the scale range,
/// the stroke, the fill, and the button that deletes the rule. A tap on a color opens a color picker below it.
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
    let masked = has_mask(state, id);
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
        if kind == GeometryKind::Line {
            width_expression_rows(ui, state, controls, id, rule);
        }
        // A marker ignores the line type.
        if kind != GeometryKind::Point {
            let has_height = state.doc.project.rasters().iter().any(crate::ui::is_height);
            if line_rows(ui, controls, rule, stroke, (kind, has_height)) {
                state.pad.page = Some(Page::DashPattern(Some(i)));
                state.pad.rules.picker = None;
            }
            if matches!(rule.stroke().map(|s| s.kind), Some(StrokeKind::Flow { .. })) {
                flow_height_row(ui, state, controls, (id, Some(i)));
            }
        }
        let value = rule.stroke_applies_mask().map(|v| (v, false));
        if let Some(v) = mask_row(ui, controls, masked, STROKE_MASK, value) {
            rule.set_stroke_applies_mask(v);
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
    let value = rule.fill_applies_mask().map(|v| (v, false));
    if let Some(v) = mask_row(ui, controls, masked, FILL_MASK, value) {
        rule.set_fill_applies_mask(v);
    }
    if kind == GeometryKind::Polygon {
        scatter_rows(ui, state, controls, (id, i), rule);
    }
}

/// The switch and the field of the width expression of a line. The width
/// of the stepper is the width where the expression gives no value.
fn width_expression_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
    rule: &mut Rule,
) {
    let help = ExprHelp::for_layer(state, id);
    let text = rule.stroke_width_expression().map(str::to_owned);
    let mut on = text.is_some();
    if widgets::switch_row(
        ui,
        controls,
        WIDTH_FROM_EXPRESSION,
        WIDTH_FROM_EXPRESSION,
        &mut on,
    ) {
        let start = rule
            .stroke()
            .map(|s| s.width.to_string())
            .unwrap_or_default();
        rule.set_stroke_width_expression(on.then_some(start.as_str()));
    }
    let Some(text) = rule.stroke_width_expression().map(str::to_owned) else {
        return;
    };
    let range = expr_help::range(state, id, &text);
    let edit = super::value::editor(
        ui,
        controls,
        WIDTH_EXPRESSION,
        &text,
        &help,
        range.as_deref(),
    );
    if let Some(text) = edit.text {
        rule.set_stroke_width_expression(Some(&text));
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
    painter.rect_filled(square, theme::SMALL_RADIUS, PAPER);
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

/// The line type of a stroke, with its dash pattern or its start width.
/// Returns true if the user taps the Pattern row.
fn line_rows(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    rule: &mut Rule,
    mut stroke: Stroke,
    (kind, has_height): (GeometryKind, bool),
) -> bool {
    let mut open = false;
    let mut kinds = vec![SOLID, DASHED, BRUSH];
    if kind == GeometryKind::Line {
        kinds.extend([TAPER, FLOW]);
    }
    let active = match stroke.kind {
        StrokeKind::Solid => 0,
        StrokeKind::Dashed { .. } => 1,
        StrokeKind::Brush => 2,
        StrokeKind::Taper { .. } => 3,
        StrokeKind::Flow { .. } => 4,
    };
    if let Some(index) = widgets::choice_row(ui, controls, LINE, &kinds, Some(active))
        && index != active
    {
        stroke.kind = match index {
            1 => StrokeKind::dashed(),
            2 => StrokeKind::Brush,
            3 => StrokeKind::taper(),
            4 => StrokeKind::flow(),
            _ => StrokeKind::Solid,
        };
        rule.set_stroke(Some(stroke));
    }
    match stroke.kind {
        StrokeKind::Dashed(p) => {
            open = widgets::open_row(ui, controls, PATTERN, pattern_name(p)).clicked();
            let mut set = |q: Option<DashPattern>| {
                if let Some(q) = q {
                    stroke.kind = StrokeKind::Dashed(q);
                    rule.set_stroke(Some(stroke));
                }
            };
            if let Some(q) = lengths_field(ui, controls, p.text()) {
                set(Some(p.with_lengths(&q)));
            }
            let active = usize::from(p.round);
            if let Some(picked) =
                widgets::choice_row(ui, controls, ENDS, &[FLAT, ROUND], Some(active))
                && picked != active
            {
                let mut q = p;
                q.round = picked == 1;
                set(Some(q));
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
        StrokeKind::Flow { min } => {
            let min = Number::new(FLOW_MIN, min, 0.0..=1.0)
                .percent()
                .slider(ui, controls);
            if let Some(min) = min {
                stroke.kind = StrokeKind::Flow { min };
                rule.set_stroke(Some(stroke));
            }
            if !has_height {
                widgets::form_note(ui, FLOW_NEEDS_HEIGHT, 20.0);
            }
        }
        StrokeKind::Solid | StrokeKind::Brush => {}
    }
    open
}

/// The names of the height layers of the project, in the order of the
/// layer list.
pub fn height_layers(doc: &Document) -> Vec<String> {
    let project = &doc.project;
    (project.layers().iter())
        .filter(|l| {
            let raster = l.raster().and_then(|r| project.raster(r));
            raster.is_some_and(crate::ui::is_height)
        })
        .map(|l| l.name.clone())
        .collect()
}

/// The height layer that the Flow lines of layer `id` read: the name in its
/// style, or else the first height layer. `None` with no height layer.
fn flow_height(state: &AppState, id: LayerId) -> Option<String> {
    let heights = height_layers(&state.doc);
    let current = state
        .doc
        .entry(id)
        .and_then(|e| e.style.flow_height.clone());
    current
        .filter(|c| heights.contains(c))
        .or_else(|| heights.first().cloned())
}

/// The row of the height layer of the Flow lines of layer `id`, which opens
/// its page. It shows if the project has a height layer.
/// `rule`: the rule of the page, or `None` for the selected rules.
fn flow_height_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (id, rule): (LayerId, Option<usize>),
) {
    let Some(current) = flow_height(state, id) else {
        return;
    };
    if widgets::open_row(ui, controls, HEIGHT_LAYER, &current).clicked() {
        state.pad.page = Some(Page::FlowHeight(rule));
        state.pad.rules.picker = None;
    }
}

/// The name of the preset of `p`, or Custom.
pub fn pattern_name(p: DashPattern) -> &'static str {
    match p.preset() {
        Some(DashPreset::Dashed) => DASHED,
        Some(DashPreset::Dotted) => DOTTED,
        Some(DashPreset::DashDot) => DASH_DOT,
        Some(DashPreset::DashDotDot) => DASH_DOT_DOT,
        None => CUSTOM,
    }
}

/// The field of the lengths of a dash pattern. `shown` is the text of
/// the lengths, or empty when the selected patterns differ. While the field has the focus, it
/// keeps the typed text. Returns the pattern of the text when the user types a valid text.
fn lengths_field(ui: &mut egui::Ui, controls: &mut Controls, shown: String) -> Option<DashPattern> {
    let id = egui::Id::new("pad-dash-lengths");
    let draft = ui.data(|d| d.get_temp::<String>(id));
    let mut text = draft.clone().unwrap_or(shown);
    let valid = draft.is_none() || DashPattern::parse(&text, false).is_some();
    let field = widgets::text_field(ui, controls, LENGTHS, LENGTHS, None, &mut text, valid);
    let parsed = DashPattern::parse(&text, false).filter(|_| field.changed());
    if field.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    parsed
}

/// The page of the dash pattern of rule `rule` of layer `id`, or of the
/// selected rules: a row for each preset.
pub fn dash_pattern_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
    rule: Option<usize>,
) {
    let Some(kind) = state.doc.layer(id).and_then(|l| l.geometry()) else {
        return;
    };
    let Some(entry) = state.doc.entry(id) else {
        return;
    };
    let rules = &entry.style.style.rules;
    let indices = match rule {
        Some(i) => vec![i],
        None => state.pad.rules.picked(id),
    };
    let current = rule_edit::shared(rules, &indices, |r| match r.stroke().map(|s| s.kind) {
        Some(StrokeKind::Dashed(p)) => Some(p.preset()),
        _ => None,
    });
    let current = current.filter(|c| !c.1).and_then(|c| c.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(8.0);
    for preset in DashPreset::ALL {
        let name = pattern_name(preset.pattern());
        let on = current == Some(preset);
        if widgets::option_row(ui, controls, name, name, &[], on).clicked()
            && !on
            && let Some(entry) = state.doc.entry_mut(id)
        {
            let change = Change::DashPattern(preset.pattern());
            if rule_edit::apply(&mut entry.style.style.rules, &indices, kind, change) {
                state.touch();
            }
        }
    }
}

/// The page of the height layer of the Flow lines of layer `id`: a row for
/// each height layer.
pub fn flow_height_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
) {
    let current = flow_height(state, id);
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(8.0);
    for name in height_layers(&state.doc) {
        let on = current.as_ref() == Some(&name);
        if widgets::option_row(ui, controls, &name, &name, &[], on).clicked()
            && !on
            && let Some(entry) = state.doc.entry_mut(id)
        {
            entry.style.flow_height = Some(name);
            state.touch();
        }
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
