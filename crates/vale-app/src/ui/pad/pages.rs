//! The Pages panel of the atlas: a row for each page, with a small picture
//! of the page, its name, and its size. A tap opens a page. A finger that
//! holds a row lifts it, and the row then moves with the finger to a new
//! place in the order. A swipe to the left shows "Delete page". The + in
//! the title row adds a page after the open page.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};
use vale_store::{PageId, PageRecord};

use super::geometry::{Rects, TOUCH};
use super::icons::Icon;
use super::widgets::{self, Controls, HIGHLIGHT_INSET};
use super::{Card, CardTitle, HOLD_SLOP, HOLD_TIME, PAGES, show_card, swipe, theme};
use crate::atlas as math;
use crate::ui::AppState;
use crate::ui::atlas;

/// The height of a row of the list.
pub const ROW: f32 = 52.0;
/// The width of the Delete button of an open row.
const DELETE_WIDTH: f32 = 112.0;
/// The text of the Delete button of an open row.
pub const DELETE_PAGE: &str = super::atlas::DELETE_PAGE;
/// The action button of the title row.
pub const ADD_PAGE: &str = super::atlas::ADD_PAGE;

/// The name of the row of a page, for example "Page 2".
pub fn row_name(index: usize, page: &PageRecord) -> String {
    super::atlas::page_title(index, &page.name)
}

/// The name of the Delete button of the row of a page.
pub fn delete_name(index: usize, page: &PageRecord) -> String {
    format!("Delete {}", row_name(index, page))
}

/// The short size of a page: the name of its preset, for example "A4", or
/// its width and height.
pub fn size_text(page: &PageRecord) -> String {
    let (w, h) = (page.width_mm, page.height_mm);
    match math::preset_of(w, h) {
        "Custom" => format!(
            "{} × {} {}",
            super::atlas::length_text(w, page.units),
            super::atlas::length_text(h, page.units),
            super::atlas::unit_name(page.units)
        ),
        preset => preset.to_owned(),
    }
}

/// A row that moves with a finger.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Reorder {
    pub page: PageId,
    /// The distance from the top of the row to the finger.
    grab: f32,
}

/// The state of the Pages panel.
#[derive(Default)]
pub struct PagesState {
    pub reorder: Option<Reorder>,
    pub swipe: Option<swipe::Swipe<PageId>>,
}

/// Shows the Pages panel. Returns its rectangle.
pub fn panel(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let head = CardTitle {
        title: PAGES,
        back: None,
        action: Some((ADD_PAGE, Icon::Plus, false)),
    };
    let scroll = match state.pad.pages.reorder {
        Some(_) => egui::scroll_area::ScrollSource::NONE,
        None => egui::scroll_area::ScrollSource::default(),
    };
    let card = Card {
        key: "pad-atlas-pages",
        place: (rects.right_panel, super::atlas::panel_width(ctx)),
        head,
        scroll,
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.close {
            state.pad.close(super::Panel::Pages);
        }
        if row.action.as_ref().is_some_and(|a| a.clicked()) {
            state.pad.pages.swipe = None;
            atlas::add_page_after(state);
        }
    };
    let top = |_: &mut egui::Ui, _: &mut AppState, _: &mut Controls| {};
    let rect = show_card(ctx, state, controls, rects, card, on_title, top, list);
    state.pad.panels.push((super::Panel::Pages, rect));
}

