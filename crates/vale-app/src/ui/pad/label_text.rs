//! The label text editor: the text template as a token field, the Blocks
//! and Code views of the one text, the preview of the label, and the bar
//! above the keyboard. Blocks write the text only when the user edits in
//! Blocks, so text that the user does not edit stays as it is.

use eframe::egui::{self, Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use vale_style::expr::editor::{Empty, Piece, Seq, Template, Tok};
use vale_style::expr::{FUNCTIONS, Function};

use super::chip_popover::{self, Action, Page};
use super::expression;
use super::geometry::TOUCH;
use super::popover;
use super::theme;
use super::widgets::{self, Controls};
use crate::ui::expr_help::{ExprHelp, ExprKind, NO_LABEL};
use crate::ui::new_feature::keyboard_top;
use crate::ui::token_field::paint::{FUNCTION_TEXT, paint_break, paint_glyph};
use crate::ui::token_field::{self, Field, Glyph, Metrics, Node, Options, template};

/// The label text of a layer: the row of the layer panel, the title of its
/// page, and the field of the desktop layer panel.
pub const LABEL_TEXT: &str = "Label text";
/// The views of the text.
pub const BLOCKS: &str = "Blocks";
pub const CODE: &str = "Code";
/// The buttons of the bar above the keyboard.
pub const GROUP: &str = "Group";
pub const LINE_BREAK: &str = "Line break";
/// The tabs of the bar.
pub const FIELDS: &str = "Fields";
pub const FUNCTIONS_TAB: &str = "Functions";
/// The source of the preview.
pub const SELECTED_FEATURE: &str = "Selected feature";
pub const FIRST_FEATURE: &str = "First feature";
/// The hint of the Functions tab while no field is selected.
pub const WRAP_HINT: &str = "Select a field to wrap it in a function.";

/// The height of the bar above the keyboard.
pub const BAR: f32 = 60.0;
/// The least height of the token field.
const FIELD_HEIGHT: f32 = 120.0;
/// The space between a focused field and the bar above the keyboard.
const GAP: f32 = 8.0;
/// The height of the row of the title and the views.
const HEAD: f32 = 48.0;

/// The control name of the bar chip that inserts `label`.
pub fn insert_name(label: &str) -> String {
    expression::chip_name(label)
}

/// What the user did in the editor in one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edit {
    pub text: Option<String>,
    /// The change is one step, such as an inserted chip, and not typing.
    pub step: bool,
}

/// The state of an editor between frames.
#[derive(Clone, Default)]
struct EditorState {
    field: Field<Piece>,
    /// The text that the field shows. The field reads the text again when
    /// the stored text is different, for example after an undo.
    source: Option<String>,
    code: bool,
    /// The text of the Code field while it has the focus.
    typed: String,
    functions: bool,
    /// The field chip whose popover is open, and the page of the popover.
    popover: Option<(Vec<usize>, Page)>,
    /// The text of the field of Text… in the popover.
    empty_text: String,
}

/// The editor of label text `current` under `heading`. The token field has
/// the control name `name`.
pub fn editor(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    (heading, name): (&str, &str),
    current: &str,
    help: &ExprHelp,
) -> Edit {
    let id = ui.id().with((name, "label-text"));
    let mut st: EditorState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let field_id = ui.id().with(name);
    if st.source.as_deref() != Some(current) {
        let focused = ui.memory(|m| m.has_focus(field_id));
        let sel = st.field.sel.clone();
        st.field = template::field_of(&Template::from_text(current), help);
        if focused {
            st.field.sel = sel;
            st.field.clamp();
        }
        st.source = Some(current.to_owned());
    }
    let mut edit = Edit::default();

    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEAD), Sense::hover());
    ui.painter().text(
        pos2(row.left(), row.center().y),
        Align2::LEFT_CENTER,
        heading,
        theme::title(),
        theme::TEXT,
    );
    let views = [BLOCKS, CODE];
    let w = widgets::small_segments_width(ui, &views);
    let seg = Rect::from_min_size(pos2(row.right() - w, row.center().y - 19.0), vec2(w, 38.0));
    if let Some(i) = widgets::small_segments(ui, controls, seg, &views, usize::from(st.code)) {
        st.code = i == 1;
        st.typed = current.to_owned();
    }

    if st.code {
        let code_name = format!("{name} code");
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
        if resp.changed() && help.valid(&st.typed) {
            edit.text = Some(st.typed.clone());
        }
        expression::line(ui, help, &st.typed, ExprKind::Text);
    } else {
        let floor = bar_top(ui.ctx());
        let focused = ui.memory(|m| m.has_focus(field_id));
        let opts = Options {
            literal: true,
            enter_breaks: true,
            min_height: field_height(ui, focused.then_some(floor)),
            long_press: true,
        };
        let out = token_field::show(ui, controls, name, &mut st.field, &Metrics::pad(), &opts);
        // The field had the focus at the start of the frame, so its height
        // already keeps it above the bar.
        if out.focused && focused {
            keep_above(ui, out.response.rect, floor);
        }
        if out.changed {
            edit.text = Some(write(&mut st));
            edit.step = out.step;
        }
        if out.focused
            && let Some(step) = bar(ui, controls, &mut st, help)
        {
            edit.text = Some(write(&mut st));
            edit.step = step;
        }
        if let Some(step) = chip_popover(ui, controls, &mut st, &out, help, name) {
            edit.text = Some(write(&mut st));
            edit.step = step;
        }
    }
    let shown = edit.text.as_deref().unwrap_or(current);
    preview(ui, help, shown);
    ui.data_mut(|d| d.insert_temp(id, st));
    edit
}

