//! The value fields of the desktop layout. A drag on a field changes its
//! value, a slider sets it, and the keyboard types it.

use std::ops::RangeInclusive;

use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::text_edit::TextEditState;
use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, Id, Key, Margin, PopupCloseBehavior, Rect, Response,
    Sense, SetOpenCommand, Stroke, pos2, vec2,
};

use super::float::{self, Glass};
use super::theme;
use crate::ui::pad::icons::{self, Icon};

/// The height of the box of a value.
const BOX_HEIGHT: f32 = 20.0;
/// The width of the box of a value in the options bar.
const BAR_BOX: f32 = 74.0;
/// The width of the box of a value in a row of a panel.
const ROW_BOX: f32 = 66.0;
/// The space between the edge of a box and its text.
const BOX_PAD: f32 = 7.0;
/// The width of the label column of a row.
pub const LABEL: f32 = 76.0;
/// The width of the column at the right of a row, for a switch or a lock.
const TRAILING: f32 = 26.0;
/// A drag of this many points moves a value across its whole scale.
const SCRUB: f64 = 300.0;
/// The width of the slider of the dropdown.
const DROPDOWN_SLIDER: f32 = 180.0;
const THUMB: f32 = 12.0;

/// The scale of a value, from `low` to `high`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Scale {
    pub low: f64,
    pub high: f64,
    /// The scale is logarithmic.
    pub log: bool,
}

impl Scale {
    pub fn linear(range: RangeInclusive<f64>) -> Scale {
        Scale {
            low: *range.start(),
            high: *range.end(),
            log: false,
        }
    }

    pub fn log(range: RangeInclusive<f64>) -> Scale {
        Scale {
            log: true,
            ..Scale::linear(range)
        }
    }

    /// The place of `value` on the scale, from 0 to 1.
    pub fn fraction(self, value: f64) -> f64 {
        let fraction = match self.log {
            true => (value / self.low).ln() / (self.high / self.low).ln(),
            false => (value - self.low) / (self.high - self.low),
        };
        if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The value at `fraction` of the scale.
    pub fn value(self, fraction: f64) -> f64 {
        let fraction = fraction.clamp(0.0, 1.0);
        match self.log {
            true => self.low * ((self.high / self.low).ln() * fraction).exp(),
            false => self.low + fraction * (self.high - self.low),
        }
    }
}

/// A number that the user can change.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: &'static str,
    pub value: f64,
    pub scale: Scale,
    /// The value as the field shows it, with its unit.
    pub text: String,
    /// The units that typed text can have: the symbol, and its size in the
    /// unit of `value`.
    pub units: Vec<(String, f64)>,
    /// The step between two values, or 0 for no step.
    pub step: f64,
    pub enabled: bool,
}

impl Field {
    /// The value on the scale and on a step.
    pub fn snap(&self, value: f64) -> f64 {
        let (low, high) = (self.scale.low, self.scale.high);
        let value = value.clamp(low, high);
        match self.step > 0.0 {
            true => ((value / self.step).round() * self.step).clamp(low, high),
            false => value,
        }
    }

    fn at(&self, fraction: f64) -> f64 {
        self.snap(self.scale.value(fraction))
    }

    /// The value of typed text. Text with no unit has the unit that the
    /// field shows.
    pub fn parse(&self, text: &str) -> Option<f64> {
        let text = text.trim().replace(',', "");
        let (number, unit) = split_number(&text);
        let number: f64 = number.parse().ok()?;
        let factor = match unit.is_empty() {
            false => self.factor(unit)?,
            true => {
                let shown = split_number(&self.text).1;
                self.factor(shown)
                    .or(self.units.first().map(|(_, f)| *f))
                    .unwrap_or(1.0)
            }
        };
        let value = number * factor;
        value.is_finite().then(|| self.snap(value))
    }

    fn factor(&self, unit: &str) -> Option<f64> {
        if self.units.is_empty() && unit.is_empty() {
            return Some(1.0);
        }
        let unit = self
            .units
            .iter()
            .find(|(u, _)| u.eq_ignore_ascii_case(unit));
        unit.map(|(_, factor)| *factor)
    }
}

/// The number at the start of `text`, and the rest of it.
fn split_number(text: &str) -> (&str, &str) {
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))
        .unwrap_or(text.len());
    (&text[..end], text[end..].trim())
}

