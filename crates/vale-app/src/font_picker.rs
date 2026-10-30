//! The font picker of iOS. It shows the font families of the system and
//! the fonts that the user installed, with a search field.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::NSString;
use objc2_ui_kit::{
    UIFontDescriptorFamilyAttribute, UIFontPickerViewController,
    UIFontPickerViewControllerConfiguration, UIFontPickerViewControllerDelegate, UIViewController,
};

/// The picked families that wait for the app.
#[derive(Clone, Default)]
pub struct FontQueue(Arc<Mutex<Vec<String>>>);

impl FontQueue {
    fn push(&self, family: String) {
        if let Ok(mut queue) = self.0.lock() {
            queue.push(family);
        }
    }

    pub fn take(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }
}

struct Ivars {
    queue: FontQueue,
    /// Asks the app for a new frame.
    wake: Box<dyn Fn()>,
}

define_class!(
    // SAFETY: NSObject allows subclasses, and the type has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ValeFontPickerDelegate"]
    #[ivars = Ivars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UIFontPickerViewControllerDelegate for Delegate {
        #[unsafe(method(fontPickerViewControllerDidPickFont:))]
        fn did_pick(&self, picker: &UIFontPickerViewController) {
            let family = picker
                .selectedFontDescriptor()
                // SAFETY: The attribute is a static string of UIKit.
                .and_then(|d| d.objectForKey(unsafe { UIFontDescriptorFamilyAttribute }))
                .and_then(|o| o.downcast::<NSString>().ok());
            if let Some(family) = family {
                self.ivars().queue.push(family.to_string());
            }
            picker.dismissViewControllerAnimated_completion(true, None);
            (self.ivars().wake)();
        }
    }
);

/// The font picker. A font picker does not retain its delegate, so the app
/// keeps this value.
pub struct FontPicker {
    presenter: Retained<UIViewController>,
    delegate: Retained<Delegate>,
}

impl FontPicker {
    /// Call it on the main thread. `controller` must point to a live
    /// UIViewController. `wake` runs after each pick.
    pub fn new(
        controller: NonNull<c_void>,
        queue: FontQueue,
        wake: impl Fn() + 'static,
    ) -> Option<FontPicker> {
        let mtm = MainThreadMarker::new()?;
        // SAFETY: winit keeps its view controller for the life of the window.
        let presenter =
            unsafe { Retained::retain(controller.cast::<UIViewController>().as_ptr()) }?;
        let this = Delegate::alloc(mtm).set_ivars(Ivars {
            queue,
            wake: Box::new(wake),
        });
        // SAFETY: NSObject has the initializer `init`.
        let delegate: Retained<Delegate> = unsafe { msg_send![super(this), init] };
        Some(FontPicker {
            presenter,
            delegate,
        })
    }

    /// Shows the font picker. Returns `false` if another picker shows.
    pub fn pick(&self) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        if self.presenter.presentedViewController().is_some() {
            return false;
        }
        let configuration = UIFontPickerViewControllerConfiguration::new(mtm);
        configuration.setIncludeFaces(false);
        let picker = UIFontPickerViewController::initWithConfiguration(mtm.alloc(), &configuration);
        picker.setDelegate(Some(ProtocolObject::from_ref(&*self.delegate)));
        self.presenter
            .presentViewController_animated_completion(&picker, true, None);
        true
    }
}
