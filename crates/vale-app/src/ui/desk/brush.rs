//! The settings of the brush, and the Brush panel of the inspector. The
//! options bar and the panel show the same settings.

use eframe::egui::{self, Align, Layout, vec2};
use vale_terrain::{ELEV_MAX, ELEV_MIN, Mode};

use super::field::{self, Field, Scale};
use super::{rail, theme, widgets};
use crate::globe::brush::{EROSION, FLOW, SCALE_KM, SEED, STRENGTH_M, TALUS_DEG, WARP_KM};
use crate::ui::pad::brush_size_text;
use crate::ui::pad::icons::Icon;
use crate::ui::{AppState, HEIGHT_UNIT, panels};
use crate::world::{self, three_significant};

/// The button of the Brush panel that sets the flatten level from the globe.
pub const PICK: &str = "Pick from globe";
/// The button of the Brush panel that flattens to the level under the start
/// of the stroke.
pub const STROKE_START: &str = "Use stroke start";
pub const LOCK: &str = "Lock size";
/// The number of steps of a limit across the value range of the layer. A
/// height has steps of 10 meters.
const LIMIT_STEPS: f64 = 1200.0;

/// A setting of the brush.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Setting {
    Size,
    Hardness,
    /// The strength, or in the warp mode the distance of a pass.
    Strength,
    /// The size of the noise of the warp mode.
    Scale,
    Seed,
    Erosion,
    Angle,
    Flow,
    Floor,
    Ceiling,
    /// The flatten level.
    Level,
}

/// The settings of the options bar.
pub const BAR: [Setting; 4] = [
    Setting::Size,
    Setting::Strength,
    Setting::Flow,
    Setting::Hardness,
];

fn percent(name: &'static str, value: f64, range: std::ops::RangeInclusive<f64>) -> Field {
    let value = value * 100.0;
    Field {
        name,
        value,
        scale: Scale::linear(range.start() * 100.0..=range.end() * 100.0),
        text: format!("{value:.0}%"),
        units: vec![("%".into(), 1.0)],
        step: 0.0,
        enabled: true,
    }
}

fn km(name: &'static str, value: f64, range: std::ops::RangeInclusive<f64>) -> Field {
    Field {
        name,
        value,
        scale: Scale::log(range),
        text: format!("{} km", three_significant(value)),
        units: vec![("km".into(), 1.0), ("m".into(), 0.001)],
        step: 0.0,
        enabled: true,
    }
}

/// A field for a value of the layer that the brush paints, in the unit of
/// the layer. A height also takes kilometers.
fn layer_value(name: &'static str, value: f64, scale: Scale, step: f64, state: &AppState) -> Field {
    let unit = state.shown_unit();
    let (min, max) = state.globe.brush.value_range;
    let mut units = Vec::new();
    if !unit.is_empty() {
        units.push((unit.to_owned(), 1.0));
    }
    if unit == HEIGHT_UNIT {
        units.push(("km".to_owned(), 1000.0));
    }
    Field {
        name,
        value,
        scale,
        text: world::value_text(value, max - min, unit),
        units,
        step,
        enabled: true,
    }
}

impl Setting {
    /// The setting as a field, in the units that the field shows.
    pub fn field(self, state: &AppState) -> Field {
        let brush = &state.globe.brush;
        let (min, max) = brush.value_range;
        let range = Scale::linear(min..=max);
        let step = (max - min) / LIMIT_STEPS;
        match self {
            Setting::Size => {
                let world = &state.doc.project.world;
                let globe = &state.globe;
                let range = brush.radius_km_range(globe.radius(), world.radius_km);
                let large = world::large_unit(world.units);
                let small = world::small_unit(world.units);
                Field {
                    name: "Size",
                    value: brush.radius_km(globe.radius(), world.radius_km),
                    scale: Scale::log(range),
                    text: brush_size_text(state),
                    units: vec![(large.1.into(), large.0), (small.1.into(), small.0)],
                    step: 0.0,
                    enabled: true,
                }
            }
            Setting::Hardness => percent("Hardness", brush.hardness, 0.0..=1.0),
            Setting::Flow => percent("Flow", brush.flow, FLOW),
            Setting::Erosion => percent("Erosion", brush.erosion, EROSION),
            Setting::Strength if brush.mode == Mode::Warp => km("Strength", brush.warp_km, WARP_KM),
            Setting::Strength => {
                let name = panels::strength_name(brush.mode);
                let factor = strength_factor(state);
                let scale = Scale::log(STRENGTH_M.start() * factor..=STRENGTH_M.end() * factor);
                layer_value(name, brush.strength_value(), scale, 0.0, state)
            }
            Setting::Scale => km("Scale", brush.scale_km, SCALE_KM),
            Setting::Seed => {
                let seed = state.doc.project.world.seed;
                let high = f64::from(*SEED.end());
                Field {
                    name: "Seed",
                    value: f64::from(seed),
                    scale: Scale::linear(0.0..=high),
                    text: seed.to_string(),
                    units: Vec::new(),
                    step: 1.0,
                    enabled: true,
                }
            }
            Setting::Angle => Field {
                name: "Angle",
                value: brush.talus_deg,
                scale: Scale::log(TALUS_DEG),
                text: format!("{}°", three_significant(brush.talus_deg)),
                units: vec![("°".into(), 1.0), ("deg".into(), 1.0)],
                step: 0.0,
                enabled: true,
            },
            Setting::Floor => Field {
                enabled: brush.floor_on,
                ..layer_value("Floor", brush.floor_m, range, step, state)
            },
            Setting::Ceiling => Field {
                enabled: brush.ceiling_on,
                ..layer_value("Ceiling", brush.ceiling_m, range, step, state)
            },
            Setting::Level => match brush.flatten_level {
                Some(level) => layer_value("Level", brush.value(level), range, 0.0, state),
                None => Field {
                    text: "Start".to_owned(),
                    ..layer_value("Level", min.max(0.0).min(max), range, 0.0, state)
                },
            },
        }
    }

