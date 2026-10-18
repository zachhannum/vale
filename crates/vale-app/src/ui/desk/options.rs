//! The options bar below the title bar.

use eframe::egui::{self, Align, Layout, Sense, Stroke};

use super::float::Glass;
use super::rail::{self, HAND, SELECT};
use super::widgets::{self, tip_text};
use super::{MapTool, brush, field, theme};
use crate::globe::Tool;
use crate::ui::pad::icons::Icon;
use crate::ui::{AppState, Workspace};

/// The button of the options bar that shows the Brush panel in the dock.
pub const BRUSH_SETTINGS: &str = "Brush settings";

/// The name of the active tool.
pub fn tool_name(state: &AppState) -> &'static str {
    match state.workspace {
        Workspace::Globe => match state.globe.tool {
            Tool::Navigate => HAND,
            Tool::Brush => rail::mode_name(state.globe.brush.mode),
        },
        Workspace::Map => match state.desk.map_tool {
            MapTool::Select => SELECT,
            MapTool::Hand => HAND,
        },
    }
}

/// The settings of the active tool, in one row across the window.
pub fn bar(ui: &mut egui::Ui, state: &mut AppState) {
    let frame = egui::Frame::NONE.fill(theme::glass(state.translucent));
    egui::Panel::top("desk-options")
        .exact_size(theme::OPTIONS)
        .frame(frame)
        .show_separator_line(false)
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let stroke = Stroke::new(1.0, theme::line());
            ui.painter()
                .hline(rect.x_range(), rect.bottom() - 0.5, stroke);
            let layout = Layout::left_to_right(Align::Center);
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space(12.0);
                let name = ui.painter().layout_no_wrap(
                    tool_name(state).to_owned(),
                    theme::label(),
                    theme::TEXT,
                );
                let (at, _) = ui.allocate_exact_size(name.size(), Sense::hover());
                ui.painter().galley(at.min, name, theme::TEXT);
                if state.workspace == Workspace::Globe && state.globe.tool == Tool::Brush {
                    brush(ui, state);
                }
            });
        });
}

fn brush(ui: &mut egui::Ui, state: &mut AppState) {
    widgets::divider(ui);
    ui.spacing_mut().item_spacing.x = 16.0;
    let glass = Glass::of(state);
    for setting in brush::BAR {
        if let Some(value) = field::bar_field(ui, glass, &setting.field(state)) {
            setting.set(state, value);
        }
    }
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(8.0);
        let tip = tip_text(BRUSH_SETTINGS, Some("B"));
        let on = Some(state.desk.brush_shows());
        let icon = Icon::Sliders;
        let resp = widgets::tool_button(ui, BRUSH_SETTINGS, icon, &tip, on, true);
        if resp.clicked() {
            state.desk.toggle_brush();
        }
    });
}
