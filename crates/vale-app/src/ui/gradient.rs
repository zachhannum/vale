//! The gradient control of a raster layer: a bar in the colors of the
//! display, with a handle for each color stop below it.

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Response, Sense, Vec2, pos2, vec2};
use vale_store::RasterDisplay;
use vale_terrain::{Bands, Rgb, Stop};

use super::elevation::{self, SNAPS};
use super::pad::geometry::TOUCH;
use super::pad::theme;
use super::pad::widgets::{self, Controls, PANEL_ROW};
use crate::world;

/// The name of the control for a screen reader and for the tests.
pub const GRADIENT: &str = "Gradient";
/// The text below the control when no stop is selected.
pub const HINT: &str =
    "Tap the bar to add a stop. Drag a stop to move it, or pull it down to remove it.";
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
/// selects the stop, or clears the selection if the stop is selected. A
/// tap at another place on the bar adds a stop. A drag that
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
    let value_of =
        |bands: &Bands, x: f32| snap_value(bands, f64::from((x - bar.left()) / bar.width()));
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
        let toggle = |index: Option<usize>| index.filter(|i| *selected != Some(*i));
        if pos.y >= row.top() {
            *selected = toggle(hit(&xs, pos.x, m.cap));
        } else if let Some(index) = hit(&xs, pos.x, BAR_GRAB) {
            *selected = toggle(Some(index));
        } else if let Some(index) = bands.add(value_of(bands, pos.x)) {
            *selected = Some(index);
        }
    }
    let lifted = lifted.filter(|_| resp.dragged());

    paint_bar(ui, bar, m.radius, bands, display, false);
    paint_ticks(ui, bar, bands, false);
    let painter = ui.painter();
    let visuals = ui.visuals();
    let (text, strong) = (visuals.text_color(), visuals.strong_text_color());
    let sea = bands.sea();
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
        paint_handle(
            painter,
            rect,
            stop,
            Some(index) == sea,
            false,
            stroke,
            alpha,
        );
    }

    labels(ui, area, m, bands, unit);
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

/// The row of the labels below the bar: the lowest value, the highest
/// value, and sea level.
fn labels(ui: &mut egui::Ui, area: Rect, m: &Metrics, bands: &Bands, unit: &str) {
    let (labels, _) = ui.allocate_exact_size(vec2(area.width(), m.labels), Sense::hover());
    let text = ui.visuals().text_color();
    let span = bands.max() - bands.min();
    let bar = bar_rect(area, m);
    let sea = bands.sea();
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
}

/// The bar of the Style section: the colors and a line at each stop, with
/// the labels below it. A tap on it gives a click on the response, which
/// has the name `name`.
pub fn strip(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    m: &Metrics,
    bands: &Bands,
    display: RasterDisplay,
    unit: &str,
) -> Response {
    let top = ui.cursor().top();
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), m.bar), Sense::hover());
    let bar = bar_rect(area, m);
    paint_bar(ui, bar, m.radius, bands, display, false);
    paint_ticks(ui, bar, bands, false);
    labels(ui, area, m, bands, unit);
    let whole = Rect::from_min_max(
        pos2(area.left(), top),
        pos2(area.right(), ui.cursor().top()),
    );
    widgets::tap_area(ui, controls, whole, name, true)
}

/// The sizes of the control of the page of the color stops.
pub struct Column {
    pub bar: f32,
    pub radius: f32,
    /// The size of a handle across and along the bar.
    pub tab: Vec2,
    pub tab_on: Vec2,
    /// The width of the space of the leader lines.
    pub gutter: f32,
    /// The height of the row of a stop.
    pub row: f32,
    /// A touch at a handle this near to a stop takes the stop.
    pub cap: f32,
    /// The size of the color of a stop in its row.
    pub swatch: f32,
}

pub const COLUMN: Column = Column {
    bar: 28.0,
    radius: 6.0,
    tab: vec2(18.0, 12.0),
    tab_on: vec2(22.0, 18.0),
    gutter: 24.0,
    row: PANEL_ROW,
    cap: TOUCH / 2.0,
    swatch: 24.0,
};

/// The parts of the control of the page of the color stops, from the left:
/// the bar, the handles, the leader lines, and the rows.
#[derive(Clone, Copy, Debug)]
pub struct ColumnParts {
    pub bar: Rect,
    pub handles: Rect,
    pub rows: Rect,
}

