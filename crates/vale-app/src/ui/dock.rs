//! The right dock of the desktop Atlas workspace: the panel of the selected
//! frame or of the open page, and the inspector of the layers. The dock
//! shows only with a selection.

use eframe::egui;
use vale_sphere::{INTERVALS, ProjectionKind, interval_text};
use vale_store::{
    BlendMode, Border, CoordFormat, EdgeLabels, EdgeOrientation, FrameRecord, FrameShape,
    Graticule, LayerId, PageUnits, RasterDisplay,
};

use super::elevation::{DISPLAYS, blend_label};
use super::pad::raster::{BLEND, SHOW_AS};

use super::atlas::{self, AtlasSelection, EdgeSet, FrameTab};
use super::pad::atlas::{
    ADD_HOLE, ADD_PAGE, AUTO_INTERVAL, AZIMUTH, BORDER, BORDER_COLOR, BORDER_WIDTH,
    CENTER_LATITUDE, CENTER_LONGITUDE, CENTER_ON_FRAME, DELETE_FRAME, DELETE_PAGE,
    DELETE_PROJECTION, FIRST_PARALLEL, FRAME_TABS, GRATICULE_COLOR, GRATICULE_WIDTH, HEIGHT,
    INTERVAL, LABEL_FORMAT, LABEL_FORMATS, LATITUDE, LONGITUDE, MAP_NAME, MAP_STYLE_HINT, OBLIQUE,
    ORIENTATIONS, PAGE_NAME, POLYGON, PROJECTION_NAME, RECTANGLE, REMOVE_OVERRIDES, REMOVE_VERTEX,
    ROTATION, SAVE_PROJECTION, SCALE, SECOND_PARALLEL, SHAPE, SIZE, UNIT_NAMES, WIDTH, custom_row,
    length_text, unit_name,
};
use super::pad::edge_labels::EDGE_LABELS;
use super::pad::{GRATICULE, NO_LAYERS};
use super::{AppState, inspector, style_editor};
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
                    AtlasSelection::Item(id) => {
                        if let Some(rec) = state.doc.project.atlas.furniture(id).cloned() {
                            item_panel(ui, state, &rec);
                            ui.separator();
                        }
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
        ui.selectable_value(tab, FrameTab::Labels, FRAME_TABS[3]);
    });
    ui.add_space(4.0);
    match state.atlas.tab {
        FrameTab::Frame => frame_tab(ui, state, frame),
        FrameTab::Projection => projection_tab(ui, state, frame),
        FrameTab::Layers => layers_tab(ui, state, frame),
        FrameTab::Labels => labels_tab(ui, state, frame),
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
    let mut graticule = frame.graticule.on;
    if ui.checkbox(&mut graticule, GRATICULE).changed() {
        atlas::set_graticule(state, id, graticule);
    }
    if frame.graticule.on {
        graticule_rows(ui, state, frame);
    }
    let mut border = frame.border.on;
    if ui.checkbox(&mut border, BORDER).changed() {
        atlas::set_border(state, frame.id, border);
    }
    if frame.border.on {
        border_rows(ui, state, frame);
    }
    ui.add_space(4.0);
    if ui.button(DELETE_FRAME).clicked() {
        atlas::delete_frame(state);
    }
}

