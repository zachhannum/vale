//! The Classify form of a vector layer: it replaces the rules of the layer
//! with a rule for each class of a field or an expression.

use eframe::egui;
use vale_store::{Field, FieldType, GeometryKind, LayerId, Value};
use vale_style::LayerStyle;
use vale_style::expr::{Expr, field_ref};

use super::Page;
use super::number::Number;
use super::raster::{heading, note, ramp_row};
use super::rules::{CIRCLE_SIZES, WIDTHS};
use super::value;
use super::widgets::{self, Controls};
use crate::classify::{self, Method, Step};
use crate::ramps::Ramp;
use crate::ui::AppState;
use crate::ui::expr_help::{self, ExprHelp};

/// The title of the form, and the button of the Style section that opens it.
pub const TITLE: &str = "Classify";
pub const CLASSIFY: &str = "Classify…";
/// The heading of the chips of the fields.
pub const FIELD: &str = "Field";
/// The field of the expression that gives the values.
pub const EXPRESSION: &str = "Expression";
pub const METHOD: &str = "Method";
pub const CLASS_COUNT: &str = "Classes";
pub const COLOR_RAMP: &str = "Color ramp";
pub const REVERSE: &str = "Reverse colors";
/// The switches that make the width or the size step from the first class
/// to the last class, and the values of the first and the last class.
pub const VARY_WIDTH: &str = "Vary width";
pub const VARY_SIZE: &str = "Vary size";
pub const FROM: &str = "From";
pub const TO: &str = "To";
pub const REPLACE_RULES: &str = "Replace rules";
/// The switch that gives each polygon a color that no neighbor has, and the
/// number of the colors.
pub const NEIGHBORS_DIFFER: &str = "Neighbors differ";
pub const COLOR_COUNT: &str = "Colors";
/// The note when no feature gives a value.
pub const NO_VALUES: &str = "No values.";

/// The values of the expression for the features of the layer.
#[derive(Clone, Debug)]
enum Values {
    /// Sorted numbers, and the count of the features with no number.
    Numbers(Vec<f64>, usize),
    /// The values, each one time, most common first.
    Categories(Vec<Value>),
    /// The text is not an expression.
    Bad,
}

/// The state of the form.
#[derive(Default)]
pub struct Draft {
    layer: Option<LayerId>,
    text: String,
    method: Option<Method>,
    classes: usize,
    /// The index of the ramp that the user picks in the library. `None`:
    /// the default ramp of the values.
    ramp: Option<usize>,
    /// The index of the ramp that the form shows.
    shown: usize,
    reverse: bool,
    neighbors: bool,
    colors: usize,
    step: bool,
    first: f64,
    last: f64,
    /// The values of `text` at a map generation.
    values: Option<(String, u64, Values)>,
    /// The edges of the numbers for a method and a class count.
    edges: Option<(Method, usize, Vec<f64>)>,
}

impl Draft {
    fn new(id: LayerId, kind: GeometryKind) -> Draft {
        let (first, last) = match kind {
            GeometryKind::Line => (0.5, 4.0),
            GeometryKind::Polygon => (0.3, 2.0),
            GeometryKind::Point => (4.0, 16.0),
        };
        Draft {
            layer: Some(id),
            classes: classify::DEFAULT_CLASSES,
            colors: classify::DEFAULT_COLORS,
            first,
            last,
            ..Draft::default()
        }
    }
}

