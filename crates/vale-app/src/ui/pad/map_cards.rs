//! The collections of a map as cards in the Layers tab of the Atlas
//! workspace.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Color32, Rect, Sense, pos2, vec2};
use vale_store::{CollectionId, LayerId, MapId, Project};

use super::collection::{CardLift, dim_row, header_name, layer_row};
use super::geometry::TOUCH;
use super::icons::{self, Icon};
use super::widgets::{self, Controls, PANEL_ROW};
use super::{layer_icon, list, popover, theme};
use crate::ui::AppState;

pub const ADD_COLLECTION: &str = "Add collection…";
/// The hint of the popover of "Add collection…" when the map has each
/// collection.
pub const ALL_COLLECTIONS: &str = "The map has each collection.";

/// The space between two cards.
const CARD_GAP: f32 = 8.0;
/// The width of the popover of "Add collection…".
const MENU_WIDTH: f32 = 280.0;

/// The name of the row of a collection in the popover of "Add collection…".
pub fn add_row(name: &str) -> String {
    format!("Add {name}")
}

/// The state of the cards of the Layers tab.
#[derive(Default)]
pub struct MapCards {
    /// The swipe of the header row of each card.
    pub heads: HashMap<CollectionId, list::ListState<CollectionId>>,
    /// The card that a hold of its header lifted. While a card is lifted,
    /// each card shows its header row alone, in a list that reorders.
    pub lift: Option<CardLift>,
    /// The list of the header rows while a card is lifted.
    pub stack: list::ListState<CollectionId>,
    /// The popover of "Add collection…" is open.
    pub adding: bool,
}

/// The fill of a card.
fn fill() -> Color32 {
    Color32::from_white_alpha(14)
}

/// The layers of a card of a map from the top, each with true if a higher
/// card of the map already draws it.
pub fn card_rows(project: &Project, map: MapId, id: CollectionId) -> Vec<(LayerId, bool)> {
    let Some(c) = project.collection(id) else {
        return Vec::new();
    };
    let stack = project.map_stack(map);
    (c.layers.iter().rev())
        .map(|l| (*l, project.drawn_above_in(&stack, id, *l)))
        .collect()
}

/// The cards of the collections of a map, from the top of its stack, and
/// "Add collection…". Returns the layer whose row the user tapped.
pub fn cards(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    map: MapId,
    edited: &[LayerId],
) -> Option<LayerId> {
    ui.spacing_mut().item_spacing.y = 0.0;
    let stack = state.doc.project.map_stack(map);
    let cards = &mut state.atlas.map_cards;
    cards.heads.retain(|id, _| stack.contains(id));
    if cards.lift.is_some_and(|l| !stack.contains(&l.id)) {
        cards.lift = None;
    }
    if cards.lift.is_some() {
        restack(ui, state, controls, map, &stack);
        return None;
    }
    let mut open = None;
    for id in stack.iter().rev() {
        let removable = stack.len() > 1;
        let tapped = ui.push_id(("map-card", id.0), |ui| {
            card(ui, state, controls, map, *id, removable, edited)
        });
        open = open.or(tapped.inner);
        ui.add_space(CARD_GAP);
    }
    add_collection(ui, state, controls, map, &stack);
    open
}

/// One card: the header row and the rows of its layers.
fn card(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    map: MapId,
    id: CollectionId,
    removable: bool,
    edited: &[LayerId],
) -> Option<LayerId> {
    let project = &state.doc.project;
    let name = project.collection(id)?.name.clone();
    let rows = card_rows(project, map, id);
    let body = rows.len().max(1) as f32 * PANEL_ROW;
    let size = vec2(ui.available_width(), PANEL_ROW + body);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, theme::CONTROL_RADIUS, fill());
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), PANEL_ROW));
    header(ui, state, controls, (map, id), head, &name, removable);
    let inner = Rect::from_min_max(pos2(rect.left(), head.bottom()), rect.max);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    if rows.is_empty() {
        let at = pos2(inner.left() + 12.0, inner.center().y);
        let font = theme::body();
        let empty = super::collection::EMPTY_COLLECTION;
        (child.painter()).text(at, Align2::LEFT_CENTER, empty, font, theme::MUTE);
        return None;
    }
    layer_rows(&mut child, state, controls, map, &name, &rows, edited)
}

