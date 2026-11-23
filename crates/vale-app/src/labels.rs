//! The label step of the map pipeline.

use std::collections::{HashMap, HashSet};

use i_overlay::core::fill_rule::FillRule as OverlayFill;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use kurbo::{BezPath, Cap, Join, PathEl, Point, Rect, StrokeOpts};
use vale_labeler::{
    AmbiguousLabels, Caps, Fallback, Fallbacks, Feature, Fonts, Geometry, LabelClass, LabelInput,
    LeaderAnchor, LineAnchor, LineOrientation, LinePlacement, LineSide, Obstacle, Options,
    Placement, PointPlacement, PointPosition, PolygonMode, PolygonPlacement, PolygonZones,
    TextStyle, UnplacedReason, Weight, place_labels,
};
use vale_render::{Color, FillRule, GlyphRun, Item};
use vale_store::{GeometryKind, LayerId};
use vale_style::expr::{Expr, Filter};
use vale_style::{
    LabelClass as StyleClass, LabelFallback, LabelPlacement as StylePlacement, LabelPosition,
    LeaderEnd, LeaderStyle, Paint, StackAlign, TextCase, Weights,
};

use crate::atlas::FrameTarget;
use crate::pipeline::{
    FrameMasks, LabelSummary, PageFeature, PageLayer, PlacedSymbol, Quality, UnplacedInfo,
};

const MAX_FEATURES: usize = 3000;

/// The most obstacles from the features of the layers with no label class.
const MAX_OBSTACLES: usize = 3000;

/// The most scatter symbols that labels avoid.
const MAX_SOFT: usize = 20_000;

/// The cost of a label over one scatter symbol.
const SCATTER_COST: f64 = 0.5;

