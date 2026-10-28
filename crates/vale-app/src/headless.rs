//! Screenshots and export without a window.

use std::path::Path;

use anyhow::{Context, anyhow, bail};
use kurbo::Point;
use vale_render::{render_pdf, render_pixmap};
use vale_sphere::LonLat;

use vale_labeler::Fonts;
use vale_store::PageRecord;

use crate::document::Document;
use crate::pipeline::{Composed, Pipeline, Quality, fonts};

/// The pixels of a PNG export of a page per point: 300 pixels per inch.
pub const PAGE_PIXEL_RATIO: f64 = 300.0 / 72.0;

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

/// Exports a page to a PNG or a PDF file, by the extension of the file.
pub fn export_page(
    pipeline: &mut Pipeline,
    doc: &Document,
    page: &PageRecord,
    fonts: &mut Fonts,
    path: &Path,
) -> anyhow::Result<()> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let png = match ext.as_deref() {
        Some("png") => true,
        Some("pdf") => false,
        _ => bail!(
            "cannot export to {}; use a .png or .pdf file",
            path.display()
        ),
    };
    let list = pipeline.compose_page(doc, page, fonts, Quality::Final)?;
    let bytes = if png {
        let pixmap = render_pixmap(&list, PAGE_PIXEL_RATIO)?;
        pixmap.into_png().map_err(|e| anyhow!("PNG error: {e:?}"))?
    } else {
        render_pdf(&list)?
    };
    write_file(path, &bytes)
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
    let spec = doc.projection(doc.first_map());
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
            if layer.visible { "visible" } else { "hidden" }
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

/// The UI on an offscreen wgpu renderer. `setup` picks the GPU adapter.
#[cfg(not(target_os = "ios"))]
pub fn ui_harness(
    state: crate::ui::AppState,
    size: (f64, f64),
    pixel_ratio: f64,
    setup: eframe::egui_wgpu::WgpuSetup,
) -> egui_kittest::Harness<'static, crate::ui::AppState> {
    let render_state = egui_kittest::wgpu::create_render_state(
        setup,
        eframe::egui_wgpu::RendererOptions::PREDICTABLE,
    );
    ui_harness_on(state, size, pixel_ratio, render_state)
}

/// The UI on an offscreen wgpu renderer with this render state.
#[cfg(not(target_os = "ios"))]
pub fn ui_harness_on(
    mut state: crate::ui::AppState,
    size: (f64, f64),
    pixel_ratio: f64,
    render_state: eframe::egui_wgpu::RenderState,
) -> egui_kittest::Harness<'static, crate::ui::AppState> {
    state.headless = true;
    // The rivers of a change show in the next frame, so an image does not
    // depend on the time.
    state.globe.set_rivers_sync(true);
    state.globe.attach(&render_state);
    egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(size.0 as f32, size.1 as f32))
        .with_pixels_per_point(pixel_ratio as f32)
        .renderer(ScreenRenderer::new(render_state))
        .build_ui_state(
            |ui, state: &mut crate::ui::AppState| crate::ui::draw(ui, state),
            state,
        )
}

/// The test renderer of egui_kittest, with one change: the image covers
/// the whole screen. The image of egui_kittest has the size of the safe
/// area, and it starts at the top-left corner of the screen.
#[cfg(not(target_os = "ios"))]
struct ScreenRenderer {
    inner: egui_kittest::wgpu::WgpuTestRenderer,
    render_state: eframe::egui_wgpu::RenderState,
}

#[cfg(not(target_os = "ios"))]
impl ScreenRenderer {
    fn new(render_state: eframe::egui_wgpu::RenderState) -> ScreenRenderer {
        ScreenRenderer {
            inner: egui_kittest::wgpu::WgpuTestRenderer::from_render_state(render_state.clone()),
            render_state,
        }
    }
}

#[cfg(not(target_os = "ios"))]
impl egui_kittest::TestRenderer for ScreenRenderer {
    fn handle_delta(&mut self, delta: &mut eframe::egui::TexturesDelta) {
        self.inner.handle_delta(delta);
    }

