//! Fonts, shaping, and glyph outlines.

use crate::model::{Caps, PlacedLabel, TextStyle};
use kurbo::{BezPath, Rect, Shape, Vec2};
use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::setting::Tag;
use parley::{
    FontContext, FontData, FontFamily, FontFeature, FontFeatures, FontStyle, FontWeight, Layout,
    LayoutContext, PositionedLayoutItem, StyleProperty,
};
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::{FontRef, GlyphId, MetadataProvider};
use std::ops::Range;
use std::sync::Arc;

/// Registered fonts and the shaping context.
pub struct Fonts {
    pub(crate) fcx: FontContext,
    pub(crate) lcx: LayoutContext<()>,
}

/// Error from [`Fonts::register`].
#[derive(Debug)]
pub enum FontError {
    /// The bytes held no font.
    NoFontInData,
}

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontError::NoFontInData => f.write_str("the data holds no font"),
        }
    }
}

impl std::error::Error for FontError {}

/// Measurements of one shaped string.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TextMetrics {
    pub advance: f64,
    pub ascent: f64,
    pub descent: f64,
    pub cap_height: f64,
    /// Relative to the baseline origin, y down.
    pub ink_bounds: Rect,
    pub glyph_count: usize,
    pub missing_glyphs: usize,
}