impl ColumnParts {
    /// The parts in `area`. The ends of the bar are at the middle of the
    /// first row and of the last row.
    pub fn new(area: Rect, m: &Column) -> ColumnParts {
        let ends = m.row / 2.0;
        let bar = Rect::from_min_max(
            pos2(area.left(), area.top() + ends),
            pos2(area.left() + m.bar, area.bottom() - ends),
        );
        let handles = Rect::from_min_max(
            pos2(bar.right(), area.top()),
            pos2(bar.right() + m.tab_on.x + 6.0, area.bottom()),
        );
        let rows = Rect::from_min_max(pos2(handles.right() + m.gutter, area.top()), area.max);
        ColumnParts { bar, handles, rows }
    }

    /// The screen height of a value on the bar.
    pub fn stop_y(&self, bands: &Bands, value: f64) -> f32 {
        let fraction = elevation::bar_fraction(bands, value) as f32;
        self.bar.bottom() - fraction * self.bar.height()
    }

    /// The middle of the handle of a value.
    pub fn handle(&self, bands: &Bands, value: f64) -> Pos2 {
        pos2(self.handles.center().x, self.stop_y(bands, value))
    }

    /// The row of the stop `index` of `count`. The row of the highest stop
    /// is at the top.
    pub fn row(&self, m: &Column, count: usize, index: usize) -> Rect {
        let top = self.rows.top() + (count - 1 - index) as f32 * m.row;
        Rect::from_min_size(pos2(self.rows.left(), top), vec2(self.rows.width(), m.row))
    }
}

/// The name of the row of a stop. The lowest stop is "Stop 1".
pub fn stop_row(index: usize) -> String {
    format!("Stop {}", index + 1)
}

