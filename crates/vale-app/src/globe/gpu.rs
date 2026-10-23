//! The wgpu side: the heightmaps on the GPU and the paint callbacks of the
//! globe.

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use eframe::egui_wgpu::{self, wgpu};

use std::collections::HashMap;

use vale_store::RasterId;
use vale_terrain::{
    ChannelMap, ChannelWindow, ErodeStep, FACES, FIELD_WGSL, GpuHeightmap, GpuPyramid, GpuTiles,
    Heightmap, MAX_BANDS, Readback, StampPlan, TILES_WGSL, TexelRect, pyramid_layout, tiles_layout,
};

use super::backdrop::{Backdrop, Canvas, REDUCTION};
use super::edges::{self, Edges};
use super::flat::{FlatMesh, Vertex};
use super::preview::{BandUniform, LineUniform};

/// The time between two polls of the device while GPU work is in flight.
const POLL_STEP: Duration = Duration::from_micros(250);

/// The most texels that one call sends. 8,192 x 512 texels are 8 MB.
const UPLOAD_TEXELS: usize = 1 << 22;

/// The values that the shader reads. The layout matches `globe.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub rot: [[f32; 4]; 3],
    pub globe: [f32; 4],
    pub params: [f32; 4],
    pub flat: [f32; 4],
    pub screen: [f32; 4],
    pub window: [f32; 4],
    pub rivers: [f32; 4],
    /// The opacity of the layer, from 0 to 1.
    pub layer: [f32; 4],
    /// The opacity of the hillshade, from 0 to 1, 1 for the highlights, the
    /// height of the range of the values over the radius of the world, and
    /// the radius of the world in meters.
    pub shade: [f32; 4],
    /// The direction of the light of the hillshade in east, north, and up,
    /// and the exaggeration of the slopes.
    pub light: [f32; 4],
    /// The shade color of the hillshade.
    pub shadow: [f32; 4],
    /// The color of the ground below the layers.
    pub ground: [f32; 4],
    /// The color of the canvas around the globe.
    pub back: [f32; 4],
    /// The level of the smooth field that the layer shows, and the coarsest
    /// level of its source.
    pub detail: [f32; 4],
    /// 1 when the passes of the lines of the stops ran for this layer, then
    /// the radius of their search in pixels.
    pub edges: [f32; 4],
    pub bands: [BandUniform; MAX_BANDS],
    pub lines: [LineUniform; MAX_BANDS + 1],
}

/// The mesh of the flat view on the GPU.
struct GpuMesh {
    spec: vale_sphere::ProjectionSpec,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
}

impl GpuMesh {
    fn new(device: &wgpu::Device, mesh: &FlatMesh) -> GpuMesh {
        use wgpu::util::DeviceExt;
        let buffer = |label, contents, usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
        };
        GpuMesh {
            spec: mesh.spec,
            vertices: buffer(
                "flat vertices",
                bytemuck::cast_slice(&mesh.vertices),
                wgpu::BufferUsages::VERTEX,
            ),
            indices: buffer(
                "flat indices",
                bytemuck::cast_slice(&mesh.indices),
                wgpu::BufferUsages::INDEX,
            ),
            count: mesh.indices.len() as u32,
        }
    }
}

/// The layer that a heightmap on the GPU belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayerKey {
    /// The heightmap of a globe that draws no list of layers.
    Solo,
    Raster(RasterId),
}

/// One heightmap on the GPU. The GPU takes a change only if it still holds
/// the heightmap with this key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MapKey {
    pub layer: LayerKey,
    /// The count of the heightmaps that the globe made before this one.
    pub generation: u64,
    /// True: the reduced copy that the globe draws. False: the full
    /// heightmap that the brush paints.
    pub display: bool,
    /// Texels on one edge of a face of this heightmap.
    pub face_size: u32,
}

/// What one pass draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pass {
    /// The plain ground below the layers.
    Ground,
    /// One raster layer.
    Layer(LayerKey),
    /// The graticule above the raster layers.
    Graticule,
    /// The soft edge of the globe. The other passes have a hard edge, so
    /// the edge has the cover of one pass.
    Rim,
}

/// The uniform buffer of one pass, with its bind group.
struct Slot {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// The number of the channel texture of the bind group, and the
    /// generation of the full heightmap that gives the window texture.
    /// `None`: the texture is empty.
    rivers: (Option<u64>, Option<u64>),
}

