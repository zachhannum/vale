//! The number controls of the iPad layout: a slider and a stepper. A drag
//! on a slider moves the value by the distance of the drag, and a drag away
//! from the track moves it more slowly. A stepper has a minus button and a
//! plus button. A tap on the value of either control opens the keypad.

use eframe::egui::{self, Align2, Color32, Order, Pos2, Rect, Sense, pos2, vec2};

use super::geometry::{CELL, PAD, TOUCH};
use super::theme;
use super::widgets::{self, Controls, KEYPAD, KeyEdit, Keys, PANEL_ROW, TICK_NAMES, fade, hit};
use crate::globe::backdrop::{card_shape, marker};

/// The height of a track, of a value box, and of a stepper.
const HEIGHT: f32 = 36.0;
/// The width of a vertical track.
const COLUMN: f32 = 28.0;
/// The width of the handle, and its space from the edges of the track.
const HANDLE: f32 = 10.0;
const HANDLE_INSET: f32 = 4.0;
/// The space from each end of a track to the place of the handle at that
/// end.
pub const TRACK_INSET: f32 = HANDLE_INSET + HANDLE / 2.0;
/// The space between a panel and the card of the keypad or the bubble.
const GAP: f32 = 12.0;
/// The smallest width of the bubble.
const BUBBLE_WIDTH: f32 = 112.0;
/// The smallest width of the label of a slider row, so that the tracks of
/// the rows line up.
pub const LABEL_COLUMN: f32 = 96.0;
/// The width of the value field at the right of a row.
const VALUE_WIDTH: f32 = 84.0;
/// The distance from the track at which each slower speed starts, and the
/// speed.
const SPEEDS: [(f32, f64, &str); 3] = [
    (60.0, 0.5, "Half speed"),
    (120.0, 0.25, "Quarter speed"),
    (180.0, 0.125, "Fine"),
];
/// The distance along the track, in points at full speed, in which the
/// value stays at the default.
const DETENT: f32 = 5.0;

/// How the place on the track follows the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    Linear,
    /// Each part of the track multiplies the value by the same factor.
    Log,
    /// The value grows with the square of the place, so the small values
    /// get more of the track.
    Square,
}

/// The state of a drag on a slider.
#[derive(Clone, Copy, Default)]
struct Drag {
    /// The place on the track, from 0 to 1, before the step and the detent.
    place: f64,
    /// The name of the speed, if it is slower than full speed.
    note: Option<&'static str>,
}

/// A number with its range, its scale, its step, and its unit. `slider`
/// adds it as a slider in a row of a panel, `vertical` as a slider that you
/// move up and down, and `stepper` as a stepper in a row of a panel.
pub struct Number<'a> {
    name: &'a str,
    label: &'a str,
    value: f64,
    range: (f64, f64),
    scale: Scale,
    step: f64,
    factor: f64,
    unit: &'a str,
    text: Option<Box<dyn Fn(f64) -> String + 'a>>,
    default: Option<f64>,
    enabled: bool,
    preview: Option<Box<dyn Fn(f64) -> f32 + 'a>>,
    steps: Steps<'a>,
    ticks: &'a [(f64, &'a str)],
    field: bool,
    keypad: bool,
    mixed: bool,
}

impl<'a> Number<'a> {
    /// A slider for `value` in `range`. `name` names the control, and it is
    /// also the label until `label` sets one.
    pub fn new(name: &'a str, value: f64, range: std::ops::RangeInclusive<f64>) -> Self {
        Self {
            name,
            label: name,
            value,
            range: (*range.start(), *range.end()),
            scale: Scale::Linear,
            step: 0.0,
            factor: 1.0,
            unit: "",
            text: None,
            default: None,
            enabled: true,
            preview: None,
            steps: Steps::Step,
            ticks: &[],
            field: true,
            keypad: true,
            mixed: false,
        }
    }

