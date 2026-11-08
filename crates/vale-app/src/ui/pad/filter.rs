//! The filter of a rule or of a label class as conditions: Match all or
//! Match any, a row for each condition, and groups of conditions. Blocks and
//! Code are two views of the one filter text. Blocks write the text only
//! when the user edits in Blocks, so text that the user does not edit stays
//! as it is.

use eframe::egui::{self, Align2, Color32, Rect, Sense, Shape, Stroke, pos2, vec2};
use vale_store::{FieldType, Value};
use vale_style::expr::Unit;
use vale_style::expr::editor::{CmpOp, Cond, Conditions, Subject, ops_for};

use super::expression;
use super::geometry::TOUCH;
use super::icons::{self, Icon};
use super::label_text::{BLOCKS, CODE};
use super::popover;
use super::theme;
use super::widgets::{self, Controls, Keys};
use crate::classify::number_text;
use crate::ui::expr_help::{ExprHelp, ExprKind};
use crate::ui::token_field::paint::{BAD, paint_chip};
use crate::ui::token_field::{ChipKind, Glyph, Look, Metrics};

/// The row that adds a condition, and the start of the name of the row
/// that adds a condition to a group. The number of the group follows.
pub const ADD_CONDITION: &str = "Add condition";
/// The row that adds a group, and the start of the name of the row that
/// adds a group to a group.
pub const ADD_GROUP: &str = "Add group";
/// The start of the name of the button that removes a condition. The number
/// of the condition follows, from 1. The number of a condition in a group
/// is the number of the group, a point, and its number in the group.
pub const REMOVE_CONDITION: &str = "Remove condition";
pub const REMOVE_GROUP: &str = "Remove group";
/// The text while the filter of a rule has no condition.
pub const NO_CONDITION: &str = "The rule draws all features.";
/// The text while the filter of a label class has no condition.
pub const NO_CLASS_CONDITION: &str = "The class labels all features.";
/// The names of the segments Blocks and Code of a filter.
pub const VIEWS: [&str; 2] = ["Filter blocks", "Filter code"];
/// The menu of Match all and Match any, and its choices.
pub const MATCH: &str = "Match";
pub const ALL: &str = "all";
pub const ANY: &str = "any";
pub const OF_THESE: &str = "of these conditions";
/// The menu of a group, and its choices.
pub const ALL_OF: &str = "All of";
pub const ANY_OF: &str = "Any of";
/// The starts of the names of the controls of a condition.
pub const FIELD: &str = "Field";
pub const COMPARISON: &str = "Comparison";
pub const VALUE: &str = "Value";
pub const SECOND_VALUE: &str = "Second value";
/// The start of the name of a row of text that is not a condition. A tap
/// opens Code.
pub const CODE_ROW: &str = "Code row";
/// The values of a yes-or-no field.
pub const YES: &str = "Yes";
pub const NO: &str = "No";
/// The search fields of the menu of text values and of the menu of
/// fields.
pub const SEARCH_VALUES: &str = "Search values";
pub const SEARCH_FIELDS: &str = "Search fields";
/// The word between the two values of Between.
pub const AND: &str = "and";
/// The text of a value that is not set.
const NO_VALUE: &str = "Value";

/// The most rows of the menu of text values.
const MENU_VALUES: usize = 60;
/// The height of a control in a condition row.
const CONTROL: f32 = TOUCH;
/// The space between the controls of a row, and around them.
const SPACE: f32 = 8.0;
const ROW_RADIUS: f32 = 12.0;
/// The height of the row of the heading and the views.
const HEAD: f32 = 48.0;
/// The width of the menu of a control.
const MENU_WIDTH: f32 = 300.0;

/// The name of the button that uses typed text as a text value.
pub fn use_value(text: &str) -> String {
    format!("Use “{text}”")
}

/// The line of the count of the features that the filter matches.
pub fn matches_text(n: usize, m: usize) -> String {
    format!("Matches {n} of {m} features")
}

/// The number of the condition at `path`, from 1, as control names use it.
pub fn number(path: &[usize]) -> String {
    let parts: Vec<String> = path.iter().map(|i| (i + 1).to_string()).collect();
    parts.join(".")
}

/// What the user did in the editor in one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edit {
    pub text: Option<String>,
    /// The change is one step, such as a new condition, and not typing.
    pub step: bool,
}

/// A menu of a condition row.
#[derive(Clone, Debug, PartialEq)]
pub enum Menu {
    /// Match all or Match any, of the list or of a group.
    Match,
    Field,
    Comparison,
    /// The value with this index.
    Value(usize),
}

/// A change of a list of conditions.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Match all or any: the list at the empty path, or a group.
    SetAll(Vec<usize>, bool),
    /// Sets the field, the variable, or the measure.
    SetSubject(Vec<usize>, Subject),
    /// Sets the unit of a measure.
    SetUnit(Vec<usize>, Unit),
    SetOp(Vec<usize>, CmpOp),
    /// Sets value `k`. False for typing.
    SetValue(Vec<usize>, usize, Value, bool),
    /// Adds a value of Is one of, or removes it.
    Toggle(Vec<usize>, Value),
    Remove(Vec<usize>),
    /// Adds a condition to the list of the group at the path.
    AddRule(Vec<usize>),
    AddGroup(Vec<usize>),
    /// Fills an empty condition with a rule.
    Fill(Vec<usize>),
    OpenCode,
    Menu(Option<(Vec<usize>, Menu)>),
}

