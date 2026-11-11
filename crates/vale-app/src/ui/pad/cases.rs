//! The Cases block of a value: a row for each case with its condition and
//! its result, the result otherwise, and "Add case". The block writes
//! nested `if` calls.

use eframe::egui::{self, Align2, Color32, Rect, Sense, Shape, Stroke, pos2, vec2};
use vale_style::expr::editor::{Cond, Seq, Tok};

use super::filter::{self, Act, Menu, Rows};
use super::geometry::TOUCH;
use super::theme;
use super::widgets::{self, Controls, Keys};
use crate::classify::number_text;
use crate::ui::expr_help::ExprHelp;
use crate::ui::token_field::paint::{BAD, FUNCTION_TEXT, paint_chip};
use crate::ui::token_field::{ChipKind, Look, Metrics};

pub const CASES: &str = "Cases";
pub const WHEN: &str = "When";
pub const OTHERWISE: &str = "Otherwise";
pub const ADD_CASE: &str = "Add case";
/// The length of the arrow between a condition and its result.
const ARROW: f32 = 16.0;
/// The width of the place of a result.
const RESULT: f32 = 110.0;
const SPACE: f32 = 8.0;

/// The start of the control names of case `k`, from 0.
pub fn case_prefix(k: usize) -> String {
    format!("Case {} ", k + 1)
}

/// The name of the result of case `k`, or of the result otherwise.
pub fn result_name(k: Option<usize>) -> String {
    match k {
        Some(k) => format!("Case {} result", k + 1),
        None => format!("{OTHERWISE} result"),
    }
}

/// The state of a Cases block between frames.
#[derive(Clone, Debug, Default)]
pub struct CasesState {
    /// The open menu: the case, and the menu of its condition.
    menu: Option<(usize, Vec<usize>, Menu)>,
    search: String,
}

/// What the user did in a Cases block.
pub enum CasesEdit {
    /// The new block, and true for one step.
    Changed(Tok, bool),
    OpenCode,
}

/// The Cases block of `cases` and `otherwise`. `error` is the path of the
/// part with the parse error in the block: `2k` for the condition of case
/// `k`, `2k + 1` for its result, and `2n` for the result otherwise.
pub fn block(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    st: &mut CasesState,
    (cases, otherwise): (&[(Cond, Seq)], &Seq),
    help: &ExprHelp,
    error: Option<&[usize]>,
) -> Option<CasesEdit> {
    let mut cases = cases.to_vec();
    let mut otherwise = otherwise.clone();
    let mut changed: Option<bool> = None;
    let mut open_code = false;
    let marked = |i: usize| error.and_then(|p| p.first()) == Some(&i);

    let frame = ui.painter().add(Shape::Noop);
    let width = ui.available_width();
    let left = ui.cursor().left();
    let top = ui.cursor().top();
    let (title, _) = ui.allocate_exact_size(vec2(width, 40.0), Sense::hover());
    let at = pos2(title.left() + 14.0, title.center().y + 2.0);
    let font = egui::FontId::proportional(17.0);
    (ui.painter()).text(at, Align2::LEFT_CENTER, CASES, font, FUNCTION_TEXT);
    let inner = Rect::from_min_max(
        pos2(left + SPACE * 1.5, ui.cursor().top()),
        pos2(left + width - SPACE * 1.5, f32::INFINITY),
    );
    let layout = egui::Layout::top_down(egui::Align::Min);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
    child.spacing_mut().item_spacing.y = 0.0;

    let mut remove = None;
    for (k, (cond, result)) in cases.iter_mut().enumerate() {
        let menu = (st.menu.clone()).and_then(|(c, p, m)| (c == k).then_some((p, m)));
        let mut rows = Rows {
            help,
            prefix: case_prefix(k),
            menu,
            anchor: None,
            acts: Vec::new(),
        };
        let row_top = child.cursor().top();
        let lead = Some(WHEN);
        let place = match &*cond {
            Cond::Rule {
                subject,
                op,
                values,
            } => {
                let rule = (subject, *op, values.as_slice());
                filter::rule_row(&mut child, controls, &mut rows, &[0], rule, lead, RESULT)
            }
            Cond::Group { items, .. } if items.is_empty() => {
                filter::hole_row(&mut child, controls, &mut rows, &[0], lead, RESULT)
            }
            c => filter::code_row(
                &mut child,
                controls,
                &mut rows,
                &[0],
                &c.to_text(),
                lead,
                RESULT,
            ),
        };
        let row = Rect::from_min_max(
            pos2(inner.left(), row_top),
            pos2(inner.right(), child.cursor().top()),
        );
        if marked(2 * k) {
            mark(&child, row);
        }
        let name = result_name(Some(k));
        match result_control(&child, controls, place, &name, result, marked(2 * k + 1)) {
            Some(Some(seq)) => {
                *result = seq;
                changed = Some(changed.unwrap_or(false));
            }
            Some(None) => open_code = true,
            None => {}
        }
        widgets::keypad_below(&mut child, controls, &name);
        let items = vec![cond.clone()];
        let more =
            filter::menu_popover(&child, controls, &mut rows, (&items, true), &mut st.search);
        rows.acts.extend(more);
        let mut items = items;
        for act in rows.acts {
            match act {
                Act::Menu(m) => {
                    if m.is_some() {
                        st.search.clear();
                    }
                    st.menu = m.map(|(p, m)| (k, p, m));
                }
                Act::OpenCode => open_code = true,
                Act::Remove(p) if p == [0] => remove = Some(k),
                act => {
                    let keep = matches!(act, Act::Toggle(..) | Act::SetValue(..));
                    let typing = matches!(act, Act::SetValue(_, _, _, false));
                    if filter::apply(&mut items, act, help) {
                        changed = Some(changed.unwrap_or(false) || !typing);
                    }
                    if !keep {
                        st.menu = None;
                    }
                }
            }
        }
        *cond = items.pop().unwrap_or_else(Cond::hole);
        child.add_space(6.0);
    }
    if let Some(k) = remove {
        cases.remove(k);
        changed = Some(true);
    }

    // The result otherwise.
    let size = vec2(inner.width(), TOUCH + 2.0 * SPACE);
    let (row, _) = child.allocate_exact_size(size, Sense::hover());
    let painter = child.painter();
    painter.rect_filled(row, 12.0, Color32::from_white_alpha(10));
    let at = pos2(row.left() + SPACE, row.center().y);
    let word = painter.text(
        at,
        Align2::LEFT_CENTER,
        OTHERWISE,
        theme::body(),
        theme::MUTE,
    );
    let place = Rect::from_min_size(
        pos2(word.right() + SPACE, row.center().y - TOUCH / 2.0),
        vec2(RESULT, TOUCH),
    );
    let name = result_name(None);
    match result_control(
        &child,
        controls,
        place,
        &name,
        &otherwise,
        marked(2 * cases.len()),
    ) {
        Some(Some(seq)) => {
            otherwise = seq;
            changed = Some(changed.unwrap_or(false));
        }
        Some(None) => open_code = true,
        None => {}
    }
    widgets::keypad_below(&mut child, controls, &name);
    child.add_space(4.0);
    if filter::add_row(&mut child, controls, ADD_CASE, ADD_CASE) {
        cases.push((filter::new_rule(help), otherwise.clone()));
        changed = Some(true);
    }
    let bottom = child.min_rect().bottom() + SPACE;
    let rest = (bottom - ui.cursor().top()).max(0.0);
    ui.allocate_exact_size(vec2(width, rest), Sense::hover());
    let rect = Rect::from_min_max(pos2(left, top), pos2(left + width, bottom));
    let border = Color32::from_rgba_unmultiplied(140, 160, 255, 97);
    ui.painter().set(
        frame,
        Shape::rect_filled(
            rect,
            14.0,
            Color32::from_rgba_unmultiplied(130, 150, 255, 14),
        ),
    );
    let stroke = Stroke::new(1.0, border);
    (ui.painter()).rect_stroke(rect, 14.0, stroke, egui::StrokeKind::Inside);

    if open_code {
        return Some(CasesEdit::OpenCode);
    }
    let step = changed?;
    if cases.is_empty() {
        // With no case, the block is its result otherwise.
        return Some(CasesEdit::Changed(Tok::Paren(otherwise), true));
    }
    Some(CasesEdit::Changed(Tok::Cases { cases, otherwise }, step))
}

