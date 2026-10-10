//! The iPad layout of the world workspace: a full-screen canvas with
//! floating cards.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Order, Rect, pos2, vec2};
use vale_sphere::LonLat;
use vale_terrain::Mode;

use super::{AppState, Workspace, elevation, globe, panels};
use crate::globe::Tool;
use crate::globe::brush::{FLOW, STRENGTH_M};

pub mod geometry;
pub mod icons;
pub mod theme;
pub mod widgets;

use geometry::{BrushCard, Input, PAD, ROW, Rects, TOOLS, TOUCH, WidthClass};
use icons::Icon;
use widgets::{BODY_PAD, Controls, PANEL_ROW};

/// A panel that a button of the layout opens.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Panel {
    Brush,
    Layers,
    Toolbox,
}

impl Panel {
    fn key(self) -> &'static str {
        match self {
            Panel::Brush => "pad-brush",
            Panel::Layers => "pad-layers",
            Panel::Toolbox => "pad-toolbox",
        }
    }
}

/// The state of the iPad layout.
#[derive(Default)]
pub struct PadState {
    /// The Brush panel is open.
    pub brush: bool,
    /// The panel at the right: Layers, Toolbox, or none.
    pub right: Option<Panel>,
    /// The Layers panel shows the Height layer.
    pub layer: bool,
    /// The workspace menu is open.
    pub menu: bool,
    /// The distance that the sheet is pulled down from its open place.
    pub sheet_offset: f32,
    /// The controls of the last frame.
    pub controls: Controls,
    /// The places of the cards in the last frame.
    pub rects: Option<Rects>,
    /// The open panels of the last frame.
    pub panels: Vec<(Panel, Rect)>,
    /// The workspace menu of the last frame.
    pub menu_rect: Option<Rect>,
    /// The height of the contents of each panel, by the key of the panel.
    heights: HashMap<&'static str, f32>,
}

impl PadState {
    pub fn is_open(&self, panel: Panel) -> bool {
        match panel {
            Panel::Brush => self.brush,
            other => self.right == Some(other),
        }
    }

    pub fn open(&mut self, panel: Panel) {
        match panel {
            Panel::Brush => self.brush = true,
            other => self.right = Some(other),
        }
        self.sheet_offset = 0.0;
    }

    pub fn close(&mut self, panel: Panel) {
        match panel {
            Panel::Brush => self.brush = false,
            _ => self.right = None,
        }
    }

    /// Opens a closed panel and closes an open panel. `one_panel`: the screen
    /// has room for one panel, so the other panel closes.
    fn toggle(&mut self, panel: Panel, one_panel: bool) {
        if self.is_open(panel) {
            self.close(panel);
            return;
        }
        if one_panel {
            self.brush = false;
            self.right = None;
        }
        self.open(panel);
    }
}

const WORKSPACES: [&str; 3] = ["World", "Maps", "Atlas"];
const VIEWS: [&str; 2] = ["Globe", "Flat"];

/// The text of the position readout.
pub fn readout_text(lonlat: Option<LonLat>, meters: Option<f64>) -> String {
    let Some([lon, lat]) = lonlat else {
        return "–".to_owned();
    };
    let east = if lon < 0.0 { 'W' } else { 'E' };
    let north = if lat < 0.0 { 'S' } else { 'N' };
    let place = format!("{:.1}°{east} {:.1}°{north}", lon.abs(), lat.abs());
    match meters {
        Some(m) => format!("{place} · {m:.0} m"),
        None => place,
    }
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let screen = ctx.viewport_rect();
    ui.scope_builder(egui::UiBuilder::new().max_rect(screen), |ui| {
        ui.set_clip_rect(screen);
        globe::canvas(ui, state, theme::CANVAS);
    });

    let class = WidthClass::of(screen.width());
    let workspace_width = if class == WidthClass::Wide {
        widgets::segments_width(ui, &WORKSPACES)
    } else {
        widgets::menu_button_width(ui, WORKSPACES[0])
    };
    let mut input = Input {
        screen,
        bands: geometry::Bands::new(screen, ctx.content_rect()),
        workspace_width,
        view_width: widgets::segments_width(ui, &VIEWS),
        panel: state.pad.brush || state.pad.right.is_some(),
        sheet_offset: state.pad.sheet_offset,
    };
    let mut rects = geometry::layout(&input);
    if rects.one_panel && state.pad.brush && state.pad.right.is_some() {
        state.pad.right = None;
    }
    state.pad.sheet_offset = input.sheet_offset.clamp(0.0, rects.sheet_travel);

    let mut controls = std::mem::take(&mut state.pad.controls);
    controls.clear();
    state.pad.panels.clear();
    state.pad.menu_rect = None;
    if state.globe.tool != Tool::Brush || state.globe.brush.mode != Mode::Flatten {
        state.globe.pick_level = false;
    }

    top_row(&ctx, state, &mut controls, &rects);
    tool_strip(&ctx, state, &mut controls, &rects);
    brush_card(&ctx, state, &mut controls, &rects);
    // A button above can open or close a panel. The panels use the new state.
    input.panel = state.pad.brush || state.pad.right.is_some();
    input.sheet_offset = state.pad.sheet_offset;
    rects = geometry::layout(&input);
    for panel in [Panel::Brush, Panel::Layers, Panel::Toolbox] {
        if state.pad.is_open(panel) {
            show_panel(&ctx, state, &mut controls, &rects, panel);
        }
    }
    readout(&ctx, ui, state, &rects);
    if state.pad.menu && class != WidthClass::Wide {
        menu(&ctx, state, &mut controls, &rects);
    } else {
        state.pad.menu = false;
    }

    state.pad.controls = controls;
    state.pad.rects = Some(rects);
}

