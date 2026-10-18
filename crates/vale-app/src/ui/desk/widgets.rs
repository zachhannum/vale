//! The controls of the desktop layout.

use eframe::egui::{self, Align, Align2, Color32, Rect, Response, Sense, Stroke, pos2, vec2};

use super::float::ON_ACCENT;
use super::theme;
use crate::ui::pad::icons::{self, Icon};

/// The padding at each side of the text of a segment.
const SEGMENT_PAD: f32 = 12.0;
const SEGMENT_HEIGHT: f32 = 22.0;
/// The space between the edge of a group of segments and its segments.
const WELL_PAD: f32 = 2.0;

/// The text of a tooltip: the name, and the key in parentheses.
pub fn tip_text(name: &str, key: Option<&str>) -> String {
    match key {
        Some(key) => format!("{name} ({key})"),
        None => name.to_owned(),
    }
}

/// A square button with an icon and a tooltip. `selected`: the state of a
/// button that stays on, or `None` for a button that acts at once.
pub fn tool_button(
    ui: &mut egui::Ui,
    name: &str,
    icon: Icon,
    tip: &str,
    selected: Option<bool>,
    enabled: bool,
) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let size = vec2(theme::TOOL, theme::TOOL);
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    resp.widget_info(|| match selected {
        Some(on) => egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, name),
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name),
    });
    let on = selected == Some(true);
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, theme::TOOL_RADIUS, theme::selected());
    } else if enabled && resp.hovered() {
        painter.rect_filled(rect, theme::TOOL_RADIUS, theme::hover());
    }
    let color = if on {
        theme::ACCENT
    } else if enabled && resp.hovered() {
        theme::TEXT
    } else if enabled {
        theme::MUTE
    } else {
        theme::MUTE.gamma_multiply(theme::OFF)
    };
    icons::paint(painter, rect, icon, theme::ICON, color);
    resp.on_hover_text(tip)
}

/// One item of a group of segments.
pub struct Segment<'a> {
    pub label: &'a str,
    pub enabled: bool,
    /// The tooltip.
    pub tip: Option<&'a str>,
}

impl<'a> Segment<'a> {
    pub fn new(label: &'a str) -> Segment<'a> {
        Segment {
            label,
            enabled: true,
            tip: None,
        }
    }
}

/// A group of segments, of which one is on. Returns the item that the user
/// chose.
pub fn segments(ui: &mut egui::Ui, items: &[Segment], active: Option<usize>) -> Option<usize> {
    let font = theme::label();
    let widths: Vec<f32> = items
        .iter()
        .map(|item| {
            let galley =
                ui.painter()
                    .layout_no_wrap(item.label.to_owned(), font.clone(), theme::TEXT);
            galley.size().x.ceil() + 2.0 * SEGMENT_PAD
        })
        .collect();
    let size = vec2(
        widths.iter().sum::<f32>() + 2.0 * WELL_PAD,
        SEGMENT_HEIGHT + 2.0 * WELL_PAD,
    );
    let (well, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect(
        well,
        7,
        theme::well(),
        Stroke::new(1.0, theme::line()),
        egui::StrokeKind::Inside,
    );
    let mut left = well.left() + WELL_PAD;
    let mut chosen = None;
    for (index, (item, width)) in items.iter().zip(widths).enumerate() {
        let rect = Rect::from_min_size(
            pos2(left, well.top() + WELL_PAD),
            vec2(width, SEGMENT_HEIGHT),
        );
        left += width;
        let on = active == Some(index);
        let sense = if item.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let resp = ui.interact(rect, ui.id().with(("segment", item.label)), sense);
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, item.enabled, on, item.label)
        });
        let painter = ui.painter();
        if on {
            painter.rect(
                rect,
                theme::CONTROL_RADIUS,
                theme::selected(),
                Stroke::new(1.0, theme::selected_line()),
                egui::StrokeKind::Inside,
            );
        }
        let color = if on {
            theme::SELECTED_TEXT
        } else if !item.enabled {
            theme::MUTE.gamma_multiply(theme::OFF)
        } else if resp.hovered() {
            theme::TEXT
        } else {
            theme::MUTE
        };
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            item.label,
            font.clone(),
            color,
        );
        let resp = match item.tip {
            Some(tip) => resp.on_hover_text(tip),
            None => resp,
        };
        if resp.clicked() && !on {
            chosen = Some(index);
        }
    }
    chosen
}

