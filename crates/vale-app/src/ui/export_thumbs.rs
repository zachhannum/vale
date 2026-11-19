//! The small pictures of the pages in the strip of the export form. A job
//! draws the pages as the export does, with the raster layers and the image
//! layers of the frames from the GPU. It draws at most one page or one
//! capture in each UI frame. If the GPU cannot draw the capture of a page,
//! the page has its vector layers only. The pictures stay until the page,
//! the maps, or the raster layers change.

use std::collections::HashMap;

use eframe::egui;
use vale_store::{PageId, PageRecord};

use super::{AppState, CaptureResult};
use crate::atlas::PT_PER_MM;
use crate::atlas_export::{ExportStep, PdfExport};

/// The height of a picture in the strip, in points.
pub const HEIGHT: f32 = 79.0;
/// The narrowest picture, in points.
pub const MIN_WIDTH: f32 = 56.0;
/// The widest picture, in points.
pub const MAX_WIDTH: f32 = 160.0;
/// The pixels in one point of a picture.
const PIXELS: f32 = 2.0;
/// The longest time that a page waits for the GPU, in seconds. After it,
/// the page has its vector layers only.
const WAIT_LIMIT: f64 = 20.0;

/// The change count of the maps, the newest edit of the raster layers, and
/// the page at the time of a picture.
type Key = (Generation, PageRecord);

/// The change count of the maps and the newest edit of the raster layers.
pub type Generation = (u64, u64);

/// The generation of the maps and the raster layers of `state`.
pub fn generation(state: &AppState) -> Generation {
    let globe = &state.globe;
    let rasters = globe.rasters().into_iter();
    let edits = rasters.filter_map(|id| globe.raster_edits(id)).max();
    (state.map_generation, edits.unwrap_or(0))
}

/// The size of the picture of a page in the strip.
pub fn size(page: &PageRecord) -> egui::Vec2 {
    let aspect = (page.width_mm / page.height_mm.max(1e-6)) as f32;
    egui::vec2((HEIGHT * aspect).clamp(MIN_WIDTH, MAX_WIDTH), HEIGHT)
}

struct Thumb {
    key: Key,
    texture: egui::TextureHandle,
    image: egui::ColorImage,
}

/// The job that draws the pictures of some pages.
struct Job {
    export: PdfExport,
    keys: HashMap<PageId, Key>,
    /// The time that the capture of the page started to wait for the GPU.
    waits_since: Option<f64>,
}

/// The pictures of the pages, and the job that draws the pictures that are
/// missing or old.
#[derive(Default)]
pub struct Thumbs {
    cache: HashMap<PageId, Thumb>,
    job: Option<Job>,
    /// The pages that a job could not draw, in the state that it tried.
    failed: HashMap<PageId, Key>,
}

impl Thumbs {
    /// The picture of a page, if it is ready. A picture of an older state of
    /// the page shows until the new one is ready.
    pub fn texture(&self, page: PageId) -> Option<egui::TextureId> {
        self.cache.get(&page).map(|thumb| thumb.texture.id())
    }

    /// The pixels of the picture of a page, if it is ready.
    pub fn image(&self, page: PageId) -> Option<&egui::ColorImage> {
        self.cache.get(&page).map(|thumb| &thumb.image)
    }

    /// True if a job tried to draw the page in its present state and could
    /// not.
    pub fn failed(&self, generation: Generation, page: &PageRecord) -> bool {
        (self.failed.get(&page.id)).is_some_and(|key| key.0 == generation && key.1 == *page)
    }

    /// True if a job draws pictures.
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// True if the page has no picture of its present state, and no job
    /// tried to draw that state.
    fn stale(&self, generation: Generation, page: &PageRecord) -> bool {
        let key = (generation, page.clone());
        let thumb = self.cache.get(&page.id);
        thumb.is_none_or(|t| t.key != key) && self.failed.get(&page.id) != Some(&key)
    }

    /// True if the picture of each page of `pages` shows its present state.
    pub fn ready(&self, generation: Generation, pages: &[PageRecord]) -> bool {
        pages.iter().all(|page| {
            (self.cache.get(&page.id)).is_some_and(|t| t.key.0 == generation && t.key.1 == *page)
        })
    }
}

