//! The services of iOS for the projects folders: the iCloud folder and the
//! files that the Files app gives to the app.

// The scene delegate of winit gives the URLs of the scene to the app
// delegate method `application:openURL:options:`.
#![allow(deprecated)]

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSDictionary, NSURL};
use objc2_ui_kit::{
    UIApplication, UIApplicationDelegate, UIApplicationDidFinishLaunchingNotification,
    UIApplicationOpenURLOptionsKey,
};
use vale_shell::cloud::watch_container;
pub use vale_shell::cloud::{CloudFiles, observe};

use super::SystemEvents;

struct Ivars {
    system: SystemEvents,
}

define_class!(
    // SAFETY: NSObject allows subclasses, and the type has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ValeAppDelegate"]
    #[ivars = Ivars]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl UIApplicationDelegate for AppDelegate {
        #[unsafe(method(application:openURL:options:))]
        fn open_url(
            &self,
            _app: &UIApplication,
            url: &NSURL,
            _options: &NSDictionary<UIApplicationOpenURLOptionsKey, AnyObject>,
        ) -> bool {
            let path = url.to_file_path();
            if let Some(path) = &path {
                self.ivars().system.open(path.clone());
            }
            path.is_some()
        }
    }
);

/// Connects the app to the system. Call it on the main thread before the
/// window starts, because the scene gives the file of the launch while the
/// app starts.
pub fn install(system: SystemEvents) {
    // SAFETY: the static is a constant of UIKit.
    let launched = unsafe { UIApplicationDidFinishLaunchingNotification };

    // winit starts the app with no delegate. This delegate gets the files
    // that the Files app opens in the app. The scene connects after the
    // launch, so the delegate gets the file of the launch too.
    let events = system.clone();
    observe(launched, move |_| {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let this = AppDelegate::alloc(mtm).set_ivars(Ivars {
            system: events.clone(),
        });
        // SAFETY: NSObject has the initializer `init`.
        let delegate: Retained<AppDelegate> = unsafe { msg_send![super(this), init] };
        let app = UIApplication::sharedApplication(mtm);
        // SAFETY: the app does not retain its delegate, so the delegate
        // lives for the life of the process.
        unsafe { app.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        std::mem::forget(delegate);
    });

    watch_container(move |dir| system.set_cloud(dir));
}
