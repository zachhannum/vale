//! The expression fields of the desktop layout. The label text and a value
//! are token fields with completion and an Insert menu. A filter is a line
//! of tokens, and a click on it opens the conditions in a popover.

use eframe::egui::{self, Align2, Key, Modifiers, Rect, Sense, Stroke, pos2, vec2};
use vale_store::Value;
use vale_style::expr::editor::FieldFormat;
use vale_style::expr::editor::{Formula, Piece, Seq, Subject, Template, Tok};
use vale_style::expr::{Category, FUNCTIONS, Function, Var};

use super::expr_help::{
    ExprHelp, ExprKind, FIELDS, FUNCTIONS_MENU, GEOMETRY_MENU, INSERT, VARIABLES_MENU, desktop_line,
};
use super::pad::{filter, popover, theme};
use super::token_field::formula::{self, Completion};
use super::token_field::paint::{self, FUNCTION_TEXT, VARIABLE_TEXT, paint_glyph};
use super::token_field::{self, Caret, Field, Glyph, Metrics, Node, Options, template};

/// The hints at the foot of the completion list.
pub const INSERT_HINT: &str = "↩ Insert";
pub const NEXT_SLOT_HINT: &str = "Tab Next slot";
pub const KEEP_TEXT_HINT: &str = "Esc Keep text";
pub const TYPE_BRACE_HINT: &str = "Esc Type “{”";
/// The note of a function in the completion list.
pub const FUNCTION_NOTE: &str = "Function";

/// The height of a row of the completion list.
const ROW: f32 = 26.0;
/// The most rows that the completion list shows.
const MOST_ROWS: usize = 8;
/// The least width of the completion list.
const MIN_WIDTH: f32 = 300.0;
/// The width of the popover of a filter.
const FILTER_WIDTH: f32 = 440.0;

/// The control name of the row of the completion list for `label`.
pub fn completion_name(label: &str) -> String {
    format!("Complete {label}")
}

/// An item of the completion list.
#[derive(Clone, Debug, PartialEq)]
enum Item {
    Field(String),
    /// A measure or a variable of the geometry, in label text.
    Measure(Subject),
    /// A variable, in a value.
    Var(Var),
    /// A function. In label text, the call with its slots filled.
    Function(&'static Function, Option<Seq>),
}

impl Item {
    fn label(&self) -> String {
        match self {
            Item::Field(name) => name.clone(),
            Item::Measure(s) => s.label(),
            Item::Var(v) => format!("@{}", v.name()),
            Item::Function(_, Some(seq)) => seq.to_text(),
            Item::Function(f, None) => f.signature.to_owned(),
        }
    }
}

/// The state of a token field between frames.
#[derive(Clone)]
struct State<P> {
    field: Field<P>,
    /// The text that the field shows. The field reads the text again when
    /// the stored text is different, for example after an undo.
    source: Option<String>,
    /// The highlighted row of the completion list.
    highlight: usize,
    /// The start of the typed word whose completion the user closed with
    /// Escape.
    dismissed: Option<Caret>,
    /// The completion list showed in the last frame.
    open: bool,
}

impl<P> Default for State<P> {
    fn default() -> Self {
        State {
            field: Field::default(),
            source: None,
            highlight: 0,
            dismissed: None,
            open: false,
        }
    }
}

/// The keys of a field with an open completion list in one frame.
#[derive(Default)]
struct Keys {
    up: bool,
    down: bool,
    enter: bool,
    tab: bool,
}

/// Takes the keys that the completion list uses before the field sees
/// them. Tab and Return go to the caller also with no list. Escape takes
/// the focus away before the field sees it, so the caller reads it with
/// `escaped`.
fn take_keys(ui: &egui::Ui, open: bool) -> Keys {
    ui.input_mut(|i| {
        let mut take = |key| i.consume_key(Modifiers::NONE, key);
        Keys {
            up: open && take(Key::ArrowUp),
            down: open && take(Key::ArrowDown),
            enter: take(Key::Enter),
            tab: take(Key::Tab),
        }
    })
}

/// True if the user pressed Escape while the completion list of the field
/// `field_id` showed. The field gets the focus back.
fn escaped(ui: &egui::Ui, field_id: egui::Id, was_open: bool) -> bool {
    let escape = was_open && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
    if escape {
        ui.memory_mut(|m| m.request_focus(field_id));
    }
    escape
}

/// The row of `heading` and the Insert menu, with a submenu for each kind
/// of item that `items` holds. Returns the item that the user picked in the
/// menu.
fn heading_row(ui: &mut egui::Ui, heading: &str, items: &[Item]) -> Option<Item> {
    let mut chosen = None;
    let groups = [FIELDS, GEOMETRY_MENU, VARIABLES_MENU, FUNCTIONS_MENU];
    let group_of = |i: &Item| match i {
        Item::Field(_) => 0,
        Item::Measure(_) => 1,
        Item::Var(_) => 2,
        Item::Function(..) => 3,
    };
    ui.horizontal(|ui| {
        ui.label(heading);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(format!("{INSERT} ⏷"), |ui| {
                for (k, title) in groups.into_iter().enumerate() {
                    let group: Vec<&Item> = items.iter().filter(|i| group_of(i) == k).collect();
                    if group.is_empty() {
                        continue;
                    }
                    ui.menu_button(title, |ui| {
                        for item in group {
                            let hint = match item {
                                Item::Function(f, _) => f.summary,
                                Item::Var(v) => v.hint(),
                                _ => "",
                            };
                            let mut button = ui.button(item.label());
                            if !hint.is_empty() {
                                button = button.on_hover_text(hint);
                            }
                            if button.clicked() {
                                chosen = Some(item.clone());
                            }
                        }
                    });
                }
            })
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, INSERT));
        });
    });
    chosen
}

