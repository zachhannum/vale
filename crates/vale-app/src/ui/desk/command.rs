//! The commands of the menus and their keys.

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use vale_store::{GeometryKind, LayerId};

use super::{DeskState, Inspect, Tab};
use crate::globe::WorldView;
use crate::ui::raster_form::RasterForm;
use crate::ui::world_form::{self, WorldForm};
use crate::ui::{Action, AppState, ExportFormat, LayerRef, Workspace};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Command {
    NewWorld,
    Open,
    Save,
    Rename,
    Copy,
    Reveal,
    Delete,
    WorldProperties,
    ImportHeightmap,
    ExportGeoTiff,
    OpenGeoJson,
    ExportPng,
    ExportPdf,
    AllWorlds,
    Quit,
    Undo,
    Redo,
    World,
    Maps,
    Globe,
    Flat,
    Greyscale,
    AddRaster,
    AddLines,
    AddPoints,
    AddPolygons,
    LayerUp,
    LayerDown,
    DeleteLayer,
    Layers,
    Toolbox,
    Dock,
    About,
}

/// The name of the menu row that shows the open project in the file manager.
pub const REVEAL: &str = if cfg!(target_os = "macos") {
    "Show in Finder"
} else if cfg!(windows) {
    "Show in Explorer"
} else {
    "Show in folder"
};

impl Command {
    pub const ALL: [Command; 33] = [
        Command::NewWorld,
        Command::Open,
        Command::Save,
        Command::Rename,
        Command::Copy,
        Command::Reveal,
        Command::Delete,
        Command::WorldProperties,
        Command::ImportHeightmap,
        Command::ExportGeoTiff,
        Command::OpenGeoJson,
        Command::ExportPng,
        Command::ExportPdf,
        Command::AllWorlds,
        Command::Quit,
        Command::Undo,
        Command::Redo,
        Command::World,
        Command::Maps,
        Command::Globe,
        Command::Flat,
        Command::Greyscale,
        Command::AddRaster,
        Command::AddLines,
        Command::AddPoints,
        Command::AddPolygons,
        Command::LayerUp,
        Command::LayerDown,
        Command::DeleteLayer,
        Command::Layers,
        Command::Toolbox,
        Command::Dock,
        Command::About,
    ];

