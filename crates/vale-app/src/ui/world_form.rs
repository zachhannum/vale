//! The form that makes a new world and that edits the world of the project.

use eframe::egui::{self, Order, Rect, pos2, vec2};
use vale_store::{Units, World};

use super::pad::geometry::{GAP, MARGIN, ROW, Rects};
use super::pad::theme;
use super::pad::widgets::{self, BODY_PAD, Controls, PANEL_ROW};
use super::{Action, AppState};
use crate::world;

/// The control that makes the world of the form.
pub const CREATE: &str = "Create world";
/// The control that opens the form of a new world.
pub const NEW_WORLD: &str = "New world…";
/// The title of the form of a new world.
pub const NEW_TITLE: &str = "New world";
/// The title of the form of the world of the project.
pub const PROPERTIES_TITLE: &str = "World";
pub const NAME: &str = "Name";
pub const RADIUS: &str = "Radius";
const WIDTH: f32 = 360.0;
/// The ID of the card of the form in the iPad layout.
const CARD: &str = "world-form";

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum FormMode {
    /// The form makes a world.
    New,
    /// The form edits the world of the project, and each change applies at once.
    Properties,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorldForm {
    pub mode: FormMode,
    pub name: String,
    /// The radius as the user typed it, in the large unit of `units`.
    pub radius: String,
    /// The last radius that `radius` gave, in kilometers.
    radius_km: f64,
    /// False: `radius` is not a radius.
    radius_ok: bool,
    pub units: Units,
    /// The form does not close until it makes a world, because the app has
    /// no world to go back to.
    pub required: bool,
}

/// What the user did on the form in one frame.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Did {
    Nothing,
    Close,
    Create,
    OpenNew,
}

impl WorldForm {
    fn new(mode: FormMode, world: &World) -> WorldForm {
        WorldForm {
            mode,
            name: world.name.clone(),
            radius: world::radius_text(world.radius_km, world.units),
            radius_km: world.radius_km,
            radius_ok: true,
            units: world.units,
            required: false,
        }
    }

    /// The form of a new world, with the name and the radius of the default world.
    pub fn new_world() -> WorldForm {
        WorldForm::new(FormMode::New, &World::default())
    }

    /// The form of the world of the project.
    pub fn properties(world: &World) -> WorldForm {
        WorldForm::new(FormMode::Properties, world)
    }

    pub fn open(mode: FormMode, world: &World) -> WorldForm {
        match mode {
            FormMode::New => WorldForm::new_world(),
            FormMode::Properties => WorldForm::properties(world),
        }
    }

    /// Reads the radius again. Call this after a change of `radius`.
    pub fn radius_changed(&mut self) {
        let km = world::parse_radius(&self.radius, self.units);
        self.radius_ok = km.is_some();
        self.radius_km = km.unwrap_or(self.radius_km);
    }

    /// The radius in kilometers. `None`: the text is not a radius.
    pub fn radius_km(&self) -> Option<f64> {
        self.radius_ok.then_some(self.radius_km)
    }

    pub fn set_radius_km(&mut self, km: f64) {
        self.radius_km = km;
        self.radius_ok = true;
        self.radius = world::radius_text(km, self.units);
    }

    /// Changes the units. The radius stays, and its text changes.
    pub fn set_units(&mut self, units: Units) {
        self.units = units;
        if self.radius_ok {
            self.radius = world::radius_text(self.radius_km, units);
        } else {
            self.radius_changed();
        }
    }

    /// The preset that has the radius of the form.
    pub fn preset(&self) -> Option<usize> {
        self.radius_km().and_then(world::preset_of)
    }

    /// The world of the form. `None`: the name is empty, or the radius is
    /// not a radius.
    pub fn world(&self) -> Option<World> {
        let name = self.name.trim();
        Some(World {
            name: (!name.is_empty()).then(|| name.to_string())?,
            radius_km: self.radius_km()?,
            units: self.units,
        })
    }

    /// The text of the unit of the radius.
    fn radius_unit(&self) -> &'static str {
        world::large_unit(self.units).1
    }

    /// The text that tells the limits of the radius, if the radius is wrong.
    fn radius_note(&self) -> Option<String> {
        (!self.radius_ok).then(|| {
            let unit = world::large_unit(self.units);
            let limit = |km: f64| world::radius_text(km, self.units);
            format!(
                "The radius is from {} to {} {}.",
                limit(*world::RADIUS_KM.start()),
                limit(*world::RADIUS_KM.end()),
                unit.1
            )
        })
    }

    /// Writes each valid field of a properties form to the world of the
    /// project.
    fn apply(&self, state: &mut AppState) {
        let name = self.name.trim();
        let world = &mut state.doc.project.world;
        if !name.is_empty() && name != world.name {
            world.name = name.to_string();
        }
        if self.units != world.units {
            world.units = self.units;
            state.touch();
        }
        if let Some(km) = self.radius_km()
            && km != state.doc.project.world.radius_km
        {
            state.doc.set_radius_km(km);
            state.touch();
        }
    }
}

/// Acts on what the user did, and keeps or closes the form.
fn finish(state: &mut AppState, mut form: WorldForm, did: Did) {
    match did {
        Did::Close if form.required => {}
        Did::Close => return,
        Did::OpenNew => form = WorldForm::new_world(),
        Did::Create => {
            let Some(world) = form.world() else {
                state.world_form = Some(form);
                return;
            };
            state.actions.push(Action::NewWorld(world));
            return;
        }
        Did::Nothing => {}
    }
    if form.mode == FormMode::Properties {
        form.apply(state);
    }
    state.world_form = Some(form);
}

