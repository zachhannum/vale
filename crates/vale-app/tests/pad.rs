//! The iPad layout of the world workspace.

use eframe::egui::{self, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use vale_app::document::Document;
use vale_app::globe::view::GlobeView;
use vale_app::globe::{Tool, WorldView};
use vale_app::headless;
use vale_app::ui::pad::geometry::{BrushCard, WidthClass};
use vale_app::ui::pad::{IMPORT_HEIGHTMAP, Panel, RECENTER, RESET, readout_text, theme};
use vale_app::ui::{Action, AppState, Layout, Workspace, draw};
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
    let in_menu = h.state().pad.menu_rect.is_some();
    let popup = in_menu || h.state().pad.projections_rect.is_some();
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
            // The open menu and the open list of projections cover the
            // cards below them.
            let row = |c: &vale_app::ui::pad::widgets::Control| {
                let name = c.name.as_str();
                let kind = vale_sphere::ProjectionKind::ALL
                    .iter()
                    .any(|k| k.name() == name);
                let menu = ["World", "Globe", "Flat", "Maps"].contains(&name);
                menu || kind || (in_menu && name == "Projection")
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
        let mut h = pad(size);
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
        let mut h = pad(size);
        tap(&mut h, "Layers");
        tap(&mut h, "Open Height");
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
    let mut h = pad(BOARDS[0]);
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
    assert_eq!(control(&h, "World").unwrap().min, pos2(20.0, 36.0));
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
