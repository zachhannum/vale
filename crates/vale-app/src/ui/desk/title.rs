//! The title bar. It holds the world and its file, the workspace switch,
//! the view switch, and the actions.

use eframe::egui::{self, Align, Layout, Rect, Sense, Stroke, UiBuilder, pos2, vec2};

use super::command::{self, Command};
use super::theme;
use super::widgets::{self, Segment};
use crate::globe::WorldView;
use crate::ui::pad::icons::Icon;
use crate::ui::{AppState, SaveMark, Workspace, pad, panels};

/// The name of the map frame in the Maps workspace.
pub const FRAME_NAME: &str = "World map";
/// The tooltip of a control of a feature that the prototype does not have.
pub const NOT_IN_PROTOTYPE: &str = "Not in the prototype";
/// The smallest width of the name of the world and of its file.
const DOC_WIDTH: f32 = 120.0;
const SAVE_MARK: f32 = 11.0;

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let frame = egui::Frame::NONE.fill(theme::glass(state.translucent));
    egui::Panel::top("desk-title")
        .exact_size(theme::BAR)
        .frame(frame)
        .show_separator_line(false)
        .show(ui, |ui| {
            let rect = ui.max_rect();
            if state.desk.title_inset > 0.0 {
                drag(ui, rect);
            }
            let stroke = Stroke::new(1.0, theme::line());
            ui.painter()
                .hline(rect.x_range(), rect.bottom() - 0.5, stroke);
            let left = rect.left() + 16.0 + state.desk.title_inset;
            let left = Rect::from_min_max(pos2(left, rect.top()), rect.max);
            place(ui, left, Layout::left_to_right(Align::Center), |ui| {
                doc(ui, state);
                ui.add_space(14.0);
                workspaces(ui, state);
            });
            let right = Rect::from_min_max(rect.min, pos2(rect.right() - 10.0, rect.bottom()));
            place(ui, right, Layout::right_to_left(Align::Center), |ui| {
                actions(ui, state);
            });
            center(ui, rect, state);
        });
}

fn place<R>(
    ui: &mut egui::Ui,
    rect: Rect,
    layout: Layout,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(layout), add_contents)
}

