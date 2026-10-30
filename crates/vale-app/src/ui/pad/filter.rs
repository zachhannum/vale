//! The filter of a rule as a list of conditions, and the page that adds a
//! condition with a field, an operator, and a value.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};
use vale_store::{FieldType, LayerId, Value};
use vale_style::expr::{Filter, field_ref};

use super::geometry::TOUCH;
use super::icons::Icon;
use super::raster::heading;
use super::widgets::{self, Controls, Keys, PANEL_ROW};
use super::{Page, theme};
use crate::ui::AppState;

/// The row of the rule page that opens the page of a new condition, the
/// title of that page, and its button.
pub const ADD_CONDITION: &str = "Add condition";
/// The start of the name of the button that removes a condition. The number
/// of the condition follows, from 1.
pub const REMOVE_CONDITION: &str = "Remove condition";
/// The text of the rule page while the filter has no condition.
pub const NO_CONDITION: &str = "The rule draws all features.";
/// The headings of the page of a new condition.
pub const FIELD: &str = "Field";
pub const OPERATOR: &str = "Operator";
/// The heading of the values, and the name of the value field.
pub const VALUE: &str = "Value";
/// The chips of the operators, and the operators that they write.
pub const OPERATORS: [(&str, &str); 6] = [
    ("=", "="),
    ("≠", "!="),
    ("<", "<"),
    ("≤", "<="),
    (">", ">"),
    ("≥", ">="),
];
/// The chips of the values of a true or false field.
pub const TRUE: &str = "True";
pub const FALSE: &str = "False";
/// The most chips of the common values of a text field.
const VALUE_CHIPS: usize = 12;
/// The longest value that has a chip, in characters.
const CHIP_TEXT: usize = 24;
/// The height of the keypad of the value field.
const KEYPAD: f32 = 4.0 * PANEL_ROW;

/// The condition that the page of a new condition makes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    /// The layer and the rule.
    at: Option<(LayerId, usize)>,
    field: Option<String>,
    /// The index in `OPERATORS`.
    operator: usize,
    value: String,
}

impl Draft {
    /// The condition as the filter syntax writes it, or `None` until the
    /// draft has a field and a value.
    pub fn text(&self, kind: FieldType) -> Option<String> {
        condition(self.field.as_deref()?, kind, self.operator, &self.value)
    }
}

/// A condition as the filter syntax writes it: the field, the operator with
/// this index in `OPERATORS`, and the value. A true or false value is `TRUE`
/// or `FALSE`. `None` if the value does not fit the kind of the field.
pub fn condition(field: &str, kind: FieldType, operator: usize, value: &str) -> Option<String> {
    let op = OPERATORS.get(operator)?.1;
    let value = match kind {
        FieldType::Integer | FieldType::Real => number_text(super::raster::parse_value(value)?),
        FieldType::Bool => match value {
            TRUE => "true".to_owned(),
            FALSE => "false".to_owned(),
            _ => return None,
        },
        FieldType::Text => format!("'{}'", value.replace('\'', "''")),
    };
    Some(format!("{} {op} {value}", field_ref(field)))
}

/// A number as the filter syntax writes it.
fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

/// The parts of a filter that `and` joins at the top level. A filter with
/// an `or` at the top level is one part in brackets. A blank filter has no
/// part.
pub fn conditions(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start, mut i) = (0i32, None, 0, 0);
    let word_at = |i: usize, word: &str| {
        let n = word.len();
        let before = i == 0 || !is_name_char(chars[i - 1]);
        let after = chars.get(i + n).is_none_or(|c| !is_name_char(*c));
        let same = chars.len() >= i + n
            && chars[i..i + n]
                .iter()
                .collect::<String>()
                .eq_ignore_ascii_case(word);
        before && after && same
    };
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c == '(' => depth += 1,
            None if c == ')' => depth -= 1,
            None if depth == 0 && word_at(i, "or") => {
                return vec![format!("({})", text.trim())];
            }
            None if depth == 0 && word_at(i, "and") => {
                parts.push(chars[start..i].iter().collect::<String>());
                start = i + 3;
                i += 2;
            }
            None => {}
        }
        i += 1;
    }
    parts.push(chars[start..].iter().collect());
    parts
        .into_iter()
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect()
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The filter text of these conditions.
pub fn join(parts: &[String]) -> String {
    parts.join(" and ")
}

