//! The checked collections in the Layers panel, the
//! collection popover, and the Collections page of a layer.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};
use vale_store::{CollectionId, LayerId, MapId, Project};

use super::geometry::TOUCH;
use super::icons::{self, Icon};
use super::widgets::{self, Controls, PANEL_ROW};
use super::{LayerRef, list, motion, popover, theme};
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
/// The space between the left edge of a header row and the name. The
/// chevron fills the touch square at the left.
const NAME_X: f32 = TOUCH;
/// The space between the left edge of the list and the content of the
/// layer rows of a collection. The icon of a layer is then below the name
/// of the header.
pub const ROW_INDENT: f32 = NAME_X - 2.0;
/// The space between each edge of the list and the content of a row. The
/// highlight of a row reaches the edges of the list.
const ROW_EDGE: f32 = 8.0;

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

/// The view whose collections the rows show: the globe, or a map in the
/// Layers tab of a frame.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum View {
    Globe,
    Map(MapId),
}

impl View {
    /// The collections of the view, from the bottom to the top.
    pub fn stack(self, project: &Project) -> Vec<CollectionId> {
        match self {
            View::Globe => project.globe_collections().to_vec(),
            View::Map(map) => project.map_stack(map),
        }
    }

    /// An ID of a spring of the view.
    fn id(self, name: &str) -> egui::Id {
        match self {
            View::Globe => egui::Id::new(name),
            View::Map(_) => egui::Id::new((name, "map")),
        }
    }
}

/// The state of the rows of a view.
fn cards_of(state: &mut AppState, view: View) -> &mut Cards {
    match view {
        View::Globe => &mut state.pad.cards,
        View::Map(_) => &mut state.atlas.map_cards,
    }
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
    /// The place of the finger when the card lifted.
    pub origin: egui::Pos2,
    /// The finger moved from `origin` by more than the slop of a hold.
    pub moved: bool,
    /// The space above the header rows, so that the row of the lifted card
    /// starts under the finger. Set in the first frame of the lift.
    pub offset: Option<f32>,
}

