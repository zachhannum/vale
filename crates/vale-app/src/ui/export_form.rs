//! The form that exports pages of the atlas to a PDF file or to PNG files.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};
use vale_store::{PageId, PageRecord};

use super::pad::geometry::{GAP, MARGIN, ROW, Rects, TOUCH, WidthClass};
use super::pad::icons::{self, Icon};
use super::pad::theme;
use super::pad::widgets::{self, BODY_PAD, Controls};
use super::{Action, AppState, ExportFormat, ExportSpec, atlas, export_thumbs};
use crate::atlas::preset_of;
use crate::atlas_export::page_pixels;
use crate::color_key::number_text;
use crate::document::Document;

/// The row of the world menu that opens the form, and the button of the
/// form that starts the export.
pub const EXPORT: &str = "Export…";
pub const TITLE: &str = "Export";
pub const CANCEL: &str = "Cancel";
pub const CANCEL_EXPORT: &str = "Cancel export";
pub const FORMAT: &str = "Format";
/// The choices of the format, in the order of `FORMATS_ORDER`.
pub const FORMATS: [&str; 2] = ["PDF", "PNG"];
const FORMATS_ORDER: [ExportFormat; 2] = [ExportFormat::Pdf, ExportFormat::Png];
pub const PAGES: &str = "Pages";
/// The choices of the pages, in the order of `MODES`.
pub const PAGE_CHOICES: [&str; 3] = ["All", "This page", "Range"];
const MODES: [PagesMode; 3] = [PagesMode::All, PagesMode::This, PagesMode::Range];
/// The name of the field of the range, which has no label.
pub const RANGE: &str = "Page range";
pub const OPTIONS: &str = "Options";
pub const TRANSPARENT_PAPER: &str = "Transparent paper";
pub const NAME: &str = "Name";
pub const DETAILS: &str = "Details";
pub const PAGE_SIZE: &str = "Page size";
pub const RESOLUTION: &str = "Resolution";
pub const IMAGE_SIZE: &str = "Image size";
pub const RASTER_SIZE: &str = "Raster size";
/// The value of a detail that differs between the selected pages.
pub const MIXED: &str = "Mixed";
/// The value of a detail with no selected page.
pub const NO_VALUE: &str = "–";
/// The width of the form in the iPad layout.
pub const WIDTH: f32 = 540.0;
/// The ID of the card of the form.
pub const CARD: &str = "export-form";
/// The ID of the strip of the pages.
const STRIP: &str = "export-strip";
/// The space above the pictures of the strip.
const STRIP_TOP: f32 = 16.0;
/// The space between two pictures of the strip.
const STRIP_GAP: f32 = 14.0;
/// The space between a picture and the ring of a selected page.
const RING: f32 = 5.0;
/// The time that a finger stays still on a page before a drag selects a
/// run of pages, in seconds.
pub const HOLD: f64 = 0.3;
/// The width of the band at each end of the strip where a run scrolls
/// the strip.
const EDGE: f32 = 40.0;
/// The speed of the scroll of a run at an end of the strip, in points per
/// second.
const EDGE_SPEED: f32 = 480.0;
/// The space below the last row.
const BOTTOM: f32 = 20.0;

/// The name of the picture of a page in the strip.
pub fn page_name(index: usize) -> String {
    format!("Page {}", index + 1)
}

/// The options of the form that the next form of the session starts with.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub format: ExportFormat,
    pub transparent: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            format: ExportFormat::Pdf,
            transparent: false,
        }
    }
}

/// The choice of pages of the form.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PagesMode {
    /// Each page of the atlas.
    All,
    /// The open page.
    This,
    /// The ticked pages of the strip, or the pages of `range`.
    Range,
}

/// The state of the export form.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportForm {
    pub format: ExportFormat,
    /// The pages of the atlas, in order.
    pub page_ids: Vec<PageId>,
    /// The index of the open page.
    pub open: usize,
    /// One tick for each page of the strip.
    pub selected: Vec<bool>,
    pub pages: PagesMode,
    /// The page numbers of the range, for example "1–4, 7".
    pub range: String,
    pub transparent: bool,
    /// The file name with no extension.
    pub name: String,
    /// The page that the strip scrolls to on the next frame.
    pub center_on: Option<usize>,
    /// The page of the last click on the strip in the desktop layout.
    pub anchor: Option<usize>,
    /// The error of the last export of the form.
    pub error: Option<String>,
}

