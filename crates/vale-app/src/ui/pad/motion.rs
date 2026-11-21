//! Springs that move the chrome, as iPadOS moves its controls. A spring
//! keeps a value and a velocity for each id, in the data of the context.
//! While a spring moves, it asks for a repaint, and at rest it stops.

use std::sync::Arc;

use eframe::egui::{self, Id, LayerId, Pos2, emath::TSTransform, epaint::ClippedShape};

/// The feel of a spring. `response` is the time of one swing in seconds,
/// and `bounce` is from 0 for no overshoot to 1 for no friction.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub response: f32,
    pub bounce: f32,
}

/// The dock, the sheet, the panes, and the rows.
pub const SNAPPY: Spring = Spring {
    response: 0.32,
    bounce: 0.15,
};

/// Menus, popovers, and the thumbs of controls.
pub const BOUNCY: Spring = Spring {
    response: 0.36,
    bounce: 0.24,
};

/// The press of a control.
pub const PRESS: Spring = Spring {
    response: 0.22,
    bounce: 0.3,
};

/// The scale of a control under a finger.
pub const PRESS_SCALE: f32 = 0.96;

/// The scale of a card when it starts to appear.
pub const APPEAR_SCALE: f32 = 0.9;

/// A card that closes, which shrinks and fades out with no overshoot.
pub const EXIT: Spring = Spring {
    response: 0.2,
    bounce: 0.0,
};

/// A distance below which a value of 0 to 1 is at rest.
const REST: f32 = 1e-3;

/// A distance in points below which a place is at rest.
const REST_POINTS: f32 = 0.1;

/// The longest step of one frame. A longer frame, as after a stall, moves
/// the spring as if it were this long.
const MAX_DT: f32 = 1.0 / 20.0;

/// The value and the velocity of a spring.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub value: f32,
    pub velocity: f32,
}

impl State {
    /// Moves the value toward `target` for `dt` seconds. Returns true at
    /// rest, when the value is within `rest` of the target and slow. A
    /// spring at rest is at its target, with no velocity.
    pub fn step(&mut self, target: f32, dt: f32, spring: Spring, rest: f32) -> bool {
        let omega = std::f32::consts::TAU / spring.response.max(1e-3);
        let zeta = (1.0 - spring.bounce).clamp(0.0, 1.0);
        let x0 = self.value - target;
        let v0 = self.velocity;
        let t = dt.clamp(0.0, MAX_DT);
        let decay = (-zeta * omega * t).exp();
        let (x, v) = if zeta < 1.0 {
            let wd = omega * (1.0 - zeta * zeta).sqrt();
            let b = (v0 + zeta * omega * x0) / wd;
            let (sin, cos) = (wd * t).sin_cos();
            let x = decay * (x0 * cos + b * sin);
            let v =
                decay * ((b * wd - zeta * omega * x0) * cos - (x0 * wd + zeta * omega * b) * sin);
            (x, v)
        } else {
            let b = v0 + omega * x0;
            let x = decay * (x0 + b * t);
            let v = decay * (b - omega * (x0 + b * t));
            (x, v)
        };
        // The velocity counts as slow if it moves the value less than
        // `rest` in a tenth of a second.
        if x.abs() < rest && v.abs() < rest * 10.0 {
            *self = State {
                value: target,
                velocity: 0.0,
            };
            true
        } else {
            *self = State {
                value: target + x,
                velocity: v,
            };
            false
        }
    }
}

fn instant_id() -> Id {
    Id::new("pad-motion-instant")
}

/// Turns the springs off for this frame, for headless output and for
/// Reduce Motion. Each value then goes to its target at once.
pub fn set_instant(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(instant_id(), on));
}

/// True if the springs are off.
pub fn instant(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp(instant_id())).unwrap_or(false)
}

/// The state of a spring and the pass that last moved it.
#[derive(Clone, Copy, Default)]
struct Kept {
    state: State,
    pass: u64,
}

fn run(ctx: &egui::Context, id: Id, target: f32, spring: Spring, rest: f32, start: f32) -> f32 {
    let pass = ctx.cumulative_pass_nr();
    if instant(ctx) {
        ctx.data_mut(|d| {
            d.insert_temp(
                id,
                Kept {
                    state: State {
                        value: target,
                        velocity: 0.0,
                    },
                    pass,
                },
            )
        });
        return target;
    }
    let dt = ctx.input(|i| i.stable_dt);
    let (value, moving) = ctx.data_mut(|d| {
        let kept = d.get_temp_mut_or_insert_with(id, || Kept {
            state: State {
                value: start,
                velocity: 0.0,
            },
            pass: pass.wrapping_sub(1),
        });
        // A second call in one pass reads the value and does not step.
        if kept.pass == pass {
            let moving = kept.state.value != target || kept.state.velocity != 0.0;
            return (kept.state.value, moving);
        }
        kept.pass = pass;
        let rested = kept.state.step(target, dt, spring, rest);
        (kept.state.value, !rested)
    });
    if moving {
        ctx.request_repaint();
    }
    value
}

