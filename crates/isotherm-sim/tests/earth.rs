//! The Earth preset against observations. The terrain is the sample world
//! of `isotherm-app`, from ETOPO.

use std::time::Instant;

use isotherm_import::Scale;
use isotherm_sim::{Model, STEPS_PER_YEAR};
use isotherm_world::{Planet, World};

const EARTH_PNG: &[u8] = include_bytes!("../../isotherm-app/data/earth.png");

/// The years before the measured year.
const SPIN_UP_YEARS: u64 = 12;

fn earth() -> World {
    let samples = isotherm_import::read_image("earth.png", EARTH_PNG).unwrap();
    let terrain = isotherm_import::to_terrain(&samples, Scale::SIGNED_16, 128).unwrap();
    World {
        name: "Earth".to_owned(),
        planet: Planet::earth(),
        terrain,
    }
}

/// The yearly mean of the sea-level temperature after the spin-up, in °C,
/// and the yearly mean of the outgoing infrared, in W/m².
fn run(model: &mut Model) -> (Vec<f64>, f64) {
    model.run(SPIN_UP_YEARS * u64::from(STEPS_PER_YEAR));
    let n = model.grid().len();
    let mut mean = vec![0.0; n];
    let mut olr = 0.0;
    let k = f64::from(STEPS_PER_YEAR);
    for _ in 0..STEPS_PER_YEAR {
        model.step();
        for (m, t) in mean.iter_mut().zip(model.sea_level_temperature_k()) {
            *m += (t - 273.15) / k;
        }
        olr += model.mean_olr() / k;
    }
    (mean, olr)
}

/// `T0` and `T2` of the fit `T0 + T2 P2(sin φ)` over the northern
/// hemisphere, weighted by area.
fn legendre_fit(model: &Model, t: &[f64]) -> (f64, f64) {
    let grid = model.grid();
    let (mut sw, mut sp, mut spp, mut st, mut spt) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, &t) in t.iter().enumerate() {
        let (lat, _) = grid.lat_lon(i);
        if lat <= 0.0 {
            continue;
        }
        let w = grid.area(i);
        let x = lat.sin();
        let p2 = 1.5 * x * x - 0.5;
        sw += w;
        sp += w * p2;
        spp += w * p2 * p2;
        st += w * t;
        spt += w * p2 * t;
    }
    let t2 = (sw * spt - sp * st) / (sw * spp - sp * sp);
    ((st - t2 * sp) / sw, t2)
}

/// The mean of each 30° band of latitude, weighted by area.
fn bands(model: &Model, values: &[f32]) -> [f64; 6] {
    let grid = model.grid();
    let mut sum = [0.0; 6];
    let mut weight = [0.0; 6];
    for (i, &v) in values.iter().enumerate() {
        let (lat, _) = grid.lat_lon(i);
        let band = (((lat.to_degrees() + 90.0) / 30.0) as usize).min(5);
        sum[band] += f64::from(v) * grid.area(i);
        weight[band] += grid.area(i);
    }
    std::array::from_fn(|b| sum[b] / weight[b])
}

#[test]
fn the_pole_to_equator_difference_of_the_earth_is_within_5_c_of_observations() {
    // North, Cahalan, and Coakley (1981), Table 1: T0 = 14.9 °C and
    // T2 = −28.0 °C, so the equator is 1.5 |T2| = 42 °C warmer than the pole.
    let world = earth();
    let mut model = Model::new(&world, 32);
    let start = Instant::now();
    let (mean, olr) = run(&mut model);
    let years = (SPIN_UP_YEARS + 1) as f64;
    let (t0, t2) = legendre_fit(&model, &mean);
    let difference = 1.5 * t2.abs();
    eprintln!(
        "n = 32: T0 {t0:.2} °C, T2 {t2:.2} °C, difference {difference:.2} °C, OLR {olr:.1} W/m², {:.3} s per year",
        start.elapsed().as_secs_f64() / years
    );
    assert!((difference - 42.0).abs() < 5.0, "{difference}");
    assert!((olr - 239.0).abs() < 5.0, "{olr}");
}

#[test]
fn two_grid_sizes_give_the_same_zonal_means() {
    let world = earth();
    let years = 4 * u64::from(STEPS_PER_YEAR);
    let mut zonal = Vec::new();
    for n in [32, 64] {
        let mut model = Model::new(&world, n);
        let start = Instant::now();
        model.run(years);
        eprintln!(
            "n = {n}: {:.3} s per year",
            start.elapsed().as_secs_f64() / 4.0
        );
        let field = model.means().field(isotherm_fields::Output::Temperature);
        let year = field.get(isotherm_fields::Period::Year).unwrap();
        zonal.push(bands(&model, year));
    }
    eprintln!("{:?}\n{:?}", zonal[0], zonal[1]);
    for (a, b) in zonal[0].iter().zip(&zonal[1]) {
        assert!((a - b).abs() < 1.0, "{a} {b}");
    }
}
