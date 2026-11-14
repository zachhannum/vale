//! The regions of the screen that the system reserves: the region of a
//! camera, and the folding region of a partially folded device. On iOS
//! 27.1 and later, UIKit gives the regions of a view. UIKit sends no
//! notification when they change, so the app reads them in each frame.

use eframe::egui::Rect;

/// The active reserved regions of the window, in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Regions {
    /// The region of a camera that covers the screen.
    pub camera: Option<Rect>,
    /// The folding region of a partially folded device.
    pub fold: Option<Rect>,
}

#[cfg(target_os = "ios")]
pub mod uikit {
    //! The reader of the reserved regions of UIKit.

    use std::ffi::c_void;
    use std::ptr::NonNull;

    use block2::RcBlock;
    use eframe::egui::{Rect, pos2, vec2};
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{Message, msg_send, sel};
    use objc2_core_foundation::CGRect;
    use objc2_foundation::NSArray;
    use objc2_ui_kit::UIView;

    /// The kinds of `UIViewReservedRegionKind`.
    #[derive(Clone, Copy)]
    enum Kind {
        Occlusion,
        Division,
    }

    pub struct Reader {
        view: Retained<UIView>,
        /// The `UIHingeInteraction` that wakes the UI after a change of the
        /// hinge.
        _hinge: Option<Retained<AnyObject>>,
    }

    impl Reader {
        /// Call it on the main thread with the view of the window. Before
        /// iOS 27.1, it gives `None`. `wake` runs after each change of the
        /// hinge.
        pub fn new(view: NonNull<c_void>, wake: Box<dyn Fn()>) -> Option<Reader> {
            // SAFETY: winit keeps its view for the life of the window.
            let view: &UIView = unsafe { view.cast::<UIView>().as_ref() };
            // SAFETY: Each NSObject has `respondsToSelector:`.
            let known: Bool =
                unsafe { msg_send![view, respondsToSelector: sel!(reservedRegionsOfKind:)] };
            if !known.as_bool() {
                return None;
            }
            let hinge = hinge_interaction(view, wake);
            Some(Reader {
                view: view.retain(),
                _hinge: hinge,
            })
        }

        /// The active regions of the view.
        pub fn read(&self) -> super::Regions {
            super::Regions {
                camera: self.first(Kind::Occlusion),
                fold: self.first(Kind::Division),
            }
        }

        /// The first active region of a kind.
        fn first(&self, kind: Kind) -> Option<Rect> {
            let class = AnyClass::get(c"UIViewReservedRegionKind")?;
            // SAFETY: The class methods of iOS 27.1 give a kind.
            let kind: Option<Retained<AnyObject>> = unsafe {
                match kind {
                    Kind::Occlusion => msg_send![class, occlusionRegionKind],
                    Kind::Division => msg_send![class, divisionRegionKind],
                }
            };
            let kind = kind?;
            // SAFETY: The method takes a kind and gives an array of
            // `UIViewReservedRegion`.
            let regions: Retained<NSArray<AnyObject>> =
                unsafe { msg_send![&*self.view, reservedRegionsOfKind: &*kind] };
            regions.iter().find_map(|region| {
                // SAFETY: `UIViewReservedRegion` has the properties
                // `active` and `frame`.
                let active: Bool = unsafe { msg_send![&*region, isActive] };
                let frame: CGRect = unsafe { msg_send![&*region, frame] };
                let min = pos2(frame.origin.x as f32, frame.origin.y as f32);
                let size = vec2(frame.size.width as f32, frame.size.height as f32);
                (active.as_bool() && size.x > 0.0 && size.y > 0.0)
                    .then(|| Rect::from_min_size(min, size))
            })
        }
    }

    /// Adds a `UIHingeInteraction` to the view. Its handler runs `wake`.
    fn hinge_interaction(view: &UIView, wake: Box<dyn Fn()>) -> Option<Retained<AnyObject>> {
        let class = AnyClass::get(c"UIHingeInteraction")?;
        let block = RcBlock::new(move |_: *mut AnyObject, _: *mut AnyObject| wake());
        // SAFETY: The class has the initializer `initWithUpdateHandler:`,
        // and the handler takes the interaction and an update. A view takes
        // each object of the protocol `UIInteraction`.
        unsafe {
            let new: Allocated<AnyObject> = msg_send![class, alloc];
            let interaction: Option<Retained<AnyObject>> =
                msg_send![new, initWithUpdateHandler: &*block];
            let interaction = interaction?;
            let _: () = msg_send![view, addInteraction: &*interaction];
            Some(interaction)
        }
    }
}