/// The drawing of condition rows in one frame: the help of the layer, the
/// open menu, and the changes that the user made.
pub struct Rows<'a> {
    pub help: &'a ExprHelp,
    /// The start of each control name.
    pub prefix: String,
    pub menu: Option<(Vec<usize>, Menu)>,
    /// The control of the open menu.
    pub anchor: Option<Rect>,
    pub acts: Vec<Act>,
}

impl Rows<'_> {
    fn name(&self, start: &str, path: &[usize]) -> String {
        if path.is_empty() {
            format!("{}{start}", self.prefix)
        } else {
            format!("{}{start} {}", self.prefix, number(path))
        }
    }

    fn is_open(&self, path: &[usize], menu: &Menu) -> bool {
        self.menu.as_ref() == Some(&(path.to_vec(), menu.clone()))
    }

    /// Opens the menu, or closes it if it is open.
    fn toggle(&mut self, path: &[usize], menu: Menu) {
        let open = self.is_open(path, &menu);
        self.acts
            .push(Act::Menu((!open).then(|| (path.to_vec(), menu))));
    }
}

/// The state of an editor between frames.
#[derive(Clone, Default)]
struct EditorState {
    model: Option<Conditions>,
    /// The text that the model shows. The model reads the text again when
    /// the stored text is different, for example after an undo.
    source: Option<String>,
    code: bool,
    /// The text of the Code field while it has the focus.
    typed: String,
    menu: Option<(Vec<usize>, Menu)>,
    search: String,
}

/// True if no part of the text fits a condition.
fn code_only(model: &Conditions) -> bool {
    !model.items.is_empty() && model.items.iter().all(|c| matches!(c, Cond::Code(_)))
}

/// The filter editor with filter `current`. `salt` tells the editors of
/// different filters apart. `count` is the count of the
/// features that the filter matches and the count of the features. `none`
/// is the text while the filter has no condition.
pub fn editor(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    salt: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
    help: &ExprHelp,
    count: Option<(usize, usize)>,
    none: &str,
) -> Edit {
    let id = ui.id().with(("pad-filter", salt));
    let mut st: EditorState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    if st.source.as_deref() != Some(current) {
        let model = Conditions::from_text(current);
        if st.source.is_none() {
            st.code = code_only(&model);
        }
        st.model = Some(model);
        st.source = Some(current.to_owned());
    }
    let mut edit = Edit::default();

    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEAD), Sense::hover());
    let at = pos2(row.left(), row.center().y);
    let heading = super::rules::FILTER;
    (ui.painter()).text(
        at,
        Align2::LEFT_CENTER,
        heading,
        theme::title(),
        theme::TEXT,
    );
    let views = [BLOCKS, CODE];
    let w = widgets::small_segments_width(ui, &views);
    let seg = Rect::from_min_size(pos2(row.right() - w, row.center().y - 19.0), vec2(w, 38.0));
    let named = (&views[..], &VIEWS[..]);
    if let Some(i) = widgets::small_segments_named(ui, controls, seg, named, usize::from(st.code)) {
        st.code = i == 1;
        st.typed = current.to_owned();
        st.menu = None;
    }

    if st.code {
        let name = super::rules::EXPRESSION;
        let code_id = ui.id().with(name);
        if !ui.memory(|m| m.has_focus(code_id)) {
            st.typed = current.to_owned();
        }
        let valid = help.valid(&st.typed);
        let resp = widgets::code_field(ui, controls, name, name, &mut st.typed, valid);
        if resp.changed() {
            edit.text = Some(st.typed.clone());
        }
        expression::line(ui, help, &st.typed, ExprKind::Filter);
    } else {
        let model = st
            .model
            .get_or_insert_with(|| Conditions::new(true, Vec::new()));
        let mut rows = Rows {
            help,
            prefix: String::new(),
            menu: st.menu.clone(),
            anchor: None,
            acts: Vec::new(),
        };
        blocks(ui, controls, &mut rows, model, none);
        let menu_acts = menu_popover(
            ui,
            controls,
            &mut rows,
            (&model.items, model.all),
            &mut st.search,
        );
        rows.acts.extend(menu_acts);
        let mut step = false;
        let mut changed = false;
        for act in rows.acts {
            match act {
                Act::Menu(m) => {
                    if m.is_some() {
                        st.search.clear();
                    }
                    st.menu = m;
                }
                Act::OpenCode => {
                    st.code = true;
                    st.typed = current.to_owned();
                    st.menu = None;
                }
                Act::SetAll(path, all) if path.is_empty() => {
                    model.all = all;
                    st.menu = None;
                    (changed, step) = (true, true);
                }
                act => {
                    let typing = matches!(act, Act::SetValue(_, _, _, false));
                    let keep = matches!(act, Act::Toggle(..) | Act::SetValue(..));
                    if apply(&mut model.items, act, help) {
                        changed = true;
                        step |= !typing;
                    }
                    if !keep {
                        st.menu = None;
                    }
                }
            }
        }
        if changed {
            let text = model.to_text();
            st.source = Some(text.clone());
            edit.text = Some(text);
            edit.step = step;
        }
    }
    if let Some((n, m)) = count
        && !current.trim().is_empty()
    {
        match_line(ui, n, m);
    }
    ui.data_mut(|d| d.insert_temp(id, st));
    edit
}

/// The rows of Blocks: Match all or any, the conditions, and the rows that
/// add a condition and a group.
fn blocks(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    rows: &mut Rows,
    model: &Conditions,
    none: &str,
) {
    if model.items.is_empty() {
        super::raster::note(ui, none);
    } else {
        match_row(ui, controls, rows, model.all);
    }
    list(ui, controls, rows, &[], &model.items);
    add_row(ui, controls, &rows.name(ADD_CONDITION, &[]), ADD_CONDITION)
        .then(|| rows.acts.push(Act::AddRule(Vec::new())));
    add_row(ui, controls, &rows.name(ADD_GROUP, &[]), ADD_GROUP)
        .then(|| rows.acts.push(Act::AddGroup(Vec::new())));
}

