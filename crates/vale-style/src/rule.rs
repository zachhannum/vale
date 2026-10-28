//! Rule-based styles.

use std::sync::Arc;

use vale_store::{BlendMode, RasterDisplay, Rgb, Stop};

use crate::expr::{Context, Filter, Property};
use crate::{Color, Stroke, StrokeKind};

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
    pub kind: StrokeKind,
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
    /// A symbol from the library at each point.
    Symbol(PointSymbolizer),
    /// Symbols spread over the inside of each polygon.
    Scatter(ScatterSymbolizer),
    /// The colors of a raster layer.
    Raster(RasterSymbolizer),
}

/// A symbol at each point. The size is the longer side of the symbol in points. The rotation is in degrees clockwise from page up, about the center. A tint replaces the colors of the symbol.
#[derive(Clone, Debug, PartialEq)]
pub struct PointSymbolizer {
    /// The name of the symbol in the library.
    pub symbol: String,
    pub size: Property<f64>,
    pub rotation: Property<f64>,
    pub tint: Option<Property<Color>>,
}

/// Symbols spread over the inside of a polygon. The size is the longer side of a symbol in points, and the spacing is the smallest distance between the centers of two symbols in points.
#[derive(Clone, Debug, PartialEq)]
pub struct ScatterSymbolizer {
    /// The name of the symbol in the library.
    pub symbol: String,
    pub size: Property<f64>,
    pub spacing: Property<f64>,
    /// From 0 to 0.5. A symbol becomes smaller by up to this fraction of the size.
    pub size_jitter: f64,
    /// From 0 to 180. A symbol turns by up to this angle in degrees, in each direction.
    pub rotation_jitter: f64,
    pub tint: Option<Property<Color>>,
}

impl PointSymbolizer {
    /// A symbol with a constant size, no rotation, and no tint.
    pub fn constant(symbol: &str, size: f64) -> Self {
        PointSymbolizer {
            symbol: symbol.to_string(),
            size: Property::constant(size),
            rotation: Property::constant(0.0),
            tint: None,
        }
    }

    fn is_constant(&self) -> bool {
        self.size.is_constant()
            && self.rotation.is_constant()
            && self.tint.as_ref().is_none_or(Property::is_constant)
    }
}

impl ScatterSymbolizer {
    pub const SIZE_JITTER: f64 = 0.2;

    /// A scatter with constant values, no rotation jitter, and no tint.
    pub fn new(symbol: &str, size: f64, spacing: f64) -> Self {
        ScatterSymbolizer {
            symbol: symbol.to_string(),
            size: Property::constant(size),
            spacing: Property::constant(spacing),
            size_jitter: Self::SIZE_JITTER,
            rotation_jitter: 0.0,
            tint: None,
        }
    }

    fn is_constant(&self) -> bool {
        self.size.is_constant()
            && self.spacing.is_constant()
            && self.tint.as_ref().is_none_or(Property::is_constant)
    }

    /// The paint for a size, a spacing, and a tint. The spacing is at least the size, and the jitters are in their ranges.
    fn paint(&self, size: f64, spacing: f64, tint: Option<Color>) -> Paint {
        Paint::Scatter {
            symbol: Arc::from(self.symbol.as_str()),
            size,
            spacing: spacing.max(size),
            size_jitter: self.size_jitter.clamp(0.0, 0.5),
            rotation_jitter: self.rotation_jitter.clamp(0.0, 180.0),
            tint,
        }
    }
}

/// The colors of a raster layer. A raster layer draws with the first raster symbolizer of its style. The filter and the scale range of its rule do not apply.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterSymbolizer {
    pub display: RasterDisplay,
    /// From 0 to 1.
    pub opacity: f32,
    /// How the layer mixes with the layers below it.
    pub blend: BlendMode,
    /// The stops at the edges of the bands, in ascending order of value, with the two end stops.
    pub stops: Vec<Stop>,
    /// The color of each band, from the lowest band up. There is one color fewer than stops.
    pub colors: Vec<Rgb>,
}

