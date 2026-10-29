//! The pictures that a UI test saves for a person to look at.

use egui_kittest::Harness;

/// Saves the picture of `h` to `target/app/test-<name>.png` if the
/// variable `VALE_SCREENSHOTS` is set. The picture needs a GPU, so a test
/// of the UI tier makes it only on request. One test at a time renders,
/// because parallel GPU devices can crash the software driver.
pub fn save<S>(h: &mut Harness<'_, S>, name: &str) {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    if std::env::var_os("VALE_SCREENSHOTS").is_none() {
        return;
    }
    let _gpu = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/app");
    std::fs::create_dir_all(dir).unwrap();
    let image = h.render().unwrap();
    image.save(format!("{dir}/test-{name}.png")).unwrap();
}
