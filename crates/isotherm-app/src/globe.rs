//! The globe and the flat view of the open world: the heightmap on the GPU,
//! the colors of land and sea, and the navigation.

use std::sync::Arc;

use eframe::egui::{self, Color32, Rect};
use eframe::egui_wgpu;
use eframe::wgpu;
use isotherm_world::{COAST, Heightmap};
use vale_globe::flat::FlatView;
use vale_globe::gpu::{
    Draw, Frame, GlobeCallback, Job, LayerKey, Link, MapKey, MapKind, Op, Pass, Uniforms,
};
use vale_globe::nav::{Nav, Painting};
use vale_globe::preview;
use vale_globe::view::GlobeView;
use vale_store::{RasterDisplay, RasterId};
use vale_terrain::{Bands, FACES, TexelRect};

/// The opacity of the overlay. The terrain shows a little below it.
const OVERLAY_OPACITY: f32 = 0.9;

/// The place at the middle of the globe at the start.
pub const START: (f64, f64) = (10.0, 20.0);

/// The view of the world: the globe, or the whole world in one projection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum WorldView {
    #[default]
    Globe,
    Flat,
}

impl WorldView {
    pub fn name(self) -> &'static str {
        match self {
            WorldView::Globe => "Globe",
            WorldView::Flat => "Flat",
        }
    }
}

/// A grid of values on a cube map, in the colors of `bands`, over the
/// terrain.
#[derive(Clone, Debug, PartialEq)]
pub struct Overlay {
    pub face_size: usize,
    /// One level of `bands` for each cell, in the order of the cube map.
    pub levels: Vec<u16>,
    pub bands: Bands,
}

/// The layer of the overlay on the GPU.
const OVERLAY_LAYER: LayerKey = LayerKey::Raster(RasterId(1));

