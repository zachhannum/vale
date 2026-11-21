//! The Style section of the panel of a raster layer: the display and the
//! color stops.

use std::ops::{Range, RangeInclusive};
use std::sync::Arc;

use eframe::egui::{self, Align2, Order, Pos2, Rect, Sense, Vec2, pos2, vec2};
use vale_store::{BlendMode, FrameId, Hillshade, LayerId, RasterDisplay, RasterId, Smoothing};
use vale_terrain::{Bands, MAX_BANDS, Part, StopLine};

use super::geometry::{PAD, Rects, TOUCH};
use super::icons::Icon;
use super::list;
use super::number::Number;
use super::stops;
use super::theme;
use super::widgets::{self, Controls, PANEL_ROW};
use super::{Page, Panel};
use crate::areas::{FrameBins, FrameKey};
use crate::globe::preview::{LAKE_KM2, LAKE_KM2_RANGE};
use crate::ui::{Action, AppState, RampApply, RampSource, elevation, gradient};
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
/// The name of the field of the band name in the card of a band.
pub const BAND_NAME: &str = "Band name";
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
pub const REMOVE_STOP: &str = "Remove stop";
/// The row that puts back the default color stops, and the button of the
/// card that asks before it.
pub const RESET_STOPS: &str = "Reset colors";
/// The row of the stops page of a frame that moves the stops into the
/// values of the frame.
pub const FIT_TO_FRAME: &str = "Fit stops to frame";
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
/// The rows that open the editor of a new ramp, and the item of the desktop
/// menu of a ramp that opens it with a copy.
pub const NEW_RAMP: &str = "New ramp…";
pub const SAVE_AS_RAMP: &str = "Save as ramp…";
pub const DUPLICATE_RAMP: &str = "Duplicate…";
/// The button behind the row of a ramp that opens the editor with a copy.
pub const DUPLICATE: &str = "Duplicate";
/// The text below the import row while the app has no ramps.
pub const IMPORT_RAMP_HINT: &str = "A row of color blocks, lowest band first.";
/// The buttons of the card of a ramp.
pub const APPLY_RAMP: &str = "Apply";
pub const DELETE_RAMP: &str = "Delete";
pub const REVERSE_RAMP: &str = "Reverse";
/// The button of the card of a ramp in the Classify form.
pub const USE_RAMP: &str = "Use";
/// The buttons that open the labels of the colors of a custom ramp and of a
/// built-in ramp.
pub const EDIT_LABELS: &str = "Edit labels…";
pub const VIEW_LABELS: &str = "View labels";
/// The title of the page of the labels of a ramp.
pub const LABELS: &str = "Labels";
pub const LABELS_HINT: &str = "A value that matches a label gets its color.";
/// The longest name of a ramp row, in characters.
const RAMP_NAME: usize = 20;
/// The width and the height of the bar of a ramp row.
const RAMP_BAR: Vec2 = vec2(96.0, 14.0);
pub const RESET: &str = "Reset";
const RESET_TEXT: &str = "Reset this layer to the default color stops? This cannot be undone.";
/// The name of the unit field.
pub const UNIT: &str = "Unit";
/// The switch of the smoothing and the slider of the minimum feature size.
pub const SMOOTHING: &str = "Smoothing";
pub const FEATURE_SIZE: &str = "Feature size";
/// The row of the panel of a raster layer that opens the page of the blend
/// mode, and the title of the page.
pub const BLEND: &str = "Blend mode";

