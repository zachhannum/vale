//! The rendering of the UI without a window, for tests and screenshots.

/// The test renderer of egui_kittest, with two changes: the image covers
/// the whole screen, and the render passes split at the backdrop markers.
/// The image of egui_kittest has the size of the safe area, and it starts at
/// the top-left corner of the screen.
pub struct ScreenRenderer {
    inner: egui_kittest::wgpu::WgpuTestRenderer,
    render_state: eframe::egui_wgpu::RenderState,
}

impl ScreenRenderer {
    pub fn new(render_state: eframe::egui_wgpu::RenderState) -> ScreenRenderer {
        ScreenRenderer {
            inner: egui_kittest::wgpu::WgpuTestRenderer::from_render_state(render_state.clone()),
            render_state,
        }
    }
}

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
            // The hook of the backdrop reads the texture.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let target = eframe::egui_wgpu::BackdropTarget {
            view: &view,
            resolve_target: None,
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            depth_stencil: None,
        };
        renderer.render_with_backdrops(device, queue, &mut encoder, &target, &shapes, &screen);
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
                timeout: Some(std::time::Duration::from_secs(120)),
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
