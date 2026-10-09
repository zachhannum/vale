//! The Krilla PDF backend.

use std::collections::HashMap;

use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Transform};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, LineCap, LineJoin, Stroke};
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph};
use kurbo::{BezPath, PathEl};

use crate::list::{Color, DisplayList, FillRule as ListRule, GlyphRun, Item, RenderError};
use crate::raster::glyph_run_outline;

fn opacity(c: Color) -> NormalizedF32 {
    NormalizedF32::new(f32::from(c.a) / 255.0).unwrap_or(NormalizedF32::ONE)
}

fn to_path(path: &BezPath) -> Option<krilla::geom::Path> {
    let mut pb = PathBuilder::new();
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(a, b) => pb.quad_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32),
            PathEl::CurveTo(a, b, c) => pb.cubic_to(
                a.x as f32, a.y as f32, b.x as f32, b.y as f32, c.x as f32, c.y as f32,
            ),
            PathEl::ClosePath => pb.close(),
        }
    }
    pb.finish()
}

fn fill_path(s: &mut Surface, path: &BezPath, color: Color, rule: FillRule) {
    let Some(p) = to_path(path) else { return };
    s.set_stroke(None);
    s.set_fill(Some(Fill {
        paint: rgb::Color::new(color.r, color.g, color.b).into(),
        opacity: opacity(color),
        rule,
    }));
    s.draw_path(&p);
}

/// The cleaned text of a run, when each character has exactly one glyph.
fn clean_text(run: &GlyphRun) -> Option<String> {
    let cleaned = run.text.split_whitespace().collect::<Vec<_>>().join(" ");
    (cleaned.chars().count() == run.glyphs.len()).then_some(cleaned)
}

fn is_straight(run: &GlyphRun) -> bool {
    let Some((_, first)) = run.glyphs.first() else {
        return false;
    };
    let y0 = first.as_coeffs()[5];
    run.glyphs.iter().all(|(_, t)| {
        let c = t.as_coeffs();
        c[0] == 1.0 && c[1] == 0.0 && c[2] == 0.0 && c[3] == 1.0 && (c[5] - y0).abs() < 1e-6
    })
}

fn draw_run(s: &mut Surface, run: &GlyphRun, fonts: &mut HashMap<(u64, u32), Option<Font>>) {
    let font = fonts
        .entry((run.font.data.id(), run.font.index))
        .or_insert_with(|| Font::new(run.font.data.as_ref().to_vec().into(), run.font.index))
        .clone();
    let text = clean_text(run);
    let (Some(font), Some(text), true) = (font, text, run.normalized_coords.is_empty()) else {
        fill_path(s, &glyph_run_outline(run), run.color, FillRule::NonZero);
        return;
    };
    if run.glyphs.is_empty() {
        return;
    }
    s.set_stroke(None);
    s.set_fill(Some(Fill {
        paint: rgb::Color::new(run.color.r, run.color.g, run.color.b).into(),
        opacity: opacity(run.color),
        rule: FillRule::NonZero,
    }));
    let size = run.font_size;
    let ranges: Vec<std::ops::Range<usize>> = text
        .char_indices()
        .map(|(i, c)| i..i + c.len_utf8())
        .collect();
    if is_straight(run) {
        let xs: Vec<f64> = run.glyphs.iter().map(|(_, t)| t.as_coeffs()[4]).collect();
        let y = run.glyphs[0].1.as_coeffs()[5];
        let glyphs: Vec<KrillaGlyph> = run
            .glyphs
            .iter()
            .enumerate()
            .map(|(i, (id, _))| {
                let advance = xs
                    .get(i + 1)
                    .map_or(0.0, |next| (next - xs[i]) / f64::from(size));
                KrillaGlyph::new(
                    GlyphId::new(*id),
                    advance as f32,
                    0.0,
                    0.0,
                    0.0,
                    ranges[i].clone(),
                    None,
                )
            })
            .collect();
        s.draw_glyphs(
            Point::from_xy(xs[0] as f32, y as f32),
            &glyphs,
            font,
            &text,
            size,
            false,
        );
    } else {
        for (i, (id, t)) in run.glyphs.iter().enumerate() {
            let c = t.as_coeffs().map(|v| v as f32);
            s.push_transform(&Transform::from_row(c[0], c[1], c[2], c[3], c[4], c[5]));
            let glyph = KrillaGlyph::new(
                GlyphId::new(*id),
                0.0,
                0.0,
                0.0,
                0.0,
                ranges[i].clone(),
                None,
            );
            s.draw_glyphs(
                Point::from_xy(0.0, 0.0),
                &[glyph],
                font.clone(),
                &text,
                size,
                false,
            );
            s.pop();
        }
    }
}

/// Draws the list as one PDF page of `width` by `height` points.
pub fn render_pdf(list: &DisplayList) -> Result<Vec<u8>, RenderError> {
    let settings = PageSettings::from_wh(list.width as f32, list.height as f32)
        .ok_or(RenderError::EmptyPage)?;
    let mut doc = krilla::Document::new();
    let mut page = doc.start_page_with(settings);
    let mut s = page.surface();
    let mut fonts = HashMap::new();
    for item in &list.items {
        match item {
            Item::Fill { path, color, rule } => fill_path(
                &mut s,
                path,
                *color,
                match rule {
                    ListRule::NonZero => FillRule::NonZero,
                    ListRule::EvenOdd => FillRule::EvenOdd,
                },
            ),
            Item::Stroke {
                path,
                color,
                width,
                round,
            } => {
                let Some(p) = to_path(path) else { continue };
                s.set_fill(None);
                s.set_stroke(Some(Stroke {
                    paint: rgb::Color::new(color.r, color.g, color.b).into(),
                    width: *width as f32,
                    opacity: opacity(*color),
                    line_cap: if *round {
                        LineCap::Round
                    } else {
                        LineCap::Butt
                    },
                    line_join: if *round {
                        LineJoin::Round
                    } else {
                        LineJoin::Miter
                    },
                    ..Default::default()
                }));
                s.draw_path(&p);
            }
            Item::Glyphs(run) => draw_run(&mut s, run, &mut fonts),
        }
    }
    s.finish();
    page.finish();
    doc.finish().map_err(|e| RenderError::Pdf(format!("{e:?}")))
}
