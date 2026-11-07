//! The field as a widget: the taps, the keys, the on-screen keyboard, and
//! the picture.

use eframe::egui::{
    self, Color32, Event, ImeEvent, Key, Pos2, Rect, Response, Sense, Stroke, Vec2, pos2, vec2,
};

use super::caret::{Caret, Dir, Selection};
use super::layout::{Layout, Metrics, PlacedKind, layout};
use super::paint::{self, FIELD_FILL};
use super::{BoxKind, Field, Node, Token, copy, cut, paste};
use crate::ui::pad::theme;
use crate::ui::pad::widgets::{self, Controls, EditAction};

/// How a field takes text.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Typed text goes in the field as chars. Without it, the output gives
    /// the typed text to the caller.
    pub literal: bool,
    /// The Return key adds a line break.
    pub enter_breaks: bool,
    /// The least height of the field.
    pub min_height: f32,
    /// A node moves after a long press, and a long press in empty space
    /// opens the edit menu, as on iPad. Without it, a node moves after a
    /// drag of a few points.
    pub long_press: bool,
}

/// The time of a long press, in seconds.
const LONG_PRESS: f64 = 0.4;
/// The most motion of a long press.
const STILL: f32 = 6.0;
/// The motion that starts a drag without a long press.
const DRAG_START: f32 = 4.0;
/// The width of the line at the place where a dragged node goes.
const DROP_LINE: f32 = 2.0;
/// The back of a lifted node.
const LIFT_BACK: Color32 = Color32::from_rgb(36, 40, 45);

/// What the user did with a field in one frame.
pub struct Output {
    pub response: Response,
    /// The nodes changed.
    pub changed: bool,
    /// The change is one step, such as a deleted chip, and not typing.
    pub step: bool,
    /// The chip that the user tapped, which is now the selection, and its
    /// rectangle.
    pub tapped: Option<(Vec<usize>, Rect)>,
    /// Text that the user typed, if the field does not take literal text.
    pub typed: String,
    /// Text that the user pasted, if the field does not take literal text
    /// and the text is not from a copy of nodes.
    pub pasted: String,
    /// The user drags a node.
    pub dragging: bool,
    pub focused: bool,
    /// The layout as the field painted it, on the screen.
    pub layout: Layout,
}

/// The places outside fields where a press keeps the focus of a field, in
/// this frame and in the last frame.
#[derive(Clone, Default)]
pub struct FocusKeep {
    pass: u64,
    now: Vec<Rect>,
    last: Vec<Rect>,
}

fn keep_id() -> egui::Id {
    egui::Id::new("token-field-keep")
}

fn keep_rects(ctx: &egui::Context, f: impl FnOnce(&mut FocusKeep)) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let keep = d.get_temp_mut_or_default::<FocusKeep>(keep_id());
        if keep.pass != pass {
            keep.last = std::mem::take(&mut keep.now);
            keep.pass = pass;
        }
        f(keep);
    });
}

/// A press in `rect` keeps the focus of a field, for a bar or a popover
/// that edits the field.
pub fn keep_focus(ctx: &egui::Context, rect: Rect) {
    keep_rects(ctx, |k| k.now.push(rect));
}

fn kept(ctx: &egui::Context, pos: Pos2) -> bool {
    let mut inside = false;
    keep_rects(ctx, |k| {
        inside = k.now.iter().chain(&k.last).any(|r| r.contains(pos));
    });
    inside
}

/// A press in a field, which can become a drag of a node or open the edit
/// menu.
#[derive(Clone, Debug)]
struct Press {
    /// The node under the press. None in empty space.
    path: Option<Vec<usize>>,
    origin: Pos2,
    time: f64,
    /// The press from the top left of the node.
    grab: Vec2,
    /// The node follows the pointer.
    lifted: bool,
    /// The press began with a touch.
    touch: bool,
    /// The press opened the edit menu or dropped a node, so its release is
    /// not a tap.
    done: bool,
    /// The finger lifted or moved away, so the press does not become a
    /// long press.
    ended: bool,
}

