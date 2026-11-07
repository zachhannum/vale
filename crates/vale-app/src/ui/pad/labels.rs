//! The Labels tab of the panel of a frame in the Atlas workspace: the label
//! classes of the map of the frame, and the panel of one class with its
//! pages.

use eframe::egui::{self, Align2, Rect, pos2, vec2};
use vale_labeler::Face;
use vale_store::{FrameId, FurnitureText, GeometryKind, LayerId, MapId};
use vale_style::expr::Filter;
use vale_style::{LabelClass, LabelPlacement, LabelPosition};

use super::atlas::{FRAME_TABS, named_tabs, panel_width};
use super::filter;
use super::geometry::{Rects, TOUCH};
use super::icons::{self, Icon};
use super::label_text;
use super::number::Number;
use super::text_style::{self, TextEdit, TextEvent, TextSpec};
use super::widgets::{self, Controls, PANEL_ROW};
use super::{Card, CardTitle, show_card, theme};
use crate::fonts;
use crate::ui::atlas::{AtlasSelection, ClassColor, ClassDrag, ClassPage, ClassSegment};
use crate::ui::expr_help::{self, ExprHelp};
use crate::ui::{Action, AppState, FontTarget};

pub const ADD_CLASS: &str = "Add class";
pub const DELETE_CLASS: &str = "Delete class";
pub const NO_LABEL_LAYERS: &str = "This map has no vector layers.";
pub const NO_CLASSES: &str = "No layer of this map has labels.";
pub const CLASS_NAME: &str = "Class name";
pub const LAYER: &str = "Layer";
pub const TEXT: &str = "Text";
/// The row of the face of the font, and the title of its page.
pub const STYLE: &str = "Style";
/// The name of the size slider. Its label is "Size".
pub const LABEL_SIZE: &str = "Label size";
pub const CASE: &str = "Case";
pub const LETTER_SPACING: &str = "Letter spacing";
pub const COLOR: &str = "Color";
pub const HALO: &str = "Halo";
pub const HALO_COLOR: &str = "Halo color";
pub const HALO_WIDTH: &str = "Halo width";
pub const PLACEMENT: &str = "Placement";
/// The segments of the panel of a class: the name of each control and its
/// label.
pub const SEGMENTS: [(&str, &str); 3] = [
    ("Text segment", "Text"),
    ("Placement segment", "Placement"),
    ("Conflicts segment", "Conflicts"),
];
/// The row of the positions of a point class, and the title of its page.
pub const POSITIONS: &str = "Positions";
pub const OFFSET: &str = "Offset";
pub const REPEAT: &str = "Repeat";
pub const REPEAT_INTERVAL: &str = "Repeat interval";
pub const CURVE_LIMIT: &str = "Curve limit";
pub const SMOOTHING: &str = "Smoothing";
pub const SPREAD: &str = "Spread";
pub const FOLLOW_PARALLELS: &str = "Follow parallels";
pub const UPSIDE_DOWN: &str = "Allow upside-down text";
/// The row of the zones of a polygon class, and the title of its page.
pub const ZONES: &str = "Zones";
/// The headings of the zones page.
pub const INSIDE: &str = "Inside";
pub const OUTSIDE: &str = "Outside";
/// The row of the leader line, and the title of its page.
pub const LEADER_LINE: &str = "Leader line";
/// The headings of the leader line page.
pub const LEADER_START: &str = "Start";
pub const LEADER_END: &str = "End";
pub const SHOW_LINE: &str = "Show line";
/// The switch that gives the line the color of the text.
pub const MATCH_TEXT: &str = "Match text";
/// The switch that gives the halo of the line the halo color of the label.
pub const MATCH_HALO: &str = "Match label halo";
pub const LINE_WIDTH: &str = "Width";
pub const DASHED: &str = "Dashed";
pub const DASH: &str = "Dash";
pub const DASH_GAP: &str = "Gap";
pub const END_SIZE: &str = "End size";
pub const LABEL_GAP: &str = "Gap at label";
pub const ANCHOR_GAP: &str = "Gap at anchor";
pub const OVER_HOLES: &str = "Label over holes";
pub const AVOID_POLYGONS: &str = "Avoid other polygons";
pub const LARGEST_PART: &str = "Largest part only";
pub const REPEAT_LABEL: &str = "Repeat label";
pub const REPEAT_DISTANCE: &str = "Repeat distance";
pub const OVERRUN: &str = "Overrun";
pub const MIN_AREA: &str = "Minimum area";
/// The text of a blocked zone.
pub const ZONE_OFF: &str = "Off";
/// The names of the zones inside a polygon, row by row from the top left.
pub const INSIDE_ZONES: [&str; 9] = [
    "top left",
    "top",
    "top right",
    "left",
    "center",
    "right",
    "bottom left",
    "bottom",
    "bottom right",
];
/// The names of the zones outside a polygon, row by row from the top left.
pub const OUTSIDE_ZONES: [&str; 8] = [
    "top left",
    "top",
    "top right",
    "left",
    "right",
    "bottom left",
    "bottom",
    "bottom right",
];
/// The highest rank that a tap on a zone gives.
pub const MAX_ZONE_RANK: u8 = 3;
pub const BUFFER: &str = "Buffer";
pub const REMOVE_DUPLICATES: &str = "Remove duplicates";
pub const DUPLICATE_DISTANCE: &str = "Duplicate distance";
pub const MOVE_UP: &str = "Move up";
pub const MOVE_DOWN: &str = "Move down";
/// The start of the name of the handle of a row of the class list. The
/// name of the class follows.
pub const REORDER: &str = "Reorder";
/// The smallest and the largest size of a label, in points.
pub const SIZES: (f64, f64) = (4.0, 72.0);
/// The smallest and the largest letter spacing, as fractions of the size.
pub const SPACINGS: (f64, f64) = (-0.1, 1.0);
/// The thinnest and the widest halo, in points.
pub const HALO_WIDTHS: (f64, f64) = (0.1, 8.0);
/// The smallest and the largest offset, in points.
pub const OFFSETS: (f64, f64) = (0.0, 20.0);
/// The shortest and the longest repeat interval, in points.
pub const REPEAT_INTERVALS: (f64, f64) = (50.0, 2000.0);
/// The smallest and the largest curve limit, in degrees per letter.
pub const CURVE_LIMITS: (f64, f64) = (5.0, 60.0);
/// The smallest and the largest smoothing, in multiples of the size.
pub const SMOOTHINGS: (f64, f64) = (0.0, 5.0);
/// The smallest and the largest spread, in multiples of the size.
pub const SPREADS: (f64, f64) = (0.0, 4.0);
/// The shortest and the longest distance between repeated polygon labels, in points.
pub const POLYGON_REPEAT_DISTANCES: (f64, f64) = (50.0, 2000.0);
/// The smallest and the largest overrun, in points.
pub const OVERRUNS: (f64, f64) = (0.0, 20.0);
/// The smallest and the largest minimum area, in square points.
pub const MIN_AREAS: (f64, f64) = (0.0, 10000.0);
/// The thinnest and the widest leader line, in points.
pub const LINE_WIDTHS: (f64, f64) = (0.25, 5.0);
/// The shortest and the longest dash and gap of a leader line, in points.
pub const DASHES: (f64, f64) = (0.5, 20.0);
/// The thinnest and the widest halo of a leader line, in points on each side.
pub const LINE_HALO_WIDTHS: (f64, f64) = (0.25, 5.0);
/// The smallest and the largest end mark, in points.
pub const END_SIZES: (f64, f64) = (1.0, 12.0);
/// The smallest and the largest gap at each end of a leader line, in points.
pub const LEADER_GAPS: (f64, f64) = (0.0, 10.0);
/// The smallest and the largest buffer, in points.
pub const BUFFERS: (f64, f64) = (0.0, 10.0);
/// The smallest and the largest duplicate distance, in points.
pub const DUPLICATE_DISTANCES: (f64, f64) = (0.0, 2000.0);