/// The row "Match all of these conditions", with a menu of all and any.
fn match_row(ui: &mut egui::Ui, controls: &mut Controls, rows: &mut Rows, all: bool) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH), Sense::hover());
    let painter = ui.painter();
    let left = painter.text(
        pos2(row.left(), row.center().y),
        Align2::LEFT_CENTER,
        MATCH,
        theme::body(),
        theme::TEXT,
    );
    let word = if all { ALL } else { ANY };
    let w = menu_width(ui, word);
    let button = Rect::from_min_size(
        pos2(left.right() + SPACE, row.center().y - CONTROL / 2.0),
        vec2(w, CONTROL),
    );
    if menu_button(ui, controls, button, &rows.name(MATCH, &[]), word) {
        rows.toggle(&[], Menu::Match);
    }
    if rows.is_open(&[], &Menu::Match) {
        rows.anchor = Some(button);
    }
    let at = pos2(button.right() + SPACE, row.center().y);
    (ui.painter()).text(
        at,
        Align2::LEFT_CENTER,
        OF_THESE,
        theme::body(),
        theme::TEXT,
    );
}

/// The rows of a list of conditions in the group at `group`.
fn list(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    rows: &mut Rows,
    group: &[usize],
    items: &[Cond],
) {
    for (i, cond) in items.iter().enumerate() {
        let mut path = group.to_vec();
        path.push(i);
        match cond {
            Cond::Rule {
                subject,
                op,
                values,
            } => {
                rule_row(ui, controls, rows, &path, (subject, *op, values), None, 0.0);
            }
            Cond::Group { items, .. } if items.is_empty() => {
                hole_row(ui, controls, rows, &path, None, 0.0);
            }
            Cond::Group { all, items } => group_card(ui, controls, rows, &path, *all, items),
            Cond::Code(text) => {
                code_row(ui, controls, rows, &path, text, None, 0.0);
            }
        }
        ui.add_space(6.0);
    }
}

/// The width of `text` in `font`.
fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, theme::TEXT);
    galley.size().x
}

/// The width of a menu button with `text`.
fn menu_width(ui: &egui::Ui, text: &str) -> f32 {
    14.0 + text_width(ui, text, theme::body()) + 6.0 + 16.0 + 10.0
}

/// A button with a text and a chevron, which opens a menu. Returns true on
/// a tap.
fn menu_button(ui: &egui::Ui, controls: &mut Controls, rect: Rect, name: &str, text: &str) -> bool {
    let resp = widgets::hit(ui, controls, rect, name, Sense::click(), true, None);
    let fill = if resp.is_pointer_button_down_on() {
        Color32::from_white_alpha(40)
    } else {
        theme::raise()
    };
    let painter = ui.painter();
    painter.rect_filled(rect, theme::CONTROL_RADIUS - 2.0, fill);
    let at = pos2(rect.left() + 14.0, rect.center().y);
    painter.text(at, Align2::LEFT_CENTER, text, theme::body(), theme::TEXT);
    let chevron = Rect::from_center_size(
        pos2(rect.right() - 10.0 - 8.0, rect.center().y),
        vec2(16.0, 16.0),
    );
    icons::paint(painter, chevron, Icon::Down, 16.0, theme::MUTE);
    resp.clicked()
}

/// A chip that opens a menu: a field chip or a value chip. Returns true on
/// a tap.
fn chip_button(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    look: &Look,
) -> bool {
    let resp = widgets::hit(ui, controls, rect, name, Sense::click(), true, None);
    let m = Metrics::pad();
    let chip = Rect::from_center_size(rect.center(), vec2(rect.width(), m.chip + 4.0));
    paint_chip(
        ui.painter(),
        chip,
        look,
        &m,
        resp.is_pointer_button_down_on(),
    );
    resp.clicked()
}

fn chip_width(ui: &egui::Ui, look: &Look) -> f32 {
    let m = Metrics::pad();
    let mut measure = |s: &str, f: &egui::FontId| text_width(ui, s, f.clone());
    crate::ui::token_field::chip_parts(look, &m, &mut measure).0
}

/// The look of the chip of a subject. A measure shows its noun, and its
/// unit has a menu of its own.
fn field_look(subject: &Subject, help: &ExprHelp) -> Look {
    let kind = match subject {
        Subject::Field(_) => ChipKind::Field,
        _ => ChipKind::Variable,
    };
    Look {
        kind,
        glyph: Some(help.subject_type(subject).map_or(Glyph::Text, Glyph::of)),
        label: subject.label(),
        badge: None,
    }
}

/// True if the field holds numbers.
fn is_number(t: Option<FieldType>) -> bool {
    matches!(t, Some(FieldType::Integer | FieldType::Real))
}

/// The text of a value in a row.
fn value_text(v: &Value) -> String {
    match v {
        Value::Text(s) => s.clone(),
        Value::Number(n) => number_text(*n),
        Value::Bool(true) => YES.to_owned(),
        Value::Bool(false) => NO.to_owned(),
        _ => String::new(),
    }
}

/// One control of a row: its width, and what it is.
enum Piece {
    Field,
    Op,
    /// The value with this index.
    Value(usize),
    And,
    /// The place of the caller.
    Trail,
}