/// The label text field `name` under `heading`. Typed text stays text, and
/// `{` opens the completion of the fields and of the text functions.
/// Returns true if the text changed.
pub fn label_text(
    ui: &mut egui::Ui,
    (heading, name): (&str, &str),
    text: &mut String,
    help: &ExprHelp,
) -> bool {
    let id = ui.id().with((name, "desk-label-text"));
    let mut st: State<Piece> = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    if st.source.as_deref() != Some(text.as_str()) {
        let sel = st.field.sel.clone();
        st.field = template::field_of(&Template::from_text(text), help);
        if ui.memory(|m| m.has_focus(ui.id().with(name))) {
            st.field.sel = sel;
            st.field.clamp();
        }
        st.source = Some(text.clone());
    }
    let menu_items = text_items("", help, true);
    let mut changed = false;
    if let Some(item) = heading_row(ui, heading, &menu_items) {
        insert_text_item(&mut st.field, &item, None, help);
        ui.memory_mut(|m| m.request_focus(ui.id().with(name)));
        changed = true;
    }

    let field_id = ui.id().with(name);
    let query = brace_query(&st.field).filter(|(at, _)| st.dismissed.as_ref() != Some(at));
    if escaped(ui, field_id, st.open) {
        st.dismissed = query.as_ref().map(|(at, _)| at.clone());
    }
    let query = query.filter(|(at, _)| st.dismissed.as_ref() != Some(at));
    let focused = ui.memory(|m| m.has_focus(field_id));
    let items = match (&query, focused) {
        (Some((_, q)), true) => text_items(q, help, false),
        _ => Vec::new(),
    };
    let open = !items.is_empty();
    let keys = if focused {
        take_keys(ui, open)
    } else {
        Keys::default()
    };
    move_highlight(&mut st.highlight, &keys, items.len());
    if open && (keys.enter || keys.tab) {
        let at = query.as_ref().map(|(at, _)| at.clone());
        insert_text_item(&mut st.field, &items[st.highlight], at, help);
        changed = true;
    } else if keys.enter && focused {
        st.field.replace(vec![Node::Break]);
        changed = true;
    }
    let opts = Options {
        literal: true,
        enter_breaks: false,
        min_height: 0.0,
        long_press: false,
    };
    let m = Metrics::desktop();
    let out = token_field::show(ui, &mut Vec::new(), name, &mut st.field, &m, &opts);
    changed |= out.changed;
    if open && !changed && out.focused {
        let labels = rows_of(&items, help);
        let foot = [INSERT_HINT, TYPE_BRACE_HINT];
        if let Some(i) = completion_list(ui, out.response.rect, &labels, st.highlight, &foot) {
            let at = query.as_ref().map(|(at, _)| at.clone());
            insert_text_item(&mut st.field, &items[i], at, help);
            changed = true;
        }
    }
    if !open {
        st.highlight = 0;
    }
    st.open = open && !changed && out.focused;
    if changed {
        template::relabel(&mut st.field);
        *text = template::template_of(&st.field).to_text();
        st.source = Some(text.clone());
    }
    desktop_line(ui, help, text, ExprKind::Text);
    ui.data_mut(|d| d.insert_temp(id, st));
    changed
}

