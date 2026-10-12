//! The start screen in the iPad layout: a grid of cards, one for each
//! project. A tap on a card opens the world. The button at the corner of a
//! card opens the menu of its actions.

use std::sync::Arc;

use eframe::egui::{self, Align2, FontId, Order, Rect, pos2, vec2};

use super::geometry::{self, Bands, GAP, Input, MARGIN, PAD, ROW, TOUCH};
use super::icons::Icon;
use super::widgets::{self, BODY_PAD, Controls, PANEL_ROW};
use super::{rule, theme};
use crate::projects::{self, Entry};
use crate::ui::browser::{ActionsMenu, Dialog, PROBLEM_TITLE, TRY_OPEN, problem_text};
use crate::ui::world_form::{self, WorldForm};
use crate::ui::{Action, AppState};

/// The title of the screen.
pub const TITLE: &str = "Worlds";
pub const RENAME: &str = "Rename…";
pub const COPY: &str = "Make a copy";
pub const DELETE: &str = "Delete…";
/// The title of the dialog that asks for a new name.
pub const RENAME_TITLE: &str = "Rename world";
pub const SAVE_NAME: &str = "Save name";
/// The title of the dialog that asks before a delete, and its button.
pub const DELETE_TITLE: &str = "Delete world";
/// The name of the field of the rename dialog.
pub const NAME: &str = "Name";

/// The height of the row with the title, in points.
const HEADER: f32 = 72.0;
/// The smallest width of a card on a screen with more than two columns, in
/// points.
const CARD_MIN: f32 = 200.0;
/// The space between two cards, in points.
const CARD_GAP: f32 = 16.0;
/// The space between the edge of a card and its picture, in points.
const CARD_PAD: f32 = 10.0;
/// The height of the part of a card below its picture, in points.
const INFO: f32 = 60.0;
const MENU_WIDTH: f32 = 220.0;
const DIALOG_WIDTH: f32 = 400.0;

/// The name of the card of a world.
pub fn open_name(world: &str) -> String {
    format!("Open {world}")
}

/// The name of the button that opens the menu of the actions of a world.
pub fn actions_name(world: &str) -> String {
    format!("Actions of {world}")
}

/// The name of the close button of a dialog with this title.
pub fn close_name(title: &str) -> String {
    format!("Close {title}")
}

fn title_font() -> FontId {
    FontId::proportional(28.0)
}

fn date_font() -> FontId {
    FontId::proportional(12.0)
}

/// One line of text that ends with "…" if it is wider than `width`.
fn one_line(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: egui::Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    ui.painter().layout_job(job)
}

/// Draws the start screen.
pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let screen = ctx.viewport_rect();
    let bands = Bands::new(screen, ctx.content_rect());
    let mut controls = std::mem::take(&mut state.pad.controls);
    controls.clear();

    let content = Rect::from_min_max(
        pos2(
            screen.left() + bands.left + MARGIN,
            screen.top() + bands.top,
        ),
        pos2(screen.right() - bands.right - MARGIN, screen.bottom()),
    );
    let layout = egui::Layout::top_down(egui::Align::Min);
    let builder = egui::UiBuilder::new().max_rect(content).layout(layout);
    ui.scope_builder(builder, |ui| {
        ui.set_clip_rect(screen);
        ui.painter().rect_filled(screen, 0.0, theme::CANVAS);
        theme::apply(ui);
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        header(ui, state, &mut controls);
        if state.browser.entries.is_empty() {
            let text = format!("No worlds yet. Tap \"{}\".", world_form::NEW_WORLD);
            let text = egui::RichText::new(text).font(theme::body());
            ui.add_space(GAP);
            ui.add(egui::Label::new(text.color(theme::MUTE)).selectable(false));
        } else {
            grid(ui, state, &mut controls, bands.bottom);
        }
    });

    if state.browser.menu.is_some() {
        actions_menu(&ctx, state, &mut controls, screen, &bands);
    }
    dialog(&ctx, state, &mut controls, screen, &bands);
    // The form of a new world takes its place from the top row of the
    // workspace.
    let rects = geometry::layout(&Input {
        screen,
        bands,
        workspace_width: 0.0,
        view_width: 0.0,
        panel: false,
        sheet_offset: 0.0,
    });
    world_form::pad(&ctx, state, &mut controls, &rects);
    state.pad.controls = controls;
}

