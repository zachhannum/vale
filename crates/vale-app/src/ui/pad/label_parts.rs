//! The text builder of a label text: a row for each part of the text, the
//! page that adds a part, the page of one part, and the Expression field
//! with the whole text. The builder writes the text only when the user
//! changes a part, so text that it cannot read stays as it is.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};
use vale_store::{FieldType, Layer};
use vale_style::expr::{TextPart, join, split};

use super::expression;
use super::geometry::TOUCH;
use super::icons::{self, Icon};
use super::raster::heading;
use super::theme;
use super::widgets::{self, Controls, PANEL_ROW};
use crate::ui::atlas::ClassDrag;
use crate::ui::expr_help::{ExprHelp, ExprKind};

/// The label text of a layer: the row of the layer panel, the title of its
/// page, and the field of the desktop layer panel.
pub const LABEL_TEXT: &str = "Label text";
/// The row that opens the page of a new part, and the title of that page.
pub const ADD_PART: &str = "Add part";
/// The start of the names of the controls of a part row. The number of the
/// part follows, from 1.
pub const PART: &str = "Part";
pub const REMOVE_PART: &str = "Remove part";
pub const REORDER_PART: &str = "Reorder part";
/// The text of the builder while the text has no part.
pub const NO_PARTS: &str = "The label has no text.";
/// The heading of the field chips of the page of a new part.
pub const FIELDS: &str = "Fields";
/// The chip of a fixed text, the title of its page, and its field.
pub const TEXT: &str = "Text";
/// The title of the page of a part that the builder cannot read.
pub const EXPRESSION: &str = "Expression";
/// The fields of the page of a field part.
pub const PREFIX: &str = "Prefix";
pub const SUFFIX: &str = "Suffix";
/// The heading of the chips of the decimal places of a number field.
pub const DECIMAL_PLACES: &str = "Decimal places";
/// The chip of no rounding.
pub const NO_ROUNDING: &str = "None";
/// The chips of the decimal places after "None".
pub const PLACES: [&str; 5] = ["0", "1", "2", "3", "4"];
/// The note of the page of a field part.
pub const AFFIX_NOTE: &str = "The prefix and the suffix show only when the field has a value.";

/// What the user did in the builder in one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Builder {
    /// The new text.
    pub text: Option<String>,
    /// True if the new text comes from the Expression field.
    pub typed: bool,
    /// The part whose row the user tapped.
    pub open: Option<usize>,
    /// The user tapped "Add part".
    pub add: bool,
}

/// The fields of a layer, each with true for a number field.
pub fn fields_of(layer: &Layer) -> Vec<(String, bool)> {
    (layer.fields.iter())
        .map(|f| {
            let number = matches!(f.kind, FieldType::Integer | FieldType::Real);
            (f.name.clone(), number)
        })
        .collect()
}

/// The title of the page of a part.
pub fn part_title(part: &TextPart) -> &str {
    match part {
        TextPart::Field { name, .. } => name,
        TextPart::Text(_) => TEXT,
        TextPart::Raw(_) => EXPRESSION,
    }
}

/// The builder of text `current` under heading `name`: a row for each
/// part, the row "Add part", and the Expression field with the whole text,
/// which has the control name `name`. `typed` keeps the text of the field
/// while it has the focus, and `drag` the row that a finger moves.
pub fn list(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    current: &str,
    typed: &mut String,
    drag: &mut Option<ClassDrag>,
    help: &ExprHelp,
) -> Builder {
    let mut out = Builder::default();
    heading(ui, name);
    let parts = split(current);
    if parts.is_empty() {
        super::raster::note(ui, NO_PARTS);
    }
    rows(ui, controls, &parts, drag, &mut out);
    out.add = widgets::open_row(ui, controls, ADD_PART, "").clicked();

    // A raw part has no page, and a tap on its row starts the Expression field.
    let id = ui.id().with(name);
    if let Some(k) = out.open
        && matches!(parts[k], TextPart::Raw(_))
    {
        out.open = None;
        ui.memory_mut(|m| m.request_focus(id));
    }
    let focused = ui.memory(|m| m.has_focus(id));
    if !focused {
        // The field shows the text again after an undo or a change of a part.
        *typed = out.text.clone().unwrap_or_else(|| current.to_owned());
    }
    let field = expression::field(ui, controls, name, typed, help, ExprKind::Text);
    if field.changed() && help.valid(typed) {
        out.text = Some(typed.clone());
        out.typed = true;
    }
    out
}

