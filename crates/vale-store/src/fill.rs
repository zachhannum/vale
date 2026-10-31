//! The fill of an area: a solid color, parallel lines, or crossed lines.

use std::ops::RangeInclusive;

use crate::Rgb;
use crate::atlas::{JsonMap, read_color, read_number};
use crate::gpkg::{hex, parse_color};

/// How an area is filled.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum FillKind {
    #[default]
    Solid,
    /// Parallel lines.
    Hatch,
    /// Two sets of parallel lines at a right angle.
    CrossHatch,
    /// No fill. The paper shows through.
    None,
}

impl FillKind {
    pub const ALL: [FillKind; 4] = [
        FillKind::Solid,
        FillKind::Hatch,
        FillKind::CrossHatch,
        FillKind::None,
    ];

    /// The name in a project file.
    pub fn id(self) -> &'static str {
        match self {
            FillKind::Solid => "solid",
            FillKind::Hatch => "hatch",
            FillKind::CrossHatch => "cross_hatch",
            FillKind::None => "none",
        }
    }

    pub fn from_id(id: &str) -> Option<FillKind> {
        FillKind::ALL.into_iter().find(|k| k.id() == id)
    }

    /// The UI text.
    pub fn label(self) -> &'static str {
        match self {
            FillKind::Solid => "Solid",
            FillKind::Hatch => "Hatch",
            FillKind::CrossHatch => "Cross-hatch",
            FillKind::None => "None",
        }
    }

    /// True for the kinds with lines.
    pub fn hatched(self) -> bool {
        matches!(self, FillKind::Hatch | FillKind::CrossHatch)
    }
}

/// The fill of an area. The lengths are in page millimeters, and the line
/// width is in points.
#[derive(Clone, Debug, PartialEq)]
pub struct FillPattern {
    pub kind: FillKind,
    /// The solid color, or the color of the lines.
    pub color: Rgb,
    /// The color behind the lines. `None`: the paper shows through.
    pub background: Option<Rgb>,
    /// The direction of the lines in degrees, counterclockwise from the
    /// horizontal. A cross-hatch adds lines at the angle plus 90°.
    pub angle: f64,
    /// The distance between two lines.
    pub spacing: f64,
    pub line_width: f64,
}

impl FillPattern {
    /// The angles that the app offers, in degrees.
    pub const ANGLES: RangeInclusive<f64> = 0.0..=180.0;
    /// The spacings in millimeters that the app offers.
    pub const SPACINGS: RangeInclusive<f64> = 0.2..=5.0;
    /// The line widths in points that the app offers.
    pub const LINE_WIDTHS: RangeInclusive<f64> = 0.1..=2.0;

    /// A solid fill of `color`.
    pub fn solid(color: Rgb) -> FillPattern {
        FillPattern {
            color,
            ..FillPattern::default()
        }
    }

    /// The fill as a JSON value.
    pub fn value(&self) -> serde_json::Value {
        let mut map = JsonMap::new();
        map.insert("kind".into(), self.kind.id().into());
        map.insert("color".into(), hex(self.color).into());
        map.insert("background".into(), self.background.map(hex).into());
        map.insert("angle".into(), self.angle.into());
        map.insert("spacing".into(), self.spacing.into());
        map.insert("line_width".into(), self.line_width.into());
        serde_json::Value::Object(map)
    }

    /// Reads [`FillPattern::value`]. A color as a string is a solid fill of
    /// that color. A missing key gets its default. `None` if a value is bad.
    pub fn read(v: &serde_json::Value) -> Option<FillPattern> {
        if let Some(text) = v.as_str() {
            return Some(FillPattern::solid(parse_color(text)?));
        }
        let map = v.as_object()?;
        let mut f = FillPattern::default();
        if let Some(k) = map.get("kind") {
            f.kind = FillKind::from_id(k.as_str()?)?;
        }
        read_color(map, "color", &mut f.color)?;
        match map.get("background") {
            None | Some(serde_json::Value::Null) => {}
            Some(c) => f.background = Some(parse_color(c.as_str()?)?),
        }
        read_number(map, "angle", &mut f.angle)?;
        read_number(map, "spacing", &mut f.spacing)?;
        read_number(map, "line_width", &mut f.line_width)?;
        (f.spacing > 0.0 && f.line_width > 0.0).then_some(f)
    }
}

impl Default for FillPattern {
    fn default() -> Self {
        FillPattern {
            kind: FillKind::Solid,
            color: [0x2b, 0x2b, 0x2b],
            background: None,
            angle: 45.0,
            spacing: 0.8,
            line_width: 0.35,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fill_survives_json() {
        let fill = FillPattern {
            kind: FillKind::CrossHatch,
            color: [1, 2, 3],
            background: Some([4, 5, 6]),
            angle: 30.0,
            spacing: 1.5,
            line_width: 0.2,
        };
        assert_eq!(FillPattern::read(&fill.value()), Some(fill.clone()));
        let clear = FillPattern {
            background: None,
            ..fill
        };
        assert_eq!(FillPattern::read(&clear.value()), Some(clear));
        for k in FillKind::ALL {
            assert_eq!(FillKind::from_id(k.id()), Some(k));
        }
    }

    #[test]
    fn a_color_is_a_solid_fill() {
        let v = serde_json::json!("102030");
        let f = FillPattern::read(&v).unwrap();
        assert_eq!(f, FillPattern::solid([0x10, 0x20, 0x30]));
        assert_eq!(
            FillPattern::read(&serde_json::json!({})),
            Some(FillPattern::default())
        );
        for bad in [
            serde_json::json!({"kind": "dots"}),
            serde_json::json!({"spacing": 0}),
            serde_json::json!({"background": 3}),
            serde_json::json!(4),
        ] {
            assert_eq!(FillPattern::read(&bad), None, "{bad}");
        }
    }
}
