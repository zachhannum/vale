//! The field as a widget: the taps, the keys, the on-screen keyboard, and
//! the picture.

use eframe::egui::{self, Event, ImeEvent, Key, Pos2, Rect, Response, Sense, Stroke, pos2, vec2};

use super::caret::{Caret, Dir, Selection};
use super::layout::{Layout, Metrics, layout};
use super::paint::{self, FIELD_FILL};
use super::{Field, Node};
use crate::ui::pad::theme;
use crate::ui::pad::widgets::{self, Controls};

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
}

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

/// The text width of `text` in `font`.
fn measurer(ctx: &egui::Context) -> impl FnMut(&str, &egui::FontId) -> f32 + '_ {
    move |text: &str, font: &egui::FontId| {
        ctx.fonts_mut(|f| text.chars().map(|c| f.glyph_width(font, c)).sum::<f32>())
    }
}

/// The field `name`, as wide as the space of `ui`. It takes the focus with
/// a tap, and while it has the focus it asks for the on-screen keyboard.
pub fn show<P: Clone>(
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
    let response = ui.interact(rect, id, Sense::click());
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

    let mut out_tapped = None;
    if response.clicked() {
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
    let mut changed = false;
    let mut step = false;
    let mut typed = String::new();
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
                Event::Text(t) | Event::Paste(t) | Event::Ime(ImeEvent::Commit(t)) => {
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
    paint::paint(ui.painter(), field, &lay, m, focused);

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
        focused,
        layout: lay,
    }
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
}
