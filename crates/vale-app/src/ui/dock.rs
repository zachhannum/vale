//! The right dock of the desktop Atlas workspace: the panel of the selected
//! frame or of the open page, and the inspector of the layers. The dock
//! shows only with a selection.

use eframe::egui;
use vale_sphere::ProjectionKind;
use vale_store::{FrameRecord, FrameShape, PageUnits};

use super::atlas::{self, AtlasSelection, FrameTab};
use super::pad::GRATICULE;
use super::pad::atlas::{
    ADD_HOLE, ADD_PAGE, CENTER_ON_FRAME, COPY_MAP, DELETE_FRAME, DELETE_PAGE, FRAME_TABS, HEIGHT,
    LATITUDE, LONGITUDE, MAP, MAP_NAME, NEW_MAP, ORIENTATIONS, PAGE_NAME, POLYGON, RECTANGLE,
    REMOVE_VERTEX, RENAME_MAP, ROTATION, SCALE, SHAPE, SIZE, UNIT_NAMES, WIDTH, length_text,
    unit_name,
};
use super::pad::widgets::delete_name;
use super::{AppState, inspector};
use crate::atlas::{self as math, PAGE_PRESETS};

/// True if the dock shows: a frame, the page, or a layer is selected.
pub fn shown(state: &AppState) -> bool {
    state.atlas.selection != AtlasSelection::None || state.selected_vector().is_some()
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if !shown(state) {
        return;
    }
    egui::Panel::right("inspector")
        .default_size(280.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_width(ui.available_width());
                match state.atlas.selection {
                    AtlasSelection::Frame(id) => {
                        if let Some(frame) = state.doc.project.atlas.frame(id).cloned() {
                            frame_panel(ui, state, &frame);
                            ui.separator();
                        }
                    }
                    AtlasSelection::Page => {
                        page_panel(ui, state);
                        ui.separator();
                    }
                    AtlasSelection::None => {}
                }
                inspector::contents(ui, state);
            });
        });
}

/// Gives a widget a name for a screen reader and for a test.
fn named(resp: &egui::Response, kind: egui::WidgetType, name: &str) {
    resp.widget_info(|| egui::WidgetInfo::labeled(kind, true, name));
}

/// A text field that shows `shown`. Returns the text at the end of an
/// edit, when the field loses the focus.
fn field(ui: &mut egui::Ui, name: &str, shown: &str) -> Option<String> {
    let key = ui.id().with(("atlas-field", name));
    let mut text = ui
        .data(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| shown.to_owned());
    let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0));
    named(&resp, egui::WidgetType::TextEdit, name);
    if resp.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(key));
        return Some(text);
    }
    if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, text));
    }
    None
}

fn frame_panel(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let map = state.doc.project.atlas.map(frame.map);
    ui.heading(map.map_or("", |m| m.name.as_str()));
    ui.horizontal(|ui| {
        let tab = &mut state.atlas.tab;
        ui.selectable_value(tab, FrameTab::Frame, FRAME_TABS[0]);
        ui.selectable_value(tab, FrameTab::Projection, FRAME_TABS[1]);
    });
    ui.add_space(4.0);
    match state.atlas.tab {
        FrameTab::Frame => frame_tab(ui, state, frame),
        FrameTab::Projection => projection_tab(ui, state, frame),
    }
    ui.add_space(6.0);
}

fn frame_tab(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let id = frame.id;
    map_menu(ui, state, frame);
    rename_dialog(ui, state, frame);
    let polygon = matches!(frame.shape, FrameShape::Polygon(_));
    ui.horizontal(|ui| {
        ui.label(SHAPE);
        let mut on = polygon;
        ui.selectable_value(&mut on, false, RECTANGLE);
        ui.selectable_value(&mut on, true, POLYGON);
        if on != polygon {
            atlas::set_polygon(state, id, on);
        }
    });
    if polygon {
        ui.horizontal(|ui| {
            if ui.button(ADD_HOLE).clicked() {
                atlas::add_hole(state, id);
            }
            let vertex = state.atlas.vertex;
            let remove = ui.add_enabled(vertex.is_some(), egui::Button::new(REMOVE_VERTEX));
            if remove.clicked()
                && let Some(v) = vertex
            {
                atlas::remove_vertex(state, v);
            }
        });
    }
    egui::Grid::new("atlas-frame")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(SCALE);
            let typed = field(ui, SCALE, &math::scale_text(frame.scale));
            if let Some(scale) = typed.as_deref().and_then(math::parse_scale) {
                atlas::set_scale(state, id, scale);
            }
            ui.end_row();
            let [lon, lat] = frame.center;
            ui.label(LONGITUDE);
            let typed = field(ui, LONGITUDE, &math::lon_text(lon));
            if let Some(lon) = typed.as_deref().and_then(math::parse_degrees) {
                atlas::set_longitude(state, id, lon);
            }
            ui.end_row();
            ui.label(LATITUDE);
            let typed = field(ui, LATITUDE, &math::lat_text(lat));
            if let Some(lat) = typed.as_deref().and_then(math::parse_degrees) {
                atlas::set_latitude(state, id, lat);
            }
            ui.end_row();
            ui.label(ROTATION);
            let mut rotation = frame.rotation;
            let drag = egui::DragValue::new(&mut rotation).speed(1.0).suffix("°");
            let drag = ui.add(drag);
            named(&drag, egui::WidgetType::DragValue, ROTATION);
            if drag.changed() {
                atlas::set_rotation(state, id, rotation);
            }
            ui.end_row();
        });
    let mut graticule = frame.graticule;
    if ui.checkbox(&mut graticule, GRATICULE).changed() {
        atlas::set_graticule(state, id, graticule);
    }
    ui.add_space(4.0);
    if ui.button(DELETE_FRAME).clicked() {
        atlas::delete_frame(state);
    }
}