/// The form in the desktop layout.
pub fn desktop(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut form) = state.world_form.take() else {
        return;
    };
    let mut did = Did::Nothing;
    let busy = state.globe.busy();
    let modal = egui::Modal::new(egui::Id::new("world-form")).show(ctx, |ui| {
        ui.set_width(300.0);
        let new = form.mode == FormMode::New;
        ui.heading(if new { NEW_TITLE } else { PROPERTIES_TITLE });
        ui.add_space(6.0);
        egui::Grid::new("world-form-grid")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label(NAME);
                ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(f32::INFINITY));
                ui.end_row();

                ui.label("Preset");
                ui.horizontal(|ui| {
                    let active = form.preset();
                    for (index, (name, km)) in world::PRESETS.into_iter().enumerate() {
                        if ui.selectable_label(active == Some(index), name).clicked() {
                            form.set_radius_km(km);
                        }
                    }
                });
                ui.end_row();

                ui.label(RADIUS);
                ui.horizontal(|ui| {
                    let edit = egui::TextEdit::singleline(&mut form.radius).desired_width(90.0);
                    let edit = if form.radius_ok {
                        edit
                    } else {
                        edit.text_color(ui.visuals().warn_fg_color)
                    };
                    if ui.add(edit).changed() {
                        form.radius_changed();
                    }
                    ui.label(form.radius_unit());
                });
                ui.end_row();

                ui.label("Units");
                ui.horizontal(|ui| {
                    for units in Units::ALL {
                        let name = world::units_name(units);
                        if ui.selectable_label(form.units == units, name).clicked() {
                            form.set_units(units);
                        }
                    }
                });
                ui.end_row();
            });
        if let Some(note) = form.radius_note() {
            ui.colored_label(ui.visuals().warn_fg_color, note);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if new {
                let create = egui::Button::new(CREATE);
                if ui
                    .add_enabled(form.world().is_some() && !busy, create)
                    .clicked()
                {
                    did = Did::Create;
                }
                if !form.required && ui.button("Cancel").clicked() {
                    did = Did::Close;
                }
            } else {
                if ui.button("Close").clicked() {
                    did = Did::Close;
                }
                if ui.button(NEW_WORLD).clicked() {
                    did = Did::OpenNew;
                }
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
    let Some(mut form) = state.world_form.take() else {
        return;
    };
    let new = form.mode == FormMode::New;
    let note = form.radius_note();
    let rows = 5.0 + if note.is_some() { 1.0 } else { 0.0 };
    let screen = rects.screen;
    let width = WIDTH.min(screen.width() - 2.0 * MARGIN);
    let height = (ROW + rows * PANEL_ROW + 12.0).min(rects.panel_bottom - rects.bands.top - GAP);
    let top = (rects.workspace.bottom() + GAP).min(rects.panel_bottom - height);
    let rect = Rect::from_min_size(
        pos2(screen.center().x - width / 2.0, top),
        vec2(width, height),
    );

    egui::Area::new(egui::Id::new("world-form-scrim"))
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
    let busy = state.globe.busy();
    let radius = theme::CARD_RADIUS.into();
    widgets::card(ctx, None, CARD, rect, Order::Foreground, radius, |ui| {
        ui.painter()
            .rect_filled(rect, radius, theme::card(false).to_opaque());
        ui.spacing_mut().item_spacing.y = 0.0;
        let title = if new { NEW_TITLE } else { PROPERTIES_TITLE };
        if widgets::title_row(ui, controls, title, None, false).close {
            did = Did::Close;
        }
        let inner = ui.available_rect_before_wrap().shrink2(vec2(BODY_PAD, 0.0));
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        let ui = &mut ui;
        ui.spacing_mut().item_spacing.y = 0.0;

        widgets::text_field(ui, controls, NAME, &mut form.name, true);
        let presets = world::PRESETS.map(|(name, _)| name);
        if let Some(index) = widgets::choice_row(ui, controls, &presets, form.preset()) {
            form.set_radius_km(world::PRESETS[index].1);
        }
        let name = format!("{RADIUS} ({})", form.radius_unit());
        let ok = form.radius_ok;
        if widgets::text_field(ui, controls, &name, &mut form.radius, ok).changed() {
            form.radius_changed();
        }
        let units = Units::ALL.map(world::units_name);
        let active = Units::ALL.iter().position(|u| *u == form.units);
        if let Some(index) = widgets::choice_row(ui, controls, &units, active) {
            form.set_units(Units::ALL[index]);
        }
        if let Some(note) = &note {
            let size = vec2(ui.available_width(), PANEL_ROW);
            ui.allocate_ui_with_layout(
                size,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_height(PANEL_ROW);
                    let text = egui::RichText::new(note)
                        .color(theme::WARN)
                        .font(theme::small());
                    ui.add(egui::Label::new(text).wrap());
                },
            );
        }
        if new {
            let enabled = form.world().is_some() && !busy;
            if widgets::button_row(ui, controls, CREATE, enabled).clicked() {
                did = Did::Create;
            }
        } else if widgets::text_row(ui, controls, NEW_WORLD, 0.0, false, true).clicked() {
            did = Did::OpenNew;
        }
    });
    // A touch on the dark fill does not put it above the card.
    ctx.move_to_top(egui::LayerId::new(Order::Foreground, egui::Id::new(CARD)));
    finish(state, form, did);
}
