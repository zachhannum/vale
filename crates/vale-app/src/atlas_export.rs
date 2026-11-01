//! PDF export of atlas pages.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

use image::{DynamicImage, ImageFormat, RgbaImage};
use kurbo::{Point, Rect, Shape};
use vale_labeler::Fonts;
use vale_render::{DisplayList, FillRule, Item, PdfDocument};
use vale_sphere::Projection;
use vale_store::{FrameRecord, ImageId, PageRecord, RasterId};

use crate::atlas::{self, FrameTarget, PT_PER_MM};
use crate::document::Document;
use crate::globe::flat::FlatMesh;
use crate::globe::gpu::{Captured, Mask};
use crate::globe::{FrameDraw, FramePlace};
use crate::pipeline::{Pipeline, Quality, paper_color};
use crate::raster_style;
use crate::{furniture, headless};

/// The pixels in one point of an exported raster layer: 300 pixels per inch.
pub const RASTER_PIXEL_RATIO: f64 = 300.0 / 72.0;

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
    /// strip has a strip of the whole frame.
    draw: FrameDraw,
    /// The first row of pixels of the strip.
    row: u32,
    /// The premultiplied pixels of the whole frame, with the strips that
    /// the GPU drew.
    rgba: Vec<u8>,
    frame: FrameRecord,
    target: FrameTarget,
    part: Rect,
    image_rect: Rect,
    receiver: Option<mpsc::Receiver<Result<Captured, String>>>,
}

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
}

enum Output {
    Pdf(Box<PdfDocument>),
    Png,
}

