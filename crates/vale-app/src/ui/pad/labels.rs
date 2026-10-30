//! The Labels tab of the panel of a frame in the Atlas workspace: the label
//! classes of the map of the frame, and the panel of one class with its
//! pages.

use eframe::egui::{self, Align2, Rect, pos2, vec2};
use vale_labeler::Face;
use vale_store::{FrameId, GeometryKind, LayerId, MapId};
use vale_style::expr::{Expr, Filter};
use vale_style::{LabelClass, LabelPlacement, TextCase};

use super::atlas::{FRAME_TABS, panel_width};
use super::filter;
use super::geometry::{Rects, TOUCH};
use super::icons::{self, Icon};
use super::widgets::{self, Controls, PANEL_ROW};
use super::{Card, CardTitle, note_text, show_card, theme};
use crate::fonts::{self, DEFAULT_FAMILY};
use crate::ui::atlas::{AtlasSelection, ClassColor, ClassDrag, ClassPage};
use crate::ui::{Action, AppState, FontTarget, Note};

pub const ADD_CLASS: &str = "Add class";
pub const DELETE_CLASS: &str = "Delete class";
pub const NO_LABEL_LAYERS: &str = "This map has no point or line layers.";
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
pub const OPACITY: &str = "Opacity";
pub const HALO: &str = "Halo";
pub const HALO_COLOR: &str = "Halo color";
pub const HALO_WIDTH: &str = "Halo width";
pub const PLACEMENT: &str = "Placement";
pub const MOVE_UP: &str = "Move up";
pub const MOVE_DOWN: &str = "Move down";
/// The start of the name of the handle of a row of the class list. The
/// name of the class follows.
pub const REORDER: &str = "Reorder";
pub const BAD_TEXT: &str = "Not a valid expression.";
/// The smallest and the largest size of a label, in points.
pub const SIZES: (f64, f64) = (4.0, 72.0);
/// The smallest and the largest letter spacing, as fractions of the size.
pub const SPACINGS: (f64, f64) = (-0.1, 1.0);
/// The thinnest and the widest halo, in points.
pub const HALO_WIDTHS: (f64, f64) = (0.5, 8.0);

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
    state.atlas.label_color = None;
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
    let list: Vec<(LayerId, usize, String, String)> = (state.label_class_list(map).into_iter())
        .map(|(layer, i, c)| (layer, i, c.name, layer_name(state, layer)))
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
/// layer, the name of the class, and the name of the layer. Returns the
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
        Some(ClassPage::Condition) => (
            "pad-atlas-class-condition",
            filter::ADD_CONDITION,
            &*class.name,
        ),
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
            Some(ClassPage::Style) => style_page(ui, state, controls, at, &class),
            Some(ClassPage::Case) => case_page(ui, state, controls, at, &class),
            Some(ClassPage::Condition) => condition_page(ui, state, controls, at, &class),
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