    /// The name on screen.
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = label;
        self
    }

    pub fn scale(mut self, scale: Scale) -> Self {
        self.scale = scale;
        self
    }

    pub fn log(self) -> Self {
        self.scale(Scale::Log)
    }

    /// The value is a whole number of steps.
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// The value on screen is the value times `factor`, with `unit` after
    /// it. The keypad takes the value on screen.
    pub fn shown(mut self, factor: f64, unit: &'a str) -> Self {
        self.factor = factor;
        self.unit = unit;
        self
    }

    pub fn unit(self, unit: &'a str) -> Self {
        let factor = self.factor;
        self.shown(factor, unit)
    }

    /// The value is different in the selected items. The value shows
    /// "Mixed", and a value that the user sets is a change also when it is
    /// equal to `value`.
    pub fn mixed(mut self, mixed: bool) -> Self {
        self.mixed = mixed;
        self
    }

    /// The buttons of a stepper go through 1, 2, 5, 10, 20, and so on.
    pub fn nice(mut self) -> Self {
        self.steps = Steps::Nice;
        self
    }

    /// The value is one of `choices`, in order. A typed value takes the
    /// nearest choice.
    pub fn choices(mut self, choices: &'a [f64]) -> Self {
        self.steps = Steps::Choices(choices);
        self
    }

    /// Named values below the track of a slider. The thumb stops at each
    /// one, as at the default.
    pub fn ticks(mut self, ticks: &'a [(f64, &'a str)]) -> Self {
        self.ticks = ticks;
        self
    }

    /// The slider has no value field, because another field of the form
    /// holds the value.
    pub fn no_field(mut self) -> Self {
        self.field = false;
        self
    }

    /// Another control shows the keypad, for example the color picker of
    /// a card.
    pub fn keypad_elsewhere(mut self) -> Self {
        self.keypad = false;
        self
    }

    /// A value from 0 to 1, shown in whole percents.
    pub fn percent(self) -> Self {
        self.shown(100.0, "%").step(0.01)
    }

    /// The text of the value, in place of the number and the unit.
    pub fn text(mut self, text: impl Fn(f64) -> String + 'a) -> Self {
        self.text = Some(Box::new(text));
        self
    }

    /// The value at which the thumb stops during a drag.
    pub fn default(mut self, default: f64) -> Self {
        self.default = Some(default);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// During a drag, a circle at the center of the screen with the radius
    /// in points that this gives for the value. The brush size shows the
    /// brush with it.
    pub fn preview(mut self, radius: impl Fn(f64) -> f32 + 'a) -> Self {
        self.preview = Some(Box::new(radius));
        self
    }

    /// The place on the track of a value, from 0 to 1.
    pub fn place(&self, value: f64) -> f64 {
        let (lo, hi) = self.range;
        let place = match self.scale {
            Scale::Linear => (value - lo) / (hi - lo),
            Scale::Log => (value / lo).ln() / (hi / lo).ln(),
            Scale::Square => ((value - lo) / (hi - lo)).max(0.0).sqrt(),
        };
        if place.is_finite() {
            place.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The value at a place on the track, before the step.
    fn at(&self, place: f64) -> f64 {
        let (lo, hi) = self.range;
        let place = place.clamp(0.0, 1.0);
        match self.scale {
            Scale::Linear => lo + place * (hi - lo),
            Scale::Log => lo * (hi / lo).powf(place),
            Scale::Square => lo + place * place * (hi - lo),
        }
    }

    /// A value in the range, as a whole number of steps.
    pub fn clean(&self, value: f64) -> f64 {
        if let Steps::Choices(choices) = self.steps {
            let near = |c: &&f64| ((*c - value).abs() * 1e9) as u64;
            return choices.iter().min_by_key(near).copied().unwrap_or(value);
        }
        let (lo, hi) = self.range;
        let value = if self.step > 0.0 {
            let value = (value / self.step).round() * self.step;
            // A step with exact decimals, such as 0.1, gives no float noise.
            match exact_decimals(self.step) {
                Some(d) => {
                    let scale = 10f64.powi(d as i32);
                    (value * scale).round() / scale
                }
                None => value,
            }
        } else {
            value
        };
        value.clamp(lo.min(hi), hi.max(lo))
    }

    /// The decimals of the value on screen.
    fn decimals(&self, shown: f64) -> usize {
        let step = self.step * self.factor;
        if step > 0.0 {
            exact_decimals(step).unwrap_or(4)
        } else {
            // Three significant digits.
            let digits = shown.abs().max(1e-9).log10().floor() as i32;
            (2 - digits).clamp(0, 6) as usize
        }
    }

    /// The number on screen, with no unit, as the keypad shows it.
    pub fn number(&self, value: f64) -> String {
        let shown = value * self.factor;
        let text = format!("{shown:.*}", self.decimals(shown));
        let text = if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text
        };
        // A value near zero has no sign.
        match text.strip_prefix('-') {
            Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_owned(),
            _ => text,
        }
    }

    /// The value on screen with its unit.
    pub fn value_text(&self, value: f64) -> String {
        if self.mixed {
            return crate::ui::select::MIXED.to_owned();
        }
        if let Some(text) = &self.text {
            return text(value);
        }
        let number = self.number(value);
        match self.unit {
            "" => number,
            "%" | "×" | "°" => format!("{number}{}", self.unit),
            unit => format!("{number} {unit}"),
        }
    }

    /// The value of a typed text, or `None` if it is not a number.
    pub fn parse(&self, text: &str) -> Option<f64> {
        let clean: String = text
            .trim()
            .chars()
            .filter(|c| *c != ',' && *c != ' ')
            .map(|c| if c == '\u{2212}' { '-' } else { c })
            .collect();
        let shown: f64 = clean.parse().ok().filter(|v: &f64| v.is_finite())?;
        Some(self.clean(shown / self.factor))
    }

    /// Adds the slider as a row of a panel: the label, the track, and the
    /// value. Returns the new value when the user changes it. A drag that
    /// starts up or down scrolls the panel.
    pub fn slider(self, ui: &mut egui::Ui, controls: &mut Controls) -> Option<f64> {
        let enabled = self.enabled;
        let names = if self.ticks.is_empty() {
            0.0
        } else {
            TICK_NAMES
        };
        let size = vec2(ui.available_width(), PANEL_ROW + names);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let row = rect.with_max_y(rect.top() + PANEL_ROW);
        let y = row.center().y;
        let text = fade(theme::TEXT, enabled);
        let label = ui
            .painter()
            .layout_no_wrap(self.label.to_owned(), theme::body(), text);
        let field = if self.field {
            Rect::from_min_max(
                pos2(row.right() - VALUE_WIDTH, y - TOUCH / 2.0),
                pos2(row.right(), y + TOUCH / 2.0),
            )
        } else {
            Rect::from_min_max(pos2(row.right(), y), pos2(row.right(), y))
        };
        let body = row.with_max_x(field.left());
        // The slider senses clicks, and takes a drag along its track. A
        // drag up or down scrolls the panel.
        let resp = hit(ui, controls, body, self.name, Sense::click(), enabled, None);
        let start = if self.label.is_empty() {
            row.left()
        } else {
            row.left() + (label.size().x + 16.0).max(LABEL_COLUMN)
        };
        let end = if self.field {
            field.left() - 8.0
        } else {
            row.right()
        };
        let track = Rect::from_min_max(pos2(start, y - HEIGHT / 2.0), pos2(end, y + HEIGHT / 2.0));
        let left = track.left() + TRACK_INSET;
        let right = track.right() - TRACK_INSET;
        let mut out = self.drag(ui, &resp, true, right - left, |p| (p.x, (p.y - y).abs()));
        let value = out.unwrap_or(self.value);

        let x_of = |v: f64| left + self.place(v) as f32 * (right - left);
        let marks: Vec<f32> = (self.default.iter())
            .chain(self.ticks.iter().map(|(v, _)| v))
            .map(|v| x_of(*v))
            .collect();
        let held = resp.dragged() || resp.is_pointer_button_down_on();
        paint_track(ui, track, x_of(value), &marks, false, enabled, held);
        let painter = ui.painter();
        let at = pos2(row.left(), y - label.size().y / 2.0);
        painter.galley(at, label, text);
        for (tick, name) in self.ticks {
            let active = (tick - value).abs() <= tick.abs() * 1e-9;
            let color = fade(if active { theme::TEXT } else { theme::MUTE }, enabled);
            let at = pos2(x_of(*tick), row.bottom() - 4.0);
            painter.text(at, Align2::CENTER_TOP, *name, theme::small(), color);
        }

        if self.field {
            let back = field.shrink2(vec2(0.0, (TOUCH - HEIGHT) / 2.0));
            let typed = self.field(ui, controls, field, back, value, Look::Box);
            out = typed.or(out);
            self.keypad_beside(ui, controls, row);
        }
        if let Some(drag) = dragging(ui, &resp) {
            self.bubble(ui, value, drag.note, row);
        }
        out.filter(|v| *v != self.value)
    }

    /// Adds the slider as a column `height` high, with the value at the top
    /// and the label at the bottom. Returns the new value when the user
    /// changes it. The keypad opens at the side of the card.
    pub fn vertical(self, ui: &mut egui::Ui, controls: &mut Controls, height: f32) -> Option<f64> {
        let enabled = self.enabled;
        let width = ui.available_width().min(CELL.0);
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let resp = hit(
            ui,
            controls,
            rect,
            self.name,
            Sense::click_and_drag(),
            enabled,
            None,
        );
        let x = rect.center().x;
        let track = Rect::from_min_max(
            pos2(x - COLUMN / 2.0, rect.top() + 22.0),
            pos2(x + COLUMN / 2.0, rect.bottom() - 22.0),
        );
        let top = track.top() + TRACK_INSET;
        let bottom = track.bottom() - TRACK_INSET;
        let mut out = self.drag(ui, &resp, false, bottom - top, |p| (-p.y, (p.x - x).abs()));
        let value = out.unwrap_or(self.value);

        let y_of = |v: f64| bottom - self.place(v) as f32 * (bottom - top);
        let marks: Vec<f32> = self.default.iter().map(|d| y_of(*d)).collect();
        let held = resp.dragged() || resp.is_pointer_button_down_on();
        paint_track(ui, track, y_of(value), &marks, true, enabled, held);
        let color = fade(theme::MUTE, enabled);
        let at = pos2(x, rect.bottom() - 4.0);
        let painter = ui.painter();
        painter.text(at, Align2::CENTER_BOTTOM, self.label, theme::small(), color);

        // The field covers the value at the top. It takes the touches there
        // from the track.
        let field = Rect::from_min_size(rect.min, vec2(rect.width(), TOUCH));
        let back = Rect::from_min_size(rect.min, vec2(rect.width(), 20.0));
        let typed = self.field(ui, controls, field, back, value, Look::Plain);
        out = typed.or(out);
        self.keypad_beside(ui, controls, rect);
        if let Some(drag) = dragging(ui, &resp) {
            self.bubble(ui, value, drag.note, rect);
        }
        out.filter(|v| *v != self.value)
    }

    /// Adds the slider as a row `width` wide, with the label and the value
    /// above the track. Returns the new value when the user changes it. The
    /// keypad opens above the card.
    pub fn horizontal(self, ui: &mut egui::Ui, controls: &mut Controls, width: f32) -> Option<f64> {
        let enabled = self.enabled;
        let (rect, _) = ui.allocate_exact_size(vec2(width, CELL.1), Sense::hover());
        let rect = rect.shrink2(vec2(PAD, 0.0));
        let resp = hit(
            ui,
            controls,
            rect,
            self.name,
            Sense::click_and_drag(),
            enabled,
            None,
        );
        let track = Rect::from_min_max(
            pos2(rect.left(), rect.top() + 22.0),
            pos2(rect.right(), rect.bottom() - 4.0),
        );
        let y = track.center().y;
        let left = track.left() + TRACK_INSET;
        let right = track.right() - TRACK_INSET;
        let mut out = self.drag(ui, &resp, false, right - left, |p| (p.x, (p.y - y).abs()));
        let value = out.unwrap_or(self.value);

        let x_of = |v: f64| left + self.place(v) as f32 * (right - left);
        let marks: Vec<f32> = self.default.iter().map(|d| x_of(*d)).collect();
        let held = resp.dragged() || resp.is_pointer_button_down_on();
        paint_track(ui, track, x_of(value), &marks, false, enabled, held);
        let color = fade(theme::MUTE, enabled);
        let at = pos2(rect.left() + 2.0, rect.top() + 2.0);
        let painter = ui.painter();
        painter.text(at, Align2::LEFT_TOP, self.label, theme::small(), color);

        // The field covers the value at the right. It takes the touches
        // there from the track.
        let size = vec2((rect.width() / 2.0).clamp(TOUCH, VALUE_WIDTH), TOUCH);
        let field = Rect::from_min_size(pos2(rect.right() - size.x, rect.top()), size);
        let back = field.with_max_y(field.top() + 20.0);
        let typed = self.field(ui, controls, field, back, value, Look::Plain);
        out = typed.or(out);
        self.keypad_beside(ui, controls, rect);
        if let Some(drag) = dragging(ui, &resp) {
            self.bubble(ui, value, drag.note, rect);
        }
        out.filter(|v| *v != self.value)
    }

    /// Adds the number as a stepper in a row of a panel: the label, and one
    /// control with the minus button, the value, and the plus button. A
    /// button that you hold repeats. Returns the new value when the user
    /// changes it.
    pub fn stepper(self, ui: &mut egui::Ui, controls: &mut Controls) -> Option<f64> {
        let enabled = self.enabled;
        let size = vec2(ui.available_width(), PANEL_ROW);
        let (row, _) = ui.allocate_exact_size(size, Sense::hover());
        let y = row.center().y;
        let text = fade(theme::TEXT, enabled);
        let at = pos2(row.left(), y);
        let painter = ui.painter();
        painter.text(at, Align2::LEFT_CENTER, self.label, theme::body(), text);
        let whole = Rect::from_min_max(
            pos2(row.right() - 2.0 * TOUCH - VALUE_WIDTH, y - HEIGHT / 2.0),
            pos2(row.right(), y + HEIGHT / 2.0),
        );
        let radius = theme::CONTROL_RADIUS;
        painter.rect_filled(whole, radius, fade(theme::raise(), enabled));
        let touch = |left: f32, width: f32| {
            Rect::from_min_size(pos2(left, y - TOUCH / 2.0), vec2(width, TOUCH))
        };
        let minus = touch(whole.left(), TOUCH);
        let field = touch(minus.right(), VALUE_WIDTH);
        let plus = touch(field.right(), TOUCH);
        for x in [field.left(), field.right()] {
            let line = Rect::from_min_max(
                pos2(x - 0.5, whole.top() + 8.0),
                pos2(x + 0.5, whole.bottom() - 8.0),
            );
            painter.rect_filled(line, 0.0, theme::divider());
        }
        let down = self.step_from(self.value, false);
        let up = self.step_from(self.value, true);
        let mut out = None;
        let r = radius as u8;
        let corners = [
            egui::CornerRadius {
                nw: r,
                sw: r,
                ne: 0,
                se: 0,
            },
            egui::CornerRadius {
                nw: 0,
                sw: 0,
                ne: r,
                se: r,
            },
        ];
        for ((rect, name, next, sign), corner) in [
            (minus, self.minus_name(), down, false),
            (plus, self.plus_name(), up, true),
        ]
        .into_iter()
        .zip(corners)
        {
            let on = enabled && next.is_some();
            let resp = hit(ui, controls, rect, &name, Sense::click(), on, None);
            if on && (resp.clicked() || repeats(ui, &resp)) {
                out = next;
            }
            let back = rect.intersect(whole);
            let painter = ui.painter();
            if resp.is_pointer_button_down_on() {
                painter.rect_filled(back, corner, theme::raise());
            }
            let color = fade(theme::TEXT, on);
            let c = back.center();
            let stroke = egui::Stroke::new(2.0, color);
            painter.line_segment([c - vec2(6.0, 0.0), c + vec2(6.0, 0.0)], stroke);
            if sign {
                painter.line_segment([c - vec2(0.0, 6.0), c + vec2(0.0, 6.0)], stroke);
            }
        }
        let value = out.unwrap_or(self.value);
        let back = field.intersect(whole);
        let typed = self.field(ui, controls, field, back, value, Look::Segment);
        out = typed.or(out);
        self.keypad_beside(ui, controls, row);
        out.filter(|v| self.mixed || *v != self.value)
    }

    /// The name of the minus button of a stepper.
    pub fn minus_name(&self) -> String {
        format!("Decrease {}", self.name)
    }

    /// The name of the plus button of a stepper.
    pub fn plus_name(&self) -> String {
        format!("Increase {}", self.name)
    }

    /// The value one step up or down from `value`, or `None` at the end of
    /// the range.
    pub fn step_from(&self, value: f64, up: bool) -> Option<f64> {
        let (lo, hi) = (
            self.range.0.min(self.range.1),
            self.range.0.max(self.range.1),
        );
        let eps = (hi - lo).abs() * 1e-9;
        let next = match self.steps {
            Steps::Choices(choices) => {
                let mut it = choices.iter().copied();
                if up {
                    it.find(|c| *c > value + eps)
                } else {
                    it.rev().find(|c| *c < value - eps)
                }
            }
            Steps::Nice => Some(nice_step(value, up)).map(|v| self.clean(v)),
            Steps::Step => {
                let step = if self.step > 0.0 {
                    self.step
                } else {
                    (hi - lo) / 100.0
                };
                let n = (value / step).round();
                let n = if up { n + 1.0 } else { n - 1.0 };
                Some(self.clean(n * step))
            }
        }?;
        let moved = if up {
            next > value + eps
        } else {
            next < value - eps
        };
        moved.then_some(next)
    }

    /// Moves the value with a drag on `resp`. If `scrolls`, `resp` senses
    /// clicks, and it takes only a drag that is more along the track than
    /// across it, so another drag scrolls the panel. `length` is the
    /// length of the track. `along` gives, for a place on the screen, its place along the
    /// track and its distance from the track. Returns the value while the
    /// drag moves it.
    fn drag(
        &self,
        ui: &egui::Ui,
        resp: &egui::Response,
        scrolls: bool,
        length: f32,
        along: impl Fn(Pos2) -> (f32, f32),
    ) -> Option<f64> {
        let id = resp.id.with("drag");
        let origin = ui.input(|i| i.pointer.press_origin());
        let started = if scrolls {
            widgets::take_drag(ui, resp, |moved| {
                let from = origin.unwrap_or_default();
                let ((a, b), (c, d)) = (along(from), along(from + moved));
                (c - a).abs() > (d - b).abs()
            })
        } else {
            resp.drag_started()
        };
        if started {
            let drag = Drag {
                place: self.place(self.value),
                note: None,
            };
            ui.data_mut(|data| data.insert_temp(id, drag));
        }
        let drag: Drag = ui.data(|data| data.get_temp(id))?;
        // In the frame that the slider takes the drag, its response is
        // not dragged yet.
        if !resp.dragged() && !started {
            if resp.drag_stopped() {
                ui.data_mut(|data| data.remove::<Drag>(id));
            }
            return None;
        }
        let pos = (resp.interact_pointer_pos()).or_else(|| ui.input(|i| i.pointer.latest_pos()))?;
        let (now, away) = along(pos);
        // The first frame of a drag counts the move from the touch, so the
        // move before the drag starts is not lost.
        let last = match origin {
            Some(origin) if started => origin,
            _ => pos - resp.drag_delta(),
        };
        let (before, _) = along(last);
        let (speed, note) = speed(away);
        let place = (drag.place + f64::from((now - before) / length) * speed).clamp(0.0, 1.0);
        let drag = Drag { place, note };
        ui.data_mut(|data| data.insert_temp(id, drag));
        let near = |d: f64| f64::from(length) * (place - self.place(d)).abs() <= f64::from(DETENT);
        let stops = self.default.iter().chain(self.ticks.iter().map(|(v, _)| v));
        let stop = stops.copied().find(|v| near(*v));
        Some(stop.unwrap_or_else(|| self.clean(self.at(place))))
    }

    fn field_name(&self) -> String {
        format!("{} value", self.name)
    }

    /// The value field: a tap opens the keypad, and a second tap or Done
    /// closes it. The typed value takes effect when the keypad closes.
    /// `rect` takes the touches, and `back` is the box of the value.
    fn field(
        &self,
        ui: &mut egui::Ui,
        controls: &mut Controls,
        rect: Rect,
        back: Rect,
        value: f64,
        look: Look,
    ) -> Option<f64> {
        let ctx = ui.ctx().clone();
        let name = self.field_name();
        let id = ui.id().with(&name);
        let resp = hit(
            ui,
            controls,
            rect,
            &name,
            Sense::click(),
            self.enabled,
            None,
        );
        let mut edit = widgets::key_edit(&ctx).filter(|edit| edit.field == id);
        if resp.clicked() {
            edit = match edit {
                Some(_) => None,
                None => Some(KeyEdit {
                    field: id,
                    name: name.clone(),
                    keys: Keys::Number,
                    text: if self.mixed {
                        String::new()
                    } else {
                        self.number(value)
                    },
                    fresh: true,
                    changed: false,
                    placed: self.keypad,
                }),
            };
            widgets::set_key_edit(&ctx, edit.clone());
        }
        if edit.is_some() {
            widgets::set_keypad_field_rect(&ctx, rect);
        }
        // The text of the open keypad, kept for the frame in which it
        // closes.
        let typed_id = id.with("typed");
        let mut out = None;
        match &edit {
            Some(edit) if !edit.fresh => {
                ui.data_mut(|data| data.insert_temp(typed_id, edit.text.clone()));
            }
            Some(_) => {}
            None => {
                let typed = ui.data_mut(|data| data.remove_temp::<String>(typed_id));
                out = typed.and_then(|text| self.parse(&text));
            }
        }

        let painter = ui.painter();
        let pressed = edit.is_some() || resp.is_pointer_button_down_on();
        let radius = theme::CONTROL_RADIUS;
        match look {
            Look::Box => {
                let fill = if pressed {
                    theme::raise().gamma_multiply(1.6)
                } else {
                    theme::raise()
                };
                painter.rect_filled(back, radius, fade(fill, self.enabled));
            }
            Look::Segment if pressed => {
                painter.rect_filled(back, 0.0, theme::raise());
            }
            _ => {}
        }
        if edit.is_some() {
            let stroke = egui::Stroke::new(1.5, theme::ACCENT);
            let kind = egui::StrokeKind::Inside;
            let r = if look == Look::Segment { 0.0 } else { radius };
            painter.rect_stroke(back, r, stroke, kind);
        }
        let (font, color) = match look {
            Look::Plain => (theme::small(), fade(theme::MUTE, self.enabled)),
            _ => (theme::mono(), fade(theme::TEXT, self.enabled)),
        };
        let text = match &edit {
            Some(edit) => {
                let suffix = match self.unit {
                    "" => String::new(),
                    unit => format!(" {unit}"),
                };
                format!("{}{suffix}", edit.text)
            }
            None => self.value_text(value),
        };
        let galley = painter.layout_no_wrap(text, font, color);
        let at = back.center() - galley.size() / 2.0;
        if let Some(edit) = &edit
            && edit.fresh
        {
            // The text that the next key replaces is selected.
            let shape = Rect::from_min_size(at, galley.size()).expand(2.0);
            painter.rect_filled(shape, 3.0, theme::ACCENT.gamma_multiply(0.45));
        }
        painter.galley(at, galley, color);
        out
    }

    /// The keypad of the field, while it is open, in a card next to the
    /// panel or the card that holds `anchor`, at the height of `anchor`.
    fn keypad_beside(&self, ui: &egui::Ui, controls: &mut Controls, anchor: Rect) {
        let open = widgets::keypad_field(ui.ctx()).as_deref() == Some(&self.field_name());
        if !self.keypad || !open {
            return;
        }
        let ctx = ui.ctx();
        let size = vec2(4.0 * TOUCH * 1.25, KEYPAD) + vec2(2.0 * PAD, 2.0 * PAD);
        let id = format!("{} keypad", self.name);
        let own = egui::LayerId::new(Order::Foreground, egui::Id::new(&id));
        let card = place(
            ctx,
            anchor,
            ui.clip_rect(),
            size,
            anchor.top() - PAD,
            Some(own),
        );
        let backdrop = widgets::backdrop(ctx);
        let radius = theme::CARD_RADIUS.into();
        widgets::card(ctx, backdrop, &id, card, Order::Foreground, radius, |ui| {
            let inner = ui.max_rect().shrink(PAD);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            widgets::keypad(&mut ui, controls, KEYPAD);
        });
    }

    /// The bubble with the value during a drag, with the name of the speed
    /// below it, and the circle of the preview. The bubble is next to the
    /// panel or the card that holds `anchor`, at the height of `anchor`.
    fn bubble(&self, ui: &egui::Ui, value: f64, note: Option<&str>, anchor: Rect) {
        let ctx = ui.ctx();
        let id = egui::Id::new(("pad slider bubble", self.name));
        let painter = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, id));
        let screen = ctx.content_rect();
        if let Some(radius) = &self.preview {
            let (center, r) = (screen.center(), radius(value).max(1.0));
            let dark = egui::Stroke::new(3.0, Color32::from_black_alpha(140));
            painter.circle_stroke(center, r, dark);
            painter.circle_stroke(center, r, egui::Stroke::new(1.5, theme::TEXT));
        }
        let text = painter.layout_no_wrap(self.value_text(value), theme::title(), theme::TEXT);
        let note = note.map(|n| painter.layout_no_wrap(n.to_owned(), theme::small(), theme::MUTE));
        let note_size = note.as_ref().map_or(vec2(0.0, -2.0), |n| n.size());
        // A width that does not follow each digit, so the bubble stays put.
        let size = vec2(
            (text.size().x.max(note_size.x) + 28.0).max(BUBBLE_WIDTH),
            text.size().y + note_size.y + 2.0 + 16.0,
        );
        let top = anchor.center().y - size.y / 2.0;
        let rect = place(ctx, anchor, ui.clip_rect(), size, top, None);
        let radius = theme::CONTROL_RADIUS;
        let backdrop = widgets::backdrop(ctx);
        if backdrop.is_some() {
            painter.add(marker(Order::Tooltip, rect));
        }
        painter.add(theme::shadow().as_shape(rect, radius));
        if let Some(format) = backdrop {
            let corner = egui::CornerRadius::from(radius);
            painter.add(card_shape(format, rect, corner, ctx.pixels_per_point()));
        }
        painter.rect_filled(rect, radius, theme::card(backdrop.is_some()));
        let x = rect.center().x;
        let y = rect.top() + 8.0;
        let below = y + text.size().y + 2.0;
        painter.galley(pos2(x - text.size().x / 2.0, y), text, theme::TEXT);
        if let Some(note) = note {
            painter.galley(pos2(x - note.size().x / 2.0, below), note, theme::MUTE);
        }
    }
}

