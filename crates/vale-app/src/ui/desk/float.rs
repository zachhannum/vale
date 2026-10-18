//! The material of each floating surface of the desktop layout: the
//! popovers, the menus, and the sheets. It is the glass of a popover, with
//! the blurred canvas behind it when the canvas has one.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Id, InnerResponse, Key, Layout, Margin, Modifiers,
    Order, Pos2, Rect, Response, Sense, Shape, Stroke, pos2, vec2,
};

use eframe::egui_wgpu::wgpu;

use super::theme;
use crate::globe::backdrop::card_shape;
use crate::ui::{AppState, Screen, Workspace};

pub const RADIUS: u8 = 9;
/// The height of a row of a menu.
pub const ITEM: f32 = 22.0;
/// The space between the edge of a menu and its rows.
const MENU_PAD: i8 = 5;
/// The column of the check mark at the left of a row of a menu.
const CHECK: f32 = 20.0;
/// The space between the text of a row and its key.
const KEY_GAP: f32 = 24.0;
/// The text on the accent color.
pub const ON_ACCENT: Color32 = Color32::from_rgb(11, 31, 26);
/// The width of a sheet.
pub const SHEET: f32 = 420.0;
const SHEET_PAD: i8 = 16;
/// The height of the title of a popover or a sheet.
const TITLE: f32 = 36.0;

/// What shows through a floating surface.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Glass {
    /// The format of the blurred canvas. `None`: no blurred canvas.
    pub backdrop: Option<wgpu::TextureFormat>,
    /// The glass of the system shows through.
    pub translucent: bool,
}

impl Glass {
    /// Only the globe canvas has a blurred copy.
    pub fn of(state: &AppState) -> Glass {
        let globe = state.screen == Screen::Editor && state.workspace == Workspace::Globe;
        Glass {
            backdrop: state.globe.backdrop().filter(|_| globe),
            translucent: state.translucent,
        }
    }

    pub fn fill(self) -> Color32 {
        theme::popover(self.backdrop.is_some() || self.translucent)
    }
}

pub fn shadow() -> egui::Shadow {
    egui::Shadow {
        offset: [0, 12],
        blur: 36,
        spread: 0,
        color: Color32::from_black_alpha(128),
    }
}

pub fn stroke() -> Stroke {
    Stroke::new(1.0, Color32::from_white_alpha(31))
}

