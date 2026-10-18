//! The dock at the right edge: the tabs, the list, and the inspector of the
//! selection.

use eframe::egui::{self, Align, Layout, Sense, Stroke, UiBuilder, pos2, vec2};
use vale_store::{LayerId, RasterId};

use super::command::{self, Command};
use super::field::{self, Field, Scale};
use super::float::{self, Glass, Item};
use super::widgets;
use super::{Inspect, Tab, brush, theme};
use crate::ui::pad::icons::Icon;
use crate::ui::pad::{self, ADD_LAYER, NEW_LAYERS, layer_icon, note_text};
use crate::ui::raster_form::RasterForm;
use crate::ui::{Action, AppState, LayerRef, Workspace, elevation, inspector, is_height, panels};
use crate::world;

/// The heading of the inspector of the Toolbox tab.
pub const BRUSH_DEBUG: &str = "Brush debug";
/// The heading of the inspector of the Maps workspace with no selection.
pub const FRAME: &str = "Frame";
/// The button of the world inspector that opens the form of the world.
pub const WORLD_PROPERTIES: &str = "World properties…";
/// The button of the world inspector that imports a heightmap.
pub const IMPORT: &str = "Import…";
/// The field of the panel of a raster layer with its opacity.
pub const OPACITY: &str = "Opacity";
/// The width of the label of a row of the inspector.
const LABEL: f32 = 76.0;
const INSET: f32 = 14.0;
/// The list takes at most this part of the height of the dock.
const LIST_SHARE: f32 = 0.45;

/// What the inspector shows.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Selected {
    /// The world, or in the Maps workspace the map frame.
    Nothing,
    Raster(RasterId),
    Layer(LayerId),
    /// A feature on the map of the Maps workspace.
    Feature,
    /// The settings of the brush.
    Brush,
}

/// The subject of the inspector.
pub fn selected(state: &AppState) -> Selected {
    let desk = &state.desk;
    let brush = state.workspace == Workspace::Globe && state.globe.brushing();
    match desk.inspect == Inspect::Brush && brush {
        true => Selected::Brush,
        false => selection(state),
    }
}

/// The selection of the layer list and of the map.
fn selection(state: &AppState) -> Selected {
    if state.workspace == Workspace::Map && state.selection.is_some() {
        return Selected::Feature;
    }
    // The map of the Maps workspace has no raster layer.
    let world = state.workspace == Workspace::Globe;
    match state.selected_layer {
        Some(LayerRef::Raster(id)) if world && state.doc.project.raster(id).is_some() => {
            Selected::Raster(id)
        }
        Some(LayerRef::Vector(id)) if state.doc.layer(id).is_some() => Selected::Layer(id),
        _ => Selected::Nothing,
    }
}

/// The selection of a layer.
fn selected_of(layer: LayerRef) -> Selected {
    match layer {
        LayerRef::Raster(id) => Selected::Raster(id),
        LayerRef::Vector(id) => Selected::Layer(id),
    }
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if !state.desk.dock_open {
        return;
    }
    let frame = egui::Frame::NONE.fill(theme::glass(state.translucent));
    egui::Panel::right("desk-dock")
        .exact_size(theme::DOCK)
        .resizable(false)
        .frame(frame)
        .show_separator_line(false)
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let stroke = Stroke::new(1.0, theme::line());
            ui.painter()
                .vline(rect.left() + 0.5, rect.y_range(), stroke);
            ui.spacing_mut().item_spacing.y = 0.0;
            tabs(ui, state);
            let height = rect.height() * LIST_SHARE;
            match state.desk.tab {
                Tab::Layers => {
                    list(ui, state, height);
                    let name = inspector_name(state);
                    inspector(ui, &name, |ui| layer_inspector(ui, state));
                }
                Tab::Toolbox => {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(INSET);
                        let text = egui::RichText::new("The toolbox has no tools yet.")
                            .font(theme::label())
                            .color(theme::MUTE);
                        ui.label(text);
                    });
                    ui.add_space(8.0);
                    inspector(ui, BRUSH_DEBUG, |ui| {
                        // The frame time needs a new frame at each refresh of
                        // the screen.
                        if !state.headless {
                            ui.ctx().request_repaint();
                        }
                        panels::debug_controls(ui, state);
                    });
                }
            }
        });
}

