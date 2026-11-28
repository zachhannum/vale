//! The energy balance model of tier 1.
//!
//! Each cell has a temperature at sea level. A sea cell has an ice
//! thickness, and a land cell has a snow mass.
//! Each step solves the energy balance of all cells together with one
//! linearized backward Euler step, so a step of any length is stable.

use isotherm_fields::{Means, Output};
use isotherm_world::{Atmosphere, Planet, World};
use vale_terrain::TexelRect;

use crate::grid::{FACES, Grid};
use crate::solver::Solver;
use crate::sun::{Sky, insolation};
use crate::{STEPS_PER_MONTH, STEPS_PER_YEAR};

/// The Stefan–Boltzmann constant, in W/(m² K⁴).
pub const SIGMA: f64 = 5.670_374e-8;

/// The version of the state of a run. A saved run of another version does
/// not open.
pub const MODEL_VERSION: u32 = 2;

/// The free constants of the model, with the values of the Earth preset.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// The reference surface pressure, in Pa.
    pub p0_pa: f64,
    /// The heat capacity of air, in J/(kg K).
    pub cp: f64,
    /// The latent heat of vaporization, in J/kg.
    pub lv: f64,
    /// The relative humidity of the air.
    pub relative_humidity: f64,
    /// The heat capacity of the mixed layer of the ocean, in J/(m² K).
    pub ocean_capacity: f64,
    /// The heat capacity of the top layer of the ground, in J/(m² K).
    pub land_capacity: f64,
    /// The freezing point of sea water, in K.
    pub freezing_k: f64,
    /// The melting point of ice and snow, in K.
    pub melting_k: f64,
    /// The conductivity of sea ice, in W/(m K).
    pub ice_conductivity: f64,
    /// The density of sea ice, in kg/m³.
    pub ice_density: f64,
    /// The latent heat of fusion, in J/kg.
    pub fusion: f64,
    /// The smallest thickness in the conduction through sea ice, in m.
    pub min_ice_m: f64,
    /// The thickness of sea ice that covers a whole cell, in m.
    pub cover_ice_m: f64,
    /// The albedo of the sea, of open ground, of snow, and of sea ice, in
    /// the visible band and in the infrared band.
    pub albedo_sea: [f64; 2],
    pub albedo_land: [f64; 2],
    pub albedo_snow: [f64; 2],
    pub albedo_ice: [f64; 2],
    /// The share of the light of the star that the air and the clouds
    /// reflect.
    pub air_reflectance: f64,
    /// The share of the light of the star that goes down through the air
    /// and the clouds and up again.
    pub air_transmission: f64,
    /// The snowfall on land below the melting point, at the saturation
    /// humidity of 0 °C, in kg/(m² s). The snowfall is in proportion to
    /// the saturation humidity.
    pub snowfall: f64,
    /// The snow of a full snow cover, in kg/m².
    pub snow_cover_kg: f64,
    /// The most snow that a cell keeps, in kg/m².
    pub max_snow_kg: f64,
    /// The lapse rate at the gravity of the Earth, in K/m.
    pub lapse_k_per_m: f64,
    /// The optical depth of a dry atmosphere with the gases of the Earth.
    pub tau0: f64,
    /// The optical depth of each e-fold of CO2.
    pub k_co2: f64,
    /// The optical depth of each square root of a ppm of CH4.
    pub k_ch4: f64,
    /// The optical depth of each e-fold of `1 + q / q_ref`, with `q` the
    /// specific humidity.
    pub k_h2o: f64,
    /// The specific humidity of the scale of the water vapor term, in kg/kg.
    pub q_ref: f64,
    /// The reference amounts of CO2 and CH4, in ppm.
    pub co2_ref_ppm: f64,
    pub ch4_ref_ppm: f64,
    /// The exponent of the pressure in the optical depth.
    pub pressure_exponent: f64,
    /// The diffusivity of moist static energy on the Earth, in m²/s.
    pub d0: f64,
    /// The largest factor of the diffusivity from a slow rotation.
    pub max_rotation_factor: f64,
    /// The rotation rate of the Earth, in rad/s.
    pub omega0: f64,
    /// The gravity of the Earth, in m/s².
    pub g0: f64,
}