/// The places of pieces of these widths in lines that wrap at `right`,
/// from `x0`: the rectangle of each piece from the top left of its row, and
/// the count of the lines.
fn flow(widths: &[f32], x0: f32, right: f32) -> (Vec<Rect>, usize) {
    let mut places = Vec::new();
    let (mut x, mut line) = (x0, 0usize);
    for &w in widths {
        if x + w > right && x > x0 {
            x = x0;
            line += 1;
        }
        let y = SPACE + line as f32 * (CONTROL + SPACE);
        places.push(Rect::from_min_size(pos2(x, y), vec2(w, CONTROL)));
        x += w + SPACE;
    }
    (places, line + 1)
}

/// The height of a row of `lines` lines.
fn row_height(lines: usize) -> f32 {
    lines as f32 * (CONTROL + SPACE) + SPACE
}

/// Paints the lead word of a row, and gives the left of the first piece.
fn lead_word(ui: &egui::Ui, row: Rect, lead: Option<&str>) -> f32 {
    if let Some(l) = lead {
        let at = pos2(row.left() + SPACE, row.top() + SPACE + CONTROL / 2.0);
        (ui.painter()).text(at, Align2::LEFT_CENTER, l, theme::body(), theme::MUTE);
    }
    SPACE + lead.map_or(0.0, |l| text_width(ui, l, theme::body()) + SPACE)
}

/// The button that removes the condition at `path`, at the top right of
/// the row.
fn remove_button(
    ui: &egui::Ui,
    controls: &mut Controls,
    rows: &mut Rows,
    path: &[usize],
    row: Rect,
) {
    let center = pos2(
        row.right() - SPACE / 2.0 - TOUCH / 2.0,
        row.top() + SPACE + CONTROL / 2.0,
    );
    let button = Rect::from_center_size(center, vec2(TOUCH, TOUCH));
    let name = rows.name(REMOVE_CONDITION, path);
    if widgets::icon_button_at(ui, controls, button, &name, Icon::Close, false, true).clicked() {
        rows.acts.push(Act::Remove(path.to_vec()));
    }
}

/// The row of a rule at `path`: the field chip, the comparison menu, the
/// values, and the button that removes the rule. `lead` is a word before
/// the field, such as "When". `trail` is the width of a place after the
/// values for the caller, or 0 for none. Returns the place.
pub fn rule_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    rows: &mut Rows,
    path: &[usize],
    (subject, op, values): (&Subject, CmpOp, &[Value]),
    lead: Option<&str>,
    trail: f32,
) -> Rect {
    let kind = rows.help.subject_type(subject);
    let look = field_look(subject, rows.help);
    let count = op.value_count().unwrap_or(1);
    let mut pieces = vec![(chip_width(ui, &look), Piece::Field)];
    pieces.push((menu_width(ui, op.label()), Piece::Op));
    for k in 0..count {
        if k > 0 {
            pieces.push((text_width(ui, AND, theme::body()), Piece::And));
        }
        let text = value_label(op, values, k);
        let w = match kind {
            Some(FieldType::Bool) => menu_width(ui, &text),
            Some(FieldType::Integer | FieldType::Real) => {
                (text_width(ui, &text, theme::body()) + 28.0).max(80.0)
            }
            _ => {
                let look = value_look(&text);
                chip_width(ui, &look).max(64.0)
            }
        };
        pieces.push((w, Piece::Value(k)));
    }
    if trail > 0.0 {
        pieces.push((trail, Piece::Trail));
    }
    let widths: Vec<f32> = pieces.iter().map(|(w, _)| *w).collect();
    let (row, places) = simple_row(ui, lead, &widths);
    let mut out = Rect::NOTHING;
    for (rect, (_, piece)) in places.into_iter().zip(pieces) {
        match piece {
            Piece::Field => {
                if chip_button(ui, controls, rect, &rows.name(FIELD, path), &look) {
                    rows.toggle(path, Menu::Field);
                }
                if rows.is_open(path, &Menu::Field) {
                    rows.anchor = Some(rect);
                }
            }
            Piece::Op => {
                let name = rows.name(COMPARISON, path);
                if menu_button(ui, controls, rect, &name, op.label()) {
                    rows.toggle(path, Menu::Comparison);
                }
                if rows.is_open(path, &Menu::Comparison) {
                    rows.anchor = Some(rect);
                }
            }
            Piece::And => {
                let at = pos2(rect.left(), rect.center().y);
                (ui.painter()).text(at, Align2::LEFT_CENTER, AND, theme::body(), theme::MUTE);
            }
            Piece::Value(k) => {
                value_control(ui, controls, rows, path, (kind, op, values, k), rect);
            }
            Piece::Trail => out = rect,
        }
    }
    remove_button(ui, controls, rows, path, row);
    // The keypad of a number value is under the row.
    for k in 0..count {
        if is_number(kind) {
            widgets::keypad_below(ui, controls, &value_name(rows, path, k));
        }
    }
    out
}

/// The name of value `k` of the rule at `path`.
fn value_name(rows: &Rows, path: &[usize], k: usize) -> String {
    rows.name(if k == 0 { VALUE } else { SECOND_VALUE }, path)
}

/// The text of value `k`: the values of Is one of, or one value.
fn value_label(op: CmpOp, values: &[Value], k: usize) -> String {
    if op == CmpOp::OneOf {
        let all: Vec<String> = values.iter().map(value_text).collect();
        return all.join(", ");
    }
    values.get(k).map(value_text).unwrap_or_default()
}