/// Opens the edit menu of the field `name` of `ui` above `anchor`, for
/// example after a tap on a chip.
pub fn open_menu(ui: &egui::Ui, name: &str, anchor: Rect) {
    open_menu_at(ui.ctx(), ui.id().with(name), anchor);
    ui.ctx().request_repaint();
}

/// The node that a press at `pos` picks up: a chip, a line break, a
/// function block at its name, or a group at its frame or its tag.
fn grab_at<P: Clone>(lay: &Layout, field: &Field<P>, pos: Pos2, m: &Metrics) -> Option<Vec<usize>> {
    let atom = |k: &PlacedKind| matches!(k, PlacedKind::Chip | PlacedKind::Break);
    if let Some(p) = (lay.placed.iter().rev()).find(|p| atom(&p.kind) && p.rect.contains(pos)) {
        return Some(p.path.clone());
    }
    // The tag of a group sits on the top line of its frame.
    let tag = vec2(0.0, m.tag_height() / 2.0);
    let frame = (lay.placed.iter().rev())
        .filter(|p| p.kind == PlacedKind::Frame)
        .find(|p| p.rect.expand2(tag).contains(pos))?;
    let Some(Node::Box(b)) = field.node(&frame.path) else {
        return None;
    };
    let on_char =
        (lay.placed.iter()).any(|p| matches!(p.kind, PlacedKind::Char(_)) && p.rect.contains(pos));
    let inner = frame
        .rect
        .shrink2(vec2(m.box_pad.0 / 2.0, m.box_pad.2 / 2.0));
    let grabs = match b.kind {
        BoxKind::Block => frame.rect.contains(pos),
        BoxKind::Group => !on_char && !inner.contains(pos),
        BoxKind::Slot => false,
    };
    grabs.then(|| frame.path.clone())
}

/// Puts `text` in place of the selection: the nodes of the last copy, if
/// it is their text, or else the chars of a literal field. Other text goes
/// to `pasted`. Returns true if the nodes changed.
fn paste_text<P: Token>(
    ctx: &egui::Context,
    field: &mut Field<P>,
    text: &str,
    literal: bool,
    pasted: &mut String,
) -> bool {
    if paste(ctx, field, text) {
        return true;
    }
    if literal {
        field.type_text(text);
        return true;
    }
    pasted.push_str(text);
    false
}

/// Paints the lifted `node` with its top left at `at`, over the panels,
/// with a shadow.
fn paint_lifted<P: Clone>(
    ctx: &egui::Context,
    id: egui::Id,
    node: &Node<P>,
    m: &Metrics,
    at: Pos2,
) {
    let tmp = Field {
        nodes: vec![node.clone()],
        sel: Selection::default(),
    };
    let mut lay = layout(&tmp.nodes, m, f32::INFINITY, &mut measurer(ctx));
    let Some(start) = lay.rect_of(&[0]) else {
        return;
    };
    lay.translate(at - start.min);
    let rect = start.translate(at - start.min);
    let layer = egui::LayerId::new(egui::Order::Tooltip, id.with("lift"));
    let painter = ctx.layer_painter(layer);
    let shadow = egui::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    painter.add(shadow.as_shape(rect, m.radius));
    // The fills of chips are clear, so the picture has a solid back.
    painter.rect_filled(rect, m.radius, LIFT_BACK);
    paint::paint(&painter, &tmp, &lay, m, false);
}

/// The text width of `text` in `font`.
fn measurer(ctx: &egui::Context) -> impl FnMut(&str, &egui::FontId) -> f32 + '_ {
    move |text: &str, font: &egui::FontId| {
        ctx.fonts_mut(|f| text.chars().map(|c| f.glyph_width(font, c)).sum::<f32>())
    }
}

