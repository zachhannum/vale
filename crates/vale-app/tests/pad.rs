//! The iPad layout of the world workspace.

use eframe::egui::{self, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use vale_app::document::Document;
use vale_app::globe::view::GlobeView;
use vale_app::globe::{Tool, WorldView};
use vale_app::headless;
use vale_app::session::Session;
use vale_app::ui::pad::geometry::{BrushCard, WidthClass};
use vale_app::ui::pad::widgets::SLIDER_INSET;
use vale_app::ui::pad::{
    IMPORT_HEIGHTMAP, MORE, Panel, RECENTER, RESET, WORLD_PROPERTIES, WORLDS, WORLDS_ROW,
    brush_size_text, readout_text, theme,
};
use vale_app::ui::world_form::{CREATE, FormMode, NAME, NEW_WORLD, RADIUS_SLIDER, WorldForm};
use vale_app::ui::{Action, AppState, Layout, Workspace, draw};
use vale_app::world::{radius_fraction, three_significant};
use vale_store::{Units, World};
use vale_terrain::{Mode, meters_to_level};

type Pad = Harness<'static, AppState>;

/// The boards of the mockup: landscape, portrait, half split view, and
/// narrow split view.
const BOARDS: [(f32, f32); 4] = [
    (1194.0, 834.0),
    (834.0, 1194.0),
    (597.0, 834.0),
    (375.0, 834.0),
];
/// The bands that the system uses at the top and at the bottom.
const BANDS: (f32, f32) = (24.0, 20.0);

fn state() -> AppState {
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    state.set_layout(Layout::Pad);
    state
}

fn pad((width, height): (f32, f32)) -> Pad {
    let mut h = Harness::builder()
        .with_size(vec2(width, height))
        .with_pixels_per_point(1.0)
        .build_ui_state(|ui, state: &mut AppState| draw(ui, state), state());
    h.run_steps(3);
    h
}

/// The layout of a world with a painted heightmap, which is a raster layer.
fn painted_pad(size: (f32, f32)) -> Pad {
    let mut h = pad(size);
    let globe = &mut h.state_mut().globe;
    globe.map.set(0, 0, 0, meters_to_level(100.0));
    assert!(h.state().has_raster());
    h.run_steps(2);
    h
}

fn control(h: &Pad, name: &str) -> Option<Rect> {
    let controls = &h.state().pad.controls;
    controls.iter().find(|c| c.name == name).map(|c| c.rect)
}

fn panel(h: &Pad, panel: Panel) -> Option<Rect> {
    let panels = &h.state().pad.panels;
    panels.iter().find(|(p, _)| *p == panel).map(|(_, r)| *r)
}

/// The first touch is also the pointer. A touch with a force is a pen.
fn touch(h: &mut Pad, phase: egui::TouchPhase, pos: Pos2, force: Option<f32>) {
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(1),
        phase,
        pos,
        force,
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

/// A touch that moves from one place to another. A force makes it a pen.
fn drag(h: &mut Pad, from: Pos2, to: Pos2, force: Option<f32>) {
    touch(h, egui::TouchPhase::Start, from, force);
    h.step();
    for i in 1..=6 {
        touch(
            h,
            egui::TouchPhase::Move,
            from.lerp(to, i as f32 / 6.0),
            force,
        );
        h.step();
    }
    touch(h, egui::TouchPhase::End, to, force);
    h.run_steps(3);
}

/// A tap of a finger on the middle of a control.
fn tap(h: &mut Pad, name: &str) {
    let rect = control(h, name).unwrap_or_else(|| panic!("no control `{name}`"));
    let pos = rect.center();
    touch(h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
}

/// The rectangles of all cards on the screen.
fn cards(h: &Pad) -> Vec<Rect> {
    let pad = &h.state().pad;
    let r = pad.rects.unwrap();
    let mut cards = vec![r.workspace, r.actions, r.tools];
    cards.extend(r.view);
    cards.extend(r.brush);
    cards.extend(pad.panels.iter().map(|(_, rect)| *rect));
    cards.extend(pad.menu_rect);
    cards.extend(pad.more_rect);
    cards
}

#[test]
fn the_canvas_fills_the_screen_and_the_panels_float_over_it() {
    for size in BOARDS {
        let mut h = pad(size);
        let screen = Rect::from_min_size(Pos2::ZERO, size.into());
        assert_eq!(h.state().globe.rect, screen, "{size:?}");
        tap(&mut h, "Layers");
        if !h.state().pad.rects.unwrap().one_panel {
            tap(&mut h, "Brush settings");
        }
        assert!(!h.state().pad.panels.is_empty());
        assert_eq!(h.state().globe.rect, screen, "{size:?}");
        for card in cards(&h) {
            assert!(screen.contains_rect(card), "{size:?}: {card:?}");
        }
    }
}

/// Makes sure that a finger can hit each control of the frame.
fn check_controls(h: &Pad, what: &str) {
    let screen = h.state().pad.rects.unwrap().screen;
    let controls = &h.state().pad.controls;
    assert!(controls.len() >= 8, "{what}: {} controls", controls.len());
    let in_menu = h.state().pad.menu_rect.is_some();
    let in_more = h.state().pad.more_rect.is_some();
    let popup = in_menu || in_more || h.state().pad.projections_rect.is_some();
    for (i, a) in controls.iter().enumerate() {
        let (name, rect) = (&a.name, a.rect);
        assert!(
            rect.width() >= 44.0 && rect.height() >= 44.0,
            "{what}: {name} is {rect:?}"
        );
        assert!(rect.top() >= BANDS.0, "{what}: {name} is in the top band");
        assert!(
            rect.bottom() <= screen.bottom() - BANDS.1,
            "{what}: {name} is in the bottom band"
        );
        assert!(
            rect.left() >= 0.0 && rect.right() <= screen.right(),
            "{what}: {name}"
        );
        for b in &controls[i + 1..] {
            assert_ne!(a.name, b.name, "{what}");
            // The open menu, the open list of projections, and the open menu
            // of the More cell cover the cards below them.
            let row = |c: &vale_app::ui::pad::widgets::Control| {
                let name = c.name.as_str();
                let kind = vale_sphere::ProjectionKind::ALL
                    .iter()
                    .any(|k| k.name() == name);
                let menu = ["World", "Globe", "Flat", "Maps", WORLDS_ROW].contains(&name);
                let more =
                    in_more && ["Carve", "Erode", "Weather", "Uplift", "Warp"].contains(&name);
                menu || kind || more || (in_menu && name == "Projection")
            };
            let covered = popup && row(a) != row(b);
            let free = !a.rect.shrink(0.5).intersects(b.rect.shrink(0.5));
            assert!(free || covered, "{what}: {name} and {} overlap", b.name);
        }
    }
}

#[test]
fn each_control_is_large_and_is_outside_the_system_bands() {
    for size in BOARDS {
        let mut h = painted_pad(size);
        check_controls(&h, &format!("{size:?}"));
        for name in ["Layers", "Open Height", "Toolbox", "Brush settings"] {
            if control(&h, name).is_none() {
                // An open sheet covers the Brush button.
                assert_eq!(name, "Brush settings");
                tap(&mut h, "Close Toolbox");
            }
            tap(&mut h, name);
            check_controls(&h, &format!("{size:?} after {name}"));
        }
        tap(&mut h, MORE);
        assert!(h.state().pad.more);
        for name in [MORE, "Carve", "Erode", "Weather", "Uplift", "Warp"] {
            assert!(control(&h, name).is_some(), "{size:?}: {name}");
        }
        check_controls(&h, &format!("{size:?} with the More menu"));
        tap(&mut h, MORE);
        if control(&h, "Workspace").is_some() {
            tap(&mut h, "Workspace");
            assert!(h.state().pad.menu);
            check_controls(&h, &format!("{size:?} with the menu"));
        }
    }
}

/// Writes a 16-bit greyscale PNG with a slope from west to east.
fn heightmap_file(name: &str, width: u32, height: u32) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/app/test-import");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let level = |x: u32, _| image::Luma([(x * 65535 / width) as u16]);
    let image = image::ImageBuffer::<image::Luma<u16>, _>::from_fn(width, height, level);
    image.save(&path).unwrap();
    path
}

#[test]
fn the_height_panel_asks_for_a_heightmap_file() {
    for size in BOARDS {
        let mut h = painted_pad(size);
        // iOS has no file dialog of the desktop.
        h.state_mut().file_buttons = false;
        tap(&mut h, "Layers");
        tap(&mut h, "Open Height");
        check_controls(&h, &format!("{size:?}"));
        assert!(h.state().actions.is_empty());
        tap(&mut h, IMPORT_HEIGHTMAP);
        let actions = &h.state().actions;
        assert_eq!(actions, &[Action::ImportHeightmapDialog], "{size:?}");
    }
}

#[test]
fn the_height_panel_shows_the_warning_of_a_wrong_aspect_ratio() {
    let path = heightmap_file("pad-ratio.png", 300, 200);
    for size in BOARDS {
        // A world with no raster layer has the import row in the Layers
        // panel. The import makes the Height layer and opens its panel.
        let mut h = pad(size);
        tap(&mut h, "Layers");
        assert!(control(&h, "Open Height").is_none());
        tap(&mut h, IMPORT_HEIGHTMAP);
        assert_eq!(
            h.state_mut().actions.pop(),
            Some(Action::ImportHeightmapDialog)
        );
        assert!(h.query_by_label_contains("twice as wide").is_none());
        h.state_mut()
            .run_action(Action::ImportHeightmap(path.clone()));
        h.run_steps(3);
        assert!(h.state().import_note.as_ref().unwrap().warning);
        assert!(
            h.query_by_label_contains("twice as wide").is_some(),
            "{size:?}"
        );
        check_controls(&h, &format!("{size:?} with the warning"));
    }
}

#[test]
fn the_import_row_is_off_while_an_import_runs() {
    let path = heightmap_file("pad-busy.png", 512, 256);
    let mut h = painted_pad(BOARDS[0]);
    tap(&mut h, "Layers");
    tap(&mut h, "Open Height");
    assert!(control(&h, IMPORT_HEIGHTMAP).is_some());
    // The worker gives one face in each frame.
    h.state_mut().globe.start_import(path);
    h.run_steps(2);
    assert!(h.state().globe.importing());
    assert!(control(&h, IMPORT_HEIGHTMAP).is_none());
    assert!(
        h.query_by_label_contains("Importing pad-busy.png")
            .is_some()
    );
    let start = std::time::Instant::now();
    while h.state().globe.busy() {
        assert!(start.elapsed().as_secs() < 30, "the import did not end");
        h.step();
    }
    h.run_steps(2);
    assert!(control(&h, IMPORT_HEIGHTMAP).is_some());
    assert!(h.query_by_label_contains("Imported pad-busy.png").is_some());
    assert!(!h.state().import_note.as_ref().unwrap().warning);
}

#[test]
fn a_finger_hits_each_control_at_the_first_try() {
    let mut h = pad(BOARDS[0]);
    for (name, mode) in [
        ("Lower", Mode::Lower),
        ("Smooth", Mode::Smooth),
        ("Flatten", Mode::Flatten),
        ("Raise", Mode::Raise),
    ] {
        tap(&mut h, name);
        assert_eq!(h.state().globe.tool, Tool::Brush);
        assert_eq!(h.state().globe.brush.mode, mode);
    }
    tap(&mut h, MORE);
    tap(&mut h, "Carve");
    assert_eq!(h.state().globe.brush.mode, Mode::Carve);
    tap(&mut h, "Pan");
    assert_eq!(h.state().globe.tool, Tool::Navigate);

    for (name, which) in [
        ("Layers", Panel::Layers),
        ("Toolbox", Panel::Toolbox),
        ("Brush settings", Panel::Brush),
    ] {
        tap(&mut h, name);
        assert!(h.state().pad.is_open(which), "{name}");
    }
    assert!(!h.state().pad.is_open(Panel::Layers));

    // A touch at the top of a slider sets its largest value.
    h.state_mut().globe.brush.flow = 0.2;
    for (name, at) in [("Flow", 0.0), ("Brush hardness", 1.0)] {
        let rect = control(&h, name).unwrap();
        let (from, to) = if at == 0.0 {
            (rect.center(), rect.center_top())
        } else {
            (rect.center(), rect.right_center())
        };
        drag(&mut h, from, to, None);
    }
    assert_eq!(h.state().globe.brush.flow, 1.0);
    assert_eq!(h.state().globe.brush.hardness, 1.0);
    let size = h.state().globe.brush.size_points;
    let rect = control(&h, "Size").unwrap();
    drag(&mut h, rect.center(), rect.center_bottom(), None);
    assert!(h.state().globe.brush.size_points < size);

    tap(&mut h, "Maps");
    assert_eq!(h.state().workspace, Workspace::Map);
}

#[test]
fn the_brush_size_uses_the_radius_and_the_units_of_the_world() {
    let mut h = pad(BOARDS[0]);
    let km = |h: &Pad| {
        let globe = &h.state().globe;
        let world_km = h.state().doc.project.world.radius_km;
        globe.brush.radius_km(globe.radius(), world_km)
    };
    let earth = km(&h);
    assert_eq!(
        brush_size_text(h.state()),
        format!("{} km", three_significant(earth))
    );

    // The brush covers the same angle on a small world, and thus less ground.
    h.state_mut().doc.set_radius_km(200.0);
    h.run_steps(2);
    let small = km(&h);
    assert!((small - earth * 200.0 / 6371.0).abs() < 1e-9);
    assert_eq!(h.state().globe.world_km, 200.0);
    assert_eq!(
        brush_size_text(h.state()),
        format!("{} km", three_significant(small))
    );

    // The slider reaches the same angles on each world.
    let globe = &h.state().globe;
    let range = globe.brush.radius_km_range(globe.radius(), 200.0);
    let earth_range = globe.brush.radius_km_range(globe.radius(), 6371.0);
    assert!((range.end() / earth_range.end() - 200.0 / 6371.0).abs() < 1e-12);

    h.state_mut().doc.project.world.units = Units::Imperial;
    h.run_steps(2);
    let miles = small / 1.609_344;
    assert_eq!(
        brush_size_text(h.state()),
        format!("{} mi", three_significant(miles))
    );
}

/// True if the More cell has the active highlight.
fn more_is_active(h: &Pad) -> bool {
    let cell = h.get_by_label(MORE);
    cell.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn the_more_cell_opens_a_menu_with_the_modes_that_have_no_cell() {
    let mut h = pad(BOARDS[0]);
    let tools = h.state().pad.rects.unwrap().tools;
    let strip = |h: &Pad| -> Vec<String> {
        let controls = h.state().pad.controls.iter();
        let cells = controls.filter(|c| tools.contains_rect(c.rect));
        cells.map(|c| c.name.clone()).collect()
    };
    let cells = ["Raise", "Lower", "Smooth", "Flatten", MORE, "Pan"];
    assert_eq!(strip(&h), cells);
    assert!(!more_is_active(&h));
    for (name, mode) in [
        ("Warp", Mode::Warp),
        ("Uplift", Mode::Uplift),
        ("Weather", Mode::Weather),
        ("Erode", Mode::Erode),
        ("Carve", Mode::Carve),
    ] {
        // A row of the menu also picks the brush tool.
        tap(&mut h, "Pan");
        assert!(!more_is_active(&h));
        assert!(control(&h, name).is_none());
        tap(&mut h, MORE);
        assert!(h.state().pad.more);
        let menu = h.state().pad.more_rect.unwrap();
        let cell = control(&h, MORE).unwrap();
        assert_eq!(menu.left_top(), pos2(tools.right() + 12.0, cell.top()));
        for row in ["Carve", "Erode", "Weather", "Uplift", "Warp"] {
            assert!(menu.contains_rect(control(&h, row).unwrap()), "{row}");
        }
        assert_eq!(strip(&h), cells);

        tap(&mut h, name);
        let s = h.state();
        assert_eq!((s.globe.tool, s.globe.brush.mode), (Tool::Brush, mode));
        assert!(!s.pad.more && s.pad.more_rect.is_none());
        assert!(control(&h, name).is_none());
        assert!(more_is_active(&h));
        assert_eq!(strip(&h), cells);
    }
    // The cell of another mode takes the highlight.
    tap(&mut h, "Smooth");
    assert!(!more_is_active(&h));

    // The Brush panel has the angle in the Weather mode only.
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush angle").is_none());
    tap(&mut h, MORE);
    // The menu is in front of the Brush panel.
    let menu = h.state().pad.more_rect.unwrap();
    assert!(menu.intersects(panel(&h, Panel::Brush).unwrap()));
    tap(&mut h, "Weather");
    assert_eq!(h.state().globe.brush.mode, Mode::Weather);
    assert!(h.state().pad.is_open(Panel::Brush));
    let rect = control(&h, "Brush angle").unwrap();
    drag(&mut h, rect.center(), rect.right_center(), None);
    assert!((h.state().globe.brush.talus_deg - 20.0).abs() < 1e-9);
}

#[test]
fn the_brush_panel_has_the_two_sliders_of_the_uplift_mode() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush erosion").is_none());
    tap(&mut h, MORE);
    tap(&mut h, "Uplift");
    let s = h.state();
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Uplift)
    );
    assert!(more_is_active(&h));
    // The strength slider sets the rate of the uplift.
    let rect = control(&h, "Brush strength").unwrap();
    drag(&mut h, rect.center(), rect.right_center(), None);
    assert!((h.state().globe.brush.strength_m - 6000.0).abs() < 1e-6);
    let rect = control(&h, "Brush erosion").unwrap();
    drag(&mut h, rect.center(), rect.left_center(), None);
    assert_eq!(h.state().globe.brush.erosion, 0.0);
    drag(&mut h, rect.left_center(), rect.right_center(), None);
    assert_eq!(h.state().globe.brush.erosion, 1.0);
    assert!(control(&h, "Brush angle").is_some());
    check_controls(&h, "the Uplift mode");
}

