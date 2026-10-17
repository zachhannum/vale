//! The controls of the desktop layout.

use eframe::egui::{self, Align2, Rect, Response, Sense, Stroke, pos2, vec2};

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
