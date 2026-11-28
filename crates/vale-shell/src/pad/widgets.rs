//! The controls of the pad layout that each app uses. Each one is at least
//! 44 points on each side.

use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, Response, Sense, pos2, vec2};

use super::icons::{self, Icon};
use super::metrics::{PAD, TOUCH};
use super::{motion, theme};

/// A control that a finger can hit, as the last frame drew it.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub name: String,
    pub rect: Rect,
}

/// The list of the controls of one frame.
pub type Controls = Vec<Control>;

/// The padding at each side of the text of a segment.
const SEGMENT_PAD: f32 = 14.0;

pub const ICON: f32 = 22.0;

/// The height of a row in a panel.
pub const PANEL_ROW: f32 = 48.0;

/// The padding at the left and the right of the body of a panel.
pub const BODY_PAD: f32 = 16.0;

pub fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, theme::TEXT);
    galley.size().x.ceil()
}

/// A color of a control, faded while the control is off.
pub fn fade(color: Color32, enabled: bool) -> Color32 {
    if enabled {
        color
    } else {
        color.gamma_multiply(theme::OFF)
    }
}

/// Makes a rectangle a control. `selected` is the state of a control that
/// stays on. A control that is not enabled takes no touch. A control is not
/// in the list while a scroll area cuts it to less than the touch size.
pub fn hit(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    sense: Sense,
    enabled: bool,
    selected: Option<bool>,
) -> Response {
    let sense = if enabled { sense } else { Sense::hover() };
    let resp = ui.interact(rect, ui.id().with(name), sense);
    resp.widget_info(|| match selected {
        Some(on) => egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, name),
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name),
    });
    if enabled {
        push(ui, controls, rect, name);
    }
    resp
}

/// Adds a control to the list, unless a scroll area cuts it to less than
/// the touch size.
pub fn push(ui: &egui::Ui, controls: &mut Controls, rect: Rect, name: &str) {
    let whole = rect;
    let rect = rect.intersect(ui.clip_rect());
    let cut = |whole: f32, part: f32| part < TOUCH.min(whole);
    let cut = cut(whole.width(), rect.width()) || cut(whole.height(), rect.height());
    if rect.is_positive() && !cut {
        controls.push(Control {
            name: name.to_owned(),
            rect,
        });
    }
}

/// The space between the highlight of a control and the edge of its touch
/// area. The highlights of two controls that touch have a margin of two times
/// this space.
pub const HIGHLIGHT_INSET: f32 = 2.0;

pub fn fill(ui: &egui::Ui, rect: Rect, resp: &Response, active: bool) {
    if active || resp.is_pointer_button_down_on() {
        let rect = rect.shrink(HIGHLIGHT_INSET);
        ui.painter()
            .rect_filled(rect, control_radius(rect), theme::raise());
    }
}

/// The space between the edge of a menu card and the rect of its rows. A
/// row highlight reaches 8 points past the sides of the rect and sits
/// `HIGHLIGHT_INSET` inside its top and bottom, so each highlight is
/// `theme::ROW_INSET` from the edge of the card.
pub fn menu_inset() -> egui::Vec2 {
    vec2(BODY_PAD, theme::ROW_INSET - HIGHLIGHT_INSET)
}

/// The corner radii of the track of a segmented control at `track`, and of
/// its thumb `inset` points inside it. Both are capsules.
pub fn menu_track_radius(track: Rect, inset: f32) -> (f32, f32) {
    let outer = theme::capsule(track);
    (outer, theme::inner_radius(outer, inset))
}

/// The corner radius of the highlight of a tab at `rect`: a capsule.
pub fn tab_radius(rect: Rect) -> f32 {
    theme::capsule(rect)
}

/// The corner radius of the highlight of a control at `rect`. A square
/// control has a circle, as the controls of Liquid Glass. With the padding
/// of the cards, the circle of a cell at the end of a bar has the center of
/// the round end of the bar. Other controls have the radius of a row.
pub fn control_radius(rect: Rect) -> f32 {
    let square = (rect.width() - rect.height()).abs() < 1.0;
    match square {
        true => theme::capsule(rect),
        false => theme::row_radius(rect),
    }
}

/// Marks the start of the paint of a control that shrinks under a finger.
/// `press_end` shrinks what the control paints after this mark.
pub fn press_start(ui: &egui::Ui) -> egui::layers::ShapeIdx {
    ui.painter().add(egui::Shape::Noop)
}

