//! The screen of the app: the globe, the workspace switch, the panel of
//! each workspace, and the view card.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, CornerRadius, Rect, Sense, pos2, vec2};
use isotherm_import::vale_project::HeightLayer;
use isotherm_import::{Samples, Scale};
use isotherm_world::Terrain;
use vale_shell::pad::icons::{self, Icon};
use vale_shell::pad::metrics::{PAD, TOUCH, WidthClass};
use vale_shell::pad::number::{slider_row, stepper_row};
use vale_shell::pad::theme;
use vale_shell::pad::widgets::{self, Controls, PANEL_ROW};
use vale_sphere::{ProjectionKind, ProjectionSpec};

use crate::cli::Args;
use crate::document::{self, Change, Document, Read};
use crate::globe::{Globe, WorldView};
use crate::worlds;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Workspace {
    Worlds,
    Terrain,
    Planet,
    Simulate,
    Explore,
    Export,
}

impl Workspace {
    pub const ALL: [Workspace; 6] = [
        Workspace::Worlds,
        Workspace::Terrain,
        Workspace::Planet,
        Workspace::Simulate,
        Workspace::Explore,
        Workspace::Export,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Workspace::Worlds => "Worlds",
            Workspace::Terrain => "Terrain",
            Workspace::Planet => "Planet",
            Workspace::Simulate => "Simulate",
            Workspace::Explore => "Explore",
            Workspace::Export => "Export",
        }
    }

    /// The empty state of a workspace with no content yet.
    pub fn hint(self) -> Option<&'static str> {
        match self {
            Workspace::Worlds => Some("Saved worlds come in a later version."),
            Workspace::Planet => Some("The planet settings come in a later version."),
            Workspace::Simulate => Some("The climate model comes in a later version."),
            Workspace::Export => Some("Export comes in a later version."),
            Workspace::Terrain | Workspace::Explore => None,
        }
    }

    /// The workspace has the view card.
    pub fn has_view(self) -> bool {
        matches!(self, Workspace::Terrain | Workspace::Explore)
    }
}

/// The arrangement of the controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Layout {
    /// A top bar and a side panel.
    Desktop,
    /// The cards of the iPad and the iPhone.
    Pad,
}

impl Layout {
    /// The layout of this platform: pad on iOS, desktop elsewhere.
    pub fn of_platform() -> Layout {
        if cfg!(target_os = "ios") {
            Layout::Pad
        } else {
            Layout::Desktop
        }
    }
}

/// The step of an import that waits for the user.
pub enum Pending {
    /// An image with no units, and the heights of black and white.
    Scale {
        name: String,
        samples: Samples,
        black_m: f64,
        white_m: f64,
    },
    /// A Vale project with more than one height layer.
    Layers {
        path: PathBuf,
        name: String,
        layers: Vec<HeightLayer>,
    },
}

/// The name of the button of the workspace menu on a compact screen.
pub const WORKSPACE_MENU: &str = "Workspace";
pub const IMPORT: &str = "Import heightmap…";
pub const SEA_LEVEL: &str = "Sea level";
pub const BLACK: &str = "Black";
pub const WHITE: &str = "White";
pub const UNDO: &str = "Undo";
pub const REDO: &str = "Redo";
pub const PROJECTION: &str = "Projection";
pub const SCALE_HINT: &str = "Set the heights of black and white.";
pub const LAYERS_HINT: &str = "Choose a height layer.";

/// The file types of an import.
pub const IMPORT_TYPES: [&str; 4] = ["png", "tif", "tiff", "gpkg"];

/// The space between a card and the edge of the safe area.
const MARGIN: f32 = 16.0;
/// The space between the top of the safe area and the top row, and between
/// the top of a screen with no safe area and the top row.
const TOP_GAP: f32 = 4.0;
const TOP_MARGIN: f32 = 12.0;
/// The width of the workspace menu.
const MENU_WIDTH: f32 = 240.0;
/// The widths of the panel card and the view card.
const PANEL_WIDTH: f32 = 360.0;
const VIEW_WIDTH: f32 = 260.0;
/// The width of the list of the projections.
const LIST_WIDTH: f32 = 280.0;
/// The space above and below the rows of a card.
const CARD_PAD: f32 = 8.0;
/// The corners of the top of a bottom sheet.
const SHEET_RADIUS: u8 = 40;
/// The height of the top bar and the width of the side panel of the
/// desktop layout.
const BAR: f32 = 56.0;
const SIDE: f32 = 320.0;
/// The range of the heights of black and white, in meters.
const SCALE_RANGE: f64 = 50_000.0;
const SCALE_STEP: f64 = 100.0;

