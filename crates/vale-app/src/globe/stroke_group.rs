//! Translucent strokes on the globe. The opaque shapes of a stroke go to a
//! target of their own, and the target goes to the screen with the alpha of
//! the stroke. Where the pieces of a stroke overlap, at a corner, a crossing,
//! a dash, or a dab, the stroke does not show darker.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use eframe::egui::{self, epaint};
use eframe::egui_wgpu::{self, wgpu};

/// The longest time that an unused target stays.
const TARGET_LIFE: Duration = Duration::from_secs(2);

/// A target grows in steps of this many pixels, so that a small change of
/// the size can use the same texture.
const TARGET_STEP: u32 = 128;

/// The values that the shader reads. The layout matches `stroke_group.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    rect: [f32; 4],
    screen: [f32; 4],
}

/// The triangles of `shapes`, as egui makes them for the screen, with the
/// feathering of the edges. Every vertex has the white UV, so the triangles
/// need no texture.
pub fn tessellate(
    shapes: Vec<egui::Shape>,
    pixels_per_point: f32,
    options: &epaint::TessellationOptions,
    clip: egui::Rect,
) -> egui::Mesh {
    let options = epaint::TessellationOptions {
        prerasterized_discs: false,
        ..options.clone()
    };
    let mut tessellator = epaint::Tessellator::new(pixels_per_point, options, [1, 1], Vec::new());
    tessellator.set_clip_rect(clip);
    let mut mesh = egui::Mesh::default();
    for shape in shapes {
        tessellator.tessellate_shape(shape, &mut mesh);
    }
    mesh
}

/// The shape that draws `shapes` as one group with the alpha `alpha`. The
/// shapes must be opaque where they are not feathered. `None`: nothing of the
/// shapes is in `clip`.
pub fn group_shape(
    ctx: &egui::Context,
    format: wgpu::TextureFormat,
    shapes: Vec<egui::Shape>,
    alpha: f32,
    clip: egui::Rect,
) -> Option<egui::Shape> {
    let pixels_per_point = ctx.pixels_per_point();
    let options = ctx.tessellation_options(|o| o.clone());
    let mesh = tessellate(shapes, pixels_per_point, &options, clip);
    let rect = mesh.calc_bounds().intersect(clip);
    if mesh.is_empty() || !rect.is_positive() {
        return None;
    }
    let callback = GroupCallback {
        format,
        mesh,
        rect,
        alpha,
        index: AtomicU32::new(u32::MAX),
    };
    Some(egui_wgpu::Callback::new_paint_callback(rect, callback).into())
}

/// One group of a frame.
struct GroupCallback {
    format: wgpu::TextureFormat,
    /// In screen points.
    mesh: egui::Mesh,
    /// The bounds of the mesh in the clip rectangle, in screen points.
    rect: egui::Rect,
    alpha: f32,
    /// The index of the group in this frame, and of its target.
    index: AtomicU32,
}

/// The target of one group, and the buffers of its mesh.
struct Target {
    /// The size of the texture, in pixels.
    size: [u32; 2],
    view: wgpu::TextureView,
    params: wgpu::Buffer,
    mesh_bind: wgpu::BindGroup,
    composite_bind: wgpu::BindGroup,
    vertices: Option<wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
    used: Instant,
}

/// The pipelines of the groups and a pool of targets. Each group of a frame
/// uses the target at its index in the frame.
struct Groups {
    format: wgpu::TextureFormat,
    mesh_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    mesh: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    targets: Vec<Target>,
    /// The number of groups so far in this frame.
    count: usize,
}

