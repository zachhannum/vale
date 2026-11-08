//! The right dock of the desktop Atlas workspace: the inspector of the
//! feature selection, the panel of the selected frame or of the open page,
//! and the inspector of the layers. The dock shows only with a selection.

use eframe::egui;
use vale_sphere::{INTERVALS, ProjectionKind, interval_text};
use vale_store::{
    BlendMode, Border, BorderStyle, CoordFormat, EdgeLabels, EdgeOrientation, FrameRecord,
    FrameShape, Graticule, LayerId, Neatline, PageUnits, RasterDisplay,
};

use super::elevation::{DISPLAYS, blend_label};
use super::pad::raster::{BLEND, SHOW_AS};

use super::atlas::{self, AtlasSelection, EdgeSet, FrameTab};
use super::expr_field;
use super::expr_help;
use super::pad::atlas::{
    ADD_HOLE, ADD_PAGE, AUTO_INTERVAL, AZIMUTH, BORDER, BORDER_COLOR, BORDER_DISTANCE,
    BORDER_STROKE, BORDER_STYLE, BORDER_STYLES, BORDER_WIDTH, CENTER_LATITUDE, CENTER_LONGITUDE,
    CENTER_ON_FRAME, COPY_GLOBE_STYLE, DELETE_MAP_STYLE, DELETE_PAGE, DELETE_PROJECTION, EDITED,
    FIRST_PARALLEL, FRAME_TABS, GRATICULE_COLOR, GRATICULE_WIDTH, GRID_COLOR, GRID_SPACING, HEIGHT,
    INTERVAL, LABEL_FORMAT, LABEL_FORMATS, LATITUDE, LAYOUT_GRID, LONGITUDE, MAP_NAME, MAP_STYLE,
    MAP_STYLE_NAME, NEATLINE, NEATLINE_COLOR, NEATLINE_WIDTH, NO_MAP_STYLE, OBLIQUE, ORIENTATIONS,
    PAGE_NAME, PAPER_COLOR, POLYGON, PROJECTION_NAME, RECTANGLE, REFERENCE_GRID,
    RESET_TO_MAP_STYLE, ROTATION, SAVE_AS_MAP_STYLE, SAVE_PROJECTION, SCALE, SECOND_COLOR,
    SECOND_PARALLEL, SHAPE, SIZE, STROKE_COLOR, STROKE_WIDTH, UNIT_NAMES, UPDATE_MAP_STYLE, WIDTH,
    custom_row, edited_value, length_text, unit_name,
};
use super::pad::atlas::{
    BORDER_GAP, DISTANCE_BY_SIDE, GRID_COLUMN_NAMES, GRID_COLUMNS, SIDE_DISTANCES,
};
use super::pad::edge_labels::EDGE_LABELS;
use super::pad::grid_labels::GRID_LABELS;
use super::pad::labels::HALO_WIDTHS;
use super::pad::{GRATICULE, NO_LAYERS};
use super::{AppState, inspector, number, style_editor};
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
    map_style_controls(ui, state, frame.map);
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
    if polygon && ui.button(ADD_HOLE).clicked() {
        atlas::add_hole(state, id);
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
            let mut rotation = frame.rotation;
            let stepper = number::stepper(ROTATION, &mut rotation).step(1.0);
            if stepper.suffix("°").row(ui).changed() {
                atlas::set_rotation(state, id, rotation);
            }
        });
    let mut graticule = frame.graticule.on;
    if ui.checkbox(&mut graticule, GRATICULE).changed() {
        atlas::set_graticule(state, id, graticule);
    }
    if frame.graticule.on {
        graticule_rows(ui, state, frame);
    }
    let mut neatline = frame.neatline.on;
    if ui.checkbox(&mut neatline, NEATLINE).changed() {
        atlas::set_neatline(state, frame.id, neatline);
    }
    if frame.neatline.on {
        neatline_rows(ui, state, frame);
    }
    let mut border = frame.border.on;
    if ui.checkbox(&mut border, BORDER).changed() {
        atlas::set_border(state, frame.id, border);
    }
    if frame.border.on {
        border_rows(ui, state, frame);
    }
    grid_rows(ui, state, frame);
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
            let button = ui.color_edit_button_srgba_unmultiplied(&mut color);
            named(&button, egui::WidgetType::ColorButton, GRATICULE_COLOR);
            if button.changed() {
                atlas::set_graticule_color(state, id, color);
            }
            ui.end_row();
            let mut width = g.width;
            let stepper = number::stepper(GRATICULE_WIDTH, &mut width)
                .range(Graticule::WIDTHS)
                .step(0.1)
                .decimals(1)
                .suffix(" pt");
            if stepper.row(ui).changed() {
                atlas::set_graticule_width(state, id, width);
            }
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
                number::stepper(edge::SIZE, &mut set.size)
                    .range(EdgeLabels::SIZES)
                    .step(0.5)
                    .suffix(" pt")
                    .row(ui);
                ui.label(edge::COLOR);
                ui.color_edit_button_srgba_unmultiplied(&mut set.color);
                ui.end_row();
                ui.label(edge::HALO);
                ui.checkbox(&mut set.halo, "");
                ui.end_row();
                if set.halo {
                    ui.label(edge::HALO_COLOR);
                    ui.color_edit_button_srgba_unmultiplied(&mut set.halo_color);
                    ui.end_row();
                    number::stepper(edge::HALO_WIDTH, &mut set.halo_width)
                        .range(EdgeLabels::HALO_WIDTHS)
                        .step(0.1)
                        .suffix(" pt")
                        .row(ui);
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
                number::stepper(edge::OFFSET, &mut set.offset)
                    .range(EdgeLabels::OFFSETS)
                    .step(0.5)
                    .suffix(" pt")
                    .row(ui);
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

/// The color and the width of the neatline of a frame.
fn neatline_rows(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let (id, b) = (frame.id, &frame.neatline);
    egui::Grid::new("atlas-neatline")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(NEATLINE_COLOR);
            let mut color = b.color;
            let button = ui.color_edit_button_srgba_unmultiplied(&mut color);
            named(&button, egui::WidgetType::ColorButton, NEATLINE_COLOR);
            if button.changed() {
                atlas::set_neatline_color(state, id, color);
            }
            ui.end_row();
            let mut width = b.width;
            let stepper = number::stepper(NEATLINE_WIDTH, &mut width)
                .range(Neatline::WIDTHS)
                .decimals(2)
                .suffix(" pt");
            if stepper.row(ui).changed() {
                atlas::set_neatline_width(state, id, width);
            }
        });
}

/// The style, the colors, the width, the distance, and the stroke of the
/// border of a frame.
fn border_rows(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let (id, b) = (frame.id, &frame.border);
    egui::Grid::new("atlas-border")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(BORDER_STYLE);
            ui.horizontal(|ui| {
                for (style, name) in BorderStyle::ALL.into_iter().zip(BORDER_STYLES) {
                    if ui.selectable_label(b.style == style, name).clicked() {
                        atlas::set_border_style(state, id, style);
                    }
                }
            });
            ui.end_row();
            if let Some(c) = color(ui, BORDER_COLOR, b.color) {
                atlas::set_border_color(state, id, false, c);
            }
            if b.style == BorderStyle::Graduated
                && let Some(c) = color(ui, SECOND_COLOR, b.color2)
            {
                atlas::set_border_color(state, id, true, c);
            }
            let mut v = b.width_mm;
            let stepper = number::stepper(BORDER_WIDTH, &mut v)
                .range(Border::WIDTHS)
                .decimals(2)
                .suffix(" mm");
            if stepper.row(ui).changed() {
                atlas::set_border_width(state, id, v);
            }
            let by_side = atlas::border_by_side(state, b);
            if !by_side {
                let d = b.distances_mm[0];
                let mut v = d;
                let stepper = number::stepper(BORDER_DISTANCE, &mut v)
                    .range(Border::DISTANCES)
                    .step(0.5)
                    .suffix(" mm");
                if stepper.row(ui).changed() {
                    atlas::set_border_distance(state, id, None, v);
                }
            }
            if let Some(on) = check(ui, DISTANCE_BY_SIDE, by_side) {
                atlas::set_border_by_side(state, id, on);
            }
            if by_side {
                for (i, name) in SIDE_DISTANCES.into_iter().enumerate() {
                    let d = b.distances_mm[i];
                    let mut v = d;
                    let stepper = number::stepper(name, &mut v)
                        .range(Border::DISTANCES)
                        .step(0.5)
                        .suffix(" mm");
                    if stepper.row(ui).changed() {
                        atlas::set_border_distance(state, id, Some(i), v);
                    }
                }
            }
            let mut v = b.gap_mm;
            let stepper = number::stepper(BORDER_GAP, &mut v)
                .range(Border::GAPS)
                .step(0.1)
                .suffix(" mm");
            if stepper.row(ui).changed() {
                atlas::set_border_gap(state, id, v);
            }
            if let Some(on) = check(ui, BORDER_STROKE, b.stroke) {
                atlas::set_border_stroke(state, id, on);
            }
            if b.stroke {
                if let Some(c) = color(ui, STROKE_COLOR, b.stroke_color) {
                    atlas::set_border_stroke_color(state, id, c);
                }
                let widths = Border::STROKE_WIDTHS;
                let mut v = b.stroke_width;
                let stepper = number::stepper(STROKE_WIDTH, &mut v)
                    .range(widths)
                    .decimals(2)
                    .suffix(" pt");
                if stepper.row(ui).changed() {
                    atlas::set_border_stroke_width(state, id, v);
                }
            }
        });
}