/// The title, the button of a new world, and the last message.
fn header(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEADER), egui::Sense::hover());
    let at = pos2(rect.left(), rect.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, TITLE, title_font(), theme::TEXT);
    let right = pos2(rect.right(), rect.center().y);
    if widgets::accent_button(ui, controls, right, world_form::NEW_WORLD).clicked() {
        state.world_form = Some(WorldForm::new_world());
    }
    if !state.status.is_empty() {
        let text = egui::RichText::new(&state.status).font(theme::body());
        ui.add(egui::Label::new(text.color(theme::MUTE)).wrap());
        ui.add_space(GAP);
    }
}

/// The cards, in rows of equal columns. The grid scrolls.
fn grid(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, bottom: f32) {
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            let width = ui.available_width();
            // A narrow screen has two columns.
            let columns = ((width + CARD_GAP) / (CARD_MIN + CARD_GAP))
                .floor()
                .max(2.0);
            let card_width = (width - (columns - 1.0) * CARD_GAP) / columns;
            let size = vec2(card_width, card_width + INFO);
            let entries = state.browser.entries.clone();
            for row in entries.chunks(columns as usize) {
                let (rect, _) = ui.allocate_exact_size(vec2(width, size.y), egui::Sense::hover());
                for (column, entry) in row.iter().enumerate() {
                    let left = rect.left() + column as f32 * (card_width + CARD_GAP);
                    let rect = Rect::from_min_size(pos2(left, rect.top()), size);
                    ui.push_id(&entry.path, |ui| card(ui, state, controls, rect, entry));
                }
                ui.add_space(CARD_GAP);
            }
            ui.add_space(bottom);
        });
}

/// One project: the picture, the name, the date, and the actions button.
fn card(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rect: Rect,
    entry: &Entry,
) {
    let open = widgets::tap_area(ui, controls, rect, &open_name(&entry.name), true);
    let painter = ui.painter();
    painter.rect_filled(rect, theme::CARD_RADIUS, theme::card(false).to_opaque());
    if open.is_pointer_button_down_on() {
        painter.rect_filled(rect, theme::CARD_RADIUS, theme::raise());
    }

    let picture = Rect::from_min_size(rect.min, vec2(rect.width(), rect.width())).shrink(CARD_PAD);
    painter.rect_filled(picture, theme::CONTROL_RADIUS, theme::track());
    match state.browser.texture(ui.ctx(), entry) {
        Some(texture) => egui::Image::new(&texture).paint_at(ui, picture),
        // A file of an older version has no picture until it is open one
        // time.
        None => {
            let stroke = egui::Stroke::new(1.5, theme::divider());
            let radius = 0.45 * picture.width();
            painter.circle_stroke(picture.center(), radius, stroke);
        }
    }

    let info = Rect::from_min_max(
        pos2(rect.left() + BODY_PAD, picture.bottom()),
        pos2(rect.right() - PAD, rect.bottom()),
    );
    let button = Rect::from_center_size(
        pos2(info.right() - TOUCH / 2.0, info.center().y),
        vec2(TOUCH, TOUCH),
    );
    let text_width = button.left() - info.left() - 4.0;
    let name = one_line(ui, &entry.name, theme::title(), theme::TEXT, text_width);
    painter.galley(pos2(info.left(), info.top() + 10.0), name, theme::TEXT);
    let (line, color) = match (&entry.error, entry.modified) {
        (Some(_), _) => ("Cannot read the file".to_string(), theme::WARN),
        (None, Some(time)) => (projects::date_text(time), theme::MUTE),
        (None, None) => (String::new(), theme::MUTE),
    };
    let line = one_line(ui, &line, date_font(), color, text_width);
    painter.galley(pos2(info.left(), info.top() + 33.0), line, color);

    let menu_open = state.browser.menu.as_ref();
    let active = menu_open.is_some_and(|menu| menu.path == entry.path);
    let name = actions_name(&entry.name);
    let actions = widgets::icon_button_at(ui, controls, button, &name, Icon::More, active, true);
    if actions.clicked() {
        state.browser.menu = Some(ActionsMenu {
            path: entry.path.clone(),
            button,
        });
    } else if open.clicked() {
        match &entry.error {
            // The card has no room for the whole error.
            Some(error) => {
                let browser = &mut state.browser;
                browser.show_problem(entry.path.clone(), error.clone());
            }
            None => state.actions.push(Action::OpenWorld(entry.path.clone())),
        }
    }
}

