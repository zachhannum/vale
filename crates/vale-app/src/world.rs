//! The radius of a world and the display of a distance on the ground.

use std::ops::RangeInclusive;

use vale_store::Units;

/// The limits of the radius of a world, in kilometers.
pub const RADIUS_KM: RangeInclusive<f64> = 100.0..=100_000.0;

/// The radius presets, each with its name and its radius in kilometers.
pub const PRESETS: [(&str, f64); 3] = [("Earth", 6371.0), ("Mars", 3389.5), ("Moon", 1737.4)];

const KM_PER_MILE: f64 = 1.609_344;
const KM_PER_FOOT: f64 = 0.000_304_8;

/// The preset that has this radius.
pub fn preset_of(radius_km: f64) -> Option<usize> {
    PRESETS
        .iter()
        .position(|(_, km)| (km - radius_km).abs() < 1e-6)
}

/// The name of the units in the UI.
pub fn units_name(units: Units) -> &'static str {
    match units {
        Units::Metric => "Kilometers",
        Units::Imperial => "Miles",
    }
}

/// The large unit of distance: its length in kilometers and its symbol.
pub fn large_unit(units: Units) -> (f64, &'static str) {
    match units {
        Units::Metric => (1.0, "km"),
        Units::Imperial => (KM_PER_MILE, "mi"),
    }
}

/// The small unit of distance: its length in kilometers and its symbol.
pub fn small_unit(units: Units) -> (f64, &'static str) {
    match units {
        Units::Metric => (0.001, "m"),
        Units::Imperial => (KM_PER_FOOT, "ft"),
    }
}

/// The unit that shows `km` as a value of 1 or more, if one does.
pub fn unit_for(km: f64, units: Units) -> (f64, &'static str) {
    let large = large_unit(units);
    if km >= large.0 {
        large
    } else {
        small_unit(units)
    }
}

/// A distance with three significant digits and its unit.
pub fn distance(km: f64, units: Units) -> String {
    let (unit_km, symbol) = unit_for(km, units);
    format!("{} {symbol}", three_significant(km / unit_km))
}

pub fn three_significant(v: f64) -> String {
    if !v.is_finite() || v <= 0.0 {
        return "–".to_string();
    }
    let digits = 2 - v.log10().floor() as i32;
    format!("{:.*}", digits.max(0) as usize, v)
}

/// The radius in kilometers that the text gives in the large unit. The
/// result is `None` if the text is not a radius within `RADIUS_KM`.
pub fn parse_radius(text: &str, units: Units) -> Option<f64> {
    let value: f64 = text.trim().replace(',', "").parse().ok()?;
    let km = value * large_unit(units).0;
    RADIUS_KM.contains(&km).then_some(km)
}

/// The radius in the large unit, as text that `parse_radius` reads.
pub fn radius_text(radius_km: f64, units: Units) -> String {
    let value = radius_km / large_unit(units).0;
    let text = format!("{value:.1}");
    text.strip_suffix(".0").unwrap_or(&text).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_distance_below_one_large_unit_is_in_the_small_unit() {
        assert_eq!(distance(12.345, Units::Metric), "12.3 km");
        assert_eq!(distance(0.25, Units::Metric), "250 m");
        assert_eq!(distance(1.0, Units::Metric), "1.00 km");
        assert_eq!(distance(16.093_44, Units::Imperial), "10.0 mi");
        assert_eq!(distance(0.3048, Units::Imperial), "1000 ft");
        assert_eq!(distance(0.0, Units::Metric), "– m");
    }

    #[test]
    fn a_preset_has_its_radius() {
        assert_eq!(preset_of(3389.5), Some(1));
        assert_eq!(preset_of(3389.0), None);
        for (_, km) in PRESETS {
            assert!(RADIUS_KM.contains(&km));
        }
    }

    #[test]
    fn the_radius_text_is_in_the_display_units() {
        assert_eq!(radius_text(6371.0, Units::Metric), "6371");
        assert_eq!(radius_text(1737.4, Units::Metric), "1737.4");
        let miles = radius_text(6371.0, Units::Imperial);
        assert_eq!(miles, "3958.8");
        let km = parse_radius(&miles, Units::Imperial).unwrap();
        assert!((km - 6371.0).abs() < 0.1, "{km}");
        assert_eq!(parse_radius(" 1,737.4 ", Units::Metric), Some(1737.4));
        assert_eq!(parse_radius("50", Units::Metric), None);
        assert_eq!(parse_radius("abc", Units::Metric), None);
        assert_eq!(parse_radius("inf", Units::Metric), None);
    }
}