    /// The name in a menu.
    pub fn label(self) -> &'static str {
        match self {
            Command::NewWorld => world_form::NEW_WORLD,
            Command::Open => "Open…",
            Command::Save => "Save",
            Command::Rename => "Rename…",
            Command::Copy => "Make a copy",
            Command::Reveal => REVEAL,
            Command::Delete => "Delete…",
            Command::WorldProperties => "World properties…",
            Command::ImportHeightmap => "Import heightmap…",
            Command::ExportGeoTiff => "Export GeoTIFF…",
            Command::OpenGeoJson => "Open GeoJSON…",
            Command::ExportPng => "Export PNG…",
            Command::ExportPdf => "Export PDF…",
            Command::AllWorlds => "All worlds",
            Command::Quit => "Quit",
            Command::Undo => "Undo",
            Command::Redo => "Redo",
            Command::World => "World",
            Command::Maps => "Maps",
            Command::Globe => "Globe",
            Command::Flat => "Flat",
            Command::Greyscale => "Greyscale",
            Command::AddRaster => "Raster…",
            Command::AddLines => "Lines",
            Command::AddPoints => "Points",
            Command::AddPolygons => "Polygons",
            Command::LayerUp => "Move layer up",
            Command::LayerDown => "Move layer down",
            Command::DeleteLayer => "Delete layer",
            Command::Layers => "Layers",
            Command::Toolbox => "Toolbox",
            Command::Dock => "Dock",
            Command::About => "About Vale",
        }
    }

    /// The ID of the command in the menu bar of the system.
    pub fn id(self) -> &'static str {
        match self {
            Command::NewWorld => "file.new",
            Command::Open => "file.open",
            Command::Save => "file.save",
            Command::Rename => "file.rename",
            Command::Copy => "file.copy",
            Command::Reveal => "file.reveal",
            Command::Delete => "file.delete",
            Command::WorldProperties => "file.world",
            Command::ImportHeightmap => "file.import-heightmap",
            Command::ExportGeoTiff => "file.export-geotiff",
            Command::OpenGeoJson => "file.open-geojson",
            Command::ExportPng => "file.export-png",
            Command::ExportPdf => "file.export-pdf",
            Command::AllWorlds => "file.all-worlds",
            Command::Quit => "file.quit",
            Command::Undo => "edit.undo",
            Command::Redo => "edit.redo",
            Command::World => "view.world",
            Command::Maps => "view.maps",
            Command::Globe => "view.globe",
            Command::Flat => "view.flat",
            Command::Greyscale => "view.greyscale",
            Command::AddRaster => "layer.add-raster",
            Command::AddLines => "layer.add-lines",
            Command::AddPoints => "layer.add-points",
            Command::AddPolygons => "layer.add-polygons",
            Command::LayerUp => "layer.up",
            Command::LayerDown => "layer.down",
            Command::DeleteLayer => "layer.delete",
            Command::Layers => "window.layers",
            Command::Toolbox => "window.toolbox",
            Command::Dock => "window.dock",
            Command::About => "help.about",
        }
    }

    pub fn from_id(id: &str) -> Option<Command> {
        Command::ALL.into_iter().find(|c| c.id() == id)
    }

    /// The key of the command. Command is Ctrl on Windows and Linux.
    pub fn shortcut(self) -> Option<KeyboardShortcut> {
        let (cmd, shift, alt) = (
            Modifiers::COMMAND,
            Modifiers::COMMAND | Modifiers::SHIFT,
            Modifiers::COMMAND | Modifiers::ALT,
        );
        let (modifiers, key) = match self {
            Command::NewWorld => (cmd, Key::N),
            Command::Open => (cmd, Key::O),
            Command::Save => (cmd, Key::S),
            Command::Copy => (cmd, Key::D),
            Command::Reveal => (alt, Key::R),
            Command::ImportHeightmap => (shift, Key::I),
            Command::ExportGeoTiff => (shift, Key::E),
            Command::Quit => (cmd, Key::Q),
            Command::Undo => (cmd, Key::Z),
            Command::Redo => (shift, Key::Z),
            Command::World => (cmd, Key::Num1),
            Command::Maps => (cmd, Key::Num2),
            Command::Layers => (alt, Key::Num1),
            Command::Toolbox => (alt, Key::Num2),
            Command::Dock => (alt, Key::Num0),
            _ => return None,
        };
        Some(KeyboardShortcut::new(modifiers, key))
    }

    /// The text of a tooltip: the name, and the key in parentheses.
    pub fn tip(self, ctx: &egui::Context) -> String {
        let key = self.shortcut().map(|s| ctx.format_shortcut(&s));
        super::widgets::tip_text(self.label(), key.as_deref())
    }

    /// True if the menu shows the command. A command that the window cannot
    /// run does not show.
    pub fn visible(self, state: &AppState) -> bool {
        match self {
            Command::Open | Command::OpenGeoJson | Command::ExportPng | Command::ExportPdf => {
                state.file_buttons
            }
            Command::Save
            | Command::Rename
            | Command::Copy
            | Command::Reveal
            | Command::Delete
            | Command::AllWorlds => state.projects.any(),
            _ => true,
        }
    }

    pub fn enabled(self, state: &AppState) -> bool {
        let open = state.session.is_some();
        let world = state.workspace == Workspace::Globe;
        match self {
            Command::Save | Command::Rename | Command::Copy | Command::Reveal | Command::Delete => {
                open
            }
            Command::ImportHeightmap => !state.globe.busy(),
            Command::ExportGeoTiff => state.can_export(),
            Command::ExportPng | Command::ExportPdf => !world,
            Command::Undo => state.can_undo(),
            Command::Redo => state.can_redo(),
            Command::Globe | Command::Flat | Command::Greyscale => world,
            Command::AddRaster => world,
            Command::LayerUp | Command::LayerDown => {
                let up = self == Command::LayerUp;
                selected_layer(state).is_some_and(|id| can_move(state, id, up))
            }
            Command::DeleteLayer => selected_layer(state).is_some(),
            _ => true,
        }
    }

    /// The state of a command that a menu shows with a tick. `None`: the
    /// command has no state.
    pub fn checked(self, state: &AppState) -> Option<bool> {
        let desk = &state.desk;
        let view = state.globe.world_view;
        Some(match self {
            Command::World => state.workspace == Workspace::Globe,
            Command::Maps => state.workspace == Workspace::Map,
            Command::Globe => view == WorldView::Globe,
            Command::Flat => view == WorldView::Flat,
            Command::Greyscale => state.globe.preview.greyscale,
            Command::Layers => desk.dock_open && desk.tab == Tab::Layers,
            Command::Toolbox => desk.dock_open && desk.tab == Tab::Toolbox,
            Command::Dock => desk.dock_open,
            _ => return None,
        })
    }
}