/// The interval, the color, the width, and the edge labels of the graticule
/// of a frame.
fn graticule_rows(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let (id, g) = (frame.id, &frame.graticule);
    let Some((drawn, auto)) = atlas::graticule_intervals(state, id) else {
        return;
    };
    let mut automatic = g.interval.is_none();
    if ui.checkbox(&mut automatic, AUTO_INTERVAL).changed() {
        atlas::set_graticule_interval(state, id, (!automatic).then_some(auto));
    }
    egui::Grid::new("atlas-graticule")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(INTERVAL);
            let combo = egui::ComboBox::from_id_salt("atlas-graticule-interval")
                .selected_text(interval_text(drawn))
                .show_ui(ui, |ui| {
                    for i in INTERVALS.into_iter().rev() {
                        let chosen = g.interval == Some(i);
                        if ui.selectable_label(chosen, interval_text(i)).clicked() {
                            atlas::set_graticule_interval(state, id, Some(i));
                        }
                    }
                });
            named(&combo.response, egui::WidgetType::ComboBox, INTERVAL);
            ui.end_row();
            ui.label(GRATICULE_COLOR);
            let mut color = g.color;
            let button = ui.color_edit_button_srgb(&mut color);
            named(&button, egui::WidgetType::ColorButton, GRATICULE_COLOR);
            if button.changed() {
                atlas::set_graticule_color(state, id, color);
            }
            ui.end_row();
            ui.label(GRATICULE_WIDTH);
            let mut width = g.width;
            let drag = egui::DragValue::new(&mut width)
                .range(Graticule::WIDTHS)
                .speed(0.05)
                .fixed_decimals(1)
                .suffix(" pt");
            let drag = ui.add(drag);
            named(&drag, egui::WidgetType::DragValue, GRATICULE_WIDTH);
            if drag.changed() {
                atlas::set_graticule_width(state, id, width);
            }
            ui.end_row();
        });
    egui::CollapsingHeader::new(EDGE_LABELS)
        .id_salt("atlas-edge-labels")
        .show(ui, |ui| edge_label_rows(ui, state, frame));
}

