//! The controls of the iPad layout. Each one is at least 44 points on each side.

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Response, Sense, pos2, vec2};

use eframe::egui_wgpu::wgpu;

use super::geometry::{self, CELL, PAD, ROW, Rects, TOUCH};
use super::icons::{self, Icon};
use super::theme;
use crate::globe::backdrop::{card_shape, marker};

/// A control that a finger can hit, as the last frame drew it.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub name: String,
    pub rect: Rect,
}

/// The list of the controls of one frame.
pub type Controls = Vec<Control>;

/// The padding at each side of the text of a segment.
const SEGMENT_PAD: f32 = 14.0;
pub const ICON: f32 = 22.0;
const THUMB: f32 = 26.0;
/// The height of a row in a panel.
pub const PANEL_ROW: f32 = 48.0;
/// The padding at the left and the right of the body of a panel.
pub const BODY_PAD: f32 = 16.0;

fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, theme::TEXT);
    galley.size().x.ceil()
}

/// A color of a control, faded while the control is off.
pub fn fade(color: Color32, enabled: bool) -> Color32 {
    if enabled {
        color
    } else {
        color.gamma_multiply(theme::OFF)
    }
}

/// A floating card. A touch on the card does not go to the canvas below it.
/// `backdrop`: the card shows the blurred canvas, in a texture of this format.
pub fn card(
    ctx: &egui::Context,
    backdrop: Option<wgpu::TextureFormat>,
    id: &str,
    rect: Rect,
    order: egui::Order,
    radius: CornerRadius,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Area::new(egui::Id::new(id))
        .order(order)
        .fixed_pos(rect.min)
        // A drag on the card goes to its controls, and the card stays in
        // its place.
        .movable(false)
        // A sheet paints past the safe area, to the bottom of the screen.
        // `constrain_to` also turns on the constraint, which moves the card
        // by the change of its size during a drag of the sheet.
        .constrain_to(ctx.viewport_rect())
        .constrain(false)
        .fade_in(false)
        .sizing_pass(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
            // The blur shows what the frame drew below this order, without
            // the shadow.
            if backdrop.is_some() {
                ui.painter().add(marker(order, rect));
            }
            ui.painter().add(theme::shadow().as_shape(rect, radius));
            if let Some(format) = backdrop {
                let shape = card_shape(format, rect, radius, ctx.pixels_per_point());
                ui.painter().add(shape);
            }
            ui.painter()
                .rect_filled(rect, radius, theme::card(backdrop.is_some()));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
            ui.set_clip_rect(rect);
            theme::apply(&mut ui);
            add_contents(&mut ui);
        });
}

/// A card of the chrome. A card that touches a side edge of the screen has
/// round inner corners only, and its controls leave out the band of the
/// system at that edge.
pub fn edge_card(
    ctx: &egui::Context,
    backdrop: Option<wgpu::TextureFormat>,
    id: &str,
    rect: Rect,
    rects: &Rects,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let radius = geometry::edge_radius(rect, rects.screen);
    let inner = geometry::edge_content(rect, rects.screen, rects.bands);
    card(ctx, backdrop, id, rect, egui::Order::Middle, radius, |ui| {
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        add_contents(&mut ui);
    });
}

/// Makes a rectangle a control. `selected` is the state of a control that
/// stays on. A control that is not enabled takes no touch. A control is not
/// in the list while a scroll area cuts it to less than the touch size.
pub(super) fn hit(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    sense: Sense,
    enabled: bool,
    selected: Option<bool>,
) -> Response {
    let sense = if enabled { sense } else { Sense::hover() };
    let resp = ui.interact(rect, ui.id().with(name), sense);
    resp.widget_info(|| match selected {
        Some(on) => egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, name),
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name),
    });
    if enabled {
        push(ui, controls, rect, name);
    }
    resp
}

/// Adds a control to the list, unless a scroll area cuts it to less than
/// the touch size.
pub(crate) fn push(ui: &egui::Ui, controls: &mut Controls, rect: Rect, name: &str) {
    let whole = rect;
    let rect = rect.intersect(ui.clip_rect());
    let cut = |whole: f32, part: f32| part < TOUCH.min(whole);
    let cut = cut(whole.width(), rect.width()) || cut(whole.height(), rect.height());
    if rect.is_positive() && !cut {
        controls.push(Control {
            name: name.to_owned(),
            rect,
        });
    }
}

/// The space between the highlight of a control and the edge of its touch
/// area. The highlights of two controls that touch have a margin of two times
/// this space.
pub const HIGHLIGHT_INSET: f32 = 2.0;

fn fill(ui: &egui::Ui, rect: Rect, resp: &Response, active: bool) {
    if active || resp.is_pointer_button_down_on() {
        let rect = rect.shrink(HIGHLIGHT_INSET);
        ui.painter()
            .rect_filled(rect, theme::CONTROL_RADIUS, theme::raise());
    }
}

/// A button with an icon, at a given place.
pub fn icon_button_at(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    icon: Icon,
    active: bool,
    enabled: bool,
) -> Response {
    let resp = hit(
        ui,
        controls,
        rect,
        name,
        Sense::click(),
        enabled,
        Some(active),
    );
    fill(ui, rect, &resp, active);
    let color = if active { theme::TEXT } else { theme::MUTE };
    icons::paint(ui.painter(), rect, icon, ICON, fade(color, enabled));
    resp
}

/// The width of a segmented list with these labels.
pub fn segments_width(ui: &egui::Ui, labels: &[&str]) -> f32 {
    let widths = labels
        .iter()
        .map(|label| (text_width(ui, label, theme::body()) + 2.0 * SEGMENT_PAD).max(TOUCH));
    widths.sum::<f32>() + 2.0 * PAD
}

/// A segmented list that fills a card. Each item is a label and its enabled
/// state. Returns the item that the user chose.
pub fn segments(
    ui: &egui::Ui,
    controls: &mut Controls,
    items: &[(&str, bool)],
    active: usize,
) -> Option<usize> {
    let card = ui.max_rect();
    let mut left = card.left() + PAD;
    let mut chosen = None;
    for (index, &(label, enabled)) in items.iter().enumerate() {
        let width = (text_width(ui, label, theme::body()) + 2.0 * SEGMENT_PAD).max(TOUCH);
        let rect = Rect::from_min_size(pos2(left, card.top() + PAD), vec2(width, TOUCH));
        left += width;
        let on = index == active;
        let resp = hit(ui, controls, rect, label, Sense::click(), enabled, Some(on));
        fill(ui, rect, &resp, on);
        let color = fade(if on { theme::TEXT } else { theme::MUTE }, enabled);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            theme::body(),
            color,
        );
        if resp.clicked() && !on {
            chosen = Some(index);
        }
    }
    chosen
}

/// The font of a small segmented control.
fn small_segment_font() -> egui::FontId {
    egui::FontId::proportional(13.0)
}

/// The padding of the track of a small segmented control.
const SMALL_TRACK_PAD: f32 = 3.0;

/// The width of a small segmented control with these labels.
pub fn small_segments_width(ui: &egui::Ui, labels: &[&str]) -> f32 {
    let widths = labels
        .iter()
        .map(|label| text_width(ui, label, small_segment_font()) + 24.0);
    widths.sum::<f32>() + 2.0 * SMALL_TRACK_PAD
}

/// A small segmented control on a dark track, for a choice in a row or a
/// title. The track is `rect`, and each item takes a touch of at least the
/// touch size. Returns the item that the user chose.
pub fn small_segments(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    labels: &[&str],
    active: usize,
) -> Option<usize> {
    small_segments_named(ui, controls, rect, (labels, labels), active)
}

/// A small segmented control whose items have the control names `names`.
pub fn small_segments_named(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    (labels, names): (&[&str], &[&str]),
    active: usize,
) -> Option<usize> {
    let painter = ui.painter();
    painter.rect_filled(rect, theme::CONTROL_RADIUS, Color32::from_black_alpha(77));
    let inner = rect.shrink(SMALL_TRACK_PAD);
    let total: f32 = labels
        .iter()
        .map(|l| text_width(ui, l, small_segment_font()) + 24.0)
        .sum();
    let scale = inner.width() / total.max(1.0);
    let mut left = inner.left();
    let mut chosen = None;
    for (i, label) in labels.iter().enumerate() {
        let w = (text_width(ui, label, small_segment_font()) + 24.0) * scale;
        let item = Rect::from_min_size(pos2(left, inner.top()), vec2(w, inner.height()));
        left += w;
        let touch = Rect::from_center_size(item.center(), vec2(w, item.height().max(TOUCH)));
        let on = i == active;
        let name = names.get(i).unwrap_or(label);
        let resp = hit(ui, controls, touch, name, Sense::click(), true, Some(on));
        if on || resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(item, 8.0, theme::raise());
        }
        let color = if on { theme::TEXT } else { theme::MUTE };
        ui.painter().text(
            item.center(),
            Align2::CENTER_CENTER,
            *label,
            small_segment_font(),
            color,
        );
        if resp.clicked() && !on {
            chosen = Some(i);
        }
    }
    chosen
}

/// The width of a menu button with this text.
pub fn menu_button_width(ui: &egui::Ui, text: &str) -> f32 {
    18.0 + text_width(ui, text, theme::body()) + 6.0 + 18.0 + 12.0
}

/// A button that fills a card and shows a text and a chevron.
pub fn menu_button(
    ui: &egui::Ui,
    controls: &mut Controls,
    name: &str,
    text: &str,
    open: bool,
) -> Response {
    let rect = ui.max_rect();
    let resp = hit(ui, controls, rect, name, Sense::click(), true, Some(open));
    if open || resp.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(rect, theme::CARD_RADIUS, theme::raise());
    }
    let at = pos2(rect.left() + 18.0, rect.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, text, theme::body(), theme::TEXT);
    let chevron =
        Rect::from_center_size(pos2(rect.right() - 21.0, rect.center().y), vec2(18.0, 18.0));
    icons::paint(ui.painter(), chevron, Icon::Down, 18.0, theme::MUTE);
    resp
}

/// The look of a tool cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellStyle {
    /// A cell of `CELL` with an icon, and the name below it for a
    /// `tool_cell`.
    #[default]
    Labeled,
    /// A cell of `COLUMN_CELL` with an icon alone, as in the tool column.
    /// A hold on a `tool_cell` shows its name next to it.
    Icon,
}

fn cell_style_id() -> egui::Id {
    egui::Id::new("pad-cell-style")
}

fn hold_id() -> egui::Id {
    egui::Id::new("pad-hold-label")
}

/// The look of the tool cells that `add_contents` adds.
pub fn with_cell_style<R>(
    ui: &mut egui::Ui,
    style: CellStyle,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let ctx = ui.ctx().clone();
    let before = cell_style(&ctx);
    ctx.data_mut(|d| d.insert_temp(cell_style_id(), style));
    let out = add_contents(ui);
    ctx.data_mut(|d| d.insert_temp(cell_style_id(), before));
    out
}

/// The look of the tool cells that are added now.
pub fn cell_style(ctx: &egui::Context) -> CellStyle {
    ctx.data(|d| d.get_temp(cell_style_id()))
        .unwrap_or_default()
}

/// The size of a tool cell in the look of now.
fn cell_size(ctx: &egui::Context) -> egui::Vec2 {
    match cell_style(ctx) {
        CellStyle::Labeled => vec2(CELL.0, CELL.1),
        CellStyle::Icon => vec2(geometry::COLUMN_CELL, geometry::COLUMN_CELL),
    }
}

/// A held cell: its ID, its name, and its place.
#[derive(Clone, Debug, PartialEq)]
struct Hold {
    id: egui::Id,
    label: String,
    rect: Rect,
}

/// The name and the place of the cell that a finger holds, if the hold is
/// long enough to show the name. After the end of the press, it forgets
/// the cell.
pub fn take_hold_label(ctx: &egui::Context) -> Option<(String, Rect)> {
    let hold: Option<Hold> = ctx.data(|d| d.get_temp(hold_id()));
    if ctx.input(|i| i.pointer.any_down()) {
        return hold.map(|h| (h.label, h.rect));
    }
    ctx.data_mut(|d| d.remove::<Hold>(hold_id()));
    None
}

