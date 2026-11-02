//! The number controls of the iPad layout: a slider and a stepper. A drag
//! on a slider moves the value by the distance of the drag, and a drag away
//! from the track moves it more slowly. A stepper has a minus button and a
//! plus button. A tap on the value of either control opens the keypad.

use eframe::egui::{self, Align2, Color32, Order, Pos2, Rect, Sense, pos2, vec2};

use super::geometry::{CELL, PAD, TOUCH};
use super::theme;
use super::widgets::{self, Controls, KEYPAD, KeyEdit, Keys, PANEL_ROW, TICK_NAMES, fade, hit};

const THUMB: f32 = 26.0;
/// The thumb while a finger holds it.
const THUMB_HELD: f32 = 32.0;
/// The height of the track.
const TRACK: f32 = 6.0;
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
    /// The drag scrolls the panel.
    scroll: bool,
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
        let resp = hit(
            ui,
            controls,
            body,
            self.name,
            Sense::click_and_drag(),
            enabled,
            None,
        );
        let left = if self.label.is_empty() {
            row.left() + 6.0 + THUMB / 2.0
        } else {
            row.left() + (label.size().x + 16.0).max(72.0) + THUMB / 2.0
        };
        let right = if self.field {
            field.left() - 8.0 - THUMB / 2.0
        } else {
            row.right() - 6.0 - THUMB / 2.0
        };
        let mut out = self.drag(ui, &resp, right - left, |p| (p.x, (p.y - y).abs()));
        let value = out.unwrap_or(self.value);

        let x_of = |v: f64| left + self.place(v) as f32 * (right - left);
        let x = x_of(value);
        let whole = Rect::from_min_max(
            pos2(left - TRACK, y - TRACK / 2.0),
            pos2(right + TRACK, y + TRACK / 2.0),
        );
        let filled = Rect::from_min_max(whole.min, pos2(x, whole.bottom()));
        let marks: Vec<Pos2> = (self.default.iter())
            .chain(self.ticks.iter().map(|(v, _)| v))
            .map(|v| pos2(x_of(*v), y))
            .collect();
        paint_track(ui, whole, filled, pos2(x, y), &marks, enabled, &resp);
        let painter = ui.painter();
        let at = pos2(row.left(), y - label.size().y / 2.0);
        painter.galley(at, label, text);
        for (tick, name) in self.ticks {
            let active = (tick - value).abs() <= tick.abs() * 1e-9;
            let color = fade(if active { theme::TEXT } else { theme::MUTE }, enabled);
            let at = pos2(x_of(*tick), row.bottom() - 6.0);
            painter.text(at, Align2::CENTER_TOP, *name, theme::small(), color);
        }

        if self.field {
            let typed = self.field(ui, controls, field, value, true);
            out = typed.or(out);
            widgets::keypad_below(ui, controls, &self.field_name());
        }
        if let (Some(pos), Some(drag)) = (resp.interact_pointer_pos(), dragging(ui, &resp)) {
            self.bubble(ui, value, drag.note, pos - vec2(0.0, 40.0), false);
        }
        out.filter(|v| *v != self.value)
    }

    /// Adds the slider as a column `height` high, with the value at the top
    /// and the label at the bottom. Returns the new value when the user
    /// changes it. The keypad opens at the right of the slider.
    pub fn vertical(self, ui: &mut egui::Ui, controls: &mut Controls, height: f32) -> Option<f64> {
        let enabled = self.enabled;
        let (rect, _) = ui.allocate_exact_size(vec2(CELL.0, height), Sense::hover());
        let resp = hit(
            ui,
            controls,
            rect,
            self.name,
            Sense::click_and_drag(),
            enabled,
            None,
        );
        let top = rect.top() + 20.0 + THUMB / 2.0;
        let bottom = rect.bottom() - 20.0 - THUMB / 2.0;
        let x = rect.center().x;
        let mut out = self.drag(ui, &resp, bottom - top, |p| (-p.y, (p.x - x).abs()));
        let value = out.unwrap_or(self.value);

        let y = bottom - self.place(value) as f32 * (bottom - top);
        let whole = Rect::from_min_max(
            pos2(x - TRACK / 2.0, top - TRACK),
            pos2(x + TRACK / 2.0, bottom + TRACK),
        );
        let filled = Rect::from_min_max(pos2(whole.left(), y), whole.max);
        let marks: Vec<Pos2> = (self.default.iter())
            .map(|d| pos2(x, bottom - self.place(*d) as f32 * (bottom - top)))
            .collect();
        paint_track(ui, whole, filled, pos2(x, y), &marks, enabled, &resp);
        let color = fade(theme::MUTE, enabled);
        let at = pos2(x, rect.bottom() - 6.0);
        let painter = ui.painter();
        painter.text(at, Align2::CENTER_BOTTOM, self.label, theme::small(), color);

        // The field covers the value at the top. It takes the touches there
        // from the track.
        let field = Rect::from_min_size(rect.min, vec2(rect.width(), TOUCH));
        let typed = self.field(ui, controls, field, value, false);
        out = typed.or(out);
        self.keypad_beside(ui, controls, rect);
        if let (Some(pos), Some(drag)) = (resp.interact_pointer_pos(), dragging(ui, &resp)) {
            let at = pos2(pos.x.max(rect.right()) + 40.0, pos.y);
            self.bubble(ui, value, drag.note, at, true);
        }
        out.filter(|v| *v != self.value)
    }

    /// Adds the number as a stepper in a row of a panel: the label, the
    /// minus button, the value, and the plus button. A button that you hold
    /// repeats. Returns the new value when the user changes it.
    pub fn stepper(self, ui: &mut egui::Ui, controls: &mut Controls) -> Option<f64> {
        let enabled = self.enabled;
        let size = vec2(ui.available_width(), PANEL_ROW);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let y = rect.center().y;
        let text = fade(theme::TEXT, enabled);
        let at = pos2(rect.left(), y);
        let painter = ui.painter();
        painter.text(at, Align2::LEFT_CENTER, self.label, theme::body(), text);
        let button = |right: f32| {
            Rect::from_min_max(
                pos2(right - TOUCH, y - TOUCH / 2.0),
                pos2(right, y + TOUCH / 2.0),
            )
        };
        let plus = button(rect.right());
        let field = Rect::from_min_max(
            pos2(plus.left() - VALUE_WIDTH, plus.top()),
            pos2(plus.left(), plus.bottom()),
        );
        let minus = button(field.left());
        let down = self.step_from(self.value, false);
        let up = self.step_from(self.value, true);
        let mut out = None;
        for (rect, name, next, sign) in [
            (minus, self.minus_name(), down, false),
            (plus, self.plus_name(), up, true),
        ] {
            let on = enabled && next.is_some();
            let resp = hit(ui, controls, rect, &name, Sense::click(), on, None);
            if on && (resp.clicked() || repeats(ui, &resp)) {
                out = next;
            }
            let back = rect.shrink(widgets::HIGHLIGHT_INSET);
            let fill = if resp.is_pointer_button_down_on() {
                theme::raise().gamma_multiply(2.0)
            } else {
                theme::raise()
            };
            let painter = ui.painter();
            painter.rect_filled(back, theme::CONTROL_RADIUS, fill);
            let color = fade(theme::TEXT, on);
            let c = back.center();
            let stroke = egui::Stroke::new(2.0, color);
            painter.line_segment([c - vec2(7.0, 0.0), c + vec2(7.0, 0.0)], stroke);
            if sign {
                painter.line_segment([c - vec2(0.0, 7.0), c + vec2(0.0, 7.0)], stroke);
            }
        }
        let value = out.unwrap_or(self.value);
        let typed = self.field(ui, controls, field.shrink2(vec2(4.0, 0.0)), value, true);
        out = typed.or(out);
        widgets::keypad_below(ui, controls, &self.field_name());
        out.filter(|v| *v != self.value)
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

    /// Moves the value with a drag on `resp`. `length` is the length of the
    /// track. `along` gives, for a place on the screen, its place along the
    /// track and its distance from the track. Returns the value while the
    /// drag moves it.
    fn drag(
        &self,
        ui: &egui::Ui,
        resp: &egui::Response,
        length: f32,
        along: impl Fn(Pos2) -> (f32, f32),
    ) -> Option<f64> {
        let id = resp.id.with("drag");
        if resp.drag_started() {
            let pointer = ui.input(|i| i.pointer.press_origin().zip(i.pointer.interact_pos()));
            let moved = pointer.map_or(egui::Vec2::ZERO, |(from, to)| to - from);
            // Only a row in a panel scrolls.
            let (a, _) = along(pos2(0.0, 1.0));
            let scroll = a == 0.0 && moved.y.abs() > moved.x.abs();
            let drag = Drag {
                place: self.place(self.value),
                scroll,
                note: None,
            };
            ui.data_mut(|data| data.insert_temp(id, drag));
        }
        let drag: Drag = ui.data(|data| data.get_temp(id))?;
        if !resp.dragged() {
            if resp.drag_stopped() {
                ui.data_mut(|data| data.remove::<Drag>(id));
            }
            return None;
        }
        if drag.scroll {
            ui.scroll_with_delta(vec2(0.0, resp.drag_delta().y));
            return None;
        }
        let pos = resp.interact_pointer_pos()?;
        let (now, away) = along(pos);
        let (before, _) = along(pos - resp.drag_delta());
        let (speed, note) = speed(away);
        let place = (drag.place + f64::from((now - before) / length) * speed).clamp(0.0, 1.0);
        let drag = Drag {
            place,
            note,
            ..drag
        };
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
    /// `boxed`: the field has a box, as in a row.
    fn field(
        &self,
        ui: &mut egui::Ui,
        controls: &mut Controls,
        rect: Rect,
        value: f64,
        boxed: bool,
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
                    text: self.number(value),
                    fresh: true,
                    changed: false,
                    placed: true,
                }),
            };
            widgets::set_key_edit(&ctx, edit.clone());
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
        let back = rect.shrink2(vec2(0.0, 6.0));
        if boxed {
            let fill = if edit.is_some() || resp.is_pointer_button_down_on() {
                theme::raise().gamma_multiply(1.6)
            } else {
                theme::raise()
            };
            painter.rect_filled(back, theme::CONTROL_RADIUS, fill);
        }
        if edit.is_some() {
            let stroke = egui::Stroke::new(1.5, theme::ACCENT);
            let kind = egui::StrokeKind::Inside;
            painter.rect_stroke(back, theme::CONTROL_RADIUS, stroke, kind);
        }
        let (font, color) = if boxed {
            (theme::mono(), fade(theme::TEXT, self.enabled))
        } else {
            (theme::small(), fade(theme::MUTE, self.enabled))
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
        let at = if boxed {
            pos2(back.right() - 10.0 - galley.size().x, back.center().y)
        } else {
            pos2(back.center().x - galley.size().x / 2.0, rect.top() + 6.0)
        };
        let at = pos2(
            at.x,
            if boxed {
                at.y - galley.size().y / 2.0
            } else {
                at.y
            },
        );
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

    /// The keypad of the field, in a card at the right of `rect`, while it
    /// is open.
    fn keypad_beside(&self, ui: &egui::Ui, controls: &mut Controls, rect: Rect) {
        if widgets::keypad_field(ui.ctx()).as_deref() != Some(&self.field_name()) {
            return;
        }
        let size = vec2(4.0 * TOUCH * 1.25, KEYPAD) + vec2(2.0 * PAD, 2.0 * PAD);
        let card = Rect::from_min_size(pos2(rect.right() + PAD + 8.0, rect.top()), size);
        let id = format!("{} keypad", self.name);
        let radius = theme::CARD_RADIUS.into();
        widgets::card(ui.ctx(), None, &id, card, Order::Foreground, radius, |ui| {
            let inner = ui.max_rect().shrink(PAD);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            widgets::keypad(&mut ui, controls, KEYPAD);
        });
    }

    /// The bubble with the value during a drag, with the name of the speed
    /// below it, and the circle of the preview. `at` is the bottom center of
    /// the bubble, or with `beside`, its left center.
    fn bubble(&self, ui: &egui::Ui, value: f64, note: Option<&str>, at: Pos2, beside: bool) {
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
        let size = vec2(
            text.size().x.max(note_size.x) + 28.0,
            text.size().y + note_size.y + 2.0 + 16.0,
        );
        let min = if beside {
            at - vec2(0.0, size.y / 2.0)
        } else {
            at - vec2(size.x / 2.0, size.y)
        };
        let rect = keep_in(Rect::from_min_size(min, size), screen);
        let radius = theme::CONTROL_RADIUS;
        painter.add(theme::shadow().as_shape(rect, radius));
        painter.rect_filled(rect, radius, theme::card(false));
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
    (resp.dragged() && !drag.scroll).then_some(drag)
}

/// A rectangle moved into `bounds`.
fn keep_in(rect: Rect, bounds: Rect) -> Rect {
    let dx = (bounds.left() - rect.left()).max(0.0) + (bounds.right() - rect.right()).min(0.0);
    let dy = (bounds.top() - rect.top()).max(0.0) + (bounds.bottom() - rect.bottom()).min(0.0);
    rect.translate(vec2(dx, dy))
}

/// The track, its filled part, the tick of the default, and the thumb.
#[allow(clippy::too_many_arguments)]
fn paint_track(
    ui: &egui::Ui,
    whole: Rect,
    filled: Rect,
    thumb: Pos2,
    marks: &[Pos2],
    enabled: bool,
    resp: &egui::Response,
) {
    let painter = ui.painter();
    let round = TRACK / 2.0;
    painter.rect_filled(whole, round, theme::track());
    if !enabled {
        return;
    }
    painter.rect_filled(filled, round, theme::ACCENT);
    for at in marks {
        painter.circle_filled(*at, 2.0, theme::TEXT.gamma_multiply(0.7));
    }
    let held = resp.dragged() || resp.is_pointer_button_down_on();
    let size = if held { THUMB_HELD } else { THUMB };
    painter.circle_filled(
        thumb + vec2(0.0, 2.0),
        size / 2.0 + 1.5,
        Color32::from_black_alpha(60),
    );
    painter.circle_filled(thumb, size / 2.0, theme::TEXT);
    painter.circle_stroke(
        thumb,
        size / 2.0,
        egui::Stroke::new(0.5, Color32::from_black_alpha(40)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