fn top_row(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let radius = theme::CARD_RADIUS.into();
    widgets::card(
        ctx,
        "pad-workspace",
        rects.workspace,
        Order::Middle,
        radius,
        |ui| {
            if rects.class == WidthClass::Wide {
                let items = [
                    (WORKSPACES[0], true),
                    (WORKSPACES[1], true),
                    (WORKSPACES[2], false),
                ];
                if widgets::segments(ui, controls, &items, 0) == Some(1) {
                    state.workspace = Workspace::Map;
                }
            } else {
                let open = state.pad.menu;
                if widgets::menu_button(ui, controls, "Workspace", WORKSPACES[0], open).clicked() {
                    state.pad.menu = !open;
                }
            }
        },
    );
    if let Some(view) = rects.view {
        widgets::card(ctx, "pad-view", view, Order::Middle, radius, |ui| {
            widgets::segments(ui, controls, &[(VIEWS[0], true), (VIEWS[1], false)], 0);
        });
    }
    widgets::card(
        ctx,
        "pad-actions",
        rects.actions,
        Order::Middle,
        radius,
        |ui| {
            let card = ui.max_rect();
            let at = |x: f32| {
                let min = pos2(card.left() + PAD + x, card.top() + PAD);
                Rect::from_min_size(min, vec2(TOUCH, TOUCH))
            };
            let step = TOUCH + PAD;
            let can_undo = state.globe.can_undo();
            let undo =
                widgets::icon_button_at(ui, controls, at(0.0), "Undo", Icon::Undo, false, can_undo);
            if undo.clicked() {
                state.globe.undo();
            }
            widgets::icon_button_at(ui, controls, at(step), "Redo", Icon::Redo, false, false);
            let divider = pos2(card.left() + PAD + 2.0 * step + 4.5, card.center().y);
            let line = Rect::from_center_size(divider, vec2(1.0, 24.0));
            ui.painter().rect_filled(line, 0.0, theme::divider());
            let x = 2.0 * step + 9.0 + PAD;
            for (panel, name, icon, x) in [
                (Panel::Layers, "Layers", Icon::Layers, x),
                (Panel::Toolbox, "Toolbox", Icon::Toolbox, x + step),
            ] {
                let active = state.pad.is_open(panel);
                if widgets::icon_button_at(ui, controls, at(x), name, icon, active, true).clicked()
                {
                    state.pad.toggle(panel, rects.one_panel);
                }
            }
        },
    );
}

/// The child of a card, inside the padding of the card.
fn padded(ui: &mut egui::Ui) -> egui::Ui {
    let rect = ui.max_rect().shrink(PAD);
    let layout = egui::Layout::top_down(egui::Align::Min);
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    ui
}

