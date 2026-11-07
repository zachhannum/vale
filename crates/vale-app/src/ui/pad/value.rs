//! The value editor: a value as tokens and function blocks in a token
//! field, the Blocks and Code views of the one text, the preview of the
//! value with its range across the layer, and the tray of fields,
//! functions, and operators. Blocks write the text only when the user
//! edits in Blocks.

use eframe::egui::{self, Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use vale_store::Value;
use vale_style::expr::editor::{Cond, Formula, Seq, Tok};
use vale_style::expr::{Category, Expr, FUNCTIONS, Function};

use super::cases::{self, CasesEdit, CasesState};
use super::expression;
use super::filter::{self, search_field};
use super::geometry::TOUCH;
use super::label_text::{
    BLOCKS, CODE, FIRST_FEATURE, SELECTED_FEATURE, bar_area, bar_chip, bar_divider, bar_top,
    field_chips, keep_above,
};
use super::theme;
use super::widgets::{self, Controls, PANEL_ROW};
use crate::ui::expr_help::{ExprHelp, ExprKind, NO_VALUE, ValueRange, range_text};
use crate::ui::token_field::formula::{self, Completion, OPERATORS, op_label};
use crate::ui::token_field::paint::{BAD, FUNCTION_TEXT, paint_glyph};
use crate::ui::token_field::{self, Field, Glyph, Metrics, Node, Options};

/// The tabs of the tray.
pub const FIELDS: &str = "Fields";
pub const FUNCTIONS_TAB: &str = "Functions";
pub const OPERATORS_TAB: &str = "Operators";
/// The search fields of the tabs of the tray.
pub const SEARCH: [&str; 3] = ["Search fields", "Search functions", "Search operators"];

/// The start of the control name of a chip of text that does not parse.
/// The number of the chip follows, from 1.
pub const CODE_CHIP: &str = "Code chip";
/// The tray row of a Cases block, and its hint.
pub const CASES_ROW: &str = "Cases";
pub const CASES_HINT: &str = "Choose a value by conditions";

/// The control name of the tray row that inserts `label`.
pub fn insert_name(label: &str) -> String {
    expression::chip_name(label)
}

/// The control name of the item `label` of the bar above the keyboard,
/// such as a tab or `insert_name("pop")`. The tray has items with the same
/// labels.
pub fn bar_name(label: &str) -> String {
    format!("{label} in bar")
}

/// The names of the segments Blocks and Code of the editor `name`.
pub fn view_names(name: &str) -> [String; 2] {
    [format!("{name} blocks"), format!("{name} code")]
}

/// What the user did in the editor in one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edit {
    pub text: Option<String>,
    /// The change is one step, such as an inserted block, and not typing.
    pub step: bool,
}

/// The state of an editor between frames.
#[derive(Clone, Default)]
struct EditorState {
    field: Field<Tok>,
    /// The text that the field shows. The field reads the text again when
    /// the stored text is different.
    source: Option<String>,
    code: bool,
    typed: String,
    tab: usize,
    /// The tab of the bar above the keyboard.
    bar_tab: usize,
    search: String,
    tray: bool,
    /// The tray in the last frame.
    tray_rect: Option<Rect>,
    cases: CasesState,
}