/// Whether the switch with this name is on.
fn switch_is_on(h: &Pad, name: &str) -> bool {
    let switch = h.get_by_label(name);
    switch.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn the_brush_panel_has_the_floor_and_the_ceiling() {
    const SWITCHES: [&str; 2] = ["Brush floor", "Brush ceiling"];
    const LEVELS: [&str; 2] = ["Brush floor level", "Brush ceiling level"];
    for mode in [None, Some("Erode"), Some("Uplift"), Some("Warp")] {
        let mut h = pad(BOARDS[0]);
        tap(&mut h, "Brush settings");
        if let Some(mode) = mode {
            tap(&mut h, MORE);
            tap(&mut h, mode);
        }
        let what = format!("{mode:?}");
        // Both switches are off at the start, and no level shows.
        let brush = h.state().globe.brush;
        assert!(!brush.floor_on && !brush.ceiling_on, "{what}");
        assert_eq!((brush.floor_m, brush.ceiling_m), (0.0, 1000.0), "{what}");
        for (switch, level) in SWITCHES.iter().zip(LEVELS) {
            assert!(!switch_is_on(&h, switch), "{what}: {switch}");
            assert!(control(&h, level).is_none(), "{what}: {level}");
        }
        check_controls(&h, &format!("{what} with the limits off"));

        // The floor goes on alone, and its level shows.
        tap(&mut h, SWITCHES[0]);
        let brush = h.state().globe.brush;
        assert!(brush.floor_on && !brush.ceiling_on, "{what}");
        assert!(switch_is_on(&h, SWITCHES[0]) && !switch_is_on(&h, SWITCHES[1]));
        assert!(control(&h, LEVELS[0]).is_some() && control(&h, LEVELS[1]).is_none());
        // The switch and its level are two rows of the panel, and the track
        // of the level is as wide as the track of the size.
        let (switch, level) = (control(&h, SWITCHES[0]), control(&h, LEVELS[0]));
        let (switch, level) = (switch.unwrap(), level.unwrap());
        let size = control(&h, "Brush size").unwrap();
        assert_eq!(level.top(), switch.bottom(), "{what}");
        assert_eq!(switch.x_range(), size.x_range(), "{what}");
        assert_eq!(level.x_range(), size.x_range(), "{what}");

        tap(&mut h, SWITCHES[1]);
        let brush = h.state().globe.brush;
        assert!(brush.floor_on && brush.ceiling_on, "{what}");
        check_controls(&h, &format!("{what} with the limits on"));

        // A drag on a level sets the meters, in steps of 50 m.
        let rect = control(&h, LEVELS[0]).unwrap();
        drag(&mut h, rect.center(), rect.left_center(), None);
        assert_eq!(h.state().globe.brush.floor_m, -6000.0, "{what}");
        let to = rect.center() + vec2(-17.0, 0.0);
        drag(&mut h, rect.left_center(), to, None);
        let floor = h.state().globe.brush.floor_m;
        assert!(floor < 0.0 && floor > -3000.0, "{what}: {floor}");
        assert_eq!(floor % 50.0, 0.0, "{what}: {floor}");
        let rect = control(&h, LEVELS[1]).unwrap();
        drag(&mut h, rect.center(), rect.right_center(), None);
        assert_eq!(h.state().globe.brush.ceiling_m, 6000.0, "{what}");
        // The floor can be above the ceiling.
        drag(&mut h, rect.right_center(), rect.left_center(), None);
        let brush = h.state().globe.brush;
        assert_eq!(brush.ceiling_m, -6000.0, "{what}");
        assert!(brush.floor_m > brush.ceiling_m, "{what}");
        let limits = brush.limits();
        assert_eq!(limits.floor, meters_to_level(floor), "{what}");
        assert_eq!(limits.ceiling, 0, "{what}");

        // A tap turns a limit off, and its level keeps its meters.
        tap(&mut h, SWITCHES[0]);
        let brush = h.state().globe.brush;
        assert!(!brush.floor_on && brush.ceiling_on, "{what}");
        assert_eq!(brush.floor_m, floor, "{what}");
        assert!(control(&h, LEVELS[0]).is_none() && control(&h, LEVELS[1]).is_some());
        assert_eq!(brush.limits().floor, 0, "{what}");
    }

    // The mode with the most rows, with both levels, on each board.
    for size in BOARDS {
        let mut h = pad(size);
        let brush = &mut h.state_mut().globe.brush;
        (brush.mode, brush.floor_on, brush.ceiling_on) = (Mode::Uplift, true, true);
        tap(&mut h, "Brush settings");
        h.run_steps(2);
        check_controls(&h, &format!("{size:?} with the limits on"));
        let shown = |h: &Pad| {
            let names = SWITCHES.iter().chain(&LEVELS);
            names.filter(|name| control(h, name).is_some()).count()
        };
        if h.state().pad.rects.unwrap().sheet.is_some() {
            // The sheet is too short for the rows. A finger that moves up
            // on a slider scrolls the rows and does not move the slider.
            assert_eq!(shown(&h), 0, "{size:?}");
            let brush = h.state().globe.brush;
            let from = control(&h, "Brush angle").unwrap().center();
            drag(&mut h, from, from - vec2(4.0, 260.0), None);
            assert_eq!(h.state().globe.brush, brush, "{size:?}");
            check_controls(&h, &format!("{size:?} after the scroll"));
        }
        assert_eq!(shown(&h), 4, "{size:?}");
        tap(&mut h, SWITCHES[1]);
        assert!(!h.state().globe.brush.ceiling_on, "{size:?}");
    }
}

#[test]
fn the_brush_panel_has_the_two_sliders_of_the_warp_mode() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush scale").is_none());
    tap(&mut h, MORE);
    tap(&mut h, "Warp");
    let s = h.state();
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Warp)
    );
    assert!(more_is_active(&h));
    assert_eq!(
        (s.globe.brush.warp_km, s.globe.brush.scale_km),
        (50.0, 200.0)
    );
    // The strength slider sets the offset in kilometers.
    let meters = s.globe.brush.strength_m;
    let rect = control(&h, "Brush strength").unwrap();
    drag(&mut h, rect.center(), rect.right_center(), None);
    assert!((h.state().globe.brush.warp_km - 500.0).abs() < 1e-6);
    assert_eq!(h.state().globe.brush.strength_m, meters);
    let rect = control(&h, "Brush scale").unwrap();
    drag(&mut h, rect.center(), rect.left_center(), None);
    assert!((h.state().globe.brush.scale_km - 5.0).abs() < 1e-9);
    drag(&mut h, rect.left_center(), rect.right_center(), None);
    assert!((h.state().globe.brush.scale_km - 2000.0).abs() < 1e-6);
    // The seed slider sets the seed of the world.
    assert_eq!(h.state().doc.project.world.seed, 0);
    let rect = control(&h, "Brush seed").unwrap();
    drag(&mut h, rect.center(), rect.right_center(), None);
    assert_eq!(h.state().doc.project.world.seed, 999);
    assert_eq!(h.state().globe.world_seed, 999);
    assert!(control(&h, "Brush angle").is_none());
    check_controls(&h, "the Warp mode");
}