/// The decimals of a step, if it has 3 or fewer.
fn exact_decimals(step: f64) -> Option<usize> {
    (0..4).find(|d| {
        let scaled = step * 10f64.powi(*d as i32);
        (scaled - scaled.round()).abs() < 1e-6
    })
}

/// How the buttons of a stepper move the value.
#[derive(Clone, Copy)]
enum Steps<'a> {
    /// One step, or a hundredth of the range with no step.
    Step,
    /// 1, 2, 5, 10, 20, and so on.
    Nice,
    Choices(&'a [f64]),
}

/// The next value of the series 1, 2, 5, 10, 20, … above or below `value`.
/// Zero and the negative values follow the same series.
fn nice_step(value: f64, up: bool) -> f64 {
    if value == 0.0 {
        return if up { 0.01 } else { -0.01 };
    }
    let sign = value.signum();
    let magnitude = value.abs();
    // Away from zero is up for a positive value.
    let away = up == (sign > 0.0);
    let decade = 10f64.powf(magnitude.log10().floor() - 1.0);
    let series = (0..5).flat_map(|k| [1.0, 2.0, 5.0].map(|m| m * decade * 10f64.powi(k)));
    let eps = magnitude * 1e-9;
    let next = if away {
        series.into_iter().find(|v| *v > magnitude + eps)
    } else {
        series.into_iter().rfind(|v| *v < magnitude - eps)
    };
    sign * next.unwrap_or(0.0)
}