/// Keeps the part of each polygon of a raster-masked layer inside its page
/// mask, if the label of the polygon stays inside the mask. A layer whose
/// page mask is not ready keeps none of those polygons.
pub fn clip_to_masks(layers: &mut [PageLayer<'_>], masks: &FrameMasks, target: FrameTarget) {
    for pl in layers.iter_mut() {
        if pl.kind != GeometryKind::Polygon {
            continue;
        }
        let Some(mask) = masks.get(&pl.layer.id) else {
            continue;
        };
        let style = &pl.entry.style.style;
        let inside: Vec<bool> = (pl.features.iter())
            .map(|f| style.label_in_mask(&pl.context(f.index)))
            .collect();
        for (f, _) in pl.features.iter_mut().zip(inside).filter(|(_, i)| *i) {
            f.rings = match mask {
                Some(mask) => mask.clip(&f.rings, f.bbox, target),
                None => Vec::new(),
            };
        }
    }
}

/// The labeler fallbacks of class `c`: the fallbacks that are on, in order.
pub fn fallbacks(c: &StyleClass) -> Fallbacks {
    Fallbacks {
        order: (c.fallbacks.iter())
            .filter(|r| r.on)
            .map(|r| match r.fallback {
                LabelFallback::SmallerFont => Fallback::SmallerFont,
                LabelFallback::Compress => Fallback::Compress,
                LabelFallback::Stack => Fallback::Stack,
                LabelFallback::Abbreviate => Fallback::Abbreviate,
                LabelFallback::Truncate => Fallback::Truncate,
                LabelFallback::Leader => Fallback::Leader,
            })
            .collect(),
        min_size: c.min_size,
        size_step: c.size_step,
        min_width: c.min_width / 100.0,
        width_step: c.width_step / 100.0,
        max_lines: c.max_lines as usize,
        stack_separators: c.stack_separators.clone(),
        min_line_chars: c.min_line_chars as usize,
        max_line_chars: c.max_line_chars as usize,
        stack_align: match c.stack_align {
            StackAlign::Left => vale_labeler::StackAlign::Left,
            StackAlign::Center => vale_labeler::StackAlign::Center,
            StackAlign::Right => vale_labeler::StackAlign::Right,
        },
        abbreviations: c.abbreviations.clone(),
        truncation_marker: c.truncation_marker.clone(),
        leader_distance: c.leader_distance,
    }
}

/// The labeler style of the text of class `c`.
pub fn text_style(c: &StyleClass, fonts: &mut Fonts) -> TextStyle {
    TextStyle {
        weight: c.weight,
        italic: c.italic,
        letter_spacing: c.letter_spacing * c.size,
        caps: match c.case {
            TextCase::SmallCaps => Caps::Small,
            TextCase::AllSmallCaps => Caps::AllSmall,
            _ => Caps::Normal,
        },
        ..TextStyle::new(crate::fonts::family(c.font.as_deref(), fonts), c.size)
    }
}

/// The mask of one placed label: the outline of its glyphs, the width of the band around them,
/// and the layers that the mask cuts.
pub struct LabelMask {
    pub outline: BezPath,
    pub width: f64,
    pub layers: Vec<LayerId>,
}

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
) -> (Vec<Item>, LabelSummary, Vec<LabelMask>) {
    let mut labeled = HashSet::new();
    let mut label_classes: Vec<LabelClass> = Vec::new();
    let mut class_colors: Vec<Color> = Vec::new();
    // The color and the width of the halo of each class, if it has one.
    let mut class_halos: Vec<Option<(Color, f64)>> = Vec::new();
    // The mask width and the masked layers of each class that masks layers.
    let mut class_masks: Vec<Option<(f64, &[LayerId])>> = Vec::new();
    // The style of the leader line of each class, with its line color and its halo color.
    let mut class_leaders: Vec<(LeaderStyle, Color, Color)> = Vec::new();
    let mut features: Vec<Feature> = Vec::new();
    let mut layer_names: Vec<&str> = Vec::new();
    let mut next_id: u64 = 0;
    // True for each layer of `layers` with a label class.
    let mut has_class: Vec<bool> = Vec::with_capacity(layers.len());

    for (pl, layer_classes) in layers.iter().zip(classes) {
        // The class of each feature of `pl.features` that gets no text: the first class whose
        // filter matches it, or else the first class of the layer.
        let mut empty: Vec<Option<usize>> = vec![None; pl.features.len()];
        // The style class of each labeler class of the layer.
        let mut styles: Vec<(usize, &StyleClass)> = Vec::new();
        for c in layer_classes {
            let Ok(text_expr) = Expr::parse(&c.text) else {
                continue;
            };
            let filter = Filter::new(&c.filter);
            let class = label_classes.len();
            styles.push((class, c));
            label_classes.push(LabelClass {
                name: c.name.clone(),
                style: text_style(c, fonts),
                priority: c.priority,
                margin: c.buffer,
                placement: placement(c, pl.kind, &pl.entry.style.weights),
                duplicate_distance: c.remove_duplicates.then_some(c.duplicate_distance),
                margin_hard: c.buffer_hard,
                background: c.background,
                never_remove: c.never_remove,
                fallbacks: fallbacks(c),
            });
            class_colors.push(c.color);
            let leader = c.leader;
            class_leaders.push((
                leader,
                leader.color.unwrap_or(c.color),
                leader.halo_color.unwrap_or(c.halo_color),
            ));
            class_halos.push(c.halo.then_some((c.halo_color, c.halo_width)));
            class_masks.push(
                (c.mask && !c.masked_layers.is_empty())
                    .then_some((c.mask_width.max(0.0), c.masked_layers.as_slice())),
            );

            for (k, f) in pl.features.iter().enumerate() {
                let Some(feature) = pl.layer.features.get(f.index) else {
                    continue;
                };
                let ctx = pl.context(f.index);
                if !filter.matches(&ctx) {
                    continue;
                }
                let text = (text_expr.eval(&ctx).label())
                    .map(|t| c.case.apply(&t))
                    .unwrap_or_default();
                if text.is_empty() {
                    empty[k].get_or_insert(class);
                    continue;
                }
                labeled.insert((pl.layer.id, f.index));
                let priority = c
                    .priority_field
                    .as_ref()
                    .and_then(|p| feature.attributes.get(p))
                    .and_then(|v| v.as_f64())
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.0);
                for geometry in geometries(pl, f, c, page) {
                    features.push(Feature {
                        id: next_id,
                        class,
                        text: text.clone(),
                        priority,
                        geometry,
                    });
                    layer_names.push(&pl.layer.name);
                    next_id += 1;
                }
            }
        }
        has_class.push(!styles.is_empty());
        // A feature with no text is an obstacle with the weights of its layer.
        let Some(&first) = styles.first() else {
            continue;
        };
        for (f, class) in pl.features.iter().zip(empty) {
            if labeled.contains(&(pl.layer.id, f.index)) {
                continue;
            }
            let (class, c) = (class.and_then(|k| styles.iter().find(|(i, _)| *i == k)))
                .copied()
                .unwrap_or(first);
            labeled.insert((pl.layer.id, f.index));
            for geometry in geometries(pl, f, c, page) {
                features.push(Feature {
                    id: next_id,
                    class,
                    text: String::new(),
                    priority: 0.0,
                    geometry,
                });
                layer_names.push(&pl.layer.name);
                next_id += 1;
            }
        }
    }

    // A layer with no label class is an obstacle with its weights. The top layers come first.
    let mut layer_obstacles: Vec<Obstacle> = Vec::new();
    let mut symbol_weights: HashMap<vale_store::LayerId, Weight> = HashMap::new();
    for (pl, _) in (layers.iter().zip(&has_class)).rev().filter(|(_, c)| !**c) {
        if pl.kind == GeometryKind::Point {
            symbol_weights.insert(pl.layer.id, weight(pl.entry.style.weights.point));
        }
        obstacles_of(pl, page, &mut labeled, &mut layer_obstacles);
    }

    // Keep the most important features when there are too many. IDs stay as they are.
    if features.len() > MAX_FEATURES {
        let mut order: Vec<usize> = (0..features.len()).collect();
        order.sort_by(|&a, &b| {
            let (fa, fb) = (&features[a], &features[b]);
            // Features with text come first.
            let ka = (
                !fa.text.is_empty(),
                label_classes[fa.class].priority,
                fa.priority,
            );
            let kb = (
                !fb.text.is_empty(),
                label_classes[fb.class].priority,
                fb.priority,
            );
            (kb.0.cmp(&ka.0))
                .then(kb.1.cmp(&ka.1))
                .then(kb.2.total_cmp(&ka.2))
                .then(a.cmp(&b))
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
        obstacles: {
            let mut all = symbol_obstacles(obstacles, symbols, &labeled, &symbol_weights, page);
            all.extend(layer_obstacles);
            all
        },
        parallels,
        options: Options {
            seed: 24301,
            improve: quality == Quality::Final,
        },
    };
    let labeling = place_labels(fonts, &input);

    let masks = (labeling.placed.iter())
        .filter_map(|label| {
            let (width, layers) = class_masks[label.class]?;
            Some(LabelMask {
                outline: label.outlines(),
                width,
                layers: layers.to_vec(),
            })
        })
        .collect();
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
            .filter(|u| u.reason != UnplacedReason::EmptyText)
            .map(|u| UnplacedInfo {
                layer: names.get(&u.feature).copied().unwrap_or("").to_string(),
                text: u.text.clone(),
                reason: format!("{:?}", u.reason),
            })
            .collect(),
    };
    (items, summary, masks)
}

