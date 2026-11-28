//! The Planet workspace: the star, the orbit, the rotation, and the
//! atmosphere, each with its presets.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};
use isotherm_world::Planet;
use isotherm_world::presets::{
    AU_M, AtmospherePreset, OrbitPreset, RotationPreset, SUN_LUMINOSITY_W, StarPreset,
};
use vale_shell::pad::metrics::TOUCH;
use vale_shell::pad::number::slider_row;
use vale_shell::pad::theme;
use vale_shell::pad::widgets::{self, Controls, PANEL_ROW};

use super::{Ask, IsothermApp, grouped, wrapped};

pub const STAR: &str = "Star";
pub const ORBIT: &str = "Orbit";
pub const ROTATION: &str = "Rotation";
pub const ATMOSPHERE: &str = "Atmosphere";
pub const LOCKED: &str = "Tidally locked";

/// The height of the heading of a section.
const HEADING: f32 = 40.0;
/// The solar day that a planet gets when it stops being locked, in seconds.
const UNLOCKED_DAY_S: f64 = 86_400.0;

/// A field of the planet with a slider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Temperature,
    Luminosity,
    Distance,
    Eccentricity,
    Perihelion,
    Tilt,
    DayLength,
    Pressure,
    Co2,
}

impl Setting {
    pub const ALL: [Setting; 9] = [
        Setting::Temperature,
        Setting::Luminosity,
        Setting::Distance,
        Setting::Eccentricity,
        Setting::Perihelion,
        Setting::Tilt,
        Setting::DayLength,
        Setting::Pressure,
        Setting::Co2,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Setting::Temperature => "Temperature",
            Setting::Luminosity => "Luminosity",
            Setting::Distance => "Distance",
            Setting::Eccentricity => "Eccentricity",
            Setting::Perihelion => "Perihelion",
            Setting::Tilt => "Tilt",
            Setting::DayLength => "Day length",
            Setting::Pressure => "Pressure",
            Setting::Co2 => "CO₂",
        }
    }

    /// The range of the slider in the unit of the value text, and whether
    /// the slider has a log scale.
    pub fn range(self) -> (f64, f64, bool) {
        match self {
            Setting::Temperature => (2300.0, 10_000.0, false),
            Setting::Luminosity => (1e-4, 100.0, true),
            Setting::Distance => (0.01, 30.0, true),
            Setting::Eccentricity => (0.0, 0.9, false),
            Setting::Perihelion => (0.0, 360.0, false),
            Setting::Tilt => (0.0, 180.0, false),
            Setting::DayLength => (1.0, 10_000.0, true),
            Setting::Pressure => (0.01, 100.0, true),
            Setting::Co2 => (1.0, 100_000.0, true),
        }
    }

    /// The value in the unit of the value text: kelvins, luminosities of
    /// the Sun, astronomical units, degrees, hours, bars, and ppm.
    pub fn get(self, planet: &Planet) -> f64 {
        match self {
            Setting::Temperature => planet.star.temperature_k,
            Setting::Luminosity => planet.star.luminosity_w / SUN_LUMINOSITY_W,
            Setting::Distance => planet.orbit.semi_major_m / AU_M,
            Setting::Eccentricity => planet.orbit.eccentricity,
            Setting::Perihelion => planet.orbit.perihelion_deg,
            Setting::Tilt => planet.rotation.tilt_deg,
            Setting::DayLength => planet.solar_day_s() / 3600.0,
            Setting::Pressure => planet.atmosphere.surface_pressure_pa / 1e5,
            Setting::Co2 => planet.atmosphere.co2_ppm,
        }
    }

    /// Sets the value in the unit of the value text. A locked planet stays
    /// locked.
    pub fn set(self, planet: &mut Planet, value: f64) {
        planet.keep_lock(|planet| match self {
            Setting::Temperature => planet.star.temperature_k = value,
            Setting::Luminosity => planet.star.luminosity_w = value * SUN_LUMINOSITY_W,
            Setting::Distance => planet.orbit.semi_major_m = value * AU_M,
            Setting::Eccentricity => planet.orbit.eccentricity = value,
            Setting::Perihelion => planet.orbit.perihelion_deg = value,
            Setting::Tilt => planet.rotation.tilt_deg = value,
            Setting::DayLength => planet.set_solar_day_s(value * 3600.0),
            Setting::Pressure => planet.atmosphere.surface_pressure_pa = value * 1e5,
            Setting::Co2 => planet.atmosphere.co2_ppm = value,
        });
    }

    /// The value as the app shows it, for example "5,772 K".
    pub fn text(self, value: f64) -> String {
        match self {
            Setting::Temperature => format!("{} K", grouped(value)),
            Setting::Luminosity => format!("{} × Sun", significant(value)),
            Setting::Distance => format!("{} AU", significant(value)),
            Setting::Eccentricity => format!("{value:.3}"),
            Setting::Perihelion => format!("{value:.0}°"),
            Setting::Tilt => format!("{value:.1}°"),
            Setting::DayLength if value < 100.0 => format!("{value:.1} h"),
            Setting::DayLength => format!("{} h", grouped(value)),
            Setting::Pressure => format!("{} bar", significant(value)),
            Setting::Co2 if value < 10.0 => format!("{value:.1} ppm"),
            Setting::Co2 => format!("{} ppm", grouped(value)),
        }
    }
}

