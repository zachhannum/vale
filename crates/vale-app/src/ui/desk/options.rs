//! The options bar below the title bar, and the Brush popover.

use eframe::egui::{self, Align, Align2, Layout, Rect, Sense, Shape, Stroke, pos2, vec2};
use vale_terrain::Mode;

use super::rail::{self, HAND, SELECT};
use super::widgets::{self, tip_text};
use super::{MapTool, theme};
use crate::globe::Tool;
use crate::globe::backdrop::card_shape;
use crate::globe::brush::{EROSION, FLOW, SCALE_KM, SEED, STRENGTH_M, TALUS_DEG, WARP_KM};
use crate::ui::pad::{log_slider, percent_slider, size_slider};
use crate::ui::{AppState, Workspace, panels};
use crate::world::three_significant;

/// The button of the options bar that opens the Brush popover.
pub const BRUSH_BUTTON: &str = "Brush…";
const POPOVER_WIDTH: f32 = 320.0;
const POPOVER_PAD: f32 = 14.0;
const POPOVER_RADIUS: u8 = 9;
/// The width of the label of a slider in the popover.
const LABEL: f32 = 72.0;

/// The name of the active tool.
pub fn tool_name(state: &AppState) -> &'static str {
    match state.workspace {
        Workspace::Globe => match state.globe.tool {
            Tool::Navigate => HAND,
            Tool::Brush => rail::mode_name(state.globe.brush.mode),
        },
        Workspace::Map => match state.desk.map_tool {
            MapTool::Select => SELECT,
            MapTool::Hand => HAND,
        },
    }
}

/// The settings of the active tool, in one row across the window.
pub fn bar(ui: &mut egui::Ui, state: &mut AppState) {
    state.desk.brush_button = None;
    let frame = egui::Frame::NONE.fill(theme::glass(state.translucent));
    egui::Panel::top("desk-options")
        .exact_size(theme::OPTIONS)
        .frame(frame)
        .show_separator_line(false)
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let stroke = Stroke::new(1.0, theme::line());
            ui.painter()
                .hline(rect.x_range(), rect.bottom() - 0.5, stroke);
            let layout = Layout::left_to_right(Align::Center);
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space(12.0);
                let name = ui.painter().layout_no_wrap(
                    tool_name(state).to_owned(),
                    theme::label(),
                    theme::TEXT,
                );
                let (at, _) = ui.allocate_exact_size(name.size(), Sense::hover());
                ui.painter().galley(at.min, name, theme::TEXT);
                if state.workspace == Workspace::Globe && state.globe.tool == Tool::Brush {
                    brush(ui, state);
                }
            });
        });
}

fn brush(ui: &mut egui::Ui, state: &mut AppState) {
    widgets::divider(ui);
    let width = widgets::SLIDER;
    ui.add_space(6.0);
    size_slider(state, |fraction, text| {
        widgets::slider(ui, "Size", None, width, fraction, text)
    });
    ui.add_space(6.0);
    let brush = &mut state.globe.brush;
    percent_slider(&mut brush.flow, FLOW, |fraction, text| {
        widgets::slider(ui, "Flow", None, width, fraction, text)
    });
    ui.add_space(6.0);
    percent_slider(&mut brush.hardness, 0.0..=1.0, |fraction, text| {
        widgets::slider(ui, "Hardness", None, width, fraction, text)
    });
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(12.0);
        let tip = tip_text(BRUSH_BUTTON, Some("B"));
        let open = state.desk.brush_open;
        let resp = widgets::button(ui, BRUSH_BUTTON, &tip, Some(open));
        if resp.clicked() {
            state.desk.brush_open = !open;
        }
        state.desk.brush_button = Some(resp.rect);
    });
}