/// The rows of the parts. A finger on the handle of a row lifts the row,
/// and the row goes to its new place when the finger lets go.
fn rows(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    parts: &[TextPart],
    drag: &mut Option<ClassDrag>,
    out: &mut Builder,
) {
    let size = vec2(ui.available_width(), parts.len() as f32 * PANEL_ROW);
    let (area, _) = ui.allocate_exact_size(size, Sense::hover());
    let slot =
        |top: f32| Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), PANEL_ROW));
    let place = |k: usize| slot(area.top() + k as f32 * PANEL_ROW);
    let (pointer, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_down()));

    let Some(lifted) = drag.filter(|d| d.from < parts.len()) else {
        *drag = None;
        let mut remove = None;
        for (k, part) in parts.iter().enumerate() {
            let rect = place(k);
            let Some(row) = part_row(ui, Some(&mut *controls), rect, k, part, false) else {
                continue;
            };
            if row.body.clicked() {
                out.open = Some(k);
            }
            if row.remove.clicked() {
                remove = Some(k);
            }
            if row.handle.drag_started() || row.handle.dragged() {
                let y = pointer.map_or(rect.center().y, |p| p.y);
                *drag = Some(ClassDrag {
                    from: k,
                    grab: y - rect.top(),
                });
                ui.ctx().request_repaint();
            }
        }
        if let Some(k) = remove {
            let mut parts = parts.to_vec();
            parts.remove(k);
            out.text = Some(join(&parts));
        }
        return;
    };

    let from = lifted.from;
    let y = pointer.map_or(place(from).top() + lifted.grab, |p| p.y);
    let top = (y - lifted.grab).clamp(area.top(), area.bottom() - PANEL_ROW);
    let to = ((top - area.top()) / PANEL_ROW).round() as usize;
    let others = parts.iter().enumerate().filter(|(k, _)| *k != from);
    for (n, (k, part)) in others.enumerate() {
        let rect = place(if n < to { n } else { n + 1 });
        part_row(ui, None, rect, k, part, false);
    }
    part_row(ui, None, slot(top), from, &parts[from], true);
    if down {
        ui.ctx().request_repaint();
    } else {
        *drag = None;
        if to != from && to < parts.len() {
            let mut parts = parts.to_vec();
            let moved = parts.remove(from);
            parts.insert(to, moved);
            out.text = Some(join(&parts));
        }
    }
}

/// The controls of a part row.
struct PartRow {
    body: egui::Response,
    remove: egui::Response,
    handle: egui::Response,
}

