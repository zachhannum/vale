//! The desktop layout: a title bar, a tool rail, a canvas, and a dock, for
//! a mouse and a keyboard.

use eframe::egui;

use super::{AppState, Workspace, canvas, globe, world_form};
use crate::globe::Tool;
use vale_terrain::Mode;

pub mod command;
pub mod dock;
pub mod menu;
pub mod options;
pub mod rail;
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

#[derive(Clone, Debug, PartialEq)]
pub struct DeskState {
    /// The dock shows at the right edge.
    pub dock_open: bool,
    pub tab: Tab,
    /// The Height row of the layer list is selected.
    pub height_selected: bool,
    /// The space at the left of the title bar for the window buttons.
    pub title_inset: f32,
    /// The Brush popover is open.
    pub brush_open: bool,
    /// The menu of the More button is open.
    pub more_open: bool,
    pub map_tool: MapTool,
    /// The Brush button of the options bar, in this frame.
    pub brush_button: Option<egui::Rect>,
    /// The More button of the tool rail, in this frame.
    pub more_button: Option<egui::Rect>,
}

impl Default for DeskState {
    fn default() -> DeskState {
        DeskState {
            dock_open: true,
            tab: Tab::default(),
            height_selected: false,
            title_inset: 0.0,
            brush_open: false,
            more_open: false,
            map_tool: MapTool::default(),
            brush_button: None,
            more_button: None,
        }
    }
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
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
                let cards = state.desk.brush_open;
                globe::draw(ui, state, cards);
            }
            Workspace::Map => canvas::draw(ui, state),
        }
    });
    rail::more_menu(&ctx, state);
    options::popover(&ctx, state);
    world_form::desktop(&ctx, state);
}