/// The channel map of the rivers of one layer on the GPU.
struct Channels {
    /// The count of the channel textures before this one.
    number: u64,
    size: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct Resources {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    flat_pipeline: wgpu::RenderPipeline,
    /// The passes that read no heightmap: the ground and the graticule, on
    /// the globe and in the flat view, and the edge of the globe.
    plain: [wgpu::RenderPipeline; 5],
    mesh: Option<GpuMesh>,
    /// Channel textures with no river, for a pass with no rivers.
    no_channels: wgpu::TextureView,
    no_window: wgpu::TextureView,
    /// The pass and the target of each slot. True: the small target of the
    /// blurred copy.
    slots: HashMap<(Pass, bool), Slot>,
    /// The heightmap that the brush paints.
    full: Option<(MapKey, GpuHeightmap)>,
    /// The reduced copies that the globe draws.
    copies: HashMap<MapKey, GpuTiles>,
    /// The smooth levels of each heightmap that the globe draws.
    pyramids: HashMap<MapKey, GpuPyramid>,
    /// The channel map of each visible height layer.
    channels: HashMap<LayerKey, Channels>,
    channel_count: u64,
    /// The lines of the stops.
    edges: Edges,
    /// The count of the changes of the heightmaps on the GPU and of the
    /// mesh of the flat view.
    version: u64,
    _poller: Poller,
}

const GROUND: usize = 0;
const GRATICULE: usize = 1;
const FLAT: usize = 2;
const RIM: usize = 4;

impl Resources {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, busy: &Arc<Busy>) -> Resources {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("globe"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{TILES_WGSL}\n{FIELD_WGSL}\n{}\n{}",
                    include_str!("globe.wgsl"),
                    include_str!("edges.wgsl")
                )
                .into(),
            ),
        });
        let texture = |binding, view_dimension| wgpu::BindGroupLayoutEntry {
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
            label: Some("globe"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(1, wgpu::TextureViewDimension::D2Array),
                texture(2, wgpu::TextureViewDimension::D2),
            ],
        });
        let tiles = tiles_layout(device);
        let levels = pyramid_layout(device);
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x3],
        };
        let edges = Edges::new(
            device,
            &shader,
            &layout,
            &tiles,
            &levels,
            vertex_layout.clone(),
        );
        let with_tiles = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("globe"),
            bind_group_layouts: &[
                Some(&layout),
                Some(&tiles),
                Some(&levels),
                Some(edges.resolve_layout()),
            ],
            immediate_size: 0,
        });
        let no_tiles = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("globe plain"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label, layout, vertex, fragment, flat: bool| {
            let buffers = [Some(vertex_layout.clone())];
            let buffers: &[Option<wgpu::VertexBufferLayout<'_>>] =
                if flat { &buffers } else { &[] };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
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
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let empty = |label, layers, dimension| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Uint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(dimension),
                ..Default::default()
            })
        };
        Resources {
            format,
            flat_pipeline: pipeline("flat", &with_tiles, "vs_flat", "fs_flat", true),
            pipeline: pipeline("globe", &with_tiles, "vs_main", "fs_main", false),
            plain: [
                pipeline("ground", &no_tiles, "vs_main", "fs_ground", false),
                pipeline("graticule", &no_tiles, "vs_main", "fs_graticule", false),
                pipeline("flat ground", &no_tiles, "vs_flat", "fs_flat_ground", true),
                pipeline(
                    "flat graticule",
                    &no_tiles,
                    "vs_flat",
                    "fs_flat_graticule",
                    true,
                ),
                pipeline("rim", &no_tiles, "vs_main", "fs_rim", false),
            ],
            mesh: None,
            no_channels: empty(
                "no channels",
                FACES as u32,
                wgpu::TextureViewDimension::D2Array,
            ),
            no_window: empty("no window", 1, wgpu::TextureViewDimension::D2),
            layout,
            slots: HashMap::new(),
            full: None,
            copies: HashMap::new(),
            pyramids: HashMap::new(),
            channels: HashMap::new(),
            channel_count: 0,
            edges,
            version: 0,
            _poller: Poller::new(device.clone(), busy.clone()),
        }
    }

    /// Makes the heightmaps on the GPU those of the frame. A heightmap with
    /// another key goes away, and a new one starts with no texels.
    fn sync(&mut self, device: &wgpu::Device, frame: &Frame) {
        let before = (self.copies.len(), self.pyramids.len());
        let mut changed = false;
        if self.full.as_ref().map(|(key, _)| *key) != frame.full {
            changed = true;
            // The old heightmap goes away before the new one takes memory.
            self.full = None;
            self.full = frame
                .full
                .map(|key| (key, GpuHeightmap::new(device, key.face_size)));
        }
        self.copies.retain(|key, _| frame.copies.contains(key));
        changed |= self.copies.len() != before.0;
        for key in &frame.copies {
            self.copies.entry(*key).or_insert_with(|| {
                changed = true;
                GpuTiles::new(device, key.face_size, 0)
            });
        }
        let full = frame.full.filter(|_| frame.smooth_full);
        let smooth = |key: &MapKey| Some(*key) == full || frame.copies.contains(key);
        self.pyramids.retain(|key, _| smooth(key));
        changed |= self.pyramids.len() != before.1;
        for key in full.iter().chain(&frame.copies) {
            self.pyramids.entry(*key).or_insert_with(|| {
                changed = true;
                GpuPyramid::new(device, key.face_size)
            });
        }
        if changed {
            self.version += 1;
        }
        let live = |pass: &Pass| match pass {
            Pass::Layer(layer) => {
                let full = frame.full.is_some_and(|key| key.layer == *layer);
                full || frame.copies.iter().any(|key| key.layer == *layer)
            }
            Pass::Ground | Pass::Graticule | Pass::Rim => true,
        };
        self.slots.retain(|(pass, _), _| live(pass));
        self.edges.retain(|layer| live(&Pass::Layer(*layer)));
        self.channels
            .retain(|layer, _| frame.rivers.contains(layer));
    }

    /// Writes the channel map of the rivers of one layer.
    fn upload_channels(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: LayerKey,
        map: &ChannelMap,
    ) {
        let size = map.size() as u32;
        if self.channels.get(&layer).is_none_or(|c| c.size != size) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("layer channel map"),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: FACES as u32,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Uint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
            self.channel_count += 1;
            let channels = Channels {
                number: self.channel_count,
                size,
                texture,
                view,
            };
            self.channels.insert(layer, channels);
        }
        let channels = &self.channels[&layer];
        queue.write_texture(
            channels.texture.as_image_copy(),
            map.bytes(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size * 4),
                rows_per_image: Some(size),
            },
            channels.texture.size(),
        );
    }

    /// The memory of the channel maps of the layers, in bytes.
    fn river_bytes(&self) -> u64 {
        let bytes = |c: &Channels| u64::from(c.size) * u64::from(c.size) * 4 * FACES as u64;
        self.channels.values().map(bytes).sum()
    }

    /// The memory of the reduced copies with their smooth levels, in bytes.
    fn copy_bytes(&self) -> u64 {
        let bytes = self.copies.iter().map(|(key, tiles)| {
            let levels = self.pyramids.get(key).map_or(0, GpuPyramid::memory_bytes);
            tiles.memory_bytes() as u64 + levels
        });
        bytes.sum()
    }

    /// Writes the uniforms of one pass. `small`: the pass draws into the
    /// small target of the blurred copy.
    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, draw: &Draw, small: bool) {
        // A layer has its channel map. Only the full heightmap has the
        // window of small rivers.
        let layer = match draw.pass {
            Pass::Layer(layer) if draw.rivers => self.channels.get(&layer),
            _ => None,
        };
        let window = match (&self.full, draw.source) {
            (Some((key, _)), Some(source)) if *key == source && layer.is_some() => {
                Some(key.generation)
            }
            _ => None,
        };
        let rivers = (layer.map(|channels| channels.number), window);
        let stale = |slot: &Slot| slot.rivers != rivers;
        if self.slots.get(&(draw.pass, small)).is_none_or(stale) {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("globe uniforms"),
                size: size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let channels = layer.map_or(&self.no_channels, |channels| &channels.view);
            let window = match &self.full {
                Some((_, heights)) if window.is_some() => heights.window_view(),
                _ => &self.no_window,
            };
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("globe"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(channels),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(window),
                    },
                ],
            });
            let slot = Slot {
                buffer,
                bind_group,
                rivers,
            };
            self.slots.insert((draw.pass, small), slot);
        }
        let slot = &self.slots[&(draw.pass, small)];
        queue.write_buffer(&slot.buffer, 0, bytemuck::bytes_of(&draw.uniforms));
    }

    /// The groups of the heightmap that a layer pass reads: its tiles and
    /// its smooth levels. `None`: the GPU does not have them.
    fn layer_groups(&self, draw: &Draw) -> Option<(wgpu::BindGroup, wgpu::BindGroup)> {
        let source = draw.source?;
        let tiles = match &self.full {
            Some((key, heights)) if source == *key => heights.tiles(),
            _ => self.copies.get(&source)?.tiles(),
        };
        Some((tiles, self.pyramids.get(&source)?.bind_group().clone()))
    }

    /// Encodes the passes of the lines of the stops of a layer pass, for a
    /// render target of `size` pixels. Returns the radius of their search,
    /// or `None` when the pass draws no lines.
    fn encode_edges(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        draw: &Draw,
        size: [u32; 2],
        flat: bool,
    ) -> Option<u32> {
        let Pass::Layer(layer) = draw.pass else {
            return None;
        };
        let radius = edges::radius(edges::max_half(&draw.uniforms.lines)).filter(|_| draw.lines)?;
        let (tiles, levels) = self.layer_groups(draw)?;
        let uniforms = self.slots.get(&(draw.pass, false))?.bind_group.clone();
        let mesh = match (&self.mesh, flat) {
            (_, false) => None,
            (Some(mesh), true) => Some(edges::Mesh {
                vertices: &mesh.vertices,
                indices: &mesh.indices,
                count: mesh.count,
            }),
            (None, true) => return None,
        };
        let groups = edges::Groups {
            uniforms: &uniforms,
            tiles: &tiles,
            levels: &levels,
        };
        // The canvas, and on the globe the box around its disk.
        let mut area = draw.clip;
        if !flat {
            let [x, y, r, _] = draw.uniforms.globe;
            area = [
                area[0].max(x - r),
                area[1].max(y - r),
                area[2].min(x + r),
                area[3].min(y + r),
            ];
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let key = (
            bytemuck::bytes_of(&draw.uniforms),
            draw.source,
            self.version,
        );
        (key, size, area.map(f32::to_bits), radius).hash(&mut hasher);
        let extent = edges::Extent {
            size,
            area,
            radius,
            inputs: hasher.finish(),
        };
        self.edges
            .encode(device, encoder, layer, extent, groups, mesh);
        Some(radius)
    }

    /// Draws one pass on the globe, or on the mesh of the flat view. `screen`
    /// is the size of the render target in pixels.
    fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        draw: &Draw,
        small: bool,
        flat: bool,
        screen: [u32; 2],
    ) {
        let Some(slot) = self.slots.get(&(draw.pass, small)) else {
            return;
        };
        let kind = match draw.pass {
            Pass::Ground => Some(GROUND),
            Pass::Graticule => Some(GRATICULE),
            Pass::Rim => Some(RIM),
            Pass::Layer(_) => None,
        };
        // The flat view has no edge of a globe.
        if flat && kind == Some(RIM) {
            return;
        }
        let pipeline = match kind {
            Some(RIM) => &self.plain[RIM],
            Some(kind) => &self.plain[kind + if flat { FLAT } else { 0 }],
            None if flat => &self.flat_pipeline,
            None => &self.pipeline,
        };
        if let Pass::Layer(layer) = draw.pass {
            let Some((tiles, levels)) = self.layer_groups(draw) else {
                return;
            };
            pass.set_bind_group(1, &tiles, &[]);
            pass.set_bind_group(2, &levels, &[]);
            pass.set_bind_group(3, self.edges.group(layer), &[]);
        }
        pass.set_bind_group(0, &slot.bind_group, &[]);
        pass.set_pipeline(pipeline);
        if !flat {
            pass.draw(0..3, 0..1);
        } else if let Some(mesh) = &self.mesh {
            // The shader places the mesh on the whole render target.
            pass.set_viewport(0.0, 0.0, screen[0] as f32, screen[1] as f32, 0.0, 1.0);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.count, 0, 0..1);
        }
    }
}

