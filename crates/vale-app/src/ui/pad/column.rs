//! The tool column at the left edge, from 600 points: the tool cells, a
//! divider, and the controls of the tool. Its cells have the icon look.

use eframe::egui::{self, CornerRadius, Order, Rect, vec2};
use eframe::egui_wgpu::wgpu;

use super::geometry::{COLUMN_DIVIDER, COLUMN_INSET, Rects};
use super::theme;
use super::widgets::{self, CellStyle};

/// Shows the column card at `rects.column` with the contents that
/// `add_contents` adds from the top. The contents scroll if they are longer
/// than the card. The cells have the look of `CellStyle::Tool` down to the
/// divider, and of `CellStyle::Icon` below it. The name of a held tool cell
/// shows next to it.
pub fn show(
    ctx: &egui::Context,
    backdrop: Option<wgpu::TextureFormat>,
    rects: &Rects,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let radius = theme::CARD_RADIUS as u8;
    let radius = CornerRadius {
        nw: 0,
        ne: radius,
        sw: 0,
        se: radius,
    };
    let rect = rects.column;
    let scroll = rects.column_content > rect.height() + 0.5;
    widgets::card(
        ctx,
        backdrop,
        "pad-tools",
        rect,
        Order::Middle,
        radius,
        |ui| {
            let inner = rect.shrink(COLUMN_INSET);
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            ui.set_clip_rect(rect.shrink2(vec2(0.0, COLUMN_INSET)));
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            widgets::with_cell_style(&mut ui, CellStyle::Tool, |ui| {
                if scroll {
                    egui::ScrollArea::vertical()
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, add_contents);
                } else {
                    add_contents(ui);
                }
            });
        },
    );
    widgets::show_hold_label(ctx, rects.screen);
}

/// The line between the tools and the controls of the tool. The cells
/// below it have the look of `CellStyle::Icon`.
pub fn divider(ui: &mut egui::Ui) {
    widgets::set_cell_style(ui.ctx(), CellStyle::Icon);
    let size = vec2(ui.available_width(), COLUMN_DIVIDER);
    let (rule, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let line = Rect::from_center_size(rule.center(), vec2(rule.width() - 4.0, 1.0));
    ui.painter().rect_filled(line, 0.0, theme::divider());
}