/// The field `name`, as wide as the space of `ui`. It takes the focus with
/// a tap, and while it has the focus it asks for the on-screen keyboard.
pub fn show<P: Token>(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    field: &mut Field<P>,
    m: &Metrics,
    opts: &Options,
) -> Output {
    let ctx = ui.ctx().clone();
    let width = ui.available_width();
    let inner = width - 2.0 * m.pad.x;
    let mut lay = layout(&field.nodes, m, inner, &mut measurer(&ctx));
    let height = (lay.size.y + 2.0 * m.pad.y).max(opts.min_height);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let origin = rect.min + m.pad;
    lay.translate(origin.to_vec2());

    let id = ui.id().with(name);
    let was = ui.memory(|mem| mem.has_focus(id));
    let sense = if opts.long_press {
        Sense::click()
    } else {
        Sense::click_and_drag()
    };
    let response = ui.interact(rect, id, sense);
    let plain = field.plain();
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::text_edit(true, &plain, &plain, "");
        info.label = Some(name.to_owned());
        info
    });
    widgets::push(ui, controls, rect, name);
    let press = ui.input(|i| {
        let any = i.pointer.any_pressed() || i.pointer.any_click();
        i.pointer.interact_pos().filter(|_| any)
    });
    if was && !response.has_focus() && press.is_some_and(|p| kept(&ctx, p)) {
        response.request_focus();
    }

    let mut changed = false;
    let mut step = false;
    let (busy, dragging) = drag(ui, &response, field, &lay, m, opts);
    if dragging.moved {
        changed = true;
        step = true;
    }

    let mut out_tapped = None;
    if response.clicked() && !busy {
        response.request_focus();
        if let Some(pos) = response.interact_pointer_pos() {
            if let Some(chip) = lay.chip_at(pos) {
                field.select_node(&chip.path);
                out_tapped = Some((chip.path.clone(), chip.rect));
            } else if let Some(caret) = lay.stop_at(pos) {
                field.sel = Selection::at(caret);
            }
        }
    }
    let focused = response.has_focus();
    let mut typed = String::new();
    let mut pasted = String::new();
    let menu_id = id.with("menu");
    if let Some(anchor) = ctx.data(|d| d.get_temp::<Rect>(menu_id)) {
        let items: &[&str] = if field.sel.is_empty() {
            &[widgets::PASTE]
        } else {
            &[widgets::CUT, widgets::COPY, widgets::PASTE]
        };
        let edited = match widgets::edit_menu_items(ui, controls, id, anchor, items) {
            Some(EditAction::Cut) => cut(&ctx, field),
            Some(EditAction::Copy) => {
                copy(&ctx, field);
                false
            }
            Some(EditAction::Paste(t)) => paste_text(&ctx, field, &t, opts.literal, &mut pasted),
            None => false,
        };
        if edited {
            changed = true;
            step = true;
        }
        if !widgets::edit_menu_open(&ctx, id) {
            ctx.data_mut(|d| d.remove::<Rect>(menu_id));
        }
    }
    if focused {
        ui.memory_mut(|mem| {
            let filter = egui::EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: false,
            };
            mem.set_focus_lock_filter(id, filter);
        });
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                Event::Copy => {
                    copy(&ctx, field);
                }
                Event::Cut => {
                    if cut(&ctx, field) {
                        changed = true;
                        step = true;
                    }
                }
                Event::Paste(t) => {
                    if paste_text(&ctx, field, &t, opts.literal, &mut pasted) {
                        changed = true;
                        step = true;
                    }
                }
                Event::Text(t) | Event::Ime(ImeEvent::Commit(t)) => {
                    if t.is_empty() {
                        continue;
                    }
                    if opts.literal {
                        field.type_text(&t);
                        changed = true;
                    } else {
                        typed.push_str(&t);
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => match key {
                    Key::Backspace | Key::Delete => {
                        let had = !field.sel.is_empty();
                        let deleted = if key == Key::Backspace {
                            field.backspace()
                        } else {
                            field.delete_forward()
                        };
                        if deleted {
                            changed = true;
                            step |= had;
                        }
                    }
                    Key::ArrowLeft => field.step(Dir::Back, modifiers.shift),
                    Key::ArrowRight => field.step(Dir::Forward, modifiers.shift),
                    Key::ArrowUp | Key::ArrowDown => {
                        let down = key == Key::ArrowDown;
                        if let Some(caret) = vertical(&lay, &field.sel.head, down, m) {
                            field.sel = Selection::at(caret);
                        }
                    }
                    Key::Home => {
                        let path = field.sel.head.path.clone();
                        field.sel = Selection::at(Caret::new(path, 0));
                    }
                    Key::End => {
                        let path = field.sel.head.path.clone();
                        let len = field.list(&path).map_or(0, <[_]>::len);
                        field.sel = Selection::at(Caret::new(path, len));
                    }
                    Key::Enter if opts.enter_breaks => {
                        field.replace(vec![Node::Break]);
                        changed = true;
                        step = true;
                    }
                    Key::A if modifiers.command => field.select_all(),
                    Key::Escape => ui.memory_mut(|mem| mem.surrender_focus(id)),
                    _ => {}
                },
                _ => {}
            }
        }
    }
    if changed {
        field.clamp();
        lay = layout(&field.nodes, m, inner, &mut measurer(&ctx));
        lay.translate(origin.to_vec2());
        ctx.request_repaint();
    }

    let painter = ui.painter_at(rect.expand(2.0));
    painter.rect_filled(rect, theme::CONTROL_RADIUS + 2.0, FIELD_FILL);
    if focused {
        let stroke = Stroke::new(1.5, theme::ACCENT);
        painter.rect_stroke(
            rect,
            theme::CONTROL_RADIUS + 2.0,
            stroke,
            egui::StrokeKind::Inside,
        );
    }
    paint::paint(
        ui.painter(),
        field,
        &lay,
        m,
        focused && dragging.lifted.is_none(),
    );
    if let Some(lift) = &dragging.lifted {
        if let Some(r) = lay.rect_of(&lift.path) {
            painter.rect_filled(r.expand(1.0), m.radius, Color32::from_black_alpha(150));
        }
        if let Some(c) = lift.target.as_ref().and_then(|t| lay.caret_rect(t)) {
            let line = Rect::from_center_size(c.center(), vec2(DROP_LINE, c.height() + 4.0));
            painter.rect_filled(line, 1.0, theme::ACCENT);
        }
        if let Some(node) = field.node(&lift.path) {
            paint_lifted(&ctx, id, node, m, lift.at);
        }
    }

    if focused {
        let cursor = lay
            .caret_rect(&field.sel.head)
            .unwrap_or_else(|| Rect::from_min_size(origin, vec2(2.0, m.caret)));
        ctx.output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                purpose: egui::IMEPurpose::Normal,
                rect,
                cursor_rect: cursor,
                should_interrupt_composition: false,
            });
        });
    }
    Output {
        response,
        changed,
        step,
        tapped: out_tapped,
        typed,
        pasted,
        dragging: dragging.lifted.is_some(),
        focused,
        layout: lay,
    }
}

