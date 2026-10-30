//! The Labels tab of the panel of a frame in the Atlas workspace: the label
//! classes of the map of the frame, and the panel of one class with its
//! pages.

use eframe::egui::{self, Rect};
use vale_store::{FrameId, GeometryKind, LayerId, MapId};
use vale_style::expr::{Expr, Filter};
use vale_style::{LabelClass, LabelPlacement};

use super::atlas::{FRAME_TABS, keypad_below, panel_width};
use super::geometry::Rects;
use super::widgets::{self, Controls, Keys};
use super::{Card, CardTitle, note_text, show_card, theme};
use crate::fonts::{self, DEFAULT_FAMILY};
use crate::ui::atlas::{AtlasSelection, ClassPage};
use crate::ui::{Action, AppState, FontTarget, Note};

pub const ADD_CLASS: &str = "Add class";
pub const DELETE_CLASS: &str = "Delete class";
pub const NO_LABEL_LAYERS: &str = "This map has no point or line layers.";
pub const NO_CLASSES: &str = "No layer of this map has labels.";
pub const CLASS_NAME: &str = "Class name";
pub const LAYER: &str = "Layer";
pub const TEXT: &str = "Text";
pub const FILTER: &str = "Filter";
/// The name of the size slider. Its label is "Size".
pub const LABEL_SIZE: &str = "Label size";
pub const COLOR: &str = "Color";
pub const HALO: &str = "Halo";
pub const ITALIC: &str = "Italic";
pub const PRIORITY: &str = "Priority";
pub const PLACEMENT: &str = "Placement";
pub const BAD_TEXT: &str = "Not a valid expression.";
/// The smallest and the largest size of a label, in points.
pub const SIZES: (f64, f64) = (4.0, 72.0);

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
        let kind = state.doc.layer(layer).and_then(|l| l.geometry());
        let fits = matches!(kind, Some(GeometryKind::Point | GeometryKind::Line));
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
    state.atlas.label_color = false;
}

/// The label classes of the map of frame `id`, by layer, and the button
/// that adds a class. A tap on a class opens its panel.
pub(super) fn tab(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, id: FrameId) {
    let Some(map) = state.doc.project.atlas.frame(id).map(|f| f.map) else {
        return;
    };
    ui.spacing_mut().item_spacing.y = 0.0;
    let layers = state.label_layers();
    if layers.is_empty() {
        hint(ui, NO_LABEL_LAYERS);
        return;
    }
    let mut open = None;
    let mut any = false;
    for &(layer, _) in &layers {
        let name = layer_name(state, layer);
        for (i, class) in state.doc.label_classes(Some(map), layer).iter().enumerate() {
            any = true;
            let tapped = ui
                .push_id(("class", layer.0, i), |ui| {
                    widgets::open_row(ui, controls, &class.name, &name).clicked()
                })
                .inner;
            if tapped {
                open = Some((layer, i));
            }
        }
    }
    if !any {
        hint(ui, NO_CLASSES);
    }
    ui.add_space(6.0);
    if widgets::button_row(ui, controls, ADD_CLASS, true).clicked() {
        open = state.add_label_class(map);
    }
    if let Some((layer, i)) = open {
        show_class(state, id, layer, i);
    }
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
        scroll: egui::scroll_area::ScrollSource::default(),
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
            state.atlas.vertex = None;
        }
    };
    let top = |_: &mut egui::Ui, _: &mut AppState, _: &mut Controls| {};
    let body = |ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let at = (id, map, layer, i);
        match page {
            Some(ClassPage::Layer) => layer_page(ui, state, controls, at),
            Some(ClassPage::Placement) => placement_page(ui, state, controls, at, kind, &class),
            None => class_body(ui, state, controls, at, &class),
        }
    };
    Some(show_card(
        ctx, state, controls, rects, card, on_title, top, body,
    ))
}

/// The place of a label class: the frame of the panel, the map, the layer,
/// and the place of the class in the classes of the layer.
type ClassAt = (FrameId, MapId, LayerId, usize);

/// True if a text is a label expression. Empty text gives no label.
fn text_ok(text: &str) -> bool {
    text.trim().is_empty() || Expr::parse(text).is_ok()
}

fn filter_ok(text: &str) -> bool {
    Filter::new(text).error().is_none()
}