/// One cell of the tool strip: an icon with a name below it. In the
/// `CellStyle::Icon` look, the icon alone, and a hold shows `label` next
/// to the cell.
pub fn tool_cell(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    label: &str,
    icon: Icon,
    active: bool,
    enabled: bool,
) -> Response {
    tool_cell_with(ui, controls, name, label, icon, active, enabled, true)
}

/// A `tool_cell`. With no `hold`, a hold does not show the name.
#[allow(clippy::too_many_arguments)]
pub(super) fn tool_cell_with(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    label: &str,
    icon: Icon,
    active: bool,
    enabled: bool,
    hold: bool,
) -> Response {
    let style = cell_style(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(cell_size(ui.ctx()), Sense::hover());
    let resp = hit(
        ui,
        controls,
        rect,
        name,
        Sense::click(),
        enabled,
        Some(active),
    );
    fill(ui, rect, &resp, active);
    let color = fade(if active { theme::TEXT } else { theme::MUTE }, enabled);
    if style == CellStyle::Icon {
        icons::paint(ui.painter(), rect, icon, ICON, color);
        if hold {
            hold_name(ui, &resp, label);
        }
        return resp;
    }
    let icon_rect = Rect::from_center_size(rect.center() - vec2(0.0, 7.0), vec2(ICON, ICON));
    icons::paint(ui.painter(), icon_rect, icon, ICON, color);
    let at = pos2(rect.center().x, rect.bottom() - 7.0);
    ui.painter()
        .text(at, Align2::CENTER_BOTTOM, label, theme::small(), color);
    resp
}

/// Follows a press on a cell. After a hold of `HOLD_TIME`, the cell is the
/// held cell until the press ends.
fn hold_name(ui: &egui::Ui, resp: &Response, label: &str) {
    let ctx = ui.ctx();
    let held: Option<Hold> = ctx.data(|d| d.get_temp(hold_id()));
    if !resp.is_pointer_button_down_on() || held.is_some_and(|h| h.id == resp.id) {
        return;
    }
    let press = ui.input(|i| {
        let (start, origin) = (i.pointer.press_start_time()?, i.pointer.press_origin()?);
        let pos = i.pointer.interact_pos()?;
        Some((i.time - start, origin.distance(pos)))
    });
    match press {
        Some((time, moved)) if moved <= super::HOLD_SLOP && time >= super::HOLD_TIME => {
            let hold = Hold {
                id: resp.id,
                label: label.to_owned(),
                rect: resp.rect,
            };
            ctx.data_mut(|d| d.insert_temp(hold_id(), hold));
            ctx.request_repaint();
        }
        Some((time, moved)) if moved <= super::HOLD_SLOP => {
            let wait = std::time::Duration::from_secs_f64(super::HOLD_TIME - time);
            ctx.request_repaint_after(wait);
        }
        _ => {}
    }
}

/// One cell of the tool strip: an icon with no name.
pub fn icon_cell(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    icon: Icon,
    active: bool,
    enabled: bool,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(cell_size(ui.ctx()), Sense::hover());
    let resp = hit(
        ui,
        controls,
        rect,
        name,
        Sense::click(),
        enabled,
        Some(active),
    );
    fill(ui, rect, &resp, active);
    let color = fade(if active { theme::TEXT } else { theme::MUTE }, enabled);
    icons::paint(ui.painter(), rect, icon, ICON, color);
    resp
}

/// The size of the track of a switch.
const SWITCH: egui::Vec2 = vec2(50.0, 30.0);

/// A switch in a row of a panel: the name at the left, and the track with
/// its thumb at the right. `label` is the name on screen. A tap on the row
/// changes `on`. Returns true if the user changed it.
pub fn switch_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    label: &str,
    on: &mut bool,
) -> bool {
    switch_row_enabled(ui, controls, name, label, on, true)
}

/// A `switch_row` that is dim and does not change while `enabled` is
/// false.
pub fn switch_row_enabled(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    label: &str,
    on: &mut bool,
    enabled: bool,
) -> bool {
    let size = vec2(ui.available_width(), PANEL_ROW);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let resp = hit(ui, controls, rect, name, Sense::click(), enabled, Some(*on));
    let changed = resp.clicked();
    if changed {
        *on = !*on;
    }
    let y = rect.center().y;
    let mut track = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    if !enabled {
        track.multiply_opacity(theme::OFF);
    }
    paint_switch(&track, pos2(rect.right(), y), *on);
    let at = pos2(rect.left(), y);
    let color = fade(theme::TEXT, enabled);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, label, theme::body(), color);
    changed
}

/// The width of the track of a switch.
pub const SWITCH_WIDTH: f32 = SWITCH.x;

/// The track and the thumb of a switch, with the middle of its right edge
/// at `right`.
pub fn paint_switch(ui: &egui::Ui, right: Pos2, on: bool) {
    let y = right.y;
    let track = Rect::from_min_size(pos2(right.x - SWITCH.x, y - SWITCH.y / 2.0), SWITCH);
    let reach = (SWITCH.x - SWITCH.y) / 2.0;
    let (color, x) = match on {
        true => (theme::ACCENT, track.center().x + reach),
        false => (theme::raise(), track.center().x - reach),
    };
    let painter = ui.painter();
    painter.rect_filled(track, SWITCH.y / 2.0, theme::track());
    painter.rect_filled(track, SWITCH.y / 2.0, color);
    painter.circle_filled(
        pos2(x, y + 1.0),
        THUMB / 2.0 + 1.0,
        Color32::from_black_alpha(70),
    );
    painter.circle_filled(pos2(x, y), THUMB / 2.0, theme::TEXT);
}

/// What the user did on the title row of a panel.
pub struct TitleRow {
    pub close: bool,
    pub back: bool,
    /// The action button, if the row has one.
    pub action: Option<Response>,
    /// The row without its buttons. A sheet moves with a drag on it.
    pub bar: Response,
}

/// The title row of a panel: an optional back button, the title, and the
/// close button. `back` is the name of the panel that the back button opens.
pub fn title_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    title: &str,
    back: Option<&str>,
    drag: bool,
) -> TitleRow {
    title_row_with(ui, controls, title, back, drag, None)
}

/// A title row with an action button at the left of the close button.
/// `action` is the name of the button, its icon, and its state.
pub fn title_row_with(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    title: &str,
    back: Option<&str>,
    drag: bool,
    action: Option<(&str, Icon, bool)>,
) -> TitleRow {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
    let sense = if drag {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let bar = ui.interact(rect, ui.id().with(("title", title)), sense);
    let button = |x: f32| Rect::from_min_size(pos2(x, rect.top() + PAD), vec2(TOUCH, TOUCH));
    let mut text_left = rect.left() + BODY_PAD;
    let back = back.is_some_and(|parent| {
        text_left = rect.left() + PAD + TOUCH + 4.0;
        let name = format!("Back to {parent}");
        icon_button_at(
            ui,
            controls,
            button(rect.left() + PAD),
            &name,
            Icon::Back,
            false,
            true,
        )
        .clicked()
    });
    let name = format!("Close {title}");
    let close_rect = button(rect.right() - PAD - TOUCH);
    let close = icon_button_at(ui, controls, close_rect, &name, Icon::Close, false, true).clicked();
    let action = action.map(|(name, icon, active)| {
        let at = close_rect.translate(vec2(-TOUCH, 0.0));
        icon_button_at(ui, controls, at, name, icon, active, true)
    });
    let text_right = action.as_ref().map_or(close_rect.left(), |a| a.rect.left());
    let clip = Rect::from_min_max(pos2(text_left, rect.top()), pos2(text_right, rect.bottom()));
    let at = pos2(text_left, rect.center().y);
    ui.painter().with_clip_rect(clip).text(
        at,
        Align2::LEFT_CENTER,
        title,
        theme::title(),
        theme::TEXT,
    );
    TitleRow {
        close,
        back,
        action,
        bar,
    }
}

/// What the user did with a row of the layer list.
pub struct LayerRow {
    /// The row without its buttons. A row that is not live has none.
    pub body: Option<Response>,
    pub eye: bool,
    pub open: bool,
    /// A tap on the Delete button of an open row.
    pub delete: bool,
    /// A tap on the Rename button of an open row.
    pub rename: bool,
}

/// The width of the Delete button of an open row of the layer list.
pub const DELETE_WIDTH: f32 = 2.0 * TOUCH;

/// The name of the Delete button of the row of a layer.
pub fn delete_name(name: &str) -> String {
    format!("Delete {name}")
}

/// A row of the layer list, at a given place: the icon of the kind, the
/// name, the eye that shows or hides the layer, and the arrow that opens the
/// panel of the layer. `visible` is `None` for a layer with no eye. A row
/// that is not `live` takes no touch. A `lifted` row covers the rows below it.
/// A negative `shift` moves the row to the left and shows the Delete button
/// behind it. The eye and the arrow take touch only at a shift of zero. A
/// `note` shows in gray at the right of the name.
#[allow(clippy::too_many_arguments)]
pub fn layer_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    icon: Icon,
    name: &str,
    visible: Option<bool>,
    selected: bool,
    live: bool,
    lifted: bool,
    shift: f32,
    note: Option<&str>,
) -> LayerRow {
    let edge = rect.right() + 8.0;
    let open_rect = Rect::from_center_size(
        pos2(edge - TOUCH / 2.0, rect.center().y),
        vec2(TOUCH, TOUCH),
    );
    let eye_rect = open_rect.translate(vec2(-TOUCH, 0.0));
    let body_right = match visible {
        Some(_) => eye_rect.left(),
        None => open_rect.left(),
    };
    let reveal = Rect::from_min_max(pos2(edge + shift, rect.top()), pos2(edge, rect.bottom()));
    let hit_right = body_right.min(reveal.left()).max(rect.left() + TOUCH);
    let body_rect = Rect::from_min_max(rect.min, pos2(hit_right, rect.bottom()));
    let body_name = format!("Layer {name}");
    let body = live.then(|| {
        hit(
            ui,
            controls,
            body_rect,
            &body_name,
            Sense::click_and_drag(),
            true,
            Some(selected),
        )
    });
    let highlight = rect.expand2(vec2(8.0, -HIGHLIGHT_INSET));
    let delete = delete_behind(ui, controls, rect, name, live, shift);
    let rename = live && rename_behind(ui, controls, rect, name, shift);
    let painter = sliding_painter(ui, rect, shift);
    let moved = vec2(shift, 0.0);
    let highlight = highlight.translate(moved);
    if lifted {
        painter.rect_filled(highlight, 5.0, theme::card(false).to_opaque());
    }
    if selected || lifted || body.as_ref().is_some_and(|b| b.is_pointer_button_down_on()) {
        painter.rect_filled(highlight, 5.0, theme::raise());
    }
    // The icon and the name stay in place while a swipe opens the row, and
    // the row slides over its buttons.
    let kind = Rect::from_center_size(pos2(rect.left() + 10.0, rect.center().y), vec2(18.0, 18.0));
    icons::paint(&painter, kind, icon, 18.0, theme::MUTE);
    let shown = visible != Some(false);
    let mut name_right = body_right - 4.0;
    if let Some(note) = note {
        let width = text_width(ui, note, theme::body());
        let place = Rect::from_min_max(
            pos2(name_right - width - 2.0, rect.top()),
            pos2(name_right, rect.bottom()),
        );
        name_right = place.left() - 8.0;
        // The note is a label, so a screen reader and a test can read it.
        let text = egui::RichText::new(note)
            .font(theme::body())
            .color(theme::MUTE);
        let layout = egui::Layout::right_to_left(egui::Align::Center);
        let builder = egui::UiBuilder::new().max_rect(place).layout(layout);
        let mut child = ui.new_child(builder);
        child.set_clip_rect(painter.clip_rect());
        child.add(egui::Label::new(text).selectable(false));
    }
    let clip = Rect::from_min_max(
        pos2(rect.left() + 28.0, rect.top()),
        pos2(name_right, rect.bottom()),
    );
    painter
        .with_clip_rect(painter.clip_rect().intersect(clip))
        .text(
            clip.left_center(),
            Align2::LEFT_CENTER,
            name,
            theme::body(),
            fade(theme::TEXT, shown),
        );
    let eye_icon = if shown { Icon::Eye } else { Icon::EyeOff };
    let eye_name = eye_name(name, shown);
    let open_name = format!("Open {name}");
    let (mut eye, mut open) = (false, false);
    if live && shift == 0.0 {
        if visible.is_some() {
            eye =
                icon_button_at(ui, controls, eye_rect, &eye_name, eye_icon, false, true).clicked();
        }
        open =
            icon_button_at(ui, controls, open_rect, &open_name, Icon::Open, false, true).clicked();
    } else if shift == 0.0 {
        if visible.is_some() {
            icons::paint(&painter, eye_rect, eye_icon, ICON, theme::MUTE);
        }
        icons::paint(&painter, open_rect, Icon::Open, ICON, theme::MUTE);
    }
    LayerRow {
        rename,
        body,
        eye,
        open,
        delete,
    }
}

