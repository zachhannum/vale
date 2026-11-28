//! The planet record and the terrain of an Isotherm world.

pub use vale_terrain::Heightmap;
use vale_terrain::{Bands, FACES, SEA_LEVEL, TexelRect};

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
    /// The length of the solar day, in seconds.
    pub day_s: f64,
    /// The angle between the axis of rotation and the normal of the orbit, in degrees.
    pub tilt_deg: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    /// The mean pressure at sea level, in pascals.
    pub surface_pressure_pa: f64,
    /// The fraction of carbon dioxide, in parts per million by volume.
    pub co2_ppm: f64,
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
            },
            orbit: Orbit {
                semi_major_m: 1.495_978_707e11,
                eccentricity: 0.0167,
                perihelion_deg: 282.9,
            },
            rotation: Rotation {
                day_s: 86_400.0,
                tilt_deg: 23.44,
            },
            atmosphere: Atmosphere {
                surface_pressure_pa: 101_325.0,
                co2_ppm: 280.0,
            },
            moons: vec![Moon {
                mass_kg: 7.342e22,
                distance_m: 3.844e8,
            }],
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
        assert_eq!(planet.rotation.day_s, 86_400.0);
        assert_eq!(planet.moons.len(), 1);
        assert!((planet.atmosphere.surface_pressure_pa - 101_325.0).abs() < 1.0);
        // The solar constant at 1 AU is about 1,361 W/m².
        let a = planet.orbit.semi_major_m;
        let flux = planet.star.luminosity_w / (4.0 * std::f64::consts::PI * a * a);
        assert!((flux - 1361.0).abs() < 5.0, "{flux}");
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