/// A node that follows the pointer: its path, the place where it goes, and
/// the top left of its picture.
struct Lift {
    path: Vec<usize>,
    target: Option<Caret>,
    at: Pos2,
}

/// What a drag did in one frame.
#[derive(Default)]
struct Dragging {
    lifted: Option<Lift>,
    moved: bool,
}

/// Follows a press in the field: a long press or a drag lifts a node, and
/// its release moves the node to the nearest place where it can go. A
/// release outside the field puts it back. A long press in empty space
/// puts the caret there and opens the edit menu. Returns true if the press
/// is not a tap, and the drag.
fn drag<P: Token>(
    ui: &egui::Ui,
    response: &Response,
    field: &mut Field<P>,
    lay: &Layout,
    m: &Metrics,
    opts: &Options,
) -> (bool, Dragging) {
    let ctx = ui.ctx();
    let id = response.id;
    let key = id.with("drag");
    let mut press: Option<Press> = ctx.data(|d| d.get_temp(key));
    let (pressed, down, touches, pos, now, released) = ui.input(|i| {
        let pressed = (i.pointer.any_pressed())
            .then(|| i.pointer.press_origin())
            .flatten();
        let released = i.events.iter().rev().find_map(|e| match e {
            Event::PointerButton {
                pos,
                pressed: false,
                ..
            } => Some(*pos),
            _ => None,
        });
        let pos = i.pointer.interact_pos();
        (
            pressed,
            i.pointer.any_down(),
            i.any_touches(),
            pos,
            i.time,
            released,
        )
    });
    let menu = widgets::edit_menu_rect(ctx);
    if let Some(p) = pressed
        && response.rect.contains(p)
        && response.contains_pointer()
        && !menu.is_some_and(|r| r.contains(p))
    {
        let path = grab_at(lay, field, p, m);
        press = (path.is_some() || opts.long_press).then(|| Press {
            grab: (path.as_ref())
                .and_then(|q| lay.rect_of(q))
                .map_or(Vec2::ZERO, |r| p - r.min),
            path,
            origin: p,
            time: now,
            touch: touches,
            lifted: false,
            done: false,
            ended: false,
        });
    }
    let mut out = Dragging::default();
    let Some(mut pr) = press else {
        return (false, out);
    };
    // The end of a touch ends the press, also before the release of the
    // pointer.
    let up = released.is_some() || !down;
    let ending = !pr.ended && (up || (pr.touch && !touches));
    let at = released.or(pos).unwrap_or(pr.origin);
    if !pr.lifted && !pr.done && !pr.ended && !ending {
        let motion = (at - pr.origin).length();
        let held = now - pr.time;
        let lift = if opts.long_press {
            if motion > STILL {
                pr.ended = true;
            } else if held < LONG_PRESS {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(LONG_PRESS - held));
            }
            !pr.ended && held >= LONG_PRESS
        } else {
            motion > DRAG_START
        };
        if lift {
            if pr.path.is_some() {
                pr.lifted = true;
                ctx.set_dragged_id(id);
            } else if let Some(caret) = lay.stop_at(pr.origin) {
                pr.done = true;
                field.sel = Selection::at(caret.clone());
                let anchor = lay.caret_rect(&caret).unwrap_or(response.rect);
                open_menu_at(ctx, id, anchor);
            }
        }
    }
    if pr.lifted
        && let Some(from) = pr.path.clone()
    {
        let target = lay.stop_near(at, |c| !c.path.starts_with(&from));
        if ending {
            if response.rect.contains(at)
                && let Some(t) = &target
            {
                out.moved = field.move_node(&from, t);
            }
            pr.lifted = false;
            pr.done = true;
        } else {
            out.lifted = Some(Lift {
                path: from,
                target,
                // The node rises above the finger, so that the drop line
                // shows.
                at: at - pr.grab - vec2(0.0, m.chip),
            });
            ctx.request_repaint();
        }
    }
    pr.ended |= ending;
    let busy = pr.lifted || pr.done;
    if up {
        ctx.data_mut(|d| d.remove::<Press>(key));
    } else {
        ctx.data_mut(|d| d.insert_temp(key, pr));
    }
    (busy, out)
}