/// The row of the tabs, and the button of the new layers.
fn tabs(ui: &mut egui::Ui, state: &mut AppState) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, theme::TABS), Sense::hover());
    let stroke = Stroke::new(1.0, theme::line());
    ui.painter()
        .hline(rect.x_range(), rect.bottom() - 0.5, stroke);
    let inner = egui::Rect::from_min_max(rect.min + vec2(8.0, 0.0), rect.max - vec2(6.0, 0.0));
    let layout = Layout::left_to_right(Align::Center);
    ui.scope_builder(UiBuilder::new().max_rect(inner).layout(layout), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for tab in [Tab::Layers, Tab::Toolbox] {
            let label = match tab {
                Tab::Layers => Command::Layers.label(),
                Tab::Toolbox => Command::Toolbox.label(),
            };
            if widgets::tab(ui, label, state.desk.tab == tab).clicked() {
                state.desk.tab = tab;
            }
        }
        if state.desk.tab != Tab::Layers {
            return;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let button = widgets::tool_button(ui, ADD_LAYER, Icon::Plus, ADD_LAYER, None, true);
            let glass = Glass::of(state);
            float::menu(&button, glass, |ui| {
                ui.set_min_width(160.0);
                if float::item(ui, Item::new(Command::AddRaster.label())).clicked() {
                    state.raster_form = Some(RasterForm::open(state));
                }
                float::separator(ui);
                for (name, kind) in NEW_LAYERS {
                    if float::item(ui, Item::new(name)).clicked() {
                        command::add_layer(state, kind);
                    }
                }
                // A world with no height layer gets one from an import.
                if !state.doc.project.rasters().iter().any(is_height) {
                    float::separator(ui);
                    let enabled = !state.globe.busy();
                    let item = Item::new(pad::IMPORT_HEIGHTMAP).enabled(enabled);
                    if float::item(ui, item).clicked() {
                        state.actions.push(Action::ImportHeightmapDialog);
                    }
                }
            });
        });
    });
}

/// The rows of the layers in draw order, with the bottom layer first. The
/// Maps workspace lists the vector layers only. A click on the selected row
/// selects nothing, or shows its panel if the Brush panel shows.
fn list(ui: &mut egui::Ui, state: &mut AppState, height: f32) {
    let chosen = selection(state);
    let shown = selected(state) != Selected::Brush;
    egui::ScrollArea::vertical()
        .id_salt("desk-list")
        .max_height(height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(4.0);
            let world = state.workspace == Workspace::Globe;
            let rows: Vec<_> = state
                .doc
                .project
                .layers()
                .iter()
                .filter(|l| world || l.raster().is_none())
                .map(|l| {
                    (
                        l.id,
                        LayerRef::of(l),
                        l.name.clone(),
                        layer_icon(l),
                        l.visible,
                    )
                })
                .collect();
            if rows.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(INSET);
                    let text = egui::RichText::new(pad::NO_LAYERS)
                        .font(theme::label())
                        .color(theme::MUTE);
                    ui.label(text);
                });
            }
            for (id, layer, name, icon, visible) in rows {
                let on = chosen == selected_of(layer);
                let row = ui
                    .push_id(id.0, |ui| {
                        widgets::list_row(ui, icon, &name, on, Some(visible), true)
                    })
                    .inner;
                if row.eye {
                    state.set_layer_visible(id, !visible);
                } else if row.body.clicked() {
                    select(state, (!on || !shown).then_some(layer));
                }
            }
            ui.add_space(4.0);
        });
}

/// Selects a layer, or nothing.
fn select(state: &mut AppState, layer: Option<LayerRef>) {
    state.desk.inspect = Inspect::Selection;
    state.select(layer);
    if state.selection.is_some() {
        state.selection = None;
        state.touch();
    }
}

fn inspector_name(state: &AppState) -> String {
    match selected(state) {
        Selected::Nothing => match state.workspace {
            Workspace::Globe => state.doc.project.world.name.clone(),
            Workspace::Map => FRAME.to_owned(),
        },
        Selected::Raster(id) => state
            .doc
            .project
            .raster_layer(id)
            .map_or(String::new(), |l| l.name.clone()),
        Selected::Layer(id) => state
            .doc
            .layer(id)
            .map_or(String::new(), |l| l.name.clone()),
        Selected::Feature => "Feature".to_owned(),
        Selected::Brush => brush::title(state.globe.brush.mode),
    }
}

