//! The fonts of the labels: the fonts of the system, and Noto Sans, which
//! ships with the app.

use std::sync::{Mutex, OnceLock};

use vale_labeler::Fonts;
use vale_style::LabelStyle;

/// The font of a label style that names no font or a missing font.
pub const DEFAULT_FAMILY: &str = "Noto Sans";

const BUNDLED: [&[u8]; 2] = [
    include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf"),
    include_bytes!("../../../assets/fonts/NotoSans-Italic.ttf"),
];

/// The fonts that ship with the app, and no system fonts.
pub fn bundled() -> anyhow::Result<Fonts> {
    register(Fonts::new())
}

/// The fonts of the system and the fonts that ship with the app. A family
/// that ships with the app hides a system family of the same name. The
/// search of the system fonts runs one time in each process.
pub fn system() -> anyhow::Result<Fonts> {
    static SYSTEM: OnceLock<Mutex<Fonts>> = OnceLock::new();
    let fonts = match SYSTEM.get() {
        Some(fonts) => fonts,
        None => {
            let fonts = register(Fonts::with_system_fonts())?;
            SYSTEM.get_or_init(|| Mutex::new(fonts))
        }
    };
    Ok(fonts.lock().unwrap_or_else(|e| e.into_inner()).clone())
}

fn register(mut fonts: Fonts) -> anyhow::Result<Fonts> {
    for data in BUNDLED {
        fonts.register(data.to_vec())?;
    }
    Ok(fonts)
}

/// The family that labels of `label` use in `fonts`.
pub fn family<'a>(label: &'a LabelStyle, fonts: &mut Fonts) -> &'a str {
    match label.font.as_deref() {
        Some(font) if fonts.has_family(font) => font,
        _ => DEFAULT_FAMILY,
    }
}

/// The warning for a label style whose font is not in `families`.
pub fn missing(label: &LabelStyle, families: &[String]) -> Option<String> {
    let font = label.font.as_deref()?;
    if families.iter().any(|f| f == font) {
        return None;
    }
    Some(format!(
        "Font {font} is missing. Labels use {DEFAULT_FAMILY}."
    ))
}

/// Sets the font of `label` to `family`. The default family is no font.
pub fn set_font(label: &mut LabelStyle, family: &str) {
    label.font = (family != DEFAULT_FAMILY).then(|| family.to_string());
}
