//! The Explore workspace: the output layers, their colors and contours,
//! the month, and the readout of one place.

use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, Sense, pos2, vec2};
use isotherm_fields::{Field, Output, Period};
use vale_shell::pad::{theme, widgets};
use vale_terrain::math::V3;
use vale_terrain::{Bands, Rgb, Stop, StopLine, face_of, warp};

/// The range of the colors of the temperature, in °C, and the spacing of
/// the stops and the contours.
pub const MIN_C: f64 = -60.0;
pub const MAX_C: f64 = 50.0;
pub const STEP_C: f64 = 10.0;

/// The color of each band of 10 °C, from the coldest.
const COLORS: [Rgb; 11] = [
    [96, 62, 140],
    [88, 92, 178],
    [68, 124, 198],
    [78, 162, 216],
    [124, 198, 226],
    [184, 228, 236],
    [206, 230, 164],
    [240, 218, 122],
    [244, 170, 84],
    [224, 110, 62],
    [168, 52, 52],
];

/// The color of the contours.
const LINE: Rgb = [24, 28, 36];

/// The spacing of the stops and the contours of the ice cover, in percent.
pub const ICE_STEP: f64 = 20.0;

/// The color of each band of 20 % of ice cover, from open water to full
/// ice.
const ICE_COLORS: [Rgb; 5] = [
    [20, 48, 86],
    [48, 88, 132],
    [98, 138, 176],
    [162, 192, 216],
    [236, 242, 248],
];

/// An output layer of the Explore workspace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layer {
    #[default]
    Temperature,
    IceCover,
}

impl Layer {
    pub const ALL: [Layer; 2] = [Layer::Temperature, Layer::IceCover];

    pub fn name(self) -> &'static str {
        match self {
            Layer::Temperature => "Temperature",
            Layer::IceCover => "Ice cover",
        }
    }

    pub fn output(self) -> Output {
        match self {
            Layer::Temperature => Output::Temperature,
            Layer::IceCover => Output::IceCover,
        }
    }

    pub fn bands(self) -> Bands {
        match self {
            Layer::Temperature => temperature_bands(),
            Layer::IceCover => ice_bands(),
        }
    }

    /// The unit of the legend.
    pub fn unit(self) -> &'static str {
        match self {
            Layer::Temperature => "°C",
            Layer::IceCover => "%",
        }
    }

    /// The factor from a value of the output to a value of the bands.
    pub fn scale(self) -> f64 {
        match self {
            Layer::Temperature => 1.0,
            Layer::IceCover => 100.0,
        }
    }

    /// The levels of the values of a grid of the output in the bands of the
    /// layer.
    pub fn levels(self, bands: &Bands, values: &[f32]) -> Vec<u16> {
        let scale = self.scale();
        values
            .iter()
            .map(|&v| bands.level(f64::from(v) * scale))
            .collect()
    }

    /// A value as the readout shows it, for example "−3.5 °C" or "65 % ice".
    pub fn value_text(self, value: f32) -> String {
        match self {
            Layer::Temperature => celsius_text(value),
            Layer::IceCover => format!("{:.0} % ice", (value * 100.0).clamp(0.0, 100.0)),
        }
    }
}

/// Contour lines at the inner stops of `bands`.
fn add_contours(bands: &mut Bands) {
    let count = bands.stops().len();
    for i in 1..count.saturating_sub(1) {
        let line = StopLine {
            on: true,
            weight: 1.0,
            color: LINE,
            ..StopLine::default()
        };
        bands.set_line(i, line);
    }
}

/// The bands of the ice cover, from 0 % to 100 %, with a stop and a contour
/// every 20 %.
pub fn ice_bands() -> Bands {
    let count = ICE_COLORS.len();
    let stops: Vec<Stop> = (0..=count)
        .map(|i| Stop {
            value: ICE_STEP * i as f64,
            line: StopLine::default(),
        })
        .collect();
    let mut bands = Bands::from_range_parts(0.0, 100.0, false, &stops, &ICE_COLORS)
        .expect("the stops of the ice cover are valid");
    add_contours(&mut bands);
    bands
}

/// The names of the months and of the year on the slider.
pub const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The bands of the temperature, with a stop and a contour every 10 °C.
pub fn temperature_bands() -> Bands {
    let count = ((MAX_C - MIN_C) / STEP_C).round() as usize;
    let stops: Vec<Stop> = (0..=count)
        .map(|i| Stop {
            value: MIN_C + STEP_C * i as f64,
            line: StopLine::default(),
        })
        .collect();
    let mut bands = Bands::from_range_parts(MIN_C, MAX_C, false, &stops, &COLORS)
        .expect("the stops of the temperature are valid");
    add_contours(&mut bands);
    bands
}