/// The control of the page of the color stops: a vertical bar with a
/// handle for each stop, and a row for each stop at the right of it, with
/// the highest value at the top. A leader line joins each handle to its
/// row. A tap on a row or a handle selects the stop, and a drag on a handle
/// moves it. A tap on the bar away from the handles adds a stop. Each
/// other drag scrolls the panel. The response is that of the bar and the
/// handles.
pub fn column(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    m: &Column,
    bands: &mut Bands,
    selected: &mut Option<usize>,
    display: RasterDisplay,
    unit: &str,
) -> (Response, ColumnParts) {
    let count = bands.stops().len();
    let size = vec2(ui.available_width(), m.row * count as f32);
    let (area, _) = ui.allocate_exact_size(size, Sense::hover());
    let parts = ColumnParts::new(area, m);
    let grab = parts.bar.union(parts.handles);
    let resp = widgets::drag_area(ui, controls, grab, GRADIENT);
    if selected.is_some_and(|i| i >= count) {
        *selected = None;
    }
    let toggle =
        |selected: &Option<usize>, index: Option<usize>| index.filter(|i| *selected != Some(*i));
    let mut rows = Vec::with_capacity(count);
    for index in 0..count {
        let rect = parts.row(m, count, index);
        let row = widgets::tap_area(ui, controls, rect, &stop_row(index), true);
        if row.clicked() {
            *selected = toggle(selected, Some(index));
        }
        rows.push((rect, row.is_pointer_button_down_on()));
    }

    let bar = parts.bar;
    let value_of =
        |bands: &Bands, y: f32| snap_value(bands, f64::from((bar.bottom() - y) / bar.height()));
    let ys = |bands: &Bands| -> Vec<f32> {
        let stops = bands.stops().iter();
        stops.map(|s| parts.stop_y(bands, s.value)).collect()
    };
    let gesture_id = resp.id.with("gesture");
    if resp.drag_started() {
        let from = ui.input(|i| i.pointer.press_origin());
        let on = from
            .filter(|p| p.x >= parts.handles.left())
            .and_then(|p| hit(&ys(bands), p.y, m.cap))
            .filter(|i| !bands.fixed(*i));
        let gesture = match on {
            Some(index) => {
                *selected = Some(index);
                Gesture::Move {
                    index,
                    lifted: false,
                }
            }
            None => Gesture::Scroll,
        };
        ui.data_mut(|data| data.insert_temp(gesture_id, gesture));
    }
    let gesture: Option<Gesture> = ui.data(|data| data.get_temp(gesture_id));
    let mut moving = None;
    if resp.dragged() {
        match gesture {
            Some(Gesture::Move { index, .. }) => {
                if let Some(pos) = resp.interact_pointer_pos() {
                    bands.move_to(index, value_of(bands, pos.y));
                }
                moving = Some(index);
            }
            _ => ui.scroll_with_delta(vec2(0.0, resp.drag_delta().y)),
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|data| data.remove::<Gesture>(gesture_id));
    }
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let ys = ys(bands);
        if pos.x >= parts.handles.left() {
            *selected = toggle(selected, hit(&ys, pos.y, m.cap));
        } else if let Some(index) = hit(&ys, pos.y, BAR_GRAB) {
            *selected = toggle(selected, Some(index));
        } else if let Some(index) = bands.add(value_of(bands, pos.y)) {
            *selected = Some(index);
        }
    }

    paint_bar(ui, bar, m.radius, bands, display, true);
    paint_ticks(ui, bar, bands, true);
    let painter = ui.painter();
    let visuals = ui.visuals();
    let (text, strong) = (visuals.text_color(), visuals.strong_text_color());
    let sea = bands.sea();
    let span = bands.max() - bands.min();
    // A stop that `add` put in this frame has no row yet.
    for (index, stop) in bands.stops().iter().enumerate().take(rows.len()) {
        let on = *selected == Some(index);
        let (row, down) = rows[index];
        if on || down {
            let back = row.shrink2(vec2(0.0, widgets::HIGHLIGHT_INSET));
            painter.rect_filled(back, theme::CONTROL_RADIUS, theme::raise());
        }
        let handle = parts.handle(bands, stop.value);
        let tab = if on { m.tab_on } else { m.tab };
        let rect = Rect::from_center_size(handle, tab);
        let stroke = handle_stroke(on, text, strong);
        let is_sea = Some(index) == sea;
        paint_handle(painter, rect, stop, is_sea, true, stroke, 1.0);

        let leader = egui::Stroke::new(1.0, if on { strong } else { text.gamma_multiply(0.5) });
        let (from, to) = (
            pos2(rect.right(), handle.y),
            pos2(row.left(), row.center().y),
        );
        let bend = m.gutter / 4.0;
        let line = vec![
            from,
            pos2(parts.handles.right(), from.y),
            pos2(to.x - bend, to.y),
            pos2(to.x + bend, to.y),
        ];
        painter.add(egui::Shape::line(line, leader));

        let swatch = Rect::from_center_size(
            pos2(row.left() + bend + 6.0 + m.swatch / 2.0, row.center().y),
            Vec2::splat(m.swatch),
        );
        let outline = egui::Stroke::new(1.0, Color32::from_black_alpha(90));
        paint_handle(painter, swatch, stop, is_sea, true, outline, 1.0);
        let at = pos2(swatch.right() + 12.0, row.center().y);
        let value = world::value_text(stop.value, span, unit);
        let label = painter.text(at, Align2::LEFT_CENTER, value, theme::body(), theme::TEXT);
        if is_sea {
            let at = pos2(label.right() + 8.0, row.center().y);
            painter.text(
                at,
                Align2::LEFT_CENTER,
                "Sea level",
                theme::small(),
                theme::MUTE,
            );
        }
        if let Some(color) = bands.line_color(index).filter(|_| stop.line.on) {
            let y = row.center().y;
            let right = row.right() - 8.0;
            let width = stop.line.weight.clamp(1.0, 6.0);
            let line = [pos2(right - 32.0, y), pos2(right, y)];
            painter.line_segment(line, egui::Stroke::new(width, color32(color)));
        }
    }
    if let Some(index) = moving {
        let y = parts.stop_y(bands, bands.stops()[index].value);
        value_bubble(
            ui,
            resp.id,
            pos2(bar.left() - BUBBLE_GAP, y),
            Align2::RIGHT_CENTER,
            &world::value_text(bands.stops()[index].value, span, unit),
        );
    }
    (resp, parts)
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
    painter.add(theme::shadow().as_shape(rect, theme::CONTROL_RADIUS));
    painter.rect_filled(rect, theme::CONTROL_RADIUS, theme::card(false).to_opaque());
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