/// The row of the Style section that opens the page of the hillshade, and
/// the title of the page.
pub const HILLSHADE: &str = "Hillshade";
/// The controls of the page of the hillshade.
pub const HILLSHADE_ON: &str = "Hillshade switch";
pub const HILLSHADE_OPACITY: &str = "Hillshade opacity";
pub const HILLSHADE_HIGHLIGHTS: &str = "Hillshade highlights";
pub const HILLSHADE_AZIMUTH: &str = "Light direction";
pub const HILLSHADE_ALTITUDE: &str = "Light height";
pub const HILLSHADE_RELIEF: &str = "Relief";
pub const HILLSHADE_COLOR: &str = "Shade color";
pub const RIVERS: &str = "Rivers";
/// The switch of the lines of the stops of a raster layer.
pub const BAND_LINES: &str = "Band lines";
pub const RIVER_WIDTH: &str = "River width";
pub const RIVER_OPACITY: &str = "River opacity";
pub const SMALLEST_LAKE: &str = "Smallest lake";

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
    /// The layer, the selected band, the left of its row, and the histogram
    /// and the list of the bands in this frame.
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
    /// The user and the ramp whose card is open.
    ramp: Option<(RampUser, usize)>,
    /// The row of the ramp in this frame, while its card is open.
    ramp_anchor: Option<Rect>,
    /// The card of a ramp in the last frame.
    pub ramp_card: Option<Rect>,
    /// The list of the ramps. A row is the index of a ramp.
    pub ramp_list: list::ListState<usize>,
    /// The part of the bands that the user chose on the card of a ramp.
    /// `None`: the part that the colors of the ramp give.
    ramp_part: Option<Part>,
    /// Whether the user chose all bands on the card of a ramp in a frame.
    /// `None`: the mode of the stops page.
    ramp_all: Option<bool>,
    /// The number of bands that the user chose on the card of a ramp.
    /// `None`: one band for each color.
    ramp_count: Option<usize>,
    /// Whether the user chose new stops on the card of a ramp. `None`: the
    /// layer keeps its stops.
    ramp_new: Option<bool>,
    /// The layer whose color picker of the shade color is open.
    shade_color: Option<RasterId>,
    /// The color picker of the shade color showed in this frame.
    shade_shown: bool,
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
/// hillshade and the rivers of a height layer. In the panel of frame
/// `frame`, the bar shows the bands that the frame shows.
pub fn style(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: RasterId,
    frame: Option<FrameId>,
) {
    let Some(raster) = state.doc.project.raster(id) else {
        return;
    };
    let (display, unit, blend) = (raster.display, raster.unit.clone(), raster.blend);
    let height = crate::ui::is_height(raster);

    heading(ui, "Style");
    let active = RasterDisplay::ALL.iter().position(|d| *d == display);
    let choice = widgets::choice_row(ui, controls, SHOW_AS, &elevation::DISPLAYS, active);
    if let Some(index) = choice {
        state.set_raster_display(id, RasterDisplay::ALL[index]);
    }
    let display = state.doc.project.raster(id).map_or(display, |r| r.display);
    let blend = state.doc.project.raster(id).map_or(blend, |r| r.blend);
    if widgets::open_row(ui, controls, BLEND, elevation::blend_label(blend)).clicked() {
        state.pad.page = Some(Page::Blend);
    }
    smoothing(ui, state, controls, id);

    let window = frame.and_then(|frame| frame_window(ui.ctx(), state, frame, id));
    let shown = window.map(|(w, _)| w.bands);
    let Some(bands) = state.globe.bands_mut(id) else {
        return;
    };
    ui.add_space(8.0);
    let strip = ui
        .scope(|ui| {
            ui.visuals_mut().override_text_color = Some(theme::MUTE);
            let m = &gradient::PAD;
            gradient::strip(ui, controls, COLOR_STOPS, m, bands, display, &unit, shown)
        })
        .inner;
    if strip.clicked() {
        state.pad.page = Some(Page::Stops);
        state.atlas.stops_all = false;
    }
    ui.add_space(8.0);
    let Some(mut lines) = state.doc.project.raster(id).map(|r| r.lines) else {
        return;
    };
    if widgets::switch_row(ui, controls, BAND_LINES, BAND_LINES, &mut lines) {
        state.set_raster_lines(id, lines);
    }

    if height {
        let on = state.doc.project.raster(id).is_some_and(|r| r.hillshade.on);
        let value = if on { "On" } else { "Off" };
        if widgets::open_row(ui, controls, HILLSHADE, value).clicked() {
            state.pad.page = Some(Page::Hillshade);
        }
    }
    let Some(mut rivers) = state.doc.project.raster(id).map(|r| r.rivers) else {
        return;
    };
    // The rivers belong to the globe. A map frame draws no rivers.
    if height && state.workspace != crate::ui::Workspace::Atlas {
        if widgets::switch_row(ui, controls, RIVERS, RIVERS, &mut rivers) {
            state.set_raster_rivers(id, rivers);
        }
        let preview = &mut state.globe.preview;
        let range = elevation::RIVER_WIDTH;
        let range = f64::from(*range.start())..=f64::from(*range.end());
        let width = Number::new(RIVER_WIDTH, f64::from(preview.river_width), range)
            .label("Width")
            .log()
            .step(0.01)
            .unit("×")
            .text(|w| format!("{w:.2}×"))
            .default(1.0)
            .slider(ui, controls);
        if let Some(width) = width {
            preview.river_width = width as f32;
        }
        let opacity = Number::new(RIVER_OPACITY, f64::from(preview.river_opacity), 0.0..=1.0)
            .label("Opacity")
            .percent()
            .default(0.9)
            .slider(ui, controls);
        if let Some(opacity) = opacity {
            preview.river_opacity = opacity as f32;
        }
        let lake = Number::new(SMALLEST_LAKE, preview.lake_km2, LAKE_KM2_RANGE)
            .log()
            .unit("km²")
            .text(|a| format!("{a:.0} km²"))
            .default(LAKE_KM2)
            .slider(ui, controls);
        if let Some(lake) = lake {
            preview.lake_km2 = lake;
        }
    }
}