/// One change of the GPU heightmap. The changes run in the order of the queue.
pub enum Op {
    /// Sets all texels to one level.
    Reset(u16),
    /// Writes texels of the CPU heightmap.
    Upload {
        face: u32,
        rect: TexelRect,
        data: Vec<u16>,
    },
    /// One stamp of a stroke. `time` is the frame that read its pen sample.
    Stamp {
        plan: Box<StampPlan>,
        stroke: u64,
        time: Instant,
    },
    /// One step of an erode stroke.
    Erode { stroke: u64, step: Arc<ErodeStep> },
    /// Reads texels back for the CPU heightmap.
    Readback {
        stroke: u64,
        face: u32,
        rect: TexelRect,
    },
    /// Writes the channel map of the rivers of a layer, for the picture.
    LayerChannels(Arc<ChannelMap>),
    /// Writes the channel map of the rivers.
    Channels(Arc<ChannelMap>),
    /// Writes the window of small rivers. `None`: the brush follows the
    /// channel map only.
    Window(Option<Arc<ChannelWindow>>),
}

/// A result of the GPU work. The UI reads the results at the next frame.
pub enum Event {
    /// The GPU work for the stamps of one pen sample is done.
    Done {
        stroke: u64,
        sample: Instant,
        done: Instant,
    },
    /// The number of passes that the stamps of one frame made.
    Passes { stroke: u64, passes: u32 },
    Texels {
        stroke: u64,
        face: u32,
        rect: TexelRect,
        data: Vec<u16>,
    },
}