/// A short vertical line between two groups of buttons.
pub fn divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 18.0), Sense::hover());
    let x = rect.center().x;
    let line = [pos2(x, rect.top()), pos2(x, rect.bottom())];
    ui.painter()
        .line_segment(line, Stroke::new(1.0, theme::divider()));
}

/// The width of the track of a slider in the options bar.
pub const SLIDER: f32 = 96.0;
/// The width of the value at the right of a slider.
pub const VALUE: f32 = 52.0;
const THUMB: f32 = 14.0;

/// A slider with its label at the left and its value at the right. `fraction`
/// is the value, from 0 to 1. `label_width` is the width of the label, or
/// `None` for the width of its text. Returns true if the user moved it.
pub fn slider(
    ui: &mut egui::Ui,
    name: &str,
    label_width: Option<f32>,
    width: f32,
    fraction: &mut f64,
    value: &str,
) -> bool {
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), theme::label(), theme::MUTE);
    let label_width_fixed = label_width.is_some();
    let label_width = label_width.unwrap_or(galley.size().x);
    let size = vec2(label_width + 8.0 + width + 12.0 + VALUE, theme::ROW);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let y = rect.center().y;
    let left = rect.left() + label_width + 8.0;
    let hit = Rect::from_min_max(
        pos2(left - THUMB / 2.0, rect.top()),
        pos2(left + width + THUMB / 2.0, rect.bottom()),
    );
    let resp = ui.interact(hit, ui.id().with(("slider", name)), Sense::click_and_drag());
    let before = *fraction;
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        *fraction = f64::from((pos.x - left) / width).clamp(0.0, 1.0);
    }
    let shown = *fraction;
    resp.widget_info(|| egui::WidgetInfo::slider(true, shown, name));
    let painter = ui.painter();
    painter.galley(
        pos2(rect.left(), y - galley.size().y / 2.0),
        galley,
        theme::MUTE,
    );
    let x = left + *fraction as f32 * width;
    let track = Rect::from_min_max(pos2(left, y - 2.0), pos2(left + width, y + 2.0));
    painter.rect_filled(track, 2.0, Color32::from_black_alpha(102));
    let filled = Rect::from_min_max(track.min, pos2(x, track.bottom()));
    painter.rect_filled(filled, 2.0, theme::ACCENT);
    let thumb = pos2(x, y);
    painter.circle_filled(
        thumb + vec2(0.0, 1.0),
        THUMB / 2.0 + 1.0,
        Color32::from_black_alpha(90),
    );
    painter.circle_filled(thumb, THUMB / 2.0, Color32::from_rgb(233, 236, 239));
    // A slider with a fixed label width is in a column of sliders, so its
    // value is at the right edge.
    let (at, align) = match label_width_fixed {
        true => (pos2(rect.right(), y), Align2::RIGHT_CENTER),
        false => (pos2(rect.right() - VALUE, y), Align2::LEFT_CENTER),
    };
    painter.text(at, align, value, theme::mono(), theme::MUTE);
    *fraction != before
}

/// A button with text. `selected`: the state of a button that stays on.
pub fn button(ui: &mut egui::Ui, label: &str, tip: &str, selected: Option<bool>) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::label(), theme::TEXT);
    let size = vec2(galley.size().x.ceil() + 22.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| match selected {
        Some(on) => egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label),
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label),
    });
    let fill = if selected == Some(true) {
        theme::selected()
    } else if resp.hovered() {
        Color32::from_white_alpha(38)
    } else {
        Color32::from_white_alpha(26)
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        theme::CONTROL_RADIUS,
        fill,
        Stroke::new(1.0, theme::line()),
        egui::StrokeKind::Inside,
    );
    let at = rect.center() - galley.size() / 2.0;
    painter.galley(at, galley, theme::TEXT);
    resp.on_hover_text(tip)
}

