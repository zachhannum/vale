//! The cards of the checked collections in the Layers panel, the
//! collection popover, and the Collections page of a layer.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};
use vale_store::{CollectionId, LayerId, Project};

use super::geometry::TOUCH;
use super::icons::{self, Icon};
use super::widgets::{self, Controls, PANEL_ROW};
use super::{LayerRef, list, popover, theme};
use crate::ui::AppState;

/// The name of the header button that opens the collection popover.
pub const COLLECTION_MENU: &str = "Choose collections";
pub const NEW_COLLECTION: &str = "New collection…";
/// The sides of the switch of the collection popover.
pub const COLLECTIONS_SIDE: &str = "Collections";
pub const MAPS_SIDE: &str = "Maps";
/// The row of a layer panel that opens the Collections page, and the title
/// of the page.
pub const COLLECTIONS: &str = "Collections";
/// The name of the name field of a collection.
pub const COLLECTION_NAME: &str = "Collection name";
pub const EMPTY_COLLECTION: &str = "No layers in this collection.";
pub const NO_CHECKED: &str = "No collection is checked.";

/// The width of the collection popover.
const MENU_WIDTH: f32 = 300.0;
/// The space between two cards.
const CARD_GAP: f32 = 10.0;
/// The space between the edge of a card and the content of its rows. The
/// highlight of a row reaches the edge of the card.
const CARD_INSET: f32 = 8.0;
const CARD_RADIUS: f32 = 12.0;

/// The name of the popover row of a collection.
pub fn collection_row(name: &str) -> String {
    format!("Collection {name}")
}

/// The name of the chevron of a card.
pub fn chevron_name(name: &str, collapsed: bool) -> String {
    match collapsed {
        true => format!("Expand {name}"),
        false => format!("Collapse {name}"),
    }
}

/// The name of the header row of a card.
pub fn header_name(name: &str) -> String {
    format!("Card {name}")
}

/// The name of the row of a layer. A layer takes touch in one card at
/// most, so the name of its row has no collection.
pub fn layer_row(layer: &str) -> String {
    format!("Layer {layer}")
}

/// The name of the dim row of a layer that a higher card already draws.
pub fn dim_row(layer: &str, collection: &str) -> String {
    format!("Layer {layer} in {collection}")
}

/// A card that a drag of its header moves.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CardLift {
    pub id: CollectionId,
    /// The distance from the top of the header to the finger.
    pub grab: f32,
}

/// The state of the cards of the Layers panel.
#[derive(Default)]
pub struct Cards {
    /// The gestures and the selected rows of the list of each card.
    pub lists: HashMap<CollectionId, list::ListState<LayerId>>,
    pub lift: Option<CardLift>,
}

impl Cards {
    /// True while a card or a row is lifted.
    pub fn lifted(&self) -> bool {
        self.lift.is_some() || self.lists.values().any(|l| l.lift.is_some())
    }

    /// Shuts the open row and ends each gesture of a row.
    pub fn shut(&mut self) {
        self.lists.values_mut().for_each(list::ListState::shut);
    }

    /// The selected layers of all cards, each one time.
    pub fn picked(&self) -> Vec<LayerId> {
        let mut picked: Vec<LayerId> = Vec::new();
        for id in self.lists.values().flat_map(|l| &l.picked) {
            if !picked.contains(id) {
                picked.push(*id);
            }
        }
        picked
    }
}

/// The checked collections from the top of the stack, then the others in
/// the order of creation.
fn popover_order(project: &Project) -> Vec<(CollectionId, bool)> {
    let checked = project.globe_collections();
    let top_first = checked.iter().rev().map(|id| (*id, true));
    let others = (project.collections().iter())
        .filter(|c| !checked.contains(&c.id))
        .map(|c| (c.id, false));
    top_first.chain(others).collect()
}

/// The layers of a card from the top, each with true if a higher card
/// already draws it.
pub fn card_rows(project: &Project, id: CollectionId) -> Vec<(LayerId, bool)> {
    let Some(c) = project.collection(id) else {
        return Vec::new();
    };
    (c.layers.iter().rev())
        .map(|l| (*l, project.drawn_above(id, *l)))
        .collect()
}