/// The count of the results that the GPU has not given yet.
#[derive(Default)]
pub struct Busy {
    count: Mutex<usize>,
    wake: Condvar,
}

impl Busy {
    pub fn any(&self) -> bool {
        *self.count.lock().expect("no panic holds the lock") > 0
    }

    fn token(self: &Arc<Busy>) -> Token {
        *self.count.lock().expect("no panic holds the lock") += 1;
        self.wake.notify_all();
        Token(self.clone())
    }
}

/// One result that the GPU has not given yet. A callback of wgpu holds it.
struct Token(Arc<Busy>);

impl Drop for Token {
    fn drop(&mut self) {
        *self.0.count.lock().expect("no panic holds the lock") -= 1;
    }
}

/// A thread that polls the device while results are in flight. wgpu runs a
/// callback only during a submit or a poll, so without the thread a result
/// arrives one frame late.
struct Poller {
    quit: Arc<AtomicBool>,
    busy: Arc<Busy>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Poller {
    fn new(device: wgpu::Device, busy: Arc<Busy>) -> Poller {
        let quit = Arc::new(AtomicBool::new(false));
        let (thread_quit, thread_busy) = (quit.clone(), busy.clone());
        let run = move || {
            let (quit, busy) = (thread_quit, thread_busy);
            loop {
                {
                    let mut count = busy.count.lock().expect("no panic holds the lock");
                    while *count == 0 && !quit.load(Ordering::Relaxed) {
                        count = busy.wake.wait(count).expect("no panic holds the lock");
                    }
                }
                if quit.load(Ordering::Relaxed) {
                    return;
                }
                // A poll that does not wait holds no lock that a submit needs.
                let _ = device.poll(wgpu::PollType::Poll);
                std::thread::sleep(POLL_STEP);
            }
        };
        let thread = std::thread::Builder::new()
            .name("vale-gpu-poll".to_string())
            .spawn(run)
            .expect("the system can start a thread");
        Poller {
            quit,
            busy,
            thread: Some(thread),
        }
    }
}

impl Drop for Poller {
    fn drop(&mut self) {
        {
            let _count = self.busy.count.lock().expect("no panic holds the lock");
            self.quit.store(true, Ordering::Relaxed);
            self.busy.wake.notify_all();
        }
        // The thread holds the device, so it ends before the renderer does.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The path from the UI to the paint callback and back.
#[derive(Clone)]
pub struct Link {
    /// The changes that wait for the GPU, each with the tag of its
    /// heightmap. egui can run a frame and not paint it, so the queue keeps
    /// them until `prepare` runs.
    ops: Arc<Mutex<Vec<(MapKey, Op)>>>,
    events: mpsc::Sender<Event>,
    pub busy: Arc<Busy>,
    /// The memory of the reduced copies on the GPU, with the epoch of the
    /// frame that the GPU prepared last.
    copy_bytes: Arc<Mutex<(u64, u64, u64)>>,
}

impl Link {
    pub fn new() -> (Link, mpsc::Receiver<Event>) {
        let (events, receiver) = mpsc::channel();
        let link = Link {
            ops: Arc::default(),
            events,
            busy: Arc::default(),
            copy_bytes: Arc::default(),
        };
        (link, receiver)
    }

    pub fn push(&self, map: MapKey, op: Op) {
        let mut ops = self.ops.lock().expect("no panic holds the lock");
        ops.push((map, op));
    }

    /// The epoch of the last frame that the GPU prepared, and the memory of
    /// the reduced copies and of the channel maps of the layers on the GPU
    /// after it, in bytes.
    pub fn copy_bytes(&self) -> (u64, u64, u64) {
        *self.copy_bytes.lock().expect("no panic holds the lock")
    }
}

/// The changes of the CPU heightmap that went to the queue.
pub struct Changes {
    /// The whole heightmap changed. `rects` has the texels that are not at
    /// the base level.
    pub reset: bool,
    pub rects: Vec<(usize, TexelRect)>,
}

/// The changed texels of a CPU heightmap that wait for one heightmap on the
/// GPU.
pub struct Uploads {
    /// The strips of each face, in the order of the changes. A strip is in
    /// the texels of the heightmap on the GPU. `true`: the strip follows a
    /// reset, so a strip with the base level only needs no upload.
    strips: VecDeque<(usize, TexelRect, bool)>,
    /// The heightmap on the GPU gets its base level in each texel before
    /// the next strip.
    reset: bool,
    /// The most texels that one call of `Globe::queue_changes` sends, in
    /// texels of the CPU heightmaps.
    pub budget: usize,
}

impl Default for Uploads {
    fn default() -> Uploads {
        Uploads {
            strips: VecDeque::new(),
            reset: false,
            budget: UPLOAD_TEXELS,
        }
    }
}

impl Uploads {
    pub fn is_empty(&self) -> bool {
        self.strips.is_empty() && !self.reset
    }

    pub fn clear(&mut self) {
        self.strips.clear();
        self.reset = false;
    }

    /// Starts again for a heightmap on the GPU that needs each texel. The
    /// strips before the call are stale.
    pub fn restart(&mut self) {
        self.strips.clear();
        self.reset = true;
    }

    /// Adds a rectangle of the heightmap on the GPU. `factor` is the number
    /// of texels of the CPU heightmap along one side of a texel of the
    /// rectangle. `fresh`: the rectangle follows a reset.
    pub fn add(&mut self, face: usize, rect: TexelRect, factor: usize, budget: usize, fresh: bool) {
        let cut = strips(rect, (budget / (factor * factor)).max(1));
        self.strips.extend(cut.map(|strip| (face, strip, fresh)));
    }

    /// Moves strips to the queue of the GPU. A strip gets its texels when
    /// the call sends it, so it has each change up to that time. `room` is
    /// the count of texels of the CPU heightmap that the call can send.
    /// `first`: the call sends one strip at least. Returns the count that
    /// the call sent.
    pub fn send(
        &mut self,
        map: &Heightmap,
        key: MapKey,
        link: &Link,
        room: usize,
        first: bool,
    ) -> usize {
        let factor = map.face_size().div_ceil(key.face_size as usize).max(1);
        let factor = factor.next_power_of_two();
        if std::mem::take(&mut self.reset) {
            link.push(key, Op::Reset(map.base()));
        }
        let mut sent = 0;
        while let Some(&(face, rect, fresh)) = self.strips.front() {
            if fresh && map.reduced_is_base(face, rect, factor) {
                self.strips.pop_front();
                continue;
            }
            let cost = texels(rect) * factor * factor;
            if (sent > 0 || !first) && sent + cost > room {
                break;
            }
            self.strips.pop_front();
            sent += cost;
            let mut data = Vec::with_capacity(texels(rect));
            map.read_reduced(face, rect, factor, &mut data);
            let face = face as u32;
            link.push(key, Op::Upload { face, rect, data });
        }
        sent
    }
}

fn texels(rect: TexelRect) -> usize {
    (rect.x1 - rect.x0) * (rect.y1 - rect.y0)
}

/// Cuts a rectangle into strips of its full width. A strip has `texels`
/// texels at most, and one row at least.
fn strips(rect: TexelRect, texels: usize) -> impl Iterator<Item = TexelRect> {
    let rows = (texels / (rect.x1 - rect.x0).max(1)).max(1);
    (rect.y0..rect.y1).step_by(rows).map(move |y0| TexelRect {
        y0,
        y1: (y0 + rows).min(rect.y1),
        ..rect
    })
}

/// Takes the changes of a CPU heightmap.
pub fn take_changes(map: &mut Heightmap) -> Changes {
    let reset = map.take_reset();
    let rects = if reset {
        map.allocated_rects().collect()
    } else {
        let dirty = map.take_dirty().into_iter().enumerate();
        dirty
            .filter_map(|(face, rect)| Some((face, rect?)))
            .collect()
    };
    Changes { reset, rects }
}

/// The heightmaps on the GPU in one frame, and the path to the UI.
pub struct Frame {
    pub format: wgpu::TextureFormat,
    pub link: Link,
    /// The heightmap that the brush paints.
    pub full: Option<MapKey>,
    /// The reduced copies that the globe draws.
    pub copies: Vec<MapKey>,
    /// The globe draws a layer from `full`, so it needs its smooth levels.
    pub smooth_full: bool,
    /// The layers that have a channel map of their rivers.
    pub rivers: Vec<LayerKey>,
    /// `Some`: the canvas shows the flat view with this mesh.
    pub flat: Option<Arc<FlatMesh>>,
    /// The count of the sets of reduced copies before this one.
    pub epoch: u64,
}

/// One pass of a frame.
#[derive(Clone, Copy)]
pub struct Draw {
    pub pass: Pass,
    /// The heightmap that a layer pass reads.
    pub source: Option<MapKey>,
    /// The pass draws the rivers of its layer.
    pub rivers: bool,
    /// The layer pass draws the lines of its stops.
    pub lines: bool,
    /// The canvas in pixels: the left, the top, the right, and the bottom.
    pub clip: [f32; 4],
    pub uniforms: Uniforms,
}

/// The work of one callback.
pub enum Job {
    /// Draws one pass on the screen.
    Draw(Box<Draw>),
    /// Draws the passes into the small target of the blurred copy that the
    /// cards show, and blurs it. It draws nothing on the screen.
    Backdrop(Canvas, Vec<Draw>),
}

/// One callback of a frame of the globe. egui calls it inside its own
/// render pass. The callbacks of a frame draw in the order of the layers,
/// between the shapes of egui.
pub struct GlobeCallback {
    pub frame: Arc<Frame>,
    pub job: Job,
}

/// The commands of one `prepare` call that wait for a submit.
struct Batch<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    /// The heightmap that the brush paints.
    heights: Option<&'a GpuHeightmap>,
    full: Option<MapKey>,
    copies: &'a HashMap<MapKey, GpuTiles>,
    pyramids: &'a HashMap<MapKey, GpuPyramid>,
    link: &'a Link,
    encoder: Option<wgpu::CommandEncoder>,
    /// The stroke and the sample time of each stamp in the encoder.
    stamps: Vec<(u64, Instant)>,
    readbacks: Vec<(u64, u32, TexelRect, Readback)>,
    /// The number of stamp passes of each stroke.
    passes: Vec<(u64, u32)>,
}

impl Batch<'_> {
    fn encoder(&mut self) -> &mut wgpu::CommandEncoder {
        self.encoder.get_or_insert_with(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("heightmap"),
                })
        })
    }

    /// Submits the commands and asks for their results.
    fn submit(&mut self) {
        let Some(encoder) = self.encoder.take() else {
            return;
        };
        self.queue.submit([encoder.finish()]);
        let mut stamps = std::mem::take(&mut self.stamps);
        if let Some(heights) = self.heights.filter(|_| !stamps.is_empty()) {
            heights.begin_batch();
            // The stamps of one pen sample give one delay.
            stamps.dedup();
            let (events, token) = (self.link.events.clone(), self.link.busy.token());
            self.queue.on_submitted_work_done(move || {
                let done = Instant::now();
                for (stroke, sample) in stamps {
                    let _ = events.send(Event::Done {
                        stroke,
                        sample,
                        done,
                    });
                }
                drop(token);
            });
        }
        for (stroke, face, rect, readback) in self.readbacks.drain(..) {
            let (events, token) = (self.link.events.clone(), self.link.busy.token());
            readback.map(move |data| {
                let _ = events.send(Event::Texels {
                    stroke,
                    face,
                    rect,
                    data,
                });
                drop(token);
            });
        }
    }

    fn count_passes(&mut self, stroke: u64, passes: u32) {
        match self.passes.iter_mut().find(|(id, _)| *id == stroke) {
            Some((_, count)) => *count += passes,
            None => self.passes.push((stroke, passes)),
        }
    }

    /// Adds stamps that follow one another in the queue, in groups. `samples`
    /// has the stroke and the sample time of each stamp.
    fn stamp_run(&mut self, plans: &[StampPlan], samples: &[(u64, Instant)]) {
        let (Some(heights), queue) = (self.heights, self.queue) else {
            return;
        };
        let mut at = 0;
        let mut full = false;
        while at < plans.len() {
            let group = heights.stamp_group(queue, self.encoder(), &plans[at..]);
            if group == 0 {
                // An empty batch takes one stamp at least.
                if full {
                    return;
                }
                full = true;
                self.submit();
                heights.begin_batch();
                continue;
            }
            full = false;
            let end = at + group;
            let touched = |face: &usize| plans[at..end].iter().any(|p| p.rects[*face].is_some());
            let passes = (0..FACES).filter(touched).count() as u32;
            for plan in &plans[at..end] {
                self.mark_full(&plan.rects);
            }
            self.count_passes(samples[at].0, passes);
            self.stamps.extend(&samples[at..end]);
            at = end;
        }
    }

    /// Marks texels of the full heightmap as changed in its smooth levels.
    fn mark_full(&self, rects: &[Option<TexelRect>; FACES]) {
        let Some(levels) = self.full.and_then(|key| self.pyramids.get(&key)) else {
            return;
        };
        for (face, rect) in rects.iter().enumerate() {
            if let Some(rect) = rect {
                levels.mark(face as u32, *rect);
            }
        }
    }

    /// Changes the smooth levels of a heightmap for a reset or an upload.
    fn levels_follow(&mut self, key: MapKey, op: &Op) {
        let pyramids = self.pyramids;
        let Some(levels) = pyramids.get(&key) else {
            return;
        };
        match op {
            Op::Reset(level) => {
                let level = *level;
                levels.clear(self.encoder(), level);
            }
            Op::Upload { face, rect, .. } => levels.mark(*face, *rect),
            _ => {}
        }
    }

    /// Runs one change of the reduced copy with this key.
    fn run_copy(&mut self, key: MapKey, op: Op) {
        let Some(tiles) = self.copies.get(&key) else {
            return;
        };
        self.levels_follow(key, &op);
        match op {
            Op::Reset(level) => tiles.clear(self.queue, level),
            Op::Upload { face, rect, data } => tiles.upload(self.queue, face, rect, &data),
            _ => {}
        }
    }

    /// Makes the dirty parts of the smooth levels of each heightmap.
    fn update_levels(&mut self) {
        let pyramids = self.pyramids;
        for (key, levels) in pyramids {
            let tiles = match self.heights {
                Some(heights) if Some(*key) == self.full => heights.tiles(),
                _ => match self.copies.get(key) {
                    Some(tiles) => tiles.tiles(),
                    None => continue,
                },
            };
            levels.update(self.encoder(), &tiles);
        }
    }

    /// Runs one change of the heightmap that the brush paints.
    fn run(&mut self, op: Op) {
        let Some(heights) = self.heights else {
            return;
        };
        if let Some(key) = self.full {
            self.levels_follow(key, &op);
        }
        match op {
            Op::Reset(level) => {
                // The change runs before the commands of the next submit, so
                // the commands before it go first.
                self.submit();
                heights.clear(self.queue, level);
            }
            Op::Upload { face, rect, data } => {
                // The write runs before the commands of the next submit, so
                // the commands before it go first.
                self.submit();
                heights.upload(self.queue, face, rect, &data);
            }
            Op::Stamp { plan, stroke, time } => self.stamp_run(&[*plan], &[(stroke, time)]),
            Op::Erode { stroke, step } => {
                // The drops of the step go to the GPU at the next submit,
                // before the commands in the encoder. So the commands before
                // the step go first, and a submit holds one step.
                self.submit();
                let queue = self.queue;
                heights.begin_batch();
                let added = heights.erode(queue, self.encoder(), &step);
                debug_assert!(added, "an empty batch takes one step");
                self.mark_full(&step.rects());
                let passes = step.rects().iter().flatten().count() as u32;
                self.count_passes(stroke, passes);
            }
            Op::Readback { stroke, face, rect } => {
                let device = self.device;
                let readback = heights.read_rect(device, self.encoder(), face, rect);
                self.readbacks.push((stroke, face, rect, readback));
            }
            Op::LayerChannels(_) => {}
            Op::Channels(map) => {
                // The stamps before the write follow the rivers before it.
                self.submit();
                heights.upload_channels(self.queue, &map);
            }
            Op::Window(window) => {
                // A stamp in the encoder after this call follows the new
                // window, so the stamps before it go first.
                self.submit();
                heights.upload_window(self.queue, window.as_deref());
            }
        }
    }
}

