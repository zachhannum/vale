//! The top bar, the workspace tabs, the view control, the dock, and the
//! floating panels of the iPad layout from 600 points.

mod screenshot;

use clap::Parser;
use eframe::egui::{self, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use vale_app::dock::{DockState, Entry, HEIGHT_MIN, WIDTH_DEFAULT, WIDTH_MAX, WIDTH_MIN};
use vale_app::document::Document;
use vale_app::globe::WorldView;
use vale_app::projects::Locations;
use vale_app::scale_bar::readout_scale;
use vale_app::session::Session;
use vale_app::ui::pad::atlas::{NEXT_PAGE, PAGE_BUTTON, PREVIOUS_PAGE};
use vale_app::ui::pad::bar::{BACK, DOCK};
use vale_app::ui::pad::dock::{EDGE_DRAG, Slot, bar_name, title_name};
use vale_app::ui::pad::flyout::Flyout;
use vale_app::ui::pad::geometry::BAR;
use vale_app::ui::pad::widgets::dock_name;
use vale_app::ui::pad::{
    ADD_LAYER, CLEAR_PATH, GRATICULE, ModeGroup, PANELS, PROJECTION, Panel, RECENTER, RESET, SAVE,
    SCULPT, TOOLBOX, WORLD_PROPERTIES, WORLDS,
};
use vale_app::ui::panels::ALL_WORLDS;
use vale_app::ui::world_form::NEW_WORLD;
use vale_app::ui::{Action, AppState, Layout, Screen, Workspace, draw, select};

type Pad = Harness<'static, AppState>;

/// The landscape and the portrait board of an iPad, and a window just below
/// 1,000 points.
const BOARDS: [(f32, f32); 3] = [(1194.0, 834.0), (834.0, 1194.0), (999.0, 744.0)];
const WORKSPACES: [Workspace; 2] = [Workspace::Globe, Workspace::Atlas];

fn state(workspace: Workspace) -> AppState {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.set_layout(Layout::Pad);
    state.workspace = workspace;
    state
}

fn pad_with(state: AppState, (width, height): (f32, f32)) -> Pad {
    let mut h = Harness::builder()
        .with_size(vec2(width, height))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(3);
    h
}

fn pad(workspace: Workspace, size: (f32, f32)) -> Pad {
    pad_with(state(workspace), size)
}

fn control(h: &Pad, name: &str) -> Option<Rect> {
    let controls = &h.state().pad.controls;
    controls.iter().find(|c| c.name == name).map(|c| c.rect)
}

/// The place of a control that can be off, as the accessibility tree has
/// it.
fn widget(h: &Pad, name: &str) -> Rect {
    h.get_by_label(name).rect()
}

fn touch(h: &mut Pad, phase: egui::TouchPhase, pos: Pos2) {
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

/// The events of one phase of a tap of the first touch, in one frame. A tap
/// is then a press and a release one frame apart, shorter than a hold.
fn tap_touch(h: &mut Pad, phase: egui::TouchPhase, pos: Pos2, force: Option<f32>) {
    let events = &mut h.input_mut().events;
    events.push(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(1),
        phase,
        pos,
        force,
    });
    events.push(egui::Event::PointerMoved(pos));
    events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: phase == egui::TouchPhase::Start,
        modifiers: egui::Modifiers::NONE,
    });
    if phase == egui::TouchPhase::End {
        events.push(egui::Event::PointerGone);
    }
}

fn tap(h: &mut Pad, name: &str) {
    let rect = control(h, name).unwrap_or_else(|| panic!("no control `{name}`"));
    tap_touch(h, egui::TouchPhase::Start, rect.center(), None);
    h.step();
    tap_touch(h, egui::TouchPhase::End, rect.center(), None);
    h.run_steps(3);
}

/// Taps the row of a panel in the Panels menu.
fn show(h: &mut Pad, name: &str) {
    tap(h, PANELS);
    let menu = h.state().pad.panels_rect.expect("the Panels menu");
    let controls = &h.state().pad.controls;
    let row = controls
        .iter()
        .find(|c| c.name == name && menu.contains_rect(c.rect))
        .unwrap_or_else(|| panic!("no row `{name}`"))
        .rect;
    tap_touch(h, egui::TouchPhase::Start, row.center(), None);
    h.step();
    tap_touch(h, egui::TouchPhase::End, row.center(), None);
    h.run_steps(3);
}

fn rects(h: &Pad) -> vale_app::ui::pad::geometry::Rects {
    h.state().pad.rects.unwrap()
}

#[test]
fn the_bar_is_44_points_high_below_the_band_and_fills_the_band() {
    for workspace in WORKSPACES {
        for size in BOARDS {
            let dir = tempfile::tempdir().unwrap();
            let mut state = state(workspace);
            state.projects = Locations::single(dir.path());
            let h = pad_with(state, size);
            let r = rects(&h);
            let what = format!("{workspace:?} {size:?}");
            let width = size.0;
            assert_eq!(
                r.band,
                Rect::from_min_size(Pos2::ZERO, vec2(width, 24.0)),
                "{what}"
            );
            let bar = Rect::from_min_size(egui::pos2(0.0, 24.0), vec2(width, BAR));
            assert_eq!(r.bar, bar, "{what}");
            // One blurred area covers the band and the bar.
            let area = h.ctx.memory(|m| m.area_rect(egui::Id::new("pad-bar")));
            assert_eq!(area, Some(r.band.union(bar)), "{what}");
            // The band has no controls. The controls of the bar are in it.
            for c in &h.state().pad.controls {
                assert!(c.rect.top() >= 24.0, "{what}: {} in the band", c.name);
            }
            for name in [BACK, WORLDS, "Undo", "Redo", "World", "Atlas"] {
                let rect = widget(&h, name);
                assert!(bar.contains_rect(rect), "{what}: {name} {rect:?}");
                assert!(
                    rect.width() >= 44.0 && rect.height() >= 44.0,
                    "{what}: {name}"
                );
            }
            // Back, the two workspace buttons, Undo, and Redo are at the
            // left in this order, and the name of the world is at the center
            // of the canvas, clear of the buttons at both ends.
            let lefts = [BACK, "World", "Atlas", "Undo", "Redo"].map(|n| widget(&h, n).left());
            assert!(lefts.is_sorted(), "{what}: {lefts:?}");
            let name = widget(&h, WORLDS);
            assert_eq!(name.center().x, r.canvas.center().x, "{what}");
            assert!(widget(&h, "Redo").right() <= name.left(), "{what}");
            assert!(name.right() <= r.actions.left(), "{what}");
        }
    }
}

