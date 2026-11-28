//! The number rows of the pad layout: a slider and a stepper. Each row is
//! `PANEL_ROW` high, with the label at the left and the value text at the
//! right. The touch area of each control is at least `TOUCH` on each side.

use std::ops::RangeInclusive;

use eframe::egui::{self, Align2, Color32, Rect, Sense, pos2, vec2};

use super::metrics::TOUCH;
use super::theme;
use super::widgets::{Controls, PANEL_ROW, fade, hit, push};

/// The height of a track and of the back of a stepper.
const HEIGHT: f32 = 36.0;
/// The width of the handle, and its space from the edges of the track.
const HANDLE: f32 = 10.0;
const HANDLE_INSET: f32 = 4.0;
/// The space from each end of a track to the place of the handle at that
/// end.
const TRACK_INSET: f32 = HANDLE_INSET + HANDLE / 2.0;
/// The smallest width of the label of a row, so that the tracks of the
/// rows line up.
pub const LABEL_COLUMN: f32 = 96.0;
/// The width of the value text at the right of a row.
const VALUE_WIDTH: f32 = 84.0;

/// What a slider did in one frame.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SliderResponse {
    /// The value changed.
    pub changed: bool,
    /// A drag started. The value is still the value before the drag.
    pub started: bool,
    /// A drag ended.
    pub released: bool,
}

/// The value text at the right of `row`.
fn value_text(ui: &egui::Ui, row: Rect, text: &str, enabled: bool) {
    let at = pos2(row.right(), row.center().y);
    let color = fade(theme::MUTE, enabled);
    ui.painter()
        .text(at, Align2::RIGHT_CENTER, text, theme::body(), color);
}

/// The label at the left of `row`. Returns its width with its space.
fn label_text(ui: &egui::Ui, row: Rect, label: &str, enabled: bool) -> f32 {
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), theme::body(), fade(theme::TEXT, enabled));
    let width = (galley.size().x + 16.0).max(LABEL_COLUMN);
    let at = pos2(row.left(), row.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, theme::TEXT);
    width
}

