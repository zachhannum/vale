//! The start screen in the Worlds workspace: the worlds of the folders as a
//! grid of cards, the buttons that make or open a world, and the dialogs
//! that rename or delete a world.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, Sense, pos2, vec2};
use vale_shell::pad::icons::Icon;
use vale_shell::pad::metrics::{PAD, TOUCH};
use vale_shell::pad::theme;
use vale_shell::pad::widgets::{self, BODY_PAD, Controls, HIGHLIGHT_INSET, PANEL_ROW};

use super::{IsothermApp, MARGIN, Pick, Workspace, wrapped};
use crate::worlds::{self, Entry, Location};

pub const WORLDS: &str = "Worlds";
pub const NEW_WORLD: &str = "New world";
pub const NEW_FROM_VALE: &str = "New from Vale project…";
pub const OPEN: &str = "Open…";
pub const RENAME: &str = "Rename…";
pub const DUPLICATE: &str = "Duplicate";
pub const DELETE: &str = "Delete…";
pub const CANCEL: &str = "Cancel";
pub const RENAME_TITLE: &str = "Rename world";
pub const RENAME_BUTTON: &str = "Rename";
pub const NAME: &str = "Name";
pub const DELETE_TITLE: &str = "Delete world";
pub const DELETE_BUTTON: &str = "Delete";
pub const NO_WORLDS: &str = "No worlds yet.";
pub const IN_ICLOUD: &str = "In iCloud";

/// The height of the row with the title.
const HEADER: f32 = 72.0;
/// The smallest width of a card on a screen with more than two columns.
const CARD_MIN: f32 = 200.0;
/// The space between two cards.
const GAP: f32 = 16.0;
/// The space between the edge of a card and its picture.
const CARD_PAD: f32 = 10.0;
/// The height of the part of a card below its picture, with two lines of
/// text and with three.
const INFO: f32 = 60.0;
const INFO_TALL: f32 = 78.0;
const MENU_WIDTH: f32 = 220.0;
const DIALOG_WIDTH: f32 = 400.0;
/// The space at the top of the safe area for the workspace switch.
const SWITCH_ROOM: f32 = 80.0;
/// The width of the label at the left of the field of a dialog.
const LABEL_WIDTH: f32 = 64.0;
/// The fill of a button that deletes, as the red of the system.
const DESTRUCTIVE: Color32 = Color32::from_rgb(235, 72, 64);

/// The name of the card of a world.
pub fn open_name(world: &str) -> String {
    format!("Open {world}")
}

/// The name of the button that opens the menu of the actions of a world.
pub fn actions_name(world: &str) -> String {
    format!("Actions of {world}")
}

/// The text of the dialog that asks before a delete.
pub fn delete_text(world: &str) -> String {
    format!("“{world}” will be deleted.")
}

/// The list of the worlds and the state of the start screen.
#[derive(Default)]
pub struct Start {
    pub entries: Vec<Entry>,
    /// The pictures of the worlds on the GPU.
    textures: HashMap<PathBuf, egui::TextureHandle>,
    /// The world whose menu of actions is open.
    pub menu: Option<PathBuf>,
    /// The control that opened the menu.
    menu_at: Option<Rect>,
    pub dialog: Option<Dialog>,
    /// The field of the dialog takes the focus in the next frame.
    focus: bool,
}

/// A dialog of the start screen.
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    Rename { path: PathBuf, name: String },
    Delete { path: PathBuf, name: String },
}

impl Start {
    /// Puts a new list in place, and drops the pictures that changed.
    pub fn set_entries(&mut self, entries: Vec<Entry>) {
        let old = std::mem::take(&mut self.entries);
        self.textures.retain(|path, _| {
            let was = old.iter().find(|e| &e.path == path);
            let now = entries.iter().find(|e| &e.path == path);
            matches!((was, now), (Some(a), Some(b)) if a.thumbnail == b.thumbnail)
        });
        self.entries = entries;
    }

