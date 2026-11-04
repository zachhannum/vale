//! Display list and rendering backends for Vale.

#[cfg(feature = "gpu")]
pub mod gpu;
pub mod hatch;
pub mod list;
pub mod pdf;
pub mod raster;
pub mod stroke;
pub mod symbol;

pub use list::*;
pub use pdf::{PdfDocument, render_pdf};
pub use raster::{Rgba, glyph_run_outline, pixel_size, render_pixmap, render_png, render_rgba};
pub use symbol::{Symbol, SymbolError, SymbolPath, Symbols, symbol_items};