/// The Delete button behind a row that a swipe moves `shift` to the left.
/// The button takes touch when the row is `live` and fully open. Returns
/// true if the button was tapped.
fn delete_behind(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    live: bool,
    shift: f32,
) -> bool {
    if shift >= 0.0 {
        return false;
    }
    let edge = rect.right() + 8.0;
    let highlight = rect.expand2(vec2(8.0, -HIGHLIGHT_INSET));
    let reveal = Rect::from_min_max(pos2(edge + shift, rect.top()), pos2(edge, rect.bottom()));
    let back = Rect::from_x_y_ranges(reveal.x_range(), highlight.y_range());
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(back));
    painter.rect_filled(back, 5.0, theme::WARN);
    let at = pos2(edge - DELETE_WIDTH / 2.0, rect.center().y);
    painter.text(
        at,
        Align2::CENTER_CENTER,
        "Delete",
        theme::body(),
        theme::CANVAS,
    );
    if !live || -shift < DELETE_WIDTH - 0.5 {
        return false;
    }
    let area = Rect::from_min_max(
        pos2(
            reveal.left().max(edge - DELETE_WIDTH),
            rect.center().y - TOUCH / 2.0,
        ),
        pos2(edge, rect.center().y + TOUCH / 2.0),
    );
    tap_area(ui, controls, area, &delete_name(name), true).clicked()
}

/// The Rename button to the left of the Delete button of a row that a swipe
/// moves `shift` to the left. Returns true if the user taps it.
fn rename_behind(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    shift: f32,
) -> bool {
    let edge = rect.right() + 8.0;
    if shift > -DELETE_WIDTH {
        return false;
    }
    let highlight = rect.expand2(vec2(8.0, -HIGHLIGHT_INSET));
    let left = edge + shift;
    let place = Rect::from_x_y_ranges(left..=edge - DELETE_WIDTH, highlight.y_range());
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(place));
    painter.rect_filled(place, 5.0, theme::MUTE);
    let at = pos2(edge - 1.5 * DELETE_WIDTH, rect.center().y);
    painter.text(
        at,
        Align2::CENTER_CENTER,
        RENAME,
        theme::body(),
        theme::CANVAS,
    );
    if -shift < 2.0 * DELETE_WIDTH - 0.5 {
        return false;
    }
    let area = Rect::from_min_max(
        pos2(left, rect.center().y - TOUCH / 2.0),
        pos2(edge - DELETE_WIDTH, rect.center().y + TOUCH / 2.0),
    );
    tap_area(ui, controls, area, &format!("{RENAME} {name}"), true).clicked()
}

pub const RENAME: &str = "Rename";

/// The painter of a row that a swipe moves `shift` to the left. The row
/// slides over its Delete button.
fn sliding_painter(ui: &egui::Ui, rect: Rect, shift: f32) -> egui::Painter {
    let left = rect.left() - 8.0;
    let clip = Rect::from_x_y_ranges(left..=rect.right() + 8.0 + shift, rect.y_range());
    ui.painter().with_clip_rect(ui.clip_rect().intersect(clip))
}

/// What the user did with a row that a swipe opens.
pub struct SwipeRow {
    /// The row without its Delete button.
    pub body: Response,
    /// A tap on the Delete button of an open row.
    pub delete: bool,
    /// A tap on the Rename button of an open row.
    pub rename: bool,
}

/// A row of a list of choices at a given place, with a check at the left
/// of the chosen row. A negative `shift` moves the row to the left and shows
/// the Delete button behind it, as in the layer list, and a Rename button to
/// its left for a row that the swipe opens by [`RENAME_WIDTH`].
pub fn swipe_option_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    title: &str,
    on: bool,
    shift: f32,
) -> SwipeRow {
    let edge = rect.right() + 8.0;
    let end = rect.right();
    let right = (edge + shift).max(rect.left() + TOUCH).min(end);
    let body_rect = Rect::from_min_max(rect.min, pos2(right, rect.bottom()));
    let body = hit(
        ui,
        controls,
        body_rect,
        name,
        Sense::click_and_drag(),
        true,
        Some(on),
    );
    let delete = delete_behind(ui, controls, rect, title, true, shift);
    let rename = rename_behind(ui, controls, rect, title, shift);
    let painter = sliding_painter(ui, rect, shift);
    let moved = vec2(shift, 0.0);
    if on || body.is_pointer_button_down_on() {
        let highlight = rect.expand2(vec2(8.0, -HIGHLIGHT_INSET)).translate(moved);
        painter.rect_filled(highlight, 5.0, theme::raise());
    }
    let mark = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(18.0, 18.0));
    // The check and the title slide only as far as the left edge of the row,
    // so that a wide swipe keeps the title in sight.
    let content = vec2(shift.max(8.0 - (mark.left() - rect.left())), 0.0);
    if on {
        icons::paint(
            &painter,
            mark.translate(content),
            Icon::Tick,
            18.0,
            theme::ACCENT,
        );
    }
    let color = if on { theme::TEXT } else { theme::MUTE };
    let clip = Rect::from_min_max(
        pos2(rect.left() + 38.0, rect.top()),
        pos2(end - 4.0, rect.bottom()),
    )
    .translate(content);
    painter
        .with_clip_rect(painter.clip_rect().intersect(clip))
        .text(
            clip.left_center(),
            Align2::LEFT_CENTER,
            title,
            theme::body(),
            color,
        );
    SwipeRow {
        body,
        delete,
        rename,
    }
}

/// The swipe of a row with a Rename button and a Delete button.
pub const RENAME_WIDTH: f32 = 2.0 * DELETE_WIDTH;

/// The name of the eye of a layer that is on or off.
pub fn eye_name(name: &str, shown: bool) -> String {
    format!("{} {name}", if shown { "Hide" } else { "Show" })
}

/// A row with the icon of the kind of a layer, its name, and an eye at the
/// right. Returns true if the eye was tapped.
pub fn eye_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    icon: Icon,
    name: &str,
    shown: bool,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let center = pos2(rect.right() + 8.0 - TOUCH / 2.0, rect.center().y);
    let eye = Rect::from_center_size(center, vec2(TOUCH, TOUCH));
    let painter = ui.painter();
    let kind = Rect::from_center_size(pos2(rect.left() + 10.0, rect.center().y), vec2(18.0, 18.0));
    icons::paint(painter, kind, icon, 18.0, theme::MUTE);
    let clip = Rect::from_min_max(
        pos2(rect.left() + 28.0, rect.top()),
        pos2(eye.left() - 4.0, rect.bottom()),
    );
    painter
        .with_clip_rect(painter.clip_rect().intersect(clip))
        .text(
            clip.left_center(),
            Align2::LEFT_CENTER,
            name,
            theme::body(),
            fade(theme::TEXT, shown),
        );
    let icon = if shown { Icon::Eye } else { Icon::EyeOff };
    let name = eye_name(name, shown);
    icon_button_at(ui, controls, eye, &name, icon, false, true).clicked()
}

/// A row of a panel or of a menu, with a text. Returns the response of the
/// whole row and its rectangle.
pub fn text_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    indent: f32,
    selected: bool,
    enabled: bool,
) -> Response {
    text_row_as(ui, controls, name, name, indent, selected, enabled)
}

/// A `text_row` that shows `text`. `name` is the name of the control.
pub fn text_row_as(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    text: &str,
    indent: f32,
    selected: bool,
    enabled: bool,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let resp = hit(
        ui,
        controls,
        rect,
        name,
        Sense::click(),
        enabled,
        Some(selected),
    );
    if selected || resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(
            rect.expand2(vec2(8.0, -HIGHLIGHT_INSET)),
            5.0,
            theme::raise(),
        );
    }
    let at = pos2(rect.left() + indent, rect.center().y);
    let color = fade(theme::TEXT, enabled);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, text, theme::body(), color);
    resp
}

/// The size of the swatch of a color row.
pub const SWATCH: f32 = 24.0;

/// A row with the name at the left and a swatch of `color` at the right.
/// The swatch shows the alpha of the color over a checkerboard. A tap opens
/// or closes the color picker of the row. `open`: the picker is open.
pub fn color_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    color: [u8; 4],
    open: bool,
) -> Response {
    let row = text_row(ui, controls, name, 0.0, open, true);
    let [r, g, b, a] = color;
    let swatch = Rect::from_center_size(
        pos2(row.rect.right() - SWATCH / 2.0, row.rect.center().y),
        vec2(SWATCH, SWATCH),
    );
    let painter = ui.painter();
    if a < 255 {
        paint_checker(painter, swatch, SWATCH / 4.0, 6);
    }
    painter.rect_filled(swatch, 6.0, Color32::from_rgba_unmultiplied(r, g, b, a));
    let stroke = egui::Stroke::new(1.0, theme::divider());
    painter.rect_stroke(swatch, 6.0, stroke, egui::StrokeKind::Inside);
    row
}

/// A color row for a color that is different in the selected items: an
/// empty swatch with a slash, and "Mixed" at its left.
pub fn mixed_color_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    open: bool,
) -> Response {
    let row = text_row(ui, controls, name, 0.0, open, true);
    let swatch = Rect::from_center_size(
        pos2(row.rect.right() - SWATCH / 2.0, row.rect.center().y),
        vec2(SWATCH, SWATCH),
    );
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.0, theme::divider());
    painter.rect_stroke(swatch, 6.0, stroke, egui::StrokeKind::Inside);
    let inset = swatch.shrink(5.0);
    let slash = egui::Stroke::new(1.5, theme::MUTE);
    painter.line_segment([inset.left_bottom(), inset.right_top()], slash);
    let at = pos2(swatch.left() - 10.0, row.rect.center().y);
    let mixed = crate::ui::select::MIXED;
    painter.text(at, Align2::RIGHT_CENTER, mixed, theme::body(), theme::MUTE);
    row
}

/// An opaque color with the red, green, and blue of `rgb`.
pub fn opaque(rgb: [u8; 3]) -> [u8; 4] {
    let [r, g, b] = rgb;
    [r, g, b, 255]
}

/// Paints a checkerboard of light and dark grey cells of the size `cell`
/// over `rect`. The cells at the corners of `rect` are rounded by `radius`.
pub fn paint_checker(painter: &egui::Painter, rect: Rect, cell: f32, radius: u8) {
    const LIGHT: Color32 = Color32::from_gray(200);
    const DARK: Color32 = Color32::from_gray(140);
    let cols = (rect.width() / cell).ceil().max(1.0) as usize;
    let rows = (rect.height() / cell).ceil().max(1.0) as usize;
    for row in 0..rows {
        for col in 0..cols {
            let min = rect.min + vec2(col as f32 * cell, row as f32 * cell);
            let at = Rect::from_min_max(min, (min + vec2(cell, cell)).min(rect.max));
            let (first, last) = (col == 0, col + 1 == cols);
            let (top, bottom) = (row == 0, row + 1 == rows);
            let round = |on: bool| if on { radius } else { 0 };
            let corners = CornerRadius {
                nw: round(top && first),
                ne: round(top && last),
                sw: round(bottom && first),
                se: round(bottom && last),
            };
            let color = if (row + col) % 2 == 0 { LIGHT } else { DARK };
            painter.rect_filled(at, corners, color);
        }
    }
}

/// Paints the arrow of a row that opens a page, at the right of the row.
pub fn paint_open(ui: &egui::Ui, row: Rect) {
    let rect = Rect::from_center_size(
        pos2(row.right() - ICON / 2.0, row.center().y),
        vec2(ICON, ICON),
    );
    icons::paint(ui.painter(), rect, Icon::Open, ICON, theme::MUTE);
}

