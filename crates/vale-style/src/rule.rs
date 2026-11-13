//! Rule-based styles.

use std::f64::consts::{SQRT_2, TAU};
use std::ops::RangeInclusive;
use std::sync::Arc;

use vale_store::{
    BlendMode, FillKind, FillPattern, Hillshade, LayerId, RasterDisplay, Rgb, Smoothing, Stop,
};

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
    /// The region that limits the fills and the strokes. `None` draws everywhere.
    pub mask: Option<Mask>,
}

/// A region of the world that limits a style.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Mask {
    /// The cells of a raster layer with a value in the range. The lower limit is in the range, and the upper limit is not. `None` has no limit.
    Raster {
        layer: LayerId,
        min: Option<f64>,
        max: Option<f64>,
        /// The distance in points that the region grows by. A negative offset shrinks it.
        offset: f32,
    },
    /// The inside of the polygons of a polygon layer.
    Polygons { layer: LayerId },
}

impl Mask {
    /// The land of the height layer `layer`: each value from 0 up.
    pub const fn land(layer: LayerId) -> Mask {
        Mask::Raster {
            layer,
            min: Some(0.0),
            max: None,
            offset: 0.0,
        }
    }

    /// The layer that gives the region.
    pub fn layer(&self) -> LayerId {
        match *self {
            Mask::Raster { layer, .. } | Mask::Polygons { layer } => layer,
        }
    }

    /// The value `v` of a raster mask is in the range. A polygon mask holds no value.
    pub fn contains(&self, v: f64) -> bool {
        match *self {
            Mask::Raster { min, max, .. } => {
                min.is_none_or(|m| v >= m) && max.is_none_or(|m| v < m)
            }
            Mask::Polygons { .. } => false,
        }
    }
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
    /// A shape at each point. The radius is in points, and the stroke is outside the fill. The hatch lines take the fill color, so a hatch with no fill draws nothing.
    Marker {
        radius: Property<f64>,
        shape: MarkerShape,
        fill: Option<Property<Color>>,
        hatch: Option<Hatch>,
        stroke: Option<StrokeSymbolizer>,
    },
    /// A symbol from the library at each point.
    Symbol(PointSymbolizer),
    /// Symbols spread over the inside of each polygon.
    Scatter(ScatterSymbolizer),
    /// The colors of a raster layer.
    Raster(RasterSymbolizer),
    /// The look of an image layer.
    Image(ImageSymbolizer),
}

/// The shape of a marker.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum MarkerShape {
    #[default]
    Circle,
    Square,
    Diamond,
    /// The apex points up.
    Triangle,
    /// Five points, and the first point is up.
    Star,
}

impl MarkerShape {
    pub const ALL: [MarkerShape; 5] = [
        MarkerShape::Circle,
        MarkerShape::Square,
        MarkerShape::Diamond,
        MarkerShape::Triangle,
        MarkerShape::Star,
    ];

    /// The name in the style JSON.
    pub fn id(self) -> &'static str {
        match self {
            MarkerShape::Circle => "circle",
            MarkerShape::Square => "square",
            MarkerShape::Diamond => "diamond",
            MarkerShape::Triangle => "triangle",
            MarkerShape::Star => "star",
        }
    }

    pub fn from_id(id: &str) -> Option<MarkerShape> {
        MarkerShape::ALL.into_iter().find(|s| s.id() == id)
    }

    /// The UI text.
    pub fn label(self) -> &'static str {
        match self {
            MarkerShape::Circle => "Circle",
            MarkerShape::Square => "Square",
            MarkerShape::Diamond => "Diamond",
            MarkerShape::Triangle => "Triangle",
            MarkerShape::Star => "Star",
        }
    }

    /// The outline for a radius of 1, inside the square from -1 to 1. The y axis points down.
    pub fn unit_outline(self) -> Vec<(f64, f64)> {
        // A point at distance `r` from the center, at `a` radians clockwise from up.
        let polar = |r: f64, a: f64| (r * a.sin(), -r * a.cos());
        match self {
            MarkerShape::Circle => (0..64)
                .map(|i| polar(1.0, TAU * f64::from(i) / 64.0))
                .collect(),
            MarkerShape::Square => vec![(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)],
            MarkerShape::Diamond => vec![(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)],
            MarkerShape::Triangle => vec![(0.0, -1.0), (1.0, 1.0), (-1.0, 1.0)],
            MarkerShape::Star => (0..10)
                .map(|i| {
                    let r = if i % 2 == 0 { 1.0 } else { 0.382 };
                    polar(r, TAU * f64::from(i) / 10.0)
                })
                .collect(),
        }
    }

    /// The distance from the center to the farthest vertex, for a radius of 1.
    pub fn reach(self) -> f64 {
        match self {
            MarkerShape::Square | MarkerShape::Triangle => SQRT_2,
            MarkerShape::Circle | MarkerShape::Diamond | MarkerShape::Star => 1.0,
        }
    }
}

