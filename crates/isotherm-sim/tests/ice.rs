//! Snow, sea ice, and the albedo of each band of the light of the star.

use isotherm_fields::{Output, Period};
use isotherm_sim::{Model, Params, STEPS_PER_YEAR};
use isotherm_world::{Heightmap, Planet, World};

const FACE: usize = 16;

/// A world with all land at sea level.
fn land() -> World {
    let mut world = World::flat("Land", Planet::earth(), FACE);
    let level = world.terrain.level(0.0);
    world.terrain.heights = Heightmap::new(FACE, level);
    world
}

/// A world with sea over the whole surface.
fn ocean(planet: Planet) -> World {
    let mut world = World::flat("Ocean", planet, FACE);
    world.terrain.sea_level_m = 100.0;
    world
}

/// The mean ice cover of a period over the cells between two latitudes,
/// weighted by area.
fn cover(model: &Model, period: Period, south: f64, north: f64) -> f64 {
    let grid = model.grid();
    let values = model.means().field(Output::IceCover).get(period).unwrap();
    let (mut sum, mut weight) = (0.0, 0.0);
    for (i, &v) in values.iter().enumerate() {
        let lat = grid.lat_lon(i).0.to_degrees();
        if lat > south && lat < north {
            sum += f64::from(v) * grid.area(i);
            weight += grid.area(i);
        }
    }
    sum / weight
}

/// The yearly mean ice cover of the planet after `years` years.
fn yearly_ice(world: &World, params: Params, years: u64) -> f64 {
    let mut model = Model::with_params(world, FACE, params);
    model.run(years * u64::from(STEPS_PER_YEAR));
    cover(&model, Period::Year, -90.0, 90.0)
}

/// Params with the albedo of snow and ice equal to the albedo of the
/// surface under them.
fn no_feedback() -> Params {
    let mut p = Params::earth();
    p.albedo_snow = p.albedo_land;
    p.albedo_ice = p.albedo_sea;
    p
}

#[test]
fn snow_and_ice_reflect_less_of_the_light_of_a_red_dwarf() {
    let p = Params::earth();
    let sun = Planet::earth().star.visible_fraction();
    let mut star = Planet::earth().star;
    star.temperature_k = 3200.0;
    let red = star.visible_fraction();
    assert!(p.albedo(p.albedo_snow, red) < p.albedo(p.albedo_snow, sun) - 0.03);
    assert!(p.albedo(p.albedo_ice, red) < p.albedo(p.albedo_ice, sun) - 0.03);
    assert_eq!(p.albedo(p.albedo_sea, red), p.albedo(p.albedo_sea, sun));
}

#[test]
fn the_same_insolation_melts_more_ice_around_a_red_dwarf() {
    // The red dwarf has the luminosity and the mass of the Sun, so the
    // insolation and the orbit are the same.
    let sun = Planet::earth();
    let mut red = sun.clone();
    red.star.temperature_k = 3200.0;
    let years = 10;
    let ice_sun = yearly_ice(&ocean(sun), Params::earth(), years);
    let ice_red = yearly_ice(&ocean(red), Params::earth(), years);
    assert!(ice_sun > 0.02, "{ice_sun}");
    assert!(ice_red < 0.8 * ice_sun, "{ice_red} {ice_sun}");
}

#[test]
fn snow_grows_in_winter_and_melts_in_summer() {
    let mut model = Model::new(&land(), FACE);
    model.run(3 * u64::from(STEPS_PER_YEAR));
    let march = cover(&model, Period::Month(2), 45.0, 70.0);
    let august = cover(&model, Period::Month(7), 45.0, 70.0);
    assert!(march > 0.05, "{march}");
    assert!(august < 0.01, "{august}");
    // The south has its winter in the other half of the year.
    let south_march = cover(&model, Period::Month(2), -70.0, -45.0);
    let south_august = cover(&model, Period::Month(7), -70.0, -45.0);
    assert!(south_march < 0.01, "{south_march}");
    assert!(south_august > 0.05, "{south_august}");
}

#[test]
fn a_dimmer_star_gives_more_ice_than_a_straight_line_predicts() {
    let years = 12;
    let ice = |params: Params, factor: f64| {
        let mut planet = Planet::earth();
        planet.star.luminosity_w *= factor;
        yearly_ice(&ocean(planet), params, years)
    };
    let [a, b, c] = [1.0, 0.95, 0.9].map(|f| ice(Params::earth(), f));
    // The line through the first two points predicts the third.
    let line = 2.0 * b - a;
    assert!(c > line + 0.01, "{a} {b} {c}");
    // Without the feedback, the same dimming adds less ice.
    let [d, e] = [1.0, 0.9].map(|f| ice(no_feedback(), f));
    assert!(c - a > 1.5 * (e - d), "{a} {c} {d} {e}");
}
