use std::path::PathBuf;

use vale_app::document::Document;
use vale_app::fonts::{self, DEFAULT_FAMILY};
use vale_app::pipeline::{Composed, Pipeline, Quality};
use vale_app::session::Session;
use vale_app::ui::{Action, AppState};
use vale_render::Item;
use vale_store::LayerId;

const SIZE: (f64, f64) = (1280.0, 800.0);
const SILKSCREEN: &[u8] = include_bytes!("data/fonts/Silkscreen-Regular.ttf");
const NOTO_SANS: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf");

fn font_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/fonts/Silkscreen-Regular.ttf")
}

fn places(doc: &Document) -> LayerId {
    let layers = doc.project.layers();
    layers.iter().find(|l| l.name == "Places").unwrap().id
}

/// Composes with the fonts of the app and of the project only.
fn compose(doc: &Document) -> Composed {
    let mut f = fonts::for_project(&doc.project).unwrap();
    Pipeline::new()
        .compose(doc, SIZE, &mut f, Quality::Final, None)
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

/// The text and the font bytes of each label.
fn labels(c: &Composed) -> Vec<(&str, &[u8])> {
    let mut out: Vec<(&str, &[u8])> = (c.list.items.iter())
        .filter_map(|i| match i {
            Item::Glyphs(g) => Some((g.text.as_str(), g.font.data.as_ref())),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a_project_font_draws_the_same_after_a_save_and_an_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let family = fonts::family_of(SILKSCREEN).unwrap();
    assert_eq!(family, "Silkscreen");
    assert!(
        doc.project
            .add_font(&family, "Silkscreen-Regular.ttf", SILKSCREEN.to_vec())
    );
    let id = places(&doc);
    doc.entry_mut(id).unwrap().style.label.font = Some(family);
    doc.store_styles();
    let before = compose(&doc);
    let cities = city_fonts(&before);
    assert!(!cities.is_empty());
    assert!(cities.iter().all(|d| *d == SILKSCREEN));
    drop(Session::open(&path, &mut doc).unwrap());

    // Another device opens the file. It has only the fonts of the app.
    let mut opened = Document::empty();
    drop(Session::open(&path, &mut opened).unwrap());
    assert_eq!(opened.project.fonts(), doc.project.fonts());
    assert!(opened.missing_fonts().is_empty());
    let after = compose(&opened);
    assert_eq!(labels(&after), labels(&before));
}

#[test]
fn a_missing_font_gives_a_warning_and_uses_the_default_font() {
    let mut doc = Document::sample();
    let id = places(&doc);
    doc.entry_mut(id).unwrap().style.label.font = Some("Lora".into());
    let warning = "Font Lora is missing. Labels use Noto Sans.";
    assert_eq!(doc.missing_fonts(), [(id, warning.to_string())]);
    let composed = compose(&doc);
    let cities = city_fonts(&composed);
    assert!(!cities.is_empty());
    assert!(cities.iter().all(|d| *d == NOTO_SANS));
    assert_eq!(DEFAULT_FAMILY, "Noto Sans");
}

#[test]
fn a_missing_font_is_in_the_status_after_an_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    let id = places(&doc);
    doc.entry_mut(id).unwrap().style.label.font = Some("Lora".into());
    doc.store_styles();
    drop(Session::open(&path, &mut doc).unwrap());

    let mut state = AppState::new(Document::empty()).unwrap();
    state.headless = true;
    state.run_action(Action::OpenWorld(path));
    assert!(
        state
            .status
            .ends_with("Font Lora is missing. Labels use Noto Sans."),
        "{}",
        state.status
    );
}

#[test]
fn an_added_font_is_saved_in_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::empty();
    let session = Session::open(&path, &mut doc).unwrap();
    let mut state = AppState::new(doc).unwrap();
    state.headless = true;
    state.session = Some(session);

    state.run_action(Action::AddFont(font_path()));
    let note = state.font_note.clone().unwrap();
    assert_eq!(note.text, "Added font Silkscreen");
    assert!(!note.warning);
    assert!(state.fonts.has_family("Silkscreen"));
    assert_eq!(
        fonts::families(&state.doc.project),
        ["Noto Sans", "Noto Serif", "Silkscreen"]
    );

    // The same file again, a bundled font, and a file that is not a font.
    state.run_action(Action::AddFont(font_path()));
    let note = state.font_note.clone().unwrap();
    assert!(
        note.warning && note.text.contains("already"),
        "{}",
        note.text
    );
    let noto =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/NotoSerif-Regular.ttf");
    state.run_action(Action::AddFont(noto));
    let note = state.font_note.clone().unwrap();
    assert!(note.warning, "{}", note.text);
    assert!(
        note.text.ends_with("Noto Serif ships with Vale"),
        "{}",
        note.text
    );
    let bad = dir.path().join("bad.ttf");
    std::fs::write(&bad, "not a font").unwrap();
    state.run_action(Action::AddFont(bad));
    assert!(state.font_note.clone().unwrap().warning);
    drop(state);

    let mut doc = Document::empty();
    drop(Session::open(&path, &mut doc).unwrap());
    assert_eq!(doc.project.fonts().len(), 1);
    assert_eq!(doc.project.fonts()[0].data, SILKSCREEN);
}

#[test]
fn a_pdf_export_embeds_a_subset_of_each_font() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = Document::sample();
    let family = fonts::family_of(SILKSCREEN).unwrap();
    doc.project
        .add_font(&family, "Silkscreen-Regular.ttf", SILKSCREEN.to_vec());
    let id = places(&doc);
    doc.entry_mut(id).unwrap().style.label.font = Some(family);
    let mut state = AppState::new(doc).unwrap();
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
    assert!(subsets.iter().any(|s| s.ends_with("+Silkscreen-Regular")));
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