#[test]
fn back_opens_the_start_screen() {
    for workspace in WORKSPACES {
        let dir = tempfile::tempdir().unwrap();
        let mut state = state(workspace);
        state.projects = Locations::single(dir.path());
        let mut h = pad_with(state, BOARDS[0]);
        tap(&mut h, BACK);
        let actions = std::mem::take(&mut h.state_mut().actions);
        assert_eq!(actions, [Action::ShowProjects], "{workspace:?}");
        h.state_mut().run_action(Action::ShowProjects);
        assert_eq!(h.state().screen, Screen::Browser, "{workspace:?}");
    }
}

#[test]
fn the_name_of_the_world_opens_the_menu_of_the_worlds() {
    for workspace in WORKSPACES {
        for size in BOARDS {
            let what = format!("{workspace:?} {size:?}");
            let dir = tempfile::tempdir().unwrap();
            let mut state = state(workspace);
            state.projects = Locations::single(dir.path());
            let path = dir.path().join("Earth.gpkg");
            state.session = Some(Session::open(&path, &mut state.doc).unwrap());
            let mut h = pad_with(state, size);
            let name = control(&h, WORLDS).unwrap();
            assert!(name.width() >= 80.0, "{what}");
            tap(&mut h, WORLDS);
            let menu = h
                .state()
                .pad
                .worlds_rect
                .unwrap_or_else(|| panic!("{what}"));
            assert!(menu.top() >= name.bottom(), "{what}");
            assert_eq!(menu.center().x, name.center().x, "{what}");
            let rows = [
                "Earth (sample)",
                NEW_WORLD,
                WORLD_PROPERTIES,
                SAVE,
                ALL_WORLDS,
            ];
            let tops = rows.map(|row| {
                let rect = control(&h, row).unwrap_or_else(|| panic!("{what}: no {row}"));
                assert!(menu.contains_rect(rect), "{what}: {row}");
                rect.top()
            });
            assert!(tops.is_sorted(), "{what}: {tops:?}");
            // The row of the open world has the width of the other rows: no
            // row keeps a column for the tick.
            let world = control(&h, "Earth (sample)").unwrap();
            let new = control(&h, NEW_WORLD).unwrap();
            assert_eq!(
                (world.left(), world.width()),
                (new.left(), new.width()),
                "{what}"
            );
            // A second tap closes the menu.
            tap(&mut h, WORLDS);
            assert!(h.state().pad.worlds.is_none(), "{what}");
        }
    }
}