/// The name of the feature type of a layer on the Layer page.
fn kind_name(kind: GeometryKind) -> &'static str {
    match kind {
        GeometryKind::Point => "Points",
        GeometryKind::Line => "Lines",
        GeometryKind::Polygon => "Polygons",
    }
}

fn layer_name(state: &AppState, layer: LayerId) -> String {
    state
        .doc
        .layer(layer)
        .map_or_else(String::new, |l| l.name.clone())
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).color(theme::MUTE));
    ui.add_space(4.0);
}

/// The open label class of the Labels tab of the selected frame, with the
/// map of the frame. An open class that is gone closes.
pub(super) fn open_class(state: &mut AppState) -> Option<(MapId, LayerId, usize)> {
    let (layer, i) = state.atlas.open_class()?;
    let map = state.atlas.frame().and_then(|f| {
        let frame = state.doc.project.atlas.frame(f)?;
        Some(frame.map)
    });
    let found = map.filter(|m| {
        let fits = state.doc.layer(layer).and_then(|l| l.geometry()).is_some();
        fits && i < state.doc.label_classes(Some(*m), layer).len()
    });
    match found {
        Some(map) => Some((map, layer, i)),
        None => {
            state.atlas.label = None;
            None
        }
    }
}

fn show_class(state: &mut AppState, frame: FrameId, layer: LayerId, i: usize) {
    state.atlas.label = Some((frame, layer, i));
    state.atlas.label_page = None;
    state.atlas.label_segment = ClassSegment::Text;
    state.atlas.label_color = None;
    state.atlas.position_drag = None;
}

/// The label classes of the map of frame `id` in priority order, and the
/// button that adds a class. A tap on a class opens its panel, and a drag
/// on the handle of a row moves the class in the order.
pub(super) fn tab(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: FrameId) {
    let Some(map) = state.doc.project.atlas.frame(id).map(|f| f.map) else {
        return;
    };
    ui.spacing_mut().item_spacing.y = 0.0;
    if state.label_layers().is_empty() {
        state.atlas.label_drag = None;
        hint(ui, NO_LABEL_LAYERS);
        return;
    }
    // A class with the name of its layer shows the name one time.
    let list: Vec<(LayerId, usize, String, String)> = (state.label_class_list(map).into_iter())
        .map(|(layer, i, c)| {
            let mut of = layer_name(state, layer);
            if of == c.name {
                of.clear();
            }
            (layer, i, c.name, of)
        })
        .collect();
    let mut open = None;
    if list.is_empty() {
        state.atlas.label_drag = None;
        hint(ui, NO_CLASSES);
    } else {
        open = class_rows(ui, state, controls, map, &list);
    }
    ui.add_space(6.0);
    if widgets::button_row(ui, controls, ADD_CLASS, true).clicked() {
        open = state.add_label_class(map);
    }
    if let Some((layer, i)) = open {
        show_class(state, id, layer, i);
    }
}

/// The rows of the class list: the layer, the place in the classes of the
/// layer, the name of the class, and the name of the layer or an empty
/// text. Returns the
/// class that the user taps.
fn class_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    map: MapId,
    list: &[(LayerId, usize, String, String)],
) -> Option<(LayerId, usize)> {
    let size = vec2(ui.available_width(), list.len() as f32 * PANEL_ROW);
    let (area, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let slot =
        |top: f32| Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), PANEL_ROW));
    let place = |k: usize| slot(area.top() + k as f32 * PANEL_ROW);
    let (pointer, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_down()));

    let Some(drag) = state.atlas.label_drag.filter(|d| d.from < list.len()) else {
        state.atlas.label_drag = None;
        let mut open = None;
        for (k, (layer, i, name, layer_name)) in list.iter().enumerate() {
            let rect = place(k);
            let row = ui
                .push_id(("class", layer.0, *i), |ui| {
                    class_row(ui, Some(&mut *controls), rect, name, layer_name, false)
                })
                .inner;
            let Some((body, handle)) = row else { continue };
            if body.clicked() {
                open = Some((*layer, *i));
            }
            if handle.drag_started() || handle.dragged() {
                let y = pointer.map_or(rect.center().y, |p| p.y);
                state.atlas.label_drag = Some(ClassDrag {
                    from: k,
                    grab: y - rect.top(),
                });
                ui.ctx().request_repaint();
            }
        }
        return open;
    };

    // The lifted row follows the finger, and the other rows make room at
    // the place where it goes.
    let from = drag.from;
    let y = pointer.map_or(place(from).top() + drag.grab, |p| p.y);
    let top = (y - drag.grab).clamp(area.top(), area.bottom() - PANEL_ROW);
    let to = ((top - area.top()) / PANEL_ROW).round() as usize;
    let others = list.iter().enumerate().filter(|(k, _)| *k != from);
    for (n, (_, (_, _, name, layer_name))) in others.enumerate() {
        let rect = place(if n < to { n } else { n + 1 });
        class_row(ui, None, rect, name, layer_name, false);
    }
    let (_, _, name, layer_name) = &list[from];
    class_row(ui, None, slot(top), name, layer_name, true);
    if down {
        ui.ctx().request_repaint();
    } else {
        state.atlas.label_drag = None;
        if to != from {
            state.reorder_label_classes(map, from, to);
        }
    }
    None
}