/// The switch and the text of the reference grid of a frame.
fn grid_rows(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let (id, b) = (frame.id, &frame.border);
    let mut grid = b.grid;
    if ui.checkbox(&mut grid, REFERENCE_GRID).changed() {
        atlas::set_border_grid(state, id, grid);
    }
    if b.grid {
        ui.horizontal(|ui| {
            ui.label(GRID_COLUMNS);
            let all = vale_store::GridColumns::ALL;
            for (columns, name) in all.into_iter().zip(GRID_COLUMN_NAMES) {
                if ui
                    .selectable_label(b.grid_columns == columns, name)
                    .clicked()
                {
                    atlas::set_grid_columns(state, id, columns);
                }
            }
        });
        ui.label(GRID_LABELS);
        let edit =
            |state: &mut AppState, step: bool, f: &dyn Fn(&mut vale_store::FurnitureText)| {
                atlas::edit_grid_text(state, id, step, f);
            };
        let target = super::FontTarget::Grid(id);
        style_grid(
            ui,
            state,
            ("atlas-grid-text", id.0),
            target,
            &b.grid_text,
            &edit,
        );
    }
}

/// A switch for each layer, which turns the layer on or off in the
/// map of the frame, and the style of the chosen vector layer, for all maps
/// or for the map of the frame.
/// The map style of a map: a menu of the map styles, the name field and
/// the delete button of a linked map style, the button that saves the map
/// as a new map style, and the buttons of a map with local changes.
fn map_style_controls(ui: &mut egui::Ui, state: &mut AppState, map: vale_store::MapId) {
    let doc_atlas = &state.doc.project.atlas;
    let linked = doc_atlas.map(map).and_then(|m| m.style);
    let styles: Vec<_> = (doc_atlas.map_styles.iter())
        .map(|s| (s.id, s.name.clone()))
        .collect();
    let current = linked.and_then(|id| styles.iter().find(|s| s.0 == id).map(|s| s.1.clone()));
    let edited = !state.doc.local_changes(map).is_empty();
    let shown = match &current {
        Some(name) if edited => edited_value(name),
        Some(name) => name.clone(),
        None => NO_MAP_STYLE.to_owned(),
    };
    ui.horizontal(|ui| {
        ui.label(MAP_STYLE);
        let mut chosen = linked;
        egui::ComboBox::from_id_salt("atlas-map-style")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut chosen, None, NO_MAP_STYLE);
                for (id, name) in &styles {
                    ui.selectable_value(&mut chosen, Some(*id), name);
                }
            });
        if chosen != linked {
            state.set_map_style_link(map, chosen);
        }
    });
    if let (Some(style), Some(name)) = (linked, current) {
        ui.horizontal(|ui| {
            ui.label("Name");
            let mut text = match &state.atlas.map_style_name {
                Some((of, text)) if *of == style => text.clone(),
                _ => name.clone(),
            };
            let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(160.0));
            named(&field, egui::WidgetType::TextEdit, MAP_STYLE_NAME);
            if field.changed() {
                atlas::rename_map_style(state, style, &text);
                state.atlas.map_style_name = Some((style, text));
            }
        });
    }
    if edited {
        ui.horizontal(|ui| {
            if ui.button(UPDATE_MAP_STYLE).clicked() {
                state.update_map_style(map);
            }
            if ui.button(RESET_TO_MAP_STYLE).clicked() {
                state.reset_to_map_style(map);
            }
        });
    }
    ui.horizontal(|ui| {
        if ui.button(SAVE_AS_MAP_STYLE).clicked() {
            state.save_as_map_style(map);
            state.atlas.map_style_name = None;
        }
        if let Some(style) = linked
            && ui.button(DELETE_MAP_STYLE).clicked()
        {
            state.delete_map_style(style);
            state.atlas.map_style_name = None;
        }
    });
}

fn layers_tab(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let edited = state.doc.local_changes(frame.map);
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
            let check = ui.checkbox(&mut shown, &name);
            if check.changed() {
                state.set_map_shown(frame.map, layer, shown);
            }
            if state.doc.entry(layer).is_some() {
                check.context_menu(|ui| {
                    if ui.button(super::table::OPEN_TABLE).clicked() {
                        state.open_table(layer);
                        ui.close();
                    }
                });
            }
            if ui.selectable_label(open == Some(layer), "Style").clicked() {
                if open != Some(layer) {
                    state.select_vector(layer);
                }
                state.atlas.layer = (open != Some(layer)).then_some((frame.id, layer));
            }
            if edited.contains(&layer) {
                ui.weak(EDITED);
            }
        });
    }
    let Some(layer) = state.atlas.open_layer() else {
        return;
    };
    ui.separator();
    let Some(kind) = state.doc.layer(layer).and_then(|l| l.geometry()) else {
        look_controls(ui, state, frame, layer);
        return;
    };
    let fields = state
        .doc
        .layer(layer)
        .map_or(Vec::new(), |l| l.fields.clone());
    let target = frame.map;
    state.open_ramps();
    let ramps = state.ramps.list().to_vec();
    let symbols = state.doc.symbols().clone();
    let help = expr_help::ExprHelp::for_layer(state, layer);
    let Some(mut style) = state.doc.map_style(target, layer) else {
        return;
    };
    let counts = style_editor::filter_counts(state, layer, &style.style.rules);
    let mut library = style_editor::Library {
        symbols: &symbols,
        note: state.symbol_note.as_ref(),
        actions: &mut state.actions,
        help: &help,
        counts: &counts,
    };
    let mut changed = style_editor::rules(ui, layer, &mut style, kind, &fields, &mut library);
    if kind != vale_store::GeometryKind::Point {
        let (mut masks, polygons) = super::pad::rules::mask_choices(&state.doc, layer);
        masks.extend(polygons);
        ui.add_space(4.0);
        changed |= style_editor::mask_rows(ui, layer, &mut style.style.mask, &masks);
    }
    let polygon = kind == vale_store::GeometryKind::Polygon;
    let classes = style_editor::classify(ui, layer, &fields, &ramps, &mut library, polygon);
    if changed && state.doc.set_map_style(target, layer, &style) {
        state.touch();
    }
    if let Some(request) = classes {
        state.edit_map_style(frame.map, layer, |state| {
            super::pad::classify::apply(state, layer, &request);
        });
    }
    style_source(ui, state, frame, layer);
    if ui.button(super::table::OPEN_TABLE).clicked() {
        state.open_table(layer);
    }
}

