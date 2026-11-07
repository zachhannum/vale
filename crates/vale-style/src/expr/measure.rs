//! The measures of the geometry of a feature on the sphere of the world.

use vale_sphere::measure::{area_center, line_center, path_length, point_center, polygon_area};
use vale_store::{Geometry, Value};

use super::Context;
use super::functions::finite;

/// The number of square kilometers in an area unit.
fn area_unit(unit: &str) -> Option<f64> {
    Some(match unit.to_ascii_lowercase().as_str() {
        "km2" => 1.0,
        "mi2" => 1.609_344 * 1.609_344,
        "ha" => 0.01,
        "acre" => 0.004_046_856_422_4,
        _ => return None,
    })
}

/// The number of kilometers in a length unit.
fn length_unit(unit: &str) -> Option<f64> {
    Some(match unit.to_ascii_lowercase().as_str() {
        "km" => 1.0,
        "mi" => 1.609_344,
        "m" => 0.001,
        _ => return None,
    })
}

/// The unit argument: its size, the default if not given, or `None` for a wrong unit.
fn unit(args: &[Value], size: fn(&str) -> Option<f64>, default: &str) -> Option<f64> {
    match args.first() {
        None => size(default),
        Some(Value::Text(u)) => size(u),
        Some(_) => None,
    }
}

pub(super) fn area(args: &[Value], ctx: &Context) -> Value {
    let (Some(Geometry::Polygons(polygons)), Some(k)) = (ctx.shape, unit(args, area_unit, "km2"))
    else {
        return Value::Null;
    };
    let km2: f64 = (polygons.iter())
        .map(|rings| polygon_area(rings, ctx.radius_km))
        .sum();
    finite(km2 / k)
}

pub(super) fn perimeter(args: &[Value], ctx: &Context) -> Value {
    let (Some(Geometry::Polygons(polygons)), Some(k)) = (ctx.shape, unit(args, length_unit, "km"))
    else {
        return Value::Null;
    };
    let km: f64 = (polygons.iter().flatten())
        .map(|ring| path_length(ring, true, ctx.radius_km))
        .sum();
    finite(km / k)
}

pub(super) fn line_length(args: &[Value], ctx: &Context) -> Value {
    let (Some(Geometry::Lines(lines)), Some(k)) = (ctx.shape, unit(args, length_unit, "km")) else {
        return Value::Null;
    };
    let km: f64 = (lines.iter())
        .map(|line| path_length(line, false, ctx.radius_km))
        .sum();
    finite(km / k)
}

/// The center of the feature in degrees, longitude first.
pub(super) fn center(ctx: &Context) -> Option<[f64; 2]> {
    match ctx.shape? {
        Geometry::Points(points) => point_center(points),
        Geometry::Lines(lines) => line_center(lines),
        Geometry::Polygons(polygons) => area_center(polygons),
    }
}

pub(super) fn parts(ctx: &Context) -> Value {
    match ctx.shape {
        Some(Geometry::Lines(lines)) => Value::Number(lines.len() as f64),
        Some(Geometry::Polygons(polygons)) => Value::Number(polygons.len() as f64),
        _ => Value::Null,
    }
}

/// A ring that repeats its first position at the end counts it once.
pub(super) fn vertices(ctx: &Context) -> Value {
    let n = match ctx.shape {
        None => return Value::Null,
        Some(Geometry::Points(points)) => points.len(),
        Some(Geometry::Lines(lines)) => lines.iter().map(Vec::len).sum(),
        Some(Geometry::Polygons(polygons)) => (polygons.iter().flatten())
            .map(|ring| match (ring.first(), ring.last()) {
                (Some(a), Some(b)) if ring.len() > 1 && a == b => ring.len() - 1,
                _ => ring.len(),
            })
            .sum(),
    };
    Value::Number(n as f64)
}
