//! The label step of the map pipeline.

use std::collections::{HashMap, HashSet};

use kurbo::{Point, Rect};
use vale_labeler::{
    Caps, Feature, Fonts, Geometry, LabelClass, LabelInput, LeaderAnchor, LinePlacement, LineSide,
    Obstacle, ObstacleKind, Options, Placement, PointPlacement, PointPosition, PolygonMode,
    PolygonPlacement, PolygonZones, TextStyle, place_labels,
};
use vale_render::{Color, FillRule, GlyphRun, Item};
use vale_store::GeometryKind;
use vale_style::expr::{Expr, Filter};
use vale_style::{
    LabelClass as StyleClass, LabelPlacement as StylePlacement, LabelPosition, LeaderEnd,
    LeaderStyle, TextCase,
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
/// `parallels` are the lines of constant latitude on the page, each from west to east.
#[allow(clippy::too_many_arguments)]
pub fn place(
    layers: &[PageLayer<'_>],
    classes: &[Vec<StyleClass>],
    page: Rect,
    fonts: &mut Fonts,
    quality: Quality,
    obstacles: &[Rect],
    symbols: &[PlacedSymbol],
    parallels: Vec<Vec<Point>>,
) -> (Vec<Item>, LabelSummary) {
    let mut labeled = HashSet::new();
    let mut label_classes: Vec<LabelClass> = Vec::new();
    let mut class_colors: Vec<Color> = Vec::new();
    // The color and the width of the halo of each class, if it has one.
    let mut class_halos: Vec<Option<(Color, f64)>> = Vec::new();
    // The style of the leader line of each class, with its line color and its halo color.
    let mut class_leaders: Vec<(LeaderStyle, Color, Color)> = Vec::new();
    let mut features: Vec<Feature> = Vec::new();
    let mut layer_names: Vec<&str> = Vec::new();
    let mut next_id: u64 = 0;

    for (pl, layer_classes) in layers.iter().zip(classes) {
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
            class_colors.push(c.color);
            let leader = c.leader;
            class_leaders.push((
                leader,
                leader.color.unwrap_or(c.color),
                leader.halo_color.unwrap_or(c.halo_color),
            ));
            class_halos.push(c.halo.then_some((c.halo_color, c.halo_width)));

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
                match pl.kind {
                    GeometryKind::Line => {
                        for line in f.lines.iter().filter(|l| l.len() >= 2) {
                            push(Geometry::Line {
                                path: line.clone(),
                                stroke_width,
                            });
                        }
                    }
                    GeometryKind::Point => {
                        for p in f.points.iter().filter(|p| page.contains(**p)) {
                            push(Geometry::Point {
                                position: *p,
                                symbol_radius,
                            });
                        }
                    }
                    GeometryKind::Polygon => {
                        let rings: Vec<_> =
                            (f.rings.iter()).filter(|r| r.len() >= 3).cloned().collect();
                        if !rings.is_empty() {
                            push(Geometry::Polygon { rings });
                        }
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
        parallels,
        options: Options {
            seed: 24301,
            improve: quality == Quality::Final,
        },
    };
    let labeling = place_labels(fonts, &input);

    let mut items = Vec::new();
    for label in &labeling.placed {
        if let Some(leader) = &label.leader {
            let (style, color, halo) = &class_leaders[label.class];
            items.extend(leader_items(
                (leader.anchor, leader.end),
                style,
                *color,
                *halo,
            ));
        }
    }
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

/// The items of a leader line from `anchor` to `end` at the label: the halo, the line, and
/// the mark at the anchor end, in `color`. A hidden line, or a line that its gaps use up, gives
/// no items.
pub fn leader_items(
    (anchor, end): (Point, Point),
    style: &LeaderStyle,
    color: Color,
    halo: Color,
) -> Vec<Item> {
    let mut items = Vec::new();
    let along = end - anchor;
    let length = along.hypot();
    let (anchor_gap, label_gap) = (style.anchor_gap.max(0.0), style.label_gap.max(0.0));
    if !style.show || !length.is_finite() || length <= anchor_gap + label_gap {
        return items;
    }
    let u = along / length;
    let start = anchor + u * anchor_gap;
    let stop = end - u * label_gap;
    let size = style.end_size.max(0.0);
    let mark = match style.end {
        LeaderEnd::None => None,
        LeaderEnd::Dot => Some(kurbo::Shape::to_path(
            &kurbo::Circle::new(start, size / 2.0),
            0.05,
        )),
        LeaderEnd::Arrow => {
            let base = start + u * size;
            let side = kurbo::Vec2::new(-u.y, u.x) * (0.4 * size);
            let mut path = kurbo::BezPath::new();
            path.move_to(start);
            path.line_to(base + side);
            path.line_to(base - side);
            path.close_path();
            Some(path)
        }
    };
    // The line stops at the base of an arrow.
    let from = match style.end {
        LeaderEnd::Arrow => start + u * size.min((stop - start).hypot()),
        _ => start,
    };
    let parts = if (stop - from).hypot() <= 1e-9 {
        Vec::new()
    } else if style.dashed {
        vale_render::stroke::dash(&[vec![from, stop]], style.dash, style.dash_gap)
    } else {
        vec![vec![from, stop]]
    };
    let mut line = kurbo::BezPath::new();
    for part in parts.iter().filter(|p| p.len() >= 2) {
        line.move_to(part[0]);
        for p in &part[1..] {
            line.line_to(*p);
        }
    }
    let width = style.width.max(0.0);
    if style.halo {
        let mut path = line.clone();
        if let Some(mark) = &mark {
            path.extend(mark.iter());
        }
        items.push(Item::Stroke {
            path,
            color: halo,
            width: width + 2.0 * style.halo_width.max(0.0),
            round: true,
        });
    }
    if !line.elements().is_empty() && width > 0.0 {
        items.push(Item::Stroke {
            path: line,
            color,
            width,
            round: true,
        });
    }
    if let Some(path) = mark {
        items.push(Item::Fill {
            path,
            color,
            rule: FillRule::NonZero,
        });
    }
    items
}

/// The labeler placement of a class. A placement that does not fit `kind`
/// gives the default placement of `kind`. A point class tries the positions
/// that are on, in their rank.
pub fn placement(c: &StyleClass, kind: GeometryKind) -> Placement {
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
            graticule: c.graticule,
            no_flip: c.no_flip,
        }),
        StylePlacement::AboveOrBelow => line(LinePlacement::default().sides),
        StylePlacement::LineAbove => line(vec![LineSide::Above]),
        StylePlacement::LineBelow => line(vec![LineSide::Below]),
        StylePlacement::OnLine => line(vec![LineSide::Centered]),
        StylePlacement::BestFit
        | StylePlacement::Curved
        | StylePlacement::Horizontal
        | StylePlacement::Parallel => Placement::Polygon(PolygonPlacement {
            mode: match p {
                StylePlacement::Curved => PolygonMode::Curved,
                StylePlacement::Horizontal => PolygonMode::Horizontal,
                StylePlacement::Parallel => PolygonMode::Parallel,
                _ => PolygonMode::Auto,
            },
            max_spread: c.spread,
            max_glyph_turn: c.curve_limit,
            graticule: c.graticule,
            no_flip: c.no_flip,
            zones: PolygonZones {
                inside: c.zones_inside,
                outside: c.zones_outside,
            },
            anchor: match c.anchor {
                vale_style::LeaderAnchor::Center => LeaderAnchor::Center,
                vale_style::LeaderAnchor::Inside => LeaderAnchor::Inside,
                vale_style::LeaderAnchor::Outline => LeaderAnchor::Outline,
            },
            over_holes: c.over_holes,
            avoid_other_polygons: c.avoid_polygons,
            largest_part_only: c.largest_part,
            repeat_distance: c.polygon_repeat.then_some(c.polygon_repeat_distance),
            overrun: c.overrun,
            min_area: c.min_area,
        }),
    }
}

/// The labeler position of a position of a class.
pub fn point_position(p: LabelPosition) -> PointPosition {
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