/// The IDs of the layers of the rows that take touch, in each card.
pub fn live_rows(project: &Project) -> Vec<(CollectionId, Vec<LayerId>)> {
    (project.globe_collections().iter())
        .map(|id| {
            let rows = card_rows(project, *id).into_iter();
            (*id, rows.filter(|r| !r.1).map(|r| r.0).collect())
        })
        .collect()
}

/// The collection popover, below the collection button `button`. A tap
/// outside closes it.
pub(super) fn popover(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    button: Rect,
) {
    let shown = popover::show(ui.ctx(), "pad-collections", button, MENU_WIDTH, |ui| {
        menu(ui, state, controls);
    });
    state.pad.collections_rect = Some(shown.rect);
    // A tap outside closes the popover, but not while a name field is open.
    if shown.closed && state.pad.collection_list.rename.is_none() {
        state.pad.collections_menu = false;
    }
}

/// The contents of the collection popover: the Collections | Maps
/// switch, the collections, and "New collection…".
fn menu(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let (field, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH), Sense::hover());
    let sides = [COLLECTIONS_SIDE, MAPS_SIDE];
    widgets::segmented_at(ui, controls, field, &sides, Some(0), |i| i == 0);
    ui.add_space(6.0);

    let project = &state.doc.project;
    let world = project.world_collection();
    let order = popover_order(project);
    let rows: Vec<list::Row<CollectionId>> = (order.iter())
        .filter_map(|(id, _)| {
            let c = project.collection(*id)?;
            Some(list::Row {
                label: collection_row(&c.name),
                rename: c.id != world,
                delete: c.id != world,
                ..list::Row::new(c.id, c.name.clone())
            })
        })
        .collect();
    let list = &mut state.pad.collection_list;
    let renamed = list.rename.as_ref().map(|r| r.0);
    let valid = |name: &str| project.collection_name_is_free(name, renamed);
    let spec = list::Spec {
        field: COLLECTION_NAME,
        valid: &valid,
        ..list::Spec::default()
    };
    let events = list::show(ui, controls, list, spec, &rows, |ui, controls, cell, i| {
        let look = list::Look {
            text: &rows[i].name,
            check: Some(order.iter().any(|(id, on)| *on && *id == rows[i].key)),
            ..list::Look::default()
        };
        list::draw(ui, controls, cell, &look)
    });
    for event in events {
        match event {
            list::Event::Tap(id) => {
                let on = order.iter().any(|(key, on)| *on && *key == id);
                state.check_collection(id, !on);
            }
            list::Event::Rename(id, name) => {
                state.rename_collection(id, &name);
            }
            list::Event::Delete(id) => {
                state.delete_collection(id);
            }
            _ => {}
        }
    }

    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    let line = Rect::from_center_size(line.center(), vec2(line.width(), 1.0));
    ui.painter().rect_filled(line, 0.0, theme::divider());
    if widgets::text_row(ui, controls, NEW_COLLECTION, 0.0, false, true).clicked() {
        // The row of the new collection is its name field.
        let name = state.doc.project.free_collection_name();
        if let Some(id) = state.add_collection(&name) {
            let list = &mut state.pad.collection_list;
            list.shut();
            list.rename = Some((id, name));
        }
    }
}

/// The cards of the checked collections, from the top of the stack.
pub(super) fn cards(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let stack: Vec<CollectionId> = state.doc.project.globe_collections().to_vec();
    if stack.is_empty() {
        state.pad.cards = Cards::default();
        ui.add_space(4.0);
        ui.label(egui::RichText::new(NO_CHECKED).color(theme::MUTE));
        return;
    }
    state.pad.cards.lists.retain(|id, _| stack.contains(id));
    let mut placed = Vec::new();
    for (i, id) in stack.iter().rev().enumerate() {
        if i > 0 {
            ui.add_space(CARD_GAP);
        }
        let rect = ui
            .push_id(("card", id.0), |ui| card(ui, state, controls, *id))
            .inner;
        placed.push((*id, rect));
    }
    move_card(ui, state, &placed);
}