/// The control is on, as the accessibility tree has it.
fn on(h: &Pad, name: &str) -> bool {
    let node = h.get_by_label(name);
    node.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn the_workspace_buttons_switch_the_workspace() {
    for size in BOARDS {
        let mut h = pad(Workspace::Globe, size);
        let back = widget(&h, BACK);
        for name in ["World", "Atlas"] {
            let button = control(&h, name).unwrap();
            assert_eq!(button.size(), vec2(44.0, 44.0), "{size:?}: {name}");
            assert!(button.left() >= back.right(), "{size:?}: {name}");
        }
        // The button of the open workspace is on.
        assert!(on(&h, "World"), "{size:?}");
        assert!(!on(&h, "Atlas"), "{size:?}");
        tap(&mut h, "Atlas");
        assert_eq!(h.state().workspace, Workspace::Atlas, "{size:?}");
        assert!(on(&h, "Atlas"), "{size:?}");
        tap(&mut h, "World");
        assert_eq!(h.state().workspace, Workspace::Globe, "{size:?}");
    }
}

#[test]
fn the_name_and_the_view_control_share_the_center_of_the_canvas() {
    for workspace in WORKSPACES {
        for open in [false, true] {
            let mut state = state(workspace);
            state.pad.docks.get_mut(workspace).open = open;
            let h = pad_with(state, BOARDS[0]);
            let what = format!("{workspace:?} open {open}");
            let name = control(&h, WORLDS).unwrap();
            let r = rects(&h);
            let view = r.view.unwrap();
            assert_eq!(name.center().x, view.center().x, "{what}");
            assert_eq!(view.center().x, r.canvas.center().x, "{what}");
            assert_eq!(view.top(), r.bar.bottom() + 8.0, "{what}");
            let right = if open { 1194.0 - 320.0 } else { 1194.0 };
            assert_eq!(r.canvas.right(), right, "{what}");
        }
    }
}

#[test]
fn the_view_control_holds_the_views_and_the_graticule() {
    for size in BOARDS {
        let mut h = pad(Workspace::Globe, size);
        let view = rects(&h).view.unwrap();
        for name in ["Globe", "Flat", GRATICULE] {
            let rect = control(&h, name).unwrap_or_else(|| panic!("{size:?}: no {name}"));
            assert!(view.contains_rect(rect), "{size:?}: {name}");
        }
        assert!(control(&h, RECENTER).is_none(), "{size:?}");
        let on = h.state().globe.preview.graticule;
        tap(&mut h, GRATICULE);
        assert_eq!(h.state().globe.preview.graticule, !on, "{size:?}");
        tap(&mut h, GRATICULE);
        assert_eq!(h.state().globe.preview.graticule, on, "{size:?}");

        // The flat view adds Recenter, Reset, and the list of projections.
        tap(&mut h, "Flat");
        assert_eq!(h.state().globe.world_view, WorldView::Flat);
        let view = rects(&h).view.unwrap();
        for name in ["Globe", "Flat", GRATICULE, RECENTER, RESET, PROJECTION] {
            assert!(
                view.contains_rect(widget(&h, name)),
                "{size:?}: flat {name}"
            );
        }
        tap(&mut h, PROJECTION);
        assert!(h.state().pad.projections, "{size:?}");
    }
}

#[test]
fn the_view_control_of_the_atlas_holds_the_page_controls() {
    for size in BOARDS {
        let h = pad(Workspace::Atlas, size);
        let view = rects(&h).view.unwrap();
        for name in [PREVIOUS_PAGE, NEXT_PAGE] {
            assert!(view.contains_rect(widget(&h, name)), "{size:?}: {name}");
        }
        // The card has room for the name of the page, and not for its size.
        let page = control(&h, PAGE_BUTTON).unwrap();
        assert!(view.contains_rect(page), "{size:?}");
        assert!(page.width() < 100.0, "{size:?}: {page:?}");
        assert!(control(&h, GRATICULE).is_none(), "{size:?}");
    }
}

#[test]
fn the_dock_zone_holds_the_panels_button_and_the_dock_button_from_1000_points() {
    for size in BOARDS {
        let mut h = pad(Workspace::Globe, size);
        let r = rects(&h);
        let panels = control(&h, PANELS).unwrap_or_else(|| panic!("{size:?}: no Panels"));
        assert!(r.dock_zone.contains_rect(panels), "{size:?}");
        for name in ["Layers", select::ATTRIBUTES, TOOLBOX] {
            assert!(control(&h, name).is_none(), "{size:?}: {name}");
        }
        // The workspace menu is gone.
        assert!(control(&h, "Workspace").is_none(), "{size:?}");
        // The Panels menu opens a panel.
        show(&mut h, TOOLBOX);
        assert!(h.state().pad.is_open(Panel::Toolbox), "{size:?}");
        if size.0 < 1000.0 {
            assert!(control(&h, DOCK).is_none(), "{size:?}");
            continue;
        }
        let dock = control(&h, DOCK).unwrap();
        assert!(panels.right() < dock.left(), "{size:?}");
        tap(&mut h, DOCK);
        assert!(h.state().pad.docks.get(Workspace::Globe).open);
        assert!(rects(&h).dock.is_some());
        tap(&mut h, DOCK);
        assert!(!h.state().pad.docks.get(Workspace::Globe).open);
    }
}

/// Holds a finger on a control for `steps` frames of a quarter second, and
/// keeps it down.
fn press(h: &mut Pad, name: &str, steps: usize) -> Pos2 {
    let at = control(h, name)
        .unwrap_or_else(|| panic!("no control `{name}`"))
        .center();
    touch(h, egui::TouchPhase::Start, at);
    for _ in 0..steps {
        h.step();
    }
    at
}

#[test]
fn a_hold_on_a_tool_cell_shows_the_name_of_the_tool() {
    let mut h = pad(Workspace::Globe, BOARDS[0]);
    let cell = control(&h, select::TOOL).unwrap();
    assert!(rects(&h).column.contains_rect(cell));
    assert_eq!(cell.size(), vec2(44.0, 44.0));
    // A short press shows no name.
    let at = press(&mut h, select::TOOL, 1);
    assert_eq!(h.state().pad.hold_label, None);
    // A hold of 0.4 seconds shows the name next to the cell.
    h.step();
    h.step();
    let (name, rect) = h.state().pad.hold_label.clone().expect("no label");
    assert_eq!((name.as_str(), rect), (select::TOOL, cell));
    let label = h
        .ctx
        .memory(|m| m.area_rect(egui::Id::new("pad-hold-label")))
        .expect("no label card");
    assert!(label.left() > rects(&h).column.right());
    assert!(label.top() < cell.center().y && cell.center().y < label.bottom());
    // The label goes away at the end of the press.
    touch(&mut h, egui::TouchPhase::End, at);
    h.run_steps(3);
    assert_eq!(h.state().pad.hold_label, None);
    // A hold on a cell of a group opens the list of the group instead.
    h.state_mut().add_height();
    h.run_steps(2);
    let sculpt = control(&h, SCULPT).unwrap();
    let at = press(&mut h, SCULPT, 3);
    assert_eq!(h.state().pad.hold_label, None);
    let flyouts = &h.state().pad.flyouts;
    assert_eq!(flyouts.open, Some(Flyout::Brush(ModeGroup::Sculpt)));
    let list = flyouts.rect.expect("no list");
    assert!(list.left() > rects(&h).column.right());
    assert!(list.top() <= sculpt.top());
    touch(&mut h, egui::TouchPhase::End, at);
    h.run_steps(3);
}

#[test]
fn a_hold_on_a_cell_of_the_tool_shows_its_name_and_does_not_run_it() {
    let mut h = pad(Workspace::Globe, BOARDS[0]);
    vale_app::ui::pad::toggle_measure(h.state_mut());
    h.state_mut().measure.add([10.0, 10.0], false);
    h.state_mut().measure.add([20.0, 10.0], false);
    h.run_steps(2);
    let column = rects(&h).column;
    let cell = control(&h, CLEAR_PATH).expect("no cell");
    assert!(column.contains_rect(cell));
    // A hold shows the name next to the cell.
    let at = press(&mut h, CLEAR_PATH, 3);
    let (name, rect) = h.state().pad.hold_label.clone().expect("no label");
    assert_eq!((name.as_str(), rect), ("Clear", cell));
    // The end of the hold does not clear the path.
    touch(&mut h, egui::TouchPhase::End, at);
    h.run_steps(3);
    assert_eq!(h.state().pad.hold_label, None);
    assert_eq!(h.state().measure.points.len(), 2);
    // A tap clears it.
    tap(&mut h, CLEAR_PATH);
    assert!(h.state().measure.points.is_empty());
}

#[test]
fn the_readout_is_at_the_lower_left_of_the_canvas_with_a_scale_bar() {
    for size in BOARDS {
        let what = format!("{size:?}");
        let mut h = pad(Workspace::Globe, size);
        for view in [WorldView::Globe, WorldView::Flat] {
            h.state_mut().globe.world_view = view;
            h.run_steps(3);
            let what = format!("{what} {view:?}");
            let r = rects(&h);
            let readout = r.readout.expect("no readout");
            assert_eq!(readout.left(), r.column.right() + 16.0, "{what}");
            assert!(readout.bottom() <= size.1 - r.bands.bottom, "{what}");
            assert!(readout.top() >= r.column.bottom(), "{what}");
            // The readout is a card at the lower left of its place.
            let card = h
                .ctx
                .memory(|m| m.area_rect(egui::Id::new("pad-readout")))
                .expect("no readout card");
            assert_eq!(card.left_bottom(), readout.left_bottom(), "{what}");
            assert_eq!(card.height(), readout.height(), "{what}");
            let label = h.state().pad.readout_scale.clone().expect("no scale bar");
            let (value, unit) = label.split_once(' ').unwrap();
            assert_eq!(unit, "km", "{what}: {label}");
            let value: f64 = value.parse().unwrap();
            let lead = value / 10f64.powf(value.log10().floor());
            assert!(
                [1.0, 2.0, 5.0].iter().any(|m| (lead - m).abs() < 1e-9),
                "{what}: {label}"
            );
            // On the globe, the scale is the scale at its center.
            if view == WorldView::Globe {
                let globe = &h.state().globe;
                let world = &h.state().doc.project.world;
                let km = world.radius_km / globe.radius();
                let want = readout_scale(km, world.units).unwrap();
                assert_eq!(label, want.label(), "{what}");
                assert!(want.length <= 140.0, "{what}");
            }
        }
    }
    // The Atlas workspace has no readout.
    for size in BOARDS {
        let h = pad(Workspace::Atlas, size);
        assert_eq!(rects(&h).readout, None, "{size:?}");
        assert_eq!(h.state().pad.readout_scale, None, "{size:?}");
    }
}

/// A touch that moves from one place to another in six steps. A force makes
/// it a pen.
fn drag(h: &mut Pad, from: Pos2, to: Pos2, force: Option<f32>) {
    let event = |h: &mut Pad, phase, pos| {
        h.event(egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(1),
            phase,
            pos,
            force,
        });
    };
    event(h, egui::TouchPhase::Start, from);
    touch(h, egui::TouchPhase::Start, from);
    h.step();
    for i in 1..=6 {
        let at = from.lerp(to, i as f32 / 6.0);
        event(h, egui::TouchPhase::Move, at);
        h.event(egui::Event::PointerMoved(at));
        h.step();
    }
    event(h, egui::TouchPhase::End, to);
    touch(h, egui::TouchPhase::End, to);
    h.run_steps(3);
}