/// The buttons that set the style of a layer in a map from its map style
/// or from the globe style.
fn style_source(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord, layer: LayerId) {
    let edited = state.doc.local_changes(frame.map).contains(&layer);
    if edited && ui.button(RESET_TO_MAP_STYLE).clicked() {
        state.reset_layer_to_map_style(frame.map, layer);
    }
    let differs = state.differs_from_globe(frame.map, layer);
    if ui
        .add_enabled(differs, egui::Button::new(COPY_GLOBE_STYLE))
        .clicked()
    {
        state.copy_globe_style(frame.map, layer);
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
        number::slider("Opacity", &mut look.opacity, 0.0..=1.0)
            .percent()
            .step(1.0)
            .row(ui);
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
    style_source(ui, state, frame, layer);
}

fn display_name(d: RasterDisplay) -> &'static str {
    let i = RasterDisplay::ALL.iter().position(|x| *x == d).unwrap_or(0);
    DISPLAYS[i]
}

fn projection_tab(ui: &mut egui::Ui, state: &mut AppState, frame: &FrameRecord) {
    let map = frame.map;
    let spec = state.doc.projection(map);
    if let Some(reason) = atlas::suggestion_reason(state, frame.id) {
        ui.add(egui::Label::new(egui::RichText::new(reason).weak()).truncate());
    }
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
                let mut value = azimuth;
                let stepper = number::stepper(AZIMUTH, &mut value).step(1.0);
                if stepper.suffix("°").row(ui).changed() {
                    atlas::set_azimuth(state, frame.id, value);
                }
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
        if let Some(c) = rgb_color(ui, PAPER_COLOR, page.paper) {
            atlas::set_paper(state, c);
        }

        let grid = state.doc.project.atlas.grid.clone();
        if let Some(on) = check(ui, LAYOUT_GRID, grid.on) {
            atlas::edit_grid(state, |g| g.on = on);
        }
        if grid.on {
            ui.label(GRID_SPACING);
            ui.horizontal(|ui| {
                let typed = field(ui, GRID_SPACING, &length_text(grid.spacing_mm, units));
                let mm = typed.and_then(|t| atlas::parse_length(&t, units));
                if let Some(mm) = mm.filter(|mm| vale_store::LayoutGrid::SPACINGS.contains(mm)) {
                    atlas::edit_grid(state, |g| g.spacing_mm = mm);
                }
                ui.label(unit_name(units));
            });
            ui.end_row();
            if let Some(c) = color(ui, GRID_COLOR, grid.color) {
                atlas::edit_grid(state, |g| g.color = c);
            }
        }
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
        ADD_CLASS, BUFFER, CASE, CURVE_LIMIT, DELETE_CLASS, DUPLICATE_DISTANCE, FOLLOW_PARALLELS,
        HALO, HALO_WIDTH, LABEL_SIZE, LETTER_SPACING, MOVE_DOWN, MOVE_UP, NO_LABEL_LAYERS, OFFSET,
        PLACEMENT, POSITIONS, REMOVE_DUPLICATES, REPEAT, REPEAT_INTERVAL, SMOOTHING, SPREAD,
        SPREADS, STYLE, UPSIDE_DOWN, can_follow_parallels, follows_parallels,
    };
    use super::pad::labels::{
        AVOID_POLYGONS, INSIDE, LARGEST_PART, LEADER_LINE, LEADER_START, MIN_AREA, MIN_AREAS,
        OUTSIDE, OVER_HOLES, OVERRUN, OVERRUNS, POLYGON_REPEAT_DISTANCES, REPEAT_DISTANCE,
        REPEAT_LABEL, ZONES,
    };
    use vale_store::GeometryKind;
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
    let classes = state.doc.label_classes_in(map, layer);
    let Some(mut class) = classes.get(i).cloned() else {
        state.atlas.label = None;
        return;
    };
    ui.separator();
    let before = class.clone();
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut class.name);
    });
    let help = expr_help::ExprHelp::for_layer(state, layer);
    let salt = egui::Id::new(("label-class", layer.0, i));
    let count = expr_help::matches(state, layer, &class.filter);
    ui.push_id(salt, |ui| {
        let text = super::pad::labels::TEXT;
        expr_field::label_text(ui, (text, text), &mut class.text, &help);
        let none = super::pad::filter::NO_CLASS_CONDITION;
        let filter = super::pad::rules::FILTER;
        expr_field::filter(ui, filter, &mut class.filter, &help, count, none);
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
        number::stepper(LABEL_SIZE, &mut class.size)
            .range(4.0..=72.0)
            .step(1.0)
            .suffix(" pt")
            .show(ui);
        style_editor::color_button(ui, &mut class.color);
    });
    egui::ComboBox::from_label(CASE)
        .selected_text(class.case.name())
        .show_ui(ui, |ui| {
            for c in vale_style::TextCase::ALL {
                ui.selectable_value(&mut class.case, c, c.name());
            }
        });
    number::stepper(LETTER_SPACING, &mut class.letter_spacing)
        .range(-0.1..=1.0)
        .factor(100.0)
        .step(1.0)
        .suffix(" %")
        .show(ui);
    ui.horizontal(|ui| {
        ui.checkbox(&mut class.halo, HALO);
        if class.halo {
            style_editor::color_button(ui, &mut class.halo_color);
            number::stepper(HALO_WIDTH, &mut class.halo_width)
                .range(HALO_WIDTHS.0..=HALO_WIDTHS.1)
                .step(0.1)
                .suffix(" pt")
                .show(ui);
        }
    });
    egui::ComboBox::from_label(PLACEMENT)
        .selected_text(class.placement.name())
        .show_ui(ui, |ui| {
            for &p in vale_style::LabelPlacement::choices(kind) {
                ui.selectable_value(&mut class.placement, p, p.name());
            }
        });
    if kind == GeometryKind::Point {
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
    if kind != GeometryKind::Polygon {
        number::stepper(OFFSET, &mut class.offset)
            .range(0.0..=20.0)
            .step(0.5)
            .suffix(" pt")
            .show(ui);
    }
    if kind == GeometryKind::Polygon {
        number::slider(SPREAD, &mut class.spread, SPREADS.0..=SPREADS.1)
            .step(0.1)
            .suffix("×")
            .show(ui);
    }
    if can_follow_parallels(kind, class.placement) {
        ui.checkbox(&mut class.graticule, FOLLOW_PARALLELS);
    }
    if follows_parallels(kind, &class) {
        ui.checkbox(&mut class.no_flip, UPSIDE_DOWN);
    }
    let curved = kind == GeometryKind::Line
        || (kind == GeometryKind::Polygon
            && class.placement != vale_style::LabelPlacement::Horizontal);
    if kind == GeometryKind::Line {
        ui.horizontal(|ui| {
            ui.checkbox(&mut class.repeat, REPEAT);
            if class.repeat {
                number::stepper(REPEAT_INTERVAL, &mut class.repeat_distance)
                    .range(50.0..=2000.0)
                    .step(10.0)
                    .suffix(" pt")
                    .show(ui);
            }
        });
    }
    if curved {
        let mut degrees = class.curve_limit.to_degrees();
        let stepper = number::stepper(CURVE_LIMIT, &mut degrees)
            .range(5.0..=60.0)
            .step(1.0)
            .decimals(0)
            .suffix("°");
        if stepper.show(ui).changed() {
            class.curve_limit = degrees.to_radians();
        }
    }
    if kind == GeometryKind::Polygon {
        ui.label(ZONES);
        ui.horizontal(|ui| {
            zone_grid(ui, INSIDE, true, &mut class.zones_inside);
            zone_grid(ui, OUTSIDE, false, &mut class.zones_outside);
        });
        if class.zones_outside.iter().any(|&r| r > 0) {
            ui.label(LEADER_LINE);
            egui::ComboBox::from_label(LEADER_START)
                .selected_text(class.anchor.name())
                .show_ui(ui, |ui| {
                    for anchor in vale_style::LeaderAnchor::ALL {
                        ui.selectable_value(&mut class.anchor, anchor, anchor.name());
                    }
                });
            leader_controls(ui, &mut class);
        }
        ui.checkbox(&mut class.over_holes, OVER_HOLES);
        ui.checkbox(&mut class.avoid_polygons, AVOID_POLYGONS);
        ui.checkbox(&mut class.largest_part, LARGEST_PART);
        ui.horizontal(|ui| {
            ui.checkbox(&mut class.polygon_repeat, REPEAT_LABEL);
            if class.polygon_repeat {
                let (lo, hi) = POLYGON_REPEAT_DISTANCES;
                number::slider(REPEAT_DISTANCE, &mut class.polygon_repeat_distance, lo..=hi)
                    .step(10.0)
                    .suffix(" pt")
                    .show(ui);
            }
        });
        number::stepper(OVERRUN, &mut class.overrun)
            .range(OVERRUNS.0..=OVERRUNS.1)
            .step(0.5)
            .suffix(" pt")
            .show(ui);
        number::stepper(MIN_AREA, &mut class.min_area)
            .range(MIN_AREAS.0..=MIN_AREAS.1)
            .step(50.0)
            .suffix(" pt²")
            .show(ui);
    }
    if kind == GeometryKind::Line {
        number::slider(SMOOTHING, &mut class.smoothing, 0.0..=5.0)
            .step(0.1)
            .suffix("×")
            .show(ui);
    }
    number::stepper(BUFFER, &mut class.buffer)
        .range(0.0..=10.0)
        .step(0.5)
        .suffix(" pt")
        .show(ui);
    ui.horizontal(|ui| {
        ui.checkbox(&mut class.remove_duplicates, REMOVE_DUPLICATES);
        if class.remove_duplicates {
            number::stepper(DUPLICATE_DISTANCE, &mut class.duplicate_distance)
                .range(0.0..=2000.0)
                .step(10.0)
                .suffix(" pt")
                .show(ui);
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
/// The row of the frame of an item of furniture.
fn frame_combo(ui: &mut egui::Ui, state: &mut AppState, rec: &vale_store::FurnitureRecord) {
    use super::pad::furniture as item;
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
                        atlas::set_item_frame(state, rec.id, Some(*frame));
                    }
                }
            });
    });
}