/// A clone shares the fonts, so it does not search the system fonts again.
impl Clone for Fonts {
    fn clone(&self) -> Self {
        Self {
            fcx: self.fcx.clone(),
            lcx: LayoutContext::new(),
        }
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl Fonts {
    /// An empty font set with system fonts off.
    pub fn new() -> Self {
        Self {
            fcx: FontContext {
                collection: Collection::new(CollectionOptions {
                    shared: false,
                    system_fonts: false,
                }),
                source_cache: SourceCache::default(),
            },
            lcx: LayoutContext::new(),
        }
    }

    /// A font set that also finds system fonts.
    pub fn with_system_fonts() -> Self {
        Self {
            fcx: FontContext::new(),
            lcx: LayoutContext::new(),
        }
    }

    /// Register font bytes. Returns the family names found, without duplicates.
    pub fn register(&mut self, data: Vec<u8>) -> Result<Vec<String>, FontError> {
        let reg = self
            .fcx
            .collection
            .register_fonts(Blob::new(Arc::new(data)), None);
        let mut names: Vec<String> = Vec::new();
        for (id, _) in &reg {
            if let Some(name) = self.fcx.collection.family_name(*id)
                && !names.iter().any(|n| n == name)
            {
                names.push(name.to_string());
            }
        }
        if names.is_empty() {
            Err(FontError::NoFontInData)
        } else {
            Ok(names)
        }
    }

    /// Whether a registered font has the family `name`.
    pub fn has_family(&mut self, name: &str) -> bool {
        self.fcx.collection.family_id(name).is_some()
    }

    /// The names of all families, registered and system, sorted and without
    /// duplicates.
    pub fn families(&mut self) -> Vec<String> {
        let mut names: Vec<String> = self
            .fcx
            .collection
            .family_names()
            .map(String::from)
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Measure `text` in `style`.
    pub fn measure(&mut self, text: &str, style: &TextStyle) -> TextMetrics {
        let s = self.shape(text, style);
        TextMetrics {
            advance: s.advance,
            ascent: s.ascent,
            descent: s.descent,
            cap_height: s.cap_height,
            ink_bounds: s.ink_bounds,
            glyph_count: s.glyph_count(),
            missing_glyphs: s.missing_glyphs,
        }
    }

    /// The faces of `family`, sorted by weight with upright before italic.
    /// An unknown family gives an empty list.
    pub fn faces(&mut self, family: &str) -> Vec<Face> {
        let coll = &mut self.fcx.collection;
        let Some(info) = coll.family_id(family).and_then(|id| coll.family(id)) else {
            return Vec::new();
        };
        let mut faces: Vec<Face> = info
            .fonts()
            .iter()
            .map(|f| Face {
                weight: f.weight().value(),
                italic: f.style() != FontStyle::Normal,
            })
            .collect();
        faces.sort_by(|a, b| a.weight.total_cmp(&b.weight).then(a.italic.cmp(&b.italic)));
        faces.dedup();
        faces
    }

    /// Shape `text` on one line.
    pub(crate) fn shape(&mut self, text: &str, style: &TextStyle) -> ShapedText {
        self.shape_caps(text, style, false)
    }

    /// Shape `text` on one line. With `make_caps`, small capitals are always
    /// made from capitals at a smaller size.
    pub(crate) fn shape_caps(
        &mut self,
        text: &str,
        style: &TextStyle,
        make_caps: bool,
    ) -> ShapedText {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let features: &[&[u8; 4]] = match style.caps {
            Caps::Normal => return self.shape_ranges(&text, style, &[], &[]),
            Caps::Small => &[b"smcp"],
            Caps::AllSmall => &[b"c2sc", b"smcp"],
        };
        if !make_caps {
            let shaped = self.shape_ranges(&text, style, features, &[]);
            if shaped
                .runs
                .iter()
                .all(|r| features.iter().all(|t| has_feature(&r.font, t)))
            {
                return shaped;
            }
        }
        let all = style.caps == Caps::AllSmall;
        let mut made = String::with_capacity(text.len());
        let mut small: Vec<Range<usize>> = Vec::new();
        for c in text.chars() {
            let start = made.len();
            if c.is_lowercase() {
                made.extend(c.to_uppercase());
            } else if all && c.is_uppercase() {
                made.push(c);
            } else {
                made.push(c);
                continue;
            }
            match small.last_mut() {
                Some(r) if r.end == start => r.end = made.len(),
                _ => small.push(start..made.len()),
            }
        }
        self.shape_ranges(&made, style, &[], &small)
    }

    /// Shape `text` with OpenType `features`, and with the `small` byte ranges
    /// at the small capital size.
    fn shape_ranges(
        &mut self,
        text: &str,
        style: &TextStyle,
        features: &[&[u8; 4]],
        small: &[Range<usize>],
    ) -> ShapedText {
        let mut out = ShapedText {
            runs: Vec::new(),
            advance: 0.0,
            ascent: 0.0,
            descent: 0.0,
            cap_height: 0.7 * style.size,
            ink_bounds: Rect::ZERO,
            missing_glyphs: 0,
        };
        let size = style.size as f32;
        let mut b = self.lcx.ranged_builder(&mut self.fcx, text, 1.0, false);
        b.push_default(StyleProperty::FontSize(size));
        b.push_default(StyleProperty::FontFamily(FontFamily::from(
            style.family.as_str(),
        )));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(style.weight)));
        b.push_default(StyleProperty::FontStyle(if style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        }));
        b.push_default(StyleProperty::LetterSpacing(style.letter_spacing as f32));
        if !features.is_empty() {
            let list: Vec<FontFeature> = features
                .iter()
                .map(|t| FontFeature::new(Tag::new(t), 1))
                .collect();
            b.push_default(StyleProperty::FontFeatures(FontFeatures::List(list.into())));
        }
        for r in small {
            b.push(StyleProperty::FontSize(SMALL_CAP_SCALE * size), r.clone());
        }
        let mut layout: Layout<()> = b.build(text);
        layout.break_all_lines(None);
        out.advance = layout.width() as f64;
        let mut ink: Option<Rect> = None;
        // The metrics come from the first run at the full size. If no run has
        // the full size, they come from the first run, scaled.
        let mut full_metrics = false;
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(gr) = item else {
                    continue;
                };
                let run = gr.run();
                let m = run.metrics();
                let font_size = run.font_size();
                let full = font_size == size;
                if !full_metrics && (full || out.runs.is_empty()) {
                    full_metrics = full;
                    let k = if full {
                        1.0
                    } else {
                        style.size / font_size as f64
                    };
                    out.ascent = k * m.ascent as f64;
                    out.descent = k * m.descent as f64;
                    out.cap_height = m.cap_height.map_or(0.7 * style.size, |c| k * c as f64);
                }
                let font = run.font().clone();
                let coords: Vec<i16> = run.normalized_coords().to_vec();
                let baseline = gr.baseline();
                let mut glyphs = Vec::new();
                for g in gr.positioned_glyphs() {
                    if g.id == 0 {
                        out.missing_glyphs += 1;
                    }
                    let bbox = glyph_outline(&font, &coords, font_size, g.id)
                        .filter(|p| !p.elements().is_empty())
                        .map(|p| p.bounding_box());
                    let (x, y) = (g.x as f64, (g.y - baseline) as f64);
                    if let Some(bb) = bbox {
                        let moved = bb + Vec2::new(x, y);
                        ink = Some(ink.map_or(moved, |r| r.union(moved)));
                    }
                    glyphs.push(ShapedGlyph {
                        id: g.id,
                        x,
                        y,
                        advance: g.advance as f64,
                        bbox,
                    });
                }
                out.runs.push(ShapedRun {
                    font,
                    font_size,
                    normalized_coords: coords,
                    glyphs,
                });
            }
        }
        out.ink_bounds = ink.unwrap_or(Rect::ZERO);
        out
    }
}