fn tool_strip(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let radius = theme::CARD_RADIUS.into();
    widgets::card(ctx, "pad-tools", rects.tools, Order::Middle, radius, |ui| {
        let mut ui = padded(ui);
        let mut cells = |ui: &mut egui::Ui| {
            let globe = &mut state.globe;
            for (name, icon, mode) in [
                ("Raise", Icon::Raise, Mode::Raise),
                ("Lower", Icon::Lower, Mode::Lower),
                ("Smooth", Icon::Smooth, Mode::Smooth),
                ("Flatten", Icon::Flatten, Mode::Flatten),
            ] {
                let active = globe.tool == Tool::Brush && globe.brush.mode == mode;
                if widgets::tool_cell(ui, controls, name, name, icon, active, true).clicked() {
                    globe.tool = Tool::Brush;
                    globe.brush.mode = mode;
                }
            }
            widgets::tool_cell(ui, controls, "Line", "Line", Icon::Line, false, false);
            let active = globe.tool == Tool::Navigate;
            if widgets::tool_cell(ui, controls, "Pan", "Move", Icon::Move, active, true).clicked() {
                globe.tool = Tool::Navigate;
            }
        };
        if rects.tool_cells < TOOLS {
            egui::ScrollArea::vertical()
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(&mut ui, cells);
        } else {
            cells(&mut ui);
        }
    });
}

/// Shows a value as a slider with a logarithmic scale.
fn log_slider(
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    text: &str,
    slider: impl FnOnce(&mut f64, &str) -> bool,
) -> Option<f64> {
    let (low, high) = (*range.start(), *range.end());
    let span = (high / low).ln();
    let mut fraction = if span > 0.0 {
        ((value / low).ln() / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    slider(&mut fraction, text).then(|| low * (fraction * span).exp())
}

/// Shows the size of the brush on the ground as a slider.
fn size_slider(state: &mut AppState, slider: impl FnOnce(&mut f64, &str) -> bool) {
    let world_km = state.doc.project.world.radius_km;
    let globe = &mut state.globe;
    let globe_radius = globe.radius();
    let range = globe.brush.radius_km_range(globe_radius, world_km);
    let km = globe.brush.radius_km(globe_radius, world_km);
    let text = format!("{} km", panels::three_significant(km));
    if let Some(km) = log_slider(km, range, &text, slider) {
        globe.brush.set_radius_km(km, globe_radius, world_km);
    }
}

/// Shows a value from 0 to 1 as a slider with a value in percent.
fn percent_slider(
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    slider: impl FnOnce(&mut f64, &str) -> bool,
) {
    let (low, high) = (*range.start(), *range.end());
    let mut fraction = ((*value - low) / (high - low)).clamp(0.0, 1.0);
    let text = format!("{:.0}%", *value * 100.0);
    if slider(&mut fraction, &text) {
        *value = low + fraction * (high - low);
    }
}

fn brush_card(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let Some(rect) = rects.brush else {
        return;
    };
    let radius = theme::CARD_RADIUS.into();
    widgets::card(ctx, "pad-brush-card", rect, Order::Middle, radius, |ui| {
        let mut ui = padded(ui);
        if let BrushCard::Full(height) = rects.brush_card {
            size_slider(state, |fraction, text| {
                widgets::vertical_slider(&mut ui, controls, "Size", height, fraction, text)
            });
            percent_slider(&mut state.globe.brush.flow, FLOW, |fraction, text| {
                widgets::vertical_slider(&mut ui, controls, "Flow", height, fraction, text)
            });
        }
        let open = state.pad.brush;
        let button = widgets::tool_cell(
            &mut ui,
            controls,
            "Brush settings",
            "Brush",
            Icon::Brush,
            open,
            true,
        );
        if button.clicked() {
            state.pad.toggle(Panel::Brush, rects.one_panel);
        }
    });
}

fn show_panel(
    ctx: &egui::Context,
    state: &mut AppState,
    controls: &mut Controls,
    rects: &Rects,
    panel: Panel,
) {
    let layer = panel == Panel::Layers && state.pad.layer;
    let (key, title, back) = match panel {
        Panel::Brush => (panel.key(), "Brush", None),
        Panel::Layers if layer => ("pad-layer", "Height", Some("Layers")),
        Panel::Layers => (panel.key(), "Layers", None),
        Panel::Toolbox => (panel.key(), "Toolbox", None),
    };
    let (rect, radius) = match rects.sheet {
        Some(sheet) => {
            let r = theme::SHEET_RADIUS as u8;
            let radius = egui::CornerRadius {
                nw: r,
                ne: r,
                sw: 0,
                se: 0,
            };
            (sheet, radius)
        }
        None => {
            let (min, width) = match panel {
                Panel::Brush => (rects.brush_panel, geometry::BRUSH_PANEL_WIDTH),
                _ => (rects.right_panel, geometry::RIGHT_PANEL_WIDTH),
            };
            let most = (rects.panel_bottom - min.y).max(ROW);
            let height = state.pad.heights.get(key).map_or(most, |h| h.min(most));
            let rect = Rect::from_min_size(min, vec2(width, height));
            (rect, theme::CARD_RADIUS.into())
        }
    };
    let sheet = rects.sheet.is_some();
    let id = if sheet { "pad-sheet" } else { key };
    state.pad.panels.push((panel, rect));
    widgets::card(ctx, id, rect, Order::Middle, radius, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        if sheet {
            let grabber =
                Rect::from_center_size(pos2(rect.center().x, rect.top() + 8.5), vec2(36.0, 5.0));
            ui.painter()
                .rect_filled(grabber, 3.0, theme::MUTE.gamma_multiply(0.6));
        }
        let row = widgets::title_row(ui, controls, title, back, sheet);
        if row.back {
            state.pad.layer = false;
        }
        if row.close {
            state.pad.close(panel);
        }
        if sheet {
            sheet_drag(ui, &mut state.pad, &row.bar, rects.sheet_travel);
            if state.pad.sheet_offset >= rects.sheet_travel {
                return;
            }
        }
        let bottom = if sheet {
            rect.bottom() - rects.bands.bottom
        } else {
            rect.bottom()
        };
        let most = (bottom - ui.cursor().top()).max(0.0);
        let out = egui::ScrollArea::vertical()
            .id_salt(key)
            .max_height(most)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let margin = egui::Margin {
                    left: BODY_PAD as i8,
                    right: BODY_PAD as i8,
                    top: 0,
                    bottom: 10,
                };
                egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    match panel {
                        Panel::Brush => brush_body(ui, state, controls),
                        Panel::Layers if layer => layer_body(ui, state),
                        Panel::Layers => layers_body(ui, state, controls),
                        Panel::Toolbox => toolbox_body(ui, state),
                    }
                });
            });
        state.pad.heights.insert(key, ROW + out.content_size.y);
    });
}