fn item_panel(ui: &mut egui::Ui, state: &mut AppState, rec: &vale_store::FurnitureRecord) {
    use super::atlas::TextPart;
    use super::pad::furniture as item;
    use vale_store::Furniture;
    let id = rec.id;
    let kind = atlas::ItemKind::of(&rec.item);
    ui.heading(kind.name());
    if let Furniture::ScaleBar(bar) = &rec.item {
        let kinds = &vale_store::ScaleBarKind::ALL;
        let style = vale_store::ScaleBarKind::label;
        egui::Grid::new("atlas-bar-kind-grid").show(ui, |ui| {
            if let Some(k) = combo(ui, "atlas-bar-kind", item::STYLE, kinds, style, bar.kind) {
                atlas::edit_bar(state, id, true, |b| b.kind = k);
            }
        });
    }
    if !matches!(rec.item, Furniture::Text(_)) {
        frame_combo(ui, state, rec);
    }
    match &rec.item {
        Furniture::Text(t) => {
            let help = expr_help::ExprHelp::for_text_item(state, id);
            let mut text = t.template.clone();
            let names = (item::TEXT, item::ITEM_TEXT);
            if expr_field::label_text(ui, names, &mut text, &help) {
                atlas::set_template(state, id, text);
            }
            // The frame row shows while the text uses a frame value.
            if let Some(now) = state.doc.project.atlas.furniture(id).cloned()
                && let Furniture::Text(t) = &now.item
                && crate::furniture::uses_frame(&t.template)
            {
                frame_combo(ui, state, &now);
            }
            text_grid(ui, state, id, TextPart::Text, &t.style);
            ui.horizontal(|ui| {
                ui.label(item::ALIGNMENT);
                let aligns = [
                    vale_store::TextAlign::Left,
                    vale_store::TextAlign::Center,
                    vale_store::TextAlign::Right,
                ];
                for (align, name) in aligns.into_iter().zip(item::ALIGNMENTS) {
                    if ui.selectable_label(t.align == align, name).clicked() {
                        atlas::item_step(state, id, |r| match &mut r.item {
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
            egui::CollapsingHeader::new(item::TITLE)
                .default_open(true)
                .show(ui, |ui| {
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
                    text_grid(ui, state, id, TextPart::Title, &l.title_style);
                });
            egui::CollapsingHeader::new(item::ENTRIES)
                .default_open(true)
                .show(ui, |ui| {
                    legend_entries(ui, state, id, l);
                    text_grid(ui, state, id, TextPart::Entries, &l.entry_style);
                });
        }
        Furniture::ScaleBar(bar) => bar_sections(ui, state, id, bar),
        Furniture::ColorKey(k) => key_sections(ui, state, id, k),
        Furniture::NorthArrow(n) => {
            ui.horizontal(|ui| {
                ui.label(item::ARROW_COLOR);
                let mut color = n.color;
                let button = ui.color_edit_button_srgba_unmultiplied(&mut color);
                named(&button, egui::WidgetType::ColorButton, item::ARROW_COLOR);
                if button.changed() {
                    atlas::edit_item(state, id, |r| match &mut r.item {
                        Furniture::NorthArrow(n) => {
                            n.color = color;
                            true
                        }
                        _ => false,
                    });
                }
            });
            let mut hide = n.hide_if_varies;
            if ui.checkbox(&mut hide, item::HIDE_VARIES).changed() {
                atlas::item_step(state, id, |r| match &mut r.item {
                    Furniture::NorthArrow(n) => {
                        n.hide_if_varies = hide;
                        true
                    }
                    _ => false,
                });
            }
            egui::CollapsingHeader::new(item::LETTER)
                .default_open(true)
                .show(ui, |ui| {
                    text_grid(ui, state, id, TextPart::Letter, &n.letter_style);
                });
        }
    }
    ui.add_space(6.0);
}

/// The entries of a legend: a switch and a name for each, and the reset
/// button.
fn legend_entries(
    ui: &mut egui::Ui,
    state: &mut AppState,
    id: vale_store::FurnitureId,
    l: &vale_store::LegendItem,
) {
    use super::pad::furniture as item;
    use vale_store::Furniture;
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
        atlas::item_step(state, id, |r| match &mut r.item {
            Furniture::Legend(l) => {
                l.entries.clear();
                true
            }
            _ => false,
        });
    }
}

/// The controls of a color key.
fn key_sections(
    ui: &mut egui::Ui,
    state: &mut AppState,
    id: vale_store::FurnitureId,
    k: &vale_store::ColorKeyItem,
) {
    use super::atlas::TextPart;
    use super::pad::furniture as item;
    use vale_store::{KeyLayout, SecondUnit};
    let layers = k
        .frame
        .map_or_else(Vec::new, |f| atlas::key_layers(&state.doc, f));
    ui.horizontal(|ui| {
        ui.label(item::KEY_LAYER);
        let shown = item::layer_name(state, k.layer);
        let resp = egui::ComboBox::from_id_salt("atlas-key-layer")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for layer in &layers {
                    let name = item::layer_name(state, Some(*layer));
                    if ui.selectable_label(k.layer == Some(*layer), name).clicked() {
                        let layer = *layer;
                        atlas::edit_key(state, id, true, |k| k.layer = Some(layer));
                    }
                }
            });
        named(&resp.response, egui::WidgetType::ComboBox, item::KEY_LAYER);
    });
    if k.frame.is_some() && atlas::key_data(state, k).is_none() {
        ui.weak(item::LAYER_HIDDEN);
    }
    ui.horizontal(|ui| {
        ui.label(item::KEY_LAYOUT);
        for (layout, name) in KeyLayout::ALL.into_iter().zip(item::KEY_LAYOUTS) {
            if ui.selectable_label(k.layout == layout, name).clicked() && k.layout != layout {
                atlas::edit_key(state, id, true, |k| k.layout = layout);
            }
        }
    });
    match k.layout {
        KeyLayout::Strip => {
            let split = vale_store::ColorKeyItem {
                split_at_sea_level: true,
                ..k.clone()
            };
            if atlas::key_data(state, &split).is_some_and(|d| d.sea.is_some()) {
                let mut on = k.split_at_sea_level;
                if ui.checkbox(&mut on, item::SPLIT).changed() {
                    atlas::edit_key(state, id, true, |k| k.split_at_sea_level = on);
                }
            }
            let preset = crate::color_key::preset_unit(&item::layer_unit(state, k.layer));
            let custom = k
                .second_unit
                .as_ref()
                .filter(|u| Some(*u) != preset.as_ref());
            let shown = match (&k.second_unit, custom) {
                (None, _) => item::NONE.to_string(),
                (Some(_), Some(_)) => item::CUSTOM.to_string(),
                (Some(u), None) => item::sentence_case(&u.name),
            };
            ui.horizontal(|ui| {
                ui.label(item::SECOND_UNIT);
                let mut chosen = None;
                let resp = egui::ComboBox::from_id_salt("atlas-key-unit")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(k.second_unit.is_none(), item::NONE)
                            .clicked()
                        {
                            chosen = Some(None);
                        }
                        if let Some(p) = &preset {
                            let on = k.second_unit.as_ref() == Some(p);
                            if ui
                                .selectable_label(on, item::sentence_case(&p.name))
                                .clicked()
                            {
                                chosen = Some(Some(p.clone()));
                            }
                        }
                        if ui
                            .selectable_label(custom.is_some(), item::CUSTOM)
                            .clicked()
                            && custom.is_none()
                        {
                            chosen = Some(Some(SecondUnit {
                                name: String::new(),
                                factor: 1.0,
                                offset: 0.0,
                            }));
                        }
                    });
                named(
                    &resp.response,
                    egui::WidgetType::ComboBox,
                    item::SECOND_UNIT,
                );
                if let Some(unit) = chosen {
                    atlas::edit_key(state, id, true, |k| k.second_unit = unit);
                }
            });
            if let Some(unit) = custom {
                egui::Grid::new("atlas-key-unit-grid").show(ui, |ui| {
                    ui.label("Name");
                    let mut name = unit.name.clone();
                    let edit = ui.text_edit_singleline(&mut name);
                    named(&edit, egui::WidgetType::TextEdit, item::UNIT_NAME);
                    if edit.changed() {
                        atlas::edit_key(state, id, false, |k| {
                            if let Some(u) = &mut k.second_unit {
                                u.name = name;
                            }
                        });
                    }
                    ui.end_row();
                    let parse = |t: &str| t.trim().replace(',', "").parse::<f64>().ok();
                    for (name, value, zero) in [
                        (item::FACTOR, unit.factor, false),
                        (item::OFFSET, unit.offset, true),
                    ] {
                        ui.label(name);
                        let typed = field(ui, name, &value.to_string());
                        let typed = typed.as_deref().and_then(parse);
                        if let Some(v) = typed.filter(|v| v.is_finite() && (zero || *v != 0.0)) {
                            atlas::edit_key(state, id, true, |k| {
                                if let Some(u) = &mut k.second_unit {
                                    if zero {
                                        u.offset = v;
                                    } else {
                                        u.factor = v;
                                    }
                                }
                            });
                        }
                        ui.end_row();
                    }
                });
            }
        }
        KeyLayout::List => {
            let mut on = k.names_in_boxes;
            if ui.checkbox(&mut on, item::NAMES_IN_BOXES).changed() {
                atlas::edit_key(state, id, true, |k| k.names_in_boxes = on);
            }
        }
    }
    egui::Grid::new("atlas-key-box-grid").show(ui, |ui| {
        for (name, value, height) in [
            (item::BOX_WIDTH, k.box_width, false),
            (item::BOX_HEIGHT, k.box_height, true),
        ] {
            ui.label(format!("{name} (mm)"));
            let shown = value.map_or_else(|| item::AUTO.to_string(), |v| v.to_string());
            if let Some(t) = field(ui, name, &shown) {
                let t = t.trim();
                let v = if t.is_empty() || t == item::AUTO {
                    Some(None)
                } else {
                    let v = t.replace(',', "").parse::<f64>().ok();
                    v.filter(|v| v.is_finite() && *v > 0.0).map(Some)
                };
                if let Some(v) = v.filter(|v| *v != value) {
                    atlas::edit_key(state, id, true, |k| {
                        if height {
                            k.box_height = v;
                        } else {
                            k.box_width = v;
                        }
                    });
                }
            }
            ui.end_row();
        }
    });
    egui::CollapsingHeader::new(item::TITLE)
        .default_open(true)
        .show(ui, |ui| {
            let mut title = k.title.clone();
            let edit = ui.text_edit_singleline(&mut title);
            named(&edit, egui::WidgetType::TextEdit, item::TITLE);
            if edit.changed() {
                atlas::edit_key(state, id, false, |k| k.title = title);
            }
            text_grid(ui, state, id, TextPart::Title, &k.title_style);
        });
    egui::CollapsingHeader::new(item::BANDS)
        .default_open(true)
        .show(ui, |ui| {
            let rows = atlas::key_data(state, k).map_or_else(Vec::new, |d| d.rows);
            for row in rows.into_iter().rev() {
                let name = k.bands.get(&row.key).and_then(|e| e.name.clone());
                ui.horizontal(|ui| {
                    let mut shown = !row.hidden;
                    let check = ui.checkbox(&mut shown, "");
                    named(
                        &check,
                        egui::WidgetType::Checkbox,
                        &format!("Show {}", row.key),
                    );
                    if check.changed() {
                        atlas::edit_band(state, id, row.key.clone(), true, |e| e.hidden = !shown);
                    }
                    let mut text = name.unwrap_or_default();
                    let edit = ui.add(egui::TextEdit::singleline(&mut text).hint_text(&row.label));
                    named(
                        &edit,
                        egui::WidgetType::TextEdit,
                        &format!("Band {}", row.key),
                    );
                    if edit.changed() {
                        let typed = text.trim().to_string();
                        let name = (!typed.is_empty()).then_some(typed);
                        atlas::edit_band(state, id, row.key, false, |e| e.name = name);
                    }
                });
            }
            let reset = ui.add_enabled(!k.bands.is_empty(), egui::Button::new(item::RESET_BANDS));
            if reset.clicked() {
                atlas::edit_key(state, id, true, |k| k.bands.clear());
            }
        });
    egui::CollapsingHeader::new(item::BORDER)
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new(("key-border", id.0))
                .num_columns(2)
                .show(ui, |ui| {
                    if let Some(c) = color(ui, item::BORDER_COLOR, k.border_color) {
                        atlas::edit_key(state, id, false, |k| k.border_color = c);
                    }
                    let mut v = k.border_width;
                    let stepper = number::stepper(item::BORDER_WIDTH, &mut v)
                        .range(0.0..=4.0)
                        .step(0.05)
                        .suffix(" pt");
                    if stepper.row(ui).changed() {
                        atlas::edit_key(state, id, false, |k| k.border_width = v.max(0.0));
                    }
                });
        });
    let mut parts = vec![(item::VALUES, TextPart::Values, &k.value_style)];
    if k.layout == KeyLayout::Strip && k.second_unit.is_some() {
        parts.push((
            item::SECOND_UNIT_VALUES,
            TextPart::SecondUnit,
            &k.second_unit_style,
        ));
    }
    if k.layout == KeyLayout::List && k.names_in_boxes {
        parts.push((item::BOX_NAMES, TextPart::BoxNames, &k.box_text_style));
    }
    for (name, part, style) in parts {
        egui::CollapsingHeader::new(name)
            .default_open(false)
            .show(ui, |ui| text_grid(ui, state, id, part, style));
    }
}