/// The form: the chips of the fields and the expression, the method and
/// the class count of numbers, the color ramp, the step of the width or
/// the size, and the button that makes the rules.
pub fn body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: LayerId) {
    let Some(layer) = state.doc.layer(id) else {
        return;
    };
    let Some(kind) = layer.geometry() else {
        return;
    };
    state.open_ramps();
    let generation = state.map_generation;
    let mut draft = std::mem::take(&mut state.pad.rules.classify);
    if draft.layer != Some(id) {
        draft = Draft::new(id, kind);
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    let Some(layer) = state.doc.layer(id) else {
        return;
    };

    if kind == GeometryKind::Polygon {
        ui.add_space(8.0);
        widgets::switch_row(
            ui,
            controls,
            NEIGHBORS_DIFFER,
            NEIGHBORS_DIFFER,
            &mut draft.neighbors,
        );
    }
    let neighbors = draft.neighbors && kind == GeometryKind::Polygon;
    if neighbors {
        neighbors_body(ui, state, controls, id, draft);
        return;
    }

    heading(ui, FIELD);
    let refs: Vec<String> = layer.fields.iter().map(|f| field_ref(&f.name)).collect();
    let names: Vec<&str> = layer.fields.iter().map(|f| f.name.as_str()).collect();
    let active = refs
        .iter()
        .position(|r| *r == draft.text.trim())
        .unwrap_or(names.len());
    if let Some(chosen) = widgets::chips(ui, controls, &names, active) {
        draft.text = refs[chosen].clone();
    }
    let help = ExprHelp::for_layer(state, id);
    let range = expr_help::range(state, id, &draft.text);
    let edit = value::editor(
        ui,
        controls,
        EXPRESSION,
        &draft.text,
        &help,
        range.as_deref(),
    );
    if let Some(text) = edit.text {
        draft.text = text;
    }

    let fresh = matches!(&draft.values, Some((t, g, _)) if *t == draft.text && *g == generation);
    if !fresh && !draft.text.trim().is_empty() {
        let values = match Expr::parse(&draft.text) {
            Ok(expr) => {
                let all = classify::values(layer, &expr);
                match classify::numbers(&all) {
                    Some((sorted, nulls)) => Values::Numbers(sorted, nulls),
                    None => Values::Categories(classify::categories(&all)),
                }
            }
            Err(_) => Values::Bad,
        };
        draft.values = Some((draft.text.clone(), generation, values));
        draft.edges = None;
    }
    let values = match &draft.values {
        Some((t, _, v)) if *t == draft.text && !t.trim().is_empty() => Some(v.clone()),
        _ => None,
    };
    let number = matches!(values, Some(Values::Numbers(..)));

    if number {
        heading(ui, METHOD);
        let labels: Vec<&str> = Method::ALL.iter().map(|m| m.label()).collect();
        let method = draft.method.unwrap_or(Method::Quantiles);
        let at = Method::ALL.iter().position(|m| *m == method).unwrap_or(0);
        if let Some(chosen) = widgets::chips(ui, controls, &labels, at) {
            draft.method = Some(Method::ALL[chosen]);
        }
        ui.add_space(8.0);
        let (min, max) = classify::CLASSES;
        let count = Number::new(CLASS_COUNT, draft.classes as f64, min as f64..=max as f64)
            .step(1.0)
            .stepper(ui, controls);
        if let Some(count) = count {
            draft.classes = (count.round() as usize).clamp(min, max);
        }
    }

    let list = state.ramps.list();
    let ramp = draft
        .ramp
        .filter(|r| *r < list.len())
        .unwrap_or_else(|| default_ramp(list, !number));
    draft.shown = ramp;
    heading(ui, COLOR_RAMP);
    if let Some(r) = list.get(ramp)
        && ramp_row(ui, controls, &r.name, &r.colors, false).clicked()
    {
        state.pad.page = Some(Page::Ramps);
    }
    ui.add_space(8.0);
    widgets::switch_row(ui, controls, REVERSE, REVERSE, &mut draft.reverse);

    if number {
        let (switch, names, range) = match kind {
            GeometryKind::Point => (VARY_SIZE, [FROM, TO], CIRCLE_SIZES),
            _ => (VARY_WIDTH, [FROM, TO], WIDTHS),
        };
        widgets::switch_row(ui, controls, switch, switch, &mut draft.step);
        if draft.step {
            for (name, value) in names.into_iter().zip([&mut draft.first, &mut draft.last]) {
                let changed = Number::new(name, *value, range.0..=range.1)
                    .nice()
                    .step(0.05)
                    .unit("pt")
                    .stepper(ui, controls);
                if let Some(v) = changed {
                    *value = v;
                }
            }
        }
    }

    // The classes of the values.
    let left = classify::left_side(&draft.text);
    let classes = match &values {
        Some(Values::Numbers(sorted, nulls)) => {
            let method = draft.method.unwrap_or(Method::Quantiles);
            let edges = match &draft.edges {
                Some((m, n, e)) if *m == method && *n == draft.classes => e.clone(),
                _ => {
                    let e = classify::breaks(sorted, method, draft.classes);
                    draft.edges = Some((method, draft.classes, e.clone()));
                    e
                }
            };
            classify::number_classes(&left, &edges, *nulls > 0)
        }
        Some(Values::Categories(all)) if !all.is_empty() => {
            let ramp = state.ramps.list().get(ramp);
            let common = classify::common(all, classify::MAX_CATEGORIES, ramp);
            classify::category_classes(&left, &common)
        }
        _ => Vec::new(),
    };
    let labeled = state.ramps.list().get(ramp).map_or(0, |r| {
        classes
            .iter()
            .filter(|c| c.value.as_deref().is_some_and(|v| r.labeled(v).is_some()))
            .count()
    });
    ui.add_space(8.0);
    match &values {
        Some(Values::Bad) | None => {}
        Some(_) if classes.is_empty() => note(ui, NO_VALUES),
        Some(_) => note(ui, &summary(classes.len(), labeled)),
    }
    let made = widgets::button_row(ui, controls, REPLACE_RULES, !classes.is_empty()).clicked();
    if made {
        let step = match (draft.step && number, kind) {
            (false, _) => Step::None,
            (true, GeometryKind::Point) => Step::Size(draft.first, draft.last),
            (true, _) => Step::Width(draft.first, draft.last),
        };
        let request = Request {
            text: draft.text.clone(),
            method: draft.method.unwrap_or(Method::Quantiles),
            classes: draft.classes,
            ramp: state.ramps.list()[ramp].clone(),
            reverse: draft.reverse,
            step,
            neighbors: None,
        };
        if apply(state, id, &request) {
            state.pad.page = None;
        }
    }
    state.pad.rules.classify = draft;
}

/// The form when neighbors differ: the count of the colors, the color
/// ramp, and the button that makes the rules.
fn neighbors_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
    mut draft: Draft,
) {
    ui.add_space(8.0);
    let (min, max) = classify::COLORS;
    let count = Number::new(COLOR_COUNT, draft.colors as f64, min as f64..=max as f64)
        .step(1.0)
        .stepper(ui, controls);
    if let Some(count) = count {
        draft.colors = (count.round() as usize).clamp(min, max);
    }
    let list = state.ramps.list();
    let ramp = draft
        .ramp
        .filter(|r| *r < list.len())
        .unwrap_or_else(|| default_ramp(list, true));
    draft.shown = ramp;
    heading(ui, COLOR_RAMP);
    if let Some(r) = list.get(ramp)
        && ramp_row(ui, controls, &r.name, &r.colors, false).clicked()
    {
        state.pad.page = Some(Page::Ramps);
    }
    ui.add_space(8.0);
    widgets::switch_row(ui, controls, REVERSE, REVERSE, &mut draft.reverse);
    ui.add_space(8.0);
    let field = state
        .doc
        .layer(id)
        .map(classify::color_field)
        .unwrap_or_default();
    note(ui, &neighbors_note(&field));
    if widgets::button_row(ui, controls, REPLACE_RULES, true).clicked() {
        let request = Request {
            text: String::new(),
            method: Method::Quantiles,
            classes: draft.classes,
            ramp: state.ramps.list()[ramp].clone(),
            reverse: draft.reverse,
            step: Step::None,
            neighbors: Some(draft.colors),
        };
        if apply(state, id, &request) {
            state.pad.page = None;
        }
    }
    state.pad.rules.classify = draft;
}