/// The label format and the two sets of edge labels of a frame, with the
/// names of the iPad page.
fn edge_label_rows(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    use super::pad::edge_labels as edge;
    let (id, g) = (frame.id, &frame.graticule);
    ui.horizontal(|ui| {
        ui.label(LABEL_FORMAT);
        for (format, text) in CoordFormat::ALL.into_iter().zip(LABEL_FORMATS) {
            if ui.selectable_label(g.format == format, text).clicked() {
                atlas::set_graticule_format(state, id, format);
            }
        }
    });
    ui.horizontal(|ui| {
        for (which, (name, label)) in EdgeSet::ALL.into_iter().zip(edge::SETS) {
            let tab = ui.selectable_label(state.atlas.edge_set == which, label);
            named(&tab, egui::WidgetType::SelectableLabel, name);
            if tab.clicked() {
                state.atlas.edge_set = which;
            }
        }
    });
    let which = state.atlas.edge_set;
    let before = which.of(g).clone();
    let mut set = before.clone();
    ui.checkbox(&mut set.on, edge::SHOW_LABELS);
    if set.on {
        let family = set.font.as_deref().unwrap_or(crate::fonts::DEFAULT_FAMILY);
        ui.horizontal(|ui| {
            ui.label(edge::FONT);
            let button = ui.button(format!("{family}…"));
            named(&button, egui::WidgetType::Button, edge::FONT);
            if button.clicked() {
                let target = super::FontTarget::Edge(id, which);
                state.actions.push(super::Action::PickFont(target));
            }
        });
        let family = crate::fonts::family(set.font.as_deref(), &mut state.fonts).to_owned();
        let mut faces = state.fonts.faces(&family);
        if faces.is_empty() {
            faces.push(vale_labeler::Face {
                weight: 400.0,
                italic: false,
            });
        }
        let face = vale_labeler::Face {
            weight: set.weight,
            italic: set.italic,
        };
        ui.horizontal(|ui| {
            ui.label(edge::STYLE);
            let combo = egui::ComboBox::from_id_salt("atlas-edge-style")
                .selected_text(face.name())
                .show_ui(ui, |ui| {
                    for f in faces {
                        if ui.selectable_label(f == face, f.name()).clicked() {
                            (set.weight, set.italic) = (f.weight, f.italic);
                        }
                    }
                });
            named(&combo.response, egui::WidgetType::ComboBox, edge::STYLE);
        });
        egui::Grid::new("atlas-edge-labels")
            .num_columns(2)
            .show(ui, |ui| {
                let drag =
                    |ui: &mut egui::Ui, name: &str, value: &mut f64, range, speed, suffix| {
                        ui.label(name);
                        let drag = egui::DragValue::new(value)
                            .range(range)
                            .speed(speed)
                            .suffix(suffix);
                        let drag = ui.add(drag);
                        named(&drag, egui::WidgetType::DragValue, name);
                        ui.end_row();
                    };
                drag(ui, edge::SIZE, &mut set.size, EdgeLabels::SIZES, 0.1, " pt");
                ui.label(edge::COLOR);
                ui.color_edit_button_srgb(&mut set.color);
                ui.end_row();
                let mut percent = set.opacity * 100.0;
                drag(ui, edge::OPACITY, &mut percent, 0.0..=100.0, 1.0, " %");
                set.opacity = (percent.round() / 100.0).clamp(0.0, 1.0);
                ui.label(edge::HALO);
                ui.checkbox(&mut set.halo, "");
                ui.end_row();
                if set.halo {
                    ui.label(edge::HALO_COLOR);
                    ui.color_edit_button_srgb(&mut set.halo_color);
                    ui.end_row();
                    let widths = EdgeLabels::HALO_WIDTHS;
                    drag(
                        ui,
                        edge::HALO_WIDTH,
                        &mut set.halo_width,
                        widths,
                        0.05,
                        " pt",
                    );
                }
                ui.label(edge::POSITION);
                ui.horizontal(|ui| {
                    for (i, text) in edge::PLACES.into_iter().enumerate() {
                        let outside = i == 1;
                        if ui.selectable_label(set.outside == outside, text).clicked() {
                            set.outside = outside;
                        }
                    }
                });
                ui.end_row();
                ui.label(edge::ORIENTATION);
                ui.horizontal(|ui| {
                    for (o, text) in EdgeOrientation::ALL.into_iter().zip(edge::ORIENTATIONS) {
                        if ui.selectable_label(set.orientation == o, text).clicked() {
                            set.orientation = o;
                        }
                    }
                });
                ui.end_row();
                drag(
                    ui,
                    edge::OFFSET,
                    &mut set.offset,
                    EdgeLabels::OFFSETS,
                    0.1,
                    " pt",
                );
                ui.label(edge::EDGES);
                ui.horizontal(|ui| {
                    let e = &mut set.edges;
                    let sides = [&mut e.top, &mut e.bottom, &mut e.left, &mut e.right];
                    for (on, (name, label)) in sides.into_iter().zip(edge::SIDES) {
                        let check = ui.checkbox(on, label);
                        named(&check, egui::WidgetType::Checkbox, name);
                    }
                });
                ui.end_row();
            });
    }
    if set != before {
        atlas::edit_edge_labels(state, id, which, |e| *e = set);
    }
}

/// The color and the width of the border of a frame.
fn border_rows(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let (id, b) = (frame.id, &frame.border);
    egui::Grid::new("atlas-border")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(BORDER_COLOR);
            let mut color = b.color;
            let button = ui.color_edit_button_srgb(&mut color);
            named(&button, egui::WidgetType::ColorButton, BORDER_COLOR);
            if button.changed() {
                atlas::set_border_color(state, id, color);
            }
            ui.end_row();
            ui.label(BORDER_WIDTH);
            let mut width = b.width;
            let drag = egui::DragValue::new(&mut width)
                .range(Border::WIDTHS)
                .speed(0.05)
                .fixed_decimals(2)
                .suffix(" pt");
            let drag = ui.add(drag);
            named(&drag, egui::WidgetType::DragValue, BORDER_WIDTH);
            if drag.changed() {
                atlas::set_border_width(state, id, width);
            }
            ui.end_row();
        });
}

