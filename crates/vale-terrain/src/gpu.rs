//! The heightmap in wgpu textures, and the brush as a shader.
//!
//! `Heightmap::stamp` is the reference. `GpuHeightmap::stamp` gives the same
//! levels to within the rounding of 32-bit numbers. `Heightmap::erode` is the
//! reference of `GpuHeightmap::erode` in the same way.
//!
//! The textures hold only the tiles that have a level other than the base
//! level. `tiles.rs` has the pool of the tiles.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};
use std::num::NonZeroU64;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::bands::SEA_LEVEL;
use crate::channel::{ChannelBuffers, ChannelLines, ChannelShape};
use crate::cube::{FACES, meters_to_level};
use crate::erode::{ErodeGrid, ErodeStep};
use crate::flow::{ChannelMap, ChannelWindow, CoarseHeights, WINDOW_CELLS, Window};
use crate::heightmap::{
    Limits, Mode, NOISE_RELIEF_M, StampPlan, TILE_SIZE, TexelRect, noise_octaves,
};
use crate::line::{LINE_SEGMENTS, LinePlan, REACH};
use crate::range::RangePlan;
use crate::ridge::RidgePlan;
use crate::tiles::{Pool, TILES_WGSL};

/// The number of face passes in one batch. One group of stamps takes one pass
/// for each face that it touches.
pub const STAMP_SLOTS: u32 = 256;

/// The largest number of stamps in one group.
pub const GROUP_STAMPS: usize = 32;

/// The number of ocean ridge plans in one batch.
pub const RIDGE_SLOTS: u32 = 8;

/// A group stops before the stamp that makes its rectangle on a face larger
/// than this number of times the largest stamp rectangle on that face. Each
/// texel of the group rectangle costs one read, one write, and one rectangle
/// test for each stamp. A straight run of `GROUP_STAMPS` stamps at 0.12 of the
/// radius apart covers 3 times one stamp rectangle, and a diagonal run covers
/// 5.5 times.
const UNION_LIMIT: usize = 8;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Uint;

const CHANNEL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Uint;

/// The row size of a texture copy to a buffer is a multiple of this number,
/// in bytes.
pub(crate) const ROW_ALIGN: usize = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;

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
    talus: f32,
    scale: f32,
    seed: u32,
    /// The motion of the push mode.
    motion: [f32; 3],
    /// 0 for the soft noise of the noise mode, 1 for the ridged noise, and 2
    /// for the inverted ridged noise.
    ridged: u32,
    /// The relief setting of the noise mode, and the levels above the sea
    /// where the noise has its full effect.
    relief: f32,
    relief_span: f32,
    /// The number of octaves of the noise mode.
    octaves: u32,
    pad: u32,
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
            talus: stamp.talus as f32,
            scale: stamp.scale as f32,
            seed: stamp.seed,
            motion: stamp.motion.map(|m| m as f32),
            ridged: u32::from(stamp.ridged) * (1 + u32::from(stamp.invert)),
            relief: stamp.relief.clamp(0.0, 1.0) as f32,
            relief_span: f32::from(meters_to_level(SEA_LEVEL + NOISE_RELIEF_M))
                - f32::from(meters_to_level(SEA_LEVEL)),
            octaves: noise_octaves(stamp.scale, size as usize),
            pad: 0,
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
    /// The level of the sea.
    sea: u32,
    /// The number of texels along one side of a face of the channel map.
    channel_size: u32,
    /// 1 if the carve mode, the ridge mode, or the deposit mode has a
    /// window, and 0 if not. In an erode pass, 1 if the cells of the step are the cells of a window.
    window_on: u32,
    /// The first texel, the face, and the cell size of the window.
    window_origin: [i32; 2],
    window_face: u32,
    window_cell: u32,
    /// The texel of the face at the first texel of the render target.
    origin: [i32; 2],
    /// The number of cells of an erode pass along one side of a face.
    cells: u32,
    /// The limits of the stamps of the group or of the erode pass. The low
    /// 16 bits are the floor, and the high 16 bits are the ceiling.
    limits: u32,
    /// The texels of the drop texture that hold the drops of an erode pass:
    /// x0, y0, x1, y1. The drop of each other cell is 0.
    drop_rect: [i32; 4],
    /// The number of bins along one side of a face of the channel lines, of
    /// the window lines, and of the lines of an erode step. 0 is no lines.
    lines_side: u32,
    window_bins: u32,
    erode_side: u32,
    /// The angle of one texel step, in radians.
    texel: f32,
    /// The width of a channel at its reference area, that area, and the
    /// width of a floodplain, as in `ChannelShape`.
    channel: [f32; 4],
    stamps: [FaceStamp; GROUP_STAMPS],
}

fn channel_values(shape: ChannelShape) -> [f32; 4] {
    [shape.width, shape.reference, shape.floodplain, 0.0].map(|v| v as f32)
}

/// New storage buffers that hold channel lines.
fn lines_buffers(device: &wgpu::Device, data: &ChannelBuffers) -> [wgpu::Buffer; 2] {
    use wgpu::util::DeviceExt;
    let buffer = |label, contents: &[u8]| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        })
    };
    [
        buffer("channel curves", bytemuck::cast_slice(&data.curves)),
        buffer("channel index", bytemuck::cast_slice(&data.index)),
    ]
}

