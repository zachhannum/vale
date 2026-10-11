//! The heightmap in wgpu textures, and the brush as a shader.
//!
//! `Heightmap::stamp` is the reference. `GpuHeightmap::stamp` gives the same
//! levels to within the rounding of 32-bit numbers. `Heightmap::erode` is the
//! reference of `GpuHeightmap::erode` in the same way.
//!
//! The textures hold only the tiles that have a level other than the base
//! level. `tiles.rs` has the pool of the tiles.

use std::f64::consts::FRAC_PI_4;
use std::num::NonZeroU64;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::bands::SEA_LEVEL;
use crate::cube::{FACES, meters_to_level};
use crate::erode::{ErodeGrid, ErodeStep};
use crate::flow::{ChannelMap, ChannelWindow, CoarseHeights, WINDOW_CELLS, Window};
use crate::heightmap::{Mode, StampPlan, TILE_SIZE, TexelRect};
use crate::tiles::{Pool, TILES_WGSL};

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

const CHANNEL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Uint;

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
    /// The level of the sea.
    sea: u32,
    /// The number of texels along one side of a face of the channel map.
    channel_size: u32,
    /// 1 if the carve mode has a window, and 0 if not. In an erode pass, 1
    /// if the cells of the step are the cells of a window.
    window_on: u32,
    /// The first texel, the face, and the cell size of the window.
    window_origin: [i32; 2],
    window_face: u32,
    window_cell: u32,
    /// The texel of the face at the first texel of the render target.
    origin: [i32; 2],
    /// The number of cells of an erode pass along one side of a face.
    cells: u32,
    pad: u32,
    /// The texels of the drop texture that hold the drops of an erode pass:
    /// x0, y0, x1, y1. The drop of each other cell is 0.
    drop_rect: [i32; 4],
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

/// The render target of a pass. A pass cannot read the texture that it
/// writes, so it writes here, and a copy moves the texels to the tiles.
struct Scratch {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: [usize; 2],
}

struct State {
    pool: Pool,
    scratch: Option<Scratch>,
}

/// The tiles of a heightmap in a pool of textures, and the brush pipeline.
pub struct GpuHeightmap {
    device: wgpu::Device,
    face_size: u32,
    tile: usize,
    state: Mutex<State>,
    tiles_layout: wgpu::BindGroupLayout,
    /// The rivers that the carve mode follows, as the bytes of a
    /// `ChannelMap`.
    channels: wgpu::Texture,
    channels_view: wgpu::TextureView,
    /// The small rivers that the carve mode follows on a part of the sphere,
    /// as the bytes of a `ChannelWindow`.
    window_channels: wgpu::Texture,
    window_view: wgpu::TextureView,
    /// The place of the window, or `None` with no window.
    window: Mutex<Option<Window>>,
    /// The drops of an erode step on the cells of `CoarseHeights`. Each face
    /// has one more cell at each side, from the faces that are there.
    drops: wgpu::Texture,
    /// The drops of an erode step on the cells of a window.
    window_drops: wgpu::Texture,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
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
    /// Makes a heightmap with `face_size` by `face_size` texels on each
    /// face, all at level 0. No tile has memory. The channel map has no
    /// rivers.
    pub fn new(device: &wgpu::Device, face_size: u32) -> GpuHeightmap {
        GpuHeightmap::with_tile_size(device, face_size, TILE_SIZE)
    }