/// The middle of the edge handle of the dock.
fn handle(h: &Pad) -> Pos2 {
    h.state()
        .pad
        .dock_frame
        .handle
        .expect("no edge handle")
        .center()
}

fn dock_of(h: &Pad, workspace: Workspace) -> &DockState {
    h.state().pad.docks.get(workspace)
}

/// The panels of the dock of the World workspace, from top to bottom, and
/// whether each is folded.
fn docked(h: &Pad) -> Vec<(Panel, bool)> {
    let dock = dock_of(h, Workspace::Globe);
    dock.panels().iter().map(|p| (p.panel, p.folded)).collect()
}

fn slot(h: &Pad, panel: Panel) -> Slot {
    h.state()
        .pad
        .dock_frame
        .slot(panel)
        .unwrap_or_else(|| panic!("no slot of {panel:?}"))
}

/// The World workspace on a landscape iPad with the dock open and these
/// panels in it.
fn docked_pad(panels: &[Panel]) -> Pad {
    let mut state = state(Workspace::Globe);
    let entries: Vec<Entry> = panels.iter().map(|&panel| Entry::new(panel)).collect();
    vale_app::dock::apply(&mut state.pad.docks, Workspace::Globe, &entries);
    pad_with(state, BOARDS[0])
}

fn shortcut(h: &mut Pad) {
    h.event(egui::Event::Key {
        key: egui::Key::I,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::ALT | egui::Modifiers::COMMAND,
    });
    h.run_steps(3);
}

#[test]
fn the_dock_button_the_shortcut_and_the_edge_handle_open_and_close_the_dock() {
    for workspace in WORKSPACES {
        let mut h = pad(workspace, BOARDS[0]);
        let open = |h: &Pad| dock_of(h, workspace).open;
        tap(&mut h, DOCK);
        assert!(open(&h));
        assert!(rects(&h).dock.is_some());
        tap(&mut h, DOCK);
        assert!(!open(&h));
        assert!(rects(&h).dock.is_none());

        shortcut(&mut h);
        assert!(open(&h), "{workspace:?}: ⌥⌘I opens");
        shortcut(&mut h);
        assert!(!open(&h), "{workspace:?}: ⌥⌘I closes");

        // A drag from the right edge to the left opens the dock.
        let at = handle(&h);
        drag(&mut h, at, at - vec2(120.0, 0.0), None);
        assert!(open(&h), "{workspace:?}: the edge opens");
        // The edge handle is at the left edge of the dock.
        let dock = rects(&h).dock.unwrap();
        assert!(
            h.state()
                .pad
                .dock_frame
                .handle
                .unwrap()
                .x_range()
                .contains(dock.left())
        );

        // A drag of the edge handle to the right edge closes it, and the dock
        // keeps its width.
        let at = handle(&h);
        let right = rects(&h).screen.right() - 2.0;
        drag(&mut h, at, pos2(right, at.y), None);
        assert!(!open(&h), "{workspace:?}: the edge closes");
        assert_eq!(dock_of(&h, workspace).width, WIDTH_DEFAULT);
    }
}

#[test]
fn a_short_drag_from_the_right_edge_does_not_open_the_dock() {
    let mut h = pad(Workspace::Globe, BOARDS[0]);
    let at = handle(&h);
    drag(&mut h, at, at - vec2(EDGE_DRAG - 10.0, 0.0), None);
    assert!(!dock_of(&h, Workspace::Globe).open);
}

#[test]
fn the_shortcut_needs_both_keys_and_no_text_field() {
    let mut h = pad(Workspace::Globe, BOARDS[0]);
    h.event(egui::Event::Key {
        key: egui::Key::I,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    });
    h.run_steps(3);
    assert!(!dock_of(&h, Workspace::Globe).open);
    // Below 1,000 points the shortcut does nothing.
    let mut h = pad(Workspace::Globe, BOARDS[2]);
    shortcut(&mut h);
    assert!(!dock_of(&h, Workspace::Globe).open);
}

#[test]
fn the_dock_holds_any_number_of_panels_in_the_order_they_open() {
    let mut h = pad(Workspace::Globe, BOARDS[0]);
    tap(&mut h, DOCK);
    for name in [TOOLBOX, "Layers", select::ATTRIBUTES] {
        show(&mut h, name);
    }
    let order = [Panel::Toolbox, Panel::Layers, Panel::Attributes];
    assert_eq!(docked(&h), order.map(|p| (p, false)));
    let tops = order.map(|p| slot(&h, p).title.top());
    assert!(tops.is_sorted(), "{tops:?}");
    let dock = rects(&h).dock.unwrap();
    for panel in order {
        let rect = slot(&h, panel).rect();
        assert!(dock.contains_rect(rect), "{panel:?}");
        assert!(h.state().pad.is_open(panel), "{panel:?}");
        assert!(control(&h, &title_name(panel)).is_some(), "{panel:?}");
    }
    assert!(control(&h, ADD_LAYER).is_some(), "the + of Layers");
    // A tap on the row of a panel in the dock removes it.
    show(&mut h, select::ATTRIBUTES);
    assert_eq!(
        docked(&h),
        [(Panel::Toolbox, false), (Panel::Layers, false)]
    );
    assert!(!h.state().pad.is_open(Panel::Attributes));
}

#[test]
fn a_tap_on_the_title_row_folds_and_opens_a_panel() {
    let mut h = docked_pad(&[Panel::Layers, Panel::Toolbox]);
    let open_height = slot(&h, Panel::Toolbox).body.unwrap().height();
    tap(&mut h, &title_name(Panel::Layers));
    assert_eq!(docked(&h), [(Panel::Layers, true), (Panel::Toolbox, false)]);
    assert_eq!(slot(&h, Panel::Layers).body, None);
    assert!(slot(&h, Panel::Toolbox).body.unwrap().height() > open_height);
    assert!(control(&h, ADD_LAYER).is_none(), "the body is gone");
    tap(&mut h, &title_name(Panel::Layers));
    assert_eq!(
        docked(&h),
        [(Panel::Layers, false), (Panel::Toolbox, false)]
    );
}

