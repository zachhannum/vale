//! The readout at the lower left of the canvas of the World workspace, its
//! View popover, and the banner of a save that failed.

use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, Order, Rect, Sense, pos2, vec2};
use vale_sphere::{ProjectionKind, ProjectionSpec};

use super::geometry::{self, PAD, ROW, Rects, TOUCH};
use super::icons::{self, Icon};
use super::widgets::{self, BODY_PAD, Controls, PANEL_ROW};
use super::{DIVIDER, GRATICULE, PROJECTION, RECENTER, RESET, VIEW, VIEWS, theme};
use crate::globe::WorldView;
use crate::ui::{AppState, SaveMark};

/// The time that the place stays after the last touch on the canvas.
pub const PLACE_TIME: Duration = Duration::from_secs(1);
/// The height of the card of the place.
pub const PLACE_HEIGHT: f32 = 34.0;
/// The space between the card of the place and the card below it.
const PLACE_GAP: f32 = 6.0;
/// The space at the sides of a text in a card of the readout.
pub(super) const READOUT_PAD: f32 = 12.0;
/// The space between the divider of the readout and the scale bar.
const SCALE_GAP: f32 = 6.0;
/// The width of the View popover. A narrow screen makes it narrower.
pub const VIEW_MENU_WIDTH: f32 = 340.0;
/// The height of the segmented control of the View popover.
const TABS: f32 = TOUCH + 2.0 * PAD;
/// The size of the icon of a segment.
const TAB_ICON: f32 = 18.0;
/// The text of the banner of a save that failed.
pub const SAVE_FAILED: &str = "Could not save. Changes are kept and saved again.";
/// The side of the warning sign of the banner.
const WARNING: f32 = 14.0;
/// The fill of the banner of a save that failed, and its warning sign.
const BANNER: Color32 = Color32::from_rgb(70, 30, 30);
const ALERT: Color32 = Color32::from_rgb(238, 116, 100);

/// True if the place shows above the readout. It shows while a touch is
/// down on the canvas, while the Measure tool has a result, and for
/// `PLACE_TIME` after `last_down`, the last time that a touch was down.
pub fn shows_place(down: bool, measure: bool, last_down: Option<Instant>, now: Instant) -> bool {
    let recent = |at: Instant| now.saturating_duration_since(at) < PLACE_TIME;
    down || measure || last_down.is_some_and(recent)
}

/// The width of a text in the monospace font.
pub(super) fn mono_width(ctx: &egui::Context, text: &str) -> f32 {
    ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), theme::mono(), theme::TEXT))
        .size()
        .x
        .ceil()
}

/// Paints a text in the monospace font at the left of `place`, at its
/// middle.
pub(super) fn paint_line(ui: &egui::Ui, place: Rect, text: &str) {
    let painter = ui.painter_at(place.expand(2.0));
    let at = pos2(place.left(), place.center().y);
    painter.text(at, Align2::LEFT_CENTER, text, theme::mono(), theme::TEXT);
}

/// The readout at the lower left of the canvas: a card with the View button
/// and the scale bar at the bottom of the band, and the card of the place
/// above it while it shows.
pub(super) fn readout(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    let pad = &mut state.pad;
    pad.readout_scale = None;
    pad.view_rect = None;
    pad.view_button = None;
    pad.place_rect = None;
    let Some(band) = rects.readout else {
        state.pad.view_menu = false;
        return;
    };
    let flat = state.globe.world_view == WorldView::Flat;
    let world = &state.doc.project.world;
    let scale = km_per_point(&state.globe, world.radius_km)
        .and_then(|km| crate::scale_bar::readout_scale(km, world.units));
    state.pad.readout_scale = scale.map(|s| s.label());
    let width = match scale {
        Some(s) => PAD + TOUCH + DIVIDER + SCALE_GAP + scale_width(ctx, s) + READOUT_PAD,
        None => ROW,
    };
    let rect = Rect::from_min_size(
        pos2(band.left(), band.bottom() - ROW),
        vec2(width.min(band.width()), ROW),
    );
    let button = Rect::from_min_size(rect.min + vec2(PAD, PAD), vec2(TOUCH, TOUCH));
    state.pad.view_rect = Some(rect);
    state.pad.view_button = Some(button);
    let backdrop = state.globe.backdrop();
    widgets::card(
        ctx,
        backdrop,
        "pad-view",
        rect,
        Order::Middle,
        theme::CARD_RADIUS.into(),
        |ui| {
            view_button(ui, state, controls, button, flat);
            if let Some(scale) = scale {
                let x = button.right() + DIVIDER / 2.0;
                let line = Rect::from_center_size(pos2(x, rect.center().y), vec2(1.0, 24.0));
                ui.painter().rect_filled(line, 0.0, theme::divider());
                let left = button.right() + DIVIDER + SCALE_GAP;
                paint_scale(ui, pos2(left, rect.center().y), rect, scale);
            }
        },
    );
    place_card(ctx, state, band, rect);
}

