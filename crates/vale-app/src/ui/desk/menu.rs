//! The menus. One model gives the menu bar of the system and the row of
//! menus at the top of the window.

use eframe::egui;

use super::command::{self, Command};
use super::theme;
use crate::projects;
use crate::ui::{Action, AppState};
use Item::{Command as C, Separator as S};

/// One row of a menu.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Command(Command),
    Separator,
    /// The menu of the recent worlds.
    Recent,
    Submenu(&'static str, &'static [Item]),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub name: &'static str,
    pub items: &'static [Item],
}

/// The name of the menu of the recent worlds.
pub const RECENT: &str = "Open recent";
/// The name of the menu of the new layers.
pub const ADD_LAYER: &str = "Add layer";

pub const MENUS: [Menu; 6] = [
    Menu {
        name: "File",
        items: &[
            C(Command::NewWorld),
            C(Command::Open),
            Item::Recent,
            S,
            C(Command::Save),
            C(Command::Rename),
            C(Command::Copy),
            C(Command::Reveal),
            C(Command::Delete),
            S,
            C(Command::WorldProperties),
            S,
            C(Command::ImportHeightmap),
            C(Command::ExportGeoTiff),
            S,
            C(Command::OpenGeoJson),
            C(Command::ExportPng),
            C(Command::ExportPdf),
            S,
            C(Command::AllWorlds),
            C(Command::Quit),
        ],
    },
    Menu {
        name: "Edit",
        items: &[C(Command::Undo), C(Command::Redo)],
    },
    Menu {
        name: "View",
        items: &[
            C(Command::World),
            C(Command::Maps),
            S,
            C(Command::Globe),
            C(Command::Flat),
            S,
            C(Command::Greyscale),
            C(Command::Elevation),
        ],
    },
    Menu {
        name: "Layer",
        items: &[
            Item::Submenu(
                ADD_LAYER,
                &[
                    C(Command::AddLines),
                    C(Command::AddPoints),
                    C(Command::AddPolygons),
                ],
            ),
            C(Command::DeleteLayer),
        ],
    },
    Menu {
        name: "Window",
        items: &[C(Command::Layers), C(Command::Toolbox), C(Command::Dock)],
    },
    Menu {
        name: "Help",
        items: &[C(Command::About)],
    },
];

/// The row of menus at the top of the window, when the system has no menu
/// bar for the app.
pub fn row(ui: &mut egui::Ui, state: &mut AppState) {
    if state.native_menu {
        return;
    }
    let frame = egui::Frame::NONE
        .fill(theme::glass(state.translucent))
        .inner_margin(egui::Margin::symmetric(6, 0));
    egui::Panel::top("desk-menu")
        .exact_size(theme::MENU)
        .frame(frame)
        .show_separator_line(false)
        .show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                for menu in MENUS {
                    ui.menu_button(menu.name, |ui| items(ui, state, menu.items));
                }
            });
        });
}

fn items(ui: &mut egui::Ui, state: &mut AppState, items: &[Item]) {
    let mut separator = false;
    for item in items {
        match *item {
            Item::Command(command) => {
                if !command.visible(state) {
                    continue;
                }
                if std::mem::take(&mut separator) {
                    ui.separator();
                }
                command_row(ui, state, command);
            }
            // A separator shows only between two rows.
            Item::Separator => separator = ui.min_rect().height() > 0.0,
            Item::Recent => {
                if state.projects.any() {
                    recent(ui, state);
                }
            }
            Item::Submenu(name, rows) => {
                let resp = ui.menu_button(name, |ui| self::items(ui, state, rows));
                name_submenu(&resp.response, name);
            }
        }
    }
}

fn command_row(ui: &mut egui::Ui, state: &mut AppState, command: Command) {
    let mut button = egui::Button::new(command.label());
    if let Some(on) = command.checked(state) {
        button = egui::Button::selectable(on, command.label());
    }
    if let Some(shortcut) = command.shortcut() {
        button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
    }
    let enabled = command.enabled(state);
    let resp = ui.add_enabled(enabled, button);
    // The name for the screen reader has no key.
    let checked = command.checked(state);
    resp.widget_info(|| match checked {
        Some(on) => {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, command.label())
        }
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, command.label()),
    });
    if resp.clicked() {
        command::run(command, ui.ctx(), state);
    }
}

fn recent(ui: &mut egui::Ui, state: &mut AppState) {
    let resp = ui.menu_button(RECENT, |ui| {
        let dir = state.projects.state_dir();
        let recent = dir.map(projects::recent).unwrap_or_default();
        if recent.is_empty() {
            ui.weak("No recent worlds");
        }
        for entry in recent.iter().map(|path| projects::entry(path)) {
            if ui.button(&entry.name).clicked() {
                state.actions.push(Action::OpenWorld(entry.path));
            }
        }
    });
    name_submenu(&resp.response, RECENT);
}

/// Gives the row of a submenu its name for the screen reader, with no arrow.
fn name_submenu(resp: &egui::Response, name: &str) {
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
}

/// Runs the command of a key that the menus show. The menu bar of the
/// system has these keys, so this does nothing with `native_menu`.
pub fn keys(ctx: &egui::Context, state: &mut AppState) {
    let dialog = state.world_form.is_some() || state.browser.dialog.is_some();
    if state.native_menu || dialog || ctx.egui_wants_keyboard_input() {
        return;
    }
    let mut commands: Vec<_> = Command::ALL
        .into_iter()
        .filter_map(|c| c.shortcut().map(|s| (c, s)))
        .collect();
    // A key without Shift or Option also matches the key with them, so the
    // keys with more modifiers come first.
    let extra = |s: &egui::KeyboardShortcut| s.modifiers.shift as u8 + s.modifiers.alt as u8;
    commands.sort_by_key(|(_, s)| std::cmp::Reverse(extra(s)));
    for (command, shortcut) in commands {
        if command.visible(state)
            && command.enabled(state)
            && ctx.input_mut(|i| i.consume_shortcut(&shortcut))
        {
            command::run(command, ctx, state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands(items: &[Item], out: &mut Vec<Command>) {
        for item in items {
            match *item {
                Item::Command(command) => out.push(command),
                Item::Submenu(_, rows) => commands(rows, out),
                _ => {}
            }
        }
    }

    #[test]
    fn each_command_is_in_one_menu() {
        let mut all = Vec::new();
        for menu in MENUS {
            commands(menu.items, &mut all);
        }
        for command in Command::ALL {
            let count = all.iter().filter(|c| **c == command).count();
            assert_eq!(count, 1, "{command:?}");
        }
    }
}