/// The list of the pages.
fn list(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let pages = state.doc.project.atlas.pages.clone();
    let size = vec2(ui.available_width(), pages.len() as f32 * ROW);
    let (area, _) = ui.allocate_exact_size(size, Sense::hover());
    let slot = |top: f32| Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), ROW));
    let place = |i: usize| slot(area.top() + i as f32 * ROW);
    let open = state.atlas.page;
    let (pointer, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_down()));

    let lifted = state
        .pad
        .pages
        .reorder
        .and_then(|r| Some((r, pages.iter().position(|p| p.id == r.page)?)));
    let Some((reorder, from)) = lifted else {
        state.pad.pages.reorder = None;
        swipe_rows(ui, state, controls, &pages, area);
        return;
    };
    let y = pointer.map_or(place(from).top() + reorder.grab, |p| p.y);
    let top = (y - reorder.grab).clamp(area.top(), area.bottom() - ROW);
    let to = ((top - area.top()) / ROW).round() as usize;
    let others = pages.iter().enumerate().filter(|(i, _)| *i != from);
    for (k, (i, page)) in others.enumerate() {
        // The rows make room at the place where the lifted row goes.
        let rect = place(if k < to { k } else { k + 1 });
        paint_row(ui, rect, i, page, page.id == open, false, 0.0);
    }
    paint_row(ui, slot(top), from, &pages[from], true, true, 0.0);
    if !down {
        state.pad.pages.reorder = None;
        if to != from {
            atlas::move_page(state, reorder.page, to);
        }
    }
}

/// The rows of the list that take touch. A sideways drag on a row swipes
/// it, and a drag up or down scrolls the list.
fn swipe_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    pages: &[PageRecord],
    area: Rect,
) {
    let place = |i: usize| {
        let top = area.top() + i as f32 * ROW;
        Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), ROW))
    };
    // The last page has no Delete button.
    let deletable = pages.len() > 1;
    if !deletable {
        state.pad.pages.swipe = None;
    }
    swipe::shut_outside(ui, &mut state.pad.pages.swipe, |id| {
        pages.iter().position(|p| p.id == id).map(place)
    });
    let open_page = state.atlas.page;
    for (i, page) in pages.iter().enumerate() {
        let (id, rect) = (page.id, place(i));
        let opened = state.pad.pages.swipe.filter(|s| s.row == id);
        let shift = swipe::shift(ui, state.pad.pages.swipe, id);
        let name = row_name(i, page);
        let body = Rect::from_min_max(rect.min, pos2(rect.right() + 8.0 + shift, rect.bottom()));
        let body = widgets::hit(
            ui,
            controls,
            body,
            &name,
            Sense::click_and_drag(),
            true,
            Some(id == open_page),
        );
        let pressed = body.is_pointer_button_down_on();
        if delete_behind(ui, controls, rect, &delete_name(i, page), shift) {
            state.pad.pages.swipe = None;
            atlas::delete_page_id(state, id);
            continue;
        }
        paint_row(ui, rect, i, page, id == open_page || pressed, false, shift);
        if deletable && swipe::drag(ui, &mut state.pad.pages.swipe, id, &body, DELETE_WIDTH) {
            continue;
        }
        if body.dragged() {
            ui.scroll_with_delta(vec2(0.0, body.drag_delta().y));
        }
        if body.clicked() {
            if opened.is_some() {
                state.pad.pages.swipe = None;
            } else {
                atlas::open_page_id(state, id);
            }
        }
        if !pressed || body.dragged() || state.pad.pages.swipe.is_some() {
            continue;
        }
        let press = ui.input(|i| {
            let (start, origin) = (i.pointer.press_start_time()?, i.pointer.press_origin()?);
            let pos = i.pointer.interact_pos()?;
            Some((i.time - start, origin.distance(pos), pos.y))
        });
        match press {
            Some((held, moved, y)) if moved <= HOLD_SLOP && held >= HOLD_TIME => {
                state.pad.pages.reorder = Some(Reorder {
                    page: id,
                    grab: y - rect.top(),
                });
                ui.ctx().request_repaint();
            }
            Some((held, moved, _)) if moved <= HOLD_SLOP => {
                let wait = std::time::Duration::from_secs_f64(HOLD_TIME - held);
                ui.ctx().request_repaint_after(wait);
            }
            _ => {}
        }
    }
}

