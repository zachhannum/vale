//! The log file of the app gets the log, the panics, and the exit.
//!
//! The logger, the panic hook, and the exit handler are global, so the test
//! runs them in a child process: this test binary again, with `CHILD` set.

use std::path::Path;
use std::process::Command;

use vale_app::log_file;

const CHILD: &str = "VALE_LOG_FILE_CHILD";

fn child(path: &Path) {
    log_file::install(path).unwrap();
    log::warn!("a warning");
    log::info!("an info");
    let _ = std::panic::catch_unwind(|| panic!("a panic"));
    log_file::write("last line");
}

#[test]
fn the_log_gets_the_warnings_the_panics_and_the_exit() {
    if let Some(path) = std::env::var_os(CHILD) {
        child(Path::new(&path));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(log_file::FILE_NAME);
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "the_log_gets_the_warnings_the_panics_and_the_exit",
        ])
        .env(CHILD, &path)
        .status()
        .unwrap();
    assert!(status.success());

    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].contains("start, version"), "{text}");
    assert!(text.contains("WARN log_file: a warning"), "{text}");
    assert!(!text.contains("an info"), "{text}");
    let panic = lines
        .iter()
        .find(|l| l.contains("panic in thread"))
        .unwrap();
    assert!(panic.contains("a panic"), "{panic}");
    assert!(panic.contains("tests/log_file.rs:"), "{panic}");
    let last = lines.iter().position(|l| l.ends_with("last line")).unwrap();
    assert!(
        lines[last + 1..].iter().any(|l| l.ends_with(" exit")),
        "{text}"
    );
}
