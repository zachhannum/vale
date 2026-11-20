//! The Toolbox panel: the operations as tiles in groups, and the page of an
//! operation with its inputs, its output, and the Run button.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};

use super::collection_choice;
use super::geometry::TOUCH;
use super::icons::{self, Icon};
use super::number::Number;
use super::rules::HEIGHT_LAYER;
use super::widgets::{self, Controls, PANEL_ROW};
use super::{TOOLBOX, filter, panels, theme};
use crate::ui::AppState;
use crate::ui::orient::{self, ORIENT_ABOUT, ORIENT_DOWNHILL};
use crate::ui::polygonize::{
    self, ABOVE, BASE, BELOW, CANCEL, COLOR_STOPS, INPUT, INTERVAL, LEVELS, Levels, NEEDS_RASTER,
    POLYGONIZE, POLYGONIZE_ABOUT, SIMPLIFY, SIMPLIFY_RANGE, SMOOTH, SPLIT, TOO_MANY_BANDS,
};
use crate::ui::raster_form::COLLECTION_NAME_TAKEN;

/// The hint of the search field of the grid.
pub const SEARCH: &str = "Search operations";
/// The empty state of the grid when no operation matches the search.
pub const NO_MATCH: &str = "No matching operations.";
/// The row of the line layer of an operation, and its page.
pub const LINES: &str = "Lines";
pub const FEATURES: &str = "Features";
pub const OUTPUT: &str = "Output";
pub const NEW_LAYER: &str = "New layer";
pub const EDIT_IN_PLACE: &str = "Edit in place";
pub const RUN: &str = "Run";
/// The value of an input with no layer.
pub const NONE: &str = "None";

/// The groups of the operations, in the order of the grid. Each operation
/// has its name and its icon.
pub const GROUPS: [(&str, &[(&str, Icon)]); 3] = [
    (
        "Terrain",
        &[
            (POLYGONIZE, Icon::Polygonize),
            ("Contour lines", Icon::Contours),
        ],
    ),
    (
        "Lines",
        &[
            (ORIENT_DOWNHILL, Icon::Orient),
            ("Smooth", Icon::SmoothLine),
            ("Simplify", Icon::Simplify),
        ],
    ),
    (
        "Polygons",
        &[("Dissolve", Icon::Dissolve), ("Clip", Icon::Clip)],
    ),
];

/// True for an operation that exists. The tile of another operation is
/// faded and takes no tap.
pub fn exists(name: &str) -> bool {
    name == ORIENT_DOWNHILL || name == POLYGONIZE
}

/// A page of the Toolbox panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolPage {
    Orient,
    /// The line layers that "Orient lines downhill" can take.
    OrientLines,
    /// The height layers that "Orient lines downhill" can take.
    OrientHeight,
    Polygonize,
    /// The raster layers that "Polygonize" can take.
    PolygonizeInput,
    /// The limits of the bands of "Polygonize".
    PolygonizeLevels,
    /// The collection of the output layers of "Polygonize".
    PolygonizeCollection,
}

/// The state of the Toolbox panel.
#[derive(Default)]
pub struct ToolboxState {
    /// The open page. `None`: the grid.
    pub page: Option<ToolPage>,
    /// The text of the search field of the grid.
    pub search: String,
}

/// The key, the title, and the name of the back target of the open page.
pub(super) fn head(page: Option<ToolPage>) -> (&'static str, &'static str, Option<&'static str>) {
    match page {
        None => ("pad-toolbox", TOOLBOX, None),
        Some(ToolPage::Orient) => ("pad-orient", ORIENT_DOWNHILL, Some(TOOLBOX)),
        Some(ToolPage::OrientLines) => ("pad-orient-lines", LINES, Some(ORIENT_DOWNHILL)),
        Some(ToolPage::OrientHeight) => ("pad-orient-height", HEIGHT_LAYER, Some(ORIENT_DOWNHILL)),
        Some(ToolPage::Polygonize) => ("pad-polygonize", POLYGONIZE, Some(TOOLBOX)),
        Some(ToolPage::PolygonizeInput) => ("pad-polygonize-input", INPUT, Some(POLYGONIZE)),
        Some(ToolPage::PolygonizeLevels) => ("pad-polygonize-levels", LEVELS, Some(POLYGONIZE)),
        Some(ToolPage::PolygonizeCollection) => (
            "pad-polygonize-collection",
            collection_choice::COLLECTION,
            Some(POLYGONIZE),
        ),
    }
}