pub struct Globe {
    link: Link,
    key: MapKey,
    overlay: Option<(MapKey, Overlay)>,
    /// The count of the overlays that went to the GPU.
    overlay_uploads: u64,
    /// The overlay shows the lines of its stops.
    pub contours: bool,
    /// The format of the render target. `None`: the app has no wgpu
    /// renderer, and the globe draws nothing.
    format: Option<wgpu::TextureFormat>,
    nav: Nav,
    /// The frames that still need a repaint after the pipelines are ready.
    waiting: u32,
    pub view: GlobeView,
    pub flat: FlatView,
    pub world_view: WorldView,
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
        let mut nav = Nav::new(true);
        nav.finger_paints = false;
        let globe = Globe {
            link,
            key,
            overlay: None,
            overlay_uploads: 0,
            contours: true,
            format: None,
            nav,
            waiting: 2,
            view: GlobeView::centered(START.0, START.1),
            flat: FlatView::default(),
            world_view: WorldView::Globe,
            rect: Rect::NOTHING,
        };
        globe.upload(map);
        globe
    }

    /// Shows a new heightmap. The texels go to the GPU again.
    pub fn set_terrain(&mut self, map: &Heightmap) {
        self.key = MapKey {
            generation: self.key.generation + 1,
            face_size: map.face_size() as u32,
            ..self.key
        };
        self.upload(map);
        self.waiting = self.waiting.max(1);
    }

    /// The count of the heightmaps that went to the GPU, from 1.
    pub fn uploads(&self) -> u64 {
        self.key.generation + 1
    }

    /// Shows `overlay` over the terrain, or no overlay. The levels go to the
    /// GPU again.
    pub fn set_overlay(&mut self, overlay: Option<Overlay>) {
        self.overlay = overlay.map(|overlay| {
            self.overlay_uploads += 1;
            let n = overlay.face_size;
            let key = MapKey {
                layer: OVERLAY_LAYER,
                generation: self.overlay_uploads,
                kind: MapKind::Copy,
                face_size: n as u32,
            };
            self.link.push(key, Op::Reset(0));
            let rect = TexelRect {
                x0: 0,
                y0: 0,
                x1: n,
                y1: n,
            };
            for (face, data) in overlay.levels.chunks_exact(n * n).enumerate() {
                let face = face as u32;
                let data = data.to_vec();
                self.link.push(key, Op::Upload { face, rect, data });
            }
            (key, overlay)
        });
        self.waiting = self.waiting.max(1);
    }

    pub fn overlay(&self) -> Option<&Overlay> {
        self.overlay.as_ref().map(|(_, overlay)| overlay)
    }

    /// The count of the overlays that went to the GPU.
    pub fn overlay_uploads(&self) -> u64 {
        self.overlay_uploads
    }

    /// The direction of the world under a place on the screen, or `None`
    /// off the world.
    pub fn direction_at(&self, pos: egui::Pos2) -> Option<[f64; 3]> {
        match self.world_view {
            WorldView::Globe => self.view.unproject(self.rect, pos),
            WorldView::Flat => self.flat.unproject(self.rect, pos),
        }
    }

    fn upload(&self, map: &Heightmap) {
        let key = self.key;
        self.link.push(key, Op::Reset(map.base()));
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
            self.link.push(key, Op::Upload { face, rect, data });
        }
    }

    /// Starts the renderer of the globe.
    pub fn attach(&mut self, render_state: &egui_wgpu::RenderState) {
        vale_globe::gpu::start(render_state, &self.link);
        self.format = Some(render_state.target_format);
    }

    /// The format of the glass of the cards. `None`: a card has a plain
    /// fill.
    pub fn backdrop(&self) -> Option<wgpu::TextureFormat> {
        self.format
    }

    /// The scale of the view: the radius of the globe, or the points of
    /// one unit of the flat map.
    fn radius(&self) -> f64 {
        match self.world_view {
            WorldView::Globe => self.view.radius(self.rect),
            WorldView::Flat => self.flat.scale(self.rect),
        }
    }

    /// Draws the world in `rect` with the colors of `bands`. One finger or
    /// the mouse turns the globe or pans the map, and two fingers or the
    /// wheel zoom it.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        background: Color32,
        bands: &Bands,
    ) -> egui::Response {
        self.rect = rect;
        let resp = ui.interact(rect, ui.id().with("globe"), egui::Sense::click_and_drag());
        match self.world_view {
            WorldView::Globe => {
                self.nav
                    .update(ui, rect, &resp, &mut self.view, Painting::default())
            }
            WorldView::Flat => {
                self.nav
                    .update(ui, rect, &resp, &mut self.flat, Painting::default())
            }
        }
        let Some(format) = self.format else {
            return resp;
        };
        // The pipelines compile on a worker, and the texels go to the GPU
        // in the first frame after that.
        if self.waiting > 0 {
            if self.link.ready() {
                self.waiting -= 1;
            }
            ui.ctx().request_repaint();
        }
        let flat = self.world_view == WorldView::Flat;
        let frame = Arc::new(Frame {
            format,
            link: self.link.clone(),
            full: Some(self.key),
            copies: self.overlay.iter().map(|(key, _)| *key).collect(),
            details: Vec::new(),
            smooth_full: true,
            rivers: Vec::new(),
            images: Vec::new(),
            vector_tiles: Vec::new(),
            extent: None,
            flat: flat.then(|| self.flat.mesh()),
            epoch: 0,
        });
        let ppp = ui.ctx().pixels_per_point();
        let uniforms = self.uniforms(ppp, background, bands, self.key, RasterDisplay::Bands);
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
        let mut draws = vec![layer];
        if let Some((key, overlay)) = &self.overlay {
            let mut uniforms = self.uniforms(
                ppp,
                background,
                &overlay.bands,
                *key,
                RasterDisplay::Gradient,
            );
            if self.contours {
                uniforms.lines = preview::line_uniforms(&overlay.bands, ppp);
            }
            uniforms.layer[0] = OVERLAY_OPACITY;
            draws.push(Draw {
                pass: Pass::Layer(OVERLAY_LAYER),
                source: Some(*key),
                lines: self.contours,
                uniforms,
                ..layer
            });
        }
        // The flat view has no rim.
        if !flat {
            draws.push(rim);
        }
        for draw in draws {
            let callback = GlobeCallback {
                frame: frame.clone(),
                job: Job::Draw(Box::new(draw)),
            };
            ui.painter()
                .add(egui_wgpu::Callback::new_paint_callback(rect, callback));
        }
        resp
    }

    /// The uniforms of the pass of the map `key` in `bands` and `display`,
    /// with no hillshade, rivers, or graticule.
    fn uniforms(
        &self,
        ppp: f32,
        background: Color32,
        bands: &Bands,
        key: MapKey,
        display: RasterDisplay,
    ) -> Uniforms {
        let row = |r: [f64; 3]| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0];
        let color = |c: Color32| {
            let [r, g, b] = [c.r(), c.g(), c.b()].map(|c| f32::from(c) / 255.0);
            [r, g, b, 1.0]
        };
        let center = self.rect.center() * ppp;
        let radius = self.radius() as f32 * ppp;
        let (band_rows, count) = preview::band_uniforms(bands);
        let sea = f32::from(bands.level(COAST)) / 65535.0;
        let face_size = key.face_size;
        let flat_center = match self.world_view {
            WorldView::Globe => [0.0; 2],
            WorldView::Flat => [self.flat.center.x as f32, self.flat.center.y as f32],
        };
        Uniforms {
            rot: self.view.rot.0.map(row),
            globe: [center.x, center.y, radius, ppp],
            params: [face_size as f32, 0.0, preview::mode(display), count as f32],
            flat: [sea, 0.0, flat_center[0], flat_center[1]],
            layer: [1.0, 0.0, 0.0, 0.0],
            light: [0.0, 0.0, 1.0, 1.0],
            ground: color(vale_shell::pad::theme::GROUND),
            back: color(background),
            detail: [0.0, vale_terrain::max_level(face_size) as f32, 0.0, 0.0],
            turn: [1.0, 0.0, 0.0, 0.0],
            bands: band_rows,
            ..bytemuck::Zeroable::zeroed()
        }
    }
}
