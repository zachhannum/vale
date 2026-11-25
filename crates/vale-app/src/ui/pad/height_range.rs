//! The elevation range of a height layer: the row of the panel, the page
//! that changes the range, and the section of the desktop panel.

use eframe::egui;
use vale_store::RasterId;
use vale_terrain::{Elevation, RangeMode};

use super::theme;
use super::widgets::{self, Controls};
use super::{Page, raster};
use crate::ui::raster_form::{HIGHEST, LEVEL_STEP, LOWEST, level_step_text};
use crate::ui::{AppState, HEIGHT_UNIT, RANGE_NOTE};
use crate::world;

/// The row of the panel that opens the page, and the title of the page.
pub const RANGE: &str = "Elevation range";
/// The fields of the page.
pub const LOWEST_FIELD: &str = "Lowest elevation";
pub const HIGHEST_FIELD: &str = "Highest elevation";
/// The choices of the mode, Keep elevations first.
pub const MODES: [&str; 2] = ["Keep elevations", "Stretch terrain"];
pub const KEEP_HINT: &str =
    "Each place keeps its elevation, and a place outside the range moves to the nearest end.";
pub const STRETCH_HINT: &str =
    "The relief stretches to fit the range, and each place keeps its color.";
/// The warning before a change that has no undo step.
pub const TOO_LARGE: &str = "This change is too large to undo.";
pub const APPLY: &str = "Apply range";

/// The edit of the range of a height layer on the page.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeEdit {
    pub id: RasterId,
    /// The lowest and the highest elevation, as the user typed them.
    pub min: String,
    pub max: String,
    pub mode: RangeMode,
    /// The range of the layer when the edit started. After an undo or a
    /// change of the range, the fields start again from the layer.
    from: (f64, f64),
    /// The count of the saves of the project file and the result of
    /// `AppState::range_change_too_large` after it.
    too_large: Option<(u64, bool)>,
}

impl RangeEdit {
    /// The edit of the range that the layer has.
    fn of(state: &AppState, id: RasterId) -> Option<RangeEdit> {
        let raster = state.doc.project.raster(id)?;
        Some(RangeEdit {
            id,
            min: format!("{}", raster.value_min),
            max: format!("{}", raster.value_max),
            mode: RangeMode::Keep,
            from: (raster.value_min, raster.value_max),
            too_large: None,
        })
    }

    /// The range of the fields. `None`: a field is not a number, or the
    /// range does not hold sea level.
    pub fn range(&self) -> Option<Elevation> {
        let number = |text: &str| text.trim().replace(',', "").parse::<f64>().ok();
        Elevation::new(number(&self.min)?, number(&self.max)?)
    }
}

/// The values of one frame of the page or of the desktop section.
struct View {
    range: Option<Elevation>,
    /// The fields have the range of the layer.
    same: bool,
    too_large: bool,
}

/// Takes the edit of the layer from the state, or a new one.
fn take_edit(state: &mut AppState, id: RasterId) -> Option<RangeEdit> {
    let now = state
        .doc
        .project
        .raster(id)
        .map(|r| (r.value_min, r.value_max));
    match state.pad.range.take() {
        Some(edit) if edit.id == id && Some(edit.from) == now => Some(edit),
        Some(edit) if edit.id == id => RangeEdit::of(state, id).map(|new| RangeEdit {
            mode: edit.mode,
            ..new
        }),
        _ => RangeEdit::of(state, id),
    }
}

fn view(state: &AppState, edit: &mut RangeEdit) -> View {
    let range = edit.range();
    let raster = state.doc.project.raster(edit.id);
    let same = match (range, raster) {
        (Some(range), Some(raster)) => {
            (range.min, range.max) == (raster.value_min, raster.value_max)
        }
        _ => false,
    };
    let saves = state.session.as_ref().map_or(0, |s| s.saves());
    let large = match edit.too_large {
        Some((at, large)) if at == saves => large,
        _ => {
            let large = state.range_change_too_large(edit.id);
            edit.too_large = Some((saves, large));
            large
        }
    };
    View {
        range,
        same,
        too_large: large && edit.mode == RangeMode::Keep,
    }
}

/// Changes the range of the layer to the range of the edit. A new edit
/// then starts from the new range.
fn apply(state: &mut AppState, edit: &RangeEdit, range: Elevation) {
    match state.set_raster_range(edit.id, range, edit.mode) {
        Ok(_) => state.status.clear(),
        Err(text) => state.status = text,
    }
}

/// The range of a raster record as a text, for example "−6,000 to 6,000 m".
pub fn range_text(min: f64, max: f64, unit: &str) -> String {
    let span = max - min;
    format!(
        "{} to {}",
        world::value_text(min, span, ""),
        world::value_text(max, span, unit)
    )
}