/// The top of the bar above the keyboard.
pub fn bar_top(ctx: &egui::Context) -> f32 {
    keyboard_top(ctx.viewport_rect()) - BAR
}

/// The least height of a token field at the cursor of `ui`. While the
/// keyboard shows, the field is lower so that it ends above the bar, at
/// `floor`, but it keeps the height of one line.
pub fn field_height(ui: &egui::Ui, floor: Option<f32>) -> f32 {
    let line = Metrics::pad().chip + 2.0 * Metrics::pad().pad.y;
    match floor {
        Some(floor) => (floor - GAP - ui.cursor().top()).clamp(line, FIELD_HEIGHT),
        None => FIELD_HEIGHT,
    }
}

/// Scrolls the panel so that a focused field at `rect` ends above the bar
/// at `floor`. The space after the field lets the panel scroll. A sheet
/// rises to its full height.
pub fn keep_above(ui: &mut egui::Ui, rect: Rect, floor: f32) {
    let over = rect.bottom() - (floor - GAP);
    // The space stays while the field ends at the floor, so that the panel
    // does not scroll back.
    if over > -1.0 {
        let screen = ui.ctx().viewport_rect();
        ui.add_space(screen.bottom() - floor);
    }
    // A sheet that rises moves the field, so the panel scrolls only after
    // the rise.
    if over > 0.5 && !super::raise_sheet(ui.ctx()) {
        // A target and not a delta, so that a second pass of the frame
        // does not scroll again.
        let below = ui.clip_rect().bottom() - (floor - GAP);
        let target = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() + below));
        let none = egui::style::ScrollAnimation::none();
        ui.scroll_to_rect_animation(target, Some(egui::Align::BOTTOM), none);
    }
}

/// The text of the field, which the field now shows.
fn write(st: &mut EditorState) -> String {
    template::relabel(&mut st.field);
    let text = template::template_of(&st.field).to_text();
    st.source = Some(text.clone());
    text
}

/// The preview of the label for the feature of the help: the label text in
/// the look of a label, and the feature that it comes from.
fn preview(ui: &mut egui::Ui, help: &ExprHelp, text: &str) {
    if !help.has_sample() || text.trim().is_empty() {
        return;
    }
    let Some(value) = help.eval(text) else {
        return;
    };
    let label = value.label().unwrap_or_else(|| NO_LABEL.to_owned());
    let lines: Vec<&str> = label.lines().collect();
    let font = egui::FontId::proportional(17.0);
    let line_h = 21.0;
    ui.add_space(12.0);
    let h = (lines.len() as f32 * line_h + 24.0).max(56.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 12.0, Color32::from_white_alpha(13));
    let ink = Color32::from_rgb(243, 239, 230);
    let dot = pos2(rect.left() + 16.0, rect.center().y);
    painter.circle(dot, 4.0, ink, Stroke::new(2.0, Color32::BLACK));
    let source = if help.sample_is_selected() {
        SELECTED_FEATURE
    } else {
        FIRST_FEATURE
    };
    let src = painter.text(
        pos2(rect.right() - 12.0, rect.center().y),
        Align2::RIGHT_CENTER,
        source,
        theme::small(),
        theme::MUTE,
    );
    let mid = (rect.left() + 28.0 + src.left() - 12.0) / 2.0;
    let top = rect.center().y - lines.len() as f32 * line_h / 2.0;
    for (i, line) in lines.iter().enumerate() {
        let at = pos2(mid, top + (i as f32 + 0.5) * line_h);
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            let halo = Color32::from_black_alpha(200);
            let at = at + vec2(dx, dy);
            painter.text(at, Align2::CENTER_CENTER, *line, font.clone(), halo);
        }
        painter.text(at, Align2::CENTER_CENTER, *line, font.clone(), ink);
    }
    let resp = ui.interact(rect, ui.id().with("label-preview"), Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
}

