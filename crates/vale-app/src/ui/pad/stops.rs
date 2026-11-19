//! The page of the color stops: a histogram of the area at each value, and
//! a list of the bands, as in a map legend.

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, pos2, vec2};
use vale_store::RasterDisplay;
use vale_terrain::{Bands, MAX_BANDS, MIN_BAND};

use super::icons::{self, Icon};
use super::theme;
use super::widgets::{self, Controls, PANEL_ROW};
use crate::ui::elevation::{self, SNAPS};
use crate::ui::gradient::{self, band_row};
use crate::world;

/// The name of the histogram for a screen reader and for the tests.
pub const HISTOGRAM: &str = "Area by value";
/// The button of the selected row that adds a stop in the middle of the
/// band.
pub const ADD_STOP: &str = "Add stop";
/// The height of the columns of the histogram, and of its labels.
const BARS: f32 = 64.0;
const LABELS: f32 = 16.0;
/// The width of a column of the histogram.
const COLUMN: f32 = 2.0;
/// The width of the color cells of the rows.
const CELL: f32 = 40.0;
/// The space between the cells and the value of a stop.
const PILL_GAP: f32 = 8.0;
const PILL_HEIGHT: f32 = 24.0;
const PILL_PAD: f32 = 8.0;
/// The space at the right of the share of a band.
const SHARE_PAD: f32 = 6.0;
const SHARE_WIDTH: f32 = 34.0;
const ADD: f32 = 44.0;
/// The fill of the value of a stop.
const PILL: Color32 = Color32::from_rgb(48, 53, 60);

/// The name of the value of a stop.
pub fn stop_name(index: usize) -> String {
    format!("Stop {index}")
}

/// The place of a stop on the bar after a vertical drag of `d` points, up
/// positive, from the place `f0`. Near the start, a point is one step of
/// the snap. The step grows with the distance.
pub fn scrub_fraction(f0: f64, d: f64) -> f64 {
    f0 + d / SNAPS + d.signum() * (d / 240.0).powi(2)
}

/// The value of the new stop of the band `band`: the middle of the band,
/// snapped to a step of the range, unless the step is too near a stop.
pub fn middle(bands: &Bands, band: usize) -> f64 {
    let stops = bands.stops();
    let (lo, hi) = (stops[band].value, stops[band + 1].value);
    let mid = (lo + hi) / 2.0;
    let snap = (bands.max() - bands.min()) / SNAPS;
    let snapped = (mid / snap).round() * snap;
    if snapped - lo >= MIN_BAND && hi - snapped >= MIN_BAND {
        snapped
    } else {
        mid
    }
}

/// The parts of the list of the bands in this frame.
pub struct Legend {
    pub rows: Rect,
    /// The row of each band, from the lowest band.
    pub row_of: Vec<Rect>,
    /// The stop that a finger drags.
    pub scrub: Option<usize>,
}

/// A drag on the value of a stop: the stop, its place on the bar at the
/// start, and the height of the start.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Scrub {
    index: usize,
    f0: f64,
    y0: f32,
}

fn color32(c: vale_terrain::Rgb) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// The color of the band `band` in its row: the color at its middle.
fn cell_color(bands: &Bands, band: &vale_terrain::Band, display: RasterDisplay) -> Color32 {
    elevation::band_color_at(bands, band, display, (band.min + band.max) / 2.0)
}

/// Takes the space of the histogram, and the line below it. A tap or a drag
/// on it does nothing. `paint_histogram` paints it.
pub fn histogram(ui: &mut egui::Ui) -> Rect {
    let size = vec2(ui.available_width(), BARS + LABELS);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, HISTOGRAM));
    ui.add_space(8.0);
    let (line, _) = ui.allocate_exact_size(vec2(size.x, 1.0), Sense::hover());
    ui.painter().rect_filled(line, 0.0, theme::divider());
    ui.add_space(6.0);
    rect
}