    fn render(
        &mut self,
        ctx: &eframe::egui::Context,
        output: &eframe::egui::FullOutput,
    ) -> Result<image::RgbaImage, String> {
        use eframe::egui_wgpu::{ScreenDescriptor, wgpu};
        let eframe::egui_wgpu::RenderState {
            device,
            queue,
            target_format,
            ..
        } = &self.render_state;
        let ppp = ctx.pixels_per_point();
        let size = ctx.viewport_rect().size() * ppp;
        let (width, height) = (size.x.round() as u32, size.y.round() as u32);
        let screen = ScreenDescriptor {
            pixels_per_point: ppp,
            size_in_pixels: [width, height],
        };
        let shapes = ctx.tessellate(output.shapes.clone(), ppp);
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut renderer = self.render_state.renderer.write();
        let buffers = renderer.update_buffers(device, queue, &mut encoder, &shapes, &screen);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: *target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            renderer.render(&mut pass, &shapes, &screen);
        }
        drop(renderer);
        // Each row of the copy is a multiple of 256 bytes.
        let row = 4 * width;
        let padded =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen"),
            size: u64::from(padded * height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: None,
                },
            },
            texture.size(),
        );
        let index = queue.submit(buffers.into_iter().chain([encoder.finish()]));
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| drop(sender.send(r)));
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .map_err(|e| format!("PollError: {e}"))?;
        receiver
            .recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let data = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| e.to_string())?;
        let pixels = data
            .chunks_exact(padded as usize)
            .flat_map(|r| &r[..row as usize])
            .copied()
            .collect();
        image::RgbaImage::from_raw(width, height, pixels).ok_or_else(|| "bad image".to_string())
    }
}

/// A wgpu setup that takes a hardware GPU before a software adapter.
#[cfg(not(target_os = "ios"))]
fn hardware_setup() -> eframe::egui_wgpu::WgpuSetup {
    use eframe::egui_wgpu::{WgpuSetup, wgpu};
    let WgpuSetup::CreateNew(mut setup) = egui_kittest::wgpu::default_wgpu_setup() else {
        unreachable!("the default setup makes a new instance");
    };
    setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters, _surface| {
        let rank = |adapter: &&wgpu::Adapter| match adapter.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => 0,
            wgpu::DeviceType::IntegratedGpu => 1,
            wgpu::DeviceType::VirtualGpu | wgpu::DeviceType::Other => 2,
            wgpu::DeviceType::Cpu => 3,
        };
        let best = adapters.iter().min_by_key(rank);
        best.cloned().ok_or_else(|| "No adapter found".to_owned())
    }));
    WgpuSetup::CreateNew(setup)
}

/// The longest time that the stroke test can take.
#[cfg(not(target_os = "ios"))]
const STROKE_TEST_LIMIT: std::time::Duration = std::time::Duration::from_secs(300);

/// Runs the stroke test of the globe offscreen. Returns the text with the
/// stroke delay. The frames run one after the other, with no display to
/// wait for.
#[cfg(not(target_os = "ios"))]
pub fn stroke_test(
    mut state: crate::ui::AppState,
    size: (f64, f64),
    pixel_ratio: f64,
) -> anyhow::Result<String> {
    state.workspace = crate::ui::Workspace::Globe;
    let mut harness = ui_harness(state, size, pixel_ratio, hardware_setup());
    // The app in a window computes the rivers on a worker thread.
    harness.state_mut().globe.set_rivers_sync(false);
    let no_gpu = |e| anyhow!("cannot render the UI offscreen (no GPU adapter?): {e}");
    // The first frames set the size of the canvas and clear the heightmap.
    for _ in 0..3 {
        harness.step();
        harness.render().map_err(no_gpu)?;
    }
    // The test paints a height layer.
    harness.state_mut().select_height();
    let globe = &mut harness.state_mut().globe;
    globe.start_stroke_test(crate::globe::stroke_test::SECONDS);
    let start = std::time::Instant::now();
    while harness.state().globe.busy() {
        if start.elapsed() > STROKE_TEST_LIMIT {
            bail!("the stroke test did not end");
        }
        harness.step();
        harness.render().map_err(no_gpu)?;
    }
    let report = harness.state_mut().globe.test_report.take();
    report.ok_or_else(|| anyhow!("the stroke test gave no numbers"))
}