fn value_look(text: &str) -> Look {
    Look {
        kind: ChipKind::Value,
        glyph: None,
        label: if text.is_empty() {
            NO_VALUE.to_owned()
        } else {
            text.to_owned()
        },
        badge: None,
    }
}

/// The control of value `k`: a menu of the values of a text field, the
/// keypad field of a number, or a menu of Yes and No.
fn value_control(
    ui: &egui::Ui,
    controls: &mut Controls,
    rows: &mut Rows,
    path: &[usize],
    (kind, op, values, k): (Option<FieldType>, CmpOp, &[Value], usize),
    rect: Rect,
) {
    let name = value_name(rows, path, k);
    let text = value_label(op, values, k);
    match kind {
        Some(FieldType::Integer | FieldType::Real) => {
            let valid = |t: &str| super::raster::parse_value(t).is_some();
            let field = (rect, false);
            if let Some(typed) =
                widgets::key_field_at(ui, controls, field, &name, "", &text, Keys::Number, valid)
                && let Some(n) = super::raster::parse_value(&typed)
            {
                (rows.acts).push(Act::SetValue(path.to_vec(), k, Value::Number(n), false));
            }
        }
        Some(FieldType::Bool) => {
            if menu_button(ui, controls, rect, &name, &text) {
                rows.toggle(path, Menu::Value(k));
            }
        }
        _ => {
            if chip_button(ui, controls, rect, &name, &value_look(&text)) {
                rows.toggle(path, Menu::Value(k));
            }
        }
    }
    if rows.is_open(path, &Menu::Value(k)) {
        rows.anchor = Some(rect);
    }
}

/// The row of an empty condition: a dashed place that takes a rule, and
/// the remove button. `lead` and `trail` are as in `rule_row`.
pub fn hole_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    rows: &mut Rows,
    path: &[usize],
    lead: Option<&str>,
    trail: f32,
) -> Rect {
    let w = text_width(ui, HOLE, theme::body()) + 28.0;
    let (row, places) = simple_row(ui, lead, &[w, trail]);
    let place = places[0].shrink2(vec2(0.0, 4.0));
    let name = rows.name(HOLE, path);
    let resp = widgets::hit(ui, controls, places[0], &name, Sense::click(), true, None);
    let stroke = Stroke::new(1.5, Color32::from_white_alpha(90));
    crate::ui::token_field::paint::dashed(ui.painter(), place, 8.0, stroke);
    let painter = ui.painter();
    painter.text(
        place.center(),
        Align2::CENTER_CENTER,
        HOLE,
        theme::body(),
        theme::MUTE,
    );
    if resp.clicked() {
        rows.acts.push(Act::Fill(path.to_vec()));
    }
    remove_button(ui, controls, rows, path, row);
    places[1]
}

/// A row with a lead word and pieces of these widths in lines that wrap.
/// A width of 0 has no place. Returns the row and the place of each piece.
fn simple_row(ui: &mut egui::Ui, lead: Option<&str>, widths: &[f32]) -> (Rect, Vec<Rect>) {
    let width = ui.available_width();
    let x0 = SPACE + lead.map_or(0.0, |l| text_width(ui, l, theme::body()) + SPACE);
    let kept: Vec<f32> = widths.iter().copied().filter(|w| *w > 0.0).collect();
    let (places, lines) = flow(&kept, x0, width - SPACE - TOUCH);
    let (row, _) = ui.allocate_exact_size(vec2(width, row_height(lines)), Sense::hover());
    ui.painter()
        .rect_filled(row, ROW_RADIUS, Color32::from_white_alpha(10));
    lead_word(ui, row, lead);
    let mut placed = places.into_iter().map(|r| r.translate(row.min.to_vec2()));
    let out = (widths.iter())
        .map(|w| {
            if *w > 0.0 {
                placed.next().unwrap_or(Rect::NOTHING)
            } else {
                Rect::NOTHING
            }
        })
        .collect();
    (row, out)
}

/// The text and the start of the name of an empty condition.
pub const HOLE: &str = "condition";

/// The row of text that is not a condition, in red, and the remove
/// button. A tap on the text opens Code. `lead` and `trail` are as in
/// `rule_row`.
pub fn code_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    rows: &mut Rows,
    path: &[usize],
    text: &str,
    lead: Option<&str>,
    trail: f32,
) -> Rect {
    let most = ui.available_width() - 3.0 * SPACE - TOUCH;
    let w = (text_width(ui, text, theme::mono()) + 24.0).min(most.max(TOUCH));
    let (row, places) = simple_row(ui, lead, &[w, trail]);
    let place = places[0];
    let name = rows.name(CODE_ROW, path);
    let resp = widgets::hit(ui, controls, place, &name, Sense::click(), true, None);
    ui.painter().rect(
        place.shrink2(vec2(0.0, 4.0)),
        8.0,
        Color32::from_rgba_unmultiplied(239, 107, 107, 30),
        Stroke::new(1.0, BAD.gamma_multiply(0.6)),
        egui::StrokeKind::Inside,
    );
    let at = pos2(place.left() + 12.0, place.center().y);
    let clip = place.shrink2(vec2(12.0, 0.0));
    (ui.painter().with_clip_rect(clip)).text(at, Align2::LEFT_CENTER, text, theme::mono(), BAD);
    if resp.clicked() {
        rows.acts.push(Act::OpenCode);
    }
    remove_button(ui, controls, rows, path, row);
    places[1]
}

