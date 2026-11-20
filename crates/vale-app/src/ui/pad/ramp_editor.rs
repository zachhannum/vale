//! The page of the editor of a ramp: its name, the qualitative switch, the
//! sea level switch, and a row for each color with its label.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};

use super::geometry::TOUCH;
use super::icons::Icon;
use super::theme;
use super::widgets::{self, Controls, PANEL_ROW};
use super::{Page, raster};
use crate::ramps::DraftFrom;
use crate::ui::AppState;

/// The title of the page.
pub const NEW_RAMP_TITLE: &str = "New ramp";
/// The title of the page for a custom ramp.
pub const EDIT_RAMP_TITLE: &str = "Edit ramp";
pub const SEA_LEVEL: &str = "Sea level";
pub const SEA_LEVEL_UP: &str = "Move sea level up";
pub const SEA_LEVEL_DOWN: &str = "Move sea level down";
pub const REVERSE_COLORS: &str = "Reverse colors";
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
/// The color of the line of sea level.
const SEA_COLOR: egui::Color32 = egui::Color32::from_rgb(95, 176, 220);

/// The title of the page: "Edit ramp" for a custom ramp, else "New ramp".
pub fn title(state: &AppState) -> &'static str {
    match state.ramp_draft.as_ref().and_then(|d| d.editing) {
        Some(_) => EDIT_RAMP_TITLE,
        None => NEW_RAMP_TITLE,
    }
}

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

/// Closes the editor and saves nothing.
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
    SeaUp,
    SeaDown,
    Reverse,
}

/// The body of the page. A tap on a swatch selects its row, and the color
/// picker and the buttons of the row show below it. With a sea level, a
/// line between two colors shows it, with the buttons that move it. "Save
/// ramp" adds the ramp, or replaces the ramp that the draft edits, and
/// opens the page of the color ramps.
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
    // The colors of a qualitative ramp do not split at sea level.
    if !draft.qualitative {
        let mut sea = draft.sea.is_some();
        if widgets::switch_row(ui, controls, SEA_LEVEL, SEA_LEVEL, &mut sea) {
            draft.set_sea(sea);
        }
    }
    raster::note(ui, FIRST_COLOR_HINT);

    let count = draft.colors.len();
    draft.selected = draft.selected.filter(|i| *i < count);
    let mut change = None;
    let sea = draft.sea.filter(|_| !draft.qualitative);
    for i in 0..count {
        if sea == Some(i) {
            let (up, down) = sea_line(ui, controls, i > 1, i + 1 < count);
            if up {
                change = Some(Change::SeaUp);
            }
            if down {
                change = Some(Change::SeaDown);
            }
        }
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
    if widgets::text_row(ui, controls, REVERSE_COLORS, 0.0, false, true).clicked() {
        change = Some(Change::Reverse);
    }
    match change {
        Some(Change::Up(i)) => draft.move_up(i),
        Some(Change::Down(i)) => draft.move_down(i),
        Some(Change::Remove(i)) => draft.remove(i),
        Some(Change::Add) => draft.add_color(),
        Some(Change::SeaUp) => draft.sea_up(),
        Some(Change::SeaDown) => draft.sea_down(),
        Some(Change::Reverse) => {
            draft.reverse();
            true
        }
        None => false,
    };
    ui.add_space(12.0);
    let save = widgets::button_row(ui, controls, SAVE_RAMP, can_save).clicked();
    state.ramp_draft = Some(draft);
    if save && state.save_ramp_draft().is_ok() {
        state.pad.page = Some(Page::Ramps);
    }
}

/// The line of sea level between two colors, with the buttons that move it
/// up and down. Returns the taps on the two buttons.
fn sea_line(ui: &mut egui::Ui, controls: &mut Controls, up: bool, down: bool) -> (bool, bool) {
    let size = vec2(ui.available_width(), PANEL_ROW);
    let (row, _) = ui.allocate_exact_size(size, Sense::hover());
    let y = row.center().y;
    let gap = 8.0;
    let down_rect = Rect::from_min_size(
        pos2(row.right() - TOUCH, y - TOUCH / 2.0),
        vec2(TOUCH, TOUCH),
    );
    let up_rect = down_rect.translate(vec2(-TOUCH - gap, 0.0));
    let line_end = up_rect.left() - gap;
    let painter = ui.painter();
    let text = painter.layout_no_wrap(SEA_LEVEL.to_owned(), theme::body(), SEA_COLOR);
    let middle = (row.left() + line_end) / 2.0;
    let half = text.size().x / 2.0 + gap;
    let stroke = egui::Stroke::new(2.0, SEA_COLOR);
    painter.line_segment([pos2(row.left(), y), pos2(middle - half, y)], stroke);
    painter.line_segment([pos2(middle + half, y), pos2(line_end, y)], stroke);
    painter.galley(
        pos2(middle - half + gap, y - text.size().y / 2.0),
        text,
        SEA_COLOR,
    );
    let up = widgets::icon_button_at(ui, controls, up_rect, SEA_LEVEL_UP, Icon::Up, false, up);
    let down = widgets::icon_button_at(
        ui,
        controls,
        down_rect,
        SEA_LEVEL_DOWN,
        Icon::Down,
        false,
        down,
    );
    (up.clicked(), down.clicked())
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