#[test]
fn a_drag_of_the_bar_between_two_panels_changes_their_heights() {
    let mut h = docked_pad(&[Panel::Layers, Panel::Toolbox]);
    let layers = slot(&h, Panel::Layers).body.unwrap().height();
    let toolbox = slot(&h, Panel::Toolbox).body.unwrap().height();
    let bar = control(&h, &bar_name(Panel::Layers)).unwrap().center();
    drag(&mut h, bar, bar + vec2(0.0, 60.0), None);
    let after = slot(&h, Panel::Layers).body.unwrap().height();
    assert!((after - layers - 60.0).abs() < 2.0, "{layers} → {after}");
    let below = slot(&h, Panel::Toolbox).body.unwrap().height();
    assert!((toolbox - below - 60.0).abs() < 2.0);
    // The stored heights hold the change, so the file keeps it.
    let stored = dock_of(&h, Workspace::Globe).panels();
    assert!((stored[0].height - after).abs() < 2.0);

    // Each panel stays at least the smallest height.
    let bar = control(&h, &bar_name(Panel::Layers)).unwrap().center();
    drag(&mut h, bar, bar + vec2(0.0, 900.0), None);
    let below = slot(&h, Panel::Toolbox).body.unwrap().height();
    assert!((below - HEIGHT_MIN).abs() < 1.0, "{below}");
}

#[test]
fn the_edge_handle_changes_the_width_from_280_to_480_points() {
    let mut h = docked_pad(&[Panel::Layers]);
    let at = handle(&h);
    drag(&mut h, at, at - vec2(60.0, 0.0), None);
    assert_eq!(dock_of(&h, Workspace::Globe).width, WIDTH_DEFAULT + 60.0);
    assert_eq!(rects(&h).dock.unwrap().width(), WIDTH_DEFAULT + 60.0);
    let at = handle(&h);
    drag(&mut h, at, at - vec2(400.0, 0.0), None);
    assert_eq!(dock_of(&h, Workspace::Globe).width, WIDTH_MAX);
    // A drag past 280 points by less than 40 points keeps the dock open.
    let at = handle(&h);
    drag(
        &mut h,
        at,
        at + vec2(WIDTH_MAX - WIDTH_MIN + 30.0, 0.0),
        None,
    );
    assert!(dock_of(&h, Workspace::Globe).open);
    assert_eq!(dock_of(&h, Workspace::Globe).width, WIDTH_MIN);
    assert_eq!(rects(&h).dock.unwrap().width(), WIDTH_MIN);
}

#[test]
fn the_dock_keeps_its_panels_while_it_is_closed() {
    let mut h = docked_pad(&[Panel::Layers, Panel::Toolbox]);
    tap(&mut h, &title_name(Panel::Toolbox));
    let at = handle(&h);
    drag(&mut h, at, at - vec2(50.0, 0.0), None);
    let before = dock_of(&h, Workspace::Globe).clone();
    tap(&mut h, DOCK);
    assert!(h.state().pad.dock_frame.slots.is_empty());
    assert!(!h.state().pad.is_open(Panel::Layers));
    assert!(control(&h, &title_name(Panel::Layers)).is_none());
    // A row of the Panels menu shows a floating card, and the dock keeps its
    // place.
    show(&mut h, "Layers");
    assert_eq!(h.state().pad.float, Some(Panel::Layers));
    show(&mut h, "Layers");
    tap(&mut h, DOCK);
    let mut after = dock_of(&h, Workspace::Globe).clone();
    after.open = before.open;
    assert_eq!(after, before);
    assert_eq!(docked(&h), [(Panel::Layers, false), (Panel::Toolbox, true)]);
}

#[test]
fn a_closed_dock_shows_one_floating_card_that_stays_open_while_painting() {
    let mut h = pad(Workspace::Globe, BOARDS[0]);
    show(&mut h, "Layers");
    assert_eq!(h.state().pad.float, Some(Panel::Layers));
    show(&mut h, select::ATTRIBUTES);
    assert_eq!(h.state().pad.float, Some(Panel::Attributes));
    assert!(!h.state().pad.is_open(Panel::Layers));
    let panels: Vec<_> = h.state().pad.panels.iter().map(|(p, _)| *p).collect();
    assert_eq!(panels, [Panel::Attributes]);

    // The card is 12 points from the right edge and 8 below the bar.
    let r = rects(&h);
    let card = h.state().pad.panels[0].1;
    assert_eq!(card.right(), r.screen.right() - r.bands.right - 12.0);
    assert!(card.top() >= r.bar.bottom() + 8.0);

    // A pen stroke on the canvas leaves the card open.
    let c = r.fit.center();
    drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.5));
    assert_eq!(h.state().pad.float, Some(Panel::Attributes));
    // Its row in the Panels menu closes it.
    show(&mut h, select::ATTRIBUTES);
    assert_eq!(h.state().pad.float, None);
}

#[test]
fn the_dock_button_of_a_card_moves_the_panel_into_the_dock() {
    let mut h = pad(Workspace::Globe, BOARDS[0]);
    show(&mut h, "Layers");
    tap(&mut h, &dock_name("Layers"));
    assert!(dock_of(&h, Workspace::Globe).open);
    assert_eq!(docked(&h), [(Panel::Layers, false)]);
    assert_eq!(h.state().pad.float, None);
    assert!(slot(&h, Panel::Layers).body.is_some());
    assert!(control(&h, &dock_name("Layers")).is_none());
}

#[test]
fn below_1000_points_each_panel_floats_and_no_dock_button_shows() {
    for size in [BOARDS[1], BOARDS[2]] {
        let mut state = state(Workspace::Globe);
        let entries = [Entry::new(Panel::Toolbox)];
        vale_app::dock::apply(&mut state.pad.docks, Workspace::Globe, &entries);
        let mut h = pad_with(state, size);
        assert!(rects(&h).dock.is_none(), "{size:?}");
        assert!(h.state().pad.dock_frame.slots.is_empty());
        assert!(!h.state().pad.is_open(Panel::Toolbox));
        show(&mut h, "Layers");
        assert_eq!(h.state().pad.float, Some(Panel::Layers), "{size:?}");
        assert!(control(&h, &dock_name("Layers")).is_none(), "{size:?}");
        assert!(control(&h, "Close Layers").is_some(), "{size:?}");
        assert!(control(&h, DOCK).is_none());
    }
}