/// Runs the changes that wait in the queue, on the heightmaps of the frame.
/// The first callback of a frame finds the changes, and each other one finds
/// an empty queue.
fn run_ops(device: &wgpu::Device, queue: &wgpu::Queue, res: &mut Resources, frame: &Frame) {
    res.sync(device, frame);
    let link = &frame.link;
    let mut ops = std::mem::take(&mut *link.ops.lock().expect("no panic holds the lock"));
    if !ops.is_empty() {
        res.version += 1;
    }
    // The channel maps of the layers are for the picture only, so their
    // order in the queue has no effect.
    ops.retain(|(map, op)| match op {
        Op::LayerChannels(channels) => {
            if frame.rivers.contains(&map.layer) {
                res.upload_channels(device, queue, map.layer, channels);
            }
            false
        }
        _ => true,
    });
    let mut batch = Batch {
        device,
        queue,
        heights: res.full.as_ref().map(|(_, heights)| heights),
        full: frame.full,
        copies: &res.copies,
        pyramids: &res.pyramids,
        link,
        encoder: None,
        stamps: Vec::new(),
        readbacks: Vec::new(),
        passes: Vec::new(),
    };
    // The stamps that follow one another in the queue.
    let (mut plans, mut samples) = (Vec::new(), Vec::new());
    for (map, op) in ops {
        if map.display {
            batch.run_copy(map, op);
            continue;
        }
        // A change of a heightmap that the app replaced.
        if Some(map) != batch.full {
            continue;
        }
        match op {
            Op::Stamp { plan, stroke, time } => {
                plans.push(*plan);
                samples.push((stroke, time));
            }
            op => {
                batch.stamp_run(&plans, &samples);
                plans.clear();
                samples.clear();
                batch.run(op);
            }
        }
    }
    batch.stamp_run(&plans, &samples);
    batch.update_levels();
    // This submit is before the submit of egui, so the frame shows the new
    // levels.
    batch.submit();
    for (stroke, passes) in batch.passes {
        let _ = link.events.send(Event::Passes { stroke, passes });
    }
    let bytes = (frame.epoch, res.copy_bytes(), res.river_bytes());
    *link.copy_bytes.lock().expect("no panic holds the lock") = bytes;
}