/// The editor of value `current`, with the control name `name`. `range`
/// is the range of the value across the layer.
pub fn editor(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    current: &str,
    help: &ExprHelp,
    range: Option<&ValueRange>,
) -> Edit {
    let id = ui.id().with((name, "value-editor"));
    let mut st: EditorState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let field_id = ui.id().with(name);
    let focused = ui.memory(|m| m.has_focus(field_id));
    if st.source.as_deref() != Some(current) {
        let sel = st.field.sel.clone();
        st.field = formula::field_of(&Formula::from_text(current), help);
        if focused {
            st.field.sel = sel;
            st.field.clamp();
        }
        st.source = Some(current.to_owned());
    }
    let mut edit = Edit::default();

    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let at = pos2(row.left(), row.center().y);
    (ui.painter()).text(at, Align2::LEFT_CENTER, name, theme::body(), theme::MUTE);
    let views = [BLOCKS, CODE];
    let names = view_names(name);
    let names = [names[0].as_str(), names[1].as_str()];
    let w = widgets::small_segments_width(ui, &views);
    let seg = Rect::from_min_size(pos2(row.right() - w, row.center().y - 19.0), vec2(w, 38.0));
    let named = (&views[..], &names[..]);
    if let Some(i) = widgets::small_segments_named(ui, controls, seg, named, usize::from(st.code)) {
        st.code = i == 1;
        st.typed = current.to_owned();
    }

    if st.code {
        let code_name = format!("{name} text");
        let code_id = ui.id().with(&code_name);
        if !ui.memory(|m| m.has_focus(code_id)) {
            st.typed = current.to_owned();
        }
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
        let valid = help.valid(&st.typed);
        let font = theme::mono();
        let resp = widgets::field_at(
            ui,
            controls,
            rect,
            &code_name,
            None,
            &mut st.typed,
            valid,
            font,
        );
        if resp.changed() {
            edit.text = Some(st.typed.clone());
        }
        expression::line(ui, help, &st.typed, ExprKind::Value);
    } else if let Some((cases, otherwise)) = cases_of(current) {
        let error = error_at(current);
        let in_block = (error.as_ref())
            .and_then(|(p, _)| p.split_first())
            .filter(|(first, _)| **first == 0)
            .map(|(_, rest)| rest.to_vec());
        let parts = (cases.as_slice(), &otherwise);
        match cases::block(
            ui,
            controls,
            &mut st.cases,
            parts,
            help,
            in_block.as_deref(),
        ) {
            Some(CasesEdit::Changed(tok, step)) => {
                edit.text = Some(tok.to_text());
                edit.step = step;
                st.source = None;
            }
            Some(CasesEdit::OpenCode) => {
                st.code = true;
                st.typed = current.to_owned();
            }
            None => {}
        }
        error_line(ui, error.as_ref().map(|(_, m)| m.as_str()));
        preview(ui, help, edit.text.as_deref().unwrap_or(current), range);
    } else {
        let floor = bar_top(ui.ctx());
        let opts = Options {
            literal: false,
            enter_breaks: false,
            min_height: super::label_text::field_height(ui, focused.then_some(floor)).min(64.0),
            long_press: true,
        };
        let m = Metrics::pad();
        let out = token_field::show(ui, controls, name, &mut st.field, &m, &opts);
        let mut changed = out.changed;
        let mut step = out.step;
        if !out.typed.is_empty() {
            formula::type_text(&mut st.field, &out.typed, help);
            changed = true;
        }
        if !out.pasted.is_empty() {
            formula::paste(&mut st.field, &out.pasted, help);
            (changed, step) = (true, true);
        }
        if out.focused {
            if let Some(s) = bar(ui, controls, &mut st, help) {
                changed = true;
                step |= s;
            }
            let (tab, enter) = ui.input_mut(|i| {
                let tab = i.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
                (tab, i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
            });
            if tab {
                formula::commit(&mut st.field, help);
                formula::next_slot(&mut st.field);
                changed = true;
            }
            if enter {
                let found = formula::completions(&formula::pending(&st.field), help);
                match found.first() {
                    Some(c) => formula::pick(&mut st.field, c, help),
                    None => {
                        formula::commit(&mut st.field, help);
                    }
                }
                (changed, step) = (true, true);
            }
        } else if has_chars(&st.field.nodes) {
            // The typed words become tokens when the field loses the focus.
            let text = formula::text_of(&st.field.nodes);
            st.field = formula::field_of(&Formula::from_text(&text), help);
        }
        // A tap on a code chip opens Code.
        let mut code_chips = 0;
        for p in &out.layout.placed {
            if let Some(Node::Chip(c)) = st.field.node(&p.path)
                && matches!(c.data, Tok::Code(_))
            {
                code_chips += 1;
                widgets::push(ui, controls, p.rect, &format!("{CODE_CHIP} {code_chips}"));
            }
        }
        if let Some((path, rect)) = &out.tapped {
            if let Some(Node::Chip(c)) = st.field.node(path)
                && matches!(c.data, Tok::Code(_))
            {
                st.code = true;
                st.typed = current.to_owned();
            } else {
                token_field::open_menu(ui, name, *rect);
            }
        }
        let shown = edit_text(&mut st, changed);
        let text = shown.as_deref().unwrap_or(current);
        let error = error_at(text);
        if let Some((path, _)) = &error
            && !has_chars(&st.field.nodes)
        {
            mark_node(ui, &st.field, &out.layout, path);
        }
        error_line(ui, error.as_ref().map(|(_, m)| m.as_str()));
        preview(ui, help, text, range);
        // The tray shows from a tap on the field until a press outside the
        // field and the tray.
        let press = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed()));
        let inside =
            |p| out.response.rect.contains(p) || st.tray_rect.is_some_and(|r| r.contains(p));
        if out.focused {
            st.tray = true;
        } else if press.is_some_and(|p| !inside(p)) {
            st.tray = false;
        }
        st.tray_rect = None;
        if st.tray
            && let Some(step) = tray(ui, controls, &mut st, help)
        {
            changed = true;
            edit.step = step;
        }
        if changed {
            edit.text = edit_text(&mut st, true);
            edit.step |= step;
        }
        // After the tray, so that the scroll goes to the panel and not to
        // the rows of the tray. The field had the focus at the start of the
        // frame, so its height already keeps it above the bar.
        if out.focused && focused {
            keep_above(ui, out.response.rect, floor);
        }
    }
    ui.data_mut(|d| d.insert_temp(id, st));
    edit
}

