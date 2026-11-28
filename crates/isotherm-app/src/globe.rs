//! The globe with the sample world: the heightmap on the GPU, the colors of
//! land and sea, and the navigation.

use std::sync::Arc;

use eframe::egui::{self, Color32, Rect};
use eframe::egui_wgpu;
use eframe::wgpu;
use vale_globe::gpu::{
    Draw, Frame, GlobeCallback, Job, LayerKey, Link, MapKey, MapKind, Op, Pass, Uniforms,
};
use vale_globe::nav::{Nav, Painting};
use vale_globe::preview;
use vale_globe::view::GlobeView;
use vale_store::RasterDisplay;
use vale_terrain::{Bands, FACES, Heightmap, SEA_LEVEL, TexelRect};

/// Texels on one edge of a cube face of the sample world.
pub const FACE_SIZE: usize = 256;

/// The place at the middle of the globe at the start.
pub const START: (f64, f64) = (10.0, 20.0);

/// The sample heightmap of the Earth: 1,024 by 512 levels, a level minus
/// 32,768 is the height in meters.
const SAMPLE: &[u8] = include_bytes!("../data/earth.png");

/// Reads the sample world into a heightmap.
pub fn sample() -> Result<Heightmap, String> {
    isotherm_import::heightmap_png(SAMPLE, FACE_SIZE).map_err(|e| e.to_string())
}

pub struct Globe {
    link: Link,
    key: MapKey,
    bands: Bands,
    /// The format of the render target. `None`: the app has no wgpu
    /// renderer, and the globe draws nothing.
    format: Option<wgpu::TextureFormat>,
    nav: Nav,
    /// The frames that still need a repaint after the pipelines are ready.
    waiting: u32,
    pub view: GlobeView,
    /// The canvas of the last frame, in points.
    pub rect: Rect,
}

impl Globe {
    /// A globe that shows `map`. The texels go to the GPU at the first
    /// paint.
    pub fn new(map: &Heightmap) -> Globe {
        let (link, _events) = Link::new();
        let key = MapKey {
            layer: LayerKey::Solo,
            generation: 0,
            kind: MapKind::Full,
            face_size: map.face_size() as u32,
        };
        link.push(key, Op::Reset(map.base()));
        let n = map.face_size();
        let rect = TexelRect {
            x0: 0,
            y0: 0,
            x1: n,
            y1: n,
        };
        for face in 0..FACES {
            let mut data = Vec::with_capacity(n * n);
            map.read_rect(face, rect, &mut data);
            let face = face as u32;
            link.push(key, Op::Upload { face, rect, data });
        }
        let mut nav = Nav::new(true);
        nav.finger_paints = false;
        Globe {
            link,
            key,
            bands: Bands::default(),
            format: None,
            nav,
            waiting: 2,
            view: GlobeView::centered(START.0, START.1),
            rect: Rect::NOTHING,
        }
    }

    /// Starts the renderer of the globe.
    pub fn attach(&mut self, render_state: &egui_wgpu::RenderState) {
        vale_globe::gpu::start(render_state, &self.link);
        self.format = Some(render_state.target_format);
    }

    /// Draws the globe in `rect` and turns it with one finger or the mouse.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, background: Color32) {
        self.rect = rect;
        let resp = ui.interact(rect, ui.id().with("globe"), egui::Sense::click_and_drag());
        self.nav
            .update(ui, rect, &resp, &mut self.view, Painting::default());
        let Some(format) = self.format else {
            return;
        };
        // The pipelines compile on a worker, and the texels go to the GPU
        // in the first frame after that.
        if self.waiting > 0 {
            if self.link.ready() {
                self.waiting -= 1;
            }
            ui.ctx().request_repaint();
        }
        let frame = Arc::new(Frame {
            format,
            link: self.link.clone(),
            full: Some(self.key),
            copies: Vec::new(),
            details: Vec::new(),
            smooth_full: true,
            rivers: Vec::new(),
            images: Vec::new(),
            vector_tiles: Vec::new(),
            extent: None,
            flat: None,
            epoch: 0,
        });
        let ppp = ui.ctx().pixels_per_point();
        let uniforms = self.uniforms(ppp, background);
        let clip = {
            let r = rect * ppp;
            [r.min.x, r.min.y, r.max.x, r.max.y]
        };
        let layer = Draw {
            pass: Pass::Layer(LayerKey::Solo),
            source: Some(self.key),
            image: None,
            rivers: false,
            lines: false,
            clip,
            uniforms,
        };
        let rim = Draw {
            pass: Pass::Rim,
            source: None,
            ..layer
        };
        for draw in [layer, rim] {
            let callback = GlobeCallback {
                frame: frame.clone(),
                job: Job::Draw(Box::new(draw)),
            };
            ui.painter()
                .add(egui_wgpu::Callback::new_paint_callback(rect, callback));
        }
    }

    /// The uniforms of the passes: the bands of the default colors, with no
    /// hillshade, rivers, or graticule.
    fn uniforms(&self, ppp: f32, background: Color32) -> Uniforms {
        let row = |r: [f64; 3]| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0];
        let color = |c: Color32| {
            let [r, g, b] = [c.r(), c.g(), c.b()].map(|c| f32::from(c) / 255.0);
            [r, g, b, 1.0]
        };
        let center = self.rect.center() * ppp;
        let radius = self.view.radius(self.rect) as f32 * ppp;
        let (bands, count) = preview::band_uniforms(&self.bands);
        let sea = f32::from(self.bands.level(SEA_LEVEL)) / 65535.0;
        let face_size = self.key.face_size;
        Uniforms {
            rot: self.view.rot.0.map(row),
            globe: [center.x, center.y, radius, ppp],
            params: [
                face_size as f32,
                0.0,
                preview::mode(RasterDisplay::Bands),
                count as f32,
            ],
            flat: [sea, 0.0, 0.0, 0.0],
            layer: [1.0, 0.0, 0.0, 0.0],
            light: [0.0, 0.0, 1.0, 1.0],
            ground: color(vale_shell::pad::theme::GROUND),
            back: color(background),
            detail: [0.0, vale_terrain::max_level(face_size) as f32, 0.0, 0.0],
            turn: [1.0, 0.0, 0.0, 0.0],
            bands,
            ..bytemuck::Zeroable::zeroed()
        }
    }
}
