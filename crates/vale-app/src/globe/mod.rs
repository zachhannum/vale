//! The globe workspace: the heightmap of the world on a globe that you rotate.

use eframe::egui::Rect;
use eframe::egui_wgpu::wgpu;
use vale_terrain::{Heightmap, meters_to_level};

pub mod gpu;
pub mod math;
pub mod nav;
pub mod view;

use gpu::{GlobeCallback, Uniforms, UploadQueue};
use math::V3;
use nav::Nav;
use view::GlobeView;

/// Texels on one edge of a cube face. The GPU texture holds six full faces.
pub const FACE_SIZE: usize = 1024;

/// The elevation of the empty world, in meters.
const START_ELEVATION: f64 = -2500.0;

pub struct Globe {
    pub map: Heightmap,
    pub view: GlobeView,
    pub nav: Nav,
    /// The canvas in screen points. Set by the canvas each frame.
    pub rect: Rect,
    /// The format of the render target. `None`: no wgpu renderer draws the UI.
    pub format: Option<wgpu::TextureFormat>,
    uploads: UploadQueue,
}

impl Default for Globe {
    fn default() -> Globe {
        Globe {
            map: Heightmap::new(FACE_SIZE, meters_to_level(START_ELEVATION)),
            view: GlobeView::centered(15.0, 25.0),
            nav: Nav::default(),
            rect: Rect::ZERO,
            format: None,
            uploads: UploadQueue::default(),
        }
    }
}

impl Globe {
    /// The paint callback of this frame, or `None` with no wgpu renderer.
    pub fn callback(&mut self, pixels_per_point: f32) -> Option<GlobeCallback> {
        let format = self.format?;
        gpu::queue_dirty(&mut self.map, &self.uploads);
        let row = |r: V3| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0];
        let center = self.rect.center();
        let radius = self.view.radius(self.rect) as f32;
        let graticule_degrees: f64 = if self.view.zoom < 3.0 {
            15.0
        } else if self.view.zoom < 12.0 {
            5.0
        } else {
            1.0
        };
        Some(GlobeCallback {
            format,
            face_size: self.map.face_size() as u32,
            uniforms: Uniforms {
                rot: self.view.rot.0.map(row),
                globe: [
                    center.x * pixels_per_point,
                    center.y * pixels_per_point,
                    radius * pixels_per_point,
                    pixels_per_point,
                ],
                params: [
                    self.map.face_size() as f32,
                    graticule_degrees.to_radians() as f32,
                    0.0,
                    0.0,
                ],
            },
            uploads: self.uploads.clone(),
        })
    }
}
