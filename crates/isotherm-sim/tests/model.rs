//! The energy balance model on flat worlds.

use isotherm_fields::{Output, Period};
use isotherm_sim::{Model, Params, STEPS_PER_YEAR};
use isotherm_world::{Heightmap, Planet, World};

/// A world with all land at `height_m`.
fn land(height_m: f64) -> World {
    let mut world = World::flat("Land", Planet::earth(), 16);
    let level = world.terrain.level(height_m);
    world.terrain.heights = Heightmap::new(16, level);
    world
}

/// A world with sea over the whole surface.
fn ocean(planet: Planet) -> World {
    let mut world = World::flat("Ocean", planet, 16);
    world.terrain.sea_level_m = 100.0;
    world
}

fn bits(model: &Model) -> Vec<u64> {
    let t = model.sea_level_temperature_k().iter();
    t.chain(model.ice_m()).map(|x| x.to_bits()).collect()
}

/// The monthly means of the temperature over the cells between two
/// latitudes, in degrees.
fn band_months(model: &Model, south: f64, north: f64) -> [f64; 12] {
    let grid = model.grid();
    let field = model.means().field(Output::Temperature);
    std::array::from_fn(|m| {
        let values = field.get(Period::Month(m)).unwrap();
        let (mut sum, mut weight) = (0.0, 0.0);
        for (i, &v) in values.iter().enumerate() {
            let lat = grid.lat_lon(i).0.to_degrees();
            if lat > south && lat < north {
                sum += f64::from(v) * grid.area(i);
                weight += grid.area(i);
            }
        }
        sum / weight
    })
}

fn peak(months: &[f64; 12]) -> usize {
    (0..12)
        .max_by(|&a, &b| months[a].total_cmp(&months[b]))
        .unwrap()
}

#[test]
fn the_same_steps_give_the_same_bits() {
    let world = ocean(Planet::earth());
    let mut a = Model::new(&world, 16);
    let mut b = Model::new(&world, 16);
    a.run(50);
    for _ in 0..50 {
        b.step();
    }
    assert_eq!(bits(&a), bits(&b));
    assert_eq!(a.means(), b.means());
}

#[test]
fn a_restored_run_continues_with_the_same_bits() {
    let world = ocean(Planet::earth());
    let mut a = Model::new(&world, 16);
    a.run(45);
    let mut b = Model::restore(&world, a.snapshot()).unwrap();
    assert_eq!(b.step_count(), 45);
    a.run(20);
    b.run(20);
    assert_eq!(bits(&a), bits(&b));
    assert_eq!(a.means(), b.means());
    let mut wrong = a.snapshot();
    wrong.ice_m.pop();
    assert!(Model::restore(&world, wrong).is_err());
}

#[test]
fn a_cell_at_1000_m_is_6_5_k_colder_than_at_sea_level() {
    let world = land(1000.0);
    let mut model = Model::new(&world, 16);
    model.run(10);
    let mut surface = vec![0.0; model.grid().len()];
    model.surface_temperature_c(&mut surface);
    for (i, ts) in surface.iter().enumerate() {
        let z = model.height_m()[i];
        assert!((z - 1000.0).abs() < 0.5, "{z}");
        let t = model.sea_level_temperature_k()[i] - 273.15;
        assert!((t - ts - 6.5).abs() < 0.01, "{t} {ts}");
    }
}

#[test]
fn the_ocean_lags_the_land_and_has_a_smaller_seasonal_range() {
    let mut l = Model::new(&land(0.0), 16);
    let mut o = Model::new(&ocean(Planet::earth()), 16);
    l.run(4 * u64::from(STEPS_PER_YEAR));
    o.run(4 * u64::from(STEPS_PER_YEAR));
    let lm = band_months(&l, 35.0, 55.0);
    let om = band_months(&o, 35.0, 55.0);
    let range = |m: &[f64; 12]| {
        m.iter().copied().fold(f64::MIN, f64::max) - m.iter().copied().fold(f64::MAX, f64::min)
    };
    assert!(range(&lm) > 2.0 * range(&om), "{lm:?} {om:?}");
    assert!(peak(&om) > peak(&lm), "{lm:?} {om:?}");
}

#[test]
fn sea_ice_grows_in_winter_and_melts_in_summer() {
    let mut o = Model::new(&ocean(Planet::earth()), 16);
    o.run(4 * u64::from(STEPS_PER_YEAR));
    let grid = o.grid();
    let ice = o.means().field(Output::IceCover);
    let cover = |m: usize| {
        let values = ice.get(Period::Month(m)).unwrap();
        let (mut sum, mut weight) = (0.0, 0.0);
        for (i, &v) in values.iter().enumerate() {
            if grid.lat_lon(i).0.to_degrees() > 60.0 {
                sum += f64::from(v) * grid.area(i);
                weight += grid.area(i);
            }
        }
        sum / weight
    };
    assert!(cover(2) > cover(8) + 0.1, "{} {}", cover(2), cover(8));
}

#[test]
fn the_optical_depth_rises_with_pressure_and_each_gas() {
    let p = Params::earth();
    let base = Planet::earth().atmosphere;
    let tau = p.optical_depth(&base, 0.008);
    let mut more = base;
    more.surface_pressure_pa *= 2.0;
    assert!(p.optical_depth(&more, 0.008) > tau);
    let mut more = base;
    more.co2_ppm *= 2.0;
    assert!(p.optical_depth(&more, 0.008) > tau);
    let mut more = base;
    more.ch4_ppm *= 2.0;
    assert!(p.optical_depth(&more, 0.008) > tau);
    assert!(p.optical_depth(&base, 0.016) > tau);
}

#[test]
fn twice_the_co2_warms_the_planet_by_1_to_4_k() {
    let mean = |planet: Planet| {
        let mut model = Model::new(&ocean(planet), 16);
        model.run(15 * u64::from(STEPS_PER_YEAR));
        let year = model
            .means()
            .field(Output::Temperature)
            .get(Period::Year)
            .unwrap();
        let values: Vec<f64> = year.iter().map(|&v| f64::from(v)).collect();
        model.global_mean(&values)
    };
    let earth = Planet::earth();
    let mut doubled = earth.clone();
    doubled.atmosphere.co2_ppm *= 2.0;
    let warming = mean(doubled) - mean(earth);
    assert!((1.0..=4.0).contains(&warming), "{warming}");
}
