//! Sphere projections, clipping, and graticule.

pub mod clip;
pub mod frame;
pub mod graticule;
pub mod projection;
pub mod spec;

pub use clip::{ClipRect, EPS, clip_line_rect, clip_ring_rect, densify, shifts, unwrap};
pub use frame::{Frame, LonLat, wrap180};
pub use graticule::{distance_km, graticule, nice_step};
pub use kurbo::{Point, Rect};
pub use projection::{Mesh, Projection, SphereError};
pub use spec::*;