impl ExportForm {
    /// A form of the pages of the atlas of `state`, with the options of the
    /// last form of the session. It starts with each page.
    pub fn new(state: &AppState) -> Self {
        let pages = state.doc.project.atlas.pages.iter().map(|p| p.id).collect();
        let (open, _) = atlas::page_index(state);
        Self::with_pages(pages, open, state.export_options, state.export_stem())
    }

    /// A form of these pages, with the page at `open` open.
    pub fn with_pages(page_ids: Vec<PageId>, open: usize, options: Options, name: String) -> Self {
        let count = page_ids.len();
        let mut form = ExportForm {
            format: options.format,
            page_ids,
            open: open.min(count.saturating_sub(1)),
            selected: vec![false; count],
            pages: PagesMode::All,
            range: String::new(),
            transparent: options.transparent,
            name,
            center_on: None,
            anchor: None,
            error: None,
        };
        form.set_mode(PagesMode::All);
        form.center_on = Some(form.open);
        form
    }

    /// The options that the next form of the session starts with.
    pub fn options(&self) -> Options {
        Options {
            format: self.format,
            transparent: self.transparent,
        }
    }

    /// Ticks the pages of a choice. "This page" scrolls the strip to the
    /// open page. A range keeps the ticks of the strip.
    pub fn set_mode(&mut self, mode: PagesMode) {
        self.pages = mode;
        match mode {
            PagesMode::All => {
                self.selected.fill(true);
                self.range.clear();
            }
            PagesMode::This => {
                self.selected.fill(false);
                if let Some(tick) = self.selected.get_mut(self.open) {
                    *tick = true;
                }
                self.range.clear();
                self.center_on = Some(self.open);
            }
            PagesMode::Range => self.range = range_text(&self.selected),
        }
    }

    /// Ticks or clears one page of the strip.
    pub fn toggle(&mut self, index: usize) {
        if let Some(tick) = self.selected.get_mut(index) {
            *tick = !*tick;
            self.set_mode(PagesMode::Range);
        }
    }

    /// Gives each page from `a` to `b` the tick `on`.
    pub fn select_run(&mut self, a: usize, b: usize, on: bool) {
        let last = self.selected.len().saturating_sub(1);
        let (start, end) = (a.min(b).min(last), a.max(b).min(last));
        if let Some(run) = self.selected.get_mut(start..=end) {
            run.fill(on);
        }
        self.set_mode(PagesMode::Range);
    }

    /// Sets the text of the range as the user types it, and ticks the
    /// pages that it gives.
    pub fn set_range(&mut self, text: &str) {
        self.pages = PagesMode::Range;
        self.range = text.to_string();
        self.selected.fill(false);
        let count = self.selected.len();
        for (first, last) in range_runs(text).flatten() {
            let last = last.min(count);
            let first = first.clamp(1, last.max(1));
            if let Some(run) = self.selected.get_mut(first.saturating_sub(1)..last) {
                run.fill(true);
            }
        }
    }

    /// Writes the range of the ticked pages in the form of `range_text`.
    pub fn normalize_range(&mut self) {
        self.range = range_text(&self.selected);
    }

    /// True if the form has a page and a name.
    pub fn can_export(&self) -> bool {
        self.selected.contains(&true) && !self.name.trim().is_empty()
    }

    /// The details of the export of the ticked pages of `doc`.
    pub fn details(&self, doc: &Document) -> Details {
        let pages: Vec<&PageRecord> = (self.page_ids.iter().zip(&self.selected))
            .filter(|(_, tick)| **tick)
            .filter_map(|(id, _)| doc.project.atlas.pages.iter().find(|p| p.id == *id))
            .collect();
        let pixels = |page: &&PageRecord| {
            let [w, h] = page_pixels(page);
            let side = |n: u32| number_text(f64::from(n));
            format!("{} × {} px", side(w), side(h))
        };
        Details {
            page_size: shared(pages.iter().map(|p| page_size_text(p))),
            resolution: shared(pages.iter().map(|p| format!("{} dpi", p.dpi))),
            image_size: shared(pages.iter().map(pixels)),
        }
    }

    /// The export of the ticked pages.
    pub fn spec(&self) -> ExportSpec {
        let pages = (self.page_ids.iter().zip(&self.selected))
            .filter(|(_, tick)| **tick)
            .map(|(id, _)| *id)
            .collect();
        ExportSpec {
            format: self.format,
            pages,
            transparent: self.transparent,
            name: self.name.trim().to_string(),
        }
    }
}

