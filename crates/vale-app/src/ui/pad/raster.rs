//! The Style section of the panel of a raster layer: the display and the
//! color stops.

use eframe::egui::{self, Align2, Order, Pos2, Rect, Sense, pos2, vec2};
use vale_store::{RasterDisplay, RasterId};
use vale_terrain::{Bands, Side};

use super::Panel;
use super::geometry::{PAD, Rects};
use super::theme;
use super::widgets::{self, Controls, PANEL_ROW};
use crate::ui::{AppState, elevation, gradient};
use crate::world;

/// The name of the segmented control of the displays.
pub const SHOW_AS: &str = "Show as";
/// The name of the field of the selected stop.
pub const STOP_VALUE: &str = "Stop value";
/// The names of the two sides of the stop at sea level.
pub const SEA_COLOR: &str = "Sea";
pub const LAND_COLOR: &str = "Land";
/// The name of the color picker of the selected stop.
pub const COLOR: &str = "Color";
pub const REMOVE_STOP: &str = "Remove the stop";
/// The name of the unit field.
pub const UNIT: &str = "Unit";
pub const RIVERS: &str = "Rivers";
pub const RIVER_WIDTH: &str = "River width";

/// The height of the heading of a section.
const HEADING: f32 = 40.0;
/// The width of the card of the selected stop.
const CARD_WIDTH: f32 = 300.0;
/// The space between the card of the selected stop and the panel.
const GAP: f32 = 14.0;
/// The size of the point of the card of the selected stop.
const POINT: f32 = 10.0;
/// The space around the contents of the card of the selected stop.
const CARD_PAD: f32 = 12.0;

/// The state of the Style section of the iPad layout.
#[derive(Default)]
pub struct StyleState {
    /// The text of the unit field while the user types.
    unit: Option<(RasterId, String)>,
    /// The text of the field of the selected stop while the user types.
    value: Option<(RasterId, usize, String)>,
    /// The side of the stop at sea level that the color picker edits.
    side: Option<(RasterId, usize, Side)>,
    /// The layer, the selected stop, its handle, and the gradient control
    /// in this frame.
    anchor: Option<(RasterId, usize, Pos2, Rect)>,
    /// The height of the contents of the card of the selected stop.
    height: Option<f32>,
    /// The card of the selected stop in the last frame.
    pub card: Option<Rect>,
}

/// The heading of a section of a panel.
pub fn heading(ui: &mut egui::Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADING), Sense::hover());
    let at = pos2(rect.left(), rect.bottom() - 8.0);
    ui.painter()
        .text(at, Align2::LEFT_BOTTOM, text, theme::title(), theme::TEXT);
}

/// The unit row. A height layer has no unit row, because its unit cannot
/// change, and the row of its values shows the unit.
pub fn unit_row(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: RasterId) {
    let Some(raster) = state.doc.project.raster(id) else {
        return;
    };
    let unit = raster.unit.clone();
    if crate::ui::is_height(raster) {
        return;
    }
    let mut text = match state.pad.style.unit.take() {
        Some((of, text)) if of == id => text,
        _ => unit.clone(),
    };
    let valid = text.trim() != crate::ui::HEIGHT_UNIT;
    let field = widgets::text_field(ui, controls, UNIT, UNIT, None, &mut text, valid);
    if field.changed() {
        state.set_raster_unit(id, &text);
    } else if !field.has_focus() {
        text = unit;
    }
    state.pad.style.unit = Some((id, text));
}

/// The Style section: the display and the color stops, and then the
/// rivers of a height layer. The selected stop has a card next to the
/// panel, which `stop_card` shows.
pub fn style(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: RasterId) {
    let Some(raster) = state.doc.project.raster(id) else {
        return;
    };
    let (display, unit) = (raster.display, raster.unit.clone());
    let height = crate::ui::is_height(raster);

    heading(ui, "Style");
    let active = RasterDisplay::ALL.iter().position(|d| *d == display);
    let choice = widgets::choice_row(ui, controls, SHOW_AS, &elevation::DISPLAYS, active);
    if let Some(index) = choice {
        state.set_raster_display(id, RasterDisplay::ALL[index]);
    }
    let display = state.doc.project.raster(id).map_or(display, |r| r.display);

    let style = &mut state.pad.style;
    let globe = &mut state.globe;
    let mut selected = elevation::selected(&globe.preview, Some(id));
    let Some(bands) = globe.bands_mut(id) else {
        return;
    };
    let before = selected;
    ui.add_space(8.0);
    let m = &gradient::PAD;
    let resp = ui
        .scope(|ui| {
            ui.visuals_mut().override_text_color = Some(theme::MUTE);
            gradient::editor(ui, Some(controls), m, bands, &mut selected, display, &unit)
        })
        .inner;
    if selected != before {
        style.value = None;
        style.side = None;
    }
    // The card hides while a finger drags a stop, so that the globe shows.
    style.anchor = selected.filter(|_| !resp.dragged()).and_then(|index| {
        let x = gradient::stop_x(resp.rect, m, bands, bands.stops()[index].value);
        let at = pos2(x, gradient::handle_row(resp.rect, m).center().y);
        ui.clip_rect()
            .contains(at)
            .then_some((id, index, at, resp.rect))
    });
    note(ui, gradient::HINT);

    let preview = &mut state.globe.preview;
    preview.selected = selected.map(|index| (id, index));
    if height {
        widgets::switch_row(ui, controls, RIVERS, RIVERS, &mut preview.rivers);
        let range = elevation::RIVER_WIDTH;
        let (low, high) = (f64::from(*range.start()), f64::from(*range.end()));
        let mut fraction = (f64::from(preview.river_width) - low) / (high - low);
        let text = format!("{:.2}×", preview.river_width);
        if widgets::row_slider(ui, controls, RIVER_WIDTH, "Width", &mut fraction, &text) {
            preview.river_width = (low + fraction * (high - low)) as f32;
        }
    }
}