/// Opens the popover of a field chip after a tap on the chip, and shows
/// it. A second tap on the chip, a press outside the popover, and a move of
/// the selection away from the chip close it. Returns `Some` after a
/// change, with true for one step.
fn chip_popover(
    ui: &egui::Ui,
    controls: &mut Controls,
    st: &mut EditorState,
    out: &token_field::Output,
    help: &ExprHelp,
    name: &str,
) -> Option<bool> {
    if out.dragging {
        st.popover = None;
    }
    // A chip with no popover opens the edit menu.
    if let Some((path, rect)) = &out.tapped
        && selected_field(&st.field).as_ref() != Some(path)
    {
        token_field::open_menu(ui, name, *rect);
    }
    if let Some((path, _)) = &out.tapped
        && selected_field(&st.field).as_ref() == Some(path)
    {
        let again = st.popover.as_ref().is_some_and(|(p, _)| p == path);
        st.popover = (!again).then(|| (path.clone(), Page::Main));
        if let Some(Node::Chip(c)) = st.field.node(path)
            && let Piece::Field { format, .. } = &c.data
            && let Empty::Text(t) = &format.empty
        {
            st.empty_text = t.clone();
        }
    }
    let (path, page) = st.popover.clone()?;
    if selected_field(&st.field).as_ref() != Some(&path) {
        st.popover = None;
        return None;
    }
    let anchor = out.layout.rect_of(&path)?;
    let Some(Node::Chip(chip)) = st.field.node(&path) else {
        return None;
    };
    let Piece::Field { name, format } = chip.data.clone() else {
        return None;
    };
    let chip = chip_popover::Chip {
        name: &name,
        format: &format,
        in_group: path.len() > 1,
    };
    let mut action = None;
    let shown = popover::show(ui.ctx(), "pad-chip-popover", anchor, 340.0, |ui| {
        action = chip_popover::contents(ui, controls, page, &chip, help, &mut st.empty_text);
    });
    if shown.closed && out.tapped.is_none() {
        st.popover = None;
    }
    match action? {
        Action::Page(page) => {
            st.popover = Some((path, page));
            None
        }
        Action::Format(format, step) => {
            if let Some(Node::Chip(c)) = st.field.node_mut(&path) {
                c.look = template::field_look(&name, &format, help);
                c.data = Piece::Field {
                    name: name.clone(),
                    format,
                };
            }
            Some(step)
        }
        Action::Wrap(f) => {
            st.popover = None;
            wrap_in_function(&mut st.field, &path, f).then_some(true)
        }
        Action::Cut => {
            st.popover = None;
            st.field.select_node(&path);
            token_field::cut(ui.ctx(), &mut st.field).then_some(true)
        }
        Action::Copy => {
            st.popover = None;
            st.field.select_node(&path);
            token_field::copy(ui.ctx(), &st.field);
            None
        }
        Action::Remove => {
            st.popover = None;
            st.field.select_node(&path);
            st.field.replace(Vec::new());
            Some(true)
        }
    }
}

/// The field chip of the selection, and its index in its list.
fn selected_field(field: &Field<Piece>) -> Option<Vec<usize>> {
    match field.selected_node()? {
        (path, Node::Chip(c)) if matches!(c.data, Piece::Field { .. }) => Some(path),
        _ => None,
    }
}