/// The state of the cards of the Layers panel.
#[derive(Default)]
pub struct Cards {
    /// The gestures and the selected rows of the list of each card.
    pub lists: HashMap<CollectionId, list::ListState<LayerId>>,
    /// The card that a hold of its header lifted. While a card is lifted,
    /// each card shows its header row alone, in a list that reorders.
    pub lift: Option<CardLift>,
    /// The list of the header rows while a card is lifted.
    pub stack: list::ListState<CollectionId>,
    /// The swipe of the header row of each collection of a map.
    pub heads: HashMap<CollectionId, list::ListState<CollectionId>>,
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
pub(super) fn popover_order(project: &Project) -> Vec<(CollectionId, bool)> {
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
    card_rows_in(project, project.globe_collections(), id)
}

/// The layers of a collection of `stack` from the top, each with true if
/// a higher collection of the stack already draws it.
pub fn card_rows_in(
    project: &Project,
    stack: &[CollectionId],
    id: CollectionId,
) -> Vec<(LayerId, bool)> {
    let Some(c) = project.collection(id) else {
        return Vec::new();
    };
    (c.layers.iter().rev())
        .map(|l| (*l, project.drawn_above_in(stack, id, *l)))
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

/// The collections of a view from the top of its stack, then the others
/// in the order of creation, each with true if the view shows it.
fn popover_order_in(project: &Project, view: View) -> Vec<(CollectionId, bool)> {
    let stack = view.stack(project);
    let top_first = stack.iter().rev().map(|id| (*id, true));
    let others = (project.collections().iter())
        .filter(|c| !stack.contains(&c.id))
        .map(|c| (c.id, false));
    top_first.chain(others).collect()
}

/// The collection popover of a view, below the collection button
/// `button`. A tap outside closes it.
pub(super) fn popover(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    button: Rect,
    view: View,
) {
    let shown = popover::show(ui.ctx(), "pad-collections", button, MENU_WIDTH, |ui| {
        menu(ui, state, controls, view);
    });
    state.pad.collections_rect = Some(shown.rect);
    // A tap outside closes the popover, but not while a name field is open.
    if shown.closed && state.pad.collection_list.rename.is_none() {
        state.pad.collections_menu = false;
    }
}

/// The contents of the collection popover: the Collections | Maps switch
/// of the globe, the collections, "New collection…", and on the globe
/// "Manage collections". In a map, a check puts a collection in the map,
/// and the check of the last collection of the map does not change.
fn menu(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls, view: View) {
    ui.spacing_mut().item_spacing.y = 0.0;
    if view == View::Globe {
        let (field, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH), Sense::hover());
        let sides = [COLLECTIONS_SIDE, MAPS_SIDE];
        widgets::segmented_at(ui, controls, field, &sides, Some(0), |i| i == 0);
        ui.add_space(6.0);
    }

    let project = &state.doc.project;
    let world = project.world_collection();
    let order = popover_order_in(project, view);
    let last = matches!(view, View::Map(_)) && order.iter().filter(|(_, on)| *on).count() == 1;
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
        let on = order.iter().any(|(id, on)| *on && *id == rows[i].key);
        let look = list::Look {
            text: &rows[i].name,
            check: Some(on),
            dim: on && last,
            ..list::Look::default()
        };
        list::draw(ui, controls, cell, &look)
    });
    for event in events {
        match event {
            list::Event::Tap(id) => {
                let on = order.iter().any(|(key, on)| *on && *key == id);
                match view {
                    View::Globe => state.check_collection(id, !on),
                    View::Map(map) if on => state.remove_map_collection(map, id),
                    View::Map(map) => state.add_map_collection(map, id),
                };
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

    if widgets::button_row(ui, controls, NEW_COLLECTION, true).clicked() {
        // The row of the new collection is its name field.
        let name = state.doc.project.free_collection_name();
        let id = match view {
            View::Globe => state.add_collection(&name),
            View::Map(map) => state.add_collection_to_map(map, &name),
        };
        if let Some(id) = id {
            let list = &mut state.pad.collection_list;
            list.shut();
            list.rename = Some((id, name));
        }
    }
    if view == View::Globe
        && widgets::open_row(ui, controls, super::manage::MANAGE_COLLECTIONS, "").clicked()
    {
        state.pad.collections_menu = false;
        state.pad.collection_list = list::ListState::default();
        state.pad.manage.open(super::manage::ManagePage::Manage);
    }
}

/// The cards of the collections of a view, from the top of the stack.
/// Returns the layer whose row the user tapped in a map.
pub(super) fn cards(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    view: View,
) -> Option<LayerId> {
    ui.spacing_mut().item_spacing.y = 0.0;
    let stack = view.stack(&state.doc.project);
    if stack.is_empty() {
        *cards_of(state, view) = Cards::default();
        ui.add_space(4.0);
        ui.label(egui::RichText::new(NO_CHECKED).color(theme::MUTE));
        return None;
    }
    let cards = cards_of(state, view);
    cards.lists.retain(|id, _| stack.contains(id));
    cards.heads.retain(|id, _| stack.contains(id));
    if cards.lift.is_some_and(|l| !stack.contains(&l.id)) {
        cards.lift = None;
    }
    // The rows of the cards close on a spring when a card lifts, and they
    // open again after the drop.
    let lifted = cards.lift.is_some();
    let target = if lifted { 0.0 } else { 1.0 };
    let open = motion::value(ui.ctx(), view.id("pad-cards-open"), target, motion::SNAPPY);
    // The cards become a list of header rows when no rows show.
    let project = &state.doc.project;
    let tallest = (stack.iter()).fold(0.0_f32, |m, id| m.max(body_height(project, *id)));
    if lifted && tallest * open < 0.5 {
        restack(ui, state, controls, view, &stack);
        return None;
    }
    // While the rows close, the space above the cards keeps the header of
    // the lifted card under the finger. After the drop, the space closes.
    let top = ui.cursor().top();
    let space = match cards_of(state, view).lift {
        Some(lift) => {
            let project = &state.doc.project;
            let above: f32 = (stack.iter().rev())
                .take_while(|id| **id != lift.id)
                .map(|id| PANEL_ROW + body_height(project, *id) * open)
                .sum();
            let space = (finger(ui, lift) - lift.grab - top - above).max(0.0);
            motion::set(ui.ctx(), view.id(OFFSET), space);
            space
        }
        None => motion::points(ui.ctx(), view.id(OFFSET), 0.0, motion::SNAPPY),
    };
    ui.add_space(space.max(0.0));
    let mut tapped = None;
    for id in stack.iter().rev() {
        let inner = ui.push_id(("card", id.0), |ui| {
            card(ui, state, controls, view, (*id, &stack), open)
        });
        tapped = tapped.or(inner.inner);
    }
    tapped
}

/// The height of the rows of an open collection. A collapsed collection
/// has none.
fn body_height(project: &Project, id: CollectionId) -> f32 {
    let rows = project.collection(id).map_or(0, |c| c.layers.len());
    match (project.is_collapsed(id), rows) {
        (true, _) => 0.0,
        (false, n) => n.max(1) as f32 * PANEL_ROW,
    }
}

/// The height of the finger, or of the place where the card lifted.
fn finger(ui: &egui::Ui, lift: CardLift) -> f32 {
    ui.input(|i| i.pointer.interact_pos())
        .map_or(lift.origin.y, |p| p.y)
}

/// The spring of the space above the header rows of a restack.
const OFFSET: &str = "pad-cards-offset";

/// The header rows of the cards in a list that reorders, while a card is
/// lifted. When the finger lets go, the lifted card moves to its new place
/// in the stack, as one undo step, and the cards open again. A hold that
/// ends in place collapses the card or opens it, as a tap does.
fn restack(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    view: View,
    stack: &[CollectionId],
) {
    let top_first: Vec<CollectionId> = stack.iter().rev().copied().collect();
    let cards = cards_of(state, view);
    let Some(mut lift) = cards.lift else {
        return;
    };
    if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
        lift.moved |= p.distance(lift.origin) > list::HOLD_SLOP;
        cards.lift = Some(lift);
    }
    let Some(from) = top_first.iter().position(|id| *id == lift.id) else {
        cards.lift = None;
        return;
    };
    if cards.stack.lift.is_none() {
        // The list takes the hold of the header.
        cards.stack = list::ListState::default();
        cards.stack.lift = Some(list::Lift {
            key: lift.id,
            from,
            grab: lift.grab,
            last: None,
        });
    }
    let project = &state.doc.project;
    let rows: Vec<list::Row<CollectionId>> = (top_first.iter())
        .filter_map(|id| {
            let c = project.collection(*id)?;
            Some(list::Row {
                label: header_name(&c.name),
                select: false,
                height: PANEL_ROW,
                ..list::Row::new(c.id, c.name.clone())
            })
        })
        .collect();
    let spec = list::Spec {
        reorder: true,
        plain: true,
        ..list::Spec::default()
    };
    let width = ui.available_width();
    let top = ui.cursor().top();
    let finger = finger(ui, lift);
    let offset = *lift.offset.get_or_insert_with(|| {
        let slot = top + from as f32 * PANEL_ROW;
        (finger - lift.grab - slot).max(0.0)
    });
    cards_of(state, view).lift = Some(lift);
    motion::set(ui.ctx(), view.id(OFFSET), offset);
    ui.add_space(offset);
    let (area, _) = ui.allocate_exact_size(vec2(width, 0.0), Sense::hover());
    let inner = Rect::from_min_max(
        pos2(area.left() + 8.0, area.top()),
        pos2(
            area.right() - 8.0,
            area.top() + rows.len() as f32 * PANEL_ROW,
        ),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let font = theme::heading(ui.ctx());
    let collapsed: Vec<bool> = (rows.iter())
        .map(|r| state.doc.project.is_collapsed(r.key))
        .collect();
    let list = &mut cards_of(state, view).stack;
    let events = list::show(&mut child, controls, list, spec, &rows, |_, _, cell, i| {
        // A header row at its full height. Only the lifted row has a fill,
        // as a lifted layer row does.
        let pill = Rect::from_min_size(
            cell.rect.min - vec2(8.0, 0.0),
            vec2(cell.rect.width() + 16.0, PANEL_ROW),
        );
        if cell.lifted {
            let fill = theme::card().to_opaque();
            cell.painter.rect_filled(pill, theme::ROW_RADIUS, fill);
            cell.painter
                .rect_filled(pill, theme::ROW_RADIUS, theme::raise());
        }
        let chevron = Rect::from_center_size(chevron_center(pill), vec2(18.0, 18.0));
        let icon = if collapsed[i] { Icon::Open } else { Icon::Down };
        icons::paint(&cell.painter, chevron, icon, 18.0, theme::MUTE);
        paint_name(&cell.painter, pill, &rows[i].name, font.clone());
        None
    });
    ui.allocate_exact_size(vec2(width, inner.height()), Sense::hover());
    if cards_of(state, view).stack.lift.is_none() {
        cards_of(state, view).lift = None;
        // A hold that ends in place acts as a tap on the header row.
        if !lift.moved && events.is_empty() {
            let collapsed = state.doc.project.is_collapsed(lift.id);
            state.set_collapsed(lift.id, !collapsed);
        }
    }
    for event in events {
        if let list::Event::Move { key, to, .. } = event {
            // The rows are from the top, and the stack is from the bottom.
            let to = top_first.len() - 1 - to;
            match view {
                View::Globe => state.move_globe_collection(key, to),
                View::Map(map) => state.move_map_collection(map, key, to),
            };
        }
    }
}

/// One card: the header row and, while the card is open, the rows of its
/// layers. `open` is the part of the rows that shows, from 0 to 1, while
/// the rows close or open around a restack. Returns the layer whose row
/// the user tapped in a map.
fn card(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    view: View,
    (id, stack): (CollectionId, &[CollectionId]),
    open: f32,
) -> Option<LayerId> {
    let project = &state.doc.project;
    let c = project.collection(id)?;
    let name = c.name.clone();
    let collapsed = project.is_collapsed(id);
    let rows = card_rows_in(project, stack, id);
    let body = body_height(project, id);
    let shown = body * open;
    let size = vec2(ui.available_width(), PANEL_ROW + shown);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let closed = collapsed || shown < 0.5;
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), PANEL_ROW));
    let removable = view == View::Globe || stack.len() > 1;
    header(ui, state, controls, view, (id, removable), head, &name);
    if closed {
        return None;
    }
    let inner = Rect::from_min_max(
        pos2(rect.left() + ROW_INDENT, head.bottom()),
        pos2(rect.right() - ROW_EDGE, head.bottom() + body),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    // The rows fade, and the collection cuts them, while they close or
    // open.
    child.set_clip_rect(child.clip_rect().intersect(rect));
    child.multiply_opacity(open);
    if rows.is_empty() {
        let at = pos2(inner.left(), inner.center().y);
        let font = theme::body();
        child
            .painter()
            .text(at, Align2::LEFT_CENTER, EMPTY_COLLECTION, font, theme::MUTE);
        return None;
    }
    match view {
        View::Globe => {
            layer_rows(&mut child, state, controls, id, &name, &rows);
            None
        }
        View::Map(map) => map_rows(&mut child, state, controls, (map, id), &name, &rows),
    }
}