/// A row with the text of a condition and a button that removes it. Returns
/// true when the user taps the button.
fn condition_row(ui: &mut egui::Ui, controls: &mut Controls, n: usize, text: &str) -> bool {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let button = Rect::from_center_size(
        pos2(row.right() - TOUCH / 2.0, row.center().y),
        vec2(TOUCH, TOUCH),
    );
    let name = format!("{REMOVE_CONDITION} {}", n + 1);
    let resp = widgets::icon_button_at(ui, controls, button, &name, Icon::Close, false, true);
    let clip = Rect::from_min_max(row.min, pos2(button.left() - 8.0, row.max.y));
    let at = pos2(row.left(), row.center().y);
    ui.painter().with_clip_rect(clip).text(
        at,
        Align2::LEFT_CENTER,
        text,
        egui::FontId::monospace(15.0),
        theme::TEXT,
    );
    resp.clicked()
}

/// The filter of the rule page: a row for each condition, the row that
/// opens the page of a new condition, and the text of the whole filter.
/// Returns the new filter text when the user changes it.
pub fn rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (id, i): (LayerId, usize),
    current: &str,
    typed: &mut String,
) -> Option<String> {
    heading(ui, super::rules::FILTER);
    let mut parts = conditions(current);
    if parts.is_empty() {
        super::raster::note(ui, NO_CONDITION);
    }
    let mut out = None;
    let mut remove = None;
    for (n, part) in parts.iter().enumerate() {
        if condition_row(ui, controls, n, part) {
            remove = Some(n);
        }
    }
    if let Some(n) = remove {
        parts.remove(n);
        out = Some(join(&parts));
    }
    if widgets::open_row(ui, controls, ADD_CONDITION, "").clicked() {
        state.pad.rules.draft = Draft {
            at: Some((id, i)),
            ..Draft::default()
        };
        state.pad.page = Some(Page::Condition(i));
    }

    // The whole filter as text, for a filter that conditions cannot give.
    let valid = Filter::new(typed).error().is_none();
    let name = super::rules::EXPRESSION;
    let field = widgets::text_field(ui, controls, name, name, None, typed, valid);
    if field.changed() {
        out = Some(typed.clone());
    } else if !field.has_focus() {
        // The field shows the filter again after an undo or a new condition.
        *typed = out.clone().unwrap_or_else(|| current.to_owned());
    }
    if !valid {
        widgets::form_note(ui, super::rules::BAD_FILTER, 20.0);
    }
    out
}

/// The values of a text field, most common first, at most `VALUE_CHIPS`.
fn common_values(state: &AppState, id: LayerId, field: &str) -> Vec<String> {
    let Some(layer) = state.doc.layer(id) else {
        return Vec::new();
    };
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for feature in &layer.features {
        if let Some(Value::Text(text)) = feature.attributes.get(field)
            && !text.is_empty()
            && text.chars().count() <= CHIP_TEXT
        {
            *counts.entry(text.as_str()).or_default() += 1;
        }
    }
    let mut values: Vec<(&str, usize)> = counts.into_iter().collect();
    values.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    values
        .into_iter()
        .take(VALUE_CHIPS)
        .map(|(v, _)| v.to_owned())
        .collect()
}