/// Moves the sheet with a drag on its title row. At the end of the drag, the
/// sheet goes to the nearer of its two places. A tap changes the place.
fn sheet_drag(ui: &egui::Ui, pad: &mut PadState, bar: &egui::Response, travel: f32) {
    if bar.dragged() {
        pad.sheet_offset = (pad.sheet_offset + bar.drag_delta().y).clamp(0.0, travel);
        ui.ctx().request_repaint();
    }
    let down = if bar.drag_stopped() {
        let speed = ui.input(|i| i.pointer.velocity().y);
        if speed.abs() > 300.0 {
            speed > 0.0
        } else {
            pad.sheet_offset > travel / 2.0
        }
    } else if bar.clicked() {
        pad.sheet_offset < travel / 2.0
    } else {
        return;
    };
    pad.sheet_offset = if down { travel } else { 0.0 };
}

fn brush_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    ui.spacing_mut().item_spacing.y = 0.0;
    size_slider(state, |fraction, text| {
        widgets::row_slider(ui, controls, "Brush size", "Size", fraction, text)
    });
    let brush = &mut state.globe.brush;
    percent_slider(&mut brush.hardness, 0.0..=1.0, |fraction, text| {
        widgets::row_slider(ui, controls, "Brush hardness", "Hardness", fraction, text)
    });
    percent_slider(&mut brush.flow, FLOW, |fraction, text| {
        widgets::row_slider(ui, controls, "Brush flow", "Flow", fraction, text)
    });
    let text = format!("{:.0} m", brush.strength_m);
    let strength = log_slider(brush.strength_m, STRENGTH_M, &text, |fraction, text| {
        widgets::row_slider(ui, controls, "Brush strength", "Strength", fraction, text)
    });
    if let Some(meters) = strength {
        brush.strength_m = meters;
    }
    ui.spacing_mut().item_spacing.y = 6.0;
    panels::brush_settings(ui, state);
}

