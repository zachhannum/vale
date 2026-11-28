//! The insolation against published values for the Earth (Hartmann 2016,
//! Berger 1978) and against the geometry of tidally locked and resonant
//! worlds.

use std::f64::consts::{FRAC_PI_2, PI};

use isotherm_sim::STEPS_PER_YEAR;
use isotherm_sim::grid::Grid;
use isotherm_sim::sun::{Sky, insolation};
use isotherm_world::Planet;

/// The time when the true longitude crosses `target`, in seconds.
fn time_of_longitude(sky: &Sky, target: f64) -> f64 {
    let p = sky.period_s();
    let k = 3650;
    let ahead = |t: f64| (sky.sun_at(t).true_longitude - target).rem_euclid(2.0 * PI) < PI;
    let mut t0 = 0.0;
    for i in 0..k {
        let t = i as f64 / k as f64 * p;
        let t1 = t + p / k as f64;
        if !ahead(t) && ahead(t1) {
            t0 = t;
            break;
        }
    }
    let (mut lo, mut hi) = (t0, t0 + p / k as f64);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if ahead(mid) { hi = mid } else { lo = mid }
    }
    lo
}

/// The daily mean insolation at a point on a day centered on `t`.
fn daily(planet: &Planet, sky: &Sky, t: f64, lat: f64) -> f64 {
    let day = planet.solar_day_s();
    sky.interval(t - day / 2.0, day).insolation(lat, 0.0)
}

/// The yearly mean of a point, from the steps of the model.
fn yearly(sky: &Sky, steps: u32, lat: f64, lon: f64) -> f64 {
    let dt = sky.period_s() / f64::from(STEPS_PER_YEAR);
    (0..steps)
        .map(|k| sky.interval(f64::from(k) * dt, dt).insolation(lat, lon))
        .sum::<f64>()
        / f64::from(steps)
}

#[test]
fn the_north_pole_at_the_june_solstice_gets_525_w_m2() {
    let planet = Planet::earth();
    let sky = Sky::new(&planet);
    let t = time_of_longitude(&sky, FRAC_PI_2);
    let q = daily(&planet, &sky, t, FRAC_PI_2);
    assert!((q - 524.7).abs() < 3.0, "{q}");
}

#[test]
fn the_south_pole_at_the_december_solstice_gets_more_near_perihelion() {
    let planet = Planet::earth();
    let sky = Sky::new(&planet);
    let t = time_of_longitude(&sky, 3.0 * FRAC_PI_2);
    let q = daily(&planet, &sky, t, -FRAC_PI_2);
    assert!((q - 560.0).abs() < 3.0, "{q}");
}

#[test]
fn the_year_starts_in_january() {
    let sky = Sky::new(&Planet::earth());
    let equinox = time_of_longitude(&sky, 0.0) / sky.period_s() * 365.25;
    assert!((equinox - 79.0).abs() < 0.5, "{equinox}");
}

#[test]
fn the_global_yearly_mean_is_a_quarter_of_s0() {
    let planet = Planet::earth();
    let sky = Sky::new(&planet);
    let grid = Grid::new(32);
    let dt = sky.period_s() / f64::from(STEPS_PER_YEAR);
    let mut q = vec![0.0; grid.len()];
    let mut sum = 0.0;
    for k in 0..STEPS_PER_YEAR {
        insolation(&sky, &grid, f64::from(k) * dt, dt, &mut q);
        sum += (0..grid.len()).map(|i| q[i] * grid.area(i)).sum::<f64>();
    }
    let mean = sum / f64::from(STEPS_PER_YEAR) / (4.0 * PI);
    let e = planet.orbit.eccentricity;
    let expected = sky.s_star / (4.0 * (1.0 - e * e).sqrt());
    assert!((mean / expected - 1.0).abs() < 1e-3, "{mean} {expected}");
    assert!((expected - 340.3).abs() < 0.5, "{expected}");
}

#[test]
fn the_yearly_mean_at_the_pole_is_172_w_m2() {
    let sky = Sky::new(&Planet::earth());
    let q = yearly(&sky, STEPS_PER_YEAR, FRAC_PI_2, 0.0);
    assert!((q / 172.4 - 1.0).abs() < 0.01, "{q}");
}