/// Shrinks the paint of the control `resp` since `start` while a finger is
/// on it, and springs it back after. A wide control shrinks by the same
/// distance as a control of 160 points, not by the same ratio.
pub fn press_end(ui: &egui::Ui, resp: &Response, start: egui::layers::ShapeIdx) {
    let down = resp.is_pointer_button_down_on();
    let scale = motion::press(ui.ctx(), resp.id, down);
    if scale == 1.0 {
        return;
    }
    let side = resp.rect.width().max(resp.rect.height()).max(1.0);
    let scale = 1.0 - (1.0 - scale) * (160.0 / side).min(1.0);
    let end = ui.painter().add(egui::Shape::Noop);
    let transform = motion::about(scale, resp.rect.center());
    ui.ctx().graphics_mut(|g| {
        g.entry(ui.layer_id())
            .transform_range(start, end, transform)
    });
}

/// The thumb of the segmented control `id` at `rect`. The thumb slides
/// on a spring from the item before. `span` is the horizontal extent of
/// the items, and the spring runs in fractions of it, so the thumb moves
/// with the control in a scroll and stays on its item when the control
/// changes width.
pub fn slide_thumb(ui: &egui::Ui, id: egui::Id, span: egui::Rangef, rect: Rect) -> Rect {
    let ctx = ui.ctx();
    let width = span.span().max(1.0);
    let at = |x: f32| (x - span.min) / width;
    let left = motion::value(ctx, id.with("left"), at(rect.left()), motion::BOUNCY);
    let right = motion::value(ctx, id.with("right"), at(rect.right()), motion::BOUNCY);
    let x = |f: f32| span.min + f * width;
    Rect::from_x_y_ranges(x(left)..=x(right), rect.y_range())
}

/// A button with an icon, at a given place.
pub fn icon_button_at(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    name: &str,
    icon: Icon,
    active: bool,
    enabled: bool,
) -> Response {
    let resp = hit(
        ui,
        controls,
        rect,
        name,
        Sense::click(),
        enabled,
        Some(active),
    );
    let start = press_start(ui);
    fill(ui, rect, &resp, active);
    let color = if active { theme::TEXT } else { theme::MUTE };
    icons::paint(ui.painter(), rect, icon, ICON, fade(color, enabled));
    press_end(ui, &resp, start);
    resp
}

/// The width of a segmented list with these labels.
pub fn segments_width(ui: &egui::Ui, labels: &[&str]) -> f32 {
    let widths = labels
        .iter()
        .map(|label| (text_width(ui, label, theme::body()) + 2.0 * SEGMENT_PAD).max(TOUCH));
    widths.sum::<f32>() + 2.0 * PAD
}

