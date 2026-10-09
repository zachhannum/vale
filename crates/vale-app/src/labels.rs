//! The label step of the map pipeline.

use std::collections::HashMap;

use kurbo::Rect;
use vale_labeler::{
    Feature, Fonts, Geometry, LabelClass, LabelInput, LinePlacement, Obstacle, ObstacleKind,
    Options, Placement, PointPlacement, TextStyle, place_labels,
};
use vale_render::{Color, GlyphRun, Item};
use vale_store::GeometryKind;

use crate::pipeline::{LabelSummary, PageLayer, Quality, UnplacedInfo};

const MAX_FEATURES: usize = 3000;

/// Places labels for the visible layers.
///
/// `layers` runs from the bottom of the drawing to the top. A layer nearer the top gets a higher
/// class priority.
pub fn place(
    layers: &[PageLayer<'_>],
    page: Rect,
    fonts: &mut Fonts,
    quality: Quality,
    obstacles: &[Rect],
) -> (Vec<Item>, LabelSummary) {
    let mut classes: Vec<LabelClass> = Vec::new();
    let mut class_colors: Vec<Color> = Vec::new();
    let mut features: Vec<Feature> = Vec::new();
    let mut layer_names: Vec<&str> = Vec::new();
    let mut next_id: u64 = 0;

    for (position, pl) in layers.iter().enumerate() {
        let style = &pl.entry.style;
        let label = &style.label;
        let Some(field) = label.field.as_deref().filter(|_| label.enabled) else {
            continue;
        };
        let is_line = match pl.layer.kind {
            GeometryKind::Point => false,
            GeometryKind::Line => true,
            GeometryKind::Polygon => continue,
        };
        let class = classes.len();
        classes.push(LabelClass {
            name: pl.layer.name.clone(),
            style: TextStyle {
                italic: label.italic,
                letter_spacing: if is_line { 0.5 } else { 0.0 },
                ..TextStyle::new("Noto Sans", label.size)
            },
            priority: position as i32,
            margin: 1.0,
            placement: if is_line {
                Placement::Line(LinePlacement {
                    repeat_distance: Some(600.0),
                    offset: 4.0,
                    ..Default::default()
                })
            } else {
                Placement::Point(PointPlacement::default())
            },
        });
        class_colors.push(label.color);

        let stroke_width = style.stroke.map_or(0.0, |s| s.width);
        let symbol_radius = if style.point_radius > 0.0 {
            style.point_radius + stroke_width
        } else {
            0.0
        };
        for f in &pl.features {
            let Some(feature) = pl.layer.features.get(f.index) else {
                continue;
            };
            let Some(text) = feature.attributes.get(field).and_then(|v| v.label()) else {
                continue;
            };
            let priority = label
                .priority_field
                .as_ref()
                .and_then(|p| feature.attributes.get(p))
                .and_then(|v| v.as_f64())
                .filter(|v| v.is_finite())
                .unwrap_or(0.0);
            let mut push = |geometry: Geometry| {
                features.push(Feature {
                    id: next_id,
                    class,
                    text: text.clone(),
                    priority,
                    geometry,
                });
                layer_names.push(&pl.layer.name);
                next_id += 1;
            };
            if is_line {
                for line in f.lines.iter().filter(|l| l.len() >= 2) {
                    push(Geometry::Line {
                        path: line.clone(),
                        stroke_width,
                    });
                }
            } else {
                for p in f.points.iter().filter(|p| page.contains(**p)) {
                    push(Geometry::Point {
                        position: *p,
                        symbol_radius,
                    });
                }
            }
        }
    }

    // Keep the most important features when there are too many. IDs stay as they are.
    if features.len() > MAX_FEATURES {
        let mut order: Vec<usize> = (0..features.len()).collect();
        order.sort_by(|&a, &b| {
            let (fa, fb) = (&features[a], &features[b]);
            let ka = (classes[fa.class].priority, fa.priority);
            let kb = (classes[fb.class].priority, fb.priority);
            kb.0.cmp(&ka.0).then(kb.1.total_cmp(&ka.1)).then(a.cmp(&b))
        });
        let mut keep = vec![false; features.len()];
        for &i in order.iter().take(MAX_FEATURES) {
            keep[i] = true;
        }
        let mut i = 0;
        features.retain(|_| {
            i += 1;
            keep[i - 1]
        });
    }
    let names: HashMap<u64, &str> = features
        .iter()
        .map(|f| (f.id, layer_names[f.id as usize]))
        .collect();

    let input = LabelInput {
        bounds: page,
        classes,
        features,
        obstacles: obstacles
            .iter()
            .map(|r| Obstacle {
                rect: *r,
                kind: ObstacleKind::Hard,
            })
            .collect(),
        options: Options {
            seed: 24301,
            improve: quality == Quality::Final,
            duplicate_distance: Some(300.0),
        },
    };
    let labeling = place_labels(fonts, &input);

    let mut items = Vec::new();
    for label in &labeling.placed {
        items.push(Item::Stroke {
            path: label.outlines(),
            color: Color::rgba(255, 255, 255, 217),
            width: 2.2,
            round: true,
        });
    }
    for label in &labeling.placed {
        let color = class_colors[label.class];
        for run in &label.glyph_runs {
            items.push(Item::Glyphs(GlyphRun {
                font: run.font.clone(),
                font_size: run.font_size,
                normalized_coords: run.normalized_coords.clone(),
                glyphs: run.glyphs.iter().map(|g| (g.id, g.transform)).collect(),
                text: label.text.clone(),
                color,
            }));
        }
    }
    let summary = LabelSummary {
        placed: labeling.placed.len(),
        unplaced: labeling
            .unplaced
            .iter()
            .map(|u| UnplacedInfo {
                layer: names.get(&u.feature).copied().unwrap_or("").to_string(),
                text: u.text.clone(),
                reason: format!("{:?}", u.reason),
            })
            .collect(),
    };
    (items, summary)
}
