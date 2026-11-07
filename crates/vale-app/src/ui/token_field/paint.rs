//! The look of a field.

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Shape, Stroke, pos2, vec2};

use super::layout::{
    Layout, Metrics, PlacedKind, SLOT_NAME_GAP, chip_parts, glyph_text, shows_name,
};
use super::{BoxKind, ChipKind, Field, Glyph, Node};
use crate::ui::pad::icons::{self, Icon};
use crate::ui::pad::theme;

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, a)
}

/// The text of a field chip.
pub const FIELD_TEXT: Color32 = Color32::from_rgb(147, 232, 211);
const BADGE_TEXT: Color32 = Color32::from_rgb(111, 195, 174);
/// The text of a function.
pub const FUNCTION_TEXT: Color32 = Color32::from_rgb(185, 196, 255);
const VALUE_TEXT: Color32 = Color32::from_rgb(245, 205, 138);
pub const BAD: Color32 = Color32::from_rgb(239, 107, 107);
/// The text of a selected chip.
const ON_ACCENT: Color32 = Color32::from_rgb(11, 20, 18);
/// The fill of the square of a type glyph.
pub const GLYPH_FILL: Color32 = rgba(69, 196, 168, 61);
/// The fill of a field.
pub const FIELD_FILL: Color32 = rgba(0, 0, 0, 97);

/// The fill, the border, and the text of a chip of a kind.
fn colors(kind: ChipKind) -> (Color32, Color32, Color32) {
    match kind {
        ChipKind::Field => (rgba(69, 196, 168, 38), rgba(69, 196, 168, 107), FIELD_TEXT),
        ChipKind::Function => (
            rgba(130, 150, 255, 26),
            rgba(140, 160, 255, 97),
            FUNCTION_TEXT,
        ),
        ChipKind::Value => (rgba(242, 184, 92, 28), rgba(242, 184, 92, 89), VALUE_TEXT),
        ChipKind::Operator => (rgba(255, 255, 255, 26), Color32::TRANSPARENT, theme::TEXT),
        ChipKind::Code => (rgba(239, 107, 107, 30), rgba(239, 107, 107, 140), BAD),
    }
}

/// The square of a type glyph with its middle at `center`.
pub fn paint_glyph(painter: &egui::Painter, center: Pos2, glyph: Glyph, m: &Metrics, on: bool) {
    let square = Rect::from_center_size(center, vec2(m.glyph, m.glyph));
    let fill = if on {
        Color32::from_black_alpha(46)
    } else {
        GLYPH_FILL
    };
    let color = if on { ON_ACCENT } else { FIELD_TEXT };
    painter.rect_filled(square, 5.0, fill);
    match glyph {
        Glyph::Bool => icons::paint(painter, square, Icon::Tick, m.glyph * 0.6, color),
        _ => {
            painter.text(
                center,
                Align2::CENTER_CENTER,
                glyph_text(glyph),
                m.glyph_font.clone(),
                color,
            );
        }
    }
}

/// The points of a rectangle with round corners, around and closed.
fn round_rect_points(rect: Rect, radius: f32) -> Vec<Pos2> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -90.0f32),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
        (pos2(rect.left() + r, rect.top() + r), 180.0),
    ];
    let mut out = Vec::new();
    for (c, start) in corners {
        for k in 0..=6 {
            let a = (start + k as f32 * 15.0).to_radians();
            out.push(c + vec2(a.cos(), a.sin()) * r);
        }
    }
    out.push(out[0]);
    out
}

/// A dashed frame with round corners.
pub fn dashed(painter: &egui::Painter, rect: Rect, radius: f32, stroke: Stroke) {
    let points = round_rect_points(rect, radius);
    painter.extend(Shape::dashed_line(&points, stroke, 4.0, 3.0));
}

/// Paints the field with its layout. The selected chip has the accent
/// fill, and the caret shows while the field has the focus.
pub fn paint<P: Clone>(
    painter: &egui::Painter,
    field: &Field<P>,
    layout: &Layout,
    m: &Metrics,
    focused: bool,
) {
    let (sel_path, a, b) = field.sel.span();
    let selected = |path: &[usize]| {
        path.len() > sel_path.len()
            && path[..sel_path.len()] == *sel_path
            && (a..b).contains(&path[sel_path.len()])
    };
    for p in &layout.placed {
        let on = selected(&p.path) && !field.sel.is_empty();
        match &p.kind {
            PlacedKind::Char(c) => {
                if on {
                    let back =
                        Rect::from_center_size(p.rect.center(), vec2(p.rect.width(), m.caret));
                    painter.rect_filled(back, 0.0, theme::ACCENT.gamma_multiply(0.4));
                }
                painter.text(
                    p.rect.center(),
                    Align2::CENTER_CENTER,
                    c.to_string(),
                    m.font.clone(),
                    theme::TEXT,
                );
            }
            PlacedKind::Break => {
                let fill = if on {
                    theme::ACCENT
                } else {
                    Color32::from_white_alpha(20)
                };
                painter.rect_filled(p.rect, 6.0, fill);
                let color = if on { ON_ACCENT } else { theme::MUTE };
                paint_break(painter, p.rect, color);
            }
            PlacedKind::Chip => {
                if let Some(Node::Chip(chip)) = field.node(&p.path) {
                    paint_chip(painter, p.rect, &chip.look, m, on);
                }
            }
            PlacedKind::Frame => {
                if let Some(Node::Box(bx)) = field.node(&p.path) {
                    if p.path.last().is_some_and(|&i| shows_name(bx, i)) {
                        let at = pos2(p.rect.left() - SLOT_NAME_GAP, p.rect.center().y);
                        let font = m.small.clone();
                        let color = FUNCTION_TEXT.gamma_multiply(0.7);
                        painter.text(at, Align2::RIGHT_CENTER, &bx.label, font, color);
                    }
                    paint_frame(
                        painter,
                        p.rect,
                        &bx.kind,
                        &bx.label,
                        bx.children.is_empty(),
                        m,
                        on,
                    );
                }
            }
        }
    }
    if focused
        && field.sel.is_empty()
        && let Some(r) = layout.caret_rect(&field.sel.head)
    {
        painter.rect_filled(r, 1.0, theme::ACCENT);
    }
}

