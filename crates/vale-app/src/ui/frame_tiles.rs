//! The tiles of the map frames of the atlas. Each frame has its display lists in page points, and
//! the canvas draws them as square textures at the scale of the screen. A pan of the page draws
//! the tiles that it has and renders only the tiles that come into view. A zoom draws the tiles
//! of the nearest scale until the tiles of the new scale are ready.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Pos2, Rect, pos2, vec2};
use kurbo::{Affine, Point};
use vale_render::{DisplayList, PreparedList};
use vale_store::FrameId;

use super::map_texture::{self, MapRenderer, MapTexture};
use crate::atlas::PT_PER_MM;
use crate::view::{View, ViewTransform};

/// The side of a tile, in pixels.
pub const TILE: u32 = 512;
/// The tile renders in one pass of the UI at rest.
const REST_RENDERS: usize = 8;
/// The tile renders in one pass of the UI while a gesture moves the page.
const GESTURE_RENDERS: usize = 2;
/// The time of the tile renders in one pass of the UI at rest. The first render of a pass
/// always runs.
const REST_TIME: Duration = Duration::from_millis(6);
/// The time of the tile renders in one pass of the UI while a gesture moves the page.
const GESTURE_TIME: Duration = Duration::from_millis(3);
/// The free textures that wait for a tile.
const POOL: usize = 16;
/// The longest side of the reduced copy of a list for the blurred backdrop, in pixels.
const THUMB_SIDE: f64 = 256.0;
/// The most tiles of a lower or a higher scale that draw one square.
const MOST_FALLBACK: usize = 64;
/// The longest side of the overview of a list, in pixels.
const OVERVIEW_SIDE: f64 = 1024.0;

/// A display list of a frame.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    /// The water below the raster layers.
    Under,
    /// The map over the raster layers.
    Map,
    /// The edge labels, the neatline, and the border. They do not move with the map.
    Decor,
}

const LAYERS: [Layer; 3] = [Layer::Under, Layer::Map, Layer::Decor];

/// A tile: a square of `TILE` pixels of a layer at a scale in pixels per page point. The tile
/// `i`, `j` starts at the pixel `i * TILE`, `j * TILE` of the page at that scale.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct TileKey {
    layer: Layer,
    /// The bits of the scale as an `f32`.
    scale: u32,
    i: i32,
    j: i32,
}

impl TileKey {
    fn scale(&self) -> f32 {
        f32::from_bits(self.scale)
    }

    /// The square of the tile in page points.
    fn rect(&self) -> kurbo::Rect {
        let side = f64::from(TILE) / f64::from(self.scale());
        let (x, y) = (f64::from(self.i) * side, f64::from(self.j) * side);
        kurbo::Rect::new(x, y, x + side, y + side)
    }

    /// The page point `p` lands on the pixel `transform * p` of the tile.
    fn transform(&self) -> Affine {
        let t = f64::from(TILE);
        Affine::translate((-f64::from(self.i) * t, -f64::from(self.j) * t))
            * Affine::scale(f64::from(self.scale()))
    }
}

/// The columns and the rows of the tiles at `scale` that touch `rect`.
fn tiles_over(scale: f32, rect: kurbo::Rect) -> (Range<i32>, Range<i32>) {
    let k = f64::from(scale) / f64::from(TILE);
    let at = |v: f64| (v * k).clamp(-1e7, 1e7);
    let span = |a: f64, b: f64| {
        if b <= a {
            return 0..0;
        }
        at(a).floor() as i32..at(b).ceil() as i32
    };
    (span(rect.x0, rect.x1), span(rect.y0, rect.y1))
}

fn keys_over(layer: Layer, scale: u32, rect: kurbo::Rect) -> impl Iterator<Item = TileKey> {
    let (is, js) = tiles_over(f32::from_bits(scale), rect);
    js.flat_map(move |j| is.clone().map(move |i| TileKey { layer, scale, i, j }))
}

fn count_over(scale: u32, rect: kurbo::Rect) -> usize {
    let (is, js) = tiles_over(f32::from_bits(scale), rect);
    is.len() * js.len()
}

/// The squares of the tiles at `scale` that touch `region`, from the middle of the region out.
fn slots(scale: f32, region: kurbo::Rect) -> Vec<kurbo::Rect> {
    let bits = scale.to_bits();
    let mut slots: Vec<_> = keys_over(Layer::Map, bits, region)
        .map(|k| k.rect())
        .collect();
    let c = region.center();
    slots.sort_by(|a, b| {
        let (da, db) = (a.center().distance(c), b.center().distance(c));
        da.total_cmp(&db)
    });
    slots
}

