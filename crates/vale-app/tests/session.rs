use std::time::Instant;

use vale_app::document::Document;
use vale_app::session::{SAVE_DELAY, Session};
use vale_store::ProjectFile;

#[test]
fn a_world_is_the_same_after_a_close_and_an_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app data/project.gpkg");

    // The first start makes the file from the document.
    let mut doc = Document::sample();
    let mut session = Session::open(&path, &mut doc).unwrap();
    let start = Instant::now();
    assert_eq!(session.tick(&doc.project, start).unwrap(), None);

    // A change is on disk after the delay, and not before it.
    doc.project.world.name = "Aerth".to_string();
    doc.set_radius_km(2500.0);
    let places = doc.project.layers()[2].id;
    doc.remove_layer(places);
    assert_eq!(session.tick(&doc.project, start).unwrap(), Some(SAVE_DELAY));
    let half = SAVE_DELAY / 2;
    assert_eq!(
        session.tick(&doc.project, start + half).unwrap(),
        Some(half)
    );
    assert_ne!(ProjectFile::open(&path).unwrap().1, doc.project);
    assert_eq!(
        session.tick(&doc.project, start + SAVE_DELAY).unwrap(),
        None
    );
    assert_eq!(ProjectFile::open(&path).unwrap().1, doc.project);

    // The app stops with no exit call. The next start has the same world,
    // and the layers keep the styles of the sample.
    drop(session);
    let mut next = Document::sample();
    assert_ne!(next.project, doc.project);
    let mut session = Session::open(&path, &mut next).unwrap();
    assert_eq!(next.project, doc.project);
    assert_eq!(next.frame.entries, doc.frame.entries);
    assert_eq!(next.frame.entries.len(), 2);

    // The exit saves a change that is newer than the delay.
    next.project.world.name = "Last".to_string();
    let now = Instant::now();
    assert!(session.tick(&next.project, now).unwrap().is_some());
    session.flush(&next.project).unwrap();
    assert_eq!(ProjectFile::open(&path).unwrap().1, next.project);
}

#[test]
fn a_new_layer_of_the_file_gets_an_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.gpkg");
    let mut doc = Document::sample();
    drop(Session::open(&path, &mut doc).unwrap());

    let mut empty = Document::empty();
    drop(Session::open(&path, &mut empty).unwrap());
    assert_eq!(empty.project, doc.project);
    let names = |d: &Document| -> Vec<String> {
        let entries = d.frame.entries.iter();
        entries
            .map(|e| d.layer(e.layer).unwrap().name.clone())
            .collect()
    };
    assert_eq!(names(&empty), ["Land", "Rivers", "Places"]);
    assert_eq!(names(&empty), names(&doc));
}
