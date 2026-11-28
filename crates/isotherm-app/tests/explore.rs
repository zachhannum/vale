//! The Explore workspace with no GPU: the month, the contours, the legend,
//! and the readout.

mod zonal;

use eframe::egui;
use egui_kittest::kittest::Queryable;
use isotherm_app::app::{
    CONTOURS, ICE_COVER, IsothermApp, Layout, MONTH, RUN_HINT, TEMPERATURE, Workspace,
};
use isotherm_app::explore::{self, Layer, READOUT};
use isotherm_app::headless::harness_with;
use isotherm_fields::{Means, Output, Period};
use vale_terrain::math::lonlat_to_dir;
use zonal::{FACE, zonal, zonal_ice, zonal_means, zonal_means_with_ice};

type H = egui_kittest::Harness<'static, IsothermApp>;

fn explore(size: (f32, f32), means: Option<Means>) -> H {
    let mut app = IsothermApp::sample(Layout::Pad).expect("the sample world reads");
    app.workspace = Workspace::Explore;
    app.set_means(means);
    let mut h = harness_with(size, app);
    h.run_steps(3);
    h
}

fn control(h: &H, name: &str) -> egui::Rect {
    let controls = &h.state().controls;
    let control = controls.iter().find(|c| c.name == name);
    control.unwrap_or_else(|| panic!("no control {name}")).rect
}

/// A drag of the mouse from `from` to `to`.
fn drag(h: &mut H, from: egui::Pos2, to: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(egui::Event::PointerMoved(from));
    h.event(button(from, true));
    h.step();
    for k in 1..=8 {
        let pos = from + (to - from) * (k as f32 / 8.0);
        h.event(egui::Event::PointerMoved(pos));
        h.step();
    }
    h.event(button(to, false));
    h.run_steps(2);
}

/// A tap of the mouse at `pos`.
fn tap(h: &mut H, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.event(egui::Event::PointerGone);
    h.run_steps(2);
}

#[test]
fn with_no_run_the_panel_asks_for_a_run() {
    let h = explore((1194.0, 834.0), None);
    h.get_by_label(RUN_HINT);
    assert!(h.query_by_label(MONTH).is_none());
    assert!(h.state().globe.overlay().is_none());
}

#[test]
fn the_panel_shows_the_layer_the_month_the_contours_and_the_legend() {
    let h = explore((1194.0, 834.0), Some(zonal_means()));
    h.get_by_label(TEMPERATURE);
    h.get_by_label(MONTH);
    h.get_by_label(CONTOURS);
    assert!(h.query_by_label(RUN_HINT).is_none());
    // The year shows first.
    assert_eq!(h.state().explore.period, Period::Year);
    assert_eq!(h.state().globe.overlay_uploads(), 1);
}

#[test]
fn the_legend_has_a_label_every_ten_degrees() {
    let bands = explore::temperature_bands();
    let values: Vec<f64> = bands.stops().iter().map(|s| s.value).collect();
    let expected: Vec<f64> = (0..12).map(|i| -60.0 + 10.0 * f64::from(i)).collect();
    assert_eq!(values, expected);
}

#[test]
fn the_month_slider_changes_the_overlay() {
    let mut h = explore((1194.0, 834.0), Some(zonal_means()));
    let year = h.state().globe.overlay().expect("an overlay").clone();
    let slider = control(&h, MONTH);
    drag(
        &mut h,
        slider.right_center(),
        slider.left_center() - egui::vec2(40.0, 0.0),
    );
    assert_eq!(h.state().explore.period, Period::Month(0));
    let january = h.state().globe.overlay().expect("an overlay");
    assert!(h.state().globe.overlay_uploads() > 1);
    assert_ne!(january.levels, year.levels);
    // January is colder than the year: the levels are lower.
    let sum = |l: &[u16]| l.iter().map(|&v| u64::from(v)).sum::<u64>();
    assert!(sum(&january.levels) < sum(&year.levels));
}

#[test]
fn the_contours_switch_turns_the_lines_off() {
    let mut h = explore((1194.0, 834.0), Some(zonal_means()));
    assert!(h.state().globe.contours);
    h.get_by_label(CONTOURS).click();
    h.run_steps(2);
    assert!(!h.state().explore.contours);
    assert!(!h.state().globe.contours);
}