/// A row that opens a page: the name at the left, and `value` and an arrow
/// at the right.
pub fn open_row(ui: &mut egui::Ui, controls: &mut Controls, name: &str, value: &str) -> Response {
    let resp = text_row(ui, controls, name, 0.0, false, true);
    let row = resp.rect;
    if !value.is_empty() {
        let at = pos2(row.right() - ICON - 8.0, row.center().y);
        ui.painter()
            .text(at, Align2::RIGHT_CENTER, value, theme::body(), theme::MUTE);
    }
    paint_open(ui, row);
    resp
}

/// The size of the selection circle of a row.
const SELECT_CIRCLE: f32 = 22.0;

/// A row of a list in which the user selects items: a selection circle at
/// the left, the name, and `value` at the right. The circle is filled and
/// has a tick when the row is `on`.
pub fn select_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    value: &str,
    on: bool,
) -> Response {
    let resp = text_row(ui, controls, name, SELECT_CIRCLE + 12.0, false, true);
    let row = resp.rect;
    let circle = Rect::from_center_size(
        pos2(row.left() + SELECT_CIRCLE / 2.0, row.center().y),
        vec2(SELECT_CIRCLE, SELECT_CIRCLE),
    );
    let painter = ui.painter();
    if on {
        painter.circle_filled(circle.center(), SELECT_CIRCLE / 2.0, theme::ACCENT);
        icons::paint(
            painter,
            circle.shrink(3.0),
            Icon::Tick,
            SELECT_CIRCLE - 6.0,
            theme::CANVAS,
        );
    } else {
        let ring = egui::Stroke::new(1.5, theme::MUTE);
        painter.circle_stroke(circle.center(), SELECT_CIRCLE / 2.0 - 0.75, ring);
    }
    if !value.is_empty() {
        let at = pos2(row.right(), row.center().y);
        painter.text(at, Align2::RIGHT_CENTER, value, theme::body(), theme::MUTE);
    }
    resp
}

/// The heading of a section with text buttons at its right, as in the
/// header of an iPadOS list. Returns the index of the tapped button.
pub fn heading_buttons(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    text: &str,
    buttons: &[&str],
) -> Option<usize> {
    let height = TOUCH.max(40.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let y = rect.bottom() - 8.0;
    ui.painter().text(
        pos2(rect.left(), y),
        Align2::LEFT_BOTTOM,
        text,
        theme::title(),
        theme::TEXT,
    );
    let mut right = rect.right();
    let mut out = None;
    for (index, name) in buttons.iter().enumerate().rev() {
        let width = text_width(ui, name, theme::body()) + 16.0;
        let place = Rect::from_min_max(pos2(right - width, rect.top()), pos2(right, rect.bottom()));
        let resp = hit(ui, controls, place, name, Sense::click(), true, None);
        let color = if resp.is_pointer_button_down_on() {
            theme::ACCENT.gamma_multiply(0.6)
        } else {
            theme::ACCENT
        };
        let at = pos2(place.right() - 8.0, y);
        ui.painter()
            .text(at, Align2::RIGHT_BOTTOM, *name, theme::body(), color);
        if resp.clicked() {
            out = Some(index);
        }
        right = place.left();
    }
    out
}

/// A row of a panel with buttons side by side, each with its name, its
/// state, and its fill color. Returns the index of the tapped button.
pub fn button_bar(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    buttons: &[(&str, bool, Color32)],
) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let gap = 8.0;
    let count = buttons.len().max(1) as f32;
    let width = (rect.width() - gap * (count - 1.0)) / count;
    let mut out = None;
    for (index, (name, enabled, fill)) in buttons.iter().enumerate() {
        let left = rect.left() + index as f32 * (width + gap);
        let place = Rect::from_min_size(pos2(left, rect.top()), vec2(width, rect.height()));
        let resp = hit(ui, controls, place, name, Sense::click(), *enabled, None);
        let color = if resp.is_pointer_button_down_on() {
            *fill
        } else {
            fill.gamma_multiply(0.8)
        };
        let back = place.shrink2(vec2(0.0, HIGHLIGHT_INSET));
        ui.painter()
            .rect_filled(back, theme::CONTROL_RADIUS, fade(color, *enabled));
        ui.painter().text(
            place.center(),
            Align2::CENTER_CENTER,
            *name,
            theme::body(),
            theme::CANVAS,
        );
        if resp.clicked() {
            out = Some(index);
        }
    }
    out
}

/// The size of the square of a compass dial.
const DIAL: f32 = 4.0 * TOUCH;

/// The angle of a place on a compass dial, in whole degrees clockwise from
/// the top, from 0 up to 360. An angle within 3 degrees of a compass point
/// snaps to it. `None`: the place is too near the center to give an angle.
pub fn compass_angle(center: Pos2, at: Pos2) -> Option<f32> {
    const DEAD: f32 = 16.0;
    let d = at - center;
    if d.length() < DEAD {
        return None;
    }
    let degrees = d.x.atan2(-d.y).to_degrees().round();
    let point = (degrees / 45.0).round() * 45.0;
    let degrees = if (degrees - point).abs() <= 3.0 {
        point
    } else {
        degrees
    };
    Some(degrees.rem_euclid(360.0))
}

/// The name of the compass point nearest to an angle in degrees.
pub(crate) fn compass_point(degrees: f32) -> &'static str {
    const POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    POINTS[((degrees / 45.0).round() as i32).rem_euclid(8) as usize]
}

/// A compass dial that sets a direction: a label, then a dial with a handle
/// at `degrees`, clockwise from north. A drag or a tap on the dial moves the
/// handle. Returns `true` when the direction changes.
pub fn compass_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    label: &str,
    degrees: &mut f32,
) -> bool {
    let width = ui.available_width();
    let (line, _) = ui.allocate_exact_size(vec2(width, PANEL_ROW), Sense::hover());
    let at = pos2(line.left(), line.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, label, theme::body(), theme::TEXT);
    let (area, _) = ui.allocate_exact_size(vec2(width, DIAL), Sense::hover());
    let square = Rect::from_center_size(area.center(), vec2(DIAL, DIAL));
    let resp = hit(
        ui,
        controls,
        square,
        name,
        Sense::click_and_drag(),
        true,
        None,
    );
    let center = square.center();
    let before = *degrees;
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
        && let Some(angle) = compass_angle(center, pos)
    {
        *degrees = angle;
    }
    ui.add_space(8.0);

    let r = DIAL / 2.0 - 22.0;
    let painter = ui.painter();
    let dir = |deg: f32| {
        let a = deg.to_radians();
        vec2(a.sin(), -a.cos())
    };
    painter.circle_filled(center, r + 14.0, theme::track());
    painter.circle_stroke(center, r, egui::Stroke::new(1.0, theme::divider()));
    for step in 0..24 {
        let deg = step as f32 * 15.0;
        let long = step % 3 == 0;
        let inner = if long { r - 8.0 } else { r - 4.0 };
        let width = if long { 1.5 } else { 1.0 };
        painter.line_segment(
            [center + dir(deg) * inner, center + dir(deg) * r],
            egui::Stroke::new(width, theme::MUTE),
        );
    }
    for (deg, point) in [(0.0, "N"), (90.0, "E"), (180.0, "S"), (270.0, "W")] {
        let at = center + dir(deg) * (r - 16.0);
        painter.text(
            at,
            Align2::CENTER_CENTER,
            point,
            theme::small(),
            theme::MUTE,
        );
    }
    let handle = center + dir(*degrees) * r;
    painter.line_segment([center, handle], egui::Stroke::new(2.0, theme::ACCENT));
    painter.circle_filled(
        handle + vec2(0.0, 1.0),
        THUMB / 2.0 + 1.0,
        Color32::from_black_alpha(70),
    );
    painter.circle_filled(handle, THUMB / 2.0, theme::TEXT);
    painter.circle_filled(center, 22.0, theme::track());
    let text = format!("{:.0}\u{b0}", *degrees);
    painter.text(
        center - vec2(0.0, 6.0),
        Align2::CENTER_CENTER,
        text,
        theme::mono(),
        theme::TEXT,
    );
    painter.text(
        center + vec2(0.0, 10.0),
        Align2::CENTER_CENTER,
        compass_point(*degrees),
        theme::small(),
        theme::MUTE,
    );
    *degrees != before
}

/// The heading of a part of a list, in small capitals.
pub fn list_heading(ui: &mut egui::Ui, title: &str) {
    let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let at = pos2(head.left() + 4.0, head.bottom() - 6.0);
    let title = title.to_uppercase();
    (ui.painter()).text(at, Align2::LEFT_BOTTOM, title, theme::small(), theme::MUTE);
}

/// A row of a form that is one choice of a list: a title at the left, and
/// lines of small text at the right that tell what the choice gives. The
/// row that is `on` has a mark. `name` is the name of the control.
pub fn option_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    title: &str,
    lines: &[String],
    on: bool,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let resp = hit(ui, controls, rect, name, Sense::click(), true, Some(on));
    if on || resp.is_pointer_button_down_on() {
        let back = rect.shrink2(vec2(0.0, HIGHLIGHT_INSET));
        ui.painter()
            .rect_filled(back, theme::CONTROL_RADIUS, theme::raise());
    }
    let mark = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(18.0, 18.0));
    if on {
        icons::paint(ui.painter(), mark, Icon::Tick, 18.0, theme::ACCENT);
    }
    let color = if on { theme::TEXT } else { theme::MUTE };
    let at = pos2(rect.left() + 38.0, rect.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, title, theme::body(), color);
    // The lines are labels, so a screen reader and a test can read them.
    let step = 15.0;
    let top = rect.center().y - step * lines.len() as f32 / 2.0;
    for (index, line) in lines.iter().enumerate() {
        let min = pos2(rect.left() + 110.0, top + index as f32 * step);
        let place = Rect::from_min_max(min, pos2(rect.right() - 12.0, min.y + step));
        let text = egui::RichText::new(line).font(theme::small()).color(color);
        let layout = egui::Layout::right_to_left(egui::Align::Center);
        // A child that is not in the layout keeps the height of the row.
        let mut line = ui.new_child(egui::UiBuilder::new().max_rect(place).layout(layout));
        line.add(egui::Label::new(text).selectable(false).truncate());
    }
    resp
}

/// The width of the label at the left of a row of a form.
const FORM_LABEL: f32 = 92.0;

/// One row of a form: the label at the left, and the place of the control
/// at the right. The controls of all rows have the same left edge.
fn form_row(ui: &mut egui::Ui, label: &str) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let at = pos2(rect.left(), rect.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, label, theme::body(), theme::MUTE);
    Rect::from_min_max(
        pos2(rect.left() + FORM_LABEL, rect.center().y - TOUCH / 2.0),
        pos2(rect.right(), rect.center().y + TOUCH / 2.0),
    )
}

/// A row of a form with a value that the user cannot change.
pub fn value_row(ui: &mut egui::Ui, label: &str, value: &str) {
    let field = form_row(ui, label);
    let at = pos2(field.left() + 12.0, field.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, value, theme::body(), theme::TEXT);
}

/// A row of a form with a field for one line of text. `name` is the name
/// of the control. `suffix` is a unit at the right end of the field. The
/// text has the warning color if it is not `valid`.
pub fn text_field(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    label: &str,
    suffix: Option<&str>,
    text: &mut String,
    valid: bool,
) -> Response {
    field_in(
        ui,
        controls,
        (name, label),
        suffix,
        text,
        valid,
        theme::body(),
    )
}

/// The name of the button of a `choice_field`.
pub fn choices_name(name: &str) -> String {
    format!("{name} values")
}

/// The name of the row of `choice` in the menu of a `choice_field`.
pub fn choice_row_name(name: &str, choice: &str) -> String {
    format!("{name}: {choice}")
}

/// The number of rows that the menu of a `choice_field` shows without a
/// scroll. The half row shows that the menu scrolls.
const MENU_ROWS: f32 = 5.5;

