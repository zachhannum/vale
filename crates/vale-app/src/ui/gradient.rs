//! The gradient control of a raster layer: a bar in the colors of the
//! display, with a handle for each color stop below it.

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Response, Sense, Vec2, pos2, vec2};
use vale_store::RasterDisplay;
use vale_terrain::{Bands, Rgb};

use super::elevation::{self, SNAPS};
use super::pad::geometry::TOUCH;
use super::pad::theme;
use super::pad::widgets::{self, Controls};
use crate::world;

/// The name of the control for a screen reader and for the tests.
pub const GRADIENT: &str = "Gradient";
/// The text below the control when no stop is selected.
pub const HINT: &str =
    "Tap the bar to add a stop. Drag a stop to move it, or drag it down to remove it.";
/// A finger this far below the handle row removes the stop on release.
pub const PULL: f32 = 36.0;
/// The space between the bubble of a dragged value and the bar.
const BUBBLE_GAP: f32 = 8.0;
/// A tap on the bar this near to a stop selects the stop.
const BAR_GRAB: f32 = 6.0;

/// The sizes of the control.
pub struct Metrics {
    pub bar: f32,
    pub radius: f32,
    /// The space at each side of the bar, so that the end handles fit.
    pub inset: f32,
    /// The height of the row of the handles.
    pub handles: f32,
    pub tab: Vec2,
    pub tab_on: Vec2,
    /// A touch in the handle row this near to a stop takes the stop.
    pub cap: f32,
    /// The height of the row of the value labels.
    pub labels: f32,
}

pub const PAD: Metrics = Metrics {
    bar: 28.0,
    radius: 6.0,
    inset: 8.0,
    handles: 32.0,
    tab: vec2(12.0, 18.0),
    tab_on: vec2(18.0, 22.0),
    cap: TOUCH / 2.0,
    labels: 16.0,
};

pub const DESKTOP: Metrics = Metrics {
    bar: 20.0,
    radius: 4.0,
    inset: 6.0,
    handles: 20.0,
    tab: vec2(10.0, 14.0),
    tab_on: vec2(14.0, 18.0),
    cap: 10.0,
    labels: 16.0,
};

/// The stop nearest to `x`, if it is at most `cap` away. Two stops split
/// the space between them at their midpoint.
pub fn hit(xs: &[f32], x: f32, cap: f32) -> Option<usize> {
    let off = |i: &usize| (xs[*i] - x).abs();
    (0..xs.len())
        .min_by(|a, b| off(a).total_cmp(&off(b)))
        .filter(|i| off(i) <= cap)
}

/// The bar inside the area of the control.
pub fn bar_rect(area: Rect, m: &Metrics) -> Rect {
    Rect::from_min_max(
        pos2(area.left() + m.inset, area.top()),
        pos2(area.right() - m.inset, area.top() + m.bar),
    )
}

/// The row of the handles inside the area of the control.
pub fn handle_row(area: Rect, m: &Metrics) -> Rect {
    Rect::from_min_max(
        pos2(area.left(), area.top() + m.bar),
        pos2(area.right(), area.top() + m.bar + m.handles),
    )
}

/// The screen place of a value on the bar of a control.
pub fn stop_x(area: Rect, m: &Metrics, bands: &Bands, value: f64) -> f32 {
    let bar = bar_rect(area, m);
    bar.left() + elevation::bar_fraction(bands, value) as f32 * bar.width()
}

/// What a drag on the control does. It is set when the control takes the
/// drag. Another drag scrolls the panel.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gesture {
    /// Moves a stop. `lifted`: the finger is in the place that removes it.
    Move { index: usize, lifted: bool },
    /// Pulls the stop at the lower edge of the selected band down to
    /// remove it.
    Pull { index: usize, lifted: bool },
}

