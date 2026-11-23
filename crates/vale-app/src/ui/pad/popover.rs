//! A popover: a card under a control, which grows out of the control. A
//! press outside the popover and its control closes it.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};

use super::geometry::{self, Bands};
use super::{theme, widgets};
use crate::ui::token_field;

/// The fill of a popover.
pub const FILL: Color32 = Color32::from_rgb(31, 34, 38);
/// The space between the popover and its control.
const GAP: f32 = 9.0;
/// The least space between the place that the card grows from and a side
/// of the card.
const ORIGIN_EDGE: f32 = theme::CARD_RADIUS + GAP;
/// The space between the popover and the edge of the screen.
const MARGIN: f32 = 8.0;
/// The distance from the left of the card to the middle of the control.
const CONTROL_LEFT: f32 = 52.0;

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

/// The card of a popover of `size` at `anchor`, inside `room`, and true if
/// it is under the anchor. The card is under the anchor if it fits there,
/// or else above it if it fits there, or else on the side with more room,
/// with the height of that side, and its contents scroll. Only a side with
/// less than `SIDE_MIN` lets the card cover its control. A card that is
/// wider than the room gets its width.
pub fn place(anchor: Rect, size: egui::Vec2, room: Rect) -> (Rect, bool) {
    let (top, bottom) = (room.top(), room.bottom().max(room.top()));
    let room_below = bottom - (anchor.bottom() + GAP);
    let room_above = anchor.top() - GAP - top;
    let below = room_below >= size.y || (room_above < size.y && room_below >= room_above);
    let side = if below { room_below } else { room_above };
    let height = match side >= SIDE_MIN {
        true => size.y.min(side),
        false => size.y.min(bottom - top),
    };
    let width = size.x.min(room.width());
    let x = (anchor.center().x - CONTROL_LEFT).clamp(room.left(), room.right() - width);
    let y = if below {
        anchor.bottom() + GAP
    } else {
        anchor.top() - GAP - height
    };
    // A popover that does not fit covers its control, and not the floor.
    let y = y.min(bottom - height).max(top);
    (Rect::from_min_size(pos2(x, y), vec2(width, height)), below)
}

/// The least height of a popover beside its control: two rows. With less
/// room on both sides, the popover covers its control.
const SIDE_MIN: f32 = 2.0 * 48.0;

fn layout_id() -> egui::Id {
    egui::Id::new("pad-popover-layout")
}

/// Keeps the places of the layout of this frame, for the room of the
/// popovers and the cards that open from a control.
pub fn set_layout(ctx: &egui::Context, rects: &geometry::Rects) {
    ctx.data_mut(|d| d.insert_temp(layout_id(), *rects));
}

/// The room of a card that opens from `anchor` on a compact screen, or
/// `None` on a wider screen.
pub fn compact_room(ctx: &egui::Context, anchor: Rect) -> Option<Rect> {
    let rects: Option<geometry::Rects> = ctx.data(|d| d.get_temp(layout_id()));
    rects
        .filter(|r| r.class == geometry::WidthClass::Compact && r.screen == ctx.viewport_rect())
        .map(|_| room(ctx, anchor))
}

/// The room of a popover or a card that opens from `anchor`: the rule of
/// `geometry::popover_room` for the layout of this frame, in the part of
/// `anchor` on a partially folded device, and above the floor of
/// `set_floor`.
pub fn room(ctx: &egui::Context, anchor: Rect) -> Rect {
    let screen = ctx.viewport_rect();
    let rects: Option<geometry::Rects> = ctx.data(|d| d.get_temp(layout_id()));
    let mut room = match rects.filter(|r| r.screen == screen) {
        Some(rects) => geometry::popover_room(&rects, anchor),
        None => {
            let top = screen.top() + Bands::of(ctx).top;
            Rect::from_min_max(pos2(screen.left(), top), screen.max).shrink(MARGIN)
        }
    };
    let part = geometry::fold_part(anchor, geometry::regions(ctx).fold, screen);
    room = room.intersect(part.shrink2(vec2(0.0, MARGIN)));
    room.max.y = room.max.y.min(floor(ctx, screen) - MARGIN);
    room
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
    let (rect, below) = place(anchor, vec2(width, height), room(ctx, anchor));
    let pressed = ctx.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed()));
    let closed = pressed.is_some_and(|p| !rect.contains(p) && !anchor.contains(p));
    token_field::keep_focus(ctx, rect);
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(rect_id(), (pass, rect)));

    // The card grows from the edge next to its control.
    let tip = pos2(
        anchor
            .center()
            .x
            .clamp(rect.left() + ORIGIN_EDGE, rect.right() - ORIGIN_EDGE),
        if below { rect.top() } else { rect.bottom() },
    );
    let grow = widgets::grow(ctx, egui::LayerId::new(egui::Order::Tooltip, id), tip);
    // The popover has a layer of its own, and the safe area does not cut
    // it.
    egui::Area::new(id)
        .order(egui::Order::Tooltip)
        .fixed_pos(rect.min)
        .movable(false)
        .constrain_to(ctx.viewport_rect())
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            let (card, _) = ui.allocate_exact_size(rect.size(), Sense::click());
            let radius = theme::CARD_RADIUS.min(theme::capsule(card));
            ui.multiply_opacity(grow.opacity);
            let painter = ui.painter();
            let order = egui::Order::Tooltip;
            widgets::paint_card(painter, order, card, radius, grow.transform);
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
        // The room from the top safe area to the top of the bar above the
        // keyboard.
        let room = Rect::from_min_max(pos2(8.0, 32.0), pos2(1358.0, 492.0));
        let inside = |r: Rect| room.expand(0.01).contains_rect(r);
        let size = vec2(340.0, 300.0);

        // Near the top, the popover opens under the chip.
        let chip = Rect::from_min_size(pos2(900.0, 40.0), vec2(80.0, 32.0));
        let (rect, below) = place(chip, size, room);
        assert!(below && inside(rect), "{rect:?}");
        assert!(rect.top() >= chip.bottom());

        // Near the bar, it opens above the chip.
        let chip = Rect::from_min_size(pos2(900.0, 440.0), vec2(80.0, 32.0));
        let (rect, below) = place(chip, size, room);
        assert!(!below && inside(rect), "{rect:?}");
        assert!(rect.bottom() <= chip.top());

        // A popover that fits on neither side opens on the side with more
        // room, with the height of that side, and does not cover the chip.
        let chip = Rect::from_min_size(pos2(900.0, 250.0), vec2(80.0, 32.0));
        for size in [vec2(340.0, 400.0), vec2(340.0, 900.0)] {
            let (rect, below) = place(chip, size, room);
            assert!(inside(rect), "{size:?}: {rect:?}");
            assert!(!below && !rect.intersects(chip), "{size:?}: {rect:?}");
            assert_eq!(rect.top(), room.top());
        }
        // With no room for two rows on either side, it covers the chip.
        let tight = Rect::from_min_max(pos2(8.0, 200.0), pos2(1358.0, 340.0));
        let (rect, _) = place(chip, size, tight);
        assert_eq!(rect.y_range(), tight.y_range());
        // A popover that fits stays its own height.
        let (rect, _) = place(chip, vec2(340.0, 120.0), room);
        assert_eq!(rect.height(), 120.0);
        // A popover wider than the room gets its width.
        let narrow = Rect::from_min_max(pos2(8.0, 32.0), pos2(300.0, 492.0));
        let (rect, _) = place(chip, size, narrow);
        assert_eq!(rect.x_range(), narrow.x_range());
    }
}
