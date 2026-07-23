//! Integration tests: tether sessions (`docs/adr/0038-tethered-capture.md`).
//!
//! No USB camera is attached in CI or a plain dev machine, so these only
//! cover what's true without hardware: a clean error instead of a panic or
//! a hang, and that `tether_disconnect` never requires a session to exist.

use leyline_core::LeylineError;
use leyline_engine::Library;

fn open_test_library(name: &str) -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), name).unwrap();
    (dir, library)
}

#[test]
fn tether_connect_without_a_camera_returns_a_tether_error() {
    let (_dir, library) = open_test_library("TetherNoCamera");
    let result = library.tether_connect();
    assert!(matches!(result, Err(LeylineError::Tether(_))));
}

#[test]
fn tether_disconnect_without_a_session_is_a_no_op() {
    let (_dir, library) = open_test_library("TetherNoSession");
    library.tether_disconnect();
}
