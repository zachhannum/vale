//! The label step of the map pipeline.

use std::collections::{HashMap, HashSet};

use kurbo::Rect;
use vale_labeler::{
    Caps, Feature, Fonts, Geometry, LabelClass, LabelInput, LinePlacement, LineSide, Obstacle,
    ObstacleKind, Options, Placement, PointPlacement, PointPosition, TextStyle, place_labels,
};
use vale_render::{Color, GlyphRun, Item};
use vale_store::GeometryKind;
use vale_style::expr::{Expr, Filter};
use vale_style::{
    LabelClass as StyleClass, LabelPlacement as StylePlacement, LabelPosition, TextCase,
};

use crate::pipeline::{LabelSummary, PageLayer, PlacedSymbol, Quality, UnplacedInfo};

const MAX_FEATURES: usize = 3000;

/// The most scatter symbols that labels avoid.
const MAX_SOFT: usize = 20_000;

/// The cost of a label over one scatter symbol.
const SCATTER_COST: f64 = 0.5;

/// Places labels for the visible layers.
///
/// `layers` runs from the bottom of the drawing to the top. `classes` holds the label classes of
/// each layer of `layers`. Each class whose filter matches a feature gives the feature a label.
/// Labels do not overlap `obstacles` or a point symbol. A label over a scatter symbol has a cost.
pub fn place(
    layers: &[PageLayer<'_>],
    classes: &[Vec<StyleClass>],
    page: Rect,
    fonts: &mut Fonts,
    quality: Quality,
    obstacles: &[Rect],
    symbols: &[PlacedSymbol],
) -> (Vec<Item>, LabelSummary) {
    let mut labeled = HashSet::new();
    let mut label_classes: Vec<LabelClass> = Vec::new();
    let mut class_colors: Vec<Color> = Vec::new();
    // The color and the width of the halo of each class, if it has one.
    let mut class_halos: Vec<Option<(Color, f64)>> = Vec::new();
    let mut features: Vec<Feature> = Vec::new();
    let mut layer_names: Vec<&str> = Vec::new();
    let mut next_id: u64 = 0;

    for (pl, layer_classes) in layers.iter().zip(classes) {
        let is_line = match pl.kind {
            GeometryKind::Point => false,
            GeometryKind::Line => true,
            GeometryKind::Polygon => continue,
        };
        for c in layer_classes {
            let Ok(text_expr) = Expr::parse(&c.text) else {
                continue;
            };
            let filter = Filter::new(&c.filter);
            let class = label_classes.len();
            label_classes.push(LabelClass {
                name: c.name.clone(),
                style: TextStyle {
                    weight: c.weight,
                    italic: c.italic,
                    letter_spacing: c.letter_spacing * c.size,
                    caps: match c.case {
                        TextCase::SmallCaps => Caps::Small,
                        TextCase::AllSmallCaps => Caps::AllSmall,
                        _ => Caps::Normal,
                    },
                    ..TextStyle::new(crate::fonts::family(c.font.as_deref(), fonts), c.size)
                },
                priority: c.priority,
                margin: c.buffer,
                placement: placement(c, pl.kind),
                duplicate_distance: c.remove_duplicates.then_some(c.duplicate_distance),
            });
            class_colors.push(fade(c.color, c.opacity));
            class_halos.push(
                c.halo
                    .then(|| (fade(c.halo_color, c.opacity), c.halo_width)),
            );

            for f in &pl.features {
                let Some(feature) = pl.layer.features.get(f.index) else {
                    continue;
                };
                let ctx = pl.context(f.index);
                if !filter.matches(&ctx) {
                    continue;
                }
                let Some(text) = text_expr.eval(&ctx).label() else {
                    continue;
                };
                let text = c.case.apply(&text);
                let priority = c
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
    }

    // Keep the most important features when there are too many. IDs stay as they are.
    if features.len() > MAX_FEATURES {
        let mut order: Vec<usize> = (0..features.len()).collect();
        order.sort_by(|&a, &b| {
            let (fa, fb) = (&features[a], &features[b]);
            let ka = (label_classes[fa.class].priority, fa.priority);
            let kb = (label_classes[fb.class].priority, fb.priority);
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
        classes: label_classes,
        features,
        obstacles: symbol_obstacles(obstacles, symbols, &labeled, page),
        options: Options {
            seed: 24301,
            improve: quality == Quality::Final,
        },
    };
    let labeling = place_labels(fonts, &input);

    let mut items = Vec::new();
    for label in &labeling.placed {
        if let Some((color, width)) = class_halos[label.class] {
            items.push(Item::Stroke {
                path: label.outlines(),
                color,
                width,
                round: true,
            });
        }
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

/// `color` with its alpha times `opacity`.
fn fade(color: Color, opacity: f64) -> Color {
    let a = (f64::from(color.a) * opacity.clamp(0.0, 1.0)).round() as u8;
    Color { a, ..color }
}

/// The labeler placement of a class. A placement that does not fit `kind`
/// gives the default placement of `kind`. A point class tries the positions
/// that are on, in their rank.
fn placement(c: &StyleClass, kind: GeometryKind) -> Placement {
    let p = if c.placement.fits(kind) {
        c.placement
    } else {
        StylePlacement::default_for(kind).unwrap_or(StylePlacement::Around)
    };
    let line = |sides| {
        Placement::Line(LinePlacement {
            sides,
            repeat_distance: c.repeat.then_some(c.repeat_distance),
            offset: c.offset,
            max_glyph_turn: c.curve_limit,
            smoothing: c.smoothing,
        })
    };
    match p {
        StylePlacement::Around => Placement::Point(PointPlacement {
            positions: (c.positions.iter())
                .filter(|r| r.on)
                .map(|r| point_position(r.position))
                .collect(),
            offset: c.offset,
        }),
        StylePlacement::AboveOrBelow => line(LinePlacement::default().sides),
        StylePlacement::LineAbove => line(vec![LineSide::Above]),
        StylePlacement::LineBelow => line(vec![LineSide::Below]),
        StylePlacement::OnLine => line(vec![LineSide::Centered]),
    }
}

fn point_position(p: LabelPosition) -> PointPosition {
    match p {
        LabelPosition::TopRight => PointPosition::TopRight,
        LabelPosition::Right => PointPosition::Right,
        LabelPosition::BottomRight => PointPosition::BottomRight,
        LabelPosition::Top => PointPosition::Top,
        LabelPosition::Bottom => PointPosition::Bottom,
        LabelPosition::TopLeft => PointPosition::TopLeft,
        LabelPosition::Left => PointPosition::Left,
        LabelPosition::BottomLeft => PointPosition::BottomLeft,
    }
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