/// The page of the color stops of the Layers panel: the histogram of the
/// area at each value, the list of the bands, the reset row, and the row of
/// the color ramps. A tap on a band opens its card, which `band_card`
/// shows. On the page of frame `frame`, a switch shows the bands that the
/// frame shows or all bands, and in frame, a row fits the stops to the
/// values of the frame.
pub fn stops_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: RasterId,
    frame: Option<FrameId>,
) {
    let Some(raster) = state.doc.project.raster(id) else {
        return;
    };
    let (display, unit, height) = (
        raster.display,
        raster.unit.clone(),
        crate::ui::is_height(raster),
    );
    if frame.is_some() {
        let choices = [stops::IN_FRAME, stops::ALL_BANDS];
        let active = usize::from(state.atlas.stops_all);
        if let Some(choice) = widgets::segmented(ui, controls, &choices, Some(active), true) {
            state.atlas.stops_all = choice == 1;
        }
        ui.add_space(12.0);
    }
    let frame = frame.filter(|_| !state.atlas.stops_all);
    state.areas.retain(|r| r == id);
    let window = frame.and_then(|frame| frame_window(ui.ctx(), state, frame, id));
    let bins = match &window {
        Some((_, bins)) => bins.clone().map(|b| Arc::new(b.bins.clone())),
        None => (state.globe.value_map(id)).and_then(|map| state.areas.bins(id, map)),
    };
    let window = window.map(|(w, _)| w);
    if state.areas.poll(false) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
    let style = &mut state.pad.style;
    let globe = &mut state.globe;
    let mut selected = elevation::selected(&globe.preview, Some(id));
    let Some(bands) = globe.bands_mut(id) else {
        return;
    };
    let before = selected;
    let count = bands.colors().len();
    let shown = window.as_ref().map_or(0..count, |w| w.bands.clone());
    let histogram = stops::histogram(ui);
    let above = count.saturating_sub(shown.end);
    let fold = |ui: &mut egui::Ui, controls: &mut Controls, name, count, side| {
        let text = stops::fold_text(count, side);
        let indent = stops::FOLD_INDENT;
        widgets::text_row_as(ui, controls, name, &text, indent, false, true).clicked()
    };
    let mut all = above > 0 && fold(ui, controls, stops::BANDS_ABOVE, above, "above");
    let legend = stops::legend(
        ui,
        controls,
        bands,
        &mut selected,
        display,
        &unit,
        bins.as_deref().map(Vec::as_slice),
        shown.clone(),
    );
    if shown.start > 0 {
        all |= fold(ui, controls, stops::BANDS_BELOW, shown.start, "below");
    }
    let marker = legend.scrub.map(|i| bands.stops()[i].value);
    let dim = selected.filter(|_| legend.scrub.is_none());
    let bins = bins.as_deref().map(Vec::as_slice);
    let range = window.as_ref().map(|w| (w.lo, w.hi));
    stops::paint_histogram(
        ui, histogram, bins, bands, display, &unit, dim, marker, range,
    );
    if all {
        state.atlas.stops_all = true;
        ui.ctx().request_repaint();
    }
    if selected != before {
        style.side = None;
    }
    // The card hides while a finger drags a stop, so that the globe shows.
    let count = bands.colors().len();
    let area = histogram.union(legend.rows);
    style.anchor = selected
        .filter(|i| legend.scrub.is_none() && *i < count)
        .and_then(|index| {
            let at = legend.row_of.get(index)?.left_center();
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
    if let Some(w) = &window {
        let on = w.lo < w.hi;
        if widgets::text_row(ui, controls, FIT_TO_FRAME, 0.0, false, on).clicked() {
            match bands.fitted_to(w.lo, w.hi) {
                Some(fitted) => *bands = fitted,
                None => {
                    let text = format!("Range is too small for {count} bands.");
                    state.ramp_note = Some(crate::ui::Note::new(text, true, Some(id)));
                }
            }
        }
        if state
            .ramp_note
            .as_ref()
            .is_some_and(|n| n.layer == Some(id))
        {
            super::note_text(ui, None, &mut state.ramp_note, Some(id));
        }
    }
    if widgets::open_row(ui, controls, COLOR_RAMPS, "").clicked() {
        state.pad.page = Some(Page::Ramps);
    }
    state.globe.preview.selected = selected.map(|index| (id, index));
    if widgets::text_row(ui, controls, SAVE_AS_RAMP, 0.0, false, true).clicked() {
        open_editor(state, Some(id), RampSource::Bands(id));
    }
}

/// The bands of layer `id` that frame `frame` shows, and the level areas
/// inside its shape. The bands come again from the areas only while no
/// finger is down, so a stop that a finger drags stays in the window. The
/// areas come again only when the frame stops.
pub(crate) fn frame_window(
    ctx: &egui::Context,
    state: &mut AppState,
    frame: FrameId,
    id: RasterId,
) -> Option<(stops::FrameWindow, Option<Arc<FrameBins>>)> {
    let record = state.doc.project.atlas.frame(frame)?;
    let spec = state.doc.projection(record.map);
    let key = FrameKey::new(record, spec, state.doc.project.world.radius_km * 1000.0);
    let wait = crate::ui::atlas::gesture(ctx, state);
    if wait {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    let held = ctx.input(|i| i.pointer.any_down() || i.multi_touch().is_some());
    let map = state.globe.value_map(id)?;
    let bins = state.areas.frame_bins(frame, id, key, map, wait);
    let bands = state.globe.bands_mut(id)?;
    let count = bands.colors().len();
    let stored =
        (state.atlas.stops_window.clone()).filter(|w| held && w.frame == frame && w.raster == id);
    let mut window = stored.unwrap_or_else(|| {
        let (lo, hi) = match bins.as_deref().filter(|b| !b.is_empty()) {
            Some(b) => (bands.value(b.lo), bands.value(b.hi)),
            None => (bands.min(), bands.max()),
        };
        stops::FrameWindow {
            frame,
            raster: id,
            bands: crate::color_key::bands_between(bands, lo, hi),
            lo,
            hi,
        }
    });
    let end = window.bands.end.clamp(1, count);
    window.bands = window.bands.start.min(end - 1)..end;
    state.atlas.stops_window = Some(window.clone());
    Some((window, bins))
}

/// Opens the page of the editor of a new ramp.
fn open_editor(state: &mut AppState, layer: Option<RasterId>, source: RampSource) {
    state.run_action(Action::NewRamp(layer, source));
    if state.ramp_draft.is_some() {
        state.pad.style.ramp = None;
        state.pad.page = Some(Page::RampEditor);
    }
}

/// The part of the app that the page of the color ramps serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RampUser {
    /// The color stops of a raster layer.
    Raster(RasterId),
    /// The Classify form of a vector layer.
    Classify(LayerId),
}

impl RampUser {
    /// The raster layer whose panel shows the note of an import.
    fn raster(self) -> Option<RasterId> {
        match self {
            RampUser::Raster(id) => Some(id),
            RampUser::Classify(_) => None,
        }
    }
}

/// The page of the color ramps of the Layers panel: the import, and one row
/// for each ramp. A tap on a row opens the card of the ramp, which
/// `ramp_card` shows.
pub fn ramps_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    user: RampUser,
) {
    if widgets::text_row(ui, controls, IMPORT_RAMP, 0.0, false, true).clicked() {
        state.actions.push(Action::ImportRampDialog(user.raster()));
    }
    if widgets::text_row(ui, controls, NEW_RAMP, 0.0, false, true).clicked() {
        open_editor(state, user.raster(), RampSource::Blank);
    }
    state.open_ramps();
    match &state.ramp_note {
        Some(n) if n.layer == user.raster() => {
            super::note_text(ui, None, &mut state.ramp_note, user.raster())
        }
        _ if state.ramps.custom().is_empty() => note(ui, IMPORT_RAMP_HINT),
        _ => {}
    }
    let chosen = match user {
        RampUser::Classify(id) => Some(super::classify::ramp_of(state, id)),
        RampUser::Raster(_) => None,
    };
    let style = &mut state.pad.style;
    style.ramp_anchor = None;
    if style
        .ramp
        .is_some_and(|(u, i)| u != user || i >= state.ramps.list().len())
    {
        style.ramp = None;
    }
    let open = style.ramp.map(|(_, i)| i);
    let ramps = state.ramps.list();
    let rows: Vec<list::Row<usize>> = ramps
        .iter()
        .enumerate()
        .map(|(i, ramp)| list::Row {
            label: short_name(&ramp.name),
            on: open == Some(i) || chosen == Some(i),
            rename: !ramp.built_in,
            delete: !ramp.built_in,
            actions: &[DUPLICATE],
            select: !ramp.built_in,
            ..list::Row::new(i, ramp.name.clone())
        })
        .collect();
    let list = &mut style.ramp_list;
    let renamed = list.rename.as_ref().map(|r| r.0);
    let valid = |name: &str| renamed.is_some_and(|i| state.ramps.can_rename(i, name));
    let spec = list::Spec {
        select: true,
        valid: &valid,
        ..list::Spec::default()
    };
    let mut places = vec![Rect::NOTHING; rows.len()];
    let events = list::show(ui, controls, list, spec, &rows, |ui, controls, cell, i| {
        places[i] = cell.rect;
        let moved = cell.rect.translate(vec2(cell.shift.max(0.0), 0.0));
        paint_bar(&cell.painter, moved, &ramps[i].colors);
        let look = list::Look {
            text: &rows[i].label,
            ..list::Look::default()
        };
        list::draw(ui, controls, cell, &look)
    });
    for event in events {
        match event {
            list::Event::Tap(index) => {
                let style = &mut state.pad.style;
                style.ramp = (open != Some(index)).then_some((user, index));
                style.ramp_part = None;
                style.ramp_all = None;
                style.ramp_count = None;
                style.ramp_new = None;
            }
            list::Event::Rename(index, name) => {
                if let Err(e) = state.ramps.rename(index, &name) {
                    state.ramp_note =
                        Some(crate::ui::Note::new(e.to_string(), true, user.raster()));
                }
            }
            list::Event::Delete(index) => remove_ramps(state, user, &[index]),
            list::Event::Action(index, DUPLICATE) => {
                open_editor(state, user.raster(), RampSource::Copy(index));
            }
            _ => {}
        }
    }
    let style = &mut state.pad.style;
    style.ramp_anchor =
        (style.ramp.and_then(|(_, i)| places.get(i).copied())).filter(|r| r.is_positive());
}

/// The name of the header button that deletes the selected ramps.
pub const DELETE_RAMPS: &str = "Delete ramps";

/// The buttons of the header of the page of the color ramps while ramps
/// are selected.
pub(super) fn ramp_buttons(state: &AppState) -> Option<Vec<(&'static str, Icon)>> {
    let picked = state.pad.style.ramp_list.picked.len();
    let count = state.ramps.custom().len();
    list::group_buttons(picked, count, &[], Some(DELETE_RAMPS))
}

/// Does the action of a header button of the selected ramps.
pub(super) fn ramp_group(state: &mut AppState, user: RampUser, name: &'static str) {
    let keys: Vec<usize> = (state.ramps.list().iter().enumerate())
        .filter(|(_, r)| !r.built_in)
        .map(|(i, _)| i)
        .collect();
    let list = &mut state.pad.style.ramp_list;
    if let Some(list::Group::Delete(indices)) =
        list::group_action(list, &keys, name, Some(DELETE_RAMPS))
    {
        remove_ramps(state, user, &indices);
    }
}

/// Deletes the custom ramps at `indices`, and closes the card of a ramp.
fn remove_ramps(state: &mut AppState, user: RampUser, indices: &[usize]) {
    state.pad.style.ramp = None;
    state.pad.style.ramp_list = list::ListState::default();
    let mut sorted = indices.to_vec();
    sorted.sort_unstable();
    for &index in sorted.iter().rev() {
        super::classify::ramp_removed(state, index);
    }
    if let Err(e) = state.ramps.remove_all(&sorted) {
        state.ramp_note = Some(crate::ui::Note::new(e.to_string(), true, user.raster()));
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
    let max = f64::from(Smoothing::FEATURE_SIZE_MAX);
    let size = Number::new(FEATURE_SIZE, f64::from(smoothing.feature_size), 0.0..=max)
        .label("Min. feature")
        .step(f64::from(WEIGHT_STEP))
        .unit("pt")
        .slider(ui, controls);
    if let Some(size) = size {
        state.set_raster_feature_size(id, size as f32);
    }
}

/// The page of the blend mode: a row for each mode, with a mark on the mode
/// of the layer.
pub fn blend_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: RasterId) {
    let Some(current) = state.doc.project.raster(id).map(|r| r.blend) else {
        return;
    };
    if let Some(mode) = blend_rows(ui, controls, current) {
        state.set_raster_blend(id, mode);
    }
}

/// The page of the blend mode of an image layer.
pub fn image_blend_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: vale_store::ImageId,
) {
    let Some(current) = state.doc.project.image(id).map(|i| i.blend) else {
        return;
    };
    if let Some(mode) = blend_rows(ui, controls, current) {
        state.set_image_blend(id, mode);
    }
}

