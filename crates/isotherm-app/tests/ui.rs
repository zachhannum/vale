//! The workspace switch, with no GPU.

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
    harness.get_by_label("The terrain tools come in a later version.");
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