/// A symbolizer with the values for one feature.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Fill(Color),
    Stroke(Stroke),
    Marker {
        radius: f64,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    Symbol {
        symbol: Arc<str>,
        size: f64,
        rotation: f64,
        tint: Option<Color>,
    },
    Scatter {
        symbol: Arc<str>,
        size: f64,
        spacing: f64,
        size_jitter: f64,
        rotation_jitter: f64,
        tint: Option<Color>,
    },
}

/// How far the paint of a feature reaches out from its geometry, in points.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Extent {
    /// The widest stroke.
    pub stroke_width: f64,
    /// The largest marker radius, or half the size of the largest symbol.
    pub marker_radius: f64,
}

impl StrokeSymbolizer {
    pub fn constant(stroke: Stroke) -> Self {
        StrokeSymbolizer {
            color: Property::constant(stroke.color),
            width: Property::constant(stroke.width),
            kind: stroke.kind,
        }
    }

    pub fn resolve(&self, ctx: &Context) -> Stroke {
        Stroke {
            color: self.color.eval(ctx),
            width: self.width.eval(ctx),
            kind: self.kind,
        }
    }

    /// The stroke from the fallbacks.
    pub fn fallback(&self) -> Stroke {
        Stroke {
            color: self.color.fallback,
            width: self.width.fallback,
            kind: self.kind,
        }
    }

    fn is_constant(&self) -> bool {
        self.color.is_constant() && self.width.is_constant()
    }
}

impl Symbolizer {
    /// The paint for the feature of `ctx`. A raster symbolizer has none.
    pub fn resolve(&self, ctx: &Context) -> Option<Paint> {
        Some(match self {
            Symbolizer::Fill { color } => Paint::Fill(color.eval(ctx)),
            Symbolizer::Stroke(s) => Paint::Stroke(s.resolve(ctx)),
            Symbolizer::Raster(_) => return None,
            Symbolizer::Marker {
                radius,
                fill,
                stroke,
            } => Paint::Marker {
                radius: radius.eval(ctx),
                fill: fill.as_ref().map(|c| c.eval(ctx)),
                stroke: stroke.as_ref().map(|s| s.resolve(ctx)),
            },
            Symbolizer::Symbol(s) => Paint::Symbol {
                symbol: Arc::from(s.symbol.as_str()),
                size: s.size.eval(ctx),
                rotation: s.rotation.eval(ctx),
                tint: s.tint.as_ref().map(|c| c.eval(ctx)),
            },
            Symbolizer::Scatter(s) => s.paint(
                s.size.eval(ctx),
                s.spacing.eval(ctx),
                s.tint.as_ref().map(|c| c.eval(ctx)),
            ),
        })
    }

    /// True if no property is an expression.
    pub fn is_constant(&self) -> bool {
        match self {
            Symbolizer::Fill { color } => color.is_constant(),
            Symbolizer::Stroke(s) => s.is_constant(),
            Symbolizer::Raster(_) => true,
            Symbolizer::Marker {
                radius,
                fill,
                stroke,
            } => {
                radius.is_constant()
                    && fill.as_ref().is_none_or(Property::is_constant)
                    && stroke.as_ref().is_none_or(StrokeSymbolizer::is_constant)
            }
            Symbolizer::Symbol(s) => s.is_constant(),
            Symbolizer::Scatter(s) => s.is_constant(),
        }
    }