/// A row for each blend mode, with a mark on `current`. Returns the mode of
/// a tapped row.
pub fn blend_rows(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    current: BlendMode,
) -> Option<BlendMode> {
    let choices: Vec<(&str, bool)> = (BlendMode::ALL.iter())
        .map(|m| (elevation::blend_label(*m), *m == current))
        .collect();
    let name = |label: &str| format!("{BLEND} {label}");
    list::options(ui, controls, &choices, name).map(|i| BlendMode::ALL[i])
}

/// The page of the hillshade of the Layers panel: the switch, and while the
/// hillshade is on, its opacity, its highlights, its light, its relief, and
/// its shade color. A tap on the shade color opens a color picker below it.
pub fn hillshade_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: RasterId,
) {
    let Some(raster) = state.doc.project.raster(id) else {
        return;
    };
    if !crate::ui::is_height(raster) {
        state.pad.page = None;
        return;
    }
    let mut hillshade = raster.hillshade;
    if widgets::switch_row(ui, controls, HILLSHADE_ON, HILLSHADE, &mut hillshade.on) {
        state.set_raster_hillshade(id, hillshade.on);
    }
    if !hillshade.on {
        return;
    }
    let opacity = Number::new(HILLSHADE_OPACITY, f64::from(hillshade.opacity), 0.0..=1.0)
        .label("Opacity")
        .percent()
        .slider(ui, controls);
    if let Some(opacity) = opacity {
        state.set_raster_hillshade_opacity(id, opacity as f32);
    }
    let name = HILLSHADE_HIGHLIGHTS;
    if widgets::switch_row(ui, controls, name, "Highlights", &mut hillshade.highlights) {
        state.set_raster_hillshade_highlights(id, hillshade.highlights);
    }

    let name = HILLSHADE_AZIMUTH;
    if widgets::compass_row(ui, controls, name, name, &mut hillshade.azimuth) {
        state.set_raster_hillshade_azimuth(id, hillshade.azimuth);
    }

    let range = Hillshade::ALTITUDES;
    let range = f64::from(*range.start())..=f64::from(*range.end());
    let altitude = Number::new(HILLSHADE_ALTITUDE, f64::from(hillshade.altitude), range)
        .step(1.0)
        .unit("\u{b0}")
        .slider(ui, controls);
    if let Some(altitude) = altitude {
        state.set_raster_hillshade_altitude(id, altitude as f32);
    }

    let range = Hillshade::EXAGGERATIONS;
    let range = f64::from(*range.start())..=f64::from(*range.end());
    let relief = Number::new(HILLSHADE_RELIEF, f64::from(hillshade.exaggeration), range)
        .log()
        .step(0.01)
        .unit("\u{d7}")
        .text(|r| format!("{r:.2}\u{d7}"))
        .default(1.0)
        .slider(ui, controls);
    if let Some(relief) = relief {
        state.set_raster_hillshade_exaggeration(id, relief as f32);
    }

    let open = state.pad.style.shade_color == Some(id);
    let row = widgets::color_row(
        ui,
        controls,
        HILLSHADE_COLOR,
        widgets::opaque(hillshade.color),
        open,
    );
    if row.clicked() {
        state.pad.style.shade_color = (!open).then_some(id);
        if open {
            widgets::close_keypad(ui.ctx());
        }
    }
    if state.pad.style.shade_color == Some(id) {
        ui.add_space(4.0);
        let mut color = hillshade.color;
        if widgets::color_picker(ui, controls, HILLSHADE_COLOR, &mut color) {
            state.set_raster_hillshade_color(id, color);
        }
        state.pad.style.shade_shown = true;
    }
}