/// A switch for each layer, which turns the layer on or off in the
/// map of the frame, and the style of the chosen vector layer, for all maps
/// or for the map of the frame.
fn layers_tab(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let rows: Vec<_> = (state.doc.project.layers().iter().rev())
        .map(|l| (l.id, l.name.clone()))
        .collect();
    if rows.is_empty() {
        ui.weak(NO_LAYERS);
    }
    let open = state.atlas.open_layer();
    for (layer, name) in rows {
        ui.horizontal(|ui| {
            let mut shown = state.doc.shown(frame.map, layer);
            if ui.checkbox(&mut shown, &name).changed() {
                state.set_map_shown(frame.map, layer, shown);
            }
            if ui.selectable_label(open == Some(layer), "Style").clicked() {
                state.atlas.layer = (open != Some(layer)).then_some((frame.id, layer));
            }
        });
    }
    let Some(layer) = state.atlas.open_layer() else {
        return;
    };
    ui.separator();
    ui.weak(MAP_STYLE_HINT);
    let Some(kind) = state.doc.layer(layer).and_then(|l| l.geometry()) else {
        look_controls(ui, state, frame, layer);
        return;
    };
    let fields = state
        .doc
        .layer(layer)
        .map_or(Vec::new(), |l| l.fields.clone());
    let symbols = state.doc.symbols().clone();
    let mut library = style_editor::Library {
        symbols: &symbols,
        note: state.symbol_note.as_ref(),
        actions: &mut state.actions,
    };
    let Some(mut style) = state.doc.map_style(frame.map, layer) else {
        return;
    };
    if style_editor::rules(ui, layer, &mut style, kind, &fields, &mut library)
        && state.doc.set_map_style(frame.map, layer, &style)
    {
        state.touch();
    }
    remove_overrides(ui, state, frame, layer);
}

fn remove_overrides(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord, layer: LayerId) {
    let any = !state.doc.overrides(frame.map, layer).is_empty();
    if ui
        .add_enabled(any, egui::Button::new(REMOVE_OVERRIDES))
        .clicked()
    {
        state.clear_overrides(frame.map, layer);
    }
}

/// The opacity, the display, and the blend mode of a raster layer or an
/// image layer in the map of the frame.
fn look_controls(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord, layer: LayerId) {
    let scope = Some(frame.map);
    let Some(mut look) = state.look(scope, layer) else {
        return;
    };
    let old = look;
    let raster = state.doc.layer(layer).is_some_and(|l| l.raster().is_some());
    egui::Grid::new("look").num_columns(2).show(ui, |ui| {
        ui.label("Opacity");
        ui.add(egui::Slider::new(&mut look.opacity, 0.0..=1.0));
        ui.end_row();
        if raster {
            ui.label(SHOW_AS);
            egui::ComboBox::from_id_salt("look-display")
                .selected_text(display_name(look.display))
                .show_ui(ui, |ui| {
                    for d in RasterDisplay::ALL {
                        ui.selectable_value(&mut look.display, d, display_name(d));
                    }
                });
            ui.end_row();
        }
        ui.label(BLEND);
        egui::ComboBox::from_id_salt("look-blend")
            .selected_text(blend_label(look.blend))
            .show_ui(ui, |ui| {
                for mode in BlendMode::ALL {
                    ui.selectable_value(&mut look.blend, mode, blend_label(mode));
                }
            });
        ui.end_row();
    });
    if look != old {
        state.set_look(scope, layer, look);
    }
    remove_overrides(ui, state, frame, layer);
}

fn display_name(d: RasterDisplay) -> &'static str {
    let i = RasterDisplay::ALL.iter().position(|x| *x == d).unwrap_or(0);
    DISPLAYS[i]
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