/// A value of 0 to 1 that follows `target` on a spring. A new id starts at
/// its target.
pub fn value(ctx: &egui::Context, id: Id, target: f32, spring: Spring) -> f32 {
    run(ctx, id, target, spring, REST, target)
}

/// A place in points that follows `target` on a spring. A new id starts at
/// its target.
pub fn points(ctx: &egui::Context, id: Id, target: f32, spring: Spring) -> f32 {
    run(ctx, id, target, spring, REST_POINTS, target)
}

/// A place in points that follows `target` on a spring, and starts at
/// `start` for a new id.
pub fn points_from(ctx: &egui::Context, id: Id, start: f32, target: f32, spring: Spring) -> f32 {
    run(ctx, id, target, spring, REST_POINTS, start)
}

/// 1 when `on`, and 0 when not, on the bouncy spring.
pub fn toggle(ctx: &egui::Context, id: Id, on: bool) -> f32 {
    value(ctx, id, f32::from(u8::from(on)), BOUNCY)
}

/// Puts a spring at `value`, with no velocity, as when it follows a finger.
pub fn set(ctx: &egui::Context, id: Id, value: f32) {
    kick(ctx, id, value, 0.0);
}

/// Puts a spring at `value` with `velocity` per second, as when a finger
/// lets go of it.
pub fn kick(ctx: &egui::Context, id: Id, value: f32, velocity: f32) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        d.insert_temp(
            id,
            Kept {
                state: State { value, velocity },
                // The next call steps the spring, also in this pass.
                pass: pass.wrapping_sub(1),
            },
        )
    });
}

/// The progress of a card that appears, from 0 to 1. Call it in each frame
/// that shows the card. A card that the last frame did not show starts
/// again at 0.
pub fn appear(ctx: &egui::Context, id: Id) -> f32 {
    appear_on(ctx, id, BOUNCY)
}

/// An `appear` on `spring`.
pub fn appear_on(ctx: &egui::Context, id: Id, spring: Spring) -> f32 {
    let pass = ctx.cumulative_pass_nr();
    let seen: Option<u64> = ctx.data(|d| d.get_temp(id.with("seen")));
    ctx.data_mut(|d| d.insert_temp(id.with("seen"), pass));
    if seen.is_none_or(|p| p + 1 < pass) {
        set(ctx, id, 0.0);
    }
    value(ctx, id, 1.0, spring)
}

/// The scale of a control that a finger is `down` on.
pub fn press(ctx: &egui::Context, id: Id, down: bool) -> f32 {
    let t = value(ctx, id.with("press"), f32::from(u8::from(down)), PRESS);
    1.0 - (1.0 - PRESS_SCALE) * t
}

/// How the picture of a closed card leaves the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Exit {
    /// It shrinks to its origin and fades out.
    Shrink,
    /// It moves down by this many points.
    Slide(f32),
    /// It fades out in its place.
    Fade,
}

/// A card that the frame shows, with the place that it grows from.
#[derive(Clone)]
struct Shown {
    layer: LayerId,
    origin: Pos2,
    exit: Exit,
}

/// The last picture of a card, and the last pass that showed it.
#[derive(Clone)]
struct Picture {
    layer: LayerId,
    origin: Pos2,
    exit: Exit,
    shapes: Arc<Vec<ClippedShape>>,
    pass: u64,
}

fn kept_id() -> Id {
    Id::new("pad-motion-kept")
}

fn pictures_id() -> Id {
    Id::new("pad-motion-pictures")
}

fn fill_id() -> Id {
    Id::new("pad-motion-fill")
}

/// Tells `paint_exits` that a form shows over a dark fill in this frame.
/// The pictures of the cards that close then paint in the middle order,
/// below the fill.
pub fn under_fill(ctx: &egui::Context) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(fill_id(), pass));
}

/// Keeps the picture of the card on `layer` at the end of the frame, so the
/// card can shrink to `origin` and fade out after it closes.
pub fn keep(ctx: &egui::Context, layer: LayerId, origin: Pos2) {
    keep_with(ctx, layer, origin, Exit::Shrink);
}