/// Closes the color picker of the shade color when it did not show in this
/// frame, with the keypad of its field.
pub fn shade_color_frame(ctx: &egui::Context, state: &mut AppState) {
    let style = &mut state.pad.style;
    let shown = std::mem::take(&mut style.shade_shown);
    if !shown && style.shade_color.take().is_some() {
        widgets::close_keypad(ctx);
    }
}

/// A button of the card of a ramp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RampChoice {
    Apply(RampApply),
    Reverse,
    Delete,
    /// Opens the editor of a custom ramp.
    Edit,
    /// Opens the page of the labels of the colors.
    Labels,
    /// Picks the ramp for the Classify form.
    Use,
    /// Opens the editor of a new ramp with a copy.
    Duplicate,
}

/// The parts of the bands, in the order of `PART_NAMES`.
pub const PARTS: [Part; 3] = [Part::All, Part::Sea, Part::Land];
/// The row of the part on the card of a ramp, and its choices.
pub const RAMP_PART: &str = "Apply to";
pub const PART_NAMES: [&str; 3] = ["All", "Sea", "Land"];
/// The choices of the bands on the card of a ramp in a frame.
pub const SCOPE_NAMES: [&str; 2] = ["Bands in frame", "All bands"];
/// The stepper of the number of bands that "Make" gives on the card of a
/// ramp.
pub const BAND_COUNT: &str = "Bands";
/// The row of the choice of the stops on the card of a ramp, and its
/// choices: the layer keeps its stops, or the ramp makes new stops.
pub const STOPS: &str = "Stops";
pub const STOP_NAMES: [&str; 2] = ["Keep", "New"];
/// The button of the card of a custom ramp that opens the editor.
pub const EDIT_RAMP: &str = "Edit ramp…";

/// The number of bands of a part, and the name of the part with a space,
/// or an empty name for all bands.
pub fn ramp_fit(bands: &Bands, part: Part) -> (usize, &'static str) {
    let part = part_of(bands, part);
    let kind = match part {
        Part::All => "",
        Part::Sea => "sea ",
        Part::Land => "land ",
    };
    (bands.part_bands(part).len(), kind)
}

/// The fewest and the most bands that "Make" can give the bands `bands` of
/// a part: two for bands that cross sea level, else one, and at most the
/// bands that fit next to the other bands.
pub fn make_range(layer: &Bands, part: Part, bands: Range<usize>) -> RangeInclusive<usize> {
    let p = layer.part_bands(part_of(layer, part));
    let r = bands.start.max(p.start)..bands.end.min(p.end);
    let crosses = layer.sea().is_some_and(|sea| r.start < sea && sea < r.end);
    let other = layer.colors().len() - r.len();
    let fewest = if crosses { 2 } else { 1 };
    fewest..=MAX_BANDS.saturating_sub(other).max(fewest)
}

/// The part, or all bands with no sea split.
fn part_of(bands: &Bands, part: Part) -> Part {
    if bands.sea().is_some() {
        part
    } else {
        Part::All
    }
}