/// The `{` before the caret and the word between it and the caret, if the
/// word is a name or empty.
fn brace_query(field: &Field<Piece>) -> Option<(Caret, String)> {
    if !field.sel.is_empty() {
        return None;
    }
    let Caret { path, index } = field.sel.head.clone();
    let list = field.list(&path)?;
    let mut word = Vec::new();
    for i in (0..index.min(list.len())).rev() {
        match &list[i] {
            Node::Char('{') => {
                let q: String = word.iter().rev().collect();
                return Some((Caret::new(path, i), q));
            }
            Node::Char(c) if c.is_alphanumeric() || *c == '_' => word.push(*c),
            _ => return None,
        }
    }
    None
}

/// The items of the label text for query `q`: the fields whose names start
/// with it, a `coalesce` of the first two text fields, the text functions
/// whose names start with it, on the first text field, and the measures
/// whose names start with it. With `all`, all of them.
fn text_items(q: &str, help: &ExprHelp, all: bool) -> Vec<Item> {
    let q = q.to_lowercase();
    let fields: Vec<&String> = (help.fields.iter())
        .filter(|f| all || f.to_lowercase().starts_with(&q))
        .collect();
    let is_text = |f: &str| help.field_type(f) == Some(vale_store::FieldType::Text);
    let mut out: Vec<Item> = fields.iter().map(|f| Item::Field((*f).clone())).collect();
    let texts: Vec<&String> = fields.iter().copied().filter(|f| is_text(f)).collect();
    let first_text = texts
        .first()
        .copied()
        .or_else(|| help.fields.iter().find(|f| is_text(f)));
    if !all
        && !q.is_empty()
        && let [a, b, ..] = texts.as_slice()
        && let Some(f) = formula::function_named("coalesce")
    {
        let args = vec![field_seq(b), field_seq(a)];
        out.push(Item::Function(f, Some(Seq(vec![Tok::Call { f, args }]))));
    }
    if let Some(field) = first_text {
        let functions = (FUNCTIONS.iter())
            .filter(|f| f.category == Category::Text && f.name != "concat")
            .filter(|f| all || !q.is_empty() && f.name.starts_with(&q));
        for f in functions {
            let mut args = vec![field_seq(field)];
            while args.len() < f.min_args.max(1) {
                args.push(Seq(vec![Tok::Num(0.0)]));
            }
            out.push(Item::Function(f, Some(Seq(vec![Tok::Call { f, args }]))));
        }
    }
    let measures = (help.measures().into_iter())
        .filter(|s| all || s.label().to_lowercase().starts_with(&q))
        .map(Item::Measure);
    out.extend(measures);
    out
}

fn field_seq(name: &str) -> Seq {
    Seq(vec![Tok::Field(name.to_owned())])
}

