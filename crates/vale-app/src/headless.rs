//! Screenshots and export without a window.

use std::path::Path;

use anyhow::{Context, anyhow, bail};
use kurbo::Point;
use vale_render::{render_pdf, render_pixmap};
use vale_sphere::LonLat;

use crate::document::Document;
use crate::pipeline::{Composed, Pipeline, Quality, fonts};

pub struct MapImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub composed: Composed,
}

fn compose_final(doc: &Document, size: (f64, f64)) -> anyhow::Result<(Pipeline, Composed)> {
    let mut pipeline = Pipeline::new();
    let mut fonts = fonts()?;
    let composed = pipeline.compose(doc, size, &mut fonts, Quality::Final, None)?;
    Ok((pipeline, composed))
}

/// Renders the map alone.
pub fn map_png(doc: &Document, size: (f64, f64), pixel_ratio: f64) -> anyhow::Result<MapImage> {
    let (_, composed) = compose_final(doc, size)?;
    let pixmap = render_pixmap(&composed.list, pixel_ratio)?;
    let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    let png = pixmap.into_png().map_err(|e| anyhow!("PNG error: {e:?}"))?;
    Ok(MapImage {
        png,
        width,
        height,
        composed,
    })
}

/// Writes the map to a `.png` or `.pdf` file.
pub fn export(
    doc: &Document,
    size: (f64, f64),
    pixel_ratio: f64,
    path: &Path,
) -> anyhow::Result<Composed> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("png") => {
            let image = map_png(doc, size, pixel_ratio)?;
            write_file(path, &image.png)?;
            Ok(image.composed)
        }
        Some("pdf") => {
            let (_, composed) = compose_final(doc, size)?;
            let bytes = render_pdf(&composed.list)?;
            write_file(path, &bytes)?;
            Ok(composed)
        }
        _ => bail!(
            "cannot export to {}; use a .png or .pdf file",
            path.display()
        ),
    }
}

/// The page position of a place and the color of the map pixel there.
pub fn probe(
    doc: &Document,
    size: (f64, f64),
    pixel_ratio: f64,
    at: LonLat,
) -> anyhow::Result<Option<(Point, [u8; 3])>> {
    let (mut pipeline, composed) = compose_final(doc, size)?;
    let Some(pos) = pipeline.lonlat_to_page(doc, size, at)? else {
        return Ok(None);
    };
    if !(pos.x >= 0.0 && pos.y >= 0.0 && pos.x < size.0 && pos.y < size.1) {
        return Ok(None);
    }
    let pixmap = render_pixmap(&composed.list, pixel_ratio)?;
    let (w, h) = (usize::from(pixmap.width()), usize::from(pixmap.height()));
    let x = ((pos.x * pixel_ratio).floor() as usize).min(w - 1);
    let y = ((pos.y * pixel_ratio).floor() as usize).min(h - 1);
    let data = pixmap.data_as_u8_slice();
    let i = (y * w + x) * 4;
    Ok(Some((pos, [data[i], data[i + 1], data[i + 2]])))
}

/// The text of `--report`.
pub fn report(doc: &Document, composed: &Composed) -> String {
    let spec = doc.frame.projection;
    let mut out = String::new();
    out.push_str(&format!(
        "world: {}, radius {} km\n",
        doc.project.world.name, doc.project.world.radius_km
    ));
    out.push_str(&format!(
        "projection: {}, center {},{}\n",
        spec.kind.name(),
        spec.lon0,
        spec.lat0
    ));
    for entry in doc.frame.entries.iter().rev() {
        let Some(layer) = doc.layer(entry.layer) else {
            continue;
        };
        out.push_str(&format!(
            "layer {}: {}, {} features, {}\n",
            layer.name,
            layer.kind.name(),
            layer.features.len(),
            if entry.visible { "visible" } else { "hidden" }
        ));
    }
    out.push_str(&format!(
        "labels: {} placed, {} unplaced\n",
        composed.labels.placed,
        composed.labels.unplaced.len()
    ));
    out.push_str(&format!("scale: 1 pt = {:.1} km\n", composed.km_per_point));
    out
}

/// Writes a file and creates its parent directories.
pub fn write_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

/// Renders the whole UI offscreen. Returns the image and the state.
#[cfg(not(target_os = "ios"))]
pub fn ui_png(
    mut state: crate::ui::AppState,
    size: (f64, f64),
    pixel_ratio: f64,
) -> anyhow::Result<(image::RgbaImage, crate::ui::AppState)> {
    state.headless = true;
    let render_state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        eframe::egui_wgpu::RendererOptions::PREDICTABLE,
    );
    state.globe.format = Some(render_state.target_format);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(size.0 as f32, size.1 as f32))
        .with_pixels_per_point(pixel_ratio as f32)
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(
            render_state,
        ))
        .build_ui_state(
            |ui, state: &mut crate::ui::AppState| crate::ui::draw(ui, state),
            state,
        );
    harness.run_steps(4);
    let image = harness.render().map_err(|e| {
        anyhow!("cannot render the UI offscreen (no GPU adapter?): {e}. Use --map-only for a CPU render.")
    })?;
    Ok((image, harness.into_state()))
}
