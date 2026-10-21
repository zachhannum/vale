//! The levels of `smooth.rs` on the GPU, for a map in tiles.
//!
//! One texture holds the levels from 1 up: mip level `k - 1` holds level
//! `k`, and each layer is a face. A render pass makes the dirty part of one
//! face at one level from the level before it. Level 1 reads the tiles, and
//! each other level reads the mip level before it in the same texture.

use std::num::NonZeroU64;
use std::sync::Mutex;

use crate::cube::FACES;
use crate::gpu::{ROW_ALIGN, Readback};
use crate::heightmap::TexelRect;
use crate::smooth::{level_size, max_level, pyramid_bytes};
use crate::tiles::{TILES_WGSL, tiles_layout};

/// The shader source of `field`, the smooth value of a map, for bind group
/// 2. Put it after `TILES_WGSL`.
pub const FIELD_WGSL: &str = include_str!("field.wgsl");

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Uint;

/// The distance between two values of `Build` in the uniform buffer.
const BUILD_STRIDE: u64 = 256;

/// The values of `Build` in the build shader.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Build {
    face: u32,
    /// The face size of the level that the pass reads.
    size: u32,
}

const BUILD_WGSL: &str = "
struct Build {
    face: u32,
    size: u32,
}

@group(0) @binding(0) var<uniform> build: Build;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(at * 2.0 - 1.0, 0.0, 1.0);
}

// The rounded mean of the 2 by 2 block of the level before, on the face.
@fragment
fn fs_main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<u32> {
    let p = vec2<i32>(at.xy) * 2;
    let size = i32(build.size);
    var sum = 0u;
    var count = 0u;
    for (var i = 0; i < 4; i++) {
        let q = p + vec2<i32>(i & 1, i >> 1u);
        if q.x < size && q.y < size {
            sum += source(i32(build.face), q);
            count += 1u;
        }
    }
    return vec4<u32>((sum + count / 2u) / count, 0u, 0u, 0u);
}
";

const FROM_TILES_WGSL: &str = "
fn source(face: i32, at: vec2<i32>) -> u32 {
    return tile_level(face, at);
}
";

const FROM_LEVEL_WGSL: &str = "
@group(1) @binding(0) var source_level: texture_2d_array<u32>;

fn source(face: i32, at: vec2<i32>) -> u32 {
    return textureLoad(source_level, at, face, 0).r;
}
";

fn texture_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Uint,
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        },
        count: None,
    }
}

/// The layout of the bind group of the levels, for bind group 2 of a shader
/// with `FIELD_WGSL`.
pub fn pyramid_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("pyramid"),
        entries: &[texture_entry(0, wgpu::ShaderStages::FRAGMENT)],
    })
}

