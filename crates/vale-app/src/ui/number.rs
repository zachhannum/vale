//! The two number controls of the desktop layout: a slider and a stepper.
//!
//! Each control shows the value in shown units, which are the stored value
//! times a factor, for example a percent of a fraction. A click on the
//! number of a slider or of a stepper lets the user type a value.

use std::ops::RangeInclusive;

use eframe::egui;
use egui::emath::Numeric;

use crate::world::three_significant;

/// A slider with a label. Make one with [`slider`].
pub struct Slider<'a, T: Numeric> {
    label: &'a str,
    value: &'a mut T,
    range: RangeInclusive<f64>,
    log: bool,
    smallest: Option<f64>,
    step: Option<f64>,
    suffix: String,
    factor: f64,
    decimals: Option<usize>,
    format: Option<Box<dyn Fn(f64) -> String + 'a>>,
    enabled: bool,
}

/// A slider of `value` in `range`. The range is in stored units.
pub fn slider<'a, T: Numeric>(
    label: &'a str,
    value: &'a mut T,
    range: RangeInclusive<T>,
) -> Slider<'a, T> {
    Slider {
        label,
        value,
        range: range.start().to_f64()..=range.end().to_f64(),
        log: false,
        smallest: None,
        step: None,
        suffix: String::new(),
        factor: 1.0,
        decimals: None,
        format: None,
        enabled: true,
    }
}

impl<'a, T: Numeric> Slider<'a, T> {
    /// Gives the slider a logarithmic scale.
    pub fn log(mut self) -> Self {
        self.log = true;
        self
    }

    /// The smallest positive value of a logarithmic slider whose range
    /// starts at zero, in stored units.
    pub fn smallest(mut self, smallest: f64) -> Self {
        self.smallest = Some(smallest);
        self
    }

    /// The step of the value, in shown units.
    pub fn step(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }

    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }

    /// The factor from stored units to shown units.
    pub fn factor(mut self, factor: f64) -> Self {
        self.factor = factor;
        self
    }

    /// Shows a percent of a fraction.
    pub fn percent(self) -> Self {
        self.factor(100.0).suffix(" %").decimals(0)
    }

    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = Some(decimals);
        self
    }

    /// The text of a shown value. It replaces the suffix and the decimals.
    pub fn format(mut self, format: impl Fn(f64) -> String + 'a) -> Self {
        self.format = Some(Box::new(format));
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Adds the slider with the label at its right.
    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        self.add(ui, true)
    }

    /// Adds the label and the slider as a row of a grid.
    pub fn row(self, ui: &mut egui::Ui) -> egui::Response {
        ui.label(self.label);
        let resp = self.add(ui, false);
        ui.end_row();
        resp
    }

    fn add(self, ui: &mut egui::Ui, text: bool) -> egui::Response {
        let factor = self.factor;
        let mut shown = self.value.to_f64() * factor;
        let range = self.range.start() * factor..=self.range.end() * factor;
        let mut slider = egui::Slider::new(&mut shown, range).logarithmic(self.log);
        if let Some(smallest) = self.smallest {
            slider = slider.smallest_positive(smallest * factor);
        }
        if let Some(step) = self.step {
            slider = slider.step_by(step);
        }
        if let Some(format) = self.format {
            slider = slider
                .custom_formatter(move |v, _| format(v))
                .custom_parser(leading_number);
        } else {
            slider = slider.suffix(self.suffix);
            match self.decimals {
                Some(d) => slider = slider.fixed_decimals(d),
                None if self.step.is_none() && self.log => {
                    slider = slider.custom_formatter(|v, _| three_significant(v));
                }
                None => {}
            }
        }
        if text {
            slider = slider.text(self.label);
        }
        let resp = ui.add_enabled(self.enabled, slider);
        if !text {
            let (enabled, label) = (self.enabled, self.label);
            resp.widget_info(|| egui::WidgetInfo::slider(enabled, shown, label));
        }
        if resp.changed() {
            *self.value = T::from_f64(shown / factor);
        }
        resp
    }
}

/// A field of a number with a "−" button and a "+" button. Make one with
/// [`stepper`].
pub struct Stepper<'a, T: Numeric> {
    label: &'a str,
    value: &'a mut T,
    range: Option<RangeInclusive<f64>>,
    step: Option<f64>,
    suffix: String,
    factor: f64,
    decimals: Option<usize>,
    enabled: bool,
}

/// A stepper of `value`. With no step, the buttons move through the series
/// 1, 2, 5, 10, 20, ….
pub fn stepper<'a, T: Numeric>(label: &'a str, value: &'a mut T) -> Stepper<'a, T> {
    Stepper {
        label,
        value,
        range: None,
        step: None,
        suffix: String::new(),
        factor: 1.0,
        decimals: None,
        enabled: true,
    }
}

impl<'a, T: Numeric> Stepper<'a, T> {
    /// The range of the value, in stored units.
    pub fn range(mut self, range: RangeInclusive<T>) -> Self {
        self.range = Some(range.start().to_f64()..=range.end().to_f64());
        self
    }