/// The index of the cell of a cube map of `face_size` that holds a
/// direction: `face * n * n + y * n + x`, as the cube map of `vale-terrain`.
pub fn cell_of(face_size: usize, d: V3) -> usize {
    let (face, a, b) = face_of(d);
    let n = face_size;
    let index = |f: f64| {
        let s = warp(f);
        (((s + 1.0) / 2.0 * n as f64).floor() as isize).clamp(0, n as isize - 1) as usize
    };
    face * n * n + index(b) * n + index(a)
}

/// The name of a period on the slider.
pub fn period_name(period: Period) -> &'static str {
    match period {
        Period::Month(m) => MONTHS[m],
        Period::Year => "Year",
    }
}

/// The period at a place of the slider: the months from 0 to 11, then the
/// year.
pub fn period_at(index: usize) -> Period {
    if index < 12 {
        Period::Month(index)
    } else {
        Period::Year
    }
}

pub fn period_index(period: Period) -> usize {
    match period {
        Period::Month(m) => m,
        Period::Year => 12,
    }
}

/// A place as the readout shows it, for example "45.2° N 7.1° E".
pub fn place_text(d: V3) -> String {
    let lat = d[2].clamp(-1.0, 1.0).asin().to_degrees();
    let lon = d[1].atan2(d[0]).to_degrees();
    let ns = if lat < 0.0 { "S" } else { "N" };
    let ew = if lon < 0.0 { "W" } else { "E" };
    format!("{:.1}° {ns} {:.1}° {ew}", lat.abs(), lon.abs())
}

/// A temperature as the app shows it, for example "−3.5 °C".
pub fn celsius_text(c: f32) -> String {
    let text = format!("{:.1}", c.abs());
    let sign = if c < 0.0 && text != "0.0" { "−" } else { "" };
    format!("{sign}{text} °C")
}

/// The value of a place in a period, or `None` before the model fills the
/// period.
pub fn value_at(field: &Field, period: Period, d: V3) -> Option<f32> {
    let grid = field.get(period)?;
    grid.get(cell_of(field.face_size, d)).copied()
}

/// The state of the Explore workspace.
#[derive(Clone, Debug, PartialEq)]
pub struct Explore {
    pub layer: Layer,
    pub period: Period,
    pub contours: bool,
    /// The place of the last tap.
    pub pin: Option<V3>,
    /// The place under the pointer.
    pub hover: Option<V3>,
}

impl Default for Explore {
    fn default() -> Explore {
        Explore {
            layer: Layer::default(),
            period: Period::Year,
            contours: true,
            pin: None,
            hover: None,
        }
    }
}

impl Explore {
    /// The place of the readout: the place under the pointer, else the
    /// place of the last tap.
    pub fn place(&self) -> Option<V3> {
        self.hover.or(self.pin)
    }
}

/// The height of the legend row: the color bar and its labels.
pub const LEGEND_HEIGHT: f32 = 44.0;
const BAR_HEIGHT: f32 = 12.0;

/// The legend of `bands`: a bar with the colors of the gradient, and the
/// value of each stop below it.
pub fn legend(ui: &mut egui::Ui, bands: &Bands, unit: &str) {
    let (row, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), LEGEND_HEIGHT), Sense::hover());
    let font = theme::hint();
    let unit_width = widgets::text_width(ui, unit, font.clone()) + 8.0;
    // The labels at the ends stay in the row, centered on their stops.
    let stops = bands.stops();
    let half = |value: f64| widgets::text_width(ui, &number_text(value), font.clone()) / 2.0;
    let left = half(bands.min()).max(4.0);
    let right = half(bands.max()).max(4.0);
    let bar = Rect::from_min_max(
        pos2(row.left() + left, row.top() + 6.0),
        pos2(
            row.right() - unit_width - right,
            row.top() + 6.0 + BAR_HEIGHT,
        ),
    );
    let x = |value: f64| {
        let t = (value - bands.min()) / (bands.max() - bands.min());
        bar.left() + bar.width() * t as f32
    };
    let color = |c: Rgb| Color32::from_rgb(c[0], c[1], c[2]);
    let mut mesh = egui::Mesh::default();
    let mut quad = |x0: f32, x1: f32, c0: Color32, c1: Color32| {
        let i = mesh.vertices.len() as u32;
        for (px, py, c) in [
            (x0, bar.top(), c0),
            (x1, bar.top(), c1),
            (x1, bar.bottom(), c1),
            (x0, bar.bottom(), c0),
        ] {
            mesh.colored_vertex(pos2(px, py), c);
        }
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    };
    for band in bands.bands() {
        let mid = (band.min + band.max) / 2.0;
        quad(x(band.min), x(mid), color(band.bottom), color(band.color));
        quad(x(mid), x(band.max), color(band.color), color(band.top));
    }
    let painter = ui.painter();
    painter.add(mesh);
    let label_y = bar.bottom() + 4.0;
    // A narrow legend labels every second stop.
    let widest = stops
        .iter()
        .map(|s| 2.0 * half(s.value))
        .fold(0.0, f32::max);
    let spacing = bar.width() / (stops.len().max(2) - 1) as f32;
    let every = if spacing < widest + 6.0 { 2 } else { 1 };
    for (i, stop) in stops.iter().enumerate() {
        let at = x(stop.value);
        painter.vline(at, bar.bottom()..=bar.bottom() + 3.0, (1.0, theme::MUTE));
        if i % every != 0 {
            continue;
        }
        let text = number_text(stop.value);
        let pos = pos2(at, label_y);
        painter.text(pos, Align2::CENTER_TOP, text, font.clone(), theme::MUTE);
    }
    let at = pos2(row.right() - 4.0, bar.center().y);
    painter.text(at, Align2::RIGHT_CENTER, unit, font, theme::MUTE);
}

