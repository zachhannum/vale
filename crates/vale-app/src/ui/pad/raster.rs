//! The Style section of the panel of a raster layer: the display and the
//! color stops.

use eframe::egui::{self, Align2, Order, Pos2, Rect, Sense, pos2, vec2};
use vale_store::{RasterDisplay, RasterId};
use vale_terrain::{Bands, Side, StopLine};

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
/// The names of the colors of a stop that the color picker edits: the two
/// sides of the stop at sea level, the one color of each other stop, and the
/// line.
pub const SEA_COLOR: &str = "Sea";
pub const LAND_COLOR: &str = "Land";
pub const FILL_COLOR: &str = "Fill";
pub const LINE_COLOR: &str = "Line";
/// The name of the color picker of the selected stop.
pub const COLOR: &str = "Color";
/// The switch, the weight, and the color source of the line of the selected
/// stop, and the amount of the darker color.
pub const LINE: &str = "Stop line";
pub const LINE_WEIGHT: &str = "Line weight";
pub const LINE_SOURCE: &str = "Line color";
pub const DARKER: &str = "Darker";
pub const CUSTOM: &str = "Custom";
pub const DARKEN: &str = "Line darken";
/// The step of the weight slider, in points.
const WEIGHT_STEP: f32 = 0.25;
pub const REMOVE_STOP: &str = "Remove the stop";
/// The row that puts back the default color stops, and the button of the
/// card that asks before it.
pub const RESET_STOPS: &str = "Reset colors";
pub const RESET: &str = "Reset";
const RESET_TEXT: &str = "Put back the default color stops of this layer? You cannot undo this.";
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
/// The time, in seconds, that the card stays up after the keyboard goes.
const KEYBOARD_HOLD: f64 = 0.4;
/// The space around the contents of the card of the selected stop.
const CARD_PAD: f32 = 12.0;

/// The state of the Style section of the iPad layout.
#[derive(Default)]
pub struct StyleState {
    /// The text of the unit field while the user types.
    unit: Option<(RasterId, String)>,
    /// The text of the field of the selected stop while the user types.
    value: Option<(RasterId, usize, String)>,
    /// The color of the selected stop that the color picker edits.
    side: Option<(RasterId, usize, Paint)>,
    /// The layer, the selected stop, its handle, and the gradient control
    /// in this frame.
    anchor: Option<(RasterId, usize, Pos2, Rect)>,
    /// The height of the contents of the card of the selected stop.
    height: Option<f32>,
    /// The card of the selected stop in the last frame.
    pub card: Option<Rect>,
    /// How far the card of the selected stop went up for the keyboard in the
    /// last frame.
    lift: f32,
    /// The last height of the keyboard that was not 0, and the time of it.
    keyboard: (f32, f64),
    /// The layer whose card that asks before a reset is open.
    reset: Option<RasterId>,
    /// The layer and the reset row in this frame, while the card is open.
    reset_anchor: Option<(RasterId, Rect)>,
    /// The card that asks before a reset in the last frame.
    pub reset_card: Option<Rect>,
}