/// A fill over the whole screen that takes each touch. Returns `true` at a
/// tap on the fill.
fn scrim(ctx: &egui::Context, id: &str, screen: Rect, alpha: u8) -> bool {
    let area = egui::Area::new(egui::Id::new(id))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .constrain(false)
        .fade_in(false);
    let shown = area.show(ctx, |ui| {
        let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
        let fill = egui::Color32::from_black_alpha(alpha);
        ui.painter().rect_filled(rect, 0.0, fill);
        resp.clicked()
    });
    shown.inner
}

/// Draws a card above a scrim. A touch on the scrim does not put it above
/// the card.
fn card_above(ctx: &egui::Context, id: &str, rect: Rect, add_contents: impl FnOnce(&mut egui::Ui)) {
    let radius = theme::CARD_RADIUS.into();
    widgets::card(ctx, None, id, rect, Order::Foreground, radius, |ui| {
        ui.painter()
            .rect_filled(rect, radius, theme::card(false).to_opaque());
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        add_contents(ui);
    });
    ctx.move_to_top(egui::LayerId::new(Order::Foreground, egui::Id::new(id)));
}

/// The menu of the actions of one project, next to its button.
fn actions_menu(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    screen: Rect,
    bands: &Bands,
) {
    let Some(menu) = state.browser.menu.clone() else {
        return;
    };
    let entries = &state.browser.entries;
    let Some(entry) = entries.iter().find(|e| e.path == menu.path).cloned() else {
        state.browser.menu = None;
        return;
    };
    let readable = entry.error.is_none();
    let rows = if readable { 3.0 } else { 1.0 };
    let rule_height = if readable { 13.0 } else { 0.0 };
    let size = vec2(MENU_WIDTH, 10.0 + rows * PANEL_ROW + rule_height + 10.0);
    // The menu opens below the button, or above it near the bottom of the
    // screen.
    let lowest = screen.bottom() - bands.bottom - MARGIN - size.y;
    let top = if menu.button.bottom() + 4.0 <= lowest {
        menu.button.bottom() + 4.0
    } else {
        (menu.button.top() - 4.0 - size.y).max(screen.top() + bands.top)
    };
    let left = (menu.button.right() - size.x).max(screen.left() + bands.left + MARGIN);
    let rect = Rect::from_min_size(pos2(left, top), size);

    let mut open = !scrim(ctx, "pad-project-menu-scrim", screen, 0);
    // The menu covers each control of the list.
    controls.clear();
    card_above(ctx, "pad-project-menu", rect, |ui| {
        let inner = rect.shrink2(vec2(BODY_PAD, 10.0));
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        let ui = &mut ui;
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let (path, name) = (entry.path.clone(), entry.name.clone());
        if readable {
            if widgets::text_row(ui, controls, RENAME, 0.0, false, true).clicked() {
                state.browser.ask_rename(path.clone(), name.clone());
                open = false;
            }
            if widgets::text_row(ui, controls, COPY, 0.0, false, true).clicked() {
                state.actions.push(Action::CopyProject(path.clone()));
                open = false;
            }
            rule(ui);
        }
        if widgets::text_row(ui, controls, DELETE, 0.0, false, true).clicked() {
            state.browser.ask_delete(path, name);
            open = false;
        }
    });
    if !open {
        state.browser.menu = None;
    }
}