/// A whole number with a true minus sign.
fn number_text(value: f64) -> String {
    let n = value.round() as i64;
    let sign = if n < 0 { "−" } else { "" };
    format!("{sign}{}", n.unsigned_abs())
}

/// The readout card: the value and the place, at the bottom middle of
/// `area`. Returns the rect of the card.
pub fn readout_card(ctx: &egui::Context, area: Rect, text: &str) -> Rect {
    let font = theme::body();
    let width = text_size(ctx, text, font.clone()) + 2.0 * widgets::BODY_PAD;
    let height = 44.0;
    let rect = Rect::from_min_size(
        pos2(area.center().x - width / 2.0, area.bottom() - height),
        vec2(width.min(area.width()), height),
    );
    let radius = CornerRadius::same(255);
    widgets::card(
        ctx,
        widgets::backdrop(ctx),
        READOUT,
        rect,
        egui::Order::Middle,
        radius,
        |ui| {
            let painter = ui.painter();
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                text,
                font,
                theme::TEXT,
            );
        },
    );
    rect
}

fn text_size(ctx: &egui::Context, text: &str, font: egui::FontId) -> f32 {
    ctx.fonts_mut(|f| {
        f.layout_no_wrap(text.to_owned(), font, Color32::WHITE)
            .size()
            .x
    })
}

/// The id of the readout card.
pub const READOUT: &str = "readout";

#[cfg(test)]
mod tests {
    use super::*;
    use vale_terrain::math::lonlat_to_dir;

    #[test]
    fn the_bands_have_a_contour_every_ten_degrees() {
        let bands = temperature_bands();
        let values: Vec<f64> = bands.stops().iter().map(|s| s.value).collect();
        assert_eq!(values.first(), Some(&-60.0));
        assert_eq!(values.last(), Some(&50.0));
        assert_eq!(values.len(), 12);
        let lines = (0..values.len())
            .filter(|&i| bands.stops()[i].line.on)
            .count();
        assert_eq!(lines, 10);
        assert!(bands.level(20.0) > bands.level(-20.0));
    }

    #[test]
    fn the_ice_bands_run_from_open_water_to_full_ice() {
        let bands = ice_bands();
        let values: Vec<f64> = bands.stops().iter().map(|s| s.value).collect();
        assert_eq!(values, [0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        let lines = bands.stops().iter().filter(|s| s.line.on).count();
        assert_eq!(lines, 4);
        let levels = Layer::IceCover.levels(&bands, &[0.0, 0.5, 1.0]);
        assert!(levels[0] < levels[1] && levels[1] < levels[2]);
        assert_eq!(Layer::IceCover.value_text(0.654), "65 % ice");
    }

    #[test]
    fn a_direction_finds_the_cell_of_its_face() {
        // +X is face 0, at the middle of the face.
        let n = 4;
        let i = cell_of(n, [1.0, 0.0, 0.0]);
        assert_eq!(i / (n * n), 0);
        // The north pole is on +Z, face 4.
        assert_eq!(cell_of(n, [0.0, 0.0, 1.0]) / (n * n), 4);
        assert!(cell_of(n, [-0.1, -0.2, -1.0]) < 6 * n * n);
    }

    #[test]
    fn texts_have_a_sign_and_a_hemisphere() {
        assert_eq!(celsius_text(-3.46), "−3.5 °C");
        assert_eq!(celsius_text(-0.01), "0.0 °C");
        assert_eq!(celsius_text(12.3), "12.3 °C");
        let d = lonlat_to_dir(-7.1, 45.2);
        assert_eq!(place_text(d), "45.2° N 7.1° W");
    }

    #[test]
    fn the_slider_has_twelve_months_then_the_year() {
        assert_eq!(period_at(0), Period::Month(0));
        assert_eq!(period_at(12), Period::Year);
        for i in 0..13 {
            assert_eq!(period_index(period_at(i)), i);
        }
        assert_eq!(period_name(Period::Month(2)), "Mar");
    }
}
