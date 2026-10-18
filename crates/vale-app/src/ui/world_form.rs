//! The form that makes a new world and that edits the world of the project.

use eframe::egui::{self, Order, Rect, pos2, vec2};
use vale_store::{Units, World};

use super::desk::float::{self, Glass, SheetButton};
use super::desk::widgets::Segment;
use super::pad::geometry::{GAP, MARGIN, ROW, Rects, TOUCH};
use super::pad::theme;
use super::pad::widgets::{self, BODY_PAD, Controls, PANEL_ROW};
use super::{Action, AppState, desk};
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
/// The name of the slider of the radius in the iPad layout.
pub const RADIUS_SLIDER: &str = "Radius slider";
const UNITS: &str = "Units";
/// The button that saves the desktop form of the world of the project.
pub const SAVE: &str = "Save";
pub const CANCEL: &str = "Cancel";
/// The row of the form that picks the folder of a new world.
pub const LOCATION: &str = "Location";
const WIDTH: f32 = 400.0;
/// The space between two groups of rows of the form in the iPad layout.
const GROUP_GAP: f32 = 10.0;
/// The space above the button that makes the world.
const BUTTON_GAP: f32 = 14.0;
/// The height of the text below a wrong radius.
const NOTE: f32 = 22.0;
/// The space below the last row.
const BOTTOM: f32 = 16.0;
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
    /// The desktop form writes the world of the project.
    Save,
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
            seed: 0,
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
                "From {} to {} {}",
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

/// Acts on what the user did, and keeps or closes the form. `live`: each
/// change of a properties form applies at once.
fn finish(state: &mut AppState, form: WorldForm, did: Did, live: bool) {
    match did {
        Did::Close if form.required => {}
        Did::Close => return,
        Did::Save => {
            form.apply(state);
            return;
        }
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
    if live && form.mode == FormMode::Properties {
        form.apply(state);
    }
    state.world_form = Some(form);
}

/// The width of the labels of a desktop form.
pub(super) const LABEL: f32 = 76.0;

/// A row of a desktop form: the label, and the controls at its right.
pub(super) fn row<R>(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.set_min_height(desk::theme::ROW);
        let (rect, _) = ui.allocate_exact_size(vec2(LABEL, desk::theme::ROW), egui::Sense::hover());
        let at = pos2(rect.left(), rect.center().y);
        let font = desk::theme::label();
        let color = desk::theme::MUTE;
        ui.painter()
            .text(at, egui::Align2::LEFT_CENTER, label, font, color);
        add(ui)
    })
    .inner
}