/// The label classes of the map of a frame in priority order, and the
/// controls of the open class.
fn labels_tab(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    use super::pad::labels::{
        ADD_CLASS, BUFFER, CASE, CURVE_LIMIT, DELETE_CLASS, DUPLICATE_DISTANCE, HALO,
        LETTER_SPACING, MOVE_DOWN, MOVE_UP, NO_LABEL_LAYERS, OFFSET, OPACITY, PLACEMENT, POSITIONS,
        REMOVE_DUPLICATES, REPEAT, REPEAT_INTERVAL, SMOOTHING, STYLE,
    };
    let map = frame.map;
    if state.label_layers().is_empty() {
        ui.weak(NO_LABEL_LAYERS);
        return;
    }
    let open = state.atlas.open_class();
    let list = state.label_class_list(map);
    for (layer, i, class) in &list {
        let name = state
            .doc
            .layer(*layer)
            .map_or_else(String::new, |l| l.name.clone());
        let text = if name == class.name {
            name
        } else {
            format!("{} · {name}", class.name)
        };
        let on = open == Some((*layer, *i));
        if ui.selectable_label(on, text).clicked() {
            state.atlas.label = (!on).then_some((frame.id, *layer, *i));
            state.atlas.label_segment = Default::default();
        }
    }
    if ui.button(ADD_CLASS).clicked()
        && let Some((layer, i)) = state.add_label_class(map)
    {
        state.atlas.label = Some((frame.id, layer, i));
        state.atlas.label_segment = Default::default();
    }
    let Some((layer, i)) = state.atlas.open_class() else {
        return;
    };
    let Some(kind) = state.doc.layer(layer).and_then(|l| l.geometry()) else {
        return;
    };
    let Some(mut class) = state.doc.label_classes(Some(map), layer).get(i).cloned() else {
        state.atlas.label = None;
        return;
    };
    ui.separator();
    let before = class.clone();
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut class.name);
    });
    ui.horizontal(|ui| {
        ui.label("Text");
        ui.text_edit_singleline(&mut class.text);
    });
    ui.horizontal(|ui| {
        ui.label("Filter");
        ui.text_edit_singleline(&mut class.filter);
    });
    let family = crate::fonts::family(class.font.as_deref(), &mut state.fonts).to_owned();
    let mut faces = state.fonts.faces(&family);
    if faces.is_empty() {
        faces.push(vale_labeler::Face {
            weight: 400.0,
            italic: false,
        });
    }
    let face = vale_labeler::Face {
        weight: class.weight,
        italic: class.italic,
    };
    egui::ComboBox::from_label(STYLE)
        .selected_text(face.name())
        .show_ui(ui, |ui| {
            for f in faces {
                if ui.selectable_label(f == face, f.name()).clicked() {
                    (class.weight, class.italic) = (f.weight, f.italic);
                }
            }
        });
    ui.horizontal(|ui| {
        ui.label("Size");
        ui.add(
            egui::DragValue::new(&mut class.size)
                .range(4.0..=72.0)
                .suffix(" pt"),
        );
        style_editor::color_button(ui, &mut class.color);
    });
    egui::ComboBox::from_label(CASE)
        .selected_text(class.case.name())
        .show_ui(ui, |ui| {
            for c in vale_style::TextCase::ALL {
                ui.selectable_value(&mut class.case, c, c.name());
            }
        });
    let mut spacing = class.letter_spacing * 100.0;
    let mut opacity = class.opacity * 100.0;
    ui.horizontal(|ui| {
        ui.label(LETTER_SPACING);
        let drag = egui::DragValue::new(&mut spacing)
            .range(-10.0..=100.0)
            .suffix(" %");
        if ui.add(drag).changed() {
            class.letter_spacing = spacing / 100.0;
        }
    });
    ui.horizontal(|ui| {
        ui.label(OPACITY);
        let slider = egui::Slider::new(&mut opacity, 0.0..=100.0).suffix(" %");
        if ui.add(slider).changed() {
            class.opacity = opacity / 100.0;
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut class.halo, HALO);
        if class.halo {
            style_editor::color_button(ui, &mut class.halo_color);
            ui.add(
                egui::DragValue::new(&mut class.halo_width)
                    .range(0.5..=8.0)
                    .speed(0.1)
                    .suffix(" pt"),
            );
        }
    });
    egui::ComboBox::from_label(PLACEMENT)
        .selected_text(class.placement.name())
        .show_ui(ui, |ui| {
            for &p in vale_style::LabelPlacement::choices(kind) {
                ui.selectable_value(&mut class.placement, p, p.name());
            }
        });
    if kind == vale_store::GeometryKind::Point {
        ui.label(POSITIONS);
        let count = class.positions.len();
        let mut swap = None;
        for (k, r) in class.positions.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.checkbox(&mut r.on, r.position.name());
                if ui.add_enabled(k > 0, egui::Button::new("Up")).clicked() {
                    swap = Some((k, k - 1));
                }
                if ui
                    .add_enabled(k + 1 < count, egui::Button::new("Down"))
                    .clicked()
                {
                    swap = Some((k, k + 1));
                }
            });
        }
        if let Some((a, b)) = swap {
            class.positions.swap(a, b);
        }
    }
    ui.horizontal(|ui| {
        ui.label(OFFSET);
        let drag = egui::DragValue::new(&mut class.offset)
            .range(0.0..=20.0)
            .speed(0.1)
            .suffix(" pt");
        ui.add(drag);
    });
    if kind == vale_store::GeometryKind::Line {
        ui.horizontal(|ui| {
            ui.checkbox(&mut class.repeat, REPEAT);
            if class.repeat {
                ui.label(REPEAT_INTERVAL);
                let drag = egui::DragValue::new(&mut class.repeat_distance)
                    .range(50.0..=2000.0)
                    .suffix(" pt");
                ui.add(drag);
            }
        });
        let mut degrees = class.curve_limit.to_degrees();
        ui.horizontal(|ui| {
            ui.label(CURVE_LIMIT);
            let drag = egui::DragValue::new(&mut degrees)
                .range(5.0..=60.0)
                .max_decimals(0)
                .suffix("°");
            if ui.add(drag).changed() {
                class.curve_limit = degrees.to_radians();
            }
        });
        ui.horizontal(|ui| {
            ui.label(SMOOTHING);
            let drag = egui::DragValue::new(&mut class.smoothing)
                .range(0.0..=5.0)
                .speed(0.05)
                .suffix("×");
            ui.add(drag);
        });
    }
    ui.horizontal(|ui| {
        ui.label(BUFFER);
        let drag = egui::DragValue::new(&mut class.buffer)
            .range(0.0..=10.0)
            .speed(0.1)
            .suffix(" pt");
        ui.add(drag);
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut class.remove_duplicates, REMOVE_DUPLICATES);
        if class.remove_duplicates {
            ui.label(DUPLICATE_DISTANCE);
            let drag = egui::DragValue::new(&mut class.duplicate_distance)
                .range(0.0..=2000.0)
                .suffix(" pt");
            ui.add(drag);
        }
    });
    let at = list.iter().position(|(l, j, _)| (*l, *j) == (layer, i));
    if let Some(at) = at {
        ui.horizontal(|ui| {
            if ui.add_enabled(at > 0, egui::Button::new(MOVE_UP)).clicked() {
                state.reorder_label_classes(map, at, at - 1);
            }
            let down = at + 1 < list.len();
            if ui.add_enabled(down, egui::Button::new(MOVE_DOWN)).clicked() {
                state.reorder_label_classes(map, at, at + 1);
            }
        });
    }
    if class != before {
        // The priority follows the list order, so a move keeps it.
        state.edit_label_class(map, layer, i, |c| {
            *c = vale_style::LabelClass {
                priority: c.priority,
                ..class
            }
        });
    }
    if ui.button(DELETE_CLASS).clicked() {
        state.delete_label_class(map, layer, i);
        state.atlas.label = None;
    }
}

