//! The shell crate is shared by Vale and Isotherm, so it depends on neither app.

#[test]
fn the_crate_depends_on_no_app() {
    let manifest = include_str!("../Cargo.toml");
    for line in manifest.lines().map(str::trim_start) {
        assert!(!line.starts_with("vale-app"), "{line}");
        assert!(!line.starts_with("isotherm-"), "{line}");
    }
}