pub struct IsothermApp {
    pub workspace: Workspace,
    pub layout: Layout,
    /// The workspace menu of a compact screen is open.
    pub menu: bool,
    /// The list of the projections is open.
    pub projections: bool,
    pub doc: Document,
    pub globe: Globe,
    /// The step of an import that waits for the user.
    pub pending: Option<Pending>,
    /// The last error, as one short sentence.
    pub status: Option<String>,
    /// The sea level at the start of a drag of the slider.
    sea_start: Option<f64>,
    /// The folder of the world files. `None`: an import can only replace
    /// the terrain of an open world file.
    pub data_dir: Option<PathBuf>,
    /// The user asked for the file picker. The app shows it after the
    /// frame.
    pub pick: bool,
    /// The controls of the last frame.
    pub controls: Controls,
    /// The cards of the last frame.
    pub cards: Vec<(&'static str, Rect)>,
    build: String,
    /// The safe area of a screenshot. `None`: the safe area of the system.
    pub safe_area: Option<egui::SafeAreaInsets>,
    #[cfg(target_os = "ios")]
    pub picker: Option<vale_shell::picker::Picker>,
    #[cfg(target_os = "ios")]
    pub picked: vale_shell::picker::PickQueue,
}

/// Sets the style and the fonts of the app.
pub fn setup(ctx: &egui::Context) {
    theme::add_fonts(ctx);
    ctx.all_styles_mut(|style| style.visuals = egui::Visuals::dark());
}

/// A height as the app shows it, for example "−1,200 m".
pub fn meters_text(m: f64) -> String {
    let n = m.round() as i64;
    let digits = n.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if n < 0 { "−" } else { "" };
    format!("{sign}{grouped} m")
}

impl IsothermApp {
    /// The app with `doc` open.
    pub fn new(doc: Document, layout: Layout) -> IsothermApp {
        let globe = Globe::new(&doc.world.terrain.heights);
        IsothermApp {
            workspace: Workspace::Explore,
            layout,
            menu: false,
            projections: false,
            doc,
            globe,
            pending: None,
            status: None,
            sea_start: None,
            data_dir: None,
            pick: false,
            controls: Controls::new(),
            cards: Vec::new(),
            build: crate::build::label(),
            safe_area: None,
            #[cfg(target_os = "ios")]
            picker: None,
            #[cfg(target_os = "ios")]
            picked: Default::default(),
        }
    }

    /// The app with the sample world.
    pub fn sample(layout: Layout) -> Result<IsothermApp, String> {
        Ok(IsothermApp::new(Document::sample()?, layout))
    }

    /// The app that the arguments ask for. With no `--project`, the app
    /// opens the last world in `resume`, or else the sample world.
    pub fn from_args(args: &Args, resume: Option<&Path>) -> Result<IsothermApp, String> {
        let last = resume.and_then(worlds::last_world);
        let doc = match (&args.project, last) {
            (Some(path), _) => Document::open(path)?,
            (None, Some(path)) => Document::open(&path).or_else(|_| Document::sample())?,
            (None, None) => Document::sample()?,
        };
        let mut app = IsothermApp::new(doc, args.layout());
        app.workspace = args.workspace;
        app.globe.world_view = args.view;
        app.globe.flat.set_spec(args.flat_spec());
        app.safe_area = args.safe_area;
        Ok(app)
    }

    /// Reads an import file. A heightmap in meters and a Vale project with
    /// one height layer go into the world at once. Else the panel asks
    /// for the rest.
    pub fn import_path(&mut self, path: &Path) {
        self.status = None;
        self.pending = None;
        match document::read_import(path) {
            Ok(Read::Terrain { name, terrain }) => self.finish_import(&name, *terrain),
            Ok(Read::Scale { name, samples }) => {
                let scale = Scale::default();
                self.pending = Some(Pending::Scale {
                    name,
                    samples,
                    black_m: scale.black_m,
                    white_m: scale.white_m,
                });
            }
            Ok(Read::Layers { path, name, layers }) => {
                self.pending = Some(Pending::Layers { path, name, layers });
            }
            Err(e) => self.status = Some(e),
        }
    }

    /// Puts an imported terrain in the world, and shows it.
    fn finish_import(&mut self, name: &str, terrain: Terrain) {
        self.pending = None;
        match self.doc.import(name, terrain, self.data_dir.as_deref()) {
            Ok(()) => {
                self.globe.set_terrain(&self.doc.world.terrain.heights);
                self.remember();
            }
            Err(e) => self.status = Some(e),
        }
    }