impl egui_wgpu::CallbackTrait for GlobeCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
        egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let frame = &*self.frame;
        let stale = resources
            .get::<Resources>()
            .is_none_or(|r| r.format != frame.format);
        if stale {
            let busy = &frame.link.busy;
            resources.insert(Resources::new(device, frame.format, busy));
        }
        let res: &mut Resources = resources.get_mut().expect("inserted above");
        run_ops(device, queue, res, frame);
        if let Some(mesh) = &frame.flat
            && res.mesh.as_ref().is_none_or(|m| m.spec != mesh.spec)
        {
            res.mesh = Some(GpuMesh::new(device, mesh));
            res.version += 1;
        }
        let size = screen_descriptor.size_in_pixels;
        let flat = frame.flat.is_some();
        match &self.job {
            Job::Draw(draw) => {
                let mut draw = **draw;
                draw.uniforms.screen = [size[0] as f32, size[1] as f32, 0.0, 0.0];
                // A write to the queue lands before each pass of the frame,
                // so the passes of the lines and the layer pass all read the
                // second write, with the radius.
                res.write(device, queue, &draw, false);
                if let Some(radius) = res.encode_edges(device, egui_encoder, &draw, size, flat) {
                    draw.uniforms.edges = [1.0, radius as f32, 0.0, 0.0];
                    res.write(device, queue, &draw, false);
                }
            }
            Job::Backdrop(canvas, draws) => {
                let small = size.map(|s| s.max(1).div_ceil(REDUCTION));
                for draw in draws {
                    let mut draw = *draw;
                    draw.uniforms.screen = [small[0] as f32, small[1] as f32, 0.0, 0.0];
                    res.write(device, queue, &draw, true);
                }
                let kept = resources.remove::<Backdrop>();
                let mut backdrop = kept
                    .filter(|b| b.format == frame.format)
                    .unwrap_or_else(|| Backdrop::new(device, frame.format));
                let res: &Resources = resources.get().expect("inserted above");
                backdrop.render(device, queue, egui_encoder, size, canvas, |pass| {
                    for draw in draws {
                        res.draw(pass, draw, true, flat, small);
                    }
                });
                resources.insert(backdrop);
            }
        }
        Vec::new()
    }

    fn paint(
        &self,
        info: eframe::egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let (Job::Draw(draw), Some(res)) = (&self.job, resources.get::<Resources>()) else {
            return;
        };
        let flat = self.frame.flat.is_some();
        res.draw(render_pass, draw, false, flat, info.screen_size_px);
    }
}

