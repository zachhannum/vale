//! Rule-based styles.

use crate::expr::{Context, Filter, Property};
use crate::{Color, Stroke};

/// The length of one point, in meters.
pub const METERS_PER_POINT: f64 = 0.0254 / 72.0;

/// The scale denominator of a view that shows `ground_meters_per_point` of ground in each point.
pub fn scale_denominator(ground_meters_per_point: f64) -> f64 {
    ground_meters_per_point / METERS_PER_POINT
}

/// An ordered list of rules. Each rule that matches a feature draws it, in list order, so a later rule draws on top.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub rules: Vec<Rule>,
}

/// A filter, a scale range, and the symbolizers that draw each feature that matches.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub name: String,
    pub filter: Filter,
    /// The smallest scale denominator at which the rule draws. `None` has no limit.
    pub min_scale: Option<f64>,
    /// The scale denominator from which the rule no longer draws. `None` has no limit.
    pub max_scale: Option<f64>,
    pub symbolizers: Vec<Symbolizer>,
}

/// A stroke with properties that can be expressions. The width is in points.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeSymbolizer {
    pub color: Property<Color>,
    pub width: Property<f64>,
}

/// An instruction that draws a feature.
#[derive(Clone, Debug, PartialEq)]
pub enum Symbolizer {
    /// Fills polygons.
    Fill { color: Property<Color> },
    /// Strokes lines and polygon outlines.
    Stroke(StrokeSymbolizer),
    /// A circle at each point. The radius is in points, and the stroke is outside the fill.
    Marker {
        radius: Property<f64>,
        fill: Option<Property<Color>>,
        stroke: Option<StrokeSymbolizer>,
    },
}

/// A symbolizer with the values for one feature.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Paint {
    Fill(Color),
    Stroke(Stroke),
    Marker {
        radius: f64,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
}

/// How far the paint of a feature reaches out from its geometry, in points.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Extent {
    /// The widest stroke.
    pub stroke_width: f64,
    /// The largest marker radius.
    pub marker_radius: f64,
}

impl StrokeSymbolizer {
    pub fn constant(stroke: Stroke) -> Self {
        StrokeSymbolizer {
            color: Property::constant(stroke.color),
            width: Property::constant(stroke.width),
        }
    }

    pub fn resolve(&self, ctx: &Context) -> Stroke {
        Stroke {
            color: self.color.eval(ctx),
            width: self.width.eval(ctx),
        }
    }

    /// The stroke from the fallbacks.
    pub fn fallback(&self) -> Stroke {
        Stroke {
            color: self.color.fallback,
            width: self.width.fallback,
        }
    }

    fn is_constant(&self) -> bool {
        self.color.is_constant() && self.width.is_constant()
    }
}

impl Symbolizer {
    pub fn resolve(&self, ctx: &Context) -> Paint {
        match self {
            Symbolizer::Fill { color } => Paint::Fill(color.eval(ctx)),
            Symbolizer::Stroke(s) => Paint::Stroke(s.resolve(ctx)),
            Symbolizer::Marker {
                radius,
                fill,
                stroke,
            } => Paint::Marker {
                radius: radius.eval(ctx),
                fill: fill.as_ref().map(|c| c.eval(ctx)),
                stroke: stroke.as_ref().map(|s| s.resolve(ctx)),
            },
        }
    }

    /// True if no property is an expression.
    pub fn is_constant(&self) -> bool {
        match self {
            Symbolizer::Fill { color } => color.is_constant(),
            Symbolizer::Stroke(s) => s.is_constant(),
            Symbolizer::Marker {
                radius,
                fill,
                stroke,
            } => {
                radius.is_constant()
                    && fill.as_ref().is_none_or(Property::is_constant)
                    && stroke.as_ref().is_none_or(StrokeSymbolizer::is_constant)
            }
        }
    }

    /// The paint from the fallbacks.
    fn fallback(&self) -> Paint {
        match self {
            Symbolizer::Fill { color } => Paint::Fill(color.fallback),
            Symbolizer::Stroke(s) => Paint::Stroke(s.fallback()),
            Symbolizer::Marker {
                radius,
                fill,
                stroke,
            } => Paint::Marker {
                radius: radius.fallback,
                fill: fill.as_ref().map(|c| c.fallback),
                stroke: stroke.as_ref().map(StrokeSymbolizer::fallback),
            },
        }
    }
}

impl Rule {
    /// A rule with no filter and no scale range.
    pub fn new(name: impl Into<String>, symbolizers: Vec<Symbolizer>) -> Self {
        Rule {
            name: name.into(),
            filter: Filter::all(),
            min_scale: None,
            max_scale: None,
            symbolizers,
        }
    }

    /// True if the rule draws at the scale denominator `scale`.
    pub fn in_scale(&self, scale: f64) -> bool {
        self.min_scale.is_none_or(|min| scale >= min)
            && self.max_scale.is_none_or(|max| scale < max)
    }

    /// True if the rule draws the feature of `ctx`. This includes the scale range.
    pub fn matches(&self, ctx: &Context) -> bool {
        self.in_scale(ctx.scale) && self.filter.matches(ctx)
    }
}

impl Style {
    /// The rules that draw at the scale denominator `scale`.
    pub fn visible(&self, scale: f64) -> impl Iterator<Item = &Rule> {
        self.rules.iter().filter(move |r| r.in_scale(scale))
    }

    /// Appends the paints of the feature of `ctx` to `out`, from bottom to top.
    pub fn paints(&self, ctx: &Context, out: &mut Vec<Paint>) {
        for rule in self.rules.iter().filter(|r| r.matches(ctx)) {
            out.extend(rule.symbolizers.iter().map(|s| s.resolve(ctx)));
        }
    }

    /// The paints at the scale denominator `scale` when they are the same for each feature: no visible rule has a filter or an expression.
    pub fn constant_paints(&self, scale: f64) -> Option<Vec<Paint>> {
        let mut out = Vec::new();
        for rule in self.visible(scale) {
            if !rule.filter.is_all() || !rule.symbolizers.iter().all(Symbolizer::is_constant) {
                return None;
            }
            out.extend(rule.symbolizers.iter().map(Symbolizer::fallback));
        }
        Some(out)
    }

    /// How far the paints of the feature of `ctx` reach.
    pub fn extent(&self, ctx: &Context) -> Extent {
        let mut paints = Vec::new();
        self.paints(ctx, &mut paints);
        extent_of(&paints)
    }
}

/// How far `paints` reach.
pub fn extent_of(paints: &[Paint]) -> Extent {
    let mut e = Extent::default();
    for p in paints {
        match p {
            Paint::Fill(_) => {}
            Paint::Stroke(s) => e.stroke_width = e.stroke_width.max(s.width),
            Paint::Marker { radius, stroke, .. } => {
                e.marker_radius = e.marker_radius.max(*radius);
                if let Some(s) = stroke {
                    e.stroke_width = e.stroke_width.max(s.width);
                }
            }
        }
    }
    e
}
