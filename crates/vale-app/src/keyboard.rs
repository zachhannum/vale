//! The height of the on-screen keyboard of iPadOS.

use std::sync::atomic::{AtomicU32, Ordering};

/// The bits of the height of the docked keyboard, in points.
static HEIGHT: AtomicU32 = AtomicU32::new(0);

/// The height of the docked keyboard at the bottom of the screen, in points.
/// It is 0 while no keyboard shows, and on other systems.
pub fn height() -> f32 {
    f32::from_bits(HEIGHT.load(Ordering::Relaxed))
}

/// Follows the frame of the keyboard for the life of the process. A change
/// draws a new frame. Call it on the main thread.
#[cfg(target_os = "ios")]
pub fn install(ctx: eframe::egui::Context) {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSNotificationName, NSValue};
    use objc2_ui_kit::{
        NSValueUIGeometryExtensions, UIKeyboardFrameEndUserInfoKey,
        UIKeyboardWillChangeFrameNotification, UIKeyboardWillHideNotification,
    };

    fn observe(name: &NSNotificationName, f: impl Fn(&NSNotification) + 'static) {
        let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
            // SAFETY: the center gives a live notification for the time of
            // the call.
            f(unsafe { notification.as_ref() });
        });
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: the block is `'static`, and UIKit posts the notifications
        // of the keyboard on the main thread.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
        };
        std::mem::forget(token);
    }

    let set = move |height: f64| {
        HEIGHT.store((height.max(0.0) as f32).to_bits(), Ordering::Relaxed);
        ctx.request_repaint();
    };
    let shown = set.clone();
    // SAFETY: the statics are constants of UIKit.
    let (change, hide) = unsafe {
        (
            UIKeyboardWillChangeFrameNotification,
            UIKeyboardWillHideNotification,
        )
    };
    observe(change, move |notification| {
        // SAFETY: the user info of this notification holds the end frame of
        // the keyboard as an `NSValue` of a `CGRect`.
        let frame = unsafe {
            let Some(info) = notification.userInfo() else {
                return;
            };
            let Some(value) = info.objectForKey(UIKeyboardFrameEndUserInfoKey) else {
                return;
            };
            let Ok(value) = value.downcast::<NSValue>() else {
                return;
            };
            value.CGRectValue()
        };
        shown(frame.size.height);
    });
    // A floating keyboard and a keyboard that goes away cover no bottom.
    observe(hide, move |_| set(0.0));
}