/// A `text_field` with a button at its right end that opens and closes a
/// menu of `choices`. The menu is a card below the field, or above it if
/// the screen has no room below. A tap outside the menu closes it. Returns
/// the response of the field and the index of the choice that the user
/// tapped. `open`: the menu is open.
#[allow(clippy::too_many_arguments)]
pub fn choice_field(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    text: &mut String,
    valid: bool,
    choices: &[String],
    open: &mut bool,
) -> (Response, Option<usize>) {
    let row = form_row(ui, name);
    let button = Rect::from_min_size(pos2(row.right() - TOUCH, row.top()), vec2(TOUCH, TOUCH));
    let field = Rect::from_min_max(row.min, pos2(button.left() - 6.0, row.bottom()));
    let resp = field_at(ui, controls, field, name, None, text, valid, theme::body());
    let tap = hit(
        ui,
        controls,
        button,
        &choices_name(name),
        Sense::click(),
        true,
        Some(*open),
    );
    fill(ui, button, &tap, *open);
    let chevron = Rect::from_center_size(button.center(), vec2(18.0, 18.0));
    icons::paint(ui.painter(), chevron, Icon::Down, 18.0, theme::MUTE);
    if tap.clicked() {
        *open = !*open;
    }
    if !*open {
        return (resp, None);
    }
    let ctx = ui.ctx().clone();
    let rows = (choices.len() as f32).min(MENU_ROWS);
    let size = vec2(row.width(), 10.0 + rows * PANEL_ROW + 10.0);
    let screen = ctx.viewport_rect();
    let below = row.bottom() + PAD;
    let y = if below + size.y <= screen.bottom() - PAD {
        below
    } else {
        (row.top() - PAD - size.y).max(screen.top() + PAD)
    };
    let rect = Rect::from_min_size(pos2(row.left(), y), size);
    let pressed = ui.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    if pressed.is_some_and(|at| !rect.contains(at) && !button.contains(at)) {
        *open = false;
        return (resp, None);
    }
    let current = choices.iter().position(|c| c == text.trim());
    let mut chosen = None;
    let id = format!("pad-choices-{}", ui.id().with(name).value());
    let radius = theme::CARD_RADIUS.into();
    card(&ctx, None, &id, rect, egui::Order::Tooltip, radius, |ui| {
        ui.painter().rect_filled(rect, radius, theme::menu());
        let inner = rect.shrink2(vec2(BODY_PAD, 10.0));
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        ui.set_clip_rect(inner);
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        egui::ScrollArea::vertical()
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut ui, |ui| {
                for (index, choice) in choices.iter().enumerate() {
                    let on = current == Some(index);
                    let row_name = choice_row_name(name, choice);
                    let row = text_row_as(ui, controls, &row_name, choice, 0.0, false, true);
                    // The current value has a tick and no highlight.
                    let info = || {
                        egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &row_name)
                    };
                    row.widget_info(info);
                    if on {
                        let tick = Rect::from_center_size(
                            pos2(row.rect.right() - 14.0, row.rect.center().y),
                            vec2(18.0, 18.0),
                        );
                        icons::paint(ui.painter(), tick, Icon::Tick, 18.0, theme::ACCENT);
                    }
                    if row.clicked() {
                        chosen = Some(index);
                    }
                }
            });
    });
    if chosen.is_some() {
        *open = false;
    }
    (resp, chosen)
}

/// A field for more than one line of text, as wide as the row and `lines`
/// lines high.
pub fn text_area(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    text: &mut String,
    lines: usize,
) -> Response {
    let line = ui.fonts_mut(|f| f.row_height(&theme::body()));
    let height = (lines as f32 * line + 2.0 * 12.0).max(PANEL_ROW);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let field = rect.shrink2(vec2(0.0, HIGHLIGHT_INSET));
    ui.painter()
        .rect_filled(field, theme::CONTROL_RADIUS, theme::raise());
    let inner = field.shrink2(vec2(12.0, 10.0));
    let edit = egui::TextEdit::multiline(text)
        .id(ui.id().with(name))
        .frame(egui::Frame::NONE)
        .font(theme::body())
        .text_color(theme::TEXT)
        .desired_rows(lines)
        .desired_width(inner.width());
    let resp = ui.put(inner, edit);
    push(ui, controls, field, name);
    resp
}

/// A `text_field` for an expression, in the mono font.
pub fn code_field(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    label: &str,
    text: &mut String,
    valid: bool,
) -> Response {
    field_in(
        ui,
        controls,
        (name, label),
        None,
        text,
        valid,
        theme::mono(),
    )
}

fn field_in(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    (name, label): (&str, &str),
    suffix: Option<&str>,
    text: &mut String,
    valid: bool,
    font: egui::FontId,
) -> Response {
    let field = form_row(ui, label);
    field_at(ui, controls, field, name, suffix, text, valid, font)
}

/// A field for one line of text in a given rectangle, with the edit menu
/// of a long press. `name` is the name of the control.
#[allow(clippy::too_many_arguments)]
pub fn field_at(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    field: Rect,
    name: &str,
    suffix: Option<&str>,
    text: &mut String,
    valid: bool,
    font: egui::FontId,
) -> Response {
    let back = field.shrink(HIGHLIGHT_INSET);
    ui.painter()
        .rect_filled(back, theme::CONTROL_RADIUS, theme::raise());
    let mut inner = field.shrink2(vec2(12.0, 0.0));
    if let Some(suffix) = suffix {
        let at = pos2(inner.right(), inner.center().y);
        ui.painter()
            .text(at, Align2::RIGHT_CENTER, suffix, theme::body(), theme::MUTE);
        inner.max.x -= text_width(ui, suffix, theme::body()) + 8.0;
    }
    let color = if valid { theme::TEXT } else { theme::WARN };
    let edit = egui::TextEdit::singleline(text)
        .id(ui.id().with(name))
        .frame(egui::Frame::NONE)
        .font(font)
        .text_color(color)
        .vertical_align(egui::Align::Center);
    let mut resp = ui.put(inner, edit);
    let shown = text.clone();
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::text_edit(true, &shown, &shown, "");
        info.label = Some(name.to_owned());
        info
    });
    push(ui, controls, field, name);
    if resp.long_touched() {
        resp.surrender_focus();
        open_edit_menu(ui.ctx(), resp.id, field);
    }
    match edit_menu(ui, controls, resp.id, field) {
        Some(EditAction::Copy) => crate::clipboard::set(ui.ctx(), text),
        Some(EditAction::Paste(pasted)) => {
            *text = pasted.trim().to_owned();
            resp.mark_changed();
        }
        Some(EditAction::Cut) | None => {}
    }
    resp
}

/// Gives a field of `field_at` the focus and selects all of its text, so
/// that the first key replaces the text.
pub fn focus_all(ui: &egui::Ui, field: &Response, text: &str) {
    use egui::text::{CCursor, CCursorRange};
    field.request_focus();
    let mut edit = egui::TextEdit::load_state(ui.ctx(), field.id).unwrap_or_default();
    let all = CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()));
    edit.cursor.set_char_range(Some(all));
    edit.store(ui.ctx(), field.id);
}

/// The names of the items of the edit menu of a field.
pub const CUT: &str = "Cut";
pub const COPY: &str = "Copy";
pub const PASTE: &str = "Paste";
/// The width of an item of the edit menu.
const EDIT_ITEM: f32 = 88.0;
/// The space between the edit menu and its field.
const EDIT_GAP: f32 = 6.0;

/// The item of the edit menu that the user tapped.
pub enum EditAction {
    Cut,
    Copy,
    Paste(String),
}

/// The field of the open edit menu, and the menu in the last frame.
#[derive(Clone, Copy)]
struct EditMenu {
    field: egui::Id,
    rect: Rect,
}

fn edit_menu_id() -> egui::Id {
    egui::Id::new("pad-edit-menu")
}

/// Opens the edit menu of a field after a long press.
pub fn open_edit_menu(ctx: &egui::Context, field: egui::Id, rect: Rect) {
    let menu = EditMenu { field, rect };
    ctx.data_mut(|data| data.insert_temp(edit_menu_id(), menu));
}

/// Closes the edit menu.
pub fn close_edit_menu(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<EditMenu>(edit_menu_id()));
}

/// True if the edit menu of field `field` is open.
pub fn edit_menu_open(ctx: &egui::Context, field: egui::Id) -> bool {
    let menu: Option<EditMenu> = ctx.data(|data| data.get_temp(edit_menu_id()));
    menu.is_some_and(|menu| menu.field == field)
}

/// The open edit menu in the last frame. A press on it does not close the
/// card of its field.
pub fn edit_menu_rect(ctx: &egui::Context) -> Option<Rect> {
    let menu: Option<EditMenu> = ctx.data(|data| data.get_temp(edit_menu_id()));
    menu.map(|menu| menu.rect)
}

/// The edit menu of a field, with Copy and Paste, above the field or below
/// it at the top of the screen. A press outside the menu closes it. The
/// menu does not bring up the keyboard.
fn edit_menu(
    ui: &egui::Ui,
    controls: &mut Controls,
    field_id: egui::Id,
    field: Rect,
) -> Option<EditAction> {
    edit_menu_items(ui, controls, field_id, field, &[COPY, PASTE])
}

/// The edit menu of a field with `items`, a list of `CUT`, `COPY`, and
/// `PASTE`, above `field` or below it at the top of the screen.
pub fn edit_menu_items(
    ui: &egui::Ui,
    controls: &mut Controls,
    field_id: egui::Id,
    field: Rect,
    items: &[&str],
) -> Option<EditAction> {
    let ctx = ui.ctx();
    let menu: EditMenu = ctx.data(|data| data.get_temp(edit_menu_id()))?;
    if menu.field != field_id {
        return None;
    }
    let size = vec2(items.len() as f32 * EDIT_ITEM, TOUCH);
    let screen = ctx.viewport_rect();
    let x =
        (field.center().x - size.x / 2.0).clamp(screen.left() + PAD, screen.right() - PAD - size.x);
    let above = field.top() - EDIT_GAP - size.y;
    let y = if above >= screen.top() + ROW {
        above
    } else {
        field.bottom() + EDIT_GAP
    };
    let rect = Rect::from_min_size(pos2(x, y), size);
    let pressed = ui.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    if pressed.is_some_and(|pos| !rect.contains(pos)) {
        ctx.data_mut(|data| data.remove::<EditMenu>(edit_menu_id()));
        return None;
    }
    let mut action = None;
    let can_paste = crate::clipboard::has_text();
    let radius = theme::CARD_RADIUS.into();
    card(
        ctx,
        None,
        "pad-edit-menu",
        rect,
        egui::Order::Tooltip,
        radius,
        |ui| {
            ui.painter().rect_filled(rect, radius, theme::menu());
            for i in 1..items.len() {
                let x = rect.left() + i as f32 * EDIT_ITEM;
                let divider = [pos2(x, rect.top() + 10.0), pos2(x, rect.bottom() - 10.0)];
                ui.painter()
                    .line_segment(divider, egui::Stroke::new(1.0, theme::divider()));
            }
            for (i, &item) in items.iter().enumerate() {
                let enabled = item != PASTE || can_paste;
                let at = Rect::from_min_size(
                    pos2(rect.left() + i as f32 * EDIT_ITEM, rect.top()),
                    vec2(EDIT_ITEM, TOUCH),
                );
                let resp = hit(ui, controls, at, item, Sense::click(), enabled, None);
                fill(ui, at, &resp, false);
                let color = fade(theme::TEXT, enabled);
                ui.painter().text(
                    at.center(),
                    Align2::CENTER_CENTER,
                    item,
                    theme::body(),
                    color,
                );
                if resp.clicked() {
                    action = Some(match item {
                        CUT => EditAction::Cut,
                        COPY => EditAction::Copy,
                        _ => match crate::clipboard::get() {
                            Some(text) => EditAction::Paste(text),
                            None => continue,
                        },
                    });
                }
            }
        },
    );
    if action.is_some() {
        ctx.data_mut(|data| data.remove::<EditMenu>(edit_menu_id()));
    } else {
        let menu = EditMenu {
            field: field_id,
            rect,
        };
        ctx.data_mut(|data| data.insert_temp(edit_menu_id(), menu));
    }
    action
}

/// The keys of a keypad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keys {
    /// The digits of a hex code, with an alpha or without.
    Hex,
    /// The digits, the minus sign, and the decimal point of a number.
    Number,
}

/// A key of a keypad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Char(char),
    Delete,
    Done,
}

/// The names of the keys that delete a character and that close the
/// keypad. The name of a key is the name of its field and the name of the
/// key, for example `Color hex A`.
pub const DELETE: &str = "Delete";
pub const DONE: &str = "Done";