/// An arrow to the right from `start`, between a condition and its result.
fn paint_arrow(ui: &egui::Ui, start: egui::Pos2) {
    let stroke = Stroke::new(1.5, theme::MUTE);
    let tip = start + vec2(ARROW, 0.0);
    let painter = ui.painter();
    painter.line_segment([start, tip], stroke);
    painter.line_segment([tip, tip + vec2(-5.0, -4.0)], stroke);
    painter.line_segment([tip, tip + vec2(-5.0, 4.0)], stroke);
}

/// A red frame around a part with an error.
fn mark(ui: &egui::Ui, rect: Rect) {
    let stroke = Stroke::new(1.5, BAD);
    (ui.painter()).rect_stroke(rect, 12.0, stroke, egui::StrokeKind::Inside);
}

/// The arrow and the result in `place`: the keypad field of a number, or a
/// chip of other text that opens Code. Returns `Some(Some(seq))` for a new
/// result, and `Some(None)` to open Code.
fn result_control(
    ui: &egui::Ui,
    controls: &mut Controls,
    place: Rect,
    name: &str,
    result: &Seq,
    bad: bool,
) -> Option<Option<Seq>> {
    let y = place.center().y;
    let arrow = pos2(place.left() + 4.0, y);
    paint_arrow(ui, arrow);
    let rect = Rect::from_min_max(
        pos2(arrow.x + ARROW + SPACE, y - TOUCH / 2.0),
        pos2(place.right(), y + TOUCH / 2.0),
    );
    if bad {
        mark(ui, rect);
    }
    match result.0.as_slice() {
        [] | [Tok::Num(_)] => {
            let shown = match result.0.first() {
                Some(Tok::Num(n)) => number_text(*n),
                _ => String::new(),
            };
            let valid = |t: &str| super::raster::parse_value(t).is_some();
            let field = (rect, false);
            let typed =
                widgets::key_field_at(ui, controls, field, name, "", &shown, Keys::Number, valid)?;
            let n = super::raster::parse_value(&typed)?;
            Some(Some(Seq(vec![Tok::Num(n)])))
        }
        _ => {
            let resp = widgets::hit(ui, controls, rect, name, Sense::click(), true, None);
            let look = Look {
                kind: ChipKind::Function,
                glyph: None,
                label: result.to_text(),
                badge: None,
            };
            let m = Metrics::pad();
            let chip = Rect::from_center_size(rect.center(), vec2(rect.width(), m.chip));
            paint_chip(&ui.painter().with_clip_rect(rect), chip, &look, &m, false);
            resp.clicked().then_some(None)
        }
    }
}