/// The note of an item of the label text: the sample value of a field, or
/// "Function".
fn text_note(item: &Item, help: &ExprHelp) -> String {
    match item {
        Item::Field(name) => help.sample(name).and_then(Value::label).unwrap_or_default(),
        Item::Measure(s) => {
            let format = FieldFormat::for_subject(s);
            let text = Template::new(vec![Piece::Field {
                subject: s.clone(),
                format,
            }])
            .to_text();
            help.eval(&text).and_then(|v| v.label()).unwrap_or_default()
        }
        Item::Var(v) => v.hint().to_owned(),
        Item::Function(..) => FUNCTION_NOTE.to_owned(),
    }
}

/// Puts `item` at the caret of a label text field, in place of the `{` at
/// `brace` and the word after it.
fn insert_text_item(field: &mut Field<Piece>, item: &Item, brace: Option<Caret>, help: &ExprHelp) {
    if let Some(at) = brace {
        let head = field.sel.head.clone();
        field.sel = token_field::Selection::range(at.path, at.index, head.index);
    }
    let piece = match item {
        Item::Field(name) => Piece::Field {
            subject: Subject::Field(name.clone()),
            format: Default::default(),
        },
        Item::Measure(s) => Piece::Field {
            subject: s.clone(),
            format: FieldFormat::for_subject(s),
        },
        Item::Var(v) => Piece::Field {
            subject: Subject::Var(*v),
            format: Default::default(),
        },
        Item::Function(_, Some(seq)) => Piece::Expr(seq.clone()),
        Item::Function(f, None) => Piece::Expr(Seq(vec![Tok::Call {
            f,
            args: vec![Seq(Vec::new()); f.min_args.max(1)],
        }])),
    };
    field.replace(template::nodes_of(&[piece], help));
}

/// The value field `name` under `heading`. A typed word opens the
/// completion of the fields and the functions. Returns true if the text
/// changed.
pub fn value(
    ui: &mut egui::Ui,
    (heading, name): (&str, &str),
    text: &mut String,
    help: &ExprHelp,
) -> bool {
    let id = ui.id().with((name, "desk-value"));
    let mut st: State<Tok> = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let field_id = ui.id().with(name);
    let focused = ui.memory(|m| m.has_focus(field_id));
    if st.source.as_deref() != Some(text.as_str()) {
        let sel = st.field.sel.clone();
        st.field = formula::field_of(&Formula::from_text(text), help);
        if focused {
            st.field.sel = sel;
            st.field.clamp();
        }
        st.source = Some(text.clone());
    }
    let mut menu_items: Vec<Item> = help.fields.iter().cloned().map(Item::Field).collect();
    menu_items.extend(Var::ALL.into_iter().map(Item::Var));
    menu_items.extend(FUNCTIONS.iter().map(|f| Item::Function(f, None)));
    let mut changed = false;
    if let Some(item) = heading_row(ui, heading, &menu_items) {
        match item {
            Item::Field(name) => formula::insert_field(&mut st.field, &name, help),
            Item::Var(v) => formula::insert_var(&mut st.field, v, help),
            Item::Function(f, _) => formula::insert_function(&mut st.field, f),
            Item::Measure(_) => {}
        }
        ui.memory_mut(|m| m.request_focus(field_id));
        changed = true;
    }

    let word = formula::pending(&st.field);
    let start = word_start(&st.field, &word);
    if escaped(ui, field_id, st.open) {
        st.dismissed = Some(start.clone());
    }
    let focused = ui.memory(|m| m.has_focus(field_id));
    let items: Vec<Item> = match focused && st.dismissed != Some(start.clone()) {
        true => formula::completions(&word, help)
            .into_iter()
            .map(|c| match c {
                Completion::Field(f) => Item::Field(f),
                Completion::Function(f) => Item::Function(f, None),
                Completion::Var(v) => Item::Var(v),
            })
            .collect(),
        false => Vec::new(),
    };
    let open = !items.is_empty();
    let keys = if focused {
        take_keys(ui, open)
    } else {
        Keys::default()
    };
    move_highlight(&mut st.highlight, &keys, items.len());
    if open && keys.enter {
        formula::pick(&mut st.field, &completion_of(&items[st.highlight]), help);
        changed = true;
    } else if keys.enter {
        changed |= formula::commit(&mut st.field, help);
    }
    if keys.tab {
        formula::commit(&mut st.field, help);
        formula::next_slot(&mut st.field);
        changed = true;
    }
    let opts = Options {
        literal: false,
        enter_breaks: false,
        min_height: 0.0,
        long_press: false,
    };
    let m = Metrics::desktop();
    let out = token_field::show(ui, &mut Vec::new(), name, &mut st.field, &m, &opts);
    changed |= out.changed;
    if !out.typed.is_empty() {
        formula::type_text(&mut st.field, &out.typed, help);
        changed = true;
    }
    if !out.pasted.is_empty() {
        formula::paste(&mut st.field, &out.pasted, help);
        changed = true;
    }
    if open && !changed && out.focused {
        let labels = rows_of(&items, help);
        let foot = [INSERT_HINT, NEXT_SLOT_HINT, KEEP_TEXT_HINT];
        if let Some(i) = completion_list(ui, out.response.rect, &labels, st.highlight, &foot) {
            formula::pick(&mut st.field, &completion_of(&items[i]), help);
            changed = true;
        }
    }
    if !open {
        st.highlight = 0;
    }
    st.open = open && !changed && out.focused;
    if !out.focused && has_chars(&st.field.nodes) {
        // The typed words become tokens when the field loses the focus.
        let typed = formula::text_of(&st.field.nodes);
        st.field = formula::field_of(&Formula::from_text(&typed), help);
        changed = true;
    }
    if changed {
        *text = formula::text_of(&st.field.nodes);
        st.source = Some(text.clone());
    }
    desktop_line(ui, help, text, ExprKind::Value);
    ui.data_mut(|d| d.insert_temp(id, st));
    changed
}