/// The panel of an item of furniture.
fn item_panel(ui: &mut egui::Ui, state: &mut AppState, rec: &vale_store::FurnitureRecord) {
    use super::pad::furniture as item;
    use vale_store::Furniture;
    let id = rec.id;
    let kind = atlas::ItemKind::of(&rec.item);
    ui.heading(kind.name());
    if !matches!(rec.item, Furniture::Text(_)) {
        let names = atlas::frame_names(state);
        let chosen = names.iter().find(|(f, _)| Some(*f) == rec.item.frame());
        let shown = chosen.map_or(item::NO_FRAME, |(_, n)| n.as_str());
        ui.horizontal(|ui| {
            ui.label(item::MAP_FRAME);
            egui::ComboBox::from_id_salt("atlas-item-frame")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for (frame, name) in &names {
                        let on = rec.item.frame() == Some(*frame);
                        if ui.selectable_label(on, name).clicked() {
                            atlas::set_item_frame(state, id, Some(*frame));
                        }
                    }
                });
        });
    }
    match &rec.item {
        Furniture::Text(t) => {
            let mut text = t.text.clone();
            let edit = ui.add(egui::TextEdit::multiline(&mut text).desired_rows(3));
            named(&edit, egui::WidgetType::TextEdit, item::ITEM_TEXT);
            if edit.changed() {
                atlas::edit_item(state, id, |r| match &mut r.item {
                    Furniture::Text(t) => {
                        t.text = text;
                        true
                    }
                    _ => false,
                });
            }
            item_style_rows(ui, state, id, &t.style, true);
            ui.horizontal(|ui| {
                ui.label(item::ALIGNMENT);
                let aligns = [
                    vale_store::TextAlign::Left,
                    vale_store::TextAlign::Center,
                    vale_store::TextAlign::Right,
                ];
                for (align, name) in aligns.into_iter().zip(item::ALIGNMENTS) {
                    if ui.selectable_label(t.align == align, name).clicked() {
                        atlas::edit_item(state, id, |r| match &mut r.item {
                            Furniture::Text(t) => {
                                t.align = align;
                                true
                            }
                            _ => false,
                        });
                    }
                }
            });
        }
        Furniture::Legend(l) => {
            ui.horizontal(|ui| {
                ui.label(item::TITLE);
                let mut title = l.title.clone();
                let edit = ui.text_edit_singleline(&mut title);
                named(&edit, egui::WidgetType::TextEdit, item::TITLE);
                if edit.changed() {
                    atlas::edit_item(state, id, |r| match &mut r.item {
                        Furniture::Legend(l) => {
                            l.title = title;
                            true
                        }
                        _ => false,
                    });
                }
            });
            item_style_rows(ui, state, id, &l.style, false);
            ui.label(item::ENTRIES);
            for row in atlas::legend_rows(state, l) {
                let key = (row.layer, row.rule.clone());
                let name = l.entries.get(&key).and_then(|e| e.name.clone());
                ui.horizontal(|ui| {
                    let mut shown = !row.hidden;
                    if ui.checkbox(&mut shown, "").changed() {
                        atlas::edit_entry(state, id, key.clone(), |e| e.hidden = !shown);
                    }
                    let mut text = name.clone().unwrap_or_default();
                    let edit = egui::TextEdit::singleline(&mut text).hint_text(&row.label);
                    if ui.add(edit).changed() {
                        let typed = text.trim().to_string();
                        let name = (!typed.is_empty()).then_some(typed);
                        atlas::edit_entry(state, id, key, |e| e.name = name);
                    }
                });
            }
            let reset = ui.add_enabled(
                !l.entries.is_empty(),
                egui::Button::new(item::RESET_ENTRIES),
            );
            if reset.clicked() {
                atlas::edit_item(state, id, |r| match &mut r.item {
                    Furniture::Legend(l) => {
                        l.entries.clear();
                        true
                    }
                    _ => false,
                });
            }
        }
        Furniture::ScaleBar(s) => {
            item_style_rows(ui, state, id, &s.style, false);
            let text = atlas::item_scale_text(state, s.frame);
            ui.label(format!(
                "{}: {}",
                item::SCALE,
                text.as_deref().unwrap_or("—")
            ));
            ui.weak(item::SCALE_HINT);
        }
        Furniture::NorthArrow(n) => {
            ui.horizontal(|ui| {
                ui.label(item::COLOR);
                let mut color = n.color;
                let button = ui.color_edit_button_srgb(&mut color);
                named(&button, egui::WidgetType::ColorButton, item::COLOR);
                if button.changed() {
                    atlas::set_item_color(state, id, color);
                }
            });
            let mut hide = n.hide_if_varies;
            if ui.checkbox(&mut hide, item::HIDE_VARIES).changed() {
                atlas::edit_item(state, id, |r| match &mut r.item {
                    Furniture::NorthArrow(n) => {
                        n.hide_if_varies = hide;
                        true
                    }
                    _ => false,
                });
            }
            let v = atlas::item_variation(state, n.frame);
            let text = v.map_or_else(|| "—".to_string(), |v| format!("{v:.1}°"));
            ui.label(format!("{}: {text}", item::NORTH_VARIES));
        }
    }
    ui.add_space(6.0);
    if ui.button(item::delete_name(kind)).clicked() {
        atlas::delete_item(state);
    }
    ui.add_space(6.0);
}