/// A combo box row of a grid. Returns the choice that the user made.
fn combo<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    all: &[T],
    name: fn(T) -> &'static str,
    current: T,
) -> Option<T> {
    let mut chosen = None;
    ui.label(label);
    let resp = egui::ComboBox::from_id_salt(id)
        .selected_text(name(current))
        .show_ui(ui, |ui| {
            for c in all {
                if ui.selectable_label(*c == current, name(*c)).clicked() && *c != current {
                    chosen = Some(*c);
                }
            }
        });
    named(&resp.response, egui::WidgetType::ComboBox, label);
    ui.end_row();
    chosen
}

/// A color row of a grid, with the alpha of the color. Returns the new
/// color.
fn color(ui: &mut egui::Ui, label: &str, color: vale_store::Rgba) -> Option<vale_store::Rgba> {
    ui.label(label);
    let mut c = color;
    let button = ui.color_edit_button_srgba_unmultiplied(&mut c);
    named(&button, egui::WidgetType::ColorButton, label);
    ui.end_row();
    button.changed().then_some(c)
}

/// A color row of a grid, for an opaque color. Returns the new color.
fn rgb_color(ui: &mut egui::Ui, label: &str, color: vale_store::Rgb) -> Option<vale_store::Rgb> {
    ui.label(label);
    let mut c = color;
    let button = ui.color_edit_button_srgb(&mut c);
    named(&button, egui::WidgetType::ColorButton, label);
    ui.end_row();
    button.changed().then_some(c)
}