/// The ticked pages as text: runs of page numbers in order, with an en
/// dash in a run of two or more pages, for example "1–4, 7, 9–10".
pub fn range_text(selected: &[bool]) -> String {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (index, _) in selected.iter().enumerate().filter(|(_, tick)| **tick) {
        match runs.last_mut() {
            Some((_, last)) if *last + 1 == index + 1 => *last = index + 1,
            _ => runs.push((index + 1, index + 1)),
        }
    }
    let text: Vec<String> = (runs.iter())
        .map(|&(first, last)| match first == last {
            true => first.to_string(),
            false => format!("{first}–{last}"),
        })
        .collect();
    text.join(", ")
}

/// The items of a page range, separated by commas or spaces. An item is a
/// page number, or two page numbers with a hyphen, an en dash, or an em
/// dash between them, in either order. Each item gives its first and last
/// page number, or `None` if it is not a page or a run of pages. A page
/// number of 0 is not a page.
pub fn range_runs(text: &str) -> impl Iterator<Item = Option<(usize, usize)>> {
    let mut text = text.replace(['–', '—'], "-");
    while text.contains(" -") || text.contains("- ") {
        text = text.replace(" -", "-").replace("- ", "-");
    }
    let items: Vec<String> = (text.split(|c: char| c == ',' || c.is_whitespace()))
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect();
    items.into_iter().map(|item| {
        let number = |n: &str| n.parse::<usize>().ok().filter(|n| *n >= 1);
        match item.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (number(a)?, number(b)?);
                Some((a.min(b), a.max(b)))
            }
            None => number(&item).map(|n| (n, n)),
        }
    })
}

/// The read-only facts of the export of the selected pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Details {
    /// The paper size and the orientation of a named size, and the size in
    /// the units of the page, for example "A4 landscape · 297 × 210 mm".
    pub page_size: String,
    /// The resolution, for example "300 dpi".
    pub resolution: String,
    /// The size in pixels of each page in a PNG, or of the raster images of
    /// a map frame that fills the page in a PDF.
    pub image_size: String,
}

/// The label of the row of the pixel size for a format.
pub fn image_label(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Pdf => RASTER_SIZE,
        ExportFormat::Png => IMAGE_SIZE,
    }
}

/// The size of a page as text, for example "A4 landscape · 297 × 210 mm",
/// or "11 × 9 in" for a size with no name.
pub fn page_size_text(page: &PageRecord) -> String {
    let (w, h, units) = (page.width_mm, page.height_mm, page.units);
    let size = format!(
        "{} × {} {}",
        super::pad::atlas::length_text(w, units),
        super::pad::atlas::length_text(h, units),
        super::pad::atlas::unit_name(units),
    );
    match preset_of(w, h) {
        "Custom" => size,
        name => {
            let orientation = super::pad::atlas::ORIENTATIONS[usize::from(w > h)];
            format!("{name} {} · {size}", orientation.to_lowercase())
        }
    }
}

/// The text that the selected pages share, "Mixed" if they differ, or "–"
/// with no page.
fn shared(values: impl IntoIterator<Item = String>) -> String {
    let mut values = values.into_iter();
    let Some(first) = values.next() else {
        return NO_VALUE.to_string();
    };
    match values.all(|v| v == first) {
        true => first,
        false => MIXED.to_string(),
    }
}

/// A run of pages that a drag selects: the first page, the tick that the
/// run gives, and the ticks before the run.
#[derive(Clone)]
struct Run {
    first: usize,
    on: bool,
    before: Vec<bool>,
}

