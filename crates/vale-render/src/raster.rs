//! The `vello_cpu` raster backend.

use std::io::Cursor;
use std::sync::Arc;

use kurbo::{Affine, BezPath, Cap, Join, Rect, Stroke};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::peniko::{Fill, ImageQuality, ImageSampler};
use vello_cpu::{Image, ImageSource, Level, Pixmap, RenderContext, RenderSettings, Resources};

use crate::list::{Color, DisplayList, FillRule, GlyphRun, Item, RenderError};

/// The largest side of a raster page, in pixels.
const MAX_SIDE: f64 = 16384.0;

/// A raster image with premultiplied RGBA8 pixels, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

fn paint(c: Color) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(c.r, c.g, c.b, c.a)
}

fn fill_rule(rule: FillRule) -> Fill {
    match rule {
        FillRule::NonZero => Fill::NonZero,
        FillRule::EvenOdd => Fill::EvenOdd,
    }
}

/// All glyph outlines of a run in page space, in one path.
pub fn glyph_run_outline(run: &GlyphRun) -> BezPath {
    let mut out = BezPath::new();
    for (id, transform) in &run.glyphs {
        if let Some(path) =
            vale_labeler::glyph_outline(&run.font, &run.normalized_coords, run.font_size, *id)
        {
            out.extend((*transform * path).elements().iter().copied());
        }
    }
    out
}

/// Draws the list at `scale` pixels per point.
pub fn render_pixmap(list: &DisplayList, scale: f64) -> Result<Pixmap, RenderError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(RenderError::EmptyPage);
    }
    let (pw, ph) = ((list.width * scale).ceil(), (list.height * scale).ceil());
    if !(pw >= 1.0 && ph >= 1.0) {
        return Err(RenderError::EmptyPage);
    }
    if pw > MAX_SIDE || ph > MAX_SIDE {
        return Err(RenderError::TooLarge {
            width: pw,
            height: ph,
        });
    }
    let (w, h) = (pw as u16, ph as u16);
    let mut ctx = RenderContext::new_with(
        w,
        h,
        RenderSettings {
            level: Level::baseline(),
            num_threads: 0,
        },
    );
    ctx.set_transform(Affine::scale(scale));
    let mut clips = 0usize;
    for item in &list.items {
        match item {
            Item::Fill { path, color, rule } => {
                ctx.set_fill_rule(fill_rule(*rule));
                ctx.set_paint(paint(*color));
                ctx.fill_path(path);
            }
            Item::Stroke {
                path,
                color,
                width,
                round,
            } => {
                let mut stroke = Stroke::new(*width);
                if *round {
                    stroke = stroke.with_join(Join::Round).with_caps(Cap::Round);
                }
                ctx.set_stroke(stroke);
                ctx.set_paint(paint(*color));
                ctx.stroke_path(path);
            }
            Item::Glyphs(run) => {
                ctx.set_fill_rule(Fill::NonZero);
                ctx.set_paint(paint(run.color));
                ctx.fill_path(&glyph_run_outline(run));
            }
            Item::Image {
                png,
                rect,
                interpolate,
            } => {
                let pixmap = Pixmap::from_png(Cursor::new(png))
                    .map_err(|e| RenderError::Png(format!("{e:?}")))?;
                let (iw, ih) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
                if !(rect.width() > 0.0 && rect.height() > 0.0) {
                    continue;
                }
                let transform = Affine::scale(scale)
                    * Affine::translate((rect.x0, rect.y0))
                    * Affine::scale_non_uniform(rect.width() / iw, rect.height() / ih);
                ctx.set_transform(transform);
                ctx.set_paint(Image {
                    image: ImageSource::Pixmap(Arc::new(pixmap)),
                    sampler: ImageSampler {
                        quality: if *interpolate {
                            ImageQuality::Medium
                        } else {
                            ImageQuality::Low
                        },
                        ..Default::default()
                    },
                });
                ctx.fill_rect(&Rect::new(0.0, 0.0, iw, ih));
                ctx.set_transform(Affine::scale(scale));
            }
            Item::PushClip { path, rule } => {
                ctx.set_fill_rule(fill_rule(*rule));
                ctx.push_clip_path(path);
                clips += 1;
            }
            Item::PopClip => {
                if clips > 0 {
                    ctx.pop_clip();
                    clips -= 1;
                }
            }
        }
    }
    ctx.flush();
    let mut pixmap = Pixmap::new(w, h);
    ctx.render(&mut pixmap, &mut Resources::new());
    Ok(pixmap)
}

/// Draws the list and returns the premultiplied pixels.
pub fn render_rgba(list: &DisplayList, scale: f64) -> Result<Rgba, RenderError> {
    let pixmap = render_pixmap(list, scale)?;
    Ok(Rgba {
        width: usize::from(pixmap.width()),
        height: usize::from(pixmap.height()),
        data: pixmap.data_as_u8_slice().to_vec(),
    })
}

/// Draws the list and encodes it as a PNG.
pub fn render_png(list: &DisplayList, scale: f64) -> Result<Vec<u8>, RenderError> {
    render_pixmap(list, scale)?
        .into_png()
        .map_err(|e| RenderError::Png(format!("{e:?}")))
}