/// One row of the class list in `rect`: the name of the class, the name of
/// its layer, and the handle at the right. With `controls`, the row takes a
/// tap, its handle takes a drag, and the function returns both. A `lifted`
/// row shows above the others.
fn class_row(
    ui: &egui::Ui,
    controls: Option<&mut Controls>,
    rect: Rect,
    name: &str,
    layer_name: &str,
    lifted: bool,
) -> Option<(egui::Response, egui::Response)> {
    let handle = Rect::from_center_size(
        pos2(rect.right() - TOUCH / 2.0 + 8.0, rect.center().y),
        vec2(TOUCH, TOUCH),
    );
    let body = Rect::from_min_max(rect.min, pos2(handle.left(), rect.bottom()));
    let out = controls.map(|controls| {
        let body = widgets::tap_area(ui, controls, body, name, true);
        let grip = format!("{REORDER} {name}");
        (body, widgets::drag_area(ui, controls, handle, &grip))
    });
    let highlight = rect.expand2(vec2(8.0, -widgets::HIGHLIGHT_INSET));
    let painter = ui.painter();
    if lifted {
        let shadow = highlight.translate(vec2(0.0, 2.0)).expand(2.0);
        painter.rect_filled(shadow, 7.0, egui::Color32::from_black_alpha(60));
        painter.rect_filled(highlight, 5.0, theme::card(false).to_opaque());
    }
    let pressed = out
        .as_ref()
        .is_some_and(|(b, _)| b.is_pointer_button_down_on());
    if lifted || pressed {
        painter.rect_filled(highlight, 5.0, theme::raise());
    }
    let at = pos2(rect.left(), rect.center().y);
    painter.text(at, Align2::LEFT_CENTER, name, theme::body(), theme::TEXT);
    let at = pos2(handle.left() - 4.0, rect.center().y);
    painter.text(
        at,
        Align2::RIGHT_CENTER,
        layer_name,
        theme::body(),
        theme::MUTE,
    );
    let color = if lifted { theme::TEXT } else { theme::MUTE };
    icons::paint(painter, handle, Icon::Grip, widgets::ICON, color);
    out
}

/// The panel of the open label class of frame `id`, or of one of its pages.
pub(super) fn class_panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    id: FrameId,
) -> Option<Rect> {
    let (map, layer, i) = open_class(state)?;
    let kind = state.doc.layer(layer)?.geometry()?;
    let class = state.doc.label_classes(Some(map), layer).get(i)?.clone();
    let page = state.atlas.label_page;
    let (key, title, back) = match page {
        Some(ClassPage::Layer) => ("pad-atlas-class-layer", LAYER, class.name.as_str()),
        Some(ClassPage::Placement) => ("pad-atlas-placement", PLACEMENT, &*class.name),
        Some(ClassPage::Style) => ("pad-atlas-class-style", STYLE, &*class.name),
        Some(ClassPage::Case) => ("pad-atlas-class-case", CASE, &*class.name),
        Some(ClassPage::Positions) => ("pad-atlas-positions", POSITIONS, &*class.name),
        Some(ClassPage::Zones) => ("pad-atlas-zones", ZONES, &*class.name),
        Some(ClassPage::LeaderLine) => ("pad-atlas-leader", LEADER_LINE, &*class.name),
        None => ("pad-atlas-class", class.name.as_str(), FRAME_TABS[3]),
    };
    let head = CardTitle {
        title,
        back: Some(back),
        action: None,
    };
    let card = Card {
        key,
        place: (rects.right_panel, panel_width(ctx)),
        head,
        // The panel stays still while a finger drags a position.
        scroll: match state.atlas.position_drag {
            Some(_) => egui::scroll_area::ScrollSource::NONE,
            None => egui::scroll_area::ScrollSource::default(),
        },
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.back {
            match page {
                Some(_) => state.atlas.label_page = None,
                None => state.atlas.label = None,
            }
        }
        if row.close {
            state.atlas.selection = AtlasSelection::None;
            state.atlas.vertices.clear();
        }
    };
    let top = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        if page.is_none() {
            segments(ui, state, controls);
        }
    };
    let segment = state.atlas.label_segment;
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let at = (id, map, layer, i);
        match page {
            Some(ClassPage::Layer) => layer_page(ui, state, controls, at),
            Some(ClassPage::Placement) => placement_page(ui, state, controls, at, kind, &class),
            Some(ClassPage::Style) => style_page(ui, state, controls, at, &class),
            Some(ClassPage::Case) => case_page(ui, state, controls, at, &class),
            Some(ClassPage::Positions) => positions_page(ui, state, controls, at, &class),
            Some(ClassPage::Zones) => zones_page(ui, state, controls, at, &class),
            Some(ClassPage::LeaderLine) => leader_page(ui, state, controls, at, &class),
            None => {
                match segment {
                    ClassSegment::Text => text_segment(ui, state, controls, at, &class),
                    ClassSegment::Placement => {
                        placement_segment(ui, state, controls, at, kind, &class);
                    }
                    ClassSegment::Conflicts => conflicts_segment(ui, state, controls, at, &class),
                }
                class_buttons(ui, state, controls, at);
            }
        }
    };
    Some(show_card(
        ctx, state, controls, rects, card, on_title, top, body,
    ))
}

/// The segments at the top of the panel of a class.
fn segments(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let items = SEGMENTS.map(|(name, label)| (name, label, true));
    let active = state.atlas.label_segment as usize;
    let all = [
        ClassSegment::Text,
        ClassSegment::Placement,
        ClassSegment::Conflicts,
    ];
    if let Some(chosen) = named_tabs(ui, controls, &items, active) {
        state.atlas.label_segment = all[chosen];
        state.atlas.label_color = None;
        widgets::close_keypad(ui.ctx());
    }
    ui.add_space(6.0);
}

/// The place of a label class: the frame of the panel, the map, the layer,
/// and the place of the class in the classes of the layer.
type ClassAt = (FrameId, MapId, LayerId, usize);

fn filter_ok(text: &str) -> bool {
    Filter::new(text).error().is_none()
}