/// The card of the place, above the card of the readout, while
/// `shows_place` is true. A test has no clock, so there the place does not
/// stay after the touch.
fn place_card(ctx: &egui::Context, state: &mut AppState, band: Rect, card: Rect) {
    let now = Instant::now();
    let down = state.pad.canvas_down;
    if down && !state.headless {
        state.pad.last_down = Some(now);
    }
    let last = state.pad.last_down.filter(|_| !state.headless);
    let measure = state.measure_readout().is_some();
    if !shows_place(down, measure, last, now) {
        return;
    }
    if let Some(at) = last.filter(|_| !down && !measure) {
        ctx.request_repaint_after(PLACE_TIME.saturating_sub(now.saturating_duration_since(at)));
    }
    let text = super::readout_line(state);
    let width = (mono_width(ctx, &text) + 2.0 * READOUT_PAD).min(band.width());
    let top = card.top() - PLACE_GAP - PLACE_HEIGHT;
    let rect = Rect::from_min_size(pos2(band.left(), top), vec2(width, PLACE_HEIGHT));
    state.pad.place_rect = Some(rect);
    widgets::card(
        ctx,
        state.globe.backdrop(),
        "pad-place",
        rect,
        Order::Middle,
        theme::CARD_RADIUS.into(),
        |ui| paint_line(ui, rect.shrink2(vec2(READOUT_PAD, 0.0)), &text),
    );
}

/// The button that opens the View popover. It shows the icon of the
/// present view, and it has the accent color while the popover is open.
fn view_button(
    ui: &egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    rect: Rect,
    flat: bool,
) {
    let open = state.pad.view_menu;
    let resp = widgets::hit(ui, controls, rect, VIEW, Sense::click(), true, Some(open));
    let painter = ui.painter();
    let inner = rect.shrink(widgets::HIGHLIGHT_INSET);
    if open {
        let fill = theme::ACCENT.gamma_multiply(0.3);
        painter.rect_filled(inner, theme::CONTROL_RADIUS, fill);
    }
    if resp.is_pointer_button_down_on() {
        painter.rect_filled(inner, theme::CONTROL_RADIUS, theme::raise());
    }
    let color = if open { theme::ACCENT } else { theme::MUTE };
    let icon = if flat { Icon::Map } else { Icon::World };
    icons::paint(painter, rect, icon, widgets::ICON, color);
    if resp.clicked() {
        state.pad.view_menu = !open;
        state.pad.projections = false;
    }
}

/// The width of the scale bar with the label at its end.
fn scale_width(ctx: &egui::Context, scale: crate::scale_bar::ReadoutScale) -> f32 {
    let label = ctx
        .fonts_mut(|f| f.layout_no_wrap(scale.label(), theme::small(), theme::TEXT))
        .size()
        .x;
    scale.length as f32 + (label / 2.0).ceil()
}

