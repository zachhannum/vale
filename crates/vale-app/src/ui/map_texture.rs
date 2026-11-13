//! Textures of display lists on the screen. The GPU Vello backend draws them in the window, and
//! `vello_cpu` draws them when the app has no GPU renderer, as in the headless commands.

use std::sync::Arc;

use eframe::egui;
use eframe::egui::mutex::RwLock;
use eframe::egui_wgpu::{self, wgpu};
use vale_render::gpu::GpuRenderer;
use vale_render::{DisplayList, PreparedList, Rgba};

/// The GPU renderer of the display lists, on the device of egui.
pub struct MapRenderer {
    renderer: GpuRenderer,
    device: wgpu::Device,
    queue: wgpu::Queue,
    egui: Arc<RwLock<egui_wgpu::Renderer>>,
}

impl MapRenderer {
    /// A renderer on the device of egui, or `None` when the device has no compute shaders.
    pub fn new(render_state: &egui_wgpu::RenderState) -> Option<MapRenderer> {
        let flags = render_state.adapter.get_downlevel_capabilities().flags;
        if !flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
            return None;
        }
        let renderer = GpuRenderer::new(&render_state.device).ok()?;
        Some(MapRenderer {
            renderer,
            device: render_state.device.clone(),
            queue: render_state.queue.clone(),
            egui: render_state.renderer.clone(),
        })
    }

    /// Makes a new renderer, so that the buffers of the old one go away.
    /// The shaders compile again.
    pub fn renew(&mut self) {
        if let Ok(renderer) = GpuRenderer::new(&self.device) {
            self.renderer = renderer;
        }
    }
}

/// A texture of a display list.
pub enum MapTexture {
    Cpu(egui::TextureHandle),
    Gpu(GpuTexture),
}

impl MapTexture {
    pub fn id(&self) -> egui::TextureId {
        match self {
            MapTexture::Cpu(t) => t.id(),
            MapTexture::Gpu(t) => t.id,
        }
    }

    /// The size in pixels.
    pub fn size(&self) -> [usize; 2] {
        match self {
            MapTexture::Cpu(t) => t.size(),
            MapTexture::Gpu(t) => [t.texture.width() as usize, t.texture.height() as usize],
        }
    }
}

/// A texture that the GPU renderer draws, registered with egui.
pub struct GpuTexture {
    texture: wgpu::Texture,
    id: egui::TextureId,
    egui: Arc<RwLock<egui_wgpu::Renderer>>,
}

impl Drop for GpuTexture {
    fn drop(&mut self) {
        self.egui.write().free_texture(&self.id);
    }
}

/// Draws the list at `scale` pixels per point to `old` when it has the right size, or to a new
/// texture. Returns the texture, and the pixels when the CPU drew them.
pub fn draw(
    ctx: &egui::Context,
    renderer: Option<&mut MapRenderer>,
    old: Option<MapTexture>,
    name: impl FnOnce() -> String,
    list: &DisplayList,
    scale: f64,
) -> Result<(MapTexture, Option<Rgba>), String> {
    let Some(r) = renderer else {
        let img = vale_render::render_rgba(list, scale).map_err(|e| format!("{e}"))?;
        let image = egui::ColorImage::from_rgba_premultiplied([img.width, img.height], &img.data);
        let options = egui::TextureOptions::LINEAR;
        let texture = match old {
            Some(MapTexture::Cpu(mut t)) => {
                t.set(image, options);
                t
            }
            _ => ctx.load_texture(name(), image, options),
        };
        return Ok((MapTexture::Cpu(texture), Some(img)));
    };
    let (w, h) = vale_render::pixel_size(list, scale).map_err(|e| format!("{e}"))?;
    let texture = match old {
        Some(MapTexture::Gpu(t)) if t.texture.size() == extent(w, h) => t,
        _ => {
            let texture =
                GpuRenderer::target(&r.device, list, scale).map_err(|e| format!("{e}"))?;
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let id =
                r.egui
                    .write()
                    .register_native_texture(&r.device, &view, wgpu::FilterMode::Linear);
            GpuTexture {
                texture,
                id,
                egui: r.egui.clone(),
            }
        }
    };
    r.renderer
        .render(&r.device, &r.queue, list, scale, &texture.texture)
        .map_err(|e| format!("{e}"))?;
    Ok((MapTexture::Gpu(texture), None))
}

/// Draws the part of a prepared list that falls in `size` pixels, where the page point `p` lands
/// on the pixel `transform * p`. Draws to `old` when it has that size, so that the GPU registers
/// no new texture with egui, or else to a new texture.
pub fn draw_region(
    ctx: &egui::Context,
    renderer: Option<&mut MapRenderer>,
    old: Option<MapTexture>,
    list: &PreparedList,
    transform: kurbo::Affine,
    [w, h]: [u32; 2],
) -> Result<MapTexture, String> {
    let Some(r) = renderer else {
        let (img, _) =
            vale_render::render_region(list, transform, w, h).map_err(|e| format!("{e}"))?;
        let image = egui::ColorImage::from_rgba_premultiplied([img.width, img.height], &img.data);
        let options = egui::TextureOptions::LINEAR;
        let texture = match old {
            Some(MapTexture::Cpu(mut t)) if t.size() == [img.width, img.height] => {
                t.set(image, options);
                t
            }
            _ => ctx.load_texture("frame-tile", image, options),
        };
        return Ok(MapTexture::Cpu(texture));
    };
    let texture = match old {
        Some(MapTexture::Gpu(t)) if (t.texture.width(), t.texture.height()) == (w, h) => t,
        _ => {
            let texture = GpuRenderer::target_sized(&r.device, w, h);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let id =
                r.egui
                    .write()
                    .register_native_texture(&r.device, &view, wgpu::FilterMode::Linear);
            GpuTexture {
                texture,
                id,
                egui: r.egui.clone(),
            }
        }
    };
    r.renderer
        .render_region(&r.device, &r.queue, list, transform, &texture.texture)
        .map_err(|e| format!("{e}"))?;
    Ok(MapTexture::Gpu(texture))
}

fn extent(w: u16, h: u16) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: w.into(),
        height: h.into(),
        depth_or_array_layers: 1,
    }
}
