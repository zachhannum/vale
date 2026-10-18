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
        ],
    },
    Menu {
        name: "Layer",
        items: &[
            Item::Submenu(
                ADD_LAYER,
                &[
                    C(Command::AddRaster),
                    S,
                    C(Command::AddLines),
                    C(Command::AddPoints),
                    C(Command::AddPolygons),
                ],
            ),
            S,
            C(Command::LayerUp),
            C(Command::LayerDown),
            S,
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
    if state.native_menu || super::dialog(state) || ctx.egui_wants_keyboard_input() {
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

/// The key of a shortcut in the form of the menu bar of macOS, for example
/// "Cmd+Shift+Z".
#[cfg(any(test, target_os = "macos"))]
pub fn accelerator(shortcut: &egui::KeyboardShortcut) -> String {
    let m = shortcut.modifiers;
    let names = [
        (m.command || m.mac_cmd, "Cmd"),
        (m.ctrl, "Ctrl"),
        (m.alt, "Alt"),
        (m.shift, "Shift"),
    ];
    let mut parts: Vec<_> = names
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, n)| *n)
        .collect();
    parts.push(shortcut.logical_key.name());
    parts.join("+")
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

    fn ids(items: &[Item], out: &mut Vec<&'static str>) {
        for item in items {
            match *item {
                Item::Command(command) => out.push(command.id()),
                Item::Submenu(name, rows) => {
                    out.push(name);
                    ids(rows, out);
                }
                Item::Recent => out.push(RECENT),
                Item::Separator => {}
            }
        }
    }

    #[test]
    fn each_row_has_its_own_id() {
        let mut all = Vec::new();
        for menu in MENUS {
            all.push(menu.name);
            ids(menu.items, &mut all);
        }
        for (i, id) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(id), "{id}");
        }
    }

    #[test]
    fn each_key_has_a_key_of_the_menu_bar() {
        let key = |c: Command| accelerator(&c.shortcut().unwrap());
        assert_eq!(key(Command::Save), "Cmd+S");
        assert_eq!(key(Command::Redo), "Cmd+Shift+Z");
        assert_eq!(key(Command::Reveal), "Cmd+Alt+R");
        assert_eq!(key(Command::World), "Cmd+1");
        assert_eq!(key(Command::Dock), "Cmd+Alt+0");
        for command in Command::ALL {
            let Some(shortcut) = command.shortcut() else {
                continue;
            };
            let text = accelerator(&shortcut);
            let (mods, key) = text.rsplit_once('+').unwrap();
            assert!(mods.starts_with("Cmd"), "{text}");
            assert!(
                key.len() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric()),
                "{text}"
            );
        }
    }

    #[test]
    fn the_menus_have_the_commands_of_the_issue() {
        let names: Vec<_> = MENUS.iter().map(|m| m.name).collect();
        assert_eq!(names, ["File", "Edit", "View", "Layer", "Window", "Help"]);
        let menu = |name: &str| MENUS.iter().find(|m| m.name == name).unwrap().items;
        let has = |name: &str, command: Command| {
            let mut all = Vec::new();
            commands(menu(name), &mut all);
            all.contains(&command)
        };
        for command in [
            Command::NewWorld,
            Command::Open,
            Command::Save,
            Command::ImportHeightmap,
            Command::ExportGeoTiff,
            Command::ExportPng,
            Command::ExportPdf,
            Command::Quit,
        ] {
            assert!(has("File", command), "{command:?}");
        }
        assert!(menu("File").contains(&Item::Recent));
        assert!(has("Edit", Command::Undo) && has("Edit", Command::Redo));
        for command in [Command::World, Command::Maps, Command::Globe, Command::Flat] {
            assert!(has("View", command), "{command:?}");
        }
        assert!(has("Layer", Command::AddLines) && has("Layer", Command::DeleteLayer));
        assert!(has("Layer", Command::AddRaster) && has("Layer", Command::LayerUp));
        for command in [Command::Layers, Command::Toolbox, Command::Dock] {
            assert!(has("Window", command), "{command:?}");
        }
        assert!(has("Help", Command::About));
    }
}