    /// The picture of a world, made from its thumbnail at the first use.
    fn texture(&mut self, ctx: &egui::Context, entry: &Entry) -> Option<egui::TextureHandle> {
        if let Some(texture) = self.textures.get(&entry.path) {
            return Some(texture.clone());
        }
        let thumb = entry.thumbnail.as_ref()?;
        let size = [thumb.width as usize, thumb.height as usize];
        if thumb.rgba.len() != size[0] * size[1] * 4 {
            return None;
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(size, &thumb.rgba);
        let name = format!("world {}", entry.path.display());
        let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
        self.textures.insert(entry.path.clone(), texture.clone());
        Some(texture)
    }

    /// Opens a dialog. Its field takes the focus.
    fn ask(&mut self, dialog: Dialog) {
        self.dialog = Some(dialog);
        self.focus = true;
        self.menu = None;
    }
}

/// The arrangement of the buttons of the start screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Header {
    /// The buttons in the row of the title.
    Wide,
    /// A narrow screen: one button in the row of the title, and the others
    /// in rows below it.
    Narrow,
    /// Only the title. The side panel of the desktop has the buttons.
    Title,
}

/// What a tap on the start screen does after the frame.
enum Tap {
    Open(PathBuf),
    Download(PathBuf, String),
    Menu(PathBuf, Rect),
    Problem(String),
}

fn title_font() -> egui::FontId {
    theme::font(28.0)
}

fn small_font() -> egui::FontId {
    theme::font(12.0)
}

/// One line of text that ends with "…" if it is wider than `width`.
fn one_line(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    ui.painter().layout_job(job)
}

/// A button in `rect` with a text on a capsule of `fill`.
fn capsule_button(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    enabled: bool,
    (fill, text): (Color32, Color32),
) -> egui::Response {
    let resp = widgets::hit(ui, controls, rect, name, Sense::click(), enabled, None);
    let start = widgets::press_start(ui);
    let color = match (enabled, resp.is_pointer_button_down_on()) {
        (false, _) => theme::raise(),
        (true, true) => fill,
        (true, false) => fill.gamma_multiply(0.85),
    };
    let back = rect.shrink(HIGHLIGHT_INSET);
    ui.painter().rect_filled(back, theme::capsule(back), color);
    let text = widgets::fade(text, enabled);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        name,
        theme::body(),
        text,
    );
    widgets::press_end(ui, &resp, start);
    resp
}

/// A `capsule_button` that fits its text, with its right edge and its
/// middle at `right`.
fn button_at(
    ui: &egui::Ui,
    controls: &mut Controls,
    right: egui::Pos2,
    name: &str,
    enabled: bool,
    colors: (Color32, Color32),
) -> egui::Response {
    let width = widgets::text_width(ui, name, theme::body()) + 2.0 * 18.0;
    let rect = Rect::from_min_size(
        pos2(right.x - width, right.y - TOUCH / 2.0),
        vec2(width, TOUCH),
    );
    capsule_button(ui, controls, rect, name, enabled, colors)
}

/// The colors of a button that is not the main one.
fn plain() -> (Color32, Color32) {
    (theme::raise(), theme::TEXT)
}

/// A field for one line of text in `rect`. `focus`: the field takes the
/// focus, with all of its text selected, so that the first key replaces it.
fn text_field(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    text: &mut String,
    focus: bool,
) -> egui::Response {
    let back = rect.shrink(HIGHLIGHT_INSET);
    ui.painter()
        .rect_filled(back, theme::capsule(back), theme::raise());
    let edit = egui::TextEdit::singleline(text)
        .id(ui.id().with(name))
        .frame(egui::Frame::NONE)
        .font(theme::body())
        .text_color(theme::TEXT)
        .vertical_align(egui::Align::Center);
    let resp = ui.put(rect.shrink2(vec2(14.0, 0.0)), edit);
    let shown = text.clone();
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::text_edit(true, &shown, &shown, "");
        info.label = Some(name.to_owned());
        info
    });
    widgets::push(ui, controls, rect, name);
    if focus {
        use egui::text::{CCursor, CCursorRange};
        resp.request_focus();
        let mut state = egui::TextEdit::load_state(ui.ctx(), resp.id).unwrap_or_default();
        let all = CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()));
        state.cursor.set_char_range(Some(all));
        state.store(ui.ctx(), resp.id);
    }
    resp
}

/// A fill over the whole screen in the foreground that takes each touch.
/// Returns `true` at a tap on the fill.
fn scrim(ctx: &egui::Context, id: &str, alpha: u8) -> bool {
    let screen = ctx.viewport_rect();
    let area = egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .movable(false)
        .constrain(false)
        .fade_in(false);
    area.show(ctx, |ui| {
        let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
        let fill = Color32::from_black_alpha(alpha);
        ui.painter().rect_filled(rect, 0.0, fill);
        resp.clicked()
    })
    .inner
}

