//! The card of the map on the globe, at the top center of the canvas: the
//! name of the map and a close button.

use eframe::egui::{self, Order, Rect, pos2, vec2};

use super::geometry::{self, PAD, Rects, TOUCH};
use super::icons::{self, Icon};
use super::theme;
use super::widgets::{self, Controls};
use crate::ui::{AppState, Workspace};

/// The name of the close button of the card.
pub const CLOSE_MAP: &str = "Close map";

/// The space at the left of the icon and between the icon and the name.
const LEAD: f32 = 14.0;
const ICON: f32 = 20.0;

/// Shows the card while the globe shows a map. A tap on the close button
/// removes the extent and keeps the checks. The card goes below the
/// banner when the banner shows.
pub(super) fn show(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    state.pad.map_card = None;
    if state.workspace != Workspace::Globe {
        return;
    }
    let Some(map) = state.map_on_globe() else {
        return;
    };
    let name = (state.doc.project.atlas.map(map)).map_or_else(String::new, |m| m.name.clone());
    let screen = geometry::canvas_part(rects.screen, rects.fold).shrink(geometry::PANEL_GAP);
    let room = (screen.width() - 2.0 * LEAD - ICON - TOUCH - 2.0 * PAD).max(TOUCH);
    let galley = ctx.fonts_mut(|f| f.layout(name.clone(), theme::body(), theme::TEXT, room));
    let text = galley.size();
    let size = vec2(
        LEAD + ICON + LEAD + text.x.ceil() + TOUCH + PAD,
        (text.y.ceil() + 2.0 * PAD).max(TOUCH) + 2.0 * PAD,
    );
    let top = state
        .pad
        .banner
        .map_or(rects.column_top, |b| b.bottom() + geometry::PANEL_GAP);
    let left = (screen.center().x - size.x / 2.0).max(screen.left());
    let rect = Rect::from_min_size(pos2(left, top), size);
    state.pad.map_card = Some(rect);
    let radius = widgets::card_radius(rect, egui::CornerRadius::from(theme::capsule(rect)));
    let backdrop = widgets::backdrop(ctx);
    let mut close = false;
    widgets::card(
        ctx,
        backdrop,
        "pad-map-card",
        rect,
        Order::Foreground,
        radius,
        |ui| {
            let y = rect.center().y;
            let icon =
                Rect::from_center_size(pos2(rect.left() + LEAD + ICON / 2.0, y), vec2(ICON, ICON));
            icons::paint(ui.painter(), icon, Icon::Map, ICON, theme::TEXT);
            let at = pos2(icon.right() + LEAD, y);
            let place = Rect::from_min_size(at - vec2(0.0, text.y / 2.0), text);
            let label = egui::Label::new(
                egui::RichText::new(&name)
                    .font(theme::body())
                    .color(theme::TEXT),
            );
            ui.put(place.expand(1.0), label.selectable(false).wrap());
            let button = Rect::from_center_size(
                pos2(rect.right() - PAD - TOUCH / 2.0, y),
                vec2(TOUCH, TOUCH),
            );
            close =
                widgets::icon_button_at(ui, controls, button, CLOSE_MAP, Icon::Close, false, true)
                    .clicked();
        },
    );
    if close {
        state.close_map_on_globe();
    }
}