fn color32(c: Rgb) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// The gradient control. `selected` is a band. A tap on a handle or near a
/// stop on the bar selects the band above the stop, or clears the selection
/// if the band is selected. A tap on the stop at the top does nothing. A tap
/// at another place on the bar adds a stop. A drag that
/// starts sideways on a handle moves the stop, and a drag that starts down
/// on the selected handle pulls it off. A release far below the handles
/// removes the stop. Each other drag scrolls the panel. With `controls`,
/// the control is a touch control of the iPad layout.
pub fn editor(
    ui: &mut egui::Ui,
    controls: Option<&mut Controls>,
    m: &Metrics,
    bands: &mut Bands,
    selected: &mut Option<usize>,
    display: RasterDisplay,
    unit: &str,
) -> Response {
    let size = vec2(ui.available_width(), m.bar + m.handles);
    let (area, _) = ui.allocate_exact_size(size, Sense::hover());
    let resp = match controls {
        Some(controls) => widgets::tap_area(ui, controls, area, GRADIENT, true),
        None => {
            let resp = ui.interact(area, ui.id().with(GRADIENT), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, GRADIENT));
            resp
        }
    };
    if selected.is_some_and(|i| i >= bands.colors().len()) {
        *selected = None;
    }
    let bar = bar_rect(area, m);
    let row = handle_row(area, m);
    let span = bands.max() - bands.min();
    let value_of =
        |bands: &Bands, x: f32| snap_value(bands, f64::from((x - bar.left()) / bar.width()));
    let xs = |bands: &Bands| -> Vec<f32> {
        let stops = bands.stops().iter();
        stops.map(|s| stop_x(area, m, bands, s.value)).collect()
    };

    let gesture_id = resp.id.with("gesture");
    let origin = ui.input(|i| i.pointer.press_origin());
    let mut chosen = None;
    widgets::take_drag(ui, &resp, |moved| {
        let Some(from) = origin else {
            return false;
        };
        let on = (from.y >= row.top())
            .then(|| hit(&xs(bands), from.x, m.cap))
            .flatten()
            .filter(|i| !bands.fixed(*i));
        chosen = match on {
            Some(index) if moved.x.abs() >= moved.y.abs() => {
                *selected = Some(index);
                Some(Gesture::Move {
                    index,
                    lifted: false,
                })
            }
            Some(index) if moved.y > 0.0 && *selected == Some(index) => Some(Gesture::Pull {
                index,
                lifted: false,
            }),
            _ => None,
        };
        chosen.is_some()
    });
    if let Some(gesture) = chosen {
        ui.data_mut(|data| data.insert_temp(gesture_id, gesture));
    }
    let mut gesture: Option<Gesture> = ui.data(|data| data.get_temp(gesture_id));
    if resp.dragged() {
        let pos = resp.interact_pointer_pos();
        let lift = |pos: Option<Pos2>| pos.is_some_and(|p| p.y > row.bottom() + PULL);
        gesture = match gesture {
            Some(Gesture::Move { index, .. }) => {
                if let Some(pos) = pos {
                    bands.move_to(index, value_of(bands, pos.x));
                }
                Some(Gesture::Move {
                    index,
                    lifted: lift(pos),
                })
            }
            Some(Gesture::Pull { index, .. }) => Some(Gesture::Pull {
                index,
                lifted: lift(pos),
            }),
            None => None,
        };
        if let Some(gesture) = gesture {
            ui.data_mut(|data| data.insert_temp(gesture_id, gesture));
        }
    }
    let lifted = match gesture {
        Some(Gesture::Move { index, lifted } | Gesture::Pull { index, lifted }) if lifted => {
            Some(index)
        }
        _ => None,
    };
    if resp.drag_stopped() {
        if let Some(index) = lifted
            && bands.remove(index)
        {
            *selected = None;
        }
        ui.data_mut(|data| data.remove::<Gesture>(gesture_id));
    }
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let xs = xs(bands);
        let on = if pos.y >= row.top() {
            Some(hit(&xs, pos.x, m.cap))
        } else {
            hit(&xs, pos.x, BAR_GRAB).map(Some)
        };
        match on {
            Some(Some(index)) => *selected = tap_band(bands, *selected, index),
            Some(None) => *selected = None,
            None => {
                if let Some(index) = bands.add(value_of(bands, pos.x)) {
                    *selected = Some(index);
                }
            }
        }
    }
    let lifted = lifted.filter(|_| resp.dragged());

    paint_bar(ui, bar, m.radius, bands, display, false, View::ALL);
    paint_ticks(ui, bar, bands, false, View::ALL);
    let painter = ui.painter();
    let visuals = ui.visuals();
    let (text, strong) = (visuals.text_color(), visuals.strong_text_color());
    for (index, stop) in bands.stops().iter().enumerate() {
        let x = stop_x(area, m, bands, stop.value);
        let on = *selected == Some(index);
        let tab = if on { m.tab_on } else { m.tab };
        let mut center = pos2(x, row.center().y);
        let mut alpha = 1.0;
        if lifted == Some(index)
            && let Some(pos) = resp.interact_pointer_pos()
        {
            center.y = pos.y - tab.y;
            alpha = 0.4;
        }
        let rect = Rect::from_center_size(center, tab);
        let stroke = handle_stroke(on, text, strong);
        paint_handle(painter, rect, stop_fill(bands, index), stroke, alpha);
    }

    labels(ui, area, m, bands, unit, View::ALL);
    if let Some(Gesture::Move { index, .. }) = gesture.filter(|_| resp.dragged()) {
        let value = bands.stops()[index].value;
        let x = stop_x(area, m, bands, value);
        value_bubble(
            ui,
            resp.id,
            pos2(x, bar.top() - BUBBLE_GAP),
            Align2::CENTER_BOTTOM,
            &world::value_text(value, span, unit),
        );
    }
    resp
}