/// A color row of a class and its color picker while it is open, with an
/// opacity strip. Returns the new color.
pub(super) fn color_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    named: (&str, ClassColor),
    c: vale_style::Color,
) -> Option<vale_style::Color> {
    picker_rows(ui, state, controls, named, c, true)
}

/// A color row and its color picker while it is open. `alpha`: the picker
/// has an opacity strip. Without it, the color stays opaque.
pub(super) fn picker_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (name, which): (&str, ClassColor),
    c: vale_style::Color,
    alpha: bool,
) -> Option<vale_style::Color> {
    let open = state.atlas.label_color == Some(which);
    if widgets::color_row(ui, controls, name, [c.r, c.g, c.b, c.a], open).clicked() {
        state.atlas.label_color = (!open).then_some(which);
        if open {
            widgets::close_keypad(ui.ctx());
        }
    }
    if state.atlas.label_color != Some(which) {
        return None;
    }
    ui.add_space(4.0);
    state.atlas.label_color_shown = true;
    let mut rgba = [c.r, c.g, c.b, c.a];
    let changed = if alpha {
        widgets::color_picker_alpha(ui, controls, name, &mut rgba)
    } else {
        let mut rgb = [c.r, c.g, c.b];
        let changed = widgets::color_picker(ui, controls, name, &mut rgb);
        rgba = widgets::opaque(rgb);
        changed
    };
    changed.then(|| {
        let [r, g, b, a] = rgba;
        vale_style::Color { r, g, b, a }
    })
}

/// The face of a class.
fn face(class: &LabelClass) -> Face {
    Face {
        weight: class.weight,
        italic: class.italic,
    }
}

/// The faces of the font family of `font`. A family with no list of faces
/// gives the regular face.
pub(super) fn faces_of(state: &mut AppState, font: Option<&str>) -> Vec<Face> {
    let family = fonts::family(font, &mut state.fonts).to_owned();
    let faces = state.fonts.faces(&family);
    if faces.is_empty() {
        vec![Face {
            weight: 400.0,
            italic: false,
        }]
    } else {
        faces
    }
}

/// The Text segment of the panel of a class.
fn text_segment(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    let mut name = class.name.clone();
    if widgets::text_field(ui, controls, CLASS_NAME, "Name", None, &mut name, true).changed() {
        state.edit_label_class(map, layer, i, |c| c.name = name);
    }
    let layer_name = layer_name(state, layer);
    if widgets::open_row(ui, controls, LAYER, &layer_name).clicked() {
        state.atlas.label_page = Some(ClassPage::Layer);
    }

    let help = ExprHelp::for_layer(state, layer);
    let names = (TEXT, TEXT);
    let edit = label_text::editor(ui, controls, names, &class.text, &help);
    if let Some(text) = edit.text {
        if edit.step {
            state.label_class_step(map, layer, i, |c| c.text = text);
        } else {
            state.edit_label_class(map, layer, i, |c| c.text = text);
        }
    }

    ui.add_space(6.0);
    let none = filter::NO_CLASS_CONDITION;
    let count = expr_help::matches(state, layer, &class.filter);
    let salt = (map, layer, i);
    let edit = filter::editor(ui, controls, salt, &class.filter, &help, count, none);
    if let Some(text) = edit.text.filter(|t| filter_ok(t)) {
        if edit.step {
            state.label_class_step(map, layer, i, |c| c.filter = text);
        } else {
            state.edit_label_class(map, layer, i, |c| c.filter = text);
        }
    }
    ui.add_space(6.0);

    let spec = TextSpec {
        size_name: LABEL_SIZE,
        size_step: 1.0,
        sizes: SIZES,
        spacings: SPACINGS,
        halo_widths: HALO_WIDTHS,
        colors: (ClassColor::Text, ClassColor::Halo),
    };
    let style = text_style(class);
    match text_style::section(ui, state, controls, &style, spec) {
        Some(TextEvent::PickFont) => {
            let target = FontTarget::Class(map, layer, i);
            state.actions.push(Action::PickFont(target));
        }
        Some(TextEvent::OpenFaces) => state.atlas.label_page = Some(ClassPage::Style),
        Some(TextEvent::OpenCases) => state.atlas.label_page = Some(ClassPage::Case),
        Some(TextEvent::Edit(edit)) => {
            let apply = |c: &mut LabelClass| apply_text(&edit, c);
            if edit.step() {
                state.label_class_step(map, layer, i, apply);
            } else {
                state.edit_label_class(map, layer, i, apply);
            }
        }
        None => {}
    }
}

/// The text settings of a class as a text style.
fn text_style(class: &LabelClass) -> FurnitureText {
    let rgba = |c: vale_style::Color| [c.r, c.g, c.b, c.a];
    FurnitureText {
        font: class.font.clone(),
        weight: class.weight,
        italic: class.italic,
        size: class.size,
        color: rgba(class.color),
        case: class.case,
        letter_spacing: class.letter_spacing,
        halo: class.halo,
        halo_color: rgba(class.halo_color),
        halo_width: class.halo_width,
    }
}

/// Applies a change of the Text section to a class.
fn apply_text(edit: &TextEdit, c: &mut LabelClass) {
    let mut style = text_style(c);
    edit.apply(&mut style);
    let color = |[r, g, b, a]: [u8; 4]| vale_style::Color { r, g, b, a };
    c.font = style.font;
    c.weight = style.weight;
    c.italic = style.italic;
    c.size = style.size;
    c.color = color(style.color);
    c.case = style.case;
    c.letter_spacing = style.letter_spacing;
    c.halo = style.halo;
    c.halo_color = color(style.halo_color);
    c.halo_width = style.halo_width;
}

/// True if a class of `kind` with `placement` can have straight text that follows the parallels.
pub fn can_follow_parallels(kind: GeometryKind, placement: LabelPlacement) -> bool {
    match kind {
        GeometryKind::Point => true,
        GeometryKind::Polygon => {
            matches!(
                placement,
                LabelPlacement::BestFit | LabelPlacement::Horizontal
            )
        }
        GeometryKind::Line => false,
    }
}

/// True if the text of a class follows the parallels.
pub fn follows_parallels(kind: GeometryKind, class: &LabelClass) -> bool {
    (kind == GeometryKind::Polygon && class.placement == LabelPlacement::Parallel)
        || (class.graticule && can_follow_parallels(kind, class.placement))
}

