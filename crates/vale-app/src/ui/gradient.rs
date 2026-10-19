//! The gradient control of a raster layer: a bar in the colors of the
//! display, with a handle for each color stop below it.

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Response, Sense, Vec2, pos2, vec2};
use vale_store::RasterDisplay;
use vale_terrain::{Bands, Rgb};

use super::elevation::{self, SNAPS};
use super::pad::geometry::TOUCH;
use super::pad::widgets::{self, Controls};
use crate::world;

/// The name of the control for a screen reader and for the tests.
pub const GRADIENT: &str = "Color stops";
/// The text below the control when no stop is selected.
pub const HINT: &str =
    "Tap the bar to add a stop. Drag a stop to move it, or pull it down to remove it.";
/// A finger this far below the handle row removes the stop on release.
pub const PULL: f32 = 36.0;
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

/// What a drag on the control does. It is set when the drag starts.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gesture {
    /// Moves a stop. `lifted`: the finger is in the place that removes it.
    Move {
        index: usize,
        lifted: bool,
    },
    /// Pulls the selected stop down to remove it.
    Pull {
        index: usize,
        lifted: bool,
    },
    Scroll,
}

fn color32(c: Rgb) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// The gradient control. A tap on a handle or near a stop on the bar
/// selects the stop, and another tap on the bar adds a stop. A drag that
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
        Some(controls) => widgets::drag_area(ui, controls, area, GRADIENT),
        None => {
            let resp = ui.interact(area, ui.id().with(GRADIENT), Sense::click_and_drag());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, GRADIENT));
            resp
        }
    };
    if selected.is_some_and(|i| i >= bands.stops().len()) {
        *selected = None;
    }
    let bar = bar_rect(area, m);
    let row = handle_row(area, m);
    let span = bands.max() - bands.min();
    let snap = span / SNAPS;
    let value_of = |bands: &Bands, x: f32| {
        let value = elevation::bar_value(bands, f64::from((x - bar.left()) / bar.width()));
        (value / snap).round() * snap
    };
    let xs = |bands: &Bands| -> Vec<f32> {
        let stops = bands.stops().iter();
        stops.map(|s| stop_x(area, m, bands, s.value)).collect()
    };

    let gesture_id = resp.id.with("gesture");
    if resp.drag_started() {
        let pointer = ui.input(|i| i.pointer.press_origin().zip(i.pointer.interact_pos()));
        let gesture = pointer.map_or(Gesture::Scroll, |(from, to)| {
            let moved = to - from;
            let on = (from.y >= row.top())
                .then(|| hit(&xs(bands), from.x, m.cap))
                .flatten()
                .filter(|i| !bands.fixed(*i));
            match on {
                Some(index) if moved.x.abs() >= moved.y.abs() => {
                    *selected = Some(index);
                    Gesture::Move {
                        index,
                        lifted: false,
                    }
                }
                Some(index) if moved.y > 0.0 && *selected == Some(index) => Gesture::Pull {
                    index,
                    lifted: false,
                },
                _ => Gesture::Scroll,
            }
        });
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
            _ => {
                ui.scroll_with_delta(vec2(0.0, resp.drag_delta().y));
                Some(Gesture::Scroll)
            }
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
        if pos.y >= row.top() {
            *selected = hit(&xs, pos.x, m.cap);
        } else if let Some(index) = hit(&xs, pos.x, BAR_GRAB) {
            *selected = Some(index);
        } else if let Some(index) = bands.add(value_of(bands, pos.x)) {
            *selected = Some(index);
        }
    }
    let lifted = lifted.filter(|_| resp.dragged());

    paint_bar(ui, bar, m, bands, display);
    let painter = ui.painter();
    let visuals = ui.visuals();
    let (text, strong) = (visuals.text_color(), visuals.strong_text_color());
    let sea = bands.sea();
    let count = bands.stops().len();
    for (index, stop) in bands.stops().iter().enumerate() {
        let x = stop_x(area, m, bands, stop.value);
        let on = *selected == Some(index);
        if index != 0 && index + 1 != count {
            let dark = luminance(bands.color_at(stop.value)) > 140.0;
            let tick = if dark {
                Color32::from_black_alpha(170)
            } else {
                Color32::from_white_alpha(210)
            };
            let line = [pos2(x, bar.top()), pos2(x, bar.bottom())];
            painter.line_segment(line, egui::Stroke::new(1.0, tick));
        }
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
        let radius = 3.0;
        if Some(index) == sea {
            let (left, right) = rect.split_left_right_at_x(x);
            let fill = |c: Rgb| color32(c).gamma_multiply(alpha);
            let corners = |l: bool| {
                let r = radius as u8;
                match l {
                    true => egui::CornerRadius {
                        nw: r,
                        sw: r,
                        ne: 0,
                        se: 0,
                    },
                    false => egui::CornerRadius {
                        nw: 0,
                        sw: 0,
                        ne: r,
                        se: r,
                    },
                }
            };
            painter.rect_filled(left, corners(true), fill(stop.below));
            painter.rect_filled(right, corners(false), fill(stop.above));
        } else {
            painter.rect_filled(rect, radius, color32(stop.above).gamma_multiply(alpha));
        }
        let stroke = if on {
            egui::Stroke::new(2.0, strong)
        } else {
            egui::Stroke::new(1.0, text.gamma_multiply(0.7))
        };
        let stroke = egui::Stroke::new(stroke.width, stroke.color.gamma_multiply(alpha));
        painter.rect_stroke(rect, radius, stroke, egui::StrokeKind::Outside);
    }

    let (labels, _) = ui.allocate_exact_size(vec2(area.width(), m.labels), Sense::hover());
    let font = egui::FontId::proportional(12.0);
    let y = labels.center().y;
    let painter = ui.painter();
    let min = world::value_text(bands.min(), span, unit);
    let max = world::value_text(bands.max(), span, unit);
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
        let x = stop_x(area, m, bands, bands.stops()[index].value);
        painter.text(pos2(x, y), Align2::CENTER_CENTER, "Sea level", font, text);
    }
    resp
}