/// The cases of a value that is one Cases block.
fn cases_of(text: &str) -> Option<(Vec<(Cond, Seq)>, Seq)> {
    match Formula::from_text(text).seq.0.as_slice() {
        [Tok::Cases { cases, otherwise }] => Some((cases.clone(), otherwise.clone())),
        _ => None,
    }
}

/// The path of the model node at the parse error of `text`, and the
/// message of the error. `None` if the text parses.
pub fn error_at(text: &str) -> Option<(Vec<usize>, String)> {
    if text.trim().is_empty() {
        return None;
    }
    let e = Expr::parse(text).err()?;
    let formula = Formula::from_text(text);
    let path = formula.path_at(e.start).unwrap_or_default();
    let message = hole_message(&formula.seq, &path).unwrap_or_else(|| e.short());
    Some((path, message))
}

/// The message for an empty slot or an empty case at `path` of `seq`, which
/// names the part in words. `None` if the node at the path is not empty.
fn hole_message(seq: &Seq, path: &[usize]) -> Option<String> {
    let (&i, rest) = path.split_first()?;
    let needs = |f: &Function, slot: usize| {
        format!("The {} function needs {}.", f.name, slot_noun(f.slot(slot)))
    };
    let tok = seq.0.get(i)?;
    let Some((&child, rest)) = rest.split_first() else {
        // A call with an empty slot or too few arguments.
        return match tok {
            Tok::Call { f, args } => (args.iter().position(|a| a.0.is_empty()))
                .or((args.len() < f.min_args).then_some(args.len()))
                .map(|slot| needs(f, slot)),
            _ => None,
        };
    };
    match tok {
        Tok::Call { f, args } => {
            let slot = args.get(child)?;
            match rest.is_empty() {
                true if slot.0.is_empty() => Some(needs(f, child)),
                true => None,
                false => hole_message(slot, rest),
            }
        }
        Tok::Cases { cases, otherwise } => {
            let n = cases.len();
            let (part, seq) = match child {
                c if c == 2 * n => (None, otherwise),
                c if c % 2 == 0 => {
                    let cond = &cases.get(c / 2)?.0;
                    let empty = matches!(cond, Cond::Group { items, .. } if items.is_empty());
                    return (rest.is_empty() && empty)
                        .then(|| format!("The {} case needs a condition.", ordinal(c / 2)));
                }
                c => (Some(c / 2), &cases.get(c / 2)?.1),
            };
            match rest.is_empty() {
                true if seq.0.is_empty() => Some(match part {
                    Some(k) => format!("The {} case needs a value.", ordinal(k)),
                    None => "Otherwise needs a value.".to_owned(),
                }),
                true => None,
                false => hole_message(seq, rest),
            }
        }
        Tok::Paren(inner) if child == 0 => match rest.is_empty() {
            true => inner
                .0
                .is_empty()
                .then(|| "The parentheses need a value.".to_owned()),
            false => hole_message(inner, rest),
        },
        _ => None,
    }
}