/// Opens the page before the open page.
pub(super) fn back(state: &mut AppState) {
    let page = &mut state.pad.toolbox.page;
    *page = match page {
        Some(ToolPage::OrientLines | ToolPage::OrientHeight) => Some(ToolPage::Orient),
        Some(
            ToolPage::PolygonizeInput | ToolPage::PolygonizeLevels | ToolPage::PolygonizeCollection,
        ) => Some(ToolPage::Polygonize),
        _ => None,
    };
}

/// The body of the Toolbox panel.
pub(super) fn body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    match state.pad.toolbox.page {
        None => grid_body(ui, state, controls),
        Some(ToolPage::Orient) => orient_body(ui, state, controls),
        Some(ToolPage::OrientLines) => lines_body(ui, state, controls),
        Some(ToolPage::OrientHeight) => height_body(ui, state, controls),
        Some(ToolPage::Polygonize) => polygonize_body(ui, state, controls),
        Some(ToolPage::PolygonizeInput) => input_body(ui, state, controls),
        Some(ToolPage::PolygonizeLevels) => levels_body(ui, state, controls),
        Some(ToolPage::PolygonizeCollection) => collection_body(ui, state, controls),
    }
}

/// The operations whose names hold `search`, by group. A group with no
/// such operation is left out.
pub fn matches(search: &str) -> Vec<(&'static str, Vec<(&'static str, Icon)>)> {
    let needle = search.trim().to_lowercase();
    (GROUPS.iter())
        .map(|&(group, ops)| {
            let ops = (ops.iter())
                .filter(|(name, _)| name.to_lowercase().contains(&needle))
                .copied()
                .collect::<Vec<_>>();
            (group, ops)
        })
        .filter(|(_, ops)| !ops.is_empty())
        .collect()
}

fn grid_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    filter::search_field(ui, controls, SEARCH, &mut state.pad.toolbox.search);
    let groups = matches(&state.pad.toolbox.search);
    if groups.is_empty() {
        ui.label(egui::RichText::new(NO_MATCH).color(theme::MUTE));
    }
    for (group, ops) in groups {
        widgets::list_heading(ui, group);
        let page = match tiles(ui, controls, &ops) {
            Some(ORIENT_DOWNHILL) => Some(ToolPage::Orient),
            Some(POLYGONIZE) => Some(ToolPage::Polygonize),
            _ => None,
        };
        if page.is_some() {
            state.pad.toolbox.page = page;
            state.toolbox_note = None;
        }
    }
    ui.add_space(16.0);
    // Temporary controls, for the tests of the brush on the device.
    ui.checkbox(&mut state.globe.debug, "Debug");
    if state.globe.debug {
        // The frame time needs a new frame at each refresh of the screen.
        if !state.headless {
            ui.ctx().request_repaint();
        }
        panels::debug_controls(ui, state);
    }
}

/// The number of tiles in a row of the grid, and the gap between them.
const COLUMNS: usize = 3;
const GAP: f32 = 8.0;
/// The size of the icon of a tile.
const TILE_ICON: f32 = 32.0;