    /// Sets the value, in the units of the field.
    pub fn set(self, state: &mut AppState, value: f64) {
        let factor = strength_factor(state);
        let world_km = state.doc.project.world.radius_km;
        let globe = &mut state.globe;
        let globe_radius = globe.radius();
        let brush = &mut globe.brush;
        match self {
            Setting::Size => brush.set_radius_km(value, globe_radius, world_km),
            Setting::Hardness => brush.hardness = value / 100.0,
            Setting::Flow => brush.flow = value / 100.0,
            Setting::Erosion => brush.erosion = value / 100.0,
            Setting::Strength if brush.mode == Mode::Warp => brush.warp_km = value,
            Setting::Strength => brush.strength_m = value / factor,
            Setting::Scale => brush.scale_km = value,
            // The seed is a property of the world, so each brush gets the
            // same noise field.
            Setting::Seed => state.doc.project.world.seed = value.round() as u32,
            Setting::Angle => brush.talus_deg = value,
            Setting::Floor => brush.floor_m = value,
            Setting::Ceiling => brush.ceiling_m = value,
            Setting::Level => brush.flatten_level = Some(brush.level(value)),
        }
    }
}

/// The size of the strength in the unit of the layer, for each meter of the
/// strength of the brush.
fn strength_factor(state: &AppState) -> f64 {
    let (min, max) = state.globe.brush.value_range;
    (max - min) / (ELEV_MAX - ELEV_MIN)
}

/// The settings of the Stroke section, for a mode.
fn stroke_settings(mode: Mode) -> Vec<Setting> {
    let mut settings = vec![Setting::Strength];
    match mode {
        Mode::Warp => settings.extend([Setting::Scale, Setting::Seed]),
        Mode::Uplift => settings.extend([Setting::Erosion, Setting::Angle]),
        Mode::Weather => settings.push(Setting::Angle),
        _ => {}
    }
    settings.push(Setting::Flow);
    settings
}

/// The heading of the Brush panel.
pub fn title(mode: Mode) -> String {
    format!("Brush · {}", rail::mode_name(mode))
}

/// A row of the panel for a setting. Returns the column at the right of
/// the row.
fn row(ui: &mut egui::Ui, state: &mut AppState, setting: Setting) -> egui::Rect {
    let row = field::row(ui, &setting.field(state));
    if let Some(value) = row.value {
        setting.set(state, value);
    }
    row.trailing
}

/// The small heading of a section of the panel.
fn section(ui: &mut egui::Ui, text: &str, first: bool) {
    ui.add_space(if first { 4.0 } else { 12.0 });
    let size = vec2(ui.available_width(), 18.0);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(size);
        let text = egui::RichText::new(text)
            .font(theme::small())
            .strong()
            .color(theme::MUTE);
        ui.label(text);
    });
}

/// The Brush panel: the sections Tip, Stroke, and Limits.
pub fn body(ui: &mut egui::Ui, state: &mut AppState) {
    ui.spacing_mut().item_spacing.y = 0.0;
    section(ui, "Tip", true);
    let lock = row(ui, state, Setting::Size);
    let locked = state.globe.brush.lock.is_some();
    let tip = "Lock size. The brush keeps its size on the ground when you zoom.";
    if field::icon_toggle(ui, lock, LOCK, Icon::Lock, tip, locked) {
        let globe = &mut state.globe;
        let radius = globe.radius();
        globe.brush.set_lock(!locked, radius);
    }
    row(ui, state, Setting::Hardness);

    section(ui, "Stroke", false);
    let mode = state.globe.brush.mode;
    for setting in stroke_settings(mode) {
        row(ui, state, setting);
    }
    if mode == Mode::Flatten {
        level(ui, state);
    }

    section(ui, "Limits", false);
    for (setting, name) in [(Setting::Floor, "Floor"), (Setting::Ceiling, "Ceiling")] {
        let trailing = row(ui, state, setting);
        let brush = &mut state.globe.brush;
        let on = match setting {
            Setting::Floor => &mut brush.floor_on,
            _ => &mut brush.ceiling_on,
        };
        if field::switch(ui, trailing, name, *on) {
            *on = !*on;
        }
    }
}

/// The flatten level, and the buttons that set it.
fn level(ui: &mut egui::Ui, state: &mut AppState) {
    row(ui, state, Setting::Level);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let globe = &mut state.globe;
        let level = globe.brush.flatten_level;
        let tip = "Flattens to the level under the start of the stroke.";
        let resp = ui.add_enabled_ui(level.is_some(), |ui| {
            widgets::button(ui, STROKE_START, tip, None)
        });
        if resp.inner.clicked() {
            globe.brush.flatten_level = None;
        }
        let tip = "The next press on the globe sets the level.";
        if widgets::button(ui, PICK, tip, Some(globe.pick_level)).clicked() {
            globe.pick_level = !globe.pick_level;
        }
    });
}
