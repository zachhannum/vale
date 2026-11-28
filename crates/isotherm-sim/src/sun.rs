//! The position of the star and the insolation of each cell.
//!
//! The time `t` counts seconds from the start of the first model year. The
//! year starts at a fixed fraction of the orbit before the northern spring
//! equinox, so that month 0 is January on the Earth. The planet turns at its
//! sidereal rate, and the star is over longitude 0 at periapsis.

use std::f64::consts::{PI, TAU};

use isotherm_world::Planet;

use crate::grid::Grid;

/// The fraction of the orbit from the start of the year to the northern
/// spring equinox: 79 days of 365.25.
pub const EQUINOX_FRACTION: f64 = 79.0 / 365.25;

/// The Newton iterations of the equation of Kepler. 8 iterations reach the
/// precision of f64 for an eccentricity up to 0.9.
const KEPLER_ITERATIONS: usize = 8;

/// The star as the planet sees it at one time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunState {
    /// The true longitude, from the northern spring equinox, in radians.
    pub true_longitude: f64,
    /// The declination, in radians.
    pub declination: f64,
    /// `(a / r)²`, with `a` the semi-major axis and `r` the distance.
    pub distance_factor: f64,
    /// The longitude on the planet where the star is overhead, in radians.
    pub subsolar_lon: f64,
}

/// The eccentric anomaly of a mean anomaly.
fn eccentric_anomaly(mean: f64, e: f64) -> f64 {
    let mut ecc = if e < 0.8 { mean } else { PI };
    for _ in 0..KEPLER_ITERATIONS {
        ecc -= (ecc - e * ecc.sin() - mean) / (1.0 - e * ecc.cos());
    }
    ecc
}

/// The true anomaly of an eccentric anomaly.
fn true_anomaly(ecc: f64, e: f64) -> f64 {
    2.0 * (((1.0 + e) / (1.0 - e)).sqrt() * (ecc / 2.0).tan()).atan()
}

/// The mean anomaly of a true anomaly.
fn mean_anomaly(nu: f64, e: f64) -> f64 {
    let ecc = 2.0 * (((1.0 - e) / (1.0 + e)).sqrt() * (nu / 2.0).tan()).atan();
    ecc - e * ecc.sin()
}

/// The orbit and the rotation of a planet, with the values that each step
/// needs.
#[derive(Clone, Copy, Debug)]
pub struct Sky {
    period_s: f64,
    sidereal_day_s: f64,
    e: f64,
    perihelion: f64,
    tilt: f64,
    /// The mean anomaly at `t = 0`.
    mean0: f64,
    /// The time of the first periapsis after `t = 0`, in seconds.
    t_peri: f64,
    /// The right ascension of the star at `t_peri`.
    ra_peri: f64,
    /// The flux of the star at the semi-major axis, in W/m².
    pub s_star: f64,
}

impl Sky {
    pub fn new(planet: &Planet) -> Sky {
        let period_s = planet.orbital_period_s();
        let e = planet.orbit.eccentricity;
        let perihelion = planet.orbit.perihelion_deg.to_radians();
        let a = planet.orbit.semi_major_m;
        // The true longitude is 0 at the equinox.
        let mean_eq = mean_anomaly(-perihelion, e);
        let mean0 = mean_eq - TAU * EQUINOX_FRACTION;
        let t_peri = (-mean0).rem_euclid(TAU) / TAU * period_s;
        let mut sky = Sky {
            period_s,
            sidereal_day_s: planet.rotation.sidereal_day_s,
            e,
            perihelion,
            tilt: planet.rotation.tilt_deg.to_radians(),
            mean0,
            t_peri,
            ra_peri: 0.0,
            s_star: planet.star.luminosity_w / (4.0 * PI * a * a),
        };
        sky.ra_peri = sky.right_ascension(sky.true_longitude(t_peri));
        sky
    }

    pub fn period_s(&self) -> f64 {
        self.period_s
    }

    /// The true longitude and the distance factor at `t`.
    fn orbit_at(&self, t: f64) -> (f64, f64) {
        let mean = (self.mean0 + TAU * t / self.period_s).rem_euclid(TAU);
        let ecc = eccentric_anomaly(mean, self.e);
        let nu = true_anomaly(ecc, self.e);
        let r = 1.0 - self.e * ecc.cos();
        (nu + self.perihelion, 1.0 / (r * r))
    }

    fn true_longitude(&self, t: f64) -> f64 {
        self.orbit_at(t).0
    }

    fn right_ascension(&self, lambda: f64) -> f64 {
        (lambda.sin() * self.tilt.cos()).atan2(lambda.cos())
    }

    /// The angle of the planet about its axis at `t`.
    fn rotation(&self, t: f64) -> f64 {
        self.ra_peri + TAU * (t - self.t_peri) / self.sidereal_day_s
    }

