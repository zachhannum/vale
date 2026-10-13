//! The tiles of a heightmap on the GPU: a pool of tiles, and a table from the
//! place of a tile on a face to its place in the pool.
//!
//! A tile that has no entry in the table reads as the base level, and it
//! takes no memory. The pool grows in chunks, and each chunk is one texture.
//! A texture does not move its texels to a larger one, so the pool never
//! holds two copies of a tile.

use std::num::NonZeroU64;
use std::sync::Mutex;

use crate::cube::FACES;
use crate::gpu::{Readback, read_rect};
use crate::heightmap::{TILE_SIZE, TexelRect};

/// The shader source of the table and the pool, for bind group 1. The
/// function `tile_level` reads one texel.
pub const TILES_WGSL: &str = include_str!("tiles.wgsl");

/// The largest number of chunks. The same number as the chunk bindings of
/// `tiles.wgsl`.
const CHUNKS: usize = 8;

/// The number of tiles along one side of a layer of a chunk.
const LAYER_TILES: usize = 8;

/// The number of tiles in one layer.
const LAYER_SLOTS: u32 = (LAYER_TILES * LAYER_TILES) as u32;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Uint;

const TABLE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;

/// The values of `Tiles` in `tiles.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    base: u32,
    tile: u32,
}

/// The place of one tile in the pool.
#[derive(Clone, Copy)]
pub(crate) struct Slot {
    pub chunk: usize,
    pub layer: u32,
    /// The first texel of the tile in the layer.
    pub x: usize,
    pub y: usize,
}

struct Chunk {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

pub(crate) struct Pool {
    n: usize,
    tile: usize,
    /// The number of tiles along one side of a face.
    per_side: usize,
    base: u16,
    /// The slot of each tile plus 1, face by face, in row order. 0 is a tile
    /// that has no memory.
    table: Vec<u32>,
    table_texture: wgpu::Texture,
    table_view: wgpu::TextureView,
    uniforms: wgpu::Buffer,
    chunks: Vec<Chunk>,
    /// The layers of all chunks for a heightmap with memory in each tile.
    layers: u32,
    /// The first slot that no tile used.
    next: u32,
    /// The slots of the tiles that lost their memory.
    free: Vec<u32>,
    /// The texture of a chunk binding that has no chunk.
    no_chunk: wgpu::TextureView,
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
}

fn array_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    format: wgpu::TextureFormat,
    size: [u32; 3],
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: size[2],
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// The first layer of a chunk, in the count of the layers of all chunks.
fn first_layer(chunk: usize) -> u32 {
    match chunk {
        0 => 0,
        _ => 1 << (chunk - 1),
    }
}

/// The layout of the bind group of the table and the pool, for bind group 1
/// of a shader that starts with `TILES_WGSL`. Each `GpuHeightmap` and each
/// `GpuTiles` makes its bind group for this layout. Thus a pipeline with this
/// layout draws from each of them.
pub fn tiles_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Uint,
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        },
        count: None,
    };
    let mut entries = vec![wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(size_of::<Uniforms>() as u64),
        },
        count: None,
    }];
    entries.extend((1..=1 + CHUNKS as u32).map(texture_entry));
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("tiles"),
        entries: &entries,
    })
}

impl Pool {
    /// A pool with no memory in each tile. Each texel reads as `base`.
    pub fn new(device: &wgpu::Device, n: usize, tile: usize, base: u16) -> Pool {
        let per_side = n.div_ceil(tile);
        let tiles = (FACES * per_side * per_side) as u32;
        let layers = tiles.div_ceil(LAYER_SLOTS);
        assert!(
            layers <= first_layer(CHUNKS),
            "the pool holds {tiles} tiles at most"
        );
        let table_texture = texture(
            device,
            "tile table",
            TABLE_FORMAT,
            [per_side as u32, per_side as u32, FACES as u32],
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let table_view = array_view(&table_texture);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tiles"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        let start = Uniforms {
            base: u32::from(base),
            tile: tile as u32,
        };
        uniforms
            .get_mapped_range_mut(..)
            .expect("the buffer is mapped")
            .copy_from_slice(bytemuck::bytes_of(&start));
        uniforms.unmap();
        let no_chunk = array_view(&texture(
            device,
            "no tile chunk",
            FORMAT,
            [1, 1, 1],
            wgpu::TextureUsages::TEXTURE_BINDING,
        ));

        let layout = tiles_layout(device);
        let bind_group = bind(device, &layout, &uniforms, &table_view, &[], &no_chunk);

        Pool {
            n,
            tile,
            per_side,
            base,
            table: vec![0; tiles as usize],
            table_texture,
            table_view,
            uniforms,
            chunks: Vec::new(),
            layers,
            next: 0,
            free: Vec::new(),
            no_chunk,
            layout,
            bind_group,
        }
    }

    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// The table and the chunks. A new chunk makes a new bind group.
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    pub fn base(&self) -> u16 {
        self.base
    }

