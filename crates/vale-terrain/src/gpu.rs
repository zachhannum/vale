//! The heightmap in a wgpu texture, and the brush as a shader.
//!
//! `Heightmap::stamp` is the reference. `GpuHeightmap::stamp` gives the same
//! levels to within the rounding of 32-bit numbers.

use std::f64::consts::FRAC_PI_4;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::cube::FACES;
use crate::heightmap::{Mode, StampPlan, TexelRect};

/// The number of face passes in one batch. One group of stamps takes one pass
/// for each face that it touches.
pub const STAMP_SLOTS: u32 = 256;

/// The largest number of stamps in one group.
pub const GROUP_STAMPS: usize = 32;

/// A group stops before the stamp that makes its rectangle on a face larger
/// than this number of times the largest stamp rectangle on that face. Each
/// texel of the group rectangle costs one read, one write, and one rectangle
/// test for each stamp. A straight run of `GROUP_STAMPS` stamps at 0.12 of the
/// radius apart covers 3 times one stamp rectangle, and a diagonal run covers
/// 5.5 times.
const UNION_LIMIT: usize = 8;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Uint;

/// The row size of a texture copy to a buffer is a multiple of this number,
/// in bytes.
const ROW_ALIGN: usize = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;

/// The values of one stamp on one face. The layout matches `stamp.wgsl`.
///
/// The brush can be a few texels wide on a face of 8192 texels. A 32-bit
/// cosine cannot hold such a small angle. Thus the values that need 64 bits
/// are made here, and the shader works with small differences.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FaceStamp {
    /// The brush center on this face in texels: the whole part and the rest.
    /// The center can be past the face edge.
    center: [i32; 2],
    center_rest: [f32; 2],
    /// The flat face coordinates of the brush center, and the cosine of the
    /// angle of each one.
    flat: [f32; 2],
    cos_center: [f32; 2],
    /// The texels that the stamp can change: x0, y0, x1, y1. The other values
    /// are valid only if the stamp has a rectangle on this face.
    rect: [i32; 4],
    radius: f32,
    hardness: f32,
    flow: f32,
    strength: f32,
    level: f32,
    pad: [u32; 3],
}

impl FaceStamp {
    fn new(size: u32, face: usize, rect: TexelRect, plan: &StampPlan) -> FaceStamp {
        let stamp = &plan.stamp;
        let axis = face / 2;
        let sign = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
        let depth = stamp.center[axis] * sign;
        let flat = [
            stamp.center[(axis + 1) % 3] / depth,
            stamp.center[(axis + 2) % 3] / depth,
        ];
        let angle = flat.map(f64::atan);
        let texel = angle.map(|t| (t / FRAC_PI_4 + 1.0) * 0.5 * f64::from(size));
        FaceStamp {
            center: texel.map(|t| t.floor() as i32),
            center_rest: texel.map(|t| (t - t.floor()) as f32),
            flat: flat.map(|a| a as f32),
            cos_center: angle.map(|t| t.cos() as f32),
            rect: [rect.x0, rect.y0, rect.x1, rect.y1].map(|t| t as i32),
            radius: stamp.radius as f32,
            hardness: stamp.hardness.clamp(0.0, 0.999) as f32,
            flow: stamp.flow as f32,
            strength: stamp.strength as f32,
            level: f32::from(stamp.level),
            pad: [0; 3],
        }
    }
}

/// The stamps of one group on one face. The layout matches `stamp.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    face: u32,
    mode: u32,
    reach: i32,
    size: i32,
    count: u32,
    pad: [u32; 3],
    stamps: [FaceStamp; GROUP_STAMPS],
}

fn area(rect: TexelRect) -> usize {
    (rect.x1 - rect.x0) * (rect.y1 - rect.y0)
}

/// The rectangle of each face that a group covers, and the area of the
/// largest stamp rectangle on that face.
#[derive(Clone, Copy, Default)]
struct Cover {
    rects: [Option<TexelRect>; FACES],
    largest: [usize; FACES],
}

impl Cover {
    fn with(mut self, plan: &StampPlan) -> Cover {
        for (face, rect) in plan.rects.iter().enumerate() {
            let Some(rect) = *rect else { continue };
            self.rects[face] = Some(self.rects[face].map_or(rect, |all| all.union(rect)));
            self.largest[face] = self.largest[face].max(area(rect));
        }
        self
    }

    fn passes(&self) -> u32 {
        self.rects.iter().flatten().count() as u32
    }

    fn is_wide(&self) -> bool {
        let mut rects = self.rects.iter().zip(self.largest);
        rects.any(|(rect, largest)| rect.is_some_and(|rect| area(rect) > UNION_LIMIT * largest))
    }
}

/// The six faces of a heightmap in one texture, and the brush pipeline.
pub struct GpuHeightmap {
    face_size: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    faces: [wgpu::TextureView; FACES],
    /// A pass cannot read the face that it writes. Each pass reads a copy of
    /// its texels from this texture.
    scratch: wgpu::Texture,
    pipeline: wgpu::RenderPipeline,
    bind_groups: [wgpu::BindGroup; FACES],
    /// One slot of `slot_size` bytes for each pass of a batch.
    uniforms: wgpu::Buffer,
    slot_size: u32,
    used_slots: AtomicU32,
}

