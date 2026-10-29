//! The fonts of the labels. The app ships some fonts, and a project holds the
//! other fonts that its styles use. System fonts are never used, so a project
//! looks the same on each device.

use vale_labeler::Fonts;
use vale_store::Project;
use vale_style::LabelStyle;

/// The font of a label style that names no font or a missing font.
pub const DEFAULT_FAMILY: &str = "Noto Sans";

/// The families that ship with the app.
pub const BUNDLED_FAMILIES: [&str; 2] = ["Noto Sans", "Noto Serif"];

const BUNDLED: [&[u8]; 4] = [
    include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf"),
    include_bytes!("../../../assets/fonts/NotoSans-Italic.ttf"),
    include_bytes!("../../../assets/fonts/NotoSerif-Regular.ttf"),
    include_bytes!("../../../assets/fonts/NotoSerif-Italic.ttf"),
];

/// The largest font file that a project takes, in bytes.
pub const MAX_FONT_BYTES: u64 = 20 << 20;

/// The fonts that ship with the app.
pub fn bundled() -> anyhow::Result<Fonts> {
    let mut fonts = Fonts::new();
    for data in BUNDLED {
        fonts.register(data.to_vec())?;
    }
    Ok(fonts)
}

/// The fonts that ship with the app and the fonts of `project`.
pub fn for_project(project: &Project) -> anyhow::Result<Fonts> {
    let mut fonts = bundled()?;
    for record in project.fonts() {
        if let Err(e) = fonts.register(record.data.clone()) {
            eprintln!("The font {:?} is not valid: {e}", record.name);
        }
    }
    Ok(fonts)
}

/// The families that a label style can use: the bundled families, and then
/// the families of the project.
pub fn families(project: &Project) -> Vec<String> {
    let mut out: Vec<String> = BUNDLED_FAMILIES.iter().map(|f| f.to_string()).collect();
    for record in project.fonts() {
        if !out.contains(&record.family) {
            out.push(record.family.clone());
        }
    }
    out
}

/// The family that labels of `label` use in `fonts`.
pub fn family<'a>(label: &'a LabelStyle, fonts: &mut Fonts) -> &'a str {
    match label.font.as_deref() {
        Some(font) if fonts.has_family(font) => font,
        _ => DEFAULT_FAMILY,
    }
}

/// The warning for a label style whose font is not in the app or the project.
pub fn missing(label: &LabelStyle, project: &Project) -> Option<String> {
    missing_in(label, &families(project))
}

/// The warning for a label style whose font is not in `families`.
pub fn missing_in(label: &LabelStyle, families: &[String]) -> Option<String> {
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

/// The family of a font file, or why the project does not take it.
pub fn family_of(data: &[u8]) -> Result<String, String> {
    if data.len() as u64 > MAX_FONT_BYTES {
        return Err("The file is larger than 20 MB".to_string());
    }
    let families = Fonts::new()
        .register(data.to_vec())
        .map_err(|_| "The file is not a font".to_string())?;
    let family = families.into_iter().next().unwrap_or_default();
    if BUNDLED_FAMILIES.contains(&family.as_str()) {
        return Err(format!("{family} ships with Vale"));
    }
    Ok(family)
}