/// One part row in `rect`: the summary of part `k`, its remove button, and
/// its handle at the right. With `controls`, the row takes a tap, the
/// button a tap, and the handle a drag. A `lifted` row shows above the
/// others.
fn part_row(
    ui: &egui::Ui,
    controls: Option<&mut Controls>,
    rect: Rect,
    k: usize,
    part: &TextPart,
    lifted: bool,
) -> Option<PartRow> {
    let handle = Rect::from_center_size(
        pos2(rect.right() - TOUCH / 2.0 + 8.0, rect.center().y),
        vec2(TOUCH, TOUCH),
    );
    let button = handle.translate(vec2(-TOUCH, 0.0));
    let body = Rect::from_min_max(rect.min, pos2(button.left(), rect.bottom()));
    let n = k + 1;
    let out = controls.map(|controls| {
        let name = format!("{PART} {n}");
        PartRow {
            body: widgets::tap_area(ui, controls, body, &name, true),
            remove: widgets::icon_button_at(
                ui,
                controls,
                button,
                &format!("{REMOVE_PART} {n}"),
                Icon::Close,
                false,
                true,
            ),
            handle: widgets::drag_area(ui, controls, handle, &format!("{REORDER_PART} {n}")),
        }
    });
    let highlight = rect.expand2(vec2(8.0, -widgets::HIGHLIGHT_INSET));
    let painter = ui.painter();
    if lifted {
        let shadow = highlight.translate(vec2(0.0, 2.0)).expand(2.0);
        painter.rect_filled(shadow, 7.0, egui::Color32::from_black_alpha(60));
        painter.rect_filled(highlight, 5.0, theme::card(false).to_opaque());
    }
    let pressed = (out.as_ref()).is_some_and(|r| r.body.is_pointer_button_down_on());
    if lifted || pressed {
        painter.rect_filled(highlight, 5.0, theme::raise());
    }
    let clip = Rect::from_min_max(rect.min, pos2(body.right() - 8.0, rect.bottom()));
    summary(
        &painter.with_clip_rect(clip),
        pos2(rect.left(), rect.center().y),
        part,
    );
    let color = if lifted { theme::TEXT } else { theme::MUTE };
    icons::paint(painter, handle, Icon::Grip, widgets::ICON, color);
    out
}

/// The summary of a part from `at`, at the left middle. A field part shows
/// its prefix, its name, and its suffix, with its decimal places. A text
/// part shows the text in quotes, and a raw part the expression in the mono
/// font.
fn summary(painter: &egui::Painter, at: egui::Pos2, part: &TextPart) {
    let mut x = at.x;
    let mut paint = |text: &str, font: egui::FontId, color| {
        if !text.is_empty() {
            let rect = painter.text(pos2(x, at.y), Align2::LEFT_CENTER, text, font, color);
            x = rect.right();
        }
    };
    match part {
        TextPart::Field {
            name,
            prefix,
            suffix,
            digits,
        } => {
            paint(prefix, theme::body(), theme::MUTE);
            paint(name, theme::body(), theme::TEXT);
            paint(suffix, theme::body(), theme::MUTE);
            if let Some(d) = digits {
                let places = if *d == 1 { "place" } else { "places" };
                let text = format!("  {d} decimal {places}");
                paint(&text, theme::small(), theme::MUTE);
            }
        }
        TextPart::Text(text) => paint(&format!("“{text}”"), theme::body(), theme::TEXT),
        TextPart::Raw(text) => paint(text, theme::mono(), theme::TEXT),
    }
}

/// The page of a new part: a chip for each field, and the "Text" chip.
/// Returns the part that the user chose.
pub fn add_page(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    fields: &[(String, bool)],
) -> Option<TextPart> {
    let mut out = None;
    if !fields.is_empty() {
        heading(ui, FIELDS);
        let names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
        if let Some(k) = widgets::chips(ui, controls, &names, names.len()) {
            out = Some(TextPart::Field {
                name: names[k].to_owned(),
                prefix: String::new(),
                suffix: String::new(),
                digits: None,
            });
        }
    }
    heading(ui, TEXT);
    if widgets::chips(ui, controls, &[TEXT], 1).is_some() {
        out = Some(TextPart::Text(String::new()));
    }
    out
}