/// The square tiles of the operations `ops`, `COLUMNS` to a row. Returns
/// the name of the tapped operation.
fn tiles(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    ops: &[(&'static str, Icon)],
) -> Option<&'static str> {
    let width = ui.available_width();
    let side = ((width - GAP * (COLUMNS - 1) as f32) / COLUMNS as f32).floor();
    let rows = ops.len().div_ceil(COLUMNS);
    let height = rows as f32 * side + rows.saturating_sub(1) as f32 * GAP;
    let (area, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let mut tapped = None;
    for (index, &(name, icon)) in ops.iter().enumerate() {
        let (row, column) = (index / COLUMNS, index % COLUMNS);
        let min = area.min + vec2(column as f32, row as f32) * (side + GAP);
        let rect = Rect::from_min_size(min, vec2(side, side));
        let ready = exists(name);
        let resp = widgets::hit(ui, controls, rect, name, Sense::click(), ready, None);
        let start = widgets::press_start(ui);
        let fill = match resp.is_pointer_button_down_on() {
            true => egui::Color32::from_white_alpha(52),
            false => theme::raise(),
        };
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            theme::CONTROL_RADIUS + 4.0,
            widgets::fade(fill, ready),
        );
        let text = theme::body();
        let job = name_job(
            name,
            text.clone(),
            widgets::fade(theme::TEXT, ready),
            side - 12.0,
        );
        let galley = painter.layout_job(job);
        let lines = galley.rows.len().max(1) as f32;
        // The icon and the name sit together in the middle of the tile.
        let line = ui.fonts_mut(|f| f.row_height(&text));
        let block = TILE_ICON + 10.0 + lines * line;
        let top = rect.center().y - block / 2.0;
        let square = Rect::from_center_size(
            pos2(rect.center().x, top + TILE_ICON / 2.0),
            vec2(TILE_ICON, TILE_ICON),
        );
        let color = widgets::fade(theme::ACCENT, ready);
        icons::paint(painter, square, icon, TILE_ICON, color);
        let at = pos2(rect.center().x, top + TILE_ICON + 10.0);
        painter.galley(at, galley, theme::TEXT);
        widgets::press_end(ui, &resp, start);
        if resp.clicked() {
            tapped = Some(name);
        }
    }
    tapped
}

/// The name of a tile, centered, in lines of `wrap` points at most.
fn name_job(
    name: &str,
    font: egui::FontId,
    color: egui::Color32,
    wrap: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple(name.to_owned(), font, color, wrap);
    job.halign = egui::Align::Center;
    job
}

/// The size of the square of the icon at the top of the page of an
/// operation.
const HERO_ICON: f32 = 56.0;

/// The icon and the text of an operation at the top of its page.
fn hero(ui: &mut egui::Ui, icon: Icon, about: &str) {
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        let (square, _) = ui.allocate_exact_size(vec2(HERO_ICON, HERO_ICON), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(square, theme::CONTROL_RADIUS + 2.0, theme::raise());
        icons::paint(painter, square, icon, 32.0, theme::ACCENT);
        ui.add_space(6.0);
        let about = egui::RichText::new(about).color(theme::MUTE);
        let height = vec2(ui.available_width(), HERO_ICON);
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        ui.allocate_ui_with_layout(height, layout, |ui| {
            ui.add(egui::Label::new(about).selectable(false).wrap());
        });
    });
    ui.add_space(8.0);
}

fn orient_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    hero(ui, Icon::Orient, ORIENT_ABOUT);
    let lines = state.orient_lines();
    let height = state.orient_height();
    let line_name = (state.line_layers().into_iter())
        .find(|(id, _)| Some(*id) == lines)
        .map_or(NONE.to_owned(), |(_, name)| name);
    let height_name = (state.held_heights().into_iter())
        .find(|(id, _)| Some(*id) == height)
        .map_or(NONE.to_owned(), |(_, name)| name);
    ui.spacing_mut().item_spacing.y = 0.0;
    if widgets::open_row(ui, controls, LINES, &line_name).clicked() {
        state.pad.toolbox.page = Some(ToolPage::OrientLines);
    }
    if widgets::open_row(ui, controls, HEIGHT_LAYER, &height_name).clicked() {
        state.pad.toolbox.page = Some(ToolPage::OrientHeight);
    }
    let selected = lines.map_or(0, |id| state.selected_in(id));
    widgets::detail_row(ui, FEATURES, &orient::features_text(selected));
    output_row(ui, controls);
    ui.add_space(12.0);
    if widgets::button_row(ui, controls, RUN, state.can_orient_downhill()).clicked() {
        state.run_orient_downhill();
    }
    ui.add_space(8.0);
    let note = state.orient_hint().or(state.toolbox_note.as_deref());
    if let Some(note) = note {
        ui.label(egui::RichText::new(note).color(theme::MUTE));
    }
}

