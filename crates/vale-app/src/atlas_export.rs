//! PDF and PNG export of atlas pages.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

use image::{DynamicImage, ImageFormat, RgbaImage};
use kurbo::{Point, Rect, Shape};
use vale_labeler::Fonts;
use vale_render::Rgba;
use vale_render::{DisplayList, FillRule, Item, PdfDocument};
use vale_sphere::Projection;
use vale_store::{FrameRecord, LayerId, PageId, PageRecord};

use crate::atlas::{self, FrameTarget, PT_PER_MM};
use crate::document::Document;
use crate::globe::flat::FlatMesh;
use crate::globe::gpu::{Captured, Mask};
use crate::globe::{FrameDraw, FrameItem, FramePlace, RasterMask};
use crate::mask::frame::{cover_mask, frame_raster_masks};
use crate::pipeline::{FrameDetail, Pipeline, Quality, paper_color};
use crate::raster_style;
use crate::{furniture, headless};

/// The pixels in one point of a raster export at `dpi` dots per inch.
pub fn pixel_ratio(dpi: u32) -> f64 {
    f64::from(dpi) / 72.0
}

/// The pixels of a length of `points` in a raster export at `dpi`.
pub fn pixels(points: f64, dpi: u32) -> u32 {
    (points * pixel_ratio(dpi)).ceil().max(1.0) as u32
}

/// The width and the height of a page in a PNG export, in pixels. A map
/// frame that fills the page has raster images of the same size.
pub fn page_pixels(page: &PageRecord) -> [u32; 2] {
    let side = |mm: f64| pixels(mm * PT_PER_MM, page.dpi);
    [side(page.width_mm), side(page.height_mm)]
}

/// The most pixels on a side of a texture that each GPU takes.
pub const MAX_SIDE: u32 = 8192;
/// The most times that egui drops the capture of one strip before the
/// export fails.
const MAX_LOST: u32 = 60;

/// The options of an export.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ExportOptions {
    /// The page has no paper fill.
    pub transparent: bool,
}

/// The fewest rows of pixels in a strip of an exported frame.
pub const MIN_STRIP_ROWS: u32 = 64;

static MASK_ID: AtomicU64 = AtomicU64::new(1);

/// The ids of the frames that an export draws. They are not the ids of the
/// frames on the screen.
static DRAW_ID: AtomicU32 = AtomicU32::new(1 << 31);

fn next_draw_id() -> u32 {
    DRAW_ID.fetch_add(1, Ordering::Relaxed) | (1 << 31)
}

struct PendingCapture {
    /// The whole frame.
    whole: FrameDraw,
    /// The strip of the frame that the GPU draws next. A frame with one
    /// strip has a strip of the whole frame. A strip has at most `max_side`
    /// pixels on a side, so a wide frame has more than one strip in a row.
    draw: FrameDraw,
    /// The first row and the first column of pixels of the strip.
    row: u32,
    col: u32,
    /// The most pixels on a side of a texture of the GPU.
    max_side: u32,
    /// The premultiplied pixels of the whole frame for each run of
    /// `FrameDraw::runs`, with the strips that the GPU drew. A run gets its
    /// pixels with its first strip.
    rgba: Vec<Vec<u8>>,
    /// The layers with a raster mask, with the cover of the mask in the
    /// whole frame, one byte for each pixel.
    masks: Vec<(LayerId, RasterMask, Vec<u8>)>,
    frame: FrameRecord,
    target: FrameTarget,
    part: Rect,
    image_rect: Rect,
    /// Page millimeters to the pixels of the raster image.
    pixels: FrameTarget,
    /// The capture of each run in the strip, then the capture of the cover
    /// of each mask, with what arrived. `None` is a run with nothing to
    /// draw, or a mask with no cover.
    receivers: Option<Vec<Receiver>>,
    /// The captures of the strip that egui dropped before the GPU drew
    /// them, as in a UI frame that egui ran and did not paint.
    lost: u32,
}

/// A capture that the GPU makes, and its result once it arrives.
type Receiver = Option<(mpsc::Receiver<Result<Captured, String>>, Option<Captured>)>;

