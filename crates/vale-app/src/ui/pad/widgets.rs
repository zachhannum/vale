//! The controls of the iPad layout. Each one is at least 44 points on each side.

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Response, Sense, pos2, vec2};

use eframe::egui_wgpu::wgpu;

use super::geometry::{CELL, PAD, ROW, TOUCH};
use super::icons::{self, Icon};
use super::theme;
use crate::globe::backdrop::card_shape;

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

fn fade(color: Color32, enabled: bool) -> Color32 {
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
        .constrain(false)
        .fade_in(false)
        .sizing_pass(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
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

/// Makes a rectangle a control. `selected` is the state of a control that
/// stays on. A control that is not enabled takes no touch. A control is not
/// in the list while a scroll area cuts it to less than the touch size.
fn hit(
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
fn push(ui: &egui::Ui, controls: &mut Controls, rect: Rect, name: &str) {
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

/// One cell of the tool strip: an icon with a name below it.
pub fn tool_cell(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    label: &str,
    icon: Icon,
    active: bool,
    enabled: bool,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(CELL.0, CELL.1), Sense::hover());
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
    let icon_rect = Rect::from_center_size(rect.center() - vec2(0.0, 7.0), vec2(ICON, ICON));
    icons::paint(ui.painter(), icon_rect, icon, ICON, color);
    let at = pos2(rect.center().x, rect.bottom() - 7.0);
    ui.painter()
        .text(at, Align2::CENTER_BOTTOM, label, theme::small(), color);
    resp
}

fn track(ui: &egui::Ui, track: Rect, filled: Rect, thumb: egui::Pos2) {
    let painter = ui.painter();
    painter.rect_filled(track, 3.0, theme::track());
    painter.rect_filled(filled, 3.0, theme::MUTE);
    painter.circle_filled(
        thumb + vec2(0.0, 1.0),
        THUMB / 2.0 + 1.0,
        Color32::from_black_alpha(70),
    );
    painter.circle_filled(thumb, THUMB / 2.0, theme::TEXT);
}

/// A slider that you move up and down. `fraction` is the value, from 0 at the
/// bottom to 1 at the top. Returns true if the user moved it.
pub fn vertical_slider(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    height: f32,
    fraction: &mut f64,
    value: &str,
    enabled: bool,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(CELL.0, height), Sense::hover());
    let resp = hit(
        ui,
        controls,
        rect,
        name,
        Sense::click_and_drag(),
        enabled,
        None,
    );
    let top = rect.top() + 20.0 + THUMB / 2.0;
    let bottom = rect.bottom() - 20.0 - THUMB / 2.0;
    let before = *fraction;
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
    {
        *fraction = f64::from((bottom - pos.y) / (bottom - top)).clamp(0.0, 1.0);
    }
    let x = rect.center().x;
    let y = bottom - *fraction as f32 * (bottom - top);
    let whole = Rect::from_min_max(pos2(x - 3.0, top - 4.0), pos2(x + 3.0, bottom + 4.0));
    let filled = Rect::from_min_max(pos2(x - 3.0, y), whole.max);
    if enabled {
        track(ui, whole, filled, pos2(x, y));
    } else {
        ui.painter().rect_filled(whole, 3.0, theme::track());
    }
    let painter = ui.painter();
    let color = fade(theme::MUTE, enabled);
    let at = pos2(x, rect.top() + 6.0);
    painter.text(at, Align2::CENTER_TOP, value, theme::small(), color);
    let at = pos2(x, rect.bottom() - 6.0);
    painter.text(at, Align2::CENTER_BOTTOM, name, theme::small(), color);
    *fraction != before
}

/// A slider in a row of a panel: the name, the track, and the value.
/// `label` is the name on screen. Returns true if the user moved it. A drag
/// that starts up or down scrolls the panel and does not move the slider.
pub fn row_slider(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    label: &str,
    fraction: &mut f64,
    value: &str,
) -> bool {
    let size = vec2(ui.available_width(), PANEL_ROW);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let resp = hit(
        ui,
        controls,
        rect,
        name,
        Sense::click_and_drag(),
        true,
        None,
    );
    let label = ui
        .painter()
        .layout_no_wrap(label.to_string(), theme::body(), theme::TEXT);
    let left = rect.left() + (label.size().x + 16.0).max(72.0) + THUMB / 2.0;
    let right = rect.right() - 56.0 - THUMB / 2.0;
    let before = *fraction;
    let scroll_id = resp.id.with("scroll");
    if resp.drag_started() {
        let pointer = ui.input(|i| i.pointer.press_origin().zip(i.pointer.interact_pos()));
        let moved = pointer.map_or(egui::Vec2::ZERO, |(from, to)| to - from);
        let scroll = moved.y.abs() > moved.x.abs();
        ui.data_mut(|data| data.insert_temp(scroll_id, scroll));
    }
    if resp.dragged() && ui.data(|data| data.get_temp(scroll_id)) == Some(true) {
        ui.scroll_with_delta(vec2(0.0, resp.drag_delta().y));
    } else if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        *fraction = f64::from((pos.x - left) / (right - left)).clamp(0.0, 1.0);
    }
    let y = rect.center().y;
    let x = left + *fraction as f32 * (right - left);
    let whole = Rect::from_min_max(pos2(left - 6.0, y - 3.0), pos2(right + 6.0, y + 3.0));
    let filled = Rect::from_min_max(whole.min, pos2(x, y + 3.0));
    track(ui, whole, filled, pos2(x, y));
    let painter = ui.painter();
    let at = pos2(rect.left(), y - label.size().y / 2.0);
    painter.galley(at, label, theme::TEXT);
    let at = pos2(rect.right(), y);
    painter.text(at, Align2::RIGHT_CENTER, value, theme::mono(), theme::MUTE);
    *fraction != before
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
    let size = vec2(ui.available_width(), PANEL_ROW);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let resp = hit(ui, controls, rect, name, Sense::click(), true, Some(*on));
    let changed = resp.clicked();
    if changed {
        *on = !*on;
    }
    let y = rect.center().y;
    let track = Rect::from_min_size(pos2(rect.right() - SWITCH.x, y - SWITCH.y / 2.0), SWITCH);
    let reach = (SWITCH.x - SWITCH.y) / 2.0;
    let (color, x) = match *on {
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
    let at = pos2(rect.left(), y);
    painter.text(at, Align2::LEFT_CENTER, label, theme::body(), theme::TEXT);
    changed
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
/// behind it. The eye and the arrow take touch only at a shift of zero.
#[allow(clippy::too_many_arguments)]
pub fn layer_row(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    icon: Icon,
    name: &str,
    visible: Option<bool>,
    selected: bool,
    live: bool,
    lifted: bool,
    shift: f32,
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
    let mut delete = false;
    if shift < 0.0 {
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
        if live && -shift >= DELETE_WIDTH - 0.5 {
            let area = Rect::from_min_max(
                pos2(reveal.left(), rect.center().y - TOUCH / 2.0),
                pos2(edge, rect.center().y + TOUCH / 2.0),
            );
            delete = tap_area(ui, controls, area, &delete_name(name), true).clicked();
        }
    }
    // The row slides over the Delete button.
    let row_clip = Rect::from_x_y_ranges(highlight.left()..=edge + shift, rect.y_range());
    let painter = ui
        .painter()
        .with_clip_rect(ui.clip_rect().intersect(row_clip));
    let moved = vec2(shift, 0.0);
    let highlight = highlight.translate(moved);
    if lifted {
        painter.rect_filled(highlight, 5.0, theme::card(false).to_opaque());
    }
    if selected || lifted || body.as_ref().is_some_and(|b| b.is_pointer_button_down_on()) {
        painter.rect_filled(highlight, 5.0, theme::raise());
    }
    let kind = Rect::from_center_size(
        pos2(rect.left() + 10.0 + shift, rect.center().y),
        vec2(18.0, 18.0),
    );
    icons::paint(&painter, kind, icon, 18.0, theme::MUTE);
    let shown = visible != Some(false);
    let clip = Rect::from_min_max(
        pos2(rect.left() + 28.0, rect.top()),
        pos2(body_right - 4.0, rect.bottom()),
    )
    .translate(moved);
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
    let eye_name = format!("{} {name}", if shown { "Hide" } else { "Show" });
    let open_name = format!("Open {name}");
    let (mut eye, mut open) = (false, false);
    if live && shift == 0.0 {
        if visible.is_some() {
            eye =
                icon_button_at(ui, controls, eye_rect, &eye_name, eye_icon, false, true).clicked();
        }
        open =
            icon_button_at(ui, controls, open_rect, &open_name, Icon::Open, false, true).clicked();
    } else {
        if visible.is_some() {
            icons::paint(
                &painter,
                eye_rect.translate(moved),
                eye_icon,
                ICON,
                theme::MUTE,
            );
        }
        icons::paint(
            &painter,
            open_rect.translate(moved),
            Icon::Open,
            ICON,
            theme::MUTE,
        );
    }
    LayerRow {
        body,
        eye,
        open,
        delete,
    }
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
        .text(at, Align2::LEFT_CENTER, name, theme::body(), color);
    resp
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
fn compass_point(degrees: f32) -> &'static str {
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

/// The distance from each end of a tick slider to the end of its scale.
pub const SLIDER_INSET: f32 = THUMB / 2.0 + 6.0;
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
    let field = form_row(ui, label);
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
        .font(theme::body())
        .text_color(color)
        .vertical_align(egui::Align::Center);
    let mut resp = ui.put(inner, edit);
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
        None => {}
    }
    resp
}

/// The names of the items of the edit menu of a field.
pub const COPY: &str = "Copy";
pub const PASTE: &str = "Paste";
/// The width of an item of the edit menu.
const EDIT_ITEM: f32 = 88.0;
/// The space between the edit menu and its field.
const EDIT_GAP: f32 = 6.0;

/// The item of the edit menu that the user tapped.
enum EditAction {
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
fn open_edit_menu(ctx: &egui::Context, field: egui::Id, rect: Rect) {
    let menu = EditMenu { field, rect };
    ctx.data_mut(|data| data.insert_temp(edit_menu_id(), menu));
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
    let ctx = ui.ctx();
    let menu: EditMenu = ctx.data(|data| data.get_temp(edit_menu_id()))?;
    if menu.field != field_id {
        return None;
    }
    let size = vec2(2.0 * EDIT_ITEM, TOUCH);
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
    let radius = theme::CONTROL_RADIUS.into();
    card(
        ctx,
        None,
        "pad-edit-menu",
        rect,
        egui::Order::Tooltip,
        radius,
        |ui| {
            // The menu is lighter than the card below it.
            let back = theme::card(false).to_opaque();
            let back = back.lerp_to_gamma(theme::TEXT, 0.12);
            ui.painter().rect_filled(rect, radius, back);
            let border = egui::Stroke::new(1.0, theme::MUTE);
            ui.painter()
                .rect_stroke(rect, radius, border, egui::StrokeKind::Inside);
            let x = rect.center().x;
            let divider = [pos2(x, rect.top() + 8.0), pos2(x, rect.bottom() - 8.0)];
            ui.painter().line_segment(divider, border);
            let items = [(COPY, true), (PASTE, can_paste)];
            for (i, (item, enabled)) in items.into_iter().enumerate() {
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
                    action = Some(match i {
                        0 => EditAction::Copy,
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
    /// The digits of a hex code.
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
            Keys::Hex => 6,
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
struct KeyEdit {
    field: egui::Id,
    name: String,
    keys: Keys,
    text: String,
    /// The text is the text of the field before the edit. The next key
    /// takes its place.
    fresh: bool,
    /// A key changed the text since the field read it.
    changed: bool,
}

fn keypad_id() -> egui::Id {
    egui::Id::new("pad-keypad")
}

fn key_edit(ctx: &egui::Context) -> Option<KeyEdit> {
    ctx.data(|data| data.get_temp(keypad_id()))
}

fn set_key_edit(ctx: &egui::Context, edit: Option<KeyEdit>) {
    ctx.data_mut(|data| match edit {
        Some(edit) => {
            data.insert_temp(keypad_id(), edit);
        }
        None => data.remove::<KeyEdit>(keypad_id()),
    });
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
    let ctx = ui.ctx().clone();
    let id = ui.id().with(name);
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
            }),
        };
        set_key_edit(&ctx, edit.clone());
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
        None => {}
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
        let size = vec2(
            text_width(ui, choice, theme::body()) + 2.0 * CHIP_PAD,
            TOUCH,
        );
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

/// A slider across a form, with ticks. Each tick has a place from 0 to 1
/// and a name below it. `active` is the tick at the place of the thumb.
/// Returns the new place of the thumb if the user moved it.
pub fn tick_slider(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    fraction: f64,
    ticks: &[(f64, &str)],
    active: Option<usize>,
) -> Option<f64> {
    let size = vec2(ui.available_width(), TOUCH + TICK_NAMES);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let bar = rect.with_max_y(rect.top() + TOUCH);
    let resp = hit(ui, controls, bar, name, Sense::click_and_drag(), true, None);
    let left = bar.left() + SLIDER_INSET;
    let right = bar.right() - SLIDER_INSET;
    let mut moved = None;
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
    {
        moved = Some(f64::from((pos.x - left) / (right - left)).clamp(0.0, 1.0));
    }
    let y = bar.center().y;
    let at = |fraction: f64| left + fraction as f32 * (right - left);
    for (place, _) in ticks {
        let mark = Rect::from_center_size(pos2(at(*place), y), vec2(2.0, 14.0));
        ui.painter()
            .rect_filled(mark, 1.0, theme::MUTE.gamma_multiply(0.5));
    }
    let x = at(fraction);
    let whole = Rect::from_min_max(pos2(left - 6.0, y - 3.0), pos2(right + 6.0, y + 3.0));
    let filled = Rect::from_min_max(whole.min, pos2(x, y + 3.0));
    track(ui, whole, filled, pos2(x, y));

    for (index, (place, tick)) in ticks.iter().enumerate() {
        let color = if active == Some(index) {
            theme::TEXT
        } else {
            theme::MUTE
        };
        let text = pos2(at(*place), bar.bottom() - 4.0);
        ui.painter()
            .text(text, Align2::CENTER_TOP, *tick, theme::small(), color);
    }
    moved
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

/// The color as the code of six hex digits.
pub fn hex_text(color: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

/// The color of a code of six hex digits, with or without a `#` before it.
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let text = text.trim().trim_start_matches('#');
    if text.len() != 6 || !text.is_ascii() {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&text[at..at + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// A color picker for a finger: a square of saturation and value, a strip
/// of hues, and a field for the hex code. `name` starts the names of its
/// controls. Returns true if the user changed the color.
pub fn color_picker(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    color: &mut [u8; 3],
) -> bool {
    use egui::ecolor::Hsva;
    // The hue stays while the color is grey, so it is kept between frames.
    let id = ui.id().with(name).with("hsva");
    let kept: Option<(Hsva, [u8; 3])> = ui.data(|data| data.get_temp(id));
    let mut hsva = match kept {
        Some((hsva, rgb)) if rgb == *color => hsva,
        _ => Hsva::from_srgb(*color),
    };
    let before = *color;
    let width = ui.available_width();
    let fill = |h: f32, s: f32, v: f32| Color32::from(Hsva::new(h, s, v, 1.0));

    // The keypad of a field takes the place of the square and the strip.
    if !keypad(ui, controls, PICKER_SQUARE + 8.0 + TOUCH) {
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

        let (strip, _) = ui.allocate_exact_size(vec2(width, TOUCH), Sense::hover());
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
            *color = hsva.to_srgb();
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
        |text| parse_hex(text).is_some(),
    );
    if let Some(typed) = typed.as_deref().and_then(parse_hex) {
        *color = typed;
        hsva = Hsva::from_srgb(typed);
    }
    ui.data_mut(|data| data.insert_temp(id, (hsva, *color)));
    *color != before
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
