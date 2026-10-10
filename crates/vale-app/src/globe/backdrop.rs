//! The blurred copy of the canvas behind the cards of the iPad layout.

use std::sync::atomic::{AtomicU32, Ordering};

use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};

/// The standard deviation of the blur, in points.
pub const BLUR_POINTS: f32 = 20.0;
/// The blurred copy has one pixel for this many pixels of the canvas, on
/// each axis. The shader takes four taps, which is correct for 4.
const REDUCTION: u32 = 4;
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

/// What the canvas gives for the blurred copy of one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Canvas {
    /// The color of the canvas where the globe is not.
    pub background: egui::Color32,
    pub pixels_per_point: f32,
}

/// The value that clears a texture to a color of egui. egui writes gamma
/// values to a target that is not sRGB.
fn clear_color(format: wgpu::TextureFormat, color: egui::Color32) -> wgpu::Color {
    let [r, g, b, a] = if format.is_srgb() {
        egui::Rgba::from(color).to_array()
    } else {
        color.to_array().map(|c| f32::from(c) / 255.0)
    };
    wgpu::Color {
        r: f64::from(r),
        g: f64::from(g),
        b: f64::from(b),
        a: f64::from(a),
    }
}

/// The textures of one screen size.
struct Targets {
    size: [u32; 2],
    small_size: [u32; 2],
    canvas: wgpu::TextureView,
    small: [wgpu::TextureView; 2],
    /// Reads the canvas texture.
    from_canvas: wgpu::BindGroup,
    /// The two blur passes. The first reads `small[0]`.
    passes: [wgpu::BindGroup; 2],
    /// Reads the blurred copy.
    from_blur: wgpu::BindGroup,
}

pub struct Backdrop {
    pub format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    blit: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    card: wgpu::RenderPipeline,
    /// The values for the canvas, and for each of the two blur passes.
    globals: [wgpu::Buffer; 3],
    targets: Option<Targets>,
    /// The cards of this frame that wait for the upload.
    cards: Vec<Instance>,
    card_buffer: wgpu::Buffer,
}

impl Backdrop {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Backdrop {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("backdrop"),
            source: wgpu::ShaderSource::Wgsl(include_str!("backdrop.wgsl").into()),
        });
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("backdrop"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let instance = wgpu::VertexBufferLayout {
            array_stride: size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4],
        };
        let pipeline =
            |label, vertex, fragment, blend, buffers: &[Option<wgpu::VertexBufferLayout>]| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                })
            };
        let over = Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let globals = [0, 1, 2].map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("backdrop globals"),
                size: size_of::<Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        Backdrop {
            format,
            blit: pipeline("backdrop blit", "vs_full", "fs_blit", None, &[]),
            down: pipeline("backdrop down", "vs_full", "fs_down", None, &[]),
            blur: pipeline("backdrop blur", "vs_full", "fs_blur", None, &[]),
            card: pipeline(
                "backdrop card",
                "vs_card",
                "fs_card",
                over,
                &[Some(instance)],
            ),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("backdrop"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            layout,
            globals,
            targets: None,
            cards: Vec::new(),
            card_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("backdrop cards"),
                size: (MAX_CARDS * size_of::<Instance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        }
    }

    fn targets(&self, device: &wgpu::Device, size: [u32; 2]) -> Targets {
        let small_size = size.map(|s| s.div_ceil(REDUCTION));
        let view = |label, [width, height]: [u32; 2]| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
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
        let bind = |globals: usize, view: &wgpu::TextureView| {
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
        };
        let canvas = view("backdrop canvas", size);
        let small = [
            view("backdrop blur", small_size),
            view("backdrop blur", small_size),
        ];
        Targets {
            size,
            small_size,
            from_canvas: bind(0, &canvas),
            passes: [bind(1, &small[0]), bind(2, &small[1])],
            from_blur: bind(0, &small[0]),
            canvas,
            small,
        }
    }

    /// Draws the canvas into its texture and makes the blurred copy. `draw`
    /// draws the canvas. The passes run before the pass of egui.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        canvas: &Canvas,
        draw: impl FnOnce(&mut wgpu::RenderPass<'_>),
    ) {
        let size = size.map(|s| s.max(1));
        if self.targets.as_ref().is_none_or(|t| t.size != size) {
            self.targets = Some(self.targets(device, size));
        }
        let targets = self.targets.as_ref().expect("set above");
        let [width, height] = targets.small_size.map(|s| s as f32);
        let sigma = BLUR_POINTS * canvas.pixels_per_point / REDUCTION as f32;
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

        let mut pass = |view: &wgpu::TextureView, load| {
            encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("backdrop"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime()
        };
        let clear = wgpu::LoadOp::Clear(clear_color(self.format, canvas.background));
        draw(&mut pass(&targets.canvas, clear));
        let black = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        let steps = [
            (&self.down, &targets.from_canvas, &targets.small[0]),
            (&self.blur, &targets.passes[0], &targets.small[1]),
            (&self.blur, &targets.passes[1], &targets.small[0]),
        ];
        for (pipeline, bind_group, view) in steps {
            let mut pass = pass(view, black);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Copies the canvas texture to the screen.
    pub fn blit(&self, pass: &mut wgpu::RenderPass<'static>) {
        let Some(targets) = &self.targets else {
            return;
        };
        pass.set_pipeline(&self.blit);
        pass.set_bind_group(0, &targets.from_canvas, &[]);
        pass.draw(0..3, 0..1);
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
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let stale = resources
            .get::<Backdrop>()
            .is_none_or(|b| b.format != self.format);
        if stale {
            resources.insert(Backdrop::new(device, self.format));
        }
        let backdrop: &mut Backdrop = resources.get_mut().expect("inserted above");
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

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let index = self.index.load(Ordering::Relaxed);
        let targets = resources
            .get::<Backdrop>()
            .and_then(|b| Some((b, b.targets.as_ref()?)));
        let Some((backdrop, targets)) = targets.filter(|_| index != u32::MAX) else {
            return;
        };
        pass.set_pipeline(&backdrop.card);
        pass.set_bind_group(0, &targets.from_blur, &[]);
        pass.set_vertex_buffer(0, backdrop.card_buffer.slice(..));
        pass.draw(0..3, index..index + 1);
    }
}