/// The lines that fill a marker. The lengths are in points. The angle is in degrees, counterclockwise from the horizontal.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Hatch {
    /// A second set of lines at the angle plus 90°.
    pub cross: bool,
    pub angle: f64,
    /// The distance between two lines.
    pub spacing: f64,
    pub line_width: f64,
}

impl Hatch {
    /// The angles that the app offers.
    pub const ANGLES: RangeInclusive<f64> = FillPattern::ANGLES;
    /// The spacings that the app offers.
    pub const SPACINGS: RangeInclusive<f64> = 1.0..=6.0;
    /// The line widths that the app offers.
    pub const LINE_WIDTHS: RangeInclusive<f64> = FillPattern::LINE_WIDTHS;

    /// The fill kind of the hatch.
    pub fn kind(&self) -> FillKind {
        if self.cross {
            FillKind::CrossHatch
        } else {
            FillKind::Hatch
        }
    }
}

impl Default for Hatch {
    fn default() -> Self {
        Hatch {
            cross: false,
            angle: 45.0,
            spacing: 2.0,
            line_width: 0.5,
        }
    }
}

/// The look of an image layer. An image layer holds colors, not values, so it has no stops. An image layer draws with the first image symbolizer of its style. The filter and the scale range of its rule do not apply.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ImageSymbolizer {
    /// From 0 to 1.
    pub opacity: f32,
    /// How the layer mixes with the layers below it.
    pub blend: BlendMode,
}

impl Default for ImageSymbolizer {
    fn default() -> Self {
        ImageSymbolizer {
            opacity: 1.0,
            blend: BlendMode::default(),
        }
    }
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
    /// The name of each band, from the lowest band up, or none.
    pub names: Vec<String>,
    /// The layer draws the lines of the stops.
    pub lines: bool,
    pub smoothing: Smoothing,
    pub hillshade: Hillshade,
}