/// The power of 2 at or above `scale`. While a gesture zooms the page, new tiles render at these
/// scales, so that the next passes can draw them.
fn quantized(scale: f32) -> f32 {
    2f32.powi(scale.log2().ceil() as i32)
}

/// A tile with no texture has nothing in it.
struct Tile {
    texture: Option<MapTexture>,
    /// The pass of the UI that last drew or rendered the tile.
    used: u64,
}

/// A texture of a frame at the reduced size of the blurred backdrop, in premultiplied bytes.
pub struct Thumb {
    pub serial: u64,
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

impl Thumb {
    fn new(img: &vale_render::Rgba) -> Thumb {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Thumb {
            serial,
            size: [img.width, img.height],
            rgba: img.data.clone(),
        }
    }
}

/// The map of a frame in a margin around its shape, at a lower resolution. It fills the shape
/// while a gesture moves the map.
pub struct Spill {
    pub texture: MapTexture,
    /// The texture in page points.
    pub rect: kurbo::Rect,
}

/// The display lists of a frame in page points.
pub struct FrameLists {
    /// The key of what the frame shows.
    pub content: u64,
    /// The key of the shape, the center, the scale, and the rotation of the frame.
    pub geometry: u64,
    under: Option<PreparedList>,
    map: PreparedList,
    decor: Option<PreparedList>,
    /// The page points that the lists draw in.
    pub bounds: kurbo::Rect,
    /// The view of the map in page points.
    vt: ViewTransform,
    thumbs: OnceLock<[Option<Arc<Thumb>>; 3]>,
}

impl FrameLists {
    /// Prepares the lists. `vt` is the view of the map in page points.
    pub fn new(
        (content, geometry): (u64, u64),
        [under, map, decor]: [Option<&DisplayList>; 3],
        bounds: kurbo::Rect,
        vt: ViewTransform,
    ) -> Result<FrameLists, String> {
        let prepare = |list: Option<&DisplayList>| {
            list.map(PreparedList::new)
                .transpose()
                .map_err(|e| e.to_string())
        };
        let map = prepare(map)?.ok_or("the frame has no map")?;
        Ok(FrameLists {
            content,
            geometry,
            under: prepare(under)?,
            map,
            decor: prepare(decor)?,
            bounds,
            vt,
            thumbs: OnceLock::new(),
        })
    }

    fn list(&self, layer: Layer) -> Option<&PreparedList> {
        match layer {
            Layer::Under => self.under.as_ref(),
            Layer::Map => Some(&self.map),
            Layer::Decor => self.decor.as_ref(),
        }
    }

    /// The reduced copies of the lists, for the blurred backdrop.
    fn thumbs(&self) -> &[Option<Arc<Thumb>>; 3] {
        self.thumbs.get_or_init(|| {
            let b = self.bounds;
            let t = THUMB_SIDE / b.width().max(b.height()).max(1e-6);
            let (w, h) = ((b.width() * t).ceil(), (b.height() * t).ceil());
            let at = Affine::scale(t) * Affine::translate((-b.x0, -b.y0));
            LAYERS.map(|layer| {
                let list = self.list(layer)?;
                let (img, _) =
                    vale_render::render_region(list, at, w.max(1.0) as u32, h.max(1.0) as u32)
                        .ok()?;
                Some(Arc::new(Thumb::new(&img)))
            })
        })
    }
}

/// A view that follows the page and the shape of a frame, but not the center, the scale, or the
/// rotation of its map.
pub fn fixed_view(vt: ViewTransform) -> ViewTransform {
    ViewTransform {
        view: View {
            center: Point::ZERO,
            scale: PT_PER_MM,
            rotation: 0.0,
        },
        ..vt
    }
}

/// The page on the screen in this pass of the UI.
#[derive(Copy, Clone, Debug)]
pub struct Screen {
    /// The top-left corner of the page.
    pub origin: Pos2,
    /// Points of the screen in one page point.
    pub zoom: f32,
    /// Pixels in one page point.
    pub scale: f32,
    /// A gesture moves the page or the maps.
    pub moving: bool,
}

impl Screen {
    /// A page point on the screen.
    fn point(self, p: Point) -> Pos2 {
        self.origin + vec2(p.x as f32, p.y as f32) * self.zoom
    }
}

/// A frame on the screen in this pass of the UI.
pub struct Shown {
    pub id: FrameId,
    /// The view of the map in page points.
    pub vt: ViewTransform,
    /// The part of the frame on the canvas.
    pub shown: Rect,
    /// The triangles of the shape on the screen. A map that moves does not show out of them.
    pub inside: Vec<[Pos2; 3]>,
}

/// Where the tiles of a layer land on the screen.
#[derive(Copy, Clone)]
struct Placement {
    screen: Screen,
    /// The view of the tiles and the view of the frame now, when they differ.
    moved: Option<(ViewTransform, ViewTransform)>,
}

impl Placement {
    /// A page point of the tiles on the screen.
    fn screen_of(self, p: Point) -> Pos2 {
        let p = match self.moved {
            Some((old, new)) => new.to_page(old.from_page(p)),
            None => p,
        };
        self.screen.point(p)
    }