impl Params {
    pub fn earth() -> Params {
        Params {
            p0_pa: 101_325.0,
            cp: 1004.0,
            lv: 2.5e6,
            relative_humidity: 0.8,
            ocean_capacity: 1025.0 * 3990.0 * 50.0,
            land_capacity: 2.4e6 * 2.0,
            freezing_k: 271.35,
            melting_k: 273.15,
            ice_conductivity: 2.0,
            ice_density: 917.0,
            fusion: 3.34e5,
            min_ice_m: 0.05,
            cover_ice_m: 1.0,
            albedo_sea: [0.06, 0.06],
            albedo_land: [0.10, 0.30],
            albedo_snow: [0.96, 0.68],
            albedo_ice: [0.73, 0.33],
            air_reflectance: 0.25,
            air_transmission: 0.37,
            snowfall: 0.5 / 86_400.0,
            snow_cover_kg: 100.0,
            max_snow_kg: 1000.0,
            lapse_k_per_m: 6.5e-3,
            tau0: 0.276,
            k_co2: 5.35 / 110.0,
            k_ch4: 1.14 / 110.0,
            k_h2o: 0.25,
            q_ref: 1e-3,
            co2_ref_ppm: 280.0,
            ch4_ref_ppm: 0.722,
            pressure_exponent: 1.0,
            d0: 0.9e6,
            max_rotation_factor: 100.0,
            omega0: 7.292e-5,
            g0: 9.81,
        }
    }

    /// The albedo of a surface at the top of the atmosphere, with the share
    /// `visible` of the light in the visible band.
    pub fn albedo(&self, surface: [f64; 2], visible: f64) -> f64 {
        let surface = visible * surface[0] + (1.0 - visible) * surface[1];
        self.air_reflectance + self.air_transmission * surface
    }

    /// The optical depth of the gases other than water vapor.
    pub fn dry_optical_depth(&self, atmosphere: &Atmosphere) -> f64 {
        let co2 = self.k_co2 * (atmosphere.co2_ppm.max(1e-3) / self.co2_ref_ppm).ln();
        let ch4 = self.k_ch4 * (atmosphere.ch4_ppm.max(0.0).sqrt() - self.ch4_ref_ppm.sqrt());
        (self.tau0 + co2 + ch4).max(0.0)
    }

    /// The infrared optical depth of the atmosphere with the specific
    /// humidity `q`.
    pub fn optical_depth(&self, atmosphere: &Atmosphere, q: f64) -> f64 {
        let pressure = (atmosphere.surface_pressure_pa / self.p0_pa).powf(self.pressure_exponent);
        pressure * (self.dry_optical_depth(atmosphere) + self.k_h2o * (1.0 + q / self.q_ref).ln())
    }
}

/// The saturation specific humidity and its derivative at `t`, in K, after
/// Bolton (1980).
fn saturation(t: f64, pressure_pa: f64) -> (f64, f64) {
    let t = t.max(100.0);
    let es = 611.2 * (17.67 * (t - 273.15) / (t - 29.65)).exp();
    let q = (0.622 * es / pressure_pa).min(1.0);
    (q, q * 17.67 * 243.5 / ((t - 29.65) * (t - 29.65)))
}

/// The state of a run that a save keeps.
#[derive(Clone, Debug, PartialEq)]
pub struct RunState {
    pub face_size: usize,
    pub step: u64,
    /// The temperature at sea level of each cell, in K.
    pub temperature_k: Vec<f64>,
    /// The thickness of the sea ice of each cell, in m.
    pub ice_m: Vec<f64>,
    /// The snow of each cell, in kg/m².
    pub snow_kg: Vec<f64>,
    pub means: Means,
}

impl RunState {
    pub fn heap_bytes(&self) -> u64 {
        ((self.temperature_k.capacity() + self.ice_m.capacity() + self.snow_kg.capacity()) * 8)
            as u64
            + self.means.heap_bytes()
    }
}

/// The f64 buffers of a model for each cell, other than the solver.
pub const F64_PER_CELL: usize = 13;