#[test]
fn a_press_outside_the_more_menu_closes_it() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Lower");
    tap(&mut h, MORE);
    assert!(h.state().pad.more);
    // A second tap on the cell closes the menu.
    tap(&mut h, MORE);
    assert!(!h.state().pad.more);

    tap(&mut h, MORE);
    let menu = h.state().pad.more_rect.unwrap();
    let outside = menu.right_bottom() + vec2(40.0, 40.0);
    touch(&mut h, egui::TouchPhase::Start, outside, None);
    h.step();
    touch(&mut h, egui::TouchPhase::End, outside, None);
    h.run_steps(3);
    let s = h.state();
    assert!(!s.pad.more && s.pad.more_rect.is_none());
    assert!(control(&h, "Weather").is_none());
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Lower)
    );

    // A tap on another control closes the menu too.
    tap(&mut h, MORE);
    tap(&mut h, "Layers");
    assert!(!h.state().pad.more && h.state().pad.is_open(Panel::Layers));
}

#[test]
fn the_globe_does_not_move_when_you_touch_a_card() {
    for size in BOARDS {
        let mut h = pad(size);
        tap(&mut h, "Layers");
        let before = h.state().globe.view;
        let screen = h.state().globe.rect;
        if control(&h, "Workspace").is_some() {
            tap(&mut h, "Workspace");
            let menu = h.state().pad.menu_rect.unwrap();
            drag(
                &mut h,
                menu.left_top() + vec2(3.0, 3.0),
                screen.center(),
                None,
            );
            assert_eq!(h.state().globe.view, before, "{size:?}: the menu");
            // A touch outside the menu closes it.
            assert!(h.state().pad.menu);
            tap(&mut h, "Close Layers");
            assert!(!h.state().pad.menu);
            tap(&mut h, "Layers");
        }
        for card in cards(&h) {
            for from in [card.center(), card.left_top() + vec2(3.0, 3.0)] {
                let to = screen.center();
                // A finger, and then a pen in the Move tool. Each one turns the globe on the canvas.
                h.state_mut().globe.tool = Tool::Navigate;
                drag(&mut h, from, to, None);
                drag(&mut h, from, to, Some(0.5));
                assert_eq!(
                    h.state().globe.view,
                    before,
                    "{size:?}: {card:?} from {from:?}"
                );
            }
        }
        // The same touch on the canvas turns the globe.
        let from = pos2(screen.center().x + 20.0, 250.0);
        drag(&mut h, from, from + vec2(40.0, 30.0), None);
        assert_ne!(h.state().globe.view, before, "{size:?}");
    }
}

