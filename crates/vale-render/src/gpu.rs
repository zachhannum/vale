//! The GPU Vello backend, for the screen.
//!
//! Vello has buffers of a fixed size, so a large list is drawn in batches. Each batch goes to a
//! scratch texture, and a blit adds it over the target with premultiplied alpha.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Cursor;
use std::num::NonZeroUsize;

use kurbo::{Affine, BezPath, Cap, Join, Rect, Shape, Stroke};
use vello::peniko::{
    Blob, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat, ImageQuality, ImageSampler,
};
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};
use vello_cpu::Pixmap;

use crate::list::{Color, DisplayList, FillRule, Item, RenderError};
use crate::raster::{Rgba, glyph_run_outline, pixel_size};

/// The format of a target. A view in `Rgba8UnormSrgb` samples the same pixels as sRGB.
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The most that one batch puts in a fixed buffer of Vello. Vello 0.11 has room for 2^21
/// lines, segments, and tiles, and for 2^18 bin entries. The limits leave room for the error
/// of the estimate.
#[derive(Copy, Clone, Debug)]
pub struct Limits {
    pub lines: u32,
    pub segments: u32,
    /// Tiles of 16 by 16 pixels.
    pub tiles: u64,
    /// Bins of 256 by 256 pixels.
    pub bins: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            lines: 1 << 20,
            segments: 1 << 20,
            tiles: 1 << 20,
            bins: 1 << 17,
        }
    }
}

const BLIT: &str = "
@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) p: vec4f) -> @location(0) vec4f {
    let c = textureLoad(source, vec2i(p.xy), 0);
    return vec4f(c.rgb * c.a, c.a);
}
";

/// Draws display lists to textures with Vello.
pub struct GpuRenderer {
    renderer: Renderer,
    blit: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    scratch: Option<wgpu::Texture>,
    images: Images,
    limits: Limits,
}

impl GpuRenderer {
    /// Compiles the shaders. The device must support compute shaders.
    pub fn new(device: &wgpu::Device) -> Result<Self, RenderError> {
        let renderer = Renderer::new(
            device,
            RendererOptions {
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: if cfg!(target_vendor = "apple") {
                    NonZeroUsize::new(1)
                } else {
                    None
                },
                ..Default::default()
            },
        )
        .map_err(|e| RenderError::Gpu(e.to_string()))?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vale-render blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vale-render blit"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vale-render blit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vale-render blit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: TARGET_FORMAT,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Ok(GpuRenderer {
            renderer,
            blit,
            layout,
            scratch: None,
            images: Images::default(),
            limits: Limits::default(),
        })
    }

    /// Sets the size of a batch.
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }

    /// A target for the list at `scale` pixels per point.
    pub fn target(
        device: &wgpu::Device,
        list: &DisplayList,
        scale: f64,
    ) -> Result<wgpu::Texture, RenderError> {
        let (w, h) = pixel_size(list, scale)?;
        Ok(device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vale-render target"),
            size: extent(w.into(), h.into()),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        }))
    }

    /// Draws the list at `scale` pixels per point to `target`, in premultiplied pixels, and
    /// returns the number of batches. The target has the size of the list at the scale.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        list: &DisplayList,
        scale: f64,
        target: &wgpu::Texture,
    ) -> Result<usize, RenderError> {
        let (w, h) = pixel_size(list, scale)?;
        let (w, h) = (u32::from(w), u32::from(h));
        if (target.width(), target.height()) != (w, h) || target.format() != TARGET_FORMAT {
            return Err(RenderError::Gpu(
                "the target does not match the list".into(),
            ));
        }
        let mut encoder = Encoder::new(
            Affine::scale(scale),
            Rect::new(0.0, 0.0, f64::from(w), f64::from(h)),
            self.limits,
            &mut self.images,
        );
        encoder.items(&list.items)?;
        let scenes = encoder.finish();
        self.images.sweep();

        let scratch = match self.scratch.take() {
            Some(t) if (t.width(), t.height()) == (w, h) => t,
            _ => device.create_texture(&wgpu::TextureDescriptor {
                label: Some("vale-render scratch"),
                size: extent(w, h),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }),
        };
        let source = scratch.create_view(&wgpu::TextureViewDescriptor::default());
        let view = target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(TARGET_FORMAT),
            ..Default::default()
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vale-render blit"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&source),
            }],
        });
        let params = RenderParams {
            base_color: vello::peniko::Color::TRANSPARENT,
            width: w,
            height: h,
            antialiasing_method: AaConfig::Area,
        };
        for (i, scene) in scenes.iter().enumerate() {
            self.renderer
                .render_to_texture(device, queue, scene, &source, &params)
                .map_err(|e| RenderError::Gpu(e.to_string()))?;
            let load = if i == 0 {
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
            } else {
                wgpu::LoadOp::Load
            };
            let mut commands = device.create_command_encoder(&Default::default());
            {
                let mut pass = commands.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("vale-render blit"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&self.blit);
                pass.set_bind_group(0, &bind, &[]);
                pass.draw(0..3, 0..1);
            }
            queue.submit([commands.finish()]);
        }
        self.scratch = Some(scratch);
        Ok(scenes.len())
    }
}