/// A color of a stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Paint {
    Fill(Side),
    Line,
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
    let default = Bands::with_range(bands.min(), bands.max(), height);
    let changed = default.as_ref().is_some_and(|d| d.stops() != bands.stops());
    let row = widgets::text_row(ui, controls, RESET_STOPS, 0.0, false, changed);
    if row.clicked() {
        style.reset = (style.reset != Some(id)).then_some(id);
    }
    if !changed {
        style.reset = None;
    }
    style.reset_anchor = (style.reset == Some(id)).then_some((id, row.rect));

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
    let guess = PANEL_ROW * if fixed { 7.0 } else { 8.0 } + 196.0 + 16.0;
    let height = state.pad.style.height.unwrap_or(guess) + 2.0 * CARD_PAD;

    let (rect, point) = place(rects, panel, at, gradient, height);
    let fits = state
        .pad
        .style
        .height
        .map(|_| rect.height() >= height - 0.5);
    let top = rects.screen.top() + rects.bands.top + PAD;
    // A keyboard that goes away and comes back at once does not move the
    // card.
    let now = ctx.input(|i| i.time);
    let mut keyboard = state.pad.keyboard;
    if keyboard > 0.0 {
        state.pad.style.keyboard = (keyboard, now);
    } else if now - state.pad.style.keyboard.1 < KEYBOARD_HOLD {
        keyboard = state.pad.style.keyboard.0;
        ctx.request_repaint_after_secs(KEYBOARD_HOLD as f32);
    }
    let lift = state.pad.style.lift;
    let lift = widgets::keyboard_lift(ctx, rect, lift, keyboard, rects.screen, top);
    state.pad.style.lift = lift;
    // A card that went up for the keyboard does not point at the handle.
    let rect = rect.translate(vec2(0.0, -lift));
    state.pad.style.card = Some(rect);

    if pressed_outside(ctx, rect, gradient) {
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
        // A card that is taller than the room on the screen scrolls. A card
        // that fits has no scroll bar over its controls.
        let scroll = egui::ScrollArea::vertical()
            .id_salt("pad-stop-scroll")
            .vscroll(fits.is_none_or(|fits| !fits))
            .max_height(inner.height())
            .auto_shrink([false, true]);
        scroll.show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let start = ui.cursor().top();

            if fixed {
                let span = bands.max() - bands.min();
                let value = bands.stops()[index].value;
                widgets::value_row(ui, "Value", &world::value_text(value, span, &unit));
            } else {
                value_field(ui, controls, bands, &mut style.value, id, index, &unit);
            }
            let stop = bands.stops()[index];
            let mut line = stop.line;
            let mut changed = widgets::switch_row(ui, controls, LINE, "Line", &mut line.on);
            let (lo, hi) = (*StopLine::WEIGHTS.start(), *StopLine::WEIGHTS.end());
            let mut fraction = f64::from((line.weight - lo) / (hi - lo));
            let text = format!("{} pt", weight_text(line.weight));
            if widgets::row_slider(ui, controls, LINE_WEIGHT, "Weight", &mut fraction, &text) {
                let weight = lo + fraction as f32 * (hi - lo);
                line.weight = (weight / WEIGHT_STEP).round() * WEIGHT_STEP;
                changed = true;
            }
            let active = Some(usize::from(!line.darker));
            if let Some(choice) =
                widgets::choice_row(ui, controls, LINE_SOURCE, &[DARKER, CUSTOM], active)
            {
                line.darker = choice == 0;
                changed = true;
            }
            if line.darker {
                let mut fraction = f64::from(line.darken);
                let text = format!("{:.0}%", line.darken * 100.0);
                if widgets::row_slider(ui, controls, DARKEN, "Darken", &mut fraction, &text) {
                    line.darken = ((fraction * 20.0).round() / 20.0) as f32;
                    changed = true;
                }
            }
            let paint = match style.side {
                Some((of, at, paint)) if of == id && at == index => paint,
                _ => Paint::Fill(Side::Above),
            };
            // A darker line has no color of its own to pick.
            let (names, paints): (&[&str], &[Paint]) = match (sea, line.darker) {
                (true, false) => (
                    &[SEA_COLOR, LAND_COLOR, LINE_COLOR],
                    &[
                        Paint::Fill(Side::Below),
                        Paint::Fill(Side::Above),
                        Paint::Line,
                    ],
                ),
                (true, true) => (
                    &[SEA_COLOR, LAND_COLOR],
                    &[Paint::Fill(Side::Below), Paint::Fill(Side::Above)],
                ),
                (false, false) => (
                    &[FILL_COLOR, LINE_COLOR],
                    &[Paint::Fill(Side::Above), Paint::Line],
                ),
                (false, true) => (&[FILL_COLOR], &[Paint::Fill(Side::Above)]),
            };
            let paint = match paint {
                Paint::Fill(_) if !sea => Paint::Fill(Side::Above),
                Paint::Line if line.darker => Paint::Fill(Side::Above),
                paint => paint,
            };
            let active = paints.iter().position(|p| *p == paint);
            let paint = match widgets::choice_row(ui, controls, COLOR, names, active) {
                Some(i) => paints[i],
                None => paint,
            };
            style.side = Some((id, index, paint));
            ui.add_space(8.0);
            let mut picked = match paint {
                Paint::Fill(Side::Below) => stop.below,
                Paint::Fill(Side::Above) => stop.above,
                Paint::Line => line.color,
            };
            if widgets::color_picker(ui, controls, COLOR, &mut picked) {
                match paint {
                    Paint::Fill(side) => {
                        bands.set_color(index, side, picked);
                    }
                    Paint::Line => {
                        line.color = picked;
                        changed = true;
                    }
                }
            }
            if changed {
                bands.set_line(index, line);
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
    });
    if lift == 0.0 {
        paint_point(ctx, key, point);
    }
    if removed {
        state.globe.preview.selected = None;
        state.pad.style.value = None;
        state.pad.style.side = None;
    }
}