    pub fn allocated_tiles(&self) -> usize {
        (self.next as usize) - self.free.len()
    }

    /// The memory of the table and the chunks, in bytes.
    pub fn memory_bytes(&self) -> usize {
        let layer = LAYER_TILES * self.tile;
        let layers: u32 = (0..self.chunks.len()).map(|c| self.chunk_layers(c)).sum();
        self.table.len() * size_of::<u32>() + layers as usize * layer * layer * size_of::<u16>()
    }

    pub fn chunk(&self, slot: Slot) -> &wgpu::Texture {
        &self.chunks[slot.chunk].texture
    }

    /// Each chunk has as many layers as all chunks before it. Thus the pool
    /// has at most two times the memory of its tiles, plus one layer.
    fn chunk_layers(&self, chunk: usize) -> u32 {
        let first = first_layer(chunk);
        first.max(1).min(self.layers - first)
    }

    fn place(&self, slot: u32) -> Slot {
        let (layer, at) = (slot / LAYER_SLOTS, (slot % LAYER_SLOTS) as usize);
        let chunk = match layer {
            0 => 0,
            _ => layer.ilog2() as usize + 1,
        };
        Slot {
            chunk,
            layer: layer - first_layer(chunk),
            x: at % LAYER_TILES * self.tile,
            y: at / LAYER_TILES * self.tile,
        }
    }

    /// The texels of one tile, clipped to the face.
    fn tile_rect(&self, tx: usize, ty: usize) -> TexelRect {
        TexelRect {
            x0: tx * self.tile,
            y0: ty * self.tile,
            x1: ((tx + 1) * self.tile).min(self.n),
            y1: ((ty + 1) * self.tile).min(self.n),
        }
    }

    /// Each tile that has texels in `rect`: its column, its row, and the
    /// texels of `rect` in it.
    pub fn tiles_in(&self, rect: TexelRect) -> Vec<(usize, usize, TexelRect)> {
        let mut tiles = Vec::new();
        for ty in rect.y0 / self.tile..rect.y1.div_ceil(self.tile) {
            for tx in rect.x0 / self.tile..rect.x1.div_ceil(self.tile) {
                let tile = self.tile_rect(tx, ty);
                let part = TexelRect {
                    x0: rect.x0.max(tile.x0),
                    y0: rect.y0.max(tile.y0),
                    x1: rect.x1.min(tile.x1),
                    y1: rect.y1.min(tile.y1),
                };
                tiles.push((tx, ty, part));
            }
        }
        tiles
    }

    /// Is `part` the full tile?
    pub fn is_full(&self, tx: usize, ty: usize, part: TexelRect) -> bool {
        part == self.tile_rect(tx, ty)
    }

    fn index(&self, face: usize, tx: usize, ty: usize) -> usize {
        (face * self.per_side + ty) * self.per_side + tx
    }

    /// The slot of a tile, or `None` if the tile has no memory.
    pub fn slot(&self, face: usize, tx: usize, ty: usize) -> Option<Slot> {
        match self.table[self.index(face, tx, ty)] {
            0 => None,
            slot => Some(self.place(slot - 1)),
        }
    }