#[test]
fn one_finger_turns_the_globe_and_two_fingers_zoom_it() {
    let mut h = pad(BOARDS[0]);
    assert_eq!(h.state().globe.tool, Tool::Brush);
    let c = h.state().globe.rect.center();
    let before = h.state().globe.view;
    drag(&mut h, c, c + vec2(60.0, 0.0), None);
    let turned = h.state().globe.view;
    assert_ne!(turned.rot, before.rot);
    assert_eq!(turned.zoom, before.zoom);

    let finger = |h: &mut Pad, id: u64, phase: egui::TouchPhase, pos: Pos2| {
        h.event(egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(id),
            phase,
            pos,
            force: None,
        });
    };
    let (a, b) = (c - vec2(50.0, 0.0), c + vec2(50.0, 0.0));
    finger(&mut h, 1, egui::TouchPhase::Start, a);
    finger(&mut h, 2, egui::TouchPhase::Start, b);
    h.step();
    finger(&mut h, 1, egui::TouchPhase::Move, a - vec2(50.0, 0.0));
    finger(&mut h, 2, egui::TouchPhase::Move, b + vec2(50.0, 0.0));
    h.step();
    finger(&mut h, 1, egui::TouchPhase::End, a - vec2(50.0, 0.0));
    finger(&mut h, 2, egui::TouchPhase::End, b + vec2(50.0, 0.0));
    h.step();
    assert!(h.state().globe.view.zoom > turned.zoom * 1.5);
}

/// One test at a time makes a wgpu device.
fn one_gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn gpu_pad(size: (f32, f32)) -> Pad {
    let mut state = state();
    state.globe.set_face_size(256);
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let size = (f64::from(size.0), f64::from(size.1));
    let mut h = headless::ui_harness(state, size, 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(3);
    h
}

fn settle(h: &mut Pad) {
    for _ in 0..400 {
        h.step();
        if !h.state().globe.busy() {
            return;
        }
    }
    panic!("the stroke did not end");
}

/// A harness with the Brush and Layers panels open, in greyscale, and with a
/// globe that covers the screen. `level` gives the level at a longitude in
/// degrees.
fn blur_pad(level: impl Fn(f64) -> u16) -> Pad {
    let mut h = gpu_pad(BOARDS[0]);
    let state = h.state_mut();
    state.pad.open(Panel::Brush);
    state.pad.open(Panel::Layers);
    let globe = &mut state.globe;
    globe.preview.greyscale = true;
    globe.preview.graticule = false;
    globe.view = GlobeView::centered(0.0, 0.0);
    globe.view.zoom = 3.0;
    let n = globe.map.face_size();
    for face in 0..6 {
        for (x, y) in (0..n).flat_map(|y| (0..n).map(move |x| (x, y))) {
            let d = globe.map.texel_dir(face, x, y);
            let level = level(d[1].atan2(d[0]).to_degrees());
            globe.map.set(face, x, y, level);
        }
    }
    h.run_steps(3);
    h
}

/// The red values of a row of pixels near the bottom of a card, inside its
/// round corners. No control is there.
fn card_row(image: &image::RgbaImage, card: Rect) -> Vec<i32> {
    let y = card.bottom() as u32 - 3;
    let (left, right) = (card.left() as u32 + 16, card.right() as u32 - 16);
    let red = |x| i32::from(image.get_pixel(x, y).0[0]);
    (left..right).map(red).collect()
}

fn difference(a: &[i32], b: &[i32]) -> i32 {
    a.iter().zip(b).map(|(a, b)| (a - b).abs()).sum()
}

#[test]
fn each_card_shows_the_blurred_canvas_and_the_blur_follows_the_globe() {
    let _gpu = one_gpu_test();
    // Light and dark stripes along the meridians, each 20 degrees wide.
    let mut h = blur_pad(|lon| match (lon / 20.0).floor().rem_euclid(2.0) {
        0.0 => u16::MAX,
        _ => 0,
    });
    let cards = cards(&h);
    assert!(cards.len() >= 7);
    let blurred = h.render().unwrap();
    blurred.save("../../target/app/test-pad-blur.png").unwrap();
    h.state_mut().globe.blur = false;
    h.run_steps(2);
    let plain = h.render().unwrap();
    h.state_mut().globe.blur = true;
    h.state_mut().globe.view = GlobeView::centered(10.0, 0.0);
    h.state_mut().globe.view.zoom = 3.0;
    h.run_steps(2);
    let turned = h.render().unwrap();

    for card in cards {
        let row = card_row(&blurred, card);
        let width = row.len() as i32;
        // The edge of a stripe is soft below a card.
        let step = row.windows(2).map(|w| (w[0] - w[1]).abs()).max().unwrap();
        assert!(step <= 4, "{card:?}: a step of {step}");
        assert!(
            difference(&row, &card_row(&plain, card)) > width * 2,
            "{card:?}"
        );
        // A turn of half a stripe moves the stripes below the card.
        assert!(
            difference(&row, &card_row(&turned, card)) > width * 2,
            "{card:?}"
        );
    }
}

/// The contrast ratio of two colors, as WCAG defines it.
fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let luminance = |c: [u8; 3]| {
        let linear = c.map(|v| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        });
        0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
fn text_on_a_card_is_easy_to_read_over_a_white_canvas() {
    let _gpu = one_gpu_test();
    let mut h = blur_pad(|_| u16::MAX);
    let image = h.render().unwrap();
    let c = h.state().globe.rect.center();
    let canvas = image.get_pixel(c.x as u32, c.y as u32).0;
    assert!(canvas[..3].iter().all(|v| *v > 240), "{canvas:?}");
    for card in cards(&h) {
        let pixel = image.get_pixel(card.left() as u32 + 16, card.bottom() as u32 - 3);
        let fill = [pixel.0[0], pixel.0[1], pixel.0[2]];
        // The canvas shows through the card.
        assert!(fill[0] > 55, "{card:?}: {fill:?}");
        for text in [theme::TEXT, theme::MUTE, theme::WARN] {
            let ratio = contrast([text.r(), text.g(), text.b()], fill);
            assert!(ratio >= 4.5, "{card:?}: {ratio:.2} for {text:?}");
        }
    }
}

#[test]
fn the_pen_paints_on_the_canvas_and_a_panel_stays_open() {
    let _gpu = one_gpu_test();
    let mut h = gpu_pad(BOARDS[0]);
    tap(&mut h, "Layers");
    tap(&mut h, "Brush settings");
    let c = h.state().globe.rect.center();

    // A pen that starts on a card paints nothing.
    for card in cards(&h) {
        drag(&mut h, card.center(), c, Some(0.5));
        settle(&mut h);
        assert!(!h.state().globe.can_undo(), "{card:?}");
    }
    assert!(control(&h, "Undo").is_none());

    let before = h.state().globe.view;
    drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.5));
    settle(&mut h);
    assert!(h.state().globe.can_undo());
    assert_eq!(h.state().globe.view, before);
    assert!(h.state().pad.is_open(Panel::Layers));
    assert!(h.state().pad.is_open(Panel::Brush));

    tap(&mut h, "Undo");
    settle(&mut h);
    assert!(!h.state().globe.can_undo());

    // A panel closes with its close button and with the button that opened it.
    tap(&mut h, "Close Layers");
    assert!(!h.state().pad.is_open(Panel::Layers));
    tap(&mut h, "Brush settings");
    assert!(!h.state().pad.is_open(Panel::Brush));
    tap(&mut h, "Toolbox");
    tap(&mut h, "Toolbox");
    assert_eq!(h.state().pad.right, None);
}

/// A tap of `count` fingers near the middle of the canvas. The first finger
/// is also the pointer. All fingers come down in one frame and go up in the
/// next frame, so the tap is short.
fn tap_fingers(h: &mut Pad, count: u64) {
    let c = h.state().globe.rect.center();
    for phase in [egui::TouchPhase::Start, egui::TouchPhase::End] {
        let events = &mut h.input_mut().events;
        for id in 1..=count {
            let pos = c + vec2(60.0 * id as f32, 0.0);
            events.push(egui::Event::Touch {
                device_id: egui::TouchDeviceId(0),
                id: egui::TouchId(id),
                phase,
                pos,
                force: None,
            });
            if id == 1 {
                events.push(egui::Event::PointerMoved(pos));
                events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: phase == egui::TouchPhase::Start,
                    modifiers: egui::Modifiers::NONE,
                });
            }
        }
        if phase == egui::TouchPhase::End {
            events.push(egui::Event::PointerGone);
        }
        h.step();
    }
    h.run_steps(3);
}

