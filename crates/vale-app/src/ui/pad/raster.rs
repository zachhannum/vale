//! The Style section of the panel of a raster layer: the display and the
//! color stops.

use eframe::egui::{self, Align2, Order, Pos2, Rect, Sense, Vec2, pos2, vec2};
use vale_store::{RasterDisplay, RasterId, Smoothing};
use vale_terrain::{Bands, MAX_BANDS, Part, StopLine};

use super::geometry::{PAD, Rects};
use super::theme;
use super::widgets::{self, Controls, PANEL_ROW};
use super::{Page, Panel};
use crate::ui::{Action, AppState, elevation, gradient};
use crate::world;

/// The name of the segmented control of the displays.
pub const SHOW_AS: &str = "Show as";
/// The name of the field of the value of the lower edge of the selected
/// band.
pub const STOP_VALUE: &str = "Stop value";
/// The names of the colors that the color picker edits: the band, and the
/// line at its lower edge.
pub const FILL_COLOR: &str = "Fill";
pub const LINE_COLOR: &str = "Line";
/// The name of the color picker of the selected band.
pub const COLOR: &str = "Color";
/// The switch, the weight, and the color source of the line at the lower
/// edge of the selected band, and the amount of the darker color.
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
/// The row of the Style section that opens the page of the color ramps, and
/// the title of the page.
pub const COLOR_RAMPS: &str = "Color ramps";
/// The row of the Style section that opens the page of the color stops,
/// and the title of the page.
pub const COLOR_STOPS: &str = "Color stops";
/// The text of the page of the color stops when the layer has the most
/// bands.
pub const FULL: &str = "Maximum of 32 bands reached.";
pub const IMPORT_RAMP: &str = "Import ramp…";
/// The text below the import row while the app has no ramps.
pub const IMPORT_RAMP_HINT: &str = "A row of color blocks, lowest band first.";
/// The buttons of the card of a ramp.
pub const APPLY_RAMP: &str = "Apply";
pub const DELETE_RAMP: &str = "Delete";
pub const REVERSE_RAMP: &str = "Reverse";
/// The longest name of a ramp row, in characters.
const RAMP_NAME: usize = 20;
/// The width and the height of the bar of a ramp row.
const RAMP_BAR: Vec2 = vec2(96.0, 14.0);
pub const RESET: &str = "Reset";
const RESET_TEXT: &str = "Put back the default color stops of this layer? You cannot undo this.";
/// The name of the unit field.
pub const UNIT: &str = "Unit";
/// The switch of the smoothing and the slider of the minimum feature size.
pub const SMOOTHING: &str = "Smoothing";
pub const FEATURE_SIZE: &str = "Feature size";
/// The switch of the hillshade, the slider of its opacity, and the switch of
/// its highlights.
pub const HILLSHADE: &str = "Hillshade";
pub const HILLSHADE_OPACITY: &str = "Hillshade opacity";
pub const HILLSHADE_HIGHLIGHTS: &str = "Hillshade highlights";
pub const RIVERS: &str = "Rivers";
pub const RIVER_WIDTH: &str = "River width";

/// The height of the heading of a section.
const HEADING: f32 = 40.0;
/// The width of the card of the selected band.
const CARD_WIDTH: f32 = 300.0;
/// The space between the card of the selected band and the panel.
const GAP: f32 = 14.0;
/// The size of the point of the card of the selected band.
const POINT: f32 = 10.0;
/// The space around the contents of the card of the selected band.
const CARD_PAD: f32 = 12.0;

/// The state of the Style section of the iPad layout.
#[derive(Default)]
pub struct StyleState {
    /// The text of the unit field while the user types.
    unit: Option<(RasterId, String)>,
    /// The layer and the band of the card in the last frame.
    stop: Option<(RasterId, usize)>,
    /// The color of the selected band that the color picker edits.
    side: Option<(RasterId, usize, Paint)>,
    /// The layer, the selected band, the left of its row, and the gradient
    /// control in this frame.
    anchor: Option<(RasterId, usize, Pos2, Rect)>,
    /// The height of the contents of the card of the selected band.
    height: Option<f32>,
    /// The card of the selected band in the last frame.
    pub card: Option<Rect>,
    /// The layer whose card that asks before a reset is open.
    reset: Option<RasterId>,
    /// The layer and the reset row in this frame, while the card is open.
    reset_anchor: Option<(RasterId, Rect)>,
    /// The card that asks before a reset in the last frame.
    pub reset_card: Option<Rect>,
    /// The layer and the ramp whose card is open.
    ramp: Option<(RasterId, usize)>,
    /// The row of the ramp in this frame, while its card is open.
    ramp_anchor: Option<Rect>,
    /// The card of a ramp in the last frame.
    pub ramp_card: Option<Rect>,
    /// The part of the bands that the user chose on the card of a ramp.
    /// `None`: the part that the colors of the ramp give.
    ramp_part: Option<Part>,
}