/// A segmented list that fills a card. Each item is a label and its enabled
/// state. Returns the item that the user chose.
pub fn segments(
    ui: &egui::Ui,
    controls: &mut Controls,
    items: &[(&str, bool)],
    active: usize,
) -> Option<usize> {
    let card = ui.max_rect();
    let mut left = card.left() + PAD;
    let rects: Vec<Rect> = items
        .iter()
        .map(|&(label, _)| {
            let width = (text_width(ui, label, theme::body()) + 2.0 * SEGMENT_PAD).max(TOUCH);
            let rect = Rect::from_min_size(pos2(left, card.top() + PAD), vec2(width, TOUCH));
            left += width;
            rect
        })
        .collect();
    if let Some(&rect) = rects.get(active) {
        let id = ui.id().with(("segments", items));
        let thumb =
            slide_thumb(ui, id, egui::Rangef::new(card.left(), left), rect).shrink(HIGHLIGHT_INSET);
        ui.painter()
            .rect_filled(thumb, theme::capsule(thumb), theme::raise());
    }
    let mut chosen = None;
    for (index, (&(label, enabled), &rect)) in items.iter().zip(&rects).enumerate() {
        let on = index == active;
        let resp = hit(ui, controls, rect, label, Sense::click(), enabled, Some(on));
        fill(ui, rect, &resp, false);
        let color = fade(if on { theme::TEXT } else { theme::MUTE }, enabled);
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

/// The font of a small segmented control.
fn small_segment_font() -> egui::FontId {
    theme::font(13.0)
}

/// The padding of the track of a small segmented control.
const SMALL_TRACK_PAD: f32 = 3.0;

/// The width of a small segmented control with these labels.
pub fn small_segments_width(ui: &egui::Ui, labels: &[&str]) -> f32 {
    let widths = labels
        .iter()
        .map(|label| text_width(ui, label, small_segment_font()) + 24.0);
    widths.sum::<f32>() + 2.0 * SMALL_TRACK_PAD
}

/// A small segmented control on a dark track, for a choice in a row or a
/// title. The track is `rect`, and each item takes a touch of at least the
/// touch size. Returns the item that the user chose.
pub fn small_segments(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    labels: &[&str],
    active: usize,
) -> Option<usize> {
    small_segments_named(ui, controls, rect, (labels, labels), active)
}

/// A small segmented control whose items have the control names `names`.
pub fn small_segments_named(
    ui: &egui::Ui,
    controls: &mut Controls,
    rect: Rect,
    (labels, names): (&[&str], &[&str]),
    active: usize,
) -> Option<usize> {
    let painter = ui.painter();
    painter.rect_filled(rect, theme::capsule(rect), theme::track());
    let inner = rect.shrink(SMALL_TRACK_PAD);
    let total: f32 = labels
        .iter()
        .map(|l| text_width(ui, l, small_segment_font()) + 24.0)
        .sum();
    let scale = inner.width() / total.max(1.0);
    let mut left = inner.left();
    let items: Vec<Rect> = labels
        .iter()
        .map(|label| {
            let w = (text_width(ui, label, small_segment_font()) + 24.0) * scale;
            let item = Rect::from_min_size(pos2(left, inner.top()), vec2(w, inner.height()));
            left += w;
            item
        })
        .collect();
    if let Some(&item) = items.get(active) {
        let id = ui.id().with(("small-segments", names));
        let thumb = slide_thumb(ui, id, rect.x_range(), item);
        ui.painter()
            .rect_filled(thumb, theme::capsule(thumb), theme::raise());
    }
    let mut chosen = None;
    for (i, (label, &item)) in labels.iter().zip(&items).enumerate() {
        let w = item.width();
        let touch = Rect::from_center_size(item.center(), vec2(w, item.height().max(TOUCH)));
        let on = i == active;
        let name = names.get(i).unwrap_or(label);
        let resp = hit(ui, controls, touch, name, Sense::click(), true, Some(on));
        if !on && resp.is_pointer_button_down_on() {
            ui.painter()
                .rect_filled(item, theme::capsule(item), theme::raise());
        }
        let color = if on { theme::TEXT } else { theme::MUTE };
        ui.painter().text(
            item.center(),
            Align2::CENTER_CENTER,
            *label,
            small_segment_font(),
            color,
        );
        if resp.clicked() && !on {
            chosen = Some(i);
        }
    }
    chosen
}

/// The width of a menu button with this text.
pub fn menu_button_width(ui: &egui::Ui, text: &str) -> f32 {
    18.0 + text_width(ui, text, theme::body()) + 6.0 + 18.0 + 12.0
}

/// A button that fills a card and shows a text and a chevron.
pub fn menu_button(
    ui: &egui::Ui,
    controls: &mut Controls,
    name: &str,
    text: &str,
    open: bool,
) -> Response {
    let rect = ui.max_rect();
    let resp = hit(ui, controls, rect, name, Sense::click(), true, Some(open));
    if open || resp.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(rect, theme::capsule(rect), theme::raise());
    }
    let at = pos2(rect.left() + 18.0, rect.center().y);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, text, theme::body(), theme::TEXT);
    let chevron =
        Rect::from_center_size(pos2(rect.right() - 21.0, rect.center().y), vec2(18.0, 18.0));
    icons::paint(ui.painter(), chevron, Icon::Down, 18.0, theme::MUTE);
    resp
}

/// A row of a panel or of a menu, with a text. Returns the response of the
/// whole row and its rectangle.
pub fn text_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    indent: f32,
    selected: bool,
    enabled: bool,
) -> Response {
    text_row_as(ui, controls, name, name, indent, selected, enabled)
}