/// The histogram in `rect`: the area at each value, in the color of the
/// band of the value, with the lowest value, sea level, and the highest
/// value below it. While a band is selected, the other bands are dim.
/// `marker`: the value of a stop that a finger drags.
#[allow(clippy::too_many_arguments)]
pub fn paint_histogram(
    ui: &egui::Ui,
    rect: Rect,
    bins: Option<&[f64]>,
    bands: &Bands,
    display: RasterDisplay,
    unit: &str,
    selected: Option<usize>,
    marker: Option<f64>,
) {
    let painter = ui.painter();
    let bars = Rect::from_min_size(rect.min, vec2(rect.width(), BARS));
    let (min, max) = (bands.min(), bands.max());
    let x_of = |v: f64| bars.left() + ((v - min) / (max - min)) as f32 * bars.width();
    if let Some(bins) = bins {
        let count = (bars.width() / COLUMN).floor().max(1.0) as usize;
        let cols = crate::areas::columns(bins, count);
        // The values out of the range of a layer sit at its two ends, so the
        // columns at the ends do not set the scale.
        let inner = cols.get(1..count.saturating_sub(1)).unwrap_or(&[]);
        let most = match inner.iter().copied().fold(0.0, f64::max) {
            0.0 => cols.iter().copied().fold(0.0, f64::max),
            most => most,
        };
        let list = bands.bands();
        let width = bars.width() / count as f32;
        for (c, &area) in cols.iter().enumerate() {
            if area <= 0.0 || most <= 0.0 {
                continue;
            }
            let v = min + (c as f64 + 0.5) / count as f64 * (max - min);
            let band = bands.stops().partition_point(|s| s.value <= v);
            let band = band.clamp(1, list.len()) - 1;
            let mut color = cell_color(bands, &list[band], display);
            if selected.is_some_and(|s| s != band) {
                color = color.gamma_multiply(0.25);
            }
            let h = ((area / most).min(1.0).sqrt() as f32 * (BARS - 4.0)).max(2.0);
            let left = bars.left() + c as f32 * width;
            let col = Rect::from_min_max(
                pos2(left, bars.bottom() - h),
                pos2(left + width + 0.3, bars.bottom()),
            );
            painter.rect_filled(col, 0.0, color);
        }
    }
    if let Some(v) = marker {
        let x = x_of(v);
        let line = [pos2(x, bars.top()), pos2(x, bars.bottom() + 3.0)];
        painter.line_segment(line, egui::Stroke::new(2.0, theme::ACCENT));
    }
    let span = max - min;
    let y = bars.bottom() + LABELS / 2.0 + 2.0;
    let font = theme::font(11.0);
    let text = |at: Pos2, align: Align2, s: String| {
        painter.text(at, align, s, font.clone(), theme::MUTE);
    };
    text(
        pos2(rect.left(), y),
        Align2::LEFT_CENTER,
        world::value_text(min, span, unit),
    );
    text(
        pos2(rect.right(), y),
        Align2::RIGHT_CENTER,
        world::value_text(max, span, unit),
    );
    if bands.sea().is_some() {
        text(pos2(x_of(0.0), y), Align2::CENTER_CENTER, "0".to_string());
    }
}

