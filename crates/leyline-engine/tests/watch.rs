//! Integration tests: watched-folder auto-import
//! (`docs/adr/0039-watched-folder-import.md`).

use std::time::Duration;

use leyline_core::LeylineError;
use leyline_engine::{Event, Library};

fn open_test_library(name: &str) -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), name).unwrap();
    (dir, library)
}

/// Waits up to a few seconds for a predicate over the event stream to hold,
/// polling with a short timeout so the test does not hang forever if the
/// event never fires.
fn wait_for(events: &std::sync::mpsc::Receiver<Event>, mut matches: impl FnMut(&Event) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if let Ok(event) = events.recv_timeout(Duration::from_millis(500)) {
            if matches(&event) {
                return;
            }
        }
    }
    panic!("expected event never arrived within the deadline");
}

#[test]
fn a_file_dropped_into_the_watched_folder_becomes_an_asset() {
    let (dir, library) = open_test_library("WatchDropsIn");
    let watched = dir.path().join("DropZone");
    std::fs::create_dir(&watched).unwrap();
    let events = library.subscribe();

    library.watch_start(&watched).unwrap();
    wait_for(&events, |event| matches!(event, Event::WatchStarted { .. }));

    image::save_buffer(
        watched.join("shot.png"),
        &[10u8; 6 * 2 * 3],
        6,
        2,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();

    wait_for(&events, |event| matches!(event, Event::AssetsAdded { .. }));

    library.watch_stop();
    wait_for(&events, |event| {
        matches!(event, Event::WatchStopped { reason: None })
    });
}

#[test]
fn a_second_watch_session_is_refused_while_one_is_running() {
    let (dir, library) = open_test_library("WatchDouble");
    let watched = dir.path().join("DropZone");
    std::fs::create_dir(&watched).unwrap();

    library.watch_start(&watched).unwrap();
    let result = library.watch_start(&watched);
    assert!(matches!(result, Err(LeylineError::Watch(_))));

    library.watch_stop();
}

#[test]
fn watch_stop_without_a_session_is_a_no_op() {
    let (_dir, library) = open_test_library("WatchNoSession");
    library.watch_stop();
}

#[test]
fn watch_start_on_a_missing_folder_returns_a_watch_error() {
    let (dir, library) = open_test_library("WatchMissingFolder");
    let missing = dir.path().join("DoesNotExist");
    let result = library.watch_start(&missing);
    assert!(matches!(result, Err(LeylineError::Watch(_))));
}