/// The window content fills the title bar of macOS, so the bar moves the
/// window, and a double click zooms it.
fn drag(ui: &egui::Ui, rect: Rect) {
    let resp = ui.interact(rect, ui.id().with("drag"), Sense::click_and_drag());
    let ctx = ui.ctx();
    if resp.drag_started() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if resp.double_clicked() {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
}

/// The text of the file line: the file name and the state of the save.
fn file_line(state: &AppState) -> Option<(String, SaveMark)> {
    let session = state.session.as_ref()?;
    let mark = state.save_mark(std::time::Instant::now())?;
    let file = session.path().file_name().unwrap_or_default();
    let text = match mark {
        SaveMark::Saving => "Saving",
        SaveMark::Saved => "Saved",
        SaveMark::Failed => "Not saved",
    };
    Some((format!("{} · {text}", file.to_string_lossy()), mark))
}

/// The name of the world, and below it the file with the save mark.
fn doc(ui: &mut egui::Ui, state: &AppState) {
    let painter = ui.painter();
    let name = &state.doc.project.world.name;
    let name = painter.layout_no_wrap(name.clone(), theme::body(), theme::TEXT);
    let file = file_line(state).map(|(text, mark)| {
        let galley = painter.layout_no_wrap(text.clone(), theme::small(), theme::MUTE);
        (text, galley, mark)
    });
    let file_width = file
        .as_ref()
        .map_or(0.0, |(_, galley, _)| galley.size().x + 4.0 + SAVE_MARK);
    let width = name.size().x.max(file_width).max(DOC_WIDTH).ceil();
    let (rect, _) = ui.allocate_exact_size(vec2(width, theme::BAR), Sense::hover());
    let middle = rect.center().y;
    let name_top = match file {
        Some(_) => middle - name.size().y,
        None => middle - name.size().y / 2.0,
    };
    let name_rect = Rect::from_min_size(pos2(rect.left(), name_top), name.size());
    label(ui, name_rect, "world", &state.doc.project.world.name);
    ui.painter().galley(name_rect.min, name, theme::TEXT);
    if let Some((text, galley, mark)) = file {
        let file_rect = Rect::from_min_size(pos2(rect.left(), middle + 1.0), galley.size());
        label(ui, file_rect, "file", &text);
        ui.painter().galley(file_rect.min, galley, theme::MUTE);
        let center = pos2(
            file_rect.right() + 4.0 + SAVE_MARK / 2.0,
            file_rect.center().y,
        );
        let mark_rect = Rect::from_center_size(center, vec2(SAVE_MARK, SAVE_MARK));
        pad::save_mark(ui, mark_rect, mark, !state.headless);
    }
}

/// Gives a painted text a name for the screen reader.
fn label(ui: &egui::Ui, rect: Rect, id: &str, text: &str) {
    let resp = ui.interact(rect, ui.id().with(id), Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
}

fn workspaces(ui: &mut egui::Ui, state: &mut AppState) {
    let items = [
        Segment::new(Command::World.label()),
        Segment::new(Command::Maps.label()),
        Segment {
            label: "Atlas",
            enabled: false,
            tip: Some(NOT_IN_PROTOTYPE),
        },
    ];
    let active = match state.workspace {
        Workspace::Globe => 0,
        Workspace::Map => 1,
    };
    match widgets::segments(ui, &items, Some(active)) {
        Some(0) => command::run(Command::World, ui.ctx(), state),
        Some(1) => command::run(Command::Maps, ui.ctx(), state),
        _ => {}
    }
}

/// The view switch in the World workspace, and the name of the map frame in
/// the Maps workspace. It is at the center of the bar.
fn center(ui: &mut egui::Ui, bar: Rect, state: &mut AppState) {
    // The width comes from the last frame.
    let id = ui.id().with("center-width");
    let width = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
    let rect = Rect::from_min_max(pos2(bar.center().x - width / 2.0, bar.top()), bar.max);
    let inner = place(
        ui,
        rect,
        Layout::left_to_right(Align::Center),
        |ui| match state.workspace {
            Workspace::Globe => views(ui, state),
            Workspace::Map => {
                let text = egui::RichText::new(FRAME_NAME)
                    .font(theme::body())
                    .color(theme::TEXT);
                ui.label(text);
            }
        },
    );
    let used = inner.response.rect.width();
    if (used - width).abs() > 0.5 {
        ui.data_mut(|d| d.insert_temp(id, used));
        ui.ctx().request_repaint();
    }
}

fn views(ui: &mut egui::Ui, state: &mut AppState) {
    let items = [
        Segment::new(Command::Globe.label()),
        Segment::new(Command::Flat.label()),
    ];
    let active = match state.globe.world_view {
        WorldView::Globe => 0,
        WorldView::Flat => 1,
    };
    match widgets::segments(ui, &items, Some(active)) {
        Some(0) => command::run(Command::Globe, ui.ctx(), state),
        Some(1) => command::run(Command::Flat, ui.ctx(), state),
        _ => {}
    }
    if state.globe.world_view == WorldView::Flat {
        ui.add_space(4.0);
        panels::flat_projection(ui, state);
    }
}

/// Undo, redo, and the button of the dock, from right to left.
fn actions(ui: &mut egui::Ui, state: &mut AppState) {
    ui.spacing_mut().item_spacing.x = 4.0;
    let ctx = ui.ctx().clone();
    let dock = Command::Dock;
    let on = state.desk.dock_open;
    let resp = widgets::tool_button(
        ui,
        dock.label(),
        Icon::Dock,
        &dock.tip(&ctx),
        Some(on),
        true,
    );
    if resp.clicked() {
        command::run(dock, &ctx, state);
    }
    widgets::divider(ui);
    for (command, icon) in [(Command::Redo, Icon::Redo), (Command::Undo, Icon::Undo)] {
        let enabled = command.enabled(state);
        let tip = command.tip(&ctx);
        let resp = widgets::tool_button(ui, command.label(), icon, &tip, None, enabled);
        if resp.clicked() {
            command::run(command, &ctx, state);
        }
    }
}
