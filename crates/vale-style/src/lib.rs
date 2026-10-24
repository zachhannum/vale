//! Layer and map styles.

pub mod expr;
mod json;
mod rule;

pub use json::{STYLE_VERSION, StyleError};
pub use rule::{
    Extent, METERS_PER_POINT, Paint, RasterSymbolizer, Rule, StrokeSymbolizer, Style, Symbolizer,
    extent_of, scale_denominator,
};
pub use vale_render::Color;
use vale_store::GeometryKind;

use expr::Property;

/// A solid stroke. The width is in points.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
}

/// How a layer is labeled.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelStyle {
    pub enabled: bool,
    /// The attribute that gives the text.
    pub field: Option<String>,
    /// A numeric attribute. Higher is placed first.
    pub priority_field: Option<String>,
    /// Points.
    pub size: f64,
    pub color: Color,
    pub italic: bool,
}

/// The style of one layer: its rules and its labels.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerStyle {
    pub style: Style,
    pub label: LabelStyle,
}

const POLYGON_FILLS: [&str; 6] = [
    "#f4f1e8", "#e3ead3", "#f0e0c8", "#dfe6ee", "#ecdcdc", "#e6e0f0",
];
const LINE_STROKES: [&str; 6] = [
    "#5a96c8", "#b0703c", "#5c8a5c", "#8a5ca8", "#c0504d", "#707070",
];
const POINT_FILLS: [&str; 6] = [
    "#3c3c3c", "#b03030", "#2f6f9f", "#3f7f3f", "#8a5ca8", "#b0703c",
];

fn pick(list: &[&str; 6], ordinal: usize) -> Color {
    Color::from_hex(list[ordinal % 6]).expect("valid built-in color")
}

fn fill(color: Color) -> Symbolizer {
    Symbolizer::Fill {
        color: Property::constant(color),
    }
}

fn stroke(color: Color, width: f64) -> Symbolizer {
    Symbolizer::Stroke(StrokeSymbolizer::constant(Stroke { color, width }))
}

impl LayerStyle {
    /// The style of a new layer: one rule with constant symbolizers. `ordinal` picks a color and wraps around.
    pub fn default_for(kind: GeometryKind, ordinal: usize, fields: &[String]) -> Self {
        let field = guess_label_field(fields);
        let on = field.is_some();
        let (symbolizers, label) = match kind {
            GeometryKind::Polygon => (
                vec![
                    fill(pick(&POLYGON_FILLS, ordinal)),
                    stroke(Color::rgb(150, 160, 165), 0.6),
                ],
                LabelStyle {
                    enabled: false,
                    field,
                    priority_field: None,
                    size: 11.0,
                    color: Color::rgb(40, 40, 40),
                    italic: false,
                },
            ),
            GeometryKind::Line => (
                vec![stroke(pick(&LINE_STROKES, ordinal), 1.2)],
                LabelStyle {
                    enabled: on,
                    field,
                    priority_field: None,
                    size: 11.0,
                    color: Color::rgb(40, 100, 160),
                    italic: true,
                },
            ),
            GeometryKind::Point => (
                vec![Symbolizer::Marker {
                    radius: Property::constant(2.5),
                    fill: Some(Property::constant(pick(&POINT_FILLS, ordinal))),
                    stroke: Some(StrokeSymbolizer::constant(Stroke {
                        color: Color::rgb(255, 255, 255),
                        width: 1.0,
                    })),
                }],
                LabelStyle {
                    enabled: on,
                    field,
                    priority_field: None,
                    size: 11.0,
                    color: Color::rgb(30, 30, 30),
                    italic: false,
                },
            ),
        };
        LayerStyle {
            style: Style {
                rules: vec![Rule::new("Default", symbolizers)],
            },
            label,
        }
    }

    /// The style of a raster layer: one rule with `colors`.
    pub fn raster(colors: RasterSymbolizer) -> Self {
        LayerStyle {
            style: Style {
                rules: vec![Rule::new("Colors", vec![Symbolizer::Raster(colors)])],
            },
            label: LabelStyle {
                enabled: false,
                field: None,
                priority_field: None,
                size: 11.0,
                color: Color::rgb(40, 40, 40),
                italic: false,
            },
        }
    }