/// One card: the header row and, while the card is open, the rows of its
/// layers. Returns the rect of the card.
fn card(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: CollectionId,
) -> Rect {
    let project = &state.doc.project;
    let Some(c) = project.collection(id) else {
        return Rect::NOTHING;
    };
    let name = c.name.clone();
    let collapsed = project.is_collapsed(id);
    let rows = card_rows(project, id);
    let body = match (collapsed, rows.len()) {
        (true, _) => 0.0,
        (false, 0) => PANEL_ROW,
        (false, n) => n as f32 * PANEL_ROW,
    };
    let size = vec2(ui.available_width(), PANEL_ROW + body);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CARD_RADIUS, theme::track());
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), PANEL_ROW));
    let radius = match collapsed {
        true => egui::CornerRadius::same(CARD_RADIUS as u8),
        false => egui::CornerRadius {
            nw: CARD_RADIUS as u8,
            ne: CARD_RADIUS as u8,
            sw: 0,
            se: 0,
        },
    };
    painter.rect_filled(head, radius, theme::raise());
    header(ui, state, controls, id, head, &name, collapsed);
    if collapsed {
        return rect;
    }
    let inner = Rect::from_min_max(
        pos2(rect.left() + CARD_INSET, head.bottom()),
        pos2(rect.right() - CARD_INSET, rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    if rows.is_empty() {
        let at = pos2(inner.left() + 8.0, inner.center().y);
        let font = theme::body();
        child
            .painter()
            .text(at, Align2::LEFT_CENTER, EMPTY_COLLECTION, font, theme::MUTE);
        return rect;
    }
    layer_rows(&mut child, state, controls, id, &name, &rows);
    rect
}

/// The header row of a card: the chevron and the name. A tap on the row
/// collapses the card or opens it, and a hold of the row lifts the card.
fn header(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: CollectionId,
    head: Rect,
    name: &str,
    collapsed: bool,
) {
    let chevron = Rect::from_center_size(
        pos2(head.left() + 4.0 + TOUCH / 2.0, head.center().y),
        vec2(TOUCH, TOUCH),
    );
    let icon = if collapsed { Icon::Open } else { Icon::Down };
    let chevron_text = chevron_name(name, collapsed);
    let tap = widgets::icon_button_at(ui, controls, chevron, &chevron_text, icon, false, true);
    // The rest of the row, at the right of the chevron.
    let rest = Rect::from_min_max(
        pos2(chevron.right(), chevron.top()),
        pos2(head.right() - 4.0, chevron.bottom()),
    );
    let row = widgets::hit(
        ui,
        controls,
        rest,
        &header_name(name),
        Sense::click(),
        true,
        None,
    );
    paint_name(ui.painter(), head, name);
    if tap.clicked() || row.clicked() {
        state.set_collapsed(id, !collapsed);
    }
    if state.pad.cards.lifted() {
        return;
    }
    if held(ui, &row) {
        ui.ctx().set_dragged_id(row.id);
        let grab = ui.input(|i| i.pointer.interact_pos());
        let grab = grab.map_or(PANEL_ROW / 2.0, |p| p.y - head.top());
        state.pad.cards.lift = Some(CardLift { id, grab });
        state.pad.cards.shut();
        ui.ctx().request_repaint();
    }
}

/// Paints the name of a header row.
fn paint_name(painter: &egui::Painter, head: Rect, name: &str) {
    let left = head.left() + 4.0 + TOUCH + 6.0;
    let clip = Rect::from_min_max(
        pos2(left, head.top()),
        pos2(head.right() - 12.0, head.bottom()),
    );
    painter
        .with_clip_rect(painter.clip_rect().intersect(clip))
        .text(
            clip.left_center(),
            Align2::LEFT_CENTER,
            name,
            theme::title(),
            theme::TEXT,
        );
}

/// True when a press on `row` became a hold of `HOLD_TIME` in place.
fn held(ui: &egui::Ui, row: &egui::Response) -> bool {
    if !row.is_pointer_button_down_on() || row.dragged() {
        return false;
    }
    let press = ui.input(|i| {
        let (start, origin) = (i.pointer.press_start_time()?, i.pointer.press_origin()?);
        let pos = i.pointer.interact_pos()?;
        Some((i.time - start, origin.distance(pos)))
    });
    match press {
        Some((time, moved)) if moved <= list::HOLD_SLOP && time >= list::HOLD_TIME => true,
        Some((time, moved)) if moved <= list::HOLD_SLOP => {
            let wait = std::time::Duration::from_secs_f64(list::HOLD_TIME - time);
            ui.ctx().request_repaint_after(wait);
            false
        }
        _ => false,
    }
}

/// The gap between the cards where a lifted card goes: 0 is above the
/// first card. `placed` are the cards from the top.
fn card_gap(placed: &[(CollectionId, Rect)], y: f32) -> usize {
    placed.iter().filter(|(_, r)| r.center().y < y).count()
}

/// Follows a lifted card. The header of the card moves with the finger, and
/// a line shows the gap where the card goes. When the finger lets go, the
/// card moves to that gap, as one undo step.
fn move_card(ui: &mut egui::Ui, state: &mut AppState, placed: &[(CollectionId, Rect)]) {
    let Some(lift) = state.pad.cards.lift else {
        return;
    };
    let Some(from) = placed.iter().position(|(id, _)| *id == lift.id) else {
        state.pad.cards.lift = None;
        return;
    };
    list::set_lifted(ui.ctx());
    let (pointer, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_down()));
    let y = pointer.map_or(placed[from].1.top() + lift.grab, |p| p.y);
    let gap = card_gap(placed, y);
    if !down {
        state.pad.cards.lift = None;
        // The gap above or below the card itself leaves it in place.
        let to = if gap > from { gap - 1 } else { gap };
        if to != from {
            // `placed` is from the top, and the stack is from the bottom.
            let last = placed.len() - 1;
            state.move_globe_collection(lift.id, last - to);
        }
        return;
    }
    let line_y = match gap {
        0 => placed[0].1.top() - CARD_GAP / 2.0,
        g if g == placed.len() => placed[g - 1].1.bottom() + CARD_GAP / 2.0,
        g => (placed[g - 1].1.bottom() + placed[g].1.top()) / 2.0,
    };
    let card = placed[from].1;
    let layer = egui::LayerId::new(egui::Order::Tooltip, ui.id().with("card-lifted"));
    let painter = ui.painter().clone().with_layer_id(layer);
    if gap != from && gap != from + 1 {
        let line = Rect::from_min_max(
            pos2(card.left() + 8.0, line_y - 1.0),
            pos2(card.right() - 8.0, line_y + 1.0),
        );
        painter.rect_filled(line, 1.0, theme::ACCENT);
    }
    let project = &state.doc.project;
    if let Some(c) = project.collection(lift.id) {
        let head = Rect::from_min_size(
            pos2(card.left(), y - lift.grab),
            vec2(card.width(), PANEL_ROW),
        );
        painter.rect_filled(head, CARD_RADIUS, theme::menu());
        let chevron = Rect::from_center_size(
            pos2(head.left() + 4.0 + TOUCH / 2.0, head.center().y),
            vec2(18.0, 18.0),
        );
        icons::paint(&painter, chevron, Icon::Open, 18.0, theme::MUTE);
        paint_name(&painter, head, &c.name);
    }
    // A finger near the top or the bottom of the panel scrolls it.
    if let Some(p) = pointer {
        let clip = ui.clip_rect();
        let delta = if p.y - clip.top() < PANEL_ROW {
            8.0
        } else if clip.bottom() - p.y < PANEL_ROW {
            -8.0
        } else {
            0.0
        };
        if delta != 0.0 {
            let none = egui::style::ScrollAnimation::none();
            ui.scroll_with_delta_animation(vec2(0.0, delta), none);
        }
    }
    ui.ctx().request_repaint();
}

