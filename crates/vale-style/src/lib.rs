//! Layer and map styles.

pub mod expr;
mod json;
mod label;
mod overrides;
mod rule;

pub use json::{STYLE_VERSION, StyleError, style_version};
pub use label::{
    ABBREVIATIONS, AmbiguousLabels, BUFFER, CLASSES_VERSION, CURVE_LIMIT, DUPLICATE_DISTANCE,
    FallbackRank, LEADER_DISTANCE, LabelClass, LabelFallback, LabelPlacement, LabelPosition,
    LeaderAnchor, LeaderEnd, LeaderStyle, LineAnchor, LineOrientation, MAX_LINE_CHARS, MAX_LINES,
    MIN_LINE_CHARS, MIN_SIZE, MIN_WIDTH, POLYGON_CURVE_LIMIT, POLYGON_REPEAT_DISTANCE,
    PositionRank, REPEAT_DISTANCE, SIZE_STEP, SMOOTHING, SPREAD, STACK_SEPARATORS, StackAlign,
    TRUNCATION_MARKER, TextCase, WIDTH_STEP, ZONES_INSIDE, ZONES_OUTSIDE, classes_from_json,
    classes_to_json, curve_limit_for, default_abbreviations, offset_for,
};
pub use overrides::Overrides;
pub use rule::{
    Extent, Hatch, ImageSymbolizer, METERS_PER_POINT, MarkerShape, Mask, Paint, PointSymbolizer,
    RasterSymbolizer, Rule, ScatterSymbolizer, StrokeSymbolizer, Style, Symbolizer, extent_of,
    scale_denominator,
};
pub use vale_render::Color;
use vale_store::GeometryKind;

use expr::Property;

/// The dashes and gaps of a dashed stroke, in multiples of the stroke width. The lengths
/// alternate dash and gap, and they start with a dash. A dash of length 0 is a dot.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DashPattern {
    lengths: [f64; vale_render::stroke::MAX_PATTERN],
    len: u8,
    /// Round ends on the dashes. A dot needs round ends.
    pub round: bool,
}

/// A named dash pattern.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DashPreset {
    Dashed,
    Dotted,
    DashDot,
    DashDotDot,
}

impl DashPreset {
    pub const ALL: [DashPreset; 4] = [
        DashPreset::Dashed,
        DashPreset::Dotted,
        DashPreset::DashDot,
        DashPreset::DashDotDot,
    ];

    pub const fn pattern(self) -> DashPattern {
        match self {
            DashPreset::Dashed => DashPattern::DASHED,
            DashPreset::Dotted => DashPattern::DOTTED,
            DashPreset::DashDot => DashPattern::DASH_DOT,
            DashPreset::DashDotDot => DashPattern::DASH_DOT_DOT,
        }
    }
}

impl DashPattern {
    pub const DASHED: Self = Self::fixed(&[4.0, 2.0], false);
    pub const DOTTED: Self = Self::fixed(&[0.0, 2.0], true);
    pub const DASH_DOT: Self = Self::fixed(&[6.0, 2.5, 0.0, 2.5], true);
    pub const DASH_DOT_DOT: Self = Self::fixed(&[6.0, 2.5, 0.0, 2.5, 0.0, 2.5], true);

    const fn fixed(l: &[f64], round: bool) -> Self {
        let mut lengths = [0.0; vale_render::stroke::MAX_PATTERN];
        let mut i = 0;
        while i < l.len() {
            lengths[i] = l[i];
            i += 1;
        }
        DashPattern {
            lengths,
            len: l.len() as u8,
            round,
        }
    }

    /// A pattern of `lengths`, or `None` if they are not a valid pattern: an even count from
    /// 2 to 16, each dash finite and not negative, and each gap finite and positive.
    pub fn new(lengths: &[f64], round: bool) -> Option<Self> {
        if !vale_render::stroke::valid_pattern(lengths) {
            return None;
        }
        let mut a = [0.0; vale_render::stroke::MAX_PATTERN];
        a[..lengths.len()].copy_from_slice(lengths);
        Some(DashPattern {
            lengths: a,
            len: lengths.len() as u8,
            round,
        })
    }

