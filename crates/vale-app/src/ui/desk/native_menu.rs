//! The menu bar of macOS. It has the menus of the model in `menu`, after
//! the menu of the app.

use std::path::PathBuf;
use std::str::FromStr as _;
use std::sync::{Arc, Mutex};

use eframe::egui;
use muda::accelerator::Accelerator;
use muda::{
    AboutMetadata, CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuId, MenuItem,
    PredefinedMenuItem, Submenu,
};

use super::command::{self, Command};
use super::menu::{self, Item, MENUS};
use crate::projects;
use crate::ui::{Action, AppState};

/// The prefix of the IDs of the rows of the recent worlds.
const RECENT_ID: &str = "recent.";

enum Row {
    Plain(MenuItem),
    Check(CheckMenuItem),
}

/// A row of a command, with the state that the menu shows now.
struct CommandRow {
    command: Command,
    row: Row,
    enabled: bool,
    checked: Option<bool>,
}

pub struct NativeMenu {
    _menu: Menu,
    rows: Vec<CommandRow>,
    recent: Option<Submenu>,
    recent_paths: Vec<PathBuf>,
    /// The open project when the recent rows were made.
    recent_for: Option<Option<PathBuf>>,
    /// The IDs of the rows that the user chose, oldest first.
    queue: Arc<Mutex<Vec<MenuId>>>,
}

impl NativeMenu {
    /// Makes the menu bar and gives it to the app.
    pub fn install(ctx: &egui::Context, state: &AppState) -> muda::Result<NativeMenu> {
        let menu = Menu::new();
        let mut rows = Vec::new();
        let mut recent = None;

        let about = AboutMetadata {
            name: Some("Vale".into()),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            ..Default::default()
        };
        let quit = command_row(Command::Quit, state);
        let app = Submenu::with_items(
            "Vale",
            true,
            &[
                &PredefinedMenuItem::about(Some("About Vale"), Some(about)),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                row_item(&quit.row),
            ],
        )?;
        rows.push(quit);
        menu.append(&app)?;

        let mut window = None;
        let mut help = None;
        for model in MENUS {
            let submenu = Submenu::new(model.name, true);
            if model.name == "Window" {
                submenu.append_items(&[
                    &PredefinedMenuItem::minimize(None),
                    &PredefinedMenuItem::zoom(None),
                    &PredefinedMenuItem::separator(),
                ])?;
            }
            add_items(&submenu, model.items, state, &mut rows, &mut recent)?;
            menu.append(&submenu)?;
            match model.name {
                "Window" => window = Some(submenu),
                "Help" => help = Some(submenu),
                _ => {}
            }
        }
        menu.init_for_nsapp();
        if let Some(window) = window {
            window.set_as_windows_menu_for_nsapp();
        }
        if let Some(help) = help {
            help.set_as_help_menu_for_nsapp();
        }

        let queue = Arc::new(Mutex::new(Vec::new()));
        let (events, ctx) = (queue.clone(), ctx.clone());
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Ok(mut events) = events.lock() {
                events.push(event.id);
            }
            ctx.request_repaint();
        }));
        Ok(NativeMenu {
            _menu: menu,
            rows,
            recent,
            recent_paths: Vec::new(),
            recent_for: None,
            queue,
        })
    }

    /// Runs the rows that the user chose since the last frame.
    pub fn run(&self, ctx: &egui::Context, state: &mut AppState) {
        let ids = match self.queue.lock() {
            Ok(mut queue) => std::mem::take(&mut *queue),
            Err(_) => return,
        };
        for id in ids {
            let id = id.as_ref();
            if let Some(command) = Command::from_id(id) {
                command::run(command, ctx, state);
            } else if let Some(path) = id
                .strip_prefix(RECENT_ID)
                .and_then(|i| i.parse::<usize>().ok())
                .and_then(|i| self.recent_paths.get(i))
            {
                state.actions.push(Action::OpenWorld(path.clone()));
            }
        }
    }

    /// Gives the rows the state of the app. It changes only the rows whose
    /// state changed.
    pub fn sync(&mut self, state: &AppState) {
        for row in &mut self.rows {
            let enabled = row.command.enabled(state);
            if enabled != row.enabled {
                row.enabled = enabled;
                match &row.row {
                    Row::Plain(item) => item.set_enabled(enabled),
                    Row::Check(item) => item.set_enabled(enabled),
                }
            }
            let checked = row.command.checked(state);
            if checked != row.checked {
                row.checked = checked;
                if let Row::Check(item) = &row.row {
                    item.set_checked(checked.unwrap_or(false));
                }
            }
        }
        self.sync_recent(state);
    }

    /// Makes the rows of the recent worlds again when another project opens.
    fn sync_recent(&mut self, state: &AppState) {
        let Some(submenu) = &self.recent else {
            return;
        };
        let open = state.session.as_ref().map(|s| s.path().to_path_buf());
        if self.recent_for.as_ref() == Some(&open) {
            return;
        }
        self.recent_for = Some(open);
        let dir = state.projects.state_dir();
        let paths = dir.map(projects::recent).unwrap_or_default();
        if paths == self.recent_paths && !submenu.items().is_empty() {
            return;
        }
        for _ in 0..submenu.items().len() {
            submenu.remove_at(0);
        }
        if paths.is_empty() {
            let _ = submenu.append(&MenuItem::new("No recent worlds", false, None));
        }
        for (i, path) in paths.iter().enumerate() {
            let name = projects::entry(path).name;
            let _ = submenu.append(&MenuItem::with_id(
                format!("{RECENT_ID}{i}"),
                name,
                true,
                None,
            ));
        }
        self.recent_paths = paths;
    }
}