/// Runs the zoom test of the globe offscreen. Returns the text with the
/// frame times. The frames run one after the other, with no display to
/// wait for.
#[cfg(not(target_os = "ios"))]
pub fn zoom_test(
    mut state: crate::ui::AppState,
    size: (f64, f64),
    pixel_ratio: f64,
) -> anyhow::Result<String> {
    state.workspace = crate::ui::Workspace::Globe;
    let mut harness = ui_harness(state, size, pixel_ratio, hardware_setup());
    let no_gpu = |e| anyhow!("cannot render the UI offscreen (no GPU adapter?): {e}");
    for _ in 0..3 {
        harness.step();
        harness.render().map_err(no_gpu)?;
    }
    // The test starts when the heightmap is on the GPU.
    for _ in 0..UPLOAD_STEPS {
        let globe = &harness.state().globe;
        if !globe.busy() && !globe.loading() {
            break;
        }
        harness.step();
        harness.render().map_err(no_gpu)?;
    }
    let globe = &mut harness.state_mut().globe;
    globe.start_zoom_test(crate::globe::zoom_test::SECONDS);
    let start = std::time::Instant::now();
    while harness.state().globe.zoom_testing() {
        if start.elapsed() > STROKE_TEST_LIMIT {
            bail!("the zoom test did not end");
        }
        harness.step();
        harness.render().map_err(no_gpu)?;
    }
    let report = harness.state_mut().globe.zoom_report.take();
    report.ok_or_else(|| anyhow!("the zoom test gave no numbers"))
}

/// The most frames that a screenshot waits for the globe.
#[cfg(not(target_os = "ios"))]
const UPLOAD_STEPS: usize = 10_000;

/// Renders the whole UI offscreen. Returns the image and the state.
#[cfg(not(target_os = "ios"))]
pub fn ui_png(
    state: crate::ui::AppState,
    size: (f64, f64),
    pixel_ratio: f64,
) -> anyhow::Result<(image::RgbaImage, crate::ui::AppState)> {
    ui_png_with(state, size, pixel_ratio, |_| Ok(()))
}

/// Renders the whole UI offscreen. `prepare` changes the state before the
/// first frame. The globe has its renderer then, so its face size is final.
#[cfg(not(target_os = "ios"))]
pub fn ui_png_with(
    state: crate::ui::AppState,
    size: (f64, f64),
    pixel_ratio: f64,
    prepare: impl FnOnce(&mut crate::ui::AppState) -> anyhow::Result<()>,
) -> anyhow::Result<(image::RgbaImage, crate::ui::AppState)> {
    ui_png_safe(state, size, pixel_ratio, None, prepare)
}

/// Renders the whole UI offscreen, as `ui_png_with` does, with the safe
/// area of a device.
#[cfg(not(target_os = "ios"))]
pub fn ui_png_safe(
    state: crate::ui::AppState,
    size: (f64, f64),
    pixel_ratio: f64,
    safe_area: Option<eframe::egui::SafeAreaInsets>,
    prepare: impl FnOnce(&mut crate::ui::AppState) -> anyhow::Result<()>,
) -> anyhow::Result<(image::RgbaImage, crate::ui::AppState)> {
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let mut harness = ui_harness(state, size, pixel_ratio, setup);
    harness.input_mut().safe_area_insets = safe_area;
    prepare(harness.state_mut())?;
    harness.run_steps(4);
    let no_gpu = |e| {
        anyhow!(
            "cannot render the UI offscreen (no GPU adapter?): {e}. Use --map-only for a CPU render."
        )
    };
    // The heightmap goes to the GPU in parts. Each render sends one part.
    for _ in 0..UPLOAD_STEPS {
        let globe = &harness.state().globe;
        if !globe.busy() && !globe.loading() {
            break;
        }
        harness.step();
        harness.render().map_err(no_gpu)?;
    }
    let image = harness.render().map_err(no_gpu)?;
    Ok((image, harness.into_state()))
}