/// A symbolizer with the values for one feature.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Fill(Color),
    Stroke(Stroke),
    Marker {
        radius: f64,
        shape: MarkerShape,
        fill: Option<Color>,
        hatch: Option<Hatch>,
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
    pub fn rewrite(&mut self, f: &dyn Fn(&str) -> String) {
        self.color.rewrite(f);
        self.width.rewrite(f);
    }

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
    /// Rewrites the text of each expression with `f`.
    pub fn rewrite(&mut self, f: &dyn Fn(&str) -> String) {
        match self {
            Symbolizer::Fill { color } => color.rewrite(f),
            Symbolizer::Stroke(s) => s.rewrite(f),
            Symbolizer::Marker {
                radius,
                fill,
                stroke,
                ..
            } => {
                radius.rewrite(f);
                if let Some(fill) = fill {
                    fill.rewrite(f);
                }
                if let Some(s) = stroke {
                    s.rewrite(f);
                }
            }
            Symbolizer::Symbol(s) => {
                s.size.rewrite(f);
                s.rotation.rewrite(f);
                if let Some(t) = &mut s.tint {
                    t.rewrite(f);
                }
            }
            Symbolizer::Scatter(s) => {
                s.size.rewrite(f);
                s.spacing.rewrite(f);
                if let Some(t) = &mut s.tint {
                    t.rewrite(f);
                }
            }
            Symbolizer::Raster(_) | Symbolizer::Image(_) => {}
        }
    }

    /// The paint for the feature of `ctx`. A raster symbolizer and an image symbolizer have none.
    pub fn resolve(&self, ctx: &Context) -> Option<Paint> {
        Some(match self {
            Symbolizer::Fill { color } => Paint::Fill(color.eval(ctx)),
            Symbolizer::Stroke(s) => Paint::Stroke(s.resolve(ctx)),
            Symbolizer::Raster(_) | Symbolizer::Image(_) => return None,
            Symbolizer::Marker {
                radius,
                shape,
                fill,
                hatch,
                stroke,
            } => Paint::Marker {
                radius: radius.eval(ctx),
                shape: *shape,
                fill: fill.as_ref().map(|c| c.eval(ctx)),
                hatch: *hatch,
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
            Symbolizer::Raster(_) | Symbolizer::Image(_) => true,
            Symbolizer::Marker {
                radius,
                fill,
                stroke,
                ..
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
    pub fn fallback(&self) -> Option<Paint> {
        Some(match self {
            Symbolizer::Fill { color } => Paint::Fill(color.fallback),
            Symbolizer::Stroke(s) => Paint::Stroke(s.fallback()),
            Symbolizer::Raster(_) | Symbolizer::Image(_) => return None,
            Symbolizer::Marker {
                radius,
                shape,
                fill,
                hatch,
                stroke,
            } => Paint::Marker {
                radius: radius.fallback,
                shape: *shape,
                fill: fill.as_ref().map(|c| c.fallback),
                hatch: *hatch,
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

/// The shape, the fill, and the hatch of a marker.
type MarkerParts<'a> = (
    &'a mut MarkerShape,
    &'a mut Option<Property<Color>>,
    &'a mut Option<Hatch>,
);

/// The shape, the fill, and the hatch of a marker.
type MarkerView<'a> = (MarkerShape, Option<&'a Property<Color>>, Option<Hatch>);

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

    /// Rewrites the text of the filter and of each symbolizer expression with `f`.
    pub fn rewrite(&mut self, f: &dyn Fn(&str) -> String) {
        self.filter.rewrite(f);
        for s in &mut self.symbolizers {
            s.rewrite(f);
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

    fn marker_mut(&mut self) -> Option<MarkerParts<'_>> {
        self.symbolizers.iter_mut().find_map(|s| match s {
            Symbolizer::Marker {
                shape, fill, hatch, ..
            } => Some((shape, fill, hatch)),
            _ => None,
        })
    }

    fn marker(&self) -> Option<MarkerView<'_>> {
        self.symbolizers.iter().find_map(|s| match s {
            Symbolizer::Marker {
                shape, fill, hatch, ..
            } => Some((*shape, fill.as_ref(), *hatch)),
            _ => None,
        })
    }

    /// The shape of the first marker of the rule.
    pub fn marker_shape(&self) -> Option<MarkerShape> {
        self.marker().map(|(shape, _, _)| shape)
    }

    /// Sets the shape of the first marker. A rule with no marker does not change.
    pub fn set_marker_shape(&mut self, value: MarkerShape) {
        if let Some((shape, _, _)) = self.marker_mut() {
            *shape = value;
        }
    }

    /// The hatch of the first marker of the rule.
    pub fn marker_hatch(&self) -> Option<Hatch> {
        self.marker().and_then(|(_, _, hatch)| hatch)
    }

    /// Sets the hatch of the first marker. A rule with no marker does not change.
    pub fn set_marker_hatch(&mut self, value: Option<Hatch>) {
        if let Some((_, _, hatch)) = self.marker_mut() {
            *hatch = value;
        }
    }

    /// How the first marker of the rule is filled, or `None` with no marker.
    pub fn marker_fill_kind(&self) -> Option<FillKind> {
        let (_, fill, hatch) = self.marker()?;
        Some(match (fill, hatch) {
            (None, _) => FillKind::None,
            (Some(_), None) => FillKind::Solid,
            (Some(_), Some(h)) => h.kind(),
        })
    }

    /// Sets how the first marker is filled. A hatch keeps its values and changes only `cross`. A new hatch has the default values. A marker with no fill gets the first default point fill. A rule with no marker does not change.
    pub fn set_marker_fill_kind(&mut self, kind: FillKind) {
        let Some((_, fill, hatch)) = self.marker_mut() else {
            return;
        };
        if kind == FillKind::None {
            *fill = None;
            *hatch = None;
            return;
        }
        if fill.is_none() {
            *fill = Some(Property::constant(crate::default_point_fill()));
        }
        *hatch = kind.hatched().then(|| Hatch {
            cross: kind == FillKind::CrossHatch,
            ..hatch.unwrap_or_default()
        });
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
    /// Rewrites the text of each expression of each rule with `f`.
    pub fn rewrite(&mut self, f: &dyn Fn(&str) -> String) {
        for r in &mut self.rules {
            r.rewrite(f);
        }
    }

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
            Paint::Marker {
                radius,
                shape,
                stroke,
                ..
            } => {
                e.marker_radius = e.marker_radius.max(radius * shape.reach());
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