/// Paints three pen strokes over the middle of the canvas. Returns the level
/// of the middle before the first stroke and after each stroke.
fn three_strokes(h: &mut Pad) -> [u16; 4] {
    let c = h.state().globe.rect.center();
    let mut levels = [middle_level(h); 4];
    for i in 1..4 {
        drag(h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.5));
        settle(h);
        levels[i] = middle_level(h);
        assert!(levels[i] > levels[i - 1], "{levels:?}");
    }
    levels
}

fn middle_level(h: &Pad) -> u16 {
    let globe = &h.state().globe;
    globe
        .map
        .sample(globe.unproject(globe.rect.center()).unwrap())
}

#[test]
fn a_new_world_has_an_empty_heightmap_on_the_gpu() {
    let _gpu = one_gpu_test();
    let mut h = gpu_pad(BOARDS[0]);
    let levels = three_strokes(&mut h);
    assert!(h.state().has_raster() && h.state().globe.can_undo());

    h.state_mut().run_action(Action::NewWorld(World::default()));
    h.run_steps(3);
    assert!(!h.state().has_raster() && !h.state().globe.can_undo());
    assert_eq!(middle_level(&h), levels[0]);
    // A stroke starts from the base level and not from the old ground.
    let c = h.state().globe.rect.center();
    drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.5));
    settle(&mut h);
    let level = middle_level(&h);
    assert!(
        levels[0] < level && level < levels[2],
        "{level} of {levels:?}"
    );
}

#[test]
fn a_tap_of_two_fingers_is_undo_and_a_tap_of_three_fingers_is_redo() {
    let _gpu = one_gpu_test();
    let mut h = gpu_pad(BOARDS[0]);
    let levels = three_strokes(&mut h);
    let view = h.state().globe.view;
    for level in levels[..3].iter().rev() {
        tap_fingers(&mut h, 2);
        settle(&mut h);
        assert_eq!(middle_level(&h), *level, "undo");
    }
    assert!(!h.state().globe.can_undo());
    // One more tap does nothing.
    tap_fingers(&mut h, 2);
    assert_eq!(middle_level(&h), levels[0]);
    for level in &levels[1..] {
        tap_fingers(&mut h, 3);
        settle(&mut h);
        assert_eq!(middle_level(&h), *level, "redo");
    }
    assert!(!h.state().globe.can_redo());
    // A tap of one finger does nothing, and no tap moves the globe.
    tap_fingers(&mut h, 1);
    assert_eq!(middle_level(&h), levels[3]);
    assert_eq!(h.state().globe.view, view);
}

#[test]
fn the_undo_button_and_the_redo_button_work() {
    let _gpu = one_gpu_test();
    let mut h = gpu_pad(BOARDS[0]);
    assert!(control(&h, "Undo").is_none() && control(&h, "Redo").is_none());
    let levels = three_strokes(&mut h);
    assert!(control(&h, "Redo").is_none());
    for level in levels[..3].iter().rev() {
        tap(&mut h, "Undo");
        settle(&mut h);
        assert_eq!(middle_level(&h), *level, "undo");
    }
    assert!(control(&h, "Undo").is_none());
    for level in &levels[1..] {
        tap(&mut h, "Redo");
        settle(&mut h);
        assert_eq!(middle_level(&h), *level, "redo");
    }
    assert!(control(&h, "Redo").is_none());
    // A new stroke removes the redo steps.
    tap(&mut h, "Undo");
    settle(&mut h);
    assert!(control(&h, "Redo").is_some());
    let c = h.state().globe.rect.center();
    drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.5));
    settle(&mut h);
    assert!(control(&h, "Redo").is_none());
}

/// A start of the app with a project file. The heightmap comes from the
/// file, if the file has one.
fn file_pad(path: &std::path::Path) -> Pad {
    let mut state = state();
    let session = Session::open(path, &mut state.doc).unwrap();
    match session.load_map().unwrap() {
        Some(map) => state.globe.set_map(map),
        None => state.globe.set_face_size(256),
    }
    state.session = Some(session);
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let size = (f64::from(BOARDS[0].0), f64::from(BOARDS[0].1));
    let mut h = headless::ui_harness(state, size, 1.0, setup);
    h.set_render_every_step(true);
    h.run_steps(3);
    h.state_mut().save_tick(false, std::time::Instant::now());
    h
}

/// Runs frames with the save of the app until the brush is idle.
fn settle_and_save(h: &mut Pad) {
    for _ in 0..400 {
        h.step();
        h.state_mut().save_tick(false, std::time::Instant::now());
        if !h.state().globe.busy() {
            return;
        }
    }
    panic!("the brush did not become idle");
}

/// The iPad test of the undo log: paint, stop the app with no exit call,
/// start it, and undo.
#[test]
fn the_undo_button_works_after_the_app_stops_and_starts() {
    let _gpu = one_gpu_test();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut h = file_pad(&path);
    assert!(control(&h, "Undo").is_none());
    let c = h.state().globe.rect.center();
    let mut levels = [middle_level(&h); 3];
    for i in 1..3 {
        drag(&mut h, c - vec2(40.0, 0.0), c + vec2(40.0, 0.0), Some(0.5));
        settle_and_save(&mut h);
        levels[i] = middle_level(&h);
        assert!(levels[i] > levels[i - 1], "{levels:?}");
    }
    // The file has the steps, and the heightmap has none in memory.
    assert!(!h.state().globe.can_undo() && h.state().can_undo());
    drop(h);

    let mut h = file_pad(&path);
    assert_eq!(middle_level(&h), levels[2]);
    assert!(control(&h, "Redo").is_none());
    // Each stroke is one step.
    for level in levels[..2].iter().rev() {
        tap(&mut h, "Undo");
        settle_and_save(&mut h);
        assert_eq!(middle_level(&h), *level, "undo");
    }
    assert!(control(&h, "Undo").is_none());
    tap(&mut h, "Redo");
    settle_and_save(&mut h);
    assert_eq!(middle_level(&h), levels[1]);
    drop(h);

    let mut h = file_pad(&path);
    assert_eq!(middle_level(&h), levels[1]);
    tap(&mut h, "Redo");
    settle_and_save(&mut h);
    assert_eq!(middle_level(&h), levels[2]);
    assert!(control(&h, "Redo").is_none());
}

#[test]
fn one_tap_switches_the_view_and_the_tool_and_the_panels_stay() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Lower");
    tap(&mut h, "Layers");
    tap(&mut h, "Brush settings");
    let panels = h.state().pad.panels.clone();
    assert_eq!(panels.len(), 2);
    for (name, view) in [("Flat", WorldView::Flat), ("Globe", WorldView::Globe)] {
        tap(&mut h, name);
        let s = h.state();
        assert_eq!(s.globe.world_view, view);
        assert_eq!(s.workspace, Workspace::Globe);
        assert_eq!(
            (s.globe.tool, s.globe.brush.mode),
            (Tool::Brush, Mode::Lower)
        );
        assert_eq!(s.pad.panels, panels);
    }
}

#[test]
fn the_view_switch_has_the_list_of_the_projections_of_the_flat_view() {
    use vale_sphere::ProjectionKind;
    let mut h = pad(BOARDS[0]);
    assert!(control(&h, "Projection").is_none());
    tap(&mut h, "Lower");
    tap(&mut h, "Flat");
    // The card is wider by the button, and it stays in the middle.
    let view = h.state().pad.rects.unwrap().view.unwrap();
    assert_eq!(view.center().x, 597.0);
    assert!(view.contains_rect(control(&h, "Projection").unwrap()));
    assert!(control(&h, "Projection").unwrap().width() >= 44.0);

    tap(&mut h, "Projection");
    let list = h.state().pad.projections_rect.unwrap();
    assert_eq!(list.top(), view.bottom() + 4.0);
    for kind in ProjectionKind::ALL {
        assert!(list.contains_rect(control(&h, kind.name()).unwrap()));
    }
    check_controls(&h, "with the projections");
    tap(&mut h, "Equal Earth");
    let s = h.state();
    assert_eq!(s.globe.flat.spec().kind, ProjectionKind::EqualEarth);
    assert!(!s.pad.projections && s.pad.projections_rect.is_none());
    assert_eq!(
        (s.globe.tool, s.globe.brush.mode),
        (Tool::Brush, Mode::Lower)
    );

    // A touch off the list closes it, and the globe has no list.
    tap(&mut h, "Projection");
    tap(&mut h, "Layers");
    assert!(!h.state().pad.projections);
    tap(&mut h, "Projection");
    tap(&mut h, "Globe");
    assert!(!h.state().pad.projections && control(&h, "Projection").is_none());
}

