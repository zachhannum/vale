//! The on-screen keyboard that the system reports. On iOS, an observer of
//! the keyboard notifications stores the part of the window that the
//! keyboard covers. Elsewhere no report comes, and the UI guesses.
//!
//! UIKit sends the notifications on the main thread, and the UI runs on
//! the main thread, so the value is local to the thread. A test sets it on
//! its own thread.

use std::cell::Cell;

thread_local! {
    static COVER: Cell<Option<f32>> = const { Cell::new(None) };
}

/// Sets the height in points of the bottom part of the window that the
/// keyboard covers. 0 is no keyboard, and `None` is no report.
pub fn set_cover(points: Option<f32>) {
    COVER.with(|c| c.set(points.map(|p| p.max(0.0))));
}

/// The height in points of the bottom part of the window that the keyboard
/// covers, if the system reported it.
pub fn cover() -> Option<f32> {
    COVER.with(Cell::get)
}

/// True if the system reported that no keyboard covers the window, for
/// example after the user dismissed the on-screen keyboard.
pub fn hidden() -> bool {
    cover() == Some(0.0)
}

#[cfg(target_os = "ios")]
pub mod uikit {
    //! The observer of the keyboard notifications of UIKit.

    use std::ffi::c_void;
    use std::ptr::NonNull;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
    use objc2::{
        DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSValue};
    use objc2_ui_kit::{
        NSValueUIGeometryExtensions, UIKeyboardFrameEndUserInfoKey,
        UIKeyboardWillChangeFrameNotification, UIKeyboardWillHideNotification, UIView,
    };

    struct Ivars {
        view: Retained<UIView>,
        /// Asks the UI for a new frame.
        wake: Box<dyn Fn()>,
    }

    define_class!(
        // SAFETY: NSObject allows subclasses, and the type has no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "ValeKeyboardTarget"]
        #[ivars = Ivars]
        struct Target;

        unsafe impl NSObjectProtocol for Target {}

        impl Target {
            #[unsafe(method(keyboardWillChangeFrame:))]
            fn will_change_frame(&self, notification: &NSNotification) {
                super::set_cover(Some(self.cover(notification).unwrap_or(0.0)));
                (self.ivars().wake)();
            }

            #[unsafe(method(keyboardWillHide:))]
            fn will_hide(&self, _notification: &NSNotification) {
                super::set_cover(Some(0.0));
                (self.ivars().wake)();
            }
        }
    );

    impl Target {
        /// The part of the view that the end frame of the keyboard covers.
        /// A floating keyboard covers no band at the bottom, so it gives 0.
        fn cover(&self, notification: &NSNotification) -> Option<f32> {
            let info = notification.userInfo()?;
            // SAFETY: The key is a constant of UIKit.
            let key: &AnyObject = unsafe { UIKeyboardFrameEndUserInfoKey };
            let value = info.objectForKey(key)?.downcast::<NSValue>().ok()?;
            // SAFETY: The value of the key holds a CGRect.
            let screen = unsafe { value.CGRectValue() };
            let view = &self.ivars().view;
            let window = view.window()?;
            // The frame is in the coordinates of the screen.
            let frame =
                view.convertRect_fromView(window.convertRect_fromWindow(screen, None), None);
            let bounds = view.bounds();
            if frame.size.width < 0.9 * bounds.size.width {
                return Some(0.0);
            }
            let bottom = bounds.origin.y + bounds.size.height;
            let cover = (bottom - frame.origin.y).clamp(0.0, bounds.size.height);
            Some(cover as f32)
        }
    }

    /// The observer of the keyboard. The notification center does not
    /// retain its observer, so the app keeps this value.
    pub struct Observer {
        target: Retained<Target>,
    }

    impl Observer {
        /// Call it on the main thread with the view of the window. `wake`
        /// runs after each change of the keyboard.
        pub fn new(view: NonNull<c_void>, wake: Box<dyn Fn()>) -> Option<Observer> {
            let mtm = MainThreadMarker::new()?;
            // SAFETY: winit keeps its view for the life of the window.
            let view: &UIView = unsafe { view.cast::<UIView>().as_ref() };
            let ivars = Ivars {
                view: view.retain(),
                wake,
            };
            let this = Target::alloc(mtm).set_ivars(ivars);
            // SAFETY: NSObject has the initializer `init`.
            let target: Retained<Target> = unsafe { msg_send![super(this), init] };
            let center = NSNotificationCenter::defaultCenter();
            let observer: &AnyObject = &target;
            // SAFETY: The names are constants of UIKit. The target has the
            // method of each selector, and each method takes one
            // notification.
            unsafe {
                center.addObserver_selector_name_object(
                    observer,
                    sel!(keyboardWillChangeFrame:),
                    Some(UIKeyboardWillChangeFrameNotification),
                    None,
                );
                center.addObserver_selector_name_object(
                    observer,
                    sel!(keyboardWillHide:),
                    Some(UIKeyboardWillHideNotification),
                    None,
                );
            }
            Some(Observer { target })
        }
    }

    impl Drop for Observer {
        fn drop(&mut self) {
            let observer: &AnyObject = &self.target;
            // SAFETY: The observer is the object that `new` added.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer) };
        }
    }
}
