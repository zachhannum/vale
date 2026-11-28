//! Snow, sea ice, and the albedo of each band of the light of the star.

use isotherm_fields::{Output, Period};
use isotherm_sim::{Grid, Model, Params, STEPS_PER_YEAR};
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

#[test]
fn the_degree_days_follow_the_spread_of_the_temperature() {
    let p = Params::earth();
    let sd = p.temperature_sd_k;
    // At a mean of 0 °C, the mean of the positive half of a normal spread.
    let zero = p.positive_degrees(273.15);
    assert!(
        (zero - sd / (2.0 * std::f64::consts::PI).sqrt()).abs() < 1e-6,
        "{zero}"
    );
    assert!((p.positive_degrees(273.15 + 25.0) - 25.0).abs() < 1e-3);
    assert!(p.positive_degrees(273.15 - 25.0) < 1e-4);
    assert!((p.snow_share(p.rain_snow_k) - 0.5).abs() < 1e-6);
    assert!(p.snow_share(p.rain_snow_k - 20.0) > 0.999);
}

#[test]
fn rough_terrain_has_less_snow_cover() {
    let p = Params::earth();
    assert_eq!(p.snow_cover(0.0, 0.0), 0.0);
    assert!((p.snow_cover(1000.0, 0.0) - 0.95).abs() < 1e-6);
    assert!(p.snow_cover(50.0, 2000.0) < 0.5 * p.snow_cover(50.0, 0.0));
}

#[test]
fn equatorial_mountains_have_ice_only_above_5000_m() {
    // A belt of land along the equator in steps of 60° of longitude, in a
    // sea 3,000 m deep. Tropical glaciers have their equilibrium line near
    // 4,900 to 5,400 m.
    let heights = [10.0, 2000.0, 3000.0, 4000.0, 5000.0, 6000.0];
    let step = |lon: f64| (((lon.to_degrees() + 180.0) / 60.0) as usize).min(5);
    let n = 32;
    let mut world = World::flat("Belt", Planet::earth(), n);
    let mut map = Heightmap::new(n, world.terrain.level(-3000.0));
    let grid = Grid::new(n);
    for i in 0..grid.len() {
        let (lat, lon) = grid.lat_lon(i);
        if lat.to_degrees().abs() < 12.0 {
            let (face, texel) = (i / (n * n), i % (n * n));
            map.set(
                face,
                texel % n,
                texel / n,
                world.terrain.level(heights[step(lon)]),
            );
        }
    }
    world.terrain.heights = map;
    let mut model = Model::new(&world, n);
    model.run(10 * u64::from(STEPS_PER_YEAR));
    let year = model
        .means()
        .field(Output::IceCover)
        .get(Period::Year)
        .unwrap();
    let mut sum = [0.0; 6];
    let mut count = [0.0; 6];
    for (i, &v) in year.iter().enumerate() {
        let (lat, lon) = grid.lat_lon(i);
        if model.land()[i] && lat.to_degrees().abs() < 8.0 {
            sum[step(lon)] += f64::from(v);
            count[step(lon)] += 1.0;
        }
    }
    let cover: [f64; 6] = std::array::from_fn(|k| sum[k] / count[k]);
    assert!(cover[..5].iter().all(|&c| c < 0.05), "{cover:?}");
    assert!(cover[5] > 0.5, "{cover:?}");
}
