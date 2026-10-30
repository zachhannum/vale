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

/// A value of 100 or more as a grouped whole number, and a smaller value
/// with three significant digits. Zero is "0".
fn total(value: f64) -> String {
    if value >= 99.95 {
        grouped(value.round() as u64)
    } else if value > 0.0 {
        three_significant(value)
    } else {
        "0".to_string()
    }
}

/// The length of a measured path and its unit.
pub fn length(km: f64, units: Units) -> String {
    let (unit_km, symbol) = unit_for(km.max(0.0), units);
    format!("{} {symbol}", total(km / unit_km))
}

/// An area in square kilometers or square miles.
pub fn area(km2: f64, units: Units) -> String {
    let (unit_km, symbol) = large_unit(units);
    format!("{} {symbol}²", total(km2 / (unit_km * unit_km)))
}

/// A whole number with a comma after each three digits.
pub fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A distance with two significant digits and its unit, for a value that
/// a person compares with another one.
pub fn short_distance(km: f64, units: Units) -> String {
    let (unit_km, symbol) = unit_for(km, units);
    let value = km / unit_km;
    if !value.is_finite() || value <= 0.0 {
        return format!("– {symbol}");
    }
    let digits = 1 - value.log10().floor() as i32;
    let step = 10f64.powi(digits);
    let rounded = (value * step).round() / step;
    if digits > 0 {
        format!("{rounded:.*} {symbol}", digits as usize)
    } else {
        format!("{} {symbol}", grouped(rounded as u64))
    }
}

/// The number of decimals that show a value of a raster layer. `span` is
/// the size of the value range of the layer.
pub fn value_decimals(span: f64) -> usize {
    if !span.is_finite() || span <= 0.0 {
        return 0;
    }
    (3.0 - span.log10()).ceil().clamp(0.0, 6.0) as usize
}

/// A value of a raster layer with its unit. `span` is the size of the value
/// range of the layer.
pub fn value_text(value: f64, span: f64, unit: &str) -> String {
    let text = format!("{value:.*}", value_decimals(span));
    // A value near zero has no sign.
    let text = match text.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
        _ => text,
    };
    if unit.is_empty() {
        text
    } else {
        format!("{text} {unit}")
    }
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

/// The radii at the two ends of the radius slider. A typed radius can be
/// outside these limits.
pub const SLIDER_KM: RangeInclusive<f64> = 1000.0..=20_000.0;

/// The part of the radius slider on each side of a preset that gives the
/// preset.
const SNAP: f64 = 0.03;

/// The place of a radius on the radius slider, from 0 to 1. The scale is
/// logarithmic. A radius outside the limits of the slider is at an end.
pub fn radius_fraction(radius_km: f64) -> f64 {
    let (start, end) = (*SLIDER_KM.start(), *SLIDER_KM.end());
    (radius_km.clamp(start, end) / start).ln() / (end / start).ln()
}

/// The radius in kilometers at a place of the radius slider. A place near
/// a preset gives the preset, and each end gives its limit. Each other
/// place gives a value of three significant digits in the large unit of
/// `units`.
pub fn slider_radius(fraction: f64, units: Units) -> f64 {
    let (start, end) = (*SLIDER_KM.start(), *SLIDER_KM.end());
    if fraction <= 0.0 {
        return start;
    }
    if fraction >= 1.0 {
        return end;
    }
    let near = |(_, km): &(&str, f64)| (radius_fraction(*km) - fraction).abs() <= SNAP;
    if let Some((_, km)) = PRESETS.iter().find(|preset| near(preset)) {
        return *km;
    }
    let unit = large_unit(units).0;
    let value = start * (end / start).powf(fraction) / unit;
    let step = 10f64.powf(value.log10().floor() - 2.0);
    ((value / step).round() * step * unit).clamp(start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_length_and_an_area_have_the_display_units() {
        assert_eq!(length(1234.4, Units::Metric), "1,234 km");
        assert_eq!(length(12.34, Units::Metric), "12.3 km");
        assert_eq!(length(0.25, Units::Metric), "250 m");
        assert_eq!(length(0.0, Units::Metric), "0 m");
        assert_eq!(length(1609.344, Units::Imperial), "1,000 mi");
        assert_eq!(area(56_700.0, Units::Metric), "56,700 km²");
        assert_eq!(area(0.5, Units::Metric), "0.500 km²");
        let square_mile = KM_PER_MILE * KM_PER_MILE;
        assert_eq!(area(2500.0 * square_mile, Units::Imperial), "2,500 mi²");
    }

    #[test]
    fn the_radius_slider_has_each_preset_at_its_place_on_the_scale() {
        assert_eq!(radius_fraction(1000.0), 0.0);
        assert_eq!(radius_fraction(20_000.0), 1.0);
        // A typed radius outside the slider is at an end.
        assert_eq!(radius_fraction(200.0), 0.0);
        assert_eq!(radius_fraction(70_000.0), 1.0);
        let places = PRESETS.map(|(_, km)| (radius_fraction(km) * 100.0).round());
        assert_eq!(places, [62.0, 41.0, 18.0]);
        for (_, km) in PRESETS {
            // A place near the preset gives the preset.
            let place = radius_fraction(km);
            for near in [place - 0.025, place, place + 0.025] {
                assert_eq!(slider_radius(near, Units::Metric), km);
                assert_eq!(slider_radius(near, Units::Imperial), km);
            }
        }
        assert_eq!(slider_radius(0.0, Units::Metric), 1000.0);
        assert_eq!(slider_radius(1.0, Units::Imperial), 20_000.0);
    }

    #[test]
    fn the_radius_slider_gives_three_significant_digits() {
        let mut last = 0.0;
        for step in 0..=400 {
            let fraction = f64::from(step) / 400.0;
            let km = slider_radius(fraction, Units::Metric);
            assert!(km >= last && SLIDER_KM.contains(&km), "{fraction}: {km}");
            last = km;
            if preset_of(km).is_none() {
                assert_eq!(three_significant(km).parse::<f64>().unwrap(), km);
            }
            // The thumb stays near the finger.
            assert!((radius_fraction(km) - fraction).abs() <= SNAP + 1e-9);
        }
        let miles = slider_radius(0.9, Units::Imperial) / KM_PER_MILE;
        let round: f64 = three_significant(miles).parse().unwrap();
        assert!((miles - round).abs() < 1e-6, "{miles}");
    }

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