    /// The page point of the tiles at a screen position.
    fn page_of(self, q: Pos2) -> Point {
        let v = (q - self.screen.origin) / self.screen.zoom;
        let p = Point::new(f64::from(v.x), f64::from(v.y));
        match self.moved {
            Some((old, new)) => old.to_page(new.from_page(p)),
            None => p,
        }
    }

    /// The box in page points of the tiles of a rectangle on the screen.
    fn region(self, r: Rect) -> kurbo::Rect {
        let corners = [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
        ];
        let mut points = corners.map(|c| self.page_of(c)).into_iter();
        let first = points.next().unwrap_or_default();
        points.fold(kurbo::Rect::from_points(first, first), |b, p| b.union_pt(p))
    }

    /// The box on the screen of a box in page points of the tiles.
    fn screen_box(self, r: kurbo::Rect) -> Rect {
        let corners = [(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)];
        let points = corners.map(|(x, y)| self.screen_of(Point::new(x, y)));
        Rect::from_points(&points)
    }
}

/// The display lists of a frame and their tiles.
struct FrameTiles {
    lists: FrameLists,
    tiles: HashMap<TileKey, Tile>,
    spill: Option<Spill>,
    /// The spill was composed, or failed.
    spilled: bool,
    /// The tiles filled each square of the frame on the canvas in the last pass.
    complete: bool,
    /// The tiles of the last lists. They show in the squares that the new tiles do not fill
    /// yet.
    previous: Option<Box<FrameTiles>>,
    /// One texture of each whole list at a low scale. It shows in the squares that no tile
    /// fills, and no eviction removes it.
    overviews: HashMap<Layer, Overview>,
}

/// A texture of a whole list, and the page points that it covers.
struct Overview {
    texture: MapTexture,
    rect: kurbo::Rect,
}

impl FrameTiles {
    fn placement(&self, layer: Layer, vt: &ViewTransform, screen: Screen) -> Placement {
        let (old, new) = match layer {
            Layer::Decor => (fixed_view(self.lists.vt), fixed_view(*vt)),
            _ => (self.lists.vt, *vt),
        };
        Placement {
            screen,
            moved: (old != new).then_some((old, new)),
        }
    }

    /// The scales of the tiles of a layer.
    fn scales(&self, layer: Layer) -> Vec<u32> {
        let mut scales: Vec<u32> = (self.tiles.keys())
            .filter(|k| k.layer == layer)
            .map(|k| k.scale)
            .collect();
        scales.sort_unstable();
        scales.dedup();
        scales
    }

    /// Whether each tile at `scale` over `rect` is ready.
    fn ready(&self, layer: Layer, scale: u32, rect: kurbo::Rect) -> bool {
        keys_over(layer, scale, rect).all(|k| self.tiles.contains_key(&k))
    }

    /// The scale that draws the page points `rect`: `scale` when its tiles are ready, or else
    /// the scale with ready tiles that is nearest: the smallest above, or the largest below.
    fn choose(&self, layer: Layer, scale: u32, rect: kurbo::Rect, scales: &[u32]) -> Option<u32> {
        if self.ready(layer, scale, rect) {
            return Some(scale);
        }
        let v = f32::from_bits(scale);
        let mut above: Vec<u32> = (scales.iter().copied())
            .filter(|c| f32::from_bits(*c) > v)
            .collect();
        above.sort_by(|a, b| f32::from_bits(*a).total_cmp(&f32::from_bits(*b)));
        let mut below: Vec<u32> = (scales.iter().copied())
            .filter(|c| f32::from_bits(*c) < v)
            .collect();
        below.sort_by(|a, b| f32::from_bits(*b).total_cmp(&f32::from_bits(*a)));
        above
            .into_iter()
            .chain(below)
            .filter(|c| count_over(*c, rect) <= MOST_FALLBACK)
            .find(|c| self.ready(layer, *c, rect))
    }