/// A `keep` with the way the picture leaves the screen.
pub fn keep_with(ctx: &egui::Context, layer: LayerId, origin: Pos2, exit: Exit) {
    ctx.data_mut(|d| {
        let kept: &mut Vec<Shown> = d.get_temp_mut_or_default(kept_id());
        kept.push(Shown {
            layer,
            origin,
            exit,
        });
    });
}

/// Keeps the pictures of the cards of this frame, and paints each card
/// that closed in this frame or a frame before as it shrinks and fades
/// out. The UI calls this after it draws everything else.
pub fn paint_exits(ctx: &egui::Context) {
    let pass = ctx.cumulative_pass_nr();
    let kept: Vec<Shown> = ctx
        .data_mut(|d| d.remove_temp(kept_id()))
        .unwrap_or_default();
    let mut pictures: Vec<Picture> = ctx.data(|d| d.get_temp(pictures_id())).unwrap_or_default();
    for k in kept {
        // The glass reads the screen of its own frame, so a picture leaves it
        // out.
        let shapes = ctx.graphics(|g| {
            g.get(k.layer).map_or_else(Vec::new, |list| {
                list.all_entries()
                    .filter(|c| !matches!(c.shape, egui::Shape::Callback(_)))
                    .cloned()
                    .collect()
            })
        });
        pictures.retain(|p| p.layer != k.layer);
        pictures.push(Picture {
            layer: k.layer,
            origin: k.origin,
            exit: k.exit,
            shapes: Arc::new(shapes),
            pass,
        });
    }
    let instant = instant(ctx);
    let fill = ctx.data(|d| d.get_temp::<u64>(fill_id())) == Some(pass);
    pictures.retain(|p| {
        if p.pass == pass {
            return true;
        }
        if instant {
            return false;
        }
        let id = p.layer.id.with("exit");
        if p.pass + 1 == pass {
            set(ctx, id, 1.0);
        }
        let t = value(ctx, id, 0.0, EXIT);
        if t <= 0.0 {
            return false;
        }
        paint_picture(ctx, p, t, fill);
        true
    });
    ctx.data_mut(|d| d.insert_temp(pictures_id(), pictures));
}

/// Paints the picture of a closed card at `t` of its size and opacity,
/// from 1 when it closes to 0 when it is gone. Under a dark `fill`, the
/// picture is at most in the middle order.
fn paint_picture(ctx: &egui::Context, picture: &Picture, t: f32, fill: bool) {
    let id = picture.layer.id.with("ghost");
    let order = match fill {
        true => picture.layer.order.min(egui::Order::Middle),
        false => picture.layer.order,
    };
    let layer = LayerId::new(order, id);
    let (transform, opacity) = match picture.exit {
        Exit::Shrink => {
            let scale = APPEAR_SCALE + (1.0 - APPEAR_SCALE) * t;
            (about(scale, picture.origin), t)
        }
        Exit::Slide(distance) => {
            let down = egui::vec2(0.0, (1.0 - t) * distance);
            (TSTransform::from_translation(down), 1.0)
        }
        Exit::Fade => (TSTransform::IDENTITY, t),
    };
    ctx.set_transform_layer(layer, transform);
    // The pictures paint in the order that the cards were kept.
    ctx.move_to_top(layer);
    egui::Area::new(id)
        .order(order)
        .fixed_pos(picture.origin)
        .interactable(false)
        .movable(false)
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            let mut painter = ui.painter().clone();
            painter.set_opacity(opacity);
            for c in picture.shapes.iter() {
                painter.with_clip_rect(c.clip_rect).add(c.shape.clone());
            }
        });
}