/// The page of a new condition of rule `i`: a chip for each field, and
/// after the choice of a field, the operators, the value, and the button that adds the condition to the
/// filter. A number has the keypad. A text has a chip for each common value
/// and a field for any other value.
pub fn condition_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: LayerId,
    i: usize,
) {
    let Some(layer) = state.doc.layer(id) else {
        return;
    };
    let fields: Vec<(String, FieldType)> = layer
        .fields
        .iter()
        .map(|f| (f.name.clone(), f.kind))
        .collect();
    let mut draft = std::mem::take(&mut state.pad.rules.draft);
    if draft.at != Some((id, i)) {
        draft = Draft {
            at: Some((id, i)),
            ..Draft::default()
        };
    }
    ui.spacing_mut().item_spacing.y = 0.0;

    // After the choice, one row shows the field, and a tap on it shows the
    // chips again.
    let names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
    match draft.field.clone() {
        Some(field) => {
            if widgets::open_row(ui, controls, FIELD, &field).clicked() {
                draft.field = None;
                widgets::close_keypad(ui.ctx());
            }
        }
        None => {
            heading(ui, FIELD);
            if let Some(chosen) = widgets::chips(ui, controls, &names, names.len()) {
                draft.field = Some(names[chosen].to_owned());
                draft.operator = 0;
                draft.value.clear();
            }
        }
    }
    let kind = draft
        .field
        .as_deref()
        .and_then(|f| fields.iter().find(|(n, _)| n == f))
        .map(|(_, k)| *k);

    if let (Some(kind), Some(field)) = (kind, draft.field.clone()) {
        let number = matches!(kind, FieldType::Integer | FieldType::Real);
        // Only a number has an order.
        let count = if number { OPERATORS.len() } else { 2 };
        let labels: Vec<&str> = OPERATORS[..count].iter().map(|(l, _)| *l).collect();
        heading(ui, OPERATOR);
        if let Some(chosen) = widgets::chips(ui, controls, &labels, draft.operator) {
            draft.operator = chosen;
        }

        heading(ui, VALUE);
        match kind {
            FieldType::Integer | FieldType::Real => {
                let valid = |t: &str| super::raster::parse_value(t).is_some();
                let shown = draft.value.clone();
                let keys = Keys::Number;
                if let Some(text) =
                    widgets::key_field(ui, controls, VALUE, VALUE, "", &shown, keys, valid)
                {
                    draft.value = text;
                }
                if widgets::keypad_field(ui.ctx()).as_deref() == Some(VALUE) {
                    ui.add_space(4.0);
                    widgets::keypad(ui, controls, KEYPAD);
                }
            }
            FieldType::Bool => {
                let choices = [TRUE, FALSE];
                let on = choices.iter().position(|c| *c == draft.value);
                if let Some(chosen) = widgets::chips(ui, controls, &choices, on.unwrap_or(2)) {
                    draft.value = choices[chosen].to_owned();
                }
            }
            FieldType::Text => {
                let values = common_values(state, id, &field);
                let chips: Vec<&str> = values.iter().map(String::as_str).collect();
                let on = chips.iter().position(|c| *c == draft.value);
                if !chips.is_empty()
                    && let Some(chosen) =
                        widgets::chips(ui, controls, &chips, on.unwrap_or(chips.len()))
                {
                    draft.value = chips[chosen].to_owned();
                }
                ui.add_space(4.0);
                let mut text = draft.value.clone();
                let label = "Other";
                if widgets::text_field(ui, controls, VALUE, label, None, &mut text, true).changed()
                {
                    draft.value = text;
                }
            }
        }
    }

    let condition = kind.and_then(|k| draft.text(k));
    if let Some(text) = &condition {
        super::raster::note(ui, text);
    }
    ui.add_space(8.0);
    let add = widgets::button_row(ui, controls, ADD_CONDITION, condition.is_some());
    if add.clicked()
        && let Some(condition) = condition
        && let Some(rule) = state
            .doc
            .entry_mut(id)
            .and_then(|e| e.style.style.rules.get_mut(i))
    {
        let mut parts = conditions(rule.filter.source().map_or("", |s| s.text()));
        parts.push(condition);
        rule.filter = Filter::new(&join(&parts));
        widgets::close_keypad(ui.ctx());
        state.pad.page = Some(Page::Rule(i));
        state.touch();
        return;
    }
    state.pad.rules.draft = draft;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions_split_at_the_top_level_and() {
        assert!(conditions("  ").is_empty());
        assert_eq!(
            conditions("a < 5 AND name = 'x and y' and (b or c)"),
            ["a < 5", "name = 'x and y'", "(b or c)"]
        );
        assert_eq!(conditions("a or b and c"), ["(a or b and c)"]);
        assert_eq!(conditions("brand = 1"), ["brand = 1"]);
        assert_eq!(join(&conditions("a and  b")), "a and b");
    }

    #[test]
    fn a_draft_writes_the_filter_syntax() {
        let mut d = Draft {
            field: Some("scalerank".into()),
            operator: 3,
            value: "5".into(),
            ..Draft::default()
        };
        assert_eq!(d.text(FieldType::Real).as_deref(), Some("scalerank <= 5"));
        d.value = "x".into();
        assert_eq!(d.text(FieldType::Real), None);
        d.field = Some("name en".into());
        d.operator = 1;
        d.value = "O'Hare".into();
        let text = d.text(FieldType::Text).unwrap();
        assert_eq!(text, "\"name en\" != 'O''Hare'");
        assert!(Filter::new(&text).error().is_none());
        assert_eq!(number_text(2.5), "2.5");
    }
}