fn layers_body(ui: &mut egui::Ui, state: &mut AppState, controls: &mut Controls) {
    let size = vec2(ui.available_width(), PANEL_ROW);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect.expand2(vec2(8.0, 0.0)), 5.0, theme::raise());
    let icon = Rect::from_center_size(pos2(rect.left() + 10.0, rect.center().y), vec2(18.0, 18.0));
    icons::paint(painter, icon, Icon::Raster, 18.0, theme::MUTE);
    let at = pos2(rect.left() + 28.0, rect.center().y);
    painter.text(
        at,
        Align2::LEFT_CENTER,
        "Height",
        theme::body(),
        theme::TEXT,
    );
    let open = Rect::from_center_size(
        pos2(rect.right() + 8.0 - TOUCH / 2.0, rect.center().y),
        vec2(TOUCH, TOUCH),
    );
    if widgets::icon_button_at(ui, controls, open, "Open Height", Icon::Open, false, true).clicked()
    {
        state.pad.layer = true;
    }
}

fn layer_body(ui: &mut egui::Ui, state: &mut AppState) {
    ui.checkbox(&mut state.globe.preview.greyscale, "Greyscale");
    elevation::controls(ui, state);
}

fn toolbox_body(ui: &mut egui::Ui, state: &mut AppState) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new("The toolbox has no tools yet.").color(theme::MUTE));
    // Temporary controls, for the tests of the brush on the device.
    ui.checkbox(&mut state.globe.debug, "Debug");
    if state.globe.debug {
        panels::debug_controls(ui, state);
    }
}

fn readout(ctx: &egui::Context, ui: &egui::Ui, state: &AppState, rects: &Rects) {
    let Some(band) = rects.readout else {
        return;
    };
    let text = readout_text(state.cursor_lonlat, state.cursor_meters);
    let galley = ui
        .painter()
        .layout_no_wrap(text, theme::mono(), theme::MUTE);
    let size = vec2(galley.size().x.ceil() + 24.0, band.height());
    let rect = if rects.class == WidthClass::Compact {
        Rect::from_min_size(band.min, size)
    } else {
        Rect::from_center_size(band.center(), size)
    };
    let radius = theme::CONTROL_RADIUS.into();
    widgets::card(ctx, "pad-readout", rect, Order::Middle, radius, |ui| {
        let at = rect.center() - galley.size() / 2.0;
        ui.painter().galley(at, galley, theme::MUTE);
    });
}

fn menu(ctx: &egui::Context, state: &mut AppState, controls: &mut Controls, rects: &Rects) {
    let compact = rects.class == WidthClass::Compact;
    let rows = if compact { 5.0 } else { 3.0 };
    let size = vec2(geometry::MENU_WIDTH, 10.0 + rows * PANEL_ROW + 13.0 + 10.0);
    let rect = Rect::from_min_size(rects.menu, size);
    state.pad.menu_rect = Some(rect);
    let pressed = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    if pressed.is_some_and(|pos| !rect.contains(pos) && !rects.workspace.contains(pos)) {
        state.pad.menu = false;
    }
    let radius = theme::CARD_RADIUS.into();
    widgets::card(ctx, "pad-menu", rect, Order::Foreground, radius, |ui| {
        // The menu covers the tool strip, so its fill is opaque.
        ui.painter()
            .rect_filled(rect, radius, theme::card().to_opaque());
        let inner = rect.shrink2(vec2(BODY_PAD, 10.0));
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
        let ui = &mut ui;
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        if widgets::text_row(ui, controls, WORKSPACES[0], 0.0, true, true).clicked() {
            state.pad.menu = false;
        }
        if compact {
            let globe = widgets::text_row(ui, controls, VIEWS[0], 16.0, false, true);
            let tick = Rect::from_center_size(
                pos2(globe.rect.right() - 14.0, globe.rect.center().y),
                vec2(18.0, 18.0),
            );
            icons::paint(ui.painter(), tick, Icon::Tick, 18.0, theme::ACCENT);
            if globe.clicked() {
                state.pad.menu = false;
            }
            widgets::text_row(ui, controls, VIEWS[1], 16.0, false, false);
        }
        let (rule, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), 13.0), egui::Sense::hover());
        let line = Rect::from_center_size(rule.center(), vec2(rule.width(), 1.0));
        ui.painter().rect_filled(line, 0.0, theme::raise());
        if widgets::text_row(ui, controls, WORKSPACES[1], 0.0, false, true).clicked() {
            state.workspace = Workspace::Map;
            state.pad.menu = false;
        }
        widgets::text_row(ui, controls, WORKSPACES[2], 0.0, false, false);
    });
}