#[test]
fn the_flat_view_has_buttons_that_move_the_center_of_the_projection_and_reset_it() {
    for size in BOARDS {
        let mut h = pad(size);
        h.state_mut().globe.world_view = WorldView::Flat;
        h.run_steps(2);
        let rects = h.state().pad.rects.unwrap();
        // The buttons are in the card of the view switch, to the right of
        // the switch. With no switch in the top row, they have a card below
        // the top row.
        let card = match rects.view {
            Some(view) => {
                assert_eq!(h.state().pad.center_rect, None);
                assert_eq!(view.center().x, size.0 / 2.0, "{size:?}");
                assert!(view.right() + 12.0 <= rects.actions.left(), "{size:?}");
                view
            }
            None => {
                let card = h.state().pad.center_rect.unwrap();
                assert_eq!(card.center().x, size.0 / 2.0, "{size:?}");
                assert_eq!(card.top(), rects.tools.top());
                assert!(!card.intersects(rects.tools));
                card
            }
        };
        // The middle of the view is the center of the projection, and the
        // view shows the whole map, so the two actions are off.
        assert!(control(&h, RECENTER).is_none() && control(&h, RESET).is_none());

        // A drag moves the map at the first zoom, with no zoom before it.
        let canvas = h.state().globe.rect;
        let from = pos2(canvas.center().x, canvas.bottom() - 120.0);
        drag(&mut h, from, from + vec2(-60.0, 0.0), None);
        let (recenter, reset) = (control(&h, RECENTER).unwrap(), control(&h, RESET).unwrap());
        assert!(card.contains_rect(recenter) && card.contains_rect(reset));
        assert_eq!(reset.left() - recenter.right(), 4.0);
        if rects.view.is_some() {
            let switch = control(&h, "Flat").unwrap();
            assert!(recenter.left() > control(&h, "Projection").unwrap().right());
            assert!(recenter.left() > switch.right() && recenter.top() == switch.top());
        }
        check_controls(&h, &format!("{size:?} with the center buttons"));
        tap(&mut h, RECENTER);
        let flat = &h.state().globe.flat;
        assert!(flat.spec().lon0 > 1.0, "{size:?}: {:?}", flat.spec());
        assert!(control(&h, RECENTER).is_none());

        tap(&mut h, RESET);
        let flat = &h.state().globe.flat;
        assert_eq!((flat.spec().lon0, flat.zoom), (0.0, 1.0));
        assert!(control(&h, RESET).is_none());

        // The globe has no buttons.
        h.state_mut().globe.world_view = WorldView::Globe;
        h.run_steps(2);
        assert_eq!(h.state().pad.center_rect, None);
        assert!(control(&h, RECENTER).is_none() && control(&h, RESET).is_none());
    }
}

#[test]
fn the_top_row_and_the_panels_match_the_landscape_board() {
    let mut h = pad(BOARDS[0]);
    let r = h.state().pad.rects.unwrap();
    assert_eq!(r.class, WidthClass::Wide);
    assert_eq!(r.workspace.min, pos2(16.0, 32.0));
    assert_eq!(r.workspace.height(), 52.0);
    // The Worlds button and a line are at the left of the workspaces.
    assert_eq!(control(&h, WORLDS).unwrap().min, pos2(20.0, 36.0));
    assert_eq!(control(&h, "World").unwrap().min, pos2(73.0, 36.0));
    assert!(control(&h, "Maps").is_some());
    assert!(control(&h, "Workspace").is_none());
    // Atlas does nothing yet, so it is not a control.
    assert!(control(&h, "Atlas").is_none());
    assert!(r.view.unwrap().contains_rect(control(&h, "Flat").unwrap()));
    assert_eq!(r.view.unwrap().center(), pos2(597.0, 58.0));
    assert!(r.view.unwrap().contains_rect(control(&h, "Globe").unwrap()));
    assert_eq!(
        r.actions,
        Rect::from_min_size(pos2(969.0, 32.0), vec2(209.0, 52.0))
    );
    let lefts = ["Layers", "Toolbox"].map(|name| control(&h, name).unwrap().left());
    assert_eq!(lefts, [1082.0, 1130.0]);
    assert_eq!(
        r.tools,
        Rect::from_min_size(pos2(16.0, 96.0), vec2(64.0, 320.0))
    );
    assert_eq!(
        control(&h, "Raise").unwrap(),
        Rect::from_min_size(pos2(20.0, 100.0), vec2(56.0, 52.0))
    );
    assert_eq!(
        r.brush.unwrap(),
        Rect::from_min_size(pos2(16.0, 428.0), vec2(64.0, 340.0))
    );
    assert_eq!(
        control(&h, "Brush settings").unwrap().min,
        pos2(20.0, 712.0)
    );

    tap(&mut h, "Layers");
    tap(&mut h, "Brush settings");
    let layers = panel(&h, Panel::Layers).unwrap();
    assert_eq!((layers.min, layers.width()), (pos2(858.0, 96.0), 320.0));
    assert_eq!(layers.height(), 52.0 + 48.0 + 10.0);
    assert_eq!(
        control(&h, "Close Layers").unwrap().min,
        pos2(1130.0, 100.0)
    );
    let brush = panel(&h, Panel::Brush).unwrap();
    assert_eq!((brush.min, brush.width()), (pos2(92.0, 96.0), 340.0));
    // Toolbox opens in the place of Layers.
    tap(&mut h, "Toolbox");
    assert_eq!(panel(&h, Panel::Layers), None);
    assert_eq!(panel(&h, Panel::Toolbox).unwrap().min, pos2(858.0, 96.0));
    assert!(panel(&h, Panel::Brush).is_some());
}

#[test]
fn the_top_row_and_the_panels_match_the_portrait_board() {
    let mut h = pad(BOARDS[1]);
    let r = h.state().pad.rects.unwrap();
    assert_eq!(r.class, WidthClass::Medium);
    assert!(control(&h, "World").is_none());
    assert_eq!(control(&h, WORLDS).unwrap().min, pos2(20.0, 36.0));
    let button = control(&h, "Workspace").unwrap();
    assert_eq!((button.left(), button.right()), (69.0, r.workspace.right()));
    assert_eq!(
        (r.workspace.min, r.workspace.height()),
        (pos2(16.0, 32.0), 52.0)
    );
    assert_eq!(r.view.unwrap().center(), pos2(417.0, 58.0));
    assert!(control(&h, "Globe").is_some());
    assert_eq!(r.actions.right(), 818.0);

    tap(&mut h, "Toolbox");
    let toolbox = panel(&h, Panel::Toolbox).unwrap();
    assert_eq!((toolbox.min, toolbox.width()), (pos2(498.0, 96.0), 320.0));
    tap(&mut h, "Brush settings");
    assert!(h.state().pad.is_open(Panel::Toolbox) && h.state().pad.is_open(Panel::Brush));

    // The menu holds the workspaces. The view switch stays in the top row.
    tap(&mut h, "Workspace");
    let menu = h.state().pad.menu_rect.unwrap();
    assert_eq!((menu.min, menu.width()), (pos2(16.0, 88.0), 240.0));
    assert!(menu.contains_rect(control(&h, "Maps").unwrap()));
    tap(&mut h, "Maps");
    assert_eq!(h.state().workspace, Workspace::Map);
    // The flat map has the desktop layout, with a way back to the world.
    h.get_by_label("Globe").click();
    h.run_steps(3);
    assert_eq!(h.state().workspace, Workspace::Globe);
    assert!(control(&h, "Workspace").is_some());
}

#[test]
fn the_top_row_and_the_panels_match_the_split_view_boards() {
    for size in [BOARDS[2], BOARDS[3]] {
        let mut h = pad(size);
        let r = h.state().pad.rects.unwrap();
        assert_eq!(r.class, WidthClass::Compact);
        assert_eq!(r.view, None);
        assert!(control(&h, "Globe").is_none());
        assert_eq!(r.workspace.min, pos2(16.0, 32.0));
        assert_eq!(r.actions.right(), size.0 - 16.0);
        assert!(r.workspace.right() < r.actions.left());
        assert_eq!(r.readout.unwrap().left_bottom(), pos2(16.0, 808.0));

        // The menu holds the views.
        tap(&mut h, "Workspace");
        let menu = h.state().pad.menu_rect.unwrap();
        assert_eq!(
            menu,
            Rect::from_min_size(pos2(16.0, 88.0), vec2(240.0, 334.0))
        );
        let tops = ["World", "Globe", "Maps"].map(|name| control(&h, name).unwrap().top());
        assert_eq!(tops, [98.0, 146.0, 255.0]);
        tap(&mut h, "Flat");
        assert!(!h.state().pad.menu);
        assert_eq!(h.state().globe.world_view, WorldView::Flat);
        // In the flat view, the menu opens the list of the projections.
        tap(&mut h, "Workspace");
        tap(&mut h, "Projection");
        assert!(!h.state().pad.menu);
        assert_eq!(h.state().pad.projections_rect.unwrap().min, menu.min);
        tap(&mut h, "Mercator");
        let kind = h.state().globe.flat.spec().kind;
        assert_eq!(kind, vale_sphere::ProjectionKind::Mercator);
        tap(&mut h, "Workspace");
        tap(&mut h, "Globe");
        assert!(!h.state().pad.menu);
        assert_eq!(h.state().globe.world_view, WorldView::Globe);

        // A panel is a sheet at the bottom, and one sheet shows at a time.
        tap(&mut h, "Layers");
        let sheet = Rect::from_min_size(pos2(0.0, 434.0), vec2(size.0, 400.0));
        assert_eq!(panel(&h, Panel::Layers), Some(sheet));
        assert_eq!(h.state().pad.rects.unwrap().readout, None);
        tap(&mut h, "Toolbox");
        assert_eq!(h.state().pad.panels, vec![(Panel::Toolbox, sheet)]);
    }
}