/// The strip of the pages: a picture of each page, with a tick on each
/// page that the export has. A tap ticks or clears a page. A finger that
/// stays still for `HOLD`, then moves, ticks or clears each page that it
/// goes over; at an end of the strip, the strip scrolls. A quick drag
/// scrolls the strip.
fn strip(
    ui: &mut egui::Ui,
    state: &AppState,
    form: &mut ExportForm,
    controls: &mut Controls,
    enabled: bool,
) {
    let pages = &state.doc.project.atlas.pages;
    let sizes: Vec<egui::Vec2> = (form.page_ids.iter())
        .map(|id| {
            let page = pages.iter().find(|p| p.id == *id);
            page.map_or(vec2(112.0, export_thumbs::HEIGHT), export_thumbs::size)
        })
        .collect();
    let height = STRIP_TOP + export_thumbs::HEIGHT + 2.0 * RING;
    let width = ui.available_width();
    let content = sizes.iter().map(|s| s.x).sum::<f32>()
        + STRIP_GAP * sizes.len().saturating_sub(1) as f32
        + 2.0 * RING;
    // A strip that is narrower than the form is in the middle of it.
    let lead = ((width - content) / 2.0).max(0.0) + RING;
    let lefts: Vec<f32> = (sizes.iter())
        .scan(lead, |x, size| {
            let left = *x;
            *x += size.x + STRIP_GAP;
            Some(left)
        })
        .collect();
    let mut scroll = egui::ScrollArea::horizontal()
        .id_salt(STRIP)
        .max_height(height)
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if let Some(index) = form.center_on.take()
        && let (Some(left), Some(size)) = (lefts.get(index), sizes.get(index))
    {
        let most = (content + 2.0 * (lead - RING) - width).max(0.0);
        let offset = (left + size.x / 2.0 - width / 2.0).clamp(0.0, most);
        scroll = scroll.horizontal_scroll_offset(offset);
    }
    let run_id = egui::Id::new(STRIP).with("run");
    scroll.show(ui, |ui| {
        let total = (content + 2.0 * (lead - RING)).max(width);
        let (area, _) = ui.allocate_exact_size(vec2(total, height), Sense::hover());
        let visible = ui.clip_rect();
        let rects: Vec<Rect> = (lefts.iter().zip(&sizes))
            .map(|(left, size)| {
                Rect::from_min_size(
                    pos2(area.left() + left, area.top() + STRIP_TOP + RING),
                    *size,
                )
            })
            .collect();
        let mut run: Option<Run> = ui.data(|d| d.get_temp(run_id));
        let mut dragging = false;
        // The drag of a run starts in the frame after the run.
        let mut taken = false;
        for (index, &rect) in rects.iter().enumerate() {
            let name = page_name(index);
            let resp = widgets::tap_area(ui, controls, rect.expand(RING), &name, enabled);
            let held = ui.input(|i| {
                let start = i.pointer.press_start_time().unwrap_or(i.time);
                i.time - start >= HOLD
            });
            if widgets::take_drag(ui, &resp, |_| held) {
                let on = !form.selected[index];
                taken = true;
                run = Some(Run {
                    first: index,
                    on,
                    before: form.selected.clone(),
                });
            }
            if resp.dragged() {
                dragging = true;
            } else if resp.clicked() {
                form.toggle(index);
            }
            let page = form
                .page_ids
                .get(index)
                .and_then(|id| pages.iter().find(|p| p.id == *id));
            paint_page(ui, state, page, rect, form.selected[index], enabled);
        }
        match (&run, dragging) {
            (Some(run), true) => {
                let x = ui.input(|i| i.pointer.latest_pos()).map_or(0.0, |p| p.x);
                let last = (rects.iter())
                    .rposition(|r| x >= r.left() - STRIP_GAP / 2.0)
                    .unwrap_or(0);
                form.selected.clone_from(&run.before);
                form.select_run(run.first, last, run.on);
                let dt = ui.input(|i| i.stable_dt).min(0.1);
                let step = EDGE_SPEED * dt;
                if x < visible.left() + EDGE {
                    ui.scroll_with_delta(vec2(step, 0.0));
                    ui.ctx().request_repaint();
                } else if x > visible.right() - EDGE {
                    ui.scroll_with_delta(vec2(-step, 0.0));
                    ui.ctx().request_repaint();
                }
            }
            (Some(_), false) if !taken => run = None,
            _ => {}
        }
        ui.data_mut(|d| match run {
            Some(run) => {
                d.insert_temp(run_id, run);
            }
            None => d.remove::<Run>(run_id),
        });
    });
}

/// The picture of a page in the strip, with the ring and the tick of a
/// selected page. The page is dim with an empty circle while it is not
/// selected. The paper color shows until the picture is ready.
fn paint_page(
    ui: &egui::Ui,
    state: &AppState,
    page: Option<&vale_store::PageRecord>,
    rect: Rect,
    on: bool,
    enabled: bool,
) {
    let painter = ui.painter();
    let dim = |color: egui::Color32| match (on, enabled) {
        (true, true) => color,
        (true, false) | (false, true) => color.gamma_multiply(theme::OFF),
        (false, false) => color.gamma_multiply(theme::OFF * theme::OFF),
    };
    if on {
        let ring = rect.expand(RING - 1.5);
        let stroke = egui::Stroke::new(3.0, widgets::fade(theme::ACCENT, enabled));
        painter.rect_stroke(ring, 6.0, stroke, egui::StrokeKind::Middle);
    }
    let paper = page.map_or([255; 3], |p| p.paper);
    let paper = egui::Color32::from_rgb(paper[0], paper[1], paper[2]);
    painter.rect_filled(rect, 3.0, dim(paper));
    let texture = page.and_then(|p| state.export_thumbs.texture(p.id));
    if let Some(texture) = texture {
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(texture, rect, uv, dim(egui::Color32::WHITE));
    }
    let badge = Rect::from_center_size(rect.right_bottom() - vec2(15.0, 15.0), vec2(20.0, 20.0));
    if on {
        painter.circle_filled(badge.center(), 10.0, widgets::fade(theme::ACCENT, enabled));
        icons::paint(painter, badge, Icon::Tick, 14.0, theme::CANVAS);
    } else {
        let fill = egui::Color32::from_white_alpha(217);
        painter.circle_filled(badge.center(), 10.0, dim(fill));
        let stroke = egui::Stroke::new(1.5, dim(theme::MUTE));
        painter.circle_stroke(badge.center(), 9.25, stroke);
    }
}