    /// The first texel of a slot that holds a texel of its tile.
    pub fn texels(&self, slot: Slot, x: usize, y: usize) -> wgpu::TexelCopyTextureInfo<'_> {
        wgpu::TexelCopyTextureInfo {
            texture: self.chunk(slot),
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: (slot.x + x % self.tile) as u32,
                y: (slot.y + y % self.tile) as u32,
                z: slot.layer,
            },
            aspect: wgpu::TextureAspect::All,
        }
    }

    fn write_entry(&self, queue: &wgpu::Queue, face: usize, tx: usize, ty: usize, entry: u32) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.table_texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: tx as u32,
                    y: ty as u32,
                    z: face as u32,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::bytes_of(&entry),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    /// The slot of a tile. A tile that has no memory gets a slot. With
    /// `fill`, its texels get the base level.
    ///
    /// The table and the fill go to the GPU at the next submit, before the
    /// commands of each encoder in that submit. A command from before this
    /// call then reads the base level from the slot, as it did from the
    /// table. Without `fill`, the caller writes each texel of the tile in the
    /// same way.
    pub fn ensure(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        face: usize,
        tx: usize,
        ty: usize,
        fill: bool,
    ) -> Slot {
        if let Some(slot) = self.slot(face, tx, ty) {
            return slot;
        }
        let number = self.free.pop().unwrap_or_else(|| {
            self.next += 1;
            self.next - 1
        });
        let slot = self.place(number);
        if slot.chunk == self.chunks.len() {
            let side = (LAYER_TILES * self.tile) as u32;
            let texture = texture(
                device,
                "tile chunk",
                FORMAT,
                [side, side, self.chunk_layers(slot.chunk)],
                wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
            );
            let view = array_view(&texture);
            self.chunks.push(Chunk { texture, view });
            self.bind(device);
        }
        if fill {
            let side = self.tile as u32;
            queue.write_texture(
                self.texels(slot, 0, 0),
                bytemuck::cast_slice(&vec![self.base; self.tile * self.tile]),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(side * 2),
                    rows_per_image: Some(side),
                },
                wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 1,
                },
            );
        }
        let index = self.index(face, tx, ty);
        self.table[index] = number + 1;
        let place = (slot.x / self.tile) as u32
            | ((slot.y / self.tile) as u32) << 8
            | slot.layer << 16
            | (slot.chunk as u32) << 24;
        self.write_entry(queue, face, tx, ty, place + 1);
        slot
    }

    /// Takes the memory from a tile. The tile then reads as the base level.
    /// The change runs at the next submit, as that of `ensure` does.
    pub fn release(&mut self, queue: &wgpu::Queue, face: usize, tx: usize, ty: usize) {
        let index = self.index(face, tx, ty);
        let entry = std::mem::take(&mut self.table[index]);
        if entry != 0 {
            self.free.push(entry - 1);
            self.write_entry(queue, face, tx, ty, 0);
        }
    }

    /// Sets the base level and takes the memory from each tile and each
    /// chunk. The change runs at the next submit, as that of `ensure` does.
    pub fn clear(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, level: u16) {
        self.base = level;
        let uniforms = Uniforms {
            base: u32::from(level),
            tile: self.tile as u32,
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        self.table.fill(0);
        let side = self.per_side as u32;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.table_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&self.table),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(side * 4),
                rows_per_image: Some(side),
            },
            self.table_texture.size(),
        );
        self.next = 0;
        self.free.clear();
        if !self.chunks.is_empty() {
            self.chunks.clear();
            self.bind(device);
        }
    }

    /// Writes the texels of a rectangle, row by row. A tile gets memory when
    /// it gets a level other than the base level. A tile that gets the base
    /// level in each texel loses its memory. The change runs at the next
    /// submit, as that of `ensure` does.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        face: usize,
        rect: TexelRect,
        data: &[u16],
    ) {
        let width = rect.x1 - rect.x0;
        let mut levels = Vec::new();
        for (tx, ty, part) in self.tiles_in(rect) {
            levels.clear();
            for y in part.y0..part.y1 {
                let row = (y - rect.y0) * width;
                levels.extend_from_slice(&data[row + part.x0 - rect.x0..row + part.x1 - rect.x0]);
            }
            let full = self.is_full(tx, ty, part);
            if levels.iter().all(|level| *level == self.base) {
                if full {
                    self.release(queue, face, tx, ty);
                    continue;
                }
                if self.slot(face, tx, ty).is_none() {
                    continue;
                }
            }
            let slot = self.ensure(device, queue, face, tx, ty, !full);
            let size = wgpu::Extent3d {
                width: (part.x1 - part.x0) as u32,
                height: (part.y1 - part.y0) as u32,
                depth_or_array_layers: 1,
            };
            queue.write_texture(
                self.texels(slot, part.x0, part.y0),
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

    fn bind(&mut self, device: &wgpu::Device) {
        let views: Vec<_> = self.chunks.iter().map(|chunk| &chunk.view).collect();
        self.bind_group = bind(
            device,
            &self.layout,
            &self.uniforms,
            &self.table_view,
            &views,
            &self.no_chunk,
        );
    }
}

fn bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    table: &wgpu::TextureView,
    chunks: &[&wgpu::TextureView],
    no_chunk: &wgpu::TextureView,
) -> wgpu::BindGroup {
    let mut entries = vec![
        wgpu::BindGroupEntry {
            binding: 0,
            resource: uniforms.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::TextureView(table),
        },
    ];
    for chunk in 0..CHUNKS {
        entries.push(wgpu::BindGroupEntry {
            binding: 2 + chunk as u32,
            resource: wgpu::BindingResource::TextureView(chunks.get(chunk).unwrap_or(&no_chunk)),
        });
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tiles"),
        layout,
        entries: &entries,
    })
}