    /// Keeps the open world file as the world of the next start.
    fn remember(&self) {
        if let (Some(dir), Some(path)) = (&self.data_dir, &self.doc.path) {
            // The next start opens the sample world if this fails.
            let _ = worlds::remember(dir, path);
        }
    }

    pub fn undo(&mut self) {
        let change = self.doc.undo();
        self.apply(change);
    }

    pub fn redo(&mut self) {
        let change = self.doc.redo();
        self.apply(change);
    }

    fn apply(&mut self, change: Result<Option<Change>, String>) {
        match change {
            Ok(Some(Change::Terrain)) => self.globe.set_terrain(&self.doc.world.terrain.heights),
            Ok(_) => {}
            Err(e) => self.status = Some(e),
        }
    }

    /// Draws one frame.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let screen = ctx.viewport_rect();
        let safe = match self.safe_area {
            Some(insets) => screen - insets.0,
            None => ctx.content_rect(),
        };
        let mut controls = Controls::new();
        self.cards.clear();
        // The cards are glass when the app has a wgpu renderer.
        widgets::set_backdrop(&ctx, self.globe.backdrop());
        match self.layout {
            Layout::Desktop => self.draw_desktop(ui, screen, safe, &mut controls),
            Layout::Pad => self.draw_pad(ui, screen, safe, &mut controls),
        }
        if self.projections && self.globe.world_view == WorldView::Flat && self.workspace.has_view()
        {
            self.projection_list(&ctx, &mut controls, safe);
        } else {
            self.projections = false;
        }
        self.controls = controls;
    }