/// Runs a command, if it is enabled.
pub fn run(command: Command, ctx: &egui::Context, state: &mut AppState) {
    if !command.enabled(state) {
        return;
    }
    let open = state.open_project_entry();
    let push = |state: &mut AppState, action| state.actions.push(action);
    match command {
        Command::NewWorld => state.world_form = Some(WorldForm::new_world()),
        Command::Open => push(state, Action::OpenProjectDialog),
        Command::Save => push(state, Action::Save),
        Command::Rename => {
            if let Some((path, name)) = open {
                state.browser.ask_rename(path, name);
            }
        }
        Command::Copy => {
            if let Some((path, _)) = open {
                push(state, Action::CopyProject(path));
            }
        }
        Command::Reveal => {
            if let Some((path, _)) = open {
                push(state, Action::Reveal(path));
            }
        }
        Command::Delete => {
            if let Some((path, name)) = open {
                state.browser.ask_delete(path, name);
            }
        }
        Command::WorldProperties => {
            state.world_form = Some(WorldForm::properties(&state.doc.project.world));
        }
        Command::ImportHeightmap => push(state, Action::ImportHeightmapDialog),
        Command::ExportGeoTiff => push(state, Action::ExportHeightmapDialog),
        Command::OpenGeoJson => push(state, Action::OpenDialog),
        Command::ExportPng => push(state, Action::ExportDialog(ExportFormat::Png)),
        Command::ExportPdf => push(state, Action::ExportDialog(ExportFormat::Pdf)),
        Command::AllWorlds => push(state, Action::ShowProjects),
        Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        Command::Undo => state.undo(),
        Command::Redo => state.redo(),
        Command::World => state.workspace = Workspace::Globe,
        Command::Maps => state.workspace = Workspace::Map,
        Command::Globe => state.globe.world_view = WorldView::Globe,
        Command::Flat => state.globe.world_view = WorldView::Flat,
        Command::Greyscale => {
            let preview = &mut state.globe.preview;
            preview.greyscale = !preview.greyscale;
        }
        Command::AddRaster => state.raster_form = Some(RasterForm::open(state)),
        Command::AddLines => add_layer(state, GeometryKind::Line),
        Command::AddPoints => add_layer(state, GeometryKind::Point),
        Command::AddPolygons => add_layer(state, GeometryKind::Polygon),
        Command::LayerUp | Command::LayerDown => {
            if let Some(id) = selected_layer(state) {
                state.move_layer_by_one(id, command == Command::LayerUp);
            }
        }
        Command::DeleteLayer => {
            if let Some(id) = selected_layer(state) {
                state.remove_layer(id);
                state.select(None);
                state.selection = None;
                state.touch();
            }
        }
        Command::Layers => show_tab(&mut state.desk, Tab::Layers),
        Command::Toolbox => show_tab(&mut state.desk, Tab::Toolbox),
        Command::Dock => state.desk.dock_open = !state.desk.dock_open,
        Command::About => {
            state.status = format!("Vale {}", env!("CARGO_PKG_VERSION"));
        }
    }
}

/// Adds a vector layer and selects it.
pub fn add_layer(state: &mut AppState, kind: GeometryKind) {
    let id = state.doc.add_layer(kind);
    state.select(Some(LayerRef::Vector(id)));
    state.desk.inspect = Inspect::Selection;
    state.selection = None;
    state.touch();
}

/// True if the layer is not at the end of the draw order. `up` is toward
/// the top.
fn can_move(state: &AppState, id: LayerId, up: bool) -> bool {
    let layers = state.doc.project.layers();
    let index = layers.iter().position(|l| l.id == id);
    index.is_some_and(|i| if up { i + 1 < layers.len() } else { i > 0 })
}

/// The record of the selected layer of each kind.
fn selected_layer(state: &AppState) -> Option<LayerId> {
    let layer = state.selected_layer?;
    state.layer_of(layer).map(|layer| layer.id)
}

fn show_tab(desk: &mut DeskState, tab: Tab) {
    desk.tab = tab;
    desk.dock_open = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_id_gives_its_command() {
        for command in Command::ALL {
            assert_eq!(Command::from_id(command.id()), Some(command));
        }
        assert_eq!(Command::from_id("file.nothing"), None);
    }

    #[test]
    fn no_two_commands_have_the_same_key() {
        let keys: Vec<_> = Command::ALL.iter().filter_map(|c| c.shortcut()).collect();
        for (i, a) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].contains(a), "{a:?}");
        }
    }

    #[test]
    fn no_two_commands_have_the_same_id() {
        for (i, a) in Command::ALL.iter().enumerate() {
            assert!(Command::ALL[i + 1..].iter().all(|b| b.id() != a.id()));
        }
    }
}
