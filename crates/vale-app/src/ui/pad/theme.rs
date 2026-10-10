//! The colors and the text sizes of the iPad layout.

use eframe::egui::{self, Color32, FontId};

use super::geometry::TOUCH;

pub const CANVAS: Color32 = Color32::from_rgb(15, 17, 19);
pub const TEXT: Color32 = Color32::from_rgb(231, 233, 236);
pub const MUTE: Color32 = Color32::from_rgb(174, 181, 190);
/// The color of a text that warns.
pub const WARN: Color32 = Color32::from_rgb(242, 184, 92);
pub const ACCENT: Color32 = Color32::from_rgb(69, 196, 168);
pub const CARD_RADIUS: f32 = 14.0;
pub const SHEET_RADIUS: f32 = 18.0;
pub const CONTROL_RADIUS: f32 = 10.0;
/// The opacity of a control that does nothing yet.
pub const OFF: f32 = 0.35;

/// The fill of a card. `blur`: the card shows the blurred canvas below the
/// fill. A card with no blur is less clear.
pub fn card(blur: bool) -> Color32 {
    let alpha = if blur { 184 } else { 228 };
    Color32::from_rgba_unmultiplied(26, 29, 33, alpha)
}

/// The fill of a selected control.
pub fn raise() -> Color32 {
    Color32::from_white_alpha(33)
}

pub fn track() -> Color32 {
    Color32::from_black_alpha(102)
}

pub fn divider() -> Color32 {
    Color32::from_white_alpha(41)
}

pub fn shadow() -> egui::Shadow {
    egui::Shadow {
        offset: [0, 4],
        blur: 20,
        spread: 0,
        color: Color32::from_black_alpha(89),
    }
}

pub fn body() -> FontId {
    FontId::proportional(15.0)
}

pub fn title() -> FontId {
    FontId::proportional(16.0)
}

pub fn small() -> FontId {
    FontId::proportional(10.0)
}

pub fn mono() -> FontId {
    FontId::monospace(12.0)
}

/// Sets the style of the egui widgets in a panel.
pub fn apply(ui: &mut egui::Ui) {
    let style = ui.style_mut();
    style.visuals = egui::Visuals::dark();
    style.spacing.interact_size = egui::vec2(TOUCH, TOUCH);
    style.spacing.button_padding = egui::vec2(14.0, 12.0);
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.icon_width = 24.0;
    style.spacing.slider_width = 140.0;
    style.visuals.override_text_color = Some(TEXT);
    for (text_style, font) in &mut style.text_styles {
        font.size = match text_style {
            egui::TextStyle::Heading => 16.0,
            egui::TextStyle::Small => 12.0,
            egui::TextStyle::Monospace => 12.0,
            _ => 15.0,
        };
    }
}
