//! The wgpu side: the cube map texture and the paint callback of the globe.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use eframe::egui_wgpu::{self, wgpu};

use vale_terrain::{FACES, GpuHeightmap, Heightmap, MAX_BANDS, Readback, StampPlan, TexelRect};

use super::backdrop::{Backdrop, Canvas};
use super::preview::BandUniform;

/// The time between two polls of the device while GPU work is in flight.
const POLL_STEP: Duration = Duration::from_micros(250);

/// The values that the shader reads. The layout matches `globe.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub rot: [[f32; 4]; 3],
    pub globe: [f32; 4],
    pub params: [f32; 4],
    pub flat: [f32; 4],
    pub bands: [BandUniform; MAX_BANDS],
}

struct Resources {
    face_size: u32,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    heights: GpuHeightmap,
    _poller: Poller,
}

impl Resources {
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        face_size: u32,
        busy: &Arc<Busy>,
    ) -> Resources {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("globe"),
            source: wgpu::ShaderSource::Wgsl(include_str!("globe.wgsl").into()),
        });
        let heights = GpuHeightmap::new(device, face_size);
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globe uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globe"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globe"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(heights.view()),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("globe"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("globe"),
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
        });
        Resources {
            face_size,
            format,
            pipeline,
            bind_group,
            uniform_buffer,
            heights,
            _poller: Poller::new(device.clone(), busy.clone()),
        }
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
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
    /// Reads texels back for the CPU heightmap.
    Readback {
        stroke: u64,
        face: u32,
        rect: TexelRect,
    },
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
    /// The changes that wait for the GPU, each with the face size of its
    /// heightmap. egui can run a frame and not paint it, so the queue keeps
    /// them until `prepare` runs.
    ops: Arc<Mutex<Vec<(u32, Op)>>>,
    events: mpsc::Sender<Event>,
    pub busy: Arc<Busy>,
}

impl Link {
    pub fn new() -> (Link, mpsc::Receiver<Event>) {
        let (events, receiver) = mpsc::channel();
        let link = Link {
            ops: Arc::default(),
            events,
            busy: Arc::default(),
        };
        (link, receiver)
    }

    pub fn push(&self, face_size: u32, op: Op) {
        let mut ops = self.ops.lock().expect("no panic holds the lock");
        ops.push((face_size, op));
    }
}

/// Moves the changes of the CPU heightmap to the queue.
pub fn queue_changes(map: &mut Heightmap, link: &Link) {
    let face_size = map.face_size() as u32;
    let mut rects: Vec<(usize, TexelRect)> = Vec::new();
    if map.take_reset() {
        link.push(face_size, Op::Reset(map.base()));
        rects.extend(map.allocated_rects());
    } else {
        let dirty = map.take_dirty().into_iter().enumerate();
        rects.extend(dirty.filter_map(|(face, rect)| Some((face, rect?))));
    }
    for (face, rect) in rects {
        let mut data = Vec::new();
        map.read_rect(face, rect, &mut data);
        let face = face as u32;
        link.push(face_size, Op::Upload { face, rect, data });
    }
}

/// One frame of the globe. egui calls it inside its own render pass.
pub struct GlobeCallback {
    pub format: wgpu::TextureFormat,
    pub face_size: u32,
    pub uniforms: Uniforms,
    pub link: Link,
    /// `Some`: the globe goes into the canvas texture of the backdrop, and
    /// the cards show a blurred copy of it.
    pub backdrop: Option<Canvas>,
}

/// The commands of one `prepare` call that wait for a submit.
struct Batch<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    heights: &'a GpuHeightmap,
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
        if !stamps.is_empty() {
            self.heights.begin_batch();
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

    /// Adds stamps that follow one another in the queue, in groups. `samples`
    /// has the stroke and the sample time of each stamp.
    fn stamp_run(&mut self, plans: &[StampPlan], samples: &[(u64, Instant)]) {
        let (heights, queue) = (self.heights, self.queue);
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
            let stroke = samples[at].0;
            match self.passes.iter_mut().find(|(id, _)| *id == stroke) {
                Some((_, count)) => *count += passes,
                None => self.passes.push((stroke, passes)),
            }
            self.stamps.extend(&samples[at..end]);
            at = end;
        }
    }

    fn run(&mut self, op: Op) {
        match op {
            Op::Reset(level) => {
                let heights = self.heights;
                heights.clear(self.encoder(), level);
            }
            Op::Upload { face, rect, data } => {
                // The write runs before the commands of the next submit, so
                // the commands before it go first.
                self.submit();
                self.heights.upload(self.queue, face, rect, &data);
            }
            Op::Stamp { plan, stroke, time } => self.stamp_run(&[*plan], &[(stroke, time)]),
            Op::Readback { stroke, face, rect } => {
                let (heights, device) = (self.heights, self.device);
                let readback = heights.read_rect(device, self.encoder(), face, rect);
                self.readbacks.push((stroke, face, rect, readback));
            }
        }
    }
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
        let stale = resources
            .get::<Resources>()
            .is_none_or(|r| r.face_size != self.face_size || r.format != self.format);
        if stale {
            let busy = &self.link.busy;
            resources.insert(Resources::new(device, self.format, self.face_size, busy));
        }
        let res: &Resources = resources.get().expect("inserted above");
        queue.write_buffer(&res.uniform_buffer, 0, bytemuck::bytes_of(&self.uniforms));
        let ops = std::mem::take(&mut *self.link.ops.lock().expect("no panic holds the lock"));
        let mut batch = Batch {
            device,
            queue,
            heights: &res.heights,
            link: &self.link,
            encoder: None,
            stamps: Vec::new(),
            readbacks: Vec::new(),
            passes: Vec::new(),
        };
        // The stamps that follow one another in the queue.
        let (mut plans, mut samples) = (Vec::new(), Vec::new());
        for (face_size, op) in ops {
            // A change of a heightmap that the app replaced.
            if face_size != self.face_size {
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
        // This submit is before the submit of egui, so the frame shows the
        // new levels.
        batch.submit();
        for (stroke, passes) in batch.passes {
            let _ = self.link.events.send(Event::Passes { stroke, passes });
        }
        if let Some(canvas) = &self.backdrop {
            let kept = resources.remove::<Backdrop>();
            let mut backdrop = kept
                .filter(|b| b.format == self.format)
                .unwrap_or_else(|| Backdrop::new(device, self.format));
            let res: &Resources = resources.get().expect("inserted above");
            let size = screen_descriptor.size_in_pixels;
            backdrop.render(device, queue, egui_encoder, size, canvas, |pass| {
                res.draw(pass);
            });
            resources.insert(backdrop);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: eframe::egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if self.backdrop.is_some() {
            if let Some(backdrop) = resources.get::<Backdrop>() {
                backdrop.blit(render_pass);
            }
            return;
        }
        if let Some(res) = resources.get::<Resources>() {
            res.draw(render_pass);
        }
    }
}