    pub fn lengths(&self) -> &[f64] {
        &self.lengths[..self.len as usize]
    }

    /// The lengths times `k`, for example in points for a stroke of width `k`.
    pub fn scaled(&self, k: f64) -> Vec<f64> {
        self.lengths().iter().map(|l| l * k).collect()
    }

    /// The preset with the same lengths and ends, if any.
    pub fn preset(&self) -> Option<DashPreset> {
        DashPreset::ALL.into_iter().find(|p| p.pattern() == *self)
    }

    /// The pattern of `text`: lengths with spaces or commas between them. An odd count of
    /// lengths repeats one time, so that the pattern alternates dash and gap.
    pub fn parse(text: &str, round: bool) -> Option<Self> {
        let words = text.split(|c: char| c.is_whitespace() || c == ',');
        let mut lengths: Vec<f64> = (words.filter(|w| !w.is_empty()))
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        if lengths.len() % 2 == 1 {
            lengths.extend_from_within(..);
        }
        Self::new(&lengths, round)
    }

    /// The lengths as text, for example `6 2.5 0 2.5`.
    pub fn text(&self) -> String {
        let words: Vec<String> = (self.lengths().iter())
            .map(|l| {
                format!("{:.2}", l)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_owned()
            })
            .collect();
        words.join(" ")
    }

    /// The pattern with the lengths of `other` and the ends of this pattern.
    pub fn with_lengths(&self, other: &DashPattern) -> Self {
        DashPattern {
            round: self.round,
            ..*other
        }
    }
}

/// How a stroke draws its line.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub enum StrokeKind {
    #[default]
    Solid,
    /// A repeating pattern of dashes and gaps.
    Dashed(DashPattern),
    /// A textured brush line.
    Brush,
    /// The width grows along the line. `start` is the fraction of the width at the first vertex. The last vertex has the full width.
    Taper { start: f64 },
    /// The width follows the area that drains through each place of the line, from the first height layer. `min` is the fraction of the width for a line with no flow. The largest river of the layer has the full width.
    Flow { min: f64 },
}

impl StrokeKind {
    pub const TAPER_START: f64 = 0.25;
    pub const FLOW_MIN: f64 = 0.25;

    /// A dashed kind with the default pattern.
    pub const fn dashed() -> Self {
        StrokeKind::Dashed(DashPattern::DASHED)
    }

    /// A taper with the default start.
    pub const fn taper() -> Self {
        StrokeKind::Taper {
            start: Self::TAPER_START,
        }
    }

    /// A flow with the default smallest width.
    pub const fn flow() -> Self {
        StrokeKind::Flow {
            min: Self::FLOW_MIN,
        }
    }

    /// True for a kind whose width changes along the line.
    pub fn along_line(&self) -> bool {
        matches!(self, StrokeKind::Taper { .. } | StrokeKind::Flow { .. })
    }
}

/// A stroke. The width is in points. A marker ignores the kind.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
    pub kind: StrokeKind,
}

impl Stroke {
    pub const fn solid(color: Color, width: f64) -> Self {
        Stroke {
            color,
            width,
            kind: StrokeKind::Solid,
        }
    }

    /// The width of a Flow stroke where `area` drains. `(floor, largest)`: the area of the smallest river, and the area of the largest river of the layer. The width grows with the square root of the area above `floor`, from the smallest width at `floor` to the full width at `largest`. Another kind has its width.
    pub fn width_at(&self, area: f64, (floor, largest): (f64, f64)) -> f64 {
        let StrokeKind::Flow { min } = self.kind else {
            return self.width;
        };
        let t = if largest > floor {
            ((area - floor) / (largest - floor)).clamp(0.0, 1.0).sqrt()
        } else {
            0.0
        };
        self.width * (min + (1.0 - min) * t)
    }
}