/// The switches of a class whose text can follow the parallels.
fn parallel_switches(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (map, layer, i): (MapId, LayerId, usize),
    kind: GeometryKind,
    class: &LabelClass,
) {
    if can_follow_parallels(kind, class.placement) {
        let mut on = class.graticule;
        if widgets::switch_row(ui, controls, FOLLOW_PARALLELS, FOLLOW_PARALLELS, &mut on) {
            state.label_class_step(map, layer, i, |c| c.graticule = on);
        }
    }
    if follows_parallels(kind, class) {
        let mut on = class.no_flip;
        if widgets::switch_row(ui, controls, UPSIDE_DOWN, UPSIDE_DOWN, &mut on) {
            state.label_class_step(map, layer, i, |c| c.no_flip = on);
        }
    }
}

/// The Placement segment of the panel of a class. A point class has its
/// positions and its offset. A line class has its offset, its repeat, its
/// curve limit, and its smoothing. A polygon class has its spread, and a
/// class that is not horizontal has its curve limit. A point class and a
/// polygon class can follow the parallels.
fn placement_segment(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    kind: GeometryKind,
    class: &LabelClass,
) {
    if widgets::open_row(ui, controls, PLACEMENT, class.placement.name()).clicked() {
        state.atlas.label_page = Some(ClassPage::Placement);
    }
    if kind == GeometryKind::Polygon {
        let spread = Number::new(SPREAD, class.spread, SPREADS.0..=SPREADS.1)
            .step(0.1)
            .unit("×")
            .default(vale_style::SPREAD)
            .slider(ui, controls);
        if let Some(spread) = spread {
            state.edit_label_class(map, layer, i, |c| c.spread = spread);
        }
        if class.placement != LabelPlacement::Horizontal {
            curve_limit(ui, state, controls, (map, layer, i), class);
        }
        parallel_switches(ui, state, controls, (map, layer, i), kind, class);
        polygon_options(ui, state, controls, (map, layer, i), class);
        return;
    }
    let point = kind == GeometryKind::Point;
    if point {
        let on = class.positions.iter().filter(|r| r.on).count();
        let value = format!("{on} of {}", class.positions.len());
        if widgets::open_row(ui, controls, POSITIONS, &value).clicked() {
            state.atlas.label_page = Some(ClassPage::Positions);
            state.atlas.position_drag = None;
        }
    }
    let offset = Number::new(OFFSET, class.offset, OFFSETS.0..=OFFSETS.1)
        .step(0.5)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(offset) = offset {
        state.edit_label_class(map, layer, i, |c| c.offset = offset);
    }
    if point {
        parallel_switches(ui, state, controls, (map, layer, i), kind, class);
        return;
    }

    let mut repeat = class.repeat;
    if widgets::switch_row(ui, controls, REPEAT, REPEAT, &mut repeat) {
        state.label_class_step(map, layer, i, |c| c.repeat = repeat);
    }
    if class.repeat {
        let (lo, hi) = REPEAT_INTERVALS;
        let distance = Number::new(REPEAT_INTERVAL, class.repeat_distance, lo..=hi)
            .nice()
            .step(10.0)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(distance) = distance {
            state.edit_label_class(map, layer, i, |c| c.repeat_distance = distance);
        }
    }
    curve_limit(ui, state, controls, (map, layer, i), class);
    let smoothing = Number::new(SMOOTHING, class.smoothing, SMOOTHINGS.0..=SMOOTHINGS.1)
        .step(0.1)
        .unit("×")
        .default(vale_style::SMOOTHING)
        .slider(ui, controls);
    if let Some(smoothing) = smoothing {
        state.edit_label_class(map, layer, i, |c| c.smoothing = smoothing);
    }
}

/// The name of a zone button: the section and the zone, for example
/// "Inside top left".
pub fn zone_name(inside: bool, k: usize) -> String {
    if inside {
        format!("{INSIDE} {}", INSIDE_ZONES[k])
    } else {
        format!("{OUTSIDE} {}", OUTSIDE_ZONES[k])
    }
}

/// The rank of a zone after a tap: 1, 2, 3, then off, then 1 again.
pub fn next_rank(rank: u8) -> u8 {
    match rank {
        0 => 1,
        r if r < MAX_ZONE_RANK => r + 1,
        _ => 0,
    }
}

/// True if a label of the class can go outside its polygon.
fn has_outside(class: &LabelClass) -> bool {
    class.zones_outside.iter().any(|&r| r > 0)
}

/// The zones, the leader start, the switches, and the numbers of a polygon
/// class.
fn polygon_options(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (map, layer, i): (MapId, LayerId, usize),
    class: &LabelClass,
) {
    let on = (class.zones_inside.iter())
        .chain(&class.zones_outside)
        .filter(|&&r| r > 0)
        .count();
    let all = class.zones_inside.len() + class.zones_outside.len();
    if widgets::open_row(ui, controls, ZONES, &format!("{on} of {all}")).clicked() {
        state.atlas.label_page = Some(ClassPage::Zones);
    }
    let value = if class.leader.show {
        class.anchor.name()
    } else {
        ZONE_OFF
    };
    if has_outside(class) && widgets::open_row(ui, controls, LEADER_LINE, value).clicked() {
        state.atlas.label_page = Some(ClassPage::LeaderLine);
    }
    type Set = fn(&mut LabelClass, bool);
    let switches: [(&str, bool, Set); 3] = [
        (OVER_HOLES, class.over_holes, |c, on| c.over_holes = on),
        (AVOID_POLYGONS, class.avoid_polygons, |c, on| {
            c.avoid_polygons = on;
        }),
        (LARGEST_PART, class.largest_part, |c, on| {
            c.largest_part = on
        }),
    ];
    for (name, mut on, set) in switches {
        if widgets::switch_row(ui, controls, name, name, &mut on) {
            state.label_class_step(map, layer, i, |c| set(c, on));
        }
    }
    let mut repeat = class.polygon_repeat;
    if widgets::switch_row(ui, controls, REPEAT_LABEL, REPEAT_LABEL, &mut repeat) {
        state.label_class_step(map, layer, i, |c| c.polygon_repeat = repeat);
    }
    if class.polygon_repeat {
        let (lo, hi) = POLYGON_REPEAT_DISTANCES;
        let distance = Number::new(REPEAT_DISTANCE, class.polygon_repeat_distance, lo..=hi)
            .nice()
            .step(10.0)
            .unit("pt")
            .default(vale_style::POLYGON_REPEAT_DISTANCE)
            .slider(ui, controls);
        if let Some(distance) = distance {
            state.edit_label_class(map, layer, i, |c| c.polygon_repeat_distance = distance);
        }
    }
    let overrun = Number::new(OVERRUN, class.overrun, OVERRUNS.0..=OVERRUNS.1)
        .step(0.5)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(overrun) = overrun {
        state.edit_label_class(map, layer, i, |c| c.overrun = overrun);
    }
    let area = Number::new(MIN_AREA, class.min_area, MIN_AREAS.0..=MIN_AREAS.1)
        .step(50.0)
        .unit("pt²")
        .stepper(ui, controls);
    if let Some(area) = area {
        state.edit_label_class(map, layer, i, |c| c.min_area = area);
    }
}