impl PdfExport {
    pub fn new(doc: Document, path: PathBuf, pages: Vec<PageRecord>) -> anyhow::Result<Self> {
        if pages.is_empty() {
            anyhow::bail!("the atlas has no pages");
        }
        let ext = path.extension().and_then(|e| e.to_str());
        let output = match ext.map(str::to_ascii_lowercase).as_deref() {
            Some("pdf") => Output::Pdf(Box::new(PdfDocument::new())),
            Some("png") if pages.len() == 1 => Output::Png,
            _ => anyhow::bail!(
                "cannot export to {}; use a .pdf file, or .png for one page",
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
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
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
        let rows = pending.draw.rect[3];
        let half = rows.div_ceil(2).max(MIN_STRIP_ROWS);
        if pending.receiver.is_some() || half >= rows {
            return false;
        }
        pending.draw = strip(&pending.whole, pending.row, half);
        true
    }

    /// Gives the pending raster capture its result channel.
    pub fn set_capture_receiver(&mut self, receiver: mpsc::Receiver<Result<Captured, String>>) {
        if let Some(pending) = &mut self.pending {
            pending.receiver = Some(receiver);
        }
    }

    fn page_list(page: &PageRecord) -> DisplayList {
        let size = Point::new(page.width_mm * PT_PER_MM, page.height_mm * PT_PER_MM);
        let mut list = DisplayList::new(size.x, size.y);
        list.push(Item::Fill {
            path: Rect::new(0.0, 0.0, size.x, size.y).to_path(0.1),
            color: paper_color(page),
            rule: FillRule::NonZero,
        });
        list
    }

    fn shown_rasters(&self, frame: &FrameRecord) -> Vec<RasterId> {
        self.doc
            .project
            .layers()
            .iter()
            .filter(|layer| self.doc.shown(frame.map, layer.id))
            .filter_map(|layer| layer.raster())
            .collect()
    }

    fn shown_images(&self, frame: &FrameRecord) -> Vec<ImageId> {
        self.doc
            .project
            .layers()
            .iter()
            .filter(|layer| self.doc.shown(frame.map, layer.id))
            .filter_map(|layer| layer.image())
            .collect()
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
        let pixel = |points: f64| (points * RASTER_PIXEL_RATIO).ceil().max(1.0) as u32;
        let size = [
            pixel(bbox.width() * PT_PER_MM),
            pixel(bbox.height() * PT_PER_MM),
        ];
        let point_size = Point::new(
            f64::from(size[0]) / RASTER_PIXEL_RATIO,
            f64::from(size[1]) / RASTER_PIXEL_RATIO,
        );
        let image_rect = Rect::from_origin_size(point_min, (point_size.x, point_size.y));
        let page_target = FrameTarget {
            origin: Point::ZERO,
            pts_per_mm: PT_PER_MM,
        };
        let under = self
            .pipeline
            .compose_frame_under(&self.doc, frame, page_target, image_rect)?;
        self.list.as_mut().unwrap().items.extend(under.items);

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
        let image = vale_render::render_rgba(&mask_list, RASTER_PIXEL_RATIO)?;
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
        let shown = self.shown_rasters(frame);
        let (raster_looks, image_looks) = raster_style::map_looks(&self.doc, frame.map);
        let draw = FrameDraw {
            id: next_draw_id(),
            place: FramePlace {
                center: [
                    (vt.origin.x * RASTER_PIXEL_RATIO) as f32,
                    (vt.origin.y * RASTER_PIXEL_RATIO) as f32,
                ],
                map_center: [
                    (vt.view.center.x / radius) as f32,
                    (vt.view.center.y / radius) as f32,
                ],
                units_to_pixels: (vt.view.scale * radius * RASTER_PIXEL_RATIO) as f32,
                rotation: vt.view.rotation,
                points_per_radian: radius * PT_PER_MM * 1000.0 / frame.scale,
                pixels_per_page_point: RASTER_PIXEL_RATIO as f32,
            },
            rect: [0, 0, size[0], size[1]],
            mesh: Arc::new(FlatMesh::new(&unit_projection)),
            mask: Some(mask),
            shown,
            images: self.shown_images(frame),
            raster_looks,
            image_looks,
        };
        Ok(PendingCapture {
            rgba: vec![0; size[0] as usize * size[1] as usize * 4],
            row: 0,
            whole: draw.clone(),
            draw,
            frame: frame.clone(),
            target: page_target,
            part: image_rect,
            image_rect,
            receiver: None,
        })
    }

    /// Puts the pixels of a strip into the pixels of the frame. Returns
    /// `true` when the frame has each strip.
    fn take_strip(&mut self, captured: Captured) -> anyhow::Result<bool> {
        let pending = self.pending.as_mut().expect("a capture is pending");
        let [.., width, height] = pending.whole.rect;
        let rows = pending.draw.rect[3];
        if captured.size != [width, rows] {
            anyhow::bail!("the raster capture has the wrong size");
        }
        let start = pending.row as usize * width as usize * 4;
        pending.rgba[start..start + captured.rgba.len()].copy_from_slice(&captured.rgba);
        pending.row += rows;
        pending.receiver = None;
        if pending.row >= height {
            return Ok(true);
        }
        pending.draw = strip(&pending.whole, pending.row, rows);
        Ok(false)
    }

    fn finish_capture(&mut self) -> anyhow::Result<()> {
        let mut pending = self.pending.take().expect("a capture is pending");
        let [.., width, height] = pending.whole.rect;
        let mut rgba = std::mem::take(&mut pending.rgba);
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
        let over = self.pipeline.compose_frame_over(
            &self.doc,
            &pending.frame,
            pending.target,
            pending.part,
            &mut self.fonts,
            Quality::Final,
            None,
        )?;
        self.list.as_mut().unwrap().items.extend(over.list.items);
        self.frame += 1;
        Ok(())
    }

    /// Advances through vector work and returns when the GPU or another UI
    /// frame is needed.
    pub fn step(&mut self) -> ExportStep {
        let result = (|| -> anyhow::Result<ExportStep> {
            if let Some(pending) = &mut self.pending {
                let Some(receiver) = &pending.receiver else {
                    return Ok(ExportStep::Capture(pending.draw.clone()));
                };
                match receiver.try_recv() {
                    Ok(Ok(captured)) => {
                        if !self.take_strip(captured)? {
                            let draw = self.pending.as_ref().unwrap().draw.clone();
                            return Ok(ExportStep::Capture(draw));
                        }
                        self.finish_capture()?;
                    }
                    Ok(Err(error)) => anyhow::bail!(error),
                    Err(mpsc::TryRecvError::Empty) => return Ok(ExportStep::More),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        anyhow::bail!("the raster capture stopped")
                    }
                }
            }

            if self.list.is_none() {
                self.list = Some(Self::page_list(&self.pages[self.page]));
                self.frame = 0;
            }
            let page = self.pages[self.page].clone();
            while let Some(frame) = page.frames.get(self.frame) {
                let page_rect = Rect::new(0.0, 0.0, page.width_mm, page.height_mm);
                let bbox = atlas::shape_bbox(&frame.shape).intersect(page_rect);
                let raster_area = bbox.width() > 0.0 && bbox.height() > 0.0;
                let layered =
                    !self.shown_rasters(frame).is_empty() || !self.shown_images(frame).is_empty();
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
                        None,
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
            if matches!(self.output, Some(Output::Png)) {
                let bytes = vale_render::render_png(&list, RASTER_PIXEL_RATIO)?;
                headless::write_file(&self.path, &bytes)?;
                return Ok(ExportStep::Done(Ok(self.path.clone())));
            }
            let Some(Output::Pdf(pdf)) = &mut self.output else {
                unreachable!("the output is a PDF")
            };
            pdf.add_page(&list)?;
            self.page += 1;
            if self.page < self.pages.len() {
                return Ok(ExportStep::More);
            }
            let Some(Output::Pdf(pdf)) = self.output.take() else {
                unreachable!("the output is a PDF")
            };
            let bytes = pdf.finish()?;
            headless::write_file(&self.path, &bytes)?;
            Ok(ExportStep::Done(Ok(self.path.clone())))
        })();
        match result {
            Ok(step) => step,
            Err(error) => ExportStep::Done(Err(error)),
        }
    }
}

/// The strip of `rows` rows of a frame from row `row`. Each strip has its
/// own id and the part of the mask under it.
fn strip(whole: &FrameDraw, row: u32, rows: u32) -> FrameDraw {
    let [.., width, height] = whole.rect;
    let rows = rows.min(height - row);
    let mut draw = whole.clone();
    draw.id = next_draw_id();
    draw.place.center[1] -= row as f32;
    draw.rect = [0, 0, width, rows];
    draw.mask = whole.mask.as_ref().map(|mask| {
        let w = mask.size[0] as usize;
        let at = row as usize * w;
        Arc::new(Mask {
            id: MASK_ID.fetch_add(1, Ordering::Relaxed),
            size: [mask.size[0], rows],
            alpha: mask.alpha[at..at + rows as usize * w].to_vec(),
        })
    });
    draw
}

/// Writes vector pages as one PDF without a window.
pub fn export_pdf(doc: &Document, pages: Vec<PageRecord>, path: &Path) -> anyhow::Result<()> {
    let mut job = PdfExport::new(doc.clone(), path.to_path_buf(), pages)?;
    loop {
        match job.step() {
            ExportStep::More => {}
            ExportStep::Capture(_) => anyhow::bail!("a raster layer needs a GPU for export"),
            ExportStep::Done(result) => return result.map(|_| ()),
        }
    }
}

/// Writes one vector page to a PNG or PDF without a window.
pub fn export_page(doc: &Document, page: PageRecord, path: &Path) -> anyhow::Result<()> {
    let mut job = PdfExport::new(doc.clone(), path.to_path_buf(), vec![page])?;
    loop {
        match job.step() {
            ExportStep::More => {}
            ExportStep::Capture(_) => anyhow::bail!("a raster layer needs a GPU for export"),
            ExportStep::Done(result) => return result.map(|_| ()),
        }
    }
}