/// A switch row of a grid. Returns the new value.
fn check(ui: &mut egui::Ui, label: &str, on: bool) -> Option<bool> {
    let mut v = on;
    ui.label("");
    let changed = ui.checkbox(&mut v, label).changed();
    ui.end_row();
    changed.then_some(v)
}

/// The text settings of a text part of an item.
fn text_grid(
    ui: &mut egui::Ui,
    state: &mut AppState,
    id: vale_store::FurnitureId,
    part: super::atlas::TextPart,
    style: &vale_store::FurnitureText,
) {
    let edit = |state: &mut AppState, step: bool, f: &dyn Fn(&mut vale_store::FurnitureText)| {
        atlas::edit_text(state, id, part, step, f);
    };
    let target = super::FontTarget::Item(id, part);
    style_grid(ui, state, ("atlas-item-text", part), target, style, &edit);
}

/// A change of a text style. With `step`, the change is one undo step.
type TextChange<'a> = dyn Fn(&mut AppState, bool, &dyn Fn(&mut vale_store::FurnitureText)) + 'a;

/// The text settings of a text style, in a grid with the id `salt`.
fn style_grid(
    ui: &mut egui::Ui,
    state: &mut AppState,
    salt: impl std::hash::Hash + std::fmt::Debug + Copy,
    target: super::FontTarget,
    style: &vale_store::FurnitureText,
    edit: &TextChange,
) {
    use super::pad::furniture as item;
    use vale_store::{FurnitureText, TextCase};
    egui::Grid::new(salt).num_columns(2).show(ui, |ui| {
        ui.label(item::FONT);
        let family = style
            .font
            .as_deref()
            .unwrap_or(crate::fonts::DEFAULT_FAMILY);
        let button = ui.button(format!("{family}…"));
        named(&button, egui::WidgetType::Button, item::FONT);
        if button.clicked() {
            state.actions.push(super::Action::PickFont(target));
        }
        ui.end_row();
        let family = crate::fonts::family(style.font.as_deref(), &mut state.fonts).to_owned();
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
        egui::ComboBox::from_id_salt(("face", salt))
            .selected_text(face.name())
            .show_ui(ui, |ui| {
                for f in faces {
                    let on = f.weight == style.weight && f.italic == style.italic;
                    if ui.selectable_label(on, f.name()).clicked() {
                        edit(state, true, &|s| {
                            s.weight = f.weight;
                            s.italic = f.italic;
                        });
                    }
                }
            });
        ui.end_row();
        let mut v = style.size;
        let stepper = number::stepper(item::SIZE, &mut v)
            .range(FurnitureText::SIZES)
            .step(0.5)
            .suffix(" pt");
        if stepper.row(ui).changed() {
            edit(state, false, &|s| s.size = v);
        }
        let cases = &TextCase::ALL;
        if let Some(c) = combo(
            ui,
            ("case", salt),
            item::CASE,
            cases,
            TextCase::name,
            style.case,
        ) {
            edit(state, true, &|s| s.case = c);
        }
        let mut v = style.letter_spacing;
        let stepper = number::stepper(item::LETTER_SPACING, &mut v)
            .range(FurnitureText::LETTER_SPACINGS)
            .factor(100.0)
            .step(1.0)
            .suffix(" %");
        if stepper.row(ui).changed() {
            edit(state, false, &|s| s.letter_spacing = v);
        }
        if let Some(c) = color(ui, item::COLOR, style.color) {
            edit(state, false, &|s| s.color = c);
        }
        if let Some(on) = check(ui, item::HALO, style.halo) {
            edit(state, true, &|s| s.halo = on);
        }
        if style.halo {
            if let Some(c) = color(ui, item::HALO_COLOR, style.halo_color) {
                edit(state, false, &|s| s.halo_color = c);
            }
            let range = FurnitureText::HALO_WIDTHS;
            let mut v = style.halo_width;
            let stepper = number::stepper(item::HALO_WIDTH, &mut v)
                .range(range)
                .step(0.1)
                .suffix(" pt");
            if stepper.row(ui).changed() {
                edit(state, false, &|s| s.halo_width = v);
            }
        }
    });
}

/// The rows of a fill of a scale bar in a grid: the kind, the color, and
/// for a hatch the background, the angle, the spacing, and the line width.
fn fill_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    id: vale_store::FurnitureId,
    label: &str,
    second: bool,
    fill: &vale_store::FillPattern,
    outline: vale_store::Rgba,
) {
    use super::pad::furniture as item;
    use vale_store::{FillKind, FillPattern};
    let edit = |state: &mut AppState, step: bool, f: &dyn Fn(&mut FillPattern)| {
        atlas::edit_fill(state, id, second, step, f);
    };
    let kinds = &FillKind::ALL;
    if let Some(k) = combo(
        ui,
        ("atlas-bar-fill", second),
        label,
        kinds,
        FillKind::label,
        fill.kind,
    ) {
        edit(state, true, &|f| item::set_kind(f, k, outline));
    }
    if fill.kind == FillKind::None {
        return;
    }
    let name = if fill.kind.hatched() {
        item::LINE_COLOR
    } else {
        item::COLOR
    };
    if let Some(c) = color(ui, name, fill.color) {
        edit(state, false, &|f| f.color = c);
    }
    if !fill.kind.hatched() {
        return;
    }
    if let Some(on) = check(ui, item::BACKGROUND, fill.background.is_some()) {
        let back = on.then_some([0xff, 0xff, 0xff, 0xff]);
        edit(state, true, &|f| f.background = back);
    }
    if let Some(back) = fill.background
        && let Some(c) = color(ui, item::BACKGROUND_COLOR, back)
    {
        edit(state, false, &|f| f.background = Some(c));
    }
    let mut v = fill.angle;
    let stepper = number::stepper(item::ANGLE, &mut v)
        .range(FillPattern::ANGLES)
        .step(5.0)
        .suffix("°");
    if stepper.row(ui).changed() {
        edit(state, false, &|f| f.angle = v);
    }
    let mut v = fill.spacing;
    let stepper = number::stepper(item::SPACING, &mut v)
        .range(FillPattern::SPACINGS)
        .step(0.1)
        .suffix(" mm");
    if stepper.row(ui).changed() {
        edit(state, false, &|f| f.spacing = v);
    }
    let widths = FillPattern::LINE_WIDTHS;
    let mut v = fill.line_width;
    let stepper = number::stepper(item::LINE_WIDTH, &mut v)
        .range(widths)
        .step(0.05)
        .suffix(" pt");
    if stepper.row(ui).changed() {
        edit(state, false, &|f| f.line_width = v);
    }
}

