//! The screen-space lines of the color stops. A pass finds the places where
//! the shown height crosses the value of a stop between two pixel centers,
//! and a second pass gives each pixel the nearest of them. The layer pass
//! then draws each line at its weight in pixels from the band edge.
//!
//! A pixel keeps one place as a 32-bit payload, relative to the pixel. The
//! layout matches `edges.wgsl`.

use std::collections::HashMap;

use eframe::egui_wgpu::wgpu;

use super::gpu::LayerKey;
use super::preview::LineUniform;

/// The largest half weight of a line in pixels. A larger weight is cut to it,
/// so that the offset of a place fits in the payload.
pub const MAX_HALF: f32 = 29.0;
/// The payload of a pixel with no place.
pub const EMPTY: u32 = u32::MAX;
/// The steps of the crossing between two pixel centers.
pub const T_STEPS: u32 = 64;

const OFFSET_BITS: u32 = 6;
const OFFSET_MASK: u32 = (1 << OFFSET_BITS) - 1;
const AXIS_SHIFT: u32 = 2 * OFFSET_BITS;
const T_SHIFT: u32 = AXIS_SHIFT + 1;
const T_MASK: u32 = (1 << 7) - 1;
const STOP_SHIFT: u32 = T_SHIFT + 7;
const STOP_MASK: u32 = (1 << 6) - 1;

/// The largest offset of a place from a pixel, on each axis.
pub const MAX_OFFSET: i32 = (1 << (OFFSET_BITS - 1)) - 1;

/// One place where the height crosses the value of a stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seed {
    /// The offset of the first pixel of the pair from the pixel that keeps
    /// the place.
    pub base: [i32; 2],
    /// The second pixel is the next pixel in x (false) or in y (true).
    pub axis: bool,
    /// The place of the crossing from the first pixel center, in steps of
    /// 1 / `T_STEPS` of a pixel.
    pub t: u32,
    /// The index of the stop.
    pub stop: u32,
}

/// The half weight of the widest line, cut to `MAX_HALF`. It is 0 when no
/// line is on.
pub fn max_half(lines: &[LineUniform]) -> f32 {
    lines
        .iter()
        .map(|line| line[0][3])
        .fold(0.0_f32, f32::max)
        .min(MAX_HALF)
}

/// The distance in pixels, on each axis, inside which a pixel looks for the
/// nearest place. `None` when no line is on.
pub fn radius(max_half: f32) -> Option<u32> {
    (max_half > 0.0).then(|| max_half.min(MAX_HALF).ceil() as u32 + 2)
}

/// The payload of a place. `None` when an offset or a field does not fit.
pub fn pack(seed: Seed) -> Option<u32> {
    let fits = |v: i32| (-MAX_OFFSET - 1..=MAX_OFFSET).contains(&v);
    if !fits(seed.base[0]) || !fits(seed.base[1]) || seed.t > T_STEPS || seed.stop > STOP_MASK {
        return None;
    }
    let bx = seed.base[0] as u32 & OFFSET_MASK;
    let by = seed.base[1] as u32 & OFFSET_MASK;
    Some(
        bx | by << OFFSET_BITS
            | u32::from(seed.axis) << AXIS_SHIFT
            | seed.t << T_SHIFT
            | seed.stop << STOP_SHIFT,
    )
}

/// The place in a payload. `None` for `EMPTY`.
pub fn unpack(payload: u32) -> Option<Seed> {
    if payload == EMPTY {
        return None;
    }
    let signed = |v: u32| ((v << (32 - OFFSET_BITS)) as i32) >> (32 - OFFSET_BITS);
    Some(Seed {
        base: [
            signed(payload & OFFSET_MASK),
            signed(payload >> OFFSET_BITS & OFFSET_MASK),
        ],
        axis: payload >> AXIS_SHIFT & 1 == 1,
        t: payload >> T_SHIFT & T_MASK,
        stop: payload >> STOP_SHIFT & STOP_MASK,
    })
}

/// The payload of a place kept by a pixel, for a pixel at `step` from it.
/// `None` when the new offset does not fit.
pub fn rebase(payload: u32, step: [i32; 2]) -> Option<u32> {
    let mut seed = unpack(payload)?;
    seed.base = [seed.base[0] - step[0], seed.base[1] - step[1]];
    pack(seed)
}

/// The pixels on a side of a block of the pass that marks the blocks with a
/// place. The value matches `edges.wgsl`.
pub const BLOCK: u32 = 8;