/// The name of the chevron of a header row in [`list::Trail::Control`].
const CHEVRON: &str = "chevron";

/// The header row of a collection: the chevron and the name. A tap on the
/// row or the chevron collapses the collection or opens it, and a hold
/// lifts it. A swipe to the left shows Remove, which takes the collection
/// out of the view: off the globe, or out of the map. The last collection
/// of a map has no Remove.
fn header(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    view: View,
    (id, removable): (CollectionId, bool),
    head: Rect,
    name: &str,
) {
    let collapsed = state.doc.project.is_collapsed(id);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(head)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let rows = [list::Row {
        label: header_name(name),
        actions: if removable { &[list::REMOVE] } else { &[] },
        select: false,
        leading: TOUCH,
        ..list::Row::new(id, name.to_owned())
    }];
    let spec = list::Spec {
        reorder: true,
        plain: true,
        ..list::Spec::default()
    };
    let font = theme::heading(ui.ctx());
    let chevron_text = chevron_name(name, collapsed);
    let lifted = cards_of(state, view).lifted();
    let list = cards_of(state, view).heads.entry(id).or_default();
    let events = list::show(
        &mut child,
        controls,
        list,
        spec,
        &rows,
        |ui, controls, cell, _| {
            let row = cell.rect.translate(vec2(cell.shift.max(0.0), 0.0));
            let chevron = Rect::from_center_size(chevron_center(row), vec2(TOUCH, TOUCH));
            let icon = if collapsed { Icon::Open } else { Icon::Down };
            paint_name(&cell.painter, row, name, font.clone());
            let tap = widgets::icon_button_at(
                ui,
                controls,
                chevron,
                &chevron_text,
                icon,
                false,
                cell.live,
            );
            tap.clicked().then_some(list::Trail::Control(CHEVRON))
        },
    );
    for event in events {
        match event {
            list::Event::Tap(_) | list::Event::Control(_, CHEVRON) => {
                state.set_collapsed(id, !collapsed);
            }
            list::Event::Lift(_) if !lifted => {
                let origin = (ui.input(|i| i.pointer.interact_pos())).unwrap_or(head.center());
                let cards = cards_of(state, view);
                cards.heads.remove(&id);
                cards.shut();
                cards.lift = Some(CardLift {
                    id,
                    grab: origin.y - head.top(),
                    origin,
                    moved: false,
                    offset: None,
                });
                ui.ctx().request_repaint();
            }
            list::Event::Action(_, list::REMOVE) => {
                match view {
                    View::Globe => state.check_collection(id, false),
                    View::Map(map) => state.remove_map_collection(map, id),
                };
            }
            _ => {}
        }
    }
}