/// The next action of a PDF export.
pub enum ExportStep {
    /// The next page starts on a later UI frame.
    More,
    /// Draw these raster layers on the GPU.
    Capture(FrameDraw),
    /// The export ended.
    Done(anyhow::Result<PathBuf>),
}

/// An export that keeps one page display list at a time.
pub struct PdfExport {
    doc: Document,
    path: PathBuf,
    pages: Vec<PageRecord>,
    page: usize,
    frame: usize,
    list: Option<DisplayList>,
    pending: Option<PendingCapture>,
    pipeline: Pipeline,
    fonts: Fonts,
    output: Option<Output>,
    options: ExportOptions,
    /// The raster masks of a frame with no raster layers get a capture.
    gpu: bool,
    /// The most pixels on a side of a texture of the GPU.
    max_side: u32,
    /// The raster layers and the image layers of a frame go to the GPU.
    /// Without them, a frame has its vector layers only.
    layers: bool,
    /// The pages that have their vector layers only, because their
    /// capture failed.
    vector_pages: Vec<usize>,
}

enum Output {
    Pdf(Box<PdfDocument>),
    /// One file for each page.
    Png {
        paths: Vec<PathBuf>,
    },
    /// One image for each page, in memory.
    Images {
        /// The pixels in one point of each image.
        ratio: f64,
        images: Vec<(PageId, Rgba)>,
    },
}

/// The PNG files of an export of `pages` to `path`. One page goes to
/// `path`. Each of several pages goes to `<name>-<page number>.png` next to
/// `path`, with the number of the page in the atlas of `doc`.
pub fn png_paths(doc: &Document, path: &Path, pages: &[PageRecord]) -> Vec<PathBuf> {
    if pages.len() == 1 {
        return vec![path.to_path_buf()];
    }
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let atlas = &doc.project.atlas.pages;
    (pages.iter().enumerate())
        .map(|(index, page)| {
            let number = atlas.iter().position(|p| p.id == page.id).unwrap_or(index) + 1;
            path.with_file_name(format!("{stem}-{number}.png"))
        })
        .collect()
}

impl PdfExport {
    pub fn new(
        doc: Document,
        path: PathBuf,
        pages: Vec<PageRecord>,
        options: ExportOptions,
    ) -> anyhow::Result<Self> {
        if pages.is_empty() {
            anyhow::bail!("the atlas has no pages");
        }
        let ext = path.extension().and_then(|e| e.to_str());
        let output = match ext.map(str::to_ascii_lowercase).as_deref() {
            Some("pdf") => Output::Pdf(Box::new(PdfDocument::new())),
            Some("png") => Output::Png {
                paths: png_paths(&doc, &path, &pages),
            },
            _ => anyhow::bail!(
                "cannot export to {}; use a .pdf or .png file",
                path.display()
            ),
        };
        let fonts = crate::fonts::system()?;
        Ok(PdfExport {
            doc,
            path,
            pages,
            page: 0,
            frame: 0,
            list: None,
            pending: None,
            pipeline: Pipeline::new(),
            fonts,
            output: Some(output),
            options,
            gpu: true,
            max_side: MAX_SIDE,
            layers: true,
            vector_pages: Vec::new(),
        })
    }

    /// An export of `pages` to images in memory, with `ratio` pixels in one
    /// point. The raster images of the frames also have `ratio` pixels in
    /// one point.
    pub fn images(doc: Document, pages: Vec<PageRecord>, ratio: f64) -> anyhow::Result<Self> {
        let options = ExportOptions::default();
        let mut job = PdfExport::new(doc, PathBuf::from("pages.png"), pages, options)?;
        job.output = Some(Output::Images {
            ratio,
            images: Vec::new(),
        });
        Ok(job)
    }

    /// Composes the page that the export composes again, with the vector
    /// layers of its frames only.
    pub fn drop_page_layers(&mut self) {
        self.pending = None;
        self.list = None;
        self.frame = 0;
        self.pipeline.clear_page_masks();
        self.vector_pages.push(self.page);
    }