/// The page of a part. A field part has its prefix, its suffix, and, for a
/// `number` field, its decimal places. A text part has its text. Returns
/// the changed part, with true if the change is one step, such as a tap on
/// a chip, and false while the user types.
pub fn part_page(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    part: &TextPart,
    number: bool,
    help: &ExprHelp,
) -> Option<(TextPart, bool)> {
    match part {
        TextPart::Field {
            name,
            prefix,
            suffix,
            digits,
        } => {
            let mut out = None;
            let mut field = |label: &str, text: &str| {
                let mut text = text.to_owned();
                let changed =
                    widgets::text_field(ui, controls, label, label, None, &mut text, true)
                        .changed();
                changed.then_some(text)
            };
            let new_prefix = field(PREFIX, prefix);
            let new_suffix = field(SUFFIX, suffix);
            if new_prefix.is_some() || new_suffix.is_some() {
                out = Some((
                    TextPart::Field {
                        name: name.clone(),
                        prefix: new_prefix.unwrap_or_else(|| prefix.clone()),
                        suffix: new_suffix.unwrap_or_else(|| suffix.clone()),
                        digits: *digits,
                    },
                    false,
                ));
            }
            super::raster::note(ui, AFFIX_NOTE);
            if number {
                heading(ui, DECIMAL_PLACES);
                let mut chips = vec![NO_ROUNDING];
                chips.extend(PLACES);
                let active = match digits {
                    None => 0,
                    Some(d) => (*d as usize + 1).min(chips.len()),
                };
                if let Some(k) = widgets::chips(ui, controls, &chips, active) {
                    let digits = k.checked_sub(1).map(|d| d as u32);
                    out = Some((
                        TextPart::Field {
                            name: name.clone(),
                            prefix: prefix.clone(),
                            suffix: suffix.clone(),
                            digits,
                        },
                        true,
                    ));
                }
            }
            out
        }
        TextPart::Text(text) => {
            let mut text = text.clone();
            let field = widgets::text_field(ui, controls, TEXT, TEXT, None, &mut text, true);
            field.changed().then_some((TextPart::Text(text), false))
        }
        TextPart::Raw(text) => {
            expression::line(ui, help, text, ExprKind::Text);
            None
        }
    }
}

/// Text `current` with part `k` replaced by `part`. `None` if there is no
/// such part.
pub fn replace(current: &str, k: usize, part: TextPart) -> Option<String> {
    let mut parts = split(current);
    *parts.get_mut(k)? = part;
    Some(join(&parts))
}

/// Text `current` with `part` at the end, and the place of the new part.
pub fn append(current: &str, part: TextPart) -> (String, usize) {
    let mut parts = split(current);
    parts.push(part);
    (join(&parts), parts.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str) -> TextPart {
        TextPart::Field {
            name: name.to_owned(),
            prefix: String::new(),
            suffix: String::new(),
            digits: None,
        }
    }

    #[test]
    fn append_and_replace_rewrite_only_the_parts() {
        let (text, k) = append("", field("name"));
        assert_eq!((text.as_str(), k), ("name", 0));
        let (text, k) = append(&text, TextPart::Text(String::new()));
        assert_eq!((text.as_str(), k), ("concat(name, '')", 1));
        let text = replace(&text, 1, TextPart::Text(" km".into())).unwrap();
        assert_eq!(text, "concat(name, ' km')");
        assert_eq!(replace(&text, 2, field("x")), None);
        let raw = "if(a, b, c)";
        assert_eq!(split(raw), [TextPart::Raw(raw.to_owned())]);
        let (text, _) = append(raw, field("name"));
        assert_eq!(text, "concat(if(a, b, c), name)");
    }

    #[test]
    fn a_number_part_rounds_to_its_decimal_places() {
        use std::collections::BTreeMap;
        use vale_store::{GeometryKind, Value};
        use vale_style::expr::{Context, Expr};
        let part = TextPart::Field {
            name: "height".into(),
            prefix: String::new(),
            suffix: " m".into(),
            digits: Some(1),
        };
        let text = join(&[part]);
        assert_eq!(text, "round(height, 1) || ' m'");
        let attributes = BTreeMap::from([("height".to_owned(), Value::Number(1234.567))]);
        let value = Expr::parse(&text).unwrap().eval(&Context {
            attributes: &attributes,
            scale: 1.0,
            geometry: GeometryKind::Point,
        });
        assert_eq!(value, Value::Text("1234.6 m".into()));
    }
}