/// A group: its menu of All of and Any of, its conditions, the rows that
/// add a condition and a group to it, and its remove button. The frame of
/// the group goes around them.
fn group_card(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    rows: &mut Rows,
    path: &[usize],
    all: bool,
    items: &[Cond],
) {
    let frame = ui.painter().add(Shape::Noop);
    let top = ui.cursor().top();
    let width = ui.available_width();
    let left = ui.cursor().left();
    let (head, _) = ui.allocate_exact_size(vec2(width, TOUCH + SPACE), Sense::hover());
    let word = if all { ALL_OF } else { ANY_OF };
    let button = Rect::from_min_size(
        pos2(head.left() + SPACE, head.top() + SPACE),
        vec2(menu_width(ui, word), CONTROL),
    );
    if menu_button(ui, controls, button, &rows.name(MATCH, path), word) {
        rows.toggle(path, Menu::Match);
    }
    if rows.is_open(path, &Menu::Match) {
        rows.anchor = Some(button);
    }
    let remove = Rect::from_center_size(
        pos2(head.right() - SPACE / 2.0 - TOUCH / 2.0, button.center().y),
        vec2(TOUCH, TOUCH),
    );
    let name = rows.name(REMOVE_GROUP, path);
    if widgets::icon_button_at(ui, controls, remove, &name, Icon::Close, false, true).clicked() {
        rows.acts.push(Act::Remove(path.to_vec()));
    }
    let inner = Rect::from_min_max(
        pos2(left + SPACE, ui.cursor().top() + SPACE / 2.0),
        pos2(left + width - SPACE, f32::INFINITY),
    );
    let layout = egui::Layout::top_down(egui::Align::Min);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
    child.spacing_mut().item_spacing.y = 0.0;
    list(&mut child, controls, rows, path, items);
    let add = rows.name(ADD_CONDITION, path);
    add_row(&mut child, controls, &add, ADD_CONDITION)
        .then(|| rows.acts.push(Act::AddRule(path.to_vec())));
    let add = rows.name(ADD_GROUP, path);
    add_row(&mut child, controls, &add, ADD_GROUP)
        .then(|| rows.acts.push(Act::AddGroup(path.to_vec())));
    let bottom = child.min_rect().bottom() + SPACE / 2.0;
    let rest = (bottom - ui.cursor().top()).max(0.0);
    ui.allocate_exact_size(vec2(width, rest), Sense::hover());
    let rect = Rect::from_min_max(pos2(left, top), pos2(left + width, bottom));
    let stroke = Stroke::new(1.0, Color32::from_white_alpha(36));
    ui.painter().set(
        frame,
        Shape::rect_stroke(rect, ROW_RADIUS, stroke, egui::StrokeKind::Inside),
    );
}

/// A row with a plus and `label` in the accent color. Returns true on a
/// tap.
pub fn add_row(ui: &mut egui::Ui, controls: &mut Controls, name: &str, label: &str) -> bool {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH), Sense::hover());
    let w = 16.0 + 18.0 + 8.0 + text_width(ui, label, theme::body()) + 16.0;
    let rect = Rect::from_min_size(row.min, vec2(w.min(row.width()), row.height()));
    let resp = widgets::hit(ui, controls, rect, name, Sense::click(), true, None);
    if resp.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(rect, theme::CONTROL_RADIUS, theme::raise());
    }
    let icon = Rect::from_center_size(
        pos2(rect.left() + 16.0 + 9.0, row.center().y),
        vec2(18.0, 18.0),
    );
    icons::paint(ui.painter(), icon, Icon::Plus, 18.0, theme::ACCENT);
    let at = pos2(icon.right() + 8.0, row.center().y);
    (ui.painter()).text(at, Align2::LEFT_CENTER, label, theme::body(), theme::ACCENT);
    resp.clicked()
}

/// The line "Matches N of M features" and a meter of the part.
fn match_line(ui: &mut egui::Ui, n: usize, m: usize) {
    ui.add_space(8.0);
    let text = egui::RichText::new(matches_text(n, m))
        .font(theme::body())
        .color(theme::TEXT);
    ui.add(egui::Label::new(text).selectable(false));
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, theme::track());
    let part = if m == 0 { 0.0 } else { n as f32 / m as f32 };
    let fill = Rect::from_min_size(
        rect.min,
        vec2(rect.width() * part.clamp(0.0, 1.0), rect.height()),
    );
    painter.rect_filled(fill, 3.0, theme::ACCENT);
    ui.add_space(4.0);
}

