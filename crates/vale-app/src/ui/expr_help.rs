//! The help of an expression field in both layouts: the fields and the
//! functions that the user can insert at the cursor, and the result of the
//! expression for one feature.

use std::collections::BTreeMap;

use eframe::egui;
use eframe::egui::text::{CCursor, CCursorRange};
use vale_store::{GeometryKind, LayerId, Value};
use vale_style::expr::{Context, Expr, FUNCTIONS, field_ref};

use super::AppState;

/// The result of a filter for the feature of the preview.
pub const MATCHES: &str = "Matches";
pub const NO_MATCH: &str = "Does not match";
/// The result of a label text that gives no text.
pub const NO_LABEL: &str = "No label";
/// The result of a value expression that gives null.
pub const NO_VALUE: &str = "No value";
/// The menu of a desktop expression field, and its submenus.
pub const INSERT: &str = "Insert";
pub const FIELDS: &str = "Fields";
pub const FUNCTIONS_MENU: &str = "Functions";

/// The map scale of the preview.
const PREVIEW_SCALE: f64 = 1_000_000.0;

/// What the result of an expression is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExprKind {
    /// True or false, for a filter.
    Filter,
    /// The text of a label.
    Text,
    /// A value, for the expression of Classify.
    Value,
}

/// A field or a function that the user can insert.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    /// The name of the field or of the function.
    pub label: String,
    /// The text that goes in the field.
    pub insert: String,
    /// A function call. The insert selects its first argument.
    pub call: bool,
}

/// The line under an expression field.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// The result for one feature.
    Preview(String),
    /// The message of the parser.
    Error(String),
}

/// The fields of a layer, and the feature of the preview.
#[derive(Clone, Debug)]
pub struct ExprHelp {
    pub fields: Vec<String>,
    sample: Option<BTreeMap<String, Value>>,
    geometry: GeometryKind,
}

impl ExprHelp {
    /// The help for layer `id`. The feature of the preview is the first
    /// selected feature of the layer, or else its first feature.
    pub fn for_layer(state: &AppState, id: LayerId) -> Self {
        let Some(layer) = state.doc.layer(id) else {
            return Self::empty();
        };
        let selected = state
            .feature_selection
            .iter()
            .filter(|f| f.layer == id)
            .find_map(|f| layer.features.iter().find(|g| g.uuid() == f.uuid));
        let sample = selected.or(layer.features.first());
        ExprHelp {
            fields: layer.fields.iter().map(|f| f.name.clone()).collect(),
            sample: sample.map(|f| f.attributes.clone()),
            geometry: layer.geometry().unwrap_or(GeometryKind::Point),
        }
    }

    /// No fields and no feature.
    pub fn empty() -> Self {
        ExprHelp {
            fields: Vec::new(),
            sample: None,
            geometry: GeometryKind::Point,
        }
    }

    /// The fields, and then the functions.
    pub fn chips(&self) -> Vec<Chip> {
        let fields = self.fields.iter().map(|name| Chip {
            label: name.clone(),
            insert: field_ref(name),
            call: false,
        });
        let functions = FUNCTIONS.iter().map(|f| Chip {
            label: f.name.to_owned(),
            insert: f.signature.to_owned(),
            call: true,
        });
        fields.chain(functions).collect()
    }

    /// The line under a field with `text`. `None` for blank text, and for a
    /// layer with no features.
    pub fn line(&self, text: &str, kind: ExprKind) -> Option<Line> {
        if text.trim().is_empty() {
            return None;
        }
        let expr = match Expr::parse(text) {
            Ok(expr) => expr,
            Err(e) => return Some(Line::Error(e.short())),
        };
        let attributes = self.sample.as_ref()?;
        let value = expr.eval(&Context {
            attributes,
            scale: PREVIEW_SCALE,
            geometry: self.geometry,
        });
        let text = match kind {
            ExprKind::Filter if value == Value::Bool(true) => MATCHES.to_owned(),
            ExprKind::Filter => NO_MATCH.to_owned(),
            ExprKind::Text => value.label().unwrap_or_else(|| NO_LABEL.to_owned()),
            ExprKind::Value => value.label().unwrap_or_else(|| NO_VALUE.to_owned()),
        };
        Some(Line::Preview(text))
    }

    /// True if `text` parses, or is blank.
    pub fn valid(&self, text: &str) -> bool {
        text.trim().is_empty() || Expr::parse(text).is_ok()
    }
}

/// Replaces the selection of the text field `id` with the text of `chip`,
/// or inserts it at the cursor. The insert of a call selects its first
/// argument. A field with no cursor gets the text at its end.
pub fn insert_at_cursor(ctx: &egui::Context, id: egui::Id, text: &mut String, chip: &Chip) {
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    let count = text.chars().count();
    let (start, end) = state.cursor.char_range().map_or((count, count), |r| {
        let (a, b): (usize, usize) = (r.primary.index.into(), r.secondary.index.into());
        let (a, b) = (a.min(count), b.min(count));
        (a.min(b), a.max(b))
    });
    let byte = |c: usize| text.char_indices().nth(c).map_or(text.len(), |(i, _)| i);
    let (from, to) = (byte(start), byte(end));
    text.replace_range(from..to, &chip.insert);
    let (a, b) = if chip.call {
        first_argument(&chip.insert)
    } else {
        let n = chip.insert.chars().count();
        (n, n)
    };
    let range = CCursorRange::two(CCursor::new(start + a), CCursor::new(start + b));
    state.cursor.set_char_range(Some(range));
    state.store(ctx, id);
}

