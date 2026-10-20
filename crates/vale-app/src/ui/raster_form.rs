//! The form that adds a raster layer to the world.

use eframe::egui::{self, Order, Rect, pos2, vec2};
use vale_store::World;
use vale_terrain::{Bands, face_size_values};

use super::pad::geometry::{GAP, MARGIN, ROW, Rects, TOUCH};
use super::pad::theme;
use super::pad::widgets::{self, BODY_PAD, Controls, PANEL_ROW};
use super::{AppState, HEIGHT_UNIT, LayerRef, RasterSpec};
use crate::world;

/// The title of the form.
pub const TITLE: &str = "New raster layer";
/// The control that adds the layer of the form.
pub const ADD: &str = "Add layer";
pub const NAME: &str = "Raster name";
pub const UNIT: &str = "Unit";
pub const MINIMUM: &str = "Minimum";
pub const MAXIMUM: &str = "Maximum";
/// The name of the choice of the preset.
pub const KIND: &str = "Kind";

/// The first values of a new raster layer of one kind.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Preset {
    /// The name of the choice, and the first name of the layer.
    pub name: &'static str,
    pub unit: &'static str,
    /// The values of the lowest and of the highest level.
    pub range: (f64, f64),
    /// Texels on one edge of a cube face.
    pub face_size: usize,
}

/// The presets of the form. The range of the precipitation is that of the
/// sum of one year.
pub const PRESETS: [Preset; 5] = [
    Preset {
        name: "Height",
        unit: HEIGHT_UNIT,
        range: (-6000.0, 6000.0),
        face_size: 8192,
    },
    Preset {
        name: "Precipitation",
        unit: "mm",
        range: (0.0, 4000.0),
        face_size: 2048,
    },
    Preset {
        name: "Temperature",
        unit: "°C",
        range: (-60.0, 60.0),
        face_size: 2048,
    },
    Preset {
        name: "Pressure",
        unit: "hPa",
        range: (950.0, 1050.0),
        face_size: 2048,
    },
    Preset {
        name: "Custom",
        unit: "",
        range: (0.0, 100.0),
        face_size: 2048,
    },
];
/// The index of the height preset. Its unit and its range are fixed, because
/// the sea, the rivers, and some brush modes need heights in meters.
pub const HEIGHT: usize = 0;
/// The index of the preset with no unit.
pub const CUSTOM: usize = 4;
/// The name of a new raster layer of the custom preset.
pub const CUSTOM_NAME: &str = "Raster";
/// The face sizes that the form offers on each device that can show them.
pub const FACE_SIZES: [usize; 4] = [1024, 2048, 4096, 8192];
const WIDTH: f32 = 400.0;
/// The space between two groups of rows of the form in the iPad layout.
const GROUP_GAP: f32 = 10.0;
/// The space above the button that adds the layer.
const BUTTON_GAP: f32 = 14.0;
/// The height of the text below a wrong value.
const NOTE: f32 = 22.0;
/// The height of the label above the face sizes.
const LABEL: f32 = 28.0;
/// The space below the last row.
const BOTTOM: f32 = 16.0;
/// The ID of the card of the form in the iPad layout.
const CARD: &str = "raster-form";

/// The face sizes that the form offers, in ascending order. `flag` is the
/// face size of `--face-size`, and `limit` is the largest face size that the
/// device can show. A device with a limit below each of `FACE_SIZES` gets
/// its limit.
pub fn face_sizes(flag: Option<usize>, limit: usize) -> Vec<usize> {
    let mut sizes: Vec<usize> = FACE_SIZES.into_iter().filter(|s| *s <= limit).collect();
    let flag = flag.map(|size| size.min(limit)).filter(|size| *size > 0);
    let least = sizes.is_empty().then_some(limit);
    if let Some(size) = flag.or(least).filter(|size| !sizes.contains(size)) {
        sizes.push(size);
        sizes.sort_unstable();
    }
    sizes
}

/// The size of `sizes` that is on at first: the largest one that is not
/// above `wanted`, or else the smallest one.
pub fn first_face_size(sizes: &[usize], wanted: usize) -> usize {
    let fits = sizes.iter().rev().find(|size| **size <= wanted);
    fits.or(sizes.first()).copied().unwrap_or(wanted)
}