/// A run of the model on one world.
pub struct Model {
    params: Params,
    sky: Sky,
    grid: Grid,
    solver: Solver,
    atmosphere: Atmosphere,
    dt: f64,
    /// `p_s D / (g R²)`, the factor of the Laplacian of moist static energy.
    diffusion: f64,
    air_capacity: f64,
    lapse: f64,
    /// The pressure factor of the optical depth and the dry optical depth.
    pressure_factor: f64,
    dry_tau: f64,
    /// The albedo of each surface at the top of the atmosphere, for the
    /// light of the star.
    albedo_sea: f64,
    albedo_land: f64,
    albedo_snow: f64,
    albedo_ice: f64,
    land: Vec<bool>,
    height_m: Vec<f64>,
    temperature: Vec<f64>,
    ice: Vec<f64>,
    snow: Vec<f64>,
    sun: Vec<f64>,
    h: Vec<f64>,
    dh: Vec<f64>,
    diag: Vec<f64>,
    rhs: Vec<f64>,
    delta: Vec<f64>,
    lap: Vec<f64>,
    out_t: Vec<f64>,
    out_ice: Vec<f64>,
    means: Means,
    step: u64,
    iterations: usize,
}

impl Model {
    /// A run that starts at the local radiative balance of each cell.
    pub fn new(world: &World, face_size: usize) -> Model {
        Model::with_params(world, face_size, Params::earth())
    }

    pub fn with_params(world: &World, face_size: usize, params: Params) -> Model {
        let mut model = Model::build(world, face_size, params);
        model.start();
        model
    }

    fn build(world: &World, face_size: usize, params: Params) -> Model {
        let planet: &Planet = &world.planet;
        let grid = Grid::new(face_size);
        let n = grid.len();
        let sky = Sky::new(planet);
        let atmosphere = planet.atmosphere;
        let ps = atmosphere.surface_pressure_pa;
        let g = planet.gravity;
        let omega = std::f64::consts::TAU / planet.rotation.sidereal_day_s.abs();
        let rotation = ((params.omega0 / omega).powi(2)).min(params.max_rotation_factor);
        let d = params.d0 * (ps / params.p0_pa) * rotation;
        let r = planet.radius_m;
        let (land, height_m) = sample_terrain(world, &grid);
        let zeros = || vec![0.0; n];
        let visible = planet.star.visible_fraction();
        Model {
            albedo_sea: params.albedo(params.albedo_sea, visible),
            albedo_land: params.albedo(params.albedo_land, visible),
            albedo_snow: params.albedo(params.albedo_snow, visible),
            albedo_ice: params.albedo(params.albedo_ice, visible),
            sky,
            solver: Solver::new(&grid),
            atmosphere,
            dt: sky.period_s() / f64::from(STEPS_PER_YEAR),
            diffusion: ps * d / (g * r * r),
            air_capacity: ps * params.cp / g,
            lapse: params.lapse_k_per_m * g / params.g0,
            pressure_factor: (ps / params.p0_pa).powf(params.pressure_exponent),
            dry_tau: params.dry_optical_depth(&atmosphere),
            land,
            height_m,
            temperature: zeros(),
            ice: zeros(),
            snow: zeros(),
            sun: zeros(),
            h: zeros(),
            dh: zeros(),
            diag: zeros(),
            rhs: zeros(),
            delta: zeros(),
            lap: zeros(),
            out_t: zeros(),
            out_ice: zeros(),
            means: Means::new(face_size, &Output::ALL),
            step: 0,
            iterations: 0,
            params,
            grid,
        }
    }

