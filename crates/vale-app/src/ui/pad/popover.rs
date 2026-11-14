//! A popover: a card under a control, with an arrow that points at the
//! control. A press outside the popover and its control closes it.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};

use super::geometry::Bands;
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

fn floor_id() -> egui::Id {
    egui::Id::new("pad-popover-floor")
}

/// Keeps popovers above `y` in this frame and the next, for a bar above the
/// keyboard.
pub fn set_floor(ctx: &egui::Context, y: f32) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(floor_id(), (pass, y)));
}

/// The bottom of the space of a popover.
fn floor(ctx: &egui::Context, screen: Rect) -> f32 {
    let set: Option<(u64, f32)> = ctx.data(|d| d.get_temp(floor_id()));
    let pass = ctx.cumulative_pass_nr();
    set.filter(|(p, _)| p + 1 >= pass)
        .map_or(screen.bottom(), |(_, y)| y.min(screen.bottom()))
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

/// The card of a popover of `size` at `anchor`, inside the visible area
/// from `top` to `bottom`, and true if it is under the anchor. The card is
/// under the anchor if it fits there, or else above it if it fits there, or
/// else on the side with more room. A card that is higher than the area
/// gets the height of the area, and its contents scroll.
pub fn place(
    anchor: Rect,
    size: egui::Vec2,
    screen: Rect,
    (top, bottom): (f32, f32),
) -> (Rect, bool) {
    let (top, bottom) = (top + MARGIN, (bottom - MARGIN).max(top + MARGIN));
    let room_below = bottom - (anchor.bottom() + ARROW);
    let room_above = anchor.top() - ARROW - top;
    let below = room_below >= size.y || (room_above < size.y && room_below >= room_above);
    let height = size.y.min(bottom - top);
    let x = (anchor.center().x - ARROW_LEFT).clamp(
        screen.left() + MARGIN,
        (screen.right() - MARGIN - size.x).max(screen.left()),
    );
    let y = if below {
        anchor.bottom() + ARROW
    } else {
        anchor.top() - ARROW - height
    };
    // A popover that does not fit covers its control, and not the floor.
    let y = y.min(bottom - height).max(top);
    (Rect::from_min_size(pos2(x, y), vec2(size.x, height)), below)
}

/// The popover `key` of `width` under `anchor`, or above it if the screen
/// has no room below it. The contents decide the height, and they scroll
/// if the visible area is too low for them. A press in the popover keeps
/// the focus of a token field. The popover stays below the top safe area
/// and above the floor of `set_floor`, and it draws above the bar of the
/// keyboard.
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
    let top = screen.top() + Bands::of(ctx).top;
    let area = (top, floor(ctx, screen));
    let (rect, below) = place(anchor, vec2(width, height), screen, area);
    let pressed = ctx.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed()));
    let closed = pressed.is_some_and(|p| !rect.contains(p) && !anchor.contains(p));
    token_field::keep_focus(ctx, rect);
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(rect_id(), (pass, rect)));

    egui::Area::new(id)
        .order(egui::Order::Tooltip)
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
            let mut used = 0.0f32;
            egui::ScrollArea::vertical()
                .id_salt(id.with("scroll"))
                .max_height(rows.height())
                .auto_shrink([false, true])
                .show(&mut inner, |ui| {
                    add_contents(ui);
                    used = ui.min_rect().height();
                });
            let used = used.max(48.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popover_stays_inside_visible_area() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1366.0, 1024.0));
        // The top safe area and the top of the bar above the keyboard.
        let (top, bottom) = (24.0, 500.0);
        let inside =
            |r: Rect| r.top() >= top + MARGIN - 0.01 && r.bottom() <= bottom - MARGIN + 0.01;
        let size = vec2(340.0, 300.0);

        // Near the top, the popover opens under the chip.
        let chip = Rect::from_min_size(pos2(900.0, 40.0), vec2(80.0, 32.0));
        let (rect, below) = place(chip, size, screen, (top, bottom));
        assert!(below && inside(rect), "{rect:?}");
        assert!(rect.top() >= chip.bottom());

        // Near the bar, it opens above the chip.
        let chip = Rect::from_min_size(pos2(900.0, 440.0), vec2(80.0, 32.0));
        let (rect, below) = place(chip, size, screen, (top, bottom));
        assert!(!below && inside(rect), "{rect:?}");
        assert!(rect.bottom() <= chip.top());

        // A popover that fits on neither side stays inside, and gets the
        // height of the area at most.
        let chip = Rect::from_min_size(pos2(900.0, 250.0), vec2(80.0, 32.0));
        for size in [vec2(340.0, 400.0), vec2(340.0, 900.0)] {
            let (rect, _) = place(chip, size, screen, (top, bottom));
            assert!(inside(rect), "{size:?}: {rect:?}");
            assert!(rect.height() <= bottom - top - 2.0 * MARGIN + 0.01);
        }
        // A popover that fits stays its own height.
        let (rect, _) = place(chip, vec2(340.0, 120.0), screen, (top, bottom));
        assert_eq!(rect.height(), 120.0);
    }
}