#[test]
fn the_globe_fits_beside_the_dock_and_below_the_view_control() {
    for panels in [&[][..], &[Panel::Layers, Panel::Attributes][..]] {
        let h = docked_pad(panels);
        let r = rects(&h);
        let globe = h.state().globe.rect;
        assert_eq!(globe, r.fit);
        assert!(r.canvas.contains_rect(globe));
        assert!(globe.top() >= r.view.unwrap().bottom());
        let dock = r.dock.unwrap();
        assert!(globe.right() <= dock.left());
        assert!((globe.center().x - r.canvas.center().x).abs() < 0.5);
        // At the start, the circle of the globe is inside the fit.
        let radius = h.state().globe.radius() as f32;
        let circle = Rect::from_center_size(globe.center(), vec2(2.0, 2.0) * radius);
        assert!(r.fit.contains_rect(circle), "{circle:?} {:?}", r.fit);
        assert!(circle.right() <= dock.left());
    }
}

#[test]
fn the_page_fits_beside_the_dock() {
    let mut h = pad(Workspace::Atlas, BOARDS[0]);
    let before = h.state().atlas.place.unwrap().rect;
    tap(&mut h, DOCK);
    let r = rects(&h);
    let dock = r.dock.unwrap();
    let page = h.state().atlas.place.unwrap().rect;
    assert!(page.right() <= dock.left(), "{page:?} {dock:?}");
    assert!(page.left() >= r.tools.right());
    assert!(page.top() >= r.view.unwrap().bottom());
    assert!(r.free.contains_rect(page));
    assert!(page.center().x < before.center().x);
}

#[test]
fn the_dock_moves_the_center_of_the_canvas_over_several_frames() {
    let mut state = state(Workspace::Globe);
    state.headless = false;
    let mut h = Harness::builder()
        .with_size(vec2(BOARDS[0].0, BOARDS[0].1))
        .with_pixels_per_point(1.0)
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state);
    h.run_steps(3);
    let start = rects(&h).canvas.center().x;
    h.state_mut().pad.toggle_dock();
    let mut centers = Vec::new();
    for _ in 0..40 {
        h.step();
        centers.push(rects(&h).canvas.center().x);
    }
    let end = WIDTH_DEFAULT / 2.0;
    let last = *centers.last().unwrap();
    assert!((start - last - end).abs() < 0.5, "{start} → {last}");
    let between = centers
        .iter()
        .filter(|&&x| x < start - 1.0 && x > last + 1.0)
        .count();
    assert!(between >= 2, "{centers:?}");
    assert!(centers.windows(2).all(|w| w[1] <= w[0] + 0.01));
    // The globe follows the canvas.
    h.run_steps(2);
    let globe = h.state().globe.rect.center().x;
    assert!((globe - last).abs() < 0.5);
}

#[test]
fn the_dock_flag_shows_the_docked_panels_with_folded_ones() {
    let args = vale_app::cli::Args::try_parse_from([
        "vale-app",
        "--layout",
        "pad",
        "--dock",
        "layers,toolbox:folded",
    ])
    .unwrap();
    let mut state = state(Workspace::Globe);
    vale_app::cli::apply_dock(&args, &mut state);
    let h = pad_with(state, BOARDS[0]);
    assert_eq!(docked(&h), [(Panel::Layers, false), (Panel::Toolbox, true)]);
    assert!(slot(&h, Panel::Layers).body.is_some());
    assert_eq!(slot(&h, Panel::Toolbox).body, None);
    assert!(control(&h, ADD_LAYER).is_some());
    let dock = rects(&h).dock.unwrap();
    assert_eq!(dock.width(), WIDTH_DEFAULT);
    // The folded Toolbox is at the bottom of the dock, as the open Layers
    // panel fills the rest.
    let bottom = rects(&h).screen.bottom() - rects(&h).bands.bottom;
    assert!((slot(&h, Panel::Toolbox).title.bottom() - bottom).abs() < 0.5);
}

#[test]
fn the_atlas_panels_go_into_the_dock() {
    let mut h = pad(Workspace::Atlas, BOARDS[0]);
    tap(&mut h, DOCK);
    show(&mut h, vale_app::ui::pad::PAGES);
    show(&mut h, select::ATTRIBUTES);
    let dock = dock_of(&h, Workspace::Atlas);
    let panels: Vec<_> = dock.panels().iter().map(|p| p.panel).collect();
    assert_eq!(panels, [Panel::Pages, Panel::Attributes]);
    assert!(slot(&h, Panel::Pages).body.is_some());
    assert!(slot(&h, Panel::Attributes).body.is_some());
    assert!(control(&h, &title_name(Panel::Pages)).is_some());
    assert!(control(&h, &title_name(Panel::Attributes)).is_some());
}

/// The rows of the open Panels menu from the top, and whether each is
/// checked.
fn menu_rows(h: &Pad) -> Vec<(String, bool)> {
    use egui_kittest::kittest::NodeT;
    let menu = h.state().pad.panels_rect.expect("the Panels menu");
    let mut rows: Vec<_> = h
        .state()
        .pad
        .controls
        .iter()
        // The rows fill the width of the menu.
        .filter(|c| menu.contains_rect(c.rect) && c.rect.width() > 0.8 * menu.width())
        .map(|c| (c.rect.top(), c.name.clone()))
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.into_iter()
        .map(|(_, name)| {
            let node = h
                .query_all_by_label(&name)
                .find(|n| menu.contains_rect(n.rect()))
                .unwrap_or_else(|| panic!("no node `{name}`"));
            let on = node.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True);
            (name, on)
        })
        .collect()
}

#[test]
fn the_panels_menu_lists_the_panels_of_each_workspace_with_a_tick_on_the_open_ones() {
    use vale_app::ui::pad::panel_face;
    for workspace in WORKSPACES {
        let mut h = pad(workspace, BOARDS[0]);
        let button = control(&h, PANELS).unwrap();
        tap(&mut h, PANELS);
        assert!(h.state().pad.panels_menu);
        let menu = h.state().pad.panels_rect.unwrap();
        assert_eq!(menu.right(), button.right(), "{workspace:?}");
        assert!(menu.top() > button.bottom(), "{workspace:?}");
        let names: Vec<String> = Panel::of(workspace)
            .iter()
            .map(|&p| panel_face(p).0.to_owned())
            .collect();
        let rows = menu_rows(&h);
        let shown: Vec<String> = rows.iter().map(|(n, _)| n.clone()).collect();
        assert_eq!(shown, names, "{workspace:?}");
        assert!(rows.iter().all(|(_, on)| !on), "{workspace:?}");

        // A tap on a row opens the panel and closes the menu. The row of the
        // open panel has the tick.
        // The button closes the menu.
        tap(&mut h, PANELS);
        assert!(!h.state().pad.panels_menu, "{workspace:?}");
        let first = Panel::of(workspace)[0];
        show(&mut h, &names[0]);
        assert!(!h.state().pad.panels_menu, "{workspace:?}");
        assert_eq!(h.state().pad.float, Some(first), "{workspace:?}");
        tap(&mut h, PANELS);
        let rows = menu_rows(&h);
        assert_eq!(rows[0], (names[0].clone(), true), "{workspace:?}");
        assert!(rows[1..].iter().all(|(_, on)| !on), "{workspace:?}");
        // A tap outside the menu closes it.
        let r = rects(&h);
        let outside = r.fit.left_bottom() + vec2(200.0, -100.0);
        tap_touch(&mut h, egui::TouchPhase::Start, outside, None);
        h.step();
        tap_touch(&mut h, egui::TouchPhase::End, outside, None);
        h.run_steps(3);
        assert!(!h.state().pad.panels_menu, "{workspace:?}");
    }
}

