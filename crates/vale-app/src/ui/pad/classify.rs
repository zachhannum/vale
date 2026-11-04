//! The Classify form of a vector layer: it replaces the rules of the layer
//! with a rule for each class of a field or an expression.

use eframe::egui;
use vale_store::{GeometryKind, LayerId, Value};
use vale_style::LayerStyle;
use vale_style::expr::{Expr, field_ref};

use super::number::Number;
use super::raster::{heading, note, ramp_row};
use super::rules::{CIRCLE_SIZES, WIDTHS};
use super::widgets::{self, Controls};
use crate::classify::{self, Method, Step};
use crate::ramps::Ramp;
use crate::ui::AppState;

/// The title of the form, and the button of the Style section that opens it.
pub const TITLE: &str = "Classify";
pub const CLASSIFY: &str = "Classify…";
pub const CLASSIFY_BY: &str = "Classify by";
/// The field of the expression that gives the values.
pub const EXPRESSION: &str = "Expression";
pub const BAD_EXPRESSION: &str = "Not a valid expression.";
pub const METHOD: &str = "Method";
pub const CLASS_COUNT: &str = "Classes";
pub const COLOR_RAMP: &str = "Color ramp";
pub const REVERSE: &str = "Reverse colors";
pub const STEP_WIDTH: &str = "Step width";
pub const STEP_SIZE: &str = "Step size";
pub const START_WIDTH: &str = "First width";
pub const END_WIDTH: &str = "Last width";
pub const START_SIZE: &str = "First size";
pub const END_SIZE: &str = "Last size";
pub const MAKE_RULES: &str = "Make rules";
/// The note when no feature gives a value.
pub const NO_VALUES: &str = "No feature has a value.";

/// The values of the expression for the features of the layer.
#[derive(Clone, Debug)]
enum Values {
    /// Sorted numbers, and the count of the features with no number.
    Numbers(Vec<f64>, usize),
    /// The common values, most common first.
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
    reverse: bool,
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

    heading(ui, CLASSIFY_BY);
    let refs: Vec<String> = layer.fields.iter().map(|f| field_ref(&f.name)).collect();
    let names: Vec<&str> = layer.fields.iter().map(|f| f.name.as_str()).collect();
    let active = refs
        .iter()
        .position(|r| *r == draft.text.trim())
        .unwrap_or(names.len());
    if let Some(chosen) = widgets::chips(ui, controls, &names, active) {
        draft.text = refs[chosen].clone();
    }
    let valid = draft.text.trim().is_empty() || Expr::parse(&draft.text).is_ok();
    widgets::text_field(
        ui,
        controls,
        EXPRESSION,
        EXPRESSION,
        None,
        &mut draft.text,
        valid,
    );
    if !valid {
        widgets::form_note(ui, BAD_EXPRESSION, 20.0);
    }

    let fresh = matches!(&draft.values, Some((t, g, _)) if *t == draft.text && *g == generation);
    if !fresh && !draft.text.trim().is_empty() {
        let values = match Expr::parse(&draft.text) {
            Ok(expr) => {
                let all = classify::values(layer, &expr);
                match classify::numbers(&all) {
                    Some((sorted, nulls)) => Values::Numbers(sorted, nulls),
                    None => {
                        Values::Categories(classify::categories(&all, classify::MAX_CATEGORIES))
                    }
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
    heading(ui, COLOR_RAMP);
    for (i, r) in list.iter().enumerate() {
        if ramp_row(ui, controls, &r.name, &r.colors, i == ramp).clicked() {
            draft.ramp = Some(i);
        }
    }
    ui.add_space(8.0);
    widgets::switch_row(ui, controls, REVERSE, REVERSE, &mut draft.reverse);

    if number {
        let (switch, names, range) = match kind {
            GeometryKind::Point => (STEP_SIZE, [START_SIZE, END_SIZE], CIRCLE_SIZES),
            _ => (STEP_WIDTH, [START_WIDTH, END_WIDTH], WIDTHS),
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
        Some(Values::Categories(common)) if !common.is_empty() => {
            classify::category_classes(&left, common)
        }
        _ => Vec::new(),
    };
    let count = state.doc.entry(id).map_or(0, |e| e.style.style.rules.len());
    ui.add_space(8.0);
    match &values {
        Some(Values::Bad) | None => {}
        Some(_) if classes.is_empty() => note(ui, NO_VALUES),
        Some(_) => note(ui, &summary(count, classes.len())),
    }
    let free = !state.pad.rules_fixed;
    let made = widgets::button_row(ui, controls, MAKE_RULES, free && !classes.is_empty()).clicked();
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
        };
        if apply(state, id, &request) {
            state.pad.page = None;
        }
    }
    state.pad.rules.classify = draft;
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
    let Some(layer) = state.doc.layer(id) else {
        return false;
    };
    let Some(kind) = layer.geometry() else {
        return false;
    };
    let Some(classes) = classify::classes_of(layer, &request.text, request.method, request.classes)
    else {
        return false;
    };
    let steps = classes.iter().filter(|c| !c.extra).count();
    let colors = classify::class_colors(&request.ramp, steps, request.reverse);
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

/// The note above the button: the count of the rules before and after.
fn summary(before: usize, after: usize) -> String {
    let rules = |n: usize| {
        if n == 1 {
            "1 rule".to_owned()
        } else {
            format!("{n} rules")
        }
    };
    format!("Replaces {} with {}.", rules(before), rules(after))
}