/// The place of the first char of the typed word before the caret.
fn word_start(field: &Field<Tok>, word: &str) -> Caret {
    let head = field.sel.head.clone();
    let n = word.chars().count();
    Caret::new(head.path, head.index.saturating_sub(n))
}

/// The completion of an item of a value. A value lists no measures.
fn completion_of(item: &Item) -> Completion {
    match item {
        Item::Field(name) => Completion::Field(name.clone()),
        Item::Function(f, _) => Completion::Function(f),
        Item::Var(v) | Item::Measure(Subject::Var(v)) => Completion::Var(*v),
        Item::Measure(s) => Completion::Field(s.label()),
    }
}

/// True if the nodes hold a char, also in a box.
fn has_chars(nodes: &[Node<Tok>]) -> bool {
    nodes.iter().any(|n| match n {
        Node::Char(_) => true,
        Node::Box(b) => has_chars(&b.children),
        _ => false,
    })
}

/// The rows of the completion list: each item, its note, and the type
/// glyph of a field.
fn rows_of(items: &[Item], help: &ExprHelp) -> Vec<(Item, String, Option<Glyph>)> {
    let glyph = |i: &Item| match i {
        Item::Field(f) => help.field_type(f).map(Glyph::of),
        Item::Measure(s) => help.subject_type(s).map(Glyph::of),
        Item::Var(v) => Some(Glyph::of(v.field_type())),
        Item::Function(..) => None,
    };
    (items.iter())
        .map(|i| (i.clone(), text_note(i, help), glyph(i)))
        .collect()
}

/// Moves the highlight with the arrow keys, and keeps it in the list.
fn move_highlight(highlight: &mut usize, keys: &Keys, len: usize) {
    if len == 0 {
        return;
    }
    if keys.down {
        *highlight = (*highlight + 1) % len;
    }
    if keys.up {
        *highlight = (*highlight + len - 1) % len;
    }
    *highlight = (*highlight).min(len - 1);
}

