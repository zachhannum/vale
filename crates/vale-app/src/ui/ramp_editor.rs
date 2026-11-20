//! The window of the editor of a ramp in the desktop layout.

use eframe::egui;

use super::pad::ramp_editor::{
    ADD_COLOR, EDIT_RAMP_TITLE, FIRST_COLOR_HINT, MOVE_DOWN, MOVE_UP, NEW_RAMP_TITLE, QUALITATIVE,
    REMOVE_COLOR, REVERSE_COLORS, SAVE_RAMP, SEA_LEVEL, SEA_LEVEL_DOWN, SEA_LEVEL_UP, color_name,
    label_name,
};
use super::region::CANCEL;
use super::{AppState, Layout};

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

/// The window, while a ramp is in the editor. "Save ramp" adds the ramp, or
/// replaces the ramp that the draft edits, and closes the window. "Cancel"
/// closes it and saves nothing.
pub fn window(ctx: &egui::Context, state: &mut AppState) {
    if state.layout == Layout::Pad {
        return;
    }
    state.open_ramps();
    let Some(mut draft) = state.ramp_draft.take() else {
        return;
    };
    let can_save = draft.can_save(&state.ramps);
    let error = state
        .ramp_note
        .as_ref()
        .filter(|n| n.warning)
        .map(|n| n.text.clone());
    let (mut save, mut cancel) = (false, false);
    let mut change = None;
    let title = match draft.editing {
        Some(_) => EDIT_RAMP_TITLE,
        None => NEW_RAMP_TITLE,
    };
    egui::Window::new(title)
        .id(egui::Id::new("ramp-editor"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(320.0);
            if let Some(error) = &error {
                ui.colored_label(ui.visuals().warn_fg_color, error);
            }
            ui.horizontal(|ui| {
                let name = ui.label("Name");
                let mut edit =
                    egui::TextEdit::singleline(&mut draft.name).desired_width(f32::INFINITY);
                // A blank or taken name gets the warning color.
                if !can_save {
                    let warn = ui.visuals().warn_fg_color;
                    edit = edit.text_color(warn);
                    let widgets = &mut ui.visuals_mut().widgets;
                    widgets.inactive.bg_stroke = egui::Stroke::new(1.0, warn);
                    widgets.hovered.bg_stroke = egui::Stroke::new(1.0, warn);
                }
                ui.add(edit).labelled_by(name.id);
            });
            ui.checkbox(&mut draft.qualitative, QUALITATIVE);
            // The colors of a qualitative ramp do not split at sea level.
            let mut sea = draft.sea.is_some();
            if !draft.qualitative && ui.checkbox(&mut sea, SEA_LEVEL).changed() {
                draft.set_sea(sea);
            }
            ui.label(egui::RichText::new(FIRST_COLOR_HINT).weak());
            let sea = draft.sea.filter(|_| !draft.qualitative);
            let count = draft.colors.len();
            let can_remove = draft.can_remove();
            egui::ScrollArea::vertical()
                .max_height(400.0)
                .show(ui, |ui| {
                    for i in 0..count {
                        if sea == Some(i) {
                            ui.horizontal(|ui| {
                                ui.label(SEA_LEVEL);
                                let up = ui.add_enabled(i > 1, egui::Button::new("⏶").small());
                                if up.on_hover_text(SEA_LEVEL_UP).clicked() {
                                    change = Some(Change::SeaUp);
                                }
                                let down =
                                    ui.add_enabled(i + 1 < count, egui::Button::new("⏷").small());
                                if down.on_hover_text(SEA_LEVEL_DOWN).clicked() {
                                    change = Some(Change::SeaDown);
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            ui.color_edit_button_srgb(&mut draft.colors[i])
                                .on_hover_text(color_name(i));
                            let label = egui::TextEdit::singleline(&mut draft.labels[i])
                                .hint_text(label_name(i))
                                .desired_width(150.0);
                            ui.add(label);
                            let up = ui.add_enabled(i > 0, egui::Button::new("⏶").small());
                            if up.on_hover_text(MOVE_UP).clicked() {
                                change = Some(Change::Up(i));
                            }
                            let down =
                                ui.add_enabled(i + 1 < count, egui::Button::new("⏷").small());
                            if down.on_hover_text(MOVE_DOWN).clicked() {
                                change = Some(Change::Down(i));
                            }
                            let remove = ui.add_enabled(can_remove, egui::Button::new("×").small());
                            if remove.on_hover_text(REMOVE_COLOR).clicked() {
                                change = Some(Change::Remove(i));
                            }
                        });
                    }
                });
            if ui
                .add_enabled(draft.can_add(), egui::Button::new(ADD_COLOR))
                .clicked()
            {
                change = Some(Change::Add);
            }
            if ui.button(REVERSE_COLORS).clicked() {
                change = Some(Change::Reverse);
            }
            ui.separator();
            ui.horizontal(|ui| {
                save = ui
                    .add_enabled(can_save, egui::Button::new(SAVE_RAMP))
                    .clicked();
                cancel = ui.button(CANCEL).clicked();
            });
        });
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
    if cancel {
        return;
    }
    state.ramp_draft = Some(draft);
    if save {
        // A failure keeps the window open, with the error.
        let _ = state.save_ramp_draft();
    }
}
