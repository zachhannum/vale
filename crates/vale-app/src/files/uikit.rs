//! The file picker of iOS. It shows the Files app. For an import, the app
//! gets a copy of the picked file in its own temporary folder. For an
//! export, the Files app moves the file to the picked place.

use std::ffi::c_void;
use std::path::Path;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSArray, NSString, NSURL, ns_string};
use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController, UIViewController};
use objc2_uniform_type_identifiers::UTType;

use super::{ExportEvent, ExportQueue, PickQueue};

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

struct ExportIvars {
    queue: ExportQueue,
    /// Asks the app for a new frame.
    wake: Box<dyn Fn()>,
}

impl ExportIvars {
    fn end(&self, event: ExportEvent) {
        self.queue.push(event);
        (self.wake)();
    }
}

define_class!(
    // SAFETY: NSObject allows subclasses, and the type has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ValeFileExportDelegate"]
    #[ivars = ExportIvars]
    struct ExportDelegate;

    unsafe impl NSObjectProtocol for ExportDelegate {}

    unsafe impl UIDocumentPickerDelegate for ExportDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn did_pick(&self, _picker: &UIDocumentPickerViewController, _urls: &NSArray<NSURL>) {
            self.ivars().end(ExportEvent::Saved);
        }

        #[unsafe(method(documentPickerWasCancelled:))]
        fn was_cancelled(&self, _picker: &UIDocumentPickerViewController) {
            self.ivars().end(ExportEvent::Cancelled);
        }
    }
);

/// The picker of the Files app. A document picker does not retain its
/// delegate, so the app keeps this value.
pub struct Picker {
    presenter: Retained<UIViewController>,
    delegate: Retained<PickerDelegate>,
    export_delegate: Retained<ExportDelegate>,
}

impl Picker {
    /// Call it on the main thread. `controller` must point to a live
    /// UIViewController. `wake` runs after each pick and after the end of
    /// each export.
    pub fn new(
        controller: NonNull<c_void>,
        queue: PickQueue,
        exports: ExportQueue,
        wake: impl Fn() + Clone + 'static,
    ) -> Option<Picker> {
        let mtm = MainThreadMarker::new()?;
        // SAFETY: winit keeps its view controller for the life of the window.
        let presenter =
            unsafe { Retained::retain(controller.cast::<UIViewController>().as_ptr()) }?;
        let this = PickerDelegate::alloc(mtm).set_ivars(Ivars {
            queue,
            wake: Box::new(wake.clone()),
        });
        // SAFETY: NSObject has the initializer `init`.
        let delegate: Retained<PickerDelegate> = unsafe { msg_send![super(this), init] };
        let this = ExportDelegate::alloc(mtm).set_ivars(ExportIvars {
            queue: exports,
            wake: Box::new(wake),
        });
        // SAFETY: NSObject has the initializer `init`.
        let export_delegate: Retained<ExportDelegate> = unsafe { msg_send![super(this), init] };
        Some(Picker {
            presenter,
            delegate,
            export_delegate,
        })
    }

    /// Shows the PNG and TIFF files of the Files app. Returns `false` if the
    /// Files app cannot show now.
    pub fn pick_heightmap(&self) -> bool {
        self.pick(&[ns_string!("public.png"), ns_string!("public.tiff")])
    }

    /// Shows the PNG, TIFF, and Photoshop files of the Files app, for an
    /// image layer. Returns `false` if the Files app cannot show now.
    pub fn pick_layer_image(&self) -> bool {
        self.pick(&[
            ns_string!("public.png"),
            ns_string!("public.tiff"),
            ns_string!("com.adobe.photoshop-image"),
        ])
    }

    /// Shows the PNG and JPEG files of the Files app. Returns `false` if the
    /// Files app cannot show now.
    pub fn pick_image(&self) -> bool {
        self.pick(&[ns_string!("public.png"), ns_string!("public.jpeg")])
    }

    /// Shows the SVG files of the Files app. Returns `false` if the Files
    /// app cannot show now.
    pub fn pick_svg(&self) -> bool {
        self.pick(&[ns_string!("public.svg-image")])
    }

    /// Shows the TrueType and OpenType files of the Files app. Returns
    /// `false` if the Files app cannot show now.
    pub fn pick_font(&self) -> bool {
        self.pick(&[
            ns_string!("public.truetype-ttf-font"),
            ns_string!("public.opentype-font"),
        ])
    }

    /// Shows the files of these types in the Files app.
    fn pick(&self, types: &[&NSString]) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        if self.presenter.presentedViewController().is_some() {
            return false;
        }
        let types: Vec<Retained<UTType>> = types
            .iter()
            .filter_map(|t| UTType::typeWithIdentifier(t))
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
        true
    }

    /// Shows the Files app, which moves `path` to the place that the user
    /// picks. Returns `false` if the Files app cannot show now.
    pub fn export_file(&self, path: &Path) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        if self.presenter.presentedViewController().is_some() {
            return false;
        }
        let Some(url) = NSURL::from_file_path(path) else {
            return false;
        };
        let urls = NSArray::from_retained_slice(&[url]);
        let picker =
            UIDocumentPickerViewController::initForExportingURLs_asCopy(mtm.alloc(), &urls, false);
        picker.setDelegate(Some(ProtocolObject::from_ref(&*self.export_delegate)));
        self.presenter
            .presentViewController_animated_completion(&picker, true, None);
        true
    }
}