/// The outline of a handle: strong on the selected stop.
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
    let a_of = |value: f64| elevation::bar_fraction(bands, value) as f32 * len;
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
        let (high, low) = elevation::band_colors(bands, &band, display);
        let mut cuts: Vec<f32> = corners
            .iter()
            .copied()
            .filter(|a| *a > low_a && *a < high_a)
            .collect();
        cuts.push(low_a);
        cuts.push(high_a);
        cuts.sort_by(f32::total_cmp);
        let color = |a: f32| lerp(low, high, (a - low_a) / (high_a - low_a));
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
fn paint_ticks(ui: &egui::Ui, bar: Rect, bands: &Bands, vertical: bool) {
    let count = bands.stops().len();
    for stop in bands.stops().iter().skip(1).take(count.saturating_sub(2)) {
        let dark = luminance(bands.color_at(stop.value)) > 140.0;
        let tick = if dark {
            Color32::from_black_alpha(170)
        } else {
            Color32::from_white_alpha(210)
        };
        let fraction = elevation::bar_fraction(bands, stop.value) as f32;
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

/// The handle of a stop, or a swatch of its color. The handle of the stop
/// at sea level shows the sea color below the stop and the land color
/// above it: at the left and the right of a horizontal handle, and at the
/// bottom and the top of a vertical one.
fn paint_handle(
    painter: &egui::Painter,
    rect: Rect,
    stop: &Stop,
    sea: bool,
    vertical: bool,
    stroke: egui::Stroke,
    alpha: f32,
) {
    let radius = 3.0;
    let fill = |c: Rgb| color32(c).gamma_multiply(alpha);
    if sea {
        let r = radius as u8;
        let (low, high, low_corners, high_corners) = match vertical {
            true => {
                let (top, bottom) = rect.split_top_bottom_at_y(rect.center().y);
                let top_corners = egui::CornerRadius {
                    nw: r,
                    ne: r,
                    sw: 0,
                    se: 0,
                };
                let bottom_corners = egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: r,
                    se: r,
                };
                (bottom, top, bottom_corners, top_corners)
            }
            false => {
                let (left, right) = rect.split_left_right_at_x(rect.center().x);
                let left_corners = egui::CornerRadius {
                    nw: r,
                    sw: r,
                    ne: 0,
                    se: 0,
                };
                let right_corners = egui::CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: r,
                    se: r,
                };
                (left, right, left_corners, right_corners)
            }
        };
        painter.rect_filled(low, low_corners, fill(stop.below));
        painter.rect_filled(high, high_corners, fill(stop.above));
    } else {
        painter.rect_filled(rect, radius, fill(stop.above));
    }
    let stroke = egui::Stroke::new(stroke.width, stroke.color.gamma_multiply(alpha));
    painter.rect_stroke(rect, radius, stroke, egui::StrokeKind::Outside);
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

    #[test]
    fn the_rows_go_from_the_highest_stop_down_and_meet_the_ends_of_the_bar() {
        let bands = Bands::with_range(-6000.0, 6000.0, true).unwrap();
        let m = &COLUMN;
        let count = bands.stops().len();
        let area = Rect::from_min_size(pos2(0.0, 100.0), vec2(300.0, m.row * count as f32));
        let parts = ColumnParts::new(area, m);
        let top = parts.row(m, count, count - 1);
        let bottom = parts.row(m, count, 0);
        assert_eq!(top.top(), area.top());
        assert_eq!(bottom.bottom(), area.bottom());
        assert_eq!(top.height(), m.row);
        let (lowest, highest) = (bands.stops()[0].value, bands.stops()[count - 1].value);
        assert_eq!(parts.stop_y(&bands, highest), top.center().y);
        assert_eq!(parts.stop_y(&bands, lowest), bottom.center().y);
        // The handles go up as the values go up.
        let ys: Vec<f32> = bands
            .stops()
            .iter()
            .map(|s| parts.stop_y(&bands, s.value))
            .collect();
        assert!(ys.windows(2).all(|w| w[1] < w[0]));
        assert!(parts.bar.right() <= parts.handles.left());
        assert!(parts.handles.right() < parts.rows.left());
    }
}
