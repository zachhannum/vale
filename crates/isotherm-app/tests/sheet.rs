//! The bottom sheet of the phone layout: a tap or a drag on its header
//! collapses and expands it.

mod zonal;

use eframe::egui;
use isotherm_app::app::{CONTOURS, IsothermApp, Layout, Workspace};
use isotherm_app::headless::harness_with;

type H = egui_kittest::Harness<'static, IsothermApp>;

const PHONE: (f32, f32) = (393.0, 852.0);

fn phone(workspace: Workspace) -> H {
    let mut app = IsothermApp::sample(Layout::Pad).expect("the sample world reads");
    app.workspace = workspace;
    app.set_means(Some(zonal::zonal_means()));
    app.safe_area = Some(egui::SafeAreaInsets(egui::epaint::MarginF32 {
        left: 0.0,
        right: 0.0,
        top: 59.0,
        bottom: 34.0,
    }));
    let mut h = harness_with(PHONE, app);
    h.run_steps(3);
    h
}

fn sheet(h: &H) -> egui::Rect {
    let cards = &h.state().cards;
    cards
        .iter()
        .find(|(id, _)| *id == "sheet")
        .expect("a sheet")
        .1
}

fn has_control(h: &H, name: &str) -> bool {
    h.state().controls.iter().any(|c| c.name == name)
}

/// The header of the sheet, from its control.
fn header(h: &H) -> egui::Rect {
    let controls = &h.state().controls;
    let names = ["Collapse sheet", "Expand sheet"];
    let control = controls.iter().find(|c| names.contains(&c.name.as_str()));
    control.expect("a sheet header").rect
}

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

fn tap(h: &mut H, pos: egui::Pos2) {
    touch(h, egui::TouchPhase::Start, pos);
    h.step();
    touch(h, egui::TouchPhase::End, pos);
    // The spring of the sheet settles.
    h.run_steps(60);
}

/// A drag of a finger by `dy` points from `from`.
fn drag(h: &mut H, from: egui::Pos2, dy: f32) {
    touch(h, egui::TouchPhase::Start, from);
    h.step();
    for i in 1..=6 {
        touch(
            h,
            egui::TouchPhase::Move,
            from + egui::vec2(0.0, dy * i as f32 / 6.0),
        );
        h.step();
    }
    touch(h, egui::TouchPhase::End, from + egui::vec2(0.0, dy));
    h.run_steps(60);
}

fn tap_header(h: &mut H) {
    let at = header(h).center();
    tap(h, at);
}

fn drag_header(h: &mut H, dy: f32) {
    let at = header(h).center();
    drag(h, at, dy);
}

#[test]
fn a_tap_on_the_header_collapses_and_expands_the_sheet() {
    let mut h = phone(Workspace::Explore);
    let open = sheet(&h);
    assert!(has_control(&h, CONTOURS));
    tap_header(&mut h);
    assert!(h.state().sheet_collapsed);
    assert!(!has_control(&h, CONTOURS), "the rows hide");
    let collapsed = sheet(&h);
    assert!(collapsed.height() < open.height() - 100.0, "{collapsed:?}");
    assert_eq!(collapsed.bottom(), open.bottom());
    tap_header(&mut h);
    assert!(!h.state().sheet_collapsed);
    assert!(has_control(&h, CONTOURS));
    assert_eq!(sheet(&h).height(), open.height());
}

#[test]
fn a_drag_down_collapses_the_sheet_and_a_drag_up_expands_it() {
    let mut h = phone(Workspace::Explore);
    drag_header(&mut h, 160.0);
    assert!(h.state().sheet_collapsed);
    assert!(!has_control(&h, CONTOURS));
    drag_header(&mut h, -160.0);
    assert!(!h.state().sheet_collapsed);
    assert!(has_control(&h, CONTOURS));
}

#[test]
fn a_short_drag_keeps_the_sheet_open() {
    let mut h = phone(Workspace::Explore);
    let open = sheet(&h);
    drag_header(&mut h, 20.0);
    assert!(!h.state().sheet_collapsed);
    assert_eq!(sheet(&h).height(), open.height());
}

#[test]
fn each_workspace_with_a_sheet_collapses_it() {
    for workspace in [Workspace::Terrain, Workspace::Simulate, Workspace::Explore] {
        let mut h = phone(workspace);
        let open = sheet(&h);
        tap_header(&mut h);
        assert!(sheet(&h).height() < open.height(), "{workspace:?}");
    }
}

#[test]
fn the_readout_stays_above_the_collapsed_sheet() {
    let mut h = phone(Workspace::Explore);
    h.state_mut().explore.pin = Some(vale_terrain::math::lonlat_to_dir(10.0, 20.0));
    h.run_steps(2);
    tap_header(&mut h);
    let cards = &h.state().cards;
    let readout = cards
        .iter()
        .find(|(id, _)| *id == isotherm_app::explore::READOUT);
    let readout = readout.expect("a readout card").1;
    assert!(readout.bottom() <= sheet(&h).top(), "{readout:?}");
}