/// Puts the field chip at `path` in the first slot of `f`. The other
/// slots that `f` needs get 0.
pub fn wrap_in_function(field: &mut Field<Piece>, path: &[usize], f: &'static Function) -> bool {
    let Some(Node::Chip(chip)) = field.node(path) else {
        return false;
    };
    let Piece::Field { .. } = &chip.data else {
        return false;
    };
    let inner = Template::new(vec![chip.data.clone()]).to_text();
    let mut args = vec![Seq(vec![Tok::Code(inner)])];
    while args.len() < f.min_args.max(1) {
        args.push(Seq(vec![Tok::Num(0.0)]));
    }
    let piece = Piece::Expr(Seq(vec![Tok::Call { f, args }]));
    let text = Template::new(vec![piece]).to_text();
    // Read the text again, so that the chip is what the text reads as.
    let help = ExprHelp::empty();
    let nodes = template::nodes_of(&Template::from_text(&text).pieces, &help);
    field.select_node(path);
    field.replace(nodes);
    true
}

/// The bar above the keyboard: the Fields and Functions tabs, Group, Line
/// break, and a chip for each field with its value in the feature of the
/// preview. Returns `Some` after a change, with true for one step.
fn bar(
    ui: &egui::Ui,
    controls: &mut Controls,
    st: &mut EditorState,
    help: &ExprHelp,
) -> Option<bool> {
    let mut changed = None;
    bar_area(ui, "pad-label-bar", |ui| {
        let tabs = [FIELDS, FUNCTIONS_TAB];
        let w = widgets::small_segments_width(ui, &tabs);
        let (seg, _) = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover());
        let active = usize::from(st.functions);
        if let Some(i) = widgets::small_segments(ui, controls, seg, &tabs, active) {
            st.functions = i == 1;
        }
        let text = (ui.painter()).layout_no_wrap(GROUP.into(), theme::body(), theme::MUTE);
        let w = 30.0 + text.size().x;
        let (group, _) = ui.allocate_exact_size(vec2(w, TOUCH), Sense::hover());
        if bar_button(ui, controls, group, GROUP, None) && template::group(&mut st.field) {
            changed = Some(true);
        }
        let (brk, _) = ui.allocate_exact_size(vec2(TOUCH, TOUCH), Sense::hover());
        if bar_button(ui, controls, brk, LINE_BREAK, Some(())) {
            st.field.replace(vec![Node::Break]);
            changed = Some(true);
        }
        bar_divider(ui);
        if st.functions {
            if let Some(f) = function_chips(ui, controls, &st.field) {
                let path = selected_field(&st.field).expect("a selected field");
                if wrap_in_function(&mut st.field, &path, f) {
                    changed = Some(true);
                }
            }
        } else if let Some(name) = field_chips(ui, controls, help, ("", insert_name)) {
            template::insert_field(&mut st.field, &name, help);
            changed = Some(true);
        }
    });
    changed
}

/// The bar above the keyboard, with the items that `add` adds from left to
/// right. The bar scrolls sideways when the screen is too narrow for its
/// items. A press on the bar keeps the focus of the field.
pub fn bar_area(ui: &egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    let ctx = ui.ctx().clone();
    let screen = ctx.viewport_rect();
    let bottom = keyboard_top(screen);
    let rect = Rect::from_min_max(
        pos2(screen.left(), bottom - BAR),
        pos2(screen.right(), bottom),
    );
    token_field::keep_focus(&ctx, rect);
    popover::set_floor(&ctx, rect.top());
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .movable(false)
        .constrain(false)
        .fade_in(false)
        .show(&ctx, |ui| {
            theme::apply(ui);
            let (rect, _) = ui.allocate_exact_size(rect.size(), Sense::click());
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, Color32::from_rgb(28, 31, 35));
            let top = [rect.left_top(), rect.right_top()];
            painter.line_segment(top, Stroke::new(1.0, Color32::from_rgb(42, 46, 51)));
            let inner = rect.shrink2(vec2(12.0, 0.0));
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(inner)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            egui::ScrollArea::horizontal()
                .id_salt((id, "chips"))
                .show(&mut child, |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.set_min_height(inner.height());
                    add(ui);
                });
        });
}

/// A thin line between two parts of the bar.
pub fn bar_divider(ui: &mut egui::Ui) {
    let (line, _) = ui.allocate_exact_size(vec2(9.0, 28.0), Sense::hover());
    ui.painter().line_segment(
        [line.center_top(), line.center_bottom()],
        Stroke::new(1.0, theme::divider()),
    );
}

