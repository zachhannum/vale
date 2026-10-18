//! The dock at the right edge: the tabs, the list, and the inspector of the
//! selection.

use eframe::egui::{self, Align, Layout, Sense, Stroke, UiBuilder, pos2, vec2};
use vale_store::{GeometryKind, LayerId};

use super::command::{self, Command};
use super::float::{self, Glass, Item};
use super::widgets;
use super::{Tab, theme};
use crate::ui::pad::icons::Icon;
use crate::ui::pad::{self, ADD_LAYER, NEW_LAYERS, kind_icon, note_text};
use crate::ui::{Action, AppState, Workspace, elevation, inspector, panels};
use crate::world;

/// The heading of the inspector of the Toolbox tab.
pub const BRUSH_DEBUG: &str = "Brush debug";
/// The heading of the inspector of the Maps workspace with no selection.
pub const FRAME: &str = "Frame";
/// The button of the world inspector that opens the form of the world.
pub const WORLD_PROPERTIES: &str = "World properties…";
/// The button of the world inspector that imports a heightmap.
pub const IMPORT: &str = "Import…";
/// The name of the Height layer.
pub const HEIGHT: &str = "Height";
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
    Height,
    Layer(LayerId),
    /// A feature on the map of the Maps workspace.
    Feature,
}

/// The selection of the layer list and of the map.
pub fn selected(state: &AppState) -> Selected {
    if state.workspace == Workspace::Map && state.selection.is_some() {
        return Selected::Feature;
    }
    match state.selected_layer {
        Some(id) if state.doc.layer(id).is_some() => Selected::Layer(id),
        _ if state.desk.height_selected && state.has_raster() => Selected::Height,
        _ => Selected::Nothing,
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
                ui.set_min_width(140.0);
                for (name, kind) in NEW_LAYERS {
                    if float::item(ui, Item::new(name)).clicked() {
                        add_layer(state, kind);
                    }
                }
            });
        });
    });
}

fn add_layer(state: &mut AppState, kind: GeometryKind) {
    state.selected_layer = Some(state.doc.add_layer(kind));
    state.selection = None;
    state.touch();
}

/// The rows of the layers in draw order, with the bottom layer first. The
/// first row is the heightmap, or the row that imports one.
fn list(ui: &mut egui::Ui, state: &mut AppState, height: f32) {
    let chosen = selected(state);
    egui::ScrollArea::vertical()
        .id_salt("desk-list")
        .max_height(height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(4.0);
            if state.has_raster() {
                let on = chosen == Selected::Height;
                let row = widgets::list_row(ui, Icon::Raster, HEIGHT, on, None, true);
                if row.body.clicked() {
                    select(state, (!on).then_some(Selected::Height));
                }
            } else {
                let enabled = !state.globe.busy();
                let name = pad::IMPORT_HEIGHTMAP;
                let row = widgets::list_row(ui, Icon::Raster, name, false, None, enabled);
                if row.body.clicked() {
                    state.actions.push(Action::ImportHeightmapDialog);
                    select(state, Some(Selected::Height));
                }
            }
            let rows: Vec<_> = state
                .doc
                .project
                .layers()
                .iter()
                .map(|l| (l.id, l.name.clone(), kind_icon(l.kind), l.visible))
                .collect();
            for (id, name, icon, visible) in rows {
                let on = chosen == Selected::Layer(id);
                let row = ui
                    .push_id(id.0, |ui| {
                        widgets::list_row(ui, icon, &name, on, Some(visible), true)
                    })
                    .inner;
                if row.eye {
                    state.doc.set_visible(id, !visible);
                    state.touch();
                } else if row.body.clicked() {
                    select(state, (!on).then_some(Selected::Layer(id)));
                }
            }
            ui.add_space(4.0);
        });
}