    fn textures(&self) -> usize {
        let own = self.tiles.values().filter(|t| t.texture.is_some()).count();
        own + self.previous.as_ref().map_or(0, |p| p.textures())
    }
}

/// What the canvas painted of a frame in the last pass of the UI.
#[derive(Clone, Debug, Default)]
pub struct FramePaint {
    /// The screen rectangles of the map, the spill, and the decorations.
    pub map: Option<Rect>,
    pub spill: Option<Rect>,
    pub decor: Option<Rect>,
    /// The squares of the painted map tiles where they are at rest.
    pub texture: Option<Rect>,
    /// The scales of the painted map tiles, in pixels per page point.
    pub scales: Vec<f32>,
    /// The number of tiles of the last lists that show.
    pub previous: usize,
    /// The number of squares of the map on the canvas with no tile.
    pub blank: usize,
    /// The number of squares of all layers that the tiles of the lists do not fill.
    empty: usize,
}

fn union(to: &mut Option<Rect>, r: Rect) {
    if r.is_positive() {
        *to = Some(to.map_or(r, |t| t.union(r)));
    }
}

/// The tiles of the frames of the atlas.
#[derive(Default)]
pub struct Tiles {
    frames: HashMap<FrameId, FrameTiles>,
    /// Free textures of the size of a tile.
    pool: Vec<MapTexture>,
    /// The pass of the UI.
    tick: u64,
    /// The number of tiles that rendered.
    pub renders: u64,
    pub painted: HashMap<FrameId, FramePaint>,
}

impl Tiles {
    /// The lists of a frame.
    pub fn lists(&self, id: FrameId) -> Option<&FrameLists> {
        self.frames.get(&id).map(|f| &f.lists)
    }

    /// The number of frames with lists.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// The number of tiles with a texture.
    pub fn textures(&self) -> usize {
        self.frames.values().map(FrameTiles::textures).sum()
    }

    /// The size in pixels of a texture of a frame.
    pub fn texture_size(&self, id: FrameId) -> Option<[usize; 2]> {
        let f = self.frames.get(&id)?;
        let mut textures = f.tiles.values().filter_map(|t| t.texture.as_ref());
        textures.next().map(MapTexture::size)
    }

    /// The number of frames with a texture that the GPU drew.
    pub fn gpu_frames(&self) -> usize {
        let gpu = |f: &FrameTiles| {
            (f.tiles.values()).any(|t| matches!(t.texture, Some(MapTexture::Gpu(_))))
        };
        self.frames.values().filter(|f| gpu(f)).count()
    }

    /// Keeps the frames for which `keep` is true.
    pub fn retain(&mut self, mut keep: impl FnMut(FrameId) -> bool) {
        let gone: Vec<FrameId> = self
            .frames
            .keys()
            .copied()
            .filter(|id| !keep(*id))
            .collect();
        for id in gone {
            if let Some(f) = self.frames.remove(&id) {
                self.recycle(f);
            }
        }
    }

    /// Puts the textures of a set of tiles in the pool.
    fn recycle(&mut self, mut f: FrameTiles) {
        if let Some(previous) = f.previous.take() {
            self.recycle(*previous);
        }
        for (_, tile) in f.tiles.drain() {
            self.free(tile);
        }
    }

    fn free(&mut self, tile: Tile) {
        if let Some(texture) = tile.texture
            && self.pool.len() < POOL
        {
            self.pool.push(texture);
        }
    }

    /// Sets new lists for a frame. Its tiles stay as the previous tiles until the new tiles
    /// fill the frame.
    pub fn replace(&mut self, id: FrameId, lists: FrameLists) {
        let previous = match self.frames.remove(&id) {
            None => None,
            Some(mut old) => {
                let older = old.previous.take();
                if old.tiles.values().any(|t| t.texture.is_some()) {
                    if let Some(older) = older {
                        self.recycle(*older);
                    }
                    old.spill = None;
                    Some(Box::new(old))
                } else {
                    self.recycle(old);
                    older
                }
            }
        };
        let tiles = FrameTiles {
            lists,
            tiles: HashMap::new(),
            spill: None,
            spilled: false,
            complete: false,
            previous,
            overviews: HashMap::new(),
        };
        self.frames.insert(id, tiles);
    }

    /// Whether the frame waits for its spill: its tiles fill it and it has no spill yet.
    pub fn wants_spill(&self, id: FrameId) -> bool {
        self.frames
            .get(&id)
            .is_some_and(|f| f.complete && !f.spilled)
    }

    /// Sets the spill of a frame, or marks that it failed.
    pub fn set_spill(&mut self, id: FrameId, spill: Option<Spill>) {
        if let Some(f) = self.frames.get_mut(&id) {
            f.spill = spill;
            f.spilled = true;
        }
    }