/// The sections of a scale bar: the units, the divisions, the bar, the
/// marks, and the numbers.
fn bar_sections(
    ui: &mut egui::Ui,
    state: &mut AppState,
    id: vale_store::FurnitureId,
    bar: &vale_store::ScaleBarItem,
) {
    use super::atlas::{BarPage, BarPick, TextPart};
    use super::pad::furniture as item;
    use vale_store::{
        ScaleBarItem, ScaleBarKind, ScaleFit, ScalePosition, ScaleUnit, TickFrequency,
        UnitLabelStyle, UnitPosition,
    };
    let edit = |state: &mut AppState, step: bool, f: &dyn Fn(&mut ScaleBarItem)| {
        atlas::edit_bar(state, id, step, f);
    };
    let section =
        |page: BarPage| egui::CollapsingHeader::new(page.name()).id_salt(("atlas-bar", page));
    section(BarPage::Units).show(ui, |ui| {
        egui::Grid::new("atlas-bar-units")
            .num_columns(2)
            .show(ui, |ui| {
                let units = &ScaleUnit::ALL;
                if let Some(u) = combo(
                    ui,
                    "atlas-bar-unit",
                    BarPick::Unit.name(),
                    units,
                    ScaleUnit::label,
                    bar.unit,
                ) {
                    edit(state, true, &|b| {
                        b.unit = u;
                        if b.second_unit == Some(u) {
                            b.second_unit = None;
                        }
                    });
                }
                ui.label(BarPick::SecondUnit.name());
                let shown = bar.second_unit.map_or(item::NONE, ScaleUnit::label);
                egui::ComboBox::from_id_salt("atlas-bar-second")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        let mut all = vec![None];
                        all.extend(units.iter().filter(|u| **u != bar.unit).map(|u| Some(*u)));
                        for u in all {
                            let name = u.map_or(item::NONE, ScaleUnit::label);
                            if ui.selectable_label(u == bar.second_unit, name).clicked() {
                                edit(state, true, &|b| {
                                    b.second_unit = u;
                                    b.second_division_value = None;
                                });
                            }
                        }
                    });
                ui.end_row();
                let styles = &UnitLabelStyle::ALL;
                let name = BarPick::UnitLabel.name();
                if let Some(s) = combo(
                    ui,
                    "atlas-bar-label",
                    name,
                    styles,
                    UnitLabelStyle::label,
                    bar.unit_label_style,
                ) {
                    edit(state, true, &|b| b.unit_label_style = s);
                }
                if bar.unit_label_style == UnitLabelStyle::Custom {
                    let mut fields = vec![(item::CUSTOM_LABEL, bar.unit_label.clone(), false)];
                    if bar.second_unit.is_some() {
                        fields.push((item::SECOND_LABEL, bar.second_unit_label.clone(), true));
                    }
                    for (label, mut text, second) in fields {
                        ui.label(label);
                        let resp = ui.text_edit_singleline(&mut text);
                        named(&resp, egui::WidgetType::TextEdit, label);
                        if resp.changed() {
                            edit(state, false, &|b| {
                                if second {
                                    b.second_unit_label = text.clone();
                                } else {
                                    b.unit_label = text.clone();
                                }
                            });
                        }
                        ui.end_row();
                    }
                }
                let places = &UnitPosition::ALL;
                let name = BarPick::UnitPosition.name();
                if let Some(p) = combo(
                    ui,
                    "atlas-bar-unit-place",
                    name,
                    places,
                    UnitPosition::label,
                    bar.unit_position,
                ) {
                    edit(state, true, &|b| b.unit_position = p);
                }
                let mut v = bar.unit_gap;
                let stepper = number::stepper(item::GAP, &mut v)
                    .range(ScaleBarItem::GAPS)
                    .step(0.1)
                    .suffix(" mm");
                if stepper.row(ui).changed() {
                    edit(state, false, &|b| b.unit_gap = v);
                }
            });
        text_grid(ui, state, id, TextPart::Units, &bar.unit_style);
    });
    section(BarPage::Divisions).show(ui, |ui| {
        egui::Grid::new("atlas-bar-divisions")
            .num_columns(2)
            .show(ui, |ui| {
                for (label, value, range, sub) in [
                    (
                        item::DIVISIONS,
                        bar.divisions,
                        ScaleBarItem::DIVISIONS,
                        false,
                    ),
                    (
                        item::SUBDIVISIONS,
                        bar.subdivisions,
                        ScaleBarItem::SUBDIVISIONS,
                        true,
                    ),
                ] {
                    let mut v = value;
                    let stepper = number::stepper(label, &mut v).range(range).step(1.0);
                    if stepper.decimals(0).row(ui).changed() {
                        edit(state, false, &|b| {
                            if sub {
                                b.subdivisions = v;
                            } else {
                                b.divisions = v;
                            }
                        });
                    }
                }
                let all = &vale_store::DivisionSpacing::ALL;
                let label = vale_store::DivisionSpacing::label;
                if let Some(v) = combo(
                    ui,
                    "atlas-bar-spacing",
                    item::SPACING,
                    all,
                    label,
                    bar.spacing,
                ) {
                    edit(state, true, &|b| b.spacing = v);
                }
                if let Some(on) = check(ui, item::BEFORE_ZERO, bar.before_zero) {
                    edit(state, true, &|b| b.before_zero = on);
                }
                let fits = &ScaleFit::ALL;
                if let Some(f) = combo(
                    ui,
                    "atlas-bar-fit",
                    BarPick::Fit.name(),
                    fits,
                    ScaleFit::label,
                    bar.fit,
                ) {
                    edit(state, true, &|b| b.fit = f);
                }
                if bar.fit == ScaleFit::AdjustDivisionValue {
                    return;
                }
                let auto = bar.division_value.is_none();
                if let Some(on) = check(ui, item::AUTO_VALUE, auto) {
                    let drawn = atlas::bar_rows(state, id);
                    let value = |i: usize| drawn.get(i).map(|r| r.value);
                    let (first, second) = (value(0).unwrap_or(1.0), value(1));
                    edit(state, true, &|b| {
                        b.division_value = (!on).then_some(first);
                        b.second_division_value = if on { None } else { b.second_unit.and(second) };
                    });
                }
                if auto {
                    return;
                }
                let rows = [
                    (
                        Some(bar.unit),
                        bar.division_value,
                        item::DIVISION_VALUE,
                        false,
                    ),
                    (
                        bar.second_unit,
                        bar.second_division_value,
                        item::SECOND_VALUE,
                        true,
                    ),
                ];
                for (unit, value, label, second) in rows {
                    let Some(unit) = unit else {
                        continue;
                    };
                    let suffix = format!(" {}", unit.abbreviation());
                    let shown = value.unwrap_or(1.0);
                    let mut v = shown;
                    let stepper = number::stepper(label, &mut v)
                        .range(1e-6..=1e9)
                        .suffix(&suffix);
                    if stepper.row(ui).changed() {
                        edit(state, false, &|b| {
                            if second {
                                b.second_division_value = Some(v);
                            } else {
                                b.division_value = Some(v);
                            }
                        });
                    }
                }
            });
    });
    section(BarPage::Bar).show(ui, |ui| {
        egui::Grid::new("atlas-bar-bar")
            .num_columns(2)
            .show(ui, |ui| {
                let kind = bar.kind;
                if kind != ScaleBarKind::Line {
                    let mut v = bar.bar_height;
                    let stepper = number::stepper(item::BAR_HEIGHT, &mut v)
                        .range(ScaleBarItem::BAR_HEIGHTS)
                        .step(0.1)
                        .suffix(" mm");
                    if stepper.row(ui).changed() {
                        edit(state, false, &|b| b.bar_height = v);
                    }
                }
                let fills: &[(&str, bool)] = match kind {
                    ScaleBarKind::Alternating | ScaleBarKind::DoubleAlternating => {
                        &[(item::FILL_1, true), (item::FILL_2, false)]
                    }
                    ScaleBarKind::SingleDivision => &[(item::FILL, true)],
                    ScaleBarKind::Hollow => &[(item::FILL, false)],
                    ScaleBarKind::SteppedLine | ScaleBarKind::Line => &[],
                };
                for &(label, first) in fills {
                    let fill = if first { &bar.fill1 } else { &bar.fill2 };
                    fill_rows(ui, state, id, label, !first, fill, bar.outline);
                }
                let line = matches!(kind, ScaleBarKind::SteppedLine | ScaleBarKind::Line);
                let label = if line {
                    item::LINE_COLOR
                } else {
                    item::OUTLINE
                };
                if let Some(c) = color(ui, label, bar.outline) {
                    edit(state, false, &|b| b.outline = c);
                }
                let mut v = bar.line_width;
                let stepper = number::stepper(item::LINE_WIDTH, &mut v)
                    .range(ScaleBarItem::LINE_WIDTHS)
                    .step(0.1)
                    .suffix(" pt");
                if stepper.row(ui).changed() {
                    edit(state, false, &|b| b.line_width = v);
                }
            });
    });
    let freqs = &TickFrequency::ALL;
    let places = &ScalePosition::ALL;
    section(BarPage::Marks).show(ui, |ui| {
        egui::Grid::new("atlas-bar-marks")
            .num_columns(2)
            .show(ui, |ui| {
                let name = BarPick::MarkFrequency.name();
                if let Some(f) = combo(
                    ui,
                    "atlas-bar-mark-freq",
                    name,
                    freqs,
                    TickFrequency::label,
                    bar.mark_frequency,
                ) {
                    edit(state, true, &|b| b.mark_frequency = f);
                }
                if let Some(p) = combo(
                    ui,
                    "atlas-bar-mark-place",
                    item::POSITION,
                    places,
                    item::position_name,
                    bar.mark_position,
                ) {
                    edit(state, true, &|b| b.mark_position = p);
                }
                let range = ScaleBarItem::MARK_HEIGHTS;
                let mut v = bar.division_mark_height;
                let stepper = number::stepper(item::DIVISION_HEIGHT, &mut v)
                    .range(range.clone())
                    .step(0.1)
                    .suffix(" mm");
                if stepper.row(ui).changed() {
                    edit(state, false, &|b| b.division_mark_height = v);
                }
                let mut v = bar.subdivision_mark_height;
                let stepper = number::stepper(item::SUBDIVISION_HEIGHT, &mut v)
                    .range(range)
                    .step(0.1)
                    .suffix(" mm");
                if stepper.row(ui).changed() {
                    edit(state, false, &|b| b.subdivision_mark_height = v);
                }
            });
    });
    section(BarPage::Numbers).show(ui, |ui| {
        egui::Grid::new("atlas-bar-numbers")
            .num_columns(2)
            .show(ui, |ui| {
                let name = BarPick::NumberFrequency.name();
                if let Some(f) = combo(
                    ui,
                    "atlas-bar-number-freq",
                    name,
                    freqs,
                    TickFrequency::label,
                    bar.number_frequency,
                ) {
                    edit(state, true, &|b| b.number_frequency = f);
                }
                if let Some(p) = combo(
                    ui,
                    "atlas-bar-number-place",
                    item::POSITION,
                    places,
                    item::position_name,
                    bar.number_position,
                ) {
                    edit(state, true, &|b| b.number_position = p);
                }
                let mut v = bar.number_gap;
                let stepper = number::stepper(item::GAP, &mut v)
                    .range(ScaleBarItem::GAPS)
                    .step(0.1)
                    .suffix(" mm");
                if stepper.row(ui).changed() {
                    edit(state, false, &|b| b.number_gap = v);
                }
                ui.label(BarPick::Decimals.name());
                let text =
                    |d: Option<u32>| d.map_or(item::AUTO_DECIMALS.to_string(), |d| d.to_string());
                egui::ComboBox::from_id_salt("atlas-bar-decimals")
                    .selected_text(text(bar.decimals))
                    .show_ui(ui, |ui| {
                        let all = std::iter::once(None).chain(ScaleBarItem::DECIMALS.map(Some));
                        for d in all {
                            if ui.selectable_label(d == bar.decimals, text(d)).clicked() {
                                edit(state, true, &|b| b.decimals = d);
                            }
                        }
                    });
                ui.end_row();
                if let Some(on) = check(ui, item::THOUSANDS, bar.thousands) {
                    edit(state, true, &|b| b.thousands = on);
                }
                if let Some(on) = check(ui, item::SHOW_ZERO, bar.zero_label) {
                    edit(state, true, &|b| b.zero_label = on);
                }
            });
        text_grid(ui, state, id, TextPart::Numbers, &bar.number_style);
    });
}