    pub fn sun_at(&self, t: f64) -> SunState {
        let (lambda, distance_factor) = self.orbit_at(t);
        let declination = (self.tilt.sin() * lambda.sin()).asin();
        let ra = self.right_ascension(lambda);
        SunState {
            true_longitude: lambda.rem_euclid(TAU),
            declination,
            distance_factor,
            subsolar_lon: wrap_pi(ra - self.rotation(t)),
        }
    }

    /// The star over the interval from `t0` to `t0 + dt`.
    pub fn interval(&self, t0: f64, dt: f64) -> Interval {
        let mid = self.sun_at(t0 + dt / 2.0);
        let ra = |t: f64| self.right_ascension(self.true_longitude(t));
        let h0 = self.rotation(t0) - ra(t0);
        let dh = TAU * dt / self.sidereal_day_s - wrap_pi(ra(t0 + dt) - ra(t0));
        Interval {
            sin_dec: mid.declination.sin(),
            cos_dec: mid.declination.cos(),
            flux: self.s_star * mid.distance_factor,
            h0,
            dh,
        }
    }
}

/// The geometry of the star over one step.
#[derive(Clone, Copy, Debug)]
pub struct Interval {
    sin_dec: f64,
    cos_dec: f64,
    /// The flux of the star at the planet, in W/m².
    pub flux: f64,
    /// The hour angle at longitude 0 at the start, in radians.
    h0: f64,
    /// The change of the hour angle over the step, in radians.
    dh: f64,
}

impl Interval {
    /// The mean insolation over the step at a point, in W/m².
    pub fn insolation(&self, lat: f64, lon: f64) -> f64 {
        let a = lat.sin() * self.sin_dec;
        let b = lat.cos() * self.cos_dec;
        let h0 = self.h0 + lon;
        let mean = if self.dh.abs() < 1e-9 {
            (a + b * (h0 + self.dh / 2.0).cos()).max(0.0)
        } else {
            clipped_cos_integral(a, b, h0, h0 + self.dh) / self.dh
        };
        self.flux * mean
    }
}

/// `∫ max(0, a + b cos h) dh` from `h0` to `h1`, with `b ≥ 0`.
pub fn clipped_cos_integral(a: f64, b: f64, h0: f64, h1: f64) -> f64 {
    if b < 1e-12 {
        return a.max(0.0) * (h1 - h0);
    }
    // The hour angle of sunset, from 0 in the polar night to π in the
    // polar day.
    let hs = (-a / b).clamp(-1.0, 1.0).acos();
    let turn = 2.0 * (a * hs + b * hs.sin());
    let antiderivative = |h: f64| {
        let turns = ((h + PI) / TAU).floor();
        let c = (h - turns * TAU).clamp(-hs, hs);
        turns * turn + a * c + b * c.sin()
    };
    antiderivative(h1) - antiderivative(h0)
}

/// An angle in the range from −π to π.
fn wrap_pi(x: f64) -> f64 {
    (x + PI).rem_euclid(TAU) - PI
}

/// The position of the star at `t`.
pub fn sun_at(planet: &Planet, t: f64) -> SunState {
    Sky::new(planet).sun_at(t)
}

/// The mean insolation of each cell over the interval from `t0` to
/// `t0 + dt`, in W/m².
pub fn insolation(sky: &Sky, grid: &Grid, t0: f64, dt: f64, out: &mut [f64]) {
    let interval = sky.interval(t0, dt);
    for (i, out) in out.iter_mut().enumerate() {
        let (lat, lon) = grid.lat_lon(i);
        *out = interval.insolation(lat, lon);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clipped_integral_of_a_full_turn_is_the_daily_sum() {
        for (a, b) in [(0.3, 0.5), (-0.3, 0.5), (0.8, 0.5), (-0.8, 0.5), (0.2, 0.0)] {
            let full = clipped_cos_integral(a, b, 0.4, 0.4 + TAU);
            let mut sum = 0.0;
            let k = 100_000;
            for i in 0..k {
                let h = 0.4 + (i as f64 + 0.5) / k as f64 * TAU;
                sum += (a + b * h.cos()).max(0.0) * TAU / k as f64;
            }
            assert!((full - sum).abs() < 1e-6, "{a} {b}: {full} {sum}");
            // Many turns and a part of a turn.
            let many = clipped_cos_integral(a, b, -7.0, -7.0 + 5.0 * TAU + 1.0);
            let part = clipped_cos_integral(a, b, -7.0 + 5.0 * TAU, -7.0 + 5.0 * TAU + 1.0);
            assert!((many - 5.0 * full - part).abs() < 1e-9);
        }
    }

    #[test]
    fn kepler_round_trips() {
        for e in [0.0, 0.0167, 0.2056, 0.6] {
            for m in [0.1, 1.0, 2.5, 3.1] {
                let nu = true_anomaly(eccentric_anomaly(m, e), e);
                assert!((mean_anomaly(nu, e) - m).abs() < 1e-12, "{e} {m}");
            }
        }
    }
}