#[test]
fn a_tilt_of_90_degrees_gives_the_poles_more_than_the_equator() {
    let mut planet = Planet::earth();
    planet.rotation.tilt_deg = 90.0;
    let sky = Sky::new(&planet);
    let pole = yearly(&sky, STEPS_PER_YEAR, FRAC_PI_2, 0.0);
    let equator = yearly(&sky, STEPS_PER_YEAR, 0.0, 0.0);
    assert!(pole > equator, "{pole} {equator}");
}

#[test]
fn a_tilt_of_180_degrees_gives_the_yearly_means_of_no_tilt() {
    let mut zero = Planet::earth();
    zero.rotation.tilt_deg = 0.0;
    let mut flipped = zero.clone();
    flipped.rotation.tilt_deg = 180.0;
    for lat in [-1.2, -0.5, 0.0, 0.7, 1.4] {
        let a = yearly(&Sky::new(&zero), STEPS_PER_YEAR, lat, 0.3);
        let b = yearly(&Sky::new(&flipped), STEPS_PER_YEAR, lat, 0.3);
        assert!((a - b).abs() < 0.01 * a.max(1.0), "{lat}: {a} {b}");
    }
}

fn locked() -> Planet {
    let mut planet = Planet::earth();
    planet.orbit.eccentricity = 0.0;
    planet.rotation.tilt_deg = 0.0;
    planet.rotation.sidereal_day_s = planet.orbital_period_s();
    planet
}

#[test]
fn a_tidally_locked_world_has_a_fixed_day_side() {
    let planet = locked();
    let sky = Sky::new(&planet);
    let grid = Grid::new(32);
    let dt = sky.period_s() / f64::from(STEPS_PER_YEAR);
    let mut first = vec![0.0; grid.len()];
    let mut later = vec![0.0; grid.len()];
    insolation(&sky, &grid, 0.0, dt, &mut first);
    insolation(&sky, &grid, 180.0 * dt, dt, &mut later);
    let sub = grid.cell_of([1.0, 0.0, 0.0]);
    let anti = grid.cell_of([-1.0, 0.0, 0.0]);
    assert!(
        (first[sub] / sky.s_star - 1.0).abs() < 0.005,
        "{}",
        first[sub]
    );
    assert_eq!(first[anti], 0.0);
    let mean = (0..grid.len())
        .map(|i| first[i] * grid.area(i))
        .sum::<f64>()
        / (4.0 * PI);
    assert!((mean / (sky.s_star / 4.0) - 1.0).abs() < 0.005, "{mean}");
    for (a, b) in first.iter().zip(&later) {
        assert!((a - b).abs() < 1e-6 * sky.s_star);
    }
}

#[test]
fn a_3_2_resonance_repeats_after_two_orbits_and_has_hot_longitudes() {
    let mut planet = Planet::earth();
    planet.orbit.eccentricity = 0.2056;
    planet.rotation.tilt_deg = 0.0;
    planet.rotation.sidereal_day_s = planet.orbital_period_s() * 2.0 / 3.0;
    let sky = Sky::new(&planet);
    let dt = sky.period_s() / f64::from(STEPS_PER_YEAR);
    let at = |k: u32, lon: f64| sky.interval(f64::from(k) * dt, dt).insolation(0.0, lon);
    let mut differs = false;
    for k in [10, 77, 200, 333] {
        for lon in [0.0, 1.0, 2.5] {
            let a = at(k, lon);
            assert!((a - at(k + 2 * STEPS_PER_YEAR, lon)).abs() < 1e-6 * sky.s_star);
            differs |= (a - at(k + STEPS_PER_YEAR, lon)).abs() > 0.1 * sky.s_star;
        }
    }
    assert!(differs);
    let hot = yearly(&sky, 2 * STEPS_PER_YEAR, 0.0, 0.0);
    let cool = yearly(&sky, 2 * STEPS_PER_YEAR, 0.0, FRAC_PI_2);
    assert!(hot > 1.3 * cool, "{hot} {cool}");
}