    /// The first raster symbolizer, in rule order.
    pub fn raster_symbolizer(&self) -> Option<&RasterSymbolizer> {
        self.style
            .rules
            .iter()
            .flat_map(|r| &r.symbolizers)
            .find_map(|s| match s {
                Symbolizer::Raster(r) => Some(r),
                _ => None,
            })
    }

    /// Replaces the first raster symbolizer, or adds a rule with `colors` if the style has none.
    pub fn set_raster_symbolizer(&mut self, colors: RasterSymbolizer) {
        let first = self
            .style
            .rules
            .iter_mut()
            .flat_map(|r| &mut r.symbolizers)
            .find_map(|s| match s {
                Symbolizer::Raster(r) => Some(r),
                _ => None,
            });
        match first {
            Some(r) => *r = colors,
            None => self
                .style
                .rules
                .push(Rule::new("Colors", vec![Symbolizer::Raster(colors)])),
        }
    }

    /// True if the style is one rule with no filter, no scale range, and no expression. The simple controls edit only such a style.
    pub fn is_simple(&self) -> bool {
        match self.style.rules.as_slice() {
            [] => true,
            [r] => {
                r.filter.is_all()
                    && r.min_scale.is_none()
                    && r.max_scale.is_none()
                    && r.symbolizers.iter().all(Symbolizer::is_constant)
            }
            _ => false,
        }
    }

    fn first_rule(&self) -> Option<&Rule> {
        self.style.rules.first()
    }

    fn first_rule_mut(&mut self) -> &mut Rule {
        if self.style.rules.is_empty() {
            self.style.rules.push(Rule::new("Default", Vec::new()));
        }
        &mut self.style.rules[0]
    }

    /// The fill of the first rule, or the fill of its marker.
    pub fn fill(&self) -> Option<Color> {
        self.first_rule()?
            .symbolizers
            .iter()
            .find_map(|s| match s {
                Symbolizer::Fill { color } => Some(Some(color.fallback)),
                Symbolizer::Marker { fill, .. } => Some(fill.as_ref().map(|c| c.fallback)),
                Symbolizer::Stroke(_) | Symbolizer::Raster(_) => None,
            })
            .flatten()
    }

    /// Sets the fill of the first rule, or the fill of its marker.
    pub fn set_fill(&mut self, color: Option<Color>) {
        let rule = self.first_rule_mut();
        if let Some(Symbolizer::Marker { fill, .. }) = rule
            .symbolizers
            .iter_mut()
            .find(|s| matches!(s, Symbolizer::Marker { .. }))
        {
            *fill = color.map(Property::constant);
            return;
        }
        let at = rule
            .symbolizers
            .iter()
            .position(|s| matches!(s, Symbolizer::Fill { .. }));
        match (at, color) {
            (Some(i), Some(c)) => rule.symbolizers[i] = fill(c),
            (Some(i), None) => {
                rule.symbolizers.remove(i);
            }
            (None, Some(c)) => rule.symbolizers.insert(0, fill(c)),
            (None, None) => {}
        }
    }

    /// The stroke of the first rule, or the stroke of its marker.
    pub fn stroke(&self) -> Option<Stroke> {
        self.first_rule()?
            .symbolizers
            .iter()
            .find_map(|s| match s {
                Symbolizer::Stroke(s) => Some(Some(s.fallback())),
                Symbolizer::Marker { stroke, .. } => {
                    Some(stroke.as_ref().map(StrokeSymbolizer::fallback))
                }
                Symbolizer::Fill { .. } | Symbolizer::Raster(_) => None,
            })
            .flatten()
    }