/// The name of a slot as the object of "needs".
fn slot_noun(slot: &str) -> String {
    match slot {
        "text" => slot.to_owned(),
        "condition" | "value" | "start" | "length" => format!("a {slot}"),
        _ => format!("a value for {slot}"),
    }
}

/// The ordinal word of index `i` from 0: "first" for 0.
fn ordinal(i: usize) -> String {
    const WORDS: [&str; 10] = [
        "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
        "tenth",
    ];
    match WORDS.get(i) {
        Some(w) => (*w).to_owned(),
        None => format!("number {}", i + 1),
    }
}

/// A red frame around the node of the field at model path `path`: the
/// deepest node on the path. The path of a token, a slot, and a token in a
/// slot is the same in the model and in the field.
fn mark_node(ui: &egui::Ui, field: &Field<Tok>, layout: &token_field::Layout, path: &[usize]) {
    let deepest = (1..=path.len())
        .rev()
        .map(|n| &path[..n])
        .find(|p| field.node(p).is_some());
    let Some(rect) = deepest.and_then(|p| layout.rect_of(p)) else {
        return;
    };
    let stroke = Stroke::new(2.0, BAD);
    (ui.painter()).rect_stroke(rect.expand(2.0), 9.0, stroke, egui::StrokeKind::Outside);
}

/// The message of a parse error under the field.
fn error_line(ui: &mut egui::Ui, message: Option<&str>) {
    let Some(message) = message else {
        return;
    };
    ui.add_space(4.0);
    let text = egui::RichText::new(message).font(theme::body()).color(BAD);
    ui.add(egui::Label::new(text).wrap().selectable(false));
}

/// The text of the field, which the field now shows, if `changed`.
fn edit_text(st: &mut EditorState, changed: bool) -> Option<String> {
    if !changed {
        return None;
    }
    let text = formula::text_of(&st.field.nodes);
    st.source = Some(text.clone());
    Some(text)
}

/// True if the nodes hold a char, also in a box.
fn has_chars(nodes: &[Node<Tok>]) -> bool {
    nodes.iter().any(|n| match n {
        Node::Char(_) => true,
        Node::Box(b) => has_chars(&b.children),
        _ => false,
    })
}

/// The space between two bars of the histogram.
const BAR_GAP: f32 = 2.0;
/// The most height of the rows of the tray.
const TRAY_HEIGHT: f32 = 300.0;
/// The groups of the Functions tab of the tray, in order.
const TRAY_CATEGORIES: [Category; 5] = [
    Category::Math,
    Category::Text,
    Category::Logic,
    Category::Convert,
    Category::Geometry,
];

/// The preview: the value for the feature of the help, the feature, a
/// histogram of the values across the layer with a line at the value, and
/// the range of the values.
fn preview(ui: &mut egui::Ui, help: &ExprHelp, text: &str, range: Option<&ValueRange>) {
    if !help.has_sample() || text.trim().is_empty() {
        return;
    }
    let Some(value) = help.eval(text) else {
        return;
    };
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 12.0, Color32::from_white_alpha(13));
    let label = value.label().unwrap_or_else(|| NO_VALUE.to_owned());
    let big = egui::FontId::proportional(24.0);
    let left = rect.left() + 16.0;
    painter.text(
        pos2(left, rect.top() + 26.0),
        Align2::LEFT_CENTER,
        &label,
        big,
        theme::TEXT,
    );
    let source = if help.sample_is_selected() {
        SELECTED_FEATURE
    } else {
        FIRST_FEATURE
    };
    let feature = help.sample("name").and_then(Value::label);
    let note = match feature {
        Some(name) => format!("{name} · {}", source.to_lowercase()),
        None => source.to_owned(),
    };
    let at = pos2(left, rect.bottom() - 18.0);
    painter.text(at, Align2::LEFT_CENTER, &note, theme::small(), theme::MUTE);
    let resp = ui.interact(rect, ui.id().with("value-preview"), Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
    let Some(range) = range else {
        return;
    };
    let chart = Rect::from_min_max(
        pos2(rect.center().x + 8.0, rect.top() + 14.0),
        pos2(rect.right() - 16.0, rect.bottom() - 14.0),
    );
    let most = range.bins.iter().copied().max().unwrap_or(0).max(1) as f32;
    let n = range.bins.len().max(1) as f32;
    let w = chart.width() / n;
    for (i, count) in range.bins.iter().enumerate() {
        let h = (*count as f32 / most * chart.height()).max(if *count > 0 { 2.0 } else { 0.0 });
        let bar = Rect::from_min_max(
            pos2(
                chart.left() + i as f32 * w + BAR_GAP / 2.0,
                chart.bottom() - h,
            ),
            pos2(
                chart.left() + (i + 1) as f32 * w - BAR_GAP / 2.0,
                chart.bottom(),
            ),
        );
        painter.rect_filled(bar, 1.0, theme::ACCENT.gamma_multiply(0.8));
    }
    if let Value::Number(v) = value
        && range.max > range.min
    {
        let t = ((v - range.min) / (range.max - range.min)).clamp(0.0, 1.0) as f32;
        let x = chart.left() + t * chart.width();
        let line = [pos2(x, chart.top() - 4.0), pos2(x, chart.bottom() + 2.0)];
        painter.line_segment(line, Stroke::new(2.0, theme::TEXT));
    }
    let text = egui::RichText::new(range_text(range))
        .font(theme::small())
        .color(theme::MUTE);
    let layout = egui::Layout::right_to_left(egui::Align::Center);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 22.0), layout, |ui| {
        ui.add(egui::Label::new(text).selectable(false));
    });
}

