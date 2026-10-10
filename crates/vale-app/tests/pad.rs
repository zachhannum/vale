//! The iPad layout of the world workspace.

use eframe::egui::{self, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use vale_app::document::Document;
use vale_app::globe::Tool;
use vale_app::headless;
use vale_app::ui::pad::geometry::{BrushCard, WidthClass};
use vale_app::ui::pad::{Panel, readout_text};
use vale_app::ui::{AppState, Layout, Workspace, draw};
use vale_terrain::Mode;

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
    let menu = h.state().pad.menu_rect;
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
            // The open menu covers the cards below it.
            let row = |c: &vale_app::ui::pad::widgets::Control| {
                ["World", "Globe", "Maps"].contains(&c.name.as_str())
            };
            let covered = menu.is_some() && row(a) != row(b);
            let free = !a.rect.shrink(0.5).intersects(b.rect.shrink(0.5));
            assert!(free || covered, "{what}: {name} and {} overlap", b.name);
        }
    }
}

#[test]
fn each_control_is_large_and_is_outside_the_system_bands() {
    for size in BOARDS {
        let mut h = pad(size);
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
        if control(&h, "Workspace").is_some() {
            tap(&mut h, "Workspace");
            assert!(h.state().pad.menu);
            check_controls(&h, &format!("{size:?} with the menu"));
        }
    }
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

#[test]
fn the_top_row_and_the_panels_match_the_landscape_board() {
    let mut h = pad(BOARDS[0]);
    let r = h.state().pad.rects.unwrap();
    assert_eq!(r.class, WidthClass::Wide);
    assert_eq!(r.workspace.min, pos2(16.0, 32.0));
    assert_eq!(r.workspace.height(), 52.0);
    assert_eq!(control(&h, "World").unwrap().min, pos2(20.0, 36.0));
    assert!(control(&h, "Maps").is_some());
    assert!(control(&h, "Workspace").is_none());
    // Atlas and Flat do nothing yet, so they are not controls.
    assert!(control(&h, "Atlas").is_none() && control(&h, "Flat").is_none());
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
    assert_eq!(control(&h, "Workspace").unwrap(), r.workspace);
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
            Rect::from_min_size(pos2(16.0, 88.0), vec2(240.0, 273.0))
        );
        let tops = ["World", "Globe", "Maps"].map(|name| control(&h, name).unwrap().top());
        assert_eq!(tops, [98.0, 146.0, 255.0]);
        tap(&mut h, "Globe");
        assert!(!h.state().pad.menu);

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
    let mut h = pad(BOARDS[3]);
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
    // The name, the size, the open panels, and the state of the menu.
    type Shot = (&'static str, (f64, f64), &'static [Panel], bool);
    let shots: [Shot; 5] = [
        ("landscape", (1194.0, 834.0), &[], false),
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
    }
}