    /// Renders the missing tiles of the frames on the canvas, from the middle of each frame out:
    /// at rest at the scale of the screen, and then a ring of tiles around them while fewer than
    /// `cap` tiles with a texture are in use. During a gesture, only the squares that no tile of
    /// another scale fills render, and then the ring. The overview of each list renders first.
    /// Returns true when tiles on the canvas are still missing.
    pub fn fill(
        &mut self,
        ctx: &egui::Context,
        mut renderer: Option<&mut MapRenderer>,
        screen: Screen,
        shown: &[Shown],
        cap: usize,
        timed: bool,
    ) -> Result<bool, String> {
        self.tick += 1;
        let tick = self.tick;
        let (renders, time) = if screen.moving {
            (GESTURE_RENDERS, GESTURE_TIME)
        } else {
            (REST_RENDERS, REST_TIME)
        };
        let mut budget = Budget {
            renders,
            until: timed.then(|| Instant::now() + time),
        };
        let mut more = false;
        let s = screen.scale.to_bits();
        for ring in [false, true] {
            if ring && more {
                break;
            }
            for f in shown {
                if !ring {
                    for layer in LAYERS {
                        if !self.overview(ctx, renderer.as_deref_mut(), f.id, layer, &mut budget)? {
                            more = true;
                        }
                    }
                }
                let Some(set) = self.frames.get(&f.id) else {
                    continue;
                };
                let mut wants: Vec<TileKey> = Vec::new();
                let mut ready: Vec<TileKey> = Vec::new();
                for layer in LAYERS {
                    if set.lists.list(layer).is_none() {
                        continue;
                    }
                    let at = set.placement(layer, &f.vt, screen);
                    if at.moved.is_some() {
                        continue;
                    }
                    let bounds = set.lists.bounds;
                    let visible = at.region(f.shown).intersect(bounds);
                    let scales = set.scales(layer);
                    let mut region = visible;
                    if ring {
                        let side = f64::from(TILE) / f64::from(screen.scale);
                        region = visible.inflate(side, side).intersect(bounds);
                    }
                    let to = if screen.moving && !scales.contains(&s) {
                        quantized(screen.scale).to_bits()
                    } else {
                        s
                    };
                    for slot in slots(screen.scale, region) {
                        let need = slot.intersect(bounds);
                        if set.ready(layer, s, need) {
                            ready.extend(keys_over(layer, s, need));
                            continue;
                        }
                        if screen.moving && set.choose(layer, s, need, &scales).is_some() {
                            continue;
                        }
                        for key in keys_over(layer, to, need) {
                            if !set.tiles.contains_key(&key) && !wants.contains(&key) {
                                wants.push(key);
                            }
                        }
                    }
                }
                // The tiles at the scale of the screen on the canvas and in the ring stay.
                if let Some(set) = self.frames.get_mut(&f.id) {
                    for key in ready {
                        if let Some(tile) = set.tiles.get_mut(&key) {
                            tile.used = tick;
                        }
                    }
                }
                for key in wants {
                    if ring && self.live() >= cap {
                        break;
                    }
                    if !self.render(ctx, renderer.as_deref_mut(), f.id, key, &mut budget)? {
                        more = true;
                        break;
                    }
                }
            }
        }
        Ok(more)
    }

    /// The number of tiles with a texture that this pass of the UI uses.
    fn live(&self) -> usize {
        let live = |f: &FrameTiles| {
            (f.tiles.values())
                .filter(|t| t.used == self.tick && t.texture.is_some())
                .count()
        };
        self.frames.values().map(live).sum()
    }

    /// Renders the overview of a layer of a frame when it has none and the budget allows a
    /// render. Returns false when the overview is still missing.
    fn overview(
        &mut self,
        ctx: &egui::Context,
        renderer: Option<&mut MapRenderer>,
        id: FrameId,
        layer: Layer,
        budget: &mut Budget,
    ) -> Result<bool, String> {
        let Some(set) = self.frames.get_mut(&id) else {
            return Ok(true);
        };
        if set.overviews.contains_key(&layer) {
            return Ok(true);
        }
        let Some(list) = set.lists.list(layer) else {
            return Ok(true);
        };
        if budget.renders == 0 {
            return Ok(false);
        }
        budget.renders -= 1;
        let b = set.lists.bounds;
        let scale = OVERVIEW_SIDE / b.width().max(b.height()).max(1.0);
        let size = |v: f64| ((v * scale).ceil() as u32).max(1);
        let (w, h) = (size(b.width()), size(b.height()));
        let transform = Affine::translate((-b.x0 * scale, -b.y0 * scale)) * Affine::scale(scale);
        let texture = map_texture::draw_region(ctx, renderer, None, list, transform, [w, h])?;
        let rect = kurbo::Rect::new(
            b.x0,
            b.y0,
            b.x0 + f64::from(w) / scale,
            b.y0 + f64::from(h) / scale,
        );
        set.overviews.insert(layer, Overview { texture, rect });
        if budget.until.is_some_and(|t| Instant::now() >= t) {
            budget.renders = 0;
        }
        Ok(true)
    }