/// The inspector below the list: a heading with `name`, and a body that
/// scrolls.
fn inspector(ui: &mut egui::Ui, name: &str, body: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
    let stroke = Stroke::new(1.0, theme::line());
    ui.painter().hline(rect.x_range(), rect.top() + 0.5, stroke);
    ui.painter().text(
        pos2(rect.left() + INSET, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        theme::body(),
        theme::TEXT,
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, name));
    ui.ctx().accesskit_node_builder(resp.id, |node| {
        node.set_role(egui::accesskit::Role::Heading);
    });
    egui::ScrollArea::vertical()
        .id_salt("desk-inspector")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: INSET as i8,
                    right: INSET as i8,
                    top: 0,
                    bottom: 12,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.set_width(ui.available_width());
                    body(ui);
                });
        });
}

fn layer_inspector(ui: &mut egui::Ui, state: &mut AppState) {
    match (selected(state), state.workspace) {
        (Selected::Nothing, Workspace::Globe) => world_body(ui, state),
        (Selected::Nothing, Workspace::Map) => frame_body(ui, state),
        (Selected::Raster(id), _) => raster_body(ui, state, id),
        (Selected::Layer(id), Workspace::Globe) => vector_body(ui, state, id),
        (Selected::Layer(_), Workspace::Map) => inspector::layer_body(ui, state),
        (Selected::Feature, _) => inspector::feature_body(ui, state),
        (Selected::Brush, _) => brush::body(ui, state),
    }
}

/// The label at the left of a row of the inspector.
fn row_label(ui: &mut egui::Ui, label: &str) {
    let size = vec2(LABEL, theme::ROW);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(size);
        ui.label(egui::RichText::new(label).color(theme::MUTE));
    });
}

/// A row with a label at the left and a value. `mono`: the value is a
/// number.
fn value_row(ui: &mut egui::Ui, label: &str, value: &str, mono: bool) {
    ui.horizontal(|ui| {
        row_label(ui, label);
        let value = match mono {
            true => egui::RichText::new(value)
                .font(theme::mono())
                .color(theme::MUTE),
            false => egui::RichText::new(value).color(theme::TEXT),
        };
        ui.label(value);
    });
}

/// The small heading of a part of the inspector.
fn subheading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    let text = egui::RichText::new(text)
        .font(theme::small())
        .strong()
        .color(theme::MUTE);
    ui.label(text);
}

/// A number with a comma between each group of three digits.
pub fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The properties of the world, and the import of a heightmap.
fn world_body(ui: &mut egui::Ui, state: &mut AppState) {
    let world = &state.doc.project.world;
    let symbol = world::large_unit(world.units).1;
    let radius = world::radius_text(world.radius_km, world.units);
    let degree = std::f64::consts::TAU * world.radius_km / 360.0;
    value_row(ui, "Name", &world.name.clone(), false);
    value_row(ui, "Radius", &format!("{radius} {symbol}"), true);
    value_row(ui, "1°", &world::distance(degree, world.units), true);
    if widgets::button(ui, WORLD_PROPERTIES, WORLD_PROPERTIES, None).clicked() {
        command::run(Command::WorldProperties, ui.ctx(), state);
    }
    subheading(ui, "Heightmap");
    ui.horizontal(|ui| import_button(ui, state, IMPORT));
    let progress = state.globe.import_progress();
    note_text(ui, progress, state.import_note.as_ref());
}

/// The button that imports a heightmap into a height layer.
fn import_button(ui: &mut egui::Ui, state: &mut AppState, label: &str) {
    let tip = Command::ImportHeightmap.tip(ui.ctx());
    let busy = state.globe.busy();
    let resp = ui.add_enabled_ui(!busy, |ui| widgets::button(ui, label, &tip, None));
    if resp.inner.clicked() {
        state.actions.push(Action::ImportHeightmapDialog);
    }
}