/// A number with three significant digits, or a whole number from 100.
pub fn significant(value: f64) -> String {
    if value >= 100.0 {
        return grouped(value);
    }
    let magnitude = value.abs().max(1e-12).log10().floor() as i32;
    let decimals = (2 - magnitude).clamp(0, 5) as usize;
    format!("{value:.decimals$}")
}

/// The index of the preset that matches, or `None`.
fn index_of<T: PartialEq>(all: &[T], preset: Option<T>) -> Option<usize> {
    preset.and_then(|p| all.iter().position(|q| *q == p))
}

impl IsothermApp {
    /// The rows of the Planet panel: the world and its undo buttons, and a
    /// section with presets and sliders for the star, the orbit, the
    /// rotation, and the atmosphere.
    pub(super) fn planet_rows(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        self.title_row(ui, controls);
        let planet = self.doc.world.planet.clone();

        heading(ui, STAR);
        let names = StarPreset::ALL.map(StarPreset::name);
        let active = index_of(&StarPreset::ALL, StarPreset::of(&planet));
        if let Some(i) = preset_row(ui, controls, STAR, &names, active) {
            self.edit(|p| p.keep_lock(|p| StarPreset::ALL[i].apply(p)));
        }
        self.slider(ui, controls, Setting::Temperature);
        self.slider(ui, controls, Setting::Luminosity);

        heading(ui, ORBIT);
        let names = OrbitPreset::ALL.map(OrbitPreset::name);
        let active = index_of(&OrbitPreset::ALL, OrbitPreset::of(&planet));
        if let Some(i) = preset_row(ui, controls, ORBIT, &names, active) {
            self.edit(|p| p.keep_lock(|p| OrbitPreset::ALL[i].apply(p)));
        }
        self.slider(ui, controls, Setting::Distance);
        self.slider(ui, controls, Setting::Eccentricity);
        self.slider(ui, controls, Setting::Perihelion);

        heading(ui, ROTATION);
        let names = RotationPreset::ALL.map(RotationPreset::name);
        let active = index_of(&RotationPreset::ALL, RotationPreset::of(&planet));
        if let Some(i) = preset_row(ui, controls, ROTATION, &names, active) {
            self.edit(|p| RotationPreset::ALL[i].apply(p));
        }
        self.slider(ui, controls, Setting::Tilt);
        let mut locked = planet.is_locked();
        if !locked {
            self.slider(ui, controls, Setting::DayLength);
        }
        if widgets::switch_row(ui, controls, LOCKED, LOCKED, &mut locked) {
            self.edit(|p| match locked {
                true => p.lock(),
                false => p.set_solar_day_s(UNLOCKED_DAY_S),
            });
        }

        heading(ui, ATMOSPHERE);
        let names = AtmospherePreset::ALL.map(AtmospherePreset::name);
        let active = index_of(&AtmospherePreset::ALL, AtmospherePreset::of(&planet));
        if let Some(i) = preset_row(ui, controls, ATMOSPHERE, &names, active) {
            self.edit(|p| AtmospherePreset::ALL[i].apply(p));
        }
        self.slider(ui, controls, Setting::Pressure);
        self.slider(ui, controls, Setting::Co2);

        if let Some(status) = &self.status {
            ui.add_space(4.0);
            wrapped(ui, status, theme::hint(), theme::WARN);
        }
        ui.add_space(4.0);
    }