/// The font, the style, the size, and the color of the text of an item.
fn item_style_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    id: vale_store::FurnitureId,
    style: &vale_store::FurnitureText,
    font: bool,
) {
    use super::pad::furniture as item;
    egui::Grid::new("atlas-item-style")
        .num_columns(2)
        .show(ui, |ui| {
            if font {
                ui.label(item::FONT);
                let family = style
                    .font
                    .as_deref()
                    .unwrap_or(crate::fonts::DEFAULT_FAMILY);
                let button = ui.button(format!("{family}…"));
                named(&button, egui::WidgetType::Button, item::FONT);
                if button.clicked() {
                    let target = super::FontTarget::Item(id);
                    state.actions.push(super::Action::PickFont(target));
                }
                ui.end_row();
                let family =
                    crate::fonts::family(style.font.as_deref(), &mut state.fonts).to_owned();
                let mut faces = state.fonts.faces(&family);
                if faces.is_empty() {
                    faces.push(vale_labeler::Face {
                        weight: 400.0,
                        italic: false,
                    });
                }
                let face = vale_labeler::Face {
                    weight: style.weight,
                    italic: style.italic,
                };
                ui.label(item::STYLE);
                egui::ComboBox::from_id_salt("atlas-item-face")
                    .selected_text(face.name())
                    .show_ui(ui, |ui| {
                        for f in faces {
                            let on = f.weight == style.weight && f.italic == style.italic;
                            if ui.selectable_label(on, f.name()).clicked() {
                                atlas::edit_style(state, id, |s| {
                                    s.weight = f.weight;
                                    s.italic = f.italic;
                                });
                            }
                        }
                    });
                ui.end_row();
            }
            ui.label(item::SIZE);
            let mut size = style.size;
            let drag = egui::DragValue::new(&mut size)
                .range(vale_store::FurnitureText::SIZES)
                .speed(0.25)
                .suffix(" pt");
            let drag = ui.add(drag);
            named(&drag, egui::WidgetType::DragValue, item::SIZE);
            if drag.changed() {
                atlas::edit_style(state, id, |s| s.size = size);
            }
            ui.end_row();
            ui.label(item::COLOR);
            let mut color = style.color;
            let button = ui.color_edit_button_srgb(&mut color);
            named(&button, egui::WidgetType::ColorButton, item::COLOR);
            if button.changed() {
                atlas::set_item_color(state, id, color);
            }
            ui.end_row();
        });
}