/// The height of a button and a field of a form.
pub const FIELD: f32 = 24.0;

/// A button with text of the accent color.
pub fn primary_button(
    ui: &mut egui::Ui,
    label: &str,
    tip: Option<&str>,
    enabled: bool,
) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::label(), ON_ACCENT);
    let size = vec2(galley.size().x.ceil() + 22.0, FIELD);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let fill = if !enabled {
        theme::ACCENT.gamma_multiply(theme::OFF)
    } else if resp.hovered() {
        Color32::from_rgb(95, 211, 185)
    } else {
        theme::ACCENT
    };
    ui.painter().rect_filled(rect, theme::CONTROL_RADIUS, fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, ON_ACCENT);
    match tip {
        Some(tip) => resp.on_hover_text(tip),
        None => resp,
    }
}

/// A button with text and no tooltip, that can be off.
pub fn plain_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> Response {
    let color = if enabled {
        theme::TEXT
    } else {
        theme::MUTE.gamma_multiply(theme::OFF)
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::label(), color);
    let size = vec2(galley.size().x.ceil() + 22.0, FIELD);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let fill = Color32::from_white_alpha(if enabled && resp.hovered() { 38 } else { 26 });
    ui.painter().rect(
        rect,
        theme::CONTROL_RADIUS,
        fill,
        Stroke::new(1.0, theme::line()),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

/// A button that shows a value and opens a menu of the values. A screen
/// reader gets the role of a combo box.
pub fn menu_button(ui: &mut egui::Ui, name: &str, value: &str) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(value.to_owned(), theme::label(), theme::TEXT);
    let size = vec2(galley.size().x.ceil() + 33.0, FIELD);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, name);
        info.current_text_value = Some(value.to_owned());
        info
    });
    let fill = Color32::from_white_alpha(if resp.hovered() { 36 } else { 23 });
    let painter = ui.painter();
    painter.rect(
        rect,
        theme::CONTROL_RADIUS,
        fill,
        Stroke::new(1.0, theme::line()),
        egui::StrokeKind::Inside,
    );
    let at = pos2(rect.left() + 9.0, rect.center().y - galley.size().y / 2.0);
    painter.galley(at, galley, theme::TEXT);
    // An arrow up and an arrow down.
    let x = rect.right() - 12.0;
    let y = rect.center().y;
    let stroke = Stroke::new(1.3, theme::MUTE);
    for dy in [-1.0, 1.0] {
        let tip = pos2(x, y + dy * 5.0);
        let base = y + dy * 1.5;
        let line = vec![pos2(x - 3.0, base), tip, pos2(x + 3.0, base)];
        painter.add(egui::Shape::line(line, stroke));
    }
    resp
}

/// A field of one line of text. `warn`: the text is not valid.
pub fn text_field(
    ui: &mut egui::Ui,
    id: egui::Id,
    name: &str,
    text: &mut String,
    width: f32,
    warn: bool,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(width, FIELD), Sense::hover());
    // The fill goes below the text of the field.
    let below = ui.painter().add(egui::Shape::Noop);
    let color = if warn { theme::WARN } else { theme::TEXT };
    let edit = egui::TextEdit::singleline(text)
        .id(id)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::symmetric(7, 0))
        .font(theme::label())
        .text_color(color)
        .vertical_align(Align::Center);
    let resp = ui.put(rect, edit);
    ui.ctx()
        .accesskit_node_builder(resp.id, |node| node.set_label(name));
    let edge = if resp.has_focus() {
        theme::ACCENT
    } else {
        theme::line()
    };
    let back = egui::Shape::rect_filled(rect, theme::CONTROL_RADIUS, theme::well());
    let edge = egui::Shape::rect_stroke(
        rect,
        theme::CONTROL_RADIUS,
        Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    ui.painter().set(below, egui::Shape::Vec(vec![back, edge]));
    resp
}