/// A row with `label`, a slider, and `text`, the value as the app shows
/// it. A drag moves the value by the distance of the drag. The touch area
/// of the slider is as high as the row.
pub fn slider_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    text: &str,
) -> SliderResponse {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let y = row.center().y;
    let start = row.left() + label_text(ui, row, label, true);
    let end = row.right() - VALUE_WIDTH - 8.0;
    let body = Rect::from_min_max(
        pos2(start, y - PANEL_ROW / 2.0),
        pos2(end.max(start + TOUCH), y + PANEL_ROW / 2.0),
    );
    let resp = ui.interact(body, ui.id().with(label), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let span = hi - lo;
    let place = |v: f64| match span > 0.0 {
        true => ((v - lo) / span).clamp(0.0, 1.0),
        false => 0.0,
    };
    let track = Rect::from_min_max(
        pos2(body.left(), y - HEIGHT / 2.0),
        pos2(body.right(), y + HEIGHT / 2.0),
    );
    let left = track.left() + TRACK_INSET;
    let length = (track.right() - TRACK_INSET - left).max(1.0);

    let id = resp.id.with("from");
    let started = resp.drag_started();
    if started {
        ui.data_mut(|data| data.insert_temp(id, place(*value)));
    }
    let mut changed = false;
    if resp.dragged()
        && let Some(from) = ui.data(|data| data.get_temp::<f64>(id))
        && let (Some(origin), Some(pos)) = (
            ui.input(|i| i.pointer.press_origin()),
            resp.interact_pointer_pos(),
        )
    {
        let moved = f64::from((pos.x - origin.x) / length);
        let next = lo + (from + moved).clamp(0.0, 1.0) * span;
        if next != *value {
            *value = next;
            changed = true;
        }
    }
    let released = resp.drag_stopped();
    if released {
        ui.data_mut(|data| data.remove::<f64>(id));
    }
    let shown = *value;
    resp.widget_info(|| egui::WidgetInfo::slider(true, shown, label));
    push(ui, controls, body, label);

    let held = resp.dragged() || resp.is_pointer_button_down_on();
    paint_track(ui, track, left + place(*value) as f32 * length, held);
    value_text(ui, row, text, true);
    SliderResponse {
        changed,
        started,
        released,
    }
}

/// The name of the minus button of a stepper.
pub fn minus_name(label: &str) -> String {
    format!("Decrease {label}")
}

/// The name of the plus button of a stepper.
pub fn plus_name(label: &str) -> String {
    format!("Increase {label}")
}

/// A row with `label`, `text`, and a minus button and a plus button that
/// move the value by `step`, in `range`. A button is off at the end of the
/// range. Returns true when the value changed.
pub fn stepper_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    label: &str,
    value: &mut f64,
    step: f64,
    range: RangeInclusive<f64>,
    text: &str,
) -> bool {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let y = row.center().y;
    label_text(ui, row, label, true);
    let whole = Rect::from_min_max(
        pos2(row.right() - 2.0 * TOUCH - VALUE_WIDTH, y - HEIGHT / 2.0),
        pos2(row.right(), y + HEIGHT / 2.0),
    );
    let radius = theme::capsule(whole);
    ui.painter().rect_filled(whole, radius, theme::raise());
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
        ui.painter().rect_filled(line, 0.0, theme::divider());
    }
    let (lo, hi) = (*range.start(), *range.end());
    let next = |up: bool| {
        let step = step.abs();
        if step == 0.0 {
            return None;
        }
        // The next multiple of the step, so a value between two steps
        // goes to the nearer one in the direction of the button.
        let at = *value / step;
        let n = match up {
            true => (at + 1e-9).floor() + 1.0,
            false => (at - 1e-9).ceil() - 1.0,
        };
        let next = (n * step).clamp(lo, hi);
        let moved = if up { next > *value } else { next < *value };
        moved.then_some(next)
    };
    let (down, up) = (next(false), next(true));
    let r = radius as u8;
    let mut out = None;
    for (rect, name, next, sign, corner) in [
        (minus, minus_name(label), down, false, (r, 0)),
        (plus, plus_name(label), up, true, (0, r)),
    ] {
        let on = next.is_some();
        let resp = hit(ui, controls, rect, &name, Sense::click(), on, None);
        if resp.clicked() {
            out = next;
        }
        let back = rect.intersect(whole);
        let painter = ui.painter();
        if resp.is_pointer_button_down_on() {
            let corner = egui::CornerRadius {
                nw: corner.0,
                sw: corner.0,
                ne: corner.1,
                se: corner.1,
            };
            painter.rect_filled(back, corner, theme::raise());
        }
        let c = back.center();
        let stroke = egui::Stroke::new(2.0, fade(theme::TEXT, on));
        painter.line_segment([c - vec2(6.0, 0.0), c + vec2(6.0, 0.0)], stroke);
        if sign {
            painter.line_segment([c - vec2(0.0, 6.0), c + vec2(0.0, 6.0)], stroke);
        }
    }
    ui.painter().text(
        field.center(),
        Align2::CENTER_CENTER,
        text,
        theme::body(),
        theme::TEXT,
    );
    match out {
        Some(next) => {
            *value = next;
            true
        }
        None => false,
    }
}

/// The track, its filled part up to the handle at `at`, and the handle.
fn paint_track(ui: &egui::Ui, track: Rect, at: f32, held: bool) {
    let painter = ui.painter();
    let radius = theme::capsule(track);
    painter.rect_filled(track, radius, theme::track());
    let filled = track.with_max_x(at.max(track.left()));
    painter.rect_filled(filled, radius, theme::ACCENT.gamma_multiply(0.85));
    let across = track.shrink(HANDLE_INSET);
    let handle = Rect::from_min_max(
        pos2(at - HANDLE / 2.0, across.top()),
        pos2(at + HANDLE / 2.0, across.bottom()),
    );
    let handle = if held { handle.expand(1.5) } else { handle };
    let shadow = handle.translate(vec2(0.0, 1.5)).expand(1.0);
    painter.rect_filled(
        shadow,
        theme::capsule(shadow),
        Color32::from_black_alpha(70),
    );
    painter.rect_filled(handle, theme::capsule(handle), theme::TEXT);
}