/// The header row of a card: the name and the grip. A swipe to the left
/// shows Remove, which takes the collection out of the map, and a hold
/// lifts the card. The last card of a map has no Remove.
fn header(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    (map, id): (MapId, CollectionId),
    head: Rect,
    name: &str,
    removable: bool,
) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(head)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let rows = [list::Row {
        label: header_name(name),
        actions: if removable { &[list::REMOVE] } else { &[] },
        select: false,
        trailing: TOUCH,
        ..list::Row::new(id, name.to_owned())
    }];
    let spec = list::Spec {
        reorder: true,
        grouped: Some(theme::CONTROL_RADIUS),
        plain: true,
        ..list::Spec::default()
    };
    let font = theme::heading(ui.ctx());
    let list = state.atlas.map_cards.heads.entry(id).or_default();
    let events = list::show(&mut child, controls, list, spec, &rows, |_, _, cell, _| {
        paint_header(&cell.painter, cell.rect, name, font.clone(), cell.lifted);
        None
    });
    for event in events {
        match event {
            list::Event::Lift(key) => {
                let origin = (ui.input(|i| i.pointer.interact_pos())).unwrap_or(head.center());
                state.atlas.map_cards.heads.remove(&key);
                state.atlas.map_cards.lift = Some(CardLift {
                    id: key,
                    grab: origin.y - head.top(),
                    origin,
                    moved: false,
                    offset: None,
                });
                ui.ctx().request_repaint();
            }
            list::Event::Action(key, list::REMOVE) => {
                state.remove_map_collection(map, key);
            }
            _ => {}
        }
    }
}

/// Paints the name and the grip of a header row.
fn paint_header(painter: &egui::Painter, head: Rect, name: &str, font: egui::FontId, lifted: bool) {
    if lifted {
        painter.rect_filled(head, theme::CONTROL_RADIUS, theme::menu());
    }
    let clip = Rect::from_min_max(
        pos2(head.left() + 12.0, head.top()),
        pos2(head.right() - TOUCH, head.bottom()),
    );
    (painter.with_clip_rect(painter.clip_rect().intersect(clip))).text(
        clip.left_center(),
        Align2::LEFT_CENTER,
        name,
        font,
        theme::TEXT,
    );
    let grip = Rect::from_center_size(
        pos2(head.right() - TOUCH / 2.0, head.center().y),
        vec2(18.0, 18.0),
    );
    icons::paint(painter, grip, Icon::Grip, 18.0, theme::MUTE);
}

/// The rows of the layers of a card, from the top, with the switch of the
/// map on each. A row that a higher card already draws is dim and takes
/// no touch. Returns the layer whose row the user tapped.
fn layer_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    map: MapId,
    name: &str,
    rows: &[(LayerId, bool)],
    edited: &[LayerId],
) -> Option<LayerId> {
    let project = &state.doc.project;
    let selected = state.selected_vector();
    let mut looks = Vec::new();
    let mut list_rows = Vec::new();
    for (layer, dim) in rows {
        let Some(l) = project.layer(*layer) else {
            continue;
        };
        looks.push((layer_icon(l), state.doc.switch_on(map, l.id), *dim));
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
        indent: 4.0,
        ..list::Spec::default()
    };
    let mut gestures = list::ListState::default();
    let events = list::show(
        ui,
        controls,
        &mut gestures,
        spec,
        &list_rows,
        |ui, controls, cell, i| {
            let (icon, on, dim) = looks[i];
            let look = list::Look {
                icon: Some(icon),
                text: &list_rows[i].name,
                note: (!dim && edited.contains(&list_rows[i].key)).then_some(super::atlas::EDITED),
                eye: (!dim).then_some(on),
                open: !dim,
                dim,
                ..list::Look::default()
            };
            list::draw(ui, controls, cell, &look)
        },
    );
    let live = |key: LayerId| (list_rows.iter().zip(&looks)).find(|(r, l)| r.key == key && !l.2);
    let mut open = None;
    for event in events {
        match event {
            list::Event::Tap(layer) | list::Event::Open(layer) if live(layer).is_some() => {
                open = Some(layer);
            }
            list::Event::Control(layer, list::EYE) => {
                if let Some((_, look)) = live(layer) {
                    state.set_map_shown(map, layer, !look.1);
                }
            }
            _ => {}
        }
    }
    open
}