/// The return arrow of a line break chip.
pub fn paint_break(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.height().min(rect.width()) * 0.22;
    let stroke = Stroke::new(1.5, color);
    let corner = pos2(c.x + s, c.y + s * 0.6);
    painter.line_segment([pos2(c.x + s, c.y - s), corner], stroke);
    let tip = pos2(c.x - s, corner.y);
    painter.line_segment([corner, tip], stroke);
    painter.line_segment([tip, tip + vec2(s * 0.6, -s * 0.6)], stroke);
    painter.line_segment([tip, tip + vec2(s * 0.6, s * 0.6)], stroke);
}

/// A chip: its fill and border, its type glyph, its label, and its badge.
pub fn paint_chip(painter: &egui::Painter, rect: Rect, look: &super::Look, m: &Metrics, on: bool) {
    let (mut fill, mut border, mut text) = colors(look.kind);
    if on {
        fill = theme::ACCENT;
        border = theme::ACCENT;
        text = ON_ACCENT;
    }
    painter.rect(
        rect,
        m.radius,
        fill,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    let mut measure = |s: &str, f: &egui::FontId| {
        painter
            .layout_no_wrap(s.to_owned(), f.clone(), text)
            .size()
            .x
    };
    let (_, label_x, badge_x) = chip_parts(look, m, &mut measure);
    let y = rect.center().y;
    if let Some(g) = look.glyph {
        let center = pos2(rect.left() + m.chip_pad.0 + m.glyph / 2.0, y);
        paint_glyph(painter, center, g, m, on);
    }
    painter.text(
        pos2(rect.left() + label_x, y),
        Align2::LEFT_CENTER,
        &look.label,
        m.font.clone(),
        text,
    );
    if let (Some(badge), Some(x)) = (&look.badge, badge_x) {
        let x = rect.left() + x;
        let line = if on {
            Color32::from_black_alpha(64)
        } else {
            border
        };
        painter.line_segment(
            [pos2(x, rect.top() + 1.0), pos2(x, rect.bottom() - 1.0)],
            Stroke::new(1.0, line),
        );
        let color = if on { ON_ACCENT } else { BADGE_TEXT };
        painter.text(
            pos2(x + 1.0 + m.chip_gap, y),
            Align2::LEFT_CENTER,
            badge,
            m.small.clone(),
            color,
        );
    }
}

fn paint_frame(
    painter: &egui::Painter,
    rect: Rect,
    kind: &BoxKind,
    label: &str,
    empty: bool,
    m: &Metrics,
    on: bool,
) {
    match kind {
        BoxKind::Group => {
            let color = if on {
                theme::ACCENT
            } else {
                rgba(174, 181, 190, 140)
            };
            if on {
                painter.rect_filled(rect, m.radius + 3.0, theme::ACCENT.gamma_multiply(0.15));
            }
            dashed(painter, rect, m.radius + 3.0, Stroke::new(1.5, color));
            if !label.is_empty() {
                let galley = painter.layout_no_wrap(label.to_owned(), m.small.clone(), theme::MUTE);
                let h = galley.size().y + 4.0;
                let tag = Rect::from_min_size(
                    pos2(rect.left() + 10.0, rect.top() - h / 2.0),
                    vec2(galley.size().x + 12.0, h),
                );
                painter.rect_filled(tag, 5.0, Color32::from_rgb(29, 32, 36));
                painter.galley(tag.min + vec2(6.0, 2.0), galley, theme::MUTE);
            }
        }
        BoxKind::Block => {
            let (fill, border, text) = colors(ChipKind::Function);
            let border = if on { theme::ACCENT } else { border };
            painter.rect(
                rect,
                m.radius + 3.0,
                fill,
                Stroke::new(1.0, border),
                egui::StrokeKind::Inside,
            );
            let at = pos2(rect.left() + m.box_pad.0, rect.center().y);
            painter.text(at, Align2::LEFT_CENTER, label, m.font.clone(), text);
        }
        BoxKind::Slot => {
            if empty {
                dashed(
                    painter,
                    rect,
                    m.radius,
                    Stroke::new(1.5, Color32::from_white_alpha(77)),
                );
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    label,
                    m.small.clone(),
                    theme::MUTE,
                );
            } else {
                painter.rect_filled(rect, m.radius, Color32::from_black_alpha(82));
            }
        }
    }
}