/// True in the frames in which a button that the user holds repeats: after
/// 0.45 seconds, ten times a second.
fn repeats(ui: &egui::Ui, resp: &egui::Response) -> bool {
    let id = resp.id.with("repeat");
    if !resp.is_pointer_button_down_on() {
        ui.data_mut(|data| data.remove::<(f64, f64)>(id));
        return false;
    }
    let now = ui.input(|i| i.time);
    let (start, last) = ui.data(|data| data.get_temp(id)).unwrap_or((now, now));
    ui.ctx().request_repaint();
    let repeat = now - start >= 0.45 && now - last >= 0.1;
    let last = if repeat { now } else { last };
    ui.data_mut(|data| data.insert_temp(id, (start, last)));
    repeat
}

/// The speed of a drag at a distance from the track, and its name.
fn speed(away: f32) -> (f64, Option<&'static str>) {
    SPEEDS
        .iter()
        .rev()
        .find(|(from, _, _)| away >= *from)
        .map_or((1.0, None), |(_, speed, name)| (*speed, Some(*name)))
}

/// The drag on `resp`, while it moves the slider.
fn dragging(ui: &egui::Ui, resp: &egui::Response) -> Option<Drag> {
    let id = resp.id.with("drag");
    let drag = ui.data(|data| data.get_temp::<Drag>(id))?;
    resp.dragged().then_some(drag)
}

