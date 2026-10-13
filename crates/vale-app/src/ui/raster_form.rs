//! The form that adds a raster layer to the world.

use eframe::egui::{self, Order, Rect, pos2, vec2};
use vale_store::World;
use vale_terrain::{Bands, face_size_values};

use super::pad::geometry::{GAP, MARGIN, ROW, Rects};
use super::pad::theme;
use super::pad::widgets::{self, BODY_PAD, Controls, PANEL_ROW};
use super::{AppState, HEIGHT_NAME, LayerRef, RasterSpec};
use crate::world;

/// The title of the form.
pub const TITLE: &str = "New raster layer";
/// The control that adds the layer of the form.
pub const ADD: &str = "Add layer";
pub const NAME: &str = "Raster name";
pub const UNIT: &str = "Unit";
pub const MINIMUM: &str = "Minimum";
pub const MAXIMUM: &str = "Maximum";
/// The two kinds of a raster layer.
pub const KINDS: [&str; 2] = ["Height", "Other"];
/// The name of a new raster layer that is not a height.
pub const OTHER_NAME: &str = "Raster";
/// The face sizes that the form offers on each device that can show them.
pub const FACE_SIZES: [usize; 2] = [4096, 8192];
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

/// The face sizes that the form offers, in ascending order, and the size
/// that is on at first. `preferred` is the face size of the globe, and
/// `limit` is the largest face size that the device can show. A preferred
/// size that is not one of `FACE_SIZES` is in the list too.
pub fn face_sizes(preferred: usize, limit: usize) -> (Vec<usize>, usize) {
    let preferred = preferred.min(limit);
    let mut sizes: Vec<usize> = FACE_SIZES.into_iter().filter(|s| *s <= limit).collect();
    if preferred > 0 && !sizes.contains(&preferred) {
        sizes.push(preferred);
        sizes.sort_unstable();
    }
    (sizes, preferred)
}

/// The three values that describe a face size on a world: the ground that
/// one pixel covers at the equator, the equirectangular world map with the
/// same detail, and the memory of a layer with paint on each pixel.
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
    /// The name that the form gave. The name follows the kind until the
    /// user changes it.
    given: String,
    /// The layer is a height in meters.
    pub height: bool,
    pub unit: String,
    /// The value of the lowest level, as the user typed it.
    pub min: String,
    /// The value of the highest level, as the user typed it.
    pub max: String,
    /// The face sizes that the form offers.
    pub sizes: Vec<usize>,
    pub face_size: usize,
}

/// What the user did on the form in one frame.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Did {
    Nothing,
    Close,
    Add,
}

impl RasterForm {
    /// The form of a new height layer for the world of the app.
    pub fn open(state: &AppState) -> RasterForm {
        let globe = &state.globe;
        let (sizes, face_size) = face_sizes(globe.new_face_size(), globe.max_face_size());
        let name = state.raster_name(HEIGHT_NAME);
        RasterForm {
            given: name.clone(),
            name,
            height: true,
            unit: "mm".to_string(),
            min: "0".to_string(),
            max: "4000".to_string(),
            sizes,
            face_size,
        }
    }

    /// Sets the kind. A name that the user did not change follows the kind.
    pub fn set_height(&mut self, height: bool, state: &AppState) {
        self.height = height;
        if self.name == self.given {
            let name = if height { HEIGHT_NAME } else { OTHER_NAME };
            self.name = state.raster_name(name);
            self.given = self.name.clone();
        }
    }

    /// The value range of a layer that is not a height. `None`: a text is
    /// not a number, or the range is empty.
    fn range(&self) -> Option<(f64, f64)> {
        let number = |text: &str| text.trim().replace(',', "").parse::<f64>().ok();
        let (min, max) = (number(&self.min)?, number(&self.max)?);
        Bands::with_range(min, max).map(|_| (min, max))
    }

    /// The text that tells what is wrong with the form.
    pub fn note(&self) -> Option<&'static str> {
        if self.name.trim().is_empty() {
            Some("The layer must have a name")
        } else if self.height {
            None
        } else if self.unit.trim().is_empty() {
            Some("The layer must have a unit")
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
        if self.height {
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

                ui.label("Kind");
                ui.horizontal(|ui| {
                    for (name, height) in KINDS.into_iter().zip([true, false]) {
                        if ui.selectable_label(form.height == height, name).clicked() {
                            form.set_height(height, state);
                        }
                    }
                });
                ui.end_row();

                if !form.height {
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
    // The rows are in three groups: the name, the kind with its values, and
    // the face size.
    let kind_rows = if form.height { 1.0 } else { 4.0 };
    let rows = 1.0 + kind_rows + form.sizes.len() as f32 + 1.0;
    let mut body = rows * PANEL_ROW + 2.0 * GROUP_GAP + LABEL + BUTTON_GAP + BOTTOM;
    if note.is_some() {
        body += NOTE;
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
        scroll.show(ui, |ui| {
            let margin = egui::Margin::symmetric(BODY_PAD as i8, 0);
            egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let valid = !form.name.trim().is_empty();
                widgets::text_field(ui, controls, NAME, "Name", None, &mut form.name, valid);
                ui.add_space(GROUP_GAP);

                let active = Some(usize::from(!form.height));
                if let Some(index) = widgets::choice_row(ui, controls, "Kind", &KINDS, active) {
                    form.set_height(index == 0, state);
                }
                if !form.height {
                    let unit_ok = !form.unit.trim().is_empty();
                    let range_ok = form.range().is_some();
                    widgets::text_field(ui, controls, UNIT, UNIT, None, &mut form.unit, unit_ok);
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
        assert_eq!(face_sizes(8192, usize::MAX), (vec![4096, 8192], 8192));
        assert_eq!(face_sizes(8192, 16384), (vec![4096, 8192], 8192));
        assert_eq!(face_sizes(4096, usize::MAX), (vec![4096, 8192], 4096));
        // A device with a smaller limit has no larger size.
        assert_eq!(face_sizes(8192, 4096), (vec![4096], 4096));
        assert_eq!(face_sizes(8192, 2048), (vec![2048], 2048));
        // The face size of `--face-size` is in the list, and it is on.
        assert_eq!(face_sizes(256, usize::MAX), (vec![256, 4096, 8192], 256));
    }
}
