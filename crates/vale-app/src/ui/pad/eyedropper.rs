//! The eyedropper of the color picker. It picks the color of a pixel of the
//! screen. While it is on, the pointer events go to the eyedropper and not
//! to egui, so no control, tool, or canvas sees them.

use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, ColorImage, Pos2, Rect, Sense, pos2, vec2};

use super::geometry::{Bands, GAP, MARGIN, PAD, ROW, TOUCH};
use super::theme;
use super::widgets::{self, Controls};

pub const HINT: &str = "Touch a color to pick it.";
pub const CANCEL: &str = "Cancel";
/// The diameter of the loupe.
const LOUPE: f32 = 64.0;
/// The distance from the finger to the center of the loupe.
const LIFT: f32 = 76.0;
const HINT_WIDTH: f32 = 340.0;

fn id() -> egui::Id {
    egui::Id::new("pad-eyedropper")
}

#[derive(Clone, Default)]
struct Eyedropper {
    /// The name of the color picker that started the eyedropper.
    picker: String,
    /// The screenshot is requested.
    shot: bool,
    image: Option<Arc<ColorImage>>,
    /// The pointer events that the input hook took from egui.
    events: Vec<egui::Event>,
    at: Option<Pos2>,
    down: bool,
    /// The place of a release that waits for the screenshot.
    release: Option<Pos2>,
    picked: Option<[u8; 3]>,
}

fn get(ctx: &egui::Context) -> Option<Eyedropper> {
    ctx.data(|data| data.get_temp(id()))
}

fn set(ctx: &egui::Context, eyedropper: Eyedropper) {
    ctx.data_mut(|data| data.insert_temp(id(), eyedropper));
}

fn end(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<Eyedropper>(id()));
}

/// The user data of the screenshot of the eyedropper.
struct Shot;

/// Takes the pointer events from the input while the eyedropper is on.
struct Filter;

impl egui::Plugin for Filter {
    fn debug_name(&self) -> &'static str {
        "vale eyedropper"
    }

    fn input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        let Some(mut eyedropper) = get(ctx).filter(|e| e.shot && e.picked.is_none()) else {
            return;
        };
        let pointer = |event: &egui::Event| {
            matches!(
                event,
                egui::Event::PointerMoved(_)
                    | egui::Event::MouseMoved(_)
                    | egui::Event::PointerButton { .. }
                    | egui::Event::PointerGone
                    | egui::Event::Touch { .. }
                    | egui::Event::MouseWheel { .. }
                    | egui::Event::Zoom(_)
            )
        };
        let (taken, kept) = std::mem::take(&mut input.events)
            .into_iter()
            .partition(pointer);
        input.events = kept;
        eyedropper.events.extend(taken);
        set(ctx, eyedropper);
    }
}

/// Starts the eyedropper for the color picker with this name.
pub fn start(ctx: &egui::Context, picker: &str) {
    ctx.add_plugin(Filter);
    widgets::close_keypad(ctx);
    set(
        ctx,
        Eyedropper {
            picker: picker.to_owned(),
            ..Default::default()
        },
    );
    ctx.request_repaint();
}

/// True while the eyedropper waits for a touch.
pub fn active(ctx: &egui::Context) -> bool {
    get(ctx).is_some_and(|e| e.picked.is_none())
}

/// True while the eyedropper of this color picker waits for a touch.
pub fn active_for(ctx: &egui::Context, picker: &str) -> bool {
    get(ctx).is_some_and(|e| e.picked.is_none() && e.picker == picker)
}

/// The color that the eyedropper picked for this color picker. The
/// eyedropper ends.
pub fn take(ctx: &egui::Context, picker: &str) -> Option<[u8; 3]> {
    let eyedropper = get(ctx).filter(|e| e.picker == picker)?;
    let picked = eyedropper.picked?;
    end(ctx);
    Some(picked)
}

/// The color of the pixel of the screenshot under a point.
fn sample(image: &ColorImage, at: Pos2, pixels_per_point: f32) -> [u8; 3] {
    let [w, h] = image.size;
    let x = ((at.x * pixels_per_point) as usize).min(w.saturating_sub(1));
    let y = ((at.y * pixels_per_point) as usize).min(h.saturating_sub(1));
    let c = image
        .pixels
        .get(y * w + x)
        .copied()
        .unwrap_or(Color32::BLACK);
    [c.r(), c.g(), c.b()]
}