/// The place of a card of `size` next to `panel`, the panel or the card
/// that holds `anchor`, with its top at `top`, over the globe. The place is
/// at the right of the panel, past any other panel there, then at the
/// left, then above the panel. With no such place, the card is below
/// `anchor`, over the panel. `own` is the layer of the card itself.
fn place(
    ctx: &egui::Context,
    anchor: Rect,
    panel: Rect,
    size: egui::Vec2,
    top: f32,
    own: Option<egui::LayerId>,
) -> Rect {
    let screen = ctx.content_rect().shrink(8.0);
    let fits = |rect: Rect| screen.contains_rect(rect);
    let top = top.clamp(screen.top(), (screen.bottom() - size.y).max(screen.top()));
    // At the right, past each panel in the way.
    let mut x = panel.right() + GAP;
    for _ in 0..4 {
        let rect = Rect::from_min_size(pos2(x, top), size);
        if !fits(rect) {
            break;
        }
        match panel_under(ctx, rect, own) {
            None => return rect,
            Some(other) => x = other.right() + GAP,
        }
    }
    // At the left, past each panel in the way.
    let mut right = panel.left() - GAP;
    for _ in 0..4 {
        let rect = Rect::from_min_size(pos2(right - size.x, top), size);
        if !fits(rect) {
            break;
        }
        match panel_under(ctx, rect, own) {
            None => return rect,
            Some(other) => right = other.left() - GAP,
        }
    }
    // Above the panel, past each card in the way, as for a sheet at the
    // bottom of the screen.
    let x = (anchor.center().x - size.x / 2.0).clamp(screen.left(), screen.right() - size.x);
    let mut bottom = panel.top() - GAP;
    for _ in 0..4 {
        let rect = Rect::from_min_size(pos2(x, bottom - size.y), size);
        if !fits(rect) {
            break;
        }
        match panel_under(ctx, rect, own) {
            None => return rect,
            Some(other) => bottom = other.top() - GAP,
        }
    }
    let below = Rect::from_min_size(pos2(anchor.right() - size.x, anchor.bottom() + 4.0), size);
    keep_in(below, screen)
}

