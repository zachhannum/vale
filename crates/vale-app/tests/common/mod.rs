//! The test symbols in `tests/data`.

use vale_app::document::Document;

/// Adds the test symbols `names` to the project of `doc`, as a user adds an
/// SVG file.
pub fn add_symbols(doc: &mut Document, names: &[&str]) {
    for name in names {
        let added = doc.add_symbol(name, svg(name)).unwrap();
        assert_eq!(added, *name);
    }
}

/// The text of the test symbol `name`.
pub fn svg(name: &str) -> String {
    let path = format!("{}/tests/data/{name}.svg", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}