/// A number as the text of a field of the form.
fn number_text(value: f64) -> String {
    format!("{value}")
}

/// The three values that describe a face size on a world: the ground that
/// one pixel covers at the equator, the equirectangular world map with the
/// same detail, and the memory of a layer with paint on each pixel.
/// The size of one pixel of a raster layer on the ground, as a text.
pub fn pixel_size_text(face_size: u32, world: &World) -> String {
    let values = face_size_values(face_size, world.radius_km);
    format!(
        "about {}",
        world::short_distance(values.km_per_pixel, world.units)
    )
}

pub fn face_size_text(face_size: usize, world: &World) -> [String; 3] {
    let values = face_size_values(face_size as u32, world.radius_km);
    let ground = world::short_distance(values.km_per_pixel, world.units);
    let (width, height) = values.map;
    let map = format!(
        "{} × {}",
        world::grouped(u64::from(width)),
        world::grouped(u64::from(height))
    );
    let megabytes = (values.bytes as f64 / 1e6).round();
    let memory = if megabytes < 1.0 {
        format!(
            "{} kB",
            world::grouped((values.bytes as f64 / 1e3).round() as u64)
        )
    } else {
        format!("{} MB", world::grouped(megabytes as u64))
    };
    [ground, map, memory]
}

/// The two lines of text next to a face size.
fn face_size_lines(face_size: usize, world: &World) -> [String; 2] {
    let [ground, map, memory] = face_size_text(face_size, world);
    [format!("{ground} per pixel"), format!("{map} · {memory}")]
}

/// The name of the control of a face size.
pub fn face_size_name(face_size: usize) -> String {
    format!("Face size {}", world::grouped(face_size as u64))
}

#[derive(Clone, Debug, PartialEq)]
pub struct RasterForm {
    pub name: String,
    /// The name that the form gave. The name follows the preset until the
    /// user changes it.
    given: String,
    /// The index of the preset in `PRESETS`.
    pub preset: usize,
    pub unit: String,
    /// The value of the lowest level, as the user typed it.
    pub min: String,
    /// The value of the highest level, as the user typed it.
    pub max: String,
    /// The face sizes that the form offers.
    pub sizes: Vec<usize>,
    pub face_size: usize,
    /// The face size of `--face-size`. With it, the face size does not
    /// follow the preset.
    flag: Option<usize>,
}

/// What the user did on the form in one frame.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Did {
    Nothing,
    Close,
    Add,
}

impl RasterForm {
    /// The form of a new raster layer for the world of the app. It starts
    /// with the custom preset.
    pub fn open(state: &AppState) -> RasterForm {
        let limit = state.globe.max_face_size();
        let flag = state.form_face_size.map(|size| size.min(limit));
        let mut form = RasterForm {
            name: String::new(),
            given: String::new(),
            preset: CUSTOM,
            unit: String::new(),
            min: String::new(),
            max: String::new(),
            sizes: face_sizes(flag, limit),
            face_size: 0,
            flag,
        };
        form.set_preset(CUSTOM, state);
        form
    }

    /// True if the layer of the form is a height in meters.
    pub fn height(&self) -> bool {
        self.preset == HEIGHT
    }

    /// Sets the preset. It gives the unit, the range, and the face size. A
    /// name that the user did not change follows the preset.
    pub fn set_preset(&mut self, preset: usize, state: &AppState) {
        let values = PRESETS[preset];
        self.preset = preset;
        if self.name == self.given {
            let name = if preset == CUSTOM {
                CUSTOM_NAME
            } else {
                values.name
            };
            self.name = state.raster_name(name);
            self.given = self.name.clone();
        }
        self.unit = values.unit.to_string();
        self.min = number_text(values.range.0);
        self.max = number_text(values.range.1);
        let wanted = self.flag.unwrap_or(values.face_size);
        self.face_size = first_face_size(&self.sizes, wanted);
    }

