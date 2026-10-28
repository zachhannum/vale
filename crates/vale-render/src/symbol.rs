//! Point symbols from SVG files, and the display list items that draw them.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock};

use kurbo::{Affine, BezPath, Shape};

use crate::list::{Color, FillRule, Item};

/// The names of the built-in symbols, in library order.
pub const BUILTIN_SYMBOLS: [&str; 9] = [
    "tree", "pine", "palm", "mountain", "hill", "marsh", "city", "capital", "castle",
];

const BUILTIN_SVG: [&str; 9] = [
    include_str!("../symbols/tree.svg"),
    include_str!("../symbols/pine.svg"),
    include_str!("../symbols/palm.svg"),
    include_str!("../symbols/mountain.svg"),
    include_str!("../symbols/hill.svg"),
    include_str!("../symbols/marsh.svg"),
    include_str!("../symbols/city.svg"),
    include_str!("../symbols/capital.svg"),
    include_str!("../symbols/castle.svg"),
];

/// One path of a symbol, with its paint. The stroke width is in symbol units.
#[derive(Clone, Debug, PartialEq)]
pub struct SymbolPath {
    pub path: BezPath,
    pub fill: Option<(Color, FillRule)>,
    pub stroke: Option<(Color, f64)>,
}

/// A symbol in symbol units: the longer side of its bounding box is 1, the box is
/// centered at 0,0, and y goes down. The bounding box holds the path geometry, not the
/// stroke widths.
#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    paths: Vec<SymbolPath>,
}

/// A failure to read a symbol.
#[derive(Clone, Debug, PartialEq)]
pub enum SymbolError {
    /// The text is not a valid SVG file.
    Parse(String),
    /// The file has no path with a fill or a stroke.
    Empty,
}

impl fmt::Display for SymbolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SymbolError::Parse(m) => write!(f, "the SVG file is not valid: {m}"),
            SymbolError::Empty => write!(f, "the SVG file has no shapes to draw"),
        }
    }
}

impl std::error::Error for SymbolError {}

impl Symbol {
    /// Reads the paths of an SVG file. Solid colors and the first stop of a gradient
    /// become the colors. Images, text, patterns, clips, and masks are dropped.
    pub fn from_svg(svg: &str) -> Result<Symbol, SymbolError> {
        let tree = usvg::Tree::from_str(svg, &usvg::Options::default())
            .map_err(|e| SymbolError::Parse(e.to_string()))?;
        let mut paths = Vec::new();
        collect(tree.root(), 1.0, &mut paths);
        normalize(&mut paths)?;
        Ok(Symbol { paths })
    }

    /// The paths in drawing order.
    pub fn paths(&self) -> &[SymbolPath] {
        &self.paths
    }
}

fn collect(group: &usvg::Group, opacity: f32, out: &mut Vec<SymbolPath>) {
    let opacity = opacity * group.opacity().get();
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect(g, opacity, out),
            usvg::Node::Path(p) => {
                if let Some(path) = symbol_path(p, opacity) {
                    out.push(path);
                }
            }
            _ => {}
        }
    }
}

fn symbol_path(p: &usvg::Path, opacity: f32) -> Option<SymbolPath> {
    if !p.is_visible() {
        return None;
    }
    let fill = p.fill().and_then(|f| {
        let color = paint_color(f.paint(), opacity * f.opacity().get())?;
        let rule = match f.rule() {
            usvg::FillRule::NonZero => FillRule::NonZero,
            usvg::FillRule::EvenOdd => FillRule::EvenOdd,
        };
        Some((color, rule))
    });
    let t = p.abs_transform();
    let affine = Affine::new([t.sx, t.ky, t.kx, t.sy, t.tx, t.ty].map(f64::from));
    let stroke = p.stroke().and_then(|s| {
        let color = paint_color(s.paint(), opacity * s.opacity().get())?;
        Some((color, f64::from(s.width().get()) * uniform_scale(affine)))
    });
    if fill.is_none() && stroke.is_none() {
        return None;
    }
    let mut path = BezPath::new();
    for seg in p.data().segments() {
        use usvg::tiny_skia_path::PathSegment as S;
        let pt = |p: usvg::tiny_skia_path::Point| (f64::from(p.x), f64::from(p.y));
        match seg {
            S::MoveTo(a) => path.move_to(pt(a)),
            S::LineTo(a) => path.line_to(pt(a)),
            S::QuadTo(a, b) => path.quad_to(pt(a), pt(b)),
            S::CubicTo(a, b, c) => path.curve_to(pt(a), pt(b), pt(c)),
            S::Close => path.close_path(),
        }
    }
    path.apply_affine(affine);
    Some(SymbolPath { path, fill, stroke })
}

fn paint_color(paint: &usvg::Paint, opacity: f32) -> Option<Color> {
    let (c, stop_opacity) = match paint {
        usvg::Paint::Color(c) => (*c, 1.0),
        usvg::Paint::LinearGradient(g) => first_stop(g.stops())?,
        usvg::Paint::RadialGradient(g) => first_stop(g.stops())?,
        usvg::Paint::Pattern(_) => return None,
    };
    let a = (opacity * stop_opacity * 255.0).round().clamp(0.0, 255.0) as u8;
    Some(Color::rgba(c.red, c.green, c.blue, a))
}

fn first_stop(stops: &[usvg::Stop]) -> Option<(usvg::Color, f32)> {
    stops.first().map(|s| (s.color(), s.opacity().get()))
}