fn pipeline(
    device: &wgpu::Device,
    source: &str,
    layouts: &[&wgpu::BindGroupLayout],
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("pyramid"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layouts: Vec<_> = layouts.iter().map(|layout| Some(*layout)).collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("pyramid"),
        bind_group_layouts: &layouts,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("pyramid"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(FORMAT.into())],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// The levels of a map from 1 to `max_level(face_size)`, in one texture.
///
/// The texture starts with all faces dirty. `update` makes the dirty parts
/// from the tiles of the map.
pub struct GpuPyramid {
    face_size: u32,
    levels: u32,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    /// The target of each level and face, level by level.
    targets: Vec<wgpu::TextureView>,
    /// The source of each level from 2 up.
    sources: Vec<wgpu::BindGroup>,
    build: wgpu::BindGroup,
    from_tiles: wgpu::RenderPipeline,
    from_level: wgpu::RenderPipeline,
    /// The dirty texels of each face, at level 0.
    dirty: Mutex<[Option<TexelRect>; FACES]>,
}

impl GpuPyramid {
    /// The levels of a map of `face_size` by `face_size` texels on each face.
    pub fn new(device: &wgpu::Device, face_size: u32) -> GpuPyramid {
        let levels = max_level(face_size);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pyramid"),
            size: wgpu::Extent3d {
                width: level_size(face_size, 1),
                height: level_size(face_size, 1),
                depth_or_array_layers: FACES as u32,
            },
            mip_level_count: levels.max(1),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let layout = pyramid_layout(device);
        let all = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pyramid"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&all),
            }],
        });

        let mut targets = Vec::new();
        for k in 1..=levels {
            for face in 0..FACES as u32 {
                targets.push(texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_mip_level: k - 1,
                    mip_level_count: Some(1),
                    base_array_layer: face,
                    array_layer_count: Some(1),
                    ..Default::default()
                }));
            }
        }
        let source_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pyramid source"),
            entries: &[texture_entry(0, wgpu::ShaderStages::FRAGMENT)],
        });
        let sources = (2..=levels)
            .map(|k| {
                let view = texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2Array),
                    base_mip_level: k - 2,
                    mip_level_count: Some(1),
                    ..Default::default()
                });
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("pyramid source"),
                    layout: &source_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    }],
                })
            })
            .collect();

        let build_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pyramid build"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(size_of::<Build>() as u64),
                },
                count: None,
            }],
        });
        let passes = (levels as usize * FACES).max(1);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pyramid build"),
            size: passes as u64 * BUILD_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: true,
        });
        let mut bytes = vec![0u8; passes * BUILD_STRIDE as usize];
        for k in 1..=levels {
            for face in 0..FACES as u32 {
                let at = pass_index(k, face) as usize * BUILD_STRIDE as usize;
                let build = Build {
                    face,
                    size: level_size(face_size, k - 1),
                };
                bytes[at..at + size_of::<Build>()].copy_from_slice(bytemuck::bytes_of(&build));
            }
        }
        uniforms
            .get_mapped_range_mut(..)
            .expect("the buffer is mapped")
            .copy_from_slice(&bytes);
        uniforms.unmap();
        let build = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pyramid build"),
            layout: &build_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniforms,
                    offset: 0,
                    size: NonZeroU64::new(size_of::<Build>() as u64),
                }),
            }],
        });

        let from_tiles = pipeline(
            device,
            &format!("{TILES_WGSL}\n{FROM_TILES_WGSL}\n{BUILD_WGSL}"),
            &[&build_layout, &tiles_layout(device)],
        );
        let from_level = pipeline(
            device,
            &format!("{FROM_LEVEL_WGSL}\n{BUILD_WGSL}"),
            &[&build_layout, &source_layout],
        );
        let pyramid = GpuPyramid {
            face_size,
            levels,
            texture,
            bind_group,
            targets,
            sources,
            build,
            from_tiles,
            from_level,
            dirty: Mutex::new([None; FACES]),
        };
        pyramid.mark_all();
        pyramid
    }

    pub fn face_size(&self) -> u32 {
        self.face_size
    }

    /// The coarsest level, `max_level(face_size)`.
    pub fn max_level(&self) -> u32 {
        self.levels
    }

    /// The levels, for bind group 2 of a shader with `FIELD_WGSL`.
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    /// The memory of the texture, in bytes.
    pub fn memory_bytes(&self) -> u64 {
        pyramid_bytes(self.face_size)
    }

    fn dirty(&self) -> std::sync::MutexGuard<'_, [Option<TexelRect>; FACES]> {
        self.dirty.lock().expect("no thread stops with the lock")
    }

    /// Marks texels of the map as changed. `rect` is in texels of level 0.
    pub fn mark(&self, face: u32, rect: TexelRect) {
        if rect.x0 >= rect.x1 || rect.y0 >= rect.y1 {
            return;
        }
        let dirty = &mut self.dirty()[face as usize];
        *dirty = Some(dirty.map_or(rect, |old| old.union(rect)));
    }

    /// Marks each texel of the map as changed.
    pub fn mark_all(&self) {
        let n = self.face_size as usize;
        let full = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        *self.dirty() = [Some(full); FACES];
    }

    /// Sets each texel of each level to `level`, for a map with all texels
    /// at `level`. Nothing is dirty after it.
    pub fn clear(&self, enc: &mut wgpu::CommandEncoder, level: u16) {
        *self.dirty() = [None; FACES];
        for view in &self.targets {
            let color = wgpu::Color {
                r: f64::from(level),
                g: 0.0,
                b: 0.0,
                a: 0.0,
            };
            enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pyramid clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
    }

    /// Makes the dirty parts of each level, from `tiles`: the bind group of
    /// the tiles of the map, from `GpuTiles::tiles` or `GpuHeightmap::tiles`.
    /// The map has the face size of the pyramid. Nothing is dirty after it.
    pub fn update(&self, enc: &mut wgpu::CommandEncoder, tiles: &wgpu::BindGroup) {
        let dirty = std::mem::replace(&mut *self.dirty(), [None; FACES]);
        for k in 1..=self.levels {
            let m = level_size(self.face_size, k);
            let step = 1usize << k;
            for (face, rect) in dirty.iter().enumerate() {
                let Some(rect) = rect else {
                    continue;
                };
                let x0 = (rect.x0 >> k) as u32;
                let y0 = (rect.y0 >> k) as u32;
                let x1 = (rect.x1.div_ceil(step) as u32).min(m);
                let y1 = (rect.y1.div_ceil(step) as u32).min(m);
                if x0 >= x1 || y0 >= y1 {
                    continue;
                }
                let index = pass_index(k, face as u32);
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("pyramid"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.targets[index as usize],
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                let offset = index * BUILD_STRIDE as u32;
                if k == 1 {
                    pass.set_pipeline(&self.from_tiles);
                    pass.set_bind_group(1, tiles, &[]);
                } else {
                    pass.set_pipeline(&self.from_level);
                    pass.set_bind_group(1, &self.sources[k as usize - 2], &[]);
                }
                pass.set_bind_group(0, &self.build, &[offset]);
                pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
                pass.draw(0..3, 0..1);
            }
        }
    }

    /// Adds a copy of one face of one level to `enc`, for a read on the CPU.
    /// `level` is from 1 to `max_level`.
    pub fn read_level(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        level: u32,
        face: u32,
    ) -> Readback {
        assert!(
            (1..=self.levels).contains(&level),
            "the pyramid has no level {level}"
        );
        let m = level_size(self.face_size, level) as usize;
        let row_bytes = (m * 2).next_multiple_of(ROW_ALIGN);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pyramid readback"),
            size: (row_bytes * m) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: level - 1,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: face,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes as u32),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: m as u32,
                height: m as u32,
                depth_or_array_layers: 1,
            },
        );
        let rect = TexelRect {
            x0: 0,
            y0: 0,
            x1: m,
            y1: m,
        };
        Readback::dense(buffer, rect, row_bytes)
    }
}

/// The place of the target and the uniforms of one pass.
fn pass_index(level: u32, face: u32) -> u32 {
    (level - 1) * FACES as u32 + face
}
