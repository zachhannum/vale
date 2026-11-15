//! The small pictures of the pages in the strip of the export form. A job
//! draws one page in each UI frame, with the vector layers of the frames
//! only, and the pictures stay until the page or the maps change.

use std::collections::HashMap;

use eframe::egui;
use vale_store::{PageId, PageRecord};

use super::AppState;
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

/// The change count of the maps and the page at the time of a picture.
type Key = (u64, PageRecord);

/// The size of the picture of a page in the strip.
pub fn size(page: &PageRecord) -> egui::Vec2 {
    let aspect = (page.width_mm / page.height_mm.max(1e-6)) as f32;
    egui::vec2((HEIGHT * aspect).clamp(MIN_WIDTH, MAX_WIDTH), HEIGHT)
}

struct Thumb {
    key: Key,
    texture: egui::TextureHandle,
}

/// The job that draws the pictures of some pages.
struct Job {
    export: PdfExport,
    keys: HashMap<PageId, Key>,
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

    /// True if the picture of each page of `pages` shows its present state.
    pub fn ready(&self, generation: u64, pages: &[PageRecord]) -> bool {
        pages.iter().all(|page| {
            (self.cache.get(&page.id)).is_some_and(|t| t.key.0 == generation && t.key.1 == *page)
        })
    }
}

/// Draws the next page of the pictures of the export form. It does
/// nothing while no export form is open or while an export runs.
pub fn poll(ctx: &egui::Context, state: &mut AppState) {
    if state.export_form.is_none() || state.atlas_export.is_some() {
        return;
    }
    let generation = state.map_generation;
    let pages = &state.doc.project.atlas.pages;
    let thumbs = &mut state.export_thumbs;
    if thumbs.job.is_none() {
        let stale: Vec<PageRecord> = (pages.iter())
            .filter(|page| {
                let key = (generation, (*page).clone());
                let thumb = thumbs.cache.get(&page.id);
                thumb.is_none_or(|t| t.key != key) && thumbs.failed.get(&page.id) != Some(&key)
            })
            .cloned()
            .collect();
        let Some(first) = stale.first() else {
            return;
        };
        let ratio = f64::from(HEIGHT * PIXELS) / (first.height_mm * PT_PER_MM).max(1.0);
        let keys: HashMap<PageId, Key> = (stale.iter())
            .map(|page| (page.id, (generation, page.clone())))
            .collect();
        // The pages of one job have the scale of the first page.
        match PdfExport::images(state.doc.clone(), stale, ratio) {
            Ok(mut export) => {
                let rasters = state.pipeline.rasters_mut();
                export.set_rasters(rasters.frozen());
                state.export_thumbs.job = Some(Job { export, keys });
            }
            Err(error) => {
                log::warn!("page pictures: {error:#}");
                state.export_thumbs.failed.extend(keys);
                return;
            }
        }
    }
    let thumbs = &mut state.export_thumbs;
    let Some(job) = &mut thumbs.job else {
        return;
    };
    let step = job.export.step();
    for (id, image) in job.export.take_images() {
        let Some(key) = job.keys.remove(&id) else {
            continue;
        };
        let size = [image.width, image.height];
        let color = egui::ColorImage::from_rgba_premultiplied(size, &image.data);
        let name = format!("export-page-{}", id.0);
        let texture = ctx.load_texture(name, color, egui::TextureOptions::LINEAR);
        thumbs.cache.insert(id, Thumb { key, texture });
    }
    match step {
        ExportStep::More => ctx.request_repaint(),
        other => {
            if let ExportStep::Done(Err(error)) = other {
                log::warn!("page pictures: {error:#}");
            }
            if let Some(job) = thumbs.job.take() {
                thumbs.failed.extend(job.keys);
            }
            ctx.request_repaint();
        }
    }
}
