//! The top bar of the iPad layout from 600 points: Back, the two workspace
//! buttons, Undo, and Redo at the left, the name of the world at the center
//! of the canvas, and the panel buttons and the Dock button in the dock zone.

use eframe::egui::{self, Rect, Sense, pos2, vec2};

use super::geometry::{DIVIDER, NAME_MIN, Rects, TOUCH};
use super::icons::{self, Icon};
use super::widgets::{self, Controls};
use super::{WORKSPACES, WORLDS, save_mark, theme, toggle_worlds};
use crate::ui::{Action, AppState, Workspace};

/// The button that opens the start screen.
pub const BACK: &str = "Back";
/// The button that opens and closes the dock.
pub const DOCK: &str = "Dock";

/// The space at each end of the name button of the world.
const NAME_PAD: f32 = 8.0;
/// The side of the chevron of the name button.
const CHEVRON: f32 = 14.0;
/// The side of the save mark.
const MARK: f32 = 14.0;

fn text_width(ui: &egui::Ui, text: &str) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::body(), theme::TEXT);
    galley.size().x.ceil()
}

/// The space of the save mark at each side of the name and the chevron, so
/// that they stay at the center of the button.
const MARK_SPACE: f32 = 8.0 + MARK;

/// The width of the name button with the full name of the world.
pub fn name_width(ui: &egui::Ui, state: &AppState) -> f32 {
    let text = text_width(ui, &state.doc.project.world.name);
    (2.0 * (NAME_PAD + MARK_SPACE) + text + 4.0 + CHEVRON).max(NAME_MIN)
}

/// The text that fits in `width`, with an ellipsis if it is shorter.
pub(super) fn fit_text(ui: &egui::Ui, text: &str, width: f32) -> String {
    if text_width(ui, text) <= width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for n in (0..chars.len()).rev() {
        let start: String = chars[..n].iter().collect();
        let short = format!("{}…", start.trim_end());
        if text_width(ui, &short) <= width {
            return short;
        }
    }
    "…".to_owned()
}

/// Draws the band and the bar. `panel_buttons` draws the panel buttons of
/// the workspace at the given places.
pub fn draw(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    panel_buttons: impl FnOnce(&egui::Ui, &mut AppState, &mut Controls, &[Rect]),
) {
    let area = rects.band.union(rects.bar);
    let edge = [
        pos2(area.left(), area.bottom() - 0.5),
        pos2(area.right(), area.bottom() - 0.5),
    ];
    widgets::docked_area(ctx, "pad-bar", area, Some(edge), |ui| {
        let workspace = state.workspace;
        let open = state.pad.docks.get(workspace).open && rects.dock.is_some();
        if open {
            let zone = rects.dock_zone;
            let line = [
                pos2(zone.left() + 0.5, zone.top()),
                pos2(zone.left() + 0.5, zone.bottom()),
            ];
            ui.painter()
                .line_segment(line, egui::Stroke::new(1.0, theme::edge()));
        }

        let can = state.projects.any();
        if widgets::icon_button_at(ui, controls, rects.back, BACK, Icon::Back, false, can).clicked()
        {
            state.pad.worlds = None;
            state.actions.push(Action::ShowProjects);
        }
        workspace_buttons(ui, state, controls, rects);
        name_button(ui, state, controls, rects);
        let line = Rect::from_center_size(
            pos2(rects.undo.left() - DIVIDER / 2.0, rects.bar.center().y),
            vec2(1.0, 20.0),
        );
        ui.painter().rect_filled(line, 0.0, theme::divider());
        let can = state.can_undo();
        if widgets::icon_button_at(ui, controls, rects.undo, "Undo", Icon::Undo, false, can)
            .clicked()
        {
            state.undo();
        }
        let can = state.can_redo();
        if widgets::icon_button_at(ui, controls, rects.redo, "Redo", Icon::Redo, false, can)
            .clicked()
        {
            state.redo();
        }

        let actions = rects.actions;
        let dock = rects.dock_button.map_or(0.0, |_| DIVIDER + TOUCH);
        let count = ((actions.width() - dock) / TOUCH).round().max(0.0) as usize;
        let buttons: Vec<Rect> = (0..count)
            .map(|i| {
                let min = pos2(actions.left() + i as f32 * TOUCH, actions.top());
                Rect::from_min_size(min, vec2(TOUCH, actions.height()))
            })
            .collect();
        panel_buttons(ui, state, controls, &buttons);
        if let Some(button) = rects.dock_button {
            let line = Rect::from_center_size(
                pos2(button.left() - DIVIDER / 2.0, rects.bar.center().y),
                vec2(1.0, 20.0),
            );
            ui.painter().rect_filled(line, 0.0, theme::divider());
            if open {
                let fill = button.shrink(widgets::HIGHLIGHT_INSET);
                ui.painter().rect_filled(
                    fill,
                    theme::CONTROL_RADIUS,
                    theme::ACCENT.gamma_multiply(0.2),
                );
            }
            let resp = widgets::icon_button_at(ui, controls, button, DOCK, Icon::Dock, false, true);
            if open {
                icons::paint(
                    ui.painter(),
                    button,
                    Icon::Dock,
                    widgets::ICON,
                    theme::ACCENT,
                );
            }
            if resp.clicked() {
                state.pad.toggle_dock();
            }
        }
    });
}

