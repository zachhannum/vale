//! The measures of the geometry of a feature on the sphere of the world.

use vale_sphere::measure::{area_center, line_center, path_length, point_center, polygon_area};
use vale_store::{Geometry, GeometryKind, Value};

use super::Context;
use super::functions::{self, Function, finite};

/// A unit of area or length.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    Km2,
    Mi2,
    Ha,
    Acre,
    Km,
    Mi,
    M,
}

impl Unit {
    pub const AREA: [Unit; 4] = [Unit::Km2, Unit::Mi2, Unit::Ha, Unit::Acre];
    pub const LENGTH: [Unit; 3] = [Unit::Km, Unit::Mi, Unit::M];

    /// The unit argument of a measure function.
    pub fn code(self) -> &'static str {
        match self {
            Unit::Km2 => "km2",
            Unit::Mi2 => "mi2",
            Unit::Ha => "ha",
            Unit::Acre => "acre",
            Unit::Km => "km",
            Unit::Mi => "mi",
            Unit::M => "m",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Unit::Km2 => "km²",
            Unit::Mi2 => "mi²",
            Unit::Ha => "ha",
            Unit::Acre => "acres",
            Unit::Km => "km",
            Unit::Mi => "mi",
            Unit::M => "m",
        }
    }

    /// The size of the unit in square kilometers or in kilometers.
    fn km(self) -> f64 {
        match self {
            Unit::Km2 | Unit::Km => 1.0,
            Unit::Mi2 => 1.609_344 * 1.609_344,
            Unit::Ha => 0.01,
            Unit::Acre => 0.004_046_856_422_4,
            Unit::Mi => 1.609_344,
            Unit::M => 0.001,
        }
    }

    /// The unit of a code, with any case.
    fn parse(s: &str) -> Option<Unit> {
        (Unit::AREA.into_iter().chain(Unit::LENGTH)).find(|u| u.code().eq_ignore_ascii_case(s))
    }
}

/// A measure function of the geometry.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Measure {
    Area,
    Perimeter,
    Length,
}

impl Measure {
    pub const ALL: [Measure; 3] = [Measure::Area, Measure::Perimeter, Measure::Length];

    /// The name of the function.
    pub fn name(self) -> &'static str {
        match self {
            Measure::Area => "area",
            Measure::Perimeter => "perimeter",
            Measure::Length => "line_length",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Measure::Area => "Area",
            Measure::Perimeter => "Perimeter",
            Measure::Length => "Length",
        }
    }

    pub fn units(self) -> &'static [Unit] {
        match self {
            Measure::Area => &Unit::AREA,
            Measure::Perimeter | Measure::Length => &Unit::LENGTH,
        }
    }

    /// The unit if the call gives none.
    pub fn default_unit(self) -> Unit {
        self.units()[0]
    }

    /// The kind of geometry that the measure has a value for.
    pub fn geometry(self) -> GeometryKind {
        match self {
            Measure::Area | Measure::Perimeter => GeometryKind::Polygon,
            Measure::Length => GeometryKind::Line,
        }
    }

    /// The measure of a function name, in the case of `name()`.
    pub fn of_name(name: &str) -> Option<Measure> {
        Measure::ALL.into_iter().find(|m| m.name() == name)
    }

    pub fn function(self) -> &'static Function {
        functions::find(self.name()).expect("each measure is a function")
    }

    /// The size of the unit argument, the default if not given, or `None` for a wrong unit.
    fn unit_km(self, args: &[Value]) -> Option<f64> {
        let unit = match args.first() {
            None => self.default_unit(),
            Some(Value::Text(u)) => Unit::parse(u).filter(|u| self.units().contains(u))?,
            Some(_) => return None,
        };
        Some(unit.km())
    }
}

pub(super) fn area(args: &[Value], ctx: &Context) -> Value {
    let (Some(Geometry::Polygons(polygons)), Some(k)) = (ctx.shape, Measure::Area.unit_km(args))
    else {
        return Value::Null;
    };
    let km2: f64 = (polygons.iter())
        .map(|rings| polygon_area(rings, ctx.radius_km))
        .sum();
    finite(km2 / k)
}

pub(super) fn perimeter(args: &[Value], ctx: &Context) -> Value {
    let (Some(Geometry::Polygons(polygons)), Some(k)) =
        (ctx.shape, Measure::Perimeter.unit_km(args))
    else {
        return Value::Null;
    };
    let km: f64 = (polygons.iter().flatten())
        .map(|ring| path_length(ring, true, ctx.radius_km))
        .sum();
    finite(km / k)
}

pub(super) fn line_length(args: &[Value], ctx: &Context) -> Value {
    let (Some(Geometry::Lines(lines)), Some(k)) = (ctx.shape, Measure::Length.unit_km(args)) else {
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