/// The row of the output of an operation: a new layer, which no operation
/// makes yet, or an edit of the input in place.
fn output_row(ui: &mut egui::Ui, controls: &mut Controls) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let at = pos2(row.left(), row.center().y);
    (ui.painter()).text(at, Align2::LEFT_CENTER, OUTPUT, theme::body(), theme::TEXT);
    let choices = [NEW_LAYER, EDIT_IN_PLACE];
    let widest = (choices.iter())
        .map(|c| ui.fonts_mut(|f| f.layout_no_wrap(c.to_string(), theme::body(), theme::TEXT)))
        .map(|g| g.size().x)
        .fold(0.0, f32::max);
    let width = (2.0 * (widest + 28.0)).min(row.width() - 80.0);
    let field = Rect::from_min_max(
        pos2(row.right() - width, row.center().y - TOUCH / 2.0),
        pos2(row.right(), row.center().y + TOUCH / 2.0),
    );
    widgets::segmented_at(ui, controls, field, &choices, Some(1), |i| i == 1);
}

fn polygonize_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    hero(ui, Icon::Polygonize, POLYGONIZE_ABOUT);
    let input = state.polygonize_input();
    let input_name = (state.held_rasters().into_iter())
        .find(|(id, _)| Some(*id) == input)
        .map_or(NONE.to_owned(), |(_, name)| name);
    ui.spacing_mut().item_spacing.y = 0.0;
    if widgets::open_row(ui, controls, INPUT, &input_name).clicked() {
        state.pad.toolbox.page = Some(ToolPage::PolygonizeInput);
    }
    let levels = levels_text(state);
    if widgets::open_row(ui, controls, LEVELS, &levels).clicked() {
        state.pad.toolbox.page = Some(ToolPage::PolygonizeLevels);
    }
    let smooth = Number::new(SMOOTH, state.polygonize.smooth, 0.0..=1.0)
        .shown(100.0, "%")
        .step(0.01)
        .slider(ui, controls);
    if let Some(smooth) = smooth {
        state.polygonize.smooth = smooth;
    }
    // The tolerance is in texels, and it shows as a distance on the ground.
    let texel = state.polygonize_texel_m().unwrap_or(1.0);
    let simplify = Number::new(SIMPLIFY, state.polygonize.simplify, SIMPLIFY_RANGE)
        .log()
        .text(move |px| polygonize::distance_text(px * texel))
        .slider(ui, controls);
    if let Some(simplify) = simplify {
        state.polygonize.simplify = simplify;
    }
    widgets::list_heading(ui, OUTPUT);
    if state.polygonize_can_split() {
        widgets::switch_row(ui, controls, SPLIT, SPLIT, &mut state.polygonize.split);
        if state.polygonize.split {
            let p = &mut state.polygonize;
            widgets::text_field(ui, controls, ABOVE, ABOVE, None, &mut p.above, true);
            widgets::text_field(ui, controls, BELOW, BELOW, None, &mut p.below, true);
        }
    }
    let stale = state.polygonize_stale();
    if let Some(names) = &stale {
        let text = polygonize::stale_text(names);
        ui.add_space(8.0);
        ui.label(egui::RichText::new(text).color(theme::MUTE));
    }
    let choice = state.polygonize.collection.clone();
    if collection_choice::row(ui, state, controls, choice.as_ref()) {
        state.pad.toolbox.page = Some(ToolPage::PolygonizeCollection);
    }
    ui.add_space(12.0);
    if let Some((share, text)) = state.polygonize_progress() {
        progress_bar(ui, share);
        ui.add_space(6.0);
        ui.label(egui::RichText::new(text).color(theme::MUTE));
        ui.add_space(8.0);
        if widgets::button_row(ui, controls, CANCEL, true).clicked() {
            state.cancel_polygonize();
        }
        return;
    }
    if widgets::button_row(ui, controls, RUN, state.can_polygonize()).clicked() {
        state.run_polygonize();
    }
    ui.add_space(8.0);
    let taken = !state.collection_choice_is_valid(choice.as_ref());
    let note = match input {
        None => Some(NEEDS_RASTER),
        Some(_) if taken => Some(COLLECTION_NAME_TAKEN),
        Some(_) => state.toolbox_note.as_deref().filter(|_| stale.is_none()),
    };
    if let Some(note) = note {
        ui.label(egui::RichText::new(note).color(theme::MUTE));
    }
}

/// The picker of the collection of the output layers of "Polygonize".
fn collection_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let default = state.default_layer_collection();
    let picked = collection_choice::picker(
        ui,
        &state.doc.project,
        default,
        controls,
        &mut state.pad.collection_picker,
        &mut state.polygonize.collection,
    );
    if picked {
        state.pad.toolbox.page = Some(ToolPage::Polygonize);
    }
}