/// The scissor of a pass in pixels, as x, y, width, and height: `area` grown
/// by `grow` pixels on each side, inside a target of `size`. `area` is the
/// left, the top, the right, and the bottom. `None` when nothing is left.
pub fn scissor(area: [f32; 4], grow: u32, size: [u32; 2]) -> Option<[u32; 4]> {
    let g = grow as f32;
    let x0 = (area[0] - g).floor().max(0.0) as u32;
    let y0 = (area[1] - g).floor().max(0.0) as u32;
    let x1 = ((area[2] + g).ceil().max(0.0) as u32).min(size[0]);
    let y1 = ((area[3] + g).ceil().max(0.0) as u32).min(size[1]);
    (x1 > x0 && y1 > y0).then(|| [x0, y0, x1 - x0, y1 - y0])
}

/// The scissor of the pass of the blocks for the scissor of the places.
fn block_scissor(places: [u32; 4], blocks: [u32; 2]) -> [u32; 4] {
    let x0 = places[0] / BLOCK;
    let y0 = places[1] / BLOCK;
    let x1 = (places[0] + places[2]).div_ceil(BLOCK).min(blocks[0]);
    let y1 = (places[1] + places[3]).div_ceil(BLOCK).min(blocks[1]);
    [x0, y0, x1 - x0, y1 - y0]
}

const HEIGHTS: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
/// A shown height and its change across one pixel.
const SHOWN: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Float;
const PLACES: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;

/// The textures that the layers share, at the size of the render target.
struct Scratch {
    size: [u32; 2],
    seeds: wgpu::TextureView,
    blocks: wgpu::TextureView,
    /// The group that reads `seeds`.
    read_seeds: wgpu::BindGroup,
    /// The group that reads `seeds` and `blocks`.
    read_blocks: wgpu::BindGroup,
}

/// The textures that the layer pass of one layer reads.
struct Target {
    size: [u32; 2],
    raw: wgpu::TextureView,
    clean: wgpu::TextureView,
    /// The group that reads `raw`.
    read_raw: wgpu::BindGroup,
    /// The group that reads `clean`.
    read_clean: wgpu::BindGroup,
    nearest: wgpu::Texture,
    nearest_view: wgpu::TextureView,
    group: wgpu::BindGroup,
}

/// The mesh of the flat view.
pub struct Mesh<'a> {
    pub vertices: &'a wgpu::Buffer,
    pub indices: &'a wgpu::Buffer,
    pub count: u32,
}

/// Where the passes of a layer run.
pub struct Extent {
    /// The size of the render target in pixels.
    pub size: [u32; 2],
    /// The part of the target that the layer pass can draw, in pixels: the
    /// left, the top, the right, and the bottom.
    pub area: [f32; 4],
    /// The radius of the search for the nearest place, in pixels.
    pub radius: u32,
}

/// The bind groups of the layer pass that the passes of the heights read.
pub struct Groups<'a> {
    pub uniforms: &'a wgpu::BindGroup,
    pub tiles: &'a wgpu::BindGroup,
    pub levels: &'a wgpu::BindGroup,
}

/// The pipelines and the textures of the lines of the stops.
pub struct Edges {
    /// The layout of the last group of the passes before the layer pass.
    layout: wgpu::BindGroupLayout,
    /// The layout of the last group of the layer pass.
    resolve: wgpu::BindGroupLayout,
    heights: wgpu::RenderPipeline,
    flat_heights: wgpu::RenderPipeline,
    clean: wgpu::RenderPipeline,
    seeds: wgpu::RenderPipeline,
    blocks: wgpu::RenderPipeline,
    nearest: wgpu::RenderPipeline,
    no_heights: wgpu::TextureView,
    no_places: wgpu::TextureView,
    /// The group of a layer pass with no lines.
    none: wgpu::BindGroup,
    scratch: Option<Scratch>,
    targets: HashMap<LayerKey, Target>,
}

