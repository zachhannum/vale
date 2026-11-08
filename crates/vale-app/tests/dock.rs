//! The top bar, the workspace tabs, and the view control of the iPad layout
//! from 600 points.

use eframe::egui::{self, Pos2, Rect, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use vale_app::document::Document;
use vale_app::globe::WorldView;
use vale_app::projects::Locations;
use vale_app::session::Session;
use vale_app::ui::pad::atlas::{NEXT_PAGE, PREVIOUS_PAGE, page_size_text};
use vale_app::ui::pad::bar::{BACK, DOCK};
use vale_app::ui::pad::geometry::{BAR, TAB_ICON};
use vale_app::ui::pad::{
    GRATICULE, PROJECTION, Panel, RECENTER, RESET, SAVE, TOOLBOX, WORLD_PROPERTIES, WORLDS,
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

fn tap(h: &mut Pad, name: &str) {
    let rect = control(h, name).unwrap_or_else(|| panic!("no control `{name}`"));
    touch(h, egui::TouchPhase::Start, rect.center());
    h.step();
    touch(h, egui::TouchPhase::End, rect.center());
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
            // One opaque area covers the band and the bar.
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
            // Back, the name, Undo, and Redo are at the left in this order.
            let lefts = [BACK, WORLDS, "Undo", "Redo"].map(|n| widget(&h, n).left());
            assert!(lefts.is_sorted(), "{what}: {lefts:?}");
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
            // A second tap closes the menu.
            tap(&mut h, WORLDS);
            assert!(h.state().pad.worlds.is_none(), "{what}");
        }
    }
}

#[test]
fn the_tabs_switch_the_workspace_and_show_names_from_1000_points() {
    for size in BOARDS {
        let mut h = pad(Workspace::Globe, size);
        let wide = size.0 >= 1000.0;
        assert_eq!(rects(&h).tab_names, wide, "{size:?}");
        for name in ["World", "Atlas"] {
            let tab = control(&h, name).unwrap();
            if wide {
                assert!(tab.width() > TAB_ICON, "{size:?}: {name}");
            } else {
                assert_eq!(tab.width(), TAB_ICON, "{size:?}: {name}");
            }
        }
        tap(&mut h, "Atlas");
        assert_eq!(h.state().workspace, Workspace::Atlas, "{size:?}");
        tap(&mut h, "World");
        assert_eq!(h.state().workspace, Workspace::Globe, "{size:?}");
    }
}

#[test]
fn the_tabs_and_the_view_control_share_the_same_center() {
    for workspace in WORKSPACES {
        for open in [false, true] {
            let mut state = state(workspace);
            state.pad.docks.get_mut(workspace).open = open;
            let h = pad_with(state, BOARDS[0]);
            let what = format!("{workspace:?} open {open}");
            let tabs = control(&h, "World")
                .unwrap()
                .union(control(&h, "Atlas").unwrap());
            let r = rects(&h);
            let view = r.view.unwrap();
            assert_eq!(tabs.center().x, view.center().x, "{what}");
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
fn the_view_control_of_the_atlas_holds_the_page_controls_and_the_size() {
    for size in BOARDS {
        let h = pad(Workspace::Atlas, size);
        let view = rects(&h).view.unwrap();
        for name in [PREVIOUS_PAGE, NEXT_PAGE] {
            assert!(view.contains_rect(widget(&h, name)), "{size:?}: {name}");
        }
        assert_eq!(page_size_text(h.state()), "A4 landscape");
        // The card has room for the name of the page and its size.
        assert!(view.width() > 2.0 * 44.0 + 150.0, "{size:?}: {view:?}");
        assert!(control(&h, GRATICULE).is_none(), "{size:?}");
    }
}

#[test]
fn the_dock_zone_holds_the_panel_buttons_and_the_dock_button_from_1000_points() {
    for size in BOARDS {
        let mut h = pad(Workspace::Globe, size);
        let r = rects(&h);
        let names = ["Layers", select::ATTRIBUTES, TOOLBOX];
        let lefts = names.map(|name| {
            let rect = control(&h, name).unwrap_or_else(|| panic!("{size:?}: no {name}"));
            assert!(r.dock_zone.contains_rect(rect), "{size:?}: {name}");
            rect.left()
        });
        assert!(lefts.is_sorted(), "{size:?}");
        // The workspace menu is gone.
        assert!(control(&h, "Workspace").is_none(), "{size:?}");
        if size.0 < 1000.0 {
            assert!(control(&h, DOCK).is_none(), "{size:?}");
            continue;
        }
        let dock = control(&h, DOCK).unwrap();
        assert!(lefts.iter().all(|&left| left < dock.left()), "{size:?}");
        tap(&mut h, DOCK);
        assert!(h.state().pad.docks.get(Workspace::Globe).open);
        assert!(rects(&h).dock.is_some());
        tap(&mut h, DOCK);
        assert!(!h.state().pad.docks.get(Workspace::Globe).open);

        // The Toolbox button opens its panel.
        tap(&mut h, TOOLBOX);
        assert!(h.state().pad.is_open(Panel::Toolbox), "{size:?}");
    }
}