/// A `text_row` that shows `text`. `name` is the name of the control.
pub fn text_row_as(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    text: &str,
    indent: f32,
    selected: bool,
    enabled: bool,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let resp = hit(
        ui,
        controls,
        rect,
        name,
        Sense::click(),
        enabled,
        Some(selected),
    );
    if selected || resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(
            rect.expand2(vec2(8.0, -HIGHLIGHT_INSET)),
            theme::row_radius(rect.shrink2(vec2(0.0, HIGHLIGHT_INSET))),
            theme::raise(),
        );
    }
    let at = pos2(rect.left() + indent, rect.center().y);
    let color = fade(theme::TEXT, enabled);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, text, theme::body(), color);
    resp
}

/// The corners of a card in `rect`. Each round corner of `radius` gets
/// `theme::CARD_RADIUS`, or the largest corner of `radius` if that is more,
/// up to half the short side, as the corners of Liquid Glass.
pub fn card_radius(rect: Rect, radius: CornerRadius) -> CornerRadius {
    let most = f32::from(radius.nw.max(radius.ne).max(radius.sw).max(radius.se));
    let round = theme::CARD_RADIUS
        .max(most)
        .min(rect.width().min(rect.height()) / 2.0) as u8;
    // A corner that is smaller than the largest corner keeps its radius.
    let pick = |corner: u8| match corner {
        0 => 0,
        c if f32::from(c) < most => c,
        _ => round,
    };
    CornerRadius {
        nw: pick(radius.nw),
        ne: pick(radius.ne),
        sw: pick(radius.sw),
        se: pick(radius.se),
    }
}

/// A button with a text on the accent color. Its right edge and its middle
/// are at `right`.
pub fn accent_button(
    ui: &egui::Ui,
    controls: &mut Controls,
    right: egui::Pos2,
    name: &str,
) -> Response {
    accent_button_in(ui, controls, right, name, true)
}

/// An `accent_button` that is gray and takes no tap while `enabled` is
/// false.
pub fn accent_button_in(
    ui: &egui::Ui,
    controls: &mut Controls,
    right: egui::Pos2,
    name: &str,
    enabled: bool,
) -> Response {
    let width = text_width(ui, name, theme::body()) + 2.0 * 18.0;
    let rect = Rect::from_min_max(
        pos2(right.x - width, right.y - TOUCH / 2.0),
        pos2(right.x, right.y + TOUCH / 2.0),
    );
    let resp = hit(ui, controls, rect, name, Sense::click(), enabled, None);
    let start = press_start(ui);
    let (color, text) = match (enabled, resp.is_pointer_button_down_on()) {
        (false, _) => (theme::raise(), fade(theme::MUTE, false)),
        (true, true) => (theme::ACCENT, theme::CANVAS),
        (true, false) => (theme::ACCENT.gamma_multiply(0.8), theme::CANVAS),
    };
    let back = rect.shrink(HIGHLIGHT_INSET);
    ui.painter().rect_filled(back, theme::capsule(back), color);
    let at = rect.center();
    ui.painter()
        .text(at, Align2::CENTER_CENTER, name, theme::body(), text);
    press_end(ui, &resp, start);
    resp
}

/// A row of a panel that is one button with its text in the middle.
pub fn button_row(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    enabled: bool,
) -> Response {
    button_row_in(ui, controls, name, enabled, theme::ACCENT)
}