/// The Map menu: the maps of the project with a check on the map of the
/// frame, a Delete button for each map that no frame shows, and the map
/// commands.
fn map_menu(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let atlas = &state.doc.project.atlas;
    let current = atlas.map(frame.map).map_or("", |m| m.name.as_str());
    let maps: Vec<_> = (atlas.maps.iter())
        .map(|m| (m.id, m.name.clone(), atlas.can_remove_map(m.id)))
        .collect();
    egui::ComboBox::from_label(MAP)
        .selected_text(current)
        .show_ui(ui, |ui| {
            for (map, name, removable) in maps {
                ui.horizontal(|ui| {
                    if ui.selectable_label(map == frame.map, &name).clicked() {
                        atlas::set_frame_map(state, frame.id, map);
                    }
                    if removable {
                        let delete = ui.small_button("Delete");
                        named(&delete, egui::WidgetType::Button, &delete_name(&name));
                        if delete.clicked() {
                            atlas::delete_map(state, map);
                        }
                    }
                });
            }
            ui.separator();
            if ui.button(NEW_MAP).clicked() {
                atlas::new_map(state, frame.id);
            }
            if ui.button(COPY_MAP).clicked() {
                atlas::copy_map(state, frame.id);
            }
            if ui.button(RENAME_MAP).clicked() {
                let name = state.doc.project.atlas.map(frame.map);
                state.atlas.map_name = name.map(|m| m.name.clone());
            }
        });
}

/// The dialog that renames the map of the frame.
fn rename_dialog(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let Some(mut name) = state.atlas.map_name.take() else {
        return;
    };
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("atlas-rename-map")).show(ui.ctx(), |ui| {
        ui.set_width(320.0);
        ui.heading("Rename map");
        let field = egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY);
        let field = ui.add(field);
        named(&field, egui::WidgetType::TextEdit, MAP_NAME);
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                close = true;
            }
            let valid = !name.trim().is_empty();
            let rename = ui.add_enabled(valid, egui::Button::new("Rename"));
            if valid && (rename.clicked() || enter) {
                atlas::rename_map(state, frame.map, &name);
                close = true;
            }
        });
    });
    if !close && !modal.should_close() {
        state.atlas.map_name = Some(name);
    }
}

fn projection_tab(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let kind = state.doc.projection(frame.map).kind;
    for k in ProjectionKind::ALL {
        if ui.selectable_label(kind == k, k.name()).clicked() {
            atlas::set_projection_kind(state, frame.id, k);
        }
    }
    ui.add_space(4.0);
    if ui.button(CENTER_ON_FRAME).clicked() {
        atlas::center_projection(state, frame.id);
    }
}

fn page_panel(ui: &mut egui::Ui, state: &mut AppState) {
    let (index, count) = atlas::page_index(state);
    let page = atlas::open_page(state).clone();
    ui.heading(format!("Page {}", index + 1));
    let units = page.units;
    egui::Grid::new("atlas-page").num_columns(2).show(ui, |ui| {
        ui.label("Name");
        let mut name = page.name.clone();
        let resp = ui.add(egui::TextEdit::singleline(&mut name).desired_width(160.0));
        named(&resp, egui::WidgetType::TextEdit, PAGE_NAME);
        if resp.changed() {
            atlas::edit_page(state, |p| p.name = name);
        }
        ui.end_row();

        ui.label(SIZE);
        let preset = math::preset_of(page.width_mm, page.height_mm);
        egui::ComboBox::from_id_salt("atlas-page-size")
            .selected_text(preset)
            .show_ui(ui, |ui| {
                for p in PAGE_PRESETS {
                    if ui.selectable_label(p.name == preset, p.name).clicked() {
                        atlas::set_page_preset(state, &p);
                    }
                }
            });
        ui.end_row();

        ui.label("Orientation");
        ui.horizontal(|ui| {
            let landscape = page.width_mm > page.height_mm;
            let mut on = landscape;
            ui.selectable_value(&mut on, false, ORIENTATIONS[0]);
            ui.selectable_value(&mut on, true, ORIENTATIONS[1]);
            if on != landscape {
                atlas::turn_page(state);
            }
        });
        ui.end_row();

        for (name, mm, width) in [
            (WIDTH, page.width_mm, true),
            (HEIGHT, page.height_mm, false),
        ] {
            ui.label(name);
            ui.horizontal(|ui| {
                let typed = field(ui, name, &length_text(mm, units));
                if let Some(mm) = typed.and_then(|t| atlas::parse_length(&t, units)) {
                    atlas::edit_page(state, |p| match width {
                        true => p.width_mm = mm,
                        false => p.height_mm = mm,
                    });
                }
                ui.label(unit_name(units));
            });
            ui.end_row();
        }

        ui.label("Units");
        ui.horizontal(|ui| {
            for (u, name) in PageUnits::ALL.into_iter().zip(UNIT_NAMES) {
                if ui.selectable_label(units == u, name).clicked() && units != u {
                    atlas::edit_page(state, |p| p.units = u);
                }
            }
        });
        ui.end_row();
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button(ADD_PAGE).clicked() {
            atlas::add_page(state);
        }
        let delete = ui.add_enabled(count > 1, egui::Button::new(DELETE_PAGE));
        if delete.clicked() {
            atlas::delete_page(state);
        }
    });
    ui.add_space(6.0);
}
