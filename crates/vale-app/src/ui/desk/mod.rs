//! The desktop layout: a title bar, a tool rail, a canvas, and a dock, for
//! a mouse and a keyboard.

use eframe::egui;

use super::{AppState, Workspace, canvas, elevation, globe, inspector, panels, world_form};
use crate::globe::Tool;
use crate::ui::pad::icons::Icon;

pub mod command;
pub mod menu;
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

#[derive(Clone, Debug, PartialEq)]
pub struct DeskState {
    /// The dock shows at the right edge.
    pub dock_open: bool,
    pub tab: Tab,
    /// The space at the left of the title bar for the window buttons.
    pub title_inset: f32,
}

impl Default for DeskState {
    fn default() -> DeskState {
        DeskState {
            dock_open: true,
            tab: Tab::default(),
            title_inset: 0.0,
        }
    }
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    menu::keys(ui.ctx(), state);
    let screen = ui.ctx().content_rect();
    ui.scope_builder(egui::UiBuilder::new().max_rect(screen), |ui| {
        ui.set_clip_rect(screen);
        menu::row(ui, state);
        title::draw(ui, state);
        match state.workspace {
            Workspace::Globe => {
                rail(ui, state);
                panels::status(ui, state);
                let desk = &state.desk;
                state.globe.debug = desk.dock_open && desk.tab == Tab::Toolbox;
                panels::brush_debug(ui, state);
                panels::brush(ui, state);
                elevation::draw(ui, state);
                globe::draw(ui, state);
            }
            Workspace::Map => {
                panels::left(ui, state);
                inspector::draw(ui, state);
                panels::status(ui, state);
                canvas::draw(ui, state);
            }
        }
    });
    world_form::desktop(ui.ctx(), state);
}

/// The tools of the World workspace.
fn rail(ui: &mut egui::Ui, state: &mut AppState) {
    let frame = egui::Frame::NONE
        .fill(theme::glass(state.translucent))
        .inner_margin(egui::Margin::symmetric(8, 8));
    egui::Panel::left("desk-rail")
        .exact_size(theme::RAIL)
        .resizable(false)
        .frame(frame)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for (tool, name, icon) in [
                (Tool::Navigate, "Navigate", Icon::Move),
                (Tool::Brush, "Brush", Icon::Brush),
            ] {
                let on = state.globe.tool == tool;
                let resp = widgets::tool_button(ui, name, icon, name, Some(on), true);
                if resp.clicked() {
                    state.globe.tool = tool;
                }
            }
        });
}
