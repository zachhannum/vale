//! The Simulate workspace with no GPU: Run and Pause, the year counter, the
//! speed, and the quality.

use eframe::egui;
use egui_kittest::kittest::Queryable;
use isotherm_app::app::{COARSE_ONLY, IsothermApp, Layout, PAUSE, RUN, Workspace, year_text};
use isotherm_app::headless::harness_with;
use isotherm_app::memory::{RESERVE, run_bytes, set_test_available};
use isotherm_fields::{Output, Period};

type H = egui_kittest::Harness<'static, IsothermApp>;

const PAD: (f32, f32) = (1194.0, 834.0);
const PHONE: (f32, f32) = (393.0, 852.0);

fn simulate(size: (f32, f32)) -> H {
    let mut app = IsothermApp::sample(Layout::Pad).expect("the sample world reads");
    app.workspace = Workspace::Simulate;
    let mut h = harness_with(size, app);
    h.run_steps(3);
    h
}

fn has_control(h: &H, name: &str) -> bool {
    h.state().controls.iter().any(|c| c.name == name)
}

fn click(h: &mut H, name: &str) {
    let rect = h
        .state()
        .controls
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no control {name}"))
        .rect;
    let pos = rect.center();
    h.event(egui::Event::PointerMoved(pos));
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(button(true));
    h.step();
    h.event(button(false));
    h.event(egui::Event::PointerGone);
    h.step();
}

fn steps(h: &H) -> u64 {
    h.state()
        .run
        .borrow()
        .sim
        .as_ref()
        .map_or(0, |sim| sim.step_count())
}

/// Steps the frames until the run has done `count` steps.
fn run_to(h: &mut H, count: u64) {
    for _ in 0..10_000 {
        if steps(h) >= count {
            return;
        }
        h.step();
    }
    panic!("the run stopped at {}", steps(h));
}

#[test]
fn run_advances_the_year_counter_and_pause_stops_it() {
    for size in [PAD, PHONE] {
        let mut h = simulate(size);
        h.get_by_label(&year_text(0));
        click(&mut h, RUN);
        assert!(h.state().running());
        assert!(has_control(&h, PAUSE));
        run_to(&mut h, 31);
        h.step();
        h.get_by_label(&year_text(steps(&h)));
        assert!(year_text(steps(&h)).ends_with("Feb"));
        click(&mut h, PAUSE);
        let stopped = steps(&h);
        h.run_steps(3);
        assert_eq!(steps(&h), stopped);
        assert!(has_control(&h, RUN));
    }
}

#[test]
fn a_month_that_ends_shows_in_the_explore_workspace() {
    let mut h = simulate(PAD);
    click(&mut h, "Fast");
    click(&mut h, RUN);
    run_to(&mut h, 31);
    let means = h.state().means().expect("the means of the run");
    let field = means.field(Output::Temperature);
    assert!(field.get(Period::Month(0)).is_some());
}

#[test]
fn the_speed_does_not_change_the_result() {
    let state = |speed: &str| {
        let mut h = simulate(PAD);
        click(&mut h, speed);
        click(&mut h, RUN);
        run_to(&mut h, 60);
        let app = h.state();
        let mut run = app.run.borrow_mut();
        run.sim.as_mut().unwrap().snapshot().unwrap()
    };
    let slow = state("Slow");
    let fast = state("Fast");
    assert_eq!(slow.step, 60);
    assert_eq!(fast.step, 60);
    let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&slow.temperature_k), bits(&fast.temperature_k));
    assert_eq!(slow, fast);
}

#[test]
fn the_quality_offers_the_grids_that_fit_in_memory() {
    set_test_available(Some(RESERVE + run_bytes(64)));
    let h = simulate(PAD);
    assert!(has_control(&h, "Coarse"));
    assert!(has_control(&h, "Medium"));
    assert!(!has_control(&h, "Fine"));
    assert_eq!(h.state().quality, 32);
    assert!(h.query_by_label(COARSE_ONLY).is_none());

    set_test_available(Some(0));
    let h = simulate(PAD);
    assert!(has_control(&h, "Coarse"));
    assert!(!has_control(&h, "Medium"));
    h.get_by_label(COARSE_ONLY);

    set_test_available(None);
    let h = simulate(PAD);
    for name in ["Coarse", "Medium", "Fine"] {
        assert!(has_control(&h, name), "{name}");
    }
}

#[test]
fn a_new_quality_deletes_the_run() {
    set_test_available(None);
    let mut h = simulate(PAD);
    click(&mut h, RUN);
    run_to(&mut h, 6);
    click(&mut h, "Medium");
    assert_eq!(h.state().quality, 64);
    assert!(h.state().run.borrow().sim.is_none());
    assert!(h.state().means().is_none());
}