    /// The value range of the layer. `None`: a text is not a number, or the
    /// range is empty.
    fn range(&self) -> Option<(f64, f64)> {
        if self.height() {
            return Some(PRESETS[HEIGHT].range);
        }
        let number = |text: &str| text.trim().replace(',', "").parse::<f64>().ok();
        let (min, max) = (number(&self.min)?, number(&self.max)?);
        Bands::with_range(min, max, false).map(|_| (min, max))
    }

    /// The text that tells what is wrong with the form.
    pub fn note(&self) -> Option<&'static str> {
        if self.name.trim().is_empty() {
            Some("The layer must have a name")
        } else if self.range().is_none() {
            Some("The maximum must be above the minimum")
        } else {
            None
        }
    }

    /// The layer of the form. `None`: the form has a wrong value.
    pub fn spec(&self) -> Option<RasterSpec> {
        if self.note().is_some() {
            return None;
        }
        let name = self.name.trim();
        if self.height() {
            return Some(RasterSpec::height(name, self.face_size));
        }
        let (min, max) = self.range()?;
        Some(RasterSpec {
            name: name.to_string(),
            unit: self.unit.trim().to_string(),
            min,
            max,
            face_size: self.face_size,
        })
    }

    /// The fixed range of a height layer, as a text with its unit.
    fn fixed_range(&self) -> String {
        let (min, max) = PRESETS[HEIGHT].range;
        format!(
            "{} to {}",
            world::value_text(min, max - min, ""),
            world::value_text(max, max - min, HEIGHT_UNIT)
        )
    }
}

/// Acts on what the user did, and keeps or closes the form. Returns the new
/// layer.
fn finish(state: &mut AppState, form: RasterForm, did: Did) -> Option<LayerRef> {
    match did {
        Did::Close => None,
        Did::Add => match form.spec().and_then(|spec| state.add_raster(spec)) {
            Some(id) => Some(LayerRef::Raster(id)),
            None => {
                state.raster_form = Some(form);
                None
            }
        },
        Did::Nothing => {
            state.raster_form = Some(form);
            None
        }
    }
}

/// The form in the desktop layout.
pub fn desktop(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut form) = state.raster_form.take() else {
        return;
    };
    let mut did = Did::Nothing;
    let world = state.doc.project.world.clone();
    let modal = egui::Modal::new(egui::Id::new("raster-form")).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.heading(TITLE);
        ui.add_space(6.0);
        egui::Grid::new("raster-form-grid")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(f32::INFINITY));
                ui.end_row();

                ui.label(KIND);
                ui.horizontal_wrapped(|ui| {
                    for (index, preset) in PRESETS.iter().enumerate() {
                        let on = form.preset == index;
                        if ui.selectable_label(on, preset.name).clicked() && !on {
                            form.set_preset(index, state);
                        }
                    }
                });
                ui.end_row();

                if form.height() {
                    ui.label("Values");
                    ui.label(form.fixed_range());
                    ui.end_row();
                } else {
                    for (label, text) in [
                        (UNIT, &mut form.unit),
                        (MINIMUM, &mut form.min),
                        (MAXIMUM, &mut form.max),
                    ] {
                        ui.label(label);
                        ui.add(egui::TextEdit::singleline(text).desired_width(90.0));
                        ui.end_row();
                    }
                }

                ui.label("Face size");
                ui.vertical(|ui| {
                    for size in form.sizes.clone() {
                        let name = world::grouped(size as u64);
                        ui.radio_value(&mut form.face_size, size, name);
                        let [ground, detail] = face_size_lines(size, &world);
                        ui.weak(format!("{ground} · {detail}"));
                    }
                });
                ui.end_row();
            });
        if let Some(note) = form.note() {
            ui.colored_label(ui.visuals().warn_fg_color, note);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let add = egui::Button::new(ADD);
            if ui.add_enabled(form.spec().is_some(), add).clicked() {
                did = Did::Add;
            }
            if ui.button("Cancel").clicked() {
                did = Did::Close;
            }
        });
    });
    if modal.should_close() && did == Did::Nothing {
        did = Did::Close;
    }
    finish(state, form, did);
}