impl IsothermApp {
    /// Reads the list of the worlds again if it is stale.
    pub(super) fn refresh_worlds(&mut self) {
        if !self.worlds_stale {
            return;
        }
        self.worlds_stale = false;
        let entries = match &self.folders {
            Some(folders) => folders.list(&*self.files),
            None => Vec::new(),
        };
        self.start.set_entries(entries);
    }

    /// The start screen in `area`: the header, the cards, and the build,
    /// in a scroll area.
    pub(super) fn start_screen(
        &mut self,
        ui: &mut egui::Ui,
        controls: &mut Controls,
        area: Rect,
        header: Header,
    ) {
        self.cards.push(("start", area));
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(layout));
        ui.set_clip_rect(area);
        theme::apply(&mut ui);
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        egui::ScrollArea::vertical()
            .id_salt("start")
            .auto_shrink(false)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut ui, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                self.start_header(ui, controls, header);
                if self.start.entries.is_empty() {
                    ui.add_space(8.0);
                    wrapped(ui, NO_WORLDS, theme::body(), theme::MUTE);
                } else {
                    self.start_grid(ui, controls, header == Header::Narrow);
                }
                if header != Header::Title {
                    ui.add_space(GAP);
                    wrapped(ui, &self.build, theme::hint(), theme::MUTE);
                    ui.add_space(GAP);
                }
            });
    }

    /// The title, the buttons, the choice of the folder, and the status.
    fn start_header(&mut self, ui: &mut egui::Ui, controls: &mut Controls, header: Header) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER), Sense::hover());
        let y = rect.center().y;
        ui.painter().text(
            pos2(rect.left(), y),
            Align2::LEFT_CENTER,
            WORLDS,
            title_font(),
            theme::TEXT,
        );
        match header {
            Header::Title => return,
            Header::Wide => {
                let new = widgets::accent_button(ui, controls, pos2(rect.right(), y), NEW_WORLD);
                let right = pos2(new.rect.left() - 8.0, y);
                let open = button_at(ui, controls, right, OPEN, true, plain());
                let right = pos2(open.rect.left() - 8.0, y);
                let vale = button_at(ui, controls, right, NEW_FROM_VALE, true, plain());
                self.header_taps(new.clicked(), vale.clicked(), open.clicked());
            }
            Header::Narrow => {
                let new = widgets::accent_button(ui, controls, pos2(rect.right(), y), NEW_WORLD);
                let vale = self.plain_row(ui, controls, NEW_FROM_VALE);
                let open = self.plain_row(ui, controls, OPEN);
                self.header_taps(new.clicked(), vale, open);
            }
        }
        self.location_row(ui, controls, header == Header::Narrow);
        self.status_line(ui);
        ui.add_space(8.0);
    }

    /// A row that is one button with a plain fill. Returns `true` at a tap.
    fn plain_row(&mut self, ui: &mut egui::Ui, controls: &mut Controls, name: &str) -> bool {
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
        capsule_button(ui, controls, rect, name, true, plain()).clicked()
    }

    fn header_taps(&mut self, new: bool, vale: bool, open: bool) {
        if new {
            self.new_world();
        } else if vale {
            self.pick = Some(Pick::ValeProject);
        } else if open {
            self.pick = Some(Pick::Open);
        }
    }

    /// The choice of the folder of new worlds, when iCloud Drive is there.
    /// `full`: the control takes the width of the row.
    fn location_row(&mut self, ui: &mut egui::Ui, controls: &mut Controls, full: bool) {
        let Some(folders) = &self.folders else {
            return;
        };
        if folders.cloud().is_none() {
            return;
        }
        let phone = full;
        let labels = Location::ALL.map(|l| l.label(phone));
        let active = Location::ALL
            .iter()
            .position(|&l| l == folders.choice())
            .unwrap_or(0);
        let (row, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
        let width = if full {
            row.width()
        } else {
            widgets::small_segments_width(ui, &labels).min(row.width())
        };
        let height = TOUCH - 8.0;
        let rect = Rect::from_min_size(
            pos2(row.left(), row.center().y - height / 2.0),
            vec2(width, height),
        );
        if let Some(i) = widgets::small_segments(ui, controls, rect, &labels, active)
            && let Some(folders) = &mut self.folders
        {
            folders.set_choice(Location::ALL[i]);
        }
    }

    fn status_line(&self, ui: &mut egui::Ui) {
        if let Some(status) = &self.status {
            ui.add_space(4.0);
            wrapped(ui, status, theme::body(), theme::WARN);
        }
    }

    /// The rows of the side panel of the desktop.
    pub(super) fn start_rows(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        ui.add_space(8.0);
        if widgets::button_row(ui, controls, NEW_WORLD, true).clicked() {
            self.new_world();
        }
        let vale = self.plain_row(ui, controls, NEW_FROM_VALE);
        let open = self.plain_row(ui, controls, OPEN);
        self.header_taps(false, vale, open);
        self.location_row(ui, controls, true);
        self.status_line(ui);
        ui.add_space(12.0);
        wrapped(ui, &self.build, theme::hint(), theme::MUTE);
    }

    /// The cards, in rows of equal columns.
    fn start_grid(&mut self, ui: &mut egui::Ui, controls: &mut Controls, narrow: bool) {
        let width = ui.available_width();
        let columns = match narrow {
            true => 2.0,
            false => ((width + GAP) / (CARD_MIN + GAP)).floor().max(2.0),
        };
        let card_width = (width - (columns - 1.0) * GAP) / columns;
        let cloud = self.folders.as_ref().and_then(|f| f.cloud()).is_some();
        let info = if cloud { INFO_TALL } else { INFO };
        let size = vec2(card_width, card_width + info);
        let entries = std::mem::take(&mut self.start.entries);
        let mut tap = None;
        for row in entries.chunks(columns as usize) {
            let (rect, _) = ui.allocate_exact_size(vec2(width, size.y), Sense::hover());
            for (column, entry) in row.iter().enumerate() {
                let left = rect.left() + column as f32 * (card_width + GAP);
                let rect = Rect::from_min_size(pos2(left, rect.top()), size);
                ui.push_id(&entry.path, |ui| {
                    if let Some(t) = self.world_card(ui, controls, rect, entry, cloud, narrow) {
                        tap = Some(t);
                    }
                });
            }
            ui.add_space(GAP);
        }
        self.start.entries = entries;
        match tap {
            Some(Tap::Open(path)) => self.open_world(&path),
            Some(Tap::Download(path, name)) => {
                self.status = Some(match self.files.start_download(&path) {
                    Ok(()) => format!("Downloading {name}…"),
                    Err(e) => format!("Could not download {name}: {e}."),
                });
            }
            Some(Tap::Menu(path, at)) => {
                self.start.menu = Some(path);
                self.start.menu_at = Some(at);
            }
            Some(Tap::Problem(error)) => self.status = Some(error),
            None => {}
        }
    }

    /// One world: the picture, the name, the date, the folder, and the
    /// button of the actions.
    fn world_card(
        &mut self,
        ui: &mut egui::Ui,
        controls: &mut Controls,
        rect: Rect,
        entry: &Entry,
        cloud: bool,
        phone: bool,
    ) -> Option<Tap> {
        let shown = rect.intersect(ui.clip_rect());
        if shown.is_positive() {
            self.cards.push(("world", shown));
        }
        let open = widgets::hit(
            ui,
            controls,
            rect,
            &open_name(&entry.name),
            Sense::click(),
            true,
            None,
        );
        let painter = ui.painter().clone();
        let radius = theme::TILE_RADIUS;
        painter.rect_filled(rect, radius, theme::card().to_opaque());
        if open.is_pointer_button_down_on() {
            painter.rect_filled(rect, radius, theme::raise());
        }
        if self.doc.path.as_ref() == Some(&entry.path) {
            let stroke = egui::Stroke::new(2.0, theme::ACCENT);
            painter.rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
        }

        let picture =
            Rect::from_min_size(rect.min, vec2(rect.width(), rect.width())).shrink(CARD_PAD);
        let inner = theme::inner_radius(radius, CARD_PAD);
        painter.rect_filled(picture, inner, theme::track());
        match self.start.texture(ui.ctx(), entry) {
            Some(texture) => {
                let image = egui::Image::new(&texture).corner_radius(inner);
                image.paint_at(ui, picture);
            }
            // A world with no picture yet shows a plain globe.
            None => {
                let radius = 0.4 * picture.width();
                painter.circle_filled(picture.center(), radius, theme::GROUND);
            }
        }

        let info = Rect::from_min_max(
            pos2(rect.left() + BODY_PAD, picture.bottom()),
            pos2(rect.right() - PAD, rect.bottom()),
        );
        let button = Rect::from_min_size(
            pos2(info.right() - TOUCH, info.top() + 6.0),
            vec2(TOUCH, TOUCH),
        );
        let text_width = button.left() - info.left() - 4.0;
        let name = one_line(ui, &entry.name, theme::title(), theme::TEXT, text_width);
        painter.galley(pos2(info.left(), info.top() + 10.0), name, theme::TEXT);
        let mut lines: Vec<(String, Color32)> = Vec::new();
        match &entry.error {
            Some(error) => lines.push((error.clone(), theme::WARN)),
            None => {
                let date = entry.modified.map(worlds::date_text).unwrap_or_default();
                lines.push((date, theme::MUTE));
            }
        }
        let on_device = entry.availability.on_device();
        if !on_device {
            lines.push((IN_ICLOUD.to_owned(), theme::MUTE));
        } else if cloud {
            lines.push((entry.location.label(phone).to_owned(), theme::MUTE));
        }
        for (i, (text, color)) in lines.iter().enumerate() {
            let line = one_line(ui, text, small_font(), *color, text_width);
            let at = pos2(info.left(), info.top() + 34.0 + 18.0 * i as f32);
            painter.galley(at, line, *color);
        }

        let menu_open = self.start.menu.as_ref() == Some(&entry.path);
        let name = actions_name(&entry.name);
        let actions =
            widgets::icon_button_at(ui, controls, button, &name, Icon::More, menu_open, true);
        if actions.clicked() || actions.secondary_clicked() || open.secondary_clicked() {
            Some(Tap::Menu(entry.path.clone(), button))
        } else if open.clicked() {
            Some(match &entry.error {
                Some(error) => Tap::Problem(format!("Could not open {}: {error}.", entry.name)),
                None if !on_device => Tap::Download(entry.path.clone(), entry.name.clone()),
                None => Tap::Open(entry.path.clone()),
            })
        } else {
            None
        }
    }

    /// The menu of the actions of one world, below its button.
    pub(super) fn start_menu(&mut self, ctx: &egui::Context, controls: &mut Controls, safe: Rect) {
        let Some(path) = self.start.menu.clone() else {
            return;
        };
        let Some(entry) = self.start.entries.iter().find(|e| e.path == path).cloned() else {
            self.start.menu = None;
            return;
        };
        let button = self
            .start
            .menu_at
            .unwrap_or(Rect::from_center_size(safe.center(), vec2(TOUCH, TOUCH)));
        let readable = entry.error.is_none() && entry.availability.on_device();
        let rows = if readable { 3.0 } else { 1.0 };
        let size = vec2(MENU_WIDTH, 2.0 * widgets::menu_inset().y + rows * PANEL_ROW);
        // The menu opens below the button, or above it near the bottom.
        let lowest = safe.bottom() - MARGIN - size.y;
        let top = if button.bottom() + 4.0 <= lowest {
            button.bottom() + 4.0
        } else {
            (button.top() - 4.0 - size.y).max(safe.top() + MARGIN)
        };
        let left = (button.right() - size.x)
            .max(safe.left() + MARGIN)
            .min(safe.right() - MARGIN - size.x);
        let rect = Rect::from_min_size(pos2(left, top), size);
        let mut open = !scrim(ctx, "start-menu-scrim", 0);
        // The menu covers each other control.
        controls.clear();
        self.cards.push(("start-menu", rect));
        let origin = button.center().clamp(rect.min, rect.max);
        let mut chosen = None;
        let backdrop = widgets::backdrop(ctx);
        widgets::menu_card(ctx, backdrop, "start-menu", rect, origin, |ui| {
            let items: &[&str] = if readable {
                &[RENAME, DUPLICATE, DELETE]
            } else {
                &[DELETE]
            };
            for &item in items {
                if widgets::text_row(ui, controls, item, 0.0, false, true).clicked() {
                    chosen = Some(item);
                }
            }
        });
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("start-menu"));
        ctx.move_to_top(layer);
        let (path, name) = (entry.path, entry.name);
        match chosen {
            Some(RENAME) => self.start.ask(Dialog::Rename { path, name }),
            Some(DUPLICATE) => {
                self.duplicate_world(&path);
                open = false;
            }
            Some(DELETE) => self.start.ask(Dialog::Delete { path, name }),
            _ => {}
        }
        if !open || chosen.is_some() {
            self.start.menu = None;
        }
    }

    /// The dialog that asks for a new name, or that asks before a delete.
    /// It is in the upper third of the safe area, above the keyboard.
    pub(super) fn start_dialog(
        &mut self,
        ctx: &egui::Context,
        controls: &mut Controls,
        safe: Rect,
    ) {
        let Some(mut dialog) = self.start.dialog.take() else {
            return;
        };
        let focus = std::mem::take(&mut self.start.focus);
        let width = DIALOG_WIDTH.min(safe.width() - 2.0 * MARGIN);
        let text_width = width - 2.0 * BODY_PAD;
        let title_height = 24.0;
        let body = match &dialog {
            Dialog::Rename { .. } => TOUCH,
            Dialog::Delete { name, .. } => {
                let galley = ctx.fonts_mut(|f| {
                    f.layout(delete_text(name), theme::body(), theme::MUTE, text_width)
                });
                galley.size().y
            }
        };
        let height = 20.0 + title_height + 12.0 + body + 20.0 + TOUCH + 14.0;
        // The dialog stays below the workspace switch.
        let top = safe.top() + ((safe.height() / 3.0 - height) / 2.0).max(SWITCH_ROOM);
        let rect = Rect::from_min_size(
            pos2(safe.center().x - width / 2.0, top),
            vec2(width, height),
        );
        let mut close = scrim(ctx, "start-dialog-scrim", 120);
        // The dialog covers each other control.
        controls.clear();
        self.cards.push(("start-dialog", rect));
        let mut confirm = false;
        let radius = CornerRadius::same(theme::CARD_RADIUS as u8);
        let backdrop = widgets::backdrop(ctx);
        let order = egui::Order::Foreground;
        widgets::card(ctx, backdrop, "start-dialog", rect, order, radius, |ui| {
            let left = rect.left() + BODY_PAD;
            let top = rect.top() + 20.0;
            let title = match &dialog {
                Dialog::Rename { .. } => RENAME_TITLE,
                Dialog::Delete { .. } => DELETE_TITLE,
            };
            ui.painter().text(
                pos2(left, top + title_height / 2.0),
                Align2::LEFT_CENTER,
                title,
                theme::heading(ctx),
                theme::TEXT,
            );
            let body_top = top + title_height + 12.0;
            let y = rect.bottom() - 14.0 - TOUCH / 2.0;
            let right = pos2(rect.right() - BODY_PAD + HIGHLIGHT_INSET, y);
            let (action, enabled, colors) = match &mut dialog {
                Dialog::Rename { name, .. } => {
                    ui.painter().text(
                        pos2(left, body_top + TOUCH / 2.0),
                        Align2::LEFT_CENTER,
                        NAME,
                        theme::body(),
                        theme::MUTE,
                    );
                    let field = Rect::from_min_max(
                        pos2(left + LABEL_WIDTH, body_top),
                        pos2(rect.right() - BODY_PAD, body_top + TOUCH),
                    );
                    let resp = text_field(ui, controls, field, NAME, name, focus);
                    let valid = !name.trim().is_empty();
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    confirm |= valid && enter;
                    (RENAME_BUTTON, valid, (theme::ACCENT, theme::CANVAS))
                }
                Dialog::Delete { name, .. } => {
                    let text = egui::RichText::new(delete_text(name))
                        .font(theme::body())
                        .color(theme::MUTE);
                    let area = Rect::from_min_size(pos2(left, body_top), vec2(text_width, body));
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(area)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    child.add(egui::Label::new(text).wrap().selectable(false));
                    (DELETE_BUTTON, true, (DESTRUCTIVE, theme::TEXT))
                }
            };
            let main = button_at(ui, controls, right, action, enabled, colors);
            confirm |= main.clicked();
            let right = pos2(main.rect.left() - 8.0, y);
            close |= button_at(ui, controls, right, CANCEL, true, plain()).clicked();
        });
        ctx.move_to_top(egui::LayerId::new(order, egui::Id::new("start-dialog")));
        // The Escape key cancels, as on macOS.
        close |= ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if confirm {
            match &dialog {
                Dialog::Rename { path, name } => self.rename_world(path, name.trim()),
                Dialog::Delete { path, .. } => self.delete_world(path),
            }
        } else if !close {
            self.start.dialog = Some(dialog);
        }
    }

    /// The start screen is on the screen.
    pub(super) fn on_start(&self) -> bool {
        self.workspace == Workspace::Worlds
    }
}