/// A field of an expression of a class. While the field has the focus, it
/// keeps the typed text, and each valid text goes to the class at once.
/// Returns the valid text that the user typed.
fn expression_field(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    typed: &mut String,
    current: &str,
    ok: fn(&str) -> bool,
    bad: &str,
) -> Option<String> {
    let focused = ui.memory(|m| m.has_focus(ui.id().with(name)));
    if !focused {
        *typed = current.to_owned();
    }
    let field = widgets::code_field(ui, controls, name, name, typed, ok(typed));
    if !ok(typed) {
        widgets::form_note(ui, bad, 20.0);
    }
    (field.changed() && ok(typed)).then(|| typed.clone())
}

fn class_body(
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

    let mut typed = std::mem::take(&mut state.atlas.label_text);
    let text = expression_field(
        ui,
        controls,
        TEXT,
        &mut typed,
        &class.text,
        text_ok,
        BAD_TEXT,
    );
    state.atlas.label_text = typed;
    if let Some(text) = text {
        state.edit_label_class(map, layer, i, |c| c.text = text);
    }
    let mut typed = std::mem::take(&mut state.atlas.label_filter);
    let bad = super::rules::BAD_FILTER;
    let filter = expression_field(
        ui,
        controls,
        FILTER,
        &mut typed,
        &class.filter,
        filter_ok,
        bad,
    );
    state.atlas.label_filter = typed;
    if let Some(filter) = filter {
        state.edit_label_class(map, layer, i, |c| c.filter = filter);
    }

    let family = class.font.as_deref().unwrap_or(DEFAULT_FAMILY);
    if widgets::open_row(ui, controls, super::fonts::FONT, family).clicked() {
        let target = FontTarget::Class(map, layer, i);
        state.actions.push(Action::PickFont(target));
    }
    let warning = fonts::missing(class.font.as_deref(), &state.font_families);
    let mut warning = warning.map(|t| Note::new(t, true, None));
    note_text(ui, None, &mut warning, None);

    let (lo, hi) = SIZES;
    let mut fraction = ((class.size - lo) / (hi - lo)).clamp(0.0, 1.0);
    let text = format!("{} pt", (class.size * 10.0).round() / 10.0);
    if widgets::row_slider(ui, controls, LABEL_SIZE, "Size", &mut fraction, &text) {
        let size = (lo + fraction * (hi - lo)).round();
        state.edit_label_class(map, layer, i, |c| c.size = size);
    }

    let c = class.color;
    let open = state.atlas.label_color;
    if widgets::color_row(ui, controls, COLOR, [c.r, c.g, c.b], open).clicked() {
        state.atlas.label_color = !open;
        if open {
            widgets::close_keypad(ui.ctx());
        }
    }
    if state.atlas.label_color {
        ui.add_space(4.0);
        let mut rgb = [c.r, c.g, c.b];
        if widgets::color_picker(ui, controls, COLOR, &mut rgb) {
            let [r, g, b] = rgb;
            state.edit_label_class(map, layer, i, |class| {
                class.color = vale_style::Color { r, g, b, a: c.a };
            });
        }
        state.atlas.label_color_shown = true;
    }

    let mut halo = class.halo;
    if widgets::switch_row(ui, controls, HALO, HALO, &mut halo) {
        state.label_class_step(map, layer, i, |c| c.halo = halo);
    }
    let mut italic = class.italic;
    if widgets::switch_row(ui, controls, ITALIC, ITALIC, &mut italic) {
        state.label_class_step(map, layer, i, |c| c.italic = italic);
    }

    let shown = class.priority.to_string();
    let parse = |t: &str| t.trim().parse::<i32>().ok();
    let typed = widgets::key_field(
        ui,
        controls,
        PRIORITY,
        PRIORITY,
        "",
        &shown,
        Keys::Number,
        |t| parse(t).is_some(),
    );
    if let Some(priority) = typed.as_deref().and_then(parse) {
        state.edit_label_class(map, layer, i, |c| c.priority = priority);
    }
    keypad_below(ui, controls, PRIORITY);

    if widgets::open_row(ui, controls, PLACEMENT, class.placement.name()).clicked() {
        state.atlas.label_page = Some(ClassPage::Placement);
    }

    ui.add_space(6.0);
    let delete = widgets::button_row_in(ui, controls, DELETE_CLASS, true, theme::WARN);
    if delete.clicked() {
        state.delete_label_class(map, layer, i);
        state.atlas.label = None;
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

/// Closes the color picker of the label class when it did not show in this
/// frame, with the keypad of its field.
pub(super) fn color_frame(ctx: &egui::Context, state: &mut AppState) {
    let shown = std::mem::take(&mut state.atlas.label_color_shown);
    if !shown && std::mem::take(&mut state.atlas.label_color) {
        widgets::close_keypad(ctx);
    }
}