/// The tiles of a heightmap in a pool of textures, for a shader that reads
/// them. It has no brush.
pub struct GpuTiles {
    device: wgpu::Device,
    face_size: u32,
    pool: Mutex<Pool>,
    tiles_layout: wgpu::BindGroupLayout,
}

impl GpuTiles {
    /// Makes a map with `face_size` by `face_size` texels on each face, all
    /// at `base`. No tile has memory.
    pub fn new(device: &wgpu::Device, face_size: u32, base: u16) -> GpuTiles {
        GpuTiles::with_tile_size(device, face_size, TILE_SIZE, base)
    }

    /// The same as `new`, with `tile` by `tile` texels in a tile.
    pub fn with_tile_size(
        device: &wgpu::Device,
        face_size: u32,
        tile: usize,
        base: u16,
    ) -> GpuTiles {
        let pool = Pool::new(device, face_size as usize, tile, base);
        GpuTiles {
            device: device.clone(),
            face_size,
            tiles_layout: pool.layout().clone(),
            pool: Mutex::new(pool),
        }
    }

    pub fn face_size(&self) -> u32 {
        self.face_size
    }

    fn pool(&self) -> std::sync::MutexGuard<'_, Pool> {
        self.pool.lock().expect("no thread stops with the lock")
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
        self.pool().bind_group().clone()
    }

    /// The number of tiles that hold memory.
    pub fn allocated_tiles(&self) -> usize {
        self.pool().allocated_tiles()
    }

    /// The memory of the textures, in bytes: the table and the pool of the
    /// tiles.
    pub fn memory_bytes(&self) -> usize {
        self.pool().memory_bytes()
    }

    /// Sets all texels to `base`, and takes the memory from each tile. The
    /// change runs at the next submit, before the commands of each encoder in
    /// that submit.
    pub fn clear(&self, queue: &wgpu::Queue, base: u16) {
        self.pool().clear(&self.device, queue, base);
    }

    /// Writes the texels of a rectangle, row by row. The write runs at the
    /// next submit, before the commands of each encoder in that submit.
    ///
    /// A tile gets memory when it gets a level other than the base level. A
    /// tile that gets the base level in each texel loses its memory.
    pub fn upload(&self, queue: &wgpu::Queue, face: u32, rect: TexelRect, data: &[u16]) {
        self.pool()
            .upload(&self.device, queue, face as usize, rect, data);
    }

    /// Adds a copy of a rectangle to `enc`, for a read on the CPU.
    pub fn read_rect(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        face: u32,
        rect: TexelRect,
    ) -> Readback {
        read_rect(&self.pool(), device, enc, face, rect)
    }
}