fn normalize(paths: &mut [SymbolPath]) -> Result<(), SymbolError> {
    let bounds = paths
        .iter()
        .map(|p| p.path.bounding_box())
        .reduce(|a, b| a.union(b))
        .ok_or(SymbolError::Empty)?;
    let side = bounds.width().max(bounds.height());
    if !side.is_finite() || side <= 0.0 {
        return Err(SymbolError::Empty);
    }
    let to_unit = Affine::scale(1.0 / side) * Affine::translate(-bounds.center().to_vec2());
    for p in paths {
        p.path.apply_affine(to_unit);
        if let Some((_, w)) = &mut p.stroke {
            *w /= side;
        }
    }
    Ok(())
}

/// The scale of an affine as one number: the square root of the absolute determinant.
fn uniform_scale(a: Affine) -> f64 {
    a.determinant().abs().sqrt()
}

fn tinted(color: Color, tint: Option<Color>) -> Color {
    match tint {
        None => color,
        Some(t) => {
            let a = (u16::from(t.a) * u16::from(color.a) + 127) / 255;
            Color::rgba(t.r, t.g, t.b, a as u8)
        }
    }
}

/// The items that draw `symbol` once for each affine, which maps symbol units to page
/// space. Stroke widths grow with the scale of each affine. A tint replaces the color of
/// each path, and the alphas multiply.
///
/// One instance gives the items in path order. More instances give one item for each
/// symbol path, with all the instances in one path, so the instances do not overlap
/// each other path by path. Strokes of instances with different scales go in separate
/// items, one for each 2% step of the scale.
pub fn symbol_items(symbol: &Symbol, instances: &[Affine], tint: Option<Color>) -> Vec<Item> {
    let mut items = Vec::new();
    if instances.is_empty() {
        return items;
    }
    if let [a] = instances {
        for p in &symbol.paths {
            push_items(&mut items, p, *a * &p.path, uniform_scale(*a), tint);
        }
        return items;
    }
    for p in &symbol.paths {
        if p.fill.is_some() {
            let mut merged = BezPath::new();
            for a in instances {
                merged.extend(*a * &p.path);
            }
            let fill = SymbolPath {
                path: BezPath::new(),
                fill: p.fill,
                stroke: None,
            };
            push_items(&mut items, &fill, merged, 1.0, tint);
        }
        if p.stroke.is_none() {
            continue;
        }
        let mut groups: Vec<(i64, BezPath)> = Vec::new();
        let mut index: HashMap<i64, usize> = HashMap::new();
        for a in instances {
            let key = scale_step(uniform_scale(*a));
            let i = *index.entry(key).or_insert_with(|| {
                groups.push((key, BezPath::new()));
                groups.len() - 1
            });
            groups[i].1.extend(*a * &p.path);
        }
        for (key, path) in groups {
            let stroke = SymbolPath {
                fill: None,
                path: BezPath::new(),
                ..*p
            };
            push_items(&mut items, &stroke, path, step_scale(key), tint);
        }
    }
    items
}

const STEP: f64 = 1.02;

/// The 2% step of a scale. A scale of zero has its own step.
fn scale_step(scale: f64) -> i64 {
    if scale > 0.0 {
        (scale.ln() / STEP.ln()).round() as i64
    } else {
        i64::MIN
    }
}

fn step_scale(step: i64) -> f64 {
    if step == i64::MIN {
        0.0
    } else {
        STEP.powf(step as f64)
    }
}

fn push_items(
    items: &mut Vec<Item>,
    p: &SymbolPath,
    path: BezPath,
    scale: f64,
    tint: Option<Color>,
) {
    if let Some((color, rule)) = p.fill {
        items.push(Item::Fill {
            path: path.clone(),
            color: tinted(color, tint),
            rule,
        });
    }
    if let Some((color, width)) = p.stroke {
        items.push(Item::Stroke {
            path,
            color: tinted(color, tint),
            width: width * scale,
            round: true,
        });
    }
}

/// A library of symbols by name, in insertion order.
#[derive(Clone, Debug, Default)]
pub struct Symbols {
    entries: Vec<(String, Arc<Symbol>)>,
}

impl Symbols {
    /// The built-in symbols.
    pub fn builtin() -> &'static Symbols {
        static BUILTIN: OnceLock<Symbols> = OnceLock::new();
        BUILTIN.get_or_init(|| {
            let mut s = Symbols::default();
            for (name, svg) in BUILTIN_SYMBOLS.iter().zip(BUILTIN_SVG) {
                let symbol = Symbol::from_svg(svg).expect("a built-in symbol is valid");
                s.insert(name, symbol);
            }
            s
        })
    }

    /// A copy of the built-in symbols, to add more to.
    pub fn with_builtins() -> Symbols {
        Symbols::builtin().clone()
    }

    /// Adds a symbol at the end, or replaces the symbol with the same name in its place.
    pub fn insert(&mut self, name: &str, symbol: Symbol) {
        let symbol = Arc::new(symbol);
        match self.entries.iter_mut().find(|(n, _)| n == name) {
            Some(entry) => entry.1 = symbol,
            None => self.entries.push((name.to_owned(), symbol)),
        }
    }

    /// The symbol with this name.
    pub fn get(&self, name: &str) -> Option<&Arc<Symbol>> {
        self.entries.iter().find(|(n, _)| n == name).map(|(_, s)| s)
    }

    /// The names in insertion order.
    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// Whether the name is the name of a built-in symbol.
    pub fn is_builtin(name: &str) -> bool {
        BUILTIN_SYMBOLS.contains(&name)
    }
}