/// The side of a zone button.
const ZONE_BUTTON: f32 = 56.0;
/// The space between two zone buttons.
const ZONE_GAP: f32 = 6.0;

/// A 3×3 grid of the zones of one section. `ranks` runs row by row from the
/// top left, and the outside grid has no center. Returns the place of the
/// zone that the user taps.
fn zone_grid(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    inside: bool,
    ranks: &[u8],
) -> Option<usize> {
    let side = 3.0 * ZONE_BUTTON + 2.0 * ZONE_GAP;
    let size = vec2(ui.available_width(), side + 8.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let grid = Rect::from_center_size(rect.center(), vec2(side, side));
    let mut tapped = None;
    for (k, &rank) in ranks.iter().enumerate() {
        // The outside grid skips the center.
        let cell = if inside || k < 4 { k } else { k + 1 };
        let (x, y) = ((cell % 3) as f32, (cell / 3) as f32);
        let min = grid.min + vec2(x, y) * (ZONE_BUTTON + ZONE_GAP);
        let button = Rect::from_min_size(min, vec2(ZONE_BUTTON, ZONE_BUTTON));
        let name = zone_name(inside, k);
        let resp = widgets::tap_area(ui, controls, button, &name, true);
        if resp.clicked() {
            tapped = Some(k);
        }
        let pressed = resp.is_pointer_button_down_on();
        let (fill, text, color) = match rank {
            0 => (theme::track(), ZONE_OFF.to_owned(), theme::MUTE),
            n => (theme::raise(), n.to_string(), theme::TEXT),
        };
        let fill = if pressed { theme::divider() } else { fill };
        let painter = ui.painter();
        painter.rect_filled(button, theme::CONTROL_RADIUS, fill);
        painter.text(
            button.center(),
            Align2::CENTER_CENTER,
            text,
            theme::body(),
            color,
        );
    }
    if !inside {
        let center = Rect::from_center_size(grid.center(), vec2(ZONE_BUTTON, ZONE_BUTTON));
        ui.painter().rect_stroke(
            center,
            theme::CONTROL_RADIUS,
            egui::Stroke::new(1.0, theme::divider()),
            egui::StrokeKind::Inside,
        );
    }
    tapped
}

/// The zones inside and outside the polygons of a class, each with its
/// rank. A tap on a zone gives it the next rank.
fn zones_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    super::raster::heading(ui, INSIDE);
    if let Some(k) = zone_grid(ui, controls, true, &class.zones_inside) {
        let rank = next_rank(class.zones_inside[k]);
        state.label_class_step(map, layer, i, |c| c.zones_inside[k] = rank);
    }
    super::raster::heading(ui, OUTSIDE);
    if let Some(k) = zone_grid(ui, controls, false, &class.zones_outside) {
        let rank = next_rank(class.zones_outside[k]);
        state.label_class_step(map, layer, i, |c| c.zones_outside[k] = rank);
    }
}

/// The leader line of a label outside its polygon: where it starts, and how
/// it looks.
fn leader_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    use vale_style::{LeaderAnchor, LeaderEnd};
    let style = class.leader;
    // A change that is one undo step.
    let step = |state: &mut AppState, f: &dyn Fn(&mut vale_style::LeaderStyle)| {
        state.label_class_step(map, layer, i, |c| f(&mut c.leader));
    };
    // A change of a number, which joins the undo step of the drag or the keypad.
    let edit = |state: &mut AppState, f: &dyn Fn(&mut vale_style::LeaderStyle)| {
        state.edit_label_class(map, layer, i, |c| f(&mut c.leader));
    };
    super::raster::heading(ui, LEADER_START);
    for anchor in LeaderAnchor::ALL {
        let name = anchor.name();
        let on = anchor == class.anchor;
        if widgets::option_row(ui, controls, name, name, &[], on).clicked() {
            state.label_class_step(map, layer, i, |c| c.anchor = anchor);
        }
    }
    ui.add_space(6.0);
    let mut show = style.show;
    if widgets::switch_row(ui, controls, SHOW_LINE, SHOW_LINE, &mut show) {
        step(state, &|l| l.show = show);
    }
    if !style.show {
        return;
    }
    let mut matched = style.color.is_none();
    if widgets::switch_row(ui, controls, MATCH_TEXT, MATCH_TEXT, &mut matched) {
        let color = (!matched).then_some(class.color);
        step(state, &|l| l.color = color);
    }
    if let Some(color) = style.color {
        let which = (COLOR, ClassColor::LeaderLine);
        if let Some(c) = color_rows(ui, state, controls, which, color) {
            edit(state, &|l| l.color = Some(c));
        }
    }
    let width = Number::new(LINE_WIDTH, style.width, LINE_WIDTHS.0..=LINE_WIDTHS.1)
        .step(0.25)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(width) = width {
        edit(state, &|l| l.width = width);
    }
    let mut dashed = style.dashed;
    if widgets::switch_row(ui, controls, DASHED, DASHED, &mut dashed) {
        step(state, &|l| l.dashed = dashed);
    }
    if style.dashed {
        let dash = Number::new(DASH, style.dash, DASHES.0..=DASHES.1)
            .step(0.5)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(dash) = dash {
            edit(state, &|l| l.dash = dash);
        }
        let gap = Number::new(DASH_GAP, style.dash_gap, DASHES.0..=DASHES.1)
            .step(0.5)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(gap) = gap {
            edit(state, &|l| l.dash_gap = gap);
        }
    }
    let mut halo = style.halo;
    if widgets::switch_row(ui, controls, HALO, HALO, &mut halo) {
        step(state, &|l| l.halo = halo);
    }
    if style.halo {
        let mut matched = style.halo_color.is_none();
        if widgets::switch_row(ui, controls, MATCH_HALO, MATCH_HALO, &mut matched) {
            let color = (!matched).then_some(class.halo_color);
            step(state, &|l| l.halo_color = color);
        }
        if let Some(color) = style.halo_color {
            let which = (HALO_COLOR, ClassColor::LeaderHalo);
            if let Some(c) = color_rows(ui, state, controls, which, color) {
                edit(state, &|l| l.halo_color = Some(c));
            }
        }
        let (lo, hi) = LINE_HALO_WIDTHS;
        let width = Number::new(HALO_WIDTH, style.halo_width, lo..=hi)
            .step(0.25)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(width) = width {
            edit(state, &|l| l.halo_width = width);
        }
    }
    super::raster::heading(ui, LEADER_END);
    for end in LeaderEnd::ALL {
        let name = end.name();
        if widgets::option_row(ui, controls, name, name, &[], end == style.end).clicked() {
            step(state, &|l| l.end = end);
        }
    }
    if style.end != LeaderEnd::None {
        let size = Number::new(END_SIZE, style.end_size, END_SIZES.0..=END_SIZES.1)
            .step(0.5)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(size) = size {
            edit(state, &|l| l.end_size = size);
        }
    }
    ui.add_space(6.0);
    let (lo, hi) = LEADER_GAPS;
    let gap = Number::new(LABEL_GAP, style.label_gap, lo..=hi)
        .step(0.5)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(gap) = gap {
        edit(state, &|l| l.label_gap = gap);
    }
    let gap = Number::new(ANCHOR_GAP, style.anchor_gap, lo..=hi)
        .step(0.5)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(gap) = gap {
        edit(state, &|l| l.anchor_gap = gap);
    }
}