    /// Sets the stroke of the first rule, or the stroke of its marker.
    pub fn set_stroke(&mut self, value: Option<Stroke>) {
        let rule = self.first_rule_mut();
        if let Some(Symbolizer::Marker { stroke, .. }) = rule
            .symbolizers
            .iter_mut()
            .find(|s| matches!(s, Symbolizer::Marker { .. }))
        {
            *stroke = value.map(StrokeSymbolizer::constant);
            return;
        }
        let at = rule
            .symbolizers
            .iter()
            .position(|s| matches!(s, Symbolizer::Stroke(_)));
        match (at, value) {
            (Some(i), Some(v)) => rule.symbolizers[i] = stroke(v.color, v.width),
            (Some(i), None) => {
                rule.symbolizers.remove(i);
            }
            (None, Some(v)) => rule.symbolizers.push(stroke(v.color, v.width)),
            (None, None) => {}
        }
    }

    /// The marker radius of the first rule, or 0 with no marker.
    pub fn point_radius(&self) -> f64 {
        self.first_rule()
            .and_then(|r| {
                r.symbolizers.iter().find_map(|s| match s {
                    Symbolizer::Marker { radius, .. } => Some(radius.fallback),
                    _ => None,
                })
            })
            .unwrap_or(0.0)
    }

    /// Sets the marker radius of the first rule. A rule with no marker does not change.
    pub fn set_point_radius(&mut self, value: f64) {
        for s in &mut self.first_rule_mut().symbolizers {
            if let Symbolizer::Marker { radius, .. } = s {
                *radius = Property::constant(value);
                return;
            }
        }
    }
}

/// The style of the whole map.
#[derive(Clone, Debug, PartialEq)]
pub struct MapStyle {
    pub background: Color,
    pub water: Color,
    pub outline: Stroke,
    pub graticule: Stroke,
}

impl Default for MapStyle {
    fn default() -> Self {
        MapStyle {
            background: Color::rgb(236, 236, 232),
            water: Color::rgb(214, 232, 242),
            outline: Stroke {
                color: Color::rgb(90, 100, 105),
                width: 0.8,
            },
            graticule: Stroke {
                color: Color::rgba(120, 140, 150, 110),
                width: 0.5,
            },
        }
    }
}

/// Guesses the attribute that holds the name of a feature.
pub fn guess_label_field(fields: &[String]) -> Option<String> {
    for want in ["name", "NAME", "Name", "title", "label"] {
        if fields.iter().any(|f| f == want) {
            return Some(want.to_string());
        }
    }
    fields
        .iter()
        .find(|f| f.to_lowercase().contains("name"))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn guess_rules() {
        assert_eq!(
            guess_label_field(&s(&["x", "title", "name"])),
            Some("name".into())
        );
        assert_eq!(
            guess_label_field(&s(&["Name", "label"])),
            Some("Name".into())
        );
        assert_eq!(guess_label_field(&s(&["x", "label"])), Some("label".into()));
        assert_eq!(
            guess_label_field(&s(&["a", "name_en"])),
            Some("name_en".into())
        );
        assert_eq!(guess_label_field(&s(&["a", "b"])), None);
    }

    #[test]
    fn point_labels() {
        let on = LayerStyle::default_for(GeometryKind::Point, 0, &s(&["name"]));
        assert!(on.label.enabled);
        assert_eq!(on.label.field.as_deref(), Some("name"));
        let off = LayerStyle::default_for(GeometryKind::Point, 0, &s(&["pop"]));
        assert!(!off.label.enabled);
    }

    #[test]
    fn polygon_and_line_defaults() {
        let p = LayerStyle::default_for(GeometryKind::Polygon, 0, &s(&["name"]));
        assert!(!p.label.enabled);
        assert!(p.label.field.is_some());
        let l = LayerStyle::default_for(GeometryKind::Line, 0, &s(&["name"]));
        assert!(l.label.enabled);
        assert!(l.label.italic);
        assert!(l.fill().is_none());
        assert!(l.is_simple());
    }

    #[test]
    fn ordinal_wraps() {
        for kind in [
            GeometryKind::Point,
            GeometryKind::Line,
            GeometryKind::Polygon,
        ] {
            assert_eq!(
                LayerStyle::default_for(kind, 6, &[]),
                LayerStyle::default_for(kind, 0, &[])
            );
        }
        assert_eq!(MapStyle::default().outline.width, 0.8);
    }
}