/// The bind group of the stamp pipeline, and the buffers of the channel
/// lines in it.
struct Bind {
    group: wgpu::BindGroup,
    /// The curves and the index of the channel lines, of the window lines,
    /// and of the lines of the last erode step.
    lines: [[wgpu::Buffer; 2]; 3],
    /// The `bin_side` of each of those lines.
    sides: [u32; 3],
}

/// The resources of the stamp bind group that do not change.
struct BindParts {
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    ridge_uniforms: wgpu::Buffer,
    /// The views of bindings 1 to 6.
    views: [wgpu::TextureView; 6],
}

impl BindParts {
    fn group(&self, device: &wgpu::Device, lines: &[[wgpu::Buffer; 2]; 3]) -> wgpu::BindGroup {
        fn buffer(binding: u32, buffer: &wgpu::Buffer, size: usize) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer,
                    offset: 0,
                    size: NonZeroU64::new(size as u64),
                }),
            }
        }
        let mut entries = vec![
            buffer(0, &self.uniforms, size_of::<Uniforms>()),
            buffer(7, &self.ridge_uniforms, size_of::<RidgeUniforms>()),
        ];
        for (i, view) in self.views.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: i as u32 + 1,
                resource: wgpu::BindingResource::TextureView(view),
            });
        }
        for (i, buffers) in lines.iter().flatten().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: i as u32 + 8,
                resource: buffers.as_entire_binding(),
            });
        }
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stamp"),
            layout: &self.layout,
            entries: &entries,
        })
    }
}

/// The settings and the segments of one ocean ridge plan. The layout matches
/// `stamp.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RidgeUniforms {
    radius: f32,
    /// The mean length of a crest segment.
    segment: f32,
    /// The texel step at the center of a face, and the sine of the reach.
    texel: f32,
    sin_reach: f32,
    crest: f32,
    world_km: f32,
    spread: f32,
    offset: f32,
    seed: u32,
    count: u32,
    group: i32,
    level: f32,
    /// The length of the line: the number of a multiple of `segment`, and
    /// the distance from that multiple.
    length: [f32; 2],
    /// The other numbers of `RidgeShape`, in its order from `round`.
    round: f32,
    top: f32,
    foot: f32,
    mean: f32,
    magma: f32,
    spacing: f32,
    mean_throw: f32,
    heave: f32,
    box_half: f32,
    half_texel: f32,
    turn_cell: f32,
    blend: f32,
    valley: f32,
    high: f32,
    valley_part: f32,
    flat_floor: f32,
    band_km: f32,
    segment_depth: f32,
    /// The numbers of a range pass: the height, the ridge spacing, the
    /// piece length, and the tilt of a crest.
    range_height: f32,
    range_spacing: f32,
    range_piece: f32,
    range_tilt: f32,
    segments: [[f32; 16]; LINE_SEGMENTS],
}

// The segments start at the offset of the array of `stamp.wgsl`: 36 numbers
// of 4 bytes, a multiple of 16.
const _: () = assert!(std::mem::offset_of!(RidgeUniforms, segments) == 36 * 4);