/// The note above the button when neighbors differ.
pub fn neighbors_note(field: &str) -> String {
    format!("Saves the colors in the field {field}.")
}

/// A request to replace the rules of a layer with classes.
pub struct Request {
    /// The expression that gives the values.
    pub text: String,
    pub method: Method,
    pub classes: usize,
    pub ramp: Ramp,
    pub reverse: bool,
    pub step: Step,
    /// The count of the colors when neighbors differ. `None`: the classes
    /// come from the values of `text`.
    pub neighbors: Option<usize>,
}

/// The index of the ramp of the Classify form of layer `id`.
pub(super) fn ramp_of(state: &AppState, id: LayerId) -> usize {
    let draft = &state.pad.rules.classify;
    if draft.layer == Some(id) {
        draft.shown
    } else {
        0
    }
}

/// Picks ramp `index` for the Classify form of layer `id`.
pub(super) fn use_ramp(state: &mut AppState, id: LayerId, index: usize) {
    let draft = &mut state.pad.rules.classify;
    if draft.layer == Some(id) {
        draft.ramp = Some(index);
    }
}

/// Keeps the ramp of the Classify form when the library deletes ramp
/// `index`.
pub(super) fn ramp_removed(state: &mut AppState, index: usize) {
    let draft = &mut state.pad.rules.classify;
    draft.ramp = match draft.ramp {
        Some(r) if r == index => None,
        Some(r) if r > index => Some(r - 1),
        other => other,
    };
}