/// The height of the bar of the progress of a run.
const PROGRESS_HEIGHT: f32 = 6.0;

/// A bar that fills from the left with the share of the work that is done.
fn progress_bar(ui: &mut egui::Ui, share: f32) {
    let size = vec2(ui.available_width(), PROGRESS_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let radius = PROGRESS_HEIGHT / 2.0;
    ui.painter().rect_filled(rect, radius, theme::raise());
    let mut fill = rect;
    fill.set_width(rect.width() * share.clamp(0.0, 1.0));
    ui.painter().rect_filled(fill, radius, theme::ACCENT);
}

/// The value of the Levels row.
fn levels_text(state: &mut AppState) -> String {
    match state.polygonize.levels {
        Levels::Stops => COLOR_STOPS.to_owned(),
        Levels::Interval { step, .. } => {
            let unit = state.polygonize_unit();
            format!("Every {}{unit}", crate::color_key::number_text(step))
        }
    }
}

fn levels_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(8.0);
    let stops = state.polygonize.levels == Levels::Stops;
    if widgets::option_row(ui, controls, COLOR_STOPS, COLOR_STOPS, &[], stops).clicked() {
        state.polygonize.levels = Levels::Stops;
    }
    if widgets::option_row(ui, controls, INTERVAL, INTERVAL, &[], !stops).clicked() && stops {
        state.polygonize.levels = state.default_interval();
    }
    let range = state.polygonize_range();
    if let (Levels::Interval { step, base }, Some((min, max))) = (state.polygonize.levels, range) {
        let unit = state.polygonize_unit();
        let span = (max - min).max(1.0);
        let new_step = Number::new(INTERVAL, step, span / 1000.0..=span)
            .nice()
            .unit(unit.trim())
            .stepper(ui, controls);
        let new_base = Number::new(BASE, base, min.min(0.0)..=max.max(0.0))
            .step(step)
            .unit(unit.trim())
            .stepper(ui, controls);
        if new_step.is_some() || new_base.is_some() {
            state.polygonize.levels = Levels::Interval {
                step: new_step.unwrap_or(step),
                base: new_base.unwrap_or(base),
            };
        }
    }
    ui.add_space(8.0);
    let note = match state.polygonize_bands() {
        Some(bands) if bands.len() == 1 => "1 band.".to_owned(),
        Some(bands) => format!("{} bands.", bands.len()),
        None if state.polygonize_input().is_some() => TOO_MANY_BANDS.to_owned(),
        None => NEEDS_RASTER.to_owned(),
    };
    ui.label(egui::RichText::new(note).color(theme::MUTE));
}

fn input_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let current = state.polygonize_input();
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(8.0);
    let layers = state.held_rasters();
    if layers.is_empty() {
        ui.label(egui::RichText::new(NEEDS_RASTER).color(theme::MUTE));
    }
    for (id, name) in layers {
        let on = current == Some(id);
        if widgets::option_row(ui, controls, &name, &name, &[], on).clicked() && !on {
            state.polygonize.raster = Some(id);
            state.toolbox_note = None;
        }
    }
}

fn lines_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let current = state.orient_lines();
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(8.0);
    let layers = state.line_layers();
    if layers.is_empty() {
        ui.label(egui::RichText::new(orient::NEEDS_LINES).color(theme::MUTE));
    }
    for (id, name) in layers {
        let on = current == Some(id);
        if widgets::option_row(ui, controls, &name, &name, &[], on).clicked() && !on {
            state.orient.lines = Some(id);
            state.toolbox_note = None;
        }
    }
}

fn height_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let current = state.orient_height();
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(8.0);
    let layers = state.held_heights();
    if layers.is_empty() {
        ui.label(egui::RichText::new(orient::NEEDS_HEIGHT).color(theme::MUTE));
    }
    for (id, name) in layers {
        let on = current == Some(id);
        if widgets::option_row(ui, controls, &name, &name, &[], on).clicked() && !on {
            state.orient.height = Some(id);
            state.toolbox_note = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_keeps_the_groups_with_a_match() {
        assert_eq!(matches("").len(), 3);
        let found = matches(" ORIENT ");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "Lines");
        assert_eq!(found[0].1, [(ORIENT_DOWNHILL, Icon::Orient)]);
        assert!(matches("xyz").is_empty());
    }
}
