//! The workspace switch, with no GPU.

use eframe::egui;
use egui_kittest::kittest::Queryable;
use isotherm_app::app::{WORKSPACE_MENU, Workspace};
use isotherm_app::headless::harness;

const NAMES: [&str; 6] = [
    "Worlds", "Terrain", "Planet", "Simulate", "Explore", "Export",
];

#[test]
fn the_ipad_switch_shows_all_six_workspaces() {
    let mut harness = harness((1194.0, 834.0));
    harness.run_steps(2);
    for name in NAMES {
        harness.get_by_label(name);
    }
    assert!(harness.query_by_label(WORKSPACE_MENU).is_none());
}

#[test]
fn a_tap_on_terrain_opens_the_terrain_workspace() {
    let mut harness = harness((1194.0, 834.0));
    harness.run_steps(2);
    harness.get_by_label("Terrain").click();
    harness.run_steps(2);
    assert_eq!(harness.state().workspace, Workspace::Terrain);
    harness.get_by_label("Heightmap import comes in a later version.");
}

#[test]
fn the_phone_switch_is_one_menu_button() {
    let mut harness = harness((393.0, 852.0));
    harness.run_steps(2);
    assert!(harness.query_by_label("Terrain").is_none());
    harness.get_by_label(WORKSPACE_MENU).click();
    harness.run_steps(2);
    for name in NAMES {
        harness.get_by_label(name);
    }
    harness.get_by_label("Terrain").click();
    harness.run_steps(2);
    assert_eq!(harness.state().workspace, Workspace::Terrain);
    assert!(!harness.state().menu);
}

#[test]
fn the_worlds_workspace_shows_the_build() {
    let mut harness = harness((1194.0, 834.0));
    harness.run_steps(2);
    harness.get_by_label("Worlds").click();
    harness.run_steps(2);
    harness.get_by_label(&isotherm_app::build::label());
}

type H = egui_kittest::Harness<'static, isotherm_app::app::IsothermApp>;

fn touch(h: &mut H, phase: egui::TouchPhase, pos: egui::Pos2) {
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

/// Drags from the middle of the globe to the right, with a finger or with the mouse.
fn drag_globe(finger: bool) -> bool {
    let mut h = harness((1194.0, 834.0));
    h.run_steps(2);
    let before = h.state().globe.view.rot.0;
    let from = h.state().globe.rect.center();
    let to = from + egui::vec2(150.0, 0.0);
    if finger {
        touch(&mut h, egui::TouchPhase::Start, from);
    } else {
        h.event(egui::Event::PointerMoved(from));
        h.event(egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.step();
    for i in 1..=6 {
        let pos = from.lerp(to, i as f32 / 6.0);
        if finger {
            touch(&mut h, egui::TouchPhase::Move, pos);
        } else {
            h.event(egui::Event::PointerMoved(pos));
        }
        h.step();
    }
    if finger {
        touch(&mut h, egui::TouchPhase::End, to);
    } else {
        h.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(3);
    h.state().globe.view.rot.0 != before
}

#[test]
fn one_finger_turns_the_globe() {
    assert!(drag_globe(true));
}

#[test]
fn the_mouse_turns_the_globe() {
    assert!(drag_globe(false));
}

#[test]
fn the_cards_are_plain_with_no_gpu() {
    let mut harness = harness((1194.0, 834.0));
    harness.run_steps(2);
    assert!(!vale_shell::pad::widgets::is_glass(&harness.ctx));
}

#[test]
fn the_globe_fills_the_bands_outside_the_safe_area() {
    // The outer screen of iPhone Duo in portrait, with the column of the
    // status bar at the right and the band of the home indicator at the bottom.
    let mut h = harness((466.0, 678.0));
    h.state_mut().safe_area = Some(egui::SafeAreaInsets(egui::epaint::MarginF32 {
        left: 0.0,
        right: 84.0,
        top: 0.0,
        bottom: 34.0,
    }));
    h.run_steps(2);
    let screen = h.ctx.viewport_rect();
    assert_eq!(h.state().globe.rect, screen);
    // The card stays in the safe area.
    let menu = h.get_by_label(WORKSPACE_MENU).rect();
    assert!(menu.right() <= screen.right() - 84.0);
}