/// The name of the world and a small chevron at the center of the button,
/// and the save mark at their right. A tap opens the menu of the worlds.
fn name_button(ui: &egui::Ui, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let rect = rects.workspace;
    let open = state.pad.worlds.is_some();
    let resp = widgets::hit(ui, controls, rect, WORLDS, Sense::click(), true, Some(open));
    if open || resp.is_pointer_button_down_on() {
        let fill = rect.shrink(widgets::HIGHLIGHT_INSET);
        ui.painter()
            .rect_filled(fill, theme::CONTROL_RADIUS, theme::raise());
    }
    let room = rect.width() - 2.0 * (NAME_PAD + MARK_SPACE) - 4.0 - CHEVRON;
    let text = fit_text(ui, &state.doc.project.world.name, room);
    let galley = ui
        .painter()
        .layout_no_wrap(text, theme::body(), theme::TEXT);
    let width = galley.size().x;
    let at = pos2(
        rect.center().x - (width + 4.0 + CHEVRON) / 2.0,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(at, galley, theme::TEXT);
    let chevron = Rect::from_center_size(
        pos2(at.x + width + 4.0 + CHEVRON / 2.0, rect.center().y),
        vec2(CHEVRON, CHEVRON),
    );
    icons::paint(ui.painter(), chevron, Icon::Down, CHEVRON, theme::MUTE);
    if let Some(mark) = state.save_mark(std::time::Instant::now()) {
        let center = pos2(chevron.right() + 8.0 + MARK / 2.0, rect.center().y);
        let at = Rect::from_center_size(center, vec2(MARK, MARK));
        // A test has no clock, so there the mark does not turn.
        save_mark(ui, at, mark, !state.headless);
    }
    if resp.clicked() {
        toggle_worlds(state);
    }
}

/// The two workspace buttons. The button of the open workspace is raised.
fn workspace_buttons(ui: &egui::Ui, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let spaces = [Workspace::Globe, Workspace::Atlas];
    let faces = [Icon::World, Icon::Book];
    for i in 0..2 {
        let min = pos2(rects.tabs.left() + i as f32 * TOUCH, rects.tabs.top());
        let rect = Rect::from_min_size(min, vec2(TOUCH, rects.tabs.height()));
        let on = state.workspace == spaces[i];
        let resp = widgets::icon_button_at(ui, controls, rect, WORKSPACES[i], faces[i], on, true);
        if resp.clicked() && !on {
            state.workspace = spaces[i];
            state.pad.worlds = None;
            state.pad.menu = false;
        }
    }
}