/// A slider row of a class for a value from `lo` to `hi`. Returns the new
/// value when the user moves it.
fn value_slider(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    (name, label): (&str, &str),
    value: f64,
    (lo, hi): (f64, f64),
    text: &str,
) -> Option<f64> {
    let mut fraction = ((value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let moved = widgets::row_slider(ui, controls, name, label, &mut fraction, text);
    moved.then_some(lo + fraction * (hi - lo))
}

/// A percentage for a slider row, for example "5 %".
fn percent(fraction: f64) -> String {
    format!("{:.0} %", fraction * 100.0)
}

/// A color row of a class and its color picker while it is open. The picker
/// keeps the alpha of the color. Returns the new color.
fn color_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (name, which): (&str, ClassColor),
    c: vale_style::Color,
) -> Option<vale_style::Color> {
    let open = state.atlas.label_color == Some(which);
    if widgets::color_row(ui, controls, name, [c.r, c.g, c.b], open).clicked() {
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
    let mut rgb = [c.r, c.g, c.b];
    widgets::color_picker(ui, controls, name, &mut rgb).then(|| {
        let [r, g, b] = rgb;
        vale_style::Color { r, g, b, a: c.a }
    })
}

/// The face of a class.
fn face(class: &LabelClass) -> Face {
    Face {
        weight: class.weight,
        italic: class.italic,
    }
}

/// The faces of the font family of a class. A family with no list of faces
/// gives the regular face.
fn faces(state: &mut AppState, class: &LabelClass) -> Vec<Face> {
    let family = fonts::family(class.font.as_deref(), &mut state.fonts).to_owned();
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

    ui.add_space(6.0);
    let mut typed = std::mem::take(&mut state.atlas.label_filter);
    let none = filter::NO_CLASS_CONDITION;
    let (changed, add) = filter::list(ui, controls, &class.filter, &mut typed, none);
    let typing = ui.memory(|m| m.has_focus(ui.id().with(super::rules::EXPRESSION)));
    state.atlas.label_filter = typed;
    if let Some(text) = changed.filter(|t| filter_ok(t)) {
        if typing {
            state.edit_label_class(map, layer, i, |c| c.filter = text);
        } else {
            state.label_class_step(map, layer, i, |c| c.filter = text);
        }
    }
    if add {
        state.atlas.label_draft = filter::Draft::new((layer, i));
        state.atlas.label_page = Some(ClassPage::Condition);
    }
    ui.add_space(6.0);

    let family = class.font.as_deref().unwrap_or(DEFAULT_FAMILY);
    if widgets::open_row(ui, controls, super::fonts::FONT, family).clicked() {
        let target = FontTarget::Class(map, layer, i);
        state.actions.push(Action::PickFont(target));
    }
    let warning = fonts::missing(class.font.as_deref(), &state.font_families);
    let mut warning = warning.map(|t| Note::new(t, true, None));
    note_text(ui, None, &mut warning, None);
    if widgets::open_row(ui, controls, STYLE, &face(class).name()).clicked() {
        state.atlas.label_page = Some(ClassPage::Style);
    }

    let text = format!("{} pt", (class.size * 10.0).round() / 10.0);
    if let Some(size) = value_slider(ui, controls, (LABEL_SIZE, "Size"), class.size, SIZES, &text) {
        state.edit_label_class(map, layer, i, |c| c.size = size.round());
    }
    if widgets::open_row(ui, controls, CASE, class.case.name()).clicked() {
        state.atlas.label_page = Some(ClassPage::Case);
    }
    let names = (LETTER_SPACING, LETTER_SPACING);
    let text = percent(class.letter_spacing);
    let spacing = value_slider(ui, controls, names, class.letter_spacing, SPACINGS, &text);
    if let Some(spacing) = spacing {
        let spacing = (spacing * 100.0).round() / 100.0;
        state.edit_label_class(map, layer, i, |c| c.letter_spacing = spacing);
    }

    if let Some(color) = color_rows(ui, state, controls, (COLOR, ClassColor::Text), class.color) {
        state.edit_label_class(map, layer, i, |c| c.color = color);
    }
    let text = percent(class.opacity);
    let opacity = value_slider(
        ui,
        controls,
        (OPACITY, OPACITY),
        class.opacity,
        (0.0, 1.0),
        &text,
    );
    if let Some(opacity) = opacity {
        let opacity = (opacity * 100.0).round() / 100.0;
        state.edit_label_class(map, layer, i, |c| c.opacity = opacity);
    }

    let mut halo = class.halo;
    if widgets::switch_row(ui, controls, HALO, HALO, &mut halo) {
        state.label_class_step(map, layer, i, |c| c.halo = halo);
    }
    if class.halo {
        let which = (HALO_COLOR, ClassColor::Halo);
        if let Some(color) = color_rows(ui, state, controls, which, class.halo_color) {
            state.edit_label_class(map, layer, i, |c| c.halo_color = color);
        }
        let names = (HALO_WIDTH, HALO_WIDTH);
        let text = format!("{:.1} pt", class.halo_width);
        let width = value_slider(ui, controls, names, class.halo_width, HALO_WIDTHS, &text);
        if let Some(width) = width {
            let width = (width * 10.0).round() / 10.0;
            state.edit_label_class(map, layer, i, |c| c.halo_width = width);
        }
    }

    if widgets::open_row(ui, controls, PLACEMENT, class.placement.name()).clicked() {
        state.atlas.label_page = Some(ClassPage::Placement);
    }

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
    let current = face(class);
    for f in faces(state, class) {
        let name = f.name();
        let on = f.weight == current.weight && f.italic == current.italic;
        if widgets::option_row(ui, controls, &name, &name, &[], on).clicked() {
            state.label_class_step(map, layer, i, |c| {
                c.weight = f.weight;
                c.italic = f.italic;
            });
            state.atlas.label_page = None;
        }
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
    for case in TextCase::ALL {
        let name = case.name();
        if widgets::option_row(ui, controls, name, name, &[], case == class.case).clicked() {
            state.label_class_step(map, layer, i, |c| c.case = case);
            state.atlas.label_page = None;
        }
    }
}

/// The page of a new condition of the filter of the class.
fn condition_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (_, map, layer, i): ClassAt,
    class: &LabelClass,
) {
    let mut draft = std::mem::take(&mut state.atlas.label_draft);
    let made = filter::new_condition(ui, state, controls, (layer, i), &mut draft);
    state.atlas.label_draft = draft;
    if let Some(condition) = made {
        let text = filter::with_condition(&class.filter, condition);
        state.label_class_step(map, layer, i, |c| c.filter = text);
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

/// Closes the color picker of the label class when it did not show in this
/// frame, with the keypad of its field.
pub(super) fn color_frame(ctx: &egui::Context, state: &mut AppState) {
    let shown = std::mem::take(&mut state.atlas.label_color_shown);
    if !shown && state.atlas.label_color.take().is_some() {
        widgets::close_keypad(ctx);
    }
}