fn packed(limits: Limits) -> u32 {
    u32::from(limits.floor) | u32::from(limits.ceiling) << 16
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

/// The render target of a pass. A pass cannot read the texture that it
/// writes, so it writes here, and a copy moves the texels to the tiles.
struct Scratch {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: [usize; 2],
}

/// The largest number of faces that one brush circle touches.
const BRUSH_FACES: usize = 3;

struct State {
    pool: Pool,
    /// A warp stamp draws each face that it touches to its own target. Each
    /// other pass uses the first target.
    scratch: [Option<Scratch>; BRUSH_FACES],
}

/// The tiles of a heightmap in a pool of textures, and the brush pipeline.
pub struct GpuHeightmap {
    device: wgpu::Device,
    face_size: u32,
    tile: usize,
    state: Mutex<State>,
    tiles_layout: wgpu::BindGroupLayout,
    /// The rivers that the carve mode and the ridge mode follow, as the
    /// bytes of a `ChannelMap`.
    channels: wgpu::Texture,
    channels_view: wgpu::TextureView,
    /// The small rivers that those modes follow on a part of the sphere,
    /// as the bytes of a `ChannelWindow`.
    window_channels: wgpu::Texture,
    window_view: wgpu::TextureView,
    /// The place of the window, or `None` with no window.
    window: Mutex<Option<Window>>,
    /// The levels that the deposit mode fills the ground to, on the cells of
    /// `CoarseHeights`.
    fills: wgpu::Texture,
    /// The same levels on the cells of the window.
    window_fills: wgpu::Texture,
    /// The drops of an erode step on the cells of `CoarseHeights`. Each face
    /// has one more cell at each side, from the faces that are there.
    drops: wgpu::Texture,
    /// The drops of an erode step on the cells of a window.
    window_drops: wgpu::Texture,
    pipeline: wgpu::RenderPipeline,
    bind_parts: BindParts,
    bind: Mutex<Bind>,
    /// One slot of `slot_size` bytes for each pass of a batch.
    uniforms: wgpu::Buffer,
    slot_size: u32,
    used_slots: AtomicU32,
    /// One slot of `ridge_slot_size` bytes for each ridge plan of a batch.
    ridge_uniforms: wgpu::Buffer,
    ridge_slot_size: u32,
    used_ridge_slots: AtomicU32,
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
    /// Makes a heightmap with `face_size` by `face_size` texels on each
    /// face, all at level 0. No tile has memory. The channel map has no
    /// rivers.
    pub fn new(device: &wgpu::Device, face_size: u32) -> GpuHeightmap {
        GpuHeightmap::with_tile_size(device, face_size, TILE_SIZE)
    }

    /// The same as `new`, with `tile` by `tile` texels in a tile.
    pub fn with_tile_size(device: &wgpu::Device, face_size: u32, tile: usize) -> GpuHeightmap {
        let pool = Pool::new(device, face_size as usize, tile, 0, 1);
        let tiles_layout = pool.layout().clone();
        // A new texture holds zeros, and a river size of 0 is no river.
        let channel_size = ChannelMap::size_for(face_size as usize) as u32;
        let channels = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("channel map"),
            size: wgpu::Extent3d {
                width: channel_size,
                height: channel_size,
                depth_or_array_layers: FACES as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CHANNEL_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let channels_view = channels.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        let window_size = 2 * WINDOW_CELLS as u32;
        let window_channels = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("channel window"),
            size: wgpu::Extent3d {
                width: window_size,
                height: window_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CHANNEL_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let window_view = window_channels.create_view(&wgpu::TextureViewDescriptor::default());

        let cell_texture = |label, size: usize, layers: usize| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size as u32,
                    height: size as u32,
                    depth_or_array_layers: layers as u32,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let cells = CoarseHeights::size_for(face_size as usize);
        let drops = cell_texture("erode drops", cells + 2, FACES);
        let drops_view = drops.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let window_drops = cell_texture("erode window drops", WINDOW_CELLS, 1);
        let window_drops_view = window_drops.create_view(&wgpu::TextureViewDescriptor::default());
        // A new texture holds zeros, and a fill level of 0 is no fill.
        let fills = cell_texture("fill levels", cells, FACES);
        let fills_view = fills.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let window_fills = cell_texture("window fill levels", WINDOW_CELLS, 1);
        let window_fills_view = window_fills.create_view(&wgpu::TextureViewDescriptor::default());

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
        let ridge_size = size_of::<RidgeUniforms>() as u32;
        let ridge_slot_size =
            ridge_size.next_multiple_of(device.limits().min_uniform_buffer_offset_alignment);
        let ridge_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ridge uniforms"),
            size: u64::from(ridge_slot_size * RIDGE_SLOTS),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ridge_size = NonZeroU64::new(u64::from(ridge_size));

        let texture_entry = |binding, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Uint,
                view_dimension,
                multisampled: false,
            },
            count: None,
        };
        let storage_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
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
                texture_entry(1, wgpu::TextureViewDimension::D2Array),
                texture_entry(2, wgpu::TextureViewDimension::D2),
                texture_entry(3, wgpu::TextureViewDimension::D2Array),
                texture_entry(4, wgpu::TextureViewDimension::D2),
                texture_entry(5, wgpu::TextureViewDimension::D2Array),
                texture_entry(6, wgpu::TextureViewDimension::D2),
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: ridge_size,
                    },
                    count: None,
                },
                storage_entry(8),
                storage_entry(9),
                storage_entry(10),
                storage_entry(11),
                storage_entry(12),
                storage_entry(13),
            ],
        });
        let bind_parts = BindParts {
            layout,
            uniforms,
            ridge_uniforms,
            views: [
                channels_view.clone(),
                window_view.clone(),
                drops_view,
                window_drops_view,
                fills_view,
                window_fills_view,
            ],
        };
        let empty = ChannelLines::empty().to_gpu();
        let lines = std::array::from_fn(|_| lines_buffers(device, &empty));
        let bind = Bind {
            group: bind_parts.group(device, &lines),
            lines,
            sides: [0; 3],
        };

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stamp"),
            source: wgpu::ShaderSource::Wgsl(
                format!("{TILES_WGSL}\n{}", include_str!("stamp.wgsl")).into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stamp"),
            bind_group_layouts: &[Some(&bind_parts.layout), Some(&tiles_layout)],
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
            device: device.clone(),
            face_size,
            tile,
            state: Mutex::new(State {
                pool,
                scratch: [const { None }; BRUSH_FACES],
            }),
            tiles_layout,
            channels,
            channels_view,
            window_channels,
            window_view,
            window: Mutex::new(None),
            fills,
            window_fills,
            drops,
            window_drops,
            pipeline,
            uniforms: bind_parts.uniforms.clone(),
            ridge_uniforms: bind_parts.ridge_uniforms.clone(),
            bind_parts,
            bind: Mutex::new(bind),
            slot_size,
            used_slots: AtomicU32::new(0),
            ridge_slot_size,
            used_ridge_slots: AtomicU32::new(0),
        }
    }

    pub fn face_size(&self) -> u32 {
        self.face_size
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("no thread stops with the lock")
    }

    fn bind(&self) -> std::sync::MutexGuard<'_, Bind> {
        self.bind.lock().expect("no thread stops with the lock")
    }

    /// Writes channel lines to the buffers of slot `slot`: 0 for the channel
    /// map, 1 for the window, and 2 for an erode step. With `None`, the slot
    /// has no lines. The write runs at the next submit. Larger lines get new
    /// buffers and a new bind group.
    fn write_lines(&self, queue: &wgpu::Queue, slot: usize, lines: Option<&ChannelLines>) {
        let bind = &mut *self.bind();
        let Some(lines) = lines else {
            bind.sides[slot] = 0;
            return;
        };
        let data = lines.to_gpu();
        bind.sides[slot] = lines.bin_side() as u32;
        let [curves, index] = &bind.lines[slot];
        let fits = curves.size() >= (data.curves.len() * 4) as u64
            && index.size() >= (data.index.len() * 4) as u64;
        if fits {
            queue.write_buffer(curves, 0, bytemuck::cast_slice(&data.curves));
            queue.write_buffer(index, 0, bytemuck::cast_slice(&data.index));
        } else {
            bind.lines[slot] = lines_buffers(&self.device, &data);
            bind.group = self.bind_parts.group(&self.device, &bind.lines);
        }
    }

    /// The layout of the bind group that `tiles` returns. A pipeline with the
    /// layout of `tiles_layout` also takes that bind group.
    pub fn tiles_layout(&self) -> &wgpu::BindGroupLayout {
        &self.tiles_layout
    }

    /// The table and the pool of the tiles, for bind group 1 of a shader
    /// that starts with `TILES_WGSL`. A change of the tiles can make a new
    /// bind group, so get it for each pass.
    pub fn tiles(&self) -> wgpu::BindGroup {
        self.state().pool.bind_group().clone()
    }

    /// The number of tiles that hold memory.
    pub fn allocated_tiles(&self) -> usize {
        self.state().pool.allocated_tiles()
    }

    /// The memory of the heightmap textures, in bytes: the table, the pool
    /// of the tiles, and the render target of the brush.
    pub fn memory_bytes(&self) -> usize {
        let state = self.state();
        let scratch = state.scratch.iter().flatten();
        let scratch: usize = scratch.map(|s| s.size[0] * s.size[1]).sum();
        state.pool.memory_bytes() + scratch * size_of::<u16>()
    }

    /// The six faces of the channel map as an array of 2D layers. Each texel
    /// holds the 4 bytes of a `ChannelMap` texel. A face has
    /// `ChannelMap::size_for` texels along one side.
    pub fn channels_view(&self) -> &wgpu::TextureView {
        &self.channels_view
    }

    /// Writes the channel map, its lines, and its fill levels. The write runs at the next submit, before the
    /// commands of each encoder in that submit. The size of the map comes
    /// from `ChannelMap::size_for`.
    pub fn upload_channels(&self, queue: &wgpu::Queue, map: &ChannelMap) {
        let size = self.channels.size();
        assert_eq!(map.size() as u32, size.width, "the channel map size");
        queue.write_texture(
            texels(&self.channels, 0, 0, 0),
            map.bytes(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * 4),
                rows_per_image: Some(size.height),
            },
            size,
        );
        self.write_lines(queue, 0, Some(map.lines()));
        let size = self.fills.size();
        queue.write_texture(
            texels(&self.fills, 0, 0, 0),
            bytemuck::cast_slice(map.fills()),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * 2),
                rows_per_image: Some(size.height),
            },
            size,
        );
    }

    /// The channel texels of the window, with `2 * WINDOW_CELLS` texels along
    /// one side. Each texel holds the 4 bytes of a `ChannelWindow` texel.
    pub fn window_view(&self) -> &wgpu::TextureView {
        &self.window_view
    }

    /// Sets the window that the carve mode, the ridge mode, and the deposit
    /// mode follow, or no window with `None`.
    /// The window has `WINDOW_CELLS` cells along one side. The stamps that go
    /// to an encoder after this call use the new window, and the texels go to
    /// the GPU at the next submit. Thus submit each encoder that holds stamps
    /// before this call.
    pub fn upload_window(&self, queue: &wgpu::Queue, window: Option<&ChannelWindow>) {
        if let Some(channels) = window {
            let window = channels.window();
            assert_eq!(window.cells, WINDOW_CELLS, "the window size");
            assert_eq!(window.face_size as u32, self.face_size, "the face size");
            let size = self.window_channels.size();
            queue.write_texture(
                texels(&self.window_channels, 0, 0, 0),
                channels.bytes(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size.width * 4),
                    rows_per_image: Some(size.height),
                },
                size,
            );
            self.write_lines(queue, 1, Some(channels.lines()));
            let size = self.window_fills.size();
            queue.write_texture(
                texels(&self.window_fills, 0, 0, 0),
                bytemuck::cast_slice(channels.fills()),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size.width * 2),
                    rows_per_image: Some(size.height),
                },
                size,
            );
        }
        let window = window.map(ChannelWindow::window);
        *self.window.lock().expect("no thread stops with the lock") = window;
    }

    /// Sets all texels to `level`, and takes the memory from each tile. The
    /// change runs at the next submit, before the commands of each encoder in
    /// that submit. Thus submit each encoder that holds stamps before this
    /// call.
    pub fn clear(&self, queue: &wgpu::Queue, level: u16) {
        let mut state = self.state();
        state.pool.clear(&self.device, queue, level);
        state.scratch = [const { None }; BRUSH_FACES];
    }

    /// Writes the texels of a rectangle, row by row. The write runs at the
    /// next submit, before the commands of each encoder in that submit. Thus
    /// submit each encoder that holds stamps before this call.
    ///
    /// A tile gets memory when it gets a level other than the base level. A
    /// tile that gets the base level in each texel loses its memory.
    pub fn upload(&self, queue: &wgpu::Queue, face: u32, rect: TexelRect, data: &[u16]) {
        let pool = &mut self.state().pool;
        pool.upload(&self.device, queue, 0, face as usize, rect, data);
    }

    /// Starts a batch of stamps. Call it after each submit of an encoder
    /// that holds stamps.
    pub fn begin_batch(&self) {
        self.used_slots.store(0, Ordering::Relaxed);
        self.used_ridge_slots.store(0, Ordering::Relaxed);
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
    /// The stamps of a group have one mode and the same limits. A smooth
    /// stamp, a weather stamp, a warp stamp, or a push stamp is a group of
    /// one.
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
        if matches!(mode, Mode::Erode | Mode::OceanRidge | Mode::Range) {
            // These stamps change nothing, so they need no pass.
            let same = plans.iter().take_while(|plan| plan.stamp.mode == mode);
            return same.count();
        }
        // A smooth stamp, a weather stamp, a warp stamp, or a push stamp
        // reads the results of the stamp before it on other texels. The other
        // modes read one texel.
        let most = match mode {
            Mode::Smooth | Mode::Weather | Mode::Warp | Mode::Push => 1,
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
            let other = plan.stamp.mode != mode
                || plan.stamp.limits != first.stamp.limits
                || plan.stamp.channel != first.stamp.channel;
            let apart = count > 0 && (other || next.is_wide());
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

        let state = &mut *self.state();
        // The faces go in rising order, as in `Heightmap::stamp`. A smooth
        // pass reads the new levels of the faces before it.
        let window = *self.window.lock().expect("no thread stops with the lock");
        let sides = self.bind().sides;
        let touched = cover.rects.iter().enumerate();
        let touched = touched.filter_map(|(face, rect)| Some((face, (*rect)?)));
        let touched: Vec<_> = touched.collect();
        // A warp pass or a push pass reads the levels from before the stamp
        // on each face. Thus all passes come before the first copy, and each
        // tile has its memory before the first pass.
        let copy_last = matches!(mode, Mode::Warp | Mode::Push);
        if copy_last {
            for &(face, rect) in &touched {
                self.ensure(queue, state, face, rect);
            }
        }
        for (slot, (i, &(face, rect))) in (first_slot..).zip(touched.iter().enumerate()) {
            let offset = slot * self.slot_size;
            let mut uniforms = Uniforms {
                face: face as u32,
                mode: match mode {
                    Mode::Raise | Mode::Uplift => 0,
                    Mode::Lower => 1,
                    Mode::Smooth => 2,
                    Mode::Flatten => 3,
                    Mode::Carve => 4,
                    Mode::Erode | Mode::OceanRidge | Mode::Range => 5,
                    Mode::Weather => 6,
                    Mode::Warp => 7,
                    Mode::Ridge => 8,
                    Mode::Deposit => 9,
                    Mode::Push => 12,
                    Mode::Noise => 13,
                },
                reach: first.reach as i32,
                size: self.face_size as i32,
                count: 0,
                sea: u32::from(meters_to_level(SEA_LEVEL)),
                channel_size: self.channels.width(),
                window_on: u32::from(window.is_some()),
                window_face: window.map_or(0, |w| w.face as u32),
                window_origin: window.map_or([0; 2], |w| [w.x0 as i32, w.y0 as i32]),
                window_cell: window.map_or(1, |w| w.cell as u32),
                origin: [rect.x0 as i32, rect.y0 as i32],
                limits: packed(first.stamp.limits),
                lines_side: sides[0],
                window_bins: if window.is_some() { sides[1] } else { 0 },
                texel: (FRAC_PI_2 / f64::from(self.face_size)) as f32,
                channel: channel_values(first.stamp.channel),
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
            if copy_last {
                self.draw(enc, state, i, rect, [offset, 0]);
            } else {
                self.pass(queue, enc, state, face, rect, [offset, 0]);
            }
        }
        if copy_last {
            for (i, &(face, rect)) in touched.iter().enumerate() {
                self.copy(enc, state, i, face, rect);
            }
        }
        count
    }

    /// Adds one pass to `enc` for a rectangle of one face, and the copies of
    /// its result to the tiles. `offsets` are the places of the uniforms of
    /// the pass and of the ridge plan.
    fn pass(
        &self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        state: &mut State,
        face: usize,
        rect: TexelRect,
        offsets: [u32; 2],
    ) {
        self.ensure(queue, state, face, rect);
        self.draw(enc, state, 0, rect, offsets);
        self.copy(enc, state, 0, face, rect);
    }

    /// Gives memory to each tile of a rectangle. A pass reads each tile from
    /// the pool, and a copy writes each tile.
    fn ensure(&self, queue: &wgpu::Queue, state: &mut State, face: usize, rect: TexelRect) {
        for (tx, ty, _) in state.pool.tiles_in(0, rect) {
            state
                .pool
                .ensure(&self.device, queue, 0, face, tx, ty, true);
        }
    }

    /// Adds one pass to `enc` that draws a rectangle of one face to the
    /// render target `target`. `offsets` are the places of the uniforms of
    /// the pass and of the ridge plan.
    fn draw(
        &self,
        enc: &mut wgpu::CommandEncoder,
        state: &mut State,
        target: usize,
        rect: TexelRect,
        offsets: [u32; 2],
    ) {
        let size = extent(rect);
        let scratch = self.scratch(&mut state.scratch[target], rect);
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stamp"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &scratch.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        let group = self.bind().group.clone();
        pass.set_bind_group(0, &group, &offsets);
        pass.set_bind_group(1, state.pool.bind_group(), &[]);
        pass.set_scissor_rect(0, 0, size.width, size.height);
        pass.draw(0..3, 0..1);
    }

    /// Adds the copies of a rectangle from the render target `target` to the
    /// tiles of one face.
    fn copy(
        &self,
        enc: &mut wgpu::CommandEncoder,
        state: &State,
        target: usize,
        face: usize,
        rect: TexelRect,
    ) {
        let scratch = state.scratch[target].as_ref().expect("a pass drew it");
        for (tx, ty, part) in state.pool.tiles_in(0, rect) {
            let slot = state
                .pool
                .slot(0, face, tx, ty)
                .expect("the tile has memory");
            enc.copy_texture_to_texture(
                texels(&scratch.texture, part.x0 - rect.x0, part.y0 - rect.y0, 0),
                state.pool.texels(slot, part.x0, part.y0),
                extent(part),
            );
        }
    }

    /// The render target for a rectangle of texels.
    ///
    /// A pass loads and stores each texel of its target on some GPUs, so the
    /// target is not much larger than the rectangle. A target that is too
    /// small or more than two times too large makes way for a new one.
    fn scratch<'a>(&self, scratch: &'a mut Option<Scratch>, rect: TexelRect) -> &'a Scratch {
        let need = [rect.x1 - rect.x0, rect.y1 - rect.y0];
        let keep = scratch.as_ref().is_some_and(|scratch| {
            let fits = need[0] <= scratch.size[0] && need[1] <= scratch.size[1];
            let large = 2 * need[0] < scratch.size[0] && 2 * need[1] < scratch.size[1];
            fits && !large
        });
        if !keep {
            // A larger target keeps its other side, so a row of stamps and a
            // column of stamps share one target.
            let old = scratch.as_ref().map_or([0; 2], |scratch| scratch.size);
            let grows = need[0] > old[0] || need[1] > old[1];
            let size = [0, 1].map(|i| {
                let side = need[i].next_multiple_of(self.tile);
                if grows { side.max(old[i]) } else { side }
            });
            let size = size.map(|side| side.min(self.face_size as usize));
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("stamp target"),
                size: wgpu::Extent3d {
                    width: size[0] as u32,
                    height: size[1] as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            *scratch = Some(Scratch {
                texture,
                view,
                size,
            });
        }
        scratch.as_ref().expect("the target is set")
    }

    /// Adds one step of the erode brush to `enc`, with one pass for each face
    /// that the step can change. The result is that of `Heightmap::erode`.
    /// The step comes from an `Erosion` with the same face size.
    ///
    /// The drops go to the GPU at the next submit, before the commands of
    /// each encoder in that submit. Thus a submit holds one step at most.
    /// A tile that the step can change gets memory.
    ///
    /// Returns `false` and adds nothing if the batch is full. Then submit
    /// `enc`, call `begin_batch`, and add the step to a new encoder.
    pub fn erode(
        &self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        step: &ErodeStep,
    ) -> bool {
        let rects = step.rects();
        let first_slot = self.used_slots.load(Ordering::Relaxed);
        let passes = rects.iter().flatten().count() as u32;
        if passes > STAMP_SLOTS - first_slot {
            return false;
        }
        self.used_slots
            .store(first_slot + passes, Ordering::Relaxed);
        let n = self.face_size as usize;
        let state = &mut *self.state();
        let grid = step.grid();
        let window = match grid {
            ErodeGrid::Global { size } => {
                assert_eq!(size as u32 + 2, self.drops.width(), "the cell size");
                None
            }
            ErodeGrid::Window(window) => {
                assert!(window.cells <= WINDOW_CELLS, "the window size");
                assert_eq!(window.face_size, n, "the face size");
                Some(window)
            }
        };
        // The cells of a window that go down, as a rectangle.
        let window_cells = window.and_then(|window| {
            let cells = window.cells;
            let lowered = |i: usize| step.drops()[i] > 0;
            let rows = |y: &usize| (0..cells).any(|x| lowered(y * cells + x));
            let columns = |x: &usize| (0..cells).any(|y| lowered(y * cells + x));
            let cells = TexelRect {
                x0: (0..cells).find(columns)?,
                y0: (0..cells).find(rows)?,
                x1: (0..cells).rfind(columns)? + 1,
                y1: (0..cells).rfind(rows)? + 1,
            };
            self.upload_drops(queue, &self.window_drops, 0, cells, |x, y| {
                step.cell_drop(window.face, x, y)
            });
            Some(cells)
        });
        self.write_lines(queue, 2, step.lines());
        let erode_side = self.bind().sides[2];
        let touched = rects.iter().enumerate();
        let touched = touched.filter_map(|(face, rect)| Some((face, (*rect)?)));
        for (slot, (face, rect)) in (first_slot..).zip(touched) {
            let cells = match grid {
                ErodeGrid::Global { size } => {
                    // The cells that the texels of the rectangle read, in
                    // the texture. The cell one place past the face edge is
                    // at 0 there.
                    let first = |t: usize| ((2 * t + 1) * size + n) / (2 * n);
                    let cells = TexelRect {
                        x0: first(rect.x0),
                        y0: first(rect.y0),
                        x1: first(rect.x1 - 1) + 2,
                        y1: first(rect.y1 - 1) + 2,
                    };
                    self.upload_drops(queue, &self.drops, face as u32, cells, |x, y| {
                        step.cell_drop(face, x - 1, y - 1)
                    });
                    cells
                }
                ErodeGrid::Window(_) => window_cells.unwrap_or(TexelRect {
                    x0: 0,
                    y0: 0,
                    x1: 0,
                    y1: 0,
                }),
            };
            let offset = slot * self.slot_size;
            let uniforms = Uniforms {
                face: face as u32,
                mode: 5,
                size: self.face_size as i32,
                window_on: u32::from(window.is_some()),
                window_face: window.map_or(0, |w| w.face as u32),
                window_origin: window.map_or([0; 2], |w| [w.x0 as i32, w.y0 as i32]),
                window_cell: window.map_or(1, |w| w.cell as u32),
                origin: [rect.x0 as i32, rect.y0 as i32],
                drop_rect: [cells.x0, cells.y0, cells.x1, cells.y1].map(|c| c as i32),
                cells: self.drops.width() - 2,
                limits: packed(step.limits()),
                erode_side,
                texel: (FRAC_PI_2 / f64::from(self.face_size)) as f32,
                channel: channel_values(step.channel()),
                ..bytemuck::Zeroable::zeroed()
            };
            let used = size_of::<Uniforms>() - GROUP_STAMPS * size_of::<FaceStamp>();
            queue.write_buffer(
                &self.uniforms,
                u64::from(offset),
                &bytemuck::bytes_of(&uniforms)[..used],
            );
            self.pass(queue, enc, state, face, rect, [offset, 0]);
        }
        true
    }

    /// Adds the ocean ridge of a plan to `enc`, with one pass for each face
    /// of the plan. The result is that of `Heightmap::ridge`. The plan comes
    /// from a `RidgeLine` with the same face size.
    ///
    /// A pass reads the ground from before the stroke in the tiles. Thus
    /// before this call, submit each encoder that holds stamps, and call
    /// `upload` with that ground for each rectangle of the plan. A texel of
    /// the plan that the ridge does not reach keeps that ground. A tile that
    /// the plan can change gets memory.
    ///
    /// Returns `false` and adds nothing if the batch is full. A batch holds
    /// `RIDGE_SLOTS` plans at most. Then submit `enc`, call `begin_batch`,
    /// and add the plan to a new encoder.
    pub fn ridge(
        &self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        plan: &RidgePlan,
    ) -> bool {
        let spec = &plan.ridge;
        let shape = plan.shape();
        let cell = shape.length;
        let length_cell = (plan.length / cell + 0.5).floor();
        let header = RidgeUniforms {
            length: [
                length_cell as f32,
                (plan.length - length_cell * cell) as f32,
            ],
            radius: spec.radius as f32,
            segment: cell as f32,
            texel: (2.0 * FRAC_PI_4 / f64::from(self.face_size)) as f32,
            sin_reach: (REACH * spec.radius).sin() as f32,
            crest: f32::from(spec.crest),
            world_km: spec.world_km as f32,
            spread: spec.spread as f32,
            offset: spec.offset as f32,
            seed: spec.seed,
            group: shape.group,
            level: shape.level as f32,
            round: shape.round as f32,
            top: shape.top as f32,
            foot: shape.foot as f32,
            mean: shape.mean as f32,
            magma: shape.magma as f32,
            spacing: shape.spacing as f32,
            mean_throw: shape.throw as f32,
            heave: shape.heave as f32,
            box_half: shape.box_half as f32,
            half_texel: shape.half_texel as f32,
            turn_cell: shape.turn_cell as f32,
            blend: shape.blend as f32,
            valley: shape.valley as f32,
            high: shape.high as f32,
            valley_part: shape.valley_part as f32,
            flat_floor: shape.flat_floor as f32,
            band_km: shape.band_km as f32,
            segment_depth: shape.segment_depth as f32,
            ..bytemuck::Zeroable::zeroed()
        };
        self.line_pass(queue, enc, &plan.line, cell, 10, spec.limits, header)
    }

    /// Adds the mountain range of a plan to `enc`, as `ridge` adds an ocean
    /// ridge. The result is that of `Heightmap::range`.
    pub fn range(
        &self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        plan: &RangePlan,
    ) -> bool {
        let spec = &plan.range;
        let cell = plan.piece();
        let length_cell = (plan.length / cell + 0.5).floor();
        let header = RidgeUniforms {
            length: [
                length_cell as f32,
                (plan.length - length_cell * cell) as f32,
            ],
            radius: spec.radius as f32,
            segment: cell as f32,
            texel: (2.0 * FRAC_PI_4 / f64::from(self.face_size)) as f32,
            sin_reach: (REACH * spec.radius).sin() as f32,
            seed: plan.seed(),
            range_height: spec.height as f32,
            range_spacing: spec.spacing as f32,
            range_piece: cell as f32,
            range_tilt: plan.tilt() as f32,
            ..bytemuck::Zeroable::zeroed()
        };
        self.line_pass(queue, enc, &plan.line, cell, 11, spec.limits, header)
    }

    /// Adds the passes of a line brush plan to `enc`, as `ridge`. `header`
    /// holds the settings of the brush. The pass sets the segments and their
    /// count, with `cell` as the cell of `LineSegment::to_f32`. `mode` is the
    /// mode of the shader.
    #[allow(clippy::too_many_arguments)]
    fn line_pass(
        &self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        plan: &LinePlan,
        cell: f64,
        mode: u32,
        limits: Limits,
        header: RidgeUniforms,
    ) -> bool {
        assert_eq!(plan.face_size as u32, self.face_size, "the face size");
        assert!(plan.segments.len() <= LINE_SEGMENTS, "the segments");
        let passes = plan.rects.iter().flatten().count() as u32;
        if passes == 0 {
            return true;
        }
        let first_slot = self.used_slots.load(Ordering::Relaxed);
        let ridge_slot = self.used_ridge_slots.load(Ordering::Relaxed);
        if passes > STAMP_SLOTS - first_slot || ridge_slot == RIDGE_SLOTS {
            return false;
        }
        self.used_slots
            .store(first_slot + passes, Ordering::Relaxed);
        self.used_ridge_slots
            .store(ridge_slot + 1, Ordering::Relaxed);
        let mut line = RidgeUniforms {
            count: plan.segments.len() as u32,
            ..header
        };
        for (to, segment) in line.segments.iter_mut().zip(&plan.segments) {
            *to = segment.to_f32(cell);
        }
        let used = size_of::<RidgeUniforms>()
            - (LINE_SEGMENTS - plan.segments.len()) * size_of::<[f32; 16]>();
        let ridge_offset = ridge_slot * self.ridge_slot_size;
        queue.write_buffer(
            &self.ridge_uniforms,
            u64::from(ridge_offset),
            &bytemuck::bytes_of(&line)[..used],
        );
        let state = &mut *self.state();
        let touched = plan.rects.iter().enumerate();
        let touched = touched.filter_map(|(face, rect)| Some((face, (*rect)?)));
        for (slot, (face, rect)) in (first_slot..).zip(touched) {
            let offset = slot * self.slot_size;
            let uniforms = Uniforms {
                face: face as u32,
                mode,
                size: self.face_size as i32,
                origin: [rect.x0 as i32, rect.y0 as i32],
                limits: packed(limits),
                ..bytemuck::Zeroable::zeroed()
            };
            let used = size_of::<Uniforms>() - GROUP_STAMPS * size_of::<FaceStamp>();
            queue.write_buffer(
                &self.uniforms,
                u64::from(offset),
                &bytemuck::bytes_of(&uniforms)[..used],
            );
            self.pass(queue, enc, state, face, rect, [offset, ridge_offset]);
        }
        true
    }

    /// Writes the drops of a rectangle of cells to one layer of a drop
    /// texture. `drop` gives the drop of the cell at a texel of the texture.
    fn upload_drops(
        &self,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        layer: u32,
        cells: TexelRect,
        drop: impl Fn(i64, i64) -> u16,
    ) {
        let mut data = Vec::with_capacity(area(cells));
        for y in cells.y0..cells.y1 {
            data.extend((cells.x0..cells.x1).map(|x| drop(x as i64, y as i64)));
        }
        let size = extent(cells);
        queue.write_texture(
            texels(texture, cells.x0, cells.y0, layer),
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * 2),
                rows_per_image: Some(size.height),
            },
            size,
        );
    }

    /// Adds a copy of a rectangle to `enc`, for a read on the CPU.
    pub fn read_rect(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        face: u32,
        rect: TexelRect,
    ) -> Readback {
        read_rect(&self.state().pool, device, enc, face, rect)
    }
}