/// How a layer is labeled.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelStyle {
    pub enabled: bool,
    /// The text expression. Empty text gives no labels.
    pub text: String,
    /// A numeric attribute. Higher is placed first.
    pub priority_field: Option<String>,
    /// Points.
    pub size: f64,
    pub color: Color,
    pub italic: bool,
    /// A font family. `None` is the default font.
    pub font: Option<String>,
}

/// The style of one layer: its rules and its labels.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerStyle {
    pub style: Style,
    pub label: LabelStyle,
    /// The name of the height layer that the Flow strokes read. `None`, or a name that no height layer has, reads the first height layer.
    pub flow_height: Option<String>,
    /// How much the labels of the map avoid the features of the layer.
    pub weights: Weights,
}

/// The weight that forbids labels over a feature. A weight is from 0, free, to this.
pub const FORBIDDEN_WEIGHT: f64 = 100.0;

/// How much labels avoid the features of a layer in a map. Each weight is from 0, free, to
/// `FORBIDDEN_WEIGHT`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weights {
    /// The point symbols.
    pub point: f64,
    /// The lines.
    pub line: f64,
    /// The inside of the polygons.
    pub fill: f64,
    /// The outlines of the polygons.
    pub outline: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            point: FORBIDDEN_WEIGHT,
            line: 30.0,
            fill: 0.0,
            outline: 0.0,
        }
    }
}

impl Weights {
    /// True if labels cross the features of a layer of `kind` at no cost.
    pub fn free(&self, kind: GeometryKind) -> bool {
        match kind {
            GeometryKind::Point => self.point <= 0.0,
            GeometryKind::Line => self.line <= 0.0,
            GeometryKind::Polygon => self.fill <= 0.0 && self.outline <= 0.0,
        }
    }
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

/// The first default fill of a point layer.
fn default_point_fill() -> Color {
    pick(&POINT_FILLS, 0)
}

fn fill(color: Color) -> Symbolizer {
    Symbolizer::Fill {
        color: Property::constant(color),
        apply_mask: true,
    }
}

fn stroke(color: Color, width: f64) -> Symbolizer {
    Symbolizer::Stroke(StrokeSymbolizer::constant(Stroke::solid(color, width)))
}

impl LayerStyle {
    /// Replaces the field `old` with `new` in the rules, the label text, and the priority field.
    pub fn rename_field(&mut self, old: &str, new: &str) {
        self.style.rewrite(&|t| expr::rename_field(t, old, new));
        self.label.text = expr::rename_field(&self.label.text, old, new);
        if self.label.priority_field.as_deref() == Some(old) {
            self.label.priority_field = Some(new.to_owned());
        }
    }

    /// Replaces the raster layer `old` with `new` in the rules and the label text.
    pub fn rename_raster(&mut self, old: &str, new: &str) {
        self.style.rewrite(&|t| expr::rename_raster(t, old, new));
        self.label.text = expr::rename_raster(&self.label.text, old, new);
        if self.flow_height.as_deref() == Some(old) {
            self.flow_height = Some(new.to_owned());
        }
    }