/// The rows of the layers of an open card, from the top. A row that a
/// higher card already draws is dim and takes no touch.
fn layer_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    id: CollectionId,
    name: &str,
    rows: &[(LayerId, bool)],
) {
    let project = &state.doc.project;
    let selected = state.selected_layer;
    struct Shown {
        layer: LayerRef,
        icon: Icon,
        visible: bool,
        large: bool,
        dim: bool,
    }
    let mut shown = Vec::new();
    let mut list_rows = Vec::new();
    for (layer, dim) in rows {
        let Some(l) = project.layer(*layer) else {
            continue;
        };
        shown.push(Shown {
            layer: LayerRef::of(l),
            icon: super::layer_icon(l),
            visible: l.visible,
            large: crate::ui::budget::too_large(ui.ctx(), l.id),
            dim: *dim,
        });
        list_rows.push(list::Row {
            label: match dim {
                true => dim_row(&l.name, name),
                false => layer_row(&l.name),
            },
            on: !dim && selected == Some(LayerRef::of(l)),
            rename: !dim,
            delete: !dim,
            select: !dim,
            lift: !dim,
            trailing: if *dim { 0.0 } else { 2.0 * TOUCH },
            ..list::Row::new(l.id, l.name.clone())
        });
    }
    let spec = list::Spec {
        select: true,
        reorder: true,
        confine: true,
        grouped: Some(CARD_RADIUS),
        field: super::LAYER_NAME,
        ..list::Spec::default()
    };
    let list = state.pad.cards.lists.entry(id).or_default();
    let events = list::show(
        ui,
        controls,
        list,
        spec,
        &list_rows,
        |ui, controls, cell, i| {
            let s = &shown[i];
            let look = list::Look {
                icon: Some(s.icon),
                text: &list_rows[i].name,
                note: (s.large && !s.dim).then_some(crate::ui::budget::TOO_LARGE),
                warn: s.large,
                eye: (!s.dim).then_some(s.visible),
                open: !s.dim,
                dim: s.dim,
                ..list::Look::default()
            };
            list::draw(ui, controls, cell, &look)
        },
    );
    let of = |key: LayerId| {
        let i = list_rows.iter().position(|r| r.key == key)?;
        (!shown[i].dim).then_some((shown[i].layer, shown[i].visible))
    };
    let len = list_rows.len();
    for event in events {
        match event {
            list::Event::Tap(key) | list::Event::Lift(key) => {
                if let Some((layer, _)) = of(key) {
                    state.select(Some(layer));
                }
            }
            list::Event::Open(key) => {
                if let Some((layer, _)) = of(key) {
                    state.select(Some(layer));
                    state.pad.layer = Some(layer);
                }
            }
            list::Event::Control(key, list::EYE) => {
                if let Some((_, visible)) = of(key) {
                    state.set_layer_visible(key, !visible);
                }
            }
            list::Event::Control(..) | list::Event::Action(..) => {}
            list::Event::Rename(key, name) => {
                state.doc.rename_layer(key, &name);
            }
            list::Event::Delete(key) => {
                state.remove_layer(key);
            }
            list::Event::Move { key, to, .. } => {
                // The rows are from the top, and the collection is from the
                // bottom.
                state.move_in_collection(id, key, len - 1 - to);
            }
        }
    }
}

