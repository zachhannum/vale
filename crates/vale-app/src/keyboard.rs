//! The on-screen keyboard and the paste action of iPadOS.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

/// The bits of the height of the docked keyboard, in points.
static HEIGHT: AtomicU32 = AtomicU32::new(0);
/// The texts that the paste action gave since the last frame.
static PASTES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Shows the edit menu of the system at a place of the window, in points.
/// Its Paste item pastes into the focused text field. Other systems have no
/// such menu.
#[cfg(not(target_os = "ios"))]
pub fn edit_menu(_at: eframe::egui::Pos2) {}

#[cfg(target_os = "ios")]
thread_local! {
    /// The edit menu of the view of the window.
    static MENU: std::cell::RefCell<Option<objc2::rc::Retained<objc2_ui_kit::UIEditMenuInteraction>>> =
        const { std::cell::RefCell::new(None) };
}

/// Shows the edit menu of the system at a place of the window, in points.
/// Its Paste item pastes into the focused text field.
#[cfg(target_os = "ios")]
pub fn edit_menu(at: eframe::egui::Pos2) {
    use objc2::MainThreadMarker;
    use objc2_core_foundation::CGPoint;
    use objc2_ui_kit::UIEditMenuConfiguration;
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    MENU.with(|menu| {
        let Some(menu) = &*menu.borrow() else {
            return;
        };
        let point = CGPoint::new(f64::from(at.x), f64::from(at.y));
        // SAFETY: a configuration with no identifier is valid.
        let config = unsafe {
            UIEditMenuConfiguration::configurationWithIdentifier_sourcePoint(None, point, mtm)
        };
        menu.presentEditMenuWithConfiguration(&config);
    });
}

/// The texts that the paste action of the system gave since the last call.
pub fn take_pastes() -> Vec<String> {
    std::mem::take(&mut *PASTES.lock().unwrap_or_else(|e| e.into_inner()))
}

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

/// Gives the view of the window the paste action and an edit menu. The
/// Paste item of the menu and Command-V then paste the text of the system
/// pasteboard.
/// The app reads the pasteboard only in the action, so iPadOS does not ask
/// for permission. `view` is the `UIView` of the window.
#[cfg(target_os = "ios")]
pub fn install_paste(view: std::ptr::NonNull<std::ffi::c_void>, ctx: eframe::egui::Context) {
    use std::sync::OnceLock;

    use objc2::ffi::class_addMethod;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use objc2::{msg_send, sel};
    use objc2_ui_kit::UIPasteboard;

    static CONTEXT: OnceLock<eframe::egui::Context> = OnceLock::new();
    static SUPER: OnceLock<&'static AnyClass> = OnceLock::new();

    extern "C-unwind" fn paste(_this: &AnyObject, _sel: Sel, _sender: *mut AnyObject) {
        // SAFETY: UIKit calls the action on the main thread.
        let text = unsafe { UIPasteboard::generalPasteboard().string() };
        if let Some(text) = text {
            let mut pastes = PASTES.lock().unwrap_or_else(|e| e.into_inner());
            pastes.push(text.to_string());
        }
        if let Some(ctx) = CONTEXT.get() {
            ctx.request_repaint();
        }
    }

    extern "C-unwind" fn can_perform(
        this: &AnyObject,
        _sel: Sel,
        action: Sel,
        sender: *mut AnyObject,
    ) -> Bool {
        if action == sel!(paste:) {
            // SAFETY: UIKit asks on the main thread. `hasStrings` does not
            // read the pasteboard.
            return Bool::new(unsafe { UIPasteboard::generalPasteboard().hasStrings() });
        }
        let Some(&superclass) = SUPER.get() else {
            return Bool::NO;
        };
        // SAFETY: the superclass of the view is a `UIResponder`, and this is
        // the signature of `canPerformAction:withSender:`.
        unsafe { msg_send![super(this, superclass), canPerformAction: action, withSender: sender] }
    }

    let _ = CONTEXT.set(ctx);
    // SAFETY: winit gives a live `UIView` for the life of the window.
    let view: &AnyObject = unsafe { view.cast().as_ref() };
    let class = view.class();
    let Some(superclass) = class.superclass() else {
        return;
    };
    let _ = SUPER.set(superclass);
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        use objc2::MainThreadOnly;
        use objc2::runtime::ProtocolObject;
        use objc2_ui_kit::{UIEditMenuInteraction, UIView};
        // SAFETY: the view of the window is a `UIView`.
        let view: &UIView = unsafe { &*(view as *const AnyObject).cast::<UIView>() };
        let menu = UIEditMenuInteraction::initWithDelegate(UIEditMenuInteraction::alloc(mtm), None);
        view.addInteraction(ProtocolObject::from_ref(&*menu));
        MENU.with(|cell| *cell.borrow_mut() = Some(menu));
    }
    let class = class as *const AnyClass as *mut AnyClass;
    type Paste = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject);
    type CanPerform = extern "C-unwind" fn(&AnyObject, Sel, Sel, *mut AnyObject) -> Bool;
    // SAFETY: the class of the view of winit does not have these methods,
    // and the type encodings match the functions.
    unsafe {
        let imp = std::mem::transmute::<Paste, Imp>(paste);
        class_addMethod(class, sel!(paste:), imp, c"v@:@".as_ptr());
        let imp = std::mem::transmute::<CanPerform, Imp>(can_perform);
        class_addMethod(
            class,
            sel!(canPerformAction:withSender:),
            imp,
            c"B@::@".as_ptr(),
        );
    }
}