/// The curve limit of a class, in degrees.
fn curve_limit(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (map, layer, i): (MapId, LayerId, usize),
    class: &LabelClass,
) {
    let (lo, hi) = CURVE_LIMITS;
    let degrees = Number::new(CURVE_LIMIT, class.curve_limit.to_degrees(), lo..=hi)
        .step(1.0)
        .unit("°")
        .stepper(ui, controls);
    if let Some(degrees) = degrees {
        let limit = degrees.to_radians();
        state.edit_label_class(map, layer, i, |c| c.curve_limit = limit);
    }
}

/// The Conflicts segment of the panel of a class: the buffer and the
/// distance between labels with the same text.
fn conflicts_segment(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    let buffer = Number::new(BUFFER, class.buffer, BUFFERS.0..=BUFFERS.1)
        .step(0.5)
        .unit("pt")
        .stepper(ui, controls);
    if let Some(buffer) = buffer {
        state.edit_label_class(map, layer, i, |c| c.buffer = buffer);
    }
    let mut remove = class.remove_duplicates;
    if widgets::switch_row(
        ui,
        controls,
        REMOVE_DUPLICATES,
        REMOVE_DUPLICATES,
        &mut remove,
    ) {
        state.label_class_step(map, layer, i, |c| c.remove_duplicates = remove);
    }
    if class.remove_duplicates {
        let (lo, hi) = DUPLICATE_DISTANCES;
        let distance = Number::new(DUPLICATE_DISTANCE, class.duplicate_distance, lo..=hi)
            .step(10.0)
            .unit("pt")
            .stepper(ui, controls);
        if let Some(distance) = distance {
            state.edit_label_class(map, layer, i, |c| c.duplicate_distance = distance);
        }
    }
}

/// The buttons at the bottom of each segment of the panel of a class.
fn class_buttons(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
) {
    ui.add_space(6.0);
    let list = state.label_class_list(map);
    let count = list.len();
    let at = list.iter().position(|(l, j, _)| (*l, *j) == (layer, i));
    if let Some(at) = at {
        if widgets::button_row(ui, controls, MOVE_UP, at > 0).clicked() {
            state.reorder_label_classes(map, at, at - 1);
        }
        if widgets::button_row(ui, controls, MOVE_DOWN, at + 1 < count).clicked() {
            state.reorder_label_classes(map, at, at + 1);
        }
    }
    let delete = widgets::button_row_in(ui, controls, DELETE_CLASS, true, theme::WARN);
    if delete.clicked() {
        state.delete_label_class(map, layer, i);
        state.atlas.label = None;
    }
}

/// The faces of the font family of the class. A tap sets the weight and the
/// italic of the class.
fn style_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    if let Some(f) = text_style::face_rows(ui, state, controls, class.font.as_deref(), face(class))
    {
        state.label_class_step(map, layer, i, |c| {
            c.weight = f.weight;
            c.italic = f.italic;
        });
        state.atlas.label_page = None;
    }
}

/// The cases of the text.
fn case_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    if let Some(case) = text_style::case_rows(ui, controls, class.case) {
        state.label_class_step(map, layer, i, |c| c.case = case);
        state.atlas.label_page = None;
    }
}

/// The point layers and the line layers. A tap moves the class to the
/// layer.
fn layer_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (id, map, layer, i): ClassAt,
) {
    for (to, kind) in state.label_layers() {
        let name = layer_name(state, to);
        let lines = [kind_name(kind).to_owned()];
        let row = ui.push_id(("class-layer", to.0), |ui| {
            widgets::option_row(ui, controls, &name, &name, &lines, to == layer)
        });
        if row.inner.clicked() {
            if let Some(j) = state.move_label_class(map, layer, i, to) {
                state.atlas.label = Some((id, to, j));
            }
            state.atlas.label_page = None;
        }
    }
}

/// The placements of the feature type of the layer of the class.
fn placement_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    kind: GeometryKind,
    class: &LabelClass,
) {
    for &placement in LabelPlacement::choices(kind) {
        let name = placement.name();
        let on = placement == class.placement;
        if widgets::option_row(ui, controls, name, name, &[], on).clicked() {
            state.label_class_step(map, layer, i, |c| c.placement = placement);
            state.atlas.label_page = None;
        }
    }
}

/// The size of a cell of the diagram of the positions.
const CELL: f32 = 30.0;

/// The column and the row of a position in the diagram of the positions.
fn cell(p: LabelPosition) -> (f32, f32) {
    match p {
        LabelPosition::TopLeft => (0.0, 0.0),
        LabelPosition::Top => (1.0, 0.0),
        LabelPosition::TopRight => (2.0, 0.0),
        LabelPosition::Left => (0.0, 1.0),
        LabelPosition::Right => (2.0, 1.0),
        LabelPosition::BottomLeft => (0.0, 2.0),
        LabelPosition::Bottom => (1.0, 2.0),
        LabelPosition::BottomRight => (2.0, 2.0),
    }
}

