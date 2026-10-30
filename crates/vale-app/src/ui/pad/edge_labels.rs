//! The Edge labels page of the Frame tab: the labels of the graticule at the
//! edge of a frame, with one set for the meridians and one for the parallels.

use eframe::egui::{self, Rect};
use vale_labeler::Face;
use vale_store::{CoordFormat, EdgeLabels, EdgeOrientation, FrameId, Graticule, Rgb};

use super::atlas::{FRAME_TABS, LABEL_FORMAT, LABEL_FORMATS, named_tabs, panel_width};
use super::geometry::Rects;
use super::labels::{color_rows, faces_of, percent, points, value_slider};
use super::raster::heading;
use super::widgets::{self, Controls};
use super::{Card, CardTitle, show_card};
use crate::fonts::DEFAULT_FAMILY;
use crate::ui::atlas::{self, AtlasSelection, ClassColor, EdgeSet};
use crate::ui::{Action, AppState, FontTarget};

pub const EDGE_LABELS: &str = "Edge labels";
pub const STYLES_TITLE: &str = "Style";
/// The segments of the page, as (name, label).
pub const SETS: [(&str, &str); 2] = [
    ("Meridians segment", "Meridians"),
    ("Parallels segment", "Parallels"),
];
pub const SHOW_LABELS: &str = "Show labels";
pub const TEXT: &str = "Text";
pub const FONT: &str = "Font";
pub const STYLE: &str = "Style";
pub const SIZE: &str = "Size";
pub const COLOR: &str = "Color";
pub const OPACITY: &str = "Opacity";
pub const HALO: &str = "Halo";
pub const HALO_COLOR: &str = "Halo color";
pub const HALO_WIDTH: &str = "Halo width";
pub const PLACEMENT: &str = "Placement";
pub const POSITION: &str = "Position";
pub const PLACES: [&str; 2] = ["Inside", "Outside"];
pub const ORIENTATION: &str = "Orientation";
/// The choices of the orientation, in the order of `EdgeOrientation::ALL`.
pub const ORIENTATIONS: [&str; 2] = ["Along line", "Horizontal"];
pub const OFFSET: &str = "Offset";
pub const EDGES: &str = "Edges";
/// The switches of the sides, as (name, label).
pub const SIDES: [(&str, &str); 4] = [
    ("Top edge", "Top"),
    ("Bottom edge", "Bottom"),
    ("Left edge", "Left"),
    ("Right edge", "Right"),
];

/// The value of the Edge labels row of the Frame tab.
pub fn summary(g: &Graticule) -> &'static str {
    match (g.meridians.on, g.parallels.on) {
        (true, true) => "On",
        (true, false) => "Meridians",
        (false, true) => "Parallels",
        (false, false) => "Off",
    }
}

/// The Edge labels page of a frame, in place of the Frame tab.
pub fn panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    id: FrameId,
) -> Option<Rect> {
    state.doc.project.atlas.frame(id)?;
    let styles = state.atlas.edge_styles;
    let (key, title, back) = if styles {
        ("pad-atlas-edge-styles", STYLES_TITLE, EDGE_LABELS)
    } else {
        ("pad-atlas-edge-labels", EDGE_LABELS, FRAME_TABS[0])
    };
    let card = Card {
        key,
        place: (rects.right_panel, panel_width(ctx)),
        head: CardTitle {
            title,
            back: Some(back),
            action: None,
        },
        scroll: egui::scroll_area::ScrollSource::default(),
    };
    let on_title = |state: &mut AppState, row: &widgets::TitleRow| {
        if row.back {
            if state.atlas.edge_styles {
                state.atlas.edge_styles = false;
            } else {
                state.atlas.edge = None;
            }
            state.atlas.label_color = None;
        }
        if row.close {
            state.atlas.selection = AtlasSelection::None;
            state.atlas.vertex = None;
        }
    };
    let top = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        if state.atlas.edge_styles {
            return;
        }
        // The format is for both sets.
        let format = state
            .doc
            .project
            .atlas
            .frame(id)
            .map(|f| f.graticule.format);
        let active = CoordFormat::ALL.iter().position(|f| Some(*f) == format);
        if let Some(i) = widgets::choice_row(ui, controls, LABEL_FORMAT, &LABEL_FORMATS, active) {
            atlas::set_graticule_format(state, id, CoordFormat::ALL[i]);
        }
        ui.add_space(4.0);
        let items = SETS.map(|(name, label)| (name, label, true));
        let active = state.atlas.edge_set as usize;
        if let Some(chosen) = named_tabs(ui, controls, &items, active) {
            state.atlas.edge_set = EdgeSet::ALL[chosen];
            state.atlas.label_color = None;
            widgets::close_keypad(ui.ctx());
        }
        ui.add_space(6.0);
    };
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        if state.atlas.edge_styles {
            styles_body(ui, state, controls, id);
        } else {
            body(ui, state, controls, id);
        }
    };
    Some(show_card(
        ctx, state, controls, rects, card, on_title, top, body,
    ))
}