/// The part of the range of the bands that a bar shows, as the fractions
/// of the whole bar at its two ends.
#[derive(Clone, Copy)]
struct View {
    lo: f64,
    hi: f64,
}

impl View {
    const ALL: View = View { lo: 0.0, hi: 1.0 };

    /// The view of the values from `lo` to `hi`.
    fn of(bands: &Bands, lo: f64, hi: f64) -> View {
        let (lo, hi) = (
            elevation::bar_fraction(bands, lo),
            elevation::bar_fraction(bands, hi),
        );
        if hi > lo { View { lo, hi } } else { View::ALL }
    }

    /// The place of `value` on the bar, from 0 to 1.
    fn fraction(self, bands: &Bands, value: f64) -> f64 {
        let f = elevation::bar_fraction(bands, value);
        ((f - self.lo) / (self.hi - self.lo)).clamp(0.0, 1.0)
    }

    /// The value at the place `fraction` of the bar.
    fn value(self, bands: &Bands, fraction: f64) -> f64 {
        elevation::bar_value(bands, self.lo + fraction * (self.hi - self.lo))
    }

    /// The values at the two ends of the bar.
    fn ends(self, bands: &Bands) -> (f64, f64) {
        (self.value(bands, 0.0), self.value(bands, 1.0))
    }
}

/// The row of the labels below the bar: the value at each end, and sea
/// level.
fn labels(ui: &mut egui::Ui, area: Rect, m: &Metrics, bands: &Bands, unit: &str, view: View) {
    let (labels, _) = ui.allocate_exact_size(vec2(area.width(), m.labels), Sense::hover());
    let text = ui.visuals().text_color();
    let (lo, hi) = view.ends(bands);
    let span = hi - lo;
    let bar = bar_rect(area, m);
    let inside = |v: f64| lo < v && v < hi;
    let sea = (bands.sea()).filter(|&i| inside(bands.stops()[i].value));
    let font = theme::font(12.0);
    let y = labels.center().y;
    let painter = ui.painter();
    let min = world::value_text(lo, span, unit);
    let max = world::value_text(hi, span, unit);
    painter.text(
        pos2(bar.left(), y),
        Align2::LEFT_CENTER,
        min,
        font.clone(),
        text,
    );
    painter.text(
        pos2(bar.right(), y),
        Align2::RIGHT_CENTER,
        max,
        font.clone(),
        text,
    );
    if let Some(index) = sea {
        let f = view.fraction(bands, bands.stops()[index].value) as f32;
        let x = bar.left() + f * bar.width();
        painter.text(pos2(x, y), Align2::CENTER_CENTER, "Sea level", font, text);
    }
}

/// The bar of the Style section: the colors and a line at each stop, with
/// the labels below it. With `shown`, the bar shows only those bands. A tap
/// on it gives a click on the response, which has the name `name`.
#[allow(clippy::too_many_arguments)]
pub fn strip(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    m: &Metrics,
    bands: &Bands,
    display: RasterDisplay,
    unit: &str,
    shown: Option<std::ops::Range<usize>>,
) -> Response {
    let top = ui.cursor().top();
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), m.bar), Sense::hover());
    let bar = bar_rect(area, m);
    let stops = bands.stops();
    let view = shown
        .filter(|r| r.start < r.end && r.end < stops.len())
        .map_or(View::ALL, |r| {
            View::of(bands, stops[r.start].value, stops[r.end].value)
        });
    paint_bar(ui, bar, m.radius, bands, display, false, view);
    paint_ticks(ui, bar, bands, false, view);
    labels(ui, area, m, bands, unit, view);
    let whole = Rect::from_min_max(
        pos2(area.left(), top),
        pos2(area.right(), ui.cursor().top()),
    );
    widgets::tap_area(ui, controls, whole, name, true)
}