/// The rectangle of a panel or a card below a part of `rect`, if there is
/// one. `own` is the layer of the card at `rect`.
fn panel_under(ctx: &egui::Context, rect: Rect, own: Option<egui::LayerId>) -> Option<Rect> {
    let points = [
        rect.left_top(),
        rect.right_top(),
        rect.left_bottom(),
        rect.right_bottom(),
        rect.center(),
        rect.left_center(),
        rect.right_center(),
    ];
    points.iter().find_map(|p| {
        let layer = ctx.layer_id_at(*p)?;
        if layer.order == Order::Background || Some(layer) == own {
            return None;
        }
        ctx.memory(|m| m.area_rect(layer.id))
    })
}

/// A rectangle moved into `bounds`.
fn keep_in(rect: Rect, bounds: Rect) -> Rect {
    let dx = (bounds.left() - rect.left()).max(0.0) + (bounds.right() - rect.right()).min(0.0);
    let dy = (bounds.top() - rect.top()).max(0.0) + (bounds.bottom() - rect.bottom()).min(0.0);
    rect.translate(vec2(dx, dy))
}

/// How the value field of a control looks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    /// A box of its own, at the right of a slider.
    Box,
    /// The middle part of a stepper.
    Segment,
    /// Small text with no box, above a vertical slider.
    Plain,
}

