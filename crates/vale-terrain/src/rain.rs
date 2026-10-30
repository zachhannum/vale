//! The rain of the erode brush.
//!
//! A wind brings the rain from one side. The air comes in wet over the sea.
//! Where the ground rises along the wind, the air drops a part of its water,
//! so the lee side of a range gets little rain. Higher ground gets more of
//! the water that the air holds.

use crate::bands::SEA_LEVEL;
use crate::cube::level_to_meters;
use crate::math::{V3, cross, dot, normalize};

/// The number of steps of the air that a cell looks back along the wind.
pub(crate) const TRACE_STEPS: usize = 48;

/// The length of a step of the air, in cells.
pub(crate) const TRACE_SPACING: f64 = 2.0;

/// The rise of the ground, in meters, that takes all but `1 / e` of the
/// water of the air.
const LIFT: f64 = 1500.0;

/// The part of its missing water that the air takes back in one step over
/// the sea.
const RECHARGE: f64 = 0.25;

/// The elevation, in meters, where the air drops the most of its water.
const HIGH: f64 = 3000.0;

/// The part of the water of the air that falls on ground at sea level.
const LOW_RAIN: f64 = 0.15;

/// The largest rain of the model before the scale to 1: the rain on the
/// windward slope of a range that rises from the sea, at the best height.
fn peak() -> f64 {
    let h = (LIFT - LOW_RAIN * HIGH / (1.0 - LOW_RAIN)).clamp(0.0, HIGH);
    (-h / LIFT).exp() * (LOW_RAIN + (1.0 - LOW_RAIN) * h / HIGH)
}

/// The rain of an erosion. The rain of a cell is a value from 0 to 1, and
/// the water of the cell in the flow is its area times its rain.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rain {
    /// The compass direction that the wind comes from, in degrees. 0 is
    /// north, and 270 is west.
    pub wind: f64,
    /// The part of the rain that follows the wind and the relief, from 0 to
    /// 1. At 0, each cell gets the same rain.
    pub shadow: f64,
}

impl Rain {
    /// Whether each cell gets the same rain.
    pub fn is_even(&self) -> bool {
        self.shadow.is_nan() || self.shadow <= 0.0
    }

    /// The direction on the sphere at `d` that the wind comes from, a unit
    /// tangent vector.
    pub(crate) fn upwind(&self, d: V3) -> V3 {
        let east = cross([0.0, 0.0, 1.0], d);
        let east = if dot(east, east) < 1e-12 {
            [0.0, 1.0, 0.0]
        } else {
            normalize(east)
        };
        let north = cross(d, east);
        let (sin, cos) = self.wind.to_radians().sin_cos();
        normalize([
            sin * east[0] + cos * north[0],
            sin * east[1] + cos * north[1],
            sin * east[2] + cos * north[2],
        ])
    }

    /// The rain at `d`. `step` is the angle of one step of the air, and
    /// `level` gives the level of the ground at a direction. A direction with
    /// no level ends the look back along the wind.
    pub(crate) fn at(&self, d: V3, step: f64, level: impl Fn(V3) -> Option<f64>) -> f64 {
        let shadow = self.shadow.clamp(0.0, 1.0);
        if shadow <= 0.0 {
            return 1.0;
        }
        let meters = |level: f64| {
            let low = level.floor().clamp(0.0, 65535.0) as u16;
            let t = level - f64::from(low);
            let high = low.saturating_add(1);
            level_to_meters(low) * (1.0 - t) + level_to_meters(high) * t - SEA_LEVEL
        };
        // The ground along the wind, from `d` back toward the wind.
        let mut ground = [0.0; TRACE_STEPS + 1];
        let mut count = 0;
        let (sin, cos) = step.sin_cos();
        let mut p = d;
        while count <= TRACE_STEPS {
            let Some(level) = level(p) else {
                break;
            };
            ground[count] = meters(level);
            count += 1;
            let up = self.upwind(p);
            p = normalize([
                cos * p[0] + sin * up[0],
                cos * p[1] + sin * up[1],
                cos * p[2] + sin * up[2],
            ]);
        }
        if count == 0 {
            return 1.0;
        }
        // The air comes in full of water and moves to `d`.
        let mut water = 1.0;
        for k in (0..count - 1).rev() {
            let (from, to) = (ground[k + 1].max(0.0), ground[k]);
            if to <= 0.0 {
                water += (1.0 - water) * RECHARGE;
            } else if to > from {
                water *= (-(to - from) / LIFT).exp();
            }
        }
        let height = ground[0].clamp(0.0, HIGH);
        let rain = water * (LOW_RAIN + (1.0 - LOW_RAIN) * height / HIGH) / peak();
        1.0 - shadow + shadow * rain.min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::meters_to_level;
    use crate::math::lonlat_to_dir;

    /// A range along a meridian at longitude 0, 2,000 m high and 10 degrees
    /// wide, with sea to the west of it at longitude -20 and lower.
    fn range(d: V3) -> Option<f64> {
        let (lon, _) = crate::math::dir_to_lonlat(d);
        let meters = if lon < -20.0 {
            -1000.0
        } else {
            200.0 + 2000.0 * (1.0 - lon.abs() / 5.0).max(0.0)
        };
        Some(f64::from(meters_to_level(meters)))
    }

    const WEST: Rain = Rain {
        wind: 270.0,
        shadow: 1.0,
    };

    fn rain_at(rain: Rain, lon: f64) -> f64 {
        rain.at(lonlat_to_dir(lon, 10.0), 0.5_f64.to_radians(), range)
    }

    #[test]
    fn the_wind_comes_from_its_direction() {
        let d = lonlat_to_dir(30.0, 40.0);
        let east = lonlat_to_dir(120.0, 0.0);
        assert!((dot(WEST.upwind(d), east) + 1.0).abs() < 1e-9);
        let north = Rain { wind: 0.0, ..WEST }.upwind(d);
        assert!(north[2] > 0.7, "{north:?}");
        assert!(dot(north, d).abs() < 1e-9);
    }

    #[test]
    fn the_lee_side_gets_less_rain() {
        let (west, east) = (rain_at(WEST, -3.0), rain_at(WEST, 3.0));
        assert!(west > 1.5 * east, "{west} {east}");
        let wind_from_east = Rain { wind: 90.0, ..WEST };
        assert!(rain_at(wind_from_east, 3.0) > rain_at(wind_from_east, -3.0));
    }

    #[test]
    fn higher_ground_on_the_windward_side_gets_more_rain() {
        let (foot, slope) = (rain_at(WEST, -4.5), rain_at(WEST, -2.0));
        assert!(slope > foot, "{slope} {foot}");
        for lon in [-15.0, -6.0, -3.0, 0.0, 3.0, 10.0] {
            let rain = rain_at(WEST, lon);
            assert!((0.0..=1.0).contains(&rain), "{lon} {rain}");
        }
    }

    #[test]
    fn no_shadow_gives_the_same_rain_everywhere() {
        let even = Rain {
            shadow: 0.0,
            ..WEST
        };
        assert!(even.is_even());
        for lon in [-10.0, -2.0, 0.0, 3.0] {
            assert_eq!(rain_at(even, lon), 1.0);
        }
        let half = Rain {
            shadow: 0.5,
            ..WEST
        };
        let lee = rain_at(half, 3.0);
        assert!(lee > 0.5 && lee < 1.0, "{lee}");
    }
}