/// The scale bar with four divisions, and "0" and its round distance below
/// it. `at` is the left end, at the middle of the bar and the labels.
fn paint_scale(ui: &egui::Ui, at: egui::Pos2, card: Rect, scale: crate::scale_bar::ReadoutScale) {
    let painter = ui.painter_at(card);
    let bar = Rect::from_min_size(pos2(at.x, at.y - 9.0), vec2(scale.length as f32, 4.0));
    let part = bar.width() / 4.0;
    for i in 0..4 {
        let fill = if i % 2 == 0 {
            theme::TEXT
        } else {
            theme::track()
        };
        let x = bar.left() + i as f32 * part;
        let rect = Rect::from_min_max(pos2(x, bar.top()), pos2(x + part, bar.bottom()));
        painter.rect_filled(rect, 0.0, fill);
    }
    let labels = bar.bottom() + 2.0;
    let font = theme::small();
    let zero = pos2(bar.left(), labels);
    painter.text(zero, Align2::LEFT_TOP, "0", font.clone(), theme::MUTE);
    let end = pos2(bar.right(), labels);
    painter.text(end, Align2::CENTER_TOP, scale.label(), font, theme::MUTE);
}

/// The kilometers in one point at the center of the view. On the flat map,
/// the distance between two places 40 points apart.
fn km_per_point(globe: &crate::globe::Globe, world_km: f64) -> Option<f64> {
    let on_globe = world_km / globe.radius();
    if globe.world_view == WorldView::Flat {
        let center = globe.rect.center();
        let side = vec2(20.0, 0.0);
        if let (Some(a), Some(b)) = (
            globe.unproject(center - side),
            globe.unproject(center + side),
        ) {
            let km = crate::globe::math::angle(a, b) * world_km / 40.0;
            if km.is_finite() && km > 0.0 {
                return Some(km);
            }
        }
    }
    (on_globe.is_finite() && on_globe > 0.0).then_some(on_globe)
}

/// The place of a touch that started in this frame.
fn pressed_at(ctx: &egui::Context) -> Option<egui::Pos2> {
    ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    })
}

/// The View popover above the card of the readout, at its left edge, and
/// the list of the projections that its Projection row opens. The list is
/// at the right of the popover. Where it has no room there, it takes the
/// place of the popover. A touch outside them and outside the View button
/// closes them.
pub(super) fn view_menu(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
) {
    state.pad.view_menu_rect = None;
    state.pad.projections_rect = None;
    let flat = state.globe.world_view == WorldView::Flat;
    let (Some(card), Some(button)) = (state.pad.view_rect, state.pad.view_button) else {
        state.pad.view_menu = false;
        state.pad.projections = false;
        return;
    };
    if !state.pad.view_menu || !flat {
        state.pad.projections = false;
    }
    if !state.pad.view_menu {
        return;
    }
    let rows = if flat { 4.0 } else { 1.0 };
    let height = 10.0 + TABS + rows * PANEL_ROW + 10.0;
    let screen = rects.screen.shrink(geometry::PANEL_GAP);
    let width = VIEW_MENU_WIDTH.min(screen.right() - card.left());
    let top = (card.top() - geometry::PANEL_GAP - height).max(rects.column_top);
    let menu = Rect::from_min_size(pos2(card.left(), top), vec2(width, height));
    let list = |open: bool| {
        open.then(|| {
            let kinds = ProjectionKind::ALL.len() as f32;
            let size = vec2(geometry::MENU_WIDTH, 10.0 + kinds * PANEL_ROW + 10.0);
            let side = menu.right() + geometry::PANEL_GAP;
            let left = if side + size.x <= screen.right() {
                side
            } else {
                menu.left()
            };
            let top = (menu.bottom() - size.y).max(rects.column_top);
            Rect::from_min_size(pos2(left, top), size)
        })
    };
    let open = list(state.pad.projections);
    if let Some(pos) = pressed_at(ctx) {
        let in_list = open.is_some_and(|l| l.contains(pos));
        let covered = open.is_some_and(|l| l.left() == menu.left());
        let in_menu = !covered && menu.contains(pos);
        if !in_list && !in_menu && !button.contains(pos) {
            state.pad.view_menu = false;
            state.pad.projections = false;
            return;
        }
        if !in_list {
            state.pad.projections = false;
        }
    }
    let list = list(state.pad.projections);
    if !list.is_some_and(|l| l.left() == menu.left()) {
        state.pad.view_menu_rect = Some(menu);
        menu_card(ctx, "pad-view-menu", menu, |ui| {
            view_rows(ui, state, controls, flat);
        });
    }
    if let Some(list) = list {
        state.pad.projections_rect = Some(list);
        projections(ctx, state, controls, list);
    }
}

