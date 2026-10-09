//! Automatic map label placement on page-space geometry.
//!
//! The library takes page-space geometry, label classes, and obstacles, and
//! returns placed glyph runs. It never sees longitude or latitude.

mod collide;
mod engine;
mod geom;
mod improve;
mod line;
mod model;
mod point;
mod text;

pub use kurbo::{self, Affine, BezPath, Point, Rect, Vec2};
pub use parley::FontData;

pub use collide::OrientedBox;
pub use model::*;
pub use text::{FontError, Fonts, TextMetrics, glyph_outline};

/// Place labels. Never panics on bad input; reports it as an [`UnplacedLabel`].
/// The same input always gives the same output.
pub fn place_labels(fonts: &mut Fonts, input: &LabelInput) -> Labeling {
    engine::run(fonts, input)
}
