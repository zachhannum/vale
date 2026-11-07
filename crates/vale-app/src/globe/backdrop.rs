//! The blurred copy of the canvas behind the cards of the iPad layout.
//!
//! A card puts a marker in the paint jobs. At the marker, egui ends its
//! render pass, and the hook shrinks the frame drawn so far to the small
//! texture and blurs it. Then the cards draw the small texture.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};

use super::compile::{Compiled, render_pipeline};

/// The standard deviation of the blur, in points.
pub const BLUR_POINTS: f32 = 20.0;
/// The blurred copy has one pixel for this many pixels of the canvas, on
/// each axis.
pub const REDUCTION: u32 = 4;
/// A card shows the blurred canvas at this part of its brightness. With the
/// tint of the card, grey text stays easy to read over a white canvas.
pub const GAIN: f32 = 0.7;
/// The largest number of cards in one frame.
const MAX_CARDS: usize = 64;

/// The values that the shader reads. The layout matches `backdrop.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    canvas: [f32; 4],
    small: [f32; 4],
    blur: [f32; 4],
}

/// The place of one card. The layout matches `backdrop.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    rect: [f32; 4],
    radii: [f32; 4],
}

/// The textures of one screen size.
struct Targets {
    size: [u32; 2],
    small_size: [u32; 2],
    /// The reduced frame, and the target of the first blur pass.
    small: [wgpu::TextureView; 2],
    /// The two blur passes. The first reads `small[0]`.
    passes: [wgpu::BindGroup; 2],
    /// Reads the blurred copy.
    from_blur: wgpu::BindGroup,
}

/// The pipelines of the backdrop.
struct Pipelines {
    down: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    card: wgpu::RenderPipeline,
}

/// What the hook changes in a frame.
struct State {
    pixels_per_point: f32,
    targets: Option<Targets>,
    /// True when the small texture holds a blur of this frame.
    captured: bool,
}

/// The part of the backdrop that the hook and the cards share.
struct Shared {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// A worker compiles them when a card first needs them. Until then,
    /// the cards show no blur.
    pipelines: Compiled<Pipelines>,
    /// The values for the cards and the shrink, and for each of the two
    /// blur passes.
    globals: [wgpu::Buffer; 3],
    state: Mutex<State>,
}

pub struct Backdrop {
    shared: Arc<Shared>,
    /// The cards of this frame that wait for the upload.
    cards: Vec<Instance>,
    card_buffer: wgpu::Buffer,
}

/// Blurs the frame at a marker.
struct Hook(Arc<Shared>);

/// The marker that a card of this order puts before its shapes. The cards
/// of a higher order show the cards of a lower order in their blur.
pub fn marker(order: egui::Order, rect: egui::Rect) -> egui::Shape {
    let level = match order {
        egui::Order::Background => 0,
        egui::Order::Middle => 1,
        egui::Order::Foreground => 2,
        egui::Order::Tooltip => 3,
        egui::Order::Debug => 4,
    };
    egui_wgpu::BackdropLevel(level).shape(rect)
}

impl Pipelines {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> Pipelines {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("backdrop"),
            source: wgpu::ShaderSource::Wgsl(include_str!("backdrop.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("backdrop"),
            bind_group_layouts: &[Some(layout)],
            immediate_size: 0,
        });
        let instance = wgpu::VertexBufferLayout {
            array_stride: size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4],
        };
        let pipeline =
            |label, vertex, fragment, blend, buffers: &[Option<wgpu::VertexBufferLayout>]| {
                render_pipeline(
                    device,
                    &wgpu::RenderPipelineDescriptor {
                        label: Some(label),
                        layout: Some(&pipeline_layout),
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
                                blend,
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                            compilation_options: Default::default(),
                        }),
                        primitive: wgpu::PrimitiveState::default(),
                        depth_stencil: None,
                        multisample: wgpu::MultisampleState::default(),
                        multiview_mask: None,
                        cache: None,
                    },
                )
            };
        let over = Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        Pipelines {
            down: pipeline("backdrop down", "vs_full", "fs_down", None, &[]),
            blur: pipeline("backdrop blur", "vs_full", "fs_blur", None, &[]),
            card: pipeline(
                "backdrop card",
                "vs_card",
                "fs_card",
                over,
                &[Some(instance)],
            ),
        }
    }
}