/// The form in the desktop layout: a sheet of the window. A properties
/// form writes the world when the user saves it.
pub fn desktop(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut form) = state.world_form.take() else {
        return;
    };
    let new = form.mode == FormMode::New;
    let busy = state.globe.busy();
    let glass = Glass::of(state);
    let top = desk::bar_bottom(state);
    let title = if new { NEW_TITLE } else { PROPERTIES_TITLE };
    let locations = state.projects.available();
    let locations = if new && locations.len() > 1 {
        locations
    } else {
        Vec::new()
    };
    let (clicked, keys) = float::sheet(ctx, glass, top, CARD, title, |ui| {
        let width = ui.available_width() - LABEL - 8.0;
        row(ui, NAME, |ui| {
            let id = egui::Id::new("world-form-name");
            desk::widgets::text_field(ui, id, NAME, &mut form.name, width, false);
        });
        row(ui, "Preset", |ui| {
            let items = world::PRESETS.map(|(name, _)| Segment::new(name));
            if let Some(index) = desk::widgets::segments(ui, &items, form.preset()) {
                form.set_radius_km(world::PRESETS[index].1);
            }
        });
        row(ui, RADIUS, |ui| {
            let id = egui::Id::new("world-form-radius");
            let ok = form.radius_ok;
            let field = desk::widgets::text_field(ui, id, RADIUS, &mut form.radius, 96.0, !ok);
            if field.changed() {
                form.radius_changed();
            }
            let unit = egui::RichText::new(form.radius_unit())
                .font(desk::theme::label())
                .color(desk::theme::MUTE);
            ui.label(unit);
        });
        if let Some(note) = form.radius_note() {
            row(ui, "", |ui| {
                let text = egui::RichText::new(note)
                    .font(desk::theme::small())
                    .color(desk::theme::WARN);
                ui.label(text);
            });
        }
        row(ui, UNITS, |ui| {
            let items = Units::ALL.map(|units| Segment::new(world::units_name(units)));
            let active = Units::ALL.iter().position(|u| *u == form.units);
            if let Some(index) = desk::widgets::segments(ui, &items, active) {
                form.set_units(Units::ALL[index]);
            }
        });
        if !locations.is_empty() {
            row(ui, LOCATION, |ui| {
                let items: Vec<_> = locations
                    .iter()
                    .map(|l| Segment::new(l.label(false)))
                    .collect();
                let active = locations.iter().position(|l| *l == state.projects.choice());
                if let Some(index) = desk::widgets::segments(ui, &items, active) {
                    state.projects.set_choice(locations[index]);
                }
            });
        }
        let ready = form.world().is_some();
        let (label, enabled) = match new {
            true => (CREATE, ready && !busy),
            false => (SAVE, ready),
        };
        let cancel = SheetButton {
            label: CANCEL,
            enabled: true,
            primary: false,
        };
        let confirm = SheetButton {
            label,
            enabled,
            primary: true,
        };
        let buttons = if form.required {
            vec![confirm]
        } else {
            vec![cancel, confirm]
        };
        let last = buttons.len() - 1;
        float::buttons(ui, &buttons).map(|index| (index == last, enabled))
    });
    let confirm = |enabled: bool| match new {
        true => Did::Create,
        false if enabled => Did::Save,
        false => Did::Nothing,
    };
    let did = match clicked {
        Some((true, enabled)) => confirm(enabled),
        Some((false, _)) => Did::Close,
        None if keys.cancel => Did::Close,
        None if keys.confirm => {
            let enabled = form.world().is_some() && (!new || !busy);
            if enabled { confirm(true) } else { Did::Nothing }
        }
        None => Did::Nothing,
    };
    finish(state, form, did, false);
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
    // The rows are in three groups: the name, the radius, and the units.
    let slider = TOUCH + widgets::TICK_NAMES;
    let mut height = ROW + 3.0 * PANEL_ROW + slider + 2.0 * GROUP_GAP + BOTTOM;
    if note.is_some() {
        height += NOTE;
    }
    if new {
        height += BUTTON_GAP + PANEL_ROW;
    }
    // The row of the location is a fourth group.
    let locations = state.projects.available();
    let locations = if new { locations } else { Vec::new() };
    if !locations.is_empty() {
        height += GROUP_GAP + PANEL_ROW;
    }
    let screen = rects.screen;
    let width = WIDTH.min(screen.width() - 2.0 * MARGIN);
    let height = height.min(rects.panel_bottom - rects.bands.top - GAP);
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

        widgets::text_field(ui, controls, NAME, NAME, None, &mut form.name, true);
        ui.add_space(GROUP_GAP);

        // The slider is the control of the radius. Its scale has each
        // preset at its place, and a tap on the value starts an edit.
        let unit = form.radius_unit();
        let name = format!("{RADIUS} ({unit})");
        let ok = form.radius_ok;
        let radius = &mut form.radius;
        if widgets::value_field(ui, controls, &name, RADIUS, unit, radius, ok).changed() {
            form.radius_changed();
        }
        if let Some(note) = &note {
            widgets::form_note(ui, note, NOTE);
        }
        let ticks = world::PRESETS.map(|(name, km)| (world::radius_fraction(km), name));
        let fraction = world::radius_fraction(form.radius_km);
        let slider =
            widgets::tick_slider(ui, controls, RADIUS_SLIDER, fraction, &ticks, form.preset());
        if let Some(fraction) = slider {
            form.set_radius_km(world::slider_radius(fraction, form.units));
        }
        ui.add_space(GROUP_GAP);

        let units = Units::ALL.map(world::units_name);
        let active = Units::ALL.iter().position(|u| *u == form.units);
        if let Some(index) = widgets::choice_row(ui, controls, UNITS, &units, active) {
            form.set_units(Units::ALL[index]);
        }
        if !locations.is_empty() {
            ui.add_space(GROUP_GAP);
            let names: Vec<&str> = locations.iter().map(|l| l.label(true)).collect();
            let active = locations.iter().position(|l| *l == state.projects.choice());
            if let Some(index) = widgets::choice_row(ui, controls, LOCATION, &names, active) {
                state.projects.set_choice(locations[index]);
            }
        }
        if new {
            ui.add_space(BUTTON_GAP);
            let enabled = form.world().is_some() && !busy;
            if widgets::button_row(ui, controls, CREATE, enabled).clicked() {
                did = Did::Create;
            }
        }
    });
    // A touch on the dark fill does not put it above the card.
    ctx.move_to_top(egui::LayerId::new(Order::Foreground, egui::Id::new(CARD)));
    finish(state, form, did, true);
}