/// The rank of each position among the positions that are on, from 1. A
/// position that is off has no rank.
fn ranks(positions: &[vale_style::PositionRank]) -> Vec<Option<usize>> {
    let mut n = 0;
    (positions.iter())
        .map(|r| {
            r.on.then(|| {
                n += 1;
                n
            })
        })
        .collect()
}

/// A diagram of the positions around a point, with the rank of each
/// position that is on.
fn diagram(ui: &mut egui::Ui, positions: &[vale_style::PositionRank]) {
    let size = vec2(ui.available_width(), 3.0 * CELL + 16.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let grid = Rect::from_center_size(rect.center(), vec2(3.0 * CELL, 3.0 * CELL));
    let painter = ui.painter();
    let at = |(x, y): (f32, f32)| {
        let min = grid.min + vec2(x * CELL, y * CELL);
        Rect::from_min_size(min, vec2(CELL, CELL)).shrink(2.0)
    };
    painter.circle_filled(at((1.0, 1.0)).center(), 4.0, theme::TEXT);
    for (r, rank) in positions.iter().zip(ranks(positions)) {
        let square = at(cell(r.position));
        let (fill, text, color) = match rank {
            Some(n) => (theme::raise(), n.to_string(), theme::TEXT),
            None => (theme::track(), "–".to_owned(), theme::MUTE),
        };
        painter.rect_filled(square, 4.0, fill);
        painter.text(
            square.center(),
            Align2::CENTER_CENTER,
            text,
            theme::small(),
            color,
        );
    }
}

/// The ranked positions of a point class, best first, under a diagram of
/// the ranks. A tap on a row turns its position on or off, and a drag on the
/// handle of a row moves the position in the rank.
fn positions_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    let list = &class.positions;
    diagram(ui, list);
    let rank = ranks(list);
    let size = vec2(ui.available_width(), list.len() as f32 * PANEL_ROW);
    let (area, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let slot =
        |top: f32| Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), PANEL_ROW));
    let place = |k: usize| slot(area.top() + k as f32 * PANEL_ROW);
    let (pointer, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_down()));

    let Some(drag) = state.atlas.position_drag.filter(|d| d.from < list.len()) else {
        state.atlas.position_drag = None;
        for (k, r) in list.iter().enumerate() {
            let rect = place(k);
            let row = (r.position, r.on, rank[k]);
            let Some((body, handle)) = position_row(ui, Some(&mut *controls), rect, row, false)
            else {
                continue;
            };
            if body.clicked() {
                state.label_class_step(map, layer, i, |c| {
                    if let Some(r) = c.positions.get_mut(k) {
                        r.on = !r.on;
                    }
                });
            }
            if handle.drag_started() || handle.dragged() {
                let y = pointer.map_or(rect.center().y, |p| p.y);
                state.atlas.position_drag = Some(ClassDrag {
                    from: k,
                    grab: y - rect.top(),
                });
                ui.ctx().request_repaint();
            }
        }
        return;
    };

    // The lifted row follows the finger, and the other rows make room at
    // the place where it goes.
    let from = drag.from;
    let y = pointer.map_or(place(from).top() + drag.grab, |p| p.y);
    let top = (y - drag.grab).clamp(area.top(), area.bottom() - PANEL_ROW);
    let to = ((top - area.top()) / PANEL_ROW).round() as usize;
    let others = list.iter().enumerate().filter(|(k, _)| *k != from);
    for (n, (k, r)) in others.enumerate() {
        let rect = place(if n < to { n } else { n + 1 });
        position_row(ui, None, rect, (r.position, r.on, rank[k]), false);
    }
    let r = list[from];
    position_row(ui, None, slot(top), (r.position, r.on, rank[from]), true);
    if down {
        ui.ctx().request_repaint();
    } else {
        state.atlas.position_drag = None;
        if to != from {
            state.label_class_step(map, layer, i, |c| {
                if from < c.positions.len() && to < c.positions.len() {
                    let moved = c.positions.remove(from);
                    c.positions.insert(to, moved);
                }
            });
        }
    }
}

/// One row of the Positions page in `rect`: the rank, the name of the
/// position, its switch, and the handle at the right. With `controls`, the
/// row takes a tap, its handle takes a drag, and the function returns both.
/// A `lifted` row shows above the others.
fn position_row(
    ui: &egui::Ui,
    controls: Option<&mut Controls>,
    rect: Rect,
    (position, on, rank): (LabelPosition, bool, Option<usize>),
    lifted: bool,
) -> Option<(egui::Response, egui::Response)> {
    let name = position.name();
    let handle = Rect::from_center_size(
        pos2(rect.right() - TOUCH / 2.0 + 8.0, rect.center().y),
        vec2(TOUCH, TOUCH),
    );
    let body = Rect::from_min_max(rect.min, pos2(handle.left(), rect.bottom()));
    let out = controls.map(|controls| {
        let body = widgets::tap_area(ui, controls, body, name, true);
        let grip = format!("{REORDER} {name}");
        (body, widgets::drag_area(ui, controls, handle, &grip))
    });
    let highlight = rect.expand2(vec2(8.0, -widgets::HIGHLIGHT_INSET));
    let painter = ui.painter();
    if lifted {
        let shadow = highlight.translate(vec2(0.0, 2.0)).expand(2.0);
        painter.rect_filled(shadow, 7.0, egui::Color32::from_black_alpha(60));
        painter.rect_filled(highlight, 5.0, theme::card(false).to_opaque());
        painter.rect_filled(highlight, 5.0, theme::raise());
    }
    let y = rect.center().y;
    if let Some(n) = rank {
        let at = pos2(rect.left(), y);
        painter.text(at, Align2::LEFT_CENTER, n, theme::mono(), theme::MUTE);
    }
    let at = pos2(rect.left() + 28.0, y);
    painter.text(at, Align2::LEFT_CENTER, name, theme::body(), theme::TEXT);
    widgets::paint_switch(ui, pos2(handle.left() - 4.0, y), on);
    let color = if lifted { theme::TEXT } else { theme::MUTE };
    icons::paint(painter, handle, Icon::Grip, widgets::ICON, color);
    out
}

/// Closes the color picker of the label class when it did not show in this
/// frame, with the keypad of its field.
pub(super) fn color_frame(ctx: &egui::Context, state: &mut AppState) {
    let shown = std::mem::take(&mut state.atlas.label_color_shown);
    if !shown && state.atlas.label_color.take().is_some() {
        widgets::close_keypad(ctx);
    }
}