/// What the user did on the form in one frame.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Did {
    Nothing,
    Close,
    Export,
}

/// The form in the iPad layout: a form sheet near the top of the screen,
/// over a dark fill that takes each touch. On a compact screen, the form
/// is a sheet with the height of the screen.
pub fn pad(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let Some(mut form) = state.export_form.take() else {
        return;
    };
    let busy = state.atlas_export.is_some();
    let compact = rects.class == WidthClass::Compact;
    let screen = rects.screen;
    // Each frame has the height of the rows of the frame before it.
    let body_id = egui::Id::new(CARD).with("body");
    let body = ctx.data(|d| d.get_temp::<f32>(body_id)).unwrap_or(460.0);
    let (rect, radius) = if compact {
        let rect = Rect::from_min_max(pos2(screen.left(), rects.bands.top), screen.max);
        let r = theme::CARD_RADIUS as u8;
        let radius = egui::CornerRadius {
            nw: r,
            ne: r,
            sw: 0,
            se: 0,
        };
        (rect, radius)
    } else {
        // On a partially folded device, the form is in one part of the
        // canvas.
        let part = super::pad::geometry::canvas_part(screen, rects.fold);
        let bottom = part.bottom() - rects.bands.bottom;
        let width = WIDTH.min(part.width() - 2.0 * MARGIN);
        let top = rects.workspace.bottom() + GAP;
        let height = (ROW + body).min(bottom - top - GAP).max(ROW);
        let rect = Rect::from_min_size(
            pos2(part.center().x - width / 2.0, top),
            vec2(width, height),
        );
        (rect, theme::CARD_RADIUS.into())
    };

    // The form covers each other control.
    controls.clear();
    let mut did = Did::Nothing;
    let mut stop = false;
    widgets::form_card(ctx, CARD, screen, rect, radius, compact, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let ready = form.can_export() && !busy;
        let bar = widgets::form_bar(ui, controls, TITLE, CANCEL, EXPORT, ready);
        if bar.cancel {
            did = Did::Close;
        } else if bar.action {
            did = Did::Export;
        }
        let out = egui::ScrollArea::vertical()
            .id_salt(CARD)
            .max_height(rect.height() - ROW)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let margin = egui::Margin::symmetric(BODY_PAD as i8, 0);
                egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    rows(ui, state, &mut form, controls, busy);
                    if busy {
                        stop = progress(ui, state, controls);
                    } else if let Some(error) = &form.error {
                        let size = vec2(ui.available_width(), 36.0);
                        let (line, _) = ui.allocate_exact_size(size, Sense::hover());
                        let at = pos2(line.left() + 2.0, line.center().y);
                        let painter = ui.painter().with_clip_rect(line);
                        painter.text(at, Align2::LEFT_CENTER, error, theme::body(), theme::WARN);
                    }
                    // The progress is above the details, so it stays in view.
                    details_rows(ui, state, &form);
                    let bottom = if compact { rects.bands.bottom } else { 0.0 };
                    ui.add_space(BOTTOM + bottom);
                });
            });
        ctx.data_mut(|d| d.insert_temp(body_id, out.content_size.y));
    });
    if stop {
        state.actions.push(Action::CancelExport);
    }
    finish(state, form, did, busy);
}

/// Keeps the form open, closes it, or asks for the place of the export.
/// A close while the export runs stops the export.
fn finish(state: &mut AppState, mut form: ExportForm, did: Did, busy: bool) {
    match did {
        Did::Nothing => state.export_form = Some(form),
        Did::Close => {
            if busy {
                state.actions.push(Action::CancelExport);
            }
        }
        Did::Export => {
            form.error = None;
            state.export_options = form.options();
            state.actions.push(Action::ExportDialog(form.spec()));
            state.export_form = Some(form);
        }
    }
}

/// The keyboard shortcut that opens the form: Cmd+E, or Ctrl+E off macOS.
pub fn shortcut() -> egui::KeyboardShortcut {
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::E)
}