#[test]
fn a_row_of_the_panels_menu_toggles_a_floating_card_or_a_docked_panel() {
    for workspace in WORKSPACES {
        let [first, second] = [Panel::of(workspace)[0], Panel::of(workspace)[1]];
        let names = [first, second].map(|p| vale_app::ui::pad::panel_face(p).0);
        // With the dock closed, there is one floating card.
        let mut h = pad(workspace, BOARDS[0]);
        show(&mut h, names[0]);
        assert_eq!(h.state().pad.float, Some(first), "{workspace:?}");
        show(&mut h, names[1]);
        assert_eq!(h.state().pad.float, Some(second), "{workspace:?}");
        show(&mut h, names[1]);
        assert_eq!(h.state().pad.float, None, "{workspace:?}");

        // With the dock open, the rows add and remove panels of the dock.
        tap(&mut h, DOCK);
        show(&mut h, names[1]);
        show(&mut h, names[0]);
        let docked = |h: &Pad| -> Vec<_> {
            let dock = dock_of(h, workspace);
            dock.panels().iter().map(|p| (p.panel, p.folded)).collect()
        };
        assert_eq!(docked(&h), [(second, false), (first, false)]);
        assert_eq!(h.state().pad.float, None);
        show(&mut h, names[1]);
        assert_eq!(docked(&h), [(first, false)], "{workspace:?}");

        // Below 1,000 points, the panel floats.
        let mut h = pad(workspace, BOARDS[2]);
        show(&mut h, names[0]);
        assert_eq!(h.state().pad.float, Some(first), "{workspace:?}");
        assert!(h.state().pad.dock_frame.slots.is_empty());
    }
}

/// The panels of each group of the dock of the World workspace.
fn groups(h: &Pad) -> Vec<Vec<Panel>> {
    let dock = dock_of(h, Workspace::Globe);
    dock.groups.iter().map(|g| g.panels.clone()).collect()
}

/// The World workspace on a landscape iPad with the dock open and these
/// groups, each in the syntax of the flag `--dock`.
fn stacked_pad(entries: &[&str]) -> Pad {
    let mut state = state(Workspace::Globe);
    let entries: Vec<Entry> = entries.iter().map(|e| e.parse().unwrap()).collect();
    vale_app::dock::apply(&mut state.pad.docks, Workspace::Globe, &entries);
    pad_with(state, BOARDS[0])
}

fn title(h: &Pad, panel: Panel) -> Pos2 {
    control(h, &title_name(panel))
        .unwrap_or_else(|| panic!("no title of {panel:?}"))
        .center()
}

#[test]
fn a_drag_of_a_title_row_between_two_groups_moves_the_panel() {
    let mut h = docked_pad(&[Panel::Layers, Panel::Attributes, Panel::Toolbox]);
    tap(&mut h, &title_name(Panel::Toolbox));
    let from = title(&h, Panel::Toolbox);
    let to = pos2(from.x, slot(&h, Panel::Attributes).title.top() - 4.0);
    drag(&mut h, from, to, None);
    assert_eq!(
        groups(&h),
        [
            vec![Panel::Layers],
            vec![Panel::Toolbox],
            vec![Panel::Attributes]
        ]
    );
    assert_eq!(
        docked(&h),
        [
            (Panel::Layers, false),
            (Panel::Toolbox, true),
            (Panel::Attributes, false),
        ],
        "the group keeps its fold"
    );
    let tops = [Panel::Layers, Panel::Toolbox, Panel::Attributes].map(|p| slot(&h, p).title.top());
    assert!(tops.is_sorted(), "{tops:?}");
    assert!(h.state().pad.dock_frame.drag.is_none());
}

#[test]
fn a_drop_on_a_title_row_stacks_the_panel_as_a_tab() {
    let mut h = docked_pad(&[Panel::Layers, Panel::Attributes, Panel::Toolbox]);
    let (from, to) = (title(&h, Panel::Toolbox), title(&h, Panel::Layers));
    drag(&mut h, from, to, None);
    assert_eq!(
        groups(&h),
        [vec![Panel::Layers, Panel::Toolbox], vec![Panel::Attributes]]
    );
    // The new tab is active, and each panel of the stack has a tab.
    let toolbox = slot(&h, Panel::Toolbox);
    assert_eq!((toolbox.group, toolbox.tabs), (0, 2));
    assert!(toolbox.body.is_some());
    assert!(h.state().pad.dock_frame.slot(Panel::Layers).is_none());
    let [layers, toolbox] =
        [Panel::Layers, Panel::Toolbox].map(|p| control(&h, &title_name(p)).unwrap());
    assert!(
        layers.right() <= toolbox.left() + 0.5,
        "{layers:?} {toolbox:?}"
    );
    assert!(layers.width() >= 44.0 && layers.height() >= 44.0);
    assert!(h.state().pad.is_open(Panel::Layers));
    assert!(!h.state().pad.shows(Panel::Layers));
    assert!(
        control(&h, ADD_LAYER).is_none(),
        "the body of Layers is behind its tab"
    );
}

#[test]
fn a_tap_on_a_tab_shows_its_panel_and_a_tap_on_the_active_tab_folds_the_stack() {
    let mut h = stacked_pad(&["layers+toolbox", "attributes"]);
    assert!(control(&h, ADD_LAYER).is_some());
    tap(&mut h, &title_name(Panel::Toolbox));
    assert!(dock_of(&h, Workspace::Globe).is_active(Panel::Toolbox));
    assert!(control(&h, ADD_LAYER).is_none());
    assert!(slot(&h, Panel::Toolbox).body.is_some());
    tap(&mut h, &title_name(Panel::Layers));
    assert!(control(&h, ADD_LAYER).is_some(), "the + of Layers");
    tap(&mut h, &title_name(Panel::Layers));
    assert_eq!(slot(&h, Panel::Layers).body, None, "folded");
    assert!(control(&h, ADD_LAYER).is_none());
    // A tap on a tab of a folded stack opens it.
    tap(&mut h, &title_name(Panel::Toolbox));
    assert!(slot(&h, Panel::Toolbox).body.is_some());
}

