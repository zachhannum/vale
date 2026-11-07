//! A popover: a card under a control, with an arrow that points at the
//! control. A press outside the popover and its control closes it.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};

use super::theme;
use crate::ui::token_field;

/// The fill of a popover.
pub const FILL: Color32 = Color32::from_rgb(31, 34, 38);
/// The size of the arrow, from the card to its tip.
const ARROW: f32 = 9.0;
/// The space between the popover and the edge of the screen.
const MARGIN: f32 = 8.0;
/// The distance from the left of the card to the arrow, as the arrow
/// points at the middle of the control.
const ARROW_LEFT: f32 = 52.0;

/// What a popover did in one frame.
pub struct Shown {
    /// The card of the popover.
    pub rect: Rect,
    /// The user pressed outside the popover and its control.
    pub closed: bool,
}

fn rect_id() -> egui::Id {
    egui::Id::new("pad-popover-rect")
}

/// The padding at the left and the right of the rows of a popover.
pub const PAD: f32 = 16.0;

/// The card of a popover in this frame or in the last frame. A press on it
/// does not close the card of its control.
pub fn rect(ctx: &egui::Context) -> Option<Rect> {
    let shown: Option<(u64, Rect)> = ctx.data(|d| d.get_temp(rect_id()));
    let pass = ctx.cumulative_pass_nr();
    shown.filter(|(p, _)| p + 1 >= pass).map(|(_, r)| r)
}

/// The popover `key` of `width` under `anchor`, or above it if the screen
/// has no room below it. The contents decide the height. A press in the
/// popover keeps the focus of a token field.
pub fn show(
    ctx: &egui::Context,
    key: &str,
    anchor: Rect,
    width: f32,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> Shown {
    let id = egui::Id::new(key);
    let height: f32 = ctx.data(|d| d.get_temp(id.with("height"))).unwrap_or(240.0);
    let screen = ctx.viewport_rect();
    let room_below = screen.bottom() - MARGIN - (anchor.bottom() + ARROW);
    let room_above = anchor.top() - ARROW - (screen.top() + MARGIN);
    let below = room_below >= height || room_below >= room_above;
    let x = (anchor.center().x - ARROW_LEFT).clamp(
        screen.left() + MARGIN,
        (screen.right() - MARGIN - width).max(screen.left()),
    );
    let y = if below {
        anchor.bottom() + ARROW
    } else {
        anchor.top() - ARROW - height
    };
    let rect = Rect::from_min_size(pos2(x, y), vec2(width, height));
    let pressed = ctx.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed()));
    let closed = pressed.is_some_and(|p| !rect.contains(p) && !anchor.contains(p));
    token_field::keep_focus(ctx, rect);
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(rect_id(), (pass, rect)));

    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .movable(false)
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            let radius = theme::CARD_RADIUS;
            let (card, _) = ui.allocate_exact_size(rect.size(), Sense::click());
            let painter = ui.painter();
            painter.add(theme::shadow().as_shape(card, radius));
            painter.rect_filled(card, radius, FILL);
            let tip_x = anchor
                .center()
                .x
                .clamp(card.left() + 20.0, card.right() - 20.0);
            let arrow = if below {
                let base = card.top() + 1.0;
                [
                    pos2(tip_x - ARROW, base),
                    pos2(tip_x, base - ARROW),
                    pos2(tip_x + ARROW, base),
                ]
            } else {
                let base = card.bottom() - 1.0;
                [
                    pos2(tip_x - ARROW, base),
                    pos2(tip_x + ARROW, base),
                    pos2(tip_x, base + ARROW),
                ]
            };
            painter.add(Shape::convex_polygon(arrow.to_vec(), FILL, Stroke::NONE));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let rows = card.shrink2(vec2(PAD, 0.0));
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rows).layout(layout));
            theme::apply(&mut inner);
            inner.spacing_mut().item_spacing.y = 0.0;
            add_contents(&mut inner);
            let used = inner.min_rect().height().max(48.0);
            if (used - height).abs() > 0.5 {
                ctx.data_mut(|d| d.insert_temp(id.with("height"), used));
                ctx.request_repaint();
            }
        });
    Shown { rect, closed }
}

/// A divider across the popover at the top of the next row.
pub fn divider(ui: &egui::Ui) {
    let y = ui.cursor().top();
    let rect = ui.max_rect().expand2(vec2(PAD, 0.0));
    let line = [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)];
    ui.painter()
        .line_segment(line, Stroke::new(1.0, theme::divider()));
}