fn extent(rect: TexelRect) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: (rect.x1 - rect.x0) as u32,
        height: (rect.y1 - rect.y0) as u32,
        depth_or_array_layers: 1,
    }
}

fn texels(texture: &wgpu::Texture, x: usize, y: usize, z: u32) -> wgpu::TexelCopyTextureInfo<'_> {
    wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d {
            x: x as u32,
            y: y as u32,
            z,
        },
        aspect: wgpu::TextureAspect::All,
    }
}

impl GpuHeightmap {
    /// Makes the texture, with `face_size` by `face_size` texels on each
    /// face. The levels are not set.
    pub fn new(device: &wgpu::Device, face_size: u32) -> GpuHeightmap {
        let face_texture = |label, layers, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: face_size,
                    height: face_size,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage,
                view_formats: &[],
            })
        };
        let texture = face_texture(
            "heightmap",
            FACES as u32,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let scratch = face_texture(
            "heightmap scratch",
            1,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let faces: [wgpu::TextureView; FACES] = std::array::from_fn(|face| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: face as u32,
                array_layer_count: Some(1),
                ..Default::default()
            })
        });
        let scratch_view = scratch.create_view(&wgpu::TextureViewDescriptor::default());

        let uniform_size = size_of::<Uniforms>() as u32;
        let slot_size =
            uniform_size.next_multiple_of(device.limits().min_uniform_buffer_offset_alignment);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stamp uniforms"),
            size: u64::from(slot_size * STAMP_SLOTS),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_size = NonZeroU64::new(u64::from(uniform_size));

        let face_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Uint,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stamp"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: uniform_size,
                    },
                    count: None,
                },
                face_entry(1),
                face_entry(2),
                face_entry(3),
                face_entry(4),
                face_entry(5),
            ],
        });
        let face_view = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let bind_groups = std::array::from_fn(|face| {
            // The faces past the +u, -u, +v, and -v edges. The opposite face
            // is not in the list, and the pass does not read its own face.
            let (u, v) = (2 * ((face / 2 + 1) % 3), 2 * ((face / 2 + 2) % 3));
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("stamp"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &uniforms,
                            offset: 0,
                            size: uniform_size,
                        }),
                    },
                    face_view(1, &scratch_view),
                    face_view(2, &faces[u]),
                    face_view(3, &faces[u + 1]),
                    face_view(4, &faces[v]),
                    face_view(5, &faces[v + 1]),
                ],
            })
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stamp"),
            source: wgpu::ShaderSource::Wgsl(include_str!("stamp.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stamp"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stamp"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: None,
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

        GpuHeightmap {
            face_size,
            texture,
            view,
            faces,
            scratch,
            pipeline,
            bind_groups,
            uniforms,
            slot_size,
            used_slots: AtomicU32::new(0),
        }
    }

    pub fn face_size(&self) -> u32 {
        self.face_size
    }

    /// The six faces as an array of 2D layers.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Sets all texels to `level`.
    pub fn clear(&self, enc: &mut wgpu::CommandEncoder, level: u16) {
        for face in 0..FACES {
            let color = wgpu::Color {
                r: f64::from(level),
                ..wgpu::Color::TRANSPARENT
            };
            self.face_pass(enc, face, wgpu::LoadOp::Clear(color));
        }
    }

    fn face_pass<'a>(
        &self,
        enc: &'a mut wgpu::CommandEncoder,
        face: usize,
        load: wgpu::LoadOp<wgpu::Color>,
    ) -> wgpu::RenderPass<'a> {
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("heightmap face"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.faces[face],
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    /// Writes the texels of a rectangle, row by row. The write runs at the
    /// next submit, before the commands of each encoder in that submit.
    pub fn upload(&self, queue: &wgpu::Queue, face: u32, rect: TexelRect, data: &[u16]) {
        let size = extent(rect);
        queue.write_texture(
            texels(&self.texture, rect.x0, rect.y0, face),
            bytemuck::cast_slice(data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * 2),
                rows_per_image: Some(size.height),
            },
            size,
        );
    }

    /// Starts a batch of stamps. Call it after each submit of an encoder
    /// that holds stamps.
    pub fn begin_batch(&self) {
        self.used_slots.store(0, Ordering::Relaxed);
    }

    /// Adds one stamp to `enc`. The plan comes from a `Heightmap` with the
    /// same face size.
    ///
    /// Returns `false` and adds nothing if the batch is full. Then submit
    /// `enc`, call `begin_batch`, and add the stamp to a new encoder.
    pub fn stamp(
        &self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        plan: &StampPlan,
    ) -> bool {
        self.stamp_group(queue, enc, std::slice::from_ref(plan)) == 1
    }

    /// Adds the first stamps of `plans` to `enc` as one group, with one pass
    /// for each face. Returns the number of stamps in the group. The result
    /// is the same as that of `stamp` for each one in order.
    ///
    /// The stamps of a group have one mode. A smooth stamp is a group of one.
    /// A group has `GROUP_STAMPS` stamps at most, and it stops before a stamp
    /// that is far from the others.
    ///
    /// Returns 0 and adds nothing if the batch is full. Then submit `enc`,
    /// call `begin_batch`, and add the stamps to a new encoder.
    pub fn stamp_group(
        &self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        plans: &[StampPlan],
    ) -> usize {
        let Some(first) = plans.first() else {
            return 0;
        };
        let mode = first.stamp.mode;
        // A smooth stamp reads the results of the stamp before it on the
        // texels around each texel. The other modes read one texel.
        let most = match mode {
            Mode::Smooth => 1,
            _ => GROUP_STAMPS,
        };
        // The values of each slot go to the GPU at the submit, so each pass
        // of the batch needs its own slot.
        let first_slot = self.used_slots.load(Ordering::Relaxed);
        let free = STAMP_SLOTS - first_slot;
        let mut cover = Cover::default();
        let mut count = 0;
        for plan in plans.iter().take(most) {
            let next = cover.with(plan);
            let apart = count > 0 && (plan.stamp.mode != mode || next.is_wide());
            if apart || next.passes() > free {
                break;
            }
            cover = next;
            count += 1;
        }
        if count == 0 {
            return 0;
        }
        let group = &plans[..count];
        self.used_slots
            .store(first_slot + cover.passes(), Ordering::Relaxed);

        let n = self.face_size as usize;
        // The smooth mode reads texels at this distance from the rectangle.
        let grow = match mode {
            Mode::Smooth => first.reach,
            _ => 0,
        };
        // The faces go in rising order, as in `Heightmap::stamp`. A smooth
        // pass reads the new levels of the faces before it.
        let touched = cover.rects.iter().enumerate();
        let touched = touched.filter_map(|(face, rect)| Some((face, (*rect)?)));
        for (slot, (face, rect)) in (first_slot..).zip(touched) {
            let offset = slot * self.slot_size;
            let mut uniforms = Uniforms {
                face: face as u32,
                mode: match mode {
                    Mode::Raise => 0,
                    Mode::Lower => 1,
                    Mode::Smooth => 2,
                    Mode::Flatten => 3,
                },
                reach: first.reach as i32,
                size: self.face_size as i32,
                count: 0,
                ..bytemuck::Zeroable::zeroed()
            };
            // A stamp with no rectangle on this face is not in the list.
            for plan in group {
                if let Some(rect) = plan.rects[face] {
                    uniforms.stamps[uniforms.count as usize] =
                        FaceStamp::new(self.face_size, face, rect, plan);
                    uniforms.count += 1;
                }
            }
            let used = size_of::<Uniforms>()
                - (GROUP_STAMPS - uniforms.count as usize) * size_of::<FaceStamp>();
            queue.write_buffer(
                &self.uniforms,
                u64::from(offset),
                &bytemuck::bytes_of(&uniforms)[..used],
            );
            let read = TexelRect {
                x0: rect.x0.saturating_sub(grow),
                y0: rect.y0.saturating_sub(grow),
                x1: (rect.x1 + grow).min(n),
                y1: (rect.y1 + grow).min(n),
            };
            enc.copy_texture_to_texture(
                texels(&self.texture, read.x0, read.y0, face as u32),
                texels(&self.scratch, read.x0, read.y0, 0),
                extent(read),
            );
            let size = extent(rect);
            let mut pass = self.face_pass(enc, face, wgpu::LoadOp::Load);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_groups[face], &[offset]);
            pass.set_scissor_rect(rect.x0 as u32, rect.y0 as u32, size.width, size.height);
            pass.draw(0..3, 0..1);
        }
        count
    }

    /// Adds a copy of a rectangle to `enc`, for a read on the CPU.
    pub fn read_rect(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        face: u32,
        rect: TexelRect,
    ) -> Readback {
        let (width, height) = (rect.x1 - rect.x0, rect.y1 - rect.y0);
        let row_bytes = (width * 2).next_multiple_of(ROW_ALIGN);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("heightmap readback"),
            size: (row_bytes * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            texels(&self.texture, rect.x0, rect.y0, face),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes as u32),
                    rows_per_image: None,
                },
            },
            extent(rect),
        );
        Readback {
            buffer,
            width,
            row_bytes,
        }
    }
}

/// Texels on their way from the GPU.
pub struct Readback {
    buffer: wgpu::Buffer,
    width: usize,
    row_bytes: usize,
}

impl Readback {
    /// Asks for the texels. Call it after the submit of the encoder. `done`
    /// gets the texels row by row, during a later poll of the device. If the
    /// device is lost, `done` does not run.
    pub fn map(self, done: impl FnOnce(Vec<u16>) + Send + 'static) {
        let Readback {
            buffer,
            width,
            row_bytes,
        } = self;
        let mapped = buffer.clone();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            if result.is_err() {
                return;
            }
            let Ok(bytes) = mapped.get_mapped_range(..) else {
                return;
            };
            let levels = bytes
                .chunks(row_bytes)
                .flat_map(|row| row[..width * 2].as_chunks::<2>().0)
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            drop(bytes);
            mapped.unmap();
            done(levels);
        });
    }
}