    /// The paint from the fallbacks.
    fn fallback(&self) -> Option<Paint> {
        Some(match self {
            Symbolizer::Fill { color } => Paint::Fill(color.fallback),
            Symbolizer::Stroke(s) => Paint::Stroke(s.fallback()),
            Symbolizer::Raster(_) => return None,
            Symbolizer::Marker {
                radius,
                fill,
                stroke,
            } => Paint::Marker {
                radius: radius.fallback,
                fill: fill.as_ref().map(|c| c.fallback),
                stroke: stroke.as_ref().map(StrokeSymbolizer::fallback),
            },
            Symbolizer::Symbol(s) => Paint::Symbol {
                symbol: Arc::from(s.symbol.as_str()),
                size: s.size.fallback,
                rotation: s.rotation.fallback,
                tint: s.tint.as_ref().map(|c| c.fallback),
            },
            Symbolizer::Scatter(s) => s.paint(
                s.size.fallback,
                s.spacing.fallback,
                s.tint.as_ref().map(|c| c.fallback),
            ),
        })
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

    /// The fill of the rule, or the fill of its marker.
    pub fn fill(&self) -> Option<Color> {
        self.symbolizers
            .iter()
            .find_map(|s| match s {
                Symbolizer::Fill { color } => Some(Some(color.fallback)),
                Symbolizer::Marker { fill, .. } => Some(fill.as_ref().map(|c| c.fallback)),
                _ => None,
            })
            .flatten()
    }

    /// Sets the fill of the rule, or the fill of its marker. `None` removes the fill.
    pub fn set_fill(&mut self, color: Option<Color>) {
        if let Some(Symbolizer::Marker { fill, .. }) = self
            .symbolizers
            .iter_mut()
            .find(|s| matches!(s, Symbolizer::Marker { .. }))
        {
            *fill = color.map(Property::constant);
            return;
        }
        let at = self
            .symbolizers
            .iter()
            .position(|s| matches!(s, Symbolizer::Fill { .. }));
        let fill = |c| Symbolizer::Fill {
            color: Property::constant(c),
        };
        match (at, color) {
            (Some(i), Some(c)) => self.symbolizers[i] = fill(c),
            (Some(i), None) => {
                self.symbolizers.remove(i);
            }
            (None, Some(c)) => self.symbolizers.insert(0, fill(c)),
            (None, None) => {}
        }
    }

    /// The stroke of the rule, or the stroke of its marker.
    pub fn stroke(&self) -> Option<Stroke> {
        self.symbolizers
            .iter()
            .find_map(|s| match s {
                Symbolizer::Stroke(s) => Some(Some(s.fallback())),
                Symbolizer::Marker { stroke, .. } => {
                    Some(stroke.as_ref().map(StrokeSymbolizer::fallback))
                }
                _ => None,
            })
            .flatten()
    }

    /// Sets the stroke of the rule, or the stroke of its marker. `None` removes the stroke.
    pub fn set_stroke(&mut self, value: Option<Stroke>) {
        if let Some(Symbolizer::Marker { stroke, .. }) = self
            .symbolizers
            .iter_mut()
            .find(|s| matches!(s, Symbolizer::Marker { .. }))
        {
            *stroke = value.map(StrokeSymbolizer::constant);
            return;
        }
        let at = self
            .symbolizers
            .iter()
            .position(|s| matches!(s, Symbolizer::Stroke(_)));
        match (at, value) {
            (Some(i), Some(v)) => {
                self.symbolizers[i] = Symbolizer::Stroke(StrokeSymbolizer::constant(v))
            }
            (Some(i), None) => {
                self.symbolizers.remove(i);
            }
            (None, Some(v)) => self
                .symbolizers
                .push(Symbolizer::Stroke(StrokeSymbolizer::constant(v))),
            (None, None) => {}
        }
    }

    /// The first symbol of the rule.
    pub fn symbol(&self) -> Option<&PointSymbolizer> {
        self.symbolizers.iter().find_map(|s| match s {
            Symbolizer::Symbol(p) => Some(p),
            _ => None,
        })
    }

    /// Sets the first symbol of the rule. A new symbol takes the place of the first marker, or else goes at the end. `None` removes the symbol.
    pub fn set_symbol(&mut self, value: Option<PointSymbolizer>) {
        let at = self
            .symbolizers
            .iter()
            .position(|s| matches!(s, Symbolizer::Symbol(_)));
        match (at, value) {
            (Some(i), Some(v)) => self.symbolizers[i] = Symbolizer::Symbol(v),
            (Some(i), None) => {
                self.symbolizers.remove(i);
            }
            (None, Some(v)) => {
                let marker = self
                    .symbolizers
                    .iter()
                    .position(|s| matches!(s, Symbolizer::Marker { .. }));
                match marker {
                    Some(i) => self.symbolizers[i] = Symbolizer::Symbol(v),
                    None => self.symbolizers.push(Symbolizer::Symbol(v)),
                }
            }
            (None, None) => {}
        }
    }

    /// The first scatter of the rule.
    pub fn scatter(&self) -> Option<&ScatterSymbolizer> {
        self.symbolizers.iter().find_map(|s| match s {
            Symbolizer::Scatter(p) => Some(p),
            _ => None,
        })
    }

    /// Sets the first scatter of the rule. A new scatter goes after the fill and before the first stroke. `None` removes the scatter.
    pub fn set_scatter(&mut self, value: Option<ScatterSymbolizer>) {
        let at = self
            .symbolizers
            .iter()
            .position(|s| matches!(s, Symbolizer::Scatter(_)));
        match (at, value) {
            (Some(i), Some(v)) => self.symbolizers[i] = Symbolizer::Scatter(v),
            (Some(i), None) => {
                self.symbolizers.remove(i);
            }
            (None, Some(v)) => {
                let after_fill = self
                    .symbolizers
                    .iter()
                    .position(|s| matches!(s, Symbolizer::Fill { .. }))
                    .map_or(0, |i| i + 1);
                let i = self.symbolizers[after_fill..]
                    .iter()
                    .position(|s| matches!(s, Symbolizer::Stroke(_)))
                    .map_or(self.symbolizers.len(), |i| after_fill + i);
                self.symbolizers.insert(i, Symbolizer::Scatter(v));
            }
            (None, None) => {}
        }
    }
}

impl Style {
    /// The first name "Rule N" that no rule has, from N = 1.
    pub fn unique_name(&self) -> String {
        (1..)
            .map(|n| format!("Rule {n}"))
            .find(|name| self.rules.iter().all(|r| &r.name != name))
            .expect("a free name")
    }

