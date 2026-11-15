//! The shared list of rows of the iPad layout, in a bare harness.

use eframe::egui::{self, Pos2, Rect, vec2};
use egui_kittest::Harness;
use vale_app::ui::pad::list::{self, Event, Group, ListState, Row, Spec};
use vale_app::ui::pad::widgets::{Controls, RENAME, RENAME_WIDTH};

/// A bare list of rows "Row 0", "Row 1", and so on. The key of a row is
/// its index.
struct Bare {
    list: ListState<usize>,
    select: bool,
    reorder: bool,
    rename: bool,
    delete: bool,
    names: Vec<String>,
    events: Vec<Event<usize>>,
    controls: Controls,
}

type H = Harness<'static, Bare>;

const FIELD: &str = "Row name";
/// The time of a frame, in seconds. A drag of six frames is quicker than
/// the hold time.
const STEP: f32 = 0.05;

fn frame(ui: &mut egui::Ui, bare: &mut Bare) {
    bare.controls.clear();
    let scroll = match list::lifted(ui.ctx()) {
        true => egui::scroll_area::ScrollSource::NONE,
        false => egui::scroll_area::ScrollSource::default(),
    };
    egui::ScrollArea::vertical()
        .max_height(300.0)
        .scroll_source(scroll)
        .show(ui, |ui| {
            let rows: Vec<Row<usize>> = (bare.names.iter().enumerate())
                .map(|(i, name)| Row {
                    rename: bare.rename,
                    delete: bare.delete,
                    ..Row::new(i, name.clone())
                })
                .collect();
            let spec = Spec {
                select: bare.select,
                reorder: bare.reorder,
                field: FIELD,
                ..Spec::default()
            };
            let events = list::show(
                ui,
                &mut bare.controls,
                &mut bare.list,
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
            for event in &events {
                match event {
                    Event::Rename(i, name) => bare.names[*i] = name.clone(),
                    Event::Delete(i) => {
                        bare.names.remove(*i);
                    }
                    Event::Move { from, to, .. } => {
                        let name = bare.names.remove(*from);
                        bare.names.insert(*to, name);
                    }
                    _ => {}
                }
            }
            bare.events.extend(events);
        });
}

/// A list of 20 rows with the gestures `select`, `reorder`, and with
/// Rename and Delete buttons behind each row if `actions`.
fn harness(select: bool, reorder: bool, actions: bool) -> H {
    let bare = Bare {
        list: ListState::default(),
        select,
        reorder,
        rename: actions,
        delete: actions,
        names: (0..20).map(|i| format!("Row {i}")).collect(),
        events: Vec::new(),
        controls: Controls::new(),
    };
    let mut h = Harness::builder()
        .with_size(vec2(400.0, 400.0))
        .with_pixels_per_point(1.0)
        .with_step_dt(STEP)
        .build_ui_state(frame, bare);
    h.run_steps(3);
    h
}

fn all() -> H {
    harness(true, true, true)
}

fn control(h: &H, name: &str) -> Option<Rect> {
    let controls = &h.state().controls;
    controls.iter().find(|c| c.name == name).map(|c| c.rect)
}

fn row(h: &H, name: &str) -> Rect {
    control(h, name).unwrap_or_else(|| panic!("no control `{name}`"))
}

fn touch(h: &mut H, phase: egui::TouchPhase, pos: Pos2) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(1),
        phase,
        pos,
        force: None,
    });
    h.event(egui::Event::PointerMoved(pos));
    if phase != egui::TouchPhase::Move {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: phase == egui::TouchPhase::Start,
            modifiers: egui::Modifiers::NONE,
        });
    }
    if phase == egui::TouchPhase::End {
        h.event(egui::Event::PointerGone);
    }
}

fn drag(h: &mut H, from: Pos2, to: Pos2) {
    touch(h, egui::TouchPhase::Start, from);
    h.step();
    for i in 1..=6 {
        touch(h, egui::TouchPhase::Move, from.lerp(to, i as f32 / 6.0));
        h.step();
    }
    touch(h, egui::TouchPhase::End, to);
    h.run_steps(3);
}