/// The rows of the View popover.
fn view_rows(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, flat: bool) {
    if let Some(view) = view_tabs(ui, controls, state.globe.world_view) {
        state.globe.world_view = view;
    }
    let graticule = &mut state.globe.preview.graticule;
    widgets::switch_row(ui, controls, GRATICULE, GRATICULE, graticule);
    if !flat {
        return;
    }
    rule(ui);
    let name = state.globe.flat.spec().kind.name();
    if widgets::open_row(ui, controls, PROJECTION, name).clicked() {
        state.pad.projections = true;
    }
    rule(ui);
    let flat = &mut state.globe.flat;
    let can = flat.can_center_here();
    if widgets::text_row(ui, controls, RECENTER, 0.0, false, can).clicked() {
        flat.center_here();
    }
    rule(ui);
    let can = flat.can_reset();
    if widgets::text_row(ui, controls, RESET, 0.0, false, can).clicked() {
        flat.reset();
    }
}

/// A line between two rows of a menu, at the top of the next row.
fn rule(ui: &egui::Ui) {
    let y = ui.cursor().top();
    let x = ui.max_rect().x_range();
    let stroke = egui::Stroke::new(1.0, theme::divider());
    ui.painter().hline(x, y, stroke);
}

/// The segmented control of the globe and the flat view, with an icon and a
/// name in each segment. Returns the view that the user chose.
fn view_tabs(ui: &mut egui::Ui, controls: &mut Controls, view: WorldView) -> Option<WorldView> {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), TABS), Sense::hover());
    ui.painter()
        .rect_filled(row, theme::CONTROL_RADIUS, theme::track());
    let inner = row.shrink(PAD);
    let width = inner.width() / 2.0;
    let views = [
        (VIEWS[0], Icon::World, WorldView::Globe),
        (VIEWS[1], Icon::Map, WorldView::Flat),
    ];
    let mut chosen = None;
    for (i, (name, icon, segment)) in views.into_iter().enumerate() {
        let min = pos2(inner.left() + i as f32 * width, inner.top());
        let rect = Rect::from_min_size(min, vec2(width, TOUCH));
        let on = view == segment;
        let resp = widgets::hit(ui, controls, rect, name, Sense::click(), true, Some(on));
        let painter = ui.painter();
        if on || resp.is_pointer_button_down_on() {
            let radius = theme::CONTROL_RADIUS - 2.0;
            painter.rect_filled(rect.shrink(2.0), radius, theme::raise());
        }
        let color = if on { theme::TEXT } else { theme::MUTE };
        let galley = painter.layout_no_wrap(name.to_owned(), theme::body(), color);
        let left = rect.center().x - (TAB_ICON + 6.0 + galley.size().x) / 2.0;
        let y = rect.center().y;
        let at = Rect::from_center_size(pos2(left + TAB_ICON / 2.0, y), vec2(TAB_ICON, TAB_ICON));
        icons::paint(painter, at, icon, TAB_ICON, color);
        let text = pos2(left + TAB_ICON + 6.0, y - galley.size().y / 2.0);
        painter.galley(text, galley, color);
        if resp.clicked() && !on {
            chosen = Some(segment);
        }
    }
    chosen
}

/// An opaque card for a menu, with rows from the top.
fn menu_card(ctx: &egui::Context, id: &str, rect: Rect, contents: impl FnOnce(&mut egui::Ui)) {
    let radius = theme::CARD_RADIUS.into();
    widgets::card(ctx, None, id, rect, Order::Foreground, radius, |ui| {
        // The menu covers other cards, so its fill is opaque.
        ui.painter()
            .rect_filled(rect, radius, theme::card(false).to_opaque());
        let inner = rect.shrink2(vec2(BODY_PAD, 10.0));
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        contents(&mut ui);
    });
}

