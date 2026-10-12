//! The controls of the iPad layout. Each one is at least 44 points on each side.

use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, Response, Sense, pos2, vec2};

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
const ICON: f32 = 22.0;
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
/// stays on. A control that is not enabled takes no touch.
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
    let rect = rect.intersect(ui.clip_rect());
    if enabled && rect.is_positive() {
        controls.push(Control {
            name: name.to_owned(),
            rect,
        });
    }
    resp
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
) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(CELL.0, height), Sense::hover());
    let resp = hit(
        ui,
        controls,
        rect,
        name,
        Sense::click_and_drag(),
        true,
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
    track(ui, whole, filled, pos2(x, y));
    let painter = ui.painter();
    let at = pos2(x, rect.top() + 6.0);
    painter.text(at, Align2::CENTER_TOP, value, theme::small(), theme::MUTE);
    let at = pos2(x, rect.bottom() - 6.0);
    painter.text(at, Align2::CENTER_BOTTOM, name, theme::small(), theme::MUTE);
    *fraction != before
}

/// A slider in a row of a panel: the name, the track, and the value.
/// `label` is the name on screen. Returns true if the user moved it.
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
    let left = rect.left() + 72.0 + THUMB / 2.0;
    let right = rect.right() - 56.0 - THUMB / 2.0;
    let before = *fraction;
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
    {
        *fraction = f64::from((pos.x - left) / (right - left)).clamp(0.0, 1.0);
    }
    let y = rect.center().y;
    let x = left + *fraction as f32 * (right - left);
    let whole = Rect::from_min_max(pos2(left - 6.0, y - 3.0), pos2(right + 6.0, y + 3.0));
    let filled = Rect::from_min_max(whole.min, pos2(x, y + 3.0));
    track(ui, whole, filled, pos2(x, y));
    let painter = ui.painter();
    let at = pos2(rect.left(), y);
    painter.text(at, Align2::LEFT_CENTER, label, theme::body(), theme::TEXT);
    let at = pos2(rect.right(), y);
    painter.text(at, Align2::RIGHT_CENTER, value, theme::mono(), theme::MUTE);
    *fraction != before
}

/// What the user did on the title row of a panel.
pub struct TitleRow {
    pub close: bool,
    pub back: bool,
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
    let at = pos2(text_left, rect.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, title, theme::title(), theme::TEXT);
    TitleRow { close, back, bar }
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
    let resp = ui.put(inner, edit);
    controls.push(Control {
        name: name.to_owned(),
        rect: field,
    });
    resp
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

/// A small text below the control of a row of a form, in the warning color.
pub fn form_note(ui: &mut egui::Ui, text: &str, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let place = rect.with_min_x(rect.left() + FORM_LABEL + HIGHLIGHT_INSET);
    let text = egui::RichText::new(text)
        .font(theme::small())
        .color(theme::WARN);
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(place).layout(layout),
        |ui| {
            ui.add(egui::Label::new(text).selectable(false));
        },
    );
}

/// A row of a panel that is one button with its text in the middle.
pub fn button_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    enabled: bool,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let resp = hit(ui, controls, rect, name, Sense::click(), enabled, None);
    let back = rect.shrink(HIGHLIGHT_INSET);
    let color = if resp.is_pointer_button_down_on() {
        theme::ACCENT
    } else {
        theme::ACCENT.gamma_multiply(0.8)
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