/// Changes the value while the user drags `resp`. A drag to the right
/// makes the value larger.
fn scrub(ui: &egui::Ui, resp: &Response, field: &Field) -> Option<f64> {
    if !field.enabled {
        return None;
    }
    let start = resp.id.with("scrub");
    if resp.drag_started() {
        let fraction = field.scale.fraction(field.value);
        ui.data_mut(|d| d.insert_temp(start, fraction));
    }
    if !resp.dragged() {
        return None;
    }
    let fraction = ui.data(|d| d.get_temp::<f64>(start))?;
    let origin = ui.input(|i| i.pointer.press_origin())?;
    let pos = resp.interact_pointer_pos()?;
    let value = field.at(fraction + f64::from(pos.x - origin.x) / SCRUB);
    (value != field.value).then_some(value)
}

/// The text of a field while the user types in it, and true in the first
/// frame.
#[derive(Clone)]
struct Typing {
    text: String,
    fresh: bool,
}

/// Lets the user type a value in the box.
fn start_typing(ui: &egui::Ui, id: Id, field: &Field) {
    let typing = Typing {
        text: field.text.clone(),
        fresh: true,
    };
    ui.data_mut(|d| d.insert_temp(id, typing));
}

/// The box of a value: a well with the text. While the user types in it,
/// Return or a click outside it sets the value, and Escape keeps the old
/// value. `open`: the dropdown of the box is open.
fn value_box(
    ui: &mut egui::Ui,
    rect: Rect,
    id: Id,
    field: &Field,
    chevron: bool,
    open: bool,
) -> (Response, Option<f64>) {
    let sense = match field.enabled {
        true => Sense::click_and_drag(),
        false => Sense::hover(),
    };
    let resp = ui.interact(rect, id, sense);
    let shown = field.value;
    resp.widget_info(|| egui::WidgetInfo::slider(field.enabled, shown, field.name));
    let typing_id = id.with("typing");
    let typing = ui.data(|d| d.get_temp::<Typing>(typing_id));
    let edge = if typing.is_some() || open {
        theme::ACCENT
    } else {
        theme::line()
    };
    let fill = match field.enabled && resp.hovered() && typing.is_none() {
        true => Color32::from_black_alpha(90),
        false => theme::well(),
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        theme::CONTROL_RADIUS,
        fill,
        Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    if let Some(mut typing) = typing {
        let text_id = id.with("text");
        let edit = egui::TextEdit::singleline(&mut typing.text)
            .id(text_id)
            .frame(egui::Frame::NONE)
            .margin(Margin::symmetric(BOX_PAD as i8 - 1, 0))
            .font(theme::mono())
            .text_color(theme::TEXT)
            .vertical_align(Align::Center);
        let edit = ui.place(rect, edit);
        ui.ctx()
            .accesskit_node_builder(edit.id, |node| node.set_label(field.name));
        let fresh = typing.fresh;
        if fresh {
            edit.request_focus();
            let mut state = TextEditState::load(ui.ctx(), text_id).unwrap_or_default();
            let end = CCursor::new(typing.text.chars().count());
            let all = CCursorRange::two(CCursor::new(0), end);
            state.cursor.set_char_range(Some(all));
            state.store(ui.ctx(), text_id);
            typing.fresh = false;
        }
        if edit.lost_focus() || (!fresh && !edit.has_focus()) {
            ui.data_mut(|d| d.remove::<Typing>(typing_id));
            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
            let value = (!cancel).then(|| field.parse(&typing.text)).flatten();
            return (resp, value.filter(|v| *v != field.value));
        }
        ui.data_mut(|d| d.insert_temp(typing_id, typing));
        return (resp, None);
    }
    let color = match field.enabled {
        true => theme::TEXT,
        false => theme::MUTE.gamma_multiply(theme::OFF),
    };
    let right = if chevron { 16.0 } else { BOX_PAD };
    let clip = Rect::from_min_max(
        pos2(rect.left() + BOX_PAD, rect.top()),
        pos2(rect.right() - right, rect.bottom()),
    );
    let at = pos2(clip.left(), rect.center().y);
    ui.painter().with_clip_rect(clip).text(
        at,
        Align2::LEFT_CENTER,
        &field.text,
        theme::mono(),
        color,
    );
    if chevron {
        let icon =
            Rect::from_center_size(pos2(rect.right() - 10.0, rect.center().y), vec2(12.0, 12.0));
        icons::paint(ui.painter(), icon, Icon::Down, 10.0, theme::MUTE);
    }
    let resp = match field.enabled {
        true => resp.on_hover_cursor(CursorIcon::ResizeHorizontal),
        false => resp,
    };
    let value = scrub(ui, &resp, field);
    (resp, value)
}

/// The label of a field. A drag on it changes the value.
fn label(ui: &mut egui::Ui, rect: Rect, id: Id, field: &Field) -> Option<f64> {
    let color = match field.enabled {
        true => theme::MUTE,
        false => theme::MUTE.gamma_multiply(theme::OFF),
    };
    let at = pos2(rect.left(), rect.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, field.name, theme::label(), color);
    if !field.enabled {
        return None;
    }
    let resp = ui.interact(rect, id, Sense::drag());
    let resp = resp.on_hover_cursor(CursorIcon::ResizeHorizontal);
    scrub(ui, &resp, field)
}

/// A slider in `rect`. Returns the value at the pointer while the user
/// drags it.
fn track(ui: &mut egui::Ui, rect: Rect, id: Id, field: &Field) -> Option<f64> {
    let left = rect.left() + THUMB / 2.0;
    let width = rect.width() - THUMB;
    let sense = match field.enabled {
        true => Sense::click_and_drag(),
        false => Sense::hover(),
    };
    let resp = ui.interact(rect, id, sense);
    let shown = field.value;
    resp.widget_info(|| egui::WidgetInfo::slider(field.enabled, shown, field.name));
    let mut value = None;
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        let v = field.at(f64::from((pos.x - left) / width));
        value = (v != field.value).then_some(v);
    }
    let fraction = field.scale.fraction(value.unwrap_or(field.value)) as f32;
    let alpha = if field.enabled { 1.0 } else { theme::OFF };
    let painter = ui.painter();
    let y = rect.center().y;
    let x = left + fraction * width;
    let line = Rect::from_min_max(pos2(left, y - 1.5), pos2(left + width, y + 1.5));
    painter.rect_filled(line, 1.5, Color32::from_black_alpha(102));
    let filled = Rect::from_min_max(line.min, pos2(x, line.bottom()));
    let fill = match field.enabled {
        true => theme::ACCENT,
        false => theme::MUTE.gamma_multiply(alpha),
    };
    painter.rect_filled(filled, 1.5, fill);
    painter.circle_filled(
        pos2(x, y + 1.0),
        THUMB / 2.0 + 0.5,
        Color32::from_black_alpha(90),
    );
    let knob = Color32::from_rgb(233, 236, 239).gamma_multiply(alpha);
    painter.circle_filled(pos2(x, y), THUMB / 2.0, knob);
    value
}