impl StyleState {
    /// Closes the color picker of the selected band.
    pub fn forget_color(&mut self) {
        self.side = None;
    }
}

/// A color of the card of a band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Paint {
    Fill,
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
/// hillshade and the rivers of a height layer.
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
    smoothing(ui, state, controls, id);

    let Some(bands) = state.globe.bands_mut(id) else {
        return;
    };
    ui.add_space(8.0);
    let strip = ui
        .scope(|ui| {
            ui.visuals_mut().override_text_color = Some(theme::MUTE);
            let m = &gradient::PAD;
            gradient::strip(ui, controls, COLOR_STOPS, m, bands, display, &unit)
        })
        .inner;
    if strip.clicked() {
        state.pad.page = Some(Page::Stops);
    }
    ui.add_space(8.0);

    if height {
        hillshade(ui, state, controls, id);
    }
    let preview = &mut state.globe.preview;
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

/// The page of the color stops of the Layers panel: the gradient control
/// with a row for each band, the reset row, and the row of the color
/// ramps. A tap on a band opens its card, which `band_card` shows.
pub fn stops_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: RasterId) {
    let Some(raster) = state.doc.project.raster(id) else {
        return;
    };
    let (display, unit, height) = (
        raster.display,
        raster.unit.clone(),
        crate::ui::is_height(raster),
    );
    let style = &mut state.pad.style;
    let globe = &mut state.globe;
    let mut selected = elevation::selected(&globe.preview, Some(id));
    let Some(bands) = globe.bands_mut(id) else {
        return;
    };
    let before = selected;
    let m = &gradient::COLUMN;
    let (resp, parts) = ui
        .scope(|ui| {
            ui.visuals_mut().override_text_color = Some(theme::MUTE);
            gradient::column(ui, controls, m, bands, &mut selected, display, &unit)
        })
        .inner;
    if selected != before {
        style.side = None;
    }
    // The card hides while a finger drags a stop, so that the globe shows.
    let count = bands.colors().len();
    let area = parts.bar.union(parts.rows);
    style.anchor = selected
        .filter(|i| !resp.dragged() && *i < count)
        .and_then(|index| {
            let at = parts.row(m, count, index).left_center();
            let keep = area.intersect(ui.clip_rect());
            ui.clip_rect().contains(at).then_some((id, index, at, keep))
        });
    if count >= MAX_BANDS {
        note(ui, FULL);
    }

    let default = Bands::with_range(bands.min(), bands.max(), height);
    let changed = default.as_ref().is_some_and(|d| d != bands);
    let row = widgets::text_row(ui, controls, RESET_STOPS, 0.0, false, changed);
    if row.clicked() {
        style.reset = (style.reset != Some(id)).then_some(id);
    }
    if !changed {
        style.reset = None;
    }
    style.reset_anchor = (style.reset == Some(id)).then_some((id, row.rect));
    if widgets::open_row(ui, controls, COLOR_RAMPS, "").clicked() {
        state.pad.page = Some(Page::Ramps);
    }
    state.globe.preview.selected = selected.map(|index| (id, index));
}

/// The page of the color ramps of the Layers panel: the import, and one row
/// for each ramp. A tap on a row opens the card of the ramp, which
/// `ramp_card` shows.
pub fn ramps_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: RasterId) {
    if widgets::text_row(ui, controls, IMPORT_RAMP, 0.0, false, true).clicked() {
        state.actions.push(Action::ImportRampDialog(id));
    }
    state.open_ramps();
    match &state.ramp_note {
        Some(n) if n.layer == Some(id) => {
            super::note_text(ui, None, &mut state.ramp_note, Some(id))
        }
        _ if state.ramps.list().is_empty() => note(ui, IMPORT_RAMP_HINT),
        _ => {}
    }
    let style = &mut state.pad.style;
    style.ramp_anchor = None;
    if style
        .ramp
        .is_some_and(|(layer, i)| layer != id || i >= state.ramps.list().len())
    {
        style.ramp = None;
    }
    for (index, ramp) in state.ramps.list().iter().enumerate() {
        let open = style.ramp == Some((id, index));
        let row = ramp_row(ui, controls, &ramp.name, &ramp.colors, open);
        if row.clicked() {
            style.ramp = (!open).then_some((id, index));
            style.ramp_part = None;
        }
        if style.ramp == Some((id, index)) {
            style.ramp_anchor = Some(row.rect);
        }
    }
}

