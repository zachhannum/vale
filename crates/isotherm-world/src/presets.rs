//! Presets for the star, the orbit, the rotation, and the atmosphere of a
//! planet. A preset sets only the fields of its group.

use crate::Planet;

/// The luminosity of the Sun, in watts.
pub const SUN_LUMINOSITY_W: f64 = 3.828e26;
/// The mass of the Sun, in kilograms.
pub const SUN_MASS_KG: f64 = 1.988_4e30;
/// The astronomical unit, in meters.
pub const AU_M: f64 = 1.495_978_707e11;
/// The sidereal day of the Earth, in seconds.
pub const EARTH_SIDEREAL_DAY_S: f64 = 86_164.1;

/// True when two values differ by less than one part in a million.
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StarPreset {
    Yellow,
    Orange,
    Red,
    White,
}

impl StarPreset {
    pub const ALL: [StarPreset; 4] = [Self::Yellow, Self::Orange, Self::Red, Self::White];

    pub fn name(self) -> &'static str {
        match self {
            Self::Yellow => "Yellow",
            Self::Orange => "Orange",
            Self::Red => "Red",
            Self::White => "White",
        }
    }

    /// The temperature in kelvins, the luminosity and the mass in units of
    /// the Sun.
    fn values(self) -> (f64, f64, f64) {
        match self {
            Self::Yellow => (5772.0, 1.0, 1.0),
            Self::Orange => (4400.0, 0.2, 0.7),
            Self::Red => (3200.0, 0.01, 0.3),
            Self::White => (6600.0, 2.5, 1.3),
        }
    }

    pub fn apply(self, planet: &mut Planet) {
        let (t, l, m) = self.values();
        planet.star.temperature_k = t;
        planet.star.luminosity_w = l * SUN_LUMINOSITY_W;
        planet.star.mass_kg = m * SUN_MASS_KG;
    }

    pub fn matches(self, planet: &Planet) -> bool {
        let (t, l, m) = self.values();
        near(planet.star.temperature_k, t)
            && near(planet.star.luminosity_w, l * SUN_LUMINOSITY_W)
            && near(planet.star.mass_kg, m * SUN_MASS_KG)
    }

    /// The first preset that matches the planet.
    pub fn of(planet: &Planet) -> Option<StarPreset> {
        Self::ALL.into_iter().find(|p| p.matches(planet))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbitPreset {
    /// The orbit of the Earth.
    Earth,
    /// The distance that gives the insolation of the Earth for the star of
    /// the planet.
    Temperate,
    /// An eccentric orbit at the temperate distance.
    Eccentric,
}

impl OrbitPreset {
    pub const ALL: [OrbitPreset; 3] = [Self::Earth, Self::Temperate, Self::Eccentric];

    pub fn name(self) -> &'static str {
        match self {
            Self::Earth => "Earth",
            Self::Temperate => "Temperate",
            Self::Eccentric => "Eccentric",
        }
    }

    /// The semi-major axis in meters, the eccentricity, and the longitude
    /// of the perihelion in degrees.
    fn values(self, planet: &Planet) -> (f64, f64, f64) {
        let temperate = (planet.star.luminosity_w / SUN_LUMINOSITY_W).sqrt() * AU_M;
        match self {
            Self::Earth => (AU_M, 0.0167, 282.9),
            Self::Temperate => (temperate, 0.0167, 282.9),
            Self::Eccentric => (temperate, 0.3, 282.9),
        }
    }

    pub fn apply(self, planet: &mut Planet) {
        let (a, e, p) = self.values(planet);
        planet.orbit.semi_major_m = a;
        planet.orbit.eccentricity = e;
        planet.orbit.perihelion_deg = p;
    }

    pub fn matches(self, planet: &Planet) -> bool {
        let (a, e, p) = self.values(planet);
        near(planet.orbit.semi_major_m, a)
            && near(planet.orbit.eccentricity, e)
            && near(planet.orbit.perihelion_deg, p)
    }

    pub fn of(planet: &Planet) -> Option<OrbitPreset> {
        Self::ALL.into_iter().find(|p| p.matches(planet))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotationPreset {
    /// The day and the tilt of the Earth.
    Earth,
    /// One side faces the star, with no tilt.
    Locked,
    /// A sidereal day of 10 hours and the tilt of the Earth.
    Fast,
    /// The day of the Earth and an axis in the plane of the orbit.
    Sideways,
}

impl RotationPreset {
    pub const ALL: [RotationPreset; 4] = [Self::Earth, Self::Locked, Self::Fast, Self::Sideways];

    pub fn name(self) -> &'static str {
        match self {
            Self::Earth => "Earth",
            Self::Locked => "Locked",
            Self::Fast => "Fast",
            Self::Sideways => "Sideways",
        }
    }

    /// The sidereal day in seconds and the tilt in degrees.
    fn values(self, planet: &Planet) -> (f64, f64) {
        match self {
            Self::Earth => (EARTH_SIDEREAL_DAY_S, 23.44),
            Self::Locked => (planet.orbital_period_s(), 0.0),
            Self::Fast => (36_000.0, 23.44),
            Self::Sideways => (EARTH_SIDEREAL_DAY_S, 90.0),
        }
    }

    pub fn apply(self, planet: &mut Planet) {
        let (day, tilt) = self.values(planet);
        planet.rotation.sidereal_day_s = day;
        planet.rotation.tilt_deg = tilt;
    }

    pub fn matches(self, planet: &Planet) -> bool {
        let (day, tilt) = self.values(planet);
        let day_matches = match self {
            Self::Locked => planet.is_locked(),
            _ => near(planet.rotation.sidereal_day_s, day),
        };
        day_matches && (planet.rotation.tilt_deg - tilt).abs() < 1e-6
    }

    pub fn of(planet: &Planet) -> Option<RotationPreset> {
        Self::ALL.into_iter().find(|p| p.matches(planet))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtmospherePreset {
    Earth,
    Thin,
    Thick,
}

impl AtmospherePreset {
    pub const ALL: [AtmospherePreset; 3] = [Self::Earth, Self::Thin, Self::Thick];

    pub fn name(self) -> &'static str {
        match self {
            Self::Earth => "Earth",
            Self::Thin => "Thin",
            Self::Thick => "Thick",
        }
    }

    /// The surface pressure in pascals, and CO2 and CH4 in ppm.
    fn values(self) -> (f64, f64, f64) {
        match self {
            Self::Earth => (101_325.0, 280.0, 0.722),
            Self::Thin => (30_000.0, 280.0, 0.722),
            Self::Thick => (300_000.0, 5_000.0, 1.8),
        }
    }

    pub fn apply(self, planet: &mut Planet) {
        let (p, co2, ch4) = self.values();
        planet.atmosphere.surface_pressure_pa = p;
        planet.atmosphere.co2_ppm = co2;
        planet.atmosphere.ch4_ppm = ch4;
    }

    pub fn matches(self, planet: &Planet) -> bool {
        let (p, co2, ch4) = self.values();
        near(planet.atmosphere.surface_pressure_pa, p)
            && near(planet.atmosphere.co2_ppm, co2)
            && near(planet.atmosphere.ch4_ppm, ch4)
    }

    pub fn of(planet: &Planet) -> Option<AtmospherePreset> {
        Self::ALL.into_iter().find(|p| p.matches(planet))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A planet that no preset matches.
    fn odd() -> Planet {
        let mut planet = Planet::earth();
        planet.star.temperature_k = 5000.0;
        planet.star.luminosity_w = 0.5 * SUN_LUMINOSITY_W;
        planet.star.mass_kg = 0.8 * SUN_MASS_KG;
        planet.orbit.semi_major_m = 2.0 * AU_M;
        planet.orbit.eccentricity = 0.1;
        planet.orbit.perihelion_deg = 10.0;
        planet.rotation.sidereal_day_s = 50_000.0;
        planet.rotation.tilt_deg = 40.0;
        planet.atmosphere.surface_pressure_pa = 200_000.0;
        planet.atmosphere.co2_ppm = 1000.0;
        planet.atmosphere.ch4_ppm = 3.0;
        planet
    }

    fn insolation(planet: &Planet) -> f64 {
        let a = planet.orbit.semi_major_m;
        planet.star.luminosity_w / (4.0 * std::f64::consts::PI * a * a)
    }

    #[test]
    fn each_preset_sets_only_its_group() {
        for p in StarPreset::ALL {
            let mut planet = odd();
            p.apply(&mut planet);
            assert!(p.matches(&planet));
            assert_eq!(StarPreset::of(&planet), Some(p));
            let before = odd();
            assert_eq!(planet.orbit, before.orbit);
            assert_eq!(planet.rotation, before.rotation);
            assert_eq!(planet.atmosphere, before.atmosphere);
        }
        for p in OrbitPreset::ALL {
            let mut planet = odd();
            p.apply(&mut planet);
            assert!(p.matches(&planet));
            let before = odd();
            assert_eq!(planet.star, before.star);
            assert_eq!(planet.rotation, before.rotation);
            assert_eq!(planet.atmosphere, before.atmosphere);
        }
        for p in RotationPreset::ALL {
            let mut planet = odd();
            p.apply(&mut planet);
            assert!(p.matches(&planet));
            assert_eq!(RotationPreset::of(&planet), Some(p));
            let before = odd();
            assert_eq!(planet.star, before.star);
            assert_eq!(planet.orbit, before.orbit);
            assert_eq!(planet.atmosphere, before.atmosphere);
        }
        for p in AtmospherePreset::ALL {
            let mut planet = odd();
            p.apply(&mut planet);
            assert!(p.matches(&planet));
            assert_eq!(AtmospherePreset::of(&planet), Some(p));
            let before = odd();
            assert_eq!(planet.star, before.star);
            assert_eq!(planet.orbit, before.orbit);
            assert_eq!(planet.rotation, before.rotation);
        }
    }

    #[test]
    fn no_preset_matches_other_values() {
        let planet = odd();
        assert_eq!(StarPreset::of(&planet), None);
        assert_eq!(OrbitPreset::of(&planet), None);
        assert_eq!(RotationPreset::of(&planet), None);
        assert_eq!(AtmospherePreset::of(&planet), None);
    }

    #[test]
    fn the_earth_matches_the_earth_presets() {
        let planet = Planet::earth();
        assert_eq!(StarPreset::of(&planet), Some(StarPreset::Yellow));
        assert_eq!(OrbitPreset::of(&planet), Some(OrbitPreset::Earth));
        assert_eq!(RotationPreset::of(&planet), Some(RotationPreset::Earth));
        assert_eq!(AtmospherePreset::of(&planet), Some(AtmospherePreset::Earth));
    }

    #[test]
    fn the_temperate_orbit_gives_the_insolation_of_the_earth() {
        let earth = insolation(&Planet::earth());
        for star in StarPreset::ALL {
            let mut planet = Planet::earth();
            star.apply(&mut planet);
            OrbitPreset::Temperate.apply(&mut planet);
            let flux = insolation(&planet);
            assert!((flux / earth - 1.0).abs() < 0.01, "{star:?}: {flux}");
        }
    }

    #[test]
    fn the_locked_preset_has_no_solar_day() {
        for star in StarPreset::ALL {
            let mut planet = Planet::earth();
            star.apply(&mut planet);
            OrbitPreset::Temperate.apply(&mut planet);
            RotationPreset::Locked.apply(&mut planet);
            assert_eq!(planet.solar_day_s(), f64::INFINITY);
            assert!(planet.is_locked());
        }
    }
}
