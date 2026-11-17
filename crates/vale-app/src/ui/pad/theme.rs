//! The colors and the text sizes of the iPad layout.

use eframe::egui::{self, Color32, FontId};

use super::geometry::TOUCH;

pub const CANVAS: Color32 = Color32::from_rgb(15, 17, 19);
/// The globe below the raster layers of the world.
pub const GROUND: Color32 = Color32::from_rgb(64, 70, 79);
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

/// The fill of a menu that opens over a card. It is opaque and lighter
/// than the card.
pub fn menu() -> Color32 {
    card(false).to_opaque().blend(raise())
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

thread_local! {
    static TEXT_SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

/// The smallest text size in points.
const MIN_TEXT: f32 = 9.0;

/// Sets the factor of the text size setting of the system. The UI reads it
/// on the thread that draws.
pub fn set_text_scale(scale: f32) {
    TEXT_SCALE.with(|s| s.set(scale));
}

pub fn text_scale() -> f32 {
    TEXT_SCALE.with(std::cell::Cell::get)
}

/// A text size in points at the text size setting, to half a point.
pub fn text(size: f32) -> f32 {
    ((size * text_scale() * 2.0).round() / 2.0).max(MIN_TEXT.min(size))
}

/// A proportional font of `size` points at the text size setting.
pub fn font(size: f32) -> FontId {
    FontId::proportional(text(size))
}

/// A monospace font of `size` points at the text size setting.
pub fn mono_font(size: f32) -> FontId {
    FontId::monospace(text(size))
}

pub fn body() -> FontId {
    font(15.0)
}

pub fn title() -> FontId {
    font(16.0)
}

/// The font of a hint next to a name.
pub fn hint() -> FontId {
    font(13.0)
}

pub fn small() -> FontId {
    font(10.0)
}

pub fn mono() -> FontId {
    mono_font(12.0)
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
        font.size = text(match text_style {
            egui::TextStyle::Heading => 16.0,
            egui::TextStyle::Small => 12.0,
            egui::TextStyle::Monospace => 12.0,
            _ => 15.0,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_text_stays_readable_at_the_smallest_size() {
        set_text_scale(14.0 / 17.0);
        assert_eq!(small().size, 9.0);
        assert_eq!(body().size, 12.5);
        set_text_scale(1.0);
        assert_eq!(small().size, 10.0);
    }
}