impl Keys {
    /// The rows of the keys, each with its width in columns.
    fn rows(self) -> &'static [&'static [(Key, u8)]] {
        use Key::{Char as C, Delete, Done};
        match self {
            Keys::Hex => &[
                &[
                    (C('1'), 1),
                    (C('2'), 1),
                    (C('3'), 1),
                    (C('4'), 1),
                    (C('5'), 1),
                ],
                &[
                    (C('6'), 1),
                    (C('7'), 1),
                    (C('8'), 1),
                    (C('9'), 1),
                    (C('0'), 1),
                ],
                &[
                    (C('A'), 1),
                    (C('B'), 1),
                    (C('C'), 1),
                    (C('D'), 1),
                    (C('E'), 1),
                ],
                &[(C('F'), 1), (Delete, 1), (Done, 3)],
            ],
            Keys::Number => &[
                &[(C('7'), 1), (C('8'), 1), (C('9'), 1), (Delete, 1)],
                &[(C('4'), 1), (C('5'), 1), (C('6'), 1), (C('-'), 1)],
                &[(C('1'), 1), (C('2'), 1), (C('3'), 1), (C('.'), 1)],
                &[(C('0'), 2), (Done, 2)],
            ],
        }
    }

    /// The most characters of a text.
    fn max_chars(self) -> usize {
        match self {
            Keys::Hex => 8,
            Keys::Number => 12,
        }
    }

    /// A pasted text with only the characters of the keys.
    fn clean(self, text: &str) -> String {
        let text = text.trim();
        let chars = match self {
            Keys::Hex => text
                .trim_start_matches('#')
                .chars()
                .filter(|c| c.is_ascii_hexdigit())
                .map(|c| c.to_ascii_uppercase())
                .collect::<String>(),
            Keys::Number => text.to_owned(),
        };
        chars.chars().take(self.max_chars()).collect()
    }
}

impl Key {
    fn name(self) -> String {
        match self {
            Key::Char(c) => c.to_string(),
            Key::Delete => DELETE.to_owned(),
            Key::Done => DONE.to_owned(),
        }
    }

    fn label(self) -> String {
        match self {
            Key::Char('-') => "\u{2212}".to_owned(),
            Key::Char(c) => c.to_string(),
            Key::Delete => DELETE.to_owned(),
            Key::Done => DONE.to_owned(),
        }
    }
}

/// The text of the field of the open keypad.
#[derive(Clone)]
pub(super) struct KeyEdit {
    pub(super) field: egui::Id,
    pub(super) name: String,
    pub(super) keys: Keys,
    pub(super) text: String,
    /// The text is the text of the field before the edit. The next key
    /// takes its place.
    pub(super) fresh: bool,
    /// A key changed the text since the field read it.
    pub(super) changed: bool,
    /// The field shows the keypad in its own place.
    pub(super) placed: bool,
}

fn keypad_id() -> egui::Id {
    egui::Id::new("pad-keypad")
}

pub(super) fn key_edit(ctx: &egui::Context) -> Option<KeyEdit> {
    ctx.data(|data| data.get_temp(keypad_id()))
}

pub(super) fn set_key_edit(ctx: &egui::Context, edit: Option<KeyEdit>) {
    ctx.data_mut(|data| match edit {
        Some(edit) => {
            data.insert_temp(keypad_id(), edit);
        }
        None => data.remove::<KeyEdit>(keypad_id()),
    });
}

fn backdrop_id() -> egui::Id {
    egui::Id::new("pad-backdrop")
}

/// Keeps the texture format of the blurred canvas for this frame, for the
/// cards that controls open, or `None` with no blur.
pub fn set_backdrop(ctx: &egui::Context, format: Option<wgpu::TextureFormat>) {
    ctx.data_mut(|data| data.insert_temp(backdrop_id(), format));
}

/// The texture format of the blurred canvas, as `set_backdrop` keeps it.
pub fn backdrop(ctx: &egui::Context) -> Option<wgpu::TextureFormat> {
    ctx.data(|data| data.get_temp(backdrop_id())).flatten()
}

/// Keeps the place of the field whose keypad is open, for this frame. A
/// touch there does not close the keypad.
pub(super) fn set_keypad_field_rect(ctx: &egui::Context, rect: Rect) {
    ctx.data_mut(|data| data.insert_temp(keypad_id().with("field"), rect));
}

/// True if the keypad of a field is open.
pub fn keypad_is_open(ctx: &egui::Context) -> bool {
    key_edit(ctx).is_some()
}

/// The name of the field whose keypad is open.
pub fn keypad_field(ctx: &egui::Context) -> Option<String> {
    key_edit(ctx).map(|edit| edit.name)
}

/// Closes the keypad.
pub fn close_keypad(ctx: &egui::Context) {
    set_key_edit(ctx, None);
}

/// A row of a form with a field that a keypad edits in place of the
/// keyboard of the system. A tap opens and closes the keypad, and a long
/// press opens the edit menu. `shown` is the text while no keypad is open.
/// The text has the warning color if it is not `valid`. A number is at the
/// right of the row, with `suffix` after it. Returns the new text when a key
/// or a paste changes it.
#[allow(clippy::too_many_arguments)]
pub fn key_field(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    label: &str,
    suffix: &str,
    shown: &str,
    keys: Keys,
    valid: impl Fn(&str) -> bool,
) -> Option<String> {
    let (field, right) = match keys {
        Keys::Hex => (form_row(ui, label), false),
        Keys::Number => {
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
            let at = pos2(rect.left(), rect.center().y);
            ui.painter()
                .text(at, Align2::LEFT_CENTER, label, theme::body(), theme::MUTE);
            let field = Rect::from_min_max(
                pos2(rect.right() - FORM_VALUE, rect.center().y - TOUCH / 2.0),
                pos2(rect.right(), rect.center().y + TOUCH / 2.0),
            );
            (field, true)
        }
    };
    key_field_at(
        ui,
        controls,
        (field, right),
        name,
        suffix,
        shown,
        keys,
        valid,
    )
}

/// The field of `key_field` at `field`, with no row. With `right`, the
/// number is at the right of the field, with `suffix` after it, and the
/// field has a fill only while its keypad is open.
#[allow(clippy::too_many_arguments)]
pub fn key_field_at(
    ui: &egui::Ui,
    controls: &mut Controls,
    (field, right): (Rect, bool),
    name: &str,
    suffix: &str,
    shown: &str,
    keys: Keys,
    valid: impl Fn(&str) -> bool,
) -> Option<String> {
    let ctx = ui.ctx().clone();
    let id = ui.id().with(name);
    let resp = hit(ui, controls, field, name, Sense::click(), true, None);
    let mut edit = key_edit(&ctx).filter(|edit| edit.field == id);
    if resp.long_touched() {
        open_edit_menu(&ctx, id, field);
    } else if resp.clicked() {
        edit = match edit {
            Some(_) => None,
            None => Some(KeyEdit {
                field: id,
                name: name.to_owned(),
                keys,
                text: shown.to_owned(),
                fresh: true,
                changed: false,
                placed: false,
            }),
        };
        set_key_edit(&ctx, edit.clone());
    }
    if edit.is_some() {
        set_keypad_field_rect(&ctx, field);
    }
    let mut out = None;
    match edit_menu(ui, controls, id, field) {
        Some(EditAction::Copy) => {
            let text = edit.as_ref().map_or(shown, |edit| edit.text.as_str());
            crate::clipboard::set(&ctx, text);
        }
        Some(EditAction::Paste(pasted)) => {
            let text = keys.clean(&pasted);
            if let Some(edit) = &mut edit {
                edit.text = text.clone();
                edit.fresh = false;
                set_key_edit(&ctx, Some(edit.clone()));
            }
            out = Some(text);
        }
        Some(EditAction::Cut) | None => {}
    }
    if let Some(edit) = edit.as_mut().filter(|edit| edit.changed) {
        edit.changed = false;
        out = Some(edit.text.clone());
        set_key_edit(&ctx, Some(edit.clone()));
    }

    let painter = ui.painter();
    let back = field.shrink(HIGHLIGHT_INSET);
    if right {
        if edit.is_some() {
            painter.rect_filled(back, theme::CONTROL_RADIUS, theme::raise());
        }
    } else {
        painter.rect_filled(back, theme::CONTROL_RADIUS, theme::raise());
        if edit.is_some() {
            let stroke = egui::Stroke::new(1.5, theme::ACCENT);
            painter.rect_stroke(
                back,
                theme::CONTROL_RADIUS,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
    }
    let mut inner = field.shrink2(vec2(12.0, 0.0));
    if right {
        let at = pos2(inner.right(), inner.center().y);
        painter.text(at, Align2::RIGHT_CENTER, suffix, theme::body(), theme::MUTE);
        inner.max.x -= text_width(ui, suffix, theme::body()) + 6.0;
    }
    let text = edit.as_ref().map_or(shown, |edit| edit.text.as_str());
    let color = if valid(text) {
        theme::TEXT
    } else {
        theme::WARN
    };
    let galley = painter.layout_no_wrap(text.to_owned(), theme::body(), color);
    let x = if right {
        inner.right() - galley.size().x
    } else {
        inner.left()
    };
    let at = pos2(x, inner.center().y - galley.size().y / 2.0);
    let shape = Rect::from_min_size(at, galley.size());
    match &edit {
        // The text that the next key replaces is selected.
        Some(edit) if edit.fresh && !text.is_empty() => {
            painter.rect_filled(shape.expand(2.0), 3.0, theme::ACCENT.gamma_multiply(0.45));
        }
        Some(_) => {
            let x = if right {
                inner.right() + 1.0
            } else {
                shape.right() + 1.0
            };
            let caret = Rect::from_center_size(pos2(x, inner.center().y), vec2(2.0, 22.0));
            painter.rect_filled(caret, 1.0, theme::ACCENT);
        }
        None => {}
    }
    painter.galley(at, galley, color);
    out
}

/// The keypad of the field whose keypad is open, `height` tall. Returns
/// false and adds nothing if no keypad is open. A key changes the text of
/// the field in this frame or the next one, as the field reads it.
pub fn keypad(ui: &mut egui::Ui, controls: &mut Controls, height: f32) -> bool {
    let ctx = ui.ctx().clone();
    let Some(mut edit) = key_edit(&ctx) else {
        return false;
    };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    // A tap anywhere but the keypad, its field, and the edit menu closes the
    // keypad. A drag, for example a scroll of the panel, does not.
    let press = ui.input(|i| i.pointer.interact_pos().filter(|_| i.pointer.any_click()));
    let field = ctx.data(|data| data.get_temp::<Rect>(keypad_id().with("field")));
    let inside = |at: Pos2| {
        let menu = edit_menu_rect(&ctx);
        rect.expand(PAD * 2.0).contains(at)
            || field.is_some_and(|f| f.contains(at))
            || menu.is_some_and(|m| m.contains(at))
    };
    if press.is_some_and(|at| !inside(at)) {
        set_key_edit(&ctx, None);
        return true;
    }
    let rows = edit.keys.rows();
    let columns: u8 = rows[0].iter().map(|(_, span)| span).sum();
    let width = rect.width() / f32::from(columns);
    let tall = rect.height() / rows.len() as f32;
    let mut done = false;
    for (r, row) in rows.iter().enumerate() {
        let mut column = 0;
        for &(key, span) in row.iter() {
            let at = Rect::from_min_size(
                pos2(
                    rect.left() + f32::from(column) * width,
                    rect.top() + r as f32 * tall,
                ),
                vec2(f32::from(span) * width, tall),
            );
            column += span;
            let name = format!("{} {}", edit.name, key.name());
            let resp = hit(ui, controls, at, &name, Sense::click(), true, None);
            let back = at.shrink(HIGHLIGHT_INSET);
            let fill = match (resp.is_pointer_button_down_on(), key) {
                (true, _) => theme::raise().gamma_multiply(2.0),
                (false, Key::Done) => theme::ACCENT,
                (false, _) => theme::raise(),
            };
            ui.painter().rect_filled(back, theme::CONTROL_RADIUS, fill);
            let font = egui::FontId::proportional(theme::body().size + 2.0);
            match key {
                Key::Delete => icons::paint(ui.painter(), at, Icon::Delete, ICON, theme::TEXT),
                _ => {
                    let label = key.label();
                    ui.painter()
                        .text(at.center(), Align2::CENTER_CENTER, label, font, theme::TEXT);
                }
            }
            if !resp.clicked() {
                continue;
            }
            match key {
                Key::Char(c) => {
                    if edit.fresh {
                        edit.text.clear();
                    }
                    if edit.text.chars().count() < edit.keys.max_chars() {
                        edit.text.push(c);
                    }
                }
                Key::Delete => {
                    if edit.fresh {
                        edit.text.clear();
                    }
                    edit.text.pop();
                }
                Key::Done => done = true,
            }
            edit.fresh = false;
            edit.changed = true;
            ctx.request_repaint();
        }
    }
    set_key_edit(&ctx, (!done).then_some(edit));
    true
}

/// The height of the keypad of a field in a panel.
pub const KEYPAD: f32 = 4.0 * PANEL_ROW;

/// The keypad of the field `name`, below the field, if it is open. The
/// panel scrolls to show the keypad when it opens.
pub fn keypad_below(ui: &mut egui::Ui, controls: &mut Controls, name: &str) {
    let id = egui::Id::new(("pad-atlas-keypad", name));
    let open = keypad_field(ui.ctx()).as_deref() == Some(name);
    let shown = ui.data_mut(|d| std::mem::replace(d.get_temp_mut_or_default::<bool>(id), open));
    if !open {
        return;
    }
    let top = ui.cursor().top();
    keypad(ui, controls, KEYPAD);
    if !shown {
        let rect = Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=ui.cursor().top());
        ui.scroll_to_rect_animation(rect, None, egui::style::ScrollAnimation::none());
    }
}

/// A row of a form with a segmented control: choices of equal width on one
/// track. `active` is the choice that is on. Returns the choice that the
/// user made.
pub fn choice_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    label: &str,
    choices: &[&str],
    active: Option<usize>,
) -> Option<usize> {
    let field = form_row(ui, label);
    let track = field.shrink(HIGHLIGHT_INSET);
    ui.painter()
        .rect_filled(track, theme::CONTROL_RADIUS, theme::track());
    let width = field.width() / choices.len() as f32;
    let mut chosen = None;
    for (index, choice) in choices.iter().enumerate() {
        let min = pos2(field.left() + index as f32 * width, field.top());
        let rect = Rect::from_min_size(min, vec2(width, TOUCH));
        let on = active == Some(index);
        let resp = hit(ui, controls, rect, choice, Sense::click(), true, Some(on));
        if on || resp.is_pointer_button_down_on() {
            let thumb = rect.intersect(track).shrink(2.0);
            ui.painter()
                .rect_filled(thumb, theme::CONTROL_RADIUS - 2.0, theme::raise());
        }
        let color = if on { theme::TEXT } else { theme::MUTE };
        let at = rect.center();
        ui.painter()
            .text(at, Align2::CENTER_CENTER, *choice, theme::body(), color);
        if resp.clicked() && !on {
            chosen = Some(index);
        }
    }
    chosen
}