/// A button of the bar with a text, or with the line break arrow.
fn bar_button(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    arrow: Option<()>,
) -> bool {
    let resp = widgets::hit(ui, controls, rect, name, Sense::click(), true, None);
    let fill = if resp.is_pointer_button_down_on() {
        Color32::from_white_alpha(56)
    } else {
        theme::raise()
    };
    ui.painter().rect_filled(rect, theme::CONTROL_RADIUS, fill);
    match arrow {
        Some(()) => paint_break(ui.painter(), rect, theme::MUTE),
        None => {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                name,
                theme::body(),
                theme::MUTE,
            );
        }
    }
    resp.clicked()
}

/// A chip of the bar: an optional type glyph, a name, and a note in small
/// text. Returns true when the user taps it.
pub fn bar_chip(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    glyph: Option<Glyph>,
    label: (&str, Color32),
    note: &str,
) -> bool {
    let note_font = egui::FontId::proportional(12.0);
    let width_of = |t: &str, f: egui::FontId| {
        ui.painter()
            .layout_no_wrap(t.into(), f, theme::TEXT)
            .size()
            .x
    };
    let lead = if glyph.is_some() {
        8.0 + 22.0 + 8.0
    } else {
        14.0
    };
    let note_w = if note.is_empty() {
        0.0
    } else {
        8.0 + width_of(note, note_font.clone())
    };
    let w = lead + width_of(label.0, theme::body()) + note_w + 14.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w.max(TOUCH), TOUCH), Sense::hover());
    let resp = widgets::hit(ui, controls, rect, name, Sense::click(), true, None);
    let fill = if resp.is_pointer_button_down_on() {
        Color32::from_white_alpha(56)
    } else {
        theme::raise()
    };
    let painter = ui.painter();
    painter.rect_filled(rect, theme::CONTROL_RADIUS, fill);
    let y = rect.center().y;
    if let Some(g) = glyph {
        paint_glyph(
            painter,
            pos2(rect.left() + 8.0 + 11.0, y),
            g,
            &Metrics::pad(),
            false,
        );
    }
    let at = pos2(rect.left() + lead + (rect.width() - w) / 2.0, y);
    let r = painter.text(at, Align2::LEFT_CENTER, label.0, theme::body(), label.1);
    if !note.is_empty() {
        let at = pos2(r.right() + 8.0, y);
        painter.text(at, Align2::LEFT_CENTER, note, note_font, theme::MUTE);
    }
    resp.clicked()
}

/// The most chars of a sample value in a bar chip.
const SAMPLE: usize = 16;

/// The chips of the fields, each with its value in the feature of the
/// preview, for the fields whose names hold `needle` in lowercase. `name`
/// gives the control name of the chip of a field. Returns the field that
/// the user tapped.
pub fn field_chips(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    help: &ExprHelp,
    (needle, name): (&str, impl Fn(&str) -> String),
) -> Option<String> {
    let mut chosen = None;
    for (field, kind) in help.fields.iter().zip(&help.types) {
        if !field.to_lowercase().contains(needle) {
            continue;
        }
        let sample = help
            .sample(field)
            .and_then(|v| v.label())
            .unwrap_or_default();
        let sample = match sample.chars().count() > SAMPLE {
            true => format!("{}…", sample.chars().take(SAMPLE - 1).collect::<String>()),
            false => sample,
        };
        let glyph = Some(Glyph::of(*kind));
        let label = (field.as_str(), theme::TEXT);
        if bar_chip(ui, controls, &name(field), glyph, label, &sample) {
            chosen = Some(field.clone());
        }
    }
    chosen
}

/// The chips of the functions while a field is selected, or a hint.
/// Returns the function that the user tapped.
fn function_chips(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    field: &Field<Piece>,
) -> Option<&'static Function> {
    if selected_field(field).is_none() {
        ui.label(
            egui::RichText::new(WRAP_HINT)
                .font(theme::body())
                .color(theme::MUTE),
        );
        return None;
    }
    let mut chosen = None;
    for f in FUNCTIONS {
        let label = (f.name, FUNCTION_TEXT);
        if bar_chip(ui, controls, &insert_name(f.name), None, label, f.hint) {
            chosen = Some(f);
        }
    }
    chosen
}