    /// The globe in `rect`, with the canvas below it.
    fn canvas(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let bands = self.doc.world.terrain.bands();
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect);
            ui.painter().rect_filled(rect, 0.0, theme::CANVAS);
            self.globe.show(ui, rect, theme::CANVAS, &bands);
        });
    }

    /// The desktop layout: a top bar with the workspace switch, a side panel
    /// at the left, and the globe in the rest of the window.
    fn draw_desktop(
        &mut self,
        ui: &mut egui::Ui,
        screen: Rect,
        safe: Rect,
        controls: &mut Controls,
    ) {
        let bar = Rect::from_min_max(screen.min, pos2(screen.right(), safe.top() + BAR));
        let side = Rect::from_min_max(
            pos2(screen.left(), bar.bottom()),
            pos2(safe.left() + SIDE, screen.bottom()),
        );
        let globe = Rect::from_min_max(pos2(side.right(), bar.bottom()), screen.max);
        self.canvas(ui, globe);
        let painter = ui.painter().clone();
        // The panels are opaque, with the color of a card.
        let [r, g, b, _] = theme::card().to_srgba_unmultiplied();
        let fill = egui::Color32::from_rgb(r, g, b);
        painter.rect_filled(bar, 0.0, fill);
        painter.rect_filled(side, 0.0, fill);
        let line = egui::Stroke::new(1.0, theme::divider());
        painter.hline(bar.x_range(), bar.bottom(), line);
        painter.vline(side.right(), side.y_range(), line);

        let names = Workspace::ALL.map(Workspace::name);
        let width = widgets::segments_width(ui, &names);
        let row = TOUCH + 2.0 * PAD;
        let switch = Rect::from_center_size(
            pos2(safe.center().x, safe.top() + BAR / 2.0),
            vec2(width, row),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(switch));
        self.switch_segments(&mut child, controls);
        self.cards.push(("workspace-switch", switch));

        let body = Rect::from_min_max(
            pos2(side.left() + 8.0, side.top() + CARD_PAD),
            pos2(side.right() - 8.0, safe.bottom() - CARD_PAD),
        );
        self.cards.push(("panel", body));
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(layout));
        child.set_clip_rect(body);
        self.scroll_body(
            &mut child,
            "desktop-panel",
            controls,
            |app, ui, controls| {
                app.panel_rows(ui, controls, true);
            },
        );
    }

    /// The pad layout: the globe fills the screen, and the cards float over
    /// it in the safe area.
    fn draw_pad(&mut self, ui: &mut egui::Ui, screen: Rect, safe: Rect, controls: &mut Controls) {
        let ctx = ui.ctx().clone();
        let class = WidthClass::of(screen.width());
        let top = safe.top()
            + if safe.top() > screen.top() {
                TOP_GAP
            } else {
                TOP_MARGIN
            };
        // The canvas and the globe fill the screen, also the bands outside
        // the safe area, and the glass of the cards shows them below the
        // cards. The cards stay in the safe area. A layer painter clips to
        // the safe area, so the canvas paints in this ui, which clips to
        // the screen.
        self.canvas(ui, screen);

        let switch = self.switch_rect(ui, safe, class, top);
        let backdrop = self.globe.backdrop();
        let current = self.workspace;
        self.cards.push(("workspace-switch", switch));
        widgets::card(
            &ctx,
            backdrop,
            "workspace-switch",
            switch,
            egui::Order::Middle,
            CornerRadius::same(255),
            |ui| {
                if class == WidthClass::Compact {
                    let open = self.menu;
                    let menu =
                        widgets::menu_button(ui, controls, WORKSPACE_MENU, current.name(), open);
                    if menu.clicked() {
                        self.menu = !open;
                    }
                    return;
                }
                self.switch_segments(ui, controls);
            },
        );
        let below = switch.bottom() + 12.0;
        let room = Rect::from_min_max(
            pos2(safe.left() + MARGIN, below),
            pos2(safe.right() - MARGIN, safe.bottom() - MARGIN),
        );
        if class == WidthClass::Compact {
            if self.workspace.has_view() {
                self.sheet(&ctx, controls, safe);
            }
        } else {
            if self.workspace.has_view() {
                let height = self.measured(&ctx, "view-card", 2.0 * PANEL_ROW);
                let rect = Rect::from_min_size(
                    pos2(room.right() - VIEW_WIDTH, room.top()),
                    vec2(VIEW_WIDTH, height.min(room.height())),
                );
                self.floating_card(&ctx, controls, "view-card", rect, |app, ui, controls| {
                    app.view_rows(ui, controls);
                });
            }
            if self.workspace == Workspace::Terrain {
                let width = PANEL_WIDTH.min(room.width() - VIEW_WIDTH - MARGIN);
                let height = self.measured(&ctx, "panel", 3.0 * PANEL_ROW);
                let rect = Rect::from_min_size(room.min, vec2(width, height.min(room.height())));
                self.floating_card(&ctx, controls, "panel", rect, |app, ui, controls| {
                    app.terrain_rows(ui, controls);
                });
            }
        }
        if class == WidthClass::Compact && self.menu {
            self.workspace_menu(&ctx, controls, switch);
        } else {
            self.menu = false;
        }
        self.hint_card(ui, safe);
    }

    /// The segmented list of the workspaces, in the max rect of `ui`.
    fn switch_segments(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        let items: Vec<(&str, bool)> = Workspace::ALL.iter().map(|w| (w.name(), true)).collect();
        let active = Workspace::ALL.iter().position(|&w| w == self.workspace);
        if let Some(index) = widgets::segments(ui, controls, &items, active.unwrap_or(0)) {
            self.workspace = Workspace::ALL[index];
        }
    }

    /// The height of the rows of a card in the last frame, with the space
    /// above and below them.
    fn measured(&self, ctx: &egui::Context, id: &str, default: f32) -> f32 {
        let id = egui::Id::new(id).with("height");
        ctx.data(|d| d.get_temp::<f32>(id)).unwrap_or(default) + 2.0 * CARD_PAD
    }

    /// The rows of a card in a scroll area. Keeps their height for the
    /// next frame.
    fn scroll_body(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        controls: &mut Controls,
        rows: impl FnOnce(&mut IsothermApp, &mut egui::Ui, &mut Controls),
    ) {
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let width = ui.available_width();
        let out = egui::ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(8, 0))
                    .show(ui, |ui| {
                        ui.set_width(width - 16.0);
                        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                        rows(self, ui, controls);
                    });
            });
        let height = out.content_size.y;
        let key = egui::Id::new(id).with("height");
        let ctx = ui.ctx();
        let old = ctx.data(|d| d.get_temp::<f32>(key));
        if old != Some(height) {
            ctx.data_mut(|d| d.insert_temp(key, height));
            ctx.request_repaint();
        }
    }

    /// A card with rows, in `rect`.
    fn floating_card(
        &mut self,
        ctx: &egui::Context,
        controls: &mut Controls,
        id: &'static str,
        rect: Rect,
        rows: impl FnOnce(&mut IsothermApp, &mut egui::Ui, &mut Controls),
    ) {
        self.cards.push((id, rect));
        let radius = CornerRadius::same(theme::CARD_RADIUS as u8);
        let backdrop = self.globe.backdrop();
        widgets::card(ctx, backdrop, id, rect, egui::Order::Middle, radius, |ui| {
            let body = rect.shrink2(vec2(8.0, CARD_PAD));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(layout));
            self.scroll_body(&mut child, id, controls, rows);
        });
    }

    /// The bottom sheet of a compact screen, with the panel and the view
    /// rows. It is at most half as high as the safe area.
    fn sheet(&mut self, ctx: &egui::Context, controls: &mut Controls, safe: Rect) {
        let id = "sheet";
        let grabber = 16.0;
        let height = (self.measured(ctx, id, 4.0 * PANEL_ROW) + grabber).min(safe.height() / 2.0);
        let rect = Rect::from_min_max(
            pos2(safe.left() + 8.0, safe.bottom() - 8.0 - height),
            pos2(safe.right() - 8.0, safe.bottom() - 8.0),
        );
        self.cards.push((id, rect));
        let radius = CornerRadius {
            nw: SHEET_RADIUS,
            ne: SHEET_RADIUS,
            sw: theme::CARD_RADIUS as u8,
            se: theme::CARD_RADIUS as u8,
        };
        let backdrop = self.globe.backdrop();
        widgets::card(ctx, backdrop, id, rect, egui::Order::Middle, radius, |ui| {
            let handle = Rect::from_center_size(
                pos2(rect.center().x, rect.top() + grabber / 2.0 + 2.0),
                vec2(36.0, 5.0),
            );
            ui.painter().rect_filled(handle, 2.5, theme::divider());
            let body = Rect::from_min_max(
                pos2(rect.left() + 8.0, rect.top() + grabber + CARD_PAD),
                pos2(rect.right() - 8.0, rect.bottom() - CARD_PAD),
            );
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(layout));
            self.scroll_body(&mut child, id, controls, |app, ui, controls| {
                app.panel_rows(ui, controls, false);
            });
        });
    }

    /// The rows of the panel of the workspace, with the view rows below
    /// them. `hint`: a workspace with no content shows its empty state.
    fn panel_rows(&mut self, ui: &mut egui::Ui, controls: &mut Controls, hint: bool) {
        if self.workspace == Workspace::Terrain {
            self.terrain_rows(ui, controls);
            ui.add_space(12.0);
        }
        if self.workspace.has_view() {
            self.view_rows(ui, controls);
        }
        if hint && let Some(text) = self.workspace.hint() {
            ui.add_space(8.0);
            wrapped(ui, text, theme::body(), theme::TEXT);
            if self.workspace == Workspace::Worlds {
                ui.add_space(4.0);
                wrapped(ui, &self.build, theme::hint(), theme::MUTE);
            }
        }
    }

    /// The rows of the Terrain panel: the world and its undo buttons, the
    /// import, and the sea level.
    fn terrain_rows(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        self.title_row(ui, controls);
        match self.pending.take() {
            Some(Pending::Scale {
                name,
                samples,
                black_m,
                white_m,
            }) => self.scale_rows(ui, controls, name, samples, black_m, white_m),
            Some(Pending::Layers { path, name, layers }) => {
                self.layer_rows(ui, controls, path, name, layers)
            }
            None => {
                if widgets::button_row(ui, controls, IMPORT, true).clicked() {
                    self.pick = true;
                }
                self.sea_row(ui, controls);
            }
        }
        if let Some(status) = &self.status {
            ui.add_space(4.0);
            wrapped(ui, status, theme::hint(), theme::WARN);
            ui.add_space(4.0);
        }
    }

    /// The name of the world, and the Undo and Redo buttons.
    fn title_row(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        let (row, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
        let redo = Rect::from_min_size(
            pos2(row.right() - TOUCH, row.center().y - TOUCH / 2.0),
            vec2(TOUCH, TOUCH),
        );
        let undo = redo.translate(vec2(-TOUCH - 4.0, 0.0));
        let name = Rect::from_min_max(row.min, pos2(undo.left() - 8.0, row.bottom()));
        let label = egui::RichText::new(&self.doc.world.name)
            .font(theme::title())
            .color(theme::TEXT);
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(name).layout(layout));
        child.add(egui::Label::new(label).truncate());
        let busy = self.pending.is_some();
        let can_undo = self.doc.can_undo() && !busy;
        let can_redo = self.doc.can_redo() && !busy;
        if widgets::icon_button_at(ui, controls, undo, UNDO, Icon::Undo, false, can_undo).clicked()
        {
            self.undo();
        }
        if widgets::icon_button_at(ui, controls, redo, REDO, Icon::Redo, false, can_redo).clicked()
        {
            self.redo();
        }
    }

    /// The sea level slider. A drag moves the coast on the globe, and the
    /// end of the drag is one undo step.
    fn sea_row(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        let terrain = &self.doc.world.terrain;
        let before = terrain.sea_level_m;
        let range = terrain.low_m..=terrain.high_m;
        let mut value = before;
        let text = meters_text(before);
        let resp = slider_row(ui, controls, SEA_LEVEL, &mut value, range, &text);
        if resp.started {
            self.sea_start = Some(before);
        }
        if resp.changed {
            let terrain = &mut self.doc.world.terrain;
            terrain.sea_level_m = terrain.clamp_sea(value);
        }
        if resp.released
            && let Some(old) = self.sea_start.take()
            && let Err(e) = self.doc.save_sea_level(old)
        {
            self.status = Some(e);
        }
    }

    /// The heights of black and white of an image with no units.
    fn scale_rows(
        &mut self,
        ui: &mut egui::Ui,
        controls: &mut Controls,
        name: String,
        samples: Samples,
        mut black_m: f64,
        mut white_m: f64,
    ) {
        wrapped(ui, SCALE_HINT, theme::hint(), theme::MUTE);
        ui.add_space(4.0);
        let text = meters_text(black_m);
        let range = -SCALE_RANGE..=white_m - SCALE_STEP;
        stepper_row(ui, controls, BLACK, &mut black_m, SCALE_STEP, range, &text);
        let text = meters_text(white_m);
        let range = black_m + SCALE_STEP..=SCALE_RANGE;
        stepper_row(ui, controls, WHITE, &mut white_m, SCALE_STEP, range, &text);
        let (row, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
        let right = pos2(row.right(), row.center().y);
        let import = widgets::accent_button(ui, controls, right, "Import");
        let width = widgets::text_width(ui, "Cancel", theme::body()) + 36.0;
        let cancel_rect = Rect::from_min_size(
            pos2(
                import.rect.left() - 8.0 - width,
                row.center().y - TOUCH / 2.0,
            ),
            vec2(width, TOUCH),
        );
        let cancel = widgets::hit(
            ui,
            controls,
            cancel_rect,
            "Cancel",
            Sense::click(),
            true,
            None,
        );
        widgets::fill(ui, cancel_rect, &cancel, false);
        ui.painter().text(
            cancel_rect.center(),
            Align2::CENTER_CENTER,
            "Cancel",
            theme::body(),
            theme::TEXT,
        );
        if cancel.clicked() {
            return;
        }
        if import.clicked() {
            let scale = Scale { black_m, white_m };
            match document::scale_terrain(&samples, scale) {
                Ok(terrain) => self.finish_import(&name, terrain),
                Err(e) => self.status = Some(e),
            }
            return;
        }
        self.pending = Some(Pending::Scale {
            name,
            samples,
            black_m,
            white_m,
        });
    }

    /// The height layers of a Vale project. A tap on a layer imports it.
    fn layer_rows(
        &mut self,
        ui: &mut egui::Ui,
        controls: &mut Controls,
        path: PathBuf,
        name: String,
        layers: Vec<HeightLayer>,
    ) {
        wrapped(ui, LAYERS_HINT, theme::hint(), theme::MUTE);
        ui.add_space(4.0);
        let mut chosen = None;
        for layer in &layers {
            let id = format!("Layer {}", layer.name);
            if widgets::text_row_as(ui, controls, &id, &layer.name, 0.0, false, true).clicked() {
                chosen = Some(layer.id);
            }
        }
        if widgets::text_row(ui, controls, "Cancel", 0.0, false, true).clicked() {
            return;
        }
        if let Some(id) = chosen {
            match document::layer_terrain(&path, id) {
                Ok(terrain) => self.finish_import(&name, terrain),
                Err(e) => self.status = Some(e),
            }
            return;
        }
        self.pending = Some(Pending::Layers { path, name, layers });
    }

    /// The Globe and Flat switch, and in the flat view, the projection.
    fn view_rows(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        let views = [WorldView::Globe, WorldView::Flat];
        let items: Vec<(&str, bool)> = views.iter().map(|v| (v.name(), true)).collect();
        let (row, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), TOUCH + 2.0 * PAD),
            Sense::hover(),
        );
        let track = row;
        ui.painter()
            .rect_filled(track, theme::capsule(track), theme::track());
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(track));
        let active = views.iter().position(|&v| v == self.globe.world_view);
        if let Some(i) = segments_fill(&mut child, controls, &items, active.unwrap_or(0)) {
            self.globe.world_view = views[i];
        }
        if self.globe.world_view != WorldView::Flat {
            return;
        }
        ui.add_space(4.0);
        let kind = self.globe.flat.spec().kind;
        let open = self.projections;
        let resp = widgets::text_row(ui, controls, PROJECTION, 0.0, open, true);
        let rect = resp.rect;
        let chevron =
            Rect::from_center_size(pos2(rect.right() - 9.0, rect.center().y), vec2(18.0, 18.0));
        icons::paint(ui.painter(), chevron, Icon::Down, 18.0, theme::MUTE);
        let room = (chevron.left()
            - 6.0
            - rect.left()
            - widgets::text_width(ui, PROJECTION, theme::body())
            - 12.0)
            .max(0.0);
        let galley = ui.painter().layout(
            kind.name().to_owned(),
            theme::body(),
            theme::MUTE,
            f32::INFINITY,
        );
        if galley.size().x <= room {
            let at = pos2(
                chevron.left() - 6.0 - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            );
            ui.painter().galley(at, galley, theme::MUTE);
        }
        if resp.clicked() {
            self.projections = !open;
        }
        let key = egui::Id::new("projection-row");
        ui.data_mut(|d| d.insert_temp(key, rect));
    }

    /// The list of the projections, next to the Projection row. A tap on
    /// a row picks the projection and closes the list.
    fn projection_list(&mut self, ctx: &egui::Context, controls: &mut Controls, safe: Rect) {
        let Some(row) = ctx.data(|d| d.get_temp::<Rect>(egui::Id::new("projection-row"))) else {
            return;
        };
        let inset = widgets::menu_inset();
        let kinds = ProjectionKind::ALL;
        let full = 2.0 * inset.y + kinds.len() as f32 * PANEL_ROW;
        let bounds = safe.shrink(8.0);
        let width = LIST_WIDTH.min(bounds.width());
        // Below the row if it fits, else above it, else as high as the
        // screen allows.
        let below = bounds.bottom() - row.bottom() - 8.0;
        let above = row.top() - 8.0 - bounds.top();
        let (top, height) = if full <= below {
            (row.bottom() + 8.0, full)
        } else if full <= above {
            (row.top() - 8.0 - full, full)
        } else if below >= above {
            (row.bottom() + 8.0, below)
        } else {
            (bounds.top(), above)
        };
        let left = row.left().clamp(bounds.left(), bounds.right() - width);
        let rect = Rect::from_min_size(pos2(left, top), vec2(width, height));
        let pressed = ctx.input(|i| {
            i.pointer
                .any_pressed()
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        if pressed.is_some_and(|p| !rect.contains(p) && !row.contains(p)) {
            self.projections = false;
            return;
        }
        self.cards.push(("projections", rect));
        let spec = self.globe.flat.spec();
        let mut chosen = None;
        let origin = row.center().clamp(rect.min, rect.max);
        let backdrop = widgets::backdrop(ctx);
        widgets::menu_card(ctx, backdrop, "projections", rect, origin, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("projections")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                    for kind in kinds {
                        let on = kind == spec.kind;
                        if widgets::text_row(ui, controls, kind.name(), 0.0, on, true).clicked() {
                            chosen = Some(kind);
                        }
                    }
                });
        });
        if let Some(kind) = chosen {
            self.globe.flat.set_spec(ProjectionSpec { kind, ..spec });
            self.projections = false;
        }
    }

    /// The card of the workspace switch: a segmented list at the middle of
    /// the top row, or a menu button at its left end on a compact screen.
    fn switch_rect(&self, ui: &egui::Ui, safe: Rect, class: WidthClass, top: f32) -> Rect {
        let row = TOUCH + 2.0 * PAD;
        if class == WidthClass::Compact {
            let width = widgets::menu_button_width(ui, self.workspace.name());
            return Rect::from_min_size(pos2(safe.left() + MARGIN, top), vec2(width, row));
        }
        let names = Workspace::ALL.map(Workspace::name);
        let width = widgets::segments_width(ui, &names);
        Rect::from_min_size(pos2(safe.center().x - width / 2.0, top), vec2(width, row))
    }

    /// The menu of the workspaces below the menu button.
    fn workspace_menu(&mut self, ctx: &egui::Context, controls: &mut Controls, button: Rect) {
        let inset = widgets::menu_inset();
        let height = 2.0 * inset.y + Workspace::ALL.len() as f32 * PANEL_ROW;
        let at = pos2(button.left(), button.bottom() + 8.0);
        let rect = Rect::from_min_size(at, vec2(MENU_WIDTH, height));
        let pressed = ctx.input(|i| {
            i.pointer
                .any_pressed()
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        if pressed.is_some_and(|p| !rect.contains(p) && !button.contains(p)) {
            self.menu = false;
            return;
        }
        self.cards.push(("workspace-menu", rect));
        let current = self.workspace;
        let mut chosen = None;
        let origin = button.center().clamp(rect.min, rect.max);
        let backdrop = widgets::backdrop(ctx);
        widgets::menu_card(ctx, backdrop, "workspace-menu", rect, origin, |ui| {
            for workspace in Workspace::ALL {
                let on = workspace == current;
                let name = workspace.name();
                if widgets::text_row(ui, controls, name, 0.0, on, true).clicked() {
                    chosen = Some(workspace);
                }
            }
        });
        if let Some(workspace) = chosen {
            self.workspace = workspace;
            self.menu = false;
        }
    }

    /// The card with the empty state of the workspace, at the bottom of the
    /// screen. The Worlds workspace also shows which build this is.
    fn hint_card(&mut self, ui: &egui::Ui, safe: Rect) {
        let Some(hint) = self.workspace.hint() else {
            return;
        };
        let build = (self.workspace == Workspace::Worlds).then_some(self.build.as_str());
        let pad = widgets::BODY_PAD;
        let mut width = widgets::text_width(ui, hint, theme::body());
        if let Some(build) = build {
            width = width.max(widgets::text_width(ui, build, theme::hint()));
        }
        let width = (width + 2.0 * pad).min(safe.width() - 2.0 * MARGIN);
        let line = 24.0;
        let lines = if build.is_some() { 2.0 } else { 1.0 };
        let height = lines * line + 2.0 * 12.0;
        let rect = Rect::from_min_size(
            pos2(
                safe.center().x - width / 2.0,
                safe.bottom() - MARGIN - height,
            ),
            vec2(width, height),
        );
        self.cards.push(("workspace-hint", rect));
        widgets::card(
            ui.ctx(),
            widgets::backdrop(ui.ctx()),
            "workspace-hint",
            rect,
            egui::Order::Middle,
            CornerRadius::same(theme::CARD_RADIUS as u8),
            |ui| {
                let body = ui.max_rect().shrink2(vec2(pad, 12.0));
                let text = |ui: &mut egui::Ui, at: f32, text: &str, font, color| {
                    let rect = Rect::from_min_size(pos2(body.left(), at), vec2(body.width(), line));
                    let label = egui::RichText::new(text).font(font).color(color);
                    ui.put(rect, egui::Label::new(label).truncate());
                };
                text(ui, body.top(), hint, theme::body(), theme::TEXT);
                if let Some(build) = build {
                    text(ui, body.top() + line, build, theme::hint(), theme::MUTE);
                }
            },
        );
    }

    /// Takes the paths that the Files app picked, and shows the picker
    /// that the user asked for.
    pub fn run_picker(&mut self) {
        #[cfg(target_os = "ios")]
        {
            for path in self.picked.take() {
                self.import_path(&path);
            }
            if std::mem::take(&mut self.pick)
                && let Some(picker) = &self.picker
            {
                picker.pick(&["public.png", "public.tiff", "org.opengeospatial.geopackage"]);
            }
        }
        #[cfg(not(target_os = "ios"))]
        if std::mem::take(&mut self.pick)
            && let Some(path) = vale_shell::picker::pick_file("Heightmap", &IMPORT_TYPES)
        {
            self.import_path(&path);
        }
    }
}

/// A text that wraps to the width of `ui`.
fn wrapped(ui: &mut egui::Ui, text: &str, font: egui::FontId, color: egui::Color32) {
    let label = egui::RichText::new(text).font(font).color(color);
    ui.add(egui::Label::new(label).wrap());
}

/// A segmented list whose items share the width of the max rect of `ui`.
fn segments_fill(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    items: &[(&str, bool)],
    active: usize,
) -> Option<usize> {
    let card = ui.max_rect();
    let width = (card.width() - 2.0 * PAD) / items.len() as f32;
    let mut chosen = None;
    for (index, &(label, enabled)) in items.iter().enumerate() {
        let min = pos2(card.left() + PAD + index as f32 * width, card.top() + PAD);
        let rect = Rect::from_min_size(min, vec2(width, TOUCH));
        let on = index == active;
        let resp = widgets::hit(ui, controls, rect, label, Sense::click(), enabled, Some(on));
        if on {
            let thumb = rect.shrink(widgets::HIGHLIGHT_INSET);
            ui.painter()
                .rect_filled(thumb, theme::capsule(thumb), theme::raise());
        }
        widgets::fill(ui, rect, &resp, false);
        let color = widgets::fade(if on { theme::TEXT } else { theme::MUTE }, enabled);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            theme::body(),
            color,
        );
        if resp.clicked() && !on {
            chosen = Some(index);
        }
    }
    chosen
}

impl eframe::App for IsothermApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.run_picker();
        self.draw(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_height_has_groups_of_three_digits_and_a_unit() {
        assert_eq!(meters_text(120.4), "120 m");
        assert_eq!(meters_text(-1200.0), "−1,200 m");
        assert_eq!(meters_text(32767.0), "32,767 m");
        assert_eq!(meters_text(0.0), "0 m");
    }
}