/// The settings of the brush that the options bar does not have. The popover
/// opens below the Brush button.
pub fn popover(ctx: &egui::Context, state: &mut AppState) {
    let button = state.desk.brush_button.filter(|_| state.desk.brush_open);
    let Some(button) = button else {
        state.desk.brush_open = false;
        return;
    };
    let at = pos2(button.right() + 4.0 - POPOVER_WIDTH, button.bottom() + 10.0);
    let backdrop = state.globe.backdrop();
    let area = egui::Area::new(egui::Id::new("desk-brush"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .constrain(false)
        .show(ctx, |ui| {
            let painter = ui.painter().clone();
            let back: Vec<_> = (0..3).map(|_| painter.add(Shape::Noop)).collect();
            let margin = egui::Margin {
                left: POPOVER_PAD as i8,
                right: POPOVER_PAD as i8,
                top: 0,
                bottom: 12,
            };
            let inner = egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                ui.set_width(POPOVER_WIDTH - 2.0 * POPOVER_PAD);
                title(ui);
                body(ui, state);
            });
            let rect = inner.response.rect;
            let radius = POPOVER_RADIUS.into();
            let shadow = egui::Shadow {
                offset: [0, 12],
                blur: 36,
                spread: 0,
                color: egui::Color32::from_black_alpha(128),
            };
            painter.set(back[0], shadow.as_shape(rect, radius));
            if let Some(format) = backdrop {
                let blur = card_shape(format, rect, radius, ctx.pixels_per_point());
                painter.set(back[1], blur);
            }
            let fill = egui::epaint::RectShape::new(
                rect,
                radius,
                theme::popover(backdrop.is_some() || state.translucent),
                Stroke::new(1.0, egui::Color32::from_white_alpha(31)),
                egui::StrokeKind::Inside,
            );
            painter.set(back[2], fill);
        });
    if rail::pressed_outside(ctx, area.response.rect, button) {
        state.desk.brush_open = false;
    }
}

fn title(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
    let painter = ui.painter();
    let at = pos2(rect.left(), rect.center().y);
    painter.text(at, Align2::LEFT_CENTER, "Brush", theme::body(), theme::TEXT);
    let line = Rect::from_min_max(
        pos2(rect.left() - POPOVER_PAD, rect.bottom() - 1.0),
        pos2(rect.right() + POPOVER_PAD, rect.bottom()),
    );
    painter.rect_filled(line, 0.0, theme::line());
}

fn body(ui: &mut egui::Ui, state: &mut AppState) {
    ui.spacing_mut().item_spacing.y = 2.0;
    ui.add_space(6.0);
    let width = ui.available_width() - LABEL - 20.0 - widgets::VALUE;
    let row = |ui: &mut egui::Ui, name: &str, fraction: &mut f64, text: &str| {
        widgets::slider(ui, name, Some(LABEL), width, fraction, text)
    };
    let brush = &mut state.globe.brush;
    if brush.mode == Mode::Warp {
        let text = format!("{} km", three_significant(brush.warp_km));
        let strength = log_slider(brush.warp_km, WARP_KM, &text, |fraction, text| {
            row(ui, "Strength", fraction, text)
        });
        if let Some(km) = strength {
            brush.warp_km = km;
        }
        let text = format!("{} km", three_significant(brush.scale_km));
        let scale = log_slider(brush.scale_km, SCALE_KM, &text, |fraction, text| {
            row(ui, "Scale", fraction, text)
        });
        if let Some(km) = scale {
            brush.scale_km = km;
        }
        // The seed is a property of the world, so each brush gets the same
        // noise field.
        let seed = &mut state.doc.project.world.seed;
        let high = f64::from(*SEED.end());
        let mut fraction = (f64::from(*seed) / high).min(1.0);
        if row(ui, "Seed", &mut fraction, &seed.to_string()) {
            *seed = (fraction * high).round() as u32;
        }
    } else {
        let text = format!("{:.0} m", brush.strength_m);
        let name = panels::strength_name(brush.mode);
        let strength = log_slider(brush.strength_m, STRENGTH_M, &text, |fraction, text| {
            row(ui, name, fraction, text)
        });
        if let Some(meters) = strength {
            brush.strength_m = meters;
        }
    }
    if brush.mode == Mode::Uplift {
        percent_slider(&mut brush.erosion, EROSION, |fraction, text| {
            row(ui, "Erosion", fraction, text)
        });
    }
    if matches!(brush.mode, Mode::Weather | Mode::Uplift) {
        let text = format!("{}°", three_significant(brush.talus_deg));
        let angle = log_slider(brush.talus_deg, TALUS_DEG, &text, |fraction, text| {
            row(ui, "Angle", fraction, text)
        });
        if let Some(degrees) = angle {
            brush.talus_deg = degrees;
        }
    }
    ui.add_space(6.0);
    ui.spacing_mut().item_spacing.y = 6.0;
    panels::brush_limits(ui, state);
    panels::brush_settings(ui, state);
}
