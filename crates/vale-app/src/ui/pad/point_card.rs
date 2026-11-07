//! The form of a new point on a compact screen: a card below the top row,
//! in place of the sheet of the inspector. The keyboard of the system is
//! at the bottom, and the globe moves the new point between the two.

use eframe::egui::{self, Align2, Order, Rect, Sense, pos2, vec2};

use super::geometry::{GAP, MARGIN, PAD, ROW, Rects, TOUCH};
use super::theme;
use super::widgets::{self, BODY_PAD, Controls, PANEL_ROW};
use crate::ui::new_feature::keyboard_top;
use crate::ui::{AppState, select};

/// The button that removes the new point.
pub const CANCEL: &str = "Cancel";
/// The button that keeps the new point with the values of the card.
pub const ADD: &str = "Add";
/// The name of the card. The card keeps its height under this name.
const KEY: &str = "pad-point-card";
/// The room between the card and the top of the keyboard, for the new
/// point. A card with more fields scrolls.
const POINT_ROOM: f32 = 96.0;

/// The title of the card: "New" and the name of the layer in the singular,
/// for example "New place" for the layer "Places".
pub fn title(layer: &str) -> String {
    let name = layer.trim().to_lowercase();
    let one = if let Some(stem) = name.strip_suffix("ies") {
        format!("{stem}y")
    } else if name.ends_with("ss") {
        name
    } else if let Some(stem) = name.strip_suffix('s') {
        stem.to_owned()
    } else {
        name
    };
    format!("New {one}")
}

/// Shows the card: the title, Cancel, and Add, and below them the fields
/// of the new point. The card is directly below the top row, and it fills
/// the width inside the margins. Returns the rectangle of the card.
pub fn show(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) -> Rect {
    let min = rects.right_panel;
    let right = rects.screen.right() - rects.bands.right - MARGIN;
    // Before the first frame of the card, the height comes from the count
    // of the fields.
    let fields = state.selection_form.as_ref().map_or(0, |f| f.fields.len());
    let guess = ROW + fields as f32 * (PANEL_ROW + 6.0) + 10.0;
    let bottom = rects
        .panel_bottom
        .min(keyboard_top(rects.screen) - POINT_ROOM);
    let most = (bottom - min.y).max(ROW);
    let height = state.pad.heights.get(KEY).copied().unwrap_or(guess);
    let rect = Rect::from_min_size(min, vec2(right - min.x, height.min(most)));
    let layer = state.new_feature.as_ref().map(|n| n.feature.layer);
    let name = layer
        .and_then(|id| state.doc.layer(id))
        .map(|l| title(&l.name));
    let name = name.unwrap_or_default();
    let (mut add, mut cancel) = (false, false);
    let backdrop = state.globe.backdrop();
    let radius = theme::CARD_RADIUS.into();
    widgets::card(ctx, backdrop, KEY, rect, Order::Middle, radius, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
        let add_button = pos2(row.right() - PAD, row.center().y);
        let add_resp = widgets::accent_button(ui, controls, add_button, ADD);
        add = add_resp.clicked();
        let font = theme::body();
        let galley = ui
            .painter()
            .layout_no_wrap(CANCEL.into(), font.clone(), theme::ACCENT);
        let width = (galley.size().x + 2.0 * GAP).max(TOUCH);
        let cancel_rect = Rect::from_min_max(
            pos2(add_resp.rect.left() - width, row.center().y - TOUCH / 2.0),
            pos2(add_resp.rect.left(), row.center().y + TOUCH / 2.0),
        );
        cancel = widgets::tap_area(ui, controls, cancel_rect, CANCEL, true).clicked();
        let at = cancel_rect.center();
        let painter = ui.painter();
        painter.text(at, Align2::CENTER_CENTER, CANCEL, font, theme::ACCENT);
        let text_left = row.left() + BODY_PAD;
        let clip = Rect::from_min_max(
            pos2(text_left, row.top()),
            pos2(cancel_rect.left(), row.bottom()),
        );
        let at = pos2(text_left, row.center().y);
        painter.with_clip_rect(clip).text(
            at,
            Align2::LEFT_CENTER,
            &name,
            theme::title(),
            theme::TEXT,
        );

        let most = (rect.bottom() - ui.cursor().top()).max(0.0);
        let out = egui::ScrollArea::vertical()
            .id_salt(KEY)
            .max_height(most)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let margin = egui::Margin {
                    left: BODY_PAD as i8,
                    right: BODY_PAD as i8,
                    top: 0,
                    bottom: 10,
                };
                egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    select::new_point_body(ui, state, controls);
                });
            });
        state.pad.heights.insert(KEY, ROW + out.content_size.y);
    });
    // The sheet of the inspector goes down to its title row, so it does
    // not cover the globe after the card.
    if add {
        state.commit_new_feature();
        state.pad.sheet_offset = rects.sheet_travel;
    } else if cancel {
        state.cancel_new_feature();
        state.pad.sheet_offset = rects.sheet_travel;
    }
    rect
}

#[cfg(test)]
mod tests {
    use super::title;

    #[test]
    fn the_title_has_the_name_of_the_layer_in_the_singular() {
        assert_eq!(title("Places"), "New place");
        assert_eq!(title("Cities"), "New city");
        assert_eq!(title("Fortress"), "New fortress");
        assert_eq!(title("Layer 3"), "New layer 3");
    }
}