/// The ramp that the form starts on: the first qualitative ramp for
/// categories, and the first other ramp for numbers.
pub fn default_ramp(list: &[Ramp], qualitative: bool) -> usize {
    list.iter()
        .position(|r| r.qualitative == qualitative)
        .unwrap_or(0)
}

/// Replaces the rules of the layer with a rule for each class. Each rule has
/// the symbolizers of the first rule of the layer. Returns false if the
/// request gives no class.
pub fn apply(state: &mut AppState, id: LayerId, request: &Request) -> bool {
    if let Some(colors) = request.neighbors {
        return apply_neighbors(state, id, colors, request);
    }
    let Some(layer) = state.doc.layer(id) else {
        return false;
    };
    let Some(kind) = layer.geometry() else {
        return false;
    };
    let Some(classes) = classify::classes_of(
        layer,
        &request.text,
        request.method,
        request.classes,
        Some(&request.ramp),
    ) else {
        return false;
    };
    let colors = classify::class_colors(&request.ramp, &classes, request.reverse);
    let Some(entry) = state.doc.entry_mut(id) else {
        return false;
    };
    let template = entry
        .style
        .style
        .rules
        .first()
        .cloned()
        .unwrap_or_else(|| LayerStyle::default_for(kind, 0, &[]).style.rules[0].clone());
    entry.style.style.rules =
        classify::make_rules(&template, kind, &classes, &colors, request.step);
    state.touch();
    true
}

/// Gives each polygon of the layer a color that no neighbor has, saves the
/// colors in a field, and replaces the rules with a rule for each color, in
/// one undo step.
fn apply_neighbors(state: &mut AppState, id: LayerId, colors: usize, request: &Request) -> bool {
    let Some(layer) = state.doc.layer(id) else {
        return false;
    };
    if layer.geometry() != Some(GeometryKind::Polygon) || layer.features.is_empty() {
        return false;
    }
    let groups = crate::neighbors::color(
        &crate::neighbors::neighbors(&layer.features),
        &crate::neighbors::areas(&layer.features),
        colors,
    );
    let count = groups.iter().max().map_or(0, |g| g + 1);
    let values: Vec<Value> = groups
        .iter()
        .map(|g| Value::Number(*g as f64 + 1.0))
        .collect();
    let field = classify::color_field(layer);
    let classes = classify::color_classes(&field, count);
    let colors = classify::class_colors(&request.ramp, &classes, request.reverse);
    state.edit_schema(id, |doc| {
        let has = doc
            .layer(id)
            .is_some_and(|l| l.fields.iter().any(|f| f.name == field));
        if !has
            && !doc
                .project
                .add_field(id, Field::new(field.clone(), FieldType::Integer))
        {
            return false;
        }
        if !doc.project.set_field_values(id, &field, &values) {
            return false;
        }
        let Some(entry) = doc.entry_mut(id) else {
            return false;
        };
        let template = entry.style.style.rules.first().cloned().unwrap_or_else(|| {
            LayerStyle::default_for(GeometryKind::Polygon, 0, &[])
                .style
                .rules[0]
                .clone()
        });
        entry.style.style.rules = classify::make_rules(
            &template,
            GeometryKind::Polygon,
            &classes,
            &colors,
            Step::None,
        );
        true
    })
}

/// The note above the button: the count of the new rules, and of the rules
/// whose value matches a label.
pub fn summary(rules: usize, labeled: usize) -> String {
    let mut text = match rules {
        1 => "1 rule".to_owned(),
        n => format!("{n} rules"),
    };
    if labeled > 0 {
        text.push_str(&format!(" · {labeled} labeled"));
    }
    text
}