fn tap(h: &mut H, name: &str) {
    let pos = row(h, name).center();
    touch(h, egui::TouchPhase::Start, pos);
    h.step();
    touch(h, egui::TouchPhase::End, pos);
    h.run_steps(3);
}

/// A sideways drag of `dx` on the row `name`, from its middle.
fn swipe(h: &mut H, name: &str, dx: f32) {
    let from = row(h, name).center();
    drag(h, from, from + vec2(dx, 0.0));
}

/// Holds a finger on the row `name` for longer than the hold time.
fn hold(h: &mut H, name: &str) -> Pos2 {
    let pos = row(h, name).center();
    touch(h, egui::TouchPhase::Start, pos);
    h.run_steps((1.5 * list::HOLD_TIME / STEP as f64) as usize);
    pos
}

fn events(h: &mut H) -> Vec<Event<usize>> {
    std::mem::take(&mut h.state_mut().events)
}

#[test]
fn a_swipe_to_the_right_selects_a_row_and_a_second_one_deselects_it() {
    let mut h = all();
    swipe(&mut h, "Row 2", 120.0);
    swipe(&mut h, "Row 0", 120.0);
    assert_eq!(h.state().list.picked, [0, 2], "in the order of the list");
    assert!(events(&mut h).is_empty());
    swipe(&mut h, "Row 2", 120.0);
    assert_eq!(h.state().list.picked, [0]);
    // The rows stay in place.
    assert!(control(&h, "Rename Row 2").is_none());
    assert!(h.state().list.open.is_none());
}

#[test]
fn a_swipe_to_the_left_shows_rename_and_delete() {
    let mut h = all();
    swipe(&mut h, "Row 1", -200.0);
    let open = h.state().list.open.unwrap();
    assert_eq!(
        (open.row, open.offset, open.moving),
        (1, -RENAME_WIDTH, false)
    );
    assert!(control(&h, &format!("{RENAME} Row 1")).is_some());
    assert!(control(&h, "Delete Row 3").is_none());
    tap(&mut h, "Delete Row 1");
    assert_eq!(events(&mut h), [Event::Delete(1)]);
    assert_eq!(h.state().names[1], "Row 2");
    assert!(h.state().list.open.is_none());

    // A row with only a Delete button opens by the width of one button.
    let mut h = all();
    h.state_mut().rename = false;
    swipe(&mut h, "Row 1", -200.0);
    assert!(control(&h, "Delete Row 1").is_some());
    assert!(control(&h, &format!("{RENAME} Row 1")).is_none());
    let open = h.state().list.open.unwrap();
    assert_eq!(open.offset, -RENAME_WIDTH / 2.0);
}

#[test]
fn rename_applies_a_valid_name_when_the_field_loses_the_focus() {
    let mut h = all();
    swipe(&mut h, "Row 1", -200.0);
    tap(&mut h, &format!("{RENAME} Row 1"));
    assert!(control(&h, FIELD).is_some());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.event(egui::Event::Text("Sea".to_owned()));
    h.run_steps(1);
    assert!(events(&mut h).is_empty());
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(events(&mut h), [Event::Rename(1, "Sea".to_owned())]);
    assert!(control(&h, FIELD).is_none());
    assert_eq!(h.state().names[1], "Sea");

    // A blank name is not applied.
    swipe(&mut h, "Sea", -200.0);
    tap(&mut h, &format!("{RENAME} Sea"));
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.key_press(egui::Key::Backspace);
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert!(events(&mut h).is_empty());
    assert_eq!(h.state().names[1], "Sea");
}

#[test]
fn a_hold_and_a_drag_move_a_row() {
    let mut h = all();
    swipe(&mut h, "Row 4", 120.0);
    assert_eq!(h.state().list.picked, [4]);
    let to = row(&h, "Row 3").center();
    let from = hold(&mut h, "Row 1");
    assert_eq!(h.state().list.lift.map(|l| l.key), Some(1));
    assert!(
        h.state().list.picked.is_empty(),
        "a lift clears the selection"
    );
    assert_eq!(events(&mut h), [Event::Lift(1)]);
    for i in 1..=6 {
        touch(
            &mut h,
            egui::TouchPhase::Move,
            from.lerp(to, i as f32 / 6.0),
        );
        h.step();
    }
    touch(&mut h, egui::TouchPhase::End, to);
    h.run_steps(3);
    assert_eq!(
        events(&mut h),
        [Event::Move {
            key: 1,
            from: 1,
            to: 3
        }]
    );
    assert_eq!(h.state().names[..4], ["Row 0", "Row 2", "Row 3", "Row 1"]);
    assert!(h.state().list.lift.is_none());
}

