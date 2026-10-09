//! Layer and map styles.

pub use vale_render::Color;
use vale_store::GeometryKind;

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

/// The style of one layer.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerStyle {
    /// Polygons and point symbols.
    pub fill: Option<Color>,
    /// Polygon outlines and lines.
    pub stroke: Option<Stroke>,
    /// Points.
    pub point_radius: f64,
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

impl LayerStyle {
    /// The style of a new layer. `ordinal` picks a color and wraps around.
    pub fn default_for(kind: GeometryKind, ordinal: usize, fields: &[String]) -> Self {
        let field = guess_label_field(fields);
        let on = field.is_some();
        match kind {
            GeometryKind::Polygon => LayerStyle {
                fill: Some(pick(&POLYGON_FILLS, ordinal)),
                stroke: Some(Stroke {
                    color: Color::rgb(150, 160, 165),
                    width: 0.6,
                }),
                point_radius: 0.0,
                label: LabelStyle {
                    enabled: false,
                    field,
                    priority_field: None,
                    size: 11.0,
                    color: Color::rgb(40, 40, 40),
                    italic: false,
                },
            },
            GeometryKind::Line => LayerStyle {
                fill: None,
                stroke: Some(Stroke {
                    color: pick(&LINE_STROKES, ordinal),
                    width: 1.2,
                }),
                point_radius: 0.0,
                label: LabelStyle {
                    enabled: on,
                    field,
                    priority_field: None,
                    size: 11.0,
                    color: Color::rgb(40, 100, 160),
                    italic: true,
                },
            },
            GeometryKind::Point => LayerStyle {
                fill: Some(pick(&POINT_FILLS, ordinal)),
                stroke: Some(Stroke {
                    color: Color::rgb(255, 255, 255),
                    width: 1.0,
                }),
                point_radius: 2.5,
                label: LabelStyle {
                    enabled: on,
                    field,
                    priority_field: None,
                    size: 11.0,
                    color: Color::rgb(30, 30, 30),
                    italic: false,
                },
            },
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
        assert!(l.fill.is_none());
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