/// The switch of the smoothing, and the minimum feature size while the
/// switch is on.
fn smoothing(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: RasterId) {
    let Some(mut smoothing) = state.doc.project.raster(id).map(|r| r.smoothing) else {
        return;
    };
    if widgets::switch_row(ui, controls, SMOOTHING, SMOOTHING, &mut smoothing.on) {
        state.set_raster_smoothing(id, smoothing.on);
    }
    if !smoothing.on {
        return;
    }
    let max = Smoothing::FEATURE_SIZE_MAX;
    let mut fraction = f64::from(smoothing.feature_size / max);
    let text = format!("{} pt", weight_text(smoothing.feature_size));
    if widgets::row_slider(
        ui,
        controls,
        FEATURE_SIZE,
        "Min. feature",
        &mut fraction,
        &text,
    ) {
        let size = (fraction as f32 * max / WEIGHT_STEP).round() * WEIGHT_STEP;
        state.set_raster_feature_size(id, size);
    }
}

/// The switch of the hillshade, and its opacity and highlights while the
/// switch is on.
fn hillshade(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: RasterId) {
    let Some(mut hillshade) = state.doc.project.raster(id).map(|r| r.hillshade) else {
        return;
    };
    if widgets::switch_row(ui, controls, HILLSHADE, HILLSHADE, &mut hillshade.on) {
        state.set_raster_hillshade(id, hillshade.on);
    }
    if !hillshade.on {
        return;
    }
    let mut fraction = f64::from(hillshade.opacity);
    let text = format!("{:.0}%", fraction * 100.0);
    let name = HILLSHADE_OPACITY;
    if widgets::row_slider(ui, controls, name, "Opacity", &mut fraction, &text) {
        // The steps are whole percents.
        let opacity = ((fraction * 100.0).round() / 100.0) as f32;
        state.set_raster_hillshade_opacity(id, opacity);
    }
    let name = HILLSHADE_HIGHLIGHTS;
    if widgets::switch_row(ui, controls, name, "Highlights", &mut hillshade.highlights) {
        state.set_raster_hillshade_highlights(id, hillshade.highlights);
    }
}

/// A button of the card of a ramp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RampChoice {
    /// With `true`, the layer keeps its stops. With as many colors as
    /// bands, the two ways are the same.
    Apply(bool),
    Reverse,
    Delete,
}

/// The parts of the bands, in the order of `PART_NAMES`.
pub const PARTS: [Part; 3] = [Part::All, Part::Sea, Part::Land];
/// The row of the part on the card of a ramp, and its choices.
pub const RAMP_PART: &str = "Apply to";
pub const PART_NAMES: [&str; 3] = ["All", "Sea", "Land"];

/// The number of bands of a part, and the name of the part with a space,
/// or an empty name for all bands.
pub fn ramp_fit(bands: &Bands, part: Part) -> (usize, &'static str) {
    let part = if bands.sea().is_some() {
        part
    } else {
        Part::All
    };
    let kind = match part {
        Part::All => "",
        Part::Sea => "sea ",
        Part::Land => "land ",
    };
    (bands.part_bands(part).len(), kind)
}