/// Draws the hint and the loupe, and picks the color at the end of a touch.
pub fn draw(ctx: &egui::Context, controls: &mut Controls) {
    let Some(mut eyedropper) = get(ctx).filter(|e| e.picked.is_none()) else {
        return;
    };
    // The screenshot shows the screen as it is before the hint shows.
    if !eyedropper.shot {
        eyedropper.shot = true;
        set(ctx, eyedropper);
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(Shot)));
        ctx.request_repaint();
        return;
    }
    let shot = ctx.input(|i| {
        i.raw.events.iter().find_map(|event| match event {
            egui::Event::Screenshot {
                user_data, image, ..
            } if user_data
                .data
                .as_ref()
                .is_some_and(|data| data.downcast_ref::<Shot>().is_some()) =>
            {
                Some(Arc::clone(image))
            }
            _ => None,
        })
    });
    if shot.is_some() {
        eyedropper.image = shot;
    }

    let screen = ctx.viewport_rect();
    let bands = Bands::of(ctx);
    let width = HINT_WIDTH.min(screen.width() - 2.0 * MARGIN);
    let hint = Rect::from_min_size(
        pos2(screen.center().x - width / 2.0, screen.top() + bands.top),
        vec2(width, ROW),
    );

    for event in std::mem::take(&mut eyedropper.events) {
        let release = match event {
            egui::Event::PointerMoved(at) => {
                eyedropper.at = Some(at);
                None
            }
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                ..
            } => {
                eyedropper.at = Some(pos);
                let release = eyedropper.down && !pressed;
                eyedropper.down = pressed;
                release.then_some(pos)
            }
            egui::Event::Touch { phase, pos, .. } => {
                eyedropper.at = Some(pos);
                let release = eyedropper.down && phase == egui::TouchPhase::End;
                eyedropper.down = matches!(phase, egui::TouchPhase::Start | egui::TouchPhase::Move);
                release.then_some(pos)
            }
            _ => None,
        };
        if let Some(at) = release {
            if hint.contains(at) {
                end(ctx);
                ctx.request_repaint();
                return;
            }
            eyedropper.release = Some(at);
            break;
        }
    }
    let ppp = ctx.pixels_per_point();
    if let (Some(at), Some(image)) = (eyedropper.release, &eyedropper.image) {
        eyedropper.picked = Some(sample(image, at, ppp));
        set(ctx, eyedropper);
        ctx.request_repaint();
        return;
    }
    if eyedropper.image.is_none() {
        ctx.request_repaint();
    }

    widgets::card(
        ctx,
        widgets::backdrop(ctx),
        "pad-eyedropper-hint",
        hint,
        egui::Order::Tooltip,
        theme::CARD_RADIUS.into(),
        |ui| {
            let rect = ui.max_rect();
            ui.painter().text(
                pos2(rect.left() + GAP, rect.center().y),
                Align2::LEFT_CENTER,
                HINT,
                theme::body(),
                theme::TEXT,
            );
            let button = Rect::from_min_max(
                pos2(rect.right() - PAD - 2.0 * TOUCH, rect.top() + PAD),
                pos2(rect.right() - PAD, rect.bottom() - PAD),
            );
            widgets::hit(ui, controls, button, CANCEL, Sense::click(), true, None);
            ui.painter().text(
                button.center(),
                Align2::CENTER_CENTER,
                CANCEL,
                theme::body(),
                theme::ACCENT,
            );
        },
    );

    let (Some(at), Some(image)) = (eyedropper.at, &eyedropper.image) else {
        set(ctx, eyedropper);
        return;
    };
    if !hint.contains(at) {
        let [r, g, b] = sample(image, at, ppp);
        let radius = LOUPE / 2.0;
        // The loupe is above the finger, or below it near the top.
        let above = at.y - LIFT - radius >= screen.top() + bands.top;
        let center = if above {
            at - vec2(0.0, LIFT)
        } else {
            at + vec2(0.0, LIFT)
        };
        let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("pad-eyedropper-loupe"));
        let painter = ctx.layer_painter(layer);
        painter.circle_filled(center, radius, Color32::from_rgb(r, g, b));
        painter.circle_stroke(center, radius, egui::Stroke::new(3.0, Color32::BLACK));
        painter.circle_stroke(center, radius, egui::Stroke::new(1.5, Color32::WHITE));
    }
    set(ctx, eyedropper);
}