    /// Renders one tile, when the budget allows a render. A tile with nothing in it needs no
    /// render. Returns false when the budget is spent.
    fn render(
        &mut self,
        ctx: &egui::Context,
        renderer: Option<&mut MapRenderer>,
        id: FrameId,
        key: TileKey,
        budget: &mut Budget,
    ) -> Result<bool, String> {
        let Some(set) = self.frames.get_mut(&id) else {
            return Ok(true);
        };
        let Some(list) = set.lists.list(key.layer) else {
            return Ok(true);
        };
        let transform = key.transform();
        let tick = self.tick;
        if list.visible_count(transform, TILE, TILE) == 0 {
            let tile = Tile {
                texture: None,
                used: tick,
            };
            set.tiles.insert(key, tile);
            return Ok(true);
        }
        if budget.renders == 0 {
            return Ok(false);
        }
        budget.renders -= 1;
        self.renders += 1;
        let old = self.pool.pop();
        let drawn = map_texture::draw_region(ctx, renderer, old, list, transform, [TILE, TILE]);
        let (texture, result) = match drawn {
            Ok(texture) => (Some(texture), Ok(true)),
            Err(e) => (None, Err(e)),
        };
        set.tiles.insert(
            key,
            Tile {
                texture,
                used: tick,
            },
        );
        if budget.until.is_some_and(|t| Instant::now() >= t) {
            budget.renders = 0;
        }
        result
    }

    /// Paints `layers` of a frame with `painter`, which clips to the part of the frame on the
    /// canvas. With `spill`, the spill shows first while the map moves.
    pub fn paint(
        &mut self,
        painter: &egui::Painter,
        screen: Screen,
        f: &Shown,
        layers: &[Layer],
        spill: bool,
    ) {
        let tick = self.tick;
        let Some(set) = self.frames.get_mut(&f.id) else {
            return;
        };
        let record = self.painted.entry(f.id).or_default();
        let at = set.placement(Layer::Map, &f.vt, screen);
        if spill
            && let Some(sp) = &set.spill
            && at.moved.is_some()
        {
            let mut mesh = egui::Mesh::with_texture(sp.texture.id());
            add_quad(&mut mesh, &at, sp.rect, sp.rect, &f.inside);
            union(
                &mut record.spill,
                mesh.calc_bounds().intersect(painter.clip_rect()),
            );
            painter.add(mesh);
        }
        let s = screen.scale.to_bits();
        for &layer in layers {
            if set.lists.list(layer).is_none() {
                continue;
            }
            let at = set.placement(layer, &f.vt, screen);
            let bounds = set.lists.bounds;
            let region = at.region(f.shown).intersect(bounds);
            let scales = set.scales(layer);
            let plan: Vec<(kurbo::Rect, Option<u32>)> = slots(screen.scale, region)
                .into_iter()
                .map(|slot| (slot, set.choose(layer, s, slot.intersect(bounds), &scales)))
                .collect();
            let empty: Vec<kurbo::Rect> = (plan.iter())
                .filter(|(_, c)| c.is_none())
                .map(|(slot, _)| *slot)
                .collect();
            record.empty += empty.len();
            let mut blank = empty.len();
            let mut still = empty.clone();
            if !empty.is_empty()
                && let Some(prev) = set.previous.as_deref_mut()
                && prev.lists.list(layer).is_some()
            {
                let pat = prev.placement(layer, &f.vt, screen);
                let pbounds = prev.lists.bounds;
                let pslots = if pat.moved.is_none() && at.moved.is_none() {
                    empty
                } else {
                    let mut r = Rect::NOTHING;
                    for slot in &empty {
                        r = r.union(at.screen_box(*slot));
                    }
                    slots(
                        screen.scale,
                        pat.region(r.intersect(f.shown)).intersect(pbounds),
                    )
                };
                let pscales = prev.scales(layer);
                let pplan: Vec<(kurbo::Rect, Option<u32>)> = (pslots.into_iter())
                    .map(|slot| {
                        (
                            slot,
                            prev.choose(layer, s, slot.intersect(pbounds), &pscales),
                        )
                    })
                    .collect();
                blank = pplan.iter().filter(|(_, c)| c.is_none()).count();
                still = if pat.moved.is_none() && at.moved.is_none() {
                    (pplan.iter())
                        .filter(|(_, c)| c.is_none())
                        .map(|(slot, _)| *slot)
                        .collect()
                } else {
                    Vec::new()
                };
                let shown = paint_plan(prev, layer, &pplan, &pat, f, painter, tick, record);
                record.previous += shown;
            }
            if !still.is_empty()
                && let Some(o) = set.overviews.get(&layer)
            {
                let inside: &[[Pos2; 3]] = if layer == Layer::Decor {
                    &[]
                } else {
                    &f.inside
                };
                let mut mesh = egui::Mesh::with_texture(o.texture.id());
                for slot in &still {
                    add_quad(&mut mesh, &at, o.rect, *slot, inside);
                }
                if layer == Layer::Map {
                    union(
                        &mut record.map,
                        mesh.calc_bounds().intersect(painter.clip_rect()),
                    );
                }
                painter.add(mesh);
                blank = 0;
            }
            if layer == Layer::Map {
                record.blank += blank;
            }
            paint_plan(set, layer, &plan, &at, f, painter, tick, record);
        }
    }

