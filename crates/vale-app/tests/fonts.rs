use vale_app::document::Document;
use vale_app::fonts::{self, DEFAULT_FAMILY};
use vale_app::pipeline::{Composed, Pipeline, Quality};
use vale_app::session::Session;
use vale_app::ui::{Action, AppState};
use vale_labeler::Fonts;
use vale_render::Item;
use vale_store::LayerId;

const SIZE: (f64, f64) = (1280.0, 800.0);
const SILKSCREEN: &[u8] = include_bytes!("data/fonts/Silkscreen-Regular.ttf");
const NOTO_SANS: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf");

fn places(doc: &Document) -> LayerId {
    let layers = doc.project.layers();
    layers.iter().find(|l| l.name == "Places").unwrap().id
}

/// The fonts of the app and Silkscreen, which stands for a system font.
fn with_silkscreen() -> Fonts {
    let mut f = fonts::bundled().unwrap();
    f.register(SILKSCREEN.to_vec()).unwrap();
    f
}

fn compose(doc: &Document, fonts: &mut Fonts) -> Composed {
    Pipeline::new()
        .compose(doc, SIZE, fonts, Quality::Final, None)
        .unwrap()
}

/// The font bytes of each label of a city.
fn city_fonts(c: &Composed) -> Vec<&[u8]> {
    c.list
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Glyphs(g) if g.text == "Tokyo" || g.text == "Cairo" => Some(g.font.data.as_ref()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_system_fonts_have_the_default_font() {
    let mut system = fonts::system().unwrap();
    let families = system.families();
    assert!(families.iter().any(|f| f == DEFAULT_FAMILY));
    // The family of the app hides a system family of the same name.
    let doc = Document::sample();
    let composed = compose(&doc, &mut system);
    let cities = city_fonts(&composed);
    assert!(!cities.is_empty());
    assert!(cities.iter().all(|d| *d == NOTO_SANS));
}

#[test]
fn a_label_uses_the_font_of_its_style() {
    let mut doc = Document::sample();
    let id = places(&doc);
    doc.entry_mut(id).unwrap().style.label.font = Some("Silkscreen".into());
    let composed = compose(&doc, &mut with_silkscreen());
    let cities = city_fonts(&composed);
    assert!(!cities.is_empty());
    assert!(cities.iter().all(|d| *d == SILKSCREEN));
}

#[test]
fn a_missing_font_gives_a_warning_and_uses_the_default_font() {
    let mut doc = Document::sample();
    let id = places(&doc);
    doc.entry_mut(id).unwrap().style.label.font = Some("Silkscreen".into());
    let mut f = fonts::bundled().unwrap();
    let warning = "Font Silkscreen is missing. Labels use Noto Sans.";
    assert_eq!(
        doc.missing_fonts(&f.families()),
        [(id, warning.to_string())]
    );
    assert!(doc.missing_fonts(&with_silkscreen().families()).is_empty());
    let composed = compose(&doc, &mut f);
    let cities = city_fonts(&composed);
    assert!(!cities.is_empty());
    assert!(cities.iter().all(|d| *d == NOTO_SANS));
}

#[test]
fn a_missing_font_is_in_the_status_after_an_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let id = places(&doc);
    doc.entry_mut(id).unwrap().style.label.font = Some("No Such Font".into());
    doc.store_styles();
    drop(Session::open(&path, &mut doc).unwrap());

    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.run_action(Action::OpenWorld(path));
    let warning = "Font No Such Font is missing. Labels use Noto Sans.";
    assert!(state.status.ends_with(warning), "{}", state.status);
    let label = &state.doc.entry(id).unwrap().style.label;
    assert_eq!(label.font.as_deref(), Some("No Such Font"));
}

#[test]
fn a_pdf_export_embeds_a_subset_of_each_font() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(Document::sample()).unwrap();
    state.headless = true;
    vale_app::ui::atlas::add_page(&mut state);
    vale_app::ui::atlas::add_frame(&mut state).unwrap();
    let pdf = dir.path().join("page.pdf");
    state.run_action(Action::Export(pdf.clone()));
    assert!(state.status.starts_with("Exported"), "{}", state.status);

    let bytes = std::fs::read(&pdf).unwrap();
    let raw = String::from_utf8_lossy(&bytes);
    // A subset font has a name of six capital letters and a plus sign.
    let subsets: Vec<&str> = raw
        .split("/BaseFont/")
        .skip(1)
        .map(|s| s.split(['/', '>', ' ']).next().unwrap())
        .collect();
    assert!(
        subsets.iter().any(|s| s.ends_with("+NotoSans-Regular")),
        "{subsets:?}"
    );
    for subset in subsets {
        let (tag, _) = subset.split_once('+').unwrap();
        assert!(
            tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()),
            "{subset}"
        );
    }
    assert!(raw.contains("/FontFile2"));
    // The whole font is larger than the PDF.
    assert!(bytes.len() < NOTO_SANS.len(), "{} bytes", bytes.len());
}
