//! Apple Pencil input from UIKit: the coalesced samples, the tilt, and the
//! hover. winit gets the same touches, because the recognizer never
//! recognizes a gesture.

use std::ffi::c_void;
use std::ptr::NonNull;

use eframe::egui;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, available, define_class, msg_send, sel,
};
use objc2_foundation::{NSArray, NSNumber, NSProcessInfo, NSSet};
use objc2_ui_kit::{
    UIEvent, UIGestureRecognizer, UIGestureRecognizerState, UIHoverGestureRecognizer, UITouch,
    UITouchType, UIView,
};

use super::{PenEvent, PenPhase, PenQueue};

struct Ivars {
    ctx: egui::Context,
    queue: PenQueue,
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Began,
    Moved,
    Ended,
    Cancelled,
}

define_class!(
    // SAFETY: UIGestureRecognizer allows subclasses, and the type has no Drop.
    #[unsafe(super(UIGestureRecognizer, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ValePenRecognizer"]
    #[ivars = Ivars]
    struct PenRecognizer;

    impl PenRecognizer {
        #[unsafe(method(touchesBegan:withEvent:))]
        fn touches_began(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.record(touches, event, Stage::Began);
            let _: () = unsafe { msg_send![super(self), touchesBegan: touches, withEvent: event] };
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn touches_moved(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.record(touches, event, Stage::Moved);
            let _: () = unsafe { msg_send![super(self), touchesMoved: touches, withEvent: event] };
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn touches_ended(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.record(touches, event, Stage::Ended);
            let _: () = unsafe { msg_send![super(self), touchesEnded: touches, withEvent: event] };
            // The state Failed makes UIKit reset the recognizer for the next stroke.
            self.setState(UIGestureRecognizerState::Failed);
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn touches_cancelled(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.record(touches, event, Stage::Cancelled);
            let _: () =
                unsafe { msg_send![super(self), touchesCancelled: touches, withEvent: event] };
            self.setState(UIGestureRecognizerState::Failed);
        }

        #[unsafe(method(canPreventGestureRecognizer:))]
        fn can_prevent(&self, _other: &UIGestureRecognizer) -> bool {
            false
        }

        #[unsafe(method(canBePreventedByGestureRecognizer:))]
        fn can_be_prevented(&self, _other: &UIGestureRecognizer) -> bool {
            false
        }

        /// The action of the hover recognizer.
        #[unsafe(method(penHover:))]
        fn pen_hover(&self, hover: &UIHoverGestureRecognizer) {
            let view = self.view();
            let view = view.as_deref();
            let state = hover.state();
            let moves = state == UIGestureRecognizerState::Began
                || state == UIGestureRecognizerState::Changed;
            let p = hover.locationInView(view);
            let height = available!(ios = 16.1).then(|| hover.zOffset() as f32);
            let tilt = available!(ios = 16.4).then(|| {
                (
                    hover.altitudeAngle() as f32,
                    hover.azimuthAngleInView(view) as f32,
                )
            });
            self.push(vec![PenEvent {
                phase: if moves {
                    PenPhase::Hover
                } else {
                    PenPhase::HoverEnd
                },
                pos: [p.x as f32, p.y as f32],
                force: 0.0,
                altitude: tilt.map(|t| t.0),
                azimuth: tilt.map(|t| t.1),
                height,
                // This clock is the clock of the touch times.
                time: NSProcessInfo::processInfo().systemUptime(),
            }]);
        }
    }

    unsafe impl NSObjectProtocol for PenRecognizer {}
);

impl PenRecognizer {
    fn new(mtm: MainThreadMarker, ivars: Ivars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        // SAFETY: This is the designated initializer, and it accepts nil.
        unsafe { msg_send![super(this), initWithTarget: None::<&AnyObject>, action: None::<Sel>] }
    }

    fn record(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>, stage: Stage) {
        let view = self.view();
        let view = view.as_deref();
        let mut out = Vec::new();
        for touch in touches {
            if touch.r#type() != UITouchType::Pencil {
                continue;
            }
            // iOS collects up to 240 samples per second, and it gives them here.
            let coalesced = match (stage, event) {
                (Stage::Cancelled, _) | (_, None) => None,
                (_, Some(event)) => event.coalescedTouchesForTouch(&touch),
            };
            let samples: Vec<Retained<UITouch>> = coalesced
                .map(|list| list.to_vec())
                .filter(|list| !list.is_empty())
                .unwrap_or_else(|| vec![touch]);
            let last = samples.len() - 1;
            for (i, sample) in samples.iter().enumerate() {
                let phase = match stage {
                    Stage::Began if i == 0 => PenPhase::Down,
                    Stage::Ended if i == last => PenPhase::Up,
                    Stage::Cancelled => PenPhase::Cancel,
                    _ => PenPhase::Move,
                };
                let p = sample.preciseLocationInView(view);
                let max = sample.maximumPossibleForce();
                let force = if max > 0.0 { sample.force() / max } else { 0.0 };
                out.push(PenEvent {
                    phase,
                    pos: [p.x as f32, p.y as f32],
                    force: force as f32,
                    altitude: Some(sample.altitudeAngle() as f32),
                    azimuth: Some(sample.azimuthAngleInView(view) as f32),
                    height: None,
                    time: sample.timestamp(),
                });
            }
        }
        self.push(out);
    }

    fn push(&self, events: Vec<PenEvent>) {
        if events.is_empty() {
            return;
        }
        let ivars = self.ivars();
        ivars.queue.push(events);
        ivars.ctx.request_repaint();
    }
}

/// Adds the pen recognizer and the hover recognizer to the UIView of winit.
/// Call it one time, on the main thread. `view` must point to a live UIView.
pub fn install(view: NonNull<c_void>, ctx: egui::Context, queue: PenQueue) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // SAFETY: winit keeps its view for the life of the window.
    let view: &UIView = unsafe { view.cast::<UIView>().as_ref() };

    let pen = PenRecognizer::new(mtm, Ivars { ctx, queue });
    let pencil =
        NSArray::from_retained_slice(&[NSNumber::numberWithInteger(UITouchType::Pencil.0)]);
    pen.setAllowedTouchTypes(&pencil);
    // winit must get each touch at the usual time.
    pen.setCancelsTouchesInView(false);
    pen.setDelaysTouchesBegan(false);
    pen.setDelaysTouchesEnded(false);
    view.addGestureRecognizer(&pen);

    // The hover recognizer does not retain its target. The view retains the
    // two recognizers.
    let target: &AnyObject = &pen;
    // SAFETY: PenRecognizer has the method penHover:, with one object argument.
    let hover = unsafe {
        UIHoverGestureRecognizer::initWithTarget_action(
            mtm.alloc(),
            Some(target),
            Some(sel!(penHover:)),
        )
    };
    hover.setCancelsTouchesInView(false);
    view.addGestureRecognizer(&hover);
}