/// A field of the options bar: a label and a box with the value. A drag on
/// either changes the value. A click on the box opens a slider below it,
/// and a double click lets the user type the value.
pub fn bar_field(ui: &mut egui::Ui, glass: Glass, field: &Field) -> Option<f64> {
    let id = ui.id().with(("bar-field", field.name));
    let galley = ui
        .painter()
        .layout_no_wrap(field.name.to_owned(), theme::label(), theme::MUTE);
    let label_width = galley.size().x.ceil();
    let size = vec2(label_width + 6.0 + BAR_BOX, theme::ROW);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let label_rect = Rect::from_min_size(rect.min, vec2(label_width + 6.0, rect.height()));
    let mut value = label(ui, label_rect, id.with("label"), field);
    let box_rect = Rect::from_min_size(
        pos2(label_rect.right(), rect.center().y - BOX_HEIGHT / 2.0),
        vec2(BAR_BOX, BOX_HEIGHT),
    );
    let popup_id = id.with("dropdown");
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    let (resp, typed) = value_box(ui, box_rect, id.with("box"), field, true, open);
    value = value.or(typed);
    let command = if resp.double_clicked() {
        start_typing(ui, id.with("box").with("typing"), field);
        Some(SetOpenCommand::Bool(false))
    } else {
        resp.clicked().then_some(SetOpenCommand::Toggle)
    };
    let dropdown = egui::Popup::from_response(&resp)
        .id(popup_id)
        .open_memory(command)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .gap(4.0)
        .frame(egui::Frame::NONE)
        .show(|ui| {
            let margin = Margin::symmetric(12, 6);
            float::material(ui, glass, float::RADIUS.into(), margin, |ui| {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(DROPDOWN_SLIDER, theme::ROW), Sense::hover());
                track(ui, rect, popup_id.with("slider"), field)
            })
            .inner
        });
    value.or(dropdown.and_then(|d| d.inner))
}

/// What the user did with a row of a panel.
pub struct Row {
    /// The new value.
    pub value: Option<f64>,
    /// The column at the right of the row, for a switch or a lock.
    pub trailing: Rect,
}