/// A scale by `scale` about `origin`.
pub fn about(scale: f32, origin: Pos2) -> TSTransform {
    let o = origin.to_vec2();
    TSTransform::from_translation(o)
        * TSTransform::from_scaling(scale)
        * TSTransform::from_translation(-o)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steps a spring from 0 to 1 at 120 frames per second. Returns the
    /// largest value and the time to rest.
    fn run_to_rest(spring: Spring, velocity: f32) -> (f32, f32) {
        let mut s = State {
            value: 0.0,
            velocity,
        };
        let dt = 1.0 / 120.0;
        let mut peak = 0.0f32;
        for i in 1..1000 {
            let rested = s.step(1.0, dt, spring, REST);
            peak = peak.max(s.value);
            if rested {
                return (peak, i as f32 * dt);
            }
        }
        panic!("the spring did not rest");
    }

    #[test]
    fn a_spring_rests_at_its_target() {
        let (_, time) = run_to_rest(SNAPPY, 0.0);
        assert!(time < 1.5 * SNAPPY.response * 2.0, "{time}");
    }

    #[test]
    fn a_bouncy_spring_overshoots_a_little() {
        let (peak, _) = run_to_rest(BOUNCY, 0.0);
        assert!(peak > 1.0 && peak < 1.12, "{peak}");
    }

    #[test]
    fn a_spring_with_no_bounce_does_not_overshoot() {
        let spring = Spring {
            response: 0.3,
            bounce: 0.0,
        };
        let (peak, _) = run_to_rest(spring, 0.0);
        assert!(peak <= 1.0, "{peak}");
    }

    #[test]
    fn a_kick_carries_the_value_past_its_target() {
        let (still, _) = run_to_rest(SNAPPY, 0.0);
        let (kicked, _) = run_to_rest(SNAPPY, 20.0);
        assert!(kicked > still, "{kicked} {still}");
    }

    #[test]
    fn a_long_frame_does_not_blow_up() {
        let mut s = State::default();
        for _ in 0..200 {
            s.step(1.0, 2.0, BOUNCY, REST);
            assert!(s.value.is_finite() && s.value.abs() < 2.0);
        }
        assert_eq!(s.value, 1.0);
    }

    #[test]
    fn an_instant_spring_is_at_its_target() {
        let ctx = egui::Context::default();
        set_instant(&ctx, true);
        let id = Id::new("t");
        value(&ctx, id, 0.0, SNAPPY);
        assert_eq!(value(&ctx, id, 1.0, SNAPPY), 1.0);
        assert_eq!(appear(&ctx, Id::new("a")), 1.0);
    }

    /// The values of `f` in `frames` frames of a context at 60 frames per
    /// second.
    fn frames(
        ctx: &egui::Context,
        frames: usize,
        mut f: impl FnMut(&egui::Context) -> f32,
    ) -> Vec<f32> {
        (0..frames)
            .map(|_| {
                let mut v = 0.0;
                let input = egui::RawInput {
                    predicted_dt: 1.0 / 60.0,
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| v = f(ui.ctx()));
                out.textures_delta.clear();
                v
            })
            .collect()
    }

    #[test]
    fn a_card_appears_from_zero_and_again_after_a_frame_away() {
        let ctx = egui::Context::default();
        let id = Id::new("card");
        let shown = frames(&ctx, 60, |ctx| appear(ctx, id));
        assert!(shown[0] < 0.5, "{shown:?}");
        assert!(shown.iter().any(|&t| t > 1.0), "{shown:?}");
        assert_eq!(shown.last(), Some(&1.0));
        frames(&ctx, 1, |_| 0.0);
        assert!(frames(&ctx, 1, |ctx| appear(ctx, id))[0] < 0.5);
    }

    #[test]
    fn a_press_shrinks_and_springs_back() {
        let ctx = egui::Context::default();
        let id = Id::new("button");
        frames(&ctx, 1, |ctx| press(ctx, id, false));
        let down = frames(&ctx, 60, |ctx| press(ctx, id, true));
        assert!(down[0] < 1.0 && down[0] > PRESS_SCALE, "{down:?}");
        assert_eq!(down.last(), Some(&PRESS_SCALE));
        let up = frames(&ctx, 60, |ctx| press(ctx, id, false));
        assert!(up.iter().any(|&s| s > 1.0), "{up:?}");
        assert_eq!(up.last(), Some(&1.0));
    }

    #[test]
    fn a_closed_card_shrinks_and_fades_out() {
        let ctx = egui::Context::default();
        let card = LayerId::new(egui::Order::Foreground, Id::new("menu"));
        let ghost = LayerId::new(egui::Order::Foreground, card.id.with("ghost"));
        let shown = |ctx: &egui::Context, open: bool| {
            if open {
                egui::Area::new(card.id).show(ctx, |ui| {
                    ui.label("Row");
                });
                keep(ctx, card, Pos2::ZERO);
            }
            paint_exits(ctx);
            f32::from(u8::from(ctx.memory(|m| m.areas().is_visible(&ghost))))
        };
        assert_eq!(frames(&ctx, 2, |ctx| shown(ctx, true)), [0.0, 0.0]);
        let closing = frames(&ctx, 60, |ctx| shown(ctx, false));
        assert_eq!(closing[1], 1.0, "{closing:?}");
        assert_eq!(closing.last(), Some(&0.0), "{closing:?}");
    }

    #[test]
    fn about_keeps_its_origin() {
        let o = egui::pos2(10.0, 20.0);
        assert_eq!(about(0.5, o) * o, o);
        assert_eq!(
            about(0.5, o) * egui::pos2(30.0, 20.0),
            egui::pos2(20.0, 20.0)
        );
    }
}
