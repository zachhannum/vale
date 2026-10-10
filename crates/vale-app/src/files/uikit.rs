//! The file picker of iOS. It shows the Files app, and the app gets a copy
//! of the picked file in its own temporary folder.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSArray, NSURL, ns_string};
use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController, UIViewController};
use objc2_uniform_type_identifiers::UTType;

use super::PickQueue;

struct Ivars {
    queue: PickQueue,
    /// Asks the app for a new frame.
    wake: Box<dyn Fn()>,
}

define_class!(
    // SAFETY: NSObject allows subclasses, and the type has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ValeFilePickerDelegate"]
    #[ivars = Ivars]
    struct PickerDelegate;

    unsafe impl NSObjectProtocol for PickerDelegate {}

    unsafe impl UIDocumentPickerDelegate for PickerDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn did_pick(&self, _picker: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            let ivars = self.ivars();
            ivars
                .queue
                .push(urls.to_vec().iter().filter_map(|url| url.to_file_path()));
            (ivars.wake)();
        }
    }
);

/// The picker of the Files app. A document picker does not retain its
/// delegate, so the app keeps this value.
pub struct Picker {
    presenter: Retained<UIViewController>,
    delegate: Retained<PickerDelegate>,
}

impl Picker {
    /// Call it on the main thread. `controller` must point to a live
    /// UIViewController. `wake` runs after each pick.
    pub fn new(
        controller: NonNull<c_void>,
        queue: PickQueue,
        wake: Box<dyn Fn()>,
    ) -> Option<Picker> {
        let mtm = MainThreadMarker::new()?;
        // SAFETY: winit keeps its view controller for the life of the window.
        let presenter =
            unsafe { Retained::retain(controller.cast::<UIViewController>().as_ptr()) }?;
        let this = PickerDelegate::alloc(mtm).set_ivars(Ivars { queue, wake });
        // SAFETY: NSObject has the initializer `init`.
        let delegate: Retained<PickerDelegate> = unsafe { msg_send![super(this), init] };
        Some(Picker {
            presenter,
            delegate,
        })
    }

    /// Shows the PNG and TIFF files of the Files app.
    pub fn pick_heightmap(&self) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if self.presenter.presentedViewController().is_some() {
            return;
        }
        let types: Vec<Retained<UTType>> = [ns_string!("public.png"), ns_string!("public.tiff")]
            .into_iter()
            .filter_map(UTType::typeWithIdentifier)
            .collect();
        let types = NSArray::from_retained_slice(&types);
        let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(
            mtm.alloc(),
            &types,
            true,
        );
        picker.setDelegate(Some(ProtocolObject::from_ref(&*self.delegate)));
        self.presenter
            .presentViewController_animated_completion(&picker, true, None);
    }
}