/// The rows of the layers of an open collection of a map, from the top,
/// with the switch of the map as the eye. A row that a higher collection
/// already draws is dim and takes no touch. Returns the layer whose row
/// the user tapped.
fn map_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (map, id): (MapId, CollectionId),
    name: &str,
    rows: &[(LayerId, bool)],
) -> Option<LayerId> {
    let project = &state.doc.project;
    let edited = state.doc.local_changes(map);
    let selected = state.selected_vector();
    let mut looks = Vec::new();
    let mut list_rows = Vec::new();
    for (layer, dim) in rows {
        let Some(l) = project.layer(*layer) else {
            continue;
        };
        let note = (!dim && edited.contains(&l.id)).then_some(super::atlas::EDITED);
        looks.push((
            super::layer_icon(l),
            state.doc.switch_on(map, l.id),
            *dim,
            note,
        ));
        list_rows.push(list::Row {
            label: match dim {
                true => dim_row(&l.name, name),
                false => layer_row(&l.name),
            },
            on: !dim && selected == Some(l.id),
            select: false,
            lift: false,
            trailing: if *dim { 0.0 } else { 2.0 * TOUCH },
            ..list::Row::new(l.id, l.name.clone())
        });
    }
    let spec = list::Spec {
        indent: ROW_INDENT - ROW_EDGE,
        ..list::Spec::default()
    };
    let list = state.atlas.map_cards.lists.entry(id).or_default();
    let events = list::show(
        ui,
        controls,
        list,
        spec,
        &list_rows,
        |ui, controls, cell, i| {
            let (icon, on, dim, note) = looks[i];
            let look = list::Look {
                icon: Some(icon),
                text: &list_rows[i].name,
                note,
                eye: (!dim).then_some(on),
                open: !dim,
                dim,
                ..list::Look::default()
            };
            list::draw(ui, controls, cell, &look)
        },
    );
    let live = |key: LayerId| {
        let i = list_rows.iter().position(|r| r.key == key)?;
        (!looks[i].2).then_some(looks[i].1)
    };
    let mut open = None;
    for event in events {
        match event {
            list::Event::Tap(layer) | list::Event::Open(layer) if live(layer).is_some() => {
                open = Some(layer);
            }
            list::Event::Control(layer, list::EYE) => {
                if let Some(on) = live(layer) {
                    state.set_map_shown(map, layer, !on);
                }
            }
            _ => {}
        }
    }
    open
}

