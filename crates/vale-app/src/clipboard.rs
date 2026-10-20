//! The text of the system clipboard. On iPadOS it is the general pasteboard.
//! On other systems, the app keeps its own copy, and a copy also goes to the
//! clipboard of the window.

use eframe::egui;

#[cfg(not(target_os = "ios"))]
static TEXT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Puts a text on the clipboard.
pub fn set(ctx: &egui::Context, text: &str) {
    #[cfg(target_os = "ios")]
    {
        let _ = ctx;
        let text = objc2_foundation::NSString::from_str(text);
        // SAFETY: the UI calls this on the main thread.
        unsafe { objc2_ui_kit::UIPasteboard::generalPasteboard().setString(Some(&text)) };
    }
    #[cfg(not(target_os = "ios"))]
    {
        ctx.copy_text(text.to_owned());
        *TEXT.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.to_owned());
    }
}

/// The text on the clipboard. On iPadOS, the system can ask the user for
/// permission first.
pub fn get() -> Option<String> {
    #[cfg(target_os = "ios")]
    {
        // SAFETY: the UI calls this on the main thread.
        let text = unsafe { objc2_ui_kit::UIPasteboard::generalPasteboard().string() };
        text.map(|text| text.to_string())
    }
    #[cfg(not(target_os = "ios"))]
    {
        TEXT.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// True if the clipboard has a text. On iPadOS, this does not read the
/// text, so the system does not ask for permission.
pub fn has_text() -> bool {
    #[cfg(target_os = "ios")]
    {
        // SAFETY: the UI calls this on the main thread.
        unsafe { objc2_ui_kit::UIPasteboard::generalPasteboard().hasStrings() }
    }
    #[cfg(not(target_os = "ios"))]
    {
        TEXT.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }
}