/// The clip of each layer that `masks` cut: `outer`, with a hole for the glyphs of each label
/// and a band of the mask width around them. The clip uses the even-odd rule. `tolerance` is
/// the flattening tolerance in target points.
pub fn mask_clips(masks: &[LabelMask], outer: Rect, tolerance: f64) -> HashMap<LayerId, BezPath> {
    let mut by_layer: HashMap<LayerId, Vec<usize>> = HashMap::new();
    for (i, m) in masks.iter().enumerate() {
        for layer in &m.layers {
            let labels = by_layer.entry(*layer).or_default();
            if labels.last() != Some(&i) {
                labels.push(i);
            }
        }
    }
    let mut contours: Vec<Option<Vec<Vec<[f64; 2]>>>> = (0..masks.len()).map(|_| None).collect();
    let mut clips: HashMap<Vec<usize>, BezPath> = HashMap::new();
    by_layer
        .into_iter()
        .map(|(layer, labels)| {
            let clip = clips.entry(labels.clone()).or_insert_with(|| {
                let mut subject = Vec::new();
                for &i in &labels {
                    let c = contours[i].get_or_insert_with(|| mask_contours(&masks[i], tolerance));
                    subject.extend(c.iter().cloned());
                }
                let empty: Vec<Vec<[f64; 2]>> = Vec::new();
                let shapes = subject.overlay(&empty, OverlayRule::Union, OverlayFill::NonZero);
                let mut path = BezPath::new();
                path.move_to((outer.x0, outer.y0));
                path.line_to((outer.x1, outer.y0));
                path.line_to((outer.x1, outer.y1));
                path.line_to((outer.x0, outer.y1));
                path.close_path();
                for ring in shapes.iter().flatten() {
                    for (k, &[x, y]) in ring.iter().enumerate() {
                        match k {
                            0 => path.move_to((x, y)),
                            _ => path.line_to((x, y)),
                        }
                    }
                    path.close_path();
                }
                path
            });
            (layer, clip.clone())
        })
        .collect()
}