/// The rows of the open set.
fn body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: FrameId) {
    let Some(frame) = state.doc.project.atlas.frame(id) else {
        return;
    };
    let which = state.atlas.edge_set;
    let set = which.of(&frame.graticule).clone();
    let edit = |state: &mut AppState, change: &dyn Fn(&mut EdgeLabels)| {
        atlas::edit_edge_labels(state, id, which, change);
    };

    let mut on = set.on;
    if widgets::switch_row(ui, controls, SHOW_LABELS, SHOW_LABELS, &mut on) {
        edit(state, &|e| e.on = on);
    }
    if !set.on {
        return;
    }

    heading(ui, TEXT);
    let family = set.font.as_deref().unwrap_or(DEFAULT_FAMILY);
    if widgets::open_row(ui, controls, FONT, family).clicked() {
        state
            .actions
            .push(Action::PickFont(FontTarget::Edge(id, which)));
    }
    let face = Face {
        weight: set.weight,
        italic: set.italic,
    };
    if widgets::open_row(ui, controls, STYLE, &face.name()).clicked() {
        state.atlas.edge_styles = true;
        state.atlas.label_color = None;
    }
    let sizes = (*EdgeLabels::SIZES.start(), *EdgeLabels::SIZES.end());
    let text = format!("{} pt", (set.size * 2.0).round() / 2.0);
    if let Some(size) = value_slider(ui, controls, (SIZE, SIZE), set.size, sizes, &text) {
        let size = (size * 2.0).round() / 2.0;
        edit(state, &|e| e.size = size);
    }
    let color = rgba(set.color);
    if let Some(c) = color_rows(ui, state, controls, (COLOR, ClassColor::EdgeText), color) {
        edit(state, &|e| e.color = [c.r, c.g, c.b]);
    }
    let text = percent(set.opacity);
    let opacity = value_slider(
        ui,
        controls,
        (OPACITY, OPACITY),
        set.opacity,
        (0.0, 1.0),
        &text,
    );
    if let Some(opacity) = opacity {
        let opacity = (opacity * 100.0).round() / 100.0;
        edit(state, &|e| e.opacity = opacity);
    }
    let mut halo = set.halo;
    if widgets::switch_row(ui, controls, HALO, HALO, &mut halo) {
        edit(state, &|e| e.halo = halo);
    }
    if set.halo {
        let which = (HALO_COLOR, ClassColor::EdgeHalo);
        if let Some(c) = color_rows(ui, state, controls, which, rgba(set.halo_color)) {
            edit(state, &|e| e.halo_color = [c.r, c.g, c.b]);
        }
        let range = (
            *EdgeLabels::HALO_WIDTHS.start(),
            *EdgeLabels::HALO_WIDTHS.end(),
        );
        let text = points(set.halo_width);
        let names = (HALO_WIDTH, HALO_WIDTH);
        if let Some(width) = value_slider(ui, controls, names, set.halo_width, range, &text) {
            let width = (width * 10.0).round() / 10.0;
            edit(state, &|e| e.halo_width = width);
        }
    }

    heading(ui, PLACEMENT);
    let active = Some(usize::from(set.outside));
    if let Some(i) = widgets::choice_row(ui, controls, POSITION, &PLACES, active) {
        edit(state, &|e| e.outside = i == 1);
    }
    let active = EdgeOrientation::ALL
        .iter()
        .position(|o| *o == set.orientation);
    if let Some(i) = widgets::choice_row(ui, controls, ORIENTATION, &ORIENTATIONS, active) {
        edit(state, &|e| e.orientation = EdgeOrientation::ALL[i]);
    }
    let range = (*EdgeLabels::OFFSETS.start(), *EdgeLabels::OFFSETS.end());
    let text = points(set.offset);
    if let Some(offset) = value_slider(ui, controls, (OFFSET, OFFSET), set.offset, range, &text) {
        let offset = (offset * 2.0).round() / 2.0;
        edit(state, &|e| e.offset = offset);
    }

    heading(ui, EDGES);
    let edges = set.edges;
    let values = [edges.top, edges.bottom, edges.left, edges.right];
    for (i, ((name, label), value)) in SIDES.into_iter().zip(values).enumerate() {
        let mut on = value;
        if widgets::switch_row(ui, controls, name, label, &mut on) {
            edit(state, &|e| {
                let side = match i {
                    0 => &mut e.edges.top,
                    1 => &mut e.edges.bottom,
                    2 => &mut e.edges.left,
                    _ => &mut e.edges.right,
                };
                *side = on;
            });
        }
    }
}

/// The faces of the font of the open set. A tap sets the face and goes back.
fn styles_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: FrameId) {
    let Some(frame) = state.doc.project.atlas.frame(id) else {
        return;
    };
    let which = state.atlas.edge_set;
    let set = which.of(&frame.graticule).clone();
    for f in faces_of(state, set.font.as_deref()) {
        let name = f.name();
        let on = f.weight == set.weight && f.italic == set.italic;
        if widgets::option_row(ui, controls, &name, &name, &[], on).clicked() {
            atlas::edit_edge_labels(state, id, which, |e| {
                e.weight = f.weight;
                e.italic = f.italic;
            });
            state.atlas.edge_styles = false;
        }
    }
}

fn rgba([r, g, b]: Rgb) -> vale_style::Color {
    vale_style::Color { r, g, b, a: 255 }
}