/// The strip of the pages in the desktop layout. A click ticks or clears a
/// page. A Shift+click gives each page from the page of the last click to
/// this page the tick of the page of the last click.
fn desktop_strip(ui: &mut egui::Ui, state: &AppState, form: &mut ExportForm, enabled: bool) {
    let pages = &state.doc.project.atlas.pages;
    let sizes: Vec<egui::Vec2> = (form.page_ids.iter())
        .map(|id| {
            let page = pages.iter().find(|p| p.id == *id);
            page.map_or(vec2(112.0, export_thumbs::HEIGHT), export_thumbs::size)
        })
        .collect();
    let mut scroll = egui::ScrollArea::horizontal()
        .id_salt((STRIP, "desktop"))
        .auto_shrink([false, true]);
    if let Some(index) = form.center_on.take() {
        let left: f32 = sizes[..index.min(sizes.len())]
            .iter()
            .map(|s| s.x + 2.0 * RING + STRIP_GAP)
            .sum();
        let size = sizes.get(index).map_or(0.0, |s| s.x);
        let offset = left + size / 2.0 + RING - ui.available_width() / 2.0;
        scroll = scroll.horizontal_scroll_offset(offset.max(0.0));
    }
    scroll.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = STRIP_GAP;
            for (index, size) in sizes.iter().enumerate() {
                let page =
                    (form.page_ids.get(index)).and_then(|id| pages.iter().find(|p| p.id == *id));
                let sense = if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                };
                let (rect, resp) = ui.allocate_exact_size(*size + vec2(2.0, 2.0) * RING, sense);
                let name = page_name(index);
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name)
                });
                if resp.clicked() {
                    match form.anchor {
                        Some(anchor) if ui.input(|i| i.modifiers.shift) => {
                            let on = form.selected.get(anchor).copied().unwrap_or(true);
                            form.select_run(anchor, index, on);
                        }
                        _ => {
                            form.toggle(index);
                            form.anchor = Some(index);
                        }
                    }
                }
                let on = form.selected.get(index).copied().unwrap_or(false);
                paint_page(ui, state, page, rect.shrink(RING), on, enabled);
            }
        });
    });
}

/// The form in the desktop layout: a modal window.
pub fn desktop(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut form) = state.export_form.take() else {
        return;
    };
    let busy = state.atlas_export.is_some();
    let enabled = !busy;
    let mut did = Did::Nothing;
    let mut stop = false;
    let modal = egui::Modal::new(egui::Id::new(CARD)).show(ctx, |ui| {
        ui.set_width(WIDTH);
        ui.heading(TITLE);
        ui.add_space(6.0);
        desktop_strip(ui, state, &mut form, enabled);
        ui.add_space(8.0);
        ui.add_enabled_ui(enabled, |ui| {
            egui::Grid::new((CARD, "grid"))
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| desktop_rows(ui, &mut form));
        });
        ui.add_space(8.0);
        ui.add_space(8.0);
        if busy {
            ui.label(state.atlas_export_progress().unwrap_or_default());
            let (page, count) = (state.atlas_export.as_ref()).map_or((0, 1), |job| job.progress());
            let done = (page.saturating_sub(1) as f32 / count.max(1) as f32).clamp(0.0, 1.0);
            ui.add(egui::ProgressBar::new(done));
            stop = ui.button(CANCEL_EXPORT).clicked();
        } else if let Some(error) = &form.error {
            ui.colored_label(ui.visuals().warn_fg_color, error);
        }
        ui.add_space(8.0);
        ui.label(egui::RichText::new(DETAILS).strong());
        let details = form.details(&state.doc);
        egui::Grid::new((CARD, "details"))
            .num_columns(2)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                let rows = [
                    (PAGE_SIZE, &details.page_size),
                    (RESOLUTION, &details.resolution),
                    (image_label(form.format), &details.image_size),
                ];
                for (label, value) in rows {
                    ui.label(label);
                    ui.weak(value.as_str());
                    ui.end_row();
                }
            });
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ready = form.can_export() && !busy;
            if ui.add_enabled(ready, egui::Button::new(EXPORT)).clicked() {
                did = Did::Export;
            }
            if ui.button(CANCEL).clicked() {
                did = Did::Close;
            }
        });
    });
    if modal.should_close() && did == Did::Nothing {
        did = Did::Close;
    }
    if stop {
        state.actions.push(Action::CancelExport);
    }
    finish(state, form, did, busy);
}

