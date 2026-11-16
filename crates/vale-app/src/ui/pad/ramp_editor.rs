//! The page of the editor of a new ramp: its name, the qualitative switch,
//! and a row for each color with its label.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};

use super::theme;
use super::widgets::{self, Controls, PANEL_ROW};
use super::{Page, raster};
use crate::ramps::DraftFrom;
use crate::ui::AppState;

/// The title of the page.
pub const NEW_RAMP_TITLE: &str = "New ramp";
/// The name of the field of the name of the ramp.
pub const RAMP_NAME: &str = "Ramp name";
pub const QUALITATIVE: &str = "Qualitative";
pub const FIRST_COLOR_HINT: &str = "First color is the lowest band.";
pub const ADD_COLOR: &str = "Add color";
pub const MOVE_UP: &str = "Move up";
pub const MOVE_DOWN: &str = "Move down";
pub const REMOVE_COLOR: &str = "Remove color";
pub const SAVE_RAMP: &str = "Save ramp";
/// The name of the color picker of the selected color.
pub const PICKED_COLOR: &str = "Ramp color";

/// The width of the tap area of a swatch, at the left of its label field.
const SWATCH_AREA: f32 = 80.0;

/// The name of the swatch of color `index`, from 1.
pub fn color_name(index: usize) -> String {
    format!("Color {}", index + 1)
}

/// The name of the label field of color `index`, from 1.
pub fn label_name(index: usize) -> String {
    format!("Label {}", index + 1)
}

/// The page that Back goes to, and the title of its button.
pub fn back_page(state: &AppState) -> (Page, &'static str) {
    match state.ramp_draft.as_ref().map(|d| d.from) {
        Some(DraftFrom::Stops) => (Page::Stops, raster::COLOR_STOPS),
        _ => (Page::Ramps, raster::COLOR_RAMPS),
    }
}

/// Closes the editor with no new ramp.
pub fn cancel(state: &mut AppState) {
    state.pad.page = Some(back_page(state).0);
    state.ramp_draft = None;
}

/// A change to the rows of the colors.
enum Change {
    Up(usize),
    Down(usize),
    Remove(usize),
    Add,
}

/// The body of the page. A tap on a swatch selects its row, and the color
/// picker and the buttons of the row show below it. "Save ramp" adds the
/// ramp and opens the page of the color ramps.
pub fn body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    state.open_ramps();
    let Some(mut draft) = state.ramp_draft.take() else {
        return;
    };
    let layer = draft.layer;
    ui.spacing_mut().item_spacing.y = 0.0;
    if state.ramp_note.as_ref().is_some_and(|n| n.warning) {
        super::note_text(ui, None, &mut state.ramp_note, layer);
    }
    let can_save = draft.can_save(&state.ramps);
    widgets::text_field(
        ui,
        controls,
        RAMP_NAME,
        "Name",
        None,
        &mut draft.name,
        can_save,
    );
    let mut qualitative = draft.qualitative;
    widgets::switch_row(ui, controls, QUALITATIVE, QUALITATIVE, &mut qualitative);
    draft.qualitative = qualitative;
    raster::note(ui, FIRST_COLOR_HINT);

    let count = draft.colors.len();
    draft.selected = draft.selected.filter(|i| *i < count);
    let mut change = None;
    for i in 0..count {
        let top = ui.cursor().top();
        let left = ui.max_rect().left();
        widgets::text_field(
            ui,
            controls,
            &label_name(i),
            "",
            None,
            &mut draft.labels[i],
            true,
        );
        let bottom = ui.cursor().top();
        let area = Rect::from_x_y_ranges(left..=left + SWATCH_AREA, top..=bottom);
        let selected = draft.selected == Some(i);
        if widgets::tap_area(ui, controls, area, &color_name(i), true).clicked() {
            draft.selected = (!selected).then_some(i);
        }
        let swatch = Rect::from_center_size(pos2(left + 28.0, area.center().y), vec2(40.0, 28.0));
        let [r, g, b] = draft.colors[i];
        let painter = ui.painter();
        painter.rect_filled(swatch, 6.0, egui::Color32::from_rgb(r, g, b));
        let (width, stroke) = if selected {
            (2.0, theme::ACCENT)
        } else {
            (1.0, theme::MUTE.gamma_multiply(0.5))
        };
        let outline = egui::Stroke::new(width, stroke);
        painter.rect_stroke(swatch, 6.0, outline, egui::StrokeKind::Outside);
        if draft.selected != Some(i) {
            continue;
        }
        ui.add_space(4.0);
        widgets::color_picker(ui, controls, PICKED_COLOR, &mut draft.colors[i]);
        ui.add_space(4.0);
        if widgets::text_row(ui, controls, MOVE_UP, 0.0, false, i > 0).clicked() {
            change = Some(Change::Up(i));
        }
        let last = i + 1 < count;
        if widgets::text_row(ui, controls, MOVE_DOWN, 0.0, false, last).clicked() {
            change = Some(Change::Down(i));
        }
        if warn_row(ui, controls, REMOVE_COLOR, draft.can_remove()).clicked() {
            change = Some(Change::Remove(i));
        }
    }
    if widgets::text_row(ui, controls, ADD_COLOR, 0.0, false, draft.can_add()).clicked() {
        change = Some(Change::Add);
    }
    match change {
        Some(Change::Up(i)) => draft.move_up(i),
        Some(Change::Down(i)) => draft.move_down(i),
        Some(Change::Remove(i)) => draft.remove(i),
        Some(Change::Add) => draft.add_color(),
        None => false,
    };
    ui.add_space(12.0);
    let save = widgets::button_row(ui, controls, SAVE_RAMP, can_save).clicked();
    state.ramp_draft = Some(draft);
    if save && state.save_ramp_draft().is_ok() {
        state.pad.page = Some(Page::Ramps);
    }
}

/// A row with a text in the warning color, for a button that removes.
fn warn_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    enabled: bool,
) -> egui::Response {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let resp = widgets::tap_area(ui, controls, row, name, enabled);
    if resp.is_pointer_button_down_on() {
        let back = row.shrink(widgets::HIGHLIGHT_INSET);
        ui.painter()
            .rect_filled(back, theme::CONTROL_RADIUS, theme::raise());
    }
    let at = pos2(row.left(), row.center().y);
    let color = widgets::fade(theme::WARN, enabled);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, name, theme::body(), color);
    resp
}
