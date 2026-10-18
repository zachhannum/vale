//! The tool rail at the left edge, the menu of the More button, and the keys
//! of the tools.

use eframe::egui::{self, Key, Modifiers, Stroke, pos2};
use vale_terrain::Mode;

use super::float::{self, Glass, Item};
use super::widgets::{self, tip_text};
use super::{MapTool, theme};
use crate::globe::Tool;
use crate::globe::brush::needs_height;
use crate::ui::pad::MORE_MODES;
use crate::ui::pad::icons::Icon;
use crate::ui::{AppState, Workspace};

/// The button that opens the menu of the other modes.
pub const MORE_TOOLS: &str = "More tools";
pub const HAND: &str = "Hand";
pub const SELECT: &str = "Select";

/// The modes that have a button, with their keys.
const MODES: [(&str, Icon, Mode, Key); 4] = [
    ("Raise", Icon::Raise, Mode::Raise, Key::R),
    ("Lower", Icon::Lower, Mode::Lower, Key::F),
    ("Smooth", Icon::Smooth, Mode::Smooth, Key::S),
    ("Flatten", Icon::Flatten, Mode::Flatten, Key::E),
];

/// The name of a brush mode.
pub fn mode_name(mode: Mode) -> &'static str {
    let rail = MODES.iter().map(|(name, _, m, _)| (*name, *m));
    let more = MORE_MODES.iter().map(|(name, _, m)| (*name, *m));
    rail.chain(more)
        .find(|(_, m)| *m == mode)
        .map_or("Brush", |(name, _)| name)
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    state.desk.more_button = None;
    let frame = egui::Frame::NONE
        .fill(theme::glass(state.translucent))
        .inner_margin(egui::Margin::symmetric(8, 8));
    egui::Panel::left("desk-rail")
        .exact_size(theme::RAIL)
        .resizable(false)
        .frame(frame)
        .show_separator_line(false)
        .show(ui, |ui| {
            let rect = ui.clip_rect();
            let stroke = Stroke::new(1.0, theme::line());
            ui.painter()
                .vline(rect.right() - 0.5, rect.y_range(), stroke);
            ui.spacing_mut().item_spacing.y = 4.0;
            match state.workspace {
                Workspace::Globe => world_tools(ui, state),
                Workspace::Map => map_tools(ui, state),
            }
        });
}

/// True if the brush can paint in `mode`. The brush paints a visible raster
/// layer, and some modes need a height layer.
pub fn can_paint(state: &AppState, mode: Mode) -> bool {
    let globe = &state.globe;
    globe.paintable && (globe.is_height() || !needs_height(mode))
}

fn world_tools(ui: &mut egui::Ui, state: &mut AppState) {
    for (name, icon, mode, key) in MODES {
        let enabled = can_paint(state, mode);
        let globe = &mut state.globe;
        let on = globe.brushing() && globe.brush.mode == mode;
        let tip = tip_text(name, Some(key.name()));
        if widgets::tool_button(ui, name, icon, &tip, Some(on), enabled).clicked() {
            globe.tool = Tool::Brush;
            globe.brush.mode = mode;
            state.desk.inspect_brush();
        }
    }
    let globe = &mut state.globe;
    let on = !globe.brushing();
    let tip = tip_text(HAND, Some("H"));
    if widgets::tool_button(ui, HAND, Icon::Move, &tip, Some(on), true).clicked() {
        globe.tool = Tool::Navigate;
    }
    // The button shows the mode of the menu that the brush has.
    let chosen = MORE_MODES
        .into_iter()
        .find(|(_, _, mode)| globe.brushing() && globe.brush.mode == *mode);
    let icon = chosen.map_or(Icon::More, |(_, icon, _)| icon);
    let on = Some(chosen.is_some());
    let paintable = globe.paintable;
    let more = widgets::tool_button(ui, MORE_TOOLS, icon, MORE_TOOLS, on, paintable);
    if more.clicked() {
        state.desk.more_open = !state.desk.more_open;
    }
    state.desk.more_button = Some(more.rect);
}

fn map_tools(ui: &mut egui::Ui, state: &mut AppState) {
    for (tool, name, icon, key) in [
        (MapTool::Select, SELECT, Icon::Select, "V"),
        (MapTool::Hand, HAND, Icon::Move, "H"),
    ] {
        let on = state.desk.map_tool == tool;
        let tip = tip_text(name, Some(key));
        if widgets::tool_button(ui, name, icon, &tip, Some(on), true).clicked() {
            state.desk.map_tool = tool;
        }
    }
}

/// The menu of the modes that have no button. It opens at the right of the
/// More button.
pub fn more_menu(ctx: &egui::Context, state: &mut AppState) {
    let open = state.desk.more_open && state.globe.paintable;
    let Some(button) = state.desk.more_button.filter(|_| open) else {
        state.desk.more_open = false;
        return;
    };
    let at = pos2(button.right() + 6.0, button.top() - 5.0);
    let glass = Glass::of(state);
    let area = egui::Area::new(egui::Id::new("desk-more"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ctx, |ui| {
            float::menu_body(ui, glass, |ui| {
                ui.set_min_width(140.0);
                for (name, _, mode) in MORE_MODES {
                    let enabled = can_paint(state, mode);
                    let globe = &mut state.globe;
                    let on = globe.brushing() && globe.brush.mode == mode;
                    let item = Item::new(name).checked(on).enabled(enabled);
                    if float::item(ui, item).clicked() {
                        globe.tool = Tool::Brush;
                        globe.brush.mode = mode;
                        state.desk.more_open = false;
                        state.desk.inspect_brush();
                    }
                }
            });
        });
    if float::pressed_outside(ctx, area.response.rect, button) {
        state.desk.more_open = false;
    }
}

/// The keys of the tools. They work while no text field has the keyboard and
/// no form is open.
pub fn keys(ctx: &egui::Context, state: &mut AppState) {
    if super::dialog(state) || ctx.egui_wants_keyboard_input() {
        return;
    }
    let key = |key| ctx.input_mut(|i| i.modifiers.is_none() && i.consume_key(Modifiers::NONE, key));
    if state.desk.more_open && key(Key::Escape) {
        state.desk.more_open = false;
    }
    match state.workspace {
        Workspace::Globe => {
            let paints = MODES.map(|(_, _, mode, _)| can_paint(state, mode));
            let desk = &mut state.desk;
            let globe = &mut state.globe;
            for ((_, _, mode, code), paints) in MODES.into_iter().zip(paints) {
                if key(code) && paints {
                    globe.tool = Tool::Brush;
                    globe.brush.mode = mode;
                    desk.inspect_brush();
                }
            }
            if key(Key::H) {
                globe.tool = Tool::Navigate;
            }
            if key(Key::B) && globe.brushing() {
                desk.toggle_brush();
            }
        }
        Workspace::Map => {
            let desk = &mut state.desk;
            if key(Key::V) {
                desk.map_tool = MapTool::Select;
            }
            if key(Key::H) {
                desk.map_tool = MapTool::Hand;
            }
        }
    }
}