    /// True if the export composes or composed `page` with the vector
    /// layers of its frames only.
    pub fn vector_only(&self, page: PageId) -> bool {
        (self.vector_pages.iter()).any(|&i| self.pages.get(i).is_some_and(|p| p.id == page))
    }

    /// Takes the images of the pages that the export finished.
    pub fn take_images(&mut self) -> Vec<(PageId, Rgba)> {
        match &mut self.output {
            Some(Output::Images { images, .. }) => std::mem::take(images),
            _ => Vec::new(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The files that the export writes.
    pub fn files(&self) -> Vec<PathBuf> {
        match &self.output {
            Some(Output::Png { paths }) => paths.clone(),
            _ => vec![self.path.clone()],
        }
    }

    /// Gives the export the raster values under the features.
    pub fn set_rasters(&mut self, rasters: crate::raster_values::RasterCache) {
        *self.pipeline.rasters_mut() = rasters;
    }

    /// Gives the export the levels inside the frames of the color keys.
    pub fn set_frame_levels(&mut self, levels: crate::color_key::FrameLevels) {
        self.pipeline.set_frame_levels(levels);
    }

    /// Sets the most pixels on a side of a texture of the GPU.
    pub fn set_max_side(&mut self, side: u32) {
        self.max_side = side.max(MIN_STRIP_ROWS);
    }

    /// The page that the next step composes, and the total page count.
    pub fn progress(&self) -> (usize, usize) {
        ((self.page + 1).min(self.pages.len()), self.pages.len())
    }

    /// Draws the rest of the pending frame in strips of half the rows of
    /// the strip that waits. Returns `false` if the strip has
    /// `MIN_STRIP_ROWS` or fewer, or the GPU draws it now.
    pub fn split_capture(&mut self) -> bool {
        let Some(pending) = &mut self.pending else {
            return false;
        };
        let [.., cols, rows] = pending.draw.rect;
        let half = rows.div_ceil(2).max(MIN_STRIP_ROWS);
        if pending.receivers.is_some() || half >= rows {
            return false;
        }
        let at = (pending.col, pending.row);
        pending.draw = strip(&pending.whole, at, [cols, half]);
        true
    }

    /// The raster masks of the frame of the pending capture. Each needs a
    /// capture of its cover in the strip, from `Globe::capture_frame_mask`.
    pub fn capture_masks(&self) -> Vec<RasterMask> {
        let masks = self.pending.iter().flat_map(|p| &p.masks);
        masks.map(|(_, mask, _)| *mask).collect()
    }

    /// Gives the pending raster capture the result channel of each run of
    /// `FrameDraw::runs`, and the channel of the cover of each mask of
    /// `capture_masks`. A run with no channel draws nothing. A mask with no
    /// channel has no cover, and its layer draws no fills and no strokes.
    pub fn set_capture_receiver(
        &mut self,
        runs: Vec<Option<mpsc::Receiver<Result<Captured, String>>>>,
        masks: Vec<Option<mpsc::Receiver<Result<Captured, String>>>>,
    ) {
        if let Some(pending) = &mut self.pending {
            let count = pending.rgba.len();
            let runs = runs.into_iter().chain(std::iter::repeat_with(|| None));
            let all = runs.take(count).chain(masks);
            pending.receivers = Some(all.map(|r| r.map(|r| (r, None))).collect());
        }
    }

    fn page_list(page: &PageRecord, transparent: bool) -> DisplayList {
        let size = Point::new(page.width_mm * PT_PER_MM, page.height_mm * PT_PER_MM);
        let mut list = DisplayList::new(size.x, size.y);
        if transparent {
            return list;
        }
        list.push(Item::Fill {
            path: Rect::new(0.0, 0.0, size.x, size.y).to_path(0.1),
            color: paper_color(page),
            rule: FillRule::NonZero,
        });
        list
    }

    /// The pixels in one point of the raster images of the page that the
    /// export draws.
    pub fn pixel_ratio(&self) -> f64 {
        self.page_ratio(&self.pages[self.page.min(self.pages.len() - 1)])
    }

    /// The pixels in one point of the raster images of `page`.
    fn page_ratio(&self, page: &PageRecord) -> f64 {
        match &self.output {
            Some(Output::Images { ratio, .. }) => *ratio,
            _ => pixel_ratio(page.dpi),
        }
    }

    fn capture(
        &mut self,
        page: &PageRecord,
        frame: &FrameRecord,
    ) -> anyhow::Result<PendingCapture> {
        let bbox = atlas::shape_bbox(&frame.shape).intersect(Rect::new(
            0.0,
            0.0,
            page.width_mm,
            page.height_mm,
        ));
        if !(bbox.width() > 0.0 && bbox.height() > 0.0) {
            anyhow::bail!("the map frame has no area on the page");
        }
        let point_min = Point::new(bbox.x0 * PT_PER_MM, bbox.y0 * PT_PER_MM);
        let ratio = self.page_ratio(page);
        let side = |mm: f64| (mm * PT_PER_MM * ratio).ceil().max(1.0) as u32;
        let size = [side(bbox.width()), side(bbox.height())];
        let point_size = Point::new(f64::from(size[0]) / ratio, f64::from(size[1]) / ratio);
        let image_rect = Rect::from_origin_size(point_min, (point_size.x, point_size.y));
        let page_target = FrameTarget {
            origin: Point::ZERO,
            pts_per_mm: PT_PER_MM,
        };
        let capture_target = FrameTarget {
            origin: Point::new(-point_min.x, -point_min.y),
            pts_per_mm: PT_PER_MM,
        };
        let part = Rect::new(0.0, 0.0, point_size.x, point_size.y);

        let spec = self.doc.projection(frame.map);
        let radius = self.doc.project.world.radius_km * 1000.0;
        let projection = self.pipeline.projection(&self.doc, frame.map)?;
        let vt = atlas::frame_transform(projection, frame, capture_target);
        let unit_projection =
            Projection::new(spec, 1.0).map_err(|error| anyhow::anyhow!("projection: {error}"))?;
        let mask_list = Pipeline::frame_mask(frame, capture_target, (part.width(), part.height()));
        let image = vale_render::render_rgba(&mask_list, ratio)?;
        let mask = Arc::new(Mask {
            id: MASK_ID.fetch_add(1, Ordering::Relaxed),
            size,
            alpha: image
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| pixel[3])
                .collect(),
        });
        let layers = raster_style::frame_items(&self.doc, frame.map);
        let draw = FrameDraw {
            id: next_draw_id(),
            place: FramePlace {
                center: [(vt.origin.x * ratio) as f32, (vt.origin.y * ratio) as f32],
                map_center: [
                    (vt.view.center.x / radius) as f32,
                    (vt.view.center.y / radius) as f32,
                ],
                units_to_pixels: (vt.view.scale * radius * ratio) as f32,
                rotation: vt.view.rotation,
                points_per_radian: radius * PT_PER_MM * 1000.0 / frame.scale,
                pixels_per_page_point: ratio as f32,
            },
            rect: [0, 0, size[0], size[1]],
            mesh: Arc::new(FlatMesh::new(&unit_projection)),
            mask: Some(mask),
            layers,
            mask_looks: raster_style::hidden_looks(&self.doc, frame.map),
        };
        let pixel_count = size[0] as usize * size[1] as usize;
        let masks = match self.gpu {
            true => frame_raster_masks(&self.doc, frame),
            false => Vec::new(),
        };
        Ok(PendingCapture {
            rgba: vec![Vec::new(); draw.runs().len()],
            row: 0,
            col: 0,
            max_side: self.max_side,
            draw: strip(&draw, (0, 0), [self.max_side, self.max_side]),
            whole: draw,
            masks: (masks.into_iter())
                .map(|(layer, mask)| (layer, mask, vec![0; pixel_count]))
                .collect(),
            frame: frame.clone(),
            target: page_target,
            part: image_rect,
            image_rect,
            pixels: FrameTarget {
                origin: Point::new(-point_min.x * ratio, -point_min.y * ratio),
                pts_per_mm: PT_PER_MM * ratio,
            },
            receivers: None,
            lost: 0,
        })
    }

    /// Puts the pixels of a strip, and the covers of the masks in it, into
    /// the pixels of the frame. Returns `true` when the frame has each
    /// strip.
    fn take_strip(&mut self, captured: Vec<Option<Captured>>) -> anyhow::Result<bool> {
        let pending = self.pending.as_mut().expect("a capture is pending");
        let [.., width, height] = pending.whole.rect;
        let [.., cols, rows] = pending.draw.rect;
        let mut captured = captured.into_iter();
        let covers = pending.masks.iter_mut().map(|(_, _, cover)| (cover, 1));
        let outs = pending.rgba.iter_mut().map(|run| (run, 4)).chain(covers);
        for (out, bytes) in outs {
            let Some(strip) = captured.next().flatten() else {
                continue;
            };
            if strip.size != [cols, rows] || strip.rgba.len() != (cols * rows * bytes) as usize {
                anyhow::bail!("the raster capture has the wrong size");
            }
            out.resize(width as usize * height as usize * bytes as usize, 0);
            let line = (cols * bytes) as usize;
            for (r, src) in strip.rgba.chunks_exact(line).enumerate() {
                let start = (((pending.row + r as u32) * width + pending.col) * bytes) as usize;
                out[start..start + line].copy_from_slice(src);
            }
        }
        pending.col += cols;
        if pending.col >= width {
            pending.col = 0;
            pending.row += rows;
        }
        pending.receivers = None;
        pending.lost = 0;
        if pending.row >= height {
            return Ok(true);
        }
        let at = (pending.col, pending.row);
        pending.draw = strip(&pending.whole, at, [pending.max_side, rows]);
        Ok(false)
    }

    fn finish_capture(&mut self) -> anyhow::Result<()> {
        let mut pending = self.pending.take().expect("a capture is pending");
        let [.., width, height] = pending.whole.rect;
        let frame = pending.frame.id;
        for (layer, _, cover) in &pending.masks {
            let mask = cover_mask(cover, width as usize, height as usize, pending.pixels);
            self.pipeline.set_page_mask(frame, *layer, Some(mask));
        }
        let under = self.pipeline.compose_frame_under(
            &self.doc,
            &pending.frame,
            pending.target,
            pending.image_rect,
        )?;
        self.list.as_mut().unwrap().items.extend(under.items);
        let over = self.pipeline.compose_frame_over(
            &self.doc,
            &pending.frame,
            pending.target,
            pending.part,
            &mut self.fonts,
            Quality::Final,
            FrameDetail::Full,
        )?;
        // Each run of raster layers goes over the vector group below it.
        let mut groups = over.vector_groups().into_iter();
        let runs = std::mem::take(&mut pending.rgba);
        for rgba in runs {
            if let Some(group) = groups.next() {
                self.list.as_mut().unwrap().items.extend(group.items);
            }
            self.push_image(&pending, rgba)?;
        }
        for group in groups {
            self.list.as_mut().unwrap().items.extend(group.items);
        }
        let decor = over.list.items.into_iter().skip(over.decor);
        self.list.as_mut().unwrap().items.extend(decor);
        self.pipeline.clear_page_masks();
        self.frame += 1;
        Ok(())
    }

    /// Puts the premultiplied pixels of a run of raster layers of a frame on
    /// the page. A run with no pixels adds nothing.
    fn push_image(&mut self, pending: &PendingCapture, mut rgba: Vec<u8>) -> anyhow::Result<()> {
        if rgba.is_empty() || rgba.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0) {
            return Ok(());
        }
        let [.., width, height] = pending.whole.rect;
        for pixel in rgba.as_chunks_mut::<4>().0 {
            let alpha = u32::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = (u32::from(*channel) * 255)
                    .checked_div(alpha)
                    .unwrap_or(0)
                    .min(255) as u8;
            }
        }
        let image = RgbaImage::from_raw(width, height, rgba)
            .ok_or_else(|| anyhow::anyhow!("the raster capture has the wrong size"))?;
        let mut png = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image).write_to(&mut png, ImageFormat::Png)?;
        self.list.as_mut().unwrap().push(Item::Image {
            png: png.into_inner(),
            rect: pending.image_rect,
            interpolate: true,
        });
        Ok(())
    }

    /// Advances through vector work and returns when the GPU or another UI
    /// frame is needed.
    pub fn step(&mut self) -> ExportStep {
        let result = (|| -> anyhow::Result<ExportStep> {
            if let Some(pending) = &mut self.pending {
                let Some(receivers) = &mut pending.receivers else {
                    return Ok(ExportStep::Capture(pending.draw.clone()));
                };
                let mut lost = false;
                let mut failed = None;
                for (receiver, got) in receivers.iter_mut().flatten() {
                    if got.is_some() {
                        continue;
                    }
                    match receiver.try_recv() {
                        Ok(Ok(captured)) => *got = Some(captured),
                        Ok(Err(error)) => failed = Some(error),
                        Err(mpsc::TryRecvError::Empty) => return Ok(ExportStep::More),
                        Err(mpsc::TryRecvError::Disconnected) => lost = true,
                    }
                }
                // A capture that egui dropped goes to the GPU again.
                if lost && failed.is_none() {
                    pending.receivers = None;
                    pending.lost += 1;
                    if pending.lost <= MAX_LOST {
                        return Ok(ExportStep::Capture(pending.draw.clone()));
                    }
                    failed = Some("the raster capture stopped".to_string());
                }
                if let Some(error) = failed {
                    // A picture of a page has the vector layers at least.
                    if !matches!(self.output, Some(Output::Images { .. })) {
                        anyhow::bail!(error);
                    }
                    log::warn!("page pictures: {error}");
                    self.drop_page_layers();
                    return Ok(ExportStep::More);
                }
                let receivers = pending.receivers.take().unwrap_or_default();
                let captured = (receivers.into_iter())
                    .map(|r| r.and_then(|(_, got)| got))
                    .collect();
                if !self.take_strip(captured)? {
                    let draw = self.pending.as_ref().unwrap().draw.clone();
                    return Ok(ExportStep::Capture(draw));
                }
                self.finish_capture()?;
            }

            if self.page >= self.pages.len() {
                return self.finish();
            }
            if self.list.is_none() {
                let transparent = self.options.transparent;
                self.list = Some(Self::page_list(&self.pages[self.page], transparent));
                self.frame = 0;
            }
            let page = self.pages[self.page].clone();
            while let Some(frame) = page.frames.get(self.frame) {
                let page_rect = Rect::new(0.0, 0.0, page.width_mm, page.height_mm);
                let bbox = atlas::shape_bbox(&frame.shape).intersect(page_rect);
                let raster_area = bbox.width() > 0.0 && bbox.height() > 0.0;
                let masked = self.gpu && !frame_raster_masks(&self.doc, frame).is_empty();
                let layered = self.layers
                    && !self.vector_pages.contains(&self.page)
                    && (raster_style::frame_items(&self.doc, frame.map)
                        .iter()
                        .any(|item| *item != FrameItem::Cut)
                        || masked);
                if !layered || !raster_area {
                    let target = FrameTarget {
                        origin: Point::ZERO,
                        pts_per_mm: PT_PER_MM,
                    };
                    let part = Rect::new(
                        0.0,
                        0.0,
                        page.width_mm * PT_PER_MM,
                        page.height_mm * PT_PER_MM,
                    );
                    let composed = self.pipeline.compose_frame_part(
                        &self.doc,
                        frame,
                        target,
                        part,
                        &mut self.fonts,
                        Quality::Final,
                        FrameDetail::Full,
                    )?;
                    self.list
                        .as_mut()
                        .unwrap()
                        .items
                        .extend(composed.list.items);
                    self.frame += 1;
                    continue;
                }
                self.pending = Some(self.capture(&page, frame)?);
                return Ok(ExportStep::Capture(
                    self.pending.as_ref().unwrap().draw.clone(),
                ));
            }

            let target = FrameTarget {
                origin: Point::ZERO,
                pts_per_mm: PT_PER_MM,
            };
            for rec in &page.furniture {
                let purpose = furniture::Purpose::Export;
                let items = furniture::items(
                    &mut self.pipeline,
                    &self.doc,
                    rec,
                    target,
                    &mut self.fonts,
                    purpose,
                );
                self.list.as_mut().unwrap().items.extend(items);
            }
            let list = self.list.take().unwrap();
            match &mut self.output {
                Some(Output::Png { paths }) => {
                    let bytes = vale_render::render_png(&list, pixel_ratio(page.dpi))?;
                    headless::write_file(&paths[self.page], &bytes)?;
                }
                Some(Output::Pdf(pdf)) => pdf.add_page(&list)?,
                Some(Output::Images { ratio, images }) => {
                    let image = vale_render::render_rgba(&list, *ratio)?;
                    images.push((page.id, image));
                }
                None => unreachable!("the export has an output"),
            }
            self.page += 1;
            if self.page < self.pages.len() {
                return Ok(ExportStep::More);
            }
            self.finish()
        })();
        match result {
            Ok(step) => step,
            Err(error) => ExportStep::Done(Err(error)),
        }
    }

    /// Writes the PDF file after the last page.
    fn finish(&mut self) -> anyhow::Result<ExportStep> {
        if let Some(Output::Pdf(_)) = self.output {
            let Some(Output::Pdf(pdf)) = self.output.take() else {
                unreachable!("the output is a PDF")
            };
            let bytes = pdf.finish()?;
            headless::write_file(&self.path, &bytes)?;
        }
        Ok(ExportStep::Done(Ok(self.path.clone())))
    }
}

