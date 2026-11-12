//! The window scene of the app.
//!
//! An app with a `UIApplicationSceneManifest` in its `Info.plist` names
//! `WinitSceneDelegate` as the delegate of its scene configuration. The
//! delegate puts the windows on the scene, and it gives the URLs of the scene
//! to the app delegate.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{declare_class, msg_send, mutability, sel, ClassType, DeclaredClass};
use objc2_foundation::{MainThreadMarker, NSDictionary, NSObjectProtocol, NSSet};
use objc2_ui_kit::{
    UIApplication, UIOpenURLContext, UIResponder, UIScene, UISceneActivationState,
    UISceneConnectionOptions, UISceneDelegate, UISceneSession, UIWindowScene,
    UIWindowSceneDelegate,
};

use super::window::WinitUIWindow;

thread_local! {
    /// The windows that the app made before a scene connected.
    static WAITING: RefCell<Vec<Retained<WinitUIWindow>>> = const { RefCell::new(Vec::new()) };
}

declare_class!(
    pub(crate) struct WinitSceneDelegate;

    unsafe impl ClassType for WinitSceneDelegate {
        #[inherits(NSObject)]
        type Super = UIResponder;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "WinitSceneDelegate";
    }

    impl DeclaredClass for WinitSceneDelegate {}

    unsafe impl NSObjectProtocol for WinitSceneDelegate {}

    unsafe impl UISceneDelegate for WinitSceneDelegate {
        #[method(scene:willConnectToSession:options:)]
        fn scene_will_connect(
            &self,
            scene: &UIScene,
            _session: &UISceneSession,
            options: &UISceneConnectionOptions,
        ) {
            if let Some(scene) = window_scene(scene) {
                for window in WAITING.with_borrow_mut(std::mem::take) {
                    unsafe { window.setWindowScene(Some(scene)) };
                    window.makeKeyAndVisible();
                }
            }
            let contexts = unsafe { options.URLContexts() };
            open_urls(&contexts);
        }

        #[method(scene:openURLContexts:)]
        fn scene_open_url_contexts(&self, _scene: &UIScene, contexts: &NSSet<UIOpenURLContext>) {
            open_urls(contexts);
        }
    }

    unsafe impl UIWindowSceneDelegate for WinitSceneDelegate {}
);

fn window_scene(scene: &UIScene) -> Option<&UIWindowScene> {
    if scene.isKindOfClass(UIWindowScene::class()) {
        // SAFETY: the class of the scene is `UIWindowScene` or a subclass.
        Some(unsafe { &*(scene as *const UIScene as *const UIWindowScene) })
    } else {
        None
    }
}

/// Gives each URL to `application:openURL:options:` of the app delegate,
/// because UIKit calls that method only in an app with no scenes.
fn open_urls(contexts: &NSSet<UIOpenURLContext>) {
    let mtm = MainThreadMarker::new().unwrap();
    let app = UIApplication::sharedApplication(mtm);
    let Some(delegate) = (unsafe { app.delegate() }) else {
        return;
    };
    let selector = sel!(application:openURL:options:);
    let responds: bool = unsafe { msg_send![&delegate, respondsToSelector: selector] };
    if !responds {
        return;
    }
    let options = NSDictionary::<AnyObject, AnyObject>::new();
    for context in contexts {
        let url = unsafe { context.URL() };
        let _: bool =
            unsafe { msg_send![&delegate, application: &*app, openURL: &*url, options: &*options] };
    }
}

/// The window scene of the app: the active scene, or else the first scene.
/// An app with no scene manifest also has one scene.
pub(crate) fn current(mtm: MainThreadMarker) -> Option<Retained<UIWindowScene>> {
    let scenes = unsafe { UIApplication::sharedApplication(mtm).connectedScenes() };
    let mut first = None;
    for scene in &scenes {
        let Some(window_scene) = window_scene(scene) else {
            continue;
        };
        if unsafe { scene.activationState() } == UISceneActivationState::ForegroundActive {
            return Some(window_scene.retain());
        }
        first.get_or_insert_with(|| window_scene.retain());
    }
    first
}

/// Puts the window on the scene when a scene connects.
pub(crate) fn wait_for_scene(window: Retained<WinitUIWindow>) {
    WAITING.with_borrow_mut(|windows| windows.push(window));
}

/// Registers the class, so that UIKit finds it by its name.
pub(crate) fn register() {
    let _ = WinitSceneDelegate::class();
}