#[test]
fn a_drag_of_a_tab_out_of_its_stack_makes_a_group() {
    let mut h = stacked_pad(&["layers+toolbox", "attributes"]);
    let from = title(&h, Panel::Toolbox);
    let below = slot(&h, Panel::Attributes).rect().bottom() - 10.0;
    drag(&mut h, from, pos2(from.x, below), None);
    assert_eq!(
        groups(&h),
        [
            vec![Panel::Layers],
            vec![Panel::Attributes],
            vec![Panel::Toolbox]
        ]
    );
    assert!(
        slot(&h, Panel::Layers).body.is_some(),
        "a plain group again"
    );
    assert_eq!(slot(&h, Panel::Layers).tabs, 1);
    assert!(slot(&h, Panel::Toolbox).body.is_some());
}

#[test]
fn a_drop_outside_the_dock_changes_nothing() {
    let mut h = stacked_pad(&["layers+toolbox", "attributes"]);
    let before = dock_of(&h, Workspace::Globe).clone();
    let canvas = rects(&h).fit.center();
    for panel in [Panel::Toolbox, Panel::Attributes] {
        let from = title(&h, panel);
        drag(&mut h, from, canvas, None);
        assert_eq!(*dock_of(&h, Workspace::Globe), before, "{panel:?}");
    }
    assert!(h.state().pad.dock_frame.drag.is_none());
}

#[test]
fn a_hold_on_a_title_row_lifts_the_panel() {
    let mut h = docked_pad(&[Panel::Layers, Panel::Toolbox]);
    let at = title(&h, Panel::Toolbox);
    touch(&mut h, egui::TouchPhase::Start, at);
    h.step();
    assert!(h.state().pad.dock_frame.drag.is_none(), "not at once");
    for _ in 0..40 {
        h.step();
    }
    let drag = h
        .state()
        .pad
        .dock_frame
        .drag
        .expect("a hold lifts the panel");
    assert_eq!(drag.panel, Panel::Toolbox);
    touch(&mut h, egui::TouchPhase::End, at);
    h.run_steps(3);
    // The release after the hold is not a tap.
    assert_eq!(
        docked(&h),
        [(Panel::Layers, false), (Panel::Toolbox, false)]
    );
    assert!(h.state().pad.dock_frame.drag.is_none());
}

#[test]
fn the_dock_keeps_its_stacks_after_a_close_and_a_restart() {
    let mut h = docked_pad(&[Panel::Layers, Panel::Attributes, Panel::Toolbox]);
    let (from, to) = (title(&h, Panel::Layers), title(&h, Panel::Toolbox));
    drag(&mut h, from, to, None);
    tap(&mut h, &title_name(Panel::Toolbox));
    tap(&mut h, &title_name(Panel::Attributes));
    let before = dock_of(&h, Workspace::Globe).clone();
    assert_eq!(
        groups(&h),
        [vec![Panel::Attributes], vec![Panel::Toolbox, Panel::Layers]]
    );
    tap(&mut h, DOCK);
    assert!(h.state().pad.dock_frame.slots.is_empty());
    tap(&mut h, DOCK);
    assert_eq!(*dock_of(&h, Workspace::Globe), before);
    assert!(slot(&h, Panel::Toolbox).body.is_some());

    // The file of the docks holds the stacks.
    let text = h.state().pad.docks.text();
    let mut state = state(Workspace::Globe);
    state.pad.docks = vale_app::dock::Docks::parse(&text);
    let h = pad_with(state, BOARDS[0]);
    assert_eq!(*dock_of(&h, Workspace::Globe), before);
    assert_eq!(slot(&h, Panel::Toolbox).tabs, 2);
    assert_eq!(slot(&h, Panel::Attributes).body, None);
}

#[test]
fn the_panels_menu_takes_a_panel_out_of_its_stack_and_adds_it_below() {
    let mut h = stacked_pad(&["layers+toolbox+attributes"]);
    show(&mut h, TOOLBOX);
    assert_eq!(groups(&h), [vec![Panel::Layers, Panel::Attributes]]);
    show(&mut h, select::ATTRIBUTES);
    assert_eq!(groups(&h), [vec![Panel::Layers]]);
    assert_eq!(slot(&h, Panel::Layers).tabs, 1);
    show(&mut h, TOOLBOX);
    assert_eq!(groups(&h), [vec![Panel::Layers], vec![Panel::Toolbox]]);
}

#[test]
fn the_dock_flag_shows_a_stack() {
    let args = vale_app::cli::Args::try_parse_from([
        "vale-app",
        "--layout",
        "pad",
        "--dock",
        "layers+attributes,toolbox:folded",
    ])
    .unwrap();
    let mut state = state(Workspace::Globe);
    vale_app::cli::apply_dock(&args, &mut state);
    let h = pad_with(state, BOARDS[0]);
    assert_eq!(
        groups(&h),
        [vec![Panel::Layers, Panel::Attributes], vec![Panel::Toolbox]]
    );
    assert_eq!(slot(&h, Panel::Layers).tabs, 2);
    assert!(control(&h, ADD_LAYER).is_some());
    assert!(control(&h, &title_name(Panel::Attributes)).is_some());
}

#[test]
#[ignore = "gpu"]
fn manual_a_panel_moves_in_the_dock() {
    let mut h = stacked_pad(&["layers+toolbox", "attributes"]);
    let from = title(&h, Panel::Toolbox);
    // The finger stays down, so the card of the panel and the mark of its
    // place show.
    let to_pic = |h: &mut Pad, to: Pos2, name: &str| {
        touch(h, egui::TouchPhase::Start, from);
        h.step();
        for i in 1..=6 {
            touch(h, egui::TouchPhase::Move, from.lerp(to, i as f32 / 6.0));
            h.step();
        }
        h.run_steps(2);
        screenshot::save(h, name);
        touch(h, egui::TouchPhase::End, rects(h).fit.center());
        h.run_steps(3);
    };
    let between = slot(&h, Panel::Attributes).rect().bottom() - 10.0;
    to_pic(&mut h, pos2(from.x, between), "dock-drag-between");
    let onto = title(&h, Panel::Attributes);
    to_pic(&mut h, onto, "dock-drag-stack");
}