impl Edges {
    /// `shader` holds `globe.wgsl` and `edges.wgsl`. `globe`, `tiles`, and
    /// `levels` are the layouts of the first 3 groups of the layer pass, and
    /// `vertex` is the layout of a vertex of the flat view.
    pub fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        globe: &wgpu::BindGroupLayout,
        tiles: &wgpu::BindGroupLayout,
        levels: &wgpu::BindGroupLayout,
        vertex: wgpu::VertexBufferLayout<'_>,
    ) -> Edges {
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let float = wgpu::TextureSampleType::Float { filterable: false };
        let uint = wgpu::TextureSampleType::Uint;
        // The passes before the layer pass read the heights, the places, the
        // cut heights, and the blocks. The layer pass reads the heights, the
        // cut heights, and the nearest places, in a group of its own, so
        // that it keeps under the limit of the textures of one pass.
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edges"),
            entries: &[
                texture(0, float),
                texture(1, uint),
                texture(2, float),
                texture(4, uint),
            ],
        });
        let resolve = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edge lines"),
            entries: &[texture(0, float), texture(2, float), texture(3, uint)],
        });
        let with_tiles = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("edge heights"),
            bind_group_layouts: &[Some(globe), Some(tiles), Some(levels)],
            immediate_size: 0,
        });
        let with_edges = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("edge places"),
            bind_group_layouts: &[Some(globe), None, None, Some(&layout)],
            immediate_size: 0,
        });
        let target = |format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        };
        let pipeline = |label,
                        layout,
                        vertex_entry,
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                        fragment,
                        targets: &[Option<wgpu::ColorTargetState>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some(vertex_entry),
                    buffers,
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(fragment),
                    targets,
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let heights = [target(SHOWN)];
        let flat = [Some(vertex)];
        let empty = |label, format| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            texture.create_view(&Default::default())
        };
        let no_heights = empty("no edge heights", HEIGHTS);
        let no_places = empty("no edge places", PLACES);
        let h = &no_heights;
        let none = group(device, &resolve, &[(0, h), (2, h), (3, &no_places)]);
        Edges {
            heights: pipeline(
                "edge heights",
                &with_tiles,
                "vs_main",
                &[],
                "fs_heights",
                &heights,
            ),
            flat_heights: pipeline(
                "flat edge heights",
                &with_tiles,
                "vs_flat",
                &flat,
                "fs_flat_heights",
                &heights,
            ),
            clean: pipeline(
                "edge clean",
                &with_edges,
                "vs_main",
                &[],
                "fs_edge_clean",
                &[target(HEIGHTS)],
            ),
            seeds: pipeline(
                "edge seeds",
                &with_edges,
                "vs_main",
                &[],
                "fs_edge_seeds",
                &[target(PLACES)],
            ),
            blocks: pipeline(
                "edge blocks",
                &with_edges,
                "vs_main",
                &[],
                "fs_edge_blocks",
                &[target(PLACES)],
            ),
            nearest: pipeline(
                "edge nearest",
                &with_edges,
                "vs_main",
                &[],
                "fs_edge_nearest",
                &[target(PLACES)],
            ),
            layout,
            resolve,
            no_heights,
            no_places,
            none,
            scratch: None,
            targets: HashMap::new(),
        }
    }

    /// The layout of the last group of the layer pass.
    pub fn resolve_layout(&self) -> &wgpu::BindGroupLayout {
        &self.resolve
    }

    /// The last group of the layer pass of a layer.
    pub fn group(&self, layer: LayerKey) -> &wgpu::BindGroup {
        self.targets.get(&layer).map_or(&self.none, |t| &t.group)
    }

    /// Keeps the textures of the layers that `live` keeps.
    pub fn retain(&mut self, live: impl Fn(&LayerKey) -> bool) {
        self.targets.retain(|layer, _| live(layer));
    }

    /// Encodes the passes of the lines of one layer for a render target of
    /// `size` pixels. `mesh` is the mesh of the flat view, or `None` on the
    /// globe.
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        layer: LayerKey,
        extent: Extent,
        groups: Groups<'_>,
        mesh: Option<Mesh<'_>>,
    ) {
        let Extent { size, area, radius } = extent;
        let size = size.map(|s| s.max(1));
        if self.scratch.as_ref().is_none_or(|s| s.size != size) {
            self.scratch = None;
            let seeds = view(device, "edge seeds", size, PLACES);
            let blocks = view(
                device,
                "edge blocks",
                size.map(|s| s.div_ceil(BLOCK)),
                PLACES,
            );
            let (h, p) = (&self.no_heights, &self.no_places);
            let read = |seeds, blocks| [(0, h), (1, seeds), (2, h), (4, blocks)];
            self.scratch = Some(Scratch {
                size,
                read_seeds: group(device, &self.layout, &read(&seeds, p)),
                read_blocks: group(device, &self.layout, &read(&seeds, &blocks)),
                seeds,
                blocks,
            });
        }
        if self.targets.get(&layer).is_none_or(|t| t.size != size) {
            self.targets.remove(&layer);
            let raw = view(device, "edge raw", size, SHOWN);
            let clean = view(device, "edge clean", size, HEIGHTS);
            let nearest = texture(device, "edge nearest", size, PLACES);
            let nearest_view = nearest.create_view(&Default::default());
            let (h, p) = (&self.no_heights, &self.no_places);
            let read = |raw, clean| [(0, raw), (1, p), (2, clean), (4, p)];
            let target = Target {
                size,
                read_raw: group(device, &self.layout, &read(&raw, h)),
                read_clean: group(device, &self.layout, &read(h, &clean)),
                group: group(
                    device,
                    &self.resolve,
                    &[(0, &raw), (2, &clean), (3, &nearest_view)],
                ),
                raw,
                clean,
                nearest,
                nearest_view,
            };
            self.targets.insert(layer, target);
        }
        let scratch = self.scratch.as_ref().expect("made above");
        let target = &self.targets[&layer];
        let attachment = |view, clear: f64| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: clear,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        let empty = f64::from(EMPTY);
        // The layer pass reads the cut heights up to 5 pixels past the
        // nearest place. A place reads the cut heights 1 pixel away, and a
        // cut height reads the heights 1 pixel away.
        let places = scissor(area, radius + 6, size);
        let clean = scissor(area, radius + 7, size);
        let heights = scissor(area, radius + 8, size);
        let nearest = scissor(area, 1, size);
        let blocks = size.map(|s| s.div_ceil(BLOCK));
        let clip = |pass: &mut wgpu::RenderPass<'_>, rect: Option<[u32; 4]>| match rect {
            Some([x, y, w, h]) => {
                pass.set_scissor_rect(x, y, w, h);
                true
            }
            None => false,
        };
        {
            let mut pass = begin(encoder, "edge heights", &[attachment(&target.raw, -1.0)]);
            if !clip(&mut pass, heights) {
                return;
            }
            pass.set_bind_group(0, groups.uniforms, &[]);
            pass.set_bind_group(1, groups.tiles, &[]);
            pass.set_bind_group(2, groups.levels, &[]);
            match &mesh {
                None => {
                    pass.set_pipeline(&self.heights);
                    pass.draw(0..3, 0..1);
                }
                Some(mesh) => {
                    pass.set_pipeline(&self.flat_heights);
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.count, 0, 0..1);
                }
            }
        }
        {
            let mut pass = begin(encoder, "edge clean", &[attachment(&target.clean, -1.0)]);
            clip(&mut pass, clean);
            pass.set_pipeline(&self.clean);
            pass.set_bind_group(0, groups.uniforms, &[]);
            pass.set_bind_group(3, &target.read_raw, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = begin(encoder, "edge seeds", &[attachment(&scratch.seeds, empty)]);
            clip(&mut pass, places);
            pass.set_pipeline(&self.seeds);
            pass.set_bind_group(0, groups.uniforms, &[]);
            pass.set_bind_group(3, &target.read_clean, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(places) = places {
            let mut pass = begin(encoder, "edge blocks", &[attachment(&scratch.blocks, 0.0)]);
            clip(&mut pass, Some(block_scissor(places, blocks)));
            pass.set_pipeline(&self.blocks);
            pass.set_bind_group(0, groups.uniforms, &[]);
            pass.set_bind_group(3, &scratch.read_seeds, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = begin(
                encoder,
                "edge nearest",
                &[attachment(&target.nearest_view, empty)],
            );
            clip(&mut pass, nearest);
            pass.set_pipeline(&self.nearest);
            pass.set_bind_group(0, groups.uniforms, &[]);
            pass.set_bind_group(3, &scratch.read_blocks, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// The nearest places of each layer: the size and the payloads, row by
    /// row. It waits for the GPU.
    pub fn read_nearest(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Vec<(LayerKey, [u32; 2], Vec<u32>)> {
        let read = |target| read_places(device, queue, target);
        let found = self.targets.iter();
        found
            .filter_map(|(layer, target)| Some((*layer, target.size, read(target)?)))
            .collect()
    }
}

/// The payloads of the nearest places of a target, row by row.
fn read_places(device: &wgpu::Device, queue: &wgpu::Queue, target: &Target) -> Option<Vec<u32>> {
    let [w, h] = target.size;
    let row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("edge nearest readback"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        target.nearest.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        target.nearest.size(),
    );
    queue.submit([encoder.finish()]);
    buffer.map_async(wgpu::MapMode::Read, .., |_| {});
    let wait = wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    };
    device.poll(wait).ok()?;
    let bytes = buffer.slice(..).get_mapped_range().ok()?;
    let rows = bytes.chunks(row as usize);
    let payloads = rows
        .flat_map(|r| r[..w as usize * 4].chunks(4))
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    Some(payloads)
}

fn begin<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    label: &str,
    targets: &[Option<wgpu::RenderPassColorAttachment<'_>>],
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: targets,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    size: [u32; 2],
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn view(
    device: &wgpu::Device,
    label: &str,
    size: [u32; 2],
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    texture(device, label, size, format).create_view(&Default::default())
}

fn group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    views: &[(u32, &wgpu::TextureView)],
) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry<'_>> = views
        .iter()
        .map(|&(binding, view)| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        })
        .collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("edges"),
        layout,
        entries: &entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scissor_grows_the_area_and_stays_in_the_target() {
        let size = [100, 80];
        assert_eq!(
            scissor([10.0, 20.0, 50.5, 60.0], 4, size),
            Some([6, 16, 49, 48])
        );
        assert_eq!(
            scissor([-5.0, 70.0, 120.0, 90.0], 2, size),
            Some([0, 68, 100, 12])
        );
        assert_eq!(scissor([200.0, 0.0, 300.0, 10.0], 4, size), None);
        assert_eq!(block_scissor([6, 16, 49, 48], [13, 10]), [0, 2, 7, 6]);
    }

    fn line(half: f32) -> LineUniform {
        [[0.0, 0.0, 0.0, half], [0.5, 0.0, 0.0, 0.0]]
    }

    #[test]
    fn the_widest_line_sets_the_radius() {
        let lines = [line(0.0), line(1.5), line(3.2), line(0.0)];
        assert_eq!(max_half(&lines), 3.2);
        assert_eq!(radius(max_half(&lines)), Some(6));
    }

    #[test]
    fn no_line_has_no_radius() {
        assert_eq!(max_half(&[line(0.0), line(0.0)]), 0.0);
        assert_eq!(radius(0.0), None);
    }

    #[test]
    fn a_wide_line_is_cut_so_its_offsets_fit() {
        assert_eq!(max_half(&[line(80.0)]), MAX_HALF);
        let r = radius(80.0).unwrap() as i32;
        assert!(r <= MAX_OFFSET);
        // A pixel at the radius reaches a place one pixel back on either side.
        for base in [[-r, r - 1], [r - 1, -r]] {
            let seed = Seed {
                base,
                axis: false,
                t: 0,
                stop: 0,
            };
            assert!(pack(seed).is_some());
        }
    }

    #[test]
    fn a_payload_keeps_each_field() {
        for base in [[0, 0], [-32, 31], [31, -32], [-1, 5]] {
            for axis in [false, true] {
                for t in [0, 1, 37, T_STEPS] {
                    for stop in [0, 7, 32, 63] {
                        let seed = Seed {
                            base,
                            axis,
                            t,
                            stop,
                        };
                        let payload = pack(seed).unwrap();
                        assert_ne!(payload, EMPTY);
                        assert_eq!(unpack(payload), Some(seed));
                    }
                }
            }
        }
    }

    #[test]
    fn a_field_that_does_not_fit_has_no_payload() {
        let seed = Seed {
            base: [0, 0],
            axis: false,
            t: 0,
            stop: 0,
        };
        assert!(
            pack(Seed {
                base: [32, 0],
                ..seed
            })
            .is_none()
        );
        assert!(
            pack(Seed {
                base: [0, -33],
                ..seed
            })
            .is_none()
        );
        assert!(
            pack(Seed {
                t: T_STEPS + 1,
                ..seed
            })
            .is_none()
        );
        assert!(pack(Seed { stop: 64, ..seed }).is_none());
        assert_eq!(unpack(EMPTY), None);
    }

    #[test]
    fn rebase_moves_the_offset_to_the_new_pixel() {
        let seed = Seed {
            base: [2, -3],
            axis: true,
            t: 20,
            stop: 4,
        };
        let moved = rebase(pack(seed).unwrap(), [1, -1]).unwrap();
        assert_eq!(
            unpack(moved),
            Some(Seed {
                base: [1, -2],
                ..seed
            })
        );
        assert_eq!(rebase(EMPTY, [1, 0]), None);
        let far = Seed {
            base: [MAX_OFFSET, 0],
            ..seed
        };
        assert_eq!(rebase(pack(far).unwrap(), [-1, 0]), None);
    }
}