/// A weight in points with no zeros at the end, for example `0.25` or `2`.
fn weight_text(weight: f32) -> String {
    let text = format!("{weight:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The place of a card next to the panel, and the point of the card. The
/// card is at the left of the panel and points at `at`. Above a sheet, or
/// with no room at the left, the card is above `avoid`, or below it if no
/// room is above.
fn place(rects: &Rects, panel: Rect, at: Pos2, avoid: Rect, height: f32) -> (Rect, [Pos2; 3]) {
    let screen = rects.screen;
    let top = screen.top() + rects.bands.top + PAD;
    let bottom = screen.bottom() - rects.bands.bottom - PAD;
    let room = panel.left() - GAP - CARD_WIDTH - PAD;
    if rects.sheet.is_none() && room >= screen.left() {
        let height = height.min(bottom - top);
        let size = vec2(CARD_WIDTH, height);
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
        return (rect, point);
    }
    let left = screen.left() + rects.bands.left + PAD;
    let right = screen.right() - rects.bands.right - PAD;
    let x = (at.x - CARD_WIDTH / 2.0).clamp(left, (right - CARD_WIDTH).max(left));
    // A card that fits nowhere goes where it has the most room, and it
    // scrolls.
    let (room_above, room_below) = (avoid.top() - GAP - top, bottom - avoid.bottom() - GAP);
    let up = height <= room_above || room_above >= room_below;
    let height = height
        .min(if up { room_above } else { room_below })
        .max(0.0);
    let size = vec2(CARD_WIDTH, height);
    let above = avoid.top() - GAP - height;
    let x_point = at.x.clamp(x + 2.0 * POINT, x + CARD_WIDTH - 2.0 * POINT);
    if up {
        let rect = Rect::from_min_size(pos2(x, above), size);
        let base = rect.bottom();
        let point = [
            pos2(x_point - POINT, base),
            pos2(x_point, base + POINT),
            pos2(x_point + POINT, base),
        ];
        (rect, point)
    } else {
        let rect = Rect::from_min_size(pos2(x, avoid.bottom() + GAP), size);
        let base = rect.top();
        let point = [
            pos2(x_point + POINT, base),
            pos2(x_point, base - POINT),
            pos2(x_point - POINT, base),
        ];
        (rect, point)
    }
}

/// A press in this frame outside `rect` and outside `keep`.
fn pressed_outside(ctx: &egui::Context, rect: Rect, keep: Rect) -> bool {
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    pressed.is_some_and(|pos| !rect.contains(pos) && !keep.contains(pos))
}

/// Paints the point of a card on the layer of the card.
fn paint_point(ctx: &egui::Context, key: &str, point: [Pos2; 3]) {
    ctx.layer_painter(egui::LayerId::new(Order::Foreground, egui::Id::new(key)))
        .add(egui::Shape::convex_polygon(
            point.to_vec(),
            theme::card(false).to_opaque(),
            egui::Stroke::NONE,
        ));
}

/// The card that asks before the reset of the color stops. It points at
/// the reset row. A touch outside the card and the row closes it.
pub fn reset_card(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    state.pad.style.reset_card = None;
    let Some((id, row)) = state.pad.style.reset_anchor.take() else {
        state.pad.style.reset = None;
        return;
    };
    let panel = state.pad.panels.iter().find(|(p, _)| *p == Panel::Layers);
    let Some(&(_, panel)) = panel else {
        return;
    };
    let height = state
        .doc
        .project
        .raster(id)
        .is_some_and(crate::ui::is_height);
    let font = theme::body();
    let wrap = CARD_WIDTH - 2.0 * CARD_PAD;
    let text = ctx.fonts_mut(|f| f.layout(RESET_TEXT.to_owned(), font, theme::MUTE, wrap));
    let size = text.size().y + 12.0 + PANEL_ROW + 2.0 * CARD_PAD;
    let at = pos2(row.left() + 40.0, row.center().y);
    let (rect, point) = place(rects, panel, at, row, size);
    if pressed_outside(ctx, rect, row) {
        state.pad.style.reset = None;
        return;
    }
    state.pad.style.reset_card = Some(rect);
    let mut reset = false;
    let radius = theme::CARD_RADIUS.into();
    let key = "pad-reset-stops";
    widgets::card(ctx, None, key, rect, Order::Foreground, radius, |ui| {
        ui.painter()
            .rect_filled(rect, radius, theme::card(false).to_opaque());
        let inner = rect.shrink(CARD_PAD);
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        let ui = &mut ui;
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let (place, _) = ui.allocate_exact_size(text.size(), Sense::hover());
        ui.painter().galley(place.min, text, theme::MUTE);
        ui.add_space(12.0);
        reset = widgets::button_row_in(ui, controls, RESET, true, theme::WARN).clicked();
    });
    paint_point(ctx, key, point);
    if !reset {
        return;
    }
    state.pad.style.reset = None;
    if let Some(bands) = state.globe.bands_mut(id)
        && let Some(default) = Bands::with_range(bands.min(), bands.max(), height)
    {
        *bands = default;
    }
    state.globe.preview.selected = None;
    state.pad.style.value = None;
    state.pad.style.side = None;
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