/// The flat contours of the mask of one label: its glyphs and the band around their outline.
/// The contours overlap, so they need the nonzero rule.
fn mask_contours(mask: &LabelMask, tolerance: f64) -> Vec<Vec<[f64; 2]>> {
    let mut contours = Vec::new();
    flat_contours(&mask.outline, tolerance, &mut contours);
    if mask.width > 0.0 {
        let style = kurbo::Stroke::new(2.0 * mask.width)
            .with_join(Join::Round)
            .with_caps(Cap::Round);
        let band = kurbo::stroke(&mask.outline, &style, &StrokeOpts::default(), tolerance);
        flat_contours(&band, tolerance, &mut contours);
    }
    contours
}

/// Adds the closed, flat contours of `path` to `out`.
fn flat_contours(path: &BezPath, tolerance: f64, out: &mut Vec<Vec<[f64; 2]>>) {
    let mut ring: Vec<[f64; 2]> = Vec::new();
    let end = |ring: &mut Vec<[f64; 2]>, out: &mut Vec<Vec<[f64; 2]>>| {
        if ring.len() > 2 {
            out.push(std::mem::take(ring));
        } else {
            ring.clear();
        }
    };
    kurbo::flatten(path, tolerance, |el| match el {
        PathEl::MoveTo(p) => {
            end(&mut ring, out);
            ring.push([p.x, p.y]);
        }
        PathEl::LineTo(p) => ring.push([p.x, p.y]),
        PathEl::ClosePath => end(&mut ring, out),
        _ => {}
    });
    end(&mut ring, out);
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
        vale_render::stroke::dash(&[vec![from, stop]], &[style.dash, style.dash_gap])
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

/// The labeler placement of a class of a layer with `weights`. A placement
/// that does not fit `kind` gives the default placement of `kind`. A point
/// class tries the positions that are on, in their rank.
pub fn placement(c: &StyleClass, kind: GeometryKind, weights: &Weights) -> Placement {
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
            orientation: match c.orientation {
                vale_style::LineOrientation::Curved => LineOrientation::Curved,
                vale_style::LineOrientation::Straight => LineOrientation::Straight,
                vale_style::LineOrientation::Horizontal => LineOrientation::Horizontal,
                vale_style::LineOrientation::Perpendicular => LineOrientation::Perpendicular,
            },
            anchor: match c.line_anchor {
                vale_style::LineAnchor::Best => LineAnchor::Best,
                vale_style::LineAnchor::FromStart => LineAnchor::FromStart,
                vale_style::LineAnchor::FromEnd => LineAnchor::FromEnd,
                vale_style::LineAnchor::BeforeStart => LineAnchor::BeforeStart,
                vale_style::LineAnchor::AfterEnd => LineAnchor::AfterEnd,
            },
            anchor_distance: c.anchor_distance,
            anchor_tolerance: c.anchor_tolerance,
            anchor_percent: c.anchor_percent,
            by_direction: c.by_direction,
            follow_direction: c.follow_direction,
            overrun: c.overrun,
            merge_connected: c.merge_lines,
            max_spread: c.line_spread,
            spread_fill: c.spread_fill,
            min_length: c.min_length,
            weight: weight(weights.line),
        })
    };
    match p {
        StylePlacement::Around | StylePlacement::Center => Placement::Point(PointPlacement {
            positions: if p == StylePlacement::Center {
                vec![PointPosition::Center]
            } else {
                (c.positions.iter())
                    .filter(|r| r.on)
                    .map(|r| point_position(r.position))
                    .collect()
            },
            offset: c.offset,
            max_offset: (c.max_offset > c.offset).then_some(c.max_offset),
            center_shift: c.center_shift,
            graticule: c.graticule,
            no_flip: c.no_flip,
            ambiguous: match c.ambiguous {
                vale_style::AmbiguousLabels::Keep => AmbiguousLabels::Keep,
                vale_style::AmbiguousLabels::AllClasses => AmbiguousLabels::AllClasses,
                vale_style::AmbiguousLabels::OwnClass => AmbiguousLabels::OwnClass,
            },
            weight: weight(weights.point),
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
            inside_weight: weight(weights.fill),
            outline_weight: weight(weights.outline),
        }),
    }
}

