//! The climate model of Isotherm: a CPU reference version of each step.

use std::f64::consts::PI;

pub mod grid;
pub mod ops;

pub use grid::{Edge, Grid, Vector};
pub use ops::Ops;

/// The mean flux of the star over one day at the top of the atmosphere, in
/// W/m², with `s0` the flux at the distance of the planet. The latitude and
/// the declination are in radians.
pub fn daily_mean_insolation(s0: f64, lat_rad: f64, declination_rad: f64) -> f64 {
    // The hour angle of sunset. 0 is polar night, and π is polar day.
    let h0 = (-lat_rad.tan() * declination_rad.tan())
        .clamp(-1.0, 1.0)
        .acos();
    s0 / PI
        * (h0 * lat_rad.sin() * declination_rad.sin()
            + lat_rad.cos() * declination_rad.cos() * h0.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    const S0: f64 = 1361.0;

    #[test]
    fn equator_at_equinox_gets_s0_over_pi() {
        let q = daily_mean_insolation(S0, 0.0, 0.0);
        assert!((q - S0 / PI).abs() < 1e-9);
    }

    #[test]
    fn pole_in_polar_night_gets_nothing() {
        let tilt = 23.44_f64.to_radians();
        assert!(daily_mean_insolation(S0, PI / 2.0, -tilt).abs() < 1e-9);
    }

    #[test]
    fn pole_in_polar_day_gets_s0_sin_declination() {
        let tilt = 23.44_f64.to_radians();
        let q = daily_mean_insolation(S0, PI / 2.0, tilt);
        assert!((q - S0 * tilt.sin()).abs() < 1e-6);
    }
}