fn luminance(c: Rgb) -> f32 {
    0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2])
}

/// The bar in the colors of `display`, with round corners.
fn paint_bar(ui: &egui::Ui, bar: Rect, m: &Metrics, bands: &Bands, display: RasterDisplay) {
    let r = m.radius.min(bar.height() / 2.0);
    // The top and the bottom of the bar at `x`, inside the round corners.
    let edge = |x: f32| {
        let off = (bar.left() + r - x).max(x - (bar.right() - r)).max(0.0);
        let dy = r - (r * r - off * off).max(0.0).sqrt();
        (bar.top() + dy, bar.bottom() - dy)
    };
    let x_of = |value: f64| bar.left() + elevation::bar_fraction(bands, value) as f32 * bar.width();
    let corners: Vec<f32> = (0..=6)
        .flat_map(|i| {
            let t = r * i as f32 / 6.0;
            [bar.left() + t, bar.right() - t]
        })
        .collect();
    let mut mesh = egui::Mesh::default();
    for band in bands.bands() {
        let (left, right) = (x_of(band.min), x_of(band.max));
        if right <= left {
            continue;
        }
        let (high, low) = elevation::band_colors(bands, &band, display);
        let mut cuts: Vec<f32> = corners
            .iter()
            .copied()
            .filter(|x| *x > left && *x < right)
            .collect();
        cuts.push(left);
        cuts.push(right);
        cuts.sort_by(f32::total_cmp);
        let color = |x: f32| lerp(low, high, (x - left) / (right - left));
        for pair in cuts.windows(2) {
            let at = mesh.vertices.len() as u32;
            for x in [pair[0], pair[1]] {
                let (top, bottom) = edge(x);
                mesh.colored_vertex(pos2(x, top), color(x));
                mesh.colored_vertex(pos2(x, bottom), color(x));
            }
            mesh.add_triangle(at, at + 1, at + 2);
            mesh.add_triangle(at + 1, at + 2, at + 3);
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    let outline = egui::Stroke::new(1.0, Color32::from_black_alpha(90));
    ui.painter()
        .rect_stroke(bar, r, outline, egui::StrokeKind::Outside);
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let c = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(c(a.r(), b.r()), c(a.g(), b.g()), c(a.b(), b.b()))
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
}