/// The center of the chevron of a header row.
fn chevron_center(head: Rect) -> egui::Pos2 {
    pos2(head.left() + TOUCH / 2.0, head.center().y)
}

/// Paints the name of a header row.
fn paint_name(painter: &egui::Painter, head: Rect, name: &str, font: egui::FontId) {
    let left = head.left() + NAME_X;
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
            font,
            theme::TEXT,
        );
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
        indent: ROW_INDENT - ROW_EDGE,
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

/// The name of the row of a collection on the Collections page.
pub fn switch_name(name: &str) -> String {
    format!("In {name}")
}

/// The hint of the search field of the Collections page.
pub const SEARCH_COLLECTIONS: &str = "Search";

/// The collections of the Collections page of a layer whose names hold
/// `search`: the collections that hold the layer, then the others, each
/// group by name. Each has true if it holds the layer, and true if the
/// layer can leave it.
pub fn page_rows(
    project: &Project,
    layer: LayerId,
    search: &str,
) -> Vec<(CollectionId, bool, bool)> {
    let search = search.trim().to_lowercase();
    let mut rows: Vec<(&str, CollectionId, bool, bool)> = (project.collections().iter())
        .filter(|c| c.name.to_lowercase().contains(&search))
        .map(|c| {
            let on = c.layers.contains(&layer);
            (
                c.name.as_str(),
                c.id,
                on,
                project.can_leave_collection(c.id, layer),
            )
        })
        .collect();
    rows.sort_by_key(|(name, _, on, _)| (!on, name.to_lowercase()));
    rows.into_iter()
        .map(|(_, id, on, free)| (id, on, free))
        .collect()
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
        state.pad.collection_search.clear();
        state.pad.layer_collections = list::ListState::default();
    }
}