/// The padding at the left and the right of the text of a chip.
const CHIP_PAD: f32 = 14.0;

/// A set of choices that wraps to the width of the form. Each choice is as
/// wide as its text and as high as a touch target. `active` is the choice
/// that is on. Returns the choice that the user made.
pub fn chips(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    choices: &[&str],
    active: usize,
) -> Option<usize> {
    let (left, top, width) = (ui.cursor().left(), ui.cursor().top(), ui.available_width());
    let mut rects = Vec::with_capacity(choices.len());
    let mut at = pos2(left, top);
    for choice in choices {
        let wide = text_width(ui, choice, theme::body()) + 2.0 * CHIP_PAD;
        let size = vec2(wide.max(TOUCH), TOUCH);
        if at.x > left && at.x + size.x > left + width {
            at = pos2(left, at.y + TOUCH);
        }
        rects.push(Rect::from_min_size(at, size));
        at.x += size.x;
    }
    ui.allocate_exact_size(vec2(width, at.y + TOUCH - top), Sense::hover());
    let mut chosen = None;
    for (index, (choice, rect)) in choices.iter().zip(rects).enumerate() {
        let on = active == index;
        let resp = hit(ui, controls, rect, choice, Sense::click(), true, Some(on));
        let back = rect.shrink(HIGHLIGHT_INSET);
        let pressed = on || resp.is_pointer_button_down_on();
        let fill = if pressed {
            theme::raise()
        } else {
            theme::track()
        };
        ui.painter().rect_filled(back, theme::CONTROL_RADIUS, fill);
        let color = if on { theme::TEXT } else { theme::MUTE };
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            *choice,
            theme::body(),
            color,
        );
        if resp.clicked() && !on {
            chosen = Some(index);
        }
    }
    chosen
}

/// One row of chips that scrolls sideways, as wide as the form. Each chip
/// is a pair of a control name and a text. Returns the chip that the user
/// tapped.
pub fn chip_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    id: egui::Id,
    chips: &[(String, String)],
) -> Option<usize> {
    let mut chosen = None;
    egui::ScrollArea::horizontal()
        .id_salt(id)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            let mut at = ui.cursor().min;
            for (index, (name, text)) in chips.iter().enumerate() {
                let wide = text_width(ui, text, theme::body()) + 2.0 * CHIP_PAD;
                let rect = Rect::from_min_size(at, vec2(wide.max(TOUCH), TOUCH));
                at.x = rect.right();
                let resp = hit(ui, controls, rect, name, Sense::click(), true, None);
                let back = rect.shrink(HIGHLIGHT_INSET);
                let fill = if resp.is_pointer_button_down_on() {
                    theme::raise()
                } else {
                    theme::track()
                };
                ui.painter().rect_filled(back, theme::CONTROL_RADIUS, fill);
                let painter = ui.painter();
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    text,
                    theme::body(),
                    theme::TEXT,
                );
                if resp.clicked() {
                    chosen = Some(index);
                }
            }
            let left = ui.cursor().left();
            ui.allocate_exact_size(vec2(at.x - left, TOUCH), Sense::hover());
        });
    chosen
}

/// The smallest width of a tile of a tile grid.
pub const TILE: f32 = 76.0;
/// The space between two tiles.
const TILE_GAP: f32 = 8.0;
/// The height of the name at the bottom of a tile.
const TILE_NAME: f32 = 20.0;

/// A grid of tiles that wraps to the width of the form. The tiles share the
/// width of the form, with a gap between them, and each one is at least
/// `TILE` wide. The edges of the grid line up with the edges of a button
/// row. Each tile has a picture at the top and its name at the bottom.
/// `paint` paints the picture of a tile in the square that it gets. `active`
/// is the tile that is on. Returns the tile that the user tapped.
pub fn tiles(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    names: &[&str],
    active: Option<usize>,
    paint: impl Fn(&egui::Painter, Rect, usize),
) -> Option<usize> {
    let width = ui.available_width() - 2.0 * HIGHLIGHT_INSET;
    let (left, top) = (ui.cursor().left() + HIGHLIGHT_INSET, ui.cursor().top());
    let columns = (((width + TILE_GAP) / (TILE + TILE_GAP)).floor() as usize).max(1);
    let rows = names.len().div_ceil(columns);
    let side = (width - (columns - 1) as f32 * TILE_GAP) / columns as f32;
    let picture = (side - TILE_NAME - 8.0).min(56.0);
    let height = 8.0 + picture + TILE_NAME + 4.0;
    let total = rows as f32 * height + rows.saturating_sub(1) as f32 * TILE_GAP;
    ui.allocate_exact_size(vec2(ui.available_width(), total), Sense::hover());
    let mut chosen = None;
    for (index, name) in names.iter().enumerate() {
        let (row, column) = (index / columns, index % columns);
        let min = pos2(
            left + column as f32 * (side + TILE_GAP),
            top + row as f32 * (height + TILE_GAP),
        );
        let rect = Rect::from_min_size(min, vec2(side, height));
        let on = active == Some(index);
        let resp = hit(ui, controls, rect, name, Sense::click(), true, Some(on));
        let fill = if on || resp.is_pointer_button_down_on() {
            theme::raise()
        } else {
            theme::track()
        };
        let painter = ui.painter();
        painter.rect_filled(rect, theme::CONTROL_RADIUS, fill);
        if on {
            let stroke = egui::Stroke::new(2.0, theme::ACCENT);
            painter.rect_stroke(
                rect,
                theme::CONTROL_RADIUS,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        let square = Rect::from_center_size(
            pos2(rect.center().x, rect.top() + 8.0 + picture / 2.0),
            vec2(picture, picture),
        );
        paint(painter, square, index);
        let color = if on { theme::TEXT } else { theme::MUTE };
        let mut text = (*name).to_owned();
        let mut galley = painter.layout_no_wrap(text.clone(), theme::small(), color);
        while galley.size().x > rect.width() - 8.0 && text.pop().is_some() {
            galley = painter.layout_no_wrap(format!("{text}…"), theme::small(), color);
        }
        let middle = pos2(rect.center().x, rect.bottom() - 4.0 - TILE_NAME / 2.0);
        let at = middle - galley.size() / 2.0;
        painter.galley(at, galley, color);
        if resp.clicked() {
            chosen = Some(index);
        }
    }
    chosen
}

/// The width of the value at the right of a row of a form.
const FORM_VALUE: f32 = 132.0;
/// The height of the names below the ticks of a tick slider.
pub const TICK_NAMES: f32 = 20.0;

/// A row of a form with a value that the user can type: the label at the
/// left, and the value with its unit at the right. A tap on the value
/// starts the edit. `name` is the name of the control. The text has the
/// warning color if it is not `valid`.
pub fn value_field(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    label: &str,
    suffix: &str,
    text: &mut String,
    valid: bool,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let at = pos2(rect.left(), rect.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, label, theme::body(), theme::MUTE);
    let field = Rect::from_min_max(
        pos2(rect.right() - FORM_VALUE, rect.center().y - TOUCH / 2.0),
        pos2(rect.right(), rect.center().y + TOUCH / 2.0),
    );
    let id = ui.id().with(name);
    if ui.memory(|memory| memory.has_focus(id)) {
        let back = field.shrink(HIGHLIGHT_INSET);
        ui.painter()
            .rect_filled(back, theme::CONTROL_RADIUS, theme::raise());
    }
    let mut inner = field.shrink2(vec2(12.0, 0.0));
    let at = pos2(inner.right(), inner.center().y);
    ui.painter()
        .text(at, Align2::RIGHT_CENTER, suffix, theme::body(), theme::MUTE);
    inner.max.x -= text_width(ui, suffix, theme::body()) + 6.0;
    let color = if valid { theme::TEXT } else { theme::WARN };
    let edit = egui::TextEdit::singleline(text)
        .id(id)
        .frame(egui::Frame::NONE)
        .font(theme::body())
        .text_color(color)
        .horizontal_align(egui::Align::RIGHT)
        .vertical_align(egui::Align::Center);
    let resp = ui.put(inner, edit);
    push(ui, controls, field, name);
    resp
}

/// A small text below the value of a row of a form, in the warning color.
pub fn form_note(ui: &mut egui::Ui, text: &str, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let text = egui::RichText::new(text)
        .font(theme::small())
        .color(theme::WARN);
    let place = rect.shrink2(vec2(12.0, 0.0));
    let layout = egui::Layout::right_to_left(egui::Align::Center);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(place).layout(layout),
        |ui| {
            ui.add(egui::Label::new(text).selectable(false));
        },
    );
}

/// A rectangle that takes a tap, with no paint of its own.
pub fn tap_area(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    enabled: bool,
) -> Response {
    hit(ui, controls, rect, name, Sense::click(), enabled, None)
}

/// A rectangle that takes a tap and a drag.
pub fn drag_area(ui: &egui::Ui, controls: &mut Controls, rect: Rect, name: &str) -> Response {
    hit(
        ui,
        controls,
        rect,
        name,
        Sense::click_and_drag(),
        true,
        None,
    )
}

/// A button with a text on the accent color. Its right edge and its middle
/// are at `right`.
pub fn accent_button(
    ui: &egui::Ui,
    controls: &mut Controls,
    right: egui::Pos2,
    name: &str,
) -> Response {
    let width = text_width(ui, name, theme::body()) + 2.0 * 18.0;
    let rect = Rect::from_min_max(
        pos2(right.x - width, right.y - TOUCH / 2.0),
        pos2(right.x, right.y + TOUCH / 2.0),
    );
    let resp = hit(ui, controls, rect, name, Sense::click(), true, None);
    let color = if resp.is_pointer_button_down_on() {
        theme::ACCENT
    } else {
        theme::ACCENT.gamma_multiply(0.8)
    };
    let back = rect.shrink(HIGHLIGHT_INSET);
    ui.painter().rect_filled(back, theme::CONTROL_RADIUS, color);
    let at = rect.center();
    ui.painter().text(
        at,
        Align2::CENTER_CENTER,
        name,
        theme::body(),
        theme::CANVAS,
    );
    resp
}

/// A row of a panel that is one button with its text in the middle.
pub fn button_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    enabled: bool,
) -> Response {
    button_row_in(ui, controls, name, enabled, theme::ACCENT)
}