    /// The style of a new layer: one rule with constant symbolizers. `ordinal` picks a color and wraps around.
    pub fn default_for(kind: GeometryKind, ordinal: usize, fields: &[String]) -> Self {
        let field = guess_label_field(fields);
        let on = field.is_some();
        let text = field.as_deref().map(expr::field_ref).unwrap_or_default();
        let (symbolizers, label) = match kind {
            GeometryKind::Polygon => (
                vec![
                    fill(pick(&POLYGON_FILLS, ordinal)),
                    stroke(Color::rgb(150, 160, 165), 0.6),
                ],
                LabelStyle {
                    enabled: false,
                    text: text.clone(),
                    priority_field: None,
                    size: 11.0,
                    color: Color::rgb(40, 40, 40),
                    italic: false,
                    font: None,
                },
            ),
            GeometryKind::Line => (
                vec![stroke(pick(&LINE_STROKES, ordinal), 1.2)],
                LabelStyle {
                    enabled: on,
                    text: text.clone(),
                    priority_field: None,
                    size: 11.0,
                    color: Color::rgb(40, 100, 160),
                    italic: true,
                    font: None,
                },
            ),
            GeometryKind::Point => (
                vec![Symbolizer::Marker {
                    radius: Property::constant(2.5),
                    shape: MarkerShape::Circle,
                    fill: Some(Property::constant(pick(&POINT_FILLS, ordinal))),
                    hatch: None,
                    stroke: Some(StrokeSymbolizer::constant(Stroke::solid(
                        Color::rgb(255, 255, 255),
                        1.0,
                    ))),
                }],
                LabelStyle {
                    enabled: on,
                    text,
                    priority_field: None,
                    size: 11.0,
                    color: Color::rgb(30, 30, 30),
                    italic: false,
                    font: None,
                },
            ),
        };
        LayerStyle {
            style: Style {
                rules: vec![Rule::new("Default", symbolizers)],
                mask: None,
            },
            label,
            flow_height: None,
            weights: Weights::default(),
        }
    }

    /// The style of a raster layer: one rule with `colors`.
    pub fn raster(colors: RasterSymbolizer) -> Self {
        LayerStyle::grid("Colors", Symbolizer::Raster(colors))
    }

    /// One rule named `name` with `symbolizer`, and no labels.
    fn grid(name: &str, symbolizer: Symbolizer) -> Self {
        LayerStyle {
            style: Style {
                rules: vec![Rule::new(name, vec![symbolizer])],
                mask: None,
            },
            label: LabelStyle {
                enabled: false,
                text: String::new(),
                priority_field: None,
                size: 11.0,
                color: Color::rgb(40, 40, 40),
                italic: false,
                font: None,
            },
            flow_height: None,
            weights: Weights::default(),
        }
    }

    /// The first raster symbolizer, in rule order.
    pub fn raster_symbolizer(&self) -> Option<&RasterSymbolizer> {
        self.style
            .rules
            .iter()
            .flat_map(|r| &r.symbolizers)
            .find_map(|s| match s {
                Symbolizer::Raster(r) => Some(r),
                _ => None,
            })
    }

    /// The style of an image layer: one rule with the default image symbolizer.
    pub fn image() -> Self {
        LayerStyle::grid("Image", Symbolizer::Image(ImageSymbolizer::default()))
    }

    /// The first image symbolizer, in rule order.
    pub fn image_symbolizer(&self) -> Option<&ImageSymbolizer> {
        self.style
            .rules
            .iter()
            .flat_map(|r| &r.symbolizers)
            .find_map(|s| match s {
                Symbolizer::Image(i) => Some(i),
                _ => None,
            })
    }

    /// Replaces the first raster symbolizer, or adds a rule with `colors` if the style has none.
    pub fn set_raster_symbolizer(&mut self, colors: RasterSymbolizer) {
        let first = self
            .style
            .rules
            .iter_mut()
            .flat_map(|r| &mut r.symbolizers)
            .find_map(|s| match s {
                Symbolizer::Raster(r) => Some(r),
                _ => None,
            });
        match first {
            Some(r) => *r = colors,
            None => self
                .style
                .rules
                .push(Rule::new("Colors", vec![Symbolizer::Raster(colors)])),
        }
    }

    /// True if the style is one rule with no filter, no scale range, and no expression. The simple controls edit only such a style.
    pub fn is_simple(&self) -> bool {
        match self.style.rules.as_slice() {
            [] => true,
            [r] => {
                r.filter.is_all()
                    && r.min_scale.is_none()
                    && r.max_scale.is_none()
                    && r.symbolizers.iter().all(Symbolizer::is_constant)
            }
            _ => false,
        }
    }

