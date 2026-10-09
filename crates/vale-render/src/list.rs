//! The display list: a page of fills, strokes, and glyph runs in page space.

pub use kurbo::{Affine, BezPath, Point, Rect};
pub use vale_labeler::FontData;

use std::fmt;

/// An RGBA color with straight alpha.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// An opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }

    /// A color with alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a }
    }

    /// `#rrggbb`, or `#rrggbbaa` when alpha is not 255.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// Reads both hex forms, with or without `#`.
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        if !s.is_ascii() || (s.len() != 6 && s.len() != 8) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        let a = if s.len() == 8 { byte(6)? } else { 255 };
        Some(Color {
            r: byte(0)?,
            g: byte(2)?,
            b: byte(4)?,
            a,
        })
    }
}

/// How a fill decides what is inside.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// Glyphs of one label in one font.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphRun {
    pub font: FontData,
    pub font_size: f32,
    pub normalized_coords: Vec<i16>,
    /// Glyph ID and glyph-local to page transform.
    pub glyphs: Vec<(u32, Affine)>,
    /// The text of the label that this run belongs to.
    pub text: String,
    pub color: Color,
}

/// One drawing command.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Fill {
        path: BezPath,
        color: Color,
        rule: FillRule,
    },
    /// `round` means round joins and caps.
    Stroke {
        path: BezPath,
        color: Color,
        width: f64,
        round: bool,
    },
    Glyphs(GlyphRun),
}

/// A page and its items, in drawing order. The size is in points.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayList {
    pub width: f64,
    pub height: f64,
    pub items: Vec<Item>,
}

impl DisplayList {
    /// An empty page.
    pub fn new(width: f64, height: f64) -> Self {
        DisplayList {
            width,
            height,
            items: Vec::new(),
        }
    }

    /// Adds an item on top.
    pub fn push(&mut self, item: Item) {
        self.items.push(item);
    }
}

/// A rendering failure.
#[derive(Debug)]
pub enum RenderError {
    EmptyPage,
    TooLarge { width: f64, height: f64 },
    Png(String),
    Pdf(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::EmptyPage => write!(f, "the page has no area"),
            RenderError::TooLarge { width, height } => {
                write!(f, "the page is too large: {width} by {height}")
            }
            RenderError::Png(m) => write!(f, "PNG error: {m}"),
            RenderError::Pdf(m) => write!(f, "PDF error: {m}"),
        }
    }
}

impl std::error::Error for RenderError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let c = Color::rgb(244, 241, 232);
        assert_eq!(c.to_hex(), "#f4f1e8");
        assert_eq!(Color::from_hex("#f4f1e8"), Some(c));
        assert_eq!(Color::from_hex("f4f1e8"), Some(c));
        let d = Color::rgba(120, 140, 150, 110);
        assert_eq!(d.to_hex(), "#788c966e");
        assert_eq!(Color::from_hex("#788c966e"), Some(d));
    }

    #[test]
    fn bad_hex() {
        assert_eq!(Color::from_hex("nope"), None);
        assert_eq!(Color::from_hex("#12345"), None);
        assert_eq!(Color::from_hex("#gggggg"), None);
    }
}