/// The form in the iPad layout: a card near the top of the screen, over a
/// dark fill that takes each touch. The keyboard of the system then does
/// not cover the card.
pub fn pad(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let Some(mut form) = state.raster_form.take() else {
        return;
    };
    let note = form.note();
    let world = state.doc.project.world.clone();
    // The rows are in three groups: the kind with its values, the name, and
    // the face size. The first frame of the form has an estimate of the
    // height of the rows, and each later frame has the height of the frame
    // before it.
    let kind_rows = if form.height() { 1.0 } else { 3.0 };
    let rows = 1.0 + kind_rows + form.sizes.len() as f32 + 1.0;
    let mut body = 2.0 * TOUCH + rows * PANEL_ROW + 3.0 * GROUP_GAP + LABEL + BUTTON_GAP + BOTTOM;
    if note.is_some() {
        body += NOTE;
    }
    let body_id = egui::Id::new("raster-form-body");
    if let Some(last) = ctx.data(|data| data.get_temp::<f32>(body_id)) {
        body = last;
    }
    let screen = rects.screen;
    let width = WIDTH.min(screen.width() - 2.0 * MARGIN);
    let most = rects.panel_bottom - rects.bands.top - GAP;
    let height = (ROW + body).min(most);
    let top = (rects.workspace.bottom() + GAP).min(rects.panel_bottom - height);
    let rect = Rect::from_min_size(
        pos2(screen.center().x - width / 2.0, top),
        vec2(width, height),
    );

    egui::Area::new(egui::Id::new("raster-form-scrim"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(120));
        });

    // The form covers each other control.
    controls.clear();
    let mut did = Did::Nothing;
    let radius = theme::CARD_RADIUS.into();
    widgets::card(ctx, None, CARD, rect, Order::Foreground, radius, |ui| {
        ui.painter()
            .rect_filled(rect, radius, theme::card(false).to_opaque());
        ui.spacing_mut().item_spacing.y = 0.0;
        if widgets::title_row(ui, controls, TITLE, None, false).close {
            did = Did::Close;
        }
        // A short screen scrolls the rows.
        let scroll = egui::ScrollArea::vertical()
            .id_salt(CARD)
            .max_height(height - ROW)
            .auto_shrink([false, false]);
        let out = scroll.show(ui, |ui| {
            let margin = egui::Margin::symmetric(BODY_PAD as i8, 0);
            egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let names = PRESETS.map(|preset| preset.name);
                if let Some(index) = widgets::chips(ui, controls, &names, form.preset) {
                    form.set_preset(index, state);
                }
                ui.add_space(GROUP_GAP);
                let valid = !form.name.trim().is_empty();
                widgets::text_field(ui, controls, NAME, "Name", None, &mut form.name, valid);
                if form.height() {
                    widgets::value_row(ui, "Values", &form.fixed_range());
                } else {
                    let range_ok = form.range().is_some();
                    widgets::text_field(ui, controls, UNIT, UNIT, None, &mut form.unit, true);
                    let unit = form.unit.trim().to_string();
                    let unit = Some(unit.as_str()).filter(|unit| !unit.is_empty());
                    for (name, text) in [(MINIMUM, &mut form.min), (MAXIMUM, &mut form.max)] {
                        widgets::text_field(ui, controls, name, name, unit, text, range_ok);
                    }
                }
                ui.add_space(GROUP_GAP);

                let (label, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), LABEL), egui::Sense::hover());
                ui.painter().text(
                    label.left_center(),
                    egui::Align2::LEFT_CENTER,
                    "Face size",
                    theme::body(),
                    theme::MUTE,
                );
                for size in form.sizes.clone() {
                    let title = world::grouped(size as u64);
                    let lines = face_size_lines(size, &world);
                    let name = face_size_name(size);
                    let on = form.face_size == size;
                    if widgets::option_row(ui, controls, &name, &title, &lines, on).clicked() {
                        form.face_size = size;
                    }
                }
                if let Some(note) = note {
                    widgets::form_note(ui, note, NOTE);
                }
                ui.add_space(BUTTON_GAP);
                let enabled = form.spec().is_some();
                if widgets::button_row(ui, controls, ADD, enabled).clicked() {
                    did = Did::Add;
                }
                ui.add_space(BOTTOM);
            });
        });
        ctx.data_mut(|data| data.insert_temp(body_id, out.content_size.y));
    });
    // A touch on the dark fill does not put it above the card.
    ctx.move_to_top(egui::LayerId::new(Order::Foreground, egui::Id::new(CARD)));
    // The panel of the new layer opens.
    if let Some(layer) = finish(state, form, did) {
        state.pad.layer = Some(layer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vale_store::Units;

    fn world(radius_km: f64, units: Units) -> World {
        World {
            radius_km,
            units,
            ..World::default()
        }
    }

    #[test]
    fn the_pixel_size_of_a_face_of_2048_on_earth_is_about_4_9_km() {
        let earth = world(6371.0, Units::Metric);
        assert_eq!(pixel_size_text(2048, &earth), "about 4.9 km");
    }

    #[test]
    fn the_values_next_to_each_face_size_follow_the_radius_of_the_world() {
        let earth = world(6371.0, Units::Metric);
        assert_eq!(
            face_size_text(4096, &earth),
            ["2.4 km", "16,384 × 8,192", "201 MB"]
        );
        assert_eq!(
            face_size_text(8192, &earth),
            ["1.2 km", "32,768 × 16,384", "805 MB"]
        );
        // On a world of half the radius, a pixel covers half the ground.
        // The map and the memory stay.
        let half = world(6371.0 / 2.0, Units::Metric);
        assert_eq!(
            face_size_text(4096, &half),
            ["1.2 km", "16,384 × 8,192", "201 MB"]
        );
        assert_eq!(
            face_size_text(8192, &half),
            ["610 m", "32,768 × 16,384", "805 MB"]
        );
        // The ground is in the display units of the world.
        let miles = world(6371.0, Units::Imperial);
        assert_eq!(
            face_size_text(4096, &miles),
            ["1.5 mi", "16,384 × 8,192", "201 MB"]
        );
        assert_eq!(face_size_text(8192, &miles)[0], "4,000 ft");
        assert_eq!(
            face_size_lines(4096, &earth),
            ["2.4 km per pixel", "16,384 × 8,192 · 201 MB"]
        );
    }

    #[test]
    fn the_form_offers_the_face_sizes_that_the_device_can_show() {
        let all = vec![1024, 2048, 4096, 8192];
        assert_eq!(face_sizes(None, usize::MAX), all);
        assert_eq!(face_sizes(None, 16384), all);
        assert_eq!(face_sizes(Some(4096), usize::MAX), all);
        // A device with a smaller limit has no larger size.
        assert_eq!(face_sizes(None, 4096), [1024, 2048, 4096]);
        assert_eq!(face_sizes(Some(8192), 2048), [1024, 2048]);
        assert_eq!(face_sizes(None, 512), [512]);
        // The face size of `--face-size` is in the list.
        assert_eq!(
            face_sizes(Some(256), usize::MAX),
            [256, 1024, 2048, 4096, 8192]
        );
        // The first size is the wanted one, or the next smaller one.
        assert_eq!(first_face_size(&all, 8192), 8192);
        assert_eq!(first_face_size(&all, 2048), 2048);
        assert_eq!(first_face_size(&[1024, 2048], 8192), 2048);
        assert_eq!(first_face_size(&[1024, 2048], 256), 1024);
        assert_eq!(first_face_size(&[256, 1024], 256), 256);
    }

    #[test]
    fn each_preset_has_a_range_with_bands() {
        for preset in PRESETS {
            let (min, max) = preset.range;
            let bands = Bands::with_range(min, max, preset.unit == HEIGHT_UNIT).unwrap();
            assert_eq!(bands.bands().len(), 8, "{}", preset.name);
            assert!(FACE_SIZES.contains(&preset.face_size), "{}", preset.name);
        }
        assert_eq!(
            (PRESETS[HEIGHT].name, PRESETS[HEIGHT].unit),
            ("Height", "m")
        );
        assert_eq!(PRESETS[CUSTOM].name, "Custom");
    }
}