/// A button row with this fill color.
pub fn button_row_in(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    name: &str,
    enabled: bool,
    fill: Color32,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
    let resp = hit(ui, controls, rect, name, Sense::click(), enabled, None);
    let start = press_start(ui);
    let back = rect.shrink(HIGHLIGHT_INSET);
    let color = if resp.is_pointer_button_down_on() {
        fill
    } else {
        fill.gamma_multiply(0.8)
    };
    ui.painter()
        .rect_filled(back, theme::capsule(back), fade(color, enabled));
    let at = rect.center();
    ui.painter().text(
        at,
        Align2::CENTER_CENTER,
        name,
        theme::body(),
        theme::CANVAS,
    );
    press_end(ui, &resp, start);
    resp
}
/// A floating card with a shadow and the fill of a card, with no glass. A
/// touch on the card does not go to the canvas below it.
pub fn plain_card(
    ctx: &egui::Context,
    id: &str,
    rect: Rect,
    order: egui::Order,
    radius: CornerRadius,
    add: impl FnOnce(&mut egui::Ui),
) {
    let radius = card_radius(rect, radius);
    egui::Area::new(egui::Id::new(id))
        .order(order)
        .fixed_pos(rect.min)
        .movable(false)
        .constrain_to(ctx.viewport_rect())
        .constrain(false)
        .fade_in(false)
        .sizing_pass(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
            ui.painter().add(theme::shadow().as_shape(rect, radius));
            ui.painter().rect_filled(rect, radius, theme::card());
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
            ui.set_clip_rect(rect);
            theme::apply(&mut ui);
            add(&mut ui);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Pos2;

    #[test]
    fn a_card_has_round_cells_and_an_even_inset() {
        let cell = Rect::from_min_size(Pos2::ZERO, vec2(40.0, 40.0));
        let tab = Rect::from_min_size(Pos2::ZERO, vec2(140.0, 40.0));
        let row = Rect::from_min_size(Pos2::ZERO, vec2(240.0, 56.0));
        assert_eq!(control_radius(cell), 20.0);
        assert_eq!(control_radius(tab), 20.0);
        assert_eq!(control_radius(row), theme::ROW_RADIUS);
        assert_eq!(tab_radius(tab), 20.0);
        // A row highlight reaches 8 points past the sides of the rows, and
        // sits `HIGHLIGHT_INSET` inside their top and bottom.
        let inset = menu_inset();
        assert_eq!(inset.x - 8.0, theme::ROW_INSET);
        assert_eq!(inset.y + HIGHLIGHT_INSET, theme::ROW_INSET);
        assert_eq!(menu_track_radius(tab, 4.0), (20.0, 16.0));
    }

    #[test]
    fn a_row_is_concentric_with_its_card() {
        assert_eq!(theme::ROW_RADIUS + theme::ROW_INSET, theme::CARD_RADIUS);
        let wide = Rect::from_min_size(Pos2::ZERO, vec2(200.0, 30.0));
        assert_eq!(theme::capsule(wide), 15.0);
        assert_eq!(theme::row_radius(wide), 15.0);
        assert_eq!(theme::inner_radius(15.0, 4.0), 11.0);
        assert_eq!(theme::inner_radius(3.0, 4.0), 0.0);
    }

    #[test]
    fn a_card_has_the_corners_of_the_rim_or_is_a_capsule() {
        let panel = Rect::from_min_size(Pos2::ZERO, vec2(320.0, 400.0));
        let card = CornerRadius::same(14);
        assert_eq!(card_radius(panel, card), CornerRadius::same(28));
        // A card 44 points high is a capsule.
        let bar = Rect::from_min_size(Pos2::ZERO, vec2(200.0, 44.0));
        assert_eq!(card_radius(bar, card), CornerRadius::same(22));
        // A larger corner stays.
        let sheet = CornerRadius::same(theme::SHEET_GLASS_RADIUS as u8);
        assert_eq!(card_radius(panel, sheet), sheet);
        // A corner at the edge of the screen stays square.
        let edge = CornerRadius {
            nw: 0,
            sw: 0,
            ..card
        };
        let round = card_radius(panel, edge);
        assert_eq!((round.nw, round.sw, round.ne, round.se), (0, 0, 28, 28));
        // A smaller corner, such as the corner at the hinge, stays.
        let hinge = CornerRadius {
            sw: theme::HINGE_RADIUS as u8,
            ..CornerRadius::same(28)
        };
        let round = card_radius(panel, hinge);
        assert_eq!((round.nw, round.sw, round.ne, round.se), (28, 6, 28, 28));
    }

    #[test]
    fn a_thumb_stays_on_its_item_when_the_control_changes_width() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("thumb");
        let mut thumb = Rect::NOTHING;
        for width in [300.0, 200.0] {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let span = egui::Rangef::new(10.0, 10.0 + width);
                let item = Rect::from_x_y_ranges(10.0 + width / 2.0..=10.0 + width, 0.0..=44.0);
                thumb = slide_thumb(ui, id, span, item);
            });
            out.textures_delta.clear();
        }
        assert_eq!(thumb.x_range(), egui::Rangef::new(110.0, 210.0));
    }

    #[test]
    fn a_plain_card_takes_the_touches_on_it() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(200.0, 100.0));
        let mut inner = Rect::NOTHING;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let radius = CornerRadius::same(14);
            plain_card(ui.ctx(), "card", rect, egui::Order::Middle, radius, |ui| {
                inner = ui.max_rect();
            });
        });
        out.textures_delta.clear();
        assert_eq!(inner, rect);
        assert!(ctx.layer_id_at(rect.center()).is_some());
    }
}
