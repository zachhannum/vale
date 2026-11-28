//! The dependency rules of the workspace.

use std::fs;
use std::path::Path;

/// The name and the manifest of each crate whose name starts with `prefix`.
fn manifests(prefix: &str) -> Vec<(String, String)> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut out = Vec::new();
    for entry in fs::read_dir(crates).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.starts_with(prefix) {
            continue;
        }
        if let Ok(text) = fs::read_to_string(path.join("Cargo.toml")) {
            out.push((name, text));
        }
    }
    out
}

/// The lines of a manifest outside the package section, without comments.
fn dependency_lines(text: &str) -> impl Iterator<Item = &str> {
    let mut in_package = false;
    text.lines().filter(move |line| {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            return false;
        }
        !in_package && !line.starts_with('#')
    })
}

#[test]
fn no_core_crate_depends_on_egui() {
    let core = manifests("isotherm-");
    assert!(!core.is_empty());
    for (name, text) in core {
        if name == "isotherm-app" {
            continue;
        }
        for line in dependency_lines(&text) {
            for banned in ["eframe", "egui", "vale-globe"] {
                assert!(!line.contains(banned), "{name} names {banned}: {line}");
            }
        }
    }
    for (name, text) in manifests("vale-") {
        for line in dependency_lines(&text) {
            assert!(!line.contains("isotherm-"), "{name} names {line}");
        }
    }
}