/// The dialog that asks for a new name, or the dialog that asks before a
/// delete. The rename dialog is near the top of the screen, so that the
/// keyboard of the system does not cover it.
fn dialog(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    screen: Rect,
    bands: &Bands,
) {
    let Some(mut dialog) = state.browser.dialog.take() else {
        return;
    };
    let width = DIALOG_WIDTH.min(screen.width() - 2.0 * MARGIN);
    let left = screen.center().x - width / 2.0;
    let mut close = scrim(ctx, "pad-project-dialog-scrim", screen, 120);
    let mut action = None;
    // The dialog covers each other control.
    controls.clear();
    match &mut dialog {
        Dialog::Rename {
            path,
            name,
            focused,
        } => {
            let height = ROW + PANEL_ROW + 14.0 + PANEL_ROW + 16.0;
            let top = screen.top() + bands.top + MARGIN + ROW + GAP;
            let rect = Rect::from_min_size(pos2(left, top), vec2(width, height));
            card_above(ctx, "pad-project-dialog", rect, |ui| {
                close |= widgets::title_row(ui, controls, RENAME_TITLE, None, false).close;
                let inner = ui.available_rect_before_wrap().shrink2(vec2(BODY_PAD, 0.0));
                let layout = egui::Layout::top_down(egui::Align::Min);
                let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
                let ui = &mut ui;
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                let valid = !name.trim().is_empty();
                let field = widgets::text_field(ui, controls, NAME, NAME, None, name, valid);
                if !*focused {
                    *focused = true;
                    field.request_focus();
                }
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(14.0);
                let save = widgets::button_row(ui, controls, SAVE_NAME, valid);
                if valid && (save.clicked() || enter) {
                    action = Some(Action::RenameProject(path.clone(), name.clone()));
                }
            });
        }
        Dialog::Problem { path, error } => {
            let text = problem_text(path, error);
            let font = theme::body();
            let wrap = width - 2.0 * BODY_PAD;
            let lines = ctx.fonts_mut(|f| f.layout(text.clone(), font, theme::MUTE, wrap));
            let text_height = lines.size().y + 18.0;
            let height = ROW + text_height + PANEL_ROW + 16.0;
            let most = screen.height() - bands.top - bands.bottom - 2.0 * MARGIN;
            let height = height.min(most);
            let top = (screen.center().y - height / 2.0).max(screen.top() + bands.top);
            let rect = Rect::from_min_size(pos2(left, top), vec2(width, height));
            card_above(ctx, "pad-project-dialog", rect, |ui| {
                close |= widgets::title_row(ui, controls, PROBLEM_TITLE, None, false).close;
                let inner = ui.available_rect_before_wrap().shrink2(vec2(BODY_PAD, 0.0));
                let layout = egui::Layout::top_down(egui::Align::Min);
                let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
                let ui = &mut ui;
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                let room = height - ROW - PANEL_ROW - 16.0;
                egui::ScrollArea::vertical()
                    .max_height(room)
                    .min_scrolled_height(room)
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let text = egui::RichText::new(text).font(theme::body());
                        ui.add(egui::Label::new(text.color(theme::MUTE)).wrap());
                    });
                if widgets::button_row(ui, controls, TRY_OPEN, true).clicked() {
                    action = Some(Action::OpenWorld(path.clone()));
                }
            });
        }
        Dialog::Delete { path, name } => {
            let text = format!("\"{name}\" and its undo steps go away. You cannot get them back.");
            let text_height = 66.0;
            let height = ROW + text_height + PANEL_ROW + 16.0;
            let top = (screen.center().y - height / 2.0).max(screen.top() + bands.top);
            let rect = Rect::from_min_size(pos2(left, top), vec2(width, height));
            card_above(ctx, "pad-project-dialog", rect, |ui| {
                close |= widgets::title_row(ui, controls, DELETE_TITLE, None, false).close;
                let inner = ui.available_rect_before_wrap().shrink2(vec2(BODY_PAD, 0.0));
                let layout = egui::Layout::top_down(egui::Align::Min);
                let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
                let ui = &mut ui;
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                let size = vec2(ui.available_width(), text_height);
                ui.allocate_ui_with_layout(size, layout, |ui| {
                    ui.set_min_height(text_height);
                    let text = egui::RichText::new(text).font(theme::body());
                    ui.add(egui::Label::new(text.color(theme::MUTE)).wrap());
                });
                let delete = widgets::button_row_in(ui, controls, DELETE_TITLE, true, theme::WARN);
                if delete.clicked() {
                    action = Some(Action::DeleteProject(path.clone()));
                }
            });
        }
    }
    if let Some(action) = action {
        state.actions.push(action);
        close = true;
    }
    if !close {
        state.browser.dialog = Some(dialog);
    }
}