    /// Sets each cell to the balance of the yearly mean insolation and the
    /// outgoing infrared, with no snow and no ice, and with open water no
    /// colder than its freezing point.
    fn start(&mut self) {
        let samples = 72;
        let stride = STEPS_PER_YEAR / samples;
        self.rhs.fill(0.0);
        for k in 0..samples {
            let t0 = f64::from(k * stride) * self.dt;
            insolation(&self.sky, &self.grid, t0, self.dt, &mut self.sun);
            for (sum, s) in self.rhs.iter_mut().zip(&self.sun) {
                *sum += s / f64::from(samples);
            }
        }
        for i in 0..self.grid.len() {
            let z = self.height_m[i];
            let (mut lo, mut hi) = (100.0, 400.0);
            for _ in 0..50 {
                let ts = (lo + hi) / 2.0;
                let albedo = if self.land[i] {
                    self.albedo_land
                } else {
                    self.albedo_sea
                };
                let net = self.rhs[i] * (1.0 - albedo) - self.olr(ts).0;
                if net > 0.0 { lo = ts } else { hi = ts }
            }
            let mut t = lo + self.lapse * z;
            if !self.land[i] {
                t = t.max(self.params.freezing_k);
            }
            self.temperature[i] = t;
        }
        self.ice.fill(0.0);
        self.snow.fill(0.0);
    }

    /// The share of a cell under snow or sea ice.
    fn cover(&self, i: usize) -> f64 {
        if self.land[i] {
            (self.snow[i] / self.params.snow_cover_kg).min(1.0)
        } else {
            (self.ice[i] / self.params.cover_ice_m).min(1.0)
        }
    }

    /// The albedo of a cell at the top of the atmosphere.
    fn albedo(&self, i: usize) -> f64 {
        let f = self.cover(i);
        if self.land[i] {
            self.albedo_land + (self.albedo_snow - self.albedo_land) * f
        } else {
            self.albedo_sea + (self.albedo_ice - self.albedo_sea) * f
        }
    }

    /// The outgoing infrared at the surface temperature `ts` and its
    /// derivative, with the humidity of the air at `ts`.
    fn olr(&self, ts: f64) -> (f64, f64) {
        let p = &self.params;
        let ps = self.atmosphere.surface_pressure_pa;
        let (qs, dqs) = saturation(ts, ps);
        let q = p.relative_humidity * qs;
        let k = self.pressure_factor * p.k_h2o;
        let tau = self.pressure_factor * self.dry_tau + k * (1.0 + q / p.q_ref).ln();
        let dtau = k * p.relative_humidity * dqs / (p.q_ref + q);
        let d = 1.0 + 0.75 * tau;
        let emission = SIGMA * ts.powi(4);
        let olr = emission / d;
        let derivative = 4.0 * SIGMA * ts.powi(3) / d - emission * 0.75 * dtau / (d * d);
        (olr, derivative)
    }