/// The track, its filled part up to the handle at `at`, the marks of the
/// values at which the handle stops, and the handle. `at` and `marks` are
/// places along the track: x, or y for a `vertical` track, which fills from
/// the bottom.
fn paint_track(
    ui: &egui::Ui,
    track: Rect,
    at: f32,
    marks: &[f32],
    vertical: bool,
    enabled: bool,
    held: bool,
) {
    let painter = ui.painter();
    let radius = theme::CONTROL_RADIUS;
    painter.rect_filled(track, radius, theme::track());
    if !enabled {
        return;
    }
    let filled = if vertical {
        track.with_min_y(at.min(track.bottom()))
    } else {
        track.with_max_x(at.max(track.left()))
    };
    painter.rect_filled(filled, radius, theme::ACCENT.gamma_multiply(0.85));
    let inner = track.shrink(HANDLE_INSET + 4.0);
    for m in marks {
        let line = if vertical {
            Rect::from_min_max(pos2(inner.left(), m - 0.75), pos2(inner.right(), m + 0.75))
        } else {
            Rect::from_min_max(pos2(m - 0.75, inner.top()), pos2(m + 0.75, inner.bottom()))
        };
        painter.rect_filled(line, 0.0, Color32::from_white_alpha(90));
    }
    let across = track.shrink(HANDLE_INSET);
    let handle = if vertical {
        Rect::from_min_max(
            pos2(across.left(), at - HANDLE / 2.0),
            pos2(across.right(), at + HANDLE / 2.0),
        )
    } else {
        Rect::from_min_max(
            pos2(at - HANDLE / 2.0, across.top()),
            pos2(at + HANDLE / 2.0, across.bottom()),
        )
    };
    let handle = if held { handle.expand(1.5) } else { handle };
    let shadow = handle.translate(vec2(0.0, 1.5)).expand(1.0);
    painter.rect_filled(shadow, 5.0, Color32::from_black_alpha(70));
    painter.rect_filled(handle, 4.0, theme::TEXT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mixed_value_shows_mixed() {
        let n = Number::new("n", 2.0, 0.0..=10.0).unit("pt");
        assert_eq!(n.value_text(2.0), "2 pt");
        assert_eq!(n.mixed(true).value_text(2.0), "Mixed");
    }

    #[test]
    fn a_place_on_the_track_and_its_value_match_on_each_scale() {
        for scale in [Scale::Linear, Scale::Log, Scale::Square] {
            let n = Number::new("n", 0.0, 1.0..=1000.0).scale(scale);
            for place in [0.0, 0.25, 0.5, 1.0] {
                let back = n.place(n.at(place));
                assert!((back - place).abs() < 1e-9, "{scale:?} {place}");
            }
        }
        let log = Number::new("n", 0.0, 1.0..=100.0).log();
        assert!((log.at(0.5) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_typed_value_is_in_the_range_and_a_whole_number_of_steps() {
        let n = Number::new("n", 0.0, 0.0..=6000.0).step(50.0).unit("m");
        assert_eq!(n.parse("1,234"), Some(1250.0));
        assert_eq!(n.parse("\u{2212}20"), Some(0.0));
        assert_eq!(n.parse("9000"), Some(6000.0));
        assert_eq!(n.parse("-"), None);
        let percent = Number::new("n", 0.5, 0.0..=1.0).percent();
        assert_eq!(percent.parse("42"), Some(0.42));
        assert_eq!(percent.value_text(0.42), "42%");
        assert_eq!(percent.number(0.42), "42");
    }

    #[test]
    fn the_text_has_the_decimals_of_the_step() {
        let n = Number::new("n", 0.0, 0.0..=10.0).step(0.25).unit("pt");
        assert_eq!(n.value_text(2.5), "2.5 pt");
        assert_eq!(n.value_text(2.25), "2.25 pt");
        let n = Number::new("n", 0.0, 0.0..=10.0).step(0.5).unit("mm");
        assert_eq!(n.value_text(2.5), "2.5 mm");
        let n = Number::new("n", 0.0, 1.0..=500.0).log().unit("km");
        assert_eq!(n.value_text(12.345), "12.3 km");
        let arc = Number::new("n", 0.0, 0.0..=30.0).unit("°");
        assert_eq!(arc.number(1.0 / 3600.0), "0.000278");
        let tenth = Number::new("n", 0.0, 0.0..=10.0).step(0.1);
        assert_eq!(tenth.clean(0.7000001), 0.7);
    }

    #[test]
    fn the_buttons_of_a_stepper_move_one_step_and_stop_at_the_ends() {
        let n = Number::new("n", 0.0, 0.0..=10.0).step(0.5);
        assert_eq!(n.step_from(2.0, true), Some(2.5));
        assert_eq!(n.step_from(2.0, false), Some(1.5));
        assert_eq!(n.step_from(10.0, true), None);
        assert_eq!(n.step_from(0.0, false), None);
        let nice = Number::new("n", 0.0, 0.05..=10.0).nice();
        assert_eq!(nice.step_from(3.0, true), Some(5.0));
        assert_eq!(nice.step_from(3.0, false), Some(2.0));
        assert_eq!(nice.step_from(1.0, false), Some(0.5));
        assert_eq!(nice.step_from(10.0, true), None);
        let choices = [1.0, 5.0, 10.0, 30.0];
        let list = Number::new("n", 0.0, 1.0..=30.0).choices(&choices);
        assert_eq!(list.step_from(5.0, true), Some(10.0));
        assert_eq!(list.step_from(5.0, false), Some(1.0));
        assert_eq!(list.parse("12"), Some(10.0));
    }

    #[test]
    fn a_drag_far_from_the_track_is_slower() {
        assert_eq!(speed(0.0).0, 1.0);
        assert_eq!(speed(70.0), (0.5, Some("Half speed")));
        assert_eq!(speed(130.0).0, 0.25);
        assert_eq!(speed(400.0).0, 0.125);
    }
}