/// The row of a color ramp: its name, and a bar of its colors at the right.
fn ramp_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    colors: &[vale_terrain::Rgb],
    open: bool,
) -> egui::Response {
    let short: String = if name.chars().count() > RAMP_NAME {
        name.chars().take(RAMP_NAME - 1).chain(['…']).collect()
    } else {
        name.to_owned()
    };
    let row = widgets::text_row(ui, controls, &short, 0.0, open, true);
    let bar = Rect::from_min_size(
        pos2(
            row.rect.right() - RAMP_BAR.x,
            row.rect.center().y - RAMP_BAR.y / 2.0,
        ),
        RAMP_BAR,
    );
    let step = bar.width() / colors.len().max(1) as f32;
    for (i, c) in colors.iter().enumerate() {
        let left = bar.left() + step * i as f32;
        let block = Rect::from_x_y_ranges(left..=left + step, bar.y_range());
        let color = egui::Color32::from_rgb(c[0], c[1], c[2]);
        ui.painter().rect_filled(block, 0.0, color);
    }
    row
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

/// The card of the selected band: its color, and the value and the line of
/// its lower edge, with the button that removes that stop. The lowest band
/// has only its color. The card is at the left of the panel and points at
/// the row of the band. Above a sheet, the card is above the gradient. A
/// touch outside the card and the gradient control closes the card. The
/// card hides during a drag on the gradient control.
pub fn band_card(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    state.pad.style.card = None;
    let Some((id, index, at, gradient)) = state.pad.style.anchor.take() else {
        // The keypad of a field of the card closes with the card.
        if state.pad.style.stop.take().is_some() {
            widgets::close_keypad(ctx);
        }
        return;
    };
    if state.pad.style.stop.replace((id, index)) != Some((id, index)) {
        widgets::close_keypad(ctx);
    }
    let panel = state.pad.panels.iter().find(|(p, _)| *p == Panel::Layers);
    let Some(&(_, panel)) = panel else {
        return;
    };
    let Some(unit) = state.doc.project.raster(id).map(|r| r.unit.clone()) else {
        return;
    };
    let lowest = index == 0;
    let fixed = state.globe.bands_mut(id).is_some_and(|b| b.fixed(index));
    let rows = match (lowest, fixed) {
        (true, _) => 0.0,
        (false, true) => 6.0,
        (false, false) => 7.0,
    };
    let guess = PANEL_ROW * rows + 196.0 + 16.0;
    let height = state.pad.style.height.unwrap_or(guess) + 2.0 * CARD_PAD;

    let (rect, point) = place(rects, panel, at, gradient, height);
    let key = "pad-stop";
    let fits = state
        .pad
        .style
        .height
        .map(|_| rect.height() >= height - 0.5);
    state.pad.style.card = Some(rect);

    let menu = widgets::edit_menu_rect(ctx).unwrap_or(Rect::NOTHING);
    if pressed_outside(ctx, rect, gradient) && pressed_outside(ctx, menu, Rect::NOTHING) {
        state.globe.preview.selected = None;
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

            let mut line = bands.stops()[index].line;
            let mut changed = false;
            if !lowest {
                if fixed {
                    let span = bands.max() - bands.min();
                    let value = bands.stops()[index].value;
                    widgets::value_row(ui, "Value", &world::value_text(value, span, &unit));
                } else {
                    value_field(ui, controls, bands, index, &unit);
                }
                changed |= line_controls(ui, controls, &mut line);
            }
            // A darker line has no color of its own to pick.
            let paints: &[Paint] = if lowest || line.darker {
                &[Paint::Fill]
            } else {
                &[Paint::Fill, Paint::Line]
            };
            let paint = match style.side {
                Some((of, at, paint)) if of == id && at == index && paints.contains(&paint) => {
                    paint
                }
                _ => Paint::Fill,
            };
            let paint = if paints.len() > 1 {
                let active = paints.iter().position(|p| *p == paint);
                let names = [FILL_COLOR, LINE_COLOR];
                match widgets::choice_row(ui, controls, COLOR, &names, active) {
                    Some(i) => paints[i],
                    None => paint,
                }
            } else {
                paint
            };
            style.side = Some((id, index, paint));
            ui.add_space(4.0);
            let mut picked = match paint {
                Paint::Fill => bands.colors()[index],
                Paint::Line => line.color,
            };
            if widgets::color_picker(ui, controls, COLOR, &mut picked) {
                match paint {
                    Paint::Fill => {
                        bands.set_color(index, picked);
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
    paint_point(ctx, key, point);
    if removed {
        state.globe.preview.selected = None;
        state.pad.style.side = None;
    }
}

/// The switch, the weight, and the color source of a line. Returns true if
/// the line changed.
fn line_controls(ui: &mut egui::Ui, controls: &mut Controls, line: &mut StopLine) -> bool {
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
    if let Some(choice) = widgets::choice_row(ui, controls, LINE_SOURCE, &[DARKER, CUSTOM], active)
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
    changed
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

    state.pad.style.side = None;
}

/// The card of the ramp of a row. If the ramp has as many colors as the
/// layer has bands, the card applies it. Otherwise, the layer keeps its
/// stops, or it gets one band for each color. A touch outside the card and
/// the row closes it.
pub fn ramp_card(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    state.pad.style.ramp_card = None;
    let (Some((id, index)), Some(row)) = (state.pad.style.ramp, state.pad.style.ramp_anchor.take())
    else {
        state.pad.style.ramp = None;
        return;
    };
    let panel = state.pad.panels.iter().find(|(p, _)| *p == Panel::Layers);
    let (Some(&(_, panel)), Some(ramp)) = (panel, state.ramps.list().get(index)) else {
        return;
    };
    let colors = ramp.colors.len();
    let Some(layer) = state.globe.bands_mut(id) else {
        return;
    };
    let split = layer.sea().is_some();
    let part = state
        .pad
        .style
        .ramp_part
        .unwrap_or(layer.part(&ramp.colors));
    let (bands, kind) = ramp_fit(layer, part);
    let keep = format!("Fit to {bands} {kind}bands");
    let fresh = format!("Make {colors} {kind}bands");
    let mut buttons: Vec<(&str, egui::Color32, RampChoice)> = if colors == bands {
        vec![(APPLY_RAMP, theme::ACCENT, RampChoice::Apply(true))]
    } else {
        vec![
            (&keep, theme::ACCENT, RampChoice::Apply(true)),
            (&fresh, theme::ACCENT, RampChoice::Apply(false)),
        ]
    };
    buttons.push((REVERSE_RAMP, theme::TEXT, RampChoice::Reverse));
    buttons.push((DELETE_RAMP, theme::WARN, RampChoice::Delete));
    let name = &ramp.name;
    let text = format!("{name} · {colors} colors");
    let choice = if split { PANEL_ROW + 8.0 } else { 0.0 };
    let font = theme::body();
    let wrap = CARD_WIDTH - 2.0 * CARD_PAD;
    let text = ctx.fonts_mut(|f| f.layout(text, font, theme::MUTE, wrap));
    let size = text.size().y + 12.0 + choice + buttons.len() as f32 * PANEL_ROW + 2.0 * CARD_PAD;
    let at = pos2(row.left() + 40.0, row.center().y);
    let (rect, point) = place(rects, panel, at, row, size);
    if pressed_outside(ctx, rect, row) {
        state.pad.style.ramp = None;
        return;
    }
    state.pad.style.ramp_card = Some(rect);
    let mut chosen = None;
    let mut new_part = None;
    let radius = theme::CARD_RADIUS.into();
    let key = "pad-ramp";
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
        if split {
            let active = PARTS.iter().position(|p| *p == part);
            let picked = widgets::choice_row(ui, controls, RAMP_PART, &PART_NAMES, active);
            new_part = picked.map(|i| PARTS[i]);
            ui.add_space(8.0);
        }
        for (name, fill, choice) in &buttons {
            if widgets::button_row_in(ui, controls, name, true, *fill).clicked() {
                chosen = Some(*choice);
            }
        }
    });
    paint_point(ctx, key, point);
    if new_part.is_some() {
        state.pad.style.ramp_part = new_part;
    }
    let Some(chosen) = chosen else {
        return;
    };
    // The card stays open after a reverse, so that the bar shows it.
    if chosen != RampChoice::Reverse {
        state.pad.style.ramp = None;
    }
    let result = match chosen {
        RampChoice::Apply(keep) => state.apply_ramp(id, index, keep, part),
        RampChoice::Reverse => state.ramps.reverse(index).map_err(|e| e.to_string()),
        RampChoice::Delete => state.ramps.remove(index).map_err(|e| e.to_string()),
    };
    if let Err(text) = result {
        state.ramp_note = Some(crate::ui::Note::new(text, true, Some(id)));
    }
}

/// The field of the value of a stop. The stop moves when the typed text is
/// a number.
fn value_field(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    bands: &mut Bands,
    index: usize,
    unit: &str,
) {
    let span = bands.max() - bands.min();
    let shown = world::value_text(bands.stops()[index].value, span, "");
    let valid = |text: &str| parse_value(text).is_some();
    let keys = widgets::Keys::Number;
    let typed = widgets::key_field(ui, controls, STOP_VALUE, "Value", unit, &shown, keys, valid);
    if let Some(value) = typed.as_deref().and_then(parse_value) {
        bands.move_to(index, value);
    }
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