/// The open menu of a row, in a popover at its control. `root_all` is
/// Match all or any of `items`. Returns the changes.
pub fn menu_popover(
    ui: &egui::Ui,
    controls: &mut Controls,
    rows: &mut Rows,
    (items, root_all): (&[Cond], bool),
    search: &mut String,
) -> Vec<Act> {
    let mut acts = Vec::new();
    let Some((path, menu)) = rows.menu.clone() else {
        return acts;
    };
    let Some(anchor) = rows.anchor else {
        // The control is gone, for example after a scroll.
        acts.push(Act::Menu(None));
        return acts;
    };
    let cond = if path.is_empty() {
        None
    } else {
        cond_at(items, &path)
    };
    let help = rows.help;
    let key = format!("pad-filter-menu-{}", rows.prefix);
    let shown = popover::show(ui.ctx(), &key, anchor, MENU_WIDTH, |ui| {
        match (&menu, cond) {
            (Menu::Match, c) => {
                let all = match c {
                    Some(Cond::Group { all, .. }) => *all,
                    _ => root_all,
                };
                let words = if path.is_empty() {
                    [ALL, ANY]
                } else {
                    [ALL_OF, ANY_OF]
                };
                for (i, word) in words.iter().enumerate() {
                    let on = all == (i == 0);
                    if widgets::option_row(ui, controls, word, word, &[], on).clicked() {
                        acts.push(Act::SetAll(path.clone(), i == 0));
                    }
                }
            }
            (Menu::Field, Some(Cond::Rule { subject, .. })) => {
                search_field(ui, controls, SEARCH_FIELDS, search);
                let needle = search.trim().to_lowercase();
                let shown = (help.fields.iter()).filter(|f| f.to_lowercase().contains(&needle));
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for name in shown {
                            let on = subject.field() == Some(name);
                            if widgets::option_row(ui, controls, name, name, &[], on).clicked() {
                                let s = Subject::Field(name.clone());
                                acts.push(Act::SetSubject(path.clone(), s));
                            }
                        }
                    });
            }
            (Menu::Comparison, Some(Cond::Rule { subject, op, .. })) => {
                let kind = help.subject_type(subject).unwrap_or(FieldType::Text);
                for o in ops_for(kind) {
                    let label = o.label();
                    if widgets::option_row(ui, controls, label, label, &[], o == op).clicked() {
                        acts.push(Act::SetOp(path.clone(), *o));
                    }
                }
            }
            (
                Menu::Value(k),
                Some(Cond::Rule {
                    subject,
                    op,
                    values,
                }),
            ) => {
                if help.subject_type(subject) == Some(FieldType::Bool) {
                    for (word, b) in [(YES, true), (NO, false)] {
                        let on = values.get(*k) == Some(&Value::Bool(b));
                        if widgets::option_row(ui, controls, word, word, &[], on).clicked() {
                            acts.push(Act::SetValue(path.clone(), *k, Value::Bool(b), true));
                        }
                    }
                } else {
                    text_values(
                        ui,
                        controls,
                        help,
                        (&path, subject, *op, values, *k),
                        search,
                        &mut acts,
                    );
                }
            }
            _ => {}
        }
    });
    if shown.closed {
        acts.push(Act::Menu(None));
    }
    acts
}

/// A search field at the top of a menu, with `name` as its hint.
pub fn search_field(ui: &mut egui::Ui, controls: &mut Controls, name: &str, search: &mut String) {
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH), Sense::hover());
    let font = theme::body();
    widgets::field_at(ui, controls, rect, name, None, search, true, font);
    if search.is_empty() {
        let at = pos2(rect.left() + 12.0, rect.center().y);
        (ui.painter()).text(at, Align2::LEFT_CENTER, name, theme::body(), theme::MUTE);
    }
    ui.add_space(4.0);
}

/// The rows of the menu of text values: the search field, the typed text
/// as a value, and the values of the field in the layer that hold the
/// search text.
fn text_values(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    help: &ExprHelp,
    (path, subject, op, values, k): (&[usize], &Subject, CmpOp, &[Value], usize),
    search: &mut String,
    acts: &mut Vec<Act>,
) {
    search_field(ui, controls, SEARCH_VALUES, search);
    let needle = search.trim().to_lowercase();
    let all = subject.field().map_or(&[][..], |f| help.values(f));
    let shown: Vec<&String> = (all.iter())
        .filter(|v| v.to_lowercase().contains(&needle))
        .take(MENU_VALUES)
        .collect();
    let one_of = op == CmpOp::OneOf;
    let pick = |v: &str, acts: &mut Vec<Act>| {
        let value = Value::Text(v.to_owned());
        if one_of {
            acts.push(Act::Toggle(path.to_vec(), value));
        } else {
            acts.push(Act::SetValue(path.to_vec(), k, value, true));
        }
    };
    let typed = search.trim();
    if !typed.is_empty() && !all.iter().any(|v| v == typed) {
        let name = use_value(typed);
        if widgets::option_row(ui, controls, &name, &name, &[], false).clicked() {
            pick(typed, acts);
        }
    }
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .show(ui, |ui| {
            for v in shown {
                let value = Value::Text(v.clone());
                let on = if one_of {
                    values.contains(&value)
                } else {
                    values.get(k) == Some(&value)
                };
                if widgets::option_row(ui, controls, v, v, &[], on).clicked() {
                    pick(v, acts);
                }
            }
        });
}

/// The condition at `path` in `items`.
pub fn cond_at<'a>(items: &'a [Cond], path: &[usize]) -> Option<&'a Cond> {
    let (first, rest) = path.split_first()?;
    let c = items.get(*first)?;
    if rest.is_empty() {
        return Some(c);
    }
    match c {
        Cond::Group { items, .. } => cond_at(items, rest),
        _ => None,
    }
}

fn cond_mut<'a>(items: &'a mut [Cond], path: &[usize]) -> Option<&'a mut Cond> {
    let (first, rest) = path.split_first()?;
    let c = items.get_mut(*first)?;
    if rest.is_empty() {
        return Some(c);
    }
    match c {
        Cond::Group { items, .. } => cond_mut(items, rest),
        _ => None,
    }
}

/// The list of the group at `path`, or `items` for the empty path.
fn list_mut<'a>(items: &'a mut Vec<Cond>, path: &[usize]) -> Option<&'a mut Vec<Cond>> {
    if path.is_empty() {
        return Some(items);
    }
    match cond_mut(items, path)? {
        Cond::Group { items, .. } => Some(items),
        _ => None,
    }
}

/// The value of a new rule on a subject of type `kind`: the first value of
/// a text field in the layer, 0, or yes.
fn default_value(help: &ExprHelp, subject: &Subject, kind: Option<FieldType>) -> Value {
    match kind {
        Some(FieldType::Integer | FieldType::Real) => Value::Number(0.0),
        Some(FieldType::Bool) => Value::Bool(true),
        _ => {
            let values = subject.field().map_or(&[][..], |f| help.values(f));
            Value::Text(values.first().cloned().unwrap_or_default())
        }
    }
}