/// The outline on the page of the symbol of a point at `p`, from the paints
/// of the feature: the largest marker, or the square of the largest symbol.
fn symbol_outline(paints: &[Paint], p: Point) -> Option<Vec<Point>> {
    let mut best: Option<(f64, Vec<Point>)> = None;
    for paint in paints {
        let (reach, ring) = match paint {
            Paint::Marker {
                radius,
                shape,
                stroke,
                ..
            } => {
                let r = radius + stroke.as_ref().map_or(0.0, |s| s.width);
                let ring = (shape.unit_outline().into_iter())
                    .map(|(x, y)| p + kurbo::Vec2::new(x * r, y * r))
                    .collect();
                (radius * shape.reach(), ring)
            }
            Paint::Symbol { size, rotation, .. } => {
                let h = size / 2.0;
                // The rotation is in degrees clockwise, and the page y axis points down.
                let (sin, cos) = rotation.to_radians().sin_cos();
                let ring = [(-h, -h), (h, -h), (h, h), (-h, h)]
                    .into_iter()
                    .map(|(x, y)| p + kurbo::Vec2::new(x * cos - y * sin, x * sin + y * cos))
                    .collect();
                (h, ring)
            }
            _ => continue,
        };
        if best.as_ref().is_none_or(|(r, _)| reach > *r) {
            best = Some((reach, ring));
        }
    }
    best.map(|(_, ring)| ring)
}

/// The labeler geometries of feature `f` of `pl` for class `c`, on `page`.
fn geometries(pl: &PageLayer<'_>, f: &PageFeature, c: &StyleClass, page: Rect) -> Vec<Geometry> {
    let ctx = pl.context(f.index);
    let extent = pl.extent(f.index);
    let stroke_width = extent.stroke_width;
    match pl.kind {
        GeometryKind::Line => (f.lines.iter())
            .filter(|l| l.len() >= 2)
            .map(|line| Geometry::Line {
                path: line.clone(),
                stroke_width,
            })
            .collect(),
        GeometryKind::Point => {
            let symbol_radius = if extent.marker_radius > 0.0 {
                extent.marker_radius + stroke_width
            } else {
                0.0
            };
            let angle = (c.angle_field.as_ref())
                .and_then(|a| pl.layer.features.get(f.index)?.attributes.get(a))
                .and_then(|v| v.as_f64())
                .filter(|v| v.is_finite())
                .map(f64::to_radians);
            let mut shape = Vec::new();
            if c.offset_from_outline {
                pl.entry.style.style.paints(&ctx, &mut shape);
            }
            (f.points.iter())
                .filter(|p| page.contains(**p))
                .map(|p| Geometry::Point {
                    position: *p,
                    symbol_radius,
                    angle,
                    outline: symbol_outline(&shape, *p),
                })
                .collect()
        }
        GeometryKind::Polygon => {
            // The part inside a polygon mask, if the label stays inside it.
            let inside = f.whole.is_none() || pl.entry.style.style.label_in_mask(&ctx);
            let rings: Vec<_> = (f.rings_for(inside).iter())
                .filter(|r| r.len() >= 3)
                .cloned()
                .collect();
            if rings.is_empty() {
                Vec::new()
            } else {
                vec![Geometry::Polygon { rings }]
            }
        }
    }
}