    /// The same as `new`, with `tile` by `tile` texels in a tile.
    pub fn with_tile_size(device: &wgpu::Device, face_size: u32, tile: usize) -> GpuHeightmap {
        let pool = Pool::new(device, face_size as usize, tile);
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

        let drop_texture = |label, size: usize, layers: usize| {
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
        let drops = drop_texture("erode drops", cells + 2, FACES);
        let drops_view = drops.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let window_drops = drop_texture("erode window drops", WINDOW_CELLS, 1);
        let window_drops_view = window_drops.create_view(&wgpu::TextureViewDescriptor::default());

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
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
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
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&channels_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&window_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&drops_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&window_drops_view),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stamp"),
            source: wgpu::ShaderSource::Wgsl(
                format!("{TILES_WGSL}\n{}", include_str!("stamp.wgsl")).into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stamp"),
            bind_group_layouts: &[Some(&layout), Some(&tiles_layout)],
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
                scratch: None,
            }),
            tiles_layout,
            channels,
            channels_view,
            window_channels,
            window_view,
            window: Mutex::new(None),
            drops,
            window_drops,
            pipeline,
            bind_group,
            uniforms,
            slot_size,
            used_slots: AtomicU32::new(0),
        }
    }

    pub fn face_size(&self) -> u32 {
        self.face_size
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("no thread stops with the lock")
    }

    /// The layout of the bind group that `tiles` returns.
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
        let scratch = state.scratch.as_ref().map_or(0, |s| s.size[0] * s.size[1]);
        state.pool.memory_bytes() + scratch * size_of::<u16>()
    }

    /// The six faces of the channel map as an array of 2D layers. Each texel
    /// holds the 4 bytes of a `ChannelMap` texel. A face has
    /// `ChannelMap::size_for` texels along one side.
    pub fn channels_view(&self) -> &wgpu::TextureView {
        &self.channels_view
    }

    /// Writes the channel map. The write runs at the next submit, before the
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
    }

    /// The channel texels of the window, with `2 * WINDOW_CELLS` texels along
    /// one side. Each texel holds the 4 bytes of a `ChannelWindow` texel.
    pub fn window_view(&self) -> &wgpu::TextureView {
        &self.window_view
    }

    /// Sets the window that the carve mode follows, or no window with `None`.
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
        state.scratch = None;
    }

    /// Writes the texels of a rectangle, row by row. The write runs at the
    /// next submit, before the commands of each encoder in that submit. Thus
    /// submit each encoder that holds stamps before this call.
    ///
    /// A tile gets memory when it gets a level other than the base level. A
    /// tile that gets the base level in each texel loses its memory.
    pub fn upload(&self, queue: &wgpu::Queue, face: u32, rect: TexelRect, data: &[u16]) {
        let pool = &mut self.state().pool;
        let face = face as usize;
        let width = rect.x1 - rect.x0;
        let mut levels = Vec::new();
        for (tx, ty, part) in pool.tiles_in(rect) {
            levels.clear();
            for y in part.y0..part.y1 {
                let row = (y - rect.y0) * width;
                levels.extend_from_slice(&data[row + part.x0 - rect.x0..row + part.x1 - rect.x0]);
            }
            let full = pool.is_full(tx, ty, part);
            if levels.iter().all(|level| *level == pool.base()) {
                if full {
                    pool.release(queue, face, tx, ty);
                    continue;
                }
                if pool.slot(face, tx, ty).is_none() {
                    continue;
                }
            }
            let slot = pool.ensure(&self.device, queue, face, tx, ty, !full);
            let size = extent(part);
            queue.write_texture(
                pool.texels(slot, part.x0, part.y0),
                bytemuck::cast_slice(&levels),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size.width * 2),
                    rows_per_image: Some(size.height),
                },
                size,
            );
        }
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
        if mode == Mode::Erode {
            // These stamps change nothing, so they need no pass.
            let same = plans.iter().take_while(|plan| plan.stamp.mode == mode);
            return same.count();
        }
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

        let state = &mut *self.state();
        // The faces go in rising order, as in `Heightmap::stamp`. A smooth
        // pass reads the new levels of the faces before it.
        let window = *self.window.lock().expect("no thread stops with the lock");
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
                    Mode::Carve => 4,
                    Mode::Erode => 5,
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
            self.pass(queue, enc, state, face, rect, offset);
        }
        count
    }

    /// Adds one pass to `enc` for a rectangle of one face, and the copies of
    /// its result to the tiles. `offset` is the place of the uniforms of the
    /// pass.
    fn pass(
        &self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        state: &mut State,
        face: usize,
        rect: TexelRect,
        offset: u32,
    ) {
        // The pass reads each tile from the pool, so the tiles get
        // their memory before it.
        let tiles = state.pool.tiles_in(rect);
        for &(tx, ty, _) in &tiles {
            state.pool.ensure(&self.device, queue, face, tx, ty, true);
        }
        let size = extent(rect);
        let scratch = self.scratch(&mut state.scratch, rect);
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
        pass.set_bind_group(0, &self.bind_group, &[offset]);
        pass.set_bind_group(1, state.pool.bind_group(), &[]);
        pass.set_scissor_rect(0, 0, size.width, size.height);
        pass.draw(0..3, 0..1);
        drop(pass);
        for (tx, ty, part) in tiles {
            let slot = state.pool.slot(face, tx, ty).expect("the tile has memory");
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
                ..bytemuck::Zeroable::zeroed()
            };
            let used = size_of::<Uniforms>() - GROUP_STAMPS * size_of::<FaceStamp>();
            queue.write_buffer(
                &self.uniforms,
                u64::from(offset),
                &bytemuck::bytes_of(&uniforms)[..used],
            );
            self.pass(queue, enc, state, face, rect, offset);
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
        let (width, height) = (rect.x1 - rect.x0, rect.y1 - rect.y0);
        let row_bytes = (width * 2).next_multiple_of(ROW_ALIGN);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("heightmap readback"),
            size: (row_bytes * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let pool = &self.state().pool;
        let mut holes = Vec::new();
        for (tx, ty, part) in pool.tiles_in(rect) {
            let Some(slot) = pool.slot(face as usize, tx, ty) else {
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