impl Groups {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Groups {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stroke group"),
            source: wgpu::ShaderSource::Wgsl(include_str!("stroke_group.wgsl").into()),
        });
        let params = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let mesh_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stroke group mesh"),
            entries: &[params],
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stroke group composite"),
            entries: &[
                params,
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline = |label,
                        layout: &wgpu::BindGroupLayout,
                        vertex,
                        fragment,
                        buffers: &[Option<wgpu::VertexBufferLayout>],
                        topology| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    buffers,
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        // An egui vertex holds the position, the UV, and the color. The
        // shader skips the UV.
        let attributes = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Unorm8x4,
                offset: 16,
                shader_location: 1,
            },
        ];
        let vertex = wgpu::VertexBufferLayout {
            array_stride: size_of::<epaint::Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        };
        let fs_mesh = if format.is_srgb() {
            "fs_mesh_linear"
        } else {
            "fs_mesh_gamma"
        };
        Groups {
            format,
            mesh: pipeline(
                "stroke group mesh",
                &mesh_layout,
                "vs_mesh",
                fs_mesh,
                &[Some(vertex)],
                wgpu::PrimitiveTopology::TriangleList,
            ),
            composite: pipeline(
                "stroke group composite",
                &composite_layout,
                "vs_composite",
                "fs_composite",
                &[],
                wgpu::PrimitiveTopology::TriangleStrip,
            ),
            mesh_layout,
            composite_layout,
            targets: Vec::new(),
            count: 0,
        }
    }

    /// Makes target `index` at least `size` pixels large.
    fn ensure_target(&mut self, device: &wgpu::Device, index: usize, size: [u32; 2]) {
        let fits = self
            .targets
            .get(index)
            .is_some_and(|t| t.size[0] >= size[0] && t.size[1] >= size[1]);
        if fits {
            return;
        }
        let old = self.targets.get(index).map_or([0, 0], |t| t.size);
        let size = [
            size[0].max(old[0]).next_multiple_of(TARGET_STEP),
            size[1].max(old[1]).next_multiple_of(TARGET_STEP),
        ];
        let max = device.limits().max_texture_dimension_2d;
        let size = size.map(|s| s.min(max));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("stroke group"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stroke group params"),
            size: size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mesh_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stroke group mesh"),
            layout: &self.mesh_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            }],
        });
        let composite_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stroke group composite"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        let (vertices, indices) = match self.targets.get_mut(index) {
            Some(t) => (t.vertices.take(), t.indices.take()),
            None => (None, None),
        };
        let target = Target {
            size,
            view,
            params,
            mesh_bind,
            composite_bind,
            vertices,
            indices,
            used: Instant::now(),
        };
        match self.targets.get_mut(index) {
            Some(t) => *t = target,
            None => self.targets.push(target),
        }
    }
}