/// The completion list under the field at `field`: a row for each item,
/// with its note at the right, and the hints at the foot. Returns the row
/// that the user clicked.
fn completion_list(
    ui: &egui::Ui,
    field: Rect,
    items: &[(Item, String, Option<Glyph>)],
    highlight: usize,
    foot: &[&str],
) -> Option<usize> {
    let ctx = ui.ctx().clone();
    let m = Metrics::desktop();
    let rows = items.len().min(MOST_ROWS);
    let first = highlight.saturating_sub(rows - 1);
    let height = rows as f32 * ROW + 28.0 + 8.0;
    let width = field.width().max(MIN_WIDTH);
    let screen = ctx.content_rect();
    let left = field.left().min(screen.right() - 8.0 - width);
    let rect = Rect::from_min_size(pos2(left, field.bottom() + 4.0), vec2(width, height));
    token_field::keep_focus(&ctx, rect);
    let mut chosen = None;
    egui::Area::new(ui.id().with("completion"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .movable(false)
        .constrain(false)
        .fade_in(false)
        .show(&ctx, |ui| {
            let (card, _) = ui.allocate_exact_size(rect.size(), Sense::click());
            let painter = ui.painter();
            painter.add(theme::shadow().as_shape(card, 8.0));
            painter.rect_filled(card, 8.0, popover::FILL);
            let mut y = card.top() + 4.0;
            for (i, (item, note, glyph)) in items.iter().enumerate().skip(first).take(rows) {
                let row =
                    Rect::from_min_size(pos2(card.left() + 4.0, y), vec2(card.width() - 8.0, ROW));
                y += ROW;
                let label = item.label();
                let resp = ui.interact(row, ui.id().with(("row", i)), Sense::click());
                let name = completion_name(&label);
                resp.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        true,
                        i == highlight,
                        &name,
                    )
                });
                if resp.clicked() {
                    chosen = Some(i);
                }
                let painter = ui.painter();
                if i == highlight {
                    painter.rect_filled(row, 5.0, theme::ACCENT.gamma_multiply(0.55));
                } else if resp.hovered() {
                    painter.rect_filled(row, 5.0, theme::raise());
                }
                let mid = row.center().y;
                let (color, font) = match item {
                    Item::Field(_) => (theme::TEXT, egui::FontId::proportional(13.0)),
                    Item::Measure(_) | Item::Var(_) => {
                        (VARIABLE_TEXT, egui::FontId::proportional(13.0))
                    }
                    Item::Function(..) => (FUNCTION_TEXT, egui::FontId::monospace(12.5)),
                };
                let mut x = row.left() + 8.0;
                match (item, glyph) {
                    (Item::Field(_) | Item::Measure(_) | Item::Var(_), Some(g)) => {
                        paint_glyph(painter, pos2(x + m.glyph / 2.0, mid), *g, &m, false);
                        x += m.glyph + 8.0;
                    }
                    (Item::Function(..), _) => {
                        let r = Rect::from_center_size(
                            pos2(x + m.glyph / 2.0, mid),
                            vec2(m.glyph, m.glyph),
                        );
                        painter.rect_filled(r, 4.0, FUNCTION_TEXT.gamma_multiply(0.25));
                        painter.text(
                            r.center(),
                            Align2::CENTER_CENTER,
                            "ƒ",
                            m.glyph_font.clone(),
                            FUNCTION_TEXT,
                        );
                        x += m.glyph + 8.0;
                    }
                    _ => {}
                }
                let note_w = if note.is_empty() {
                    0.0
                } else {
                    let galley = painter.layout_no_wrap(note.clone(), theme::small(), theme::MUTE);
                    let w = galley.size().x;
                    let at = pos2(row.right() - 8.0 - w, mid - galley.size().y / 2.0);
                    painter.galley(at, galley, theme::MUTE);
                    w + 16.0
                };
                let clip = Rect::from_min_max(
                    pos2(x, row.top()),
                    pos2(row.right() - 8.0 - note_w, row.bottom()),
                );
                (painter.with_clip_rect(clip)).text(
                    pos2(x, mid),
                    Align2::LEFT_CENTER,
                    &label,
                    font,
                    color,
                );
            }
            let line_y = y + 2.0;
            let line = [
                pos2(card.left() + 8.0, line_y),
                pos2(card.right() - 8.0, line_y),
            ];
            ui.painter()
                .line_segment(line, Stroke::new(1.0, theme::divider()));
            let mut x = card.left() + 12.0;
            for hint in foot {
                let r = ui.painter().text(
                    pos2(x, line_y + 14.0),
                    Align2::LEFT_CENTER,
                    hint,
                    theme::small(),
                    theme::MUTE,
                );
                x = r.right() + 18.0;
            }
        });
    chosen
}