/// The rows of the form in the desktop layout, in a grid of two columns.
fn desktop_rows(ui: &mut egui::Ui, form: &mut ExportForm) {
    ui.label(FORMAT);
    ui.horizontal(|ui| {
        for (format, name) in FORMATS_ORDER.into_iter().zip(FORMATS) {
            if ui.selectable_label(form.format == format, name).clicked() {
                form.format = format;
            }
        }
    });
    ui.end_row();

    ui.label(PAGES);
    ui.horizontal(|ui| {
        for (mode, name) in MODES.into_iter().zip(PAGE_CHOICES) {
            if ui.selectable_label(form.pages == mode, name).clicked() {
                form.set_mode(mode);
            }
        }
        if form.pages == PagesMode::Range {
            ui.add_space(8.0);
            let mut text = form.range.clone();
            let edit = egui::TextEdit::singleline(&mut text).desired_width(ui.available_width());
            let resp = ui.add(edit.id_salt(RANGE));
            resp.widget_info(|| {
                let mut info = egui::WidgetInfo::text_edit(true, &text, &text, "");
                info.label = Some(RANGE.to_owned());
                info
            });
            if resp.changed() {
                form.set_range(&text);
            }
            if resp.lost_focus() {
                form.normalize_range();
            }
        }
    });
    ui.end_row();

    if form.format == ExportFormat::Png {
        ui.label("");
        ui.checkbox(&mut form.transparent, TRANSPARENT_PAPER);
        ui.end_row();
    }

    ui.label(NAME);
    ui.horizontal(|ui| {
        let width = ui.available_width() - 40.0;
        ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(width));
        ui.weak(format!(".{}", form.format.ext()));
    });
    ui.end_row();
}

/// The rows of the form below its bar.
fn rows(
    ui: &mut egui::Ui,
    state: &AppState,
    form: &mut ExportForm,
    controls: &mut Controls,
    busy: bool,
) {
    let enabled = !busy;
    strip(ui, state, form, controls, enabled);

    widgets::form_label(ui, FORMAT);
    let active = FORMATS_ORDER.iter().position(|f| *f == form.format);
    if let Some(index) = widgets::segmented(ui, controls, &FORMATS, active, enabled) {
        form.format = FORMATS_ORDER[index];
    }

    widgets::form_label(ui, PAGES);
    let active = MODES.iter().position(|m| *m == form.pages);
    if let Some(index) = widgets::segmented(ui, controls, &PAGE_CHOICES, active, enabled) {
        form.set_mode(MODES[index]);
    }
    if form.pages == PagesMode::Range {
        ui.add_space(4.0);
        range_field(ui, controls, form, enabled);
    }

    if form.format == ExportFormat::Png {
        widgets::form_label(ui, OPTIONS);
        let on = &mut form.transparent;
        widgets::switch_row_enabled(
            ui,
            controls,
            TRANSPARENT_PAPER,
            TRANSPARENT_PAPER,
            on,
            enabled,
        );
    }

    widgets::form_label(ui, NAME);
    let ext = form.format.ext();
    widgets::file_name_field(ui, controls, NAME, &mut form.name, ext, enabled);
}

/// The field of the range, below the choice of the pages, which ticks the
/// pages as the user types. When it loses the focus, it shows the range in
/// the form of `range_text`.
fn range_field(ui: &mut egui::Ui, controls: &mut Controls, form: &mut ExportForm, enabled: bool) {
    let (field, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH), Sense::hover());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field));
    if !enabled {
        child.disable();
    }
    let mut text = form.range.clone();
    let valid = range_runs(&text).all(|run| run.is_some());
    let resp = widgets::field_at(
        &mut child,
        controls,
        field,
        RANGE,
        None,
        &mut text,
        valid,
        theme::body(),
    );
    if resp.changed() {
        form.set_range(&text);
    }
    if resp.lost_focus() {
        form.normalize_range();
    }
}

/// The Details section: the page size, the resolution, the image size, and
/// the files of the selected pages.
fn details_rows(ui: &mut egui::Ui, state: &AppState, form: &ExportForm) {
    let details = form.details(&state.doc);
    widgets::form_label(ui, DETAILS);
    widgets::detail_row(ui, PAGE_SIZE, &details.page_size);
    widgets::detail_row(ui, RESOLUTION, &details.resolution);
    widgets::detail_row(ui, image_label(form.format), &details.image_size);
}