/// The first argument of `call`, as a range of chars. The place after the
/// opening parenthesis for a call with no arguments.
fn first_argument(call: &str) -> (usize, usize) {
    let chars: Vec<char> = call.chars().collect();
    let Some(open) = chars.iter().position(|c| *c == '(') else {
        return (chars.len(), chars.len());
    };
    let start = open + 1;
    let end = chars[start..]
        .iter()
        .position(|c| *c == ',' || *c == ')')
        .map_or(chars.len(), |p| start + p);
    (start, end)
}

/// The "Insert" menu of a desktop expression field `id`, with the fields
/// and the functions. Returns true when the user inserts one.
pub fn insert_menu(ui: &mut egui::Ui, help: &ExprHelp, id: egui::Id, text: &mut String) -> bool {
    let mut chosen = None;
    ui.menu_button(INSERT, |ui| {
        let chips = help.chips();
        let (fields, functions) = chips.split_at(help.fields.len());
        ui.add_enabled_ui(!fields.is_empty(), |ui| {
            ui.menu_button(FIELDS, |ui| {
                for chip in fields {
                    if ui.button(&chip.label).clicked() {
                        chosen = Some(chip.clone());
                    }
                }
            });
        });
        ui.menu_button(FUNCTIONS_MENU, |ui| {
            for (chip, f) in functions.iter().zip(FUNCTIONS) {
                let button = ui.button(&chip.insert).on_hover_text(f.summary);
                if button.clicked() {
                    chosen = Some(chip.clone());
                }
            }
        });
    });
    let Some(chip) = chosen else {
        return false;
    };
    insert_at_cursor(ui.ctx(), id, text, &chip);
    ui.memory_mut(|m| m.request_focus(id));
    true
}

/// The line under a desktop expression field: the result in weak text, or
/// the message of the parser in the warning color.
pub fn desktop_line(ui: &mut egui::Ui, help: &ExprHelp, text: &str, kind: ExprKind) {
    match help.line(text, kind) {
        Some(Line::Preview(result)) => {
            ui.weak(result);
        }
        Some(Line::Error(message)) => {
            ui.colored_label(ui.visuals().warn_fg_color, message);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn help(attributes: &[(&str, Value)]) -> ExprHelp {
        let sample = attributes
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect();
        ExprHelp {
            fields: attributes.iter().map(|(k, _)| (*k).to_owned()).collect(),
            sample: Some(sample),
            geometry: GeometryKind::Point,
        }
    }

    #[test]
    fn the_line_gives_the_result_of_each_kind() {
        let h = help(&[("name", Value::Text("Ardent".into())), ("pop", Value::Null)]);
        let line = |text, kind| h.line(text, kind);
        let preview = |s: &str| Some(Line::Preview(s.to_owned()));
        assert_eq!(line("name = 'Ardent'", ExprKind::Filter), preview(MATCHES));
        assert_eq!(line("name = 'Bree'", ExprKind::Filter), preview(NO_MATCH));
        assert_eq!(line("upper(name)", ExprKind::Text), preview("ARDENT"));
        assert_eq!(line("pop", ExprKind::Text), preview(NO_LABEL));
        assert_eq!(line("pop", ExprKind::Value), preview(NO_VALUE));
        assert_eq!(line("round(2.46, 1)", ExprKind::Value), preview("2.5"));
        assert_eq!(line(" ", ExprKind::Filter), None);
        let error = Some(Line::Error("Unknown function 'shout'.".to_owned()));
        assert_eq!(line("shout(name)", ExprKind::Text), error);
        assert_eq!(ExprHelp::empty().line("name", ExprKind::Text), None);
    }

    #[test]
    fn the_chips_list_the_fields_and_then_the_functions() {
        let h = help(&[("river name", Value::Null), ("pop", Value::Null)]);
        let chips = h.chips();
        assert_eq!(chips[0].insert, "\"river name\"");
        assert_eq!(chips[1].insert, "pop");
        assert!(!chips[0].call);
        assert_eq!(chips.len(), 2 + FUNCTIONS.len());
        let round = chips.iter().find(|c| c.label == "round").unwrap();
        assert_eq!(round.insert, "round(x, digits)");
        assert!(round.call);
    }

    #[test]
    fn an_insert_replaces_the_selection_and_selects_the_first_argument() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("field");
        let select = |a: usize, b: usize| {
            let mut state = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
            let range = CCursorRange::two(CCursor::new(a), CCursor::new(b));
            state.cursor.set_char_range(Some(range));
            state.store(&ctx, id);
        };
        let selection = || {
            let r = egui::TextEdit::load_state(&ctx, id)
                .and_then(|s| s.cursor.char_range())
                .unwrap();
            (usize::from(r.secondary.index), usize::from(r.primary.index))
        };
        let h = help(&[("river name", Value::Null)]);
        let chips = h.chips();
        let round = chips.iter().find(|c| c.label == "round").unwrap();

        let mut text = "é = ".to_owned();
        select(4, 4);
        insert_at_cursor(&ctx, id, &mut text, &chips[0]);
        assert_eq!(text, "é = \"river name\"");
        assert_eq!(selection(), (16, 16));

        let mut text = "a + b".to_owned();
        select(4, 5);
        insert_at_cursor(&ctx, id, &mut text, round);
        assert_eq!(text, "a + round(x, digits)");
        assert_eq!(selection(), (10, 11));
    }
}
