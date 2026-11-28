//! The planet record and the terrain of an Isotherm world.

pub use vale_terrain::Heightmap;
use vale_terrain::{Bands, FACES, SEA_LEVEL, TexelRect};

/// The gravitational constant, in m³/(kg s²).
pub const G: f64 = 6.674_30e-11;

/// The bulk properties of a planet.
#[derive(Clone, Debug, PartialEq)]
pub struct Planet {
    /// The mean radius, in meters.
    pub radius_m: f64,
    /// The surface gravity, in m/s².
    pub gravity: f64,
    pub star: Star,
    pub orbit: Orbit,
    pub rotation: Rotation,
    pub atmosphere: Atmosphere,
    pub moons: Vec<Moon>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Star {
    /// The luminosity, in watts.
    pub luminosity_w: f64,
    /// The temperature of the photosphere, in kelvins.
    pub temperature_k: f64,
    /// The mass, in kilograms.
    pub mass_kg: f64,
}

/// The wavelength between the visible band and the infrared band, in meters.
pub const VISIBLE_EDGE_M: f64 = 0.7e-6;

/// The second radiation constant `h c / k`, in m K.
const C2: f64 = 1.438_776_877e-2;

impl Star {
    /// The share of the light of the star in the visible band, below
    /// [`VISIBLE_EDGE_M`]. The star is a black body at its temperature.
    pub fn visible_fraction(&self) -> f64 {
        black_body_fraction(VISIBLE_EDGE_M, self.temperature_k)
    }
}

/// The share of the emission of a black body at `temperature_k` below the
/// wavelength `lambda_m`, from the series of the Planck integral of Widger
/// and Woodall (1976).
pub fn black_body_fraction(lambda_m: f64, temperature_k: f64) -> f64 {
    let x = C2 / (lambda_m * temperature_k.max(1.0));
    let mut sum = 0.0;
    for n in 1..=200 {
        let n = f64::from(n);
        let term = (-n * x).exp() / n
            * (x.powi(3) + 3.0 * x * x / n + 6.0 * x / (n * n) + 6.0 / (n * n * n));
        sum += term;
        if term < 1e-12 * sum {
            break;
        }
    }
    (15.0 / std::f64::consts::PI.powi(4) * sum).clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    /// The semi-major axis, in meters.
    pub semi_major_m: f64,
    pub eccentricity: f64,
    /// The longitude of the perihelion from the vernal equinox, in degrees.
    pub perihelion_deg: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation {
    /// The period of one turn about the axis against the stars, in seconds.
    pub sidereal_day_s: f64,
    /// The angle between the axis of rotation and the normal of the orbit, in degrees.
    pub tilt_deg: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    /// The mean pressure at sea level, in pascals.
    pub surface_pressure_pa: f64,
    /// The fraction of carbon dioxide, in parts per million by volume.
    pub co2_ppm: f64,
    /// The fraction of methane, in parts per million by volume.
    pub ch4_ppm: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moon {
    /// The mass, in kilograms.
    pub mass_kg: f64,
    /// The semi-major axis of the orbit around the planet, in meters.
    pub distance_m: f64,
}

impl Planet {
    /// The Earth, with the Sun and the Moon.
    pub fn earth() -> Planet {
        Planet {
            radius_m: 6_371_000.0,
            gravity: 9.81,
            star: Star {
                luminosity_w: 3.828e26,
                temperature_k: 5772.0,
                mass_kg: 1.988_4e30,
            },
            orbit: Orbit {
                semi_major_m: 1.495_978_707e11,
                eccentricity: 0.0167,
                perihelion_deg: 282.9,
            },
            rotation: Rotation {
                sidereal_day_s: 86_164.1,
                tilt_deg: 23.44,
            },
            atmosphere: Atmosphere {
                surface_pressure_pa: 101_325.0,
                co2_ppm: 280.0,
                ch4_ppm: 0.722,
            },
            moons: vec![Moon {
                mass_kg: 7.342e22,
                distance_m: 3.844e8,
            }],
        }
    }

    /// The mass of the planet, in kilograms, from its gravity and radius.
    pub fn mass_kg(&self) -> f64 {
        self.gravity * self.radius_m * self.radius_m / G
    }

    /// The period of the orbit, in seconds, from the third law of Kepler.
    pub fn orbital_period_s(&self) -> f64 {
        let a = self.orbit.semi_major_m;
        2.0 * std::f64::consts::PI * (a * a * a / (G * (self.star.mass_kg + self.mass_kg()))).sqrt()
    }

    /// The length of the solar day, in seconds. A tidally locked planet has
    /// an infinite solar day. A planet that turns against its orbit has a
    /// negative sidereal day.
    pub fn solar_day_s(&self) -> f64 {
        let rate = 1.0 / self.rotation.sidereal_day_s - 1.0 / self.orbital_period_s();
        if rate.abs() < 1e-12 / self.rotation.sidereal_day_s.abs() {
            f64::INFINITY
        } else {
            (1.0 / rate).abs()
        }
    }
}

impl Default for Planet {
    fn default() -> Planet {
        Planet::earth()
    }
}

/// The heights of black and of white in an imported heightmap with no
/// units, in meters.
pub const DEFAULT_LOW_M: f64 = -8000.0;
pub const DEFAULT_HIGH_M: f64 = 8000.0;

/// A heightmap on a cube map and its sea level. Level 0 of the heightmap is
/// `low_m`, and level 65,535 is `high_m`.
pub struct Terrain {
    pub heights: Heightmap,
    pub low_m: f64,
    pub high_m: f64,
    /// The height of the sea surface, from `low_m` to `high_m`.
    pub sea_level_m: f64,
}

impl Terrain {
    /// A terrain with the whole surface at `level`.
    pub fn flat(face_size: usize, low_m: f64, high_m: f64, level: u16) -> Terrain {
        Terrain::new(Heightmap::new(face_size, level), low_m, high_m)
    }

    /// A terrain from a heightmap, with the sea at 0 m, or at the middle of
    /// a range that does not include 0 m.
    pub fn new(heights: Heightmap, low_m: f64, high_m: f64) -> Terrain {
        let sea_level_m = if low_m < 0.0 && high_m > 0.0 {
            0.0
        } else {
            (low_m + high_m) / 2.0
        };
        Terrain {
            heights,
            low_m,
            high_m,
            sea_level_m,
        }
    }

    /// The height of a level, in meters.
    pub fn meters(&self, level: u16) -> f64 {
        self.low_m + f64::from(level) / 65535.0 * (self.high_m - self.low_m)
    }

    /// The level of a height in meters. A height outside the range gets the
    /// nearest end.
    pub fn level(&self, meters: f64) -> u16 {
        let t = (meters - self.low_m) / (self.high_m - self.low_m);
        (t.clamp(0.0, 1.0) * 65535.0).round() as u16
    }

    /// A sea level in the range of the heights.
    pub fn clamp_sea(&self, meters: f64) -> f64 {
        meters.clamp(self.low_m, self.high_m)
    }

    /// The colors of the land and the sea. The bands measure heights from
    /// the sea level, so the split of land and sea is at the coast. The
    /// width of the range does not change with the sea level, so the levels
    /// of the heightmap stay valid.
    pub fn bands(&self) -> Bands {
        let low = self.low_m - self.sea_level_m;
        let high = self.high_m - self.sea_level_m;
        Bands::with_range(low, high, true).unwrap_or_default()
    }

    /// The fraction of the texels below the sea level.
    pub fn sea_fraction(&self) -> f64 {
        let n = self.heights.face_size();
        let rect = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        let sea = self.level(self.sea_level_m);
        let mut below = 0;
        let mut data = Vec::with_capacity(n * n);
        for face in 0..FACES {
            data.clear();
            self.heights.read_rect(face, rect, &mut data);
            below += data.iter().filter(|&&level| level < sea).count();
        }
        below as f64 / (FACES * n * n) as f64
    }
}

/// The height of the sea split of [`Terrain::bands`].
pub const COAST: f64 = SEA_LEVEL;

/// A planet and its terrain.
pub struct World {
    pub name: String,
    pub planet: Planet,
    pub terrain: Terrain,
}

impl World {
    /// A world with the whole surface at sea level.
    pub fn flat(name: &str, planet: Planet, face_size: usize) -> World {
        let mut terrain = Terrain::flat(face_size, DEFAULT_LOW_M, DEFAULT_HIGH_M, 0);
        terrain.heights = Heightmap::new(face_size, terrain.level(0.0));
        World {
            name: name.to_owned(),
            planet,
            terrain,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_planet_is_the_earth() {
        let planet = Planet::default();
        assert_eq!(planet.radius_m, 6_371_000.0);
        assert!((planet.solar_day_s() - 86_400.0).abs() < 1.0);
        assert_eq!(planet.moons.len(), 1);
        assert!((planet.atmosphere.surface_pressure_pa - 101_325.0).abs() < 1.0);
        // The solar constant at 1 AU is about 1,361 W/m².
        let a = planet.orbit.semi_major_m;
        let flux = planet.star.luminosity_w / (4.0 * std::f64::consts::PI * a * a);
        assert!((flux - 1361.0).abs() < 5.0, "{flux}");
    }

    #[test]
    fn a_cooler_star_puts_less_of_its_light_in_the_visible_band() {
        let mut star = Planet::earth().star;
        // About 49 % of the light of a black body at 5,772 K is below 0.7 µm.
        let sun = star.visible_fraction();
        assert!((sun - 0.49).abs() < 0.01, "{sun}");
        star.temperature_k = 3200.0;
        let red = star.visible_fraction();
        assert!(red < 0.15, "{red}");
        // The series and a sum of the Planck function agree.
        let planck = |l: f64| 1.0 / (l.powi(5) * ((C2 / (l * 5772.0)).exp() - 1.0));
        let dl = 1e-9;
        let below: f64 = (1..700).map(|i| planck(f64::from(i) * dl)).sum();
        let all: f64 = (1..200_000).map(|i| planck(f64::from(i) * dl)).sum();
        assert!((below / all - sun).abs() < 1e-3);
    }

    #[test]
    fn the_earth_orbits_in_one_year() {
        let days = Planet::earth().orbital_period_s() / 86_400.0;
        assert!((days - 365.25).abs() < 0.01, "{days}");
    }

    #[test]
    fn a_tidally_locked_planet_has_no_solar_day() {
        let mut planet = Planet::earth();
        planet.rotation.sidereal_day_s = planet.orbital_period_s();
        assert_eq!(planet.solar_day_s(), f64::INFINITY);
    }

    #[test]
    fn a_three_to_two_resonance_has_a_solar_day_of_two_orbits() {
        let mut planet = Planet::earth();
        let year = planet.orbital_period_s();
        planet.rotation.sidereal_day_s = year * 2.0 / 3.0;
        assert!((planet.solar_day_s() / year - 2.0).abs() < 1e-9);
    }

    #[test]
    fn levels_and_meters_round_trip_over_the_full_range() {
        let terrain = Terrain::flat(4, -8000.0, 8000.0, 0);
        assert_eq!(terrain.level(-8000.0), 0);
        assert_eq!(terrain.level(8000.0), 65535);
        assert_eq!(terrain.level(9000.0), 65535);
        for m in [-7999.0, -1234.5, 0.0, 4321.0, 7999.0] {
            assert!((terrain.meters(terrain.level(m)) - m).abs() < 0.2, "{m}");
        }
    }

    #[test]
    fn the_coast_of_the_bands_follows_the_sea_level() {
        let mut terrain = Terrain::flat(4, -8000.0, 8000.0, 0);
        let width = |b: &Bands| b.value(65535) - b.value(0);
        let before = terrain.bands();
        assert_eq!(before.level(COAST), terrain.level(0.0));
        terrain.sea_level_m = 2000.0;
        let after = terrain.bands();
        assert_eq!(after.level(COAST), terrain.level(2000.0));
        assert!((width(&before) - width(&after)).abs() < 1e-6);
    }

    #[test]
    fn a_higher_sea_covers_more_of_the_world() {
        // Levels from 0 at the south pole to 65,535 at the north pole.
        let (w, h) = (64, 32);
        let levels = (0..w * h)
            .map(|i| ((h - 1 - i / w) * 65535 / (h - 1)) as u16)
            .collect();
        let src = vale_terrain::Equirect::new(w, h, levels).unwrap();
        let mut heights = Heightmap::new(8, 0);
        heights.import_equirect(&src);
        let mut terrain = Terrain::new(heights, -8000.0, 8000.0);
        let low = terrain.sea_fraction();
        terrain.sea_level_m = 7000.0;
        assert!(terrain.sea_fraction() > low);
    }

    #[test]
    fn a_range_above_zero_puts_the_sea_in_the_middle() {
        let terrain = Terrain::flat(4, 100.0, 900.0, 0);
        assert_eq!(terrain.sea_level_m, 500.0);
    }
}