/// The row of a color ramp: its name, and a bar of its colors at the right.
pub(super) fn ramp_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    colors: &[vale_terrain::Rgb],
    open: bool,
) -> egui::Response {
    let row = widgets::text_row(ui, controls, &short_name(name), 0.0, open, true);
    paint_bar(ui.painter(), row.rect, colors);
    row
}

/// The name of a ramp, shortened to `RAMP_NAME` characters.
fn short_name(name: &str) -> String {
    if name.chars().count() > RAMP_NAME {
        name.chars().take(RAMP_NAME - 1).chain(['…']).collect()
    } else {
        name.to_owned()
    }
}

/// Paints the bar of the colors of a ramp at the right of a row.
fn paint_bar(painter: &egui::Painter, row: Rect, colors: &[vale_terrain::Rgb]) {
    let bar = Rect::from_min_size(
        pos2(row.right() - RAMP_BAR.x, row.center().y - RAMP_BAR.y / 2.0),
        RAMP_BAR,
    );
    let step = bar.width() / colors.len().max(1) as f32;
    for (i, c) in colors.iter().enumerate() {
        let left = bar.left() + step * i as f32;
        let block = Rect::from_x_y_ranges(left..=left + step, bar.y_range());
        let color = egui::Color32::from_rgb(c[0], c[1], c[2]);
        painter.rect_filled(block, 0.0, color);
    }
}