#[test]
fn a_fast_vertical_drag_scrolls_and_emits_nothing() {
    let mut h = all();
    let top = row(&h, "Row 0").center().y;
    let from = row(&h, "Row 4").center();
    drag(&mut h, from, from - vec2(10.0, 150.0));
    assert!(events(&mut h).is_empty());
    let list = &h.state().list;
    assert!(list.open.is_none() && list.lift.is_none() && list.picked.is_empty());
    assert!(
        control(&h, "Row 0").is_none_or(|r| r.center().y < top - 50.0),
        "the list did not scroll"
    );
}

#[test]
fn a_short_swipe_snaps_shut() {
    let mut h = all();
    swipe(&mut h, "Row 1", -30.0);
    assert!(h.state().list.open.is_none());
    assert!(control(&h, "Delete Row 1").is_none());
    swipe(&mut h, "Row 1", 20.0);
    assert!(h.state().list.picked.is_empty());
    assert!(events(&mut h).is_empty());

    // A tap on an open row only shuts it.
    swipe(&mut h, "Row 1", -200.0);
    assert!(h.state().list.open.is_some());
    tap(&mut h, "Row 1");
    assert!(h.state().list.open.is_none());
    assert!(events(&mut h).is_empty());
}

#[test]
fn a_list_with_no_gestures_gives_only_taps() {
    let mut h = harness(false, false, false);
    swipe(&mut h, "Row 1", 120.0);
    swipe(&mut h, "Row 1", -200.0);
    hold(&mut h, "Row 2");
    let pos = row(&h, "Row 2").center();
    touch(&mut h, egui::TouchPhase::Move, pos + vec2(0.0, 100.0));
    h.step();
    touch(&mut h, egui::TouchPhase::End, pos + vec2(0.0, 100.0));
    h.run_steps(3);
    let list = &h.state().list;
    assert!(list.open.is_none() && list.lift.is_none() && list.picked.is_empty());
    assert!(control(&h, "Delete Row 1").is_none());
    let names: Vec<String> = (0..20).map(|i| format!("Row {i}")).collect();
    assert_eq!(h.state().names, names);
    // The hold that moved ended as no tap.
    assert!(events(&mut h).iter().all(|e| matches!(e, Event::Tap(_))));
    tap(&mut h, "Row 3");
    assert_eq!(events(&mut h), [Event::Tap(3)]);
}

#[test]
fn the_group_buttons_select_all_deselect_all_and_delete() {
    use list::{DESELECT_ALL, SELECT_ALL, group_action, group_buttons};
    const DELETE: &str = "Delete rows";
    assert_eq!(group_buttons(0, 3, &[], Some(DELETE)), None);
    let names = |picked, count| -> Vec<&str> {
        let buttons = group_buttons(picked, count, &[], Some(DELETE)).unwrap();
        buttons.into_iter().map(|b| b.0).collect()
    };
    assert_eq!(names(1, 3), [SELECT_ALL, DELETE, DESELECT_ALL]);
    assert_eq!(names(3, 3), [DELETE, DESELECT_ALL]);

    let mut state = ListState::<usize>::default();
    let keys = [0, 1, 2];
    assert_eq!(
        group_action(&mut state, &keys, SELECT_ALL, Some(DELETE)),
        None
    );
    assert_eq!(state.picked, keys);
    assert_eq!(
        group_action(&mut state, &keys, DESELECT_ALL, Some(DELETE)),
        None
    );
    assert!(state.picked.is_empty());
    state.picked = vec![0, 2];
    let group = group_action(&mut state, &keys, DELETE, Some(DELETE));
    assert_eq!(group, Some(Group::Delete(vec![0, 2])));
    assert!(state.picked.is_empty());
    state.picked = vec![1];
    let group = group_action(&mut state, &keys, "Edit rows", Some(DELETE));
    assert_eq!(group, Some(Group::Extra("Edit rows")));
}