/// Size of a made small capital, as a share of the font size.
const SMALL_CAP_SCALE: f32 = 0.7;

/// Whether the GSUB table of `font` has the feature `tag`.
fn has_feature(font: &FontData, tag: &[u8; 4]) -> bool {
    let tag = skrifa::Tag::new(tag);
    FontRef::from_index(font.data.as_ref(), font.index)
        .ok()
        .and_then(|f| f.gsub().ok())
        .and_then(|g| g.feature_list().ok())
        .is_some_and(|l| l.feature_records().iter().any(|r| r.feature_tag() == tag))
}

/// One face of a font family.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    /// Font weight; 400.0 is regular.
    pub weight: f32,
    /// Italic or oblique.
    pub italic: bool,
}

impl Face {
    /// The standard style name, for example "Bold italic".
    pub fn name(&self) -> String {
        const NAMES: [&str; 9] = [
            "Thin",
            "Extra light",
            "Light",
            "Regular",
            "Medium",
            "Semibold",
            "Bold",
            "Extra bold",
            "Black",
        ];
        let i = (self.weight / 100.0).round().clamp(1.0, 9.0) as usize - 1;
        match (NAMES[i], self.italic) {
            ("Regular", true) => "Italic".to_string(),
            (n, true) => format!("{n} italic"),
            (n, false) => n.to_string(),
        }
    }
}

/// One positioned glyph.
pub(crate) struct ShapedGlyph {
    pub id: u32,
    /// Pen position along the baseline, from the label origin.
    pub x: f64,
    /// Offset from the baseline, y down.
    pub y: f64,
    pub advance: f64,
    /// Outline bounds in glyph-local space; `None` for an empty outline.
    pub bbox: Option<Rect>,
}

pub(crate) struct ShapedRun {
    pub font: FontData,
    pub font_size: f32,
    pub normalized_coords: Vec<i16>,
    pub glyphs: Vec<ShapedGlyph>,
}

pub(crate) struct ShapedText {
    pub runs: Vec<ShapedRun>,
    pub advance: f64,
    pub ascent: f64,
    pub descent: f64,
    pub cap_height: f64,
    /// Union of glyph boxes moved by (x, y); `Rect::ZERO` when there is no ink.
    pub ink_bounds: Rect,
    pub missing_glyphs: usize,
}

impl ShapedText {
    pub fn glyph_count(&self) -> usize {
        self.runs.iter().map(|r| r.glyphs.len()).sum()
    }

    pub fn glyphs(&self) -> impl Iterator<Item = (&ShapedRun, &ShapedGlyph)> {
        self.runs
            .iter()
            .flat_map(|r| r.glyphs.iter().map(move |g| (r, g)))
    }
}