/// The header rows of the cards in a list that reorders, while a card is
/// lifted. When the finger lets go, the lifted card moves to its new place
/// in the stack of the map, as one undo step.
fn restack(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    map: MapId,
    stack: &[CollectionId],
) {
    let top_first: Vec<CollectionId> = stack.iter().rev().copied().collect();
    let cards = &mut state.atlas.map_cards;
    let Some(lift) = cards.lift else {
        return;
    };
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
                height: PANEL_ROW + CARD_GAP,
                ..list::Row::new(c.id, c.name.clone())
            })
        })
        .collect();
    let spec = list::Spec {
        reorder: true,
        plain: true,
        ..list::Spec::default()
    };
    let font = theme::heading(ui.ctx());
    let list = &mut state.atlas.map_cards.stack;
    let events = list::show(ui, controls, list, spec, &rows, |_, _, cell, i| {
        let head = Rect::from_min_size(cell.rect.min, vec2(cell.rect.width(), PANEL_ROW));
        cell.painter
            .rect_filled(head, theme::CONTROL_RADIUS, fill());
        paint_header(
            &cell.painter,
            head,
            &rows[i].name,
            font.clone(),
            cell.lifted,
        );
        None
    });
    if state.atlas.map_cards.stack.lift.is_none() {
        state.atlas.map_cards.lift = None;
    }
    for event in events {
        if let list::Event::Move { key, to, .. } = event {
            // The rows are from the top, and the stack is from the bottom.
            state.move_map_collection(map, key, top_first.len() - 1 - to);
        }
    }
}

/// The row "Add collection…" and its popover, with a row for each
/// collection that the map does not have. A tap on a row puts the
/// collection on top of the map, as one undo step, and closes the popover.
fn add_collection(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controls: &mut Controls,
    map: MapId,
    stack: &[CollectionId],
) {
    let button = widgets::button_row(ui, controls, ADD_COLLECTION, true);
    if button.clicked() {
        state.atlas.map_cards.adding = !state.atlas.map_cards.adding;
    }
    if !state.atlas.map_cards.adding {
        return;
    }
    let others: Vec<(CollectionId, String)> = (state.doc.project.collections().iter())
        .filter(|c| !stack.contains(&c.id))
        .map(|c| (c.id, c.name.clone()))
        .collect();
    let mut add = None;
    let shown = popover::show(
        ui.ctx(),
        "pad-map-collections",
        button.rect,
        MENU_WIDTH,
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if others.is_empty() {
                let (rect, _) = ui.allocate_exact_size(vec2(MENU_WIDTH, PANEL_ROW), Sense::hover());
                let at = pos2(rect.left() + 12.0, rect.center().y);
                let font = theme::body();
                (ui.painter()).text(at, Align2::LEFT_CENTER, ALL_COLLECTIONS, font, theme::MUTE);
                return;
            }
            let rows: Vec<list::Row<CollectionId>> = (others.iter())
                .map(|(id, name)| list::Row {
                    label: add_row(name),
                    select: false,
                    lift: false,
                    ..list::Row::new(*id, name.clone())
                })
                .collect();
            let mut gestures = list::ListState::default();
            let spec = list::Spec::default();
            let events = list::show(
                ui,
                controls,
                &mut gestures,
                spec,
                &rows,
                |ui, controls, cell, i| {
                    let look = list::Look {
                        text: &rows[i].name,
                        ..list::Look::default()
                    };
                    list::draw(ui, controls, cell, &look)
                },
            );
            for event in events {
                if let list::Event::Tap(id) = event {
                    add = Some(id);
                }
            }
        },
    );
    if let Some(id) = add {
        state.add_map_collection(map, id);
        state.atlas.map_cards.adding = false;
    } else if shown.closed && !button.clicked() {
        state.atlas.map_cards.adding = false;
    }
}
