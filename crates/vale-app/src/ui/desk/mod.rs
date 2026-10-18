//! The desktop layout: a title bar, a tool rail, a canvas, and a dock, for
//! a mouse and a keyboard.

use eframe::egui;

use super::{AppState, Workspace, canvas, globe, world_form};
use crate::globe::Tool;
use vale_terrain::Mode;

pub mod brush;
pub mod command;
pub mod dock;
pub mod field;
pub mod float;
pub mod menu;
#[cfg(target_os = "macos")]
pub mod native_menu;
pub mod options;
pub mod rail;
pub mod start;
pub mod status;
pub mod theme;
pub mod title;
pub mod widgets;

/// A tab of the dock.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Layers,
    Toolbox,
}

/// A tool of the Maps workspace.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum MapTool {
    #[default]
    Select,
    /// Moves the map. A click selects nothing.
    Hand,
}

/// The subject of the inspector.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Inspect {
    /// The selected layer, or the world with no selected layer.
    #[default]
    Selection,
    Height,
    /// The Brush panel, while a brush mode is the tool.
    Brush,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeskState {
    /// The dock shows at the right edge.
    pub dock_open: bool,
    pub tab: Tab,
    pub inspect: Inspect,
    /// The subject of the inspector before the Brush panel. B goes back
    /// to it.
    pub before_brush: Inspect,
    /// The space at the left of the title bar for the window buttons.
    pub title_inset: f32,
    /// The menu of the More button is open.
    pub more_open: bool,
    pub map_tool: MapTool,
    /// The More button of the tool rail, in this frame.
    pub more_button: Option<egui::Rect>,
    pub start: start::StartState,
}

impl Default for DeskState {
    fn default() -> DeskState {
        DeskState {
            dock_open: true,
            tab: Tab::default(),
            inspect: Inspect::default(),
            before_brush: Inspect::default(),
            title_inset: 0.0,
            more_open: false,
            map_tool: MapTool::default(),
            more_button: None,
            start: start::StartState::default(),
        }
    }
}

impl DeskState {
    /// Makes the Brush panel the subject of the inspector.
    pub fn inspect_brush(&mut self) {
        if self.inspect != Inspect::Brush {
            self.before_brush = self.inspect;
            self.inspect = Inspect::Brush;
        }
    }

    /// True if the dock shows the Brush panel. The tool must be a brush.
    pub fn brush_shows(&self) -> bool {
        self.dock_open && self.tab == Tab::Layers && self.inspect == Inspect::Brush
    }

    /// Shows the Brush panel in the dock, or the subject before it.
    pub fn toggle_brush(&mut self) {
        if self.brush_shows() {
            self.inspect = self.before_brush;
        } else {
            self.inspect_brush();
            self.dock_open = true;
            self.tab = Tab::Layers;
        }
    }
}

/// The bottom edge of the title bar, below the row of the menus if the
/// window has it.
pub fn bar_bottom(state: &AppState) -> f32 {
    match state.native_menu {
        true => theme::BAR,
        false => theme::MENU + theme::BAR,
    }
}

/// True if a dropdown, a menu, or a sheet is open. Then the canvas makes
/// its blurred copy.
fn covered(ctx: &egui::Context, state: &AppState) -> bool {
    let desk = &state.desk;
    desk.more_open
        || state.world_form.is_some()
        || state.browser.dialog.is_some()
        || egui::Popup::is_any_open(ctx)
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    if state.world_form.is_some() || state.browser.dialog.is_some() {
        state.desk.more_open = false;
    }
    menu::keys(&ctx, state);
    rail::keys(&ctx, state);
    let globe = &mut state.globe;
    if globe.tool != Tool::Brush || globe.brush.mode != Mode::Flatten {
        globe.pick_level = false;
    }
    let screen = ctx.content_rect();
    ui.scope_builder(egui::UiBuilder::new().max_rect(screen), |ui| {
        ui.set_clip_rect(screen);
        menu::row(ui, state);
        title::draw(ui, state);
        options::bar(ui, state);
        status::draw(ui, state);
        rail::draw(ui, state);
        let desk = &state.desk;
        state.globe.debug = desk.dock_open && desk.tab == Tab::Toolbox;
        dock::draw(ui, state);
        match state.workspace {
            Workspace::Globe => {
                let cards = covered(ui.ctx(), state);
                globe::draw(ui, state, cards);
            }
            Workspace::Map => canvas::draw(ui, state),
        }
    });
    rail::more_menu(&ctx, state);
    world_form::desktop(&ctx, state);
}