/// Adds to `out` the obstacles of the features of `pl`, a layer with no label
/// class, with the weights of the layer, up to `MAX_OBSTACLES`. A point with a
/// marker is a disc and goes in `labeled`, so its symbol is not an obstacle again.
fn obstacles_of(
    pl: &PageLayer<'_>,
    page: Rect,
    labeled: &mut HashSet<(vale_store::LayerId, usize)>,
    out: &mut Vec<Obstacle>,
) {
    let w = pl.entry.style.weights;
    if w.free(pl.kind) {
        return;
    }
    for f in &pl.features {
        if out.len() >= MAX_OBSTACLES {
            return;
        }
        let extent = pl.extent(f.index);
        match pl.kind {
            GeometryKind::Point => {
                if extent.marker_radius <= 0.0 {
                    continue;
                }
                let radius = extent.marker_radius + extent.stroke_width;
                labeled.insert((pl.layer.id, f.index));
                out.extend(f.points.iter().filter(|p| page.contains(**p)).map(|p| {
                    Obstacle::Point {
                        position: *p,
                        radius,
                        weight: weight(w.point),
                    }
                }));
            }
            GeometryKind::Line => {
                out.extend(
                    f.lines
                        .iter()
                        .filter(|l| l.len() >= 2)
                        .map(|l| Obstacle::Line {
                            path: l.clone(),
                            width: extent.stroke_width,
                            weight: weight(w.line),
                        }),
                );
            }
            GeometryKind::Polygon => {
                let rings: Vec<_> = (f.rings.iter()).filter(|r| r.len() >= 3).cloned().collect();
                if !rings.is_empty() {
                    out.push(Obstacle::Polygon {
                        rings,
                        inside: weight(w.fill),
                        outline: weight(w.outline),
                    });
                }
            }
        }
    }
    out.truncate(MAX_OBSTACLES);
}

/// The labeler weight of a weight of a layer, from 0, free, to `FORBIDDEN_WEIGHT`.
pub fn weight(w: f64) -> Weight {
    if w >= vale_style::FORBIDDEN_WEIGHT {
        Weight::Forbidden
    } else {
        Weight::Cost(w.max(0.0) / 10.0)
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
/// scatter symbols. A feature that goes to the labeler, with text or with
/// none, has its symbol as its geometry, so its symbol is not an obstacle.
/// The symbol of a layer in `weights` has that weight, and the symbol of
/// another layer is forbidden.
fn symbol_obstacles(
    hard: &[Rect],
    symbols: &[PlacedSymbol],
    labeled: &HashSet<(vale_store::LayerId, usize)>,
    weights: &HashMap<vale_store::LayerId, Weight>,
    page: Rect,
) -> Vec<Obstacle> {
    let mut out: Vec<Obstacle> = hard
        .iter()
        .map(|r| Obstacle::rect(*r, Weight::Forbidden))
        .collect();
    let meets = |r: Rect| r.x0 <= page.x1 && r.x1 >= page.x0 && r.y0 <= page.y1 && r.y1 >= page.y0;
    let shown = symbols.iter().filter(|s| meets(s.bounds));
    let (scatter, points): (Vec<_>, Vec<_>) = shown.partition(|s| s.lonlat.is_some());
    out.extend(
        points
            .into_iter()
            .filter(|s| !labeled.contains(&(s.layer, s.feature)))
            .map(|s| {
                (
                    s,
                    weights.get(&s.layer).copied().unwrap_or(Weight::Forbidden),
                )
            })
            .filter(|(_, w)| *w != Weight::FREE)
            .map(|(s, w)| Obstacle::rect(s.bounds, w)),
    );
    out.extend(
        scatter
            .into_iter()
            .take(MAX_SOFT)
            .map(|s| Obstacle::rect(s.bounds, Weight::Cost(SCATTER_COST))),
    );
    out
}
