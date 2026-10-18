//! The colors and the sizes of the desktop layout.

use eframe::egui::{self, Color32, FontId, Stroke, vec2};

use super::float;
pub use crate::ui::pad::theme::{ACCENT, TEXT, WARN};

/// The height of the row of the menus.
pub const MENU: f32 = 24.0;
/// The height of the title bar.
pub const BAR: f32 = 52.0;
pub const OPTIONS: f32 = 30.0;
pub const RAIL: f32 = 44.0;
pub const DOCK: f32 = 300.0;
pub const TABS: f32 = 34.0;
pub const ROW: f32 = 26.0;
/// The side of a tool button.
pub const TOOL: f32 = 28.0;
pub const ICON: f32 = 16.0;
pub const STATUS: f32 = 22.0;
/// The width that the window buttons of macOS take at the left of the
/// title bar.
pub const LIGHTS: f32 = 78.0;
pub const TOOL_RADIUS: u8 = 6;
pub const CONTROL_RADIUS: u8 = 5;

pub const MUTE: Color32 = Color32::from_rgb(163, 170, 179);
/// The text of a selected segment or tab.
pub const SELECTED_TEXT: Color32 = Color32::from_rgb(201, 245, 234);
/// The opacity of a control that does nothing now.
pub const OFF: f32 = 0.4;

/// The tint of a selected tool, segment, tab, or row.
pub fn selected() -> Color32 {
    Color32::from_rgba_unmultiplied(69, 196, 168, 51)
}

/// The edge of a selected segment.
pub fn selected_line() -> Color32 {
    Color32::from_rgba_unmultiplied(69, 196, 168, 89)
}

/// The fill of a control under the pointer.
pub fn hover() -> Color32 {
    Color32::from_white_alpha(15)
}

/// The lines between the places of the window.
pub fn line() -> Color32 {
    Color32::from_white_alpha(20)
}

/// The short vertical line between two groups of buttons.
pub fn divider() -> Color32 {
    Color32::from_white_alpha(36)
}

/// The fill behind a group of segments.
pub fn well() -> Color32 {
    Color32::from_black_alpha(64)
}

/// The fill of the title bar, the tool rail, and the dock. With
/// `translucent`, the glass of the system shows through it.
pub fn glass(translucent: bool) -> Color32 {
    if translucent {
        Color32::from_rgba_unmultiplied(30, 33, 38, 153)
    } else {
        Color32::from_rgb(30, 33, 38)
    }
}

/// The fill of the canvas: the glass with a darker tint.
pub fn canvas(translucent: bool) -> Color32 {
    if translucent {
        Color32::from_rgba_unmultiplied(10, 12, 15, 204)
    } else {
        Color32::from_rgb(14, 16, 19)
    }
}

/// The fill of a popover, a menu, and a tooltip. With `see_through`, the
/// glass of the system or a blurred copy of the canvas is behind it.
pub fn popover(see_through: bool) -> Color32 {
    if see_through {
        Color32::from_rgba_unmultiplied(40, 43, 49, 199)
    } else {
        Color32::from_rgb(40, 43, 49)
    }
}

pub fn body() -> FontId {
    FontId::proportional(13.0)
}

pub fn label() -> FontId {
    FontId::proportional(12.0)
}

pub fn small() -> FontId {
    FontId::proportional(11.0)
}

pub fn mono() -> FontId {
    FontId::monospace(12.0)
}

/// Sets the style of the desktop layout. It changes the style only when
/// `translucent` changes.
pub fn install(ctx: &egui::Context, translucent: bool) {
    let id = egui::Id::new("desk-theme");
    if ctx.data(|d| d.get_temp::<bool>(id)) == Some(translucent) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(id, translucent));
    ctx.all_styles_mut(|style| {
        style.visuals = egui::Visuals::dark();
        let visuals = &mut style.visuals;
        visuals.selection.bg_fill = selected();
        visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        visuals.hyperlink_color = ACCENT;
        visuals.warn_fg_color = WARN;
        visuals.panel_fill = glass(translucent);
        // The menus and the tooltips of egui have the look of the floating
        // surfaces, with no blur.
        visuals.window_fill = popover(translucent);
        visuals.window_stroke = float::stroke();
        visuals.window_corner_radius = float::RADIUS.into();
        visuals.menu_corner_radius = float::RADIUS.into();
        visuals.window_shadow = float::shadow();
        visuals.popup_shadow = float::shadow();
        let widgets = &mut visuals.widgets;
        for widget in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            widget.corner_radius = CONTROL_RADIUS.into();
        }
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, line());
        style.spacing.button_padding = vec2(9.0, 3.0);
        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.menu_margin = egui::Margin::same(5);
        for (text_style, font) in &mut style.text_styles {
            font.size = match text_style {
                egui::TextStyle::Heading => 15.0,
                egui::TextStyle::Small => 11.0,
                egui::TextStyle::Monospace => 12.0,
                _ => 13.0,
            };
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_translucent_fills_let_the_glass_through() {
        for fill in [glass, canvas, popover] {
            assert!(fill(true).a() < 255);
            assert_eq!(fill(false).a(), 255);
        }
    }

    #[test]
    fn the_canvas_is_darker_than_the_glass() {
        for translucent in [false, true] {
            let (canvas, glass) = (canvas(translucent), glass(translucent));
            assert!(canvas.r() < glass.r() && canvas.g() < glass.g() && canvas.b() < glass.b());
        }
    }
}
