//! Backdrop markers: points in the paint jobs where the frame is split into render passes,
//! so that a hook can read the image drawn so far.

use crate::{Renderer, ScreenDescriptor};

/// A marker in the paint jobs. Use it as the callback of an [`epaint::PaintCallback`].
///
/// [`Renderer::render_with_backdrops`] ends the render pass at a marker, calls the
/// [`BackdropHook`], and starts a new pass. It ignores a marker whose level is at or below the
/// level of the last capture in the frame. A marker draws nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackdropLevel(pub u32);

impl BackdropLevel {
    /// A paint callback that holds this marker.
    pub fn paint_callback(self, rect: epaint::Rect) -> epaint::PaintCallback {
        epaint::PaintCallback {
            rect,
            callback: std::sync::Arc::new(self),
        }
    }

    /// A shape that holds this marker.
    pub fn shape(self, rect: epaint::Rect) -> epaint::Shape {
        epaint::Shape::Callback(self.paint_callback(rect))
    }
}

/// Reads the image that the frame drew up to a [`BackdropLevel`] marker.
pub trait BackdropHook: Send + Sync {
    /// Records commands that read `frame`. `frame` has the usage `TEXTURE_BINDING`, and
    /// `size` is its size in pixels. The frame continues in a new render pass after this call.
    fn capture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::TextureView,
        size: [u32; 2],
    );
}

/// The hook of the markers, in [`Renderer::callback_resources`]. Without it, markers are
/// ignored.
pub struct BackdropHooks(pub Box<dyn BackdropHook>);

/// The target of [`Renderer::render_with_backdrops`].
pub struct BackdropTarget<'a> {
    /// The view that the passes draw into. With MSAA, this is the multisampled view.
    pub view: &'a wgpu::TextureView,

    /// With MSAA, the single-sample view that each pass resolves into.
    ///
    /// The hook reads this view, or `view` if there is none. That view needs the usage
    /// `TEXTURE_BINDING` if a hook is registered.
    pub resolve_target: Option<&'a wgpu::TextureView>,

    /// The load operation of the color in the first pass. Later passes load.
    pub load: wgpu::LoadOp<wgpu::Color>,

    /// The depth-stencil view and its format. The first pass clears it.
    pub depth_stencil: Option<(&'a wgpu::TextureView, wgpu::TextureFormat)>,
}

impl Renderer {
    /// Draws `paint_jobs` in order in one or more render passes on `target`, and splits the
    /// passes at [`BackdropLevel`] markers.
    ///
    /// At a marker with a level above the level of the last capture in this frame, the pass
    /// ends, the [`BackdropHook`] in [`BackdropHooks`] is called, and a new pass starts with
    /// [`wgpu::LoadOp::Load`].
    ///
    /// # Panic
    /// Always ensure that [`Renderer::update_buffers`] has been called before.
    pub fn render_with_backdrops(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &BackdropTarget<'_>,
        paint_jobs: &[epaint::ClippedPrimitive],
        screen_descriptor: &ScreenDescriptor,
    ) {
        profiling::function_scope!();

        let hook = self.callback_resources.get::<BackdropHooks>();
        let splits = if hook.is_some() {
            backdrop_splits(paint_jobs)
        } else {
            Vec::new()
        };

        let frame = target.resolve_target.unwrap_or(target.view);
        let mut cursor = self.buffer_cursor();
        let mut start = 0;
        for pass in 0..=splits.len() {
            let end = splits.get(pass).copied().unwrap_or(paint_jobs.len());
            let first = pass == 0;
            let last = pass == splits.len();
            let render_pass = begin_pass(encoder, target, first, last);
            self.render_jobs(
                &mut render_pass.forget_lifetime(),
                &paint_jobs[start..end],
                screen_descriptor,
                &mut cursor,
            );
            if let (false, Some(hook)) = (last, hook) {
                hook.0.capture(
                    device,
                    queue,
                    encoder,
                    frame,
                    screen_descriptor.size_in_pixels,
                );
            }
            // Skip the marker.
            start = end + 1;
        }
    }
}

/// Whether the paint jobs hold a [`BackdropLevel`] marker.
pub(crate) fn has_marker(paint_jobs: &[epaint::ClippedPrimitive]) -> bool {
    paint_jobs.iter().any(|job| {
        matches!(&job.primitive, epaint::Primitive::Callback(callback)
            if callback.callback.is::<BackdropLevel>())
    })
}

/// The indices of the markers that split the passes.
fn backdrop_splits(paint_jobs: &[epaint::ClippedPrimitive]) -> Vec<usize> {
    let mut splits = Vec::new();
    let mut captured: Option<u32> = None;
    for (i, job) in paint_jobs.iter().enumerate() {
        let epaint::Primitive::Callback(callback) = &job.primitive else {
            continue;
        };
        let Some(BackdropLevel(level)) = callback.callback.downcast_ref::<BackdropLevel>() else {
            continue;
        };
        if captured.is_none_or(|c| *level > c) {
            captured = Some(*level);
            splits.push(i);
        }
    }
    splits
}

fn begin_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    target: &BackdropTarget<'_>,
    first: bool,
    last: bool,
) -> wgpu::RenderPass<'e> {
    let color_load = if first {
        target.load
    } else {
        wgpu::LoadOp::Load
    };
    // The depth buffer is not needed after the last pass. (Discarding it can improve
    // performance on tiling GPUs.)
    let depth_store = if last {
        wgpu::StoreOp::Discard
    } else {
        wgpu::StoreOp::Store
    };
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("egui_render"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target.view,
            resolve_target: target.resolve_target,
            ops: wgpu::Operations {
                load: color_load,
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: target.depth_stencil.map(|(view, format)| {
            wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: format.has_depth_aspect().then_some(wgpu::Operations {
                    load: if first {
                        wgpu::LoadOp::Clear(1.0)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: depth_store,
                }),
                stencil_ops: format.has_stencil_aspect().then_some(wgpu::Operations {
                    load: if first {
                        wgpu::LoadOp::Clear(0)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: depth_store,
                }),
            }
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}