struct PathPen(BezPath);

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((x as f64, -(y as f64)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((x as f64, -(y as f64)));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0
            .quad_to((cx0 as f64, -(cy0 as f64)), (x as f64, -(y as f64)));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to(
            (cx0 as f64, -(cy0 as f64)),
            (cx1 as f64, -(cy1 as f64)),
            (x as f64, -(y as f64)),
        );
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}

/// The outline of one glyph in glyph-local space, y down.
pub fn glyph_outline(
    font: &FontData,
    normalized_coords: &[i16],
    font_size: f32,
    glyph_id: u32,
) -> Option<BezPath> {
    let fref = FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let coords: Vec<NormalizedCoord> = normalized_coords
        .iter()
        .map(|c| NormalizedCoord::from_bits(*c))
        .collect();
    let glyph = fref.outline_glyphs().get(GlyphId::new(glyph_id))?;
    let mut pen = PathPen(BezPath::new());
    glyph
        .draw(
            DrawSettings::unhinted(Size::new(font_size), LocationRef::new(&coords)),
            &mut pen,
        )
        .ok()?;
    if pen.0.elements().is_empty() {
        None
    } else {
        Some(pen.0)
    }
}

/// All glyph outlines of a placed label in page space.
pub(crate) fn label_outlines(label: &PlacedLabel) -> BezPath {
    let mut out = BezPath::new();
    for run in &label.glyph_runs {
        for g in &run.glyphs {
            if let Some(p) = glyph_outline(&run.font, &run.normalized_coords, run.font_size, g.id) {
                out.extend((g.transform * p).elements().iter().copied());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/../../assets/fonts/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn registers_noto_sans() {
        let names = Fonts::new().register(read("NotoSans-Regular.ttf")).unwrap();
        assert_eq!(names, vec!["Noto Sans".to_string()]);
    }

    #[test]
    fn knows_the_registered_families() {
        let mut f = Fonts::new();
        assert!(!f.has_family("Noto Sans"));
        f.register(read("NotoSans-Regular.ttf")).unwrap();
        assert!(f.has_family("Noto Sans"));
        assert!(!f.has_family("Noto Serif"));
        assert_eq!(f.families(), ["Noto Sans"]);
        assert!(f.clone().has_family("Noto Sans"));
    }

    #[test]
    fn rejects_garbage() {
        let err = Fonts::new().register(vec![1, 2, 3]).unwrap_err();
        assert!(matches!(err, FontError::NoFontInData));
    }

    fn fonts() -> Fonts {
        let mut f = Fonts::new();
        f.register(read("NotoSans-Regular.ttf")).unwrap();
        f.register(read("NotoSans-Italic.ttf")).unwrap();
        f
    }

    fn st(size: f64) -> TextStyle {
        TextStyle::new("Noto Sans", size)
    }

    #[test]
    fn parana_metrics() {
        let mut f = fonts();
        let m = f.measure("Paraná", &st(20.0));
        assert_eq!(m.glyph_count, 6);
        assert_eq!(m.missing_glyphs, 0);
        assert!((m.advance - 65.98).abs() < 0.05, "{}", m.advance);
        assert!((m.cap_height - 14.28).abs() < 0.05, "{}", m.cap_height);
        let mut s = st(20.0);
        s.italic = true;
        let sh = f.shape("Paraná", &s);
        assert!((sh.advance - 64.56).abs() < 0.05, "{}", sh.advance);
        assert_eq!(
            sh.runs[0].font.data.len(),
            read("NotoSans-Italic.ttf").len()
        );
    }

    #[test]
    fn scales_and_spacing() {
        let mut f = fonts();
        let a = f.measure("Paraná", &st(20.0)).advance;
        let b = f.measure("Paraná", &st(40.0)).advance;
        assert!((b - 2.0 * a).abs() < 0.1);
        let mut s = st(20.0);
        s.letter_spacing = 2.0;
        assert!(f.measure("abc", &s).advance > f.measure("abc", &st(20.0)).advance);
    }

    #[test]
    fn missing_family_and_glyphs() {
        let mut f = fonts();
        let m = f.measure("abc", &TextStyle::new("No Such Family", 20.0));
        assert_eq!(m.glyph_count, 0);
        assert_eq!(f.measure("北京", &st(20.0)).missing_glyphs, 2);
    }

    #[test]
    fn space_has_no_ink_and_whitespace_collapses() {
        let mut f = fonts();
        let s = f.shape("a b", &st(20.0));
        let g: Vec<_> = s.glyphs().map(|(_, g)| g).collect();
        assert_eq!(g.len(), 3);
        assert!(g[0].bbox.is_some() && g[1].bbox.is_none() && g[2].bbox.is_some());
        assert!(s.ink_bounds.y0 < 0.0);
        assert!(s.ink_bounds.width() <= s.advance);
        let t = f.shape("  a \n b ", &st(20.0));
        assert_eq!(t.advance, s.advance);
        assert_eq!(t.ink_bounds, s.ink_bounds);
        assert_eq!(t.glyph_count(), 3);
    }

    #[test]
    fn outlines_match_bbox() {
        let mut f = fonts();
        let s = f.shape("P", &st(20.0));
        let (run, g) = s.glyphs().next().unwrap();
        let p = glyph_outline(&run.font, &run.normalized_coords, run.font_size, g.id).unwrap();
        assert!(!p.elements().is_empty());
        let bb = p.bounding_box();
        let sb = g.bbox.unwrap();
        assert!((bb.x0 - sb.x0).abs() < 1e-6 && (bb.y1 - sb.y1).abs() < 1e-6);
        let label = PlacedLabel {
            feature: 0,
            class: 0,
            repeat: 0,
            text: "P".into(),
            kind: crate::model::CandidateKind::Point(crate::model::PointPosition::Right),
            glyph_runs: vec![crate::model::PlacedGlyphRun {
                font: run.font.clone(),
                font_size: run.font_size,
                normalized_coords: run.normalized_coords.clone(),
                glyphs: vec![crate::model::PlacedGlyph {
                    id: g.id,
                    transform: kurbo::Affine::translate((100.0, 50.0)),
                }],
            }],
            boxes: vec![],
            bounds: Rect::ZERO,
            cost: 0.0,
        };
        let ob = label.outlines().bounding_box();
        let want = sb + Vec2::new(100.0, 50.0);
        assert!((ob.x0 - want.x0).abs() < 1e-6 && (ob.y0 - want.y0).abs() < 1e-6);
        assert!((ob.x1 - want.x1).abs() < 1e-6 && (ob.y1 - want.y1).abs() < 1e-6);
    }

    #[test]
    fn faces_list_the_styles_of_a_family() {
        let mut f = fonts();
        let faces = f.faces("Noto Sans");
        assert_eq!(
            faces,
            [
                Face {
                    weight: 400.0,
                    italic: false
                },
                Face {
                    weight: 400.0,
                    italic: true
                }
            ]
        );
        f.register(read("NotoSans-Regular.ttf")).unwrap();
        assert_eq!(f.faces("Noto Sans").len(), 2);
        assert!(f.faces("No Such Family").is_empty());
    }

    #[test]
    fn a_face_has_a_standard_name() {
        let face = |weight, italic| Face { weight, italic }.name();
        assert_eq!(face(400.0, false), "Regular");
        assert_eq!(face(400.0, true), "Italic");
        assert_eq!(face(700.0, true), "Bold italic");
        assert_eq!(face(200.0, false), "Extra light");
        assert_eq!(face(620.0, false), "Semibold");
        assert_eq!(face(50.0, false), "Thin");
        assert_eq!(face(1000.0, true), "Black italic");
    }

    fn caps(caps: Caps) -> TextStyle {
        TextStyle { caps, ..st(20.0) }
    }

    #[test]
    fn noto_sans_has_small_caps() {
        let mut f = fonts();
        let s = f.shape("a", &st(20.0));
        assert!(has_feature(&s.runs[0].font, b"smcp"));
        assert!(has_feature(&s.runs[0].font, b"c2sc"));
        assert!(!has_feature(&s.runs[0].font, b"zzzz"));
    }

    #[test]
    fn small_caps_are_narrower_than_capitals_and_wider_than_lowercase() {
        let mut f = fonts();
        let lower = f.measure("vale", &st(20.0)).advance;
        let upper = f.measure("VALE", &st(20.0)).advance;
        let small = f.measure("Vale", &caps(Caps::Small)).advance;
        assert!(lower < small && small < upper, "{lower} {small} {upper}");
    }

    #[test]
    fn all_small_caps_turn_capitals_small_too() {
        let mut f = fonts();
        for make in [false, true] {
            let small = f.shape_caps("Vale", &caps(Caps::Small), make);
            let all = f.shape_caps("Vale", &caps(Caps::AllSmall), make);
            assert_eq!(all.glyph_count(), 4);
            assert!(all.advance < small.advance);
            let h = |s: &ShapedText, i: usize| s.glyphs().nth(i).unwrap().1.bbox.unwrap().height();
            assert!(h(&all, 0) < 0.9 * h(&small, 0), "{make}");
            assert!((h(&all, 0) / h(&all, 2) - 1.0).abs() < 0.02, "{make}");
            let same = f.shape_caps("vale", &caps(Caps::AllSmall), make);
            assert!((same.advance - all.advance).abs() < 1e-6);
        }
    }

    #[test]
    fn made_small_caps_use_a_smaller_size() {
        let mut f = fonts();
        let s = f.shape_caps("Vale", &caps(Caps::Small), true);
        let sizes: Vec<f32> = s.runs.iter().map(|r| r.font_size).collect();
        assert_eq!(sizes, [20.0, 14.0]);
        let h: Vec<f64> = s.glyphs().map(|(_, g)| g.bbox.unwrap().height()).collect();
        for x in &h[1..] {
            assert!((x / h[0] - 0.7).abs() < 0.03, "{h:?}");
        }
        assert!((s.cap_height - 14.28).abs() < 0.05);
        let m = f.measure("Vale", &caps(Caps::Small));
        assert!(m.cap_height > 14.0);
    }
}