/// The progress of the export that runs, and the button that stops it.
/// Returns true if the user stopped the export.
fn progress(ui: &mut egui::Ui, state: &AppState, controls: &mut Controls) -> bool {
    let text = state.atlas_export_progress().unwrap_or_default();
    ui.add_space(14.0);
    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let at = pos2(line.left() + 2.0, line.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, text, theme::body(), theme::TEXT);
    ui.add_space(8.0);
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    ui.painter()
        .rect_filled(bar, theme::capsule(bar), theme::raise());
    let (page, count) = state
        .atlas_export
        .as_ref()
        .map_or((0, 1), |job| job.progress());
    let done = (page.saturating_sub(1) as f32 / count.max(1) as f32).clamp(0.0, 1.0);
    let filled = Rect::from_min_size(bar.min, vec2(bar.width() * done, bar.height()));
    ui.painter()
        .rect_filled(filled, theme::capsule(bar), theme::ACCENT);
    ui.add_space(12.0);
    widgets::button_row_in(ui, controls, CANCEL_EXPORT, true, theme::WARN).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(count: u32, open: usize) -> ExportForm {
        let pages = (1..=count).map(PageId).collect();
        ExportForm::with_pages(pages, open, Options::default(), "Terra".to_string())
    }

    fn ticks(form: &ExportForm) -> Vec<usize> {
        (form.selected.iter().enumerate())
            .filter(|(_, tick)| **tick)
            .map(|(index, _)| index)
            .collect()
    }

    #[test]
    fn all_pages_ticks_each_page() {
        let mut form = form(5, 2);
        form.set_mode(PagesMode::This);
        form.set_mode(PagesMode::All);
        assert_eq!(ticks(&form), [0, 1, 2, 3, 4]);
        assert_eq!(form.spec().pages, (1..=5).map(PageId).collect::<Vec<_>>());
    }

    #[test]
    fn this_page_ticks_only_the_open_page_and_scrolls_to_it() {
        let mut form = form(5, 3);
        form.center_on = None;
        form.set_mode(PagesMode::This);
        assert_eq!(ticks(&form), [3]);
        assert_eq!(form.center_on, Some(3));
        assert_eq!(form.spec().pages, [PageId(4)]);
    }

    #[test]
    fn one_run_in_the_strip_gives_a_range_with_its_page_numbers() {
        let mut form = form(6, 0);
        form.set_mode(PagesMode::This);
        form.select_run(3, 1, true);
        assert_eq!(form.pages, PagesMode::Range);
        assert_eq!(ticks(&form), [0, 1, 2, 3]);
        assert_eq!(form.range, "1–4");
    }

    #[test]
    fn gaps_in_the_strip_give_runs_and_single_pages() {
        let mut form = form(10, 0);
        form.toggle(4);
        form.toggle(5);
        assert_eq!(form.pages, PagesMode::Range);
        assert_eq!(form.range, "1–4, 7–10");
        form.select_run(0, 9, false);
        for index in [1, 4, 6] {
            form.toggle(index);
        }
        assert_eq!(form.range, "2, 5, 7");
        form.select_run(0, 9, false);
        assert_eq!(form.range, "");
    }

    #[test]
    fn a_typed_range_ticks_its_pages() {
        let mut form = form(10, 0);
        form.set_range("1-4, 7–10");
        assert_eq!(form.pages, PagesMode::Range);
        assert_eq!(ticks(&form), [0, 1, 2, 3, 6, 7, 8, 9]);
        // The text stays as typed while the field has the focus.
        assert_eq!(form.range, "1-4, 7–10");
        form.set_range("2 — 3 5");
        assert_eq!(ticks(&form), [1, 2, 4]);
    }

    #[test]
    fn a_typed_range_reverses_clamps_and_ignores_what_is_not_a_page() {
        let mut form = form(8, 0);
        form.set_range("3-1, 12, x, 0");
        assert_eq!(ticks(&form), [0, 1, 2, 7]);
        form.set_range("x");
        assert_eq!(ticks(&form), Vec::<usize>::new());
        assert!(!form.can_export());
    }

    #[test]
    fn a_range_is_normalized_when_the_field_loses_the_focus() {
        let mut form = form(10, 0);
        form.set_range("9-7,1 2-3, 3");
        form.normalize_range();
        assert_eq!(form.range, "1–3, 7–9");
        form.set_range("abc");
        form.normalize_range();
        assert_eq!(form.range, "");
        // A range of each page stays a range.
        form.set_range("1-10");
        form.normalize_range();
        assert_eq!(form.pages, PagesMode::Range);
        assert_eq!(form.range, "1–10");
    }

    #[test]
    fn a_form_with_no_page_or_no_name_cannot_export() {
        let mut form = form(2, 0);
        assert!(form.can_export());
        form.select_run(0, 1, false);
        assert!(!form.can_export());
        form.set_mode(PagesMode::All);
        form.name = "  ".to_string();
        assert!(!form.can_export());
    }
}