/// A button row with this fill color.
pub fn button_row_in(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    enabled: bool,
    fill: Color32,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let resp = hit(ui, controls, rect, name, Sense::click(), enabled, None);
    let back = rect.shrink(HIGHLIGHT_INSET);
    let color = if resp.is_pointer_button_down_on() {
        fill
    } else {
        fill.gamma_multiply(0.8)
    };
    ui.painter()
        .rect_filled(back, theme::CONTROL_RADIUS, fade(color, enabled));
    let at = rect.center();
    ui.painter().text(
        at,
        Align2::CENTER_CENTER,
        name,
        theme::body(),
        theme::CANVAS,
    );
    resp
}

/// The height of the square of saturation and value of the color picker.
const PICKER_SQUARE: f32 = 180.0;
/// The number of cells along each side of the meshes of the color picker.
const PICKER_CELLS: usize = 24;

/// The color as a hex code: `RRGGBB` for an opaque color, and `RRGGBBAA`
/// for a color with alpha.
pub fn hex_text(color: [u8; 4]) -> String {
    let [r, g, b, a] = color;
    if a == 255 {
        format!("{r:02X}{g:02X}{b:02X}")
    } else {
        format!("{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

/// The color of a hex code, with or without a `#` before it. Six digits
/// give an opaque color. `alpha`: the code can also have eight digits, the
/// last two of which are the alpha.
pub fn parse_hex(text: &str, alpha: bool) -> Option<[u8; 4]> {
    let text = text.trim().trim_start_matches('#');
    let digits = text.len();
    if !(digits == 6 || alpha && digits == 8) || !text.is_ascii() {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&text[at..at + 2], 16).ok();
    let a = if digits == 8 { byte(6)? } else { 255 };
    Some([byte(0)?, byte(2)?, byte(4)?, a])
}

/// A color picker for a finger, for an opaque color: a square of saturation
/// and value, a strip of hues, and a field for the hex code. `name` starts
/// the names of its controls. Returns true if the user changed the color.
pub fn color_picker(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    color: &mut [u8; 3],
) -> bool {
    let [r, g, b] = *color;
    let mut rgba = [r, g, b, 255];
    let changed = picker(ui, controls, name, &mut rgba, false);
    *color = [rgba[0], rgba[1], rgba[2]];
    changed
}

/// A color picker for a finger, for a color with alpha: the picker of
/// [`color_picker`] with a strip of opacities below the strip of hues.
pub fn color_picker_alpha(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    color: &mut [u8; 4],
) -> bool {
    picker(ui, controls, name, color, true)
}

fn picker(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    color: &mut [u8; 4],
    alpha: bool,
) -> bool {
    use egui::ecolor::Hsva;
    // The hue stays while the color is grey, so it is kept between frames.
    let id = ui.id().with(name).with("hsva");
    let kept: Option<(Hsva, [u8; 3])> = ui.data(|data| data.get_temp(id));
    let rgb = |c: &[u8; 4]| [c[0], c[1], c[2]];
    let mut hsva = match kept {
        Some((hsva, kept)) if kept == rgb(color) => hsva,
        _ => Hsva::from_srgb(rgb(color)),
    };
    let before = *color;
    // The eyedropper picks the red, green, and blue, and the alpha stays.
    if let Some([r, g, b]) = super::eyedropper::take(ui.ctx(), name) {
        *color = [r, g, b, color[3]];
        hsva = Hsva::from_srgb([r, g, b]);
    }
    let width = ui.available_width();
    let fill = |h: f32, s: f32, v: f32| Color32::from(Hsva::new(h, s, v, 1.0));

    // The keypad of a field takes the place of the square and the strip,
    // unless the field shows its keypad itself.
    let shown = key_edit(ui.ctx()).is_some_and(|edit| !edit.placed);
    let strips = if alpha { 2.0 * TOUCH } else { TOUCH };
    if !(shown && keypad(ui, controls, PICKER_SQUARE + 8.0 + strips)) {
        let (square, _) = ui.allocate_exact_size(vec2(width, PICKER_SQUARE), Sense::hover());
        let shade = format!("{name} shade");
        let resp = hit(
            ui,
            controls,
            square,
            &shade,
            Sense::click_and_drag(),
            true,
            None,
        );
        let mut moved = false;
        if let Some(pos) = resp.interact_pointer_pos()
            && (resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
        {
            hsva.s = ((pos.x - square.left()) / square.width()).clamp(0.0, 1.0);
            hsva.v = (1.0 - (pos.y - square.top()) / square.height()).clamp(0.0, 1.0);
            moved = true;
        }
        let mut mesh = egui::Mesh::default();
        let n = PICKER_CELLS;
        for row in 0..=n {
            for col in 0..=n {
                let (s, v) = (col as f32 / n as f32, 1.0 - row as f32 / n as f32);
                let at = pos2(
                    square.left() + s * square.width(),
                    square.top() + (1.0 - v) * square.height(),
                );
                mesh.colored_vertex(at, fill(hsva.h, s, v));
            }
        }
        let corner = |row: usize, col: usize| (row * (n + 1) + col) as u32;
        for row in 0..n {
            for col in 0..n {
                let (a, b) = (corner(row, col), corner(row, col + 1));
                let (c, d) = (corner(row + 1, col + 1), corner(row + 1, col));
                mesh.add_triangle(a, b, c);
                mesh.add_triangle(a, c, d);
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        let at = pos2(
            square.left() + hsva.s * square.width(),
            square.top() + (1.0 - hsva.v) * square.height(),
        );
        ui.painter()
            .circle_stroke(at, THUMB / 2.0, egui::Stroke::new(3.0, Color32::BLACK));
        ui.painter()
            .circle_stroke(at, THUMB / 2.0, egui::Stroke::new(1.5, Color32::WHITE));
        ui.add_space(8.0);

        let (row, _) = ui.allocate_exact_size(vec2(width, TOUCH), Sense::hover());
        let (strip, button) = row.split_left_right_at_x(row.right() - TOUCH - PAD);
        let button = Rect::from_min_size(button.right_top() - vec2(TOUCH, 0.0), vec2(TOUCH, TOUCH));
        let eyedropper = format!("{name} eyedropper");
        let on = super::eyedropper::active_for(ui.ctx(), name);
        let icon = Icon::Eyedropper;
        if icon_button_at(ui, controls, button, &eyedropper, icon, on, true).clicked() {
            super::eyedropper::start(ui.ctx(), name);
        }
        let hue = format!("{name} hue");
        let resp = hit(
            ui,
            controls,
            strip,
            &hue,
            Sense::click_and_drag(),
            true,
            None,
        );
        if let Some(pos) = resp.interact_pointer_pos()
            && (resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
        {
            hsva.h = ((pos.x - strip.left()) / strip.width()).clamp(0.0, 1.0);
            moved = true;
        }
        let bar = strip.shrink2(vec2(0.0, 8.0));
        let mut mesh = egui::Mesh::default();
        for col in 0..=n {
            let h = col as f32 / n as f32;
            let x = bar.left() + h * bar.width();
            mesh.colored_vertex(pos2(x, bar.top()), fill(h, 1.0, 1.0));
            mesh.colored_vertex(pos2(x, bar.bottom()), fill(h, 1.0, 1.0));
        }
        for col in 0..n as u32 {
            let at = 2 * col;
            mesh.add_triangle(at, at + 1, at + 3);
            mesh.add_triangle(at, at + 3, at + 2);
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        let x = bar.left() + hsva.h * bar.width();
        let thumb = Rect::from_center_size(pos2(x, strip.center().y), vec2(8.0, TOUCH - 8.0));
        ui.painter().rect_filled(thumb, 3.0, theme::TEXT);
        ui.painter().rect_stroke(
            thumb,
            3.0,
            egui::Stroke::new(1.0, Color32::BLACK),
            egui::StrokeKind::Outside,
        );
        if moved {
            let [r, g, b] = hsva.to_srgb();
            *color = [r, g, b, color[3]];
        }
        if alpha {
            opacity_strip(ui, controls, name, color, width);
        }
    }
    let hex = format!("{name} hex");
    let typed = key_field(
        ui,
        controls,
        &hex,
        "Hex",
        "",
        &hex_text(*color),
        Keys::Hex,
        |text| parse_hex(text, alpha).is_some(),
    );
    if let Some(typed) = typed.as_deref().and_then(|text| parse_hex(text, alpha)) {
        *color = typed;
        hsva = Hsva::from_srgb(rgb(&typed));
    }
    ui.data_mut(|data| data.insert_temp(id, (hsva, rgb(color))));
    *color != before
}

/// The strip of opacities of the color picker: the color from transparent
/// at the left to opaque at the right, over a checkerboard. The percent of
/// the alpha is at the right of the strip. A tap or a drag sets the alpha.
fn opacity_strip(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    color: &mut [u8; 4],
    width: f32,
) {
    let (row, _) = ui.allocate_exact_size(vec2(width, TOUCH), Sense::hover());
    let (strip, label) = row.split_left_right_at_x(row.right() - TOUCH - PAD);
    let opacity = format!("{name} opacity");
    let resp = hit(
        ui,
        controls,
        strip,
        &opacity,
        Sense::click_and_drag(),
        true,
        None,
    );
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
    {
        let t = ((pos.x - strip.left()) / strip.width()).clamp(0.0, 1.0);
        color[3] = (t * 255.0).round() as u8;
    }
    let bar = strip.shrink2(vec2(0.0, 8.0));
    let painter = ui.painter();
    paint_checker(painter, bar, bar.height() / 2.0, 0);
    let [r, g, b, a] = *color;
    let mut mesh = egui::Mesh::default();
    let clear = Color32::from_rgba_unmultiplied(r, g, b, 0);
    let solid = Color32::from_rgb(r, g, b);
    mesh.colored_vertex(bar.left_top(), clear);
    mesh.colored_vertex(bar.left_bottom(), clear);
    mesh.colored_vertex(bar.right_top(), solid);
    mesh.colored_vertex(bar.right_bottom(), solid);
    mesh.add_triangle(0, 1, 3);
    mesh.add_triangle(0, 3, 2);
    painter.add(egui::Shape::mesh(mesh));
    let x = bar.left() + a as f32 / 255.0 * bar.width();
    let thumb = Rect::from_center_size(pos2(x, strip.center().y), vec2(8.0, TOUCH - 8.0));
    painter.rect_filled(thumb, 3.0, theme::TEXT);
    painter.rect_stroke(
        thumb,
        3.0,
        egui::Stroke::new(1.0, Color32::BLACK),
        egui::StrokeKind::Outside,
    );
    let percent = format!("{}%", (a as f32 / 2.55).round());
    painter.text(
        label.right_center(),
        Align2::RIGHT_CENTER,
        percent,
        theme::small(),
        theme::MUTE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_on_the_compass_dial_gives_its_angle() {
        let c = pos2(100.0, 100.0);
        assert_eq!(compass_angle(c, pos2(105.0, 90.0)), None);
        assert_eq!(compass_angle(c, pos2(100.0, 40.0)), Some(0.0));
        assert_eq!(compass_angle(c, pos2(160.0, 100.0)), Some(90.0));
        assert_eq!(compass_angle(c, pos2(100.0, 160.0)), Some(180.0));
        assert_eq!(compass_angle(c, pos2(40.0, 100.0)), Some(270.0));
        // 2 degrees off a compass point snaps to it, 5 degrees does not.
        let at = |deg: f32| {
            let a = deg.to_radians();
            c + 60.0 * vec2(a.sin(), -a.cos())
        };
        assert_eq!(compass_angle(c, at(47.0)), Some(45.0));
        assert_eq!(compass_angle(c, at(358.0)), Some(0.0));
        assert_eq!(compass_angle(c, at(50.0)), Some(50.0));
    }
}