/// True if the export form is open and a picture of a page is not ready.
pub fn pending(state: &AppState) -> bool {
    let generation = generation(state);
    let thumbs = &state.export_thumbs;
    let pages = &state.doc.project.atlas.pages;
    state.export_form.is_some()
        && (thumbs.busy() || pages.iter().any(|page| thumbs.stale(generation, page)))
}

/// Draws the next page of the pictures of the export form, or gives the GPU
/// the next capture of the page. It does nothing while an export runs, and
/// it stops the job when the form closes.
pub fn poll(ui: &egui::Ui, state: &mut AppState) {
    if state.atlas_export.is_some() {
        return;
    }
    if state.export_form.is_none() {
        if state.export_thumbs.job.take().is_some() {
            state.end_export_frame();
        }
        return;
    }
    let ctx = ui.ctx();
    let generation = generation(state);
    let pages = &state.doc.project.atlas.pages;
    let thumbs = &mut state.export_thumbs;
    if thumbs.job.is_none() {
        let stale: Vec<PageRecord> = (pages.iter())
            .filter(|page| thumbs.stale(generation, page))
            .cloned()
            .collect();
        let Some(first) = stale.first() else {
            return;
        };
        let ratio = f64::from(HEIGHT * PIXELS) / (first.height_mm * PT_PER_MM).max(1.0);
        let keys: HashMap<PageId, Key> = (stale.iter())
            .map(|page| (page.id, (generation, page.clone())))
            .collect();
        super::atlas::update_key_levels(state, &stale, false, true);
        // The pages of one job have the scale of the first page.
        match PdfExport::images(state.doc.clone(), stale, ratio) {
            Ok(mut export) => {
                export.set_frame_levels(state.pipeline.frame_levels().clone());
                let rasters = state.pipeline.rasters_mut();
                export.set_rasters(rasters.frozen());
                state.export_thumbs.job = Some(Job {
                    export,
                    keys,
                    waits_since: None,
                });
            }
            Err(error) => {
                log::warn!("page pictures: {error:#}");
                state.export_thumbs.failed.extend(keys);
                return;
            }
        }
    }
    let Some(mut job) = state.export_thumbs.job.take() else {
        return;
    };
    let step = job.export.step();
    let thumbs = &mut state.export_thumbs;
    for (id, image) in job.export.take_images() {
        let Some(key) = job.keys.remove(&id) else {
            continue;
        };
        // The job does not try a page with the vector layers only again
        // until the page changes.
        if job.export.vector_only(id) {
            thumbs.failed.insert(id, key.clone());
        }
        let size = [image.width, image.height];
        let color = egui::ColorImage::from_rgba_premultiplied(size, &image.data);
        let name = format!("export-page-{}", id.0);
        let texture = ctx.load_texture(name, color.clone(), egui::TextureOptions::LINEAR);
        let image = color;
        thumbs.cache.insert(
            id,
            Thumb {
                key,
                texture,
                image,
            },
        );
    }
    ctx.request_repaint();
    match step {
        ExportStep::More => state.export_thumbs.job = Some(job),
        ExportStep::Capture(draw) => {
            let result = match state.globe.format {
                Some(_) => state.serve_capture(&mut job.export, draw, Some(ui)),
                None => CaptureResult::NoGpu,
            };
            let now = ctx.input(|i| i.time);
            let failed = match result {
                CaptureResult::Waiting => {
                    let since = *job.waits_since.get_or_insert(now);
                    now - since > WAIT_LIMIT
                }
                CaptureResult::Sent | CaptureResult::Split => {
                    job.waits_since = None;
                    false
                }
                CaptureResult::NoGpu => true,
                CaptureResult::Failed(error) => {
                    log::warn!("page pictures: {error}");
                    true
                }
            };
            // The page gets a picture of its vector layers.
            if failed {
                job.waits_since = None;
                job.export.drop_page_layers();
                state.end_export_frame();
            }
            state.export_thumbs.job = Some(job);
        }
        ExportStep::Done(result) => {
            if let Err(error) = result {
                log::warn!("page pictures: {error:#}");
            }
            state.export_thumbs.failed.extend(job.keys);
            state.end_export_frame();
        }
    }
}