/// The Collections page of a layer: the search field, a row with a check
/// for each collection that holds the layer, and "New collection…". A tap
/// on a row puts the layer in the collection or takes it out. The row of
/// World is dim while the layer is only in World.
pub(super) fn page_body(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    layer: LayerId,
) {
    ui.spacing_mut().item_spacing.y = 0.0;
    super::filter::search_field(
        ui,
        controls,
        SEARCH_COLLECTIONS,
        &mut state.pad.collection_search,
    );
    let project = &state.doc.project;
    let shown = page_rows(project, layer, &state.pad.collection_search);
    let rows: Vec<list::Row<CollectionId>> = (shown.iter())
        .filter_map(|(id, _, free)| {
            let c = project.collection(*id)?;
            Some(list::Row {
                label: switch_name(&c.name),
                enabled: *free,
                ..list::Row::new(c.id, c.name.clone())
            })
        })
        .collect();
    let list = &mut state.pad.layer_collections;
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
            check: Some(shown[i].1),
            dim: !shown[i].2,
            ..list::Look::default()
        };
        list::draw(ui, controls, cell, &look)
    });
    for event in events {
        match event {
            list::Event::Tap(id) => {
                let on = shown.iter().any(|(key, on, _)| *on && *key == id);
                state.set_in_collection(id, layer, !on);
            }
            list::Event::Rename(id, name) => {
                state.rename_collection(id, &name);
            }
            _ => {}
        }
    }

    if widgets::button_row(ui, controls, NEW_COLLECTION, true).clicked() {
        // The row of the new collection is its name field.
        state.pad.collection_search.clear();
        let name = state.doc.project.free_collection_name();
        if let Some(id) = state.add_collection_with(&name, layer) {
            let list = &mut state.pad.layer_collections;
            list.shut();
            list.rename = Some((id, name));
        }
    }
}