/// The bar above the keyboard: the tabs Fields, Functions, and Operators,
/// and a chip for each item of the tab. The word that the user types
/// filters the fields and the functions. Returns `Some` after a change,
/// with true for one step.
fn bar(
    ui: &egui::Ui,
    controls: &mut Controls,
    st: &mut EditorState,
    help: &ExprHelp,
) -> Option<bool> {
    let needle = formula::pending(&st.field).trim().to_lowercase();
    let mut out = None;
    bar_area(ui, "pad-value-bar", |ui| {
        let tabs = [FIELDS, FUNCTIONS_TAB, OPERATORS_TAB];
        let names = tabs.map(bar_name);
        let names = [names[0].as_str(), names[1].as_str(), names[2].as_str()];
        let w = widgets::small_segments_width(ui, &tabs);
        let (seg, _) = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover());
        let named = (&tabs[..], &names[..]);
        if let Some(i) = widgets::small_segments_named(ui, controls, seg, named, st.bar_tab) {
            st.bar_tab = i;
        }
        bar_divider(ui);
        match st.bar_tab {
            0 => {
                let name = |f: &str| bar_name(&insert_name(f));
                if let Some(field) = field_chips(ui, controls, help, (&needle, name)) {
                    formula::pick(&mut st.field, &Completion::Field(field), help);
                    out = Some(true);
                }
            }
            1 => {
                for f in FUNCTIONS.iter().filter(|f| f.name.contains(&needle)) {
                    let label = (f.name, FUNCTION_TEXT);
                    let name = bar_name(&insert_name(f.name));
                    if bar_chip(ui, controls, &name, None, label, f.hint) {
                        insert_function(st, f, help);
                        out = Some(true);
                    }
                }
            }
            _ => {
                for op in OPERATORS {
                    let label = op_label(op);
                    let name = bar_name(&insert_name(label));
                    if bar_chip(ui, controls, &name, None, (label, theme::TEXT), "") {
                        formula::insert_op(&mut st.field, op, help);
                        out = Some(true);
                    }
                }
            }
        }
    });
    out
}

/// Puts function `f` in the field: in place of the typed word, or around
/// the selection.
fn insert_function(st: &mut EditorState, f: &'static Function, help: &ExprHelp) {
    if st.field.sel.is_empty() && !formula::pending(&st.field).is_empty() {
        formula::pick(&mut st.field, &Completion::Function(f), help);
    } else {
        formula::insert_function(&mut st.field, f);
    }
}