/// Opens the edit menu of the field `id` above `anchor`.
fn open_menu_at(ctx: &egui::Context, id: egui::Id, anchor: Rect) {
    widgets::open_edit_menu(ctx, id, anchor);
    ctx.data_mut(|d| d.insert_temp(id.with("menu"), anchor));
}

/// The place of the caret on the line above or below `from`.
fn vertical(lay: &Layout, from: &Caret, down: bool, m: &Metrics) -> Option<Caret> {
    let r = lay.caret_rect(from)?;
    let dy = m.chip + m.line_gap;
    let y = if down {
        r.center().y + dy
    } else {
        r.center().y - dy
    };
    let target = lay.stop_at(pos2(r.center().x, y))?;
    let moved = lay.caret_rect(&target)?;
    ((moved.center().y - r.center().y).abs() > m.chip / 2.0).then_some(target)
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use vale_style::expr::editor::{Piece, Template};

    use super::*;
    use crate::ui::expr_help::ExprHelp;
    use crate::ui::token_field::template;

    #[test]
    fn token_field_asks_for_keyboard() {
        let start = Template::from_text("concat('Pop ', pop)");
        let field = template::field_of(&start, &ExprHelp::empty());
        let opts = Options {
            literal: true,
            enter_breaks: true,
            min_height: 56.0,
            long_press: true,
        };
        let app = move |ui: &mut egui::Ui, f: &mut Field<Piece>| {
            show(ui, &mut Vec::new(), "Label text", f, &Metrics::pad(), &opts);
        };
        let mut h = Harness::new_ui_state(app, field);
        h.run();
        assert!(h.output().platform_output.ime.is_none());
        h.get_by_label("Label text").click();
        h.run();
        assert!(h.output().platform_output.ime.is_some());
        // The tap was right of the chip, so the caret is at the end.
        h.event(Event::Text("!".into()));
        h.run();
        assert_eq!(
            template::template_of(h.state()).to_text(),
            "concat('Pop ', pop, '!')"
        );
        h.key_press(Key::Backspace);
        h.key_press(Key::Backspace);
        h.run();
        assert!(h.state().selected_node().is_some());
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(template::template_of(h.state()).to_text(), "'Pop \n'");
        assert!(h.output().platform_output.ime.is_some());
    }

    /// A desktop field of label text `text`, and the layout of its last
    /// frame.
    fn desktop(text: &str) -> Harness<'static, (Field<Piece>, Layout)> {
        let field = template::field_of(&Template::from_text(text), &ExprHelp::empty());
        let opts = Options {
            literal: true,
            enter_breaks: false,
            min_height: 0.0,
            long_press: false,
        };
        let app = move |ui: &mut egui::Ui, (f, lay): &mut (Field<Piece>, Layout)| {
            let out = show(
                ui,
                &mut Vec::new(),
                "Label text",
                f,
                &Metrics::desktop(),
                &opts,
            );
            *lay = out.layout;
        };
        let mut h = Harness::new_ui_state(app, (field, Layout::default()));
        h.run();
        h
    }

    fn button(h: &mut Harness<'_, (Field<Piece>, Layout)>, pos: Pos2, pressed: bool) {
        h.event(Event::PointerMoved(pos));
        h.event(Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }

    fn text(h: &Harness<'_, (Field<Piece>, Layout)>) -> String {
        template::template_of(&h.state().0).to_text()
    }

    #[test]
    fn desktop_drag_moves_chip() {
        let mut h = desktop("concat(name, ' ', pop)");
        let lay = h.state().1.clone();
        let pop = lay.rect_of(&[2]).unwrap().center();
        let start = lay.caret_rect(&Caret::new(Vec::new(), 0)).unwrap().center();
        button(&mut h, pop, true);
        for i in 1..=4 {
            h.event(Event::PointerMoved(pop.lerp(start, i as f32 / 4.0)));
            h.step();
        }
        button(&mut h, start, false);
        h.run();
        assert_eq!(text(&h), "concat(pop, name, ' ')");
        assert_eq!(h.state().0.sel, Selection::range(Vec::new(), 0, 1));
    }

    #[test]
    fn cmd_c_cmd_v_on_desktop() {
        let mut h = desktop("concat(name, ' ', round(pop, 1))");
        h.get_by_label("Label text").click();
        h.run();
        h.state_mut().0.select_node(&[2]);
        h.event(Event::Copy);
        h.step();
        let copied = h
            .output()
            .platform_output
            .commands
            .iter()
            .find_map(|c| match c {
                egui::OutputCommand::CopyText(t) => Some(t.clone()),
                _ => None,
            });
        assert_eq!(copied.as_deref(), Some("round(pop, 1)"));
        h.state_mut().0.sel = Selection::at(Caret::new(Vec::new(), 0));
        h.event(Event::Paste("round(pop, 1)".into()));
        h.run();
        assert_eq!(text(&h), "concat(round(pop, 1), name, ' ', round(pop, 1))");
        // Text from another app is literal text.
        h.event(Event::Paste("upper(x)".into()));
        h.run();
        assert_eq!(
            text(&h),
            "concat(round(pop, 1), 'upper(x)', name, ' ', round(pop, 1))"
        );
        h.state_mut().0.select_node(&[0]);
        h.event(Event::Cut);
        h.run();
        assert_eq!(text(&h), "concat('upper(x)', name, ' ', round(pop, 1))");
    }
}