/// A 3×3 grid of the zones of one section, under its title. `ranks` runs
/// row by row from the top left, and the outside grid has no center. A
/// click on a zone gives it the next rank.
fn zone_grid(ui: &mut egui::Ui, title: &str, inside: bool, ranks: &mut [u8]) {
    use super::pad::labels::{ZONE_OFF, next_rank, zone_name};
    ui.vertical(|ui| {
        ui.label(title);
        egui::Grid::new(("zones", inside)).show(ui, |ui| {
            for cell in 0..9 {
                let k = match (inside, cell) {
                    (true, _) => Some(cell),
                    (false, 4) => None,
                    (false, c) if c < 4 => Some(c),
                    (false, c) => Some(c - 1),
                };
                match k {
                    Some(k) => {
                        let text = match ranks[k] {
                            0 => ZONE_OFF.to_owned(),
                            n => n.to_string(),
                        };
                        let button = egui::Button::new(text).min_size(egui::vec2(32.0, 24.0));
                        let resp = ui.add(button).on_hover_text(zone_name(inside, k));
                        if resp.clicked() {
                            ranks[k] = next_rank(ranks[k]);
                        }
                    }
                    None => {
                        ui.label("");
                    }
                }
                if cell % 3 == 2 {
                    ui.end_row();
                }
            }
        });
    });
}

/// The style of the leader line of a label class.
fn leader_controls(ui: &mut egui::Ui, class: &mut vale_style::LabelClass) {
    use super::pad::labels::{
        ANCHOR_GAP, COLOR, DASH, DASH_GAP, DASHED, DASHES, END_SIZE, END_SIZES, HALO, HALO_COLOR,
        HALO_WIDTH, LABEL_GAP, LEADER_END, LEADER_GAPS, LINE_HALO_WIDTHS, LINE_WIDTH, LINE_WIDTHS,
        MATCH_HALO, MATCH_TEXT, SHOW_LINE,
    };
    use vale_style::LeaderEnd;
    let (text, text_halo) = (class.color, class.halo_color);
    let l = &mut class.leader;
    ui.checkbox(&mut l.show, SHOW_LINE);
    if !l.show {
        return;
    }
    let color_button = |ui: &mut egui::Ui, label: &str, c: &mut vale_style::Color| {
        ui.horizontal(|ui| {
            style_editor::color_button(ui, c);
            ui.label(label);
        });
    };
    let mut matched = l.color.is_none();
    if ui.checkbox(&mut matched, MATCH_TEXT).changed() {
        l.color = (!matched).then_some(text);
    }
    if let Some(c) = &mut l.color {
        color_button(ui, COLOR, c);
    }
    number::stepper(LINE_WIDTH, &mut l.width)
        .range(LINE_WIDTHS.0..=LINE_WIDTHS.1)
        .step(0.25)
        .suffix(" pt")
        .show(ui);
    ui.horizontal(|ui| {
        ui.checkbox(&mut l.dashed, DASHED);
        if l.dashed {
            for (label, value) in [(DASH, &mut l.dash), (DASH_GAP, &mut l.dash_gap)] {
                number::stepper(label, value)
                    .range(DASHES.0..=DASHES.1)
                    .step(0.5)
                    .suffix(" pt")
                    .show(ui);
            }
        }
    });
    ui.checkbox(&mut l.halo, HALO);
    if l.halo {
        let mut matched = l.halo_color.is_none();
        if ui.checkbox(&mut matched, MATCH_HALO).changed() {
            l.halo_color = (!matched).then_some(text_halo);
        }
        if let Some(c) = &mut l.halo_color {
            color_button(ui, HALO_COLOR, c);
        }
        number::stepper(HALO_WIDTH, &mut l.halo_width)
            .range(LINE_HALO_WIDTHS.0..=LINE_HALO_WIDTHS.1)
            .step(0.25)
            .suffix(" pt")
            .show(ui);
    }
    egui::ComboBox::from_label(LEADER_END)
        .selected_text(l.end.name())
        .show_ui(ui, |ui| {
            for end in LeaderEnd::ALL {
                ui.selectable_value(&mut l.end, end, end.name());
            }
        });
    if l.end != LeaderEnd::None {
        number::stepper(END_SIZE, &mut l.end_size)
            .range(END_SIZES.0..=END_SIZES.1)
            .step(0.5)
            .suffix(" pt")
            .show(ui);
    }
    for (label, value) in [
        (LABEL_GAP, &mut l.label_gap),
        (ANCHOR_GAP, &mut l.anchor_gap),
    ] {
        number::stepper(label, value)
            .range(LEADER_GAPS.0..=LEADER_GAPS.1)
            .step(0.5)
            .suffix(" pt")
            .show(ui);
    }
}