/// The list of the bands, with the highest band at the top. The color
/// cells touch, and the value of each stop between the two ends is on the
/// seam between its two bands. A row shows the name of its band and its
/// share of the area of the world, from `bins`, the area of each level bin. `selected` is a band. A tap on
/// a row or on the value of the stop at its lower edge selects the band, or
/// clears the selection if the band is selected. A drag on the value of a
/// stop that moves changes the value, and each other drag scrolls the
/// panel. The selected row has a button that adds a stop in the middle of
/// the band.
pub fn legend(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    bands: &mut Bands,
    selected: &mut Option<usize>,
    display: RasterDisplay,
    unit: &str,
    bins: Option<&[f64]>,
) -> Legend {
    let count = bands.colors().len();
    let size = vec2(ui.available_width(), PANEL_ROW * count as f32);
    let (rows, _) = ui.allocate_exact_size(size, Sense::hover());
    if selected.is_some_and(|i| i >= count) {
        *selected = None;
    }
    let row_rect = |i: usize| {
        let top = rows.top() + (count - 1 - i) as f32 * PANEL_ROW;
        Rect::from_min_size(pos2(rows.left(), top), vec2(rows.width(), PANEL_ROW))
    };
    let row_of: Vec<Rect> = (0..count).map(row_rect).collect();
    let toggle = |selected: Option<usize>, i: usize| (selected != Some(i)).then_some(i);

    let mut down = vec![false; count];
    for (i, rect) in row_of.iter().enumerate() {
        let row = widgets::tap_area(ui, controls, *rect, &band_row(i), true);
        if row.clicked() {
            *selected = toggle(*selected, i);
        }
        down[i] = row.is_pointer_button_down_on();
    }

    // The button of the selected row.
    let add_rect = |row: Rect| {
        let right = row.right() - SHARE_PAD - SHARE_WIDTH - 4.0;
        Rect::from_center_size(pos2(right - ADD / 2.0, row.center().y), vec2(ADD, ADD))
    };
    let full = count >= MAX_BANDS;
    let mut add = None;
    if let Some(band) = selected.filter(|_| !full) {
        let value = middle(bands, band);
        let can = bands.clone().add(value).is_some();
        let resp = widgets::icon_button_at(
            ui,
            controls,
            add_rect(row_of[band]),
            ADD_STOP,
            Icon::Plus,
            false,
            can,
        );
        add = Some((band, resp.rect));
        if resp.clicked()
            && let Some(index) = bands.add(value)
        {
            *selected = Some(index);
        }
    }

    // The values of the stops. A stop has the index of the band above it.
    let span = bands.max() - bands.min();
    let pill_font = theme::font(14.0);
    let pill_width = |ui: &egui::Ui, value: f64, scrub: bool| {
        let text = world::value_text(value, span, unit);
        let galley = ui
            .painter()
            .layout_no_wrap(text, pill_font.clone(), theme::TEXT);
        let icon = if scrub { 18.0 } else { 0.0 };
        galley.size().x + 2.0 * PILL_PAD + icon
    };
    let seam = |i: usize| row_rect(i).bottom();
    let pill_left = rows.left() + CELL + PILL_GAP;
    let scrub_id = ui.id().with("stops-scrub");
    let mut scrub: Option<Scrub> = ui.data(|d| d.get_temp(scrub_id));
    let origin = ui.input(|i| i.pointer.press_origin());
    let mut dragging = false;
    let inner = bands.stops().len().saturating_sub(1);
    for i in 1..inner.min(count) {
        let value = bands.stops()[i].value;
        let w = pill_width(ui, value, false);
        let hit = Rect::from_center_size(
            pos2(pill_left + w / 2.0, seam(i)),
            vec2((w + 16.0).max(44.0), 44.0),
        );
        let resp = widgets::tap_area(ui, controls, hit, &stop_name(i), true);
        if resp.clicked() {
            *selected = toggle(*selected, i);
        }
        if bands.fixed(i) {
            continue;
        }
        let taken = widgets::take_drag(ui, &resp, |_| true);
        if taken && let Some(origin) = origin {
            let f0 = elevation::bar_fraction(bands, value);
            scrub = Some(Scrub {
                index: i,
                f0,
                y0: origin.y,
            });
            ui.data_mut(|d| d.insert_temp(scrub_id, scrub.unwrap()));
        }
        let ours = scrub.is_some_and(|s| s.index == i);
        if ours && (taken || resp.dragged()) {
            dragging = true;
            let s = scrub.unwrap();
            if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
                let f = scrub_fraction(s.f0, f64::from(s.y0 - pos.y));
                bands.move_to(i, gradient::snap_value(bands, f));
            }
        }
        if ours && resp.drag_stopped() {
            ui.data_mut(|d| d.remove::<Scrub>(scrub_id));
        }
    }
    let scrub = scrub.filter(|_| dragging).map(|s| s.index);
    if scrub.is_none() && !ui.input(|i| i.pointer.primary_down()) {
        ui.data_mut(|d| d.remove::<Scrub>(scrub_id));
    }

    // The paint.
    let painter = ui.painter().clone();
    let list = bands.bands();
    let count = list.len().min(row_of.len());
    for i in 0..count {
        let row = row_of[i];
        if *selected == Some(i) || down[i] {
            painter.rect_filled(row, theme::CONTROL_RADIUS, theme::raise());
        }
        let cell = Rect::from_min_size(row.min, vec2(CELL, row.height()));
        let r = 8;
        let radius = CornerRadius {
            nw: if i + 1 == count { r } else { 0 },
            ne: if i + 1 == count { r } else { 0 },
            sw: if i == 0 { r } else { 0 },
            se: if i == 0 { r } else { 0 },
        };
        painter.rect_filled(cell, radius, cell_color(bands, &list[i], display));
    }
    let areas = bins.map(|bins| crate::areas::band_areas(bins, bands));
    let areas = areas.as_deref();
    let total: f64 = areas.map_or(0.0, |a| a.iter().sum());
    for (i, &row) in row_of.iter().enumerate().take(count) {
        let name = bands.name(i);
        let right = match add {
            Some((band, rect)) if band == i => rect.left() - 4.0,
            _ => row.right() - SHARE_PAD - SHARE_WIDTH - 4.0,
        };
        if !name.is_empty() {
            let left = pill_left + PILL_PAD;
            let mut job = egui::text::LayoutJob::simple_singleline(
                name.to_string(),
                theme::font(14.0),
                theme::MUTE,
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(0.0));
            let galley = painter.layout_job(job);
            let at = pos2(left, row.center().y - galley.size().y / 2.0);
            painter.galley(at, galley, theme::MUTE);
        }
        if let Some(part) = areas.and_then(|a| a.get(i)) {
            painter.text(
                pos2(row.right() - SHARE_PAD, row.center().y),
                Align2::RIGHT_CENTER,
                crate::areas::share_text(*part, total),
                theme::hint(),
                theme::MUTE,
            );
        }
    }
    let stops = bands.stops();
    for i in 1..stops.len().saturating_sub(1).min(count) {
        let stop = stops[i];
        let y = seam(i);
        let on = scrub == Some(i);
        let cut = [pos2(rows.left(), y), pos2(rows.left() + CELL, y)];
        if on {
            painter.line_segment(cut, egui::Stroke::new(3.0, theme::ACCENT));
        } else if let Some(color) = bands.line_color(i).filter(|_| stop.line.on) {
            let width = stop.line.weight.clamp(1.0, 4.0);
            painter.line_segment(cut, egui::Stroke::new(width, color32(color)));
        }
        let w = pill_width(ui, stop.value, on);
        let pill =
            Rect::from_min_size(pos2(pill_left, y - PILL_HEIGHT / 2.0), vec2(w, PILL_HEIGHT));
        let (fill, ink) = if on {
            (theme::ACCENT, theme::CANVAS)
        } else {
            (PILL, theme::TEXT)
        };
        painter.rect_filled(pill, 7.0, fill);
        let mut x = pill.left() + PILL_PAD;
        if on {
            let icon = Rect::from_min_size(pos2(x - 2.0, y - 7.0), vec2(14.0, 14.0));
            icons::paint(&painter, icon, Icon::UpDown, 14.0, ink);
            x += 18.0;
        }
        let text = world::value_text(stop.value, span, unit);
        painter.text(
            pos2(x, y),
            Align2::LEFT_CENTER,
            text,
            pill_font.clone(),
            ink,
        );
        let x = pill.right() + 8.0;
        let hint = |x: f32, s: String| {
            painter.text(
                pos2(x, y),
                Align2::LEFT_CENTER,
                s,
                theme::hint(),
                theme::MUTE,
            );
        };
        if on {
            let lo = world::value_text(stops[i - 1].value, span, unit);
            let hi = world::value_text(stops[i + 1].value, span, unit);
            hint(x, format!("{lo} to {hi}"));
        } else if Some(i) == bands.sea() {
            let icon = Rect::from_min_size(pos2(x, y - 7.0), vec2(14.0, 14.0));
            icons::paint(&painter, icon, Icon::Lock, 14.0, theme::MUTE);
            hint(x + 18.0, "Sea level".to_string());
        } else if let Some(color) = bands.line_color(i).filter(|_| stop.line.on) {
            let sample = Rect::from_center_size(pos2(x + 8.0, y), vec2(16.0, 2.0));
            painter.rect_filled(sample, 0.0, color32(color));
            hint(x + 24.0, format!("{} pt", stop.line.weight));
        }
    }
    Legend {
        rows,
        row_of,
        scrub,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_is_fine_near_the_start_and_monotone() {
        // A point near the start is one step of the snap.
        let step = scrub_fraction(0.5, 1.0) - 0.5;
        assert!((step - 1.0 / SNAPS).abs() < 1e-4);
        assert_eq!(scrub_fraction(0.5, 0.0), 0.5);
        let mut last = f64::NEG_INFINITY;
        for d in -600..=600 {
            let f = scrub_fraction(0.5, f64::from(d));
            assert!(f > last);
            last = f;
        }
        // A long drag crosses the whole bar.
        assert!(scrub_fraction(0.0, 300.0) > 1.0);
        assert!(scrub_fraction(1.0, -300.0) < 0.0);
    }

    #[test]
    fn a_new_stop_goes_in_the_middle_of_the_band() {
        let bands = Bands::with_range(0.0, 1200.0, false).unwrap();
        let stops = bands.stops();
        let mid = middle(&bands, 0);
        assert!((mid - (stops[0].value + stops[1].value) / 2.0).abs() <= 0.5);
    }
}