/// The strip of `cols` columns and `rows` rows of a frame from the column
/// and the row `(col, row)`. Each strip has its own id and the part of the
/// mask under it.
fn strip(whole: &FrameDraw, (col, row): (u32, u32), [cols, rows]: [u32; 2]) -> FrameDraw {
    let [.., width, height] = whole.rect;
    let (cols, rows) = (cols.min(width - col), rows.min(height - row));
    let mut draw = whole.clone();
    draw.id = next_draw_id();
    draw.place.center[0] -= col as f32;
    draw.place.center[1] -= row as f32;
    draw.rect = [0, 0, cols, rows];
    draw.mask = whole.mask.as_ref().map(|mask| {
        let w = mask.size[0] as usize;
        let (col, cols) = (col as usize, cols as usize);
        let lines = mask.alpha.chunks_exact(w).skip(row as usize);
        let alpha = (lines.take(rows as usize))
            .flat_map(|line| &line[col..col + cols])
            .copied()
            .collect();
        Arc::new(Mask {
            id: MASK_ID.fetch_add(1, Ordering::Relaxed),
            size: [cols as u32, rows],
            alpha,
        })
    });
    draw
}

/// Writes vector pages as one PDF without a window.
pub fn export_pdf(doc: &Document, pages: Vec<PageRecord>, path: &Path) -> anyhow::Result<()> {
    export_pages(doc, pages, path, ExportOptions::default())
}

/// Writes one vector page to a PNG or PDF without a window.
pub fn export_page(doc: &Document, page: PageRecord, path: &Path) -> anyhow::Result<()> {
    export_pages(doc, vec![page], path, ExportOptions::default())
}

/// Writes vector pages to one PDF, or to PNG files, without a window.
pub fn export_pages(
    doc: &Document,
    pages: Vec<PageRecord>,
    path: &Path,
    options: ExportOptions,
) -> anyhow::Result<()> {
    let mut job = PdfExport::new(doc.clone(), path.to_path_buf(), pages, options)?;
    job.gpu = false;
    loop {
        match job.step() {
            ExportStep::More => {}
            ExportStep::Capture(_) => anyhow::bail!("a raster layer needs a GPU for export"),
            ExportStep::Done(result) => return result.map(|_| ()),
        }
    }
}