/// A small muted text across the panel.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    let text = egui::RichText::new(text)
        .font(theme::small())
        .color(theme::MUTE);
    ui.add(egui::Label::new(text).wrap());
    ui.add_space(6.0);
}

/// The card of the selected stop: its value, its color, and the button
/// that removes it. The card is at the left of the panel and points at the
/// handle of the stop. Above a sheet, the card is above the gradient. A
/// touch outside the card and the gradient control closes the card. The
/// card hides during a drag on the gradient control.
pub fn stop_card(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    state.pad.style.card = None;
    let Some((id, index, at, gradient)) = state.pad.style.anchor.take() else {
        return;
    };
    let panel = state.pad.panels.iter().find(|(p, _)| *p == Panel::Layers);
    let Some(&(_, panel)) = panel else {
        return;
    };
    let Some(unit) = state.doc.project.raster(id).map(|r| r.unit.clone()) else {
        return;
    };
    let sea = state
        .globe
        .bands_mut(id)
        .is_some_and(|b| b.sea() == Some(index));
    let fixed = state.globe.bands_mut(id).is_some_and(|b| b.fixed(index));
    let guess = PANEL_ROW * if fixed { 4.0 } else { 5.0 } + 196.0 + 16.0;
    let height = state.pad.style.height.unwrap_or(guess) + 2.0 * CARD_PAD;
    let size = vec2(CARD_WIDTH, height);

    let screen = rects.screen;
    let top = screen.top() + rects.bands.top + PAD;
    let bottom = screen.bottom() - rects.bands.bottom - PAD;
    let room = panel.left() - GAP - CARD_WIDTH - PAD;
    let (rect, point) = if rects.sheet.is_none() && room >= screen.left() {
        let y = (at.y - height / 2.0).clamp(top, (bottom - height).max(top));
        let rect = Rect::from_min_size(pos2(panel.left() - GAP - CARD_WIDTH, y), size);
        let y =
            at.y.clamp(rect.top() + 2.0 * POINT, rect.bottom() - 2.0 * POINT);
        let base = rect.right();
        let point = [
            pos2(base, y - POINT),
            pos2(base + POINT, y),
            pos2(base, y + POINT),
        ];
        (rect, point)
    } else {
        let left = screen.left() + rects.bands.left + PAD;
        let right = screen.right() - rects.bands.right - PAD;
        let x = (at.x - CARD_WIDTH / 2.0).clamp(left, (right - CARD_WIDTH).max(left));
        let above = gradient.top() - GAP - height;
        let x_point = at.x.clamp(x + 2.0 * POINT, x + CARD_WIDTH - 2.0 * POINT);
        if above >= top {
            let rect = Rect::from_min_size(pos2(x, above), size);
            let base = rect.bottom();
            let point = [
                pos2(x_point - POINT, base),
                pos2(x_point, base + POINT),
                pos2(x_point + POINT, base),
            ];
            (rect, point)
        } else {
            let rect = Rect::from_min_size(pos2(x, gradient.bottom() + GAP), size);
            let base = rect.top();
            let point = [
                pos2(x_point + POINT, base),
                pos2(x_point, base - POINT),
                pos2(x_point - POINT, base),
            ];
            (rect, point)
        }
    };
    state.pad.style.card = Some(rect);

    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    if pressed.is_some_and(|pos| !rect.contains(pos) && !gradient.contains(pos)) {
        state.globe.preview.selected = None;
        state.pad.style.value = None;
        state.pad.style.side = None;
        state.pad.style.card = None;
        return;
    }

    let style = &mut state.pad.style;
    let Some(bands) = state.globe.bands_mut(id) else {
        return;
    };
    let mut removed = false;
    let radius = theme::CARD_RADIUS.into();
    let key = "pad-stop";
    widgets::card(ctx, None, key, rect, Order::Foreground, radius, |ui| {
        // The card covers the globe and other cards, so its fill is opaque.
        let fill = theme::card(false).to_opaque();
        ui.painter().rect_filled(rect, radius, fill);
        let inner = rect.shrink(CARD_PAD);
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        let ui = &mut ui;
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let start = ui.cursor().top();

        if fixed {
            let span = bands.max() - bands.min();
            let value = bands.stops()[index].value;
            widgets::value_row(ui, "Value", &world::value_text(value, span, &unit));
        } else {
            value_field(ui, controls, bands, &mut style.value, id, index, &unit);
        }
        let side = match style.side {
            Some((of, at, side)) if sea && of == id && at == index => side,
            _ => Side::Above,
        };
        let side = if sea {
            let names = [SEA_COLOR, LAND_COLOR];
            let active = Some(usize::from(side == Side::Above));
            match widgets::choice_row(ui, controls, COLOR, &names, active) {
                Some(0) => Side::Below,
                Some(_) => Side::Above,
                None => side,
            }
        } else {
            Side::Above
        };
        style.side = Some((id, index, side));
        ui.add_space(8.0);
        let stop = bands.stops()[index];
        let mut picked = match side {
            Side::Below => stop.below,
            Side::Above => stop.above,
        };
        if widgets::color_picker(ui, controls, COLOR, &mut picked) {
            bands.set_color(index, side, picked);
        }
        if !fixed {
            let (row, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
            let resp = widgets::tap_area(ui, controls, row, REMOVE_STOP, true);
            if resp.is_pointer_button_down_on() {
                let back = row.shrink(widgets::HIGHLIGHT_INSET);
                ui.painter()
                    .rect_filled(back, theme::CONTROL_RADIUS, theme::raise());
            }
            let at = pos2(row.left(), row.center().y);
            let text = "Remove stop";
            ui.painter()
                .text(at, Align2::LEFT_CENTER, text, theme::body(), theme::WARN);
            removed = resp.clicked() && bands.remove(index);
        }
        style.height = Some(ui.cursor().top() - start);
    });
    ctx.layer_painter(egui::LayerId::new(Order::Foreground, egui::Id::new(key)))
        .add(egui::Shape::convex_polygon(
            point.to_vec(),
            theme::card(false).to_opaque(),
            egui::Stroke::NONE,
        ));
    if removed {
        state.globe.preview.selected = None;
        state.pad.style.value = None;
        state.pad.style.side = None;
    }
}

/// The field of the value of the selected stop. The stop moves when the
/// text is a number.
fn value_field(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    bands: &mut Bands,
    text: &mut Option<(RasterId, usize, String)>,
    id: RasterId,
    index: usize,
    unit: &str,
) {
    let span = bands.max() - bands.min();
    let value = bands.stops()[index].value;
    let shown = world::value_text(value, span, "");
    let mut edit = match text.take() {
        Some((of, at, edit)) if of == id && at == index => edit,
        _ => shown.clone(),
    };
    let parsed = parse_value(&edit);
    let resp = widgets::value_field(
        ui,
        controls,
        STOP_VALUE,
        "Value",
        unit,
        &mut edit,
        parsed.is_some(),
    );
    if resp.changed()
        && let Some(value) = parse_value(&edit)
    {
        bands.move_to(index, value);
    } else if !resp.has_focus() {
        edit = shown;
    }
    *text = Some((id, index, edit));
}

/// A number as the user types it, with or without the separators of the
/// thousands, and with a hyphen or a minus sign.
pub fn parse_value(text: &str) -> Option<f64> {
    let clean: String = text
        .trim()
        .chars()
        .filter(|c| *c != ',' && *c != ' ')
        .map(|c| if c == '\u{2212}' { '-' } else { c })
        .collect();
    clean.parse().ok().filter(|v: &f64| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_value_can_have_separators_and_a_minus_sign() {
        assert_eq!(parse_value("1,800"), Some(1800.0));
        assert_eq!(parse_value(" \u{2212}1,000 "), Some(-1000.0));
        assert_eq!(parse_value("-3.5"), Some(-3.5));
        assert_eq!(parse_value("abc"), None);
        assert_eq!(parse_value("inf"), None);
    }

    #[test]
    fn a_hex_code_gives_its_color() {
        use super::widgets::{hex_text, parse_hex};
        assert_eq!(parse_hex("#1A2b3C"), Some([0x1a, 0x2b, 0x3c]));
        assert_eq!(parse_hex(" ff0000 "), Some([255, 0, 0]));
        assert_eq!(parse_hex("ff00"), None);
        assert_eq!(parse_hex("gg0000"), None);
        assert_eq!(parse_hex("ÿÿÿ"), None);
        assert_eq!(hex_text([1, 171, 255]), "01ABFF");
    }
}