    /// The step of the buttons, in shown units.
    pub fn step(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }

    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }

    /// The factor from stored units to shown units.
    pub fn factor(mut self, factor: f64) -> Self {
        self.factor = factor;
        self
    }

    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = Some(decimals);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Adds the stepper with the label at its right.
    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let label = self.label;
        ui.horizontal(|ui| {
            let resp = self.add(ui);
            ui.label(label);
            resp
        })
        .inner
    }

    /// Adds the label and the stepper as a row of a grid.
    pub fn row(self, ui: &mut egui::Ui) -> egui::Response {
        ui.label(self.label);
        let resp = ui.horizontal(|ui| self.add(ui)).inner;
        ui.end_row();
        resp
    }

    /// The range in shown units.
    fn shown_range(&self) -> Option<RangeInclusive<f64>> {
        let range = self.range.as_ref()?;
        let (a, b) = (range.start() * self.factor, range.end() * self.factor);
        Some(a.min(b)..=a.max(b))
    }

    fn add(self, ui: &mut egui::Ui) -> egui::Response {
        let factor = self.factor;
        let range = self.shown_range();
        let mut shown = self.value.to_f64() * factor;
        let down = step_from(shown, self.step, range.clone(), false);
        let up = step_from(shown, self.step, range.clone(), true);
        ui.spacing_mut().item_spacing.x = 2.0;
        let (enabled, label) = (self.enabled, self.label);
        let minus = ui.add_enabled(enabled && down.is_some(), egui::Button::new("−").small());
        minus.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                enabled,
                format!("Decrease {label}"),
            )
        });
        let speed = match (self.step, &range) {
            (Some(step), _) => step / 4.0,
            (None, Some(r)) => (r.end() - r.start()) / 200.0,
            (None, None) => 0.1,
        };
        let mut drag = egui::DragValue::new(&mut shown)
            .speed(speed)
            .suffix(self.suffix);
        if let Some(r) = range.clone() {
            drag = drag.range(r);
        }
        drag = match (self.decimals, self.step) {
            (Some(d), _) => drag.fixed_decimals(d),
            (None, Some(step)) => drag.max_decimals(decimals_of(step).max(2)),
            (None, None) => drag.max_decimals(3),
        };
        let mut resp = ui.add_enabled(enabled, drag);
        let plus = ui.add_enabled(enabled && up.is_some(), egui::Button::new("+").small());
        plus.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                enabled,
                format!("Increase {label}"),
            )
        });
        let stepped = if minus.clicked() {
            down
        } else if plus.clicked() {
            up
        } else {
            None
        };
        if let Some(v) = stepped {
            shown = v;
            resp.mark_changed();
        }
        resp.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::WidgetType::DragValue, enabled, label);
            info.value = Some(shown);
            info
        });
        if resp.changed() {
            *self.value = T::from_f64(shown / factor);
        }
        resp
    }
}

/// The value one step up or down from `value`, or `None` at the end of the
/// range. With no step, the value moves through the series 1, 2, 5, 10, ….
pub fn step_from(
    value: f64,
    step: Option<f64>,
    range: Option<RangeInclusive<f64>>,
    up: bool,
) -> Option<f64> {
    let next = match step {
        Some(step) if step > 0.0 => {
            let n = (value / step).round() + if up { 1.0 } else { -1.0 };
            let scale = 10f64.powi(decimals_of(step) as i32 + 2);
            (n * step * scale).round() / scale
        }
        _ => nice_step(value, up),
    };
    let next = match range {
        Some(r) => next.clamp(*r.start(), *r.end()),
        None => next,
    };
    let eps = value.abs().max(1.0) * 1e-9;
    let moved = if up {
        next > value + eps
    } else {
        next < value - eps
    };
    moved.then_some(next)
}

/// The number of decimals of a step, up to six.
fn decimals_of(step: f64) -> usize {
    (0..6usize)
        .find(|d| {
            let scaled = step * 10f64.powi(*d as i32);
            (scaled - scaled.round()).abs() < 1e-6
        })
        .unwrap_or(6)
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

/// The number at the start of a text, for a value with a unit or a sign.
fn leading_number(text: &str) -> Option<f64> {
    let text = text.trim();
    let end = text
        .char_indices()
        .find(|(i, c)| !(c.is_ascii_digit() || *c == '.' || (*i == 0 && "+-−".contains(*c))))
        .map_or(text.len(), |(i, _)| i);
    text[..end].replace('−', "-").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_moves_to_the_next_multiple() {
        assert_eq!(step_from(10.0, Some(0.25), None, true), Some(10.25));
        assert_eq!(step_from(10.1, Some(0.25), None, false), Some(9.75));
        assert_eq!(step_from(0.3, Some(0.1), None, true), Some(0.4));
    }

    #[test]
    fn a_step_stops_at_the_end_of_the_range() {
        assert_eq!(step_from(8.0, Some(0.25), Some(0.25..=8.0), true), None);
        assert_eq!(
            step_from(7.9, Some(0.25), Some(0.25..=8.0), true),
            Some(8.0)
        );
    }

    #[test]
    fn no_step_moves_through_one_two_five() {
        assert_eq!(step_from(1.0, None, None, true), Some(2.0));
        assert_eq!(step_from(2.0, None, None, true), Some(5.0));
        assert_eq!(step_from(5.0, None, None, true), Some(10.0));
        assert_eq!(step_from(10.0, None, None, false), Some(5.0));
        assert_eq!(step_from(3.0, None, None, false), Some(2.0));
    }

    #[test]
    fn a_number_with_a_unit_or_a_sign_parses() {
        assert_eq!(leading_number("+5%"), Some(5.0));
        assert_eq!(leading_number("−12.5 m"), Some(-12.5));
        assert_eq!(leading_number("250 m"), Some(250.0));
    }
}