/// A buffer that holds at least `bytes`, made again only when it is too small.
fn ensure_buffer(
    device: &wgpu::Device,
    buffer: &mut Option<wgpu::Buffer>,
    bytes: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    if buffer.as_ref().is_none_or(|b| b.size() < bytes) {
        *buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stroke group mesh"),
            size: bytes.next_power_of_two().max(256),
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
    buffer.clone().expect("made above")
}

impl egui_wgpu::CallbackTrait for GroupCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
        egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let stale = resources
            .get::<Groups>()
            .is_none_or(|g| g.format != self.format);
        if stale {
            resources.insert(Groups::new(device, self.format));
        }
        let groups: &mut Groups = resources.get_mut().expect("inserted above");
        let pixels_per_point = screen_descriptor.pixels_per_point;
        let screen = screen_descriptor.size_in_pixels;
        let left = (self.rect.min.x * pixels_per_point).floor().max(0.0) as u32;
        let top = (self.rect.min.y * pixels_per_point).floor().max(0.0) as u32;
        let right = ((self.rect.max.x * pixels_per_point).ceil() as u32).min(screen[0]);
        let bottom = ((self.rect.max.y * pixels_per_point).ceil() as u32).min(screen[1]);
        if right <= left || bottom <= top {
            return Vec::new();
        }
        let size = [right - left, bottom - top];
        let index = groups.count;
        groups.count += 1;
        groups.ensure_target(device, index, size);
        let mesh_pipeline = groups.mesh.clone();
        let target = &mut groups.targets[index];
        let size = [size[0].min(target.size[0]), size[1].min(target.size[1])];
        target.used = Instant::now();
        let params = Params {
            rect: [
                left as f32,
                top as f32,
                (left + size[0]) as f32,
                (top + size[1]) as f32,
            ],
            screen: [
                screen[0] as f32,
                screen[1] as f32,
                pixels_per_point,
                self.alpha,
            ],
        };
        queue.write_buffer(&target.params, 0, bytemuck::bytes_of(&params));
        let vertex_bytes: &[u8] = bytemuck::cast_slice(&self.mesh.vertices);
        let index_bytes: &[u8] = bytemuck::cast_slice(&self.mesh.indices);
        let vertices = ensure_buffer(
            device,
            &mut target.vertices,
            vertex_bytes.len() as u64,
            wgpu::BufferUsages::VERTEX,
        );
        let indices = ensure_buffer(
            device,
            &mut target.indices,
            index_bytes.len() as u64,
            wgpu::BufferUsages::INDEX,
        );
        queue.write_buffer(&vertices, 0, vertex_bytes);
        queue.write_buffer(&indices, 0, index_bytes);
        let mut pass = egui_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stroke group"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
        pass.set_pipeline(&mesh_pipeline);
        pass.set_bind_group(0, &target.mesh_bind, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..vertex_bytes.len() as u64));
        pass.set_index_buffer(
            indices.slice(..index_bytes.len() as u64),
            wgpu::IndexFormat::Uint32,
        );
        pass.draw_indexed(0..self.mesh.indices.len() as u32, 0, 0..1);
        drop(pass);
        self.index.store(index as u32, Ordering::Relaxed);
        Vec::new()
    }

    /// Starts the count of the next frame, and drops the targets that no
    /// frame used for a while.
    fn finish_prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(groups) = resources.get_mut::<Groups>() {
            groups.count = 0;
            let now = Instant::now();
            while groups
                .targets
                .last()
                .is_some_and(|t| now.duration_since(t.used) > TARGET_LIFE)
            {
                groups.targets.pop();
            }
        }
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let index = self.index.load(Ordering::Relaxed);
        let Some(groups) = resources.get::<Groups>() else {
            return;
        };
        let Some(target) = groups.targets.get(index as usize) else {
            return;
        };
        let screen = info.screen_size_px;
        pass.set_viewport(0.0, 0.0, screen[0] as f32, screen[1] as f32, 0.0, 1.0);
        pass.set_pipeline(&groups.composite);
        pass.set_bind_group(0, &target.composite_bind, &[]);
        pass.draw(0..4, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws `mesh` over a clear image of `size` pixels, with premultiplied
    /// source-over blending, and returns the alpha of each pixel.
    fn rasterize(mesh: &egui::Mesh, size: usize) -> Vec<f32> {
        let mut alpha = vec![0.0f32; size * size];
        // A small offset keeps the samples off the shared edges.
        let (dx, dy) = (0.5013, 0.4987);
        for tri in mesh.indices.as_chunks::<3>().0 {
            let v = tri.map(|i| mesh.vertices[i as usize]);
            let [a, b, c] = v.map(|v| v.pos);
            let area = (b - a).x * (c - a).y - (b - a).y * (c - a).x;
            if area.abs() < 1e-12 {
                continue;
            }
            let min = a.min(b).min(c);
            let max = a.max(b).max(c);
            let x0 = min.x.floor().max(0.0) as usize;
            let y0 = min.y.floor().max(0.0) as usize;
            let x1 = (max.x.ceil() as usize).min(size);
            let y1 = (max.y.ceil() as usize).min(size);
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = egui::pos2(x as f32 + dx, y as f32 + dy);
                    let w = |p0: egui::Pos2, p1: egui::Pos2| {
                        ((p1 - p0).x * (p - p0).y - (p1 - p0).y * (p - p0).x) / area
                    };
                    let (wa, wb, wc) = (w(b, c), w(c, a), w(a, b));
                    if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                        continue;
                    }
                    let src = (wa * f32::from(v[0].color.a())
                        + wb * f32::from(v[1].color.a())
                        + wc * f32::from(v[2].color.a()))
                        / 255.0;
                    let dst = &mut alpha[y * size + x];
                    *dst = src + *dst * (1.0 - src);
                }
            }
        }
        alpha
    }

    /// An L-shaped line with the short steps of a line on the globe.
    fn l_line(color: egui::Color32) -> Vec<egui::Shape> {
        let mut pts: Vec<egui::Pos2> = Vec::new();
        let mut t = 0.0;
        while t < 60.0 {
            pts.push(egui::pos2(20.0, 80.0 - t));
            t += if t > 55.0 { 0.3 } else { 2.0 };
        }
        let mut t = 0.0;
        while t <= 60.0 {
            pts.push(egui::pos2(20.0 + t, 20.0));
            t += if t < 5.0 { 0.3 } else { 2.0 };
        }
        vec![egui::Shape::line(pts, egui::Stroke::new(6.0, color))]
    }

    #[test]
    fn a_group_gives_the_corner_of_a_line_the_alpha_of_its_middle() {
        let size = 100;
        let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let options = epaint::TessellationOptions::default();
        let at = |image: &[f32], x: usize, y: usize| image[y * size + x];
        // The shapes of a translucent stroke are opaque, and the group
        // alpha applies once to the whole layer.
        let group = 0.5;
        let mesh = tessellate(l_line(egui::Color32::RED), 1.0, &options, clip);
        let image: Vec<f32> = rasterize(&mesh, size).iter().map(|a| a * group).collect();
        let middle = at(&image, 20, 50);
        assert!((middle - group).abs() < 1e-3, "{middle}");
        for (x, y) in [(19, 19), (20, 20), (21, 21), (19, 21), (21, 19), (50, 20)] {
            let corner = at(&image, x, y);
            assert!((corner - middle).abs() < 2.0 / 255.0, "{x} {y}: {corner}");
        }
        // The same line drawn shape by shape with the alpha in its color
        // shows darker where the strip folds over at the corner.
        let translucent = egui::Color32::from_rgba_unmultiplied(255, 0, 0, 128);
        let mesh = tessellate(l_line(translucent), 1.0, &options, clip);
        let direct = rasterize(&mesh, size);
        let darkest = (17..24)
            .flat_map(|y| (17..24).map(move |x| (x, y)))
            .map(|(x, y)| at(&direct, x, y))
            .fold(0.0, f32::max);
        assert!(darkest > at(&direct, 20, 50) + 0.05, "{darkest}");
    }
}