    /// Advances the run by one step.
    pub fn step(&mut self) {
        let n = self.grid.len();
        let t0 = self.step as f64 * self.dt;
        insolation(&self.sky, &self.grid, t0, self.dt, &mut self.sun);
        let p = self.params.clone();
        let ps = self.atmosphere.surface_pressure_pa;
        for i in 0..n {
            let t = self.temperature[i];
            let ts = t - self.lapse * self.height_m[i];
            let ice = self.ice[i];
            let albedo = self.albedo(i);
            let (olr, dolr) = self.olr(ts);
            let (qs, dqs) = saturation(t, ps);
            self.h[i] = p.cp * t + p.lv * p.relative_humidity * qs;
            self.dh[i] = p.cp + p.lv * p.relative_humidity * dqs;
            let surface = if self.land[i] {
                p.land_capacity
            } else if ice > 0.0 {
                0.0
            } else {
                p.ocean_capacity
            };
            let mut net = self.sun[i] * (1.0 - albedo) - olr;
            let mut diag = (self.air_capacity + surface) / self.dt + dolr;
            if !self.land[i] && ice > 0.0 {
                let k = p.ice_conductivity / ice.max(p.min_ice_m);
                net += k * (p.freezing_k - ts);
                diag += k;
            }
            self.diag[i] = diag;
            self.rhs[i] = net;
        }
        self.solver.stencil().apply(&self.h, &mut self.lap);
        for (r, l) in self.rhs.iter_mut().zip(&self.lap) {
            *r += self.diffusion * l;
        }
        self.iterations = self.solver.solve(
            &self.grid,
            &self.diag,
            self.diffusion,
            &self.dh,
            &self.rhs,
            &mut self.delta,
        );
        let latent = p.ice_density * p.fusion;
        let melt_humidity = saturation(p.melting_k, ps).0;
        for i in 0..n {
            let mut t = self.temperature[i] + self.delta[i];
            if !self.land[i] {
                let mut ice = self.ice[i];
                if ice > 0.0 {
                    // Growth and melt at the base, from the conduction
                    // through the ice.
                    let k = p.ice_conductivity / ice.max(p.min_ice_m);
                    ice += self.dt * k * (p.freezing_k - t) / latent;
                    if t > p.melting_k {
                        ice -= self.air_capacity * (t - p.melting_k) / latent;
                        t = p.melting_k;
                    }
                    if ice <= 0.0 {
                        let left = -ice * latent;
                        ice = 0.0;
                        t = p.freezing_k + left / (self.air_capacity + p.ocean_capacity);
                    }
                } else if t < p.freezing_k {
                    let capacity = self.air_capacity + p.ocean_capacity;
                    ice = capacity * (p.freezing_k - t) / latent;
                    t = p.freezing_k;
                }
                self.ice[i] = ice;
            } else {
                // Snow falls below the melting point. Above it, the heat
                // above the melting point melts the snow.
                let ts = t - self.lapse * self.height_m[i];
                let mut snow = self.snow[i];
                if ts < p.melting_k {
                    snow += self.dt * p.snowfall * saturation(ts, ps).0 / melt_humidity;
                } else if snow > 0.0 {
                    let capacity = self.air_capacity + p.land_capacity;
                    let melt = (capacity * (ts - p.melting_k) / p.fusion).min(snow);
                    snow -= melt;
                    t -= melt * p.fusion / capacity;
                }
                self.snow[i] = snow.min(p.max_snow_kg);
            }
            self.temperature[i] = t;
        }
        for i in 0..n {
            let ts = self.temperature[i] - self.lapse * self.height_m[i];
            self.out_t[i] = ts - 273.15;
            self.out_ice[i] = self.cover(i);
        }
        let month = (self.step % u64::from(STEPS_PER_YEAR)) as usize / STEPS_PER_MONTH as usize;
        self.means.add(month, &[&self.out_t, &self.out_ice]);
        self.step += 1;
        // The last step of a month closes it, so a run of whole years has
        // the mean of each month.
        if self.step.is_multiple_of(u64::from(STEPS_PER_MONTH)) {
            self.means.close();
        }
    }

    /// Advances the run by `steps` steps.
    pub fn run(&mut self, steps: u64) {
        for _ in 0..steps {
            self.step();
        }
    }

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    /// The length of a step, in seconds.
    pub fn dt_s(&self) -> f64 {
        self.dt
    }

    pub fn step_count(&self) -> u64 {
        self.step
    }

    /// The year of the next step, from 0.
    pub fn year(&self) -> u64 {
        self.step / u64::from(STEPS_PER_YEAR)
    }

    /// The month of the next step, from 0 to 11.
    pub fn month(&self) -> usize {
        (self.step % u64::from(STEPS_PER_YEAR)) as usize / STEPS_PER_MONTH as usize
    }

    /// The iterations of the last linear solve.
    pub fn iterations(&self) -> usize {
        self.iterations
    }

    pub fn means(&self) -> &Means {
        &self.means
    }

    /// The temperature at sea level of each cell, in K.
    pub fn sea_level_temperature_k(&self) -> &[f64] {
        &self.temperature
    }

    /// The thickness of the sea ice of each cell, in m.
    pub fn ice_m(&self) -> &[f64] {
        &self.ice
    }

    /// The snow of each cell, in kg/m².
    pub fn snow_kg(&self) -> &[f64] {
        &self.snow
    }

    /// The share of each cell under snow or sea ice.
    pub fn ice_cover(&self, out: &mut [f64]) {
        for (i, out) in out.iter_mut().enumerate() {
            *out = self.cover(i);
        }
    }

    /// The mean height of the land of each cell, in m. A sea cell has 0.
    pub fn height_m(&self) -> &[f64] {
        &self.height_m
    }

    pub fn land(&self) -> &[bool] {
        &self.land
    }

    /// The temperature at the height of each cell, in °C.
    pub fn surface_temperature_c(&self, out: &mut [f64]) {
        for (i, out) in out.iter_mut().enumerate() {
            *out = self.temperature[i] - self.lapse * self.height_m[i] - 273.15;
        }
    }