/// A small muted text across the panel.
pub fn note(ui: &mut egui::Ui, text: &str) {
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
/// the row of the band. Above a sheet, the card is above the list. A touch
/// outside the card, the histogram, and the list closes the card. The card
/// hides during a drag on the value of a stop.
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
        (true, _) => 1.0,
        (false, true) => 7.0,
        (false, false) => 8.0,
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

    let backdrop = state.globe.backdrop();
    let style = &mut state.pad.style;
    let Some(bands) = state.globe.bands_mut(id) else {
        return;
    };
    let mut removed = false;
    let radius = theme::CARD_RADIUS.into();
    // The card grows from the tip of its point.
    let order = Order::Foreground;
    widgets::card_from(ctx, backdrop, key, rect, order, radius, point[1], |ui| {
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

            let mut name = bands.name(index).to_owned();
            let field = widgets::text_field(ui, controls, BAND_NAME, "Name", None, &mut name, true);
            if field.changed() {
                bands.set_name(index, &name);
            }
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
                let text = REMOVE_STOP;
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
    let range = StopLine::WEIGHTS;
    let range = f64::from(*range.start())..=f64::from(*range.end());
    let weight = Number::new(LINE_WEIGHT, f64::from(line.weight), range)
        .label("Weight")
        .step(f64::from(WEIGHT_STEP))
        .unit("pt")
        .keypad_elsewhere()
        .stepper(ui, controls);
    if let Some(weight) = weight {
        line.weight = weight as f32;
        changed = true;
    }
    let active = Some(usize::from(!line.darker));
    if let Some(choice) = widgets::choice_row(ui, controls, LINE_SOURCE, &[DARKER, CUSTOM], active)
    {
        line.darker = choice == 0;
        changed = true;
    }
    if line.darker {
        let darken = Number::new(DARKEN, f64::from(line.darken), 0.0..=1.0)
            .label("Darken")
            .percent()
            .step(0.05)
            .keypad_elsewhere()
            .slider(ui, controls);
        if let Some(darken) = darken {
            line.darken = darken as f32;
            changed = true;
        }
    }
    changed
}

/// The place of a card next to the panel, and the point of the card. The
/// card is at the left of the panel and points at `at`. Above a sheet, or
/// with no room at the left, the card is above `avoid`, or below it if no
/// room is above.
fn place(rects: &Rects, panel: Rect, at: Pos2, avoid: Rect, height: f32) -> (Rect, [Pos2; 3]) {
    let screen = rects.screen;
    // On a partially folded device, the card is in the part of its row.
    let part = super::geometry::fold_part(Rect::from_pos(at), rects.fold, screen);
    let top = (screen.top() + rects.bands.top + PAD).max(part.top());
    let bottom = (screen.bottom() - rects.bands.bottom - PAD).min(part.bottom());
    let room = panel.left() - GAP - CARD_WIDTH - PAD;
    let side = |height: f32| {
        let height = height.min(bottom - top);
        let size = vec2(CARD_WIDTH, height);
        let y = (at.y - height / 2.0).clamp(top, (bottom - height).max(top));
        Rect::from_min_size(pos2(panel.left() - GAP - CARD_WIDTH, y), size)
    };
    let crosses = |rect: Rect| rects.fold.is_some_and(|f| f.intersects(rect));
    if rects.sheet.is_none() && room >= screen.left() && !crosses(side(height)) {
        let rect = side(height);
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
    let left = (screen.left() + rects.bands.left + PAD).max(part.left());
    let right = (screen.right() - rects.bands.right - PAD).min(part.right());
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
            theme::glass_tone(),
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
    let backdrop = state.globe.backdrop();
    // The card grows from the tip of its point.
    let order = Order::Foreground;
    widgets::card_from(ctx, backdrop, key, rect, order, radius, point[1], |ui| {
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

/// The card of the ramp of a row. With Keep, the layer keeps its stops,
/// and with New, it gets the bands of the stepper. The card of a custom
/// ramp opens its editor. A touch outside the card and
/// the row closes it. With frame `frame`, the card has a choice of the bands
/// that the frame shows or all bands. It starts on the mode of the stops
/// page, and it is on all bands while the values of the frame are unknown.
pub fn ramp_card(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    frame: Option<FrameId>,
) {
    state.pad.style.ramp_card = None;
    let (Some((user, index)), Some(row)) =
        (state.pad.style.ramp, state.pad.style.ramp_anchor.take())
    else {
        state.pad.style.ramp = None;
        // The keypad of the stepper closes with the card.
        let field = format!("{BAND_COUNT} value");
        if widgets::keypad_field(ctx).as_deref() == Some(field.as_str()) {
            widgets::close_keypad(ctx);
        }
        return;
    };
    let panel = state.pad.panels.iter().find(|(p, _)| *p == Panel::Layers);
    let (Some(&(_, panel)), Some(ramp)) = (panel, state.ramps.list().get(index)) else {
        return;
    };
    let colors = ramp.colors.len();
    let built_in = ramp.built_in;
    let labeled = ramp.labels.iter().any(|l| !l.is_empty());
    let known = match (user, frame) {
        (RampUser::Raster(id), Some(frame)) => frame_window(ctx, state, frame, id)
            .filter(|(_, bins)| bins.as_ref().is_some_and(|b| !b.is_empty()))
            .map(|w| w.0),
        _ => None,
    };
    let scoped = frame.is_some() && matches!(user, RampUser::Raster(_));
    let all = known.is_none() || (state.pad.style.ramp_all).unwrap_or(state.atlas.stops_all);
    let window = known.clone().filter(|_| !all);
    let Some(ramp) = state.ramps.list().get(index) else {
        return;
    };
    let mut none = false;
    // The stepper of the number of new bands, with its range, for New.
    let mut count = None;
    let new_stops = state.pad.style.ramp_new.unwrap_or(false);
    let (button, split, part) = match user {
        RampUser::Raster(id) => {
            let Some(layer) = state.globe.bands_mut(id) else {
                return;
            };
            let split = layer.sea().is_some();
            let part = state
                .pad
                .style
                .ramp_part
                .unwrap_or(layer.ramp_part(&ramp.colors()));
            let (mut bands, kind) = ramp_fit(layer, part);
            if let Some(w) = &window {
                let p = layer.part_bands(part_of(layer, part));
                bands = w
                    .bands
                    .end
                    .min(p.end)
                    .saturating_sub(w.bands.start.max(p.start));
                none = bands == 0;
            }
            let button = if new_stops {
                let all = 0..layer.colors().len();
                let bands = window.as_ref().map_or(all, |w| w.bands.clone());
                let range = make_range(layer, part, bands);
                let made = state.pad.style.ramp_count.unwrap_or(colors);
                let made = made.clamp(*range.start(), *range.end());
                count = Some((made, range));
                let text = format!("Make {made} {kind}bands");
                (text, RampChoice::Apply(RampApply::Make(made)))
            } else if colors == bands {
                (APPLY_RAMP.to_owned(), RampChoice::Apply(RampApply::Fit))
            } else {
                let text = format!("Fit to {bands} {kind}bands");
                (text, RampChoice::Apply(RampApply::Fit))
            };
            (button, split, part)
        }
        RampUser::Classify(_) => ((USE_RAMP.to_owned(), RampChoice::Use), false, Part::All),
    };
    let raster = matches!(user, RampUser::Raster(_));
    // A built-in ramp does not change.
    let other = if !built_in {
        Some((EDIT_RAMP, RampChoice::Edit))
    } else if labeled {
        Some((VIEW_LABELS, RampChoice::Labels))
    } else {
        None
    };
    let name = &ramp.name;
    let text = format!("{name} · {colors} colors");
    let row_height = PANEL_ROW + 8.0;
    let choices = u8::from(split) + u8::from(scoped) + u8::from(raster);
    let choice = f32::from(choices) * row_height;
    let font = theme::body();
    let wrap = CARD_WIDTH - 2.0 * CARD_PAD;
    let text = ctx.fonts_mut(|f| f.layout(text, font, theme::MUTE, wrap));
    let rows = 1 + usize::from(other.is_some()) + usize::from(count.is_some());
    let size = text.size().y + 12.0 + choice + rows as f32 * PANEL_ROW + 2.0 * CARD_PAD;
    let at = pos2(row.left() + 40.0, row.center().y);
    let (rect, point) = place(rects, panel, at, row, size);
    let on_keypad = ctx
        .input(|i| i.pointer.interact_pos())
        .and_then(|pos| ctx.layer_id_at(pos))
        == Some(super::number::keypad_layer(BAND_COUNT));
    if pressed_outside(ctx, rect, row) && !on_keypad {
        state.pad.style.ramp = None;
        return;
    }
    state.pad.style.ramp_card = Some(rect);
    let mut chosen = None;
    let mut new_part = None;
    let mut new_all = None;
    let mut new_count = None;
    let mut new_new = None;
    let radius = theme::CARD_RADIUS.into();
    let key = "pad-ramp";
    let backdrop = state.globe.backdrop();
    // The card grows from the tip of its point.
    let order = Order::Foreground;
    widgets::card_from(ctx, backdrop, key, rect, order, radius, point[1], |ui| {
        let inner = rect.shrink(CARD_PAD);
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        let ui = &mut ui;
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let (place, _) = ui.allocate_exact_size(text.size(), Sense::hover());
        ui.painter().galley(place.min, text, theme::MUTE);
        ui.add_space(12.0);
        if scoped {
            let (row, _) = ui.allocate_exact_size(vec2(inner.width(), PANEL_ROW), Sense::hover());
            let field = Rect::from_center_size(row.center(), vec2(row.width(), TOUCH));
            let active = Some(usize::from(all));
            let enabled = |i: usize| i == 1 || known.is_some();
            let picked = widgets::segmented_at(ui, controls, field, &SCOPE_NAMES, active, enabled);
            new_all = picked.map(|i| i == 1);
            ui.add_space(8.0);
        }
        if split {
            let active = PARTS.iter().position(|p| *p == part);
            let picked = widgets::choice_row(ui, controls, RAMP_PART, &PART_NAMES, active);
            new_part = picked.map(|i| PARTS[i]);
            ui.add_space(8.0);
        }
        if raster {
            let active = Some(usize::from(new_stops));
            let picked = widgets::choice_row(ui, controls, STOPS, &STOP_NAMES, active);
            new_new = picked.map(|i| i == 1);
            ui.add_space(8.0);
        }
        if let Some((made, range)) = &count {
            let range = *range.start() as f64..=*range.end() as f64;
            new_count = Number::new(BAND_COUNT, *made as f64, range)
                .step(1.0)
                .enabled(!none)
                .stepper(ui, controls)
                .map(|v| v.round() as usize);
        }
        let (name, choice) = &button;
        let on = !(none && matches!(choice, RampChoice::Apply(_)));
        if widgets::button_row_in(ui, controls, name, on, theme::ACCENT).clicked() {
            chosen = Some(*choice);
        }
        if let Some((name, choice)) = other
            && widgets::button_row_in(ui, controls, name, true, theme::TEXT).clicked()
        {
            chosen = Some(choice);
        }
    });
    paint_point(ctx, key, point);
    if new_part.is_some() {
        state.pad.style.ramp_part = new_part;
    }
    if new_all.is_some() {
        state.pad.style.ramp_all = new_all;
    }
    if new_count.is_some() {
        state.pad.style.ramp_count = new_count;
    }
    if new_new.is_some() {
        state.pad.style.ramp_new = new_new;
    }
    let Some(chosen) = chosen else {
        return;
    };
    state.pad.style.ramp = None;
    let result = match (chosen, user) {
        (RampChoice::Apply(how), RampUser::Raster(id)) => {
            state.apply_ramp(id, index, how, part, window.as_ref())
        }
        (RampChoice::Apply(_), RampUser::Classify(_)) => Ok(()),
        (RampChoice::Use, RampUser::Classify(id)) => {
            super::classify::use_ramp(state, id, index);
            state.pad.page = Some(Page::Classify);
            Ok(())
        }
        (RampChoice::Use, RampUser::Raster(_)) => Ok(()),
        (RampChoice::Edit, _) => {
            open_editor(state, user.raster(), RampSource::Edit(index));
            Ok(())
        }
        // Only the desktop menu reverses and deletes.
        (RampChoice::Reverse | RampChoice::Delete, _) => Ok(()),
        (RampChoice::Labels, _) => {
            state.pad.page = Some(Page::RampLabels(index));
            Ok(())
        }
        (RampChoice::Duplicate, _) => {
            open_editor(state, user.raster(), RampSource::Copy(index));
            Ok(())
        }
    };
    if let Err(text) = result {
        state.ramp_note = Some(crate::ui::Note::new(text, true, user.raster()));
    }
}

/// The page of the labels of the colors of ramp `index`: a row for each
/// color, with its color and its label. The label of a custom ramp is a
/// field, and the label of a built-in ramp is text.
pub fn labels_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, index: usize) {
    let Some(ramp) = state.ramps.list().get(index).cloned() else {
        return;
    };
    ui.spacing_mut().item_spacing.y = 0.0;
    note(ui, LABELS_HINT);
    ui.add_space(4.0);
    // The field of a row takes the touches at the right of the swatch.
    let trailing = ui.available_width() + 8.0 - widgets::FORM_LABEL;
    let rows: Vec<list::Row<usize>> = (0..ramp.colors.len())
        .map(|i| list::Row {
            trailing,
            ..list::Row::new(i, format!("Color {}", i + 1))
        })
        .collect();
    let mut typed = None;
    let mut list = list::ListState::default();
    let spec = list::Spec::default();
    list::show(
        ui,
        controls,
        &mut list,
        spec,
        &rows,
        |ui, controls, cell, i| {
            let rect = cell.rect;
            let swatch =
                Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), vec2(32.0, 24.0));
            let color = ramp.colors[i];
            let fill = egui::Color32::from_rgb(color[0], color[1], color[2]);
            cell.painter.rect_filled(swatch, 4.0, fill);
            let field = Rect::from_min_max(
                pos2(
                    rect.left() + widgets::FORM_LABEL,
                    rect.center().y - TOUCH / 2.0,
                ),
                pos2(rect.right(), rect.center().y + TOUCH / 2.0),
            );
            let label = &ramp.labels[i];
            if ramp.built_in {
                let at = pos2(field.left() + 12.0, field.center().y);
                let (font, color) = (theme::body(), theme::TEXT);
                cell.painter
                    .text(at, Align2::LEFT_CENTER, label, font, color);
                return None;
            }
            let name = format!("Label {}", i + 1);
            let mut text = label.clone();
            let font = theme::body();
            let edit = ui.push_id(("ramp-label", i), |ui| {
                widgets::field_at(ui, controls, field, &name, None, &mut text, true, font)
            });
            if edit.inner.changed() {
                typed = Some((i, text));
            }
            None
        },
    );
    if let Some((i, text)) = typed
        && let Err(e) = state.ramps.set_label(index, i, &text)
    {
        state.ramp_note = Some(crate::ui::Note::new(e.to_string(), true, None));
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
        assert_eq!(parse_hex("#1A2b3C", false), Some([0x1a, 0x2b, 0x3c, 255]));
        assert_eq!(parse_hex(" ff0000 ", false), Some([255, 0, 0, 255]));
        assert_eq!(parse_hex("ff00", true), None);
        assert_eq!(parse_hex("gg0000", true), None);
        assert_eq!(parse_hex("ÿÿÿ", true), None);
        assert_eq!(hex_text([1, 171, 255, 255]), "01ABFF");
        // Eight digits give the alpha, if the picker has an opacity strip.
        assert_eq!(parse_hex("ff000080", true), Some([255, 0, 0, 0x80]));
        assert_eq!(parse_hex("ff0000", true), Some([255, 0, 0, 255]));
        assert_eq!(parse_hex("ff000080", false), None);
        assert_eq!(hex_text([255, 0, 0, 0x80]), "FF000080");
    }
}