/// Draws the contents with the material behind them.
pub fn material<R>(
    ui: &mut egui::Ui,
    glass: Glass,
    radius: CornerRadius,
    margin: Margin,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> InnerResponse<R> {
    let painter = ui.painter().clone();
    let back: Vec<_> = (0..3).map(|_| painter.add(Shape::Noop)).collect();
    let inner = egui::Frame::NONE
        .inner_margin(margin)
        .show(ui, add_contents);
    let rect = inner.response.rect;
    painter.set(back[0], shadow().as_shape(rect, radius));
    if let Some(format) = glass.backdrop {
        let pixels = ui.ctx().pixels_per_point();
        painter.set(back[1], card_shape(format, rect, radius, pixels));
    }
    let fill = egui::epaint::RectShape::new(
        rect,
        radius,
        glass.fill(),
        stroke(),
        egui::StrokeKind::Inside,
    );
    painter.set(back[2], fill);
    inner
}

/// A floating surface with its `pivot` at `at`.
#[allow(clippy::too_many_arguments)]
pub fn floating<R>(
    ctx: &egui::Context,
    id: Id,
    at: Pos2,
    pivot: Align2,
    glass: Glass,
    radius: CornerRadius,
    margin: Margin,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> InnerResponse<R> {
    egui::Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(at)
        .pivot(pivot)
        .constrain(false)
        .show(ctx, |ui| {
            material(ui, glass, radius, margin, add_contents).inner
        })
}

/// The rows of a menu, with the material behind them.
pub fn menu_body<R>(
    ui: &mut egui::Ui,
    glass: Glass,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let margin = Margin::same(MENU_PAD);
    let layout = Layout::top_down_justified(Align::Min);
    material(ui, glass, RADIUS.into(), margin, |ui| {
        ui.with_layout(layout, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            add_contents(ui)
        })
        .inner
    })
    .inner
}

/// A menu below a button. A click on the button opens and closes it, and a
/// click on a row closes it.
pub fn menu<R>(
    button: &Response,
    glass: Glass,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<InnerResponse<R>> {
    egui::Popup::menu(button)
        .frame(egui::Frame::NONE)
        .gap(4.0)
        .show(|ui| menu_body(ui, glass, add_contents))
}

/// A row of a menu.
#[derive(Copy, Clone, Debug)]
pub struct Item<'a> {
    pub label: &'a str,
    /// The key of the row, at the right.
    pub key: Option<&'a str>,
    /// `Some`: the row has a check mark when it is on.
    pub checked: Option<bool>,
    pub enabled: bool,
}

impl<'a> Item<'a> {
    pub fn new(label: &'a str) -> Item<'a> {
        Item {
            label,
            key: None,
            checked: None,
            enabled: true,
        }
    }

    pub fn checked(self, on: bool) -> Item<'a> {
        Item {
            checked: Some(on),
            ..self
        }
    }

    pub fn key(self, key: &'a str) -> Item<'a> {
        Item {
            key: Some(key),
            ..self
        }
    }

    pub fn enabled(self, enabled: bool) -> Item<'a> {
        Item { enabled, ..self }
    }
}

/// A row of a menu: a check mark column, the text, and the key. The row
/// under the pointer has the accent color.
pub fn item(ui: &mut egui::Ui, item: Item) -> Response {
    let font = theme::label();
    let painter = ui.painter();
    let text = painter.layout_no_wrap(item.label.to_owned(), font.clone(), theme::TEXT);
    let key = item
        .key
        .map(|key| painter.layout_no_wrap(key.to_owned(), font.clone(), theme::MUTE));
    let key_width = key.as_ref().map_or(0.0, |k| KEY_GAP + k.size().x);
    let width = (CHECK + text.size().x + key_width + 8.0).ceil();
    let width = match ui.layout().horizontal_justify() {
        true => width.max(ui.available_width()),
        false => width,
    };
    let sense = if item.enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ITEM), sense);
    resp.widget_info(|| match item.checked {
        Some(on) => {
            egui::WidgetInfo::selected(egui::WidgetType::Button, item.enabled, on, item.label)
        }
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, item.enabled, item.label),
    });
    let lit = item.enabled && resp.hovered();
    let (color, key_color) = if lit {
        (ON_ACCENT, ON_ACCENT)
    } else if item.enabled {
        (theme::TEXT, theme::MUTE)
    } else {
        let off = theme::MUTE.gamma_multiply(0.6);
        (off, off)
    };
    let painter = ui.painter();
    if lit {
        painter.rect_filled(rect, theme::CONTROL_RADIUS, theme::ACCENT);
    }
    let y = rect.center().y;
    if item.checked == Some(true) {
        let x = rect.left() + CHECK / 2.0;
        let mark = vec![
            pos2(x - 4.0, y),
            pos2(x - 1.0, y + 3.0),
            pos2(x + 4.5, y - 3.5),
        ];
        painter.add(Shape::line(mark, Stroke::new(1.6, color)));
    }
    let at = pos2(rect.left() + CHECK, y - text.size().y / 2.0);
    painter.galley(at, text, color);
    if let Some(key) = key {
        let at = pos2(rect.right() - 8.0 - key.size().x, y - key.size().y / 2.0);
        painter.galley(at, key, key_color);
    }
    resp
}

/// The line between two groups of rows of a menu.
pub fn separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    let line = Rect::from_min_max(
        pos2(rect.left() + 8.0, rect.center().y - 0.5),
        pos2(rect.right() - 8.0, rect.center().y + 0.5),
    );
    ui.painter()
        .rect_filled(line, 0.0, Color32::from_white_alpha(26));
}