impl Backdrop {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Backdrop {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("backdrop"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    1,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    2,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let globals = [0, 1, 2].map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("backdrop globals"),
                size: size_of::<Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let (worker_device, worker_layout) = (device.clone(), layout.clone());
        let pipelines =
            Compiled::lazy(move || Pipelines::new(&worker_device, &worker_layout, format));
        let shared = Shared {
            format,
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("backdrop"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            pipelines,
            globals,
            state: Mutex::new(State {
                pixels_per_point: 1.0,
                targets: None,
                captured: false,
            }),
        };
        Backdrop {
            shared: Arc::new(shared),
            cards: Vec::new(),
            card_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("backdrop cards"),
                size: (MAX_CARDS * size_of::<Instance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        }
    }

    /// The hook that blurs the frame for this backdrop.
    pub fn hook(&self) -> egui_wgpu::BackdropHooks {
        egui_wgpu::BackdropHooks(Box::new(Hook(self.shared.clone())))
    }
}

impl Shared {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn bind(
        &self,
        device: &wgpu::Device,
        globals: usize,
        view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("backdrop"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.globals[globals].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    fn targets(&self, device: &wgpu::Device, size: [u32; 2]) -> Targets {
        let small_size = size.map(|s| s.div_ceil(REDUCTION));
        let view = |[width, height]: [u32; 2]| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("backdrop blur"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            texture.create_view(&wgpu::TextureViewDescriptor::default())
        };
        let small = [view(small_size), view(small_size)];
        Targets {
            size,
            small_size,
            passes: [
                self.bind(device, 1, &small[0]),
                self.bind(device, 2, &small[1]),
            ],
            from_blur: self.bind(device, 0, &small[0]),
            small,
        }
    }

    /// Makes the textures for a frame of this size, and writes the values of
    /// the shader, unless the state has them.
    fn resize(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        state: &mut State,
        size: [u32; 2],
        pixels_per_point: f32,
    ) {
        let size = size.map(|s| s.max(1));
        let same = state.targets.as_ref().is_some_and(|t| t.size == size);
        if same && state.pixels_per_point == pixels_per_point {
            return;
        }
        state.pixels_per_point = pixels_per_point;
        if !same {
            state.targets = Some(self.targets(device, size));
        }
        let targets = state.targets.as_ref().expect("set above");
        let [width, height] = targets.small_size.map(|s| s as f32);
        let sigma = BLUR_POINTS * state.pixels_per_point / REDUCTION as f32;
        for (buffer, direction) in self
            .globals
            .iter()
            .zip([[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]])
        {
            let globals = Globals {
                canvas: [0.0, 0.0, size[0] as f32, size[1] as f32],
                small: [width, height, direction[0], direction[1]],
                blur: [sigma, REDUCTION as f32, GAIN, 0.0],
            };
            queue.write_buffer(buffer, 0, bytemuck::bytes_of(&globals));
        }
    }
}

impl egui_wgpu::BackdropHook for Hook {
    fn capture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::TextureView,
        size: [u32; 2],
    ) {
        let shared = &*self.0;
        let Some(pipelines) = shared.pipelines.get() else {
            return;
        };
        let mut state = shared.state();
        let pixels_per_point = state.pixels_per_point;
        shared.resize(device, queue, &mut state, size, pixels_per_point);
        let targets = state.targets.as_ref().expect("set above");
        let from_frame = shared.bind(device, 0, frame);
        let steps = [
            (&pipelines.down, &from_frame, &targets.small[0]),
            (&pipelines.blur, &targets.passes[0], &targets.small[1]),
            (&pipelines.blur, &targets.passes[1], &targets.small[0]),
        ];
        for (pipeline, bind_group, view) in steps {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("backdrop"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        state.captured = true;
    }
}

/// The blurred canvas behind one card. egui calls it inside its own render
/// pass.
pub struct CardCallback {
    format: wgpu::TextureFormat,
    place: Instance,
    /// The index of the card in the buffer of this frame.
    index: AtomicU32,
}

/// The shape that draws the blurred canvas in the rounded rectangle of a card.
pub fn card_shape(
    format: wgpu::TextureFormat,
    rect: egui::Rect,
    radius: egui::CornerRadius,
    pixels_per_point: f32,
) -> egui::PaintCallback {
    let center = rect.center() * pixels_per_point;
    let half = rect.size() * 0.5 * pixels_per_point;
    let radii = [radius.nw, radius.ne, radius.sw, radius.se];
    let callback = CardCallback {
        format,
        place: Instance {
            rect: [center.x, center.y, half.x, half.y],
            radii: radii.map(|r| f32::from(r) * pixels_per_point),
        },
        index: AtomicU32::new(u32::MAX),
    };
    egui_wgpu::Callback::new_paint_callback(rect, callback)
}

impl egui_wgpu::CallbackTrait for CardCallback {
    /// The first card of a frame sets up the backdrop and its hook, and
    /// makes the textures for the size of the screen.
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let stale = resources
            .get::<Backdrop>()
            .is_none_or(|b| b.shared.format != self.format);
        if stale {
            let backdrop = Backdrop::new(device, self.format);
            resources.insert(backdrop.hook());
            resources.insert(backdrop);
        }
        let backdrop: &mut Backdrop = resources.get_mut().expect("inserted above");
        if backdrop.cards.is_empty() {
            let shared = &backdrop.shared;
            let mut state = shared.state();
            let (size, pixels_per_point) = (
                screen_descriptor.size_in_pixels,
                screen_descriptor.pixels_per_point,
            );
            shared.resize(device, queue, &mut state, size, pixels_per_point);
            state.captured = false;
        }
        let index = backdrop.cards.len();
        if index < MAX_CARDS {
            backdrop.cards.push(self.place);
            self.index.store(index as u32, Ordering::Relaxed);
        }
        Vec::new()
    }

    /// The first card of a frame uploads the places of all cards.
    fn finish_prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(backdrop) = resources.get_mut::<Backdrop>()
            && !backdrop.cards.is_empty()
        {
            let bytes = bytemuck::cast_slice(&backdrop.cards);
            queue.write_buffer(&backdrop.card_buffer, 0, bytes);
            backdrop.cards.clear();
        }
        Vec::new()
    }

    /// egui calls it after the hook blurred the frame below the card.
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let index = self.index.load(Ordering::Relaxed);
        let Some(backdrop) = resources.get::<Backdrop>().filter(|_| index != u32::MAX) else {
            return;
        };
        let shared = &backdrop.shared;
        let Some(pipelines) = shared.pipelines.get() else {
            return;
        };
        let state = shared.state();
        let Some(targets) = state.targets.as_ref().filter(|_| state.captured) else {
            return;
        };
        pass.set_pipeline(&pipelines.card);
        pass.set_bind_group(0, &targets.from_blur, &[]);
        pass.set_vertex_buffer(0, backdrop.card_buffer.slice(..));
        pass.draw(0..3, index..index + 1);
    }
}