/// Adds the rows of a menu of the model. A separator shows only between two
/// rows. About and Quit are in the menu of the app.
fn add_items(
    submenu: &Submenu,
    items: &[Item],
    state: &AppState,
    rows: &mut Vec<CommandRow>,
    recent: &mut Option<Submenu>,
) -> muda::Result<()> {
    let mut separator = false;
    let mut any = false;
    let mut add = |item: &dyn IsMenuItem, separator: &mut bool| {
        if std::mem::take(separator) && any {
            submenu.append(&PredefinedMenuItem::separator())?;
        }
        any = true;
        submenu.append(item)
    };
    for item in items {
        match *item {
            Item::Command(Command::About | Command::Quit) => {}
            Item::Command(command) => {
                if !command.visible(state) {
                    continue;
                }
                let row = command_row(command, state);
                add(row_item(&row.row), &mut separator)?;
                rows.push(row);
            }
            Item::Separator => separator = true,
            Item::Recent => {
                if state.projects.any() {
                    let list = Submenu::new(menu::RECENT, true);
                    add(&list, &mut separator)?;
                    *recent = Some(list);
                }
            }
            Item::Submenu(name, items) => {
                let list = Submenu::new(name, true);
                add_items(&list, items, state, rows, recent)?;
                add(&list, &mut separator)?;
            }
        }
    }
    Ok(())
}

fn command_row(command: Command, state: &AppState) -> CommandRow {
    let shortcut = command.shortcut();
    let key = shortcut.and_then(|s| Accelerator::from_str(&menu::accelerator(&s)).ok());
    let enabled = command.enabled(state);
    let checked = command.checked(state);
    let (id, label) = (command.id(), command.label());
    let label = if command == Command::Quit {
        "Quit Vale"
    } else {
        label
    };
    let row = match checked {
        Some(on) => Row::Check(CheckMenuItem::with_id(id, label, enabled, on, key)),
        None => Row::Plain(MenuItem::with_id(id, label, enabled, key)),
    };
    CommandRow {
        command,
        row,
        enabled,
        checked,
    }
}

fn row_item(row: &Row) -> &dyn IsMenuItem {
    match row {
        Row::Plain(item) => item,
        Row::Check(item) => item,
    }
}
