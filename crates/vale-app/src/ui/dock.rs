//! The right dock of the desktop Atlas workspace: the panel of the selected
//! frame or of the open page, and the inspector of the layers. The dock
//! shows only with a selection.

use eframe::egui;
use vale_sphere::ProjectionKind;
use vale_store::{FrameRecord, FrameShape, LayerKind, PageUnits};

use super::atlas::{self, AtlasSelection, FrameTab};
use super::pad::atlas::{
    ADD_HOLE, ADD_PAGE, AZIMUTH, CENTER_LATITUDE, CENTER_LONGITUDE, CENTER_ON_FRAME, DELETE_FRAME,
    DELETE_PAGE, DELETE_PROJECTION, FIRST_PARALLEL, FRAME_TABS, HEIGHT, LATITUDE, LONGITUDE,
    MAP_NAME, OBLIQUE, ORIENTATIONS, PAGE_NAME, POLYGON, PROJECTION_NAME, RECTANGLE, REMOVE_VERTEX,
    ROTATION, SAVE_PROJECTION, SCALE, SECOND_PARALLEL, SHAPE, SIZE, UNIT_NAMES, WIDTH, custom_row,
    length_text, unit_name,
};
use super::pad::{GRATICULE, NO_LAYERS};
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
        ui.selectable_value(tab, FrameTab::Layers, FRAME_TABS[2]);
    });
    ui.add_space(4.0);
    match state.atlas.tab {
        FrameTab::Frame => frame_tab(ui, state, frame),
        FrameTab::Projection => projection_tab(ui, state, frame),
        FrameTab::Layers => layers_tab(ui, state, frame),
    }
    ui.add_space(6.0);
}

fn frame_tab(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let id = frame.id;
    ui.horizontal(|ui| {
        ui.label("Name");
        let mut name =
            (state.doc.project.atlas.map(frame.map)).map_or_else(String::new, |m| m.name.clone());
        let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(160.0));
        named(&field, egui::WidgetType::TextEdit, MAP_NAME);
        if field.changed() {
            atlas::rename_map(state, id, &name);
        }
    });
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

/// A switch for each vector layer, which turns the layer on or off in the
/// map of the frame.
fn layers_tab(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let rows: Vec<_> = (state.doc.project.layers().iter().rev())
        .filter(|l| matches!(l.kind, LayerKind::Vector(_)))
        .map(|l| (l.id, l.name.clone()))
        .collect();
    if rows.is_empty() {
        ui.weak(NO_LAYERS);
    }
    for (layer, name) in rows {
        let mut shown = state.doc.shown(frame.map, layer);
        if ui.checkbox(&mut shown, name).changed() {
            state.set_map_shown(frame.map, layer, shown);
        }
    }
}

fn projection_tab(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let map = frame.map;
    let spec = state.doc.projection(map);
    for k in ProjectionKind::ALL {
        if ui.selectable_label(spec.kind == k, k.name()).clicked() {
            atlas::set_projection_kind(state, frame.id, k);
        }
    }
    let current = atlas::current_preset(state, frame.id);
    let names: Vec<String> = state.doc.presets().iter().map(|p| p.name.clone()).collect();
    for name in names {
        let on = current.as_deref() == Some(name.as_str());
        let resp = ui.selectable_label(on, &name);
        named(&resp, egui::WidgetType::SelectableLabel, &custom_row(&name));
        if resp.clicked() {
            atlas::apply_preset(state, frame.id, &name);
        }
    }
    ui.add_space(4.0);
    egui::Grid::new("atlas-projection")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(CENTER_LONGITUDE);
            let typed = field(ui, CENTER_LONGITUDE, &math::lon_text(spec.lon0));
            if let Some(lon) = typed.as_deref().and_then(math::parse_degrees) {
                atlas::set_projection_longitude(state, frame.id, lon);
            }
            ui.end_row();
            let has_latitude = spec.kind.has_origin_latitude() || spec.azimuth.is_some();
            if has_latitude {
                ui.label(CENTER_LATITUDE);
                let typed = field(ui, CENTER_LATITUDE, &math::lat_text(spec.lat0));
                if let Some(lat) = typed.as_deref().and_then(math::parse_degrees) {
                    atlas::set_projection_latitude(state, frame.id, lat);
                }
                ui.end_row();
            }
            if spec.kind.is_conic() {
                for (name, value, first) in [
                    (FIRST_PARALLEL, spec.lat1, true),
                    (SECOND_PARALLEL, spec.lat2, false),
                ] {
                    ui.label(name);
                    let typed = field(ui, name, &math::lat_text(value));
                    if let Some(v) = typed.as_deref().and_then(math::parse_degrees) {
                        let (a, b) = if first {
                            (v, spec.lat2)
                        } else {
                            (spec.lat1, v)
                        };
                        atlas::set_parallels(state, frame.id, a, b);
                    }
                    ui.end_row();
                }
            }
            if let Some(azimuth) = spec.azimuth {
                ui.label(AZIMUTH);
                let mut value = azimuth;
                let drag = egui::DragValue::new(&mut value).speed(1.0).suffix("°");
                let drag = ui.add(drag);
                named(&drag, egui::WidgetType::DragValue, AZIMUTH);
                if drag.changed() {
                    atlas::set_azimuth(state, frame.id, value);
                }
                ui.end_row();
            }
        });
    if spec.kind.can_tilt() {
        let mut oblique = spec.azimuth.is_some();
        let resp = ui.checkbox(&mut oblique, OBLIQUE);
        if resp.changed() {
            atlas::set_oblique(state, frame.id, oblique);
        }
    }
    ui.add_space(4.0);
    if ui.button(CENTER_ON_FRAME).clicked() {
        atlas::center_projection(state, frame.id);
    }
    ui.add_space(4.0);
    let mut name = state.atlas.preset_name.clone();
    let resp = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Name"));
    named(&resp, egui::WidgetType::TextEdit, PROJECTION_NAME);
    if resp.changed() {
        state.atlas.preset_name = name;
    }
    let named_now = !state.atlas.preset_name.trim().is_empty();
    ui.horizontal(|ui| {
        let save = ui.add_enabled(named_now, egui::Button::new(SAVE_PROJECTION));
        if save.clicked() {
            atlas::save_preset(state, frame.id);
        }
        let saved = state
            .doc
            .presets()
            .iter()
            .any(|p| p.name == state.atlas.preset_name);
        let delete = ui.add_enabled(saved, egui::Button::new(DELETE_PROJECTION));
        if delete.clicked() {
            atlas::delete_preset(state);
        }
    });
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