    /// The mean of the outgoing infrared over the planet, in W/m².
    pub fn mean_olr(&self) -> f64 {
        let sum: f64 = (0..self.grid.len())
            .map(|i| {
                let ts = self.temperature[i] - self.lapse * self.height_m[i];
                self.olr(ts).0 * self.grid.area(i)
            })
            .sum();
        sum / (4.0 * std::f64::consts::PI)
    }

    /// The mean of a field over the planet, weighted by area.
    pub fn global_mean(&self, values: &[f64]) -> f64 {
        let sum: f64 = (0..self.grid.len())
            .map(|i| values[i] * self.grid.area(i))
            .sum();
        sum / (4.0 * std::f64::consts::PI)
    }

    pub fn snapshot(&self) -> RunState {
        RunState {
            face_size: self.grid.n(),
            step: self.step,
            temperature_k: self.temperature.clone(),
            ice_m: self.ice.clone(),
            snow_kg: self.snow.clone(),
            means: self.means.clone(),
        }
    }

    /// A run that continues a saved state on `world`.
    pub fn restore(world: &World, state: RunState) -> Result<Model, String> {
        let n = state.face_size;
        if n < 2 {
            return Err(format!("A grid of {n} cells is too small."));
        }
        let mut model = Model::build(world, n, Params::earth());
        let cells = model.grid.len();
        let fits = state.temperature_k.len() == cells
            && state.ice_m.len() == cells
            && state.snow_kg.len() == cells
            && state.means.outputs() == Output::ALL
            && state.means.fields().iter().all(|f| f.face_size == n);
        if !fits {
            return Err("The saved run does not match its grid.".to_owned());
        }
        model.step = state.step;
        model.temperature = state.temperature_k;
        model.ice = state.ice_m;
        model.snow = state.snow_kg;
        model.means = state.means;
        Ok(model)
    }

    /// The bytes that the run allocates on the heap.
    pub fn heap_bytes(&self) -> u64 {
        let f64s = [
            &self.height_m,
            &self.temperature,
            &self.ice,
            &self.snow,
            &self.sun,
            &self.h,
            &self.dh,
            &self.diag,
            &self.rhs,
            &self.delta,
            &self.lap,
            &self.out_t,
            &self.out_ice,
        ]
        .iter()
        .map(|v| v.capacity())
        .sum::<usize>();
        let own = f64s * 8 + self.land.capacity();
        own as u64 + self.grid.heap_bytes() + self.solver.heap_bytes() + self.means.heap_bytes()
    }
}

/// The land of each cell and the mean height of its land, from the
/// terrain texels in the cell. A cell is land if half of its texels or more
/// are at or above the sea level.
fn sample_terrain(world: &World, grid: &Grid) -> (Vec<bool>, Vec<f64>) {
    let terrain = &world.terrain;
    let big = terrain.heights.face_size();
    let n = grid.n();
    let sea = terrain.level(terrain.sea_level_m);
    let mut land = Vec::with_capacity(grid.len());
    let mut height = Vec::with_capacity(grid.len());
    let span = |c: usize| {
        let lo = c * big / n;
        (lo, ((c + 1) * big / n).max(lo + 1))
    };
    let mut texels = Vec::new();
    for face in 0..FACES {
        for y in 0..n {
            for x in 0..n {
                let (x0, x1) = span(x);
                let (y0, y1) = span(y);
                texels.clear();
                terrain
                    .heights
                    .read_rect(face, TexelRect { x0, y0, x1, y1 }, &mut texels);
                let above: Vec<u16> = texels.iter().copied().filter(|&l| l >= sea).collect();
                let is_land = 2 * above.len() >= texels.len();
                let z = if is_land {
                    let sum: f64 = above
                        .iter()
                        .map(|&l| terrain.meters(l) - terrain.sea_level_m)
                        .sum();
                    (sum / above.len() as f64).max(0.0)
                } else {
                    0.0
                };
                land.push(is_land);
                height.push(z);
            }
        }
    }
    (land, height)
}
