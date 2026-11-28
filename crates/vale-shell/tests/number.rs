//! The slider row and the stepper row of the pad layout.

use eframe::egui::{self, Pos2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use vale_shell::pad::metrics::TOUCH;
use vale_shell::pad::number::{self, SliderResponse};
use vale_shell::pad::widgets::Controls;

#[derive(Default)]
struct State {
    value: f64,
    controls: Controls,
    /// Each response of the slider that was not empty.
    events: Vec<SliderResponse>,
    changes: usize,
}

fn slider() -> Harness<'static, State> {
    let state = State {
        value: 0.5,
        ..State::default()
    };
    Harness::builder()
        .with_size(vec2(400.0, 100.0))
        .build_ui_state(
            |ui, state: &mut State| {
                state.controls.clear();
                let text = format!("{:.2}", state.value);
                let resp = number::slider_row(
                    ui,
                    &mut state.controls,
                    "Albedo",
                    &mut state.value,
                    0.0..=1.0,
                    &text,
                );
                if resp != SliderResponse::default() {
                    state.events.push(resp);
                }
            },
            state,
        )
}

fn stepper(value: f64) -> Harness<'static, State> {
    let state = State {
        value,
        ..State::default()
    };
    Harness::builder()
        .with_size(vec2(400.0, 100.0))
        .build_ui_state(
            |ui, state: &mut State| {
                state.controls.clear();
                let text = format!("{:.0} m", state.value);
                if number::stepper_row(
                    ui,
                    &mut state.controls,
                    "Height",
                    &mut state.value,
                    100.0,
                    0.0..=300.0,
                    &text,
                ) {
                    state.changes += 1;
                }
            },
            state,
        )
}

fn pointer(h: &mut Harness<State>, pos: Pos2, pressed: Option<bool>) {
    h.event(egui::Event::PointerMoved(pos));
    if let Some(pressed) = pressed {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.step();
}

fn assert_touch_size(controls: &Controls) {
    assert!(!controls.is_empty());
    for control in controls {
        let size = control.rect.size();
        assert!(size.x >= TOUCH && size.y >= TOUCH, "{control:?}");
    }
}

#[test]
fn a_drag_on_the_slider_changes_the_value() {
    let mut h = slider();
    h.run();
    let rect = h.get_by_label("Albedo").rect();
    let from = pos2(rect.center().x, rect.center().y);
    pointer(&mut h, from, Some(true));
    for i in 1..=5 {
        pointer(&mut h, from + vec2(8.0 * i as f32, 0.0), None);
    }
    pointer(&mut h, from + vec2(40.0, 0.0), Some(false));
    h.run();
    let state = h.state();
    assert!(state.value > 0.5, "{}", state.value);
    assert!(state.value <= 1.0);
    let started = state.events.iter().position(|e| e.started).unwrap();
    let changed = state.events.iter().position(|e| e.changed).unwrap();
    let released = state.events.iter().position(|e| e.released).unwrap();
    assert!(
        started <= changed && changed <= released,
        "{:?}",
        state.events
    );
    assert_eq!(state.events.iter().filter(|e| e.started).count(), 1);
    assert_eq!(state.events.iter().filter(|e| e.released).count(), 1);
}

#[test]
fn a_drag_past_the_end_stops_at_the_end_of_the_range() {
    let mut h = slider();
    h.run();
    let from = h.get_by_label("Albedo").rect().center();
    pointer(&mut h, from, Some(true));
    for i in 1..=5 {
        pointer(&mut h, from + vec2(-200.0 * i as f32, 0.0), None);
    }
    pointer(&mut h, from + vec2(-1000.0, 0.0), Some(false));
    h.run();
    assert_eq!(h.state().value, 0.0);
}

#[test]
fn the_slider_is_a_touch_control() {
    let mut h = slider();
    h.run();
    assert_touch_size(&h.state().controls);
}

#[test]
fn the_stepper_buttons_change_the_value_in_its_range() {
    let mut h = stepper(150.0);
    h.run();
    h.get_by_label("Increase Height").click();
    h.run();
    assert_eq!(h.state().value, 200.0);
    h.get_by_label("Increase Height").click();
    h.run();
    assert_eq!(h.state().value, 300.0);
    // The plus button is off at the end of the range.
    h.get_by_label("Increase Height").click();
    h.run();
    assert_eq!(h.state().value, 300.0);
    for _ in 0..5 {
        h.get_by_label("Decrease Height").click();
        h.run();
    }
    assert_eq!(h.state().value, 0.0);
    assert_eq!(h.state().changes, 5);
}

#[test]
fn the_stepper_buttons_are_touch_controls() {
    let mut h = stepper(100.0);
    h.run();
    let controls = &h.state().controls;
    assert_eq!(controls.len(), 2);
    assert_touch_size(controls);
}