/// The tray: the tabs Fields, Functions, and Operators, a search field, and
/// the rows of the tab. A tap on a row puts it in the field. While the
/// search field is empty, the word that the user types filters the rows. Returns
/// `Some` after a change, with true for one step.
fn tray(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    st: &mut EditorState,
    help: &ExprHelp,
) -> Option<bool> {
    ui.add_space(10.0);
    let top = ui.cursor().top();
    let tabs = [FIELDS, FUNCTIONS_TAB, OPERATORS_TAB];
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let w = widgets::small_segments_width(ui, &tabs);
    let seg = Rect::from_min_size(pos2(row.left(), row.center().y - 19.0), vec2(w, 38.0));
    if let Some(i) = widgets::small_segments(ui, controls, seg, &tabs, st.tab) {
        st.tab = i;
        st.search.clear();
    }
    search_field(ui, controls, SEARCH[st.tab], &mut st.search);
    let word = if st.search.trim().is_empty() {
        formula::pending(&st.field)
    } else {
        st.search.clone()
    };
    let needle = word.trim().to_lowercase();
    let mut out = None;
    let scroll = egui::ScrollArea::vertical()
        .id_salt("value-tray")
        .max_height(TRAY_HEIGHT)
        // The panel scrolls, so the tray takes its height also at the
        // bottom of the panel.
        .min_scrolled_height(TRAY_HEIGHT);
    scroll.show(ui, |ui| match st.tab {
        0 => {
            for (name, kind) in help.fields.iter().zip(&help.types) {
                if !name.to_lowercase().contains(&needle) {
                    continue;
                }
                let sample = help.sample(name).and_then(Value::label).unwrap_or_default();
                let glyph = Some(Glyph::of(*kind));
                if tray_row(
                    ui,
                    controls,
                    &insert_name(name),
                    glyph,
                    (name, theme::TEXT),
                    &sample,
                ) {
                    let pick = Completion::Field(name.clone());
                    formula::pick(&mut st.field, &pick, help);
                    out = Some(true);
                }
            }
        }
        1 => {
            for category in TRAY_CATEGORIES {
                let functions: Vec<_> = (FUNCTIONS.iter())
                    .filter(|f| f.category == category && f.name.contains(&needle))
                    .collect();
                if functions.is_empty() {
                    continue;
                }
                let (head, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                let at = pos2(head.left() + 4.0, head.bottom() - 6.0);
                let title = category.name().to_uppercase();
                (ui.painter()).text(at, Align2::LEFT_BOTTOM, title, theme::small(), theme::MUTE);
                if category == Category::Logic
                    && CASES_ROW.to_lowercase().contains(&needle)
                    && tray_row(
                        ui,
                        controls,
                        &insert_name(CASES_ROW),
                        None,
                        (CASES_ROW, FUNCTION_TEXT),
                        CASES_HINT,
                    )
                {
                    let tok = Tok::Cases {
                        cases: vec![(filter::new_rule(help), Seq(vec![Tok::Num(0.0)]))],
                        otherwise: Seq(vec![Tok::Num(0.0)]),
                    };
                    st.field.replace(vec![formula::chip(tok, help)]);
                    out = Some(true);
                }
                for f in functions {
                    let label = (f.signature, FUNCTION_TEXT);
                    if tray_row(ui, controls, &insert_name(f.name), None, label, f.hint) {
                        insert_function(st, f, help);
                        out = Some(true);
                    }
                }
            }
        }
        _ => {
            let width = ui.available_width();
            let size = vec2(56.0, TOUCH);
            let per_row = ((width + 8.0) / (size.x + 8.0)).floor().max(1.0) as usize;
            for chunk in OPERATORS.chunks(per_row) {
                let (line, _) = ui.allocate_exact_size(vec2(width, TOUCH + 8.0), Sense::hover());
                for (i, op) in chunk.iter().enumerate() {
                    let rect = Rect::from_min_size(
                        pos2(line.left() + i as f32 * (size.x + 8.0), line.top()),
                        size,
                    );
                    let label = op_label(*op);
                    if op_button(ui, controls, rect, &insert_name(label), label) {
                        formula::insert_op(&mut st.field, *op, help);
                        out = Some(true);
                    }
                }
            }
        }
    });
    let rect = Rect::from_min_max(
        pos2(ui.max_rect().left(), top),
        pos2(ui.max_rect().right(), ui.cursor().top()),
    );
    st.tray_rect = Some(rect);
    out
}

/// A row of the tray: an optional type glyph, a label, and a note. Returns
/// true on a tap.
fn tray_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    glyph: Option<Glyph>,
    (label, color): (&str, Color32),
    note: &str,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH + 4.0), Sense::hover());
    let resp = widgets::hit(ui, controls, rect, name, Sense::click(), true, None);
    if resp.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(rect, theme::CONTROL_RADIUS, theme::raise());
    }
    let mut x = rect.left() + 8.0;
    let y = rect.center().y;
    if let Some(g) = glyph {
        paint_glyph(ui.painter(), pos2(x + 11.0, y), g, &Metrics::pad(), false);
        x += 22.0 + 10.0;
    }
    let font = if glyph.is_some() {
        theme::body()
    } else {
        theme::mono()
    };
    let r = (ui.painter()).text(pos2(x, y), Align2::LEFT_CENTER, label, font, color);
    if !note.is_empty() {
        let clip = Rect::from_min_max(
            pos2(r.right() + 12.0, rect.top()),
            pos2(rect.right() - 8.0, rect.bottom()),
        );
        let at = pos2(clip.left(), y);
        (ui.painter().with_clip_rect(clip)).text(
            at,
            Align2::LEFT_CENTER,
            note,
            theme::body(),
            theme::MUTE,
        );
    }
    resp.clicked()
}