    fn first_rule(&self) -> Option<&Rule> {
        self.style.rules.first()
    }

    fn first_rule_mut(&mut self) -> &mut Rule {
        if self.style.rules.is_empty() {
            self.style.rules.push(Rule::new("Default", Vec::new()));
        }
        &mut self.style.rules[0]
    }

    /// The fill of the first rule, or the fill of its marker.
    pub fn fill(&self) -> Option<Color> {
        self.first_rule()?.fill()
    }

    /// Sets the fill of the first rule, or the fill of its marker.
    pub fn set_fill(&mut self, color: Option<Color>) {
        self.first_rule_mut().set_fill(color);
    }

    /// The stroke of the first rule, or the stroke of its marker.
    pub fn stroke(&self) -> Option<Stroke> {
        self.first_rule()?.stroke()
    }

    /// Sets the stroke of the first rule, or the stroke of its marker.
    pub fn set_stroke(&mut self, value: Option<Stroke>) {
        self.first_rule_mut().set_stroke(value);
    }

    /// Appends a rule with a unique name, no filter, and no scale range, and returns its index. It copies the symbolizers of the last rule, or of the default style of `kind`. The rule gets the next default color of `kind`, so that it shows on the map: the stroke color of a line layer, and the fill color of a polygon layer or a point layer. A point symbol with a tint gets the color as its tint.
    pub fn add_rule(&mut self, kind: GeometryKind) -> usize {
        let n = self.style.rules.len();
        // The color of a rule that tells it apart: the stroke of a line layer, or else the fill.
        let color = |r: &Rule| match kind {
            GeometryKind::Line => r.stroke().map(|s| s.color),
            GeometryKind::Polygon | GeometryKind::Point => r.fill().or_else(|| symbol_tint(r)),
        };
        let used: Vec<Color> = self.style.rules.iter().filter_map(color).collect();
        let defaults =
            (n..n + 6).map(|o| LayerStyle::default_for(kind, o, &[]).style.rules[0].clone());
        let mut colors = None;
        for d in defaults {
            let free = color(&d).is_some_and(|c| !used.contains(&c));
            if free || colors.is_none() {
                colors = Some(d);
            }
            if free {
                break;
            }
        }
        let colors = colors.expect("six defaults");
        let mut rule = match self.style.rules.last() {
            Some(r) => Rule::new(self.style.unique_name(), r.symbolizers.clone()),
            None => Rule::new(self.style.unique_name(), colors.symbolizers.clone()),
        };
        match kind {
            GeometryKind::Line => {
                if let (Some(mut s), Some(c)) = (rule.stroke(), colors.stroke()) {
                    s.color = c.color;
                    rule.set_stroke(Some(s));
                }
            }
            GeometryKind::Polygon | GeometryKind::Point => {
                if rule.fill().is_some() {
                    rule.set_fill(colors.fill());
                } else if let (Some(s), Some(c)) = (rule.symbol(), colors.fill())
                    && s.tint.is_some()
                {
                    let mut s = s.clone();
                    s.tint = Some(Property::constant(c));
                    rule.set_symbol(Some(s));
                }
            }
        }
        self.style.rules.push(rule);
        n
    }

    /// The marker radius of the first rule, or 0 with no marker.
    pub fn point_radius(&self) -> f64 {
        self.first_rule()
            .and_then(|r| {
                r.symbolizers.iter().find_map(|s| match s {
                    Symbolizer::Marker { radius, .. } => Some(radius.fallback),
                    _ => None,
                })
            })
            .unwrap_or(0.0)
    }