#[test]
fn you_pull_the_sheet_down_to_its_title_row() {
    let mut h = painted_pad(BOARDS[3]);
    tap(&mut h, "Layers");
    assert!(control(&h, "Open Height").is_some());
    let bar = pos2(150.0, 460.0);
    drag(&mut h, bar, bar + vec2(0.0, 250.0), None);
    let sheet = panel(&h, Panel::Layers).unwrap();
    assert_eq!(sheet.top(), 834.0 - 20.0 - 52.0);
    // The title row shows, and its close button is above the bottom band.
    assert!(control(&h, "Open Height").is_none());
    assert_eq!(
        control(&h, "Close Layers").unwrap().bottom(),
        834.0 - 20.0 - 4.0
    );
    // The Brush card has room again.
    assert!(control(&h, "Brush settings").is_some());

    // A short pull does not close the sheet, and a pull up opens it.
    let bar = pos2(150.0, 790.0);
    drag(&mut h, bar, bar - vec2(0.0, 300.0), None);
    assert_eq!(panel(&h, Panel::Layers).unwrap().top(), 434.0);
    let bar = pos2(150.0, 460.0);
    drag(&mut h, bar, bar + vec2(0.0, 60.0), None);
    assert_eq!(panel(&h, Panel::Layers).unwrap().top(), 434.0);

    tap(&mut h, "Close Layers");
    assert!(h.state().pad.panels.is_empty());
}

#[test]
fn the_layout_follows_the_turn_of_the_ipad() {
    let mut h = pad(BOARDS[0]);
    tap(&mut h, "Layers");
    tap(&mut h, "Brush settings");
    for size in [BOARDS[1], BOARDS[3], BOARDS[0]] {
        h.set_size(size.into());
        h.run_steps(3);
        let screen = Rect::from_min_size(Pos2::ZERO, size.into());
        assert_eq!(h.state().globe.rect, screen);
        check_controls(&h, &format!("turned to {size:?}"));
    }
    // The narrow window has room for one panel.
    assert!(h.state().pad.is_open(Panel::Brush));
    assert!(!h.state().pad.is_open(Panel::Layers));
}

#[test]
fn a_short_window_makes_the_sliders_short_and_then_leaves_them_out() {
    // 1024 by 768 is an iPad in landscape. The sliders fit with less height.
    let h = pad((1024.0, 768.0));
    assert_eq!(
        h.state().pad.rects.unwrap().brush_card,
        BrushCard::Full(122.0)
    );
    check_controls(&h, "768 high");

    // The smallest iPad in landscape. The strip of six cells leaves room for
    // the sliders.
    let mut h = pad((1133.0, 744.0));
    let rects = h.state().pad.rects.unwrap();
    assert_eq!(rects.brush_card, BrushCard::Full(110.0));
    assert_eq!(rects.tool_cells, 6);
    tap(&mut h, MORE);
    check_controls(&h, "744 high with the More menu");

    let mut h = pad((1024.0, 700.0));
    assert_eq!(h.state().pad.rects.unwrap().brush_card, BrushCard::Button);
    assert!(control(&h, "Size").is_none() && control(&h, "Flow").is_none());
    check_controls(&h, "700 high");
    // The Brush panel holds the two sliders.
    tap(&mut h, "Brush settings");
    assert!(control(&h, "Brush size").is_some() && control(&h, "Brush flow").is_some());
    check_controls(&h, "700 high with the Brush panel");
}

#[test]
fn the_cards_stay_out_of_the_safe_area_of_the_device() {
    let mut h = pad(BOARDS[0]);
    let insets = egui::epaint::MarginF32 {
        left: 0.0,
        right: 0.0,
        top: 40.0,
        bottom: 34.0,
    };
    h.input_mut().safe_area_insets = Some(egui::SafeAreaInsets(insets));
    h.run_steps(3);
    let screen = Rect::from_min_size(Pos2::ZERO, BOARDS[0].into());
    assert_eq!(h.state().globe.rect, screen);
    for c in &h.state().pad.controls {
        assert!(
            c.rect.top() >= 40.0 && c.rect.bottom() <= 834.0 - 34.0,
            "{}",
            c.name
        );
    }
}

#[test]
fn the_readout_shows_the_place_and_the_elevation() {
    assert_eq!(
        readout_text(Some([24.6, 31.2]), Some(640.0)),
        "24.6°E 31.2°N · 640 m"
    );
    assert_eq!(
        readout_text(Some([-70.04, -8.0]), Some(-120.4)),
        "70.0°W 8.0°S · -120 m"
    );
    assert_eq!(readout_text(None, None), "–");
}

#[test]
fn the_screenshots_show_the_boards() {
    let _gpu = one_gpu_test();
    std::fs::create_dir_all("../../target/app").unwrap();
    // The name, the size, the open panels, and the state of the menu. The
    // shot "more" has the menu of the More cell in the Weather mode.
    type Shot = (&'static str, (f64, f64), &'static [Panel], bool);
    let shots: [Shot; 6] = [
        ("landscape", (1194.0, 834.0), &[], false),
        ("more", (1194.0, 834.0), &[Panel::Brush], false),
        (
            "open",
            (1194.0, 834.0),
            &[Panel::Brush, Panel::Layers],
            false,
        ),
        ("portrait", (834.0, 1194.0), &[Panel::Toolbox], false),
        ("half", (597.0, 834.0), &[], true),
        ("third", (375.0, 834.0), &[Panel::Layers], false),
    ];
    for (name, size, panels, menu) in shots {
        let mut state = state();
        for panel in panels {
            state.pad.open(*panel);
        }
        state.pad.menu = menu;
        if name == "more" {
            state.pad.more = true;
            state.globe.tool = Tool::Brush;
            state.globe.brush.mode = Mode::Weather;
        }
        let (image, state) = headless::ui_png(state, size, 1.0).unwrap();
        image
            .save(format!("../../target/app/test-pad-{name}.png"))
            .unwrap();
        // The canvas is behind the cards, and a card is not as dark as the canvas.
        let r = state.pad.rects.unwrap();
        let card = image.get_pixel(r.tools.center().x as u32, r.tools.bottom() as u32 - 3);
        let canvas = image.get_pixel(size.0 as u32 - 3, 300);
        assert_eq!(canvas.0, [15, 17, 19, 255], "{name}");
        assert_ne!(card.0, canvas.0, "{name}");
        assert_eq!(state.pad.more_rect.is_some(), name == "more", "{name}");
    }
}

/// Types text into the field that has the focus, in place of its text.
fn type_text(h: &mut Pad, name: &str, text: &str) {
    tap(h, name);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.event(egui::Event::Text(text.to_owned()));
    h.run_steps(2);
}

/// Opens the menu of the worlds.
fn open_worlds(h: &mut Pad) {
    if h.state().pad.rects.unwrap().class == WidthClass::Compact {
        tap(h, "Workspace");
        tap(h, WORLDS_ROW);
    } else {
        tap(h, WORLDS);
    }
    assert!(h.state().pad.worlds.is_some() && !h.state().pad.menu);
}

#[test]
fn the_menu_of_the_worlds_opens_another_world() {
    for size in BOARDS {
        let dir = tempfile::tempdir().unwrap();
        let earth = dir.path().join("Earth.gpkg");
        let mut h = painted_pad(size);
        let state = h.state_mut();
        state.projects = Some(dir.path().to_path_buf());
        state.session = Some(Session::open(&earth, &mut state.doc).unwrap());
        let layers = state.doc.project.layers().len();
        let world = World {
            name: "Pebble".to_string(),
            radius_km: 200.0,
            units: Units::Metric,
            seed: 0,
        };
        state.run_action(Action::NewWorld(world));
        assert!(!state.has_raster());
        h.run_steps(2);

        // The menu lists the two worlds, and the open world is not a new row.
        open_worlds(&mut h);
        let menu = h.state().pad.worlds_rect.unwrap();
        let screen = h.state().pad.rects.unwrap().screen;
        assert!(
            screen.shrink(16.0).contains_rect(menu),
            "{size:?}: {menu:?}"
        );
        let rows = ["Earth (sample)", "Pebble", NEW_WORLD, WORLD_PROPERTIES];
        let tops = rows.map(|name| {
            let row = control(&h, name).unwrap_or_else(|| panic!("{size:?}: no {name}"));
            assert!(menu.contains_rect(row) && row.height() >= 44.0, "{name}");
            row.top()
        });
        assert!(tops.is_sorted(), "{size:?}: {tops:?}");

        tap(&mut h, "Earth (sample)");
        assert!(h.state().pad.worlds.is_none());
        let actions = std::mem::take(&mut h.state_mut().actions);
        assert_eq!(actions, [Action::OpenWorld(earth.clone())]);
        h.state_mut().run_action(actions[0].clone());
        h.run_steps(2);
        let state = h.state();
        assert_eq!(state.doc.project.world.name, "Earth (sample)");
        assert_eq!(state.doc.project.layers().len(), layers);
        assert!(state.has_raster(), "{size:?}");
        assert_eq!(state.session.as_ref().unwrap().path(), earth);

        // A touch off the menu closes it.
        open_worlds(&mut h);
        let off = h.state().globe.rect.right_bottom() - vec2(30.0, 120.0);
        touch(&mut h, egui::TouchPhase::Start, off, None);
        h.step();
        touch(&mut h, egui::TouchPhase::End, off, None);
        h.run_steps(2);
        assert!(h.state().pad.worlds.is_none(), "{size:?}");
    }
}