/// The filter field `name`: the filter as a line of tokens, or `none`
/// while the filter has no condition. A click opens the conditions editor
/// in a popover. `count` is the count of the features that the filter
/// matches and the count of the features. Returns true if the text changed.
pub fn filter(
    ui: &mut egui::Ui,
    name: &str,
    text: &mut String,
    help: &ExprHelp,
    count: Option<(usize, usize)>,
    none: &str,
) -> bool {
    let id = ui.id().with((name, "desk-filter"));
    let mut open: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    ui.label(name);
    let m = Metrics::desktop();
    let field = formula::field_of(&Formula::from_text(text), help);
    let width = ui.available_width();
    let ctx = ui.ctx().clone();
    let mut measure = |s: &str, font: &egui::FontId| {
        ctx.fonts_mut(|f| s.chars().map(|c| f.glyph_width(font, c)).sum::<f32>())
    };
    let mut lay = token_field::layout(&field.nodes, &m, width - 2.0 * m.pad.x, &mut measure);
    let height = lay.size.y.max(m.chip) + 2.0 * m.pad.y;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    lay.translate((rect.min + m.pad).to_vec2());
    let painter = ui.painter_at(rect.expand(2.0));
    painter.rect_filled(rect, 6.0, paint::FIELD_FILL);
    if open || resp.hovered() {
        let stroke = Stroke::new(
            1.0,
            theme::ACCENT.gamma_multiply(if open { 1.0 } else { 0.5 }),
        );
        painter.rect_stroke(rect, 6.0, stroke, egui::StrokeKind::Inside);
    }
    if text.trim().is_empty() {
        let at = pos2(rect.left() + m.pad.x, rect.center().y);
        painter.text(at, Align2::LEFT_CENTER, none, m.font.clone(), theme::MUTE);
    } else {
        let mut shown = field.clone();
        shown.sel = token_field::Selection::at(Caret::new(Vec::new(), usize::MAX));
        paint::paint(&painter, &shown, &lay, &m, false);
    }
    match count {
        Some((n, all)) if !text.trim().is_empty() => {
            ui.weak(filter::matches_text(n, all));
        }
        _ => desktop_line(ui, help, text, ExprKind::Filter),
    }
    if resp.clicked() {
        open = !open;
    }
    let mut changed = false;
    if open {
        // A menu of the editor is a popover of its own.
        let inner = popover::rect(&ctx);
        let key = format!("desk-filter-{:?}", id);
        let shown = popover::show(&ctx, &key, rect, FILTER_WIDTH, |ui| {
            ui.add_space(4.0);
            let mut controls = Vec::new();
            let edit = filter::editor(ui, &mut controls, id, text, help, count, none);
            if let Some(t) = edit.text {
                *text = t;
                changed = true;
            }
            ui.add_space(10.0);
        });
        let press = ctx.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed()));
        let in_menu = press.is_some_and(|p| inner.is_some_and(|r| r.contains(p)));
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        if shown.closed && !in_menu || escape {
            open = false;
        }
    }
    ui.data_mut(|d| d.insert_temp(id, open));
    changed
}