    /// Sets the marker radius of the first rule. A rule with no marker does not change.
    pub fn set_point_radius(&mut self, value: f64) {
        for s in &mut self.first_rule_mut().symbolizers {
            if let Symbolizer::Marker { radius, .. } = s {
                *radius = Property::constant(value);
                return;
            }
        }
    }
}

fn symbol_tint(r: &Rule) -> Option<Color> {
    r.symbol()?.tint.as_ref().map(|t| t.fallback)
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
            outline: Stroke::solid(Color::rgb(90, 100, 105), 0.8),
            graticule: Stroke::solid(Color::rgba(120, 140, 150, 110), 0.5),
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
        assert_eq!(on.label.text, "name");
        let off = LayerStyle::default_for(GeometryKind::Point, 0, &s(&["pop"]));
        assert!(!off.label.enabled);
    }

    #[test]
    fn polygon_and_line_defaults() {
        let p = LayerStyle::default_for(GeometryKind::Polygon, 0, &s(&["name"]));
        assert!(!p.label.enabled);
        assert_eq!(p.label.text, "name");
        let l = LayerStyle::default_for(GeometryKind::Line, 0, &s(&["name"]));
        assert!(l.label.enabled);
        assert!(l.label.italic);
        assert!(l.fill().is_none());
        assert!(l.is_simple());
    }

    #[test]
    fn move_rule_swaps_with_a_neighbor() {
        let mut st = Style {
            rules: ["a", "b", "c"].map(|n| Rule::new(n, Vec::new())).to_vec(),
            mask: None,
        };
        assert_eq!(st.move_rule(0, true), 0);
        assert_eq!(st.move_rule(2, false), 2);
        assert_eq!(st.move_rule(2, true), 1);
        assert_eq!(st.move_rule(0, false), 1);
        let names: Vec<_> = st.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["c", "a", "b"]);
    }

    #[test]
    fn move_rule_to_takes_a_rule_to_any_place() {
        let mut st = Style {
            rules: ["a", "b", "c", "d"]
                .map(|n| Rule::new(n, Vec::new()))
                .to_vec(),
            mask: None,
        };
        assert_eq!(st.move_rule_to(0, 2), 2);
        assert_eq!(st.move_rule_to(3, 0), 0);
        assert_eq!(st.move_rule_to(1, 9), 3);
        assert_eq!(st.move_rule_to(9, 0), 9);
        let names: Vec<_> = st.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["d", "c", "a", "b"]);
    }

    #[test]
    fn rule_stroke_and_fill() {
        let mut r = Rule::new("r", Vec::new());
        let s = Stroke::solid(Color::rgb(1, 2, 3), 2.5);
        r.set_stroke(Some(s));
        r.set_fill(Some(Color::rgb(4, 5, 6)));
        assert_eq!(r.stroke(), Some(s));
        assert_eq!(r.fill(), Some(Color::rgb(4, 5, 6)));
        assert!(matches!(r.symbolizers[0], Symbolizer::Fill { .. }));
        r.set_fill(None);
        r.set_stroke(None);
        assert!(r.symbolizers.is_empty());
    }

    #[test]
    fn add_rule_copies_the_last_rule() {
        let mut l = LayerStyle::default_for(GeometryKind::Line, 0, &[]);
        l.style.rules[0].name = "Rule 2".into();
        assert_eq!(l.add_rule(GeometryKind::Line), 1);
        assert_eq!(l.style.rules[1].name, "Rule 1");
        let (first, second) = (l.style.rules[0].stroke(), l.style.rules[1].stroke());
        assert_eq!(first.map(|s| s.width), second.map(|s| s.width));
        assert_ne!(first.map(|s| s.color), second.map(|s| s.color));
        assert_eq!(l.add_rule(GeometryKind::Line), 2);
        assert_eq!(l.style.rules[2].name, "Rule 3");
        let mut colors: Vec<_> = l
            .style
            .rules
            .iter()
            .map(|r| r.stroke().unwrap().color.r)
            .collect();
        colors.dedup();
        assert_eq!(colors.len(), 3);
        let mut empty = LayerStyle {
            style: Style::default(),
            label: l.label.clone(),
            flow_height: None,
            weights: Weights::default(),
        };
        empty.add_rule(GeometryKind::Polygon);
        assert!(empty.fill().is_some());
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