    /// Ends the paint of a frame. When its tiles fill it, the previous tiles go.
    pub fn finish(&mut self, id: FrameId) {
        let empty = self.painted.get(&id).is_none_or(|p| p.empty > 0);
        let Some(set) = self.frames.get_mut(&id) else {
            return;
        };
        set.complete = !empty;
        if !empty && let Some(previous) = set.previous.take() {
            self.recycle(*previous);
        }
    }

    /// The reduced copies of the lists of a frame for the blurred backdrop, with their places on
    /// the screen.
    pub fn thumbs(&self, screen: Screen, f: &Shown) -> Vec<(Arc<Thumb>, Rect)> {
        let Some(set) = self.frames.get(&f.id) else {
            return Vec::new();
        };
        let thumbs = set.lists.thumbs();
        let mut out = Vec::new();
        for (layer, thumb) in LAYERS.into_iter().zip(thumbs) {
            if let Some(thumb) = thumb {
                let at = set.placement(layer, &f.vt, screen);
                let rect = at.screen_box(set.lists.bounds).intersect(f.shown);
                out.push((thumb.clone(), rect));
            }
        }
        out
    }

    /// Frees the tiles that no pass of the UI drew for the longest time, until no more than
    /// `cap` tiles have a texture and no more than four times `cap` tiles are kept. The tiles
    /// of this pass stay, and the tiles at another scale than `scale` go first.
    pub fn evict(&mut self, cap: usize, scale: f32) {
        let textures = self.textures();
        let entries: usize = self.frames.values().map(entry_count).sum();
        if textures <= cap && entries <= 4 * cap {
            return;
        }
        let tick = self.tick;
        let s = scale.to_bits();
        let mut old: Vec<Unused> = Vec::new();
        for (id, f) in &self.frames {
            let sets = std::iter::once((false, f)).chain(f.previous.as_deref().map(|p| (true, p)));
            for (previous, set) in sets {
                for (key, tile) in &set.tiles {
                    if tile.used < tick {
                        old.push(Unused {
                            order: (key.scale == s, tile.used),
                            id: *id,
                            previous,
                            key: *key,
                            texture: tile.texture.is_some(),
                        });
                    }
                }
            }
        }
        old.sort_by_key(|o| o.order);
        let (mut textures, mut entries) = (textures, entries);
        for Unused {
            id,
            previous,
            key,
            texture,
            ..
        } in old
        {
            if textures <= cap && entries <= 4 * cap {
                break;
            }
            let Some(f) = self.frames.get_mut(&id) else {
                continue;
            };
            let set = if previous {
                f.previous.as_deref_mut()
            } else {
                Some(f)
            };
            let Some(tile) = set.and_then(|set| set.tiles.remove(&key)) else {
                continue;
            };
            entries -= 1;
            if texture {
                textures -= 1;
            }
            self.free(tile);
        }
    }
}

/// The tile renders that a pass of the UI has left: a number, and with a time, the end of
/// the time.
struct Budget {
    renders: usize,
    until: Option<Instant>,
}

/// A tile that the last pass of the UI did not draw.
struct Unused {
    /// The order of eviction: the tiles at another scale first, and then the oldest.
    order: (bool, u64),
    id: FrameId,
    /// The tile is of the previous lists.
    previous: bool,
    key: TileKey,
    texture: bool,
}

fn entry_count(f: &FrameTiles) -> usize {
    f.tiles.len() + f.previous.as_deref().map_or(0, entry_count)
}

/// Paints the tiles of a plan: each square with the tiles of its scale, cut to the square. Each
/// tile is one mesh. Returns the number of tiles that show.
#[allow(clippy::too_many_arguments)]
fn paint_plan(
    set: &mut FrameTiles,
    layer: Layer,
    plan: &[(kurbo::Rect, Option<u32>)],
    at: &Placement,
    f: &Shown,
    painter: &egui::Painter,
    tick: u64,
    record: &mut FramePaint,
) -> usize {
    let inside: &[[Pos2; 3]] = if layer == Layer::Decor {
        &[]
    } else {
        &f.inside
    };
    let bounds = set.lists.bounds;
    let mut meshes: HashMap<TileKey, egui::Mesh> = HashMap::new();
    for (slot, scale) in plan {
        let Some(scale) = *scale else {
            continue;
        };
        for key in keys_over(layer, scale, slot.intersect(bounds)) {
            let Some(tile) = set.tiles.get_mut(&key) else {
                continue;
            };
            tile.used = tick;
            let Some(texture) = &tile.texture else {
                continue;
            };
            let mesh =
                (meshes.entry(key)).or_insert_with(|| egui::Mesh::with_texture(texture.id()));
            add_quad(mesh, at, key.rect(), *slot, inside);
            if layer == Layer::Map {
                let rest = key.rect().intersect(*slot);
                let rest = Rect::from_two_pos(
                    at.screen.point(rest.origin()),
                    at.screen.point(Point::new(rest.x1, rest.y1)),
                );
                union(&mut record.texture, rest);
                if !record.scales.contains(&key.scale()) {
                    record.scales.push(key.scale());
                }
            }
        }
    }
    let count = meshes.len();
    for (_, mesh) in meshes {
        if mesh.is_empty() {
            continue;
        }
        let rect = mesh.calc_bounds().intersect(painter.clip_rect());
        match layer {
            Layer::Map => union(&mut record.map, rect),
            Layer::Decor => union(&mut record.decor, rect),
            Layer::Under => {}
        }
        painter.add(mesh);
    }
    count
}

/// Adds the part of a tile in `crop` to a mesh. Both are in page points of the tiles. A tile that
/// moves shows only inside the triangles of `inside`, and with no triangles, the whole part
/// shows.
fn add_quad(
    mesh: &mut egui::Mesh,
    at: &Placement,
    tile: kurbo::Rect,
    crop: kurbo::Rect,
    inside: &[[Pos2; 3]],
) {
    let q = tile.intersect(crop);
    if q.width() <= 0.0 || q.height() <= 0.0 {
        return;
    }
    let corners = [(q.x0, q.y0), (q.x1, q.y0), (q.x1, q.y1), (q.x0, q.y1)];
    let uv_of = |p: Point| {
        pos2(
            ((p.x - tile.x0) / tile.width()) as f32,
            ((p.y - tile.y0) / tile.height()) as f32,
        )
    };
    let quad = corners.map(|(x, y)| at.screen_of(Point::new(x, y)));
    let mut fan = |points: &[Pos2], uvs: &dyn Fn(Pos2, usize) -> Pos2| {
        let base = mesh.vertices.len() as u32;
        for (k, &pos) in points.iter().enumerate() {
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv: uvs(pos, k),
                color: Color32::WHITE,
            });
        }
        for i in 2..points.len() as u32 {
            mesh.add_triangle(base, base + i - 1, base + i);
        }
    };
    if at.moved.is_none() || inside.is_empty() {
        let uvs = corners.map(|(x, y)| uv_of(Point::new(x, y)));
        fan(&quad, &|_, k| uvs[k]);
    } else {
        for triangle in inside {
            fan(&clip_convex(triangle, &quad), &|p, _| uv_of(at.page_of(p)));
        }
    }
}

/// The part of the convex polygon `poly` inside the convex polygon `by`.
fn clip_convex(poly: &[Pos2], by: &[Pos2]) -> Vec<Pos2> {
    let cross = |a: Pos2, b: Pos2, p: Pos2| (b - a).x * (p - a).y - (b - a).y * (p - a).x;
    let n = by.len();
    let turn = (0..n)
        .map(|i| cross(by[i], by[(i + 1) % n], by[(i + 2) % n]))
        .sum::<f32>();
    let mut out = poly.to_vec();
    for i in 0..n {
        let (a, b) = (by[i], by[(i + 1) % n]);
        let side = |p: Pos2| cross(a, b, p) * turn.signum();
        let input = std::mem::take(&mut out);
        for (k, &p) in input.iter().enumerate() {
            let q = input[(k + 1) % input.len()];
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                out.push(p + (q - p) * (sp / (sp - sq)));
            }
        }
    }
    out
}