/// The title of a popover or a sheet, with a line below it across the
/// surface. `pad` is the space at each side of the surface.
pub fn title(ui: &mut egui::Ui, text: &str, pad: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TITLE), Sense::hover());
    let painter = ui.painter();
    let at = pos2(rect.left(), rect.center().y);
    painter.text(at, Align2::LEFT_CENTER, text, theme::body(), theme::TEXT);
    let line = Rect::from_min_max(
        pos2(rect.left() - pad, rect.bottom() - 1.0),
        pos2(rect.right() + pad, rect.bottom()),
    );
    painter.rect_filled(line, 0.0, theme::line());
    super::title::label(ui, rect, "title", text);
}

/// The keys of a sheet in one frame.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SheetKeys {
    /// Escape.
    pub cancel: bool,
    /// Return.
    pub confirm: bool,
}

/// A sheet of the window: a card that hangs from the bottom edge of the
/// title bar, at the middle of the window. `top` is the bottom edge of the
/// title bar. The window behind the sheet is dim and takes no input.
pub fn sheet<R>(
    ctx: &egui::Context,
    glass: Glass,
    top: f32,
    id: &str,
    title_text: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, SheetKeys) {
    let screen = ctx.content_rect();
    let scrim = Id::new(id).with("scrim");
    egui::Area::new(scrim)
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            let below = Rect::from_min_max(pos2(rect.left(), top), rect.max);
            ui.painter()
                .rect_filled(below, 0.0, Color32::from_black_alpha(46));
        });
    let at = pos2(screen.center().x - SHEET / 2.0, top);
    let radius = CornerRadius {
        nw: 0,
        ne: 0,
        sw: RADIUS,
        se: RADIUS,
    };
    let margin = Margin {
        left: SHEET_PAD,
        right: SHEET_PAD,
        top: 0,
        bottom: 14,
    };
    let inner = floating(
        ctx,
        Id::new(id),
        at,
        Align2::LEFT_TOP,
        glass,
        radius,
        margin,
        |ui| {
            ui.set_width(SHEET - 2.0 * f32::from(SHEET_PAD));
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            title(ui, title_text, f32::from(SHEET_PAD));
            ui.add_space(8.0);
            add_contents(ui)
        },
    );
    ctx.move_to_top(egui::LayerId::new(Order::Foreground, Id::new(id)));
    let keys = ctx.input_mut(|i| SheetKeys {
        cancel: i.consume_key(Modifiers::NONE, Key::Escape),
        confirm: i.consume_key(Modifiers::NONE, Key::Enter),
    });
    (inner.inner, keys)
}

/// A button of a sheet.
#[derive(Copy, Clone, Debug)]
pub struct SheetButton<'a> {
    pub label: &'a str,
    pub enabled: bool,
    /// The button of the accent color, that Return clicks.
    pub primary: bool,
}

/// The buttons at the bottom right of a sheet, from left to right. Returns
/// the index of the button that the user clicked.
pub fn buttons(ui: &mut egui::Ui, buttons: &[SheetButton]) -> Option<usize> {
    ui.add_space(8.0);
    let layout = Layout::right_to_left(Align::Center);
    let width = ui.available_width();
    ui.allocate_ui_with_layout(vec2(width, 24.0), layout, |ui| {
        let mut chosen = None;
        for (index, button) in buttons.iter().enumerate().rev() {
            let resp = match button.primary {
                true => super::widgets::primary_button(ui, button.label, None, button.enabled),
                false => super::widgets::plain_button(ui, button.label, button.enabled),
            };
            if resp.clicked() {
                chosen = Some(index);
            }
        }
        chosen
    })
    .inner
}

/// True if the user pressed outside `rect` and outside the button that
/// opened it.
pub fn pressed_outside(ctx: &egui::Context, rect: Rect, button: Rect) -> bool {
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    pressed.is_some_and(|pos| !rect.contains(pos) && !button.contains(pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_glass_shows_through_only_with_a_blur_or_the_system_glass() {
        let glass = Glass {
            backdrop: None,
            translucent: false,
        };
        assert_eq!(glass.fill().a(), 255);
        let translucent = Glass {
            translucent: true,
            ..glass
        };
        assert!(translucent.fill().a() < 255);
    }
}