/// True if value `v` fits a field of type `kind`.
fn fits(v: &Value, kind: Option<FieldType>) -> bool {
    match kind {
        Some(FieldType::Integer | FieldType::Real) => matches!(v, Value::Number(_)),
        Some(FieldType::Bool) => matches!(v, Value::Bool(_)),
        _ => matches!(v, Value::Text(_)),
    }
}

/// The values for comparison `op` on `subject`: the values that fit, and
/// default values for the rest.
fn values_for(help: &ExprHelp, subject: &Subject, op: CmpOp, old: &[Value]) -> Vec<Value> {
    let kind = help.subject_type(subject);
    let count = op.value_count().unwrap_or(old.len().max(1));
    (0..count)
        .map(|i| match old.get(i) {
            Some(v) if fits(v, kind) => v.clone(),
            _ => default_value(help, subject, kind),
        })
        .collect()
}

/// A new rule on the first field, or on the first measure of a layer with
/// no fields.
pub fn new_rule(help: &ExprHelp) -> Cond {
    let subject = match help.fields.first() {
        Some(f) => Subject::Field(f.clone()),
        None => (help.measures().into_iter().next()).unwrap_or(Subject::Field(String::new())),
    };
    let kind = help.subject_type(&subject).unwrap_or(FieldType::Text);
    let op = ops_for(kind)[0];
    let values = values_for(help, &subject, op, &[]);
    Cond::Rule {
        subject,
        op,
        values,
    }
}

/// Applies a change to `items`. Returns true if the conditions changed.
pub fn apply(items: &mut Vec<Cond>, act: Act, help: &ExprHelp) -> bool {
    match act {
        Act::SetAll(path, value) => match cond_mut(items, &path) {
            Some(Cond::Group { all, .. }) => {
                *all = value;
                true
            }
            _ => false,
        },
        Act::SetSubject(path, new) => {
            let Some(Cond::Rule {
                subject,
                op,
                values,
            }) = cond_mut(items, &path)
            else {
                return false;
            };
            // A measure keeps its unit.
            let new = if new.same_kind(subject) {
                subject.clone()
            } else {
                new
            };
            let kind = help.subject_type(&new).unwrap_or(FieldType::Text);
            if !ops_for(kind).contains(op) {
                *op = ops_for(kind)[0];
            }
            *values = values_for(help, &new, *op, values);
            *subject = new;
            true
        }
        Act::SetUnit(path, unit) => match cond_mut(items, &path) {
            Some(Cond::Rule {
                subject: Subject::Measure(m, u),
                ..
            }) if m.units().contains(&unit) => {
                *u = Some(unit);
                true
            }
            _ => false,
        },
        Act::SetOp(path, new) => {
            let Some(Cond::Rule {
                subject,
                op,
                values,
            }) = cond_mut(items, &path)
            else {
                return false;
            };
            *values = values_for(help, subject, new, values);
            *op = new;
            true
        }
        Act::SetValue(path, k, v, _) => {
            let Some(Cond::Rule { values, .. }) = cond_mut(items, &path) else {
                return false;
            };
            while values.len() <= k {
                values.push(v.clone());
            }
            values[k] = v;
            true
        }
        Act::Toggle(path, v) => {
            let Some(Cond::Rule { values, .. }) = cond_mut(items, &path) else {
                return false;
            };
            if let Some(i) = values.iter().position(|x| *x == v) {
                if values.len() > 1 {
                    values.remove(i);
                }
            } else {
                values.push(v);
            }
            true
        }
        Act::Remove(path) => {
            let Some((last, parent)) = path.split_last() else {
                return false;
            };
            let Some(list) = list_mut(items, parent) else {
                return false;
            };
            if *last >= list.len() {
                return false;
            }
            list.remove(*last);
            true
        }
        Act::AddRule(path) => {
            let Some(list) = list_mut(items, &path) else {
                return false;
            };
            list.push(new_rule(help));
            true
        }
        Act::AddGroup(path) => {
            let Some(list) = list_mut(items, &path) else {
                return false;
            };
            list.push(Cond::Group {
                all: false,
                items: vec![new_rule(help)],
            });
            true
        }
        Act::Fill(path) => {
            let Some(c) = cond_mut(items, &path) else {
                return false;
            };
            *c = new_rule(help);
            true
        }
        Act::OpenCode | Act::Menu(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_field_keeps_the_comparison_that_fits() {
        let help = ExprHelp::empty();
        let mut items = vec![Cond::Rule {
            subject: Subject::Field("a".into()),
            op: CmpOp::Contains,
            values: vec![Value::Text("x".into())],
        }];
        assert!(apply(&mut items, Act::AddGroup(Vec::new()), &help));
        assert!(apply(&mut items, Act::AddRule(vec![1]), &help));
        let Cond::Group { items: inner, .. } = &items[1] else {
            panic!("{items:?}");
        };
        assert_eq!(inner.len(), 2);
        assert!(apply(&mut items, Act::Remove(vec![1, 0]), &help));
        assert!(apply(&mut items, Act::SetOp(vec![0], CmpOp::Empty), &help));
        assert!(matches!(&items[0], Cond::Rule { values, .. } if values.is_empty()));
        let b = Subject::Field("b".into());
        assert!(apply(&mut items, Act::SetSubject(vec![0], b), &help));
        let text = Conditions::new(true, items).to_text();
        assert!(text.starts_with("b is null and "), "{text}");
    }
}