/// The Delete button behind a row that a swipe moves `shift` to the left.
/// Returns true if the button was tapped.
fn delete_behind(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    shift: f32,
) -> bool {
    if shift >= 0.0 {
        return false;
    }
    let edge = rect.right() + 8.0;
    let back = Rect::from_min_max(
        pos2(edge + shift, rect.top() + HIGHLIGHT_INSET),
        pos2(edge, rect.bottom() - HIGHLIGHT_INSET),
    );
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(back));
    painter.rect_filled(back, 5.0, theme::WARN);
    painter.text(
        pos2(edge - DELETE_WIDTH / 2.0, rect.center().y),
        Align2::CENTER_CENTER,
        DELETE_PAGE,
        theme::body(),
        theme::CANVAS,
    );
    if -shift < DELETE_WIDTH - 0.5 {
        return false;
    }
    let area = Rect::from_min_max(
        pos2(edge - DELETE_WIDTH, rect.center().y - TOUCH / 2.0),
        pos2(edge, rect.center().y + TOUCH / 2.0),
    );
    widgets::tap_area(ui, controls, area, name, true).clicked()
}

/// Paints a row: the picture of the page, its name, and its size. A
/// negative `shift` moves the row to the left over its Delete button.
fn paint_row(
    ui: &egui::Ui,
    rect: Rect,
    index: usize,
    page: &PageRecord,
    on: bool,
    lifted: bool,
    shift: f32,
) {
    let clip = Rect::from_min_max(
        pos2(rect.left() - 8.0, rect.top()),
        pos2(rect.right() + 8.0 + shift, rect.bottom()),
    );
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(clip));
    let rect = rect.translate(vec2(shift, 0.0));
    let highlight = rect.expand2(vec2(8.0, -HIGHLIGHT_INSET));
    if lifted {
        painter.rect_filled(highlight, 5.0, theme::card(false).to_opaque());
    }
    if on || lifted {
        painter.rect_filled(highlight, 5.0, theme::raise());
    }
    let picture = Rect::from_min_size(pos2(rect.left(), rect.center().y - 16.0), vec2(48.0, 32.0));
    thumbnail(&painter, picture, page);
    let size = size_text(page);
    let size_width = painter
        .layout_no_wrap(size.clone(), theme::body(), theme::MUTE)
        .size()
        .x;
    let right = rect.right() - 4.0;
    painter.text(
        pos2(right, rect.center().y),
        Align2::RIGHT_CENTER,
        size,
        theme::body(),
        theme::MUTE,
    );
    let name_clip = Rect::from_min_max(
        pos2(picture.right() + 12.0, rect.top()),
        pos2(right - size_width - 8.0, rect.bottom()),
    );
    painter
        .with_clip_rect(painter.clip_rect().intersect(name_clip))
        .text(
            name_clip.left_center(),
            Align2::LEFT_CENTER,
            row_name(index, page),
            theme::body(),
            theme::TEXT,
        );
}

/// A small picture of a page in `area`: the paper, and the outlines of its
/// frames and of its items of furniture.
fn thumbnail(painter: &egui::Painter, area: Rect, page: &PageRecord) {
    let (w, h) = (page.width_mm as f32, page.height_mm as f32);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let scale = (area.width() / w).min(area.height() / h);
    let paper = Rect::from_center_size(area.center(), vec2(w * scale, h * scale));
    let [r, g, b] = page.paper;
    painter.rect_filled(paper, 2.0, egui::Color32::from_rgb(r, g, b));
    let to_screen = |r: kurbo::Rect| {
        Rect::from_min_max(
            paper.min + vec2(r.x0 as f32, r.y0 as f32) * scale,
            paper.min + vec2(r.x1 as f32, r.y1 as f32) * scale,
        )
    };
    let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(110));
    for frame in &page.frames {
        let rect = to_screen(math::shape_bbox(&frame.shape));
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(214, 228, 236));
        painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Inside);
    }
    let thin = egui::Stroke::new(1.0, egui::Color32::from_gray(150));
    for item in &page.furniture {
        let rect = to_screen(crate::furniture::rect_of(item));
        painter.rect_stroke(rect, 0.0, thin, egui::StrokeKind::Inside);
    }
}