/// The row of the panel of a height layer that opens the page.
pub fn open_row(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: RasterId) {
    let Some(raster) = state.doc.project.raster(id) else {
        return;
    };
    let text = range_text(raster.value_min, raster.value_max, &raster.unit);
    if widgets::open_row(ui, controls, RANGE, &text).clicked() {
        state.pad.range = RangeEdit::of(state, id);
        state.pad.page = Some(Page::Range);
    }
}

/// A small text across the panel in the warning color.
fn warning(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    let text = egui::RichText::new(text)
        .font(theme::small())
        .color(theme::WARN);
    ui.add(egui::Label::new(text).wrap());
    ui.add_space(6.0);
}

/// The page of the range: the two fields, the meters of one level, the
/// mode with its hint, the warning of a change with no undo step, and the
/// button that applies the change.
pub fn body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: RasterId) {
    let height = state
        .doc
        .project
        .raster(id)
        .is_some_and(crate::ui::is_height);
    let Some(mut edit) = take_edit(state, id).filter(|_| height) else {
        state.pad.page = None;
        return;
    };
    ui.spacing_mut().item_spacing.y = 0.0;
    let valid = edit.range().is_some();
    let unit = Some(HEIGHT_UNIT);
    for (name, label, text) in [
        (LOWEST_FIELD, LOWEST, &mut edit.min),
        (HIGHEST_FIELD, HIGHEST, &mut edit.max),
    ] {
        widgets::text_field(ui, controls, name, label, unit, text, valid);
    }
    let range = edit.range();
    let step = range.map_or_else(|| "–".to_string(), level_step_text);
    widgets::value_row(ui, LEVEL_STEP, &step);
    ui.add_space(10.0);
    let active = usize::from(edit.mode == RangeMode::Stretch);
    if let Some(index) = widgets::segmented(ui, controls, &MODES, Some(active), true) {
        edit.mode = [RangeMode::Keep, RangeMode::Stretch][index];
    }
    let view = view(state, &mut edit);
    raster::note(ui, hint(edit.mode));
    if view.range.is_none() {
        warning(ui, RANGE_NOTE);
    } else if view.too_large && !view.same {
        warning(ui, TOO_LARGE);
    }
    let enabled = view.range.is_some() && !view.same;
    let applied = widgets::button_row(ui, controls, APPLY, enabled).clicked();
    if let Some(range) = view.range.filter(|_| applied && enabled) {
        apply(state, &edit, range);
        edit.too_large = None;
    }
    state.pad.range = Some(edit);
}

/// The hint of a mode.
pub fn hint(mode: RangeMode) -> &'static str {
    match mode {
        RangeMode::Keep => KEEP_HINT,
        RangeMode::Stretch => STRETCH_HINT,
    }
}

/// The section of the range in the desktop panel of a height layer.
pub fn desktop(ui: &mut egui::Ui, state: &mut AppState, id: RasterId) {
    let Some(mut edit) = take_edit(state, id) else {
        return;
    };
    egui::CollapsingHeader::new(RANGE)
        .id_salt(("desktop-range", id.0))
        .show(ui, |ui| {
            egui::Grid::new(("desktop-range-grid", id.0))
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    for (name, label, text) in [
                        (LOWEST_FIELD, LOWEST, &mut edit.min),
                        (HIGHEST_FIELD, HIGHEST, &mut edit.max),
                    ] {
                        ui.label(label);
                        ui.horizontal(|ui| {
                            let field = egui::TextEdit::singleline(text).desired_width(80.0);
                            let field = ui.add(field);
                            field.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name)
                            });
                            ui.label(HEIGHT_UNIT);
                        });
                        ui.end_row();
                    }
                    ui.label(LEVEL_STEP);
                    ui.label(
                        edit.range()
                            .map_or_else(|| "–".to_string(), level_step_text),
                    );
                    ui.end_row();
                });
            ui.horizontal(|ui| {
                for (index, name) in MODES.iter().enumerate() {
                    let mode = [RangeMode::Keep, RangeMode::Stretch][index];
                    if ui.selectable_label(edit.mode == mode, *name).clicked() {
                        edit.mode = mode;
                    }
                }
            });
            let view = view(state, &mut edit);
            ui.label(egui::RichText::new(hint(edit.mode)).weak());
            let warn = ui.visuals().warn_fg_color;
            if view.range.is_none() {
                ui.colored_label(warn, RANGE_NOTE);
            } else if view.too_large && !view.same {
                ui.colored_label(warn, TOO_LARGE);
            }
            let enabled = view.range.is_some() && !view.same;
            let applied = ui.add_enabled(enabled, egui::Button::new(APPLY)).clicked();
            if let Some(range) = view.range.filter(|_| applied) {
                apply(state, &edit, range);
                edit.too_large = None;
            }
        });
    state.pad.range = Some(edit);
}
