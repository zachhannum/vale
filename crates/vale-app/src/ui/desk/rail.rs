//! The tool rail at the left edge, the menu of the More button, and the keys
//! of the tools.

use eframe::egui::{self, Key, Modifiers, Rect, Stroke, pos2, vec2};
use vale_terrain::Mode;

use super::widgets::{self, tip_text};
use super::{MapTool, theme};
use crate::globe::Tool;
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

fn world_tools(ui: &mut egui::Ui, state: &mut AppState) {
    let globe = &mut state.globe;
    for (name, icon, mode, key) in MODES {
        let on = globe.tool == Tool::Brush && globe.brush.mode == mode;
        let tip = tip_text(name, Some(key.name()));
        if widgets::tool_button(ui, name, icon, &tip, Some(on), true).clicked() {
            globe.tool = Tool::Brush;
            globe.brush.mode = mode;
        }
    }
    let on = globe.tool == Tool::Navigate;
    let tip = tip_text(HAND, Some("H"));
    if widgets::tool_button(ui, HAND, Icon::Move, &tip, Some(on), true).clicked() {
        globe.tool = Tool::Navigate;
    }
    // The button shows the mode of the menu that the brush has.
    let chosen = MORE_MODES
        .into_iter()
        .find(|(_, _, mode)| globe.tool == Tool::Brush && globe.brush.mode == *mode);
    let icon = chosen.map_or(Icon::More, |(_, icon, _)| icon);
    let on = Some(chosen.is_some());
    let more = widgets::tool_button(ui, MORE_TOOLS, icon, MORE_TOOLS, on, true);
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
    let Some(button) = state.desk.more_button.filter(|_| state.desk.more_open) else {
        state.desk.more_open = false;
        return;
    };
    let at = pos2(button.right() + 6.0, button.top() - 5.0);
    let area = egui::Area::new(egui::Id::new("desk-more"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(140.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let globe = &mut state.globe;
                for (name, _, mode) in MORE_MODES {
                    let on = globe.tool == Tool::Brush && globe.brush.mode == mode;
                    let row = egui::Button::selectable(on, name).min_size(vec2(140.0, theme::ROW));
                    if ui.add(row).clicked() {
                        globe.tool = Tool::Brush;
                        globe.brush.mode = mode;
                        state.desk.more_open = false;
                    }
                }
            });
        });
    if pressed_outside(ctx, area.response.rect, button) {
        state.desk.more_open = false;
    }
}

/// True if the pointer went down outside `rect` and outside the button that
/// opened it.
pub(super) fn pressed_outside(ctx: &egui::Context, rect: Rect, button: Rect) -> bool {
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    pressed.is_some_and(|pos| !rect.contains(pos) && !button.contains(pos))
}

/// The keys of the tools. They work while no text field has the keyboard and
/// no form is open.
pub fn keys(ctx: &egui::Context, state: &mut AppState) {
    let dialog = state.world_form.is_some() || state.browser.dialog.is_some();
    if dialog || ctx.egui_wants_keyboard_input() {
        return;
    }
    let key = |key| ctx.input_mut(|i| i.modifiers.is_none() && i.consume_key(Modifiers::NONE, key));
    let desk = &mut state.desk;
    if (desk.brush_open || desk.more_open) && key(Key::Escape) {
        desk.brush_open = false;
        desk.more_open = false;
    }
    match state.workspace {
        Workspace::Globe => {
            let globe = &mut state.globe;
            for (_, _, mode, code) in MODES {
                if key(code) {
                    globe.tool = Tool::Brush;
                    globe.brush.mode = mode;
                }
            }
            if key(Key::H) {
                globe.tool = Tool::Navigate;
            }
            if key(Key::B) && globe.tool == Tool::Brush {
                desk.brush_open = !desk.brush_open;
            }
        }
        Workspace::Map => {
            if key(Key::V) {
                desk.map_tool = MapTool::Select;
            }
            if key(Key::H) {
                desk.map_tool = MapTool::Hand;
            }
        }
    }
}