/// The value of the Collections row of a layer: the name of its one
/// collection, or the number of its collections.
pub fn row_value(project: &Project, layer: LayerId) -> String {
    match project.collections_of(layer).as_slice() {
        [] => "None".to_owned(),
        [one] => one.name.clone(),
        many => format!("{} collections", many.len()),
    }
}

/// The name of the switch of a collection on the Collections page.
pub fn switch_name(name: &str) -> String {
    format!("In {name}")
}

/// The row of a layer panel that opens the Collections page.
pub(super) fn open_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    layer: LayerId,
) {
    let value = row_value(&state.doc.project, layer);
    if widgets::open_row(ui, controls, COLLECTIONS, &value).clicked() {
        state.pad.page = Some(super::Page::Collections);
    }
}

/// The Collections page of a layer: a switch for each collection. The
/// switch of World is dim while the layer is only in World.
pub(super) fn page_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    layer: LayerId,
) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let project = &state.doc.project;
    let switches: Vec<(CollectionId, String, bool, bool)> = (project.collections().iter())
        .map(|c| {
            let on = c.layers.contains(&layer);
            let free = project.can_leave_collection(c.id, layer);
            (c.id, c.name.clone(), on, free)
        })
        .collect();
    for (id, name, mut on, free) in switches {
        let switch = switch_name(&name);
        if widgets::switch_row_enabled(ui, controls, &switch, &name, &mut on, free) {
            state.set_in_collection(id, layer, on);
        }
    }
}
