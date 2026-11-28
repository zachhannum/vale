//! The screen of the app: the globe, the workspace switch, and the card of
//! each workspace.

use eframe::egui::{self, CornerRadius, Rect, pos2, vec2};
use vale_shell::pad::metrics::{PAD, TOUCH, WidthClass};
use vale_shell::pad::theme;
use vale_shell::pad::widgets::{self, Controls, PANEL_ROW};

use crate::globe::{self, Globe};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    /// The empty state of the workspace. Explore shows the globe, so it has
    /// none.
    pub fn hint(self) -> Option<&'static str> {
        match self {
            Workspace::Worlds => Some("Saved worlds come in a later version."),
            Workspace::Terrain => Some("Heightmap import comes in a later version."),
            Workspace::Planet => Some("The planet settings come in a later version."),
            Workspace::Simulate => Some("The climate model comes in a later version."),
            Workspace::Explore => None,
            Workspace::Export => Some("Export comes in a later version."),
        }
    }
}

/// The name of the button of the workspace menu on a compact screen.
pub const WORKSPACE_MENU: &str = "Workspace";

/// The space between a card and the edge of the safe area.
const MARGIN: f32 = 16.0;
/// The space between the top of the safe area and the top row, and between
/// the top of a screen with no safe area and the top row.
const TOP_GAP: f32 = 4.0;
const TOP_MARGIN: f32 = 12.0;
/// The width of the workspace menu.
const MENU_WIDTH: f32 = 240.0;

pub struct IsothermApp {
    pub workspace: Workspace,
    /// The workspace menu of a compact screen is open.
    pub menu: bool,
    pub globe: Globe,
    /// The controls of the last frame.
    pub controls: Controls,
    build: String,
    /// The safe area of a screenshot. `None`: the safe area of the system.
    pub safe_area: Option<egui::SafeAreaInsets>,
}

/// Sets the style and the fonts of the app.
pub fn setup(ctx: &egui::Context) {
    theme::add_fonts(ctx);
    ctx.all_styles_mut(|style| style.visuals = egui::Visuals::dark());
}

impl IsothermApp {
    /// The app with the sample world on the globe.
    pub fn new() -> Result<IsothermApp, String> {
        let map = globe::sample()?;
        Ok(IsothermApp {
            workspace: Workspace::Explore,
            menu: false,
            globe: Globe::new(&map),
            controls: Controls::new(),
            build: crate::build::label(),
            safe_area: None,
        })
    }

    /// Draws one frame.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let screen = ctx.viewport_rect();
        let safe = match self.safe_area {
            Some(insets) => screen - insets.0,
            None => ctx.content_rect(),
        };
        let class = WidthClass::of(screen.width());
        let mut controls = Controls::new();
        // The cards are glass when the app has a wgpu renderer.
        let backdrop = self.globe.backdrop();
        widgets::set_backdrop(&ctx, backdrop);
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
        ui.scope_builder(egui::UiBuilder::new().max_rect(screen), |ui| {
            ui.set_clip_rect(screen);
            ui.painter().rect_filled(screen, 0.0, theme::CANVAS);
            self.globe.show(ui, screen, theme::CANVAS);
        });

        let switch = self.switch_rect(ui, safe, class, top);
        let current = self.workspace;
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
                    let menu = widgets::menu_button(
                        ui,
                        &mut controls,
                        WORKSPACE_MENU,
                        current.name(),
                        open,
                    );
                    if menu.clicked() {
                        self.menu = !open;
                    }
                    return;
                }
                let items: Vec<(&str, bool)> =
                    Workspace::ALL.iter().map(|w| (w.name(), true)).collect();
                let active = Workspace::ALL.iter().position(|&w| w == current);
                let chosen = widgets::segments(ui, &mut controls, &items, active.unwrap_or(0));
                if let Some(index) = chosen {
                    self.workspace = Workspace::ALL[index];
                }
            },
        );
        if class == WidthClass::Compact && self.menu {
            self.workspace_menu(&ctx, &mut controls, switch);
        } else {
            self.menu = false;
        }
        self.hint_card(ui, safe);
        self.controls = controls;
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
    fn hint_card(&self, ui: &egui::Ui, safe: Rect) {
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
}

impl eframe::App for IsothermApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}