/// A row of a panel: a label, a slider, and the box with the value. A drag
/// on the label or the box changes the value, and a click on the box lets
/// the user type it.
pub fn row(ui: &mut egui::Ui, field: &Field) -> Row {
    let id = ui.id().with(("row-field", field.name));
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, theme::ROW), Sense::hover());
    let y = rect.center().y;
    let trailing = Rect::from_min_max(pos2(rect.right() - TRAILING, rect.top()), rect.max);
    let box_rect = Rect::from_min_size(
        pos2(trailing.left() - 6.0 - ROW_BOX, y - BOX_HEIGHT / 2.0),
        vec2(ROW_BOX, BOX_HEIGHT),
    );
    let label_rect = Rect::from_min_size(rect.min, vec2(LABEL, rect.height()));
    let track_rect = Rect::from_min_max(
        pos2(label_rect.right() - THUMB / 2.0, rect.top()),
        pos2(box_rect.left() - 4.0, rect.bottom()),
    );
    let mut value = label(ui, label_rect, id.with("label"), field);
    value = value.or(track(ui, track_rect, id.with("slider"), field));
    let box_id = id.with("box");
    let (resp, typed) = value_box(ui, box_rect, box_id, field, false, false);
    if resp.clicked() {
        start_typing(ui, box_id.with("typing"), field);
    }
    Row {
        value: value.or(typed),
        trailing,
    }
}

/// A switch in the middle of `rect`. Returns true if the user flipped it.
pub fn switch(ui: &mut egui::Ui, rect: Rect, name: &str, on: bool) -> bool {
    let rect = Rect::from_center_size(rect.center(), vec2(26.0, 15.0));
    let resp = ui.interact(rect, ui.id().with(("switch", name)), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, name));
    let painter = ui.painter();
    let fill = match on {
        true => theme::ACCENT,
        false if resp.hovered() => Color32::from_white_alpha(56),
        false => Color32::from_white_alpha(40),
    };
    painter.rect_filled(rect, 7.5, fill);
    let x = match on {
        true => rect.right() - 7.5,
        false => rect.left() + 7.5,
    };
    painter.circle_filled(pos2(x, rect.center().y), 5.5, Color32::WHITE);
    resp.clicked()
}

/// A small button with an icon in the middle of `rect`, that stays on.
/// Returns true if the user clicked it.
pub fn icon_toggle(
    ui: &mut egui::Ui,
    rect: Rect,
    name: &str,
    icon: Icon,
    tip: &str,
    on: bool,
) -> bool {
    let rect = Rect::from_center_size(rect.center(), vec2(22.0, 22.0));
    let resp = ui.interact(rect, ui.id().with(("toggle", name)), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name));
    let painter = ui.painter();
    let color = if on {
        painter.rect_filled(rect, theme::CONTROL_RADIUS, theme::selected());
        theme::ACCENT
    } else if resp.hovered() {
        painter.rect_filled(rect, theme::CONTROL_RADIUS, theme::hover());
        theme::TEXT
    } else {
        theme::MUTE
    };
    icons::paint(painter, rect, icon, 14.0, color);
    resp.on_hover_text(tip).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meters() -> Field {
        Field {
            name: "Strength",
            value: 1500.0,
            scale: Scale::log(50.0..=6000.0),
            text: "1500 m".to_owned(),
            units: vec![("m".into(), 1.0), ("km".into(), 1000.0)],
            step: 0.0,
            enabled: true,
        }
    }

    #[test]
    fn a_scale_gives_back_the_value_of_a_fraction() {
        for scale in [Scale::linear(-6000.0..=6000.0), Scale::log(50.0..=6000.0)] {
            for value in [50.0, 300.0, 1500.0, 6000.0] {
                let back = scale.value(scale.fraction(value));
                assert!((back - value).abs() < 1e-6, "{scale:?} {value}");
            }
        }
        assert_eq!(Scale::log(1.0..=100.0).fraction(10.0), 0.5);
    }

    #[test]
    fn typed_text_has_the_unit_of_the_field_or_its_own() {
        let field = meters();
        assert_eq!(field.parse("800"), Some(800.0));
        assert_eq!(field.parse(" 2 km"), Some(2000.0));
        assert_eq!(field.parse("1,200 m"), Some(1200.0));
        assert_eq!(field.parse("9000"), Some(6000.0));
        assert_eq!(field.parse("2 mi"), None);
        assert_eq!(field.parse("high"), None);
    }

    #[test]
    fn a_field_with_a_step_rounds_its_values() {
        let field = Field {
            step: 10.0,
            scale: Scale::linear(-6000.0..=6000.0),
            ..meters()
        };
        assert_eq!(field.parse("123"), Some(120.0));
        assert_eq!(field.snap(-7000.0), -6000.0);
    }
}