/// Reads the premultiplied pixels of a target. This waits for the GPU.
pub fn read_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::Texture,
) -> Result<Rgba, RenderError> {
    let (w, h) = (target.width(), target.height());
    let row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vale-render readback"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut commands = device.create_command_encoder(&Default::default());
    commands.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        target.size(),
    );
    queue.submit([commands.finish()]);
    buffer.map_async(wgpu::MapMode::Read, .., |_| {});
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .map_err(|e| RenderError::Gpu(e.to_string()))?;
    let bytes = buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|e| RenderError::Gpu(e.to_string()))?;
    let data = bytes
        .chunks(row as usize)
        .flat_map(|r| &r[..w as usize * 4])
        .copied()
        .collect();
    Ok(Rgba {
        width: w as usize,
        height: h as usize,
        data,
    })
}

fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

fn paint(c: Color) -> vello::peniko::Color {
    vello::peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn fill_rule(rule: FillRule) -> Fill {
    match rule {
        FillRule::NonZero => Fill::NonZero,
        FillRule::EvenOdd => Fill::EvenOdd,
    }
}

/// The decoded images of the last render, by the hash of the PNG. Vello 0.11 can lose an image
/// that stays in its atlas from one render to the next, so each draw gives the pixels a new blob.
#[derive(Default)]
struct Images {
    map: HashMap<u64, (ImageData, bool)>,
}

impl Images {
    fn get(&mut self, png: &[u8]) -> Result<ImageData, RenderError> {
        let mut hasher = DefaultHasher::new();
        png.hash(&mut hasher);
        let key = hasher.finish();
        let image = match self.map.get_mut(&key) {
            Some((image, used)) => {
                *used = true;
                image.clone()
            }
            None => {
                let pixmap = Pixmap::from_png(Cursor::new(png))
                    .map_err(|e| RenderError::Png(format!("{e:?}")))?;
                let image = ImageData {
                    data: Blob::from(pixmap.data_as_u8_slice().to_vec()),
                    format: ImageFormat::Rgba8,
                    alpha_type: ImageAlphaType::AlphaPremultiplied,
                    width: pixmap.width().into(),
                    height: pixmap.height().into(),
                };
                self.map.insert(key, (image.clone(), true));
                image
            }
        };
        Ok(ImageData {
            data: Blob::new(image.data.into_raw_parts().0),
            ..image
        })
    }

    /// Drops the images that the last render did not use.
    fn sweep(&mut self) {
        self.map.retain(|_, (_, used)| std::mem::take(used));
    }
}

/// Splits a list into scenes that fit the buffers of Vello.
struct Encoder<'a> {
    base: Affine,
    viewport: Rect,
    limits: Limits,
    images: &'a mut Images,
    scenes: Vec<Scene>,
    scene: Scene,
    tiles: u64,
    bins: u64,
    clips: Vec<(&'a BezPath, Fill)>,
}

impl<'a> Encoder<'a> {
    fn new(base: Affine, viewport: Rect, limits: Limits, images: &'a mut Images) -> Self {
        Encoder {
            base,
            viewport,
            limits,
            images,
            scenes: Vec::new(),
            scene: Scene::new(),
            tiles: 0,
            bins: 0,
            clips: Vec::new(),
        }
    }

    /// Counts the tiles and bins of a shape with the page-space bounds `bounds`.
    fn cover(&mut self, bounds: Rect) {
        let r = self
            .base
            .transform_rect_bbox(bounds)
            .intersect(self.viewport);
        if !(r.width() >= 0.0 && r.height() >= 0.0) {
            return;
        }
        let cells = |side: f64| {
            let x = ((r.x1 / side).ceil() - (r.x0 / side).floor()).max(1.0);
            let y = ((r.y1 / side).ceil() - (r.y0 / side).floor()).max(1.0);
            (x * y) as u64
        };
        self.tiles += cells(16.0);
        self.bins += cells(256.0);
    }

    /// Starts a new scene if this one is full. The new scene opens the same clips.
    fn check(&mut self) {
        let m = self.scene.bump_estimate(None);
        let full = self.tiles > self.limits.tiles
            || self.bins > self.limits.bins
            || m.lines.len() > self.limits.lines
            || m.segments.len() > self.limits.segments;
        if !full {
            return;
        }
        for _ in &self.clips {
            self.scene.pop_layer();
        }
        self.scenes.push(std::mem::take(&mut self.scene));
        self.tiles = 0;
        self.bins = 0;
        for i in 0..self.clips.len() {
            let (path, fill) = self.clips[i];
            self.scene.push_clip_layer(fill, self.base, path);
            self.cover(path.bounding_box());
        }
    }

    /// Adds the items, and closes the clips that they leave open.
    fn items(&mut self, items: &'a [Item]) -> Result<(), RenderError> {
        let floor = self.clips.len();
        for item in items {
            match item {
                Item::Fill { path, color, rule } => {
                    self.scene
                        .fill(fill_rule(*rule), self.base, paint(*color), None, path);
                    self.cover(path.bounding_box());
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
                    self.scene
                        .stroke(&stroke, self.base, paint(*color), None, path);
                    let half = width / 2.0;
                    self.cover(path.bounding_box().inflate(half, half));
                }
                Item::Glyphs(run) => {
                    let path = glyph_run_outline(run);
                    self.scene
                        .fill(Fill::NonZero, self.base, paint(run.color), None, &path);
                    self.cover(path.bounding_box());
                }
                Item::Image {
                    png,
                    rect,
                    interpolate,
                } => {
                    let image = self.images.get(png)?;
                    if !(rect.width() > 0.0 && rect.height() > 0.0) {
                        continue;
                    }
                    let (iw, ih) = (f64::from(image.width), f64::from(image.height));
                    let transform = self.base
                        * Affine::translate((rect.x0, rect.y0))
                        * Affine::scale_non_uniform(rect.width() / iw, rect.height() / ih);
                    let brush = ImageBrush {
                        image,
                        sampler: ImageSampler {
                            quality: if *interpolate {
                                ImageQuality::Medium
                            } else {
                                ImageQuality::Low
                            },
                            ..Default::default()
                        },
                    };
                    self.scene.draw_image(&brush, transform);
                    self.cover(*rect);
                }
                Item::Rasterize { items, .. } => self.items(items)?,
                Item::PushClip { path, rule } => {
                    let fill = fill_rule(*rule);
                    self.scene.push_clip_layer(fill, self.base, path);
                    self.clips.push((path, fill));
                    self.cover(path.bounding_box());
                }
                Item::PopClip => {
                    if self.clips.len() > floor {
                        self.scene.pop_layer();
                        self.clips.pop();
                    }
                }
            }
            self.check();
        }
        while self.clips.len() > floor {
            self.scene.pop_layer();
            self.clips.pop();
        }
        Ok(())
    }

    fn finish(mut self) -> Vec<Scene> {
        self.scenes.push(self.scene);
        self.scenes
    }
}