    /// Swaps rule `i` with the rule below it (`up` false) or above it (`up` true), and returns its new index. A rule at the end does not move.
    pub fn move_rule(&mut self, i: usize, up: bool) -> usize {
        let j = if up { i.checked_sub(1) } else { Some(i + 1) };
        match j {
            Some(j) if i < self.rules.len() && j < self.rules.len() => {
                self.rules.swap(i, j);
                j
            }
            _ => i,
        }
    }

    /// The rules that draw at the scale denominator `scale`.
    pub fn visible(&self, scale: f64) -> impl Iterator<Item = &Rule> {
        self.rules.iter().filter(move |r| r.in_scale(scale))
    }

    /// Appends the paints of the feature of `ctx` to `out`, from bottom to top.
    pub fn paints(&self, ctx: &Context, out: &mut Vec<Paint>) {
        for rule in self.rules.iter().filter(|r| r.matches(ctx)) {
            out.extend(rule.symbolizers.iter().filter_map(|s| s.resolve(ctx)));
        }
    }

    /// The paints at the scale denominator `scale` when they are the same for each feature: no visible rule has a filter or an expression.
    pub fn constant_paints(&self, scale: f64) -> Option<Vec<Paint>> {
        let mut out = Vec::new();
        for rule in self.visible(scale) {
            if !rule.filter.is_all() || !rule.symbolizers.iter().all(Symbolizer::is_constant) {
                return None;
            }
            out.extend(rule.symbolizers.iter().filter_map(Symbolizer::fallback));
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
            Paint::Symbol { size, .. } => e.marker_radius = e.marker_radius.max(size / 2.0),
            Paint::Scatter { .. } => {}
        }
    }
    e
}
