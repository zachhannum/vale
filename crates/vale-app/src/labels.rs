//! The label step of the map pipeline.

use std::collections::{HashMap, HashSet};

use kurbo::Rect;
use vale_labeler::{
    Feature, Fonts, Geometry, LabelClass, LabelInput, LinePlacement, Obstacle, ObstacleKind,
    Options, Placement, PointPlacement, TextStyle, place_labels,
};
use vale_render::{Color, GlyphRun, Item};
use vale_store::GeometryKind;

use crate::pipeline::{LabelSummary, PageLayer, PlacedSymbol, Quality, UnplacedInfo};

const MAX_FEATURES: usize = 3000;

/// The most scatter symbols that labels avoid.
const MAX_SOFT: usize = 20_000;

/// The cost of a label over one scatter symbol.
const SCATTER_COST: f64 = 0.5;

/// Places labels for the visible layers.
///
/// `layers` runs from the bottom of the drawing to the top. A layer nearer the top gets a higher
/// class priority. Labels do not overlap `obstacles` or a point symbol. A label over a scatter
/// symbol has a cost.
pub fn place(
    layers: &[PageLayer<'_>],
    page: Rect,
    fonts: &mut Fonts,
    quality: Quality,
    obstacles: &[Rect],
    symbols: &[PlacedSymbol],
) -> (Vec<Item>, LabelSummary) {
    let mut labeled = HashSet::new();
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
        let is_line = match pl.kind {
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
                ..TextStyle::new(crate::fonts::family(label, fonts), label.size)
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
            let extent = pl.extent(f.index);
            let stroke_width = extent.stroke_width;
            let symbol_radius = if extent.marker_radius > 0.0 {
                extent.marker_radius + stroke_width
            } else {
                0.0
            };
            labeled.insert((pl.layer.id, f.index));
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
        obstacles: symbol_obstacles(obstacles, symbols, &labeled, page),
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

/// The obstacles: each rectangle of `hard`, each point symbol, and the
/// scatter symbols. The label of a feature avoids its own symbol by its
/// symbol radius, so the symbol of a labeled feature is not an obstacle.
fn symbol_obstacles(
    hard: &[Rect],
    symbols: &[PlacedSymbol],
    labeled: &HashSet<(vale_store::LayerId, usize)>,
    page: Rect,
) -> Vec<Obstacle> {
    let mut out: Vec<Obstacle> = hard
        .iter()
        .map(|r| Obstacle {
            rect: *r,
            kind: ObstacleKind::Hard,
        })
        .collect();
    let meets = |r: Rect| r.x0 <= page.x1 && r.x1 >= page.x0 && r.y0 <= page.y1 && r.y1 >= page.y0;
    let shown = symbols.iter().filter(|s| meets(s.bounds));
    let (scatter, points): (Vec<_>, Vec<_>) = shown.partition(|s| s.lonlat.is_some());
    out.extend(
        points
            .into_iter()
            .filter(|s| !labeled.contains(&(s.layer, s.feature)))
            .map(|s| Obstacle {
                rect: s.bounds,
                kind: ObstacleKind::Hard,
            }),
    );
    out.extend(scatter.into_iter().take(MAX_SOFT).map(|s| Obstacle {
        rect: s.bounds,
        kind: ObstacleKind::Soft { cost: SCATTER_COST },
    }));
    out
}