#[test]
fn the_world_form_makes_a_new_world_with_a_small_radius() {
    for size in BOARDS {
        let mut h = painted_pad(size);
        let earth = brush_size_text(h.state());
        open_worlds(&mut h);
        tap(&mut h, WORLD_PROPERTIES);
        assert!(h.state().pad.worlds.is_none());
        let form = h.state().world_form.clone().unwrap();
        assert_eq!(form.mode, FormMode::Properties);
        assert_eq!(
            (form.name.as_str(), form.radius.as_str()),
            ("Earth (sample)", "6371")
        );
        check_form(&h, &format!("{size:?} properties"));

        tap(&mut h, "Close World");
        assert!(h.state().world_form.is_none());
        open_worlds(&mut h);
        tap(&mut h, NEW_WORLD);
        assert_eq!(h.state().world_form.as_ref().unwrap().mode, FormMode::New);
        check_form(&h, &format!("{size:?} new"));
        tap_radius(&mut h, 1737.4);
        assert_eq!(h.state().world_form.as_ref().unwrap().radius, "1737.4");
        type_text(&mut h, NAME, "Pebble");
        type_text(&mut h, "Radius (km)", "200");
        let form = h.state().world_form.as_ref().unwrap();
        assert_eq!(form.radius_km(), Some(200.0), "{size:?}: {:?}", form.radius);
        assert_eq!(form.preset(), None);

        tap(&mut h, CREATE);
        assert!(h.state().world_form.is_none());
        let world = World {
            name: "Pebble".to_string(),
            radius_km: 200.0,
            units: Units::Metric,
            seed: 0,
        };
        let actions = std::mem::take(&mut h.state_mut().actions);
        assert_eq!(actions, [Action::NewWorld(world.clone())]);
        h.state_mut().run_action(actions[0].clone());
        h.run_steps(2);

        let state = h.state();
        assert_eq!(state.doc.project.world, world);
        assert!(state.doc.project.layers().is_empty());
        assert!(state.doc.project.rasters().is_empty());
        assert!(!state.has_raster() && !state.globe.can_undo());
        // The brush has the same size on the screen, and less ground under it.
        let km = state.globe.brush.radius_km(state.globe.radius(), 200.0);
        assert_eq!(
            brush_size_text(state),
            format!("{} km", three_significant(km))
        );
        assert_ne!(brush_size_text(state), earth);
        assert_eq!(state.globe.world_km, 200.0);

        // The menu of the worlds also opens the form of a new world, and
        // the first form of an app with no project does not close.
        open_worlds(&mut h);
        tap(&mut h, NEW_WORLD);
        let form = h.state_mut().world_form.as_mut().unwrap();
        assert_eq!(form.mode, FormMode::New);
        form.required = true;
        h.run_steps(2);
        tap(&mut h, "Close New world");
        assert!(h.state().world_form.is_some(), "{size:?}");
        tap(&mut h, CREATE);
        assert_eq!(h.state().actions.len(), 1, "{size:?}");
    }
}

/// A tap on the radius slider of the open form at the place of this radius.
fn tap_radius(h: &mut Pad, km: f64) {
    let slider = control(h, RADIUS_SLIDER).unwrap();
    let (left, width) = (
        slider.left() + SLIDER_INSET,
        slider.width() - 2.0 * SLIDER_INSET,
    );
    let pos = pos2(left + radius_fraction(km) as f32 * width, slider.center().y);
    touch(h, egui::TouchPhase::Start, pos, None);
    h.step();
    touch(h, egui::TouchPhase::End, pos, None);
    h.run_steps(3);
}

#[test]
fn the_radius_slider_snaps_to_the_known_radii() {
    for size in BOARDS {
        let mut h = pad(size);
        h.state_mut().world_form = Some(WorldForm::new_world());
        h.run_steps(2);
        check_form(&h, &format!("{size:?} slider"));
        let slider = control(&h, RADIUS_SLIDER).unwrap();
        let (left, width) = (
            slider.left() + SLIDER_INSET,
            slider.width() - 2.0 * SLIDER_INSET,
        );
        let at = |km: f64| pos2(left + radius_fraction(km) as f32 * width, slider.center().y);
        let radius = |h: &Pad| h.state().world_form.as_ref().unwrap().radius_km().unwrap();

        // A drag to a place near Mars gives the radius of Mars.
        drag(&mut h, at(6371.0), at(3389.5) + vec2(3.0, 0.0), None);
        assert_eq!(radius(&h), 3389.5, "{size:?}");
        let form = h.state().world_form.as_ref().unwrap();
        assert_eq!((form.radius.as_str(), form.preset()), ("3389.5", Some(1)));
        // A place off the presets gives a round value.
        drag(&mut h, at(3389.5), at(12_000.0), None);
        let km = radius(&h);
        assert!(10_000.0 < km && km < 14_000.0, "{size:?}: {km}");
        assert_eq!(three_significant(km).parse::<f64>().unwrap(), km);
        assert_eq!(h.state().world_form.as_ref().unwrap().preset(), None);
        // The end of the slider is its upper limit.
        drag(
            &mut h,
            at(12_000.0),
            pos2(slider.right(), slider.center().y),
            None,
        );
        assert_eq!(radius(&h), 20_000.0);
        tap_radius(&mut h, 1737.4);
        assert_eq!(radius(&h), 1737.4);
        // A typed value can be outside the limits of the slider.
        type_text(&mut h, "Radius (km)", "250");
        assert_eq!(radius(&h), 250.0, "{size:?}");
    }
}

/// Makes sure that the open form holds each control of the frame, and that
/// a finger can hit each one.
fn check_form(h: &Pad, what: &str) {
    let screen = h.state().pad.rects.unwrap().screen;
    let controls = &h.state().pad.controls;
    assert!(controls.len() >= 6, "{what}: {} controls", controls.len());
    for (i, a) in controls.iter().enumerate() {
        let (name, rect) = (&a.name, a.rect);
        assert!(
            rect.width() >= 44.0 && rect.height() >= 44.0,
            "{what}: {name} is {rect:?}"
        );
        assert!(rect.top() >= BANDS.0, "{what}: {name}");
        assert!(rect.bottom() <= screen.bottom() - BANDS.1, "{what}: {name}");
        assert!(
            rect.left() >= 0.0 && rect.right() <= screen.right(),
            "{what}: {name}"
        );
        for b in &controls[i + 1..] {
            assert_ne!(a.name, b.name, "{what}");
            let free = !a.rect.shrink(0.5).intersects(b.rect.shrink(0.5));
            assert!(free, "{what}: {name} and {} overlap", b.name);
        }
    }
}

#[test]
fn the_properties_of_the_world_apply_at_once_and_a_touch_does_not_reach_the_canvas() {
    let mut h = pad(BOARDS[0]);
    h.state_mut().world_form = Some(WorldForm::properties(&World::default()));
    h.run_steps(2);
    let view = h.state().globe.view;
    let layers = h.state().doc.project.layers().to_vec();
    drag(&mut h, pos2(900.0, 600.0), pos2(700.0, 500.0), None);
    assert!(h.state().globe.view == view);

    tap_radius(&mut h, 3389.5);
    tap(&mut h, "Miles");
    let world = &h.state().doc.project.world;
    assert_eq!((world.radius_km, world.units), (3389.5, Units::Imperial));
    assert_eq!(h.state().world_form.as_ref().unwrap().radius, "2106.1");
    // The change of the units keeps the radius.
    tap(&mut h, "Kilometers");
    assert_eq!(h.state().doc.project.world.radius_km, 3389.5);
    // A radius that is too small does not go to the world.
    type_text(&mut h, "Radius (km)", "5");
    assert_eq!(h.state().doc.project.world.radius_km, 3389.5);
    assert!(h.query_by_label_contains("From 100 to 100000 km").is_some());
    type_text(&mut h, "Radius (km)", "500");
    assert_eq!(h.state().doc.project.world.radius_km, 500.0);
    assert_eq!(h.state().doc.project.layers(), layers);
    tap(&mut h, "Close World");
    assert!(h.state().world_form.is_none());
}
