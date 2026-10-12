//! The life cycle of the app on iOS. The system can stop the app in the
//! background with no message, so the app saves when it leaves the
//! foreground.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_foundation::{NSNotification, NSNotificationCenter, ns_string};

struct Ivars {
    /// Saves each change that waits.
    save: Box<dyn Fn()>,
}

define_class!(
    // SAFETY: NSObject allows subclasses, and the type has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ValeLifecycleTarget"]
    #[ivars = Ivars]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        #[unsafe(method(leaveForeground:))]
        fn leave_foreground(&self, _notification: &NSNotification) {
            (self.ivars().save)();
        }
    }
);

/// The observer of the notifications of the app. The notification center
/// does not retain its observer, so the app keeps this value.
pub struct Observer {
    target: Retained<Target>,
}

impl Observer {
    /// Call it on the main thread. `save` runs when the app stops being
    /// active, and again when it goes to the background.
    pub fn new(save: Box<dyn Fn()>) -> Option<Observer> {
        let mtm = MainThreadMarker::new()?;
        let this = Target::alloc(mtm).set_ivars(Ivars { save });
        // SAFETY: NSObject has the initializer `init`.
        let target: Retained<Target> = unsafe { msg_send![super(this), init] };
        let center = NSNotificationCenter::defaultCenter();
        let observer: &AnyObject = &target;
        // The name of each notification is the text of its constant.
        for name in [
            ns_string!("UIApplicationWillResignActiveNotification"),
            ns_string!("UIApplicationDidEnterBackgroundNotification"),
        ] {
            // SAFETY: The target has the method of the selector, and the
            // method takes one notification.
            unsafe {
                center.addObserver_selector_name_object(
                    observer,
                    sel!(leaveForeground:),
                    Some(name),
                    None,
                );
            }
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