    /// A change of the planet at once, as one undo step.
    fn edit(&mut self, edit: impl FnOnce(&mut Planet)) {
        let mut planet = self.doc.world.planet.clone();
        edit(&mut planet);
        self.change(Ask::Planet(Box::new(planet)));
    }

    /// The slider of a setting. A drag changes the planet at once, and the
    /// end of the drag is one undo step. While a run exists, a drag asks to
    /// delete the run first.
    fn slider(&mut self, ui: &mut egui::Ui, controls: &mut Controls, setting: Setting) {
        let (lo, hi, log) = setting.range();
        let to = |v: f64| if log { v.log10() } else { v };
        let from = |v: f64| if log { 10f64.powf(v) } else { v };
        let shown = setting.get(&self.doc.world.planet);
        let mut value = to(shown.clamp(lo, hi));
        let text = setting.text(shown);
        let resp = slider_row(
            ui,
            controls,
            setting.label(),
            &mut value,
            to(lo)..=to(hi),
            &text,
        );
        if resp.started {
            let mut next = self.doc.world.planet.clone();
            if resp.changed {
                setting.set(&mut next, from(value));
            }
            if self.has_run() {
                self.ask = Some(Ask::Planet(Box::new(next)));
                return;
            }
            self.planet_start = Some(self.doc.world.planet.clone());
            self.clear_run();
        }
        if resp.changed && self.planet_start.is_some() {
            setting.set(&mut self.doc.world.planet, from(value));
        }
        if resp.released
            && let Some(old) = self.planet_start.take()
            && let Err(e) = self.doc.save_planet(&old)
        {
            self.status = Some(e);
        }
    }
}

/// The heading of a section of the panel.
fn heading(ui: &mut egui::Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADING), Sense::hover());
    let at = pos2(rect.left(), rect.bottom() - 6.0);
    let font = theme::heading(ui.ctx());
    ui.painter()
        .text(at, Align2::LEFT_BOTTOM, text, font, theme::TEXT);
}

/// A row with the presets of a section, as a small segmented control as
/// wide as the row. The control name of a preset is the section and the
/// preset, for example "Orbit Temperate". `active`: the preset that matches
/// the planet, or none.
fn preset_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    section: &str,
    labels: &[&str],
    active: Option<usize>,
) -> Option<usize> {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let names: Vec<String> = labels.iter().map(|l| preset_name(section, l)).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let rect = Rect::from_center_size(row.center(), vec2(row.width(), TOUCH - 8.0));
    let active = active.unwrap_or(usize::MAX);
    widgets::small_segments_named(ui, controls, rect, (labels, &names), active)
}

/// The control name of a preset, for example "Orbit Temperate".
pub fn preset_name(section: &str, preset: &str) -> String {
    format!("{section} {preset}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_value_texts_have_units() {
        let earth = Planet::earth();
        let text = |s: Setting| s.text(s.get(&earth));
        assert_eq!(text(Setting::Temperature), "5,772 K");
        assert_eq!(text(Setting::Luminosity), "1.00 × Sun");
        assert_eq!(text(Setting::Distance), "1.00 AU");
        assert_eq!(text(Setting::Eccentricity), "0.017");
        assert_eq!(text(Setting::Perihelion), "283°");
        assert_eq!(text(Setting::Tilt), "23.4°");
        assert_eq!(text(Setting::DayLength), "24.0 h");
        assert_eq!(text(Setting::Pressure), "1.01 bar");
        assert_eq!(text(Setting::Co2), "280 ppm");
        assert_eq!(significant(0.0001), "0.00010");
        assert_eq!(significant(0.0123), "0.0123");
    }

    #[test]
    fn each_setting_round_trips_through_the_planet() {
        for setting in Setting::ALL {
            let (lo, hi, _) = setting.range();
            let value = lo + (hi - lo) * 0.37;
            let mut planet = Planet::earth();
            setting.set(&mut planet, value);
            let back = setting.get(&planet);
            assert!((back / value - 1.0).abs() < 1e-6, "{setting:?}: {back}");
        }
    }

    #[test]
    fn a_new_distance_keeps_a_locked_planet_locked() {
        let mut planet = Planet::earth();
        planet.lock();
        Setting::Distance.set(&mut planet, 0.05);
        assert!(planet.is_locked());
    }
}