/// The panel of a raster layer: its name, its value range, its face size,
/// its opacity, the import into a height layer, the export, the button that
/// deletes it, and the bands of the layer.
fn raster_body(ui: &mut egui::Ui, state: &mut AppState, id: RasterId) {
    let (Some(raster), Some(layer)) = (
        state.doc.project.raster(id),
        state.doc.project.raster_layer(id),
    ) else {
        return;
    };
    let (unit, height, opacity) = (raster.unit.clone(), is_height(raster), raster.opacity);
    let span = raster.value_max - raster.value_min;
    let range = format!(
        "{} to {}",
        world::value_text(raster.value_min, span, ""),
        world::value_text(raster.value_max, span, &unit)
    );
    let face_size = grouped(raster.face_size as usize);
    let layer = layer.id;
    name_row(ui, state, layer);
    value_row(ui, "Values", &range, true);
    value_row(ui, "Face size", &face_size, true);
    let percent = f64::from(opacity) * 100.0;
    let opacity = Field {
        name: OPACITY,
        value: percent,
        scale: Scale::linear(0.0..=100.0),
        text: format!("{percent:.0}%"),
        units: vec![("%".into(), 1.0)],
        step: 1.0,
        enabled: true,
    };
    if let Some(value) = field::row(ui, &opacity).value {
        state.set_raster_opacity(id, (value / 100.0) as f32);
    }
    ui.horizontal(|ui| {
        if height {
            import_button(ui, state, pad::IMPORT_HEIGHTMAP);
        }
        let export = Command::ExportGeoTiff;
        let tip = export.tip(ui.ctx());
        let resp = ui.add_enabled_ui(state.can_export(), |ui| {
            widgets::button(ui, export.label(), &tip, None)
        });
        if resp.inner.clicked() {
            state.actions.push(Action::ExportHeightmapDialog);
        }
    });
    if height {
        let progress = state.globe.import_progress();
        note_text(ui, progress, state.import_note.as_ref());
    }
    note_text(
        ui,
        state.globe.export_progress(),
        state.export_note.as_ref(),
    );
    let delete = pad::DELETE_LAYER;
    if widgets::button(ui, delete, delete, None).clicked() {
        command::run(Command::DeleteLayer, ui.ctx(), state);
        return;
    }
    subheading(ui, "Style");
    ui.checkbox(&mut state.globe.preview.greyscale, "Greyscale");
    let mut preview = state.globe.preview;
    if let Some(bands) = state.globe.bands_mut(id) {
        elevation::controls(ui, bands, &mut preview, &unit);
    }
    state.globe.preview = preview;
}

/// The field with the name of a layer of each kind.
fn name_row(ui: &mut egui::Ui, state: &mut AppState, id: LayerId) {
    let Some(layer) = state.doc.layer(id) else {
        return;
    };
    // The text of the field, from the first key to the end of the edit.
    let key = ui.id().with(("desk-layer-name", id.0));
    let mut name = ui
        .data(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| layer.name.clone());
    ui.horizontal(|ui| {
        row_label(ui, "Name");
        let edit = egui::TextEdit::singleline(&mut name).hint_text(pad::LAYER_NAME);
        let field = ui.add(edit);
        ui.ctx()
            .accesskit_node_builder(field.id, |node| node.set_label(pad::LAYER_NAME));
        if field.lost_focus() {
            if !name.trim().is_empty() {
                state.doc.rename_layer(id, &name);
            }
            ui.data_mut(|d| d.remove::<String>(key));
        } else if field.has_focus() {
            ui.data_mut(|d| d.insert_temp(key, name));
        }
    });
}

/// The panel of a vector layer in the World workspace: its name, its kind,
/// the number of its features, and the button that deletes it.
fn vector_body(ui: &mut egui::Ui, state: &mut AppState, id: LayerId) {
    let Some(layer) = state.doc.layer(id) else {
        return;
    };
    let kind = NEW_LAYERS
        .iter()
        .find(|(_, k)| Some(*k) == layer.geometry())
        .map_or("", |(name, _)| name);
    let features = grouped(layer.features.len());
    name_row(ui, state, id);
    value_row(ui, "Type", kind, false);
    value_row(ui, "Features", &features, true);
    let delete = pad::DELETE_LAYER;
    if widgets::button(ui, delete, delete, None).clicked() {
        command::run(Command::DeleteLayer, ui.ctx(), state);
    }
}

/// The panel of the map frame: the projection, the graticule, the labels,
/// and the labels that did not fit.
fn frame_body(ui: &mut egui::Ui, state: &mut AppState) {
    panels::projection_section(ui, state);
    panels::map_section(ui, state);
    inspector::unplaced_section(ui, state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_has_a_comma_between_the_groups_of_three_digits() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(512), "512");
        assert_eq!(grouped(4096), "4,096");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}