/// A button of an operator. Returns true on a tap.
fn op_button(ui: &egui::Ui, controls: &mut Controls, rect: Rect, name: &str, label: &str) -> bool {
    let resp = widgets::hit(ui, controls, rect, name, Sense::click(), true, None);
    let fill = if resp.is_pointer_button_down_on() {
        Color32::from_white_alpha(56)
    } else {
        theme::raise()
    };
    ui.painter().rect_filled(rect, theme::CONTROL_RADIUS, fill);
    (ui.painter()).text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        theme::body(),
        theme::TEXT,
    );
    resp.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::token_field::ChipKind;

    #[test]
    fn the_tray_lists_each_function() {
        for f in FUNCTIONS {
            assert!(TRAY_CATEGORIES.contains(&f.category), "{}", f.name);
        }
    }

    #[test]
    fn missing_condition_marks_slot() {
        // The condition of the first case is empty.
        let (path, message) = error_at("if(, 12, 10)").unwrap();
        assert_eq!(path, [0, 0]);
        assert!(!message.is_empty());
        // The second slot of a block is empty.
        let (path, _) = error_at("clamp(1, , 3)").unwrap();
        assert_eq!(path, [0, 1]);
        assert_eq!(error_at("clamp(1, 2, 3)"), None);
    }

    #[test]
    fn hole_error_names_the_slot() {
        let message = |t: &str| error_at(t).unwrap().1;
        assert_eq!(message("if(, 12, 10)"), "The first case needs a condition.");
        assert_eq!(
            message("if(a, 1, if(b, 2, if(, 3, 4)))"),
            "The third case needs a condition."
        );
        assert_eq!(message("if(a, , 4)"), "The first case needs a value.");
        assert_eq!(message("if(a, 1, )"), "Otherwise needs a value.");
        assert_eq!(
            message("clamp(1, , 3)"),
            "The clamp function needs a value for x."
        );
        assert_eq!(
            message("round(upper(), 1)"),
            "The upper function needs text."
        );
        // Other errors keep the text of the parser.
        let short = Expr::parse("1 +").unwrap_err().short();
        assert_eq!(message("1 +"), short);
    }

    #[test]
    fn broken_text_shows_code_chip() {
        let help = ExprHelp::empty();
        let field = formula::field_of(&Formula::from_text("round(x) + )"), &help);
        let code = field
            .nodes
            .iter()
            .any(|n| matches!(n, Node::Chip(c) if c.look.kind == ChipKind::Code));
        assert!(code, "{:?}", field.nodes);
    }

    #[test]
    fn nested_ifs_read_as_cases() {
        let (cases, otherwise) = cases_of("if(a > 1, 18, if(b, 14, 10))").unwrap();
        assert_eq!(cases.len(), 2);
        assert_eq!(otherwise.to_text(), "10");
        assert!(cases_of("if(a > 1, 18, 10) + 1").is_none());
    }
}
