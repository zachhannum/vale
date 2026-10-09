//! Map furniture, the scale bar.

use kurbo::{BezPath, Point, Rect, Shape};
use vale_labeler::{
    Feature, Fonts, Geometry, LabelClass, LabelInput, Options, Placement, PointPlacement,
    PointPosition, TextStyle, place_labels,
};
use vale_render::{Color, FillRule, GlyphRun, Item};

/// The largest of 1, 2, 5 times a power of ten that is at most `v`.
pub fn nice(v: f64) -> f64 {
    let p = 10f64.powf(v.log10().floor());
    [5.0, 2.0, 1.0]
        .iter()
        .map(|m| m * p)
        .find(|c| *c <= v * (1.0 + 1e-12))
        .unwrap_or(p)
}

/// The scale bar and the rectangle that it covers.
pub fn scale_bar(page: Rect, km_per_point: f64, fonts: &mut Fonts) -> (Vec<Item>, Rect) {
    if !(km_per_point.is_finite() && km_per_point > 0.0)
        || page.width() < 240.0
        || page.height() < 120.0
    {
        return (vec![], Rect::ZERO);
    }
    let km = nice(140.0 * km_per_point);
    let length = km / km_per_point;
    let text = if km >= 1.0 {
        format!("{} km", km.round() as i64)
    } else {
        format!("{} m", (km * 1000.0).round() as i64)
    };
    let (x0, y) = (page.x0 + 22.0, page.y1 - 22.0);
    let x1 = x0 + length;
    let ink = Color::rgb(40, 40, 40);

    let mut path = BezPath::new();
    path.move_to((x0, y - 5.0));
    path.line_to((x0, y));
    path.line_to((x1, y));
    path.line_to((x1, y - 5.0));

    let input = LabelInput {
        bounds: page,
        classes: vec![LabelClass {
            name: "scale".to_string(),
            style: TextStyle::new("Noto Sans", 10.0),
            priority: 0,
            margin: 0.0,
            placement: Placement::Point(PointPlacement {
                positions: vec![PointPosition::TopRight],
                offset: 0.0,
            }),
        }],
        features: vec![Feature {
            id: 0,
            class: 0,
            text,
            priority: 0.0,
            geometry: Geometry::Point {
                position: Point::new(x0, y - 7.0),
                symbol_radius: 0.0,
            },
        }],
        obstacles: vec![],
        options: Options {
            seed: 1,
            improve: false,
            duplicate_distance: None,
        },
    };
    let labeling = place_labels(fonts, &input);

    let mut covered = Rect::new(x0, y - 5.0, x1, y);
    let mut glyphs = Vec::new();
    if let Some(label) = labeling.placed.first() {
        covered = covered.union(label.bounds);
        for run in &label.glyph_runs {
            glyphs.push(Item::Glyphs(GlyphRun {
                font: run.font.clone(),
                font_size: run.font_size,
                normalized_coords: run.normalized_coords.clone(),
                glyphs: run.glyphs.iter().map(|g| (g.id, g.transform)).collect(),
                text: label.text.clone(),
                color: ink,
            }));
        }
    }
    let backing = covered.inflate(8.0, 8.0);
    let mut items = vec![
        Item::Fill {
            path: backing.to_path(0.1),
            color: Color::rgba(255, 255, 255, 200),
            rule: FillRule::NonZero,
        },
        Item::Stroke {
            path,
            color: ink,
            width: 1.5,
            round: false,
        },
    ];
    items.extend(glyphs);
    (items, backing)
}