/// The count of the runs of the passes of the lines of the stops.
pub fn line_runs(render_state: &egui_wgpu::RenderState) -> u64 {
    let renderer = render_state.renderer.read();
    let res = renderer.callback_resources.get::<Resources>();
    res.map_or(0, |res| res.edges.runs())
}

/// The nearest places of the lines of the stops of each layer, from the
/// last frame that drew them: the size of the render target and a payload
/// for each pixel, row by row. It waits for the GPU.
pub fn nearest_places(
    render_state: &egui_wgpu::RenderState,
) -> Vec<(LayerKey, [u32; 2], Vec<u32>)> {
    let renderer = render_state.renderer.read();
    let Some(res) = renderer.callback_resources.get::<Resources>() else {
        return Vec::new();
    };
    let (device, queue) = (&render_state.device, &render_state.queue);
    res.edges.read_nearest(device, queue)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The strips cover the rectangle from the top, with no gap and no
    /// overlap. Returns the number of strips.
    fn check(rect: TexelRect, budget: usize) -> usize {
        let mut y = rect.y0;
        let mut count = 0;
        for strip in strips(rect, budget) {
            assert_eq!((strip.x0, strip.x1), (rect.x0, rect.x1));
            assert_eq!(strip.y0, y);
            assert!(strip.y1 > strip.y0 && strip.y1 <= rect.y1);
            let rows = strip.y1 - strip.y0;
            assert!(texels(strip) <= budget || rows == 1, "{strip:?}");
            y = strip.y1;
            count += 1;
        }
        assert_eq!(y, rect.y1);
        count
    }

    #[test]
    fn strips_cover_a_rectangle_within_the_budget() {
        let rect = |x0, y0, x1, y1| TexelRect { x0, y0, x1, y1 };
        assert_eq!(check(rect(0, 0, 8192, 8192), UPLOAD_TEXELS), 16);
        assert_eq!(check(rect(0, 0, 256, 256), UPLOAD_TEXELS), 1);
        // The last strip is shorter than the others.
        assert_eq!(check(rect(3, 5, 103, 75), 1000), 7);
        assert_eq!(check(rect(3, 5, 103, 75), 999), 8);
        // A row that is wider than the budget is one strip.
        assert_eq!(check(rect(10, 20, 110, 27), 64), 7);
        assert_eq!(check(rect(0, 0, 1, 1), 1), 1);
    }
}