/// The name of the row of a band. The lowest band is "Band 1".
pub fn band_row(index: usize) -> String {
    format!("Band {}", index + 1)
}

/// The band that a tap on the stop `index` selects: the band above the
/// stop, or no band if that band is selected. A tap on the stop at the top
/// does not change the selection.
fn tap_band(bands: &Bands, selected: Option<usize>, index: usize) -> Option<usize> {
    if index + 1 == bands.stops().len() {
        return selected;
    }
    (selected != Some(index)).then_some(index)
}

/// The value of the stop that a finger drags, in a bubble next to the bar,
/// so that the finger does not cover it. The point `align` of the bubble is
/// at `at`.
fn value_bubble(ui: &egui::Ui, id: egui::Id, at: Pos2, align: Align2, text: &str) {
    let layer = egui::LayerId::new(egui::Order::Tooltip, id.with("value"));
    let painter = ui.ctx().layer_painter(layer);
    let galley = painter.layout_no_wrap(text.to_owned(), theme::body(), theme::TEXT);
    let size = galley.size() + vec2(20.0, 10.0);
    let screen = ui.ctx().content_rect();
    let rect = align.anchor_size(at, size);
    let left = rect.left().clamp(screen.left(), screen.right() - size.x);
    let rect = Rect::from_min_size(pos2(left, rect.top()), size);
    let identity = egui::emath::TSTransform::IDENTITY;
    widgets::paint_card(&painter, layer.order, rect, theme::capsule(rect), identity);
    painter.galley(rect.center() - galley.size() / 2.0, galley, theme::TEXT);
}

/// The value at `fraction` of the bar, snapped to a step of the range.
pub fn snap_value(bands: &Bands, fraction: f64) -> f64 {
    let snap = (bands.max() - bands.min()) / SNAPS;
    let value = elevation::bar_value(bands, fraction);
    (value / snap).round() * snap
}

/// The value in the middle of the widest gap between two stops on the bar.
pub fn widest_gap_value(bands: &Bands) -> f64 {
    let stops = bands.stops().iter();
    let at: Vec<f64> = stops
        .map(|s| elevation::bar_fraction(bands, s.value))
        .collect();
    let gap = |w: &&[f64]| w[1] - w[0];
    let middle = at
        .windows(2)
        .max_by(|a, b| gap(a).total_cmp(&gap(b)))
        .map_or(0.5, |w| (w[0] + w[1]) / 2.0);
    snap_value(bands, middle)
}

/// The outline of a handle: strong at the lower edge of the selected band.
fn handle_stroke(on: bool, text: Color32, strong: Color32) -> egui::Stroke {
    if on {
        egui::Stroke::new(2.0, strong)
    } else {
        egui::Stroke::new(1.0, text.gamma_multiply(0.7))
    }
}

fn luminance(c: Rgb) -> f32 {
    0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2])
}