/// Selects a row of the list, or nothing. A click on the selected row
/// selects nothing.
fn select(state: &mut AppState, selected: Option<Selected>) {
    state.desk.height_selected = selected == Some(Selected::Height);
    state.selected_layer = match selected {
        Some(Selected::Layer(id)) => Some(id),
        _ => None,
    };
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
        Selected::Height => HEIGHT.to_owned(),
        Selected::Layer(id) => state
            .doc
            .layer(id)
            .map_or(String::new(), |l| l.name.clone()),
        Selected::Feature => "Feature".to_owned(),
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
        (Selected::Height, _) => height_body(ui, state),
        (Selected::Layer(id), Workspace::Globe) => vector_body(ui, state, id),
        (Selected::Layer(_), Workspace::Map) => inspector::layer_body(ui, state),
        (Selected::Feature, _) => inspector::feature_body(ui, state),
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

/// The properties of the world, and the import and the export of the
/// heightmap.
fn world_body(ui: &mut egui::Ui, state: &mut AppState) {
    let world = &state.doc.project.world;
    let symbol = world::large_unit(world.units).1;
    let radius = world::radius_text(world.radius_km, world.units);
    let degree = std::f64::consts::TAU * world.radius_km / 360.0;
    value_row(ui, "Name", &world.name.clone(), false);
    value_row(ui, "Radius", &format!("{radius} {symbol}"), true);
    value_row(ui, "1°", &world::distance(degree, world.units), true);
    value_row(ui, "Face size", &grouped(state.globe.map.face_size()), true);
    if widgets::button(ui, WORLD_PROPERTIES, WORLD_PROPERTIES, None).clicked() {
        command::run(Command::WorldProperties, ui.ctx(), state);
    }
    subheading(ui, "Heightmap");
    heightmap_buttons(ui, state, IMPORT);
}

/// The buttons that import and export the heightmap, and the text of the
/// import and the export below them.
fn heightmap_buttons(ui: &mut egui::Ui, state: &mut AppState, import: &str) {
    ui.horizontal(|ui| {
        let busy = state.globe.busy();
        let tip = Command::ImportHeightmap.tip(ui.ctx());
        let resp = ui.add_enabled_ui(!busy, |ui| widgets::button(ui, import, &tip, None));
        if resp.inner.clicked() {
            state.actions.push(Action::ImportHeightmapDialog);
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
    note_text(
        ui,
        state.globe.import_progress(),
        state.import_note.as_ref(),
    );
    note_text(
        ui,
        state.globe.export_progress(),
        state.export_note.as_ref(),
    );
}

/// The panel of the heightmap: the import, the export, and the elevation
/// controls.
fn height_body(ui: &mut egui::Ui, state: &mut AppState) {
    heightmap_buttons(ui, state, pad::IMPORT_HEIGHTMAP);
    subheading(ui, "Style");
    ui.checkbox(&mut state.globe.preview.greyscale, "Greyscale");
    elevation::controls(ui, state);
}

/// The panel of a vector layer in the World workspace: its name, its kind,
/// the number of its features, and the button that deletes it.
fn vector_body(ui: &mut egui::Ui, state: &mut AppState, id: LayerId) {
    let Some(layer) = state.doc.layer(id) else {
        return;
    };
    let kind = NEW_LAYERS
        .iter()
        .find(|(_, k)| *k == layer.kind)
        .map_or("", |(name, _)| name);
    let features = grouped(layer.features.len());
    // The text of the field, from the first key to the end of the edit.
    let key = ui.id().with(("desk-layer-name", id.0));
    let mut name = ui
        .data(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| layer.name.clone());
    ui.horizontal(|ui| {
        row_label(ui, "Name");
        let edit = egui::TextEdit::singleline(&mut name).hint_text(pad::LAYER_NAME);
        let field = ui.add(edit);
        if field.lost_focus() {
            if !name.trim().is_empty() {
                state.doc.rename_layer(id, &name);
            }
            ui.data_mut(|d| d.remove::<String>(key));
        } else if field.has_focus() {
            ui.data_mut(|d| d.insert_temp(key, name));
        }
    });
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