#[test]
fn a_tap_on_the_globe_shows_the_value_of_its_cell() {
    let mut h = explore((1194.0, 834.0), Some(zonal_means()));
    let (lon, lat) = isotherm_app::globe::START;
    let globe = &h.state().globe;
    let pos = globe
        .view
        .project(globe.rect, lonlat_to_dir(lon, lat))
        .expect("visible");
    tap(&mut h, pos);
    let app = h.state();
    let place = app.explore.pin.expect("a pinned place");
    let field = app.means().unwrap().field(Output::Temperature);
    let value = field.get(Period::Year).unwrap()[explore::cell_of(FACE, place)];
    // The mean of the twelve months is the field of June and July.
    let expected = zonal(lat.to_radians(), 0) + 5.5;
    assert!(
        (f64::from(value) - expected).abs() < 3.0,
        "{value} {expected}"
    );
    let text = app.readout().expect("a readout");
    assert!(text.starts_with(&explore::celsius_text(value)), "{text}");
    assert!(text.contains("° N"), "{text}");
    assert!(app.cards.iter().any(|(id, _)| *id == READOUT));
}

#[test]
fn the_phone_sheet_holds_the_explore_rows() {
    let h = explore((393.0, 852.0), Some(zonal_means()));
    h.get_by_label(MONTH);
    h.get_by_label(CONTOURS);
    assert!(h.state().cards.iter().any(|(id, _)| *id == "sheet"));
}

#[test]
fn another_workspace_hides_the_overlay() {
    let mut h = explore((1194.0, 834.0), Some(zonal_means()));
    assert!(h.state().globe.overlay().is_some());
    h.get_by_label("Terrain").click();
    h.run_steps(2);
    assert!(h.state().globe.overlay().is_none());
}

/// The texts that the last frame painted.
fn painted_texts(h: &H) -> Vec<String> {
    h.output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_layer_switch_shows_the_ice_cover_with_its_legend_and_readout() {
    let mut h = explore((1194.0, 834.0), Some(zonal_means_with_ice()));
    assert!(painted_texts(&h).iter().any(|t| t == "°C"));
    let temperature = h.state().globe.overlay().expect("an overlay").clone();
    h.get_by_label(ICE_COVER).click();
    h.run_steps(2);
    assert_eq!(h.state().explore.layer, Layer::IceCover);
    let ice = h.state().globe.overlay().expect("an overlay").clone();
    assert_eq!(ice.bands, explore::ice_bands());
    assert_ne!(ice.levels, temperature.levels);
    let texts = painted_texts(&h);
    assert!(texts.iter().any(|t| t == "%"), "{texts:?}");
    assert!(!texts.iter().any(|t| t == "°C"), "{texts:?}");

    let (lon, lat) = isotherm_app::globe::START;
    let globe = &h.state().globe;
    let pos = globe
        .view
        .project(globe.rect, lonlat_to_dir(lon, lat))
        .expect("visible");
    tap(&mut h, pos);
    let app = h.state();
    let place = app.explore.pin.expect("a pinned place");
    let field = app.means().unwrap().field(Output::IceCover);
    let value = field.get(Period::Year).unwrap()[explore::cell_of(FACE, place)];
    assert!((f64::from(value) - zonal_ice(lat.to_radians())).abs() < 0.1);
    let text = app.readout().expect("a readout");
    assert!(
        text.starts_with(&Layer::IceCover.value_text(value)),
        "{text}"
    );
    assert!(text.contains(" % ice · "), "{text}");

    // The contours follow the stops of the ice cover.
    assert!(h.state().globe.contours);
    h.get_by_label(TEMPERATURE).click();
    h.run_steps(2);
    let back = h.state().globe.overlay().expect("an overlay");
    assert_eq!(back.levels, temperature.levels);
}

#[test]
fn a_run_with_no_ice_cover_shows_an_empty_period() {
    let mut h = explore((1194.0, 834.0), Some(zonal_means()));
    h.get_by_label(ICE_COVER).click();
    h.run_steps(2);
    assert!(h.state().globe.overlay().is_none());
    h.get_by_label(isotherm_app::app::EMPTY_PERIOD);
}