/// A tab of the dock. A screen reader gets the role of a tab.
pub fn tab(ui: &mut egui::Ui, label: &str, on: bool) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::label(), theme::TEXT);
    let size = vec2(galley.size().x.ceil() + 18.0, SEGMENT_HEIGHT);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
    ui.ctx().accesskit_node_builder(resp.id, |node| {
        node.set_role(egui::accesskit::Role::Tab);
    });
    let color = if on {
        ui.painter()
            .rect_filled(rect, theme::CONTROL_RADIUS, theme::selected());
        theme::SELECTED_TEXT
    } else if resp.hovered() {
        theme::TEXT
    } else {
        theme::MUTE
    };
    let at = rect.center() - galley.size() / 2.0;
    ui.painter().galley(at, galley, color);
    resp
}

/// The side of the eye button of a row.
const EYE: f32 = 22.0;

/// What the user did with a row of a list.
pub struct ListRow {
    pub body: Response,
    /// The eye button was clicked.
    pub eye: bool,
}

/// A row of a list: an icon, a name, and an eye button. `visible` is `None`
/// for a row with no eye. The eye of a hidden layer is crossed and dim.
pub fn list_row(
    ui: &mut egui::Ui,
    icon: Icon,
    name: &str,
    selected: bool,
    visible: Option<bool>,
    enabled: bool,
) -> ListRow {
    let width = ui.available_width();
    let (outer, _) = ui.allocate_exact_size(vec2(width, theme::ROW), Sense::hover());
    let rect = outer.shrink2(vec2(6.0, 0.0));
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let body = ui.interact(rect, ui.id().with(("row", name)), sense);
    body.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, name)
    });
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, theme::CONTROL_RADIUS, theme::selected());
    } else if enabled && body.hovered() {
        painter.rect_filled(rect, theme::CONTROL_RADIUS, theme::hover());
    }
    let alpha = if enabled { 1.0 } else { theme::OFF };
    let icon_rect =
        Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), vec2(16.0, 16.0));
    icons::paint(
        painter,
        icon_rect,
        icon,
        14.0,
        theme::MUTE.gamma_multiply(alpha),
    );
    let galley = painter.layout(
        name.to_owned(),
        theme::label(),
        theme::TEXT,
        rect.width() - 36.0 - EYE - 4.0,
    );
    let at = pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0);
    painter.galley(at, galley, theme::TEXT.gamma_multiply(alpha));
    let eye = visible.is_some_and(|shown| {
        let eye_rect = Rect::from_center_size(
            pos2(rect.right() - 4.0 - EYE / 2.0, rect.center().y),
            vec2(EYE, EYE),
        );
        let label = format!("{} {name}", if shown { "Hide" } else { "Show" });
        let resp = ui.interact(eye_rect, ui.id().with(("eye", name)), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        let painter = ui.painter();
        if resp.hovered() {
            painter.rect_filled(eye_rect, theme::CONTROL_RADIUS, theme::hover());
        }
        let (icon, color) = match shown {
            true => (Icon::Eye, theme::MUTE),
            false => (Icon::EyeOff, theme::MUTE.gamma_multiply(0.55)),
        };
        icons::paint(painter, eye_rect, icon, 14.0, color);
        resp.on_hover_text(&label).clicked()
    });
    ListRow { body, eye }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tip_has_the_key_in_parentheses() {
        assert_eq!(tip_text("Smooth", Some("S")), "Smooth (S)");
        assert_eq!(tip_text("Undo", Some("Ctrl+Z")), "Undo (Ctrl+Z)");
        assert_eq!(tip_text("More tools", None), "More tools");
    }
}