/// Adds a copy of a rectangle of the tiles of `pool` to `enc`, for a read on
/// the CPU.
pub(crate) fn read_rect(
    pool: &Pool,
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
    let mut holes = Vec::new();
    for (tx, ty, part) in pool.tiles_in(0, rect) {
        let Some(slot) = pool.slot(0, face as usize, tx, ty) else {
            holes.push(part);
            continue;
        };
        let offset = (part.y0 - rect.y0) * row_bytes + (part.x0 - rect.x0) * 2;
        enc.copy_texture_to_buffer(
            pool.texels(slot, part.x0, part.y0),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: offset as u64,
                    bytes_per_row: Some(row_bytes as u32),
                    rows_per_image: None,
                },
            },
            extent(part),
        );
    }
    Readback {
        buffer,
        rect,
        row_bytes,
        base: pool.base(),
        holes,
    }
}

/// Texels on their way from the GPU.
pub struct Readback {
    buffer: wgpu::Buffer,
    rect: TexelRect,
    row_bytes: usize,
    base: u16,
    /// The parts of the rectangle in tiles that have no memory. The buffer
    /// holds no level for them.
    holes: Vec<TexelRect>,
}

impl Readback {
    /// A readback of a buffer that holds each texel of `rect`, with rows of
    /// `row_bytes`.
    pub(crate) fn dense(buffer: wgpu::Buffer, rect: TexelRect, row_bytes: usize) -> Readback {
        Readback {
            buffer,
            rect,
            row_bytes,
            base: 0,
            holes: Vec::new(),
        }
    }

    /// Asks for the texels. Call it after the submit of the encoder. `done`
    /// gets the texels row by row, during a later poll of the device. If the
    /// device is lost, `done` does not run.
    pub fn map(self, done: impl FnOnce(Vec<u16>) + Send + 'static) {
        let Readback {
            buffer,
            rect,
            row_bytes,
            base,
            holes,
        } = self;
        let width = rect.x1 - rect.x0;
        let mapped = buffer.clone();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            if result.is_err() {
                return;
            }
            let Ok(bytes) = mapped.get_mapped_range(..) else {
                return;
            };
            let mut levels: Vec<u16> = bytes
                .chunks(row_bytes)
                .flat_map(|row| row[..width * 2].as_chunks::<2>().0)
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            drop(bytes);
            mapped.unmap();
            for hole in holes {
                for y in hole.y0..hole.y1 {
                    let row = (y - rect.y0) * width;
                    levels[row + hole.x0 - rect.x0..row + hole.x1 - rect.x0].fill(base);
                }
            }
            done(levels);
        });
    }
}