/// The list of the projections of the flat view in `rect`. A tap on a row
/// picks the projection and closes the list.
fn projections(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rect: Rect) {
    menu_card(ctx, "pad-projections", rect, |ui| {
        let spec = state.globe.flat.spec();
        for kind in ProjectionKind::ALL {
            let row = widgets::text_row(ui, controls, kind.name(), 0.0, false, true);
            if kind == spec.kind {
                let tick = Rect::from_center_size(
                    pos2(row.rect.right() - 14.0, row.rect.center().y),
                    vec2(18.0, 18.0),
                );
                icons::paint(ui.painter(), tick, Icon::Tick, 18.0, theme::ACCENT);
            }
            if row.clicked() {
                state.globe.flat.set_spec(ProjectionSpec { kind, ..spec });
                state.pad.projections = false;
            }
        }
    });
}

/// The banner of a save that failed, at the top center of the screen below
/// the top row. It stays while the save state is failed, and a touch goes
/// through it.
pub(super) fn save_banner(ctx: &egui::Context, state: &mut AppState, rects: &Rects) {
    state.pad.save_banner = None;
    if state.save_mark(Instant::now()) != Some(SaveMark::Failed) {
        return;
    }
    let screen = rects.screen.shrink(geometry::PANEL_GAP);
    let lead = WARNING + 10.0;
    let side = BODY_PAD;
    let room = (screen.width() - 2.0 * side - lead).max(TOUCH);
    let galley =
        ctx.fonts_mut(|f| f.layout(SAVE_FAILED.to_owned(), theme::body(), theme::TEXT, room));
    let text = galley.size();
    let size = vec2(
        text.x.ceil() + lead + 2.0 * side,
        (text.y.ceil() + 24.0).max(TOUCH),
    );
    let left = (screen.center().x - size.x / 2.0).max(screen.left());
    let rect = Rect::from_min_size(pos2(left, rects.column_top), size);
    state.pad.save_banner = Some(rect);
    let radius = egui::CornerRadius::from(theme::CARD_RADIUS);
    egui::Area::new(egui::Id::new("pad-save-banner"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .interactable(false)
        .fade_in(false)
        .show(ctx, |ui| {
            let painter = ui.painter();
            painter.add(theme::shadow().as_shape(rect, radius));
            painter.rect_filled(rect, radius, BANNER);
            let y = rect.center().y;
            let sign = Rect::from_center_size(
                pos2(rect.left() + side + WARNING / 2.0, y),
                vec2(WARNING, WARNING),
            );
            paint_warning(painter, sign, ALERT);
            let at = pos2(rect.left() + side + lead, y - text.y / 2.0);
            painter.galley(at, galley, theme::TEXT);
        });
}

/// A warning sign: a triangle with an exclamation mark.
fn paint_warning(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = egui::Stroke::new(1.4, color);
    let (center, top) = (rect.center(), rect.top() + 1.0);
    let half = rect.width() / 2.0 - 0.5;
    let corners = vec![
        pos2(center.x, top),
        pos2(center.x + half, rect.bottom() - 1.0),
        pos2(center.x - half, rect.bottom() - 1.0),
    ];
    painter.add(egui::Shape::closed_line(corners, stroke));
    let line = [pos2(center.x, top + 4.5), pos2(center.x, top + 8.0)];
    painter.line_segment(line, stroke);
    painter.circle_filled(pos2(center.x, top + 10.2), 0.8, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_place_shows_while_a_touch_is_down_and_for_a_second_after_it() {
        let now = Instant::now();
        assert!(!shows_place(false, false, None, now));
        assert!(shows_place(true, false, None, now));
        assert!(shows_place(false, true, None, now));
        let just = now - Duration::from_millis(200);
        assert!(shows_place(false, false, Some(just), now));
        let long = now - PLACE_TIME;
        assert!(!shows_place(false, false, Some(long), now));
        // The Measure tool keeps the place.
        assert!(shows_place(false, true, Some(long), now));
    }
}