/// The bar in the colors of `display`, with round corners. A vertical bar
/// has the lowest value at the bottom.
fn paint_bar(
    ui: &egui::Ui,
    bar: Rect,
    radius: f32,
    bands: &Bands,
    display: RasterDisplay,
    vertical: bool,
    view: View,
) {
    let (len, across) = match vertical {
        true => (bar.height(), bar.width()),
        false => (bar.width(), bar.height()),
    };
    let r = radius.min(across / 2.0);
    // The place of a point that is `a` along the bar and `c` across it.
    let at = |a: f32, c: f32| match vertical {
        true => pos2(bar.left() + c, bar.bottom() - a),
        false => pos2(bar.left() + a, bar.top() + c),
    };
    // The two edges of the bar at `a`, inside the round corners.
    let edge = |a: f32| {
        let off = (r - a).max(a - (len - r)).max(0.0);
        let d = r - (r * r - off * off).max(0.0).sqrt();
        (d, across - d)
    };
    let a_of = |value: f64| view.fraction(bands, value) as f32 * len;
    let corners: Vec<f32> = (0..=6)
        .flat_map(|i| {
            let t = r * i as f32 / 6.0;
            [t, len - t]
        })
        .collect();
    let mut mesh = egui::Mesh::default();
    for band in bands.bands() {
        let (low_a, high_a) = (a_of(band.min), a_of(band.max));
        if high_a <= low_a {
            continue;
        }
        let mut cuts: Vec<f32> = corners
            .iter()
            .copied()
            .filter(|a| *a > low_a && *a < high_a)
            .collect();
        // The Gradient display has the color of the band at its middle.
        cuts.extend([low_a, a_of((band.min + band.max) / 2.0), high_a]);
        cuts.sort_by(f32::total_cmp);
        let color = |a: f32| {
            let value = view.value(bands, f64::from(a / len));
            elevation::band_color_at(bands, &band, display, value)
        };
        for pair in cuts.windows(2) {
            let first = mesh.vertices.len() as u32;
            for a in [pair[0], pair[1]] {
                let (near, far) = edge(a);
                mesh.colored_vertex(at(a, near), color(a));
                mesh.colored_vertex(at(a, far), color(a));
            }
            mesh.add_triangle(first, first + 1, first + 2);
            mesh.add_triangle(first + 1, first + 2, first + 3);
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    let outline = egui::Stroke::new(1.0, Color32::from_black_alpha(90));
    ui.painter()
        .rect_stroke(bar, r, outline, egui::StrokeKind::Outside);
}

/// A line across the bar at each stop between the two ends.
fn paint_ticks(ui: &egui::Ui, bar: Rect, bands: &Bands, vertical: bool, view: View) {
    let count = bands.stops().len();
    for stop in bands.stops().iter().skip(1).take(count.saturating_sub(2)) {
        let fraction = view.fraction(bands, stop.value) as f32;
        if fraction <= 0.0 || fraction >= 1.0 {
            continue;
        }
        let dark = luminance(bands.color_at(stop.value)) > 140.0;
        let tick = if dark {
            Color32::from_black_alpha(170)
        } else {
            Color32::from_white_alpha(210)
        };
        let line = match vertical {
            true => {
                let y = bar.bottom() - fraction * bar.height();
                [pos2(bar.left(), y), pos2(bar.right(), y)]
            }
            false => {
                let x = bar.left() + fraction * bar.width();
                [pos2(x, bar.top()), pos2(x, bar.bottom())]
            }
        };
        ui.painter()
            .line_segment(line, egui::Stroke::new(1.0, tick));
    }
}

/// The handle of a stop, or a swatch of a band, in the color `fill`.
fn paint_handle(painter: &egui::Painter, rect: Rect, fill: Rgb, stroke: egui::Stroke, alpha: f32) {
    let radius = 3.0;
    painter.rect_filled(rect, radius, color32(fill).gamma_multiply(alpha));
    let stroke = egui::Stroke::new(stroke.width, stroke.color.gamma_multiply(alpha));
    painter.rect_stroke(rect, radius, stroke, egui::StrokeKind::Outside);
}

/// The color of the band above a stop. The top stop has the color of the
/// band below it.
fn stop_fill(bands: &Bands, index: usize) -> Rgb {
    let colors = bands.colors();
    colors[index.min(colors.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_stop_takes_the_touch_up_to_the_cap() {
        let xs = [0.0, 30.0, 40.0, 200.0];
        assert_eq!(hit(&xs, 5.0, 22.0), Some(0));
        // Two near stops split the space at their midpoint.
        assert_eq!(hit(&xs, 34.0, 22.0), Some(1));
        assert_eq!(hit(&xs, 36.0, 22.0), Some(2));
        assert_eq!(hit(&xs, 62.0, 22.0), Some(2));
        assert_eq!(hit(&xs, 63.0, 22.0), None);
        assert_eq!(hit(&xs, 120.0, 22.0), None);
        assert_eq!(hit(&xs, 190.0, 10.0), Some(3));
        assert_eq!(hit(&[], 0.0, 22.0), None);
    }

    #[test]
    fn a_new_stop_goes_in_the_middle_of_the_widest_gap() {
        let mut bands = Bands::with_range(0.0, 1000.0, false).unwrap();
        while bands.stops().len() > 2 {
            bands.remove(1);
        }
        bands.add(800.0).unwrap();
        // The gap from 0 to 800 is the widest.
        assert!((widest_gap_value(&bands) - 400.0).abs() < 1.0);
        bands.add(300.0).unwrap();
        assert!((widest_gap_value(&bands) - 550.0).abs() < 1.0);
    }
}
